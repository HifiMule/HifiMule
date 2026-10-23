//! Target-independent source decoding and validation used by sync and mediacheck.
//!
//! This module reads one native frame at a time, retaining the same bounded
//! recovery and metadata exceptions regardless of whether frames are encoded.
use anyhow::{Context, Result, anyhow, ensure};
use ffmpeg_next::{self as ff, ChannelLayout, codec, format, frame, media};
use std::collections::BTreeSet;
use std::ffi::{CStr, CString};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// A few libraries prepend an ID3 tag to a RIFF/WAVE file. FFmpeg probes the
/// WAVE container but otherwise attempts to read its header from the ID3 bytes.
/// Trust only the declared tag boundary and a complete, bounded RIFF header.
fn id3_prefixed_wave_offset(input: &Path, input_len: u64) -> Option<u64> {
    let mut file = std::fs::File::open(input).ok()?;
    let mut header = [0; 10];
    file.read_exact(&mut header).ok()?;
    if &header[..3] != b"ID3" || header[4] == 0xff {
        return None;
    }
    let allowed_flags = match header[3] {
        2 => 0xc0,
        3 => 0xe0,
        4 => 0xf0,
        _ => return None,
    };
    if header[5] & !allowed_flags != 0 || header[6..].iter().any(|byte| byte & 0x80 != 0) {
        return None;
    }
    let tag_size = header[6..]
        .iter()
        .fold(0u64, |size, byte| (size << 7) | u64::from(*byte));
    let mut offset = 10 + tag_size;
    if header[3] == 4 && header[5] & 0x10 != 0 {
        file.seek(SeekFrom::Start(offset)).ok()?;
        let mut footer = [0; 10];
        file.read_exact(&mut footer).ok()?;
        if &footer[..3] != b"3DI" || footer[3..] != header[3..] {
            return None;
        }
        offset += 10;
    }
    let remaining = input_len.checked_sub(offset)?;
    if remaining < 12 {
        return None;
    }
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut riff = [0; 12];
    file.read_exact(&mut riff).ok()?;
    let riff_size = u64::from(u32::from_le_bytes(riff[4..8].try_into().ok()?));
    (&riff[..4] == b"RIFF" && &riff[8..] == b"WAVE" && riff_size >= 4 && riff_size <= remaining - 8)
        .then_some(offset)
}

fn open_prefixed_wave(input: &Path, offset: u64) -> Result<format::context::Input, ff::Error> {
    let path = CString::new(input.to_str().ok_or(ff::Error::InvalidData)?)
        .map_err(|_| ff::Error::InvalidData)?;
    let mut options = ff::Dictionary::new();
    options.set("skip_initial_bytes", &offset.to_string());
    // Explicitly choose the already-validated container: automatic probing runs
    // before skip_initial_bytes and can miss WAVE behind a large ID3 tag.
    unsafe {
        let wave = ff::ffi::av_find_input_format(c"wav".as_ptr());
        if wave.is_null() {
            return Err(ff::Error::DemuxerNotFound);
        }
        let mut context = std::ptr::null_mut();
        let mut options = options.disown();
        let result = ff::ffi::avformat_open_input(&mut context, path.as_ptr(), wave, &mut options);
        // FFmpeg returns unused options to the caller, which owns them again.
        drop(ff::Dictionary::own(options));
        if result < 0 {
            // avformat_open_input frees its context on failure.
            return Err(ff::Error::from(result));
        }
        // Own the opened context before the next fallible operation so stream
        // discovery failures close the input and release all native resources.
        let mut context = format::context::Input::wrap(context);
        let result = ff::ffi::avformat_find_stream_info(context.as_mut_ptr(), std::ptr::null_mut());
        if result < 0 {
            return Err(ff::Error::from(result));
        }
        Ok(context)
    }
}

