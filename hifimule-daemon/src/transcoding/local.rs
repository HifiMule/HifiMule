//! Native, file-to-file encoding. Each job owns its codec contexts and bounded
//! sample FIFO; no FFmpeg command-line process or whole-file PCM buffer is used.
use crate::providers::TranscodeProfile;
use anyhow::{Context, Result, anyhow, ensure};
use ff::packet::Mut;
use ffmpeg_next::{self as ff, ChannelLayout, codec, format, frame, media};
use std::io::{Read, Seek, SeekFrom, Write};
use std::{
    path::Path,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Debug, Clone)]
pub struct Target {
    pub encoder: &'static str,
    pub muxer: &'static str,
    pub extension: &'static str,
}

/// Normalize bitrate choices without converting observed source properties into
/// device ceilings. A codec may need a higher sample representation than its input.
pub fn normalized_for_source(
    profile: &TranscodeProfile,
    source: crate::domain::models::SourceAudioProperties,
) -> TranscodeProfile {
    let mut normalized = profile.normalized_for_source(source);
    normalized.max_sample_rate_hz = profile.max_sample_rate_hz;
    normalized.max_bit_depth = profile.max_bit_depth;
    normalized.max_channels = profile.max_channels;
    normalized
}

pub fn resolve(profile: &TranscodeProfile) -> Result<Target> {
    let container = profile
        .container
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let codec = profile
        .audio_codec
        .as_deref()
        .unwrap_or(match container.as_str() {
            "m4a" | "mp4" | "adts" => "aac",
            "ogg" | "oga" => "vorbis",
            "wav" | "wave" => "pcm_s16le",
            other => other,
        })
        .trim()
        .to_ascii_lowercase();
    let encoder = match codec.as_str() {
        "mp3" | "libmp3lame" => "libmp3lame",
        "aac" => "aac",
        "flac" => "flac",
        "alac" => "alac",
        "opus" | "libopus" => "libopus",
        "vorbis" | "libvorbis" => "libvorbis",
        "pcm_s16le" => "pcm_s16le",
        "pcm_s24le" => "pcm_s24le",
        "pcm_s32le" => "pcm_s32le",
        "pcm_f32le" => "pcm_f32le",
        _ => anyhow::bail!("Local encoding does not support audio codec '{codec}'"),
    };
    let container = if container.is_empty() {
        match encoder {
            "libmp3lame" => "mp3",
            "aac" | "alac" => "m4a",
            "flac" => "flac",
            "libopus" => "opus",
            "libvorbis" => "ogg",
            _ => "wav",
        }
    } else {
        container.as_str()
    };
    let (muxer, extension, compatible) = match container {
        "mp3" | "mpeg" => ("mp3", "mp3", encoder == "libmp3lame"),
        "m4a" | "ipod" => ("ipod", "m4a", matches!(encoder, "aac" | "alac")),
        "mp4" | "mov" => ("mp4", "mp4", matches!(encoder, "aac" | "alac")),
        "aac" | "adts" => ("adts", "aac", encoder == "aac"),
        "flac" => ("flac", "flac", encoder == "flac"),
        "opus" => ("opus", "opus", encoder == "libopus"),
        "ogg" | "oga" => (
            "ogg",
            "ogg",
            matches!(encoder, "libvorbis" | "libopus" | "flac"),
        ),
        "wav" | "wave" => ("wav", "wav", encoder.starts_with("pcm_")),
        _ => anyhow::bail!("Local encoding does not support container '{container}'"),
    };
    ensure!(
        compatible,
        "Local encoding cannot put codec '{codec}' in container '{container}'"
    );
    for (name, value) in [
        ("sample rate", profile.max_sample_rate_hz),
        ("bit depth", profile.max_bit_depth),
        ("channels", profile.max_channels),
        ("bitrate", profile.max_bitrate_kbps),
    ] {
        ensure!(
            value != Some(0),
            "Local encoding {name} ceiling must be positive"
        );
    }
    if encoder == "libopus" {
        ensure!(
            profile.max_sample_rate_hz.is_none_or(|rate| rate >= 48000),
            "Opus files decode at 48000 Hz; choose another codec for a lower sample-rate ceiling"
        );
    }
    ff::init()?;
    ensure!(
        ff::encoder::find_by_name(encoder).is_some(),
        "Local encoder '{encoder}' is unavailable; rebuild the controlled audio runtime"
    );
    let supported = ff::encoder::find_by_name(encoder).unwrap().audio()?;
    if let Some(ceiling) = profile.max_sample_rate_hz
        && let Some(rates) = supported.rates()
    {
        ensure!(
            rates
                .into_iter()
                .any(|rate| rate > 0 && rate as u32 <= ceiling),
            "No local encoder sample rate meets the profile ceiling"
        );
    }
    if matches!(encoder, "flac" | "alac") {
        ensure!(
            profile.max_bit_depth.is_none_or(|depth| depth >= 16),
            "Local lossless encoding requires a bit-depth ceiling of at least 16"
        );
    }
    if encoder.starts_with("pcm_") {
        ensure!(
            profile
                .max_bit_depth
                .is_none_or(|depth| profile.pcm_bit_depth().unwrap_or(32) <= depth),
            "Requested PCM codec exceeds profile bit-depth ceiling"
        );
    }
    let name = std::ffi::CString::new(muxer)?;
    ensure!(
        !unsafe { ff::ffi::av_guess_format(name.as_ptr(), std::ptr::null(), std::ptr::null()) }
            .is_null(),
        "Local muxer '{muxer}' is unavailable; rebuild the controlled audio runtime"
    );
    Ok(Target {
        encoder,
        muxer,
        extension,
    })
}

/// Reject constraints which the file encoder cannot faithfully interpret. HTTP
/// transport flags deliberately never become encoder options.
pub fn validate_constraints(profile: &serde_json::Value) -> Result<()> {
    for key in [
        "MaxAudioSampleRate",
        "MaxAudioChannels",
        "MaxAudioBitDepth",
        "MaxStreamingBitrate",
        "MusicStreamingTranscodingBitrate",
    ] {
        if let Some(value) = profile.get(key) {
            ensure!(
                value
                    .as_u64()
                    .is_some_and(|n| n > 0 && n <= u64::from(u32::MAX)),
                "Local profile {key} must be a positive integer"
            );
        }
    }
    if let Some(target) = profile["TranscodingProfiles"]
        .as_array()
        .and_then(|targets| {
            targets.iter().find(|t| {
                t["Type"]
                    .as_str()
                    .is_none_or(|v| v.eq_ignore_ascii_case("Audio"))
            })
        })
    {
        for key in [
            "AudioProfile",
            "AudioChannels",
            "AudioSampleRate",
            "MaxAudioChannels",
            "MaxAudioSampleRate",
            "MaxAudioBitDepth",
            "Bitrate",
            "Conditions",
        ] {
            if let Some(value) = target.get(key) {
                ensure!(
                    value.is_null()
                        || value.as_str() == Some("")
                        || value.as_array().is_some_and(Vec::is_empty),
                    "Local encoding cannot interpret TranscodingProfiles.{key}; use device-wide audio ceilings or server encoding"
                );
            }
        }
    }
    for codec in profile["CodecProfiles"].as_array().into_iter().flatten() {
        if codec["Conditions"]
            .as_array()
            .is_some_and(|v| !v.is_empty())
        {
            anyhow::bail!(
                "Local encoding does not support CodecProfiles conditions; use MaxAudioSampleRate, MaxAudioChannels and MaxAudioBitDepth ceilings or select server encoding"
            );
        }
    }
    Ok(())
}

