use anyhow::Context as _;
use crossbeam_queue::ArrayQueue;
use ffmpeg_next::{self as ffmpeg, ChannelLayout, format::Sample, frame::Audio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::providers::PlaybackSeekMechanism;

pub const UNKNOWN_SEEK_LANDING_FRAME: u64 = u64::MAX;

#[inline]
fn apply_album_gain(sample: f32, gain: f32) -> f32 {
    if gain.to_bits() == 1.0f32.to_bits() {
        sample
    } else {
        sample * gain
    }
}

pub struct DecodeSummary {
    pub frames: u64,
    pub emitted_frames: u64,
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

#[cfg(test)]
pub fn decode_stream(
    reader: super::streaming::BoundedHttpReader,
    filename_hint: Option<&str>,
    output_rate: u32,
    output_channels: u16,
    start_frame: u64,
    pcm: Arc<ArrayQueue<f32>>,
    cancel: Arc<AtomicBool>,
) -> anyhow::Result<DecodeSummary> {
    decode_stream_with_seek(
        reader,
        filename_hint,
        output_rate,
        output_channels,
        start_frame,
        None,
        false,
        None,
        None,
        None,
        pcm,
        cancel,
    )
}

#[allow(clippy::too_many_arguments)]
#[cfg(test)]
pub fn decode_stream_with_seek(
    reader: super::streaming::BoundedHttpReader,
    filename_hint: Option<&str>,
    output_rate: u32,
    output_channels: u16,
    start_frame: u64,
    seek_mechanism: Option<PlaybackSeekMechanism>,
    media_seek_requested: bool,
    provider_duration_ms: Option<u64>,
    decoded_duration_ms: Option<Arc<AtomicU64>>,
    seek_landing_frame: Option<Arc<AtomicU64>>,
    pcm: Arc<ArrayQueue<f32>>,
    cancel: Arc<AtomicBool>,
) -> anyhow::Result<DecodeSummary> {
    decode_stream_with_seek_and_gain(
        reader,
        filename_hint,
        output_rate,
        output_channels,
        start_frame,
        seek_mechanism,
        media_seek_requested,
        provider_duration_ms,
        decoded_duration_ms,
        seek_landing_frame,
        1.0,
        None,
        pcm,
        cancel,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn decode_stream_with_seek_and_gain(
    mut reader: super::streaming::BoundedHttpReader,
    filename_hint: Option<&str>,
    output_rate: u32,
    output_channels: u16,
    start_frame: u64,
    seek_mechanism: Option<PlaybackSeekMechanism>,
    media_seek_requested: bool,
    provider_duration_ms: Option<u64>,
    decoded_duration_ms: Option<Arc<AtomicU64>>,
    seek_landing_frame: Option<Arc<AtomicU64>>,
    gain: f32,
    qualified_suffix: Option<&str>,
    pcm: Arc<ArrayQueue<f32>>,
    cancel: Arc<AtomicBool>,
) -> anyhow::Result<DecodeSummary> {
    if !gain.is_finite() || gain <= 0.0 {
        anyhow::bail!("invalid frozen album gain");
    }
    let source_failure = reader.failure_state();
    let preparation = reader.preparation();
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
    // MP3 must remain sequential, but its demuxer still needs AVSEEK_SIZE to
    // validate Xing/LAME frame counts and attach exact terminal discard data.
    // A read-only AVIO with no size callback loses that padding information.
    // Keep the reader's bounded size/seek callback while explicitly disabling
    // random-access advertising, so probing cannot evict and reread the head.
    let sequential_mp3 = !media_seek_requested && reader_is_mp3(&mut reader, filename_hint)?;
    let io = if !media_seek_requested && flac_stream {
        ffmpeg::format::context::StreamIo::from_read_with_capacity(reader, 32 * 1024)?
    } else {
        let mut io =
            ffmpeg::format::context::StreamIo::from_read_seek_with_capacity(reader, 32 * 1024)?;
        if sequential_mp3 {
            // SAFETY: this worker exclusively owns the not-yet-opened AVIO.
            // Only capability flags change; callbacks, buffer and ownership stay intact.
            unsafe {
                (*io.as_mut_ptr()).seekable = 0;
            }
        }
        io
    };
    let token = cancel.clone();
    let mut input =
        ffmpeg::format::input_from_stream_with_interrupt(io, filename_hint, None, move || {
            token.load(Ordering::Acquire)
                || preparation
                    .as_ref()
                    .is_some_and(|preparation| preparation.check().is_err())
        })?;
    let container_name = input.format().name().to_ascii_lowercase();
    let stream = input
        .streams()
        .find(|s| s.parameters().medium() == ffmpeg::media::Type::Audio)
        .ok_or_else(|| anyhow::anyhow!("no audio stream"))?;
    let stream_index = stream.index();
    let stream_time_base = stream.time_base();
    let stream_start = stream.start_time();
    let stream_duration = stream.duration();
    let context = ffmpeg::codec::context::Context::from_parameters(stream.parameters())?;
    let mut decoder = context.decoder().audio()?;
    if let Some(suffix) = qualified_suffix
        && !decoded_representation_matches(suffix, &container_name, decoder.id().name())
    {
        anyhow::bail!("resolved representation contradicts frozen album gain policy");
    }
    let mut validated_duration_ms = None;
    if let Some(seek_mechanism) = seek_mechanism {
        let container = container_name.clone();
        let codec = decoder.id().name();
        let qualified = seek_representation_matches(seek_mechanism, &container, codec);
        if qualified {
            validated_duration_ms = validated_seek_duration_ms(
                seek_mechanism,
                stream_duration,
                stream_time_base.numerator(),
                stream_time_base.denominator(),
                provider_duration_ms,
            );
            if let (Some(duration), Some(observed)) = (validated_duration_ms, &decoded_duration_ms)
            {
                observed.store(duration, Ordering::Release);
            }
        }
        if media_seek_requested && validated_duration_ms.is_none() {
            anyhow::bail!("representation or duration failed media seek qualification");
        }
    }
    let start_frame = if !media_seek_requested
        && validated_duration_ms
            .or(provider_duration_ms)
            .filter(|duration| *duration > 0)
            .is_some_and(|duration| {
                start_frame == duration.saturating_mul(u64::from(output_rate)) / 1000
            }) {
        0 // A restored terminal cursor restarts only on explicit Resume.
    } else {
        start_frame
    };
    if media_seek_requested
        && validated_duration_ms.is_some_and(|duration| {
            start_frame >= duration.saturating_mul(u64::from(output_rate)) / 1000
        })
    {
        anyhow::bail!("seek target is not before validated media end");
    }
    if media_seek_requested && start_frame > 0 {
        let relative_us = start_frame
            .checked_mul(1_000_000)
            .and_then(|value| value.checked_div(u64::from(output_rate)))
            .and_then(|value| i64::try_from(value).ok())
            .ok_or_else(|| anyhow::anyhow!("seek timestamp overflow"))?;
        let origin_us = if stream_start == ffmpeg::ffi::AV_NOPTS_VALUE {
            0
        } else {
            let scaled = i128::from(stream_start)
                .saturating_mul(i128::from(stream_time_base.numerator()))
                .saturating_mul(1_000_000)
                / i128::from(stream_time_base.denominator());
            i64::try_from(scaled.max(0))
                .map_err(|_| anyhow::anyhow!("stream timestamp origin overflow"))?
        };
        // Compressed codecs may need packets before the requested timestamp to
        // reconstruct predictor or bit-reservoir state. Decode a bounded lead-in
        // and discard it below so the first emitted sample still maps to target.
        let preroll_us = if matches!(
            seek_mechanism,
            Some(PlaybackSeekMechanism::JellyfinOriginalPcmWav)
                | Some(PlaybackSeekMechanism::NavidromeOriginalPcmWav)
        ) {
            0
        } else {
            50_000
        };
        let timestamp = origin_us
            .checked_add(relative_us.saturating_sub(preroll_us))
            .ok_or_else(|| anyhow::anyhow!("seek timestamp overflow"))?;
        input
            .seek(timestamp, ..timestamp.saturating_add(1))
            .context("seek audio demuxer")?;
        decoder.flush();
    }
    let output_layout = match output_channels {
        1 => ChannelLayout::MONO,
        2 => ChannelLayout::STEREO,
        _ => anyhow::bail!("unsupported output layout"),
    };
    let mut resampler: Option<ffmpeg::software::resampling::Context> = None;
    let mut frames = 0u64;
    let mut emitted_samples = 0u64;
    let mut discard_samples = if media_seek_requested {
        0
    } else {
        start_frame.saturating_mul(u64::from(output_channels))
    };
    let mut seek_trim_pending = media_seek_requested && start_frame > 0;
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
            if seek_trim_pending {
                let pts = frame
                    .pts()
                    .ok_or_else(|| anyhow::anyhow!("seek landing frame has no timestamp"))?;
                let origin = if stream_start == ffmpeg::ffi::AV_NOPTS_VALUE {
                    0
                } else {
                    stream_start
                };
                let relative = pts.saturating_sub(origin).max(0) as i128;
                let numerator = i128::from(stream_time_base.numerator()) * i128::from(output_rate);
                let denominator = i128::from(stream_time_base.denominator());
                if denominator <= 0 {
                    anyhow::bail!("invalid audio time base");
                }
                let landed_frame = u64::try_from(relative.saturating_mul(numerator) / denominator)
                    .map_err(|_| anyhow::anyhow!("seek landing timestamp overflow"))?;
                let tolerance_frames = u64::from(output_rate) * 50 / 1000;
                if landed_frame > start_frame.saturating_add(tolerance_frames) {
                    anyhow::bail!("seek landing exceeded 50 ms tolerance");
                }
                discard_samples = start_frame
                    .saturating_sub(landed_frame)
                    .saturating_mul(u64::from(output_channels));
                seek_trim_pending = false;
            }
            let frame_start = if media_seek_requested {
                let pts = frame
                    .pts()
                    .ok_or_else(|| anyhow::anyhow!("seek output frame has no timestamp"))?;
                let origin = if stream_start == ffmpeg::ffi::AV_NOPTS_VALUE {
                    0
                } else {
                    stream_start
                };
                let relative = pts.saturating_sub(origin).max(0) as i128;
                let numerator = i128::from(stream_time_base.numerator()) * i128::from(output_rate);
                let denominator = i128::from(stream_time_base.denominator());
                Some(
                    u64::try_from(relative.saturating_mul(numerator) / denominator)
                        .map_err(|_| anyhow::anyhow!("seek output timestamp overflow"))?,
                )
            } else {
                None
            };
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
            for (sample_index, sample) in converted.plane::<f32>(0).iter().enumerate() {
                if discard_samples > 0 {
                    discard_samples -= 1;
                    continue;
                }
                if let (Some(landing), Some(frame_start)) = (&seek_landing_frame, frame_start) {
                    let actual = frame_start
                        .saturating_add(sample_index as u64 / u64::from(output_channels));
                    let _ = landing.compare_exchange(
                        UNKNOWN_SEEK_LANDING_FRAME,
                        actual,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    );
                }
                let sample = apply_album_gain(*sample, gain);
                while pcm.push(sample).is_err() {
                    if cancel.load(Ordering::Acquire) {
                        anyhow::bail!("cancelled");
                    }
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
                emitted_samples = emitted_samples.saturating_add(1);
            }
            frames += converted.samples() as u64;
        }
        Ok(())
    };
    let mut packet_count = 0u64;
    loop {
        if cancel.load(Ordering::Acquire) {
            anyhow::bail!("cancelled");
        }
        let mut packet = ffmpeg::Packet::empty();
        match packet.read(&mut input) {
            Ok(()) => {}
            Err(ffmpeg::Error::Eof) => break,
            Err(error) => return Err(anyhow::Error::new(error).context("read audio packet")),
        }
        if packet.stream() != stream_index {
            continue;
        }
        packet_count += 1;
        // FFmpeg applies validated packet/container skip and discard metadata.
        // Tags, silent packets and provider durations cannot authorize extra trimming.
        decoder.send_packet(&packet).with_context(|| {
            format!("send audio packet {packet_count} ({} bytes)", packet.size())
        })?;
        receive(&mut decoder, false).context("receive audio frame")?;
    }
    if let Some(error) = source_failure.error() {
        return Err(error.into());
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
                let sample = apply_album_gain(*sample, gain);
                while pcm.push(sample).is_err() {
                    if cancel.load(Ordering::Acquire) {
                        anyhow::bail!("cancelled");
                    }
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
                emitted_samples = emitted_samples.saturating_add(1);
            }
            frames += tail.samples() as u64;
            if delay.is_none() {
                return Ok(DecodeSummary {
                    frames,
                    emitted_frames: emitted_samples / u64::from(output_channels),
                });
            }
            // swr_get_delay may retain fixed filter latency after the final
            // drainable sample. Require stable delay across another empty
            // flush before treating that latency as non-drainable.
            if tail.samples() == 0 && previous_delay == delay {
                return Ok(DecodeSummary {
                    frames,
                    emitted_frames: emitted_samples / u64::from(output_channels),
                });
            }
            previous_delay = delay;
        }
        anyhow::bail!("resampler did not drain");
    }
    Ok(DecodeSummary {
        frames,
        emitted_frames: emitted_samples / u64::from(output_channels),
    })
}

fn decoded_representation_matches(suffix: &str, container: &str, codec: &str) -> bool {
    let suffix = suffix.trim().trim_start_matches('.').to_ascii_lowercase();
    let container = container.to_ascii_lowercase();
    let codec = codec.to_ascii_lowercase();
    match suffix.as_str() {
        "wav" => {
            container.contains("wav")
                && matches!(codec.as_str(), "pcm_s16le" | "pcm_s24le" | "pcm_s32le")
        }
        "flac" => container.contains("flac") && codec == "flac",
        "m4a" => {
            (container.contains("mov") || container.contains("mp4") || container.contains("m4a"))
                && matches!(codec.as_str(), "alac" | "aac")
        }
        "mp3" => container.contains("mp3") && codec == "mp3",
        _ => false,
    }
}

// Provider duration is whole seconds. Reconcile sub-second rounding, but never
// qualify a contradictory (one second or more) or absent duration as a precise end.
fn seek_representation_matches(
    mechanism: PlaybackSeekMechanism,
    container: &str,
    codec: &str,
) -> bool {
    let has_container = |expected| container.split(',').any(|name| name == expected);
    match mechanism {
        PlaybackSeekMechanism::JellyfinOriginalPcmWav
        | PlaybackSeekMechanism::NavidromeOriginalPcmWav => {
            has_container("wav") && matches!(codec, "pcm_s16le" | "pcm_s24le" | "pcm_s32le")
        }
        PlaybackSeekMechanism::JellyfinOriginalM4a
        | PlaybackSeekMechanism::NavidromeOriginalM4a => {
            has_container("mov") && matches!(codec, "aac" | "alac")
        }
        PlaybackSeekMechanism::AudiobookshelfDirectM4a => has_container("mov") && codec == "aac",
        PlaybackSeekMechanism::JellyfinOriginalOpus
        | PlaybackSeekMechanism::NavidromeOriginalOpus => has_container("ogg") && codec == "opus",
        PlaybackSeekMechanism::JellyfinOriginalMp3
        | PlaybackSeekMechanism::NavidromeOriginalMp3
        | PlaybackSeekMechanism::AudiobookshelfDirectMp3 => has_container("mp3") && codec == "mp3",
        PlaybackSeekMechanism::JellyfinOriginalFlac
        | PlaybackSeekMechanism::NavidromeOriginalFlac => has_container("flac") && codec == "flac",
    }
}

fn validated_seek_duration_ms(
    mechanism: PlaybackSeekMechanism,
    ticks: i64,
    numerator: i32,
    denominator: i32,
    provider: Option<u64>,
) -> Option<u64> {
    let provider = provider.filter(|duration| *duration > 0)?;
    if ticks <= 0 || numerator <= 0 || denominator <= 0 {
        return matches!(
            mechanism,
            PlaybackSeekMechanism::JellyfinOriginalMp3
                | PlaybackSeekMechanism::NavidromeOriginalMp3
        )
        .then_some(provider);
    }
    let duration = u64::try_from(
        i128::from(ticks)
            .checked_mul(i128::from(numerator))?
            .checked_mul(1000)?
            / i128::from(denominator),
    )
    .ok()?;
    (duration > 0 && duration <= 9_007_199_254_740_991 && duration.abs_diff(provider) < 1000)
        .then_some(duration)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn album_gain_has_a_bit_exact_unity_path_and_scales_once() {
        for sample in [0.0f32, -0.0, 0.25, -0.5, 1.25] {
            assert_eq!(apply_album_gain(sample, 1.0).to_bits(), sample.to_bits());
        }
        assert!((apply_album_gain(0.4, 0.75) - 0.3).abs() <= f32::EPSILON);
        assert!((apply_album_gain(-0.4, 0.75) + 0.3).abs() <= f32::EPSILON);
    }

    fn decode_fixture_samples(path: &std::path::Path, gain: f32) -> (DecodeSummary, Vec<f32>) {
        let cancel = Arc::new(AtomicBool::new(false));
        let reader = BoundedHttpReader::from_source(
            std::fs::File::open(path).unwrap(),
            cancel.clone(),
            Arc::new(AtomicU64::new(0)),
        );
        let pcm = Arc::new(ArrayQueue::<f32>::new(64 * 1024));
        let drain_pcm = pcm.clone();
        let done = Arc::new(AtomicBool::new(false));
        let drain_done = done.clone();
        let samples = Arc::new(std::sync::Mutex::new(Vec::new()));
        let drain_samples = samples.clone();
        let consumer = std::thread::spawn(move || {
            while !drain_done.load(Ordering::Acquire) || !drain_pcm.is_empty() {
                if let Some(sample) = drain_pcm.pop() {
                    drain_samples.lock().unwrap().push(sample);
                } else {
                    std::thread::yield_now();
                }
            }
        });
        let summary = decode_stream_with_seek_and_gain(
            reader,
            path.file_name().and_then(|name| name.to_str()),
            48_000,
            2,
            0,
            None,
            false,
            None,
            None,
            None,
            gain,
            Some("wav"),
            pcm,
            cancel,
        )
        .unwrap();
        done.store(true, Ordering::Release);
        consumer.join().unwrap();
        let samples = Arc::try_unwrap(samples).unwrap().into_inner().unwrap();
        (summary, samples)
    }

    #[test]
    fn production_decoder_scales_converted_and_drained_samples_once() {
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/generated-seek-pcm16.wav");
        let (unity_summary, unity) = decode_fixture_samples(&fixture, 1.0);
        let (scaled_summary, scaled) = decode_fixture_samples(&fixture, 0.5);
        assert_eq!(unity_summary.emitted_frames, scaled_summary.emitted_frames);
        assert_eq!(unity.len(), scaled.len());
        assert!(unity.iter().any(|sample| *sample == 0.0));
        for (original, adjusted) in unity.iter().zip(&scaled) {
            assert_eq!(adjusted.to_bits(), (original * 0.5).to_bits());
        }
    }
    use crate::playback::streaming::{
        BoundedHttpReader, COMPRESSED_CAPACITY_BYTES, COMPRESSED_CHUNK_BYTES,
    };
    use std::io::{Read, Write};

    #[test]
    fn review_source_error_cannot_be_clean_decoder_eof() {
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, reader) = BoundedHttpReader::channel(cancel.clone());
        let bytes = include_bytes!("../../tests/fixtures/generated-pcm16.aif");
        for part in bytes.chunks(COMPRESSED_CHUNK_BYTES) {
            tx.blocking_send(Ok(bytes::Bytes::copy_from_slice(part)))
                .unwrap();
        }
        tx.blocking_send(Err(super::super::streaming::StreamReadError::Timeout))
            .unwrap();
        drop(tx);
        let result = decode_stream(
            reader,
            Some("stream.aif"),
            48_000,
            2,
            0,
            Arc::new(ArrayQueue::new(1_000_000)),
            cancel,
        );
        assert!(
            result.is_err(),
            "terminal source error was accepted as clean EOF"
        );
    }

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
        let reader = BoundedHttpReader::from_source(
            std::fs::File::open(path)?,
            cancel.clone(),
            compressed_high_water.clone(),
        );
        let pcm = Arc::new(ArrayQueue::<f32>::new(64 * 1024));
        let invalid_pcm = Arc::new(AtomicBool::new(false));
        let consumer_invalid_pcm = invalid_pcm.clone();
        let done = Arc::new(AtomicBool::new(false));
        let drained = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let drain_pcm = pcm.clone();
        let drain_done = done.clone();
        let drain_count = drained.clone();
        let consumer = std::thread::spawn(move || {
            while !drain_done.load(Ordering::Acquire) || !drain_pcm.is_empty() {
                if let Some(sample) = drain_pcm.pop() {
                    if !sample.is_finite() {
                        consumer_invalid_pcm.store(true, Ordering::Relaxed);
                    }
                    drain_count.fetch_add(1, Ordering::Relaxed);
                } else {
                    std::thread::yield_now();
                }
            }
        });
        let result = decode_stream(reader, hint, 48_000, 2, start_frame, pcm, cancel);
        done.store(true, Ordering::Release);
        consumer.join().expect("PCM consumer panicked");
        assert!(
            !invalid_pcm.load(Ordering::Relaxed),
            "non-finite decoded PCM"
        );
        match result {
            Err(error) => Err(error),
            Ok(summary) => Ok((
                summary,
                drained.load(Ordering::Relaxed),
                compressed_high_water.load(Ordering::Relaxed),
            )),
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
    #[ignore = "uses HIFIMULE_DIAGNOSTIC_M4R; never committed as a fixture"]
    fn diagnostic_external_m4r() {
        let path = std::env::var("HIFIMULE_DIAGNOSTIC_M4R").unwrap();
        let (result, samples) = decode_test_file_at(std::path::Path::new(&path), 0).unwrap();
        assert!(result.frames > 0, "decoded no frames");
        assert!(samples > 0, "decoded no PCM samples");
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

    fn collect_seek_fixture(
        name: &str,
        start_frame: u64,
        seek_mechanism: Option<PlaybackSeekMechanism>,
        media_seek: bool,
    ) -> (Vec<f32>, Option<u64>, bool) {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        let cancel = Arc::new(AtomicBool::new(false));
        let reader = BoundedHttpReader::from_source(
            std::fs::File::open(path).unwrap(),
            cancel.clone(),
            Arc::new(std::sync::atomic::AtomicU64::new(0)),
        );
        let pcm = Arc::new(ArrayQueue::new(220_000));
        let landing = media_seek.then(|| Arc::new(AtomicU64::new(UNKNOWN_SEEK_LANDING_FRAME)));
        let qualified = Arc::new(AtomicU64::new(0));
        decode_stream_with_seek(
            reader,
            Some(name),
            48_000,
            2,
            start_frame,
            seek_mechanism,
            media_seek,
            Some(2_000),
            Some(qualified.clone()),
            landing.clone(),
            pcm.clone(),
            cancel,
        )
        .unwrap();
        let mut samples = Vec::new();
        while let Some(sample) = pcm.pop() {
            samples.push(sample);
        }
        let landing = landing
            .map(|landing| landing.load(Ordering::Acquire))
            .filter(|landing| *landing != UNKNOWN_SEEK_LANDING_FRAME);
        if media_seek {
            assert!(qualified.load(Ordering::Acquire) > 0);
        }
        (samples, landing, qualified.load(Ordering::Acquire) > 0)
    }

    #[test]
    fn qualified_pcm_wav_matrix_lands_on_independent_fixture_samples() {
        for name in [
            "generated-seek-pcm16.wav",
            "generated-seek-pcm24.wav",
            "generated-seek-pcm32.wav",
        ] {
            let (all, _, _) = collect_seek_fixture(name, 0, None, false);
            for target_frame in [1u64, 36_000, 90_000, 36_000] {
                let (sought, actual_landing, qualified) = collect_seek_fixture(
                    name,
                    target_frame,
                    Some(PlaybackSeekMechanism::JellyfinOriginalPcmWav),
                    true,
                );
                assert!(qualified, "{name}");
                let expected_offset = target_frame as usize * 2;
                let compared = sought.len().min(9_600);
                assert!(compared > 0, "{name} at {target_frame}");
                for (actual, expected) in sought[..compared]
                    .iter()
                    .zip(&all[expected_offset..expected_offset + compared])
                {
                    assert!(
                        (actual - expected).abs() < 1.0e-6,
                        "{name} at {target_frame}"
                    );
                }
                let landed_frames = (all.len() - sought.len()) / 2;
                assert!(
                    landed_frames.abs_diff(target_frame as usize) <= 48_000 * 50 / 1000,
                    "{name} at {target_frame}"
                );
                assert!(
                    actual_landing.unwrap().abs_diff(target_frame) <= 48_000 * 50 / 1000,
                    "reported landing for {name} at {target_frame}"
                );
            }
        }
    }

    #[test]
    fn qualified_compressed_matrix_lands_on_independent_fixture_samples() {
        for (name, mechanism, max_mean_error) in [
            (
                "generated-seek-aac.m4a",
                PlaybackSeekMechanism::JellyfinOriginalM4a,
                0.015,
            ),
            (
                "generated-seek-aac.m4a",
                PlaybackSeekMechanism::AudiobookshelfDirectM4a,
                0.015,
            ),
            (
                "generated-seek-alac.m4a",
                PlaybackSeekMechanism::JellyfinOriginalM4a,
                1.0e-6,
            ),
            (
                "generated-seek-opus.oga",
                PlaybackSeekMechanism::JellyfinOriginalOpus,
                0.015,
            ),
            (
                "generated-seek-mp3.mp3",
                PlaybackSeekMechanism::JellyfinOriginalMp3,
                0.02,
            ),
            (
                "generated-seek-mp3.mp3",
                PlaybackSeekMechanism::AudiobookshelfDirectMp3,
                0.02,
            ),
            (
                "generated-seek-flac.flac",
                PlaybackSeekMechanism::JellyfinOriginalFlac,
                1.0e-6,
            ),
        ] {
            let (all, _, _) = collect_seek_fixture(name, 0, None, false);
            for target_frame in [36_000u64, 90_000, 36_000] {
                let (sought, actual_landing, qualified) =
                    collect_seek_fixture(name, target_frame, Some(mechanism), true);
                assert!(qualified, "{name}");
                let compared = sought.len().min(2_048);
                assert!(compared > 0, "{name} at {target_frame}");
                let tolerance_frames = 48_000usize * 50 / 1000;
                let first_frame = (target_frame as usize).saturating_sub(tolerance_frames);
                let last_frame = (target_frame as usize + tolerance_frames)
                    .min((all.len().saturating_sub(compared)) / 2);
                let (matched_frame, mean_error) = (first_frame..=last_frame)
                    .map(|candidate| {
                        let offset = candidate * 2;
                        let error = sought[..compared]
                            .iter()
                            .zip(&all[offset..offset + compared])
                            .map(|(actual, expected)| f64::from((actual - expected).abs()))
                            .sum::<f64>()
                            / compared as f64;
                        (candidate, error)
                    })
                    .min_by(|left, right| left.1.total_cmp(&right.1))
                    .unwrap();
                assert!(
                    mean_error < max_mean_error,
                    "{name} at {target_frame}: mean sample error {mean_error}"
                );
                assert!(
                    matched_frame.abs_diff(target_frame as usize) <= tolerance_frames,
                    "correlated landing for {name} at {target_frame}"
                );
                assert!(
                    actual_landing.unwrap().abs_diff(target_frame) <= 48_000 * 50 / 1000,
                    "reported landing for {name} at {target_frame}"
                );
            }
        }
    }

    #[test]
    fn mp3_qualification_uses_positive_provider_duration_when_probe_duration_is_unknown() {
        let (samples, landing, qualified) = collect_seek_fixture(
            "generated-seek-mp3.mp3",
            0,
            Some(PlaybackSeekMechanism::JellyfinOriginalMp3),
            false,
        );
        assert!(qualified);
        assert!(landing.is_none());
        assert!(!samples.is_empty());
    }

    #[test]
    fn seek_mechanism_requires_the_matching_opened_container_and_codec() {
        assert!(seek_representation_matches(
            PlaybackSeekMechanism::JellyfinOriginalM4a,
            "mov,mp4,m4a,3gp,3g2,mj2",
            "aac"
        ));
        assert!(seek_representation_matches(
            PlaybackSeekMechanism::JellyfinOriginalM4a,
            "mov,mp4,m4a,3gp,3g2,mj2",
            "alac"
        ));
        assert!(seek_representation_matches(
            PlaybackSeekMechanism::JellyfinOriginalOpus,
            "ogg",
            "opus"
        ));
        assert!(!seek_representation_matches(
            PlaybackSeekMechanism::JellyfinOriginalM4a,
            "mov,mp4,m4a,3gp,3g2,mj2",
            "mp3"
        ));
        assert!(!seek_representation_matches(
            PlaybackSeekMechanism::JellyfinOriginalOpus,
            "ogg",
            "vorbis"
        ));
        assert!(seek_representation_matches(
            PlaybackSeekMechanism::JellyfinOriginalMp3,
            "mp3",
            "mp3"
        ));
        assert!(seek_representation_matches(
            PlaybackSeekMechanism::JellyfinOriginalFlac,
            "flac",
            "flac"
        ));
        assert!(seek_representation_matches(
            PlaybackSeekMechanism::NavidromeOriginalPcmWav,
            "wav",
            "pcm_s24le"
        ));
        assert!(seek_representation_matches(
            PlaybackSeekMechanism::NavidromeOriginalM4a,
            "mov,mp4,m4a,3gp,3g2,mj2",
            "alac"
        ));
        assert!(seek_representation_matches(
            PlaybackSeekMechanism::NavidromeOriginalOpus,
            "ogg",
            "opus"
        ));
        assert!(seek_representation_matches(
            PlaybackSeekMechanism::NavidromeOriginalMp3,
            "mp3",
            "mp3"
        ));
        assert!(seek_representation_matches(
            PlaybackSeekMechanism::AudiobookshelfDirectMp3,
            "mp3",
            "mp3"
        ));
        assert!(seek_representation_matches(
            PlaybackSeekMechanism::AudiobookshelfDirectM4a,
            "mov,mp4,m4a,3gp,3g2,mj2",
            "aac"
        ));
        assert!(!seek_representation_matches(
            PlaybackSeekMechanism::AudiobookshelfDirectM4a,
            "mov,mp4,m4a,3gp,3g2,mj2",
            "alac"
        ));
        assert!(seek_representation_matches(
            PlaybackSeekMechanism::NavidromeOriginalFlac,
            "flac",
            "flac"
        ));
    }

    #[test]
    fn unsupported_wav_candidate_keeps_ordinary_playback_usable() {
        let (samples, landing, qualified) = collect_seek_fixture(
            "generated-seek-pcm-f32.wav",
            0,
            Some(PlaybackSeekMechanism::JellyfinOriginalPcmWav),
            false,
        );
        assert!(!qualified);
        assert!(landing.is_none());
        assert!(samples.len() >= 96_000);
    }

    #[test]
    fn id3_tag_followed_by_mpeg_audio_frame_is_recognized_as_mp3() {
        let bytes = [
            b'I', b'D', b'3', 4, 0, 0, 0, 0, 0, 0, 0xff, 0xfb, 0x90, 0x64,
        ];
        assert!(prefix_is_mp3(&bytes));
    }

    const NEW_FORMAT_FIXTURES: &[(&str, u64)] = &[
        ("generated-pcm16.aif", 96_000),
        ("generated-pcm24.aiff", 96_000),
        ("generated-pcm32.aiff", 96_000),
        ("generated-vorbis.ogg", 96_000),
        ("generated-opus.oga", 96_000),
        ("generated-wmav1.wma", 96_000),
        ("generated-wmav2.wma", 96_000),
    ];

    // Expand metadata at runtime so eviction coverage does not require huge binary fixtures.
    fn with_large_metadata(source: &[u8], name: &str) -> Vec<u8> {
        let padding = COMPRESSED_CAPACITY_BYTES + 1024;
        if source.starts_with(b"FORM") {
            let mut expanded = source[..12].to_vec();
            expanded.extend_from_slice(b"ANNO");
            expanded.extend_from_slice(&(padding as u32).to_be_bytes());
            expanded.resize(expanded.len() + padding, b' ');
            expanded.extend_from_slice(&source[12..]);
            let size = (expanded.len() - 8) as u32;
            expanded[4..8].copy_from_slice(&size.to_be_bytes());
            return expanded;
        }
        if name.ends_with(".wma") {
            let header_size = u64::from_le_bytes(source[16..24].try_into().unwrap()) as usize;
            let mut expanded = source[..header_size].to_vec();
            // ASF Padding Object GUID, followed by its 64-bit object size.
            expanded.extend_from_slice(&[
                0x74, 0xd4, 0x06, 0x18, 0xdf, 0xca, 0x09, 0x45, 0xa4, 0xba, 0x9a, 0xab, 0xcb, 0x96,
                0xaa, 0xe8,
            ]);
            expanded.extend_from_slice(&((padding + 24) as u64).to_le_bytes());
            expanded.resize(expanded.len() + padding, 0);
            let new_header_size = expanded.len() as u64;
            expanded[16..24].copy_from_slice(&new_header_size.to_le_bytes());
            let objects = u32::from_le_bytes(source[24..28].try_into().unwrap());
            expanded[24..28].copy_from_slice(&(objects + 1).to_le_bytes());
            expanded.extend_from_slice(&source[header_size..]);
            // The generated fixtures start with the standard File Properties Object.
            let file_size = expanded.len() as u64;
            expanded[70..78].copy_from_slice(&file_size.to_le_bytes());
            return expanded;
        }
        assert!(source.starts_with(b"OggS"));
        let mut expanded = Vec::new();
        let mut offset = 0;
        let mut sequence = 0u32;
        while offset < source.len() {
            let segments = source[offset + 26] as usize;
            let payload_offset = offset + 27 + segments;
            let lacing = &source[offset + 27..payload_offset];
            let payload_size: usize = lacing.iter().map(|&v| v as usize).sum();
            let end = payload_offset + payload_size;
            if offset > 0 && expanded.len() < 128 {
                // The second page starts with Vorbis comments or OpusTags. Replace
                // only its vendor string, preserving comment entries and setup data.
                let prefix_size = if name.ends_with(".ogg") { 7 } else { 8 };
                let vendor_size = u32::from_le_bytes(
                    source[payload_offset + prefix_size..payload_offset + prefix_size + 4]
                        .try_into()
                        .unwrap(),
                ) as usize;
                let first_packet_size: usize = lacing
                    .iter()
                    .take_while(|&&v| v == 255)
                    .map(|&v| v as usize)
                    .sum::<usize>()
                    + usize::from(*lacing.iter().find(|&&v| v < 255).unwrap());
                let mut packet = source[payload_offset..payload_offset + prefix_size].to_vec();
                packet.extend_from_slice(&(padding as u32).to_le_bytes());
                packet.resize(packet.len() + padding, b'v');
                packet.extend_from_slice(
                    &source[payload_offset + prefix_size + 4 + vendor_size
                        ..payload_offset + first_packet_size],
                );
                let mut packet_offset = 0;
                while packet_offset < packet.len() {
                    let length = (packet.len() - packet_offset).min(255 * 255);
                    let mut page_lacing = vec![255; length / 255];
                    if length < 255 * 255 {
                        page_lacing.push((length % 255) as u8);
                    }
                    let continued = u8::from(packet_offset > 0);
                    append_ogg_page(
                        &mut expanded,
                        &source[offset..offset + 27],
                        continued,
                        sequence,
                        &page_lacing,
                        &packet[packet_offset..packet_offset + length],
                    );
                    packet_offset += length;
                    sequence += 1;
                }
                let first_segments = first_packet_size / 255 + 1;
                if first_segments < segments {
                    append_ogg_page(
                        &mut expanded,
                        &source[offset..offset + 27],
                        0,
                        sequence,
                        &lacing[first_segments..],
                        &source[payload_offset + first_packet_size..end],
                    );
                    sequence += 1;
                }
            } else {
                append_ogg_page(
                    &mut expanded,
                    &source[offset..offset + 27],
                    source[offset + 5],
                    sequence,
                    lacing,
                    &source[payload_offset..end],
                );
                sequence += 1;
            }
            offset = end;
        }
        expanded
    }

    fn append_ogg_page(
        output: &mut Vec<u8>,
        header: &[u8],
        flags: u8,
        sequence: u32,
        lacing: &[u8],
        payload: &[u8],
    ) {
        let mut page = header.to_vec();
        page[5] = flags;
        page[18..22].copy_from_slice(&sequence.to_le_bytes());
        page[22..26].fill(0);
        page[26] = lacing.len() as u8;
        page.extend_from_slice(lacing);
        page.extend_from_slice(payload);
        let mut crc = 0u32;
        for &byte in &page {
            crc ^= u32::from(byte) << 24;
            for _ in 0..8 {
                crc = if crc & 0x8000_0000 != 0 {
                    (crc << 1) ^ 0x04c1_1db7
                } else {
                    crc << 1
                };
            }
        }
        page[22..26].copy_from_slice(&crc.to_le_bytes());
        output.extend_from_slice(&page);
    }

    #[test]
    fn new_formats_with_metadata_beyond_window_decode_completely_and_resume() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        for &(name, _) in NEW_FORMAT_FIXTURES {
            let source = std::fs::read(root.join(name)).unwrap();
            let (baseline, _) = decode_test_file_at(&root.join(name), 0).unwrap();
            let expanded = with_large_metadata(&source, name);
            assert!(expanded.len() > COMPRESSED_CAPACITY_BYTES);
            let suffix = format!(".{}", name.rsplit('.').next().unwrap());
            let mut file = tempfile::Builder::new().suffix(&suffix).tempfile().unwrap();
            file.write_all(&expanded).unwrap();
            file.flush().unwrap();
            for start in [0, 48_000] {
                let (summary, samples, high_water) =
                    decode_test_file_at_with_compressed_high_water(file.path(), start)
                        .unwrap_or_else(|error| panic!("{name}, start {start}: {error:#}"));
                assert_eq!(summary.frames, baseline.frames, "{name}");
                assert_eq!(samples, (baseline.frames - start) * 2, "{name}");
                assert!(high_water <= COMPRESSED_CAPACITY_BYTES as u64, "{name}");
            }
        }
    }

    #[test]
    fn new_formats_decode_complete_finite_pcm_and_resume() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        for &(name, expected_frames) in NEW_FORMAT_FIXTURES {
            let path = root.join(name);
            let (summary, samples, high_water) =
                decode_test_file_at_with_compressed_high_water(&path, 0)
                    .unwrap_or_else(|error| panic!("{name}: {error:#}"));
            eprintln!("{name}: {} frames", summary.frames);
            // WMA encoders can pad or trim up to one 2048-sample frame.
            let tolerance = if name.ends_with(".wma") { 2048 } else { 0 };
            assert!(
                summary.frames.abs_diff(expected_frames) <= tolerance,
                "{name}: {} frames",
                summary.frames
            );
            assert_eq!(samples, summary.frames * 2, "{name}");
            assert!(high_water <= COMPRESSED_CAPACITY_BYTES as u64);
            let (resumed, samples, high_water) =
                decode_test_file_at_with_compressed_high_water(&path, 48_000).unwrap();
            assert_eq!(resumed.frames, summary.frames, "{name}");
            assert_eq!(samples, (summary.frames - 48_000) * 2, "{name}");
            assert!(high_water <= COMPRESSED_CAPACITY_BYTES as u64);
        }
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
    #[test]
    fn review_stream_duration_reconciles_rounding_but_rejects_conflicting_metadata() {
        assert_eq!(
            validated_seek_duration_ms(
                PlaybackSeekMechanism::JellyfinOriginalPcmWav,
                504_000,
                1,
                48_000,
                Some(10_000)
            ),
            Some(10_500)
        );
        assert_eq!(
            validated_seek_duration_ms(
                PlaybackSeekMechanism::JellyfinOriginalPcmWav,
                504_000,
                1,
                48_000,
                Some(11_000)
            ),
            Some(10_500)
        );
        for provider in [None, Some(0), Some(9_000), Some(12_000)] {
            assert_eq!(
                validated_seek_duration_ms(
                    PlaybackSeekMechanism::JellyfinOriginalPcmWav,
                    504_000,
                    1,
                    48_000,
                    provider
                ),
                None
            );
        }
        assert_eq!(
            validated_seek_duration_ms(
                PlaybackSeekMechanism::JellyfinOriginalPcmWav,
                0,
                1,
                48_000,
                Some(10_000)
            ),
            None
        );
        assert_eq!(
            validated_seek_duration_ms(
                PlaybackSeekMechanism::JellyfinOriginalPcmWav,
                504_000,
                1,
                0,
                Some(10_000)
            ),
            None
        );
        assert_eq!(
            validated_seek_duration_ms(
                PlaybackSeekMechanism::JellyfinOriginalMp3,
                0,
                1,
                48_000,
                Some(10_000)
            ),
            Some(10_000)
        );
        for mechanism in [
            PlaybackSeekMechanism::AudiobookshelfDirectMp3,
            PlaybackSeekMechanism::AudiobookshelfDirectM4a,
        ] {
            assert_eq!(
                validated_seek_duration_ms(mechanism, 0, 1, 48_000, Some(10_000)),
                None
            );
            assert_eq!(
                validated_seek_duration_ms(mechanism, 504_000, 1, 48_000, Some(12_000)),
                None
            );
            assert_eq!(
                validated_seek_duration_ms(mechanism, 504_000, 1, 48_000, Some(10_000)),
                Some(10_500)
            );
        }
    }

    #[test]
    fn review_decoded_duration_comes_from_pcm_stream_and_restored_end_restarts() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/generated-seek-pcm16.wav");
        for (provider, start_frame, media_seek, succeeds) in [
            (Some(2_000), 96_000, false, true),
            (Some(5_000), 48_000, true, false),
        ] {
            let cancel = Arc::new(AtomicBool::new(false));
            let reader = BoundedHttpReader::from_source(
                std::fs::File::open(&path).unwrap(),
                cancel.clone(),
                Arc::new(AtomicU64::new(0)),
            );
            let observed = Arc::new(AtomicU64::new(0));
            let pcm = Arc::new(ArrayQueue::new(220_000));
            let result = decode_stream_with_seek(
                reader,
                Some("stream.wav"),
                48_000,
                2,
                start_frame,
                Some(PlaybackSeekMechanism::JellyfinOriginalPcmWav),
                media_seek,
                provider,
                Some(observed.clone()),
                None,
                pcm.clone(),
                cancel,
            );
            assert_eq!(result.is_ok(), succeeds);
            if succeeds {
                assert_eq!(observed.load(Ordering::Acquire), 2_000);
                assert_eq!(
                    pcm.len(),
                    192_000,
                    "restored exact end must restart from the beginning"
                );
            } else {
                assert_eq!(observed.load(Ordering::Acquire), 0);
            }
        }
    }
    #[test]
    fn review_unsupported_wav_preserves_ordinary_terminal_resume() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/generated-seek-pcm-f32.wav");
        let cancel = Arc::new(AtomicBool::new(false));
        let reader = BoundedHttpReader::from_source(
            std::fs::File::open(path).unwrap(),
            cancel.clone(),
            Arc::new(AtomicU64::new(0)),
        );
        let observed = Arc::new(AtomicU64::new(0));
        let pcm = Arc::new(ArrayQueue::new(220_000));
        decode_stream_with_seek(
            reader,
            Some("stream.wav"),
            48_000,
            2,
            96_000,
            Some(PlaybackSeekMechanism::JellyfinOriginalPcmWav),
            false,
            Some(2_000),
            Some(observed.clone()),
            None,
            pcm.clone(),
            cancel,
        )
        .unwrap();
        assert_eq!(observed.load(Ordering::Acquire), 0);
        assert_eq!(pcm.len(), 192_000);
    }
}

#[cfg(test)]
#[path = "decoder_continuity_tests.rs"]
mod continuity_tests;
