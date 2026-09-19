//! Decoder + boundary regression evidence, not native/physical-output evidence.
//! The independent reference is the FFmpeg CLI with exactly the linked component
//! versions. Set HIFIMULE_TEST_FFMPEG to its executable when it is not on PATH.
use super::*;
use crate::playback::{output::BoundaryPcmConsumer, streaming::BoundedHttpReader};
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
};

fn reference_cli() -> &'static Path {
    static CLI: OnceLock<PathBuf> = OnceLock::new();
    CLI.get_or_init(|| {
        let path = std::env::var_os("HIFIMULE_TEST_FFMPEG")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("ffmpeg"));
        let result = Command::new(&path)
            .arg("-version")
            .output()
            .expect("continuity tests require FFmpeg CLI; set HIFIMULE_TEST_FFMPEG");
        assert!(result.status.success());
        let version = String::from_utf8(result.stdout).unwrap();
        let components = unsafe {
            [
                ("libavcodec", ffmpeg::ffi::avcodec_version()),
                ("libavformat", ffmpeg::ffi::avformat_version()),
                ("libavutil", ffmpeg::ffi::avutil_version()),
                ("libswresample", ffmpeg::ffi::swresample_version()),
            ]
        };
        for (name, linked) in components {
            let line = version
                .lines()
                .find(|line| line.starts_with(name))
                .unwrap_or_else(|| panic!("missing {name} identity in {version}"));
            let actual: String = line.split('/').next().unwrap()[name.len()..]
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect();
            let expected = format!("{}.{}.{}", linked >> 16, (linked >> 8) & 255, linked & 255);
            assert_eq!(
                actual, expected,
                "reference {name} must match the linked production runtime"
            );
        }
        eprintln!("continuity reference: {version}");
        path
    })
    .as_path()
}