pub fn validate_profile(profile: &serde_json::Value) -> Result<()> {
    validate_constraints(profile)?;
    let parsed = crate::sync::transcode_profile_from_device_profile(profile)
        .ok_or_else(|| anyhow!("Local encoding requires an audio TranscodingProfiles target"))?;
    resolve(&parsed)?;
    Ok(())
}

#[derive(Default)]
struct PoolState {
    active: usize,
    next: u64,
    sessions: std::collections::HashMap<u64, usize>,
}
#[derive(Default)]
struct Pool {
    state: Mutex<PoolState>,
    changed: tokio::sync::Notify,
}
fn pool() -> &'static Arc<Pool> {
    static POOL: OnceLock<Arc<Pool>> = OnceLock::new();
    POOL.get_or_init(|| Arc::new(Pool::default()))
}
/// All active runs share the strictest captured machine CPU budget. Changing
/// config.json cannot create an independent pool while older work is running.
pub struct Session {
    id: u64,
}
impl Session {
    pub fn new(workers: usize) -> Arc<Self> {
        let workers = crate::sync_performance::resolve_parallelism(workers);
        let mut state = pool().state.lock().unwrap();
        state.next += 1;
        let id = state.next;
        state.sessions.insert(id, workers);
        Arc::new(Self { id })
    }
    pub async fn acquire(&self, cancelled: &AtomicBool) -> Result<Permit> {
        loop {
            ensure!(
                !cancelled.load(Ordering::Relaxed),
                "Local encoding cancelled"
            );
            let changed = pool().changed.notified();
            {
                let mut state = pool().state.lock().unwrap();
                let limit = state.sessions.values().copied().min().unwrap_or(1);
                if state.active < limit {
                    state.active += 1;
                    return Ok(Permit);
                }
            }
            tokio::select! { _ = changed => {}, _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => {} }
        }
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        pool().state.lock().unwrap().sessions.remove(&self.id);
        pool().changed.notify_waiters();
    }
}
pub struct Permit;
impl Drop for Permit {
    fn drop(&mut self) {
        pool().state.lock().unwrap().active -= 1;
        pool().changed.notify_waiters();
    }
}

/// Enforces the reservation at the I/O boundary, including muxer seeks and trailers.
struct CappedFile {
    file: std::fs::File,
    position: u64,
    max: u64,
}
impl Write for CappedFile {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() as u64 > self.max.saturating_sub(self.position) {
            return Err(std::io::Error::other(
                "Local output exceeds reserved staging capacity",
            ));
        }
        let written = self.file.write(bytes)?;
        self.position += written as u64;
        Ok(written)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}
impl Seek for CappedFile {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        let original = self.position;
        let position = self.file.seek(position)?;
        if position > self.max {
            self.file.seek(SeekFrom::Start(original))?;
            return Err(std::io::Error::other(
                "Local output seek exceeds staging capacity",
            ));
        }
        self.position = position;
        Ok(position)
    }
}

// Cover packets are copied unchanged. Read only their PNG/JPEG dimensions so
// the reduced runtime need not enable image decoders merely to mux artwork.
fn picture_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") && bytes.len() >= 24 {
        return Some((
            u32::from_be_bytes(bytes[16..20].try_into().ok()?),
            u32::from_be_bytes(bytes[20..24].try_into().ok()?),
        ));
    }
    if !bytes.starts_with(&[0xff, 0xd8]) {
        return None;
    }
    let mut offset = 2;
    while offset + 4 <= bytes.len() {
        if bytes[offset] != 0xff {
            return None;
        }
        while offset < bytes.len() && bytes[offset] == 0xff {
            offset += 1;
        }
        let marker = *bytes.get(offset)?;
        offset += 1;
        if marker == 0xd9 || marker == 0xda {
            return None;
        }
        if marker == 1 || (0xd0..=0xd7).contains(&marker) {
            continue;
        }
        let size = u16::from_be_bytes(bytes.get(offset..offset + 2)?.try_into().ok()?) as usize;
        if size < 2 || offset.checked_add(size)? > bytes.len() {
            return None;
        }
        if matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) && size >= 7 {
            return Some((
                u16::from_be_bytes(bytes[offset + 5..offset + 7].try_into().ok()?) as u32,
                u16::from_be_bytes(bytes[offset + 3..offset + 5].try_into().ok()?) as u32,
            ));
        }
        offset += size;
    }
    None
}

/// Vorbis comments carry covers as a base64 FLAC picture block. The Ogg muxer
/// has no attached-picture stream support; encode only the comment, never pixels.
fn picture_comment(bytes: &[u8], description: &str) -> Result<String> {
    let mime = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(&[0xff, 0xd8]) {
        "image/jpeg"
    } else {
        "application/octet-stream"
    };
    let (width, height) = picture_dimensions(bytes).unwrap_or((0, 0));
    let mut block = Vec::with_capacity(bytes.len() + description.len() + mime.len() + 32);
    let mut integer = |value: u32| block.extend_from_slice(&value.to_be_bytes());
    integer(3);
    integer(mime.len() as u32);
    drop(integer);
    block.extend_from_slice(mime.as_bytes());
    block.extend_from_slice(&(description.len() as u32).to_be_bytes());
    block.extend_from_slice(description.as_bytes());
    for value in [width, height, 24, 0, bytes.len() as u32] {
        block.extend_from_slice(&value.to_be_bytes());
    }
    block.extend_from_slice(bytes);
    let capacity = (block.len() + 2) / 3 * 4 + 1;
    let mut encoded = vec![0u8; capacity];
    ensure!(
        capacity <= i32::MAX as usize && block.len() <= i32::MAX as usize,
        "Artwork comment is too large"
    );
    let result = unsafe {
        ff::ffi::av_base64_encode(
            encoded.as_mut_ptr().cast(),
            capacity as i32,
            block.as_ptr(),
            block.len() as i32,
        )
    };
    ensure!(!result.is_null(), "Cannot encode Ogg artwork comment");
    encoded.pop();
    Ok(String::from_utf8(encoded)?)
}

struct Fifo(*mut ff::ffi::AVAudioFifo);
impl Drop for Fifo {
    fn drop(&mut self) {
        unsafe { ff::ffi::av_audio_fifo_free(self.0) };
    }
}

fn drained(error: ff::Error) -> Result<()> {
    match error {
        ff::Error::Eof => Ok(()),
        ff::Error::Other { errno } if errno == ff::error::EAGAIN => Ok(()),
        other => Err(other.into()),
    }
}

const MAX_MP3_METADATA_TAIL: usize = 1_048_576;

/// Some MP3 demuxers emit trailing tags across audio packet boundaries. Check
/// the file's tags independently of those boundaries, and only tolerate a
/// rejected EOF packet wholly within that metadata. Never trim accepted audio.
fn is_mp3_metadata_tail(input: &Path, data: &[u8], position: isize, file_len: u64) -> bool {
    if data.is_empty()
        || data.len() > MAX_MP3_METADATA_TAIL
        || position < 0
        || (position as u64).checked_add(data.len() as u64) != Some(file_len)
    {
        return false;
    }
    // A packet starting with a plausible MPEG audio header may contain audio,
    // even when a metadata footer follows it. Preserve the decoding error.
    if data.len() >= 4
        && data[0] == 0xff
        && data[1] & 0xe0 == 0xe0
        && data[1] & 0x18 != 0x08
        && data[1] & 0x06 != 0
        && data[2] & 0xf0 != 0xf0
        && data[2] & 0x0c != 0x0c
    {
        return false;
    }
    // Read only on a decoding error. A bounded, separate file handle leaves the
    // demuxer's buffered input untouched. On an I/O error retain the decoder's
    // original failure instead of allowing an unverified packet through.
    let Ok(mut file) = std::fs::File::open(input) else {
        return false;
    };
    if file.metadata().map(|m| m.len()).ok() != Some(file_len) {
        return false;
    }
    let len = file_len.min(MAX_MP3_METADATA_TAIL as u64) as usize;
    let mut tail = vec![0; len];
    if file.seek(SeekFrom::End(-(len as i64))).is_err() || file.read_exact(&mut tail).is_err() {
        return false;
    }
    data.len() <= mp3_trailing_metadata_len(&tail)
}

