# Linux VM playback validation

Executed 2026-09-11 against probe source commit `90ee210`. No Rust or production sources changed. Guest source hashes match the Mac checkout.

Follow-up against checkout `ed9d110`: a controlled FFmpeg 9 build passes all six formats and silent PipeWire playback. The original FFmpeg 8 failure below remains valid; see the final comparison section for the successful configuration.

## Outcome

Native ARM64 Linux compilation, all 14 Rust tests, and silent PipeWire output passed. WAV, FLAC, ALAC, MP3 and Opus passed the existing decoded continuity checks. **AAC failed with Ubuntu's FFmpeg 8.0.1 libraries**, producing 1,751 extra frames across three tracks. The full six-format requirement therefore remains failed on this Linux configuration.

This establishes a working Rust/native output path in the Ubuntu desktop session. It does not establish gapless AAC album playback or physical output quality. The earlier Mac and Windows FFmpeg 9 results must not be generalized to the distribution's FFmpeg 8 build.

## Environment and build

- UTM Ubuntu VM using Apple Virtualization; native aarch64 guest, no Rosetta execution of the probe.
- Ubuntu 26.04.1 LTS, kernel `7.0.0-31-generic`, desktop user `alexis`, UID 1000.
- Distribution Cargo and Rust 1.93.1; Clang development tools for binding generation.
- CPAL 0.16.0, ffmpeg-next and ffmpeg-sys-next 9.0.0, unchanged Cargo lockfile.
- Ubuntu FFmpeg package `7:8.0.1-3ubuntu2`: libavcodec 62.11.100, libavformat 62.3.100, libavutil 60.8.100. Runtime identity and `ldd` confirm the linked libraries; the wrapper version does not select the runtime library version.
- ALSA 1.2.15.3 and PipeWire 1.6.2. Enumerated output names: `default`, `pipewire`, `VirtIO SoundCard`.
- Explicitly selected `pipewire`, 48 kHz stereo float32, volume zero. The existing desktop sink volume and default device were not changed.

Dependencies were installed from Ubuntu's package repositories:

```sh
sudo apt-get install -y build-essential cargo rustc pkg-config libasound2-dev libavcodec-dev libavformat-dev libavutil-dev clang libclang-dev
```

Python 3 was already available. The first Cargo attempt timed out retrieving the crates.io index, before compilation. An offline retry used `cargo vendor --locked --offline` on the Mac to package the pinned registry sources, plus a source-replacement configuration inside the temporary guest workspace. No dependency version was changed. The build then completed in 11.34 seconds; all 14 native Rust tests passed.

From the standalone probe directory, with the vendor configuration present:

```sh
cargo build --locked --offline --features native-ffmpeg
cargo test --locked --offline --features native-ffmpeg
python3 verify.py --decoder native-ffmpeg --require wav,flac,alac,mp3,aac,opus --output results/decode.json
```

The verifier ran in Ubuntu. Its AAC failure is preserved in `required_failed`; the temporary shell runner deliberately continued to collect evidence after verification failure.

## Decoded continuity

The unchanged fixtures contain three independently encoded tracks with 96,017, 72,011 and 120,013 source frames, totaling 288,041 at 48 kHz stereo. Thresholds, frame checks and boundary windows are unchanged; no fixture-length trimming was introduced.

| Format | Decoded frames | Full-signal RMS error | Result |
|---|---:|---:|---|
| WAV | 288,041 | 0 | Pass |
| FLAC | 288,041 | 0 | Pass |
| ALAC | 288,041 | 0 | Pass |
| MP3 | 288,041 | 0.002652617 | Pass |
| AAC | 289,792 | 0.109742121 | Fail |
| Opus | 288,041 | 0.000944166 | Pass |

AAC returned 96,256, 72,704 and 120,832 frames. The excess is 239, 693 and 819 frames respectively: about 36.5 ms in total. Both boundary RMS checks also failed (0.071297 and 0.167629 against 0.02). This is a timing/continuity failure in the tested decoding combination, not merely a small lossy-sample difference.

