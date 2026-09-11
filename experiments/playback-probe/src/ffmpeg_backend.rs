//! Contexts never leave this invocation; the sink receives only owned PCM samples.
use super::{ensure_format, Format};
use anyhow::{bail, Context, Result};
use ffmpeg_next::{self as ffmpeg, format::Sample, frame::Audio, ChannelLayout};
use serde_json::json;
use std::io::{Read, Seek, SeekFrom};

pub(super) fn info() -> serde_json::Value {
    json!({"decoder":"native-ffmpeg", "binding":"ffmpeg-next", "binding_version":"9.0.0",
        "libavcodec":{"version":ffmpeg::codec::version(), "configuration":ffmpeg::codec::configuration()},
        "libavformat":{"version":ffmpeg::format::version(), "configuration":ffmpeg::format::configuration()},
        "libavutil":{"version":ffmpeg::util::version(), "configuration":ffmpeg::util::configuration()},
        "conversion":"stateless sample representation conversion; original rate; no timeline trimming"})
}

// Native-endian byte reads avoid alignment assumptions and exclude frame padding.
fn sample(bytes: &[u8], format: Sample) -> Result<f32> {
    Ok(match format {
        Sample::U8(_) => (bytes[0] as f32 - 128.0) / 128.0,
        Sample::I16(_) => i16::from_ne_bytes(bytes.try_into()?) as f32 / 32768.0,
        Sample::I32(_) => (i32::from_ne_bytes(bytes.try_into()?) as f64 / 2147483648.0) as f32,
        Sample::I64(_) => {
            (i64::from_ne_bytes(bytes.try_into()?) as f64 / 9223372036854775808.0) as f32
        }
        Sample::F32(_) => f32::from_ne_bytes(bytes.try_into()?),
        Sample::F64(_) => f64::from_ne_bytes(bytes.try_into()?) as f32,
        Sample::None => bail!("unsupported FFmpeg sample format"),
    })
}
fn convert(frame: &Audio) -> Result<Vec<f32>> {
    // Untagged two-channel PCM WAV uses conventional left/right order. Explicit
    // alternative layouts remain rejected; this does not remap any channels.
    let layout = frame.channel_layout();
    if layout != ChannelLayout::STEREO && !(layout.is_empty() && layout.channels() == 2) {
        bail!("native FFmpeg requires standard left/right stereo");
    }
    let format = frame.format();
    let width = format.bytes();
    if width == 0 {
        bail!("unsupported FFmpeg sample format");
    }
    let planar = format.is_planar();
    let required_planes = if planar { 2 } else { 1 };
    if frame.planes() != required_planes {
        bail!("invalid decoded audio planes");
    }
    let count = frame
        .samples()
        .checked_mul(2)
        .context("sample count overflow")?;
    let mut pcm = Vec::with_capacity(count);
    for i in 0..count {
        let (plane, index) = if planar { (i % 2, i / 2) } else { (0, i) };
        let offset = index.checked_mul(width).context("sample offset overflow")?;
        let bytes = frame
            .data(plane)
            .get(offset..offset + width)
            .context("short decoded audio plane")?;
        let value = sample(bytes, format)?;
        if !value.is_finite() {
            bail!("nonfinite decoded audio sample");
        }
        pcm.push(value);
    }
    Ok(pcm)
}

// FFmpeg can shorten its duration estimate to the actual file size and report a
// clean EOF at a packet boundary. Validate declared RIFF/WAVE chunk bounds first.
// RF64 and other containers retain FFmpeg's own error/corruption detection.
fn validate_wave_bounds(path: &str) -> Result<()> {
    let mut file = std::fs::File::open(path)?;
    let length = file.metadata()?.len();
    if length < 12 {
        return Ok(());
    }
    let mut header = [0u8; 12];
    file.read_exact(&mut header)?;
    if &header[..4] != b"RIFF" || &header[8..] != b"WAVE" {
        return Ok(());
    }
    let end = u32::from_le_bytes(header[4..8].try_into()?) as u64 + 8;
    if end < 12 || end > length {
        bail!("invalid or truncated RIFF/WAVE length");
    }
    let mut offset = 12;
    while offset < end {
        if end - offset < 8 {
            bail!("truncated RIFF/WAVE chunk header");
        }
        file.seek(SeekFrom::Start(offset))?;
        let mut chunk = [0u8; 8];
        file.read_exact(&mut chunk)?;
        let size = u32::from_le_bytes(chunk[4..].try_into()?) as u64;
        offset += 8 + size + size % 2;
        if offset > end {
            bail!("truncated RIFF/WAVE chunk data");
        }
    }
    Ok(())
}