fn reference(path: &Path, rate: u32, channels: u16) -> Vec<f32> {
    let result = Command::new(reference_cli())
        .args(["-nostdin", "-v", "error", "-xerror", "-i"])
        .arg(path)
        .args([
            "-map",
            "0:a:0",
            "-ar",
            &rate.to_string(),
            "-ac",
            &channels.to_string(),
            "-c:a",
            "pcm_f32le",
            "-f",
            "f32le",
            "pipe:1",
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}: {}",
        path.display(),
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(result.stdout.len() % 4, 0);
    result
        .stdout
        .chunks_exact(4)
        .map(|v| f32::from_le_bytes(v.try_into().unwrap()))
        .collect()
}

fn production(path: &Path, rate: u32, channels: u16) -> anyhow::Result<Vec<f32>> {
    let cancel = Arc::new(AtomicBool::new(false));
    let reader = BoundedHttpReader::from_source(
        std::fs::File::open(path)?,
        cancel.clone(),
        Arc::new(AtomicU64::new(0)),
    );
    collect_production(reader, path, rate, channels, cancel)
}

fn collect_production(
    reader: BoundedHttpReader,
    path: &Path,
    rate: u32,
    channels: u16,
    cancel: Arc<AtomicBool>,
) -> anyhow::Result<Vec<f32>> {
    // A small ring exercises producer backpressure; the test oracle may retain
    // complete fixtures, but production decode still uses its bounded reader/ring.
    let pcm = Arc::new(ArrayQueue::new(4096));
    let done = Arc::new(AtomicBool::new(false));
    let consumer_pcm = pcm.clone();
    let consumer_done = done.clone();
    let consumer = std::thread::spawn(move || {
        let mut samples = Vec::new();
        while !consumer_done.load(Ordering::Acquire) || !consumer_pcm.is_empty() {
            if let Some(sample) = consumer_pcm.pop() {
                samples.push(sample);
            } else {
                std::thread::yield_now();
            }
        }
        samples
    });
    let summary = decode_stream(
        reader,
        path.file_name().and_then(|s| s.to_str()),
        rate,
        channels,
        0,
        pcm,
        cancel,
    );
    done.store(true, Ordering::Release);
    let samples = consumer.join().unwrap();
    if let Err(error) = &summary {
        if format!("{error:#}").contains("unsupported") {
            assert!(samples.is_empty(), "unsupported layout must not emit audio");
        }
    }
    let summary = summary?;
    assert_eq!(
        samples.len() as u64,
        summary.emitted_frames * u64::from(channels)
    );
    assert_eq!(
        summary.frames, summary.emitted_frames,
        "no application-level padding guesses"
    );
    Ok(samples)
}

fn assert_samples(actual: &[f32], expected: &[f32], label: &str) {
    assert_eq!(
        actual.len(),
        expected.len(),
        "{label}: missing/extra frames"
    );
    for (index, (&actual, &expected)) in actual.iter().zip(expected).enumerate() {
        assert!(
            actual.is_finite() && (actual - expected).abs() <= 2.0e-6,
            "{label}: sample {index}: {actual} != {expected}"
        );
    }
}

fn join_at_boundary(a: &[f32], b: &[f32], channels: u16) -> Vec<f32> {
    let qa = ArrayQueue::new(a.len().max(1));
    let qb = ArrayQueue::new(b.len().max(1));
    for &sample in a {
        qa.push(sample).unwrap();
    }
    for &sample in b {
        qb.push(sample).unwrap();
    }
    let mut consumer =
        BoundaryPcmConsumer::new(usize::from(channels), 4800 * usize::from(channels));
    let mut actual = vec![f32::NAN; a.len() + b.len()];
    // One span deliberately crosses the boundary, including short predecessors.
    let rendered = consumer.render(&mut actual, &qa, true, true, &qb, true, true, true);
    assert_eq!(
        rendered.boundary_frame,
        Some(a.len() / usize::from(channels))
    );
    assert_eq!(rendered.rendered.samples as usize, actual.len());
    assert!(qa.is_empty() && qb.is_empty());
    actual
}

#[test]
fn continuity_six_format_adjacent_tracks_match_independent_drained_references() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../experiments/playback-probe/fixtures");
    for suffix in ["wav", "flac", "m4a", "mp3", "aac.m4a", "opus"] {
        let declared_frames = [96_017, 72_011, 120_013];
        let mut decoded = Vec::new();
        let mut expected = Vec::new();
        for (index, frames) in declared_frames.into_iter().enumerate() {
            let path = root.join(format!("track-{}.{suffix}", index + 1));
            let actual = production(&path, 48_000, 2).unwrap();
            let reference = reference(&path, 48_000, 2);
            assert_eq!(
                reference.len(),
                frames * 2,
                "{suffix}: declared padding/duration"
            );
            assert_samples(&actual, &reference, &path.display().to_string());
            decoded.push(actual);
            expected.push(reference);
        }
        for index in 0..2 {
            let actual = join_at_boundary(&decoded[index], &decoded[index + 1], 2);
            let reference = [expected[index].as_slice(), expected[index + 1].as_slice()].concat();
            assert_samples(&actual, &reference, suffix);
        }
    }
}

fn wav(path: &Path, rate: u32, channels: u16, samples: &[i16]) {
    assert_eq!(samples.len() % usize::from(channels), 0);
    let data_size = (samples.len() * 2) as u32;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_size).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&channels.to_le_bytes());
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&(rate * u32::from(channels) * 2).to_le_bytes());
    bytes.extend_from_slice(&(channels * 2).to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_size.to_le_bytes());
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    std::fs::write(path, bytes).unwrap();
}

fn marked_samples(frames: usize, channels: u16) -> Vec<i16> {
    (0..frames)
        .flat_map(|frame| {
            (0..channels).map(move |channel| {
                // Known leading/trailing/internal silence plus channel-specific markers.
                if frame < 17 || frame >= frames - 23 || (101..119).contains(&frame) {
                    0
                } else {
                    (((frame * 173 + usize::from(channel) * 2371) % 12001) as i16) - 6000
                }
            })
        })
        .collect()
}

#[test]
fn continuity_short_tracks_preserve_known_silence_and_boundary_markers() {
    let dir = tempfile::tempdir().unwrap();
    let mut decoded = Vec::new();
    let mut expected = Vec::new();
    for (index, frames) in [257, 389].into_iter().enumerate() {
        let path = dir.path().join(format!("{index}.wav"));
        let samples = marked_samples(frames, 2);
        wav(&path, 48_000, 2, &samples);
        let actual = production(&path, 48_000, 2).unwrap();
        let source: Vec<f32> = samples.iter().map(|&s| f32::from(s) / 32768.0).collect();
        assert_eq!(
            actual, source,
            "same-rate PCM preserves every intentional zero exactly"
        );
        decoded.push(actual);
        expected.extend(source);
    }
    assert_eq!(join_at_boundary(&decoded[0], &decoded[1], 2), expected);
}