/// Extensions supported by demuxers in HifiMule's linked controlled runtime.
/// Conventional aliases supplement demuxers with incomplete extension metadata.
/// A recognized container can still hold an unsupported codec or no audio.
pub fn supported_extensions() -> Result<BTreeSet<String>> {
    ff::init().context("Cannot initialize HifiMule audio runtime")?;
    let mut extensions = BTreeSet::new();
    let mut opaque = std::ptr::null_mut();
    loop {
        let demuxer = unsafe { ff::ffi::av_demuxer_iterate(&mut opaque) };
        if demuxer.is_null() {
            break;
        }
        // Some probe-based demuxers advertise no extensions; Ogg omits its
        // codec-specific aliases. Gate these conventional container suffixes on
        // the actual enabled demuxer so runtime changes still apply to auditing.
        let demuxer_names = unsafe { CStr::from_ptr((*demuxer).name) }.to_string_lossy();
        for name in demuxer_names.split(',') {
            let aliases: &[&str] = match name {
                "wav" => &["wav", "wave"],
                "aiff" => &["aif", "aiff", "aifc", "afc"],
                "asf" => &["asf", "wma", "wmv"],
                "ogg" => &["ogg", "oga", "opus"],
                _ => &[],
            };
            extensions.extend(aliases.iter().map(|extension| (*extension).to_owned()));
        }
        let names = unsafe { (*demuxer).extensions };
        if !names.is_null() {
            for extension in unsafe { CStr::from_ptr(names) }
                .to_string_lossy()
                .split(',')
            {
                let extension = extension.trim().to_ascii_lowercase();
                if !extension.is_empty() {
                    extensions.insert(extension);
                }
            }
        }
    }
    // The MOV demuxer also advertises image-only containers. They are not
    // audio library candidates, even though they share that demuxer's parser.
    for image in ["avif", "heic", "heif"] {
        extensions.remove(image);
    }
    ensure!(
        !extensions.is_empty(),
        "HifiMule audio runtime advertises no source extensions"
    );
    Ok(extensions)
}

/// Evidence available to the encoder's logger and native-buffer ownership tests.
pub enum SourceEvent<'a> {
    Packet(&'a ff::Packet),
    IgnoredMp3Metadata { position: isize, bytes: usize },
}

/// An opened audio source. Decoder state and compressed packets are never shared
/// between jobs. Consuming `decode` releases native resources on every exit.
pub struct SourceDecoder {
    input: PathBuf,
    context: format::context::Input,
    decoder: ff::decoder::Audio,
    input_len: u64,
    input_index: usize,
    input_is_mp3: bool,
    input_layout: ChannelLayout,
    source_rate: u32,
    source_duration: i64,
    duration_method: ff::ffi::AVDurationEstimationMethod,
    skipped_id3_bytes: Option<u64>,
}

impl SourceDecoder {
    pub fn open(input: &Path) -> Result<Self> {
        ff::init().context("Cannot initialize HifiMule audio runtime")?;
        // ffmpeg-next unwraps UTF-8 paths internally. Report an unsupported
        // filename rather than aborting the entire library audit.
        ensure!(
            input.to_str().is_some(),
            "Original audio path is not valid UTF-8; the audio runtime cannot open this filename"
        );
        let input_len = std::fs::metadata(input)
            .context("Cannot inspect original audio")?
            .len();
        let skipped_id3_bytes = id3_prefixed_wave_offset(input, input_len);
        let context = if let Some(offset) = skipped_id3_bytes {
            open_prefixed_wave(input, offset)
        } else {
            format::input(input)
        }
        .context("Cannot open original audio")?;
        let stream = context
            .streams()
            .best(media::Type::Audio)
            .ok_or_else(|| anyhow!("Original contains no audio stream"))?;
        let input_index = stream.index();
        let input_is_mp3 = stream.parameters().id() == codec::Id::MP3;
        let mut decoder_context = codec::context::Context::from_parameters(stream.parameters())?;
        decoder_context.set_threading(codec::threading::Config {
            count: 1,
            ..Default::default()
        });
        let decoder = decoder_context
            .decoder()
            .audio()
            .context("Original audio decoder unavailable")?;
        ensure!(
            decoder.rate() > 0 && decoder.channels() > 0,
            "Original has invalid audio properties"
        );
        let source_rate = decoder.rate();
        let source_duration = context.duration();
        let duration_method = unsafe { (*context.as_ptr()).duration_estimation_method };
        let input_layout = if decoder.channel_layout().is_empty() {
            ChannelLayout::default(decoder.channels() as i32)
        } else {
            decoder.channel_layout()
        };
        Ok(Self {
            input: input.to_path_buf(),
            context,
            decoder,
            input_len,
            input_index,
            input_is_mp3,
            input_layout,
            source_rate,
            source_duration,
            duration_method,
            skipped_id3_bytes,
        })
    }

