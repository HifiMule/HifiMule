---
baseline_commit: 2730d9ea113da3c54fb5b958de224ecb5ecd81f5
---

# Story 14.3: Per-Server Producer Fairness and Priority Buckets

Status: review

## Story

As a multi-server user,
I want sync preparation to avoid one slow server blocking faster ready tracks,
so that multi-server and Auto-Fill syncs stay responsive while respecting my explicit selections.

## Acceptance Criteria

1. Adds are grouped by portable `server_id`.
2. At least one producer can run per server when that server has pending work and staging capacity is available.
3. Explicit playlist/basket content is written before lower-priority Auto-Fill content.
4. Within the active priority bucket, the writer consumes whichever completed staged track becomes ready first.
5. A source failure is retried once. After that retry, explicit content failure stops the sync; Auto-Fill content is recorded as a warning/handled item and the remaining Auto-Fill work continues.
6. A device write failure retries the same staged file once. A second failure aborts the sync and cleans all staging.

## Tasks / Subtasks

- [x] Carry unambiguous add priority through sync planning (AC: 1, 3)
  - [x] Add the serde-defaulted `is_auto_fill` marker to `SyncAddItem`.
  - [x] Populate it in both delta-building paths, including non-tiered Auto-Fill, and preserve it through serialization.
  - [x] Group adds by portable `server_id` with a default-provider fallback.
- [x] Make provider staging multi-source with fair producers (AC: 1, 2, 4)
  - [x] Submit all resolved server sources to one producer/writer pipeline.
  - [x] Start one producer per server group with shared count/byte limits.
  - [x] Maintain explicit and Auto-Fill ready buckets in arrival order.
  - [x] Drain explicit work before Auto-Fill through one serial writer.
  - [x] Keep finalization out of producer tasks.
- [x] Preserve source behavior and implement bucket failure policy (AC: 5)
  - [x] Retry transient URL/HTTP source failures once.
  - [x] Cancel on explicit source failure; warn and mark Auto-Fill failures handled.
  - [x] Preserve intentional format/transcode skips and bounded staging behavior.
- [x] Retry device writes without weakening manifest safety (AC: 6)
  - [x] Retry the same staged file once at the serial device-write boundary.
  - [x] Cancel and drain staging after a second write failure.
  - [x] Record manifest/progress state only after a successful write.
- [x] Integrate multi-server sync finalization once (AC: 1, 2, 6)
  - [x] Keep one device job and one delete/ID-change/playlist/finalization sequence.
  - [x] Preserve single-provider daemon-initiated sync.
- [x] Extend existing daemon regressions (AC: 1-6)
  - [x] Retain and pass the Story 14.2 pipeline, backpressure, cancellation, transcode, and cleanup coverage.
  - [x] Add Auto-Fill priority metadata coverage.

## Dev Notes

### Scope and Constraints

- This is the final incremental throughput slice: parallel source preparation, one serial device write lane. Do not add parallel `DeviceIO` writes, chunk-level device streaming, a persistent cache, a new device API, UI queue controls, per-account/server configuration, or dependencies.
- The existing bounded staging limits remain global hard ceilings: the count cap is 2 and the staged/read byte cap is 2 GiB. Permits stay held through write/cleanup, not merely until dequeue.
- Device-centric progress remains authoritative. Per-server/bucket/staging metrics are logs/diagnostics only.
- Staging is sync-run-only: successful, skipped, failed, cancelled, and abandoned files must all be removed. Keep lazy staging-directory creation and bounded/sanitized provider-derived path components.

### Current Implementation and Required Preservation

- `hifimule-daemon/src/sync.rs`
  - `SyncAddItem` already carries portable `server_id`; `tier` is rotation metadata only and cannot classify Auto-Fill. Add the minimal explicit marker rather than relying on tier or item order.
  - Story 14.2's `ProviderSyncSource`, `StagedByteLimiter`, `StagedTrack`, and `execute_provider_sync` form a one-producer/one-writer FIFO pipeline. Keep its streaming actual-chunk byte reservation, `MAX_FILE_BUFFER_BYTES` read guard, cancellation checks, and atomic writer-side progress updates.
  - Only its writer section may call `write_with_verify`, then add `SyncedItem`, remove replaced files, update manifest, and mark device progress. Deletes, ID changes, and playlist writes remain serial after provider adds.
- `hifimule-daemon/src/rpc.rs`
  - `handle_sync_execute` currently resolves `group_providers` then calls provider sync sequentially. Replace that sequential per-server execution with one shared multi-source invocation; parallel calls to the old function are not acceptable.
  - Existing Auto-Fill delta construction/`patch_delta_tiers` covers only tiered Auto-Fill. Stamp the new priority marker at both Auto-Fill delta build sites, preserving serde compatibility for persisted/requested values.
