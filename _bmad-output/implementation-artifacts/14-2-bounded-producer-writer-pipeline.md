---
baseline_commit: 4210c2245336b6fbf6c6776ebd4581dd89f3271d
---

# Story 14.2: Bounded Producer/Writer Pipeline

Status: review

## Story

As a user with a slow device writer,
I want HifiMule to prepare upcoming tracks while the current track is being written,
so that the writer spends less time idle between files.

## Acceptance Criteria

1. Provider sync adds are split into one producer task and one device writer task.
2. The producer emits completed `StagedTrack` items into a bounded ready queue.
3. The ready queue is capped by track count and staged byte count.
4. Device writes remain single-lane and serial through `DeviceIO::write_with_verify`.
5. Cancellation stops producer and writer and cleans queued, current, and partial staged files.
6. Logs include writer idle time, producer blocked time, queue depth, and staged bytes.

## Tasks / Subtasks

- [x] Refactor provider add handling into producer and writer phases inside `execute_provider_sync` (AC: 1, 4)
  - [x] Keep URL resolution, response status handling, content-type validation, extension override, path construction, and skip/warning behavior equivalent to Story 14.1.
  - [x] Add a small internal `StagedTrack` struct carrying the cloned `SyncAddItem`, staged path, relative device path, staged size, staging timing, original filename metadata, and add index.
  - [x] Keep deletes, id-changes, playlist writes, and final warning propagation in their existing serial flow after add processing.
- [x] Add bounded queue/backpressure (AC: 2, 3)
  - [x] Use existing Tokio primitives; do not add a dependency.
  - [x] Cap ready items by count and by bytes for all staged files not yet cleaned up, including the file currently being written.
  - [x] Release byte capacity only after the writer removes the staged file or after cleanup handles an abandoned staged file.
  - [x] Keep staging directory creation lazy: no add item that is skipped before staging should create the temp directory.
- [x] Implement serial writer consumption (AC: 4)
  - [x] Only the writer task calls `device_io.write_with_verify`.
  - [x] Read the staged file into memory immediately before write; keep the `MAX_FILE_BUFFER_BYTES` guard.
  - [x] Push `SyncedItem`, cleanup replaced files, update per-file manifest, and update operation progress only after `write_with_verify` succeeds.
- [x] Handle cancellation, errors, and cleanup (AC: 5)
  - [x] Poll cancellation before preparing each add, while staging chunks, before queue send, before read/write, and while waiting for queued items.
  - [x] Remove partial staged files when staging fails or is cancelled.
  - [x] Drain and delete queued staged files if either task exits early.
  - [x] Preserve current behavior for recoverable provider errors: direct-download errors are returned in `errors`; required-transcode skips remain warnings and mark the item handled.
- [x] Add diagnostics (AC: 6)
  - [x] Log writer idle time as time spent waiting for the next queued `StagedTrack`.
  - [x] Log producer blocked time as time spent waiting for queue count or byte capacity.
  - [x] Log queue depth and staged bytes when enqueueing/dequeueing and in the final provider sync summary.
  - [x] Keep user progress device-centric; staging/queue metrics are logs only.
- [x] Extend provider sync tests (AC: 1-6)
  - [x] Preserve the existing provider transcode/direct/skip and staging cleanup tests.
  - [x] Add a regression proving the second item can be staged while the first item is blocked in `write_with_verify`.
  - [x] Add a regression proving count or byte backpressure prevents unbounded staged files.
  - [x] Add a regression proving cancellation cleans queued/current/partial staged files and leaves no manifest entry for unwritten items.

## Dev Notes

### Scope

Implement the one-producer, one-writer version only. Do not add per-server producers, priority buckets, persistent cache, chunk-level device streaming, parallel device writes, UI queue visuals, or a new `DeviceIO` API; Story 14.3 owns multi-server fairness.

### Current Code To Touch

- `hifimule-daemon/src/sync.rs`
  - `execute_provider_sync` currently does all provider add work in one loop: resolve URL, open HTTP stream, validate output, construct target path, stage to temp, read staged file, write through `DeviceIO`, remove staged file, then update manifest/progress.
  - `stream_to_staging_file` already streams provider bytes to a bounded temp file and reports progress. Reuse or minimally extend it for cancellation and partial-file cleanup.
  - `provider_sync_staging_prefix` and `provider_sync_staging_path_component` already bound provider-derived temp paths; keep using them.
  - `mark_operation_preparing_file` and `mark_operation_item_handled` already encode progress semantics for preparing/skipped files. Reuse them.
  - Existing provider sync tests live near the Story 14.1 tests and already cover transcode suffixes, direct suffix preservation, incompatible response skips, unconfirmed transcode skips, staging cleanup, and cancellation cleanup.