    pub fn skipped_id3_bytes(&self) -> Option<u64> {
        self.skipped_id3_bytes
    }

    pub fn input_context(&self) -> &format::context::Input {
        &self.context
    }

    pub fn audio_stream_index(&self) -> usize {
        self.input_index
    }

    pub fn decoder(&self) -> &ff::decoder::Audio {
        &self.decoder
    }

    /// Decode without converting or encoding. `check` supports cancellation and
    /// caller capacity checks. Callback failures propagate without recovery.
    pub fn decode(
        self,
        check: impl FnMut() -> Result<()>,
        on_frame: impl FnMut(&mut frame::Audio) -> Result<()>,
    ) -> Result<SourceReport> {
        self.decode_with_observer(check, on_frame, |_| {})
    }

    pub fn decode_with_observer(
        mut self,
        mut check: impl FnMut() -> Result<()>,
        mut on_frame: impl FnMut(&mut frame::Audio) -> Result<()>,
        mut observe: impl FnMut(SourceEvent<'_>),
    ) -> Result<SourceReport> {
        let mut recovery = DecodeRecovery::default();
        let mut decoded_samples = 0u64;
        let input_layout = self.input_layout;
        let mut receive = |decoder: &mut ff::decoder::Audio,
                           recovery: &mut DecodeRecovery,
                           check: &mut dyn FnMut() -> Result<()>|
         -> Result<()> {
            let mut decoded = frame::Audio::empty();
            loop {
                match decoder.receive_frame(&mut decoded) {
                    Ok(()) => {
                        recovery.consecutive = 0;
                        if decoded.channel_layout().is_empty()
                            || decoded.channel_layout().bits() == 0
                        {
                            decoded.set_channel_layout(input_layout);
                        }
                        check()?;
                        ensure!(
                            decoded.samples() <= 1_048_576,
                            "Original audio frame is too large"
                        );
                        // Count before any target conversion or resampling.
                        decoded_samples += decoded.samples() as u64;
                        on_frame(&mut decoded)?;
                    }
                    Err(error) => match error {
                        ff::Error::Eof => break,
                        ff::Error::Other { errno } if errno == ff::error::EAGAIN => break,
                        error => recovery
                            .reject_buffered(error)
                            .context("Cannot decode original audio frame")?,
                    },
                }
            }
            Ok(())
        };
        loop {
            check()?;
            // av_read_frame requires an empty packet. Its previous native buffer
            // must be released for skipped streams and tolerated metadata too.
            let mut packet = ff::Packet::empty();
            match packet.read(&mut self.context) {
                Ok(()) => {
                    observe(SourceEvent::Packet(&packet));
                    if packet.stream() == self.input_index {
                        let sent = self.decoder.send_packet(&packet);
                        if let Err(error) = sent {
                            if error == ff::Error::InvalidData
                                && self.input_is_mp3
                                && is_mp3_metadata_tail(
                                    &self.input,
                                    packet.data().unwrap_or_default(),
                                    packet.position(),
                                    self.input_len,
                                )
                            {
                                observe(SourceEvent::IgnoredMp3Metadata {
                                    position: packet.position(),
                                    bytes: packet.size(),
                                });
                                continue;
                            }
                        }
                        recovery.submit(packet.size());
                        if let Err(error) = sent {
                            recovery
                                .reject(error, packet.position(), packet.size())
                                .with_context(|| {
                                    format!(
                                        "Cannot decode original audio packet at byte {} ({} bytes)",
                                        packet.position(),
                                        packet.size()
                                    )
                                })?;
                        }
                        receive(&mut self.decoder, &mut recovery, &mut check)?;
                    }
                }
                Err(ff::Error::Eof) => break,
                Err(error) => return Err(error).context("Cannot read original audio packet"),
            }
        }
        if let Err(error) = self.decoder.send_eof() {
            recovery
                .reject_buffered(error)
                .context("Cannot finish decoding original audio")?;
        }
        receive(&mut self.decoder, &mut recovery, &mut check)?;
        recovery.validate()?;
        drop(receive);
        ensure!(decoded_samples > 0, "Original audio decoded no samples");
        let decoded_duration =
            (u128::from(decoded_samples) * 1_000_000 / u128::from(self.source_rate)) as i64;
        Ok(SourceReport {
            probed_duration_micros: self.source_duration,
            decoded_duration_micros: decoded_duration,
            duration_estimation_method: self.duration_method,
            recovery: recovery.report,
        })
    }
}

#[derive(Debug)]
pub struct SourceReport {
    pub probed_duration_micros: i64,
    pub decoded_duration_micros: i64,
    pub duration_estimation_method: ff::ffi::AVDurationEstimationMethod,
    pub recovery: RecoveryReport,
}

impl SourceReport {
    /// Check source completeness without choosing a target format. Bitrate-based
    /// estimates can include metadata bytes and cannot establish truncation.
    pub fn validate_source(&self) -> Result<()> {
        self.validate_duration(self.decoded_duration_micros, "Decoded", "decoded")
    }

