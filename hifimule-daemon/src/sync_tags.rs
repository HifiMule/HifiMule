//! Local-only audiobook tagging. Audio bytes are copied, never decoded/re-encoded.
use crate::providers::SyncBookTags;
use anyhow::{Context, Result, ensure};
use id3::TagLike;
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

// Bound metadata allocation independently of audiobook size. A second disk copy is
// reserved by the caller before this module starts any rewrite.
const MAX_METADATA: u64 = 16 * 1024 * 1024;
pub fn rewrite_reservation(source: u64) -> Result<u64> {
    source
        .checked_mul(2)
        .and_then(|n| n.checked_add(MAX_METADATA * 2))
        .context("Audiobook tag workspace size overflow")
}

#[cfg(test)]
fn apply(path: &Path, container: &str, tags: &SyncBookTags) -> Result<u64> {
    apply_cancellable(path, container, tags, &AtomicBool::new(false))
}

pub fn apply_cancellable(
    path: &Path,
    container: &str,
    tags: &SyncBookTags,
    cancel: &AtomicBool,
) -> Result<u64> {
    check_cancel(cancel)?;
    match container {
        "mp3" => write_mp3(path, tags, cancel)?,
        "m4a" | "m4b" | "mp4" => write_mp4(path, tags, cancel)?,
        _ => anyhow::bail!("Unsupported audiobook tag format"),
    }
    Ok(std::fs::metadata(path)?.len())
}

fn write_mp3(path: &Path, tags: &SyncBookTags, cancel: &AtomicBool) -> Result<()> {
    let mut source = File::open(path)?;
    let mut header = [0; 10];
    source.read_exact(&mut header)?;
    if &header[..3] == b"ID3" {
        ensure!(
            header[6..10].iter().all(|b| b & 0x80 == 0),
            "Invalid ID3 size"
        );
        let size = header[6..10]
            .iter()
            .fold(0u64, |n, b| (n << 7) | u64::from(*b));
        ensure!(size <= MAX_METADATA, "ID3 metadata exceeds safety limit");
        // Check nested frame sizes before the library allocates their buffers.
        ensure!(
            (2..=4).contains(&header[3]) && header[5] & 0x50 == 0,
            "Unsupported ID3 extended header, footer or compression"
        );
        let mut frames = vec![0; size as usize];
        source.read_exact(&mut frames)?;
        if header[3] < 4 && header[5] & 0x80 != 0 {
            undo_unsynchronisation(&mut frames);
        }
        validate_id3_frames(&frames, header[3], 0)?;
    }
    source.rewind()?;
    let mut tag = match id3::Tag::read_from2(&mut source) {
        Ok(tag) => tag,
        Err(e) if matches!(e.kind, id3::ErrorKind::NoTag) => {
            let mut tag = id3::Tag::with_version(id3::Version::Id3v23);
            // Preserve optional legacy values when introducing the first v2 tag;
            // players generally prefer v2 fields over the trailing v1 block.
            match id3::v1::Tag::read_from(&mut source) {
                Ok(legacy) => {
                    let legacy: id3::Tag = legacy.into();
                    for frame in legacy.frames() {
                        tag.add_frame(frame.clone());
                    }
                }
                Err(error) if matches!(error.kind, id3::ErrorKind::NoTag) => {}
                Err(error) => return Err(error).context("Malformed legacy ID3 metadata"),
            }
            tag
        }
        Err(e) => return Err(e).context("Unsafe or malformed ID3 metadata"),
    };
    ensure!(
        tag.chapters()
            .all(|c| c.start_offset == u32::MAX && c.end_offset == u32::MAX),
        "ID3 chapters with absolute byte offsets cannot be safely rewritten"
    );
    let version = tag.version();
    if let Some(v) = &tags.album {
        tag.set_album(v);
    }
    if let Some(v) = &tags.authors {
        tag.set_artist(v);
        tag.set_album_artist(v);
    }
    if let Some(v) = &tags.title {
        tag.set_title(v);
    }
    tag.set_track(tags.track);
    tag.set_total_tracks(tags.total);
    tag.set_disc(1);
    tag.set_total_discs(1);
    if let Some(year) = tags.year {
        tag.set_year(year as i32);
        if version == id3::Version::Id3v24 {
            tag.set_text("TDRC", format!("{year:04}"));
        }
    }
    let mut encoded = Vec::new();
    tag.write_to(&mut encoded, version)?;
    ensure!(
        encoded.len() as u64 <= MAX_METADATA,
        "Updated ID3 metadata exceeds safety limit"
    );
    // Reject lossy library serialization, including unsupported legacy frames.
    let reopened = id3::Tag::read_from2(std::io::Cursor::new(&encoded))?;
    ensure!(
        reopened == tag,
        "ID3 metadata cannot be rewritten without loss"
    );
    source.rewind()?;
    if !id3::Tag::skip(&mut source)? {
        source.rewind()?;
    }
    let audio_start = source.stream_position()?;
    validate_mp3_frame(&mut source, audio_start)?;
    source.seek(SeekFrom::Start(audio_start))?;
    let mut output =
        tempfile::NamedTempFile::new_in(path.parent().context("Missing staging directory")?)?;
    output.write_all(&encoded)?;
    copy_cancellable(&mut source, &mut output, cancel)?;
    output.flush()?;
    check_cancel(cancel)?;
    drop(source);
    output.persist(path).map_err(|e| e.error)?;
    Ok(())
}

