use anyhow::{bail, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_queue::ArrayQueue;
use serde_json::json;
use std::{
    fs::{File, OpenOptions},
    io::{BufRead, BufWriter, Write},
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};
use symphonia::core::{
    audio::SampleBuffer, codecs::DecoderOptions, errors::Error, formats::FormatOptions,
    io::MediaSourceStream, meta::MetadataOptions, probe::Hint,
};

#[cfg(feature = "native-ffmpeg")]
mod ffmpeg_backend;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Backend {
    Symphonia,
    NativeFfmpeg,
}
impl Backend {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "symphonia" => Ok(Self::Symphonia),
            "native-ffmpeg" => {
                if !cfg!(feature = "native-ffmpeg") {
                    bail!("native-ffmpeg requires a build with --features native-ffmpeg");
                }
                Ok(Self::NativeFfmpeg)
            }
            _ => bail!("unknown decoder {value}"),
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Symphonia => "symphonia",
            Self::NativeFfmpeg => "native-ffmpeg",
        }
    }
    fn info(self) -> serde_json::Value {
        match self {
            Self::Symphonia => json!({"decoder":"symphonia","binding_version":"0.5.5"}),
            Self::NativeFfmpeg => {
                #[cfg(feature = "native-ffmpeg")]
                {
                    ffmpeg_backend::info()
                }
                #[cfg(not(feature = "native-ffmpeg"))]
                {
                    unreachable!("unavailable backend rejected by parser")
                }
            }
        }
    }
    fn decode(
        self,
        path: &str,
        sink: impl FnMut(Format, &[f32]) -> Result<()>,
    ) -> Result<(Format, u64)> {
        match self {
            Self::Symphonia => decode(path, sink),
            Self::NativeFfmpeg => {
                #[cfg(feature = "native-ffmpeg")]
                {
                    ffmpeg_backend::decode(path, sink)
                }
                #[cfg(not(feature = "native-ffmpeg"))]
                {
                    bail!("native-ffmpeg requires a build with --features native-ffmpeg")
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Format {
    rate: u32,
    channels: usize,
}
fn decode(path: &str, mut sink: impl FnMut(Format, &[f32]) -> Result<()>) -> Result<(Format, u64)> {
    if !std::fs::metadata(path)?.is_file() {
        bail!("input must be a finite regular file: {path}");
    }
    let file = File::open(path).with_context(|| format!("open {path}"))?;
    let mut hint = Hint::new();
    if let Some(ext) = Path::new(path).extension().and_then(|s| s.to_str()) {
        hint.with_extension(ext);
    }
    let mut reader = symphonia::default::get_probe()
        .format(
            &hint,
            MediaSourceStream::new(Box::new(file), Default::default()),
            &FormatOptions {
                enable_gapless: true,
                ..Default::default()
            },
            &MetadataOptions::default(),
        )?
        .format;
    let track = reader.default_track().context("no default audio track")?;
    let id = track.id;
    let declared_frames = track.codec_params.n_frames;
    let mut decoder =
        symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default())?;
    let mut format = None;
    let mut frames = 0;
    loop {
        let packet = match reader.next_packet() {
            Ok(p) => p,
            Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.into()),
        };
        if packet.track_id() != id {
            continue;
        }
        let decoded = decoder
            .decode(&packet)
            .with_context(|| format!("decode {path}"))?;
        let current = Format {
            rate: decoded.spec().rate,
            channels: decoded.spec().channels.count(),
        };
        let stereo = symphonia::core::audio::Channels::FRONT_LEFT
            | symphonia::core::audio::Channels::FRONT_RIGHT;
        if decoded.spec().channels != stereo {
            bail!(
                "probe supports standard left/right stereo only; channel mapping not implemented"
            );
        }
        ensure_format(&mut format, current)?;
        let mut pcm = SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
        pcm.copy_interleaved_ref(decoded);
        let samples = pcm.samples();
        // Symphonia decoders honor packet trim metadata when enable_gapless is set.
        sink(current, samples)?;
        frames += (samples.len() / current.channels) as u64;
    }
    if frames == 0 {
        bail!("{path}: no decoded audio frames");
    }
    if let Some(expected) = declared_frames {
        if frames < expected {
            bail!("{path}: truncated audio: decoded {frames} frames, declared {expected}");
        }
    }
    Ok((format.context("no audio format")?, frames))
}
fn ensure_format(expected: &mut Option<Format>, current: Format) -> Result<()> {
    if current.channels == 0 || current.rate == 0 {
        bail!("invalid audio format");
    }
    match expected {
        Some(f) if *f != current => {
            bail!("format change: {f:?} -> {current:?}; resampling/remapping not implemented")
        }
        None => *expected = Some(current),
        _ => (),
    }
    Ok(())
}
#[cfg(test)]
fn decode_files(output: &str, files: &[String]) -> Result<()> {
    decode_files_with_backend(output, files, Backend::Symphonia)
}
fn decode_files_with_backend(output: &str, files: &[String], backend: Backend) -> Result<()> {
    if let Ok(out) = std::fs::canonicalize(output) {
        for file in files {
            if std::fs::canonicalize(file)? == out {
                bail!("output must not overwrite an input");
            }
        }
    }
    // Exclusive creation also protects hard links, which canonicalize cannot detect.
    let mut writer = BufWriter::new(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output)?,
    );
    let mut format = None;
    let mut total = 0;
    for path in files {
        let (f, frames) = backend.decode(path, |f, samples| {
            ensure_format(&mut format, f)?;
            for sample in samples {
                writer.write_all(&sample.to_le_bytes())?;
            }
            Ok(())
        })?;
        total += frames;
        println!(
            "{}",
            json!({"decoder":backend.name(),"file":path,"frames":frames,"channels":f.channels,"sample_rate":f.rate})
        );
    }
    writer.flush()?;
    let f = format.context("no files")?;
    println!(
        "{}",
        json!({"decoder":backend.name(),"total_frames":total,"channels":f.channels,"sample_rate":f.rate})
    );
    Ok(())
}
#[derive(Default)]
struct State {
    done: AtomicBool,
    failed: AtomicBool,
    cancel: AtomicBool,
    paused: AtomicBool,
    stop_requested: AtomicBool,
    consumed: AtomicU64,
    total_frames: AtomicU64,
    underrun_frames: AtomicU64,
}
fn render(
    data: &mut [f32],
    queue: &ArrayQueue<[f32; 32]>,
    state: &State,
    channels: usize,
    volume: f32,
) {
    if state.paused.load(Ordering::Acquire) {
        data.fill(0.0);
        return;
    }
    for frame in data.chunks_mut(channels) {
        if let Some(samples) = queue.pop() {
            for (dst, src) in frame.iter_mut().zip(&samples) {
                *dst = *src * volume;
            }
            state.consumed.fetch_add(1, Ordering::Relaxed);
        } else {
            frame.fill(0.0);
            if state.consumed.load(Ordering::Relaxed) < state.total_frames.load(Ordering::Relaxed) {
                state.underrun_frames.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}
fn play(
    files: Vec<String>,
    volume: f32,
    device_name: Option<String>,
    backend: Backend,
) -> Result<()> {
    if !volume.is_finite() || !(0.0..=1.0).contains(&volume) {
        bail!("volume must be finite and in 0..=1");
    }
    // Preflight validates the complete finite sequence before opening a native stream.
    let mut format = None;
    let mut total = 0;
    for path in &files {
        total += backend
            .decode(path, |f, _| ensure_format(&mut format, f))?
            .1;
    }
    let f = format.context("no files")?;
    if f.channels != 2 {
        bail!("native probe supports stereo only; multichannel mapping not implemented");
    }
    let host = cpal::default_host();
    let device = match device_name {
        Some(name) => host
            .output_devices()?
            .find(|d| d.name().ok().as_deref() == Some(&name))
            .context("requested device not found (no fallback)")?,
        None => host
            .default_output_device()
            .context("no default output device")?,
    };
    let supported = device
        .supported_output_configs()?
        .find(|c| {
            c.channels() as usize == f.channels
                && c.sample_format() == cpal::SampleFormat::F32
                && c.min_sample_rate().0 <= f.rate
                && c.max_sample_rate().0 >= f.rate
        })
        .context("endpoint lacks exact f32 source format; no resampling/remapping implemented")?;
    let config = supported
        .with_sample_rate(cpal::SampleRate(f.rate))
        .config();
    // Fixed-size frames avoid allocation and deallocation inside the audio callback.
    let queue = Arc::new(ArrayQueue::new(32768));
    let state = Arc::new(State::default());
    state.total_frames.store(total, Ordering::Relaxed);
    let q = queue.clone();
    let s = state.clone();
    let worker = thread::spawn(move || -> Result<()> {
        let result = (|| {
            for path in files {
                backend.decode(&path, |actual, samples| {
                    if actual != f {
                        bail!("input format changed since preflight");
                    }
                    for source_frame in samples.chunks_exact(f.channels) {
                        let mut block = [0.0; 32];
                        block[..f.channels].copy_from_slice(source_frame);
                        loop {
                            if s.cancel.load(Ordering::Acquire) {
                                bail!("playback cancelled");
                            }
                            match q.push(block) {
                                Ok(()) => break,
                                Err(b) => {
                                    block = b;
                                    thread::sleep(Duration::from_millis(1));
                                }
                            }
                        }
                    }
                    Ok(())
                })?;
            }
            Ok(())
        })();
        if result.is_err() {
            s.failed.store(true, Ordering::Release);
        }
        s.done.store(true, Ordering::Release);
        result
    });
    let result = (|| -> Result<()> {
        let start = Instant::now();
        while queue.len() < 16384 && !state.done.load(Ordering::Acquire) {
            if start.elapsed() > Duration::from_secs(10) {
                bail!("initial buffering timed out");
            }
            thread::sleep(Duration::from_millis(2));
        }
        if state.failed.load(Ordering::Acquire) {
            bail!("decoder worker failed");
        }
        let q = queue.clone();
        let s = state.clone();
        let error_state = state.clone();
        let stream = device.build_output_stream(
            &config,
            move |data: &mut [f32], _| render(data, &q, &s, f.channels, volume),
            move |_error| {
                error_state.failed.store(true, Ordering::Release);
            },
            None,
        )?;
        println!(
            "{}",
            json!({"decoder":backend.name(),"backend_info":backend.info(),"device":device.name()?,"sample_rate":f.rate,"channels":f.channels,"buffer_capacity_frames":32768,"volume":volume})
        );
        let controls = state.clone();
        thread::spawn(move || {
            for line in std::io::stdin().lock().lines() {
                match line.as_deref().map(str::trim) {
                    Ok("pause") => controls.paused.store(true, Ordering::Release),
                    Ok("resume") => controls.paused.store(false, Ordering::Release),
                    Ok("stop") => {
                        controls.stop_requested.store(true, Ordering::Release);
                        break;
                    }
                    Err(_) => break,
                    _ => eprintln!("controls: pause, resume, stop (one per line)"),
                }
            }
        });
        stream.play()?;
        let mut progress = Instant::now();
        let mut last = 0;
        loop {
            if state.stop_requested.load(Ordering::Acquire) {
                break;
            }
            if state.failed.load(Ordering::Acquire) {
                bail!("audio device or decoder failed; stopped without device fallback");
            }
            let current = state.consumed.load(Ordering::Relaxed);
            if current != last || state.paused.load(Ordering::Acquire) {
                progress = Instant::now();
                last = current;
            }
            if state.done.load(Ordering::Acquire) && current >= total {
                break;
            }
            if progress.elapsed() > Duration::from_secs(10) {
                bail!("no audio progress for 10 seconds");
            }
            thread::sleep(Duration::from_millis(10));
        }
        // Callback consumption precedes physical output: allow the last buffer to drain.
        if !state.stop_requested.load(Ordering::Acquire) {
            thread::sleep(Duration::from_millis(250));
            if state.failed.load(Ordering::Acquire) {
                bail!("audio device failed during drain");
            }
        }
        drop(stream);
        Ok(())
    })();
    state.cancel.store(true, Ordering::Release);
    // On failure/stop, detach rather than joining an uninterruptible storage read.
    // This command-line process exits immediately after returning its result.
    result?;
    if !state.stop_requested.load(Ordering::Acquire) {
        worker
            .join()
            .map_err(|_| anyhow::anyhow!("decoder worker panicked"))??;
    }
    println!(
        "{}",
        json!({"decoder":backend.name(),"consumed_frames":state.consumed.load(Ordering::Relaxed),"underrun_frames":state.underrun_frames.load(Ordering::Relaxed),"stopped":state.stop_requested.load(Ordering::Relaxed),"media_controls":"stdin pause/resume/stop only; OS keys not implemented","physical_output_verified":false})
    );
    Ok(())
}
fn run() -> Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        bail!("usage: playback-probe devices | decode --output PATH FILE... | play [--volume 0.02] [--device NAME] FILE...");
    }
    let command = args.remove(0);
    let mut backend = Backend::Symphonia;
    if let Some(index) = args.iter().position(|s| s == "--decoder") {
        if !matches!(command.as_str(), "decode" | "play" | "backend-info") {
            bail!("--decoder is not supported by {command}");
        }
        if index + 1 >= args.len() {
            bail!("missing value for --decoder");
        }
        backend = Backend::parse(&args.remove(index + 1))?;
        args.remove(index);
    }
    match command.as_str() {
        "backend-info" => {
            if !args.is_empty() {
                bail!("backend-info takes only --decoder NAME");
            }
            println!("{}", backend.info());
            Ok(())
        }
        "devices" => {
            if !args.is_empty() {
                bail!("devices takes no arguments");
            }
            for device in cpal::default_host().output_devices()? {
                println!("{}", json!({"name":device.name()?}));
            }
            Ok(())
        }
        "decode" => {
            if args.len() < 3 || args[0] != "--output" {
                bail!("decode --output PATH FILE...");
            }
            decode_files_with_backend(&args[1], &args[2..], backend)
        }
        "play" => {
            let mut volume = 0.02;
            let mut device = None;
            while args.first().is_some_and(|s| s.starts_with("--")) {
                let flag = args.remove(0);
                if args.is_empty() {
                    bail!("missing value for {flag}");
                }
                let value = args.remove(0);
                match flag.as_str() {
                    "--volume" => volume = value.parse()?,
                    "--device" => device = Some(value),
                    _ => bail!("unknown flag {flag}"),
                }
            }
            if args.is_empty() {
                bail!("play requires files");
            }
            play(args, volume, device, backend)
        }
        _ => bail!("unknown command {command}"),
    }
}
fn main() {
    // This finite-fixture experiment has a whole-process deadline, including preflight
    // and storage reads. A blocked regular file cannot defeat a thread join timeout.
    let (finished, receiver) = std::sync::mpsc::channel();
    thread::spawn(move || {
        if receiver.recv_timeout(Duration::from_secs(60)).is_err() {
            eprintln!("probe exceeded its 60-second experiment deadline");
            std::process::exit(124);
        }
    });
    if let Err(e) = run() {
        eprintln!("{e:#}");
        std::process::exit(1);
    }
    let _ = finished.send(());
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_input_is_an_error() {
        let path =
            std::env::temp_dir().join(format!("playback-probe-invalid-{}.wav", std::process::id()));
        std::fs::write(&path, b"not an audio file").unwrap();
        let result = decode(path.to_str().unwrap(), |_, _| Ok(()));
        #[cfg(feature = "native-ffmpeg")]
        assert!(Backend::NativeFfmpeg
            .decode(path.to_str().unwrap(), |_, _| Ok(()))
            .is_err());
        std::fs::remove_file(path).unwrap();
        assert!(result.is_err());
    }
    #[test]
    fn output_cannot_destroy_an_input() {
        let path =
            std::env::temp_dir().join(format!("playback-probe-alias-{}.wav", std::process::id()));
        std::fs::write(&path, b"preserve this input").unwrap();
        let name = path.to_str().unwrap().to_owned();
        assert!(decode_files(&name, std::slice::from_ref(&name)).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"preserve this input");
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn output_cannot_destroy_a_hard_linked_input() {
        let path = std::env::temp_dir().join(format!("probe-hardlink-{}.wav", std::process::id()));
        let alias = path.with_extension("f32le");
        std::fs::write(&path, b"preserve linked input").unwrap();
        std::fs::hard_link(&path, &alias).unwrap();
        let result = decode_files(
            alias.to_str().unwrap(),
            &[path.to_str().unwrap().to_owned()],
        );
        let bytes = std::fs::read(&path).unwrap();
        std::fs::remove_file(alias).unwrap();
        std::fs::remove_file(path).unwrap();
        assert!(result.is_err());
        assert_eq!(bytes, b"preserve linked input");
    }
    #[test]
    fn truncated_pcm_is_not_a_successful_decode() {
        let path = std::env::temp_dir().join(format!("probe-truncated-{}.wav", std::process::id()));
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&52u32.to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&48000u32.to_le_bytes());
        wav.extend_from_slice(&192000u32.to_le_bytes());
        wav.extend_from_slice(&4u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&[0; 12]); // Declares four stereo frames, supplies only three.
        std::fs::write(&path, wav).unwrap();
        let result = decode(path.to_str().unwrap(), |_, _| Ok(()));
        #[cfg(feature = "native-ffmpeg")]
        assert!(Backend::NativeFfmpeg
            .decode(path.to_str().unwrap(), |_, _| Ok(()))
            .is_err());
        std::fs::remove_file(path).unwrap();
        assert!(result.is_err());
    }
    #[test]
    #[cfg(feature = "native-ffmpeg")]
    fn native_rejects_wav_truncated_at_complete_packet_boundary() {
        let path = std::env::temp_dir().join(format!("probe-boundary-{}.wav", std::process::id()));
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36u32 + 32768).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&48000u32.to_le_bytes());
        wav.extend_from_slice(&192000u32.to_le_bytes());
        wav.extend_from_slice(&4u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&32768u32.to_le_bytes());
        wav.resize(44 + 16384, 0); // Whole packets exist; the advertised second half is absent.
        std::fs::write(&path, &wav).unwrap();
        let result = Backend::NativeFfmpeg.decode(path.to_str().unwrap(), |_, _| Ok(()));
        assert!(result.is_err());
        // Also catch a lying data chunk when the outer RIFF length fits the file.
        wav[4..8].copy_from_slice(&(36u32 + 16384).to_le_bytes());
        std::fs::write(&path, &wav).unwrap();
        let result = Backend::NativeFfmpeg.decode(path.to_str().unwrap(), |_, _| Ok(()));
        assert!(result.is_err());
        wav[40..44].copy_from_slice(&16384u32.to_le_bytes());
        std::fs::write(&path, &wav).unwrap();
        let valid = Backend::NativeFfmpeg.decode(path.to_str().unwrap(), |_, _| Ok(()));
        std::fs::remove_file(path).unwrap();
        assert_eq!(valid.unwrap().1, 4096);
    }
    #[test]
    fn mismatched_format_is_rejected() {
        let mut f = Some(Format {
            rate: 48000,
            channels: 2,
        });
        assert!(ensure_format(
            &mut f,
            Format {
                rate: 44100,
                channels: 2
            }
        )
        .is_err());
    }
    #[test]
    fn starvation_is_counted_but_final_padding_is_not() {
        let q = ArrayQueue::new(2);
        let mut frame = [0.0; 32];
        frame[..2].copy_from_slice(&[0.5, -0.5]);
        q.push(frame).unwrap();
        let s = State::default();
        s.total_frames.store(2, Ordering::Relaxed);
        let mut out = [9.0; 4];
        render(&mut out, &q, &s, 2, 0.5);
        assert_eq!(out, [0.25, -0.25, 0.0, 0.0]);
        assert_eq!(s.underrun_frames.load(Ordering::Relaxed), 1);
        s.done.store(true, Ordering::Release);
        s.consumed.store(2, Ordering::Relaxed);
        render(&mut out, &q, &s, 2, 1.0);
        assert_eq!(out, [0.0; 4]);
        assert_eq!(s.underrun_frames.load(Ordering::Relaxed), 1);
    }
    #[test]
    fn blocks_join_without_silent_frames() {
        let q = ArrayQueue::new(2);
        let mut frame = [0.0; 32];
        frame[..2].copy_from_slice(&[1.0, 2.0]);
        q.push(frame).unwrap();
        frame[..2].copy_from_slice(&[3.0, 4.0]);
        q.push(frame).unwrap();
        let s = State::default();
        s.done.store(true, Ordering::Release);
        let mut out = [0.0; 4];
        render(&mut out, &q, &s, 2, 1.0);
        assert_eq!(out, [1.0, 2.0, 3.0, 4.0]);
    }
    #[test]
    fn pause_preserves_queued_frames() {
        let q = ArrayQueue::new(1);
        q.push([0.5; 32]).unwrap();
        let s = State::default();
        s.total_frames.store(1, Ordering::Relaxed);
        s.paused.store(true, Ordering::Relaxed);
        let mut out = [1.0; 2];
        render(&mut out, &q, &s, 2, 1.0);
        assert_eq!(out, [0.0; 2]);
        assert_eq!(q.len(), 1);
        assert_eq!(s.underrun_frames.load(Ordering::Relaxed), 0);
        s.paused.store(false, Ordering::Relaxed);
        render(&mut out, &q, &s, 2, 1.0);
        assert_eq!(out, [0.5; 2]);
    }
    #[test]
    fn producer_done_does_not_hide_missing_frames() {
        let q = ArrayQueue::new(1);
        let s = State::default();
        s.total_frames.store(2, Ordering::Relaxed);
        s.done.store(true, Ordering::Relaxed);
        render(&mut [0.0; 2], &q, &s, 2, 1.0);
        assert_eq!(s.underrun_frames.load(Ordering::Relaxed), 1);
    }
}
