use anyhow::Context as _;
use crossbeam_queue::ArrayQueue;
use ffmpeg_next::{self as ffmpeg, ChannelLayout, format::Sample, frame::Audio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub struct DecodeSummary {
    pub frames: u64,
}

fn hint_is_mp3(filename_hint: Option<&str>) -> bool {
    filename_hint.is_some_and(|hint| {
        let hint = hint
            .split(['?', '#', ';'])
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        let tail = hint.rsplit(['/', '\\']).next().unwrap_or_default();
        matches!(tail.rsplit('.').next(), Some("mp3"))
    })
}

fn prefix_is_mp3(prefix: &[u8]) -> bool {
    if mpeg_audio_header(prefix) {
        return true;
    }
    let Some(audio_offset) = id3_audio_offset(prefix) else {
        return false;
    };
    prefix.get(audio_offset..).is_some_and(mpeg_audio_header)
}

fn id3_audio_offset(prefix: &[u8]) -> Option<usize> {
    if prefix.len() < 10 || !prefix.starts_with(b"ID3") {
        return None;
    }
    let size_bytes = &prefix[6..10];
    if size_bytes.iter().any(|byte| byte & 0x80 != 0) {
        return None;
    }
    let tag_size = size_bytes
        .iter()
        .fold(0usize, |size, byte| (size << 7) | usize::from(*byte));
    let footer_size = usize::from(prefix[5] & 0x10 != 0) * 10;
    Some(10usize.saturating_add(tag_size).saturating_add(footer_size))
}

fn mpeg_audio_header(prefix: &[u8]) -> bool {
    if prefix.len() < 3 || prefix[0] != 0xff || prefix[1] & 0xe0 != 0xe0 {
        return false;
    }
    let version = (prefix[1] >> 3) & 0x03;
    let layer = (prefix[1] >> 1) & 0x03;
    let bitrate = (prefix[2] >> 4) & 0x0f;
    let sample_rate = (prefix[2] >> 2) & 0x03;
    version != 0x01 && layer != 0 && !matches!(bitrate, 0 | 0x0f) && sample_rate != 0x03
}

fn reader_is_mp3(
    reader: &mut super::streaming::BoundedHttpReader,
    filename_hint: Option<&str>,
) -> std::io::Result<bool> {
    if hint_is_mp3(filename_hint) {
        return Ok(true);
    }
    let header = reader.peek_prefix(10)?;
    if prefix_is_mp3(&header) {
        return Ok(true);
    }
    let Some(audio_offset) = id3_audio_offset(&header) else {
        return Ok(false);
    };
    const MAX_ID3_PROBE_BYTES: usize = super::streaming::COMPRESSED_CAPACITY_BYTES;
    if audio_offset.saturating_add(4) > MAX_ID3_PROBE_BYTES {
        return Ok(false);
    }
    let prefix = reader.peek_prefix(audio_offset + 4)?;
    Ok(prefix_is_mp3(&prefix))
}

pub fn decode_stream(
    mut reader: super::streaming::BoundedHttpReader,
    filename_hint: Option<&str>,
    output_rate: u32,
    output_channels: u16,
    start_frame: u64,
    pcm: Arc<ArrayQueue<f32>>,
    cancel: Arc<AtomicBool>,
) -> anyhow::Result<DecodeSummary> {
    ffmpeg::init()?;
    ffmpeg::util::log::set_level(ffmpeg::util::log::Level::Quiet);
    // The HTTP reader only retains a bounded sliding window. Advertising FLAC as
    // fully seekable lets stream probing read ahead, evict early bytes, and
    // then fail a backward seek. Some demuxers ignore that failed seek and
    // resume mid-frame after their probe packets (notably FLAC with artwork).
    // FLAC playback and restored-position fallback are sequential, so expose
    // the truthful non-seekable contract for that demuxer. Containers such as
    // MP4 still need bounded seeking to locate their metadata.
    let flac_stream = reader.peek_prefix(4)? == b"fLaC";
    let sequential_stream = flac_stream || reader_is_mp3(&mut reader, filename_hint)?;
    let io = if sequential_stream {
        ffmpeg::format::context::StreamIo::from_read_with_capacity(reader, 32 * 1024)?
    } else {
        reader.enable_seek_history()?;
        ffmpeg::format::context::StreamIo::from_read_seek_with_capacity(reader, 32 * 1024)?
    };
    let token = cancel.clone();
    let mut input =
        ffmpeg::format::input_from_stream_with_interrupt(io, filename_hint, None, move || {
            token.load(Ordering::Acquire)
        })?;
    let stream = input
        .streams()
        .find(|s| s.parameters().medium() == ffmpeg::media::Type::Audio)
        .ok_or_else(|| anyhow::anyhow!("no audio stream"))?;
    let stream_index = stream.index();
    let context = ffmpeg::codec::context::Context::from_parameters(stream.parameters())?;
    let mut decoder = context.decoder().audio()?;
    let output_layout = match output_channels {
        1 => ChannelLayout::MONO,
        2 => ChannelLayout::STEREO,
        _ => anyhow::bail!("unsupported output layout"),
    };
    let mut resampler: Option<ffmpeg::software::resampling::Context> = None;
    let mut frames = 0u64;
    let mut discard_samples = start_frame.saturating_mul(u64::from(output_channels));
    let mut receive = |decoder: &mut ffmpeg::decoder::Audio, drain: bool| -> anyhow::Result<()> {
        loop {
            let mut frame = Audio::empty();
            match decoder.receive_frame(&mut frame) {
                Ok(()) => {}
                Err(ffmpeg::Error::Eof) if drain => break,
                Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::error::EAGAIN && !drain => {
                    break;
                }
                Err(error) => return Err(error.into()),
            }
            let input_layout = if frame.channel_layout().is_empty() {
                match frame.channels() {
                    1 => ChannelLayout::MONO,
                    2 => ChannelLayout::STEREO,
                    _ => anyhow::bail!("unsupported input layout"),
                }
            } else {
                frame.channel_layout()
            };
            if input_layout.channels() > 2 {
                anyhow::bail!("unsupported input layout");
            }
            if frame.channel_layout().is_empty() {
                frame.set_channel_layout(input_layout);
            }
            let converter = resampler.get_or_insert(ffmpeg::software::resampling::Context::get(
                frame.format(),
                input_layout,
                frame.rate(),
                Sample::F32(ffmpeg::format::sample::Type::Packed),
                output_layout,
                output_rate,
            )?);
            let capacity = (frame.samples().saturating_mul(output_rate as usize)
                / frame.rate() as usize)
                .saturating_add(256);
            let mut converted = Audio::new(
                Sample::F32(ffmpeg::format::sample::Type::Packed),
                capacity,
                output_layout,
            );
            converted.set_rate(output_rate);
            converter.run(&frame, &mut converted)?;
            for sample in converted.plane::<f32>(0) {
                if discard_samples > 0 {
                    discard_samples -= 1;
                    continue;
                }
                while pcm.push(*sample).is_err() {
                    if cancel.load(Ordering::Acquire) {
                        anyhow::bail!("cancelled");
                    }
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
            }
            frames += converted.samples() as u64;
        }
        Ok(())
    };
    let mut packet_count = 0u64;
    for (stream, packet) in input.packets() {
        if cancel.load(Ordering::Acquire) {
            anyhow::bail!("cancelled");
        }
        if stream.index() != stream_index {
            continue;
        }
        packet_count += 1;
        decoder.send_packet(&packet).with_context(|| {
            format!("send audio packet {packet_count} ({} bytes)", packet.size())
        })?;
        receive(&mut decoder, false).context("receive audio frame")?;
    }
    decoder.send_eof().context("send decoder EOF")?;
    receive(&mut decoder, true).context("drain audio decoder")?;
    if let Some(converter) = resampler.as_mut() {
        let mut previous_delay = None;
        for _ in 0..32 {
            let mut tail = Audio::new(
                Sample::F32(ffmpeg::format::sample::Type::Packed),
                4096,
                output_layout,
            );
            tail.set_rate(output_rate);
            let delay = converter
                .flush(&mut tail)
                .context("flush audio resampler")?;
            for sample in tail.plane::<f32>(0) {
                if discard_samples > 0 {
                    discard_samples -= 1;
                    continue;
                }
                while pcm.push(*sample).is_err() {
                    if cancel.load(Ordering::Acquire) {
                        anyhow::bail!("cancelled");
                    }
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
            }
            frames += tail.samples() as u64;
            if delay.is_none() {
                return Ok(DecodeSummary { frames });
            }
            // swr_get_delay may retain fixed filter latency after the final
            // drainable sample. Require stable delay across another empty
            // flush before treating that latency as non-drainable.
            if tail.samples() == 0 && previous_delay == delay {
                return Ok(DecodeSummary { frames });
            }
            previous_delay = delay;
        }
        anyhow::bail!("resampler did not drain");
    }
    Ok(DecodeSummary { frames })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playback::streaming::{
        BoundedHttpReader, COMPRESSED_CAPACITY_BYTES, COMPRESSED_CHUNK_BYTES,
    };
    use std::io::{Read, Write};

    fn decode_test_file_at(
        path: &std::path::Path,
        start_frame: u64,
    ) -> anyhow::Result<(DecodeSummary, u64)> {
        decode_test_file_at_with_compressed_high_water(path, start_frame)
            .map(|(summary, samples, _)| (summary, samples))
    }

    fn decode_test_file_at_with_compressed_high_water(
        path: &std::path::Path,
        start_frame: u64,
    ) -> anyhow::Result<(DecodeSummary, u64, u64)> {
        decode_test_file_at_with_hint_and_high_water(
            path,
            start_frame,
            path.file_name().and_then(|name| name.to_str()),
        )
    }

    fn decode_test_file_at_with_hint_and_high_water(
        path: &std::path::Path,
        start_frame: u64,
        hint: Option<&str>,
    ) -> anyhow::Result<(DecodeSummary, u64, u64)> {
        let cancel = Arc::new(AtomicBool::new(false));
        let compressed_high_water = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let (tx, reader) = BoundedHttpReader::channel_with_high_water(
            cancel.clone(),
            compressed_high_water.clone(),
        );
        let mut bytes = Vec::new();
        std::fs::File::open(path)?.read_to_end(&mut bytes)?;
        let producer = std::thread::spawn(move || -> Result<(), String> {
            for chunk in bytes.chunks(COMPRESSED_CHUNK_BYTES) {
                tx.blocking_send(Ok(bytes::Bytes::copy_from_slice(chunk)))
                    .map_err(|_| "decoder closed compressed input".to_string())?;
            }
            Ok(())
        });
        let pcm = Arc::new(ArrayQueue::new(64 * 1024));
        let done = Arc::new(AtomicBool::new(false));
        let drained = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let drain_pcm = pcm.clone();
        let drain_done = done.clone();
        let drain_count = drained.clone();
        let consumer = std::thread::spawn(move || {
            while !drain_done.load(Ordering::Acquire) || !drain_pcm.is_empty() {
                if drain_pcm.pop().is_some() {
                    drain_count.fetch_add(1, Ordering::Relaxed);
                } else {
                    std::thread::yield_now();
                }
            }
        });
        let result = decode_stream(reader, hint, 48_000, 2, start_frame, pcm, cancel);
        done.store(true, Ordering::Release);
        consumer.join().expect("PCM consumer panicked");
        let producer = producer.join().expect("compressed producer panicked");
        match result {
            Err(error) => Err(error),
            Ok(summary) => {
                producer.map_err(anyhow::Error::msg)?;
                Ok((
                    summary,
                    drained.load(Ordering::Relaxed),
                    compressed_high_water.load(Ordering::Relaxed),
                ))
            }
        }
    }

    fn decode_test_file(path: &std::path::Path) -> anyhow::Result<DecodeSummary> {
        decode_test_file_at(path, 0).map(|(summary, _)| summary)
    }

    #[test]
    #[ignore = "uses HIFIMULE_DIAGNOSTIC_FLAC; never committed as a fixture"]
    fn diagnostic_external_flac() {
        let path = std::env::var("HIFIMULE_DIAGNOSTIC_FLAC").unwrap();
        let result = decode_test_file(std::path::Path::new(&path)).unwrap();
        assert!(
            result.frames > 8_000_000,
            "decoded only {} frames",
            result.frames
        );
    }

    #[test]
    #[ignore = "uses HIFIMULE_DIAGNOSTIC_AUDIO; never committed as a fixture"]
    fn diagnostic_external_audio_decodes_beyond_the_compressed_window() {
        let path = std::env::var("HIFIMULE_DIAGNOSTIC_AUDIO").unwrap();
        assert!(
            std::fs::metadata(&path).unwrap().len() > COMPRESSED_CAPACITY_BYTES as u64,
            "diagnostic must exercise eviction beyond the compressed window"
        );
        let (result, _, compressed_high_water) =
            decode_test_file_at_with_compressed_high_water(std::path::Path::new(&path), 0).unwrap();
        assert!(
            result.frames > 2_000_000,
            "decoded only {} frames",
            result.frames
        );
        assert!(compressed_high_water <= COMPRESSED_CAPACITY_BYTES as u64);
    }

    #[test]
    #[ignore = "uses HIFIMULE_DIAGNOSTIC_MP3; never committed as a fixture"]
    fn oversized_mp3_decodes_complete_stream_with_bounded_memory() {
        let path = std::env::var("HIFIMULE_DIAGNOSTIC_MP3").unwrap();
        let (result, _, compressed_high_water) =
            decode_test_file_at_with_compressed_high_water(std::path::Path::new(&path), 0).unwrap();
        assert!(
            result.frames > 17_000_000,
            "decoded only {} frames",
            result.frames
        );
        assert!(compressed_high_water <= COMPRESSED_CAPACITY_BYTES as u64);
    }

    #[test]
    #[ignore = "uses HIFIMULE_DIAGNOSTIC_MP3; never committed as a fixture"]
    fn oversized_id3_mp3_without_filename_hint_decodes_complete_stream() {
        let path = std::env::var("HIFIMULE_DIAGNOSTIC_MP3").unwrap();
        let (result, _, _) =
            decode_test_file_at_with_hint_and_high_water(std::path::Path::new(&path), 0, None)
                .unwrap();
        assert!(
            result.frames > 17_000_000,
            "decoded only {} frames",
            result.frames
        );
    }

    #[test]
    fn valid_mpeg_audio_frame_header_is_recognized_as_mp3() {
        assert!(prefix_is_mp3(&[0xff, 0xfb, 0x90, 0x64]));
    }

    #[test]
    fn ambiguous_mpeg_hint_is_not_treated_as_mp3() {
        assert!(!hint_is_mp3(Some("movie.mpeg")));
        assert!(!hint_is_mp3(Some("mpeg")));
    }

    #[test]
    fn extensionless_mp3_with_large_id3_tag_is_recognized_within_bounded_window() {
        let tag_size = 300 * 1024usize;
        let mut header = vec![0u8; 10];
        header[..3].copy_from_slice(b"ID3");
        header[6] = ((tag_size >> 21) & 0x7f) as u8;
        header[7] = ((tag_size >> 14) & 0x7f) as u8;
        header[8] = ((tag_size >> 7) & 0x7f) as u8;
        header[9] = (tag_size & 0x7f) as u8;
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, mut reader) = BoundedHttpReader::channel(cancel);
        let producer = std::thread::spawn(move || {
            tx.blocking_send(Ok(bytes::Bytes::from(header))).unwrap();
            for _ in 0..(tag_size / COMPRESSED_CHUNK_BYTES) {
                tx.blocking_send(Ok(bytes::Bytes::from(vec![0; COMPRESSED_CHUNK_BYTES])))
                    .unwrap();
            }
            let remainder = tag_size % COMPRESSED_CHUNK_BYTES;
            tx.blocking_send(Ok(bytes::Bytes::from(vec![0; remainder])))
                .unwrap();
            tx.blocking_send(Ok(bytes::Bytes::from_static(&[0xff, 0xfb, 0x90, 0x64])))
                .unwrap();
        });
        assert!(reader_is_mp3(&mut reader, None).unwrap());
        producer.join().unwrap();
    }

    #[test]
    fn adts_aac_header_is_not_recognized_as_mp3() {
        assert!(!prefix_is_mp3(&[0xff, 0xf1, 0x50, 0x80]));
    }

    #[test]
    fn id3_tag_followed_by_mpeg_audio_frame_is_recognized_as_mp3() {
        let bytes = [
            b'I', b'D', b'3', 4, 0, 0, 0, 0, 0, 0, 0xff, 0xfb, 0x90, 0x64,
        ];
        assert!(prefix_is_mp3(&bytes));
    }

    #[test]
    fn production_decoder_accepts_six_format_fixture_matrix() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../experiments/playback-probe/fixtures");
        for name in [
            "track-1.wav",
            "track-1.flac",
            "track-1.m4a",
            "track-1.mp3",
            "track-1.aac.m4a",
            "track-1.opus",
        ] {
            eprintln!("decoding {name}");
            let cancel = Arc::new(AtomicBool::new(false));
            let (tx, reader) = BoundedHttpReader::channel(cancel.clone());
            let mut bytes = Vec::new();
            std::fs::File::open(root.join(name))
                .unwrap()
                .read_to_end(&mut bytes)
                .unwrap();
            for chunk in bytes.chunks(COMPRESSED_CHUNK_BYTES) {
                tx.blocking_send(Ok(bytes::Bytes::copy_from_slice(chunk)))
                    .unwrap();
            }
            drop(tx);
            let pcm = Arc::new(ArrayQueue::new(20_000_000));
            let result =
                decode_stream(reader, Some(name), 48_000, 2, 0, pcm.clone(), cancel).unwrap();
            eprintln!("decoded {name}: {} frames", result.frames);
            assert!(result.frames > 0, "{name}");
            assert!(!pcm.is_empty(), "{name}");
        }
    }

    #[test]
    fn alac_mp4_with_tail_metadata_beyond_window_decodes_completely() {
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../experiments/playback-probe/fixtures/track-1.m4a");
        let source = std::fs::read(fixture).unwrap();
        let mut offset = 0usize;
        let mut moov_offset = None;
        while offset + 8 <= source.len() {
            let size = u32::from_be_bytes(source[offset..offset + 4].try_into().unwrap()) as usize;
            assert!(size >= 8 && offset + size <= source.len());
            if &source[offset + 4..offset + 8] == b"moov" {
                moov_offset = Some(offset);
                break;
            }
            offset += size;
        }
        let moov_offset = moov_offset.expect("fixture must keep MP4 metadata after media data");
        assert!(source[..moov_offset].windows(4).any(|atom| atom == b"mdat"));

        let free_payload = COMPRESSED_CAPACITY_BYTES + 1024;
        let free_size = free_payload + 8;
        let mut expanded = Vec::with_capacity(source.len() + free_size);
        expanded.extend_from_slice(&source[..moov_offset]);
        expanded.extend_from_slice(&(free_size as u32).to_be_bytes());
        expanded.extend_from_slice(b"free");
        expanded.resize(expanded.len() + free_payload, 0);
        expanded.extend_from_slice(&source[moov_offset..]);

        let mut file = tempfile::Builder::new().suffix(".m4a").tempfile().unwrap();
        file.write_all(&expanded).unwrap();
        file.flush().unwrap();
        let (result, samples, compressed_high_water) =
            decode_test_file_at_with_compressed_high_water(file.path(), 0).unwrap();

        assert_eq!(result.frames, 96_017);
        assert_eq!(samples, 96_017 * 2);
        assert!(compressed_high_water <= COMPRESSED_CAPACITY_BYTES as u64);
    }

    #[test]
    fn decodes_flac_with_an_attached_picture() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/generated-attached-cover.flac");
        let result = decode_test_file(&path).unwrap();
        assert_eq!(result.frames, 192_000);
    }

    #[test]
    fn attached_picture_flac_resume_discards_exact_start_frames() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/generated-attached-cover.flac");
        let (_, samples) = decode_test_file_at(&path, 48_000).unwrap();
        assert_eq!(samples, (192_000 - 48_000) * 2);
    }

    #[test]
    fn attached_picture_flac_with_metadata_beyond_window_decodes_sequentially() {
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/generated-attached-cover.flac");
        let mut source = Vec::new();
        std::fs::File::open(fixture)
            .unwrap()
            .read_to_end(&mut source)
            .unwrap();
        assert_eq!(&source[..4], b"fLaC");
        let mut cursor = 4usize;
        loop {
            let header = cursor;
            let last = source[header] & 0x80 != 0;
            let length = ((source[header + 1] as usize) << 16)
                | ((source[header + 2] as usize) << 8)
                | source[header + 3] as usize;
            cursor += 4 + length;
            if last {
                source[header] &= 0x7f;
                break;
            }
        }
        let padding = COMPRESSED_CAPACITY_BYTES + 1024;
        let mut expanded = Vec::with_capacity(source.len() + padding + 4);
        expanded.extend_from_slice(&source[..cursor]);
        expanded.extend_from_slice(&[
            0x81,
            ((padding >> 16) & 0xff) as u8,
            ((padding >> 8) & 0xff) as u8,
            (padding & 0xff) as u8,
        ]);
        expanded.resize(expanded.len() + padding, 0);
        expanded.extend_from_slice(&source[cursor..]);
        let mut file = tempfile::Builder::new().suffix(".flac").tempfile().unwrap();
        file.write_all(&expanded).unwrap();
        file.flush().unwrap();
        let result = decode_test_file(file.path()).unwrap();
        assert_eq!(result.frames, 192_000);
    }
}