// ID3's decoder trusts v2.3 frame lengths and can inflate compressed frames.
// Validate the bounded on-disk frame tree first, rejecting compression rather
// than letting a tiny downloaded tag allocate an unbounded decoded payload.
fn validate_id3_frames(mut bytes: &[u8], version: u8, depth: usize) -> Result<()> {
    ensure!(depth <= 16, "ID3 chapter nesting exceeds safety limit");
    while !bytes.is_empty() {
        if bytes[0] == 0 {
            ensure!(bytes.iter().all(|b| *b == 0), "Invalid ID3 padding");
            break;
        }
        let header = if version == 2 { 6 } else { 10 };
        ensure!(bytes.len() >= header, "Truncated ID3 frame");
        let id_len = if version == 2 { 3 } else { 4 };
        let id = &bytes[..id_len];
        ensure!(
            id.iter()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit()),
            "Invalid ID3 frame identifier"
        );
        let size = if version == 2 {
            (usize::from(bytes[3]) << 16) | (usize::from(bytes[4]) << 8) | usize::from(bytes[5])
        } else if version == 3 {
            u32::from_be_bytes(bytes[4..8].try_into()?) as usize
        } else {
            ensure!(
                bytes[4..8].iter().all(|b| b & 0x80 == 0),
                "Invalid ID3 frame size"
            );
            bytes[4..8]
                .iter()
                .fold(0usize, |n, b| (n << 7) | usize::from(*b))
        };
        ensure!(
            size <= bytes.len() - header,
            "ID3 frame exceeds declared tag size"
        );
        let flags = if version == 2 { 0 } else { bytes[9] };
        let unsupported = if version == 3 { 0xe0 } else { 0x4c };
        ensure!(
            flags & unsupported == 0,
            "Compressed, encrypted or grouped ID3 frames cannot be safely rewritten"
        );
        let mut content = &bytes[header..header + size];
        if version == 4 && flags & 1 != 0 {
            ensure!(content.len() >= 4, "Truncated ID3 data length");
            content = &content[4..];
        }
        let mut decoded;
        if version == 4 && flags & 2 != 0 {
            decoded = content.to_vec();
            undo_unsynchronisation(&mut decoded);
            content = &decoded;
        }
        if id == b"CHAP" || id == b"CTOC" {
            let end = content
                .iter()
                .position(|b| *b == 0)
                .context("Invalid ID3 chapter identifier")?;
            content = &content[end + 1..];
            if id == b"CHAP" {
                ensure!(content.len() >= 16, "Truncated ID3 chapter");
                content = &content[16..];
            } else {
                ensure!(content.len() >= 2, "Truncated ID3 chapter table");
                let count = content[1];
                content = &content[2..];
                for _ in 0..count {
                    let end = content
                        .iter()
                        .position(|b| *b == 0)
                        .context("Invalid ID3 chapter reference")?;
                    content = &content[end + 1..];
                }
            }
            validate_id3_frames(content, version, depth + 1)?;
        }
        bytes = &bytes[header + size..];
    }
    Ok(())
}
fn undo_unsynchronisation(bytes: &mut Vec<u8>) {
    let mut write = 0;
    let mut previous_ff = false;
    for read in 0..bytes.len() {
        let byte = bytes[read];
        if previous_ff && byte == 0 {
            previous_ff = false;
            continue;
        }
        bytes[write] = byte;
        write += 1;
        previous_ff = byte == 0xff;
    }
    bytes.truncate(write);
}
fn validate_mp3_frame(source: &mut File, start: u64) -> Result<()> {
    let mut header = [0; 4];
    source.read_exact(&mut header)?;
    let version = (header[1] >> 3) & 3;
    let layer = (header[1] >> 1) & 3;
    let bitrate_index = usize::from(header[2] >> 4);
    let sample_index = usize::from((header[2] >> 2) & 3);
    ensure!(
        header[0] == 0xff
            && header[1] & 0xe0 == 0xe0
            && version != 1
            && layer == 1
            && (1..15).contains(&bitrate_index)
            && sample_index < 3
            && header[3] & 3 != 2,
        "Invalid or unsupported MP3 audio header"
    );
    let bitrates = if version == 3 {
        [
            0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0,
        ]
    } else {
        [
            0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160, 0,
        ]
    };
    let rate = [44100, 48000, 32000][sample_index]
        / match version {
            3 => 1,
            2 => 2,
            _ => 4,
        };
    let frame_size = (if version == 3 { 144000u64 } else { 72000 }) * bitrates[bitrate_index]
        / rate
        + u64::from((header[2] >> 1) & 1);
    ensure!(
        source.metadata()?.len().saturating_sub(start) >= frame_size,
        "Truncated MP3 audio frame"
    );
    Ok(())
}

