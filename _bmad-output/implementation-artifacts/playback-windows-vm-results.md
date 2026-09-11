# Windows VM playback validation

Executed 2026-09-11 against probe source commit `855e9eb`. No Rust or production sources changed for this run.

## Outcome

Native ARM64 Windows decoding passed all six available formats. Silent AAC and Opus playback through the VM's WASAPI endpoint each consumed all 288,041 frames with zero reported underruns. This extends the native Mac proof to this Windows ARM64 guest; it does not certify physical output or all Windows configurations.

## Environment and build

- UTM Windows VM, Windows 11 Pro 10.0.26200, ARM64.
- Interactive console session 1, signed-in user Alexis. Guest-agent setup commands run as SYSTEM; actual decoding and playback ran in the user session through a temporary limited-privilege task.
- Endpoint: `Speakers (High Definition Audio Device)`, 48 kHz stereo, volume zero. Windows also reported its High Definition Audio Device status as OK.
- Host cross-build: Rust 1.98.1, target `aarch64-pc-windows-gnullvm`, [LLVM-MinGW 20260908](https://github.com/mstorsjo/llvm-mingw/releases/tag/20260908), optional `native-ffmpeg` feature. PE inspection confirmed COFF-ARM64 / machine AA64; the probe does not depend on x64 emulation.
- Bindings: ffmpeg-next and ffmpeg-sys-next 9.0.0. [BtbN FFmpeg builds](https://github.com/BtbN/FFmpeg-Builds/releases/tag/latest), asset `ffmpeg-n9.0-latest-winarm64-lgpl-shared-9.0.zip`, build stamp 20260910. Actual linked versions: libavcodec 63.1.101, libavformat 63.1.101, libavutil 61.1.101. Full runtime configurations are retained with the raw capture.
- Included FFmpeg DLLs and the compiler's ARM64 `libunwind.dll`, identified by inspecting executable imports. No Windows developer tools or installer were needed.

The host had separate Homebrew and rustup compilers. The cross-build used the rustup compiler that owns the Windows standard library target; using the Homebrew compiler initially failed to find that target. This was a toolchain selection issue, not an application incompatibility.

### Build recipe

Extract the matching portable compiler and FFmpeg development bundle. With the rustup compiler's `bin` directory first on PATH, configure these process-local variables (paths refer to extracted bundles):

```text
FFMPEG_DIR=<ffmpeg-root>
CARGO_TARGET_AARCH64_PC_WINDOWS_GNULLVM_LINKER=<llvm-root>/bin/aarch64-w64-mingw32-clang
CC_aarch64_pc_windows_gnullvm=<llvm-root>/bin/aarch64-w64-mingw32-clang
AR_aarch64_pc_windows_gnullvm=<llvm-root>/bin/llvm-ar
BINDGEN_EXTRA_CLANG_ARGS=--target=aarch64-w64-windows-gnu -isystem <llvm-root>/aarch64-w64-mingw32/include
```

Then run:

```sh
rtk cargo build --manifest-path experiments/playback-probe/Cargo.toml --target aarch64-pc-windows-gnullvm --features native-ffmpeg
```

This is an experimental cross-build recipe, not a selected production Windows toolchain or packaging policy. The output needs its matching runtime DLLs beside the executable.

## Measurements

Each sequence consists of three independently encoded tracks containing 96,017, 72,011 and 120,013 source frames, respectively, at 48 kHz stereo.

| Format | Total decoded frames | Full-signal RMS error | Result |
|---|---:|---:|---|
| WAV | 288,041 | 0 | Pass |
| FLAC | 288,041 | 0 | Pass |
| ALAC | 288,041 | 0 | Pass |
| MP3 | 288,041 | 0.002652617 | Pass |
| AAC | 288,041 | 0.002405662 | Pass |
| Opus | 288,041 | 0.000944166 | Pass |

All individual track lengths and boundary windows also passed the existing verifier without changing thresholds or trimming to fixture lengths. PCM and JSONL were produced inside Windows, then copied to the Mac for numerical comparison with the original reference. A temporary adapter replayed those captured files into the unchanged verifier; it did not decode them again on macOS. `verification.json` records this distinction explicitly. Vorbis remains not run because the original fixture set has no Vorbis encode.

| Silent WASAPI sequence | Consumed frames | Underrun frames | Exit code | Approximate command wall time |
|---|---:|---:|---:|---:|
| AAC | 288,041 | 0 | 0 | 6.95 seconds |
| Opus | 288,041 | 0 | 0 | 6.67 seconds |

Wall times derive from Windows shell start/end clock captures, not a precision benchmark. No Windows memory/CPU benchmark was performed. The same bounded queue, decoder worker and callback implementation used on the Mac ran here. Physical audio was deliberately silent and was not captured.

FFmpeg logged warnings about updating timestamps for skipped/discarded samples. The frame counts and PCM checks still passed. This does not validate future seeking or timestamp-based transitions; those need their own tests.

## Evidence and cleanup

Raw capture is in the ignored `experiments/playback-probe/results/windows-arm64/` directory: decoded PCM, per-format JSONL/stderr/exit codes, endpoint enumeration, session/account evidence, playback start/end clocks, linked-library information, `verification.json`, and the actual temporary build/run scripts.

Downloaded compiler archive SHA-256:
`d1dc5d1ecf3a3ced5ed5544c72f1acd0c8e84eb3024d520ecc6b143eec62a149`

Downloaded FFmpeg archive SHA-256:
`a605ea941988d20287ccbfb787da4f712ea273f35640fd082b77e46f33787039`

Transferred test package SHA-256, verified inside Windows:
`6d481bc44476369bacf02e95af8095fc196092df6410807aad2b3a98f6c5b41d`

UTM execution returns before output files are ready, so completion was established from guest-written markers and exit files. The first PowerShell file launcher exited before the probe started; direct executable commands subsequently completed in the user session. No execution-policy or audio-setting changes were made.

The temporary scheduled task was removed and its removal verified. The package-transfer server, bound to the VM-private interface and restricted to the guest IP, was stopped. Test files remain in the guest user's temporary `HifiMulePlaybackProbe` directory for inspection; no persistent launcher was left behind.

## Remaining limits

Still untested: native Windows x64, physical Windows hardware, the user's USB interface, audible/loopback gaplessness, endpoint unplug, concurrent real sync, media keys, production daemon/UI lifetime, live-provider streaming and quality adaptation, and Linux runtime. Existing wrapper threading and release-packaging decisions remain open. This result establishes a working native ARM64 Windows probe and its observed virtual-device behavior.
