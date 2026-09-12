# Story 15.4 installed playback checklist

Use a clean installed package for each row: Windows x64, Linux x64, macOS x64,
and macOS ARM64. Do not count a source-tree run or VM-only ARM64 run as installed
hardware evidence.

## Setup

1. Configure one Jellyfin server and one Subsonic/OpenSubsonic server containing
   WAV, FLAC, ALAC/M4A, MP3, AAC/M4A, and Opus tracks.
2. Record package SHA-256, source revision, OS version/architecture, audio endpoint,
   server brand/version, and whether the machine has any system FFmpeg installed.
3. Start HifiMule and retain only the sanitized `audioRuntime` object returned by
   authenticated `daemon.health`. Never retain the owner token, request headers,
   authenticated URLs, or provider error bodies.

## Required runs

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