The five passing formats met individual track lengths and boundary checks. Vorbis was not run because the original shared fixture set has no Vorbis encode; Ubuntu's library configuration listing libvorbis does not constitute a fixture test.

Separate native decoding captured raw PCM and JSONL for every format. Recomputing frame counts and full-signal RMS on the Mac from those guest-produced files reproduced the Ubuntu report. The Mac did not decode the fixtures again for this verification.

## Native output and desktop routing

| Silent sequence | Consumed frames | Underrun frames | Exit code | Command wall time |
|---|---:|---:|---:|---:|
| AAC | 289,792 | 0 | 0 | 6.43 seconds |
| Opus | 288,041 | 0 | 0 | 6.32 seconds |

Both ran as UID 1000 with `play --decoder native-ffmpeg --volume 0 --device pipewire`, using the corresponding three filenames from the fixture manifest. Timing includes preflight and the experimental final drain; it is not startup latency or a precision performance benchmark. No Linux memory benchmark was performed.

During each run, `wpctl status` showed `PipeWire ALSA [playback-probe]` with active left/right links to `VirtIO PCM 0:playback_FL` and `playback_FR`. This confirms CPAL's ALSA path reached the desktop PipeWire server instead of directly selecting the hardware device. After playback, the probe clients and streams disappeared. Concurrent mixing with a second audio application was not tested.

AAC's zero-underrun callback result does **not** repair its extra decoded frames. It successfully delivered the wrong-length sequence. Physical audio was deliberately silent and was not recorded. Opus logged skipped/discarded timestamp warnings while still passing its frame and sample checks; seeking remains untested.

## Evidence and cleanup

The ignored `experiments/playback-probe/results/linux-arm64/` directory retains environment/build/test logs, runtime identity, ELF and linkage inspection, the guest verifier report, raw PCM, decoder and playback outputs/exit codes/commands, PipeWire before/during/after snapshots, source hashes, independent capture verification, and actual temporary runner scripts. The initial network failure is retained separately.

Offline transfer package SHA-256:
`21a83e659cce152ed4e8dd7c8a44671605a73402e1d6baa4a38893b88350079a`

The temporary HTTP transfer server was restricted to the VM's private interface and guest IP and was stopped after capture. A temporary UTM share attempted during setup could not be mounted; it was removed and removal verified. Root was exited before all builds/tests/audio runs. No SSH service, persistent launcher, or audio configuration change was introduced. Installed development packages and temporary test files remain available for inspection; an unused empty `/mnt/hifimule` directory also remains from setup.

## Decision and remaining work

Rust can drive this Linux desktop audio route with the same worker, bounded PCM queue and callback code as Mac and Windows. However, linking whichever FFmpeg version a distribution supplies is insufficient for the tested AAC requirement. A controlled FFmpeg runtime/build needs validation on Linux; repeating with the FFmpeg 9 family used on the other platforms is the next focused comparison. This experiment does not establish the minimum fixed version or the exact upstream cause.

Still untested: physical gaplessness and USB-interface behavior, Linux x86_64 and other distributions/desktops, concurrent audio mixing and real sync, endpoint loss, OS media keys, production daemon/UI lifetime, clean-machine packaging, and live-server buffering/adaptation. The wrapper threading assessment remains open. Production playback is not implemented.

## Controlled FFmpeg 9 comparison

