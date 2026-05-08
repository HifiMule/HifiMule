---
title: 'MTP shell copy files not folders'
type: 'bugfix'
created: '2026-05-03'
status: 'done'
baseline_commit: 'NO_VCS'
context: []
---

<frozen-after-approval reason="human-owned intent - do not modify unless human renegotiates">

## Intent

**Problem:** During MTP sync, the WPD direct write path can create a folder for each track name, then the Shell fallback reports success while the expected file is still missing.

**Approach:** Make the Shell fallback delete any stale destination child and copy a local temp file already named like the target file, avoiding provider-specific rename behavior.

## Boundaries & Constraints

**Always:** Keep the existing direct WPD write path as the first attempt. Keep Shell fallback for Garmin-style MTP devices.

**Ask First:** Any broad rewrite of WPD object creation or sync dirty-marker semantics.

**Never:** Do not report success without using the target filename, and do not leave intentionally created local temp files behind.

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/device/mtp.rs` -- Windows WPD backend and Shell copy fallback.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/device/mtp.rs` -- remove stale target child before Shell copy -- avoids copying into a bogus folder left by failed WPD creation.
- [x] `hifimule-daemon/src/device/mtp.rs` -- create the temp source file with the final filename and copy without Shell rename -- avoids MTP providers ignoring `pszNewName`.
- [x] `hifimule-daemon/src/device/mtp.rs` -- use Shell copy as the first file-write path for Garmin WPD devices -- avoids creating bogus folders before fallback starts.
- [x] `hifimule-daemon/src/device/mtp.rs` -- initialize Shell copy with STA COM and verify destination visibility -- prevents silent success when `IFileOperation` does not materialize a file.
- [x] `hifimule-daemon/src/device_io.rs` -- write real MTP target objects directly instead of pre-writing `.dirty` marker files -- avoids blocking before the actual track copy.
- [x] `hifimule-daemon/src/device_io.rs` and `hifimule-daemon/src/sync.rs` -- let Garmin MTP request MP3 audio and write `.mp3` target names -- avoids Garmin Shell import rejection of FLAC originals.

**Acceptance Criteria:**
- Given WPD direct write creates a bad folder object for a track filename, when Shell fallback runs, then it removes that stale child before copying.
- Given Shell fallback copies a file to MTP, when `CopyItem` runs, then the source already has the final filename and no Shell rename is required.
- Given a Garmin WPD device ID or friendly name, when writing a file, then the direct WPD file object creation path is skipped.
- Given `IFileOperation` returns success without a visible destination item, when Shell copy finishes, then the write fails with a specific verification error.
- Given an MTP sync writes a track, when `write_with_verify` runs, then it attempts the real track path first rather than a synthetic `.dirty` marker.
- Given a Garmin WPD device syncs a FLAC track, when sync resolves the stream and destination path, then Jellyfin is asked for MP3 and the target filename ends in `.mp3`.

## Verification

**Commands:**
- `rtk cargo check -p hifimule-daemon` -- passed with 0 errors and 4 existing dead-code warnings.
- `rtk cargo test -p hifimule-daemon mtp_write_with_verify_writes_target_only` -- passed.
- `rtk cargo test -p hifimule-daemon test_construct_file_path_extension_override` -- passed.

## Suggested Review Order

- Delete stale target Shell child before fallback copy.
  [`mtp.rs:506`](../../hifimule-daemon/src/device/mtp.rs#L506)

- Detect Garmin-style WPD devices that need Shell-first writes.
  [`mtp.rs:258`](../../hifimule-daemon/src/device/mtp.rs#L258)

- Initialize Shell `IFileOperation` in an STA apartment.
  [`mtp.rs:210`](../../hifimule-daemon/src/device/mtp.rs#L210)

- Skip direct WPD file creation before Shell copy on Garmin.
  [`mtp.rs:792`](../../hifimule-daemon/src/device/mtp.rs#L792)

- Verify the destination appears after Shell reports success.
  [`mtp.rs:587`](../../hifimule-daemon/src/device/mtp.rs#L587)

- Copy source file as-is instead of depending on Shell rename.
  [`mtp.rs:554`](../../hifimule-daemon/src/device/mtp.rs#L554)

- Create local temp source with the destination filename.
  [`mtp.rs:899`](../../hifimule-daemon/src/device/mtp.rs#L899)

- Avoid synthetic `.dirty` marker writes on MTP backends.
  [`device_io.rs:247`](../../hifimule-daemon/src/device_io.rs#L247)

- Confirm MTP write verification targets the real file path.
  [`device_io.rs:436`](../../hifimule-daemon/src/device_io.rs#L436)

- Advertise Garmin's preferred import-safe audio container.
  [`mtp.rs:1031`](../../hifimule-daemon/src/device/mtp.rs#L1031)

- Force Jellyfin MP3 streaming for devices that require it.
  [`sync.rs:355`](../../hifimule-daemon/src/sync.rs#L355)

- Use the forced container for both stream profile and destination extension.
  [`sync.rs:543`](../../hifimule-daemon/src/sync.rs#L543)