#[test]
fn continuity_mixed_rate_and_layout_match_individually_drained_conversions() {
    let dir = tempfile::tempdir().unwrap();
    for (output_rate, output_channels) in [(48_000, 2), (44_100, 1)] {
        let mut actual = Vec::new();
        let mut expected = Vec::new();
        for (index, rate, channels, frames) in [(0, 44_100, 1, 3001), (1, 48_000, 2, 4003)] {
            let path = dir.path().join(format!("{index}.wav"));
            wav(&path, rate, channels, &marked_samples(frames, channels));
            let decoded = production(&path, output_rate, output_channels).unwrap();
            let reference = reference(&path, output_rate, output_channels);
            assert_samples(
                &decoded,
                &reference,
                "individual conversion with filter drain",
            );
            actual.push(decoded);
            expected.extend(reference);
        }
        assert_samples(
            &join_at_boundary(&actual[0], &actual[1], output_channels),
            &expected,
            "mixed boundary",
        );
    }
}

#[test]
fn continuity_unsupported_layout_fails_without_emitting_audio() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("three-channel.wav");
    wav(&path, 48_000, 3, &marked_samples(257, 3));
    let error = production(&path, 48_000, 2)
        .err()
        .expect("unsupported source must fail");
    assert!(format!("{error:#}").contains("unsupported input layout"));
    let path = dir.path().join("mono.wav");
    wav(&path, 48_000, 1, &marked_samples(257, 1));
    let error = production(&path, 48_000, 3)
        .err()
        .expect("unsupported target must fail");
    assert!(format!("{error:#}").contains("unsupported output layout"));
}

#[test]
fn continuity_undeclared_aac_padding_and_itunes_tags_never_authorize_extra_trim() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.wav");
    let mut samples = vec![0; 4410 * 2];
    samples.extend(marked_samples(4001, 2));
    wav(&source, 44_100, 2, &samples);
    for (name, options) in [
        ("untimed.m4a", vec!["-use_editlist", "0"]),
        (
            "tagged.m4a",
            vec![
                "-use_editlist",
                "0",
                "-movflags",
                "use_metadata_tags",
                "-metadata",
                "iTunNORM=00000000 00000000",
            ],
        ),
        ("raw.aac", vec!["-f", "adts"]),
    ] {
        let path = dir.path().join(name);
        let result = Command::new(reference_cli())
            .args(["-nostdin", "-v", "error", "-i"])
            .arg(&source)
            .args(["-c:a", "aac"])
            .args(options)
            .arg(&path)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let actual = production(&path, 48_000, 2).unwrap();
        let expected = reference(&path, 48_000, 2);
        assert_samples(&actual, &expected, name);
        assert!(
            actual.len() / 2 > 9000,
            "undeclared padding must not shorten decoded audio"
        );
        assert!(
            actual[..4000].iter().all(|sample| sample.abs() < 1.0e-6),
            "intentional leading silence"
        );
    }
}

#[test]
fn continuity_cancelled_decoder_releases_a_full_pcm_queue() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../experiments/playback-probe/fixtures/track-1.flac");
    let cancel = Arc::new(AtomicBool::new(false));
    let reader = BoundedHttpReader::from_source(
        std::fs::File::open(&path).unwrap(),
        cancel.clone(),
        Arc::new(AtomicU64::new(0)),
    );
    let pcm = Arc::new(ArrayQueue::new(2));
    let worker_pcm = pcm.clone();
    let worker_cancel = cancel.clone();
    let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        let result = decode_stream(
            reader,
            Some("track.flac"),
            48_000,
            2,
            0,
            worker_pcm,
            worker_cancel,
        );
        result_tx.send(result.map(|_| ())).unwrap();
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while pcm.len() < 2 && std::time::Instant::now() < deadline {
        std::thread::yield_now();
    }
    let was_full = pcm.len() == 2;
    cancel.store(true, Ordering::Release);
    let result = result_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("cancelled decoder must leave a full queue without waiting for consumption");
    worker.join().unwrap();
    assert!(was_full, "fixture must reach PCM backpressure");
    assert!(format!("{:#}", result.unwrap_err()).contains("cancelled"));
    assert_eq!(pcm.len(), 2, "cancellation cannot consume queued PCM");
}