    /// Retain sync's completed-output check, independently of decoded samples.
    pub fn validate_output(&self, actual_duration_micros: i64) -> Result<()> {
        self.validate_duration(actual_duration_micros, "Encoded", "output")
    }

    fn validate_duration(&self, actual: i64, kind: &str, actual_name: &str) -> Result<()> {
        let source = self.probed_duration_micros;
        let decoded = self.decoded_duration_micros;
        let method = self.duration_estimation_method;
        let use_decoded = source <= 0
            || method == ff::ffi::AVDurationEstimationMethod::AVFMT_DURATION_FROM_BITRATE;
        let (expected, expectation) = if use_decoded {
            (decoded, "decoded samples")
        } else {
            (source, "probed source")
        };
        if expected > 0 && actual > 0 {
            ensure!(
                expected.abs_diff(actual) <= 2_000_000,
                "{kind} duration does not match original: expected {:.3}s, {actual_name} {:.3}s; probed source {:.3}s, decoded {:.3}s; duration estimation method {:?}; expectation {}",
                expected as f64 / 1_000_000.0,
                actual as f64 / 1_000_000.0,
                source as f64 / 1_000_000.0,
                decoded as f64 / 1_000_000.0,
                method,
                expectation
            );
        }
        Ok(())
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
    data.len() <= mp3_trailing_metadata_len(&tail, file_len - len as u64)
}

/// Return the contiguous, recognizable metadata suffix. Tag sizes are used
/// solely to verify rejected packets, never to truncate the source or audio.
fn mp3_trailing_metadata_len(data: &[u8], file_offset: u64) -> usize {
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
    // The observed lookup trailer is separate from an ID3 MLLT frame:
    // MLLT\0, little-endian offsets, entry count, then this fixed footer.
    // Its header can sit inside an accepted audio packet, so locate it from
    // the count at EOF instead of requiring a rejected packet to start there.
    const MLLT_FOOTER: &[u8] = b"\x10MLLT 01234567890j";
    if end >= 27 && data[..end].ends_with(MLLT_FOOTER) {
        let count_at = end - MLLT_FOOTER.len() - 4;
        let count = u32::from_le_bytes(data[count_at..count_at + 4].try_into().unwrap()) as usize;
        if (1..=(MAX_MP3_METADATA_TAIL - 27) / 4).contains(&count) {
            let size = 27 + count * 4;
            if size <= end {
                let start = end - size;
                let mut previous = 0;
                let valid_offsets = data[start + 5..count_at].chunks_exact(4).all(|entry| {
                    let offset = u32::from_le_bytes(entry.try_into().unwrap());
                    // Offsets are relative to the audio's beginning. Even
                    // without locating it, they cannot exceed the trailer's
                    // absolute position in the file.
                    let valid =
                        offset > previous && u64::from(offset) <= file_offset + start as u64;
                    previous = offset;
                    valid
                });
                if &data[start..start + 5] == b"MLLT\0" && valid_offsets {
                    end = start;
                }
            }
        }
    }
    data.len() - end
}

/// Bounded evidence of damaged audio skipped by the decoder. Recovery can leave
/// audible defects; successful output does not imply that damaged sound was restored.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RecoveryReport {
    pub invalid_events: u64,
    /// Charged bytes: exact for rejected packets, conservatively the largest
    /// submitted audio packet for errors from buffered decoding or EOF draining.
    pub rejected_bytes: u64,
    pub first_position: Option<isize>,
}

#[derive(Default)]
struct DecodeRecovery {
    report: RecoveryReport,
    submitted_packets: u64,
    largest_packet: usize,
    consecutive: u64,
}

impl DecodeRecovery {
    fn submit(&mut self, bytes: usize) {
        self.submitted_packets += 1;
        self.largest_packet = self.largest_packet.max(bytes);
    }