- `hifimule-daemon/src/device_io.rs`
  - `DeviceIO::write_with_verify(&self, path, data: &[u8])` stays unchanged.
  - MTP/WPD safety depends on serialized `DeviceIO` writes; the writer task is the only write lane.
- `hifimule-daemon/Cargo.toml`
  - `tokio`, `futures`, `bytes`, and `tempfile` are already present. No new dependency is expected.

### Implementation Guidance

- Prefer one private enum for producer-to-writer messages if needed, e.g. `Staged(StagedTrack)` plus a terminal producer result; avoid a new module.
- A simple count-bounded `tokio::sync::mpsc` channel covers track count. For byte capacity, use a small shared limiter with Tokio sync primitives and release-on-drop/write-cleanup semantics. Keep it local to `execute_provider_sync`.
- Treat staged byte capacity as a hard temp-disk ceiling, not just a queue metric.
- If the producer finds a skip before staging, it should update warnings/progress itself and continue without sending anything to the writer.
- If the writer fails a device write, abort the add pipeline, request/observe cancellation, clean remaining staged files, and return the write error. This avoids preparing more files after a device failure.
- Preserve manifest correctness: no `SyncedItem` and no per-file manifest write before `write_with_verify` succeeds.
- Preserve provider neutrality: all provider-specific behavior remains behind `MediaProvider::download_url` and existing content-type/extension validation.

### Previous Story Intelligence

- Story 14.1 established temp-only provider staging and kept `DeviceIO::write_with_verify` unchanged.
- Review patches from 14.1 must not regress:
  - Keep the 2 GB staged/read guard before whole-file reads.
  - Keep staging directory creation lazy.
  - Keep provider-derived staging filename components bounded.
- 14.1 verification passed `rtk cargo test -p hifimule-daemon`, provider-sync targeted tests, and `rtk cargo check -p hifimule-daemon`; full clippy was blocked by pre-existing warnings outside the story.

### Architecture Constraints

- Device progress remains device-centric: files and bytes written to the device are primary. Queue depth and staged bytes are diagnostics, not user-visible completion.
- Staging files are sync-run artifacts only. They must not survive success, failure, or cancellation and must not become a cache.
- All device writes go through `DeviceIO`; never bypass MSC/MTP backend behavior.
- The single writer lane is intentional for MTP/WPD compatibility.

### References

- [sprint-change-proposal-2026-07-11-sync-throughput-pipeline.md](../planning-artifacts/sprint-change-proposal-2026-07-11-sync-throughput-pipeline.md)
- [14-1-temp-only-disk-staging-for-provider-sync.md](14-1-temp-only-disk-staging-for-provider-sync.md)
- [architecture.md](../planning-artifacts/architecture.md)
- [sync.rs](../../hifimule-daemon/src/sync.rs)
- [device_io.rs](../../hifimule-daemon/src/device_io.rs)

## Dev Agent Record

### Agent Model Used

GPT-5 Codex

### Debug Log References

- `rtk cargo test -p hifimule-daemon test_execute_provider_sync` - 9 provider-sync tests passed.
- `rtk cargo test -p hifimule-daemon` - 624 daemon tests passed.
- `rtk cargo check -p hifimule-daemon` - passed with one pre-existing `api.rs::rename_item` dead-code warning.

### Completion Notes List

- Implemented the provider add pipeline as a producer task plus serial writer consumer inside `execute_provider_sync`.
- Added bounded count and byte permits held by each `StagedTrack` until staged cleanup, with lazy staging directory creation preserved.
- Added cancellation checks during preparation, staging, enqueue waits, dequeue/write waits, and staged-file cleanup for failed or abandoned tracks.
- Added queue diagnostics for writer idle time, producer blocked time, queue depth, staged bytes, and final provider pipeline summary.
- Added provider-sync regressions for staging overlap, count backpressure, and queued staged cleanup on cancellation.

### File List

- `hifimule-daemon/src/sync.rs`
- `_bmad-output/implementation-artifacts/14-2-bounded-producer-writer-pipeline.md`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`

### Change Log

- 2026-07-11: Implemented bounded provider producer/writer pipeline and added provider-sync regression coverage.
- 2026-07-11: Story created from approved Epic 14 change proposal and Story 14.1 implementation/review context.