#[test]
fn continuity_unknown_length_mp3_preserves_streamed_decode_without_certifying_padding() {
    // FFmpeg 9 cannot validate terminal Xing padding without a source byte size.
    // Keep ordinary streaming intact; do not infer a trim from provider duration.
    struct UnknownLength(std::fs::File);
    impl std::io::Read for UnknownLength {
        fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
            std::io::Read::read(&mut self.0, output)
        }
    }
    impl std::io::Seek for UnknownLength {
        fn seek(&mut self, from: std::io::SeekFrom) -> std::io::Result<u64> {
            std::io::Seek::seek(&mut self.0, from)
        }
    }
    impl crate::playback::streaming::CompressedSource for UnknownLength {}
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../experiments/playback-probe/fixtures/track-1.mp3");
    let cancel = Arc::new(AtomicBool::new(false));
    let reader = BoundedHttpReader::from_source(
        UnknownLength(std::fs::File::open(&path).unwrap()),
        cancel.clone(),
        Arc::new(AtomicU64::new(0)),
    );
    let actual = collect_production(reader, &path, 48_000, 2, cancel).unwrap();
    let output = Command::new(reference_cli())
        .args([
            "-nostdin",
            "-v",
            "error",
            "-i",
            "pipe:0",
            "-ar",
            "48000",
            "-ac",
            "2",
            "-c:a",
            "pcm_f32le",
            "-f",
            "f32le",
            "pipe:1",
        ])
        .stdin(std::fs::File::open(&path).unwrap())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected: Vec<f32> = output
        .stdout
        .chunks_exact(4)
        .map(|v| f32::from_le_bytes(v.try_into().unwrap()))
        .collect();
    assert_samples(&actual, &expected, "unknown-length MP3 ordinary playback");
    assert!(
        actual.len() > 96_017 * 2,
        "limitation must remain explicit until metadata interpretation is fixed"
    );
}

#[test]
fn continuity_legacy_four_marker_counterexample_keeps_all_undeclared_samples() {
    let dir = tempfile::tempdir().unwrap();
    let raw = dir.path().join("silence.aac");
    // Four valid AAC-LC stereo silent frames. No encoder-delay declaration is
    // present. These are source content, regardless of their packet signature.
    let mut adts = Vec::new();
    let filler = [0x20, 0x00, 0x20, 0x00, 0x00, 0x80, 0x0e];
    for _ in 0..4 {
        adts.extend_from_slice(&[0xff, 0xf1, 0x50, 0x80, 0x01, 0xdf, 0xfc]);
        adts.extend_from_slice(&filler);
    }
    std::fs::write(&raw, adts).unwrap();
    let path = dir.path().join("tagged-silence.m4a");
    let output = Command::new(reference_cli())
        .args(["-nostdin", "-v", "error", "-i"])
        .arg(&raw)
        .args([
            "-c:a",
            "copy",
            "-use_editlist",
            "0",
            "-movflags",
            "use_metadata_tags",
            "-metadata",
            "iTunNORM=00000000 00000000",
        ])
        .arg(&path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut input = ffmpeg::format::input(&path).unwrap();
    assert!(input.format().name().split(',').any(|name| name == "mov"));
    assert!(input.metadata().get("iTunNORM").is_some());
    let stream = input.streams().best(ffmpeg::media::Type::Audio).unwrap();
    assert_eq!(stream.parameters().id(), ffmpeg::codec::Id::AAC);
    let mut packet = ffmpeg::Packet::empty();
    packet.read(&mut input).unwrap();
    assert_eq!(packet.data().unwrap(), filler);
    let actual = production(&path, 44_100, 2).unwrap();
    assert_eq!(
        actual.len(),
        4096 * 2,
        "all four matching markers still cannot authorize discarding 2112 frames"
    );
    assert_samples(
        &actual,
        &reference(&path, 44_100, 2),
        "legacy marker counterexample",
    );
    assert!(actual.iter().all(|&sample| sample == 0.0));
}

#[test]
fn continuity_missing_or_unrecognized_mp3_padding_is_not_guessed() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../experiments/playback-probe/fixtures/track-1.mp3");
    let original = std::fs::read(fixture).unwrap();
    let info = original.windows(4).position(|v| v == b"Info").unwrap();
    let flags = u32::from_be_bytes(original[info + 4..info + 8].try_into().unwrap());
    let encoder = info
        + 8
        + if flags & 1 != 0 { 4 } else { 0 }
        + if flags & 2 != 0 { 4 } else { 0 }
        + if flags & 4 != 0 { 100 } else { 0 }
        + if flags & 8 != 0 { 4 } else { 0 };
    assert_eq!(&original[encoder..encoder + 4], b"Lavc");
    for missing in [true, false] {
        let mut bytes = original.clone();
        if missing {
            bytes[info..info + 4].copy_from_slice(b"Junk");
        } else {
            bytes[encoder..encoder + 9].copy_from_slice(b"unknown00");
        }
        let path = dir.path().join(format!("padding-missing-{missing}.mp3"));
        std::fs::write(&path, bytes).unwrap();
        let actual = production(&path, 48_000, 2).unwrap();
        let expected = reference(&path, 48_000, 2);
        assert_samples(&actual, &expected, "MP3 without validated encoder padding");
        assert!(
            actual.len() > 96_017 * 2,
            "padding may not be inferred from the original fixture length"
        );
    }
}

