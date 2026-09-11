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

## Native output

```sh
rtk cargo run --manifest-path experiments/playback-probe/Cargo.toml -- devices
rtk cargo run --manifest-path experiments/playback-probe/Cargo.toml -- play --volume 0.02 experiments/playback-probe/fixtures/track-1.wav experiments/playback-probe/fixtures/track-2.wav experiments/playback-probe/fixtures/track-3.wav
```

Use `--volume 0` for a silent callback exercise; it still opens an audio stream. An explicit `--device NAME` selects an endpoint without changing system defaults. Inspect the probe's usage for supported options. No Tauri process is needed. This establishes independence of the experiment, not the lifecycle of HifiMule's production sidecar.

The native path deliberately uses a bounded queue and rejects incompatible source formats rather than introduce an unvalidated resampler. The probe accepts standard left/right stereo only. The output callback consumes ready PCM and records starvation; file reading/decoding happens outside it. Unexpected stream errors must stop playback and must not silently fall back to another device. Platform device-loss notification remains a hardware test requirement.

While `play` runs, type `pause`, `resume`, or `stop` followed by Enter for basic transport control. These are stdin commands; OS keyboard media controls are not implemented. Assess their event-loop and user-session requirements separately before production integration.

Every probe command has a hard 60-second process deadline, including preflight and reads; this is a short-fixture experiment, not a long-running player. Decode requires a new output path and never overwrites an existing file. On failure the output may contain partial PCM, so always check exit status. Premature EOF is rejected when decoded frames fall below the declared count; this is not exhaustive corruption detection for containers with missing/unreliable length metadata.

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