#[derive(Clone, Copy)]
struct Atom {
    start: u64,
    size: u64,
    kind: [u8; 4],
}
fn atom(file: &mut File, start: u64, end: u64) -> Result<Atom> {
    ensure!(end.saturating_sub(start) >= 8, "Truncated MP4 atom");
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = [0; 8];
    file.read_exact(&mut bytes)?;
    let mut size = u64::from(u32::from_be_bytes(bytes[..4].try_into()?));
    let mut header = 8;
    if size == 1 {
        file.read_exact(&mut bytes[..8])?;
        size = u64::from_be_bytes(bytes);
        header = 16;
    }
    // Size-zero atoms would absorb the relocated moov. Reject instead of guessing.
    ensure!(
        size >= header && size <= end - start,
        "Invalid or open-ended MP4 atom"
    );
    file.seek(SeekFrom::Start(start + 4))?;
    let mut kind = [0; 4];
    file.read_exact(&mut kind)?;
    Ok(Atom { start, size, kind })
}

// Append a copy of moov and turn its former slot into free space. Keeping every
// media byte at its original offset preserves audio AND chapter sample tables,
// without buffering the audiobook or needing a custom chunk-offset rewriter.
// mp4ameta then only shifts bounded metadata at the end of the file.
fn trailing_moov(path: &Path, cancel: &AtomicBool) -> Result<()> {
    let mut source = OpenOptions::new().read(true).write(true).open(path)?;
    let length = source.metadata()?.len();
    let mut pos = 0;
    let mut moov = None;
    let mut has_media = false;
    while pos < length {
        check_cancel(cancel)?;
        let a = atom(&mut source, pos, length)?;
        if pos == 0 {
            ensure!(
                &a.kind == b"ftyp" && a.size <= MAX_METADATA,
                "Invalid MP4 file type atom"
            );
        }
        ensure!(
            !matches!(&a.kind, b"moof" | b"mfra" | b"sidx"),
            "Fragmented MP4 tagging is unsupported"
        );
        if &a.kind == b"moov" {
            ensure!(moov.is_none(), "Duplicate MP4 movie atom");
            moov = Some(a);
        }
        has_media |= &a.kind == b"mdat";
        pos += a.size;
    }
    let moov = moov.context("Missing MP4 movie atom")?;
    ensure!(
        moov.size <= MAX_METADATA && has_media,
        "Unsupported MP4 metadata or missing media"
    );
    if moov.start + moov.size == length {
        return Ok(());
    }
    let mut bytes = vec![0; moov.size as usize];
    source.seek(SeekFrom::Start(moov.start))?;
    source.read_exact(&mut bytes)?;
    check_cancel(cancel)?;
    source.seek(SeekFrom::End(0))?;
    source.write_all(&bytes)?;
    source.seek(SeekFrom::Start(moov.start + 4))?;
    source.write_all(b"free")?;
    source.flush()?;
    Ok(())
}

