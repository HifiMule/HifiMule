---
baseline_commit: 2730d9ea113da3c54fb5b958de224ecb5ecd81f5
---

# Story 14.3: Per-Server Producer Fairness and Priority Buckets

Status: ready-for-dev

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

- [ ] Carry unambiguous add priority through sync planning (AC: 1, 3)
  - [ ] Add the smallest serde-defaulted priority marker needed on `SyncAddItem` to distinguish every Auto-Fill add from explicit content; do not infer it from optional rotation `tier`.
  - [ ] Populate it in both delta-building paths from the per-sync Auto-Fill IDs, including non-tiered Auto-Fill, and preserve it in RPC/delta serialization.
  - [ ] Group adds by their existing portable `server_id`; legacy/missing IDs must use the already-selected/default provider path or return the existing routing error—never panic.
- [ ] Make provider staging multi-source with fair producers (AC: 1, 2, 4)
  - [ ] Evolve `execute_provider_sync` behind a minimal single-source-compatible wrapper or equivalent shared helper so `rpc.rs` submits all resolved server sources to one producer/writer pipeline.
  - [ ] Start at most one producer per ready server; each producer keeps its own source order and uses the existing global count/byte staging limits.
  - [ ] Feed completed tracks into separate explicit and Auto-Fill ready buckets. Within a bucket, use arrival order so the first completed staged item is written first.
  - [ ] The sole writer drains all explicit work before Auto-Fill; when explicit work remains pending but none is staged, wait for explicit work rather than writing a lower-priority item.
  - [ ] Do not run the existing `execute_provider_sync` concurrently once per server: that would create competing device writers and duplicate finalization.
- [ ] Preserve source behavior and implement bucket failure policy (AC: 5)
  - [ ] Retry a transient source-stage failure once for the same item before applying its bucket policy.
  - [ ] After the retry, fail and cancel the shared pipeline for explicit content; for Auto-Fill, add a warning, mark the item handled using the established progress path, release staging capacity, and continue.
  - [ ] Keep existing intentional skips (unsupported/invalid output and required-transcode skip behavior) unchanged; they are not converted into fatal failures merely because the item is explicit.
  - [ ] Preserve URL resolution, direct/transcode negotiation, response/content-type/extension validation, bounded filename construction, streaming byte reservations, and cancellation-aware staging.
- [ ] Retry device writes without weakening manifest safety (AC: 6)
  - [ ] Have only the serial writer call `DeviceIO::write_with_verify`; retry that same staged file once on a write error.
  - [ ] On a second write failure, cancel producers, drain/delete queued and current staging files, and return the device error.
  - [ ] Create `SyncedItem`, clean replaced files, write per-file manifest state, and advance device progress only after a successful write; never record the failed first attempt.
- [ ] Integrate multi-server sync finalization once (AC: 1, 2, 6)
  - [ ] In `handle_sync_execute`, retain one device `begin_sync_job`/`end_sync_job`, one post-add delete/id-change/playlist sequence, and one dirty/history/status finalization for the whole job.
  - [ ] Keep the existing first/last provider ownership of deletes, ID changes, and playlist work unless the shared helper makes that ownership explicit without changing behavior.
  - [ ] Keep daemon-initiated single-provider sync working; update its call mechanically only if the provider-source API changes.
- [ ] Extend existing daemon regressions (AC: 1-6)
  - [ ] Fast server staging/writing proceeds while another server is slow or pending; device writes never overlap.
  - [ ] An explicit staged item is selected before a ready Auto-Fill item; ready items retain completion order within a bucket.
  - [ ] Source retry succeeds on the second attempt; exhausted explicit failure aborts and cleans staging; exhausted Auto-Fill failure warns/continues.
  - [ ] First device write failure retries successfully without duplicate manifest/progress state; two failures abort and clean staging.
  - [ ] Retain the Story 14.2 overlap, count/byte backpressure, cancellation, transcode/direct, and staging-cleanup coverage.

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

### File List

- `_bmad-output/implementation-artifacts/14-3-per-server-producer-fairness-and-priority-buckets.md`

### Change Log

- 2026-07-11: Story created and marked ready for development.