/// Return the contiguous, recognizable metadata suffix. Tag sizes are used
/// solely to verify rejected packets, never to truncate the source or audio.
fn mp3_trailing_metadata_len(data: &[u8]) -> usize {
    let mut end = data.len();
    if end >= 128 && &data[end - 128..end - 125] == b"TAG" {
        end -= 128;
    }
    if end >= 15 && &data[end - 9..end] == b"LYRICS200" {
        let digits = &data[end - 15..end - 9];
        if !digits.iter().all(u8::is_ascii_digit) {
            return data.len() - end;
        }
        let size = digits
            .iter()
            .fold(0usize, |n, b| n * 10 + (b - b'0') as usize);
        if size < 11 || size > end - 15 {
            return data.len() - end;
        }
        let start = end - 15 - size;
        if &data[start..start + 11] != b"LYRICSBEGIN" {
            return data.len() - end;
        }
        end = start;
    }
    if end >= 32 && &data[end - 32..end - 24] == b"APETAGEX" {
        let footer = &data[end - 32..end];
        let field = |offset| u32::from_le_bytes(footer[offset..offset + 4].try_into().unwrap());
        let size = field(12) as usize;
        if matches!(field(8), 1000 | 2000)
            && (32..=MAX_MP3_METADATA_TAIL).contains(&size)
            && size <= end
            && field(16) <= 65_536
            && field(20) & !0x80000001 == 0 // Footer, optional header/read-only flags.
            && footer[24..].iter().all(|b| *b == 0)
        {
            end -= size;
        }
    }
    data.len() - end
}

/// The caller reserves original+output bytes before download. Abort rather than
/// exceed that output headroom; it cannot wait for bytes held by other originals.
pub fn encode(
    input: &Path,
    output: &Path,
    profile: &TranscodeProfile,
    cancelled: &AtomicBool,
    max_output: u64,
) -> Result<u64> {
    let result = encode_inner(input, output, profile, cancelled, max_output);
    if result.is_err() {
        let _ = std::fs::remove_file(output);
    }
    result.with_context(|| {
        format!(
            "Local {} encoding failed",
            profile.audio_codec.as_deref().unwrap_or("audio")
        )
    })
}