    // A buffered decoder error cannot be assigned to the current input packet.
    // Charge an upper bound for any submitted packet rather than zero at EOF.
    fn reject_buffered(&mut self, error: ff::Error) -> Result<()> {
        self.reject(error, -1, self.largest_packet)
    }

    fn reject(&mut self, error: ff::Error, position: isize, bytes: usize) -> Result<()> {
        if error != ff::Error::InvalidData {
            return Err(error.into());
        }
        self.report.invalid_events += 1;
        self.report.rejected_bytes = self.report.rejected_bytes.saturating_add(bytes as u64);
        if self.report.invalid_events == 1 && position >= 0 {
            self.report.first_position = Some(position);
        }
        self.consecutive += 1;
        ensure!(
            self.report.invalid_events <= 8,
            "Too many invalid audio decoding events (maximum 8)"
        );
        ensure!(
            self.consecutive <= 3,
            "Sustained invalid audio decoding (maximum 3 consecutive events)"
        );
        ensure!(
            self.report.rejected_bytes <= 1_048_576,
            "Rejected audio exceeds 1 MiB recovery limit"
        );
        Ok(())
    }

    fn validate(&self) -> Result<()> {
        ensure!(
            self.report.invalid_events.saturating_mul(100) <= self.submitted_packets,
            "Invalid audio decoding exceeds one percent of submitted packets ({} events / {} packets)",
            self.report.invalid_events,
            self.submitted_packets
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(path: &Path, seconds: u32) {
        use std::io::Write;
        let samples = 48_000 * seconds;
        let mut file = std::fs::File::create(path).unwrap();
        file.write_all(b"RIFF").unwrap();
        file.write_all(&(36 + samples * 2).to_le_bytes()).unwrap();
        file.write_all(b"WAVEfmt ").unwrap();
        file.write_all(&16u32.to_le_bytes()).unwrap();
        file.write_all(&1u16.to_le_bytes()).unwrap();
        file.write_all(&1u16.to_le_bytes()).unwrap();
        file.write_all(&48_000u32.to_le_bytes()).unwrap();
        file.write_all(&96_000u32.to_le_bytes()).unwrap();
        file.write_all(&2u16.to_le_bytes()).unwrap();
        file.write_all(&16u16.to_le_bytes()).unwrap();
        file.write_all(b"data").unwrap();
        file.write_all(&(samples * 2).to_le_bytes()).unwrap();
        file.write_all(&vec![0; samples as usize * 2]).unwrap();
    }

    #[test]
    fn source_decode_streams_complete_audio_without_creating_an_output() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source.wav");
        wav(&path, 3);
        let original = std::fs::read(&path).unwrap();
        let mut samples = 0;
        let report = SourceDecoder::open(&path)
            .unwrap()
            .decode(
                || Ok(()),
                |frame| {
                    samples += frame.samples();
                    Ok(())
                },
            )
            .unwrap();
        report.validate_source().unwrap();
        assert_eq!(samples, 144_000);
        assert_eq!(report.decoded_duration_micros, 3_000_000);
        assert_eq!(report.recovery, RecoveryReport::default());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn id3_prefixed_wave_decodes_with_mp3_or_extensionless_names_without_source_changes() {
        let dir = tempfile::tempdir().unwrap();
        let clean = dir.path().join("clean.wav");
        wav(&clean, 1);
        let wave = std::fs::read(&clean).unwrap();
        for (version, footer, tag_size) in [
            (2, false, 128),
            (3, false, 128),
            (4, false, 128),
            (4, true, 128),
            (4, false, 8 * 1024 * 1024),
        ] {
            let header = [
                b'I',
                b'D',
                b'3',
                version,
                0,
                if footer { 0x10 } else { 0 },
                ((tag_size >> 21) & 0x7f) as u8,
                ((tag_size >> 14) & 0x7f) as u8,
                ((tag_size >> 7) & 0x7f) as u8,
                (tag_size & 0x7f) as u8,
            ];
            let mut bytes = header.to_vec();
            bytes.resize(10 + tag_size, 0);
            if footer {
                bytes.extend_from_slice(b"3DI");
                bytes.extend_from_slice(&header[3..]);
            }
            let offset = bytes.len() as u64;
            bytes.extend_from_slice(&wave);
            for name in ["tagged.MP3", "000001-staged"] {
                let path = dir.path().join(name);
                std::fs::write(&path, &bytes).unwrap();
                assert_eq!(
                    id3_prefixed_wave_offset(&path, bytes.len() as u64),
                    Some(offset)
                );
                let source = SourceDecoder::open(&path).unwrap();
                assert_eq!(source.input_context().format().name(), "wav");
                let report = source.decode(|| Ok(()), |_| Ok(())).unwrap();
                report.validate_source().unwrap();
                assert_eq!(report.decoded_duration_micros, 1_000_000);
                assert_eq!(report.recovery, RecoveryReport::default());
                assert_eq!(std::fs::read(&path).unwrap(), bytes);
            }
        }
    }

    #[test]
    fn id3_wave_offset_rejects_invalid_tags_and_container_boundaries() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source");
        let valid = b"ID3\x04\x00\x00\x00\x00\x00\x00RIFF\x04\x00\x00\x00WAVE".to_vec();
        let mut invalid = vec![valid[..9].to_vec(), valid[..21].to_vec()];
        for (at, value) in [
            (3, 5),     // Unsupported ID3 version.
            (4, 0xff),  // Invalid revision.
            (5, 1),     // Reserved ID3 flags.
            (6, 0x80),  // Non-synchsafe length.
            (9, 127),   // Tag extends beyond EOF.
            (10, b'X'), // Not RIFF.
            (14, 3),    // RIFF size excludes its format identifier.
            (14, 5),    // RIFF size extends beyond EOF.
            (18, b'X'), // Not WAVE.
        ] {
            let mut bytes = valid.clone();
            bytes[at] = value;
            invalid.push(bytes);
        }
        // The footer flag must have a matching footer before the container.
        let mut missing_footer = valid.clone();
        missing_footer[5] = 0x10;
        invalid.push(missing_footer);
        for bytes in invalid {
            std::fs::write(&path, &bytes).unwrap();
            assert_eq!(
                id3_prefixed_wave_offset(&path, bytes.len() as u64),
                None,
                "{bytes:?}"
            );
        }
        std::fs::write(&path, &valid[10..]).unwrap();
        assert_eq!(
            id3_prefixed_wave_offset(&path, (valid.len() - 10) as u64),
            None
        );
        // A normal tagged MPEG stream must retain FFmpeg's usual tag handling.
        let mut mp3 = valid[..10].to_vec();
        mp3.extend_from_slice(&[0xff, 0xfb, 0x90, 0].repeat(3));
        std::fs::write(&path, &mp3).unwrap();
        assert_eq!(id3_prefixed_wave_offset(&path, mp3.len() as u64), None);
    }

    #[test]
    fn source_decode_rejects_empty_audio_and_preserves_callback_failures() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source.wav");
        wav(&path, 0);
        let error = SourceDecoder::open(&path)
            .unwrap()
            .decode(|| Ok(()), |_| Ok(()))
            .unwrap_err();
        assert!(
            error.to_string().contains("decoded no samples"),
            "{error:#}"
        );
        wav(&path, 1);
        let error = SourceDecoder::open(&path)
            .unwrap()
            .decode(|| anyhow::bail!("cancelled by caller"), |_| Ok(()))
            .unwrap_err();
        assert_eq!(error.to_string(), "cancelled by caller");
        let error = SourceDecoder::open(&path)
            .unwrap()
            .decode(|| Ok(()), |_| Err(ff::Error::InvalidData.into()))
            .unwrap_err();
        assert_eq!(
            error.downcast_ref::<ff::Error>(),
            Some(&ff::Error::InvalidData)
        );
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_source_paths_return_an_error_without_panicking() {
        use std::os::unix::ffi::OsStrExt;
        let path = Path::new(std::ffi::OsStr::from_bytes(b"invalid-\xff.wav"));
        let error = SourceDecoder::open(path).err().unwrap();
        assert!(error.to_string().contains("path is not valid UTF-8"));
    }

    #[test]
    fn source_validation_shares_trusted_duration_and_estimate_rules() {
        use ff::ffi::AVDurationEstimationMethod::*;
        let mut report = SourceReport {
            probed_duration_micros: 8_000_000,
            decoded_duration_micros: 3_000_000,
            duration_estimation_method: AVFMT_DURATION_FROM_STREAM,
            recovery: RecoveryReport::default(),
        };
        assert!(report.validate_source().is_err());
        assert!(report.validate_output(3_000_000).is_err());
        report.duration_estimation_method = AVFMT_DURATION_FROM_BITRATE;
        report.validate_source().unwrap();
        report.validate_output(3_000_000).unwrap();
        assert!(report.validate_output(500_000).is_err());
        report.duration_estimation_method = AVFMT_DURATION_FROM_PTS;
        report.probed_duration_micros = ff::ffi::AV_NOPTS_VALUE;
        report.validate_source().unwrap();
        report.probed_duration_micros = 5_000_000;
        report.validate_source().unwrap();
        report.probed_duration_micros += 1;
        assert!(report.validate_source().is_err());
    }

    #[test]
    fn supported_extensions_come_from_the_enabled_runtime() {
        let extensions = supported_extensions().unwrap();
        for extension in [
            "mp3", "flac", "m4a", "aac", "ogg", "opus", "wav", "wma", "aif", "aiff",
        ] {
            assert!(
                extensions.contains(extension),
                "missing {extension}: {extensions:?}"
            );
        }
        assert!(!extensions.contains("ac3"));
        assert!(!extensions.contains("txt"));
        for image in ["avif", "heic", "heif"] {
            assert!(!extensions.contains(image));
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

    fn mllt_mp3_tags() -> Vec<u8> {
        let mut bytes = b"MLLT\0".to_vec();
        for index in 1..=128u32 {
            bytes.extend_from_slice(&(index * 417).to_le_bytes());
        }
        bytes.extend_from_slice(&128u32.to_le_bytes());
        bytes.extend_from_slice(b"\x10MLLT 01234567890jTAG");
        bytes.extend_from_slice(&[0; 125]);
        bytes
    }

    #[test]
    fn mllt_metadata_requires_a_complete_bounded_table_with_valid_offsets() {
        let tags = mllt_mp3_tags();
        let count_at = 5 + 128 * 4;
        assert_eq!(mp3_trailing_metadata_len(&tags, 60_000), tags.len());
        let mut malformed = Vec::new();
        for offset in [0, 4, count_at + 4, count_at + 21] {
            let mut bad = tags.clone();
            bad[offset] ^= 0xff;
            malformed.push(bad);
        }
        for count in [0, 127, 129, u32::MAX] {
            let mut bad = tags.clone();
            bad[count_at..count_at + 4].copy_from_slice(&count.to_le_bytes());
            malformed.push(bad);
        }
        for (offset, value) in [(5, 0), (9, 417), (13, 416), (13, u32::MAX)] {
            let mut bad = tags.clone();
            bad[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            malformed.push(bad);
        }
        let mut truncated = tags.clone();
        truncated.remove(count_at + 5);
        malformed.push(truncated);
        let mut oversized = b"MLLT\0".to_vec();
        let count = MAX_MP3_METADATA_TAIL as u32 / 4;
        for offset in 1..=count {
            oversized.extend_from_slice(&offset.to_le_bytes());
        }
        oversized.extend_from_slice(&count.to_le_bytes());
        oversized.extend_from_slice(&tags[count_at + 4..]);
        assert_eq!(mp3_trailing_metadata_len(&oversized, 1_000_000), 128);
        for bad in malformed {
            assert_eq!(mp3_trailing_metadata_len(&bad, 60_000), 128);
        }
        assert_eq!(mp3_trailing_metadata_len(&tags, 0), 128);
    }

    #[test]
    fn mllt_metadata_split_across_packets_is_verified_from_the_file() {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("split-mllt.mp3");
        let tags = mllt_mp3_tags();
        let mut bytes = vec![0; 60_000];
        bytes.extend_from_slice(&tags);
        std::fs::write(&input, &bytes).unwrap();
        for offset in 0..tags.len() {
            assert!(
                is_mp3_metadata_tail(
                    &input,
                    &tags[offset..],
                    (60_000 + offset) as isize,
                    bytes.len() as u64,
                ),
                "MLLT split at {offset}"
            );
        }
        assert!(!is_mp3_metadata_tail(
            &input,
            &bytes[59_999..],
            59_999,
            bytes.len() as u64
        ));
        // Keep the verified file suffix valid so only the packet's MPEG
        // header guard can reject this audio-like packet.
        let mut audio_like_packet = tags[46..].to_vec();
        audio_like_packet[..4].copy_from_slice(&[0xff, 0xfb, 0x90, 0]);
        assert!(!is_mp3_metadata_tail(
            &input,
            &audio_like_packet,
            60_046,
            bytes.len() as u64
        ));
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
    fn recovery_policy_bounds_events_consecutive_bytes_fraction_and_error_kind() {
        let mut recovery = DecodeRecovery::default();
        assert!(recovery.reject(ff::Error::InvalidData, 12, 100).is_ok());
        assert_eq!(recovery.report.first_position, Some(12));
        recovery.submitted_packets = 99;
        assert!(recovery.validate().is_err());
        recovery.submitted_packets = 100;
        recovery.validate().unwrap();
        for _ in 0..2 {
            recovery.reject(ff::Error::InvalidData, 20, 1).unwrap();
        }
        assert!(recovery.reject(ff::Error::InvalidData, 20, 1).is_err());
        let mut recovery = DecodeRecovery::default();
        for _ in 0..8 {
            recovery.consecutive = 0;
            recovery.reject(ff::Error::InvalidData, 20, 1).unwrap();
        }
        recovery.consecutive = 0;
        assert!(recovery.reject(ff::Error::InvalidData, 20, 1).is_err());
        let mut recovery = DecodeRecovery::default();
        recovery
            .reject(ff::Error::InvalidData, 20, 1_048_576)
            .unwrap();
        assert!(recovery.reject(ff::Error::InvalidData, 20, 1).is_err());
        let mut recovery = DecodeRecovery::default();
        assert!(
            recovery
                .reject(
                    ff::Error::Other {
                        errno: ff::error::EIO
                    },
                    20,
                    1
                )
                .is_err()
        );
        assert_eq!(recovery.report, RecoveryReport::default());
    }

    #[test]
    fn buffered_decode_errors_charge_high_water_bytes_and_keep_unknown_first_position() {
        let mut recovery = DecodeRecovery::default();
        recovery.submit(700_000);
        recovery.submit(100);
        recovery.reject_buffered(ff::Error::InvalidData).unwrap();
        assert_eq!(recovery.report.rejected_bytes, 700_000);
        assert_eq!(recovery.report.first_position, None);
        recovery.reject(ff::Error::InvalidData, 42, 100).unwrap();
        assert_eq!(
            recovery.report.first_position, None,
            "a later location is not the first error"
        );
        // EOF has no current packet, but cannot bypass the cumulative byte cap.
        assert!(recovery.reject_buffered(ff::Error::InvalidData).is_err());
        assert_eq!(recovery.report.rejected_bytes, 1_400_100);
        let mut fatal = DecodeRecovery::default();
        fatal.submit(700_000);
        assert!(
            fatal
                .reject_buffered(ff::Error::Other {
                    errno: ff::error::EIO
                })
                .is_err()
        );
        assert_eq!(fatal.report, RecoveryReport::default());
    }
}