fn check_cancel(cancel: &AtomicBool) -> Result<()> {
    ensure!(
        !cancel.load(Ordering::Relaxed),
        "Cancelled while updating audiobook tags"
    );
    Ok(())
}
fn copy_cancellable(
    source: &mut impl Read,
    dest: &mut impl Write,
    cancel: &AtomicBool,
) -> Result<()> {
    let mut buffer = [0; 64 * 1024];
    loop {
        check_cancel(cancel)?;
        let count = source.read(&mut buffer)?;
        if count == 0 {
            return Ok(());
        }
        dest.write_all(&buffer[..count])?;
    }
}
fn write_mp4(path: &Path, tags: &SyncBookTags, cancel: &AtomicBool) -> Result<()> {
    let track = u16::try_from(tags.track).context("MP4 track index exceeds format limit")?;
    let total = u16::try_from(tags.total).context("MP4 track total exceeds format limit")?;
    trailing_moov(path, cancel)?;
    let read_config = mp4ameta::ReadConfig {
        read_meta_items: true,
        read_image_data: true,
        ..mp4ameta::ReadConfig::NONE
    };
    let mut tag = mp4ameta::Tag::read_with_path(path, &read_config)?;
    if let Some(v) = &tags.album {
        tag.set_album(v);
    }
    if let Some(v) = &tags.authors {
        tag.set_artist(v);
        tag.set_album_artist(v);
    }
    if let Some(v) = &tags.title {
        tag.set_title(v);
    }
    tag.set_track(track, total);
    tag.set_disc(1, 1);
    if let Some(year) = tags.year {
        tag.set_year(format!("{year:04}"));
    }
    let mut file = OpenOptions::new().read(true).write(true).open(path)?;
    let max = file
        .metadata()?
        .len()
        .checked_add(MAX_METADATA)
        .context("MP4 size overflow")?;
    let mut bounded = BoundedFile {
        file: &mut file,
        max,
        cancel,
    };
    tag.write_with(
        &mut bounded,
        &mp4ameta::WriteConfig {
            write_meta_items: true,
            ..mp4ameta::WriteConfig::NONE
        },
    )?;
    let reopened = mp4ameta::Tag::read_with_path(path, &read_config)?;
    ensure!(
        reopened.userdata == tag.userdata,
        "MP4 metadata cannot be rewritten without loss"
    );
    Ok(())
}
struct BoundedFile<'a> {
    file: &'a mut File,
    max: u64,
    cancel: &'a AtomicBool,
}
impl Read for BoundedFile<'_> {
    fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
        check_cancel(self.cancel).map_err(std::io::Error::other)?;
        self.file.read(b)
    }
}
impl Seek for BoundedFile<'_> {
    fn seek(&mut self, p: SeekFrom) -> std::io::Result<u64> {
        check_cancel(self.cancel).map_err(std::io::Error::other)?;
        self.file.seek(p)
    }
}
impl Write for BoundedFile<'_> {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        check_cancel(self.cancel).map_err(std::io::Error::other)?;
        if b.len() as u64 > self.max.saturating_sub(self.file.stream_position()?) {
            return Err(std::io::Error::other(
                "MP4 rewrite exceeds reserved capacity",
            ));
        }
        self.file.write(b)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}