pub(super) fn decode(
    path: &str,
    mut sink: impl FnMut(Format, &[f32]) -> Result<()>,
) -> Result<(Format, u64)> {
    if !std::fs::metadata(path)?.is_file() {
        bail!("input must be a finite regular file: {path}");
    }
    validate_wave_bounds(path)?;
    ffmpeg::init()?;
    let mut options = ffmpeg::Dictionary::new();
    options.set("protocol_whitelist", "file");
    let mut input = ffmpeg::format::input_with_dictionary(&path, options)
        .with_context(|| format!("open native FFmpeg input {path}"))?;
    let stream = input
        .streams()
        .find(|stream| stream.parameters().medium() == ffmpeg::media::Type::Audio)
        .context("no audio stream")?;
    let index = stream.index();
    let context = ffmpeg::codec::context::Context::from_parameters(stream.parameters())?;
    let mut decoder = context.decoder().audio()?;
    let mut format = None;
    let mut frames = 0u64;
    let mut receive = |decoder: &mut ffmpeg::decoder::Audio, draining: bool| -> Result<()> {
        loop {
            let mut frame = Audio::empty();
            match decoder.receive_frame(&mut frame) {
                Ok(()) => {}
                Err(ffmpeg::Error::Eof) if draining => return Ok(()),
                Err(ffmpeg::Error::Other { errno })
                    if errno == ffmpeg::error::EAGAIN && !draining =>
                {
                    return Ok(())
                }
                Err(e) => return Err(e).context("receive decoded audio (including drain)"),
            }
            if frame.is_corrupt() {
                bail!("corrupt decoded audio frame");
            }
            let current = Format {
                rate: frame.rate(),
                channels: frame.channels() as usize,
            };
            ensure_format(&mut format, current)?;
            if frame.samples() == 0 {
                continue;
            }
            let pcm = convert(&frame)?;
            sink(current, &pcm)?;
            frames += frame.samples() as u64;
        }
    };
    loop {
        let mut packet = ffmpeg::Packet::empty();
        match packet.read(&mut input) {
            Ok(()) => {}
            Err(ffmpeg::Error::Eof) => break,
            Err(e) => return Err(e).context("read native FFmpeg packet"),
        }
        if packet.stream() != index {
            continue;
        }
        if packet.is_corrupt() {
            bail!("corrupt audio packet");
        }
        decoder.send_packet(&packet).context("send audio packet")?;
        receive(&mut decoder, false)?;
    }
    decoder.send_eof().context("start audio decoder drain")?;
    receive(&mut decoder, true)?;
    if frames == 0 {
        bail!("{path}: no decoded audio frames");
    }
    Ok((format.context("no audio format")?, frames))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ffmpeg::format::sample::Type::{Packed, Planar};
    #[test]
    fn packed_and_planar_stereo_are_interleaved_without_padding() {
        for layout in [Packed, Planar] {
            let mut frame = Audio::new(Sample::F32(layout), 2, ChannelLayout::STEREO);
            if layout == Packed {
                frame
                    .plane_mut::<f32>(0)
                    .copy_from_slice(&[0.25, -0.5, 0.75, -1.0]);
            } else {
                frame.plane_mut::<f32>(0).copy_from_slice(&[0.25, 0.75]);
                frame.plane_mut::<f32>(1).copy_from_slice(&[-0.5, -1.0]);
            }
            assert_eq!(convert(&frame).unwrap(), [0.25, -0.5, 0.75, -1.0]);
        }
    }
    #[test]
    fn rejects_nonfinite_and_nonstereo() {
        let mut frame = Audio::new(Sample::F32(Packed), 1, ChannelLayout::STEREO);
        frame.plane_mut::<f32>(0).copy_from_slice(&[f32::NAN, 0.0]);
        assert!(convert(&frame).is_err());
        assert!(convert(&Audio::new(Sample::F32(Packed), 1, ChannelLayout::MONO)).is_err());
    }
    #[test]
    fn accepts_untagged_stereo_but_rejects_explicit_downmix() {
        let mut frame = Audio::new(Sample::F32(Packed), 1, ChannelLayout::STEREO);
        frame.plane_mut::<f32>(0).copy_from_slice(&[0.25, -0.5]);
        let mut untagged = ChannelLayout::STEREO;
        untagged.0.order = ffmpeg::ffi::AVChannelOrder::AV_CHANNEL_ORDER_UNSPEC;
        frame.set_channel_layout(untagged);
        assert_eq!(convert(&frame).unwrap(), [0.25, -0.5]);
        frame.set_channel_layout(ChannelLayout::STEREO_DOWNMIX);
        assert!(convert(&frame).is_err());
    }
    #[test]
    fn integer_normalization_preserves_endpoints() {
        assert_eq!(
            sample(&i16::MIN.to_ne_bytes(), Sample::I16(Packed)).unwrap(),
            -1.0
        );
        assert_eq!(sample(&[128], Sample::U8(Packed)).unwrap(), 0.0);
        assert_eq!(
            sample(&i32::MIN.to_ne_bytes(), Sample::I32(Planar)).unwrap(),
            -1.0
        );
    }
}
