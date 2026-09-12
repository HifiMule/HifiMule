use anyhow::Context as _;
use crossbeam_queue::ArrayQueue;
use ffmpeg_next::{self as ffmpeg, ChannelLayout, format::Sample, frame::Audio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub struct DecodeSummary {
    pub frames: u64,
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
    let io = if flac_stream {
        ffmpeg::format::context::StreamIo::from_read_with_capacity(reader, 32 * 1024)?
    } else {
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
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, reader) = BoundedHttpReader::channel(cancel.clone());
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
        let result = decode_stream(
            reader,
            path.file_name().and_then(|name| name.to_str()),
            48_000,
            2,
            start_frame,
            pcm,
            cancel,
        );
        done.store(true, Ordering::Release);
        consumer.join().expect("PCM consumer panicked");
        let producer = producer.join().expect("compressed producer panicked");
        match result {
            Err(error) => Err(error),
            Ok(summary) => {
                producer.map_err(anyhow::Error::msg)?;
                Ok((summary, drained.load(Ordering::Relaxed)))
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