fn production_gain(
    path: &Path,
    rate: u32,
    channels: u16,
    start_frame: u64,
    gain: f32,
    suffix: &str,
) -> Vec<f32> {
    let cancel = Arc::new(AtomicBool::new(false));
    let reader = BoundedHttpReader::from_source(
        std::fs::File::open(path).unwrap(),
        cancel.clone(),
        Arc::new(AtomicU64::new(0)),
    );
    let pcm = Arc::new(ArrayQueue::new(4096));
    let done = Arc::new(AtomicBool::new(false));
    let drain = pcm.clone();
    let finished = done.clone();
    let consumer = std::thread::spawn(move || {
        let mut samples = Vec::new();
        while !finished.load(Ordering::Acquire) || !drain.is_empty() {
            if let Some(sample) = drain.pop() {
                samples.push(sample);
            } else {
                std::thread::yield_now();
            }
        }
        samples
    });
    let result = decode_stream_with_seek_and_gain(
        reader,
        path.file_name().and_then(|v| v.to_str()),
        rate,
        channels,
        start_frame,
        None,
        false,
        None,
        None,
        None,
        gain,
        Some(suffix),
        pcm,
        cancel,
    );
    done.store(true, Ordering::Release);
    let samples = consumer.join().unwrap();
    assert_eq!(
        result.unwrap().emitted_frames * u64::from(channels),
        samples.len() as u64
    );
    samples
}

