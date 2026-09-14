# Story 15.4 installed playback checklist

Use a clean installed package for each row: Windows x64, Linux x64, macOS x64,
and macOS ARM64. Do not count a source-tree run or VM-only ARM64 run as installed
hardware evidence.

## Reported smoke evidence

On 2026-09-13, user testing confirmed audible AAC, ALAC, FLAC, M4A, Opus, WAV,
and MP3 playback with the latest changes on Windows, Linux, and macOS. The
explicit three-platform format matrix is recorded in
[`playback-evidence-cross-platform-formats-2026-09-13.json`](playback-evidence-cross-platform-formats-2026-09-13.json).
The report did not include every target's package hash, exact architecture,
provider/server version, loaded native-library paths, high-water measurements,
or every lifecycle scenario below, so it does not by itself complete the full
installed-evidence matrix.

The Windows build also produced a working installation package: installation
completed successfully and the installed application was usable. Together with
the playback report above, this confirms the Windows build → package → install →
playback smoke path.

The Windows x64 identity and native-module evidence is captured in
[`playback-evidence-windows-x64-2026-09-13.json`](playback-evidence-windows-x64-2026-09-13.json):
Windows 11 Professional 10.0.22631 x64, NSIS package SHA-256
`f6fafcbf8950580e9e3b8a4ab295894bbc9896fcc122a151d9935f53ddededd6`, and
all four required FFmpeg DLLs loaded from `%LOCALAPPDATA%\HifiMule`. Fields not
present in the supplied output remain explicitly `unverified` in that record.

A separate manual official-source result is captured in
[`playback-evidence-windows-x64-official-source-2026-09-13.json`](playback-evidence-windows-x64-official-source-2026-09-13.json).
After forcing English MSVC detection with `VSLANG=1033`, generating `.lib`
import libraries from FFmpeg's installed `.def` files, and installing LLVM for
the Clang runtime required by Rust bindgen, the HifiMule build and deployed build
succeeded. Subsequent explicit format reporting confirms AAC, ALAC, FLAC, M4A,
Opus, WAV and MP3 playback on Windows. The new package/runtime identity fields
remain explicitly unverified because they were not included in the report.

## Setup

1. Configure one Jellyfin server and one Subsonic/OpenSubsonic server containing
   WAV, FLAC, ALAC/M4A, MP3, AAC/M4A, and Opus tracks.
2. Record package SHA-256, source revision, OS version/architecture, audio endpoint,
   server brand/version, and whether the machine has any system FFmpeg installed.
3. Start HifiMule and retain only the sanitized `audioRuntime` object returned by
   authenticated `daemon.health`. Never retain the owner token, request headers,
   authenticated URLs, or provider error bodies.

## Required runs

The production policy under test is recorded in `hifimule-daemon/audio-runtime.json`:
64 KiB compressed chunks retained within an 8 MiB compressed window, a 500 ms
PCM target capped at 1 MiB, and a 100 ms startup fill. These are configured
bounds, not substitutes for the per-target observed high-water values below.

- Play every format from each applicable provider; record loading → active and
  audible output, then Pause, Resume, and Stop.
- Pause after measurable progress, quit normally, relaunch, confirm the session
  stays paused, and explicitly Resume from the restored position.
- Start a slow-loading track and immediately Play another track. Confirm only the
  second track becomes audible or publishes metadata.
- Close all UI windows while active, wait, reopen, and confirm playback continued
  without another Play command.
- Remove/invalidate the active endpoint. Confirm silence plus `OUTPUT_LOST`, no
  automatic endpoint migration, then explicitly Resume after selecting a valid
  shared default.
- Quit while playing and while loading. Confirm audio stops before exit, sync
  cancellation is not delayed, no worker remains, and relaunch is paused.
- Repeat rapid replacement and a long/slow stream. Record
  `compressedHighWaterBytes`, `pcmHighWaterSamples`, underrun/timeout outcome,
  process RSS/native heap separately, and cleanup outcome.

## Native-library evidence

Record the actual loaded module paths from the installed daemon process, not only
pkg-config metadata or binary link declarations.

- macOS: `lsof -p <daemon-pid>` and retain only `libavcodec`, `libavformat`,
  `libavutil`, and `libswresample` rows.
- Linux: inspect `/proc/<daemon-pid>/maps` and retain only those four library rows.
- Windows PowerShell: `(Get-Process -Id <daemon-pid>).Modules` filtered to those
  four DLL names.

Each path must resolve inside the installed application/package. Record an
explicit failure if a developer path, Homebrew path, distro FFmpeg, or another
system location is loaded.

## Result record

For each target, save a sanitized JSON record containing: target, OS/architecture,
source revision, package hash, FFmpeg source hash/configuration, actual native
versions and loaded paths, endpoint/backend, provider/version, fixture, observed
transitions, audible outcome, decoded/consumed frames, compressed/PCM high-water,
timeout/underrun result, worker cleanup, and `passed` or `failed`. Keep unavailable
provider/hardware combinations as `unverified`; never infer them from another row.