The same Ubuntu VM, user session, Rust sources, lockfile, fixture files and verifier were used with an isolated [BtbN FFmpeg 9 ARM64 shared build](https://github.com/BtbN/FFmpeg-Builds/releases/tag/latest). Asset: `ffmpeg-n9.0-latest-linuxarm64-lgpl-shared-9.0.tar.xz`, published 2026-09-11 at 13:43 UTC, 53,577,376 bytes. The moving release URL is recorded with the asset metadata and checksum rather than treated as an immutable version pin.

SHA-256, checked against GitHub's asset digest on the Mac and again after transfer inside Ubuntu:
`37fc094ef6625fc1f9d04cd9560d64b105530e4fc6bf898e1a948c0cce3bc992`

Runtime identity and `ldd` confirmed libavcodec 63.1.101, libavformat 63.1.101 and libavutil 61.1.101 from the private extracted bundle, with the system ALSA library retained. Distribution FFmpeg packages were not replaced. Source hashes matched the checkout; no Rust change or decoder trimming was needed.

### Build selection and verification

The first attempt set `FFMPEG_DIR`, `PKG_CONFIG_PATH`, `LD_LIBRARY_PATH` and a separate Cargo target directory. Although pkg-config reported version 9 libraries, the executable's runtime identity still reported distribution version 8. The explicit identity assertion stopped the experiment before decoding/playback. This attempt is not counted as a version 9 test.

A temporary compiler wrapper prepended the private library search directory to the linker invocation. A fresh Cargo target directory then produced an executable resolving the intended version 9 libraries. The successful process-local setup was:

```sh
export FFMPEG_DIR="<absolute extracted FFmpeg bundle directory>"
export PKG_CONFIG_PATH="$FFMPEG_DIR/lib/pkgconfig"
export LD_LIBRARY_PATH="$FFMPEG_DIR/lib"
export CARGO_TARGET_DIR="$PWD/target-nine-fixed"
cat > ffmpeg-nine/linker <<'LINKER'
#!/bin/sh
exec /usr/bin/cc -L"$FFMPEG_DIR/lib" "$@"
LINKER
chmod u+x ffmpeg-nine/linker
export RUSTFLAGS="-C linker=$PWD/ffmpeg-nine/linker"
cargo build --locked --offline --features native-ffmpeg
cargo test --locked --offline --features native-ffmpeg
```

Here `ffmpeg-nine` is an existing temporary directory within the standalone probe workspace. This is an experiment recipe, not a production installer or selected release toolchain. The environment changes existed only in the runner shell and its children. Production packaging must select both matching headers and actual runtime libraries; a successful build or pkg-config version alone is insufficient evidence.

The corrected native build completed in 9.45 seconds, and all 14 Rust tests passed. The unchanged verifier explicitly targeted `target-nine-fixed/debug/playback-probe` and required all six available formats.

### Measurements

| Format | FFmpeg 9 frames | Full-signal RMS error | Continuity result |
|---|---:|---:|---|
| WAV / FLAC / ALAC | 288,041 each | 0 | Pass |
| MP3 | 288,041 | 0.002652617 | Pass |
| AAC | 288,041 | 0.002405662 | Pass |
| Opus | 288,041 | 0.000944166 | Pass |

Every individual track length and boundary check passed. AAC's 1,751 excess frames observed with the distribution build disappeared. Captured guest PCM independently reproduced the reported frame counts and full-signal RMS when inspected on the Mac. The thresholds and original reference were unchanged. Vorbis remains not run.

Silent AAC and Opus playback each consumed 288,041 frames with zero reported underruns, exit code 0, and approximately 6.33 seconds command wall time. Both used the explicit `pipewire` endpoint at volume zero in the same user session. PipeWire snapshots showed active stereo links to the virtual output during each run and no probe streams afterward. Physical output was not captured. Skipped/discarded timestamp warnings remain in stderr; this does not validate seeking.

### Evidence and conclusion

Raw evidence is retained separately in ignored `experiments/playback-probe/results/linux-arm64-ffmpeg9/`: successful build/test logs, linked-library identity, PCM, decoder/playback commands and results, PipeWire snapshots, source hashes, independent capture checks, asset metadata, both runners, and the first library-selection failure. The previous FFmpeg 8 capture is preserved in `linux-arm64/`. The temporary transfer listener was stopped after collection. No persistent launcher or global library/audio configuration was introduced; test files remain in the temporary guest workspace.

This closes the observed six-format continuity gap on the tested Ubuntu ARM64 configuration using this controlled FFmpeg 9 build. Together with the Mac and Windows results, it supports the native Rust/FFmpeg approach on all three tested OS families. It does not isolate the precise upstream change or establish the minimum fixed FFmpeg version: version and build configuration both differ. Runtime packaging, wrapper thread-safety assessment, physical gaplessness, media keys and durable daemon/UI lifecycle remain open.