// Exercise the actual adapter and admission plan, file-backed persistence, both
// occurrence preparations, production decode and native boundary/replay consumer.
// This is digital evidence; it does not claim physical output or device coverage.
#[tokio::test]
async fn album_gain_adapter_admission_restart_decode_boundary_and_replay() {
    use crate::playback::{
        PlaybackSession,
        album::prepare_album,
        model::*,
        output::{SubmittedFrame, SubmittedTail},
        session::AlbumAdmission,
    };
    use crate::providers::{
        CredentialKind, MediaProvider, ProviderCredentials, subsonic::SubsonicProvider,
    };
    use uuid::Uuid;
    let dir = tempfile::tempdir().unwrap();
    let mut files = Vec::new();
    let mut references = Vec::new();
    for (index, amplitude) in [8192i16, 16384].into_iter().enumerate() {
        let input = dir.path().join(format!("{index}.wav"));
        let samples: Vec<i16> = (0..4003)
            .flat_map(|frame| {
                let v = match frame % 4 {
                    0 => 0,
                    1 => amplitude,
                    2 => -amplitude,
                    _ => 0,
                };
                [v, v]
            })
            .collect();
        wav(&input, 48_000, 2, &samples);
        references.push(
            samples
                .iter()
                .map(|&v| f32::from(v) / 32768.0)
                .collect::<Vec<_>>(),
        );
        let output = dir.path().join(format!("{index}.flac"));
        let result = Command::new(reference_cli())
            .args(["-nostdin", "-v", "error", "-i"])
            .arg(&input)
            .args([
                "-c:a",
                "flac",
                "-metadata",
                "REPLAYGAIN_ALBUM_GAIN=20 dB",
                "-metadata",
                "REPLAYGAIN_TRACK_GAIN=-40 dB",
                "-metadata",
                "REPLAYGAIN_ALBUM_PEAK=10",
            ])
            .arg(&output)
            .output()
            .unwrap();
        assert!(result.status.success());
        files.push(output);
    }
    for gain_db in [-6.0f64, 0.0, 6.0] {
        let mut server = mockito::Server::new_async().await;
        let mock = server.mock("GET", "/rest/getAlbum.view")
            .match_query(mockito::Matcher::Any)
            .with_header("content-type", "application/json")
            .with_body(serde_json::json!({"subsonic-response": {"status":"ok", "openSubsonic":true,
                "album":{"id":"album", "name":"Album", "songCount":2,
                    "song":[
                        {"id":"loud", "title":"Loud", "albumId":"album", "track":2, "suffix":"flac",
                         "replayGain":{"albumGain":gain_db, "albumPeak":0.5}},
                        {"id":"quiet", "title":"Quiet", "albumId":"album", "track":1, "suffix":"flac",
                         "replayGain":{"albumGain":gain_db, "albumPeak":0.5}}
                    ]}}}).to_string()).create_async().await;
        let provider = SubsonicProvider::from_stored_config(
            ProviderCredentials {
                server_url: server.url(),
                credential: CredentialKind::Password {
                    username: "fixture".into(),
                    password: "fixture".into(),
                },
            },
            true,
            None,
        )
        .unwrap();
        let source = AlbumSource {
            server_id: "server".into(),
            album_id: "album".into(),
        };
        let plan = prepare_album(provider.get_album("album").await.unwrap(), &source).unwrap();
        mock.assert_async().await;
        let expected_scalar = 10f64
            .powf(gain_db / 20.0)
            .min(10f64.powf(-1.0 / 20.0) / 0.5);
        assert!((f64::from(plan.policy.scalar()) - expected_scalar).abs() < 1e-7);
        let db_path = dir.path().join(format!("{gain_db}.sqlite"));
        let db = Arc::new(crate::db::Database::new(db_path.clone()).unwrap());
        let owner = PlaybackSession::restore(db.clone(), Uuid::new_v4().to_string());
        let before = owner.snapshot().unwrap();
        let admission = owner
            .reserve_album(
                PlayAlbumParams {
                    schema_version: 1,
                    instance_id: before.instance_id,
                    session_id: before.session_id,
                    command_id: Uuid::new_v4().to_string(),
                    expected_queue_revision: before.queue_revision,
                    expected_generation_id: before.generation_id,
                    source,
                },
                None,
            )
            .unwrap();
        let AlbumAdmission::Resolve(reservation) = admission else {
            panic!("fresh album");
        };
        let initial = owner
            .commit_album_with_policy(reservation, plan.sources, plan.policy, plan.representations)
            .unwrap();
        assert_eq!(initial.current.as_ref().unwrap().source.track_id, "quiet");
        let candidate = owner
            .successor_candidate(&initial.generation_id, initial.resume_epoch)
            .unwrap();
        assert_eq!(candidate.successor.source.track_id, "loud");
        assert_eq!(candidate.gain_bits, initial.gain_bits);
        owner.stop_and_join().unwrap();
        drop(owner);
        drop(db);
        let db = Arc::new(crate::db::Database::new(db_path).unwrap());
        let restored = PlaybackSession::restore(db, Uuid::new_v4().to_string());
        let snapshot = restored.snapshot().unwrap();
        assert_eq!(snapshot.state, TransportState::Paused);
        assert_eq!(snapshot.gain_bits, initial.gain_bits);
        assert_eq!(snapshot.qualified_suffix, initial.qualified_suffix);
        let gain = f32::from_bits(snapshot.gain_bits);
        let decoded: Vec<_> = files
            .iter()
            .enumerate()
            .map(|(index, file)| {
                let bits = if index == 0 {
                    snapshot.gain_bits
                } else {
                    candidate.gain_bits
                };
                let actual = production_gain(file, 48_000, 2, 0, f32::from_bits(bits), "flac");
                let expected: Vec<_> = references[index]
                    .iter()
                    .map(|&v| (f64::from(v) * expected_scalar) as f32)
                    .collect();
                assert_samples(
                    &actual,
                    &expected,
                    "admitted FLAC gain with conflicting embedded tags",
                );
                assert!(
                    actual
                        .iter()
                        .all(|v| v.abs() <= 10f32.powf(-1.0 / 20.0) + 2e-6)
                );
                let sought = production_gain(file, 48_000, 2, 123, gain, "flac");
                assert_samples(&sought, &actual[246..], "non-unity seek discard");
                actual
            })
            .collect();
        assert!((decoded[1][2] / decoded[0][2] - 2.0).abs() < 1e-6);
        let expected = [decoded[0].as_slice(), decoded[1].as_slice()].concat();
        assert_samples(
            &join_at_boundary(&decoded[0], &decoded[1], 2),
            &expected,
            "frozen album boundary",
        );
        let qa = ArrayQueue::new(decoded[0].len());
        let qb = ArrayQueue::new(decoded[1].len());
        for &v in &decoded[0] {
            qa.push(v).unwrap();
        }
        for &v in &decoded[1] {
            qb.push(v).unwrap();
        }
        let replay = ArrayQueue::<SubmittedFrame>::new(16);
        let tail = SubmittedTail::new(16);
        let mut consumer = BoundaryPcmConsumer::new(2, 2);
        let mut first = vec![0.0; decoded[0].len() + 8];
        let rendered = consumer.render_with_tail(
            &mut first, &qa, true, true, &qb, true, true, &replay, &tail, true,
        );
        assert_eq!(rendered.boundary_frame, Some(decoded[0].len() / 2));
        assert_eq!(first, expected[..first.len()]);
        tail.reconcile(4, &replay).unwrap();
        let mut paused = [1.0; 2];
        consumer.render_with_tail(
            &mut paused,
            &qa,
            true,
            true,
            &qb,
            true,
            true,
            &replay,
            &tail,
            false,
        );
        assert_eq!(paused, [0.0; 2]);
        let mut partial = [0.0; 2];
        consumer.render_with_tail(
            &mut partial,
            &qa,
            true,
            true,
            &qb,
            true,
            true,
            &replay,
            &tail,
            true,
        );
        assert_eq!(partial, decoded[1][..2]);
        // Pause again before the just-resubmitted frame is played.
        tail.reconcile(1, &replay).unwrap();
        let mut resumed = [0.0; 8];
        consumer.render_with_tail(
            &mut resumed,
            &qa,
            true,
            true,
            &qb,
            true,
            true,
            &replay,
            &tail,
            true,
        );
        assert_eq!(
            resumed,
            decoded[1][..8],
            "replay retains one multiplication, never gain squared"
        );
        restored.stop_and_join().unwrap();
    }
}

#[test]
fn album_gain_converted_tails_match_independent_reference() {
    let dir = tempfile::tempdir().unwrap();
    for (rate, channels) in [(44_100, 1), (48_000, 2)] {
        let path = dir.path().join(format!("{rate}.wav"));
        wav(&path, rate, channels, &marked_samples(4003, channels));
        for (out_rate, out_channels) in [(48_000, 2), (44_100, 1)] {
            let independent = reference(&path, out_rate, out_channels);
            for gain in [0.5, 1.5] {
                let actual = production_gain(&path, out_rate, out_channels, 0, gain, "wav");
                let expected: Vec<_> = independent.iter().map(|v| v * gain).collect();
                assert_samples(&actual, &expected, "gain after conversion including drain");
                eprintln!(
                    "gain conversion {rate}/{channels} -> {out_rate}/{out_channels}: frames={}, peak={}",
                    actual.len() / usize::from(out_channels),
                    actual.iter().fold(0.0f32, |m, v| m.max(v.abs()))
                );
            }
        }
    }
}
