---
baseline_commit: cb01a65eee051da0c0ebbf4cf1a67ddf03239d01
---

# Story 14.1: Temp-Only Disk Staging for Provider Sync

Status: done

## Story

As a user syncing larger libraries,
I want provider downloads/transcodes to stage as temporary local files before device write,
so that HifiMule reduces memory pressure and prepares the sync path for later pipeline overlap.

## Acceptance Criteria

1. `execute_provider_sync` streams each provider response to a temp-only staging file instead of buffering the network stream directly into memory.
2. The existing `DeviceIO::write_with_verify(&self, path, data: &[u8])` API remains unchanged; the staged file is read into bytes immediately before write.
3. Staged files are deleted after successful write.
4. Sync failure and cancellation clean the run staging directory.
5. Logs include staging duration, write duration, staged byte size, and cleanup result.
6. Existing provider transcode/extension validation behavior is preserved.

## Tasks / Subtasks

- [x] Replace provider-response buffering with temp-file staging in `execute_provider_sync` (AC: 1, 2, 6)
  - [x] Keep URL resolution, response status handling, content-type validation, extension override, and target path construction in their current order.
  - [x] Stream `response.bytes_stream()` chunks into a per-run temp staging file and keep the existing progress callback behavior during staging.
  - [x] Read the completed staged file into `Vec<u8>` immediately before calling `device_io.write_with_verify`.
- [x] Add cleanup and cancellation handling (AC: 3, 4)
  - [x] Create one run-scoped staging directory for provider sync adds.
  - [x] Remove each staged file after successful device write.
  - [x] Remove the run staging directory on errors, warnings that skip staged work, cancellation, and normal completion.
- [x] Add targeted diagnostics (AC: 5)
  - [x] Log staging duration, write duration, staged byte size, and cleanup result.
  - [x] Keep user progress device-centric; do not report staged bytes as device-written bytes.
- [x] Extend the existing provider sync tests (AC: 1-6)
  - [x] Preserve existing transcode/direct/skip tests.
  - [x] Add one regression test proving staging files are cleaned after success.
  - [x] Add one regression test proving the run staging directory is cleaned after an error or cancellation.

### Review Findings

- [x] [Review][Patch] Restore a maximum staged/read size guard before whole-file read [hifimule-daemon/src/sync.rs:2964]
- [x] [Review][Patch] Avoid unconditional staging-dir creation after `begin_sync_job` [hifimule-daemon/src/sync.rs:2297]
- [x] [Review][Patch] Bound staged filename components derived from provider IDs [hifimule-daemon/src/sync.rs:2599]

## Dev Notes

### Scope

Implement the first throughput slice only. Do not add a producer/writer queue, parallel device writes, a persistent cache, or a new `DeviceIO` streaming API; those belong to Stories 14.2 and 14.3.

### Current Code To Touch

- `hifimule-daemon/src/sync.rs`
  - `execute_provider_sync` currently downloads each add with `buffer_stream(response.bytes_stream(), total_size, progress_callback).await`, then writes the resulting `Vec<u8>` through `device_io.write_with_verify(&rel_path, &buffer).await`.
  - `buffer_stream` is still used by the older non-provider `execute_sync` path. Do not delete or globally change it unless all callers are handled.
  - Existing tests around lines 4177+ cover provider transcode suffixes, direct suffix preservation, incompatible response skips, and unconfirmed transcode output skips. Extend these instead of adding a new test harness.
- `hifimule-daemon/src/device_io.rs`
  - `DeviceIO::write_with_verify` accepts whole-file byte slices and must stay unchanged.
  - All device writes must continue through `DeviceIO`; do not bypass MSC/MTP safety behavior.

### Implementation Guidance

- Reuse the existing `tempfile = "3"` dependency in `hifimule-daemon/Cargo.toml`.
- Prefer a tiny helper near `buffer_stream`, e.g. stream-to-temp-file returning `(PathBuf, u64)`, if it keeps `execute_provider_sync` readable. No trait or module split is needed for this story.
- Use `tokio::fs::File` plus `tokio::io::AsyncWriteExt` for async staging writes; keep chunk progress updates equivalent to `buffer_stream`.
- Treat cleanup as best-effort but visible: log both successful cleanup and cleanup errors.
- Preserve manifest behavior: only push a `SyncedItem` after `write_with_verify` succeeds.
- Preserve skip behavior: incompatible or unconfirmed transcode output must not create manifest entries and must mark handled progress exactly as today.

### Architecture Constraints

- Provider sync remains provider-neutral: Jellyfin/Subsonic/OpenSubsonic differences stay behind `MediaProvider::download_url`.
- Device writes remain single-lane and serial. MTP/WPD compatibility depends on that.
- Progress remains device-centric: files/bytes written to the device are primary; staging is diagnostic/log detail.
- Staging files are sync-run artifacts only, not a cache. They must not survive success, failure, or cancellation.

### References

- [sprint-change-proposal-2026-07-11-sync-throughput-pipeline.md](../planning-artifacts/sprint-change-proposal-2026-07-11-sync-throughput-pipeline.md)
- [architecture.md](../planning-artifacts/architecture.md)
- [sync.rs](../../hifimule-daemon/src/sync.rs)
- [device_io.rs](../../hifimule-daemon/src/device_io.rs)

## Dev Agent Record

### Agent Model Used

GPT-5 Codex

### Debug Log References

- `rtk cargo test -p hifimule-daemon execute_provider_sync` - 6 passed
- `rtk cargo test -p hifimule-daemon provider_sync_staging_path_component_bounds_long_values` - 1 passed
- `rtk cargo test -p hifimule-daemon` - 620 passed
- `rtk cargo check -p hifimule-daemon` - 0 errors, 1 pre-existing dead-code warning
- `rtk cargo clippy -p hifimule-daemon --all-targets -- -D warnings` - blocked by pre-existing clippy/dead-code warnings outside this story; also surfaced the story dependency placement issue, now fixed

### Completion Notes List

- Provider sync now streams each accepted provider response to a run-scoped temp staging file, then reads the staged bytes immediately before the unchanged `DeviceIO::write_with_verify` call.
- Successful writes remove their staged file; the run staging directory is explicitly closed and logged on normal completion, skip/error paths, and cancellation.
- Diagnostics now include staged byte size, staging timing, write timing, staged-file cleanup, and run-directory cleanup while preserving device-centric progress counters.
- Existing provider transcode/direct/skip behavior stayed covered; added success cleanup and cancellation cleanup regressions.
- Code review patches restored the 2 GB staged/read guard, made staging directory creation lazy, and bounded provider-derived staging filename components.

### File List

- `hifimule-daemon/Cargo.toml`
- `hifimule-daemon/src/sync.rs`
- `_bmad-output/implementation-artifacts/14-1-temp-only-disk-staging-for-provider-sync.md`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`

### Change Log

- 2026-07-11: Implemented temp-only provider sync staging, cleanup diagnostics, and provider sync cleanup regression tests.
- 2026-07-11: Applied code review patches for staged size bounds, lazy staging directory creation, and bounded staging filenames.
