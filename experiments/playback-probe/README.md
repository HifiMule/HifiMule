# Playback feasibility probe

This standalone experiment tests native Rust decoding and buffered output before HifiMule adopts a playback backend. It does not add a player to the application. It has its own Cargo workspace and dependencies; production builds do not include it.

Targets: Windows, macOS, and Linux. A local build on one OS does not validate the others.

## Run deterministic decoding checks

Requirements: Rust toolchain, Python 3, and optionally FFmpeg with the required encoders. Linux additionally needs the development dependencies for the selected audio backend. See the pinned CPAL version's documentation rather than assuming its latest README applies unchanged.

From the repository root:

```sh
rtk cargo test --manifest-path experiments/playback-probe/Cargo.toml
rtk cargo build --manifest-path experiments/playback-probe/Cargo.toml
rtk proxy python3 experiments/playback-probe/generate-fixtures.py
rtk proxy python3 experiments/playback-probe/verify.py
rtk proxy python3 experiments/playback-probe/test_tools.py
```

On Windows, pass the executable explicitly if needed:

```sh
rtk proxy python3 experiments/playback-probe/verify.py --probe experiments/playback-probe/target/debug/playback-probe.exe
```

The generator creates three contiguous, original stereo test tones at 48 kHz, with non-round frame counts. Each track is encoded independently to exercise container metadata and encoder padding. Missing FFmpeg encoders are reported rather than hidden. Files are generated locally, not downloaded.

The verifier compares concatenated decoded float32 samples to the original PCM reference. WAV and FLAC are required by default. Other formats are exploratory: their individual failures are retained in the JSON even if the command succeeds. Use `--require wav,flac,alac,mp3,aac,vorbis,opus` to require all formats (including encoder availability). Lossless data uses a tight numerical tolerance; lossy comparisons use a fixture-specific RMS threshold and inspect track-boundary windows. These checks do not establish perceptual quality or prove gapless physical output.

Generated results are written to `experiments/playback-probe/results/decode.json`. The fixture manifest records exact frame counts, successfully encoded variants, and generation failures.

## Compare an FFmpeg decoder

The optional `ffmpeg_decode.py` adapter uses installed `ffmpeg` and `ffprobe` executables through the same verifier. Python remains a test harness, not a proposed production playback runtime. Select the backend explicitly and retain separate reports:

```sh
rtk proxy python3 experiments/playback-probe/verify.py --decoder symphonia --output experiments/playback-probe/results/symphonia-comparison.json
rtk proxy python3 experiments/playback-probe/verify.py --decoder ffmpeg --require wav,flac,alac,mp3,aac,opus --output experiments/playback-probe/results/ffmpeg-comparison.json
rtk proxy python3 experiments/playback-probe/test_ffmpeg_decode.py
```

Both backends use identical frame, sample-error, and boundary checks. The FFmpeg adapter does not read the fixture manifest or trim samples to its expected lengths. Reports retain the FFmpeg/ffprobe version and build configuration; a pass with one version is not evidence for another. This CLI comparison is decoded PCM only. The optional Rust backend below tests native library integration separately.

The adapter selects the first audio stream, requires finite local standard-stereo inputs with matching sample rates, and rejects existing output paths. It uses temporary files and bounded copying instead of capturing decoded audio in memory. Nonzero subprocess exits, malformed metadata, incomplete frames, and timeouts are errors. Partial output on failure is not a successful decode.

## Native FFmpeg libraries in Rust

The optional `native-ffmpeg` feature links FFmpeg libraries directly; it does not launch `ffmpeg`, `ffprobe`, or Python during playback. Default builds retain Symphonia without requiring FFmpeg development libraries.

```sh
rtk cargo build --manifest-path experiments/playback-probe/Cargo.toml --features native-ffmpeg
rtk cargo test --manifest-path experiments/playback-probe/Cargo.toml --features native-ffmpeg
rtk cargo run --manifest-path experiments/playback-probe/Cargo.toml --features native-ffmpeg -- backend-info --decoder native-ffmpeg
rtk proxy python3 experiments/playback-probe/verify.py --decoder native-ffmpeg --require wav,flac,alac,mp3,aac,opus --output experiments/playback-probe/results/native-ffmpeg.json
```