fn encode_inner(
    input: &Path,
    output: &Path,
    profile: &TranscodeProfile,
    cancelled: &AtomicBool,
    max_output: u64,
) -> Result<u64> {
    let target = resolve(profile)?;
    #[cfg(test)]
    test_support::wait_at_gate(input, cancelled)?;
    let check = || -> Result<()> {
        ensure!(
            !cancelled.load(Ordering::Relaxed),
            "Local encoding cancelled"
        );
        ensure!(
            std::fs::metadata(output).map(|m| m.len()).unwrap_or(0) <= max_output,
            "Local encoding exceeds reserved output capacity; increase sync_performance.staging_max_mib or check source duration"
        );
        Ok(())
    };
    check()?;
    let input_len = std::fs::metadata(input)
        .context("Cannot inspect original audio")?
        .len();
    let mut ictx = format::input(input).context("Cannot open original audio")?;
    let stream = ictx
        .streams()
        .best(media::Type::Audio)
        .ok_or_else(|| anyhow!("Original contains no audio stream"))?;
    let input_index = stream.index();
    let input_is_mp3 = stream.parameters().id() == codec::Id::MP3;
    let input_metadata = stream.metadata().to_owned();
    let mut context = codec::context::Context::from_parameters(stream.parameters())?;
    context.set_threading(codec::threading::Config {
        count: 1,
        ..Default::default()
    });
    let mut decoder = context
        .decoder()
        .audio()
        .context("Original audio decoder unavailable")?;
    ensure!(
        decoder.rate() > 0 && decoder.channels() > 0,
        "Original has invalid audio properties"
    );
    let source_duration = ictx.duration();
    let encoder_codec = ff::encoder::find_by_name(target.encoder).unwrap().audio()?;
    let desired_rate = if target.encoder == "libopus" {
        48000
    } else {
        decoder
            .rate()
            .min(profile.max_sample_rate_hz.unwrap_or(decoder.rate()))
    };
    let rate = match encoder_codec.rates() {
        Some(rates) => rates
            .filter(|r| *r > 0 && *r as u32 <= desired_rate)
            .max()
            .ok_or_else(|| anyhow!("No supported sample rate within {desired_rate} Hz ceiling"))?
            as u32,
        None => desired_rate,
    };
    let channels = u32::from(decoder.channels()).min(
        profile
            .max_channels
            .unwrap_or(u32::from(decoder.channels())),
    );
    let layout = match encoder_codec.channel_layouts() {
        Some(layouts) => layouts
            .filter(|l| l.channels() > 0 && l.channels() as u32 <= channels)
            .max_by_key(|l| l.channels())
            .ok_or_else(|| anyhow!("No encoder channel layout within {channels} channels"))?,
        None => ChannelLayout::default(channels as i32),
    };
    let depth = if matches!(target.encoder, "flac" | "alac") {
        let source_bits = unsafe {
            let context = decoder.as_ptr();
            (*context)
                .bits_per_raw_sample
                .max((*context).bits_per_coded_sample)
                .max(ff::ffi::av_get_bits_per_sample((*context).codec_id))
        };
        let source_bits = if source_bits > 0 {
            source_bits as u32
        } else {
            24
        };
        if profile.max_bit_depth.unwrap_or(24) >= 24 && source_bits > 16 {
            24
        } else {
            16
        }
    } else {
        profile.max_bit_depth.unwrap_or_else(|| {
            if target.encoder.starts_with("pcm_") {
                profile.pcm_bit_depth().unwrap_or(32)
            } else {
                24
            }
        })
    };
    let formats: Vec<_> = encoder_codec
        .formats()
        .ok_or_else(|| anyhow!("Encoder sample formats unavailable"))?
        .collect();
    let sample = if matches!(target.encoder, "flac" | "alac") {
        ensure!(
            depth >= 16,
            "Lossless encoder requires a bit-depth ceiling of at least 16"
        );
        formats
            .iter()
            .copied()
            .find(|f| f.bytes() * 8 == if depth < 24 { 16 } else { 32 })
            .or_else(|| formats.first().copied())
    } else {
        formats.first().copied()
    }
    .ok_or_else(|| anyhow!("No supported encoder sample format"))?;
    if target.encoder.starts_with("pcm_") {
        ensure!(
            profile.pcm_bit_depth().unwrap_or(32) <= depth,
            "Requested PCM codec exceeds bit-depth ceiling"
        );
    }
    let file = std::fs::File::create(output)?;
    let io = format::context::StreamIo::from_write_seek(CappedFile {
        file,
        position: 0,
        max: max_output,
    })?;
    let mut octx = format::output_to_stream(
        io,
        Some(output.to_string_lossy().as_ref()),
        Some(target.muxer),
    )?;
    let global = octx
        .format()
        .flags()
        .contains(format::flag::Flags::GLOBAL_HEADER);
    let mut context = codec::context::Context::new_with_codec(*encoder_codec);
    context.set_threading(codec::threading::Config {
        count: 1,
        ..Default::default()
    });
    let mut encoder = context.encoder().audio()?;
    encoder.set_rate(rate as i32);
    encoder.set_channel_layout(layout);
    encoder.set_format(sample);
    encoder.set_time_base((1, rate as i32));
    if global {
        encoder.set_flags(codec::flag::Flags::GLOBAL_HEADER);
    }
    if !profile.is_lossless() {
        encoder.set_bit_rate(profile.max_bitrate_kbps.unwrap_or(256) as usize * 1000);
    }
    // bits_per_raw_sample carries effective lossless precision even for s32 frames.
    if matches!(target.encoder, "flac" | "alac") {
        unsafe {
            (*encoder.as_mut_ptr()).bits_per_raw_sample = if depth < 24 { 16 } else { 24 };
        }
    }
    let mut encoder = encoder.open_as(encoder_codec)?;
    {
        let mut stream = octx.add_stream(encoder_codec)?;
        stream.set_time_base((1, rate as i32));
        stream.set_parameters(&encoder);
        stream.set_metadata(input_metadata.clone());
    }
    let mut artwork = Vec::new();
    let mut artwork_bytes = 0usize;
    let mut metadata = ictx.metadata().to_owned();
    for (key, value) in input_metadata.iter() {
        if metadata.get(key).is_none() {
            metadata.set(key, value);
        }
    }
    if matches!(
        target.muxer,
        "mp3" | "flac" | "ipod" | "mp4" | "ogg" | "opus"
    ) {
        for stream in ictx.streams() {
            if !stream
                .disposition()
                .contains(format::stream::Disposition::ATTACHED_PIC)
            {
                continue;
            }
            let mut packet = ff::Packet::empty();
            let copied = unsafe {
                ff::ffi::av_packet_ref(packet.as_mut_ptr(), &(*stream.as_ptr()).attached_pic)
            };
            ensure!(copied >= 0, "Cannot copy attached artwork");
            if let Some(bytes) = packet.data() {
                artwork_bytes = artwork_bytes.saturating_add(bytes.len());
                ensure!(
                    artwork_bytes <= 16 * 1024 * 1024,
                    "Attached artwork exceeds 16 MiB bound"
                );
                if matches!(target.muxer, "ogg" | "opus") {
                    let description = stream.metadata().get("title").unwrap_or("").to_string();
                    let comment = picture_comment(bytes, &description)?;
                    // Repeated picture keys are valid Vorbis comments; retain all covers.
                    let key = std::ffi::CString::new("METADATA_BLOCK_PICTURE")?;
                    let value = std::ffi::CString::new(comment)?;
                    unsafe {
                        let mut raw = metadata.disown();
                        let result = ff::ffi::av_dict_set(
                            &mut raw,
                            key.as_ptr(),
                            value.as_ptr(),
                            ff::ffi::AV_DICT_MULTIKEY,
                        );
                        metadata = ff::Dictionary::own(raw);
                        ensure!(result >= 0, "Cannot preserve Ogg artwork metadata");
                    }
                    continue;
                }
            }
            let mut out = octx.add_stream(ff::encoder::find(codec::Id::None))?;
            out.set_parameters(stream.parameters());
            out.set_metadata(stream.metadata().to_owned());
            unsafe {
                (*out.as_mut_ptr()).disposition = ff::ffi::AV_DISPOSITION_ATTACHED_PIC;
                (*out.parameters().as_mut_ptr()).codec_tag = 0;
            }
            if let Some((width, height)) = packet.data().and_then(picture_dimensions) {
                unsafe {
                    let mut parameters = out.parameters();
                    (*parameters.as_mut_ptr()).width = width as i32;
                    (*parameters.as_mut_ptr()).height = height as i32;
                }
            }
            packet.set_stream(out.index());
            artwork.push(packet);
        }
    }
    octx.set_metadata(metadata);
    octx.write_header()
        .context("Cannot write encoded audio header")?;
    for packet in &mut artwork {
        packet.write_interleaved(&mut octx)?;
    }
    let output_time_base = octx.stream(0).unwrap().time_base();
    let input_layout = if decoder.channel_layout().is_empty() {
        ChannelLayout::default(decoder.channels() as i32)
    } else {
        decoder.channel_layout()
    };
    let mut resampler = ff::software::resampling::Context::get(
        decoder.format(),
        input_layout,
        decoder.rate(),
        sample,
        layout,
        rate,
    )?;
    let fifo = Fifo(unsafe { ff::ffi::av_audio_fifo_alloc(sample.into(), layout.channels(), 1) });
    ensure!(!fifo.0.is_null(), "Cannot allocate audio FIFO");
    let frame_size = if encoder.frame_size() == 0 {
        1024
    } else {
        encoder.frame_size() as usize
    };
    let mut pts = 0i64;
    let mut samples = 0u64;
    let drain_packets = |encoder: &mut ff::encoder::audio::Encoder,
                         octx: &mut format::context::Output|
     -> Result<()> {
        let mut packet = ff::Packet::empty();
        loop {
            match encoder.receive_packet(&mut packet) {
                Ok(()) => {
                    check()?;
                    packet.set_stream(0);
                    packet.rescale_ts((1, rate as i32), output_time_base);
                    packet
                        .write_interleaved(octx)
                        .context("Cannot write encoded audio packet")?;
                }
                Err(error) => {
                    drained(error).context("Cannot receive encoded audio packet")?;
                    break;
                }
            }
        }
        Ok(())
    };
    let mut process_fifo = |encoder: &mut ff::encoder::audio::Encoder,
                            octx: &mut format::context::Output,
                            last: bool|
     -> Result<()> {
        loop {
            let count = unsafe { ff::ffi::av_audio_fifo_size(fifo.0) } as usize;
            if count == 0 || (!last && count < frame_size) {
                break;
            }
            check()?;
            let count = count.min(frame_size);
            let mut frame = frame::Audio::new(sample, count, layout);
            frame.set_rate(rate);
            frame.set_pts(Some(pts));
            let read = unsafe {
                ff::ffi::av_audio_fifo_read(
                    fifo.0,
                    (*frame.as_mut_ptr()).extended_data.cast(),
                    count as i32,
                )
            };
            ensure!(read == count as i32, "Incomplete audio FIFO read");
            pts += count as i64;
            samples += count as u64;
            encoder
                .send_frame(&frame)
                .context("Cannot encode converted audio frame")?;
            drain_packets(encoder, octx)?;
        }
        Ok(())
    };
    let mut queue_frame = |converted: &mut frame::Audio,
                           encoder: &mut ff::encoder::audio::Encoder,
                           octx: &mut format::context::Output|
     -> Result<()> {
        let count = converted.samples();
        if count == 0 {
            return Ok(());
        }
        ensure!(
            count <= 1_048_576,
            "Decoded audio frame exceeds bounded conversion capacity"
        );
        let written = unsafe {
            ff::ffi::av_audio_fifo_write(
                fifo.0,
                (*converted.as_mut_ptr()).extended_data.cast(),
                count as i32,
            )
        };
        ensure!(written == count as i32, "Cannot buffer converted audio");
        process_fifo(encoder, octx, false)
    };
    let mut receive = |decoder: &mut ff::decoder::Audio,
                       encoder: &mut ff::encoder::audio::Encoder,
                       octx: &mut format::context::Output|
     -> Result<()> {
        let mut decoded = frame::Audio::empty();
        loop {
            match decoder.receive_frame(&mut decoded) {
                Ok(()) => {
                    if decoded.channel_layout().is_empty() || decoded.channel_layout().bits() == 0 {
                        decoded.set_channel_layout(input_layout);
                    }
                    check()?;
                    ensure!(
                        decoded.samples() <= 1_048_576,
                        "Original audio frame is too large"
                    );
                    let capacity = ((decoded.samples() as u64 * rate as u64)
                        .div_ceil(decoder.rate() as u64)
                        + 256) as usize;
                    let mut converted = frame::Audio::new(sample, capacity, layout);
                    resampler
                        .run(&decoded, &mut converted)
                        .context("resampling decoded audio")?;
                    queue_frame(&mut converted, encoder, octx)?;
                }
                Err(error) => {
                    drained(error).context("Cannot decode original audio frame")?;
                    break;
                }
            }
        }
        Ok(())
    };
    loop {
        check()?;
        // av_read_frame requires an empty packet; its previous buffer must be
        // released even for skipped streams and tolerated trailing metadata.
        let mut packet = ff::Packet::empty();
        match packet.read(&mut ictx) {
            Ok(()) => {
                #[cfg(test)]
                test_support::observe_input_packet(&packet);
                if packet.stream() == input_index {
                    if let Err(error) = decoder.send_packet(&packet) {
                        if error == ff::Error::InvalidData
                            && input_is_mp3
                            && is_mp3_metadata_tail(
                                input,
                                packet.data().unwrap_or_default(),
                                packet.position(),
                                input_len,
                            )
                        {
                            crate::daemon_log!(
                                "[Sync] Ignoring rejected trailing MP3 metadata packet at byte {} ({} bytes)",
                                packet.position(),
                                packet.size()
                            );
                            continue;
                        }
                        return Err(error).with_context(|| {
                            format!(
                                "Cannot decode original audio packet at byte {} ({} bytes)",
                                packet.position(),
                                packet.size()
                            )
                        });
                    }
                    receive(&mut decoder, &mut encoder, &mut octx)?;
                }
            }
            Err(ff::Error::Eof) => break,
            Err(error) => return Err(error).context("Cannot read original audio packet"),
        }
    }
    decoder
        .send_eof()
        .context("Cannot finish decoding original audio")?;
    receive(&mut decoder, &mut encoder, &mut octx)?;
    drop(receive);
    loop {
        let mut converted = frame::Audio::new(sample, 4096, layout);
        check()?;
        let delay = resampler
            .flush(&mut converted)
            .context("flushing resampled audio")?;
        queue_frame(&mut converted, &mut encoder, &mut octx)?;
        if delay.is_none() || converted.samples() == 0 {
            break;
        }
    }
    drop(queue_frame);
    process_fifo(&mut encoder, &mut octx, true)?;
    drop(process_fifo);
    encoder
        .send_eof()
        .context("Cannot finish encoding converted audio")?;
    drain_packets(&mut encoder, &mut octx)?;
    ensure!(samples > 0, "Original audio decoded no samples");
    octx.write_trailer()
        .context("Cannot finish encoded audio container")?;
    drop(octx);
    check()?;
    // Reopen the completed file before it may enter the serialized device writer.
    let verify = format::input(output).context("Cannot reopen encoded output")?;
    let stream = verify
        .streams()
        .best(media::Type::Audio)
        .ok_or_else(|| anyhow!("Encoded output has no audio"))?;
    let expected_demuxer = match target.muxer {
        "ipod" | "mp4" => "mov",
        "opus" => "ogg",
        "adts" => "aac",
        other => other,
    };
    ensure!(
        verify
            .format()
            .name()
            .split(',')
            .any(|name| name == expected_demuxer),
        "Encoded output container does not match profile"
    );
    ensure!(
        stream.parameters().id() == encoder_codec.id(),
        "Encoded output codec does not match profile"
    );
    let actual = codec::context::Context::from_parameters(stream.parameters())?
        .decoder()
        .audio()?;
    ensure!(
        actual.rate() <= profile.max_sample_rate_hz.unwrap_or(u32::MAX),
        "Encoded sample rate exceeds profile"
    );
    ensure!(
        u32::from(actual.channels()) <= profile.max_channels.unwrap_or(u32::MAX),
        "Encoded channels exceed profile"
    );
    if profile.is_lossless() {
        let parameters = stream.parameters();
        let bits = unsafe {
            let p = parameters.as_ptr();
            (*p).bits_per_raw_sample
                .max((*p).bits_per_coded_sample)
                .max(ff::ffi::av_get_bits_per_sample((*p).codec_id))
        };
        ensure!(
            bits > 0 && bits as u32 <= profile.max_bit_depth.unwrap_or(u32::MAX),
            "Encoded precision exceeds profile bit-depth ceiling"
        );
    } else if let Some(bitrate) = profile.max_bitrate_kbps {
        ensure!(
            actual.bit_rate() == 0 || actual.bit_rate() as u64 <= u64::from(bitrate) * 1100,
            "Encoded audio bitrate exceeds profile ceiling"
        );
    }
    if source_duration > 0 && verify.duration() > 0 {
        ensure!(
            (source_duration - verify.duration()).abs() <= 2_000_000,
            "Encoded duration does not match original"
        );
    }
    Ok(std::fs::metadata(output)?.len())
}

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
#[path = "local_memory_benchmark.rs"]
mod memory_benchmark;

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;

    pub fn wav(path: &Path, seconds: u32) {
        let samples = 48000 * seconds;
        let mut file = std::fs::File::create(path).unwrap();
        file.write_all(b"RIFF").unwrap();
        file.write_all(&(36 + samples * 4).to_le_bytes()).unwrap();
        file.write_all(b"WAVEfmt ").unwrap();
        file.write_all(&16u32.to_le_bytes()).unwrap();
        file.write_all(&1u16.to_le_bytes()).unwrap();
        file.write_all(&2u16.to_le_bytes()).unwrap();
        file.write_all(&48000u32.to_le_bytes()).unwrap();
        file.write_all(&192000u32.to_le_bytes()).unwrap();
        file.write_all(&4u16.to_le_bytes()).unwrap();
        file.write_all(&16u16.to_le_bytes()).unwrap();
        file.write_all(b"data").unwrap();
        file.write_all(&(samples * 4).to_le_bytes()).unwrap();
        for i in 0..samples {
            let sample =
                ((i as f32 * 440.0 * std::f32::consts::TAU / 48000.0).sin() * 12000.0) as i16;
            file.write_all(&sample.to_le_bytes()).unwrap();
            file.write_all(&sample.to_le_bytes()).unwrap();
        }
    }

    #[test]
    fn mp3_with_stacked_trailing_tags_encodes_without_losing_audio() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.wav");
        let clean = temp.path().join("clean.mp3");
        let tagged = temp.path().join("tagged.mp3");
        let reference = temp.path().join("reference.wav");
        let output = temp.path().join("output.wav");
        wav(&source, 1);
        let mp3 = TranscodeProfile {
            container: Some("mp3".into()),
            audio_codec: Some("mp3".into()),
            max_bitrate_kbps: Some(192),
            ..Default::default()
        };
        let pcm = TranscodeProfile {
            container: Some("wav".into()),
            audio_codec: Some("pcm_s16le".into()),
            ..Default::default()
        };
        let cancelled = AtomicBool::new(false);
        encode(&source, &clean, &mp3, &cancelled, 1_000_000).unwrap();
        encode(&clean, &reference, &pcm, &cancelled, 1_000_000).unwrap();
        let mut bytes = std::fs::read(&clean).unwrap();
        bytes.extend_from_slice(&stacked_mp3_tags());
        std::fs::write(&tagged, bytes).unwrap();
        let (result, packets) = test_support::capture_input_packets(|| {
            encode(&tagged, &output, &pcm, &cancelled, 1_000_000)
        });
        result.unwrap_or_else(|e| panic!("{e:#}"));
        packets.assert_released();
        // Compare decoded PCM, including its length, rather than only accepting
        // an output file: ignoring the trailer must preserve all audio samples.
        let pcm_bytes = |path: &Path| {
            let bytes = std::fs::read(path).unwrap();
            let start = bytes.windows(4).position(|b| b == b"data").unwrap();
            bytes[start + 8..].to_vec()
        };
        assert_eq!(pcm_bytes(&output), pcm_bytes(&reference));
    }

    #[test]
    fn input_packet_buffers_are_released_after_conversion_and_failure() {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("source.wav");
        wav(&input, 3);
        let profile = TranscodeProfile {
            container: Some("wav".into()),
            audio_codec: Some("pcm_s16le".into()),
            ..Default::default()
        };
        for (capacity, succeeds) in [(1_000_000, true), (64_000, false)] {
            let output = temp.path().join(format!("output-{capacity}.wav"));
            let (result, packets) = test_support::capture_input_packets(|| {
                encode(&input, &output, &profile, &AtomicBool::new(false), capacity)
            });
            assert_eq!(result.is_ok(), succeeds, "{result:?}");
            packets.assert_released();
            assert_eq!(output.exists(), succeeds);
        }
    }

    fn stacked_mp3_tags() -> Vec<u8> {
        // Regression layout: a partial APE item, its footer, Lyrics3v2, ID3v1.
        // The APE size extends into the previous packet; never use it to trim
        // accepted audio packets or alter the source file.
        let mut bytes = b"\0Year\x002005\x04\0\0\0\0\0\0\0Track\x005/15".to_vec();
        bytes.extend_from_slice(b"APETAGEX");
        for value in [2000u32, 205, 7, 0x80000000, 0, 0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(b"LYRICSBEGININD0000210LYR00001 000030LYRICS200");
        bytes.extend_from_slice(b"TAG");
        bytes.extend_from_slice(&[0; 125]);
        assert_eq!(bytes.len(), 233);
        bytes
    }

    #[test]
    fn mp3_metadata_exception_requires_a_bounded_recognized_eof_packet() {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("metadata-tail.mp3");
        let tags = stacked_mp3_tags();
        let at_eof = |data: &[u8]| {
            let mut bytes = vec![0; 1000];
            bytes.extend_from_slice(data);
            std::fs::write(&input, &bytes).unwrap();
            is_mp3_metadata_tail(&input, data, 1000, bytes.len() as u64)
        };
        assert!(at_eof(&tags));
        assert!(!is_mp3_metadata_tail(&input, &tags, 1000, 1234));
        assert!(!is_mp3_metadata_tail(&input, &tags, -1, tags.len() as u64));
        assert!(!at_eof(&[]));
        assert!(!at_eof(b"arbitrary damaged audio"));
        assert!(!at_eof(&vec![0; 1_048_577]));
        assert!(at_eof(&tags[105..])); // Standalone ID3v1.
        assert!(at_eof(&tags[60..])); // Lyrics3v2 followed by ID3v1.
        for offset in [28, 36, 40, 47, 48, 52, 60, 90] {
            let mut bad = tags.clone();
            bad[offset] ^= 0xff;
            assert!(!at_eof(&bad), "invalid tag field at {offset}");
        }
        let mut short_ape = tags.clone();
        short_ape[40..44].copy_from_slice(&32u32.to_le_bytes());
        assert!(!at_eof(&short_ape)); // Declared APE size cannot cover this packet.
        let mut with_audio = tags.clone();
        with_audio[..4].copy_from_slice(&[0xff, 0xfb, 0x90, 0]);
        assert!(!at_eof(&with_audio));
        let mut untagged_prefix = b"damaged audio".to_vec();
        untagged_prefix.extend_from_slice(&tags[60..]);
        assert!(!at_eof(&untagged_prefix));
    }

    #[test]
    fn mp3_tags_split_across_packets_are_recognized_from_the_file_tail() {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("split-tags.mp3");
        let tags = stacked_mp3_tags();
        let mut bytes = vec![0; 1000];
        bytes.extend_from_slice(&tags);
        std::fs::write(&input, &bytes).unwrap();
        let file_len = bytes.len() as u64;
        for offset in 0..tags.len() {
            assert!(
                is_mp3_metadata_tail(&input, &tags[offset..], (1000 + offset) as isize, file_len),
                "tag split at {offset}"
            );
        }
        // The reported case: only the last 52 bytes of ID3v1 are in the packet.
        assert!(is_mp3_metadata_tail(&input, &tags[181..], 1181, file_len));
        assert!(!is_mp3_metadata_tail(&input, &bytes[800..], 800, file_len));
        assert!(!is_mp3_metadata_tail(&input, &tags[181..], 1180, file_len));
        std::fs::remove_file(&input).unwrap();
        assert!(!is_mp3_metadata_tail(&input, &tags[181..], 1181, file_len));
    }

    #[test]
    fn unrecognized_mp3_corruption_still_fails_with_packet_context() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.wav");
        let damaged = temp.path().join("damaged.mp3");
        let output = temp.path().join("output.mp3");
        wav(&source, 1);
        let profile = TranscodeProfile {
            container: Some("mp3".into()),
            audio_codec: Some("mp3".into()),
            max_bitrate_kbps: Some(192),
            ..Default::default()
        };
        let cancelled = AtomicBool::new(false);
        encode(&source, &damaged, &profile, &cancelled, 1_000_000).unwrap();
        let mut bytes = std::fs::read(&damaged).unwrap();
        bytes.extend_from_slice(&[0x55; 233]);
        std::fs::write(&damaged, bytes).unwrap();
        let error = encode(&damaged, &output, &profile, &cancelled, 1_000_000).unwrap_err();
        assert!(format!("{error:#}").contains("Cannot decode original audio packet at byte"));
        assert!(!output.exists());
    }

    #[test]
    fn real_encoding_all_required_targets_reopens_and_decodes() {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("source.wav");
        wav(&input, 3);
        for (container, codec) in [
            ("mp3", "mp3"),
            ("m4a", "aac"),
            ("aac", "aac"),
            ("flac", "flac"),
            ("m4a", "alac"),
            ("wav", "pcm_s16le"),
            ("wav", "pcm_s24le"),
            ("wav", "pcm_s32le"),
            ("wav", "pcm_f32le"),
            ("ogg", "vorbis"),
            ("opus", "opus"),
        ] {
            let profile = TranscodeProfile {
                container: Some(container.into()),
                audio_codec: Some(codec.into()),
                max_bitrate_kbps: Some(192),
                max_channels: Some(1),
                max_sample_rate_hz: Some(48000),
                max_bit_depth: Some(if codec.contains("32") { 32 } else { 24 }),
            };
            let target = resolve(&profile).unwrap();
            let output = temp
                .path()
                .join(format!("output-{codec}-{}.{}", container, target.extension));
            let size = encode(
                &input,
                &output,
                &profile,
                &AtomicBool::new(false),
                8 * 1024 * 1024,
            )
            .unwrap_or_else(|e| panic!("{codec}/{container}: {e:#}"));
            assert!(size > 0);
            let mut context = format::input(&output).unwrap();
            let stream = context.streams().best(media::Type::Audio).unwrap();
            let index = stream.index();
            let mut decoder = codec::context::Context::from_parameters(stream.parameters())
                .unwrap()
                .decoder()
                .audio()
                .unwrap();
            assert_eq!(decoder.channels(), 1);
            assert!(decoder.rate() <= 48000);
            let mut samples = 0;
            let mut frame = frame::Audio::empty();
            for (stream, packet) in context.packets() {
                if stream.index() == index {
                    decoder.send_packet(&packet).unwrap();
                    while decoder.receive_frame(&mut frame).is_ok() {
                        samples += frame.samples();
                    }
                }
            }
            decoder.send_eof().unwrap();
            while decoder.receive_frame(&mut frame).is_ok() {
                samples += frame.samples();
            }
            assert!(
                (samples as i64 - 144000).abs() < 5000,
                "{codec}: decoded samples {samples}"
            );
        }
    }

    #[test]
    fn source_precision_and_rate_do_not_become_device_ceiling() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("source-44100.wav");
        wav(&input, 2);
        let mut bytes = std::fs::read(&input).unwrap();
        bytes[24..28].copy_from_slice(&44100u32.to_le_bytes());
        bytes[28..32].copy_from_slice(&176400u32.to_le_bytes());
        std::fs::write(&input, bytes).unwrap();
        let source = crate::domain::models::SourceAudioProperties {
            sample_rate_hz: Some(44100),
            bit_depth: Some(16),
            channels: Some(2),
            ..Default::default()
        };
        for (container, codec) in [
            ("opus", "opus"),
            ("wav", "pcm_s24le"),
            ("wav", "pcm_s32le"),
            ("wav", "pcm_f32le"),
        ] {
            let profile = TranscodeProfile {
                container: Some(container.into()),
                audio_codec: Some(codec.into()),
                max_sample_rate_hz: Some(48000),
                ..Default::default()
            };
            let normalized = normalized_for_source(&profile, source);
            assert_eq!(normalized.max_sample_rate_hz, Some(48000));
            assert_eq!(normalized.max_bit_depth, None);
            let target = resolve(&normalized).unwrap();
            let output = dir
                .path()
                .join(format!("output-{codec}.{}", target.extension));
            encode(
                &input,
                &output,
                &normalized,
                &AtomicBool::new(false),
                8 * 1024 * 1024,
            )
            .unwrap_or_else(|e| panic!("{codec}: {e:#}"));
            let context = format::input(&output).unwrap();
            let stream = context.streams().best(media::Type::Audio).unwrap();
            let decoder = codec::context::Context::from_parameters(stream.parameters())
                .unwrap()
                .decoder()
                .audio()
                .unwrap();
            if codec == "opus" {
                assert_eq!(decoder.rate(), 48000);
            } else {
                assert_eq!(
                    stream.parameters().id(),
                    ff::encoder::find_by_name(target.encoder).unwrap().id()
                );
            }
            let mut restricted = profile.clone();
            restricted.max_bit_depth = Some(16);
            if codec.starts_with("pcm_") {
                assert!(resolve(&normalized_for_source(&restricted, source)).is_err());
            }
        }
        for codec in ["flac", "alac"] {
            let container = if codec == "flac" { "flac" } else { "m4a" };
            let output = dir.path().join(format!("precision-{codec}.{container}"));
            encode(
                &input,
                &output,
                &TranscodeProfile {
                    container: Some(container.into()),
                    audio_codec: Some(codec.into()),
                    max_bit_depth: Some(24),
                    ..Default::default()
                },
                &AtomicBool::new(false),
                8 * 1024 * 1024,
            )
            .unwrap();
            let context = format::input(&output).unwrap();
            let parameters = context
                .streams()
                .best(media::Type::Audio)
                .unwrap()
                .parameters();
            assert_eq!(unsafe { (*parameters.as_ptr()).bits_per_raw_sample }, 16);
        }
        let profile = TranscodeProfile {
            container: Some("opus".into()),
            audio_codec: Some("opus".into()),
            ..Default::default()
        };
        assert_eq!(
            normalized_for_source(&profile, source).max_sample_rate_hz,
            None
        );
        assert!(
            resolve(&TranscodeProfile {
                max_sample_rate_hz: Some(44100),
                ..profile
            })
            .is_err()
        );
    }

    #[test]
    fn preserves_metadata_and_supported_attached_artwork() {
        use id3::TagLike;
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("source.wav");
        wav(&input, 3);
        let mp3 = dir.path().join("tagged.mp3");
        let source_profile = TranscodeProfile {
            container: Some("mp3".into()),
            audio_codec: Some("mp3".into()),
            max_bitrate_kbps: Some(192),
            ..Default::default()
        };
        encode(
            &input,
            &mp3,
            &source_profile,
            &AtomicBool::new(false),
            4 * 1024 * 1024,
        )
        .unwrap();
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(2, 2)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let mut tags = id3::Tag::new();
        tags.set_title("Native metadata fixture");
        tags.set_artist("HifiMule");
        tags.add_frame(id3::frame::Picture {
            mime_type: "image/png".into(),
            picture_type: id3::frame::PictureType::CoverFront,
            description: "Cover".into(),
            data: png.into_inner(),
        });
        tags.write_to_path(&mp3, id3::Version::Id3v24).unwrap();
        for (container, codec) in [
            ("mp3", "mp3"),
            ("flac", "flac"),
            ("m4a", "aac"),
            ("m4a", "alac"),
            ("ogg", "vorbis"),
            ("opus", "opus"),
        ] {
            let output = dir.path().join(format!("tagged-{codec}.{container}"));
            let profile = TranscodeProfile {
                container: Some(container.into()),
                audio_codec: Some(codec.into()),
                max_bitrate_kbps: Some(192),
                ..Default::default()
            };
            encode(
                &mp3,
                &output,
                &profile,
                &AtomicBool::new(false),
                8 * 1024 * 1024,
            )
            .unwrap_or_else(|e| panic!("{codec}: {e:#}"));
            let context = format::input(&output).unwrap();
            assert_eq!(
                context
                    .metadata()
                    .get("title")
                    .map(str::to_string)
                    .or_else(|| context
                        .streams()
                        .best(media::Type::Audio)
                        .and_then(|s| s.metadata().get("title").map(str::to_string)))
                    .as_deref(),
                Some("Native metadata fixture"),
                "{codec}"
            );
            assert!(
                context.streams().any(|s| s
                    .disposition()
                    .contains(format::stream::Disposition::ATTACHED_PIC)),
                "{codec}: missing artwork"
            );
            if matches!(codec, "vorbis" | "opus") {
                let back = dir.path().join(format!("from-{codec}.flac"));
                encode(
                    &output,
                    &back,
                    &TranscodeProfile {
                        container: Some("flac".into()),
                        audio_codec: Some("flac".into()),
                        ..Default::default()
                    },
                    &AtomicBool::new(false),
                    8 * 1024 * 1024,
                )
                .unwrap();
                let restored = format::input(&back).unwrap();
                assert!(restored.streams().any(|s| {
                    s.disposition()
                        .contains(format::stream::Disposition::ATTACHED_PIC)
                }));
                assert_eq!(
                    restored.metadata().get("title"),
                    Some("Native metadata fixture")
                );
            }
        }
    }

    #[test]
    fn high_resolution_source_resamples_and_reduces_lossless_precision() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("high.wav");
        let mut file = std::fs::File::create(&input).unwrap();
        let count = 96000u32;
        file.write_all(b"RIFF").unwrap();
        file.write_all(&(36 + count * 6).to_le_bytes()).unwrap();
        file.write_all(b"WAVEfmt ").unwrap();
        file.write_all(&16u32.to_le_bytes()).unwrap();
        file.write_all(&1u16.to_le_bytes()).unwrap();
        file.write_all(&2u16.to_le_bytes()).unwrap();
        file.write_all(&96000u32.to_le_bytes()).unwrap();
        file.write_all(&576000u32.to_le_bytes()).unwrap();
        file.write_all(&6u16.to_le_bytes()).unwrap();
        file.write_all(&24u16.to_le_bytes()).unwrap();
        file.write_all(b"data").unwrap();
        file.write_all(&(count * 6).to_le_bytes()).unwrap();
        for i in 0..count {
            let value =
                ((i as f64 * 440.0 * std::f64::consts::TAU / 96000.0).sin() * 1_000_000.0) as i32;
            for _ in 0..2 {
                file.write_all(&value.to_le_bytes()[..3]).unwrap();
            }
        }
        drop(file);
        let output = dir.path().join("limited.flac");
        let profile = TranscodeProfile {
            container: Some("flac".into()),
            audio_codec: Some("flac".into()),
            max_sample_rate_hz: Some(44100),
            max_channels: Some(1),
            max_bit_depth: Some(16),
            ..Default::default()
        };
        encode(
            &input,
            &output,
            &profile,
            &AtomicBool::new(false),
            4 * 1024 * 1024,
        )
        .unwrap();
        let context = format::input(&output).unwrap();
        let stream = context.streams().best(media::Type::Audio).unwrap();
        let parameters = stream.parameters();
        assert_eq!(unsafe { (*parameters.as_ptr()).bits_per_raw_sample }, 16);
        let decoder = codec::context::Context::from_parameters(parameters)
            .unwrap()
            .decoder()
            .audio()
            .unwrap();
        assert_eq!(decoder.rate(), 44100);
        assert_eq!(decoder.channels(), 1);
    }

    #[test]
    fn cancellation_failure_and_capacity_leave_no_partial_output() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("source.wav");
        wav(&input, 2);
        let profile = TranscodeProfile {
            container: Some("flac".into()),
            audio_codec: Some("flac".into()),
            ..Default::default()
        };
        let output = dir.path().join("output.flac");
        assert!(encode(&input, &output, &profile, &AtomicBool::new(true), 1_000_000).is_err());
        assert!(!output.exists());
        assert!(encode(&input, &output, &profile, &AtomicBool::new(false), 10).is_err());
        assert!(!output.exists());
        std::fs::write(&input, b"invalid audio").unwrap();
        assert!(
            encode(
                &input,
                &output,
                &profile,
                &AtomicBool::new(false),
                1_000_000
            )
            .is_err()
        );
        assert!(!output.exists());
    }

    #[test]
    fn rejects_unsupported_and_incompatible_targets_and_constraints() {
        for (container, codec) in [
            ("wav", "aac"),
            ("mp3", "flac"),
            ("unknown", "aac"),
            ("m4a", "unknown"),
        ] {
            assert!(
                resolve(&TranscodeProfile {
                    container: Some(container.into()),
                    audio_codec: Some(codec.into()),
                    ..Default::default()
                })
                .is_err()
            );
        }
        assert!(validate_profile(&serde_json::json!({"TranscodingProfiles":[{"Container":"mp3","AudioCodec":"mp3"}],"CodecProfiles":[{"Conditions":[{"Property":"AudioProfile"}]}]})).is_err());
    }

    #[tokio::test]
    async fn shared_pool_limits_across_sessions_and_releases_on_drop() {
        let _serial_pool = test_support::lock_pool().await;
        let session = Session::new(1);
        let second = Session::new(2);
        let cancel = AtomicBool::new(false);
        let permit = session.acquire(&cancel).await.unwrap();
        assert!(
            tokio::time::timeout(
                std::time::Duration::from_millis(100),
                second.acquire(&cancel)
            )
            .await
            .is_err()
        );
        drop(permit);
        let permit =
            tokio::time::timeout(std::time::Duration::from_secs(1), second.acquire(&cancel))
                .await
                .unwrap()
                .unwrap();
        drop(permit);
        cancel.store(true, Ordering::Relaxed);
        assert!(session.acquire(&cancel).await.is_err());
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use ff::packet::Ref;
    use std::cell::RefCell;

    // Retain a bounded set of native references so tests can inspect ownership
    // after all codec/demux contexts have dropped. Packet::clone() would copy
    // writable data and hide a leaked reference to the original allocation.
    #[derive(Default)]
    pub struct InputPacketBuffers(Vec<*mut ff::ffi::AVBufferRef>);

    impl InputPacketBuffers {
        pub fn assert_released(&self) {
            assert!(
                self.0.len() > 1,
                "fixture must exercise multiple packet buffers"
            );
            for reference in &self.0 {
                assert_eq!(
                    unsafe { ff::ffi::av_buffer_get_ref_count(*reference) },
                    1,
                    "input packet buffer retained after conversion returned"
                );
            }
        }
    }

    impl Drop for InputPacketBuffers {
        fn drop(&mut self) {
            for reference in &mut self.0 {
                unsafe { ff::ffi::av_buffer_unref(reference) };
            }
        }
    }

    thread_local! {
        static INPUT_PACKETS: RefCell<Option<InputPacketBuffers>> = const { RefCell::new(None) };
    }

    pub fn capture_input_packets<T>(work: impl FnOnce() -> T) -> (T, InputPacketBuffers) {
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                INPUT_PACKETS.with(|slot| slot.borrow_mut().take());
            }
        }
        INPUT_PACKETS.with(|slot| {
            let mut slot = slot.borrow_mut();
            assert!(slot.is_none(), "packet capture must not be nested");
            *slot = Some(InputPacketBuffers::default());
        });
        let _reset = Reset;
        let result = work();
        let packets = INPUT_PACKETS.with(|slot| slot.borrow_mut().take().unwrap());
        (result, packets)
    }

    pub fn observe_input_packet(packet: &ff::Packet) {
        INPUT_PACKETS.with(|slot| {
            let mut slot = slot.borrow_mut();
            let Some(packets) = slot.as_mut() else { return };
            let reference = unsafe { (*packet.as_ptr()).buf };
            if reference.is_null() {
                return;
            }
            // Several packets may share one allocation. Keep only one probe
            // per backing buffer so its expected post-conversion refcount is 1.
            if packets
                .0
                .iter()
                .any(|previous| unsafe { (**previous).buffer == (*reference).buffer })
            {
                return;
            }
            let retained = unsafe { ff::ffi::av_buffer_ref(reference) };
            assert!(!retained.is_null(), "cannot retain packet buffer probe");
            if packets.0.len() == 32 {
                // Keep early allocations plus the latest one, so the bounded
                // probe also checks trailing metadata after many audio packets.
                unsafe { ff::ffi::av_buffer_unref(&mut packets.0[31]) };
                packets.0[31] = retained;
            } else {
                packets.0.push(retained);
            }
        });
    }

    // Gate tests must not wait on two permits while a separate one-permit
    // pool test waits on the gated worker. Production scheduling has no gates.
    pub async fn lock_pool() -> tokio::sync::MutexGuard<'static, ()> {
        static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
        LOCK.lock().await
    }
    pub struct Gate {
        pub entered: std::sync::atomic::AtomicUsize,
        allowed: std::sync::atomic::AtomicUsize,
        released: AtomicBool,
    }
    static GATE: OnceLock<Mutex<std::collections::HashMap<String, Arc<Gate>>>> = OnceLock::new();
    pub fn install(prefix: String) -> Arc<Gate> {
        let gate = Arc::new(Gate {
            entered: Default::default(),
            allowed: Default::default(),
            released: AtomicBool::new(false),
        });
        GATE.get_or_init(Default::default)
            .lock()
            .unwrap()
            .insert(prefix, Arc::clone(&gate));
        gate
    }
    impl Gate {
        pub fn allow(&self, count: usize) {
            self.allowed.store(count, Ordering::Release);
        }
        pub fn release(&self) {
            self.released.store(true, Ordering::Release);
        }
    }
    pub fn wait_at_gate(input: &Path, cancelled: &AtomicBool) -> Result<()> {
        let gate = GATE
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .iter()
            .find(|(prefix, _)| input.to_string_lossy().contains(prefix.as_str()))
            .map(|(_, gate)| Arc::clone(gate));
        if let Some(gate) = gate {
            let index = gate.entered.fetch_add(1, Ordering::AcqRel);
            while !gate.released.load(Ordering::Acquire)
                && index >= gate.allowed.load(Ordering::Acquire)
            {
                ensure!(
                    !cancelled.load(Ordering::Relaxed),
                    "Local encoding cancelled at test gate"
                );
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
        Ok(())
    }
}