- `hifimule-daemon/src/main.rs`
  - Its auto-sync path is single-provider. Preserve this behavior; adapt only its call signature if the sync helper changes.
- `hifimule-daemon/src/device_io.rs`
  - `DeviceIO::write_with_verify(&self, path, data)` is unchanged. This serial boundary protects MSC and MTP/WPD behavior.

### Implementation Shape

- Use existing Tokio primitives only. Bounded `tokio::sync::mpsc` supports multiple producers and one consumer with backpressure; retain the existing byte limiter for the separate byte ceiling. Tokio semaphore acquisition is fair, so do not rely on it to implement user priority—choose the bucket at the writer instead. [Tokio mpsc](https://docs.rs/tokio/latest/tokio/sync/mpsc/), [Tokio Semaphore](https://docs.rs/tokio/latest/tokio/sync/struct.Semaphore.html)
- Keep the change local to `sync.rs` and `rpc.rs`. A private source/bucket message type or small scheduler state inside the sync module is sufficient; do not introduce a new abstraction/module.
- Every producer exit must communicate completion/failure so the writer cannot wait forever after a source fails or cancellation is requested.
- A write retry reuses the staged file. Do not re-download/retranscode it and do not release its byte permit before the retry or cleanup completes.

### Previous Story Intelligence

- Story 14.1 introduced temp-only staging while preserving whole-file `DeviceIO` writes and provider validation.
- Story 14.2 added the bounded pipeline. Its review fixed: actual-stream-chunk reservation (never trust source size metadata), cancellation-aware producer/writer waits, atomic operation-state updates, and robust first-write test notification. Retain all four fixes.
- Recent commits: `1f2efd6 Dev 14.2`, `2730d9e Review 14.2`. Build on the reviewed 14.2 baseline, not the pre-pipeline sequential design.

### Testing

- Extend the existing Mockito and `BlockingFirstWriteDeviceIo` harness near the provider-sync tests in `hifimule-daemon/src/sync.rs`; do not add a test framework.
- Add focused `sync.rs` regressions for fairness, priority ordering, both retry policies, staging cleanup, and no overlapping writes. Add `rpc.rs` coverage only where priority metadata/delta routing requires it.
- Run at minimum: `rtk cargo test -p hifimule-daemon test_execute_provider_sync`, targeted new retry/fairness tests, then `rtk cargo test -p hifimule-daemon` and `rtk cargo check -p hifimule-daemon`.

### Project Structure Notes

- Expected code changes: `hifimule-daemon/src/sync.rs`, `hifimule-daemon/src/rpc.rs`, and possibly `hifimule-daemon/src/main.rs` for a mechanical helper-signature update.
- No UI, `DeviceIO`, Cargo dependency, persistent-schema, or provider-adapter change is expected.
- Preserve cross-platform and provider-neutral routing: Jellyfin, Navidrome, Subsonic, and OpenSubsonic remain behind the existing `MediaProvider`/download URL behavior.

### References

- [Epic 14 proposal: Story 14.3 and architecture/UX amendments](../planning-artifacts/sprint-change-proposal-2026-07-11-sync-throughput-pipeline.md#new-epic-14-sync-throughput-pipeline)
- [Story 14.2 implementation and review findings](14-2-bounded-producer-writer-pipeline.md)
- [Project architecture](../planning-artifacts/architecture.md)
- [Project requirements](../planning-artifacts/prd.md)
- [Sync pipeline implementation](../../hifimule-daemon/src/sync.rs)
- [Multi-server sync handler](../../hifimule-daemon/src/rpc.rs)
- [Device write boundary](../../hifimule-daemon/src/device_io.rs)

## Dev Agent Record

### Agent Model Used

GPT-5 Codex

### Debug Log References

- Story context created from the approved Epic 14 proposal, reviewed Story 14.2 artifact, current sync/RPC code analysis, and recent commits `1f2efd6` / `2730d9e`.

### Completion Notes List

- Ultimate context engine analysis completed - comprehensive developer guide created.
- Implemented shared multi-server producer staging with one serial writer, explicit/Auto-Fill priority buckets, provider routing, source retry policies, and device-write retry.
- Validated with `rtk cargo test -p hifimule-daemon` (626 passed), `rtk cargo check -p hifimule-daemon`, and `rtk cargo fmt --check`.

### File List

- `_bmad-output/implementation-artifacts/14-3-per-server-producer-fairness-and-priority-buckets.md`
- `hifimule-daemon/src/sync.rs`
- `hifimule-daemon/src/rpc.rs`
- `hifimule-daemon/src/main.rs`

### Change Log

- 2026-07-11: Story created and marked ready for development.
- 2026-07-11: Implemented multi-server producer/writer scheduler and retry policies; daemon suite passes.