impl mp4ameta::StorageFile for BoundedFile<'_> {
    fn set_len(&mut self, n: u64) -> mp4ameta::Result<()> {
        if n > self.max {
            return Err(std::io::Error::other("MP4 rewrite exceeds reserved capacity").into());
        }
        Ok(self.file.set_len(n)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn artwork() -> Vec<u8> {
        let mut encoded = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(1, 1)
            .write_to(&mut encoded, image::ImageFormat::Png)
            .unwrap();
        encoded.into_inner()
    }
    fn tags() -> SyncBookTags {
        SyncBookTags {
            album: Some("Corrected book".into()),
            authors: Some("Author One; Author Two".into()),
            title: Some("Corrected book — Part 2".into()),
            track: 2,
            total: 3,
            year: Some(2024),
        }
    }
    fn fixture(name: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/audiobookshelf/tags")
            .join(name)
    }
    fn packets(path: &Path) -> Vec<Vec<u8>> {
        ffmpeg_next::init().unwrap();
        let mut input = ffmpeg_next::format::input(path).unwrap();
        let index = input
            .streams()
            .best(ffmpeg_next::media::Type::Audio)
            .unwrap()
            .index();
        input
            .packets()
            .filter(|(s, _)| s.index() == index)
            .map(|(_, p)| p.data().unwrap().to_vec())
            .collect()
    }
    fn pcm(path: &Path) -> Vec<u8> {
        ffmpeg_next::init().unwrap();
        let mut input = ffmpeg_next::format::input(path).unwrap();
        let stream = input
            .streams()
            .best(ffmpeg_next::media::Type::Audio)
            .unwrap();
        let index = stream.index();
        let mut decoder =
            ffmpeg_next::codec::context::Context::from_parameters(stream.parameters())
                .unwrap()
                .decoder()
                .audio()
                .unwrap();
        let mut samples = Vec::new();
        let mut drain = |decoder: &mut ffmpeg_next::decoder::Audio| {
            loop {
                let mut frame = ffmpeg_next::frame::Audio::empty();
                match decoder.receive_frame(&mut frame) {
                    Ok(()) => {
                        let per_plane = frame.samples()
                            * frame.format().bytes()
                            * if frame.is_planar() {
                                1
                            } else {
                                frame.channels() as usize
                            };
                        for plane in 0..frame.planes() {
                            samples.extend_from_slice(&frame.data(plane)[..per_plane]);
                        }
                    }
                    Err(ffmpeg_next::Error::Eof) => break,
                    Err(ffmpeg_next::Error::Other { errno })
                        if errno == ffmpeg_next::error::EAGAIN =>
                    {
                        break;
                    }
                    Err(error) => panic!("Audio decode failed: {error}"),
                }
            }
        };
        for (stream, packet) in input.packets() {
            if stream.index() == index {
                decoder.send_packet(&packet).unwrap();
                drain(&mut decoder);
            }
        }
        decoder.send_eof().unwrap();
        drain(&mut decoder);
        assert!(!samples.is_empty());
        samples
    }
    #[test]
    fn sync_tags_mp3_versions_preserve_audio_artwork_chapters_and_custom_frames() {
        for version in [
            None,
            Some(id3::Version::Id3v22),
            Some(id3::Version::Id3v23),
            Some(id3::Version::Id3v24),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("staged");
            std::fs::copy(fixture("untagged.mp3"), &path).unwrap();
            if let Some(version) = version {
                let mut tag = id3::Tag::with_version(version);
                tag.set_title("Old title");
                tag.set_artist("Old author");
                tag.set_album("Old album");
                tag.set_disc(7);
                tag.set_total_discs(9);
                tag.add_frame(id3::frame::ExtendedText {
                    description: "custom".into(),
                    value: "Preserve me".into(),
                });
                tag.add_frame(id3::frame::Picture {
                    mime_type: "image/png".into(),
                    picture_type: id3::frame::PictureType::CoverFront,
                    description: "cover".into(),
                    data: artwork(),
                });
                if version != id3::Version::Id3v22 {
                    tag.add_frame(id3::frame::Chapter {
                        element_id: "chapter-one".into(),
                        start_time: 0,
                        end_time: 250,
                        start_offset: u32::MAX,
                        end_offset: u32::MAX,
                        frames: vec![],
                    });
                }
                tag.write_to_path(&path, version).unwrap();
            }
            let before = id3::Tag::read_from_path(&path).ok();
            let audio = packets(&path);
            let decoded = pcm(&path);
            apply(&path, "mp3", &tags()).unwrap();
            let after = id3::Tag::read_from_path(&path).unwrap();
            assert_eq!(after.version(), version.unwrap_or(id3::Version::Id3v23));
            assert_eq!(after.album(), Some("Corrected book"));
            assert_eq!(after.artist(), Some("Author One; Author Two"));
            assert_eq!(after.album_artist(), after.artist());
            assert_eq!(after.track(), Some(2));
            assert_eq!(after.total_tracks(), Some(3));
            assert_eq!(after.disc(), Some(1));
            assert_eq!(after.total_discs(), Some(1));
            if let Some(before) = before {
                assert_eq!(
                    before.pictures().collect::<Vec<_>>(),
                    after.pictures().collect::<Vec<_>>()
                );
                assert_eq!(
                    before.chapters().collect::<Vec<_>>(),
                    after.chapters().collect::<Vec<_>>()
                );
                assert_eq!(
                    before.extended_texts().collect::<Vec<_>>(),
                    after.extended_texts().collect::<Vec<_>>()
                );
            }
            assert_eq!(audio, packets(&path));
            assert_eq!(decoded, pcm(&path));
        }
    }
    #[test]
    fn sync_tags_mp4_faststart_and_trailing_preserve_chapter_tracks_lists_and_audio() {
        for trailing in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("staged");
            std::fs::copy(fixture("chapters-faststart.m4a"), &path).unwrap();
            let mut tag = mp4ameta::Tag::read_from_path(&path).unwrap();
            assert_eq!(tag.chapter_list().len(), 2);
            assert_eq!(tag.chapter_track().len(), 2);
            tag.set_artwork(mp4ameta::Img::png(artwork()));
            tag.set_data(
                mp4ameta::FreeformIdent::new_static("org.hifimule", "custom"),
                mp4ameta::Data::Utf8("preserved".into()),
            );
            tag.write_with_path(
                &path,
                &mp4ameta::WriteConfig {
                    write_meta_items: true,
                    ..mp4ameta::WriteConfig::NONE
                },
            )
            .unwrap();
            if trailing {
                trailing_moov(&path, &AtomicBool::new(false)).unwrap();
            }
            let before = mp4ameta::Tag::read_from_path(&path).unwrap();
            let audio = packets(&path);
            let decoded = pcm(&path);
            let size = apply(&path, "m4b", &tags()).unwrap();
            assert_eq!(size, std::fs::metadata(&path).unwrap().len());
            let after = mp4ameta::Tag::read_from_path(&path).unwrap();
            assert_eq!(after.album(), Some("Corrected book"));
            assert_eq!(after.artist(), Some("Author One; Author Two"));
            assert_eq!(after.title(), tags().title.as_deref());
            assert_eq!(after.track(), (Some(2), Some(3)));
            assert_eq!(after.disc(), (Some(1), Some(1)));
            assert_eq!(after.year(), Some("2024"));
            assert_eq!(
                after
                    .strings_of(&mp4ameta::FreeformIdent::new_static(
                        "org.hifimule",
                        "custom"
                    ))
                    .next(),
                Some("preserved")
            );
            assert_eq!(before.chapter_list(), after.chapter_list());
            assert_eq!(before.chapter_track(), after.chapter_track());
            assert_eq!(
                before.artworks().collect::<Vec<_>>(),
                after.artworks().collect::<Vec<_>>()
            );
            assert_eq!(audio, packets(&path));
            assert_eq!(decoded, pcm(&path));
        }
    }
    #[test]
    fn sync_tags_cancellation_stops_copy_and_preserves_source() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("staged");
        let original = std::fs::read(fixture("untagged.mp3")).unwrap();
        std::fs::write(&path, &original).unwrap();
        let cancelled = AtomicBool::new(true);
        assert!(apply_cancellable(&path, "mp3", &tags(), &cancelled).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        struct CancellingReader<'a> {
            flag: &'a AtomicBool,
        }
        impl Read for CancellingReader<'_> {
            fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
                self.flag.store(true, Ordering::Relaxed);
                bytes[0] = 7;
                Ok(1)
            }
        }
        cancelled.store(false, Ordering::Relaxed);
        let mut copied = Vec::new();
        assert!(
            copy_cancellable(
                &mut CancellingReader { flag: &cancelled },
                &mut copied,
                &cancelled
            )
            .is_err()
        );
        assert_eq!(copied, vec![7]);
    }

    #[test]
    fn sync_tags_rejects_unsafe_chapter_offsets_and_mp4_layouts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("staged");
        std::fs::copy(fixture("untagged.mp3"), &path).unwrap();
        let mut tag = id3::Tag::new();
        tag.add_frame(id3::frame::Chapter {
            element_id: "one".into(),
            start_time: 0,
            end_time: 250,
            start_offset: 128,
            end_offset: 256,
            frames: vec![],
        });
        tag.write_to_path(&path, id3::Version::Id3v24).unwrap();
        let original = std::fs::read(&path).unwrap();
        assert!(
            apply(&path, "mp3", &tags())
                .unwrap_err()
                .to_string()
                .contains("absolute byte offsets")
        );
        assert_eq!(std::fs::read(&path).unwrap(), original);
        for kind in [b"moof", b"mfra", b"sidx"] {
            let mut bytes = std::fs::read(fixture("chapters-faststart.m4a")).unwrap();
            bytes.extend_from_slice(&8u32.to_be_bytes());
            bytes.extend_from_slice(kind);
            std::fs::write(&path, &bytes).unwrap();
            assert!(
                apply(&path, "m4a", &tags())
                    .unwrap_err()
                    .to_string()
                    .contains("Fragmented")
            );
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
        assert!(rewrite_reservation(u64::MAX).is_err());
    }

    #[test]
    fn sync_tags_rejects_invalid_mpeg_and_oversized_or_compressed_id3_frames() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("staged");
        for bytes in [
            vec![0xff, 0xe0, 0, 0, 0, 0, 0, 0, 0, 0],
            vec![0xff, 0xfb, 0x90, 0, 0, 0, 0, 0, 0, 0],
        ] {
            std::fs::write(&path, &bytes).unwrap();
            assert!(apply(&path, "mp3", &tags()).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
        for frame in [
            b"APIC\xff\xff\xff\xff\x00\x00".to_vec(),
            b"APIC\x00\x00\x00\x00\x00\x80".to_vec(),
        ] {
            let mut bytes = b"ID3\x03\x00\x00\x00\x00\x00\x0a".to_vec();
            bytes.extend_from_slice(&frame);
            std::fs::write(&path, &bytes).unwrap();
            assert!(apply(&path, "mp3", &tags()).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
        let mut chapter = b"CHAP\x00\x00\x00\x1c\x00\x00x\x00".to_vec();
        chapter.extend_from_slice(&[0; 16]);
        chapter.extend_from_slice(b"APIC\xff\xff\xff\xff\x00\x00");
        assert!(validate_id3_frames(&chapter, 3, 0).is_err());
    }

    #[test]
    fn sync_tags_preserves_optional_id3v1_fields_when_creating_v23() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("staged");
        let mut bytes = std::fs::read(fixture("untagged.mp3")).unwrap();
        let mut legacy = [0u8; 128];
        legacy[..3].copy_from_slice(b"TAG");
        legacy[33..39].copy_from_slice(b"Author");
        legacy[93..97].copy_from_slice(b"1998");
        bytes.extend_from_slice(&legacy);
        std::fs::write(&path, &bytes).unwrap();
        let mut values = tags();
        values.authors = None;
        values.year = None;
        apply(&path, "mp3", &values).unwrap();
        let after = id3::Tag::read_from_path(&path).unwrap();
        assert_eq!(after.version(), id3::Version::Id3v23);
        assert_eq!(after.artist(), Some("Author"));
        assert_eq!(after.year(), Some(1998));
    }

    #[test]
    fn sync_tags_sparse_fields_preserve_existing_and_malformed_fails() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("staged");
        std::fs::copy(fixture("untagged.mp3"), &path).unwrap();
        apply(&path, "mp3", &tags()).unwrap();
        let mut sparse = tags();
        sparse.authors = None;
        sparse.album = None;
        sparse.title = None;
        sparse.year = None;
        apply(&path, "mp3", &sparse).unwrap();
        let tag = id3::Tag::read_from_path(&path).unwrap();
        assert_eq!(tag.artist(), tags().authors.as_deref());
        assert_eq!(tag.year(), Some(2024));
        std::fs::write(&path, b"ID3\x04\x00\x00\x00\x00\x7f\x7fbroken").unwrap();
        assert!(apply(&path, "mp3", &tags()).is_err());
        assert!(apply(&path, "m4a", &tags()).is_err());
        assert!(apply(&path, "flac", &tags()).is_err());
    }
}