Build prerequisites include compatible FFmpeg development libraries and Clang for binding generation. Unix builds use pkg-config; Windows needs matching headers, import libraries and runtime DLLs (see [wrapper build notes](https://github.com/zmwangx/rust-ffmpeg/wiki/Notes-on-building)). This experiment uses the installed library build; it does not provide distributable FFmpeg packages. Its results identify linked library versions/configuration, separately from the CLI executable version.

Select the decoder for native output explicitly:

```sh
rtk cargo run --manifest-path experiments/playback-probe/Cargo.toml --features native-ffmpeg -- play --decoder native-ffmpeg --volume 0 --device "YOUR EXACT DEVICE NAME" experiments/playback-probe/fixtures/track-1.m4a experiments/playback-probe/fixtures/track-2.m4a experiments/playback-probe/fixtures/track-3.m4a
```

The native backend selects the first audio stream and restricts nested input protocols to local files. It validates RIFF/WAVE chunk bounds to catch clean-packet truncation; RF64 and other containers rely on FFmpeg error/corruption detection. FFmpeg contexts remain local to each decoding invocation. Preflight and the playback worker create their own contexts; only PCM reaches the existing queue. The pinned wrapper's thread-sharing limitations still need resolution before production adoption. Native sample conversion changes representation to interleaved float32 at the same source rate; it does not add endpoint-rate conversion or multichannel mapping.

## Native output

```sh
rtk cargo run --manifest-path experiments/playback-probe/Cargo.toml -- devices
rtk cargo run --manifest-path experiments/playback-probe/Cargo.toml -- play --volume 0.02 experiments/playback-probe/fixtures/track-1.wav experiments/playback-probe/fixtures/track-2.wav experiments/playback-probe/fixtures/track-3.wav
```

Use `--volume 0` for a silent callback exercise; it still opens an audio stream. An explicit `--device NAME` selects an endpoint without changing system defaults. Inspect the probe's usage for supported options. No Tauri process is needed. This establishes independence of the experiment, not the lifecycle of HifiMule's production sidecar.

The native path deliberately uses a bounded queue and rejects incompatible source formats rather than introduce an unvalidated resampler. The probe accepts standard left/right stereo only. The output callback consumes ready PCM and records starvation; file reading/decoding happens outside it. Unexpected stream errors must stop playback and must not silently fall back to another device. Platform device-loss notification remains a hardware test requirement.

While `play` runs, type `pause`, `resume`, or `stop` followed by Enter for basic transport control. These are stdin commands; OS keyboard media controls are not implemented. Assess their event-loop and user-session requirements separately before production integration.

Every probe command has a hard 60-second process deadline, including preflight and reads; this is a short-fixture experiment, not a long-running player. Decode requires a new output path and never overwrites an existing file. On failure the output may contain partial PCM, so always check exit status. The Symphonia backend rejects premature EOF when decoded frames fall below the declared count; this is not exhaustive corruption detection for containers with missing/unreliable length metadata.

## Platform and integration validation

Record each check as **passed**, **failed**, or **not run**, with OS version, device/driver, file format, and exact command.

| Check | Windows | macOS | Linux |
|---|---|---|---|
| Native shared output | WASAPI | CoreAudio | Shared PipeWire/PulseAudio route; verify backend/version |
| Gapless output capture | Compare recorded transitions to decoded reference | Same | Same |
| Media keys with no main UI | User-session native handle/SMTC | App event loop/Now Playing | Session D-Bus/MPRIS |
| Output unplug | Pause, no speaker fallback | Same | Same; include desktop rerouting |
| UI-created daemon lifetime | Close UI, reopen, explicitly Quit daemon | Same plus LaunchAgent branch | Same plus sidecar branch |
| Concurrent real sync | Measure buffer starvation and sync throughput | Same | Same |
| Packaging | Installer runtime dependencies | Minimum OS, Intel/Apple Silicon scope | Chosen distributions, GNOME/KDE, Wayland/X11 |

For a gapless hardware check, capture the output via a suitable loopback or audio interface. A callback reporting zero underruns does not exclude driver/device artifacts. Verify shared output by playing another application's audio concurrently. Never use an audible tone at an unexpectedly high level.

Synthetic temporary-file I/O can exercise local scheduling but is not a substitute for USB/MTP sync, server load, or real device throughput measurements. Do not run device initialization or sync as part of fixture tests.

```sh
rtk proxy python3 experiments/playback-probe/stress.py --device "YOUR EXACT DEVICE NAME"
```

This plays the generated WAV fixtures at volume zero while repeatedly rewriting and flushing a 16 MiB temporary file. It reports submitted bytes, not physical device throughput. Like `verify.py`, it accepts `--probe` for the Windows `.exe` path. Scratch data is removed afterward.

## Streaming and metadata checks

Real server tests need representative Jellyfin/Navidrome instances and authorized access. No credentials are discovered by the scripts. Test bitrate selection, direct-file seeking, transcoded offsets, interruption, and recovery independently for each provider/version.

Metadata sampling should count recording IDs, contributor/artist relationships, gain fields, and candidate duplicate matches. Keep counts rather than exporting private library details. A successful API call does not establish enough metadata coverage for meaningful artist drift.

See `_bmad-output/implementation-artifacts/playback-feasibility-results.md` for the actual local results and remaining work.
