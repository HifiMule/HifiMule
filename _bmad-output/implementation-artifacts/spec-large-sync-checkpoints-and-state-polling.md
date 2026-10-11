---
title: 'Reduce sustained large-sync memory and manifest write overhead'
type: 'refactor'
created: '2026-10-11'
status: 'done'
baseline_commit: 'ef69a49fdefa68bc4576860455167d76b90c2058'
context:
  - 'spec-reduce-large-library-sync-memory.md'
---

<frozen-after-approval reason="User explicitly renegotiated checkpoint durability on 2026-10-11; other sync behavior remains unchanged">

## Intent

**Problem:** The installed daemon still retains roughly 449 MB during a real 45,000-song sync. The first executor-only benchmark omitted full manifest polling. Audio copying finished, but final ID updates took a long quiet period and ended with a manifest access error.

**Approach:** Use compact UI state polling and keep track metadata in memory between bounded manifest checkpoints. The user requested periodic writes instead of writes after every file, suggested ten minutes and then 1000 tracks, and explicitly requested checkpoint logs. Use 1000 track updates or ten minutes, whichever comes first, and flush remaining metadata at finalization. Preserve the legacy full state response for callers that do not request compact state.

## Boundaries & Constraints

**Always:** Durably record the initial dirty marker and pending identities before modifying device files. Persist accumulated updates after 1000 track mutations, ten minutes, and completion/cancellation/error finalization. Bind deferred writes to the original physical connection. Drain checkpoint ownership before shutdown or operation completion. Failed checkpoints preserve dirty/pending recovery evidence and report failure. Retain routing, selection, managed deletion, ID changes, playlist contents, provenance/history, and existing DeviceIO verification.

**Ask First:** Increasing the checkpoint interval beyond these bounds, changing selection or deleting recovery evidence.

**Never:** Publish successful completion before required persistence succeeds, send full song records through compact state polling, discard legacy full-state access, hide checkpoint failures, or promise an unmeasured absolute RAM ceiling.

## I/O & Edge-Case Matrix

| Scenario | Expected behavior |
| --- | --- |
| 1000 completed track metadata updates | Persist one checkpoint and reset the count after success |
| Slow file or protection wait | Timer persists earlier completed updates within ten minutes |
| Fewer than 1000 updates at end | Finalization persists remaining metadata before completion |
| Cancel, transfer error, or checkpoint failure | Preserve dirty/pending evidence; attempt safe final flush and report the actual outcome |
| Disconnect, copied ID, or rediscovery | A delayed checkpoint cannot write to a replacement connection |
| Transient Windows replacement lock | Bounded retry; preserve the previous canonical file on exhaustion |
| Compact state request | Exact counts/capacity/settings without full manifest cloning or song JSON trees |
| Legacy state request | Existing full manifest and response contract remain available |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/device/mod.rs` — authoritative in-memory manifests, persistence serialization, device connection fences.
- `hifimule-daemon/src/device_io.rs` — verified MSC temporary writes and atomic replacement.
- `hifimule-daemon/src/sync.rs` — file/ID/delete/provenance/playlist mutations and finalization.
- `hifimule-daemon/src/rpc.rs`, `main.rs` — state responses and manual/automatic sync admission/finalization.
- `hifimule-ui/src/rpc.ts`, `components/BasketSidebar.ts`, `syncCapacity.ts` — shared state polling and capacity/count consumers.

## Tasks & Acceptance

- [x] `device/mod.rs`, `sync.rs`, `rpc.rs`, `main.rs` — introduce operation-owned staged manifest mutations with count/time checkpoints, original-connection fences, explicit flush/drain, and failure/recovery behavior. Keep immediate persistence for settings and initial admission.
- [x] `rpc.rs`, `device/mod.rs`, UI state/capacity consumers — add opt-in compact state projection computed under the device lock; update shared UI polling and preserve the legacy response.
- [x] `device_io.rs` — reproduce transient Windows replacement denial and implement bounded retry and temporary-file cleanup, without changing file permissions.
- [x] Sync/checkpoint code — log checkpoint reason, update count, elapsed interval, encoded manifest size, duration and result; identify post-transfer metadata and directory/playlist phases without song names.
- [x] Rust/UI tests, memory fixture and sampler — verify checkpoint thresholds, timer during blocked transfer, finalization, persistence failures, reconnect/shutdown fences, legacy/compact state parity and capacity. Compare matched fresh-process full versus compact polling, and executor/checkpoint retention.

**Acceptance Criteria:**

- Given fewer than 1000 updates within ten minutes, when files finish, then metadata updates remain in memory without one full manifest write per file.
- Given a threshold, timer or finalization, when persistence succeeds, then all accumulated changes become durable and checkpoint logs identify the trigger and outcome.
- Given interruption or storage failure, when the operation ends, then no clean success is published and recovery evidence remains usable.
- Given the same device data, when compact versus legacy state is requested, then settings, identities, counts, capacity and destination behavior agree.
- Given realistic periodic state polling, when comparing fresh processes, then compact polling eliminates full per-track JSON allocation and reduces measured sustained memory.

## Spec Change Log

- 2026-10-11: The user's periodic-checkpoint request supersedes the previous per-file durability requirement. The original completed spec remains an historical record.

## Design Notes

The existing manifest is the authoritative in-memory state. Metadata staging must coordinate with the existing per-device commit lock and lifecycle admission; it must not create an independent persistent library cache. A timer must flush accumulated progress even while copying a slow file. Default full RPC behavior remains for compatibility; UI requests the compact representation explicitly.

The ten-minute bound schedules a checkpoint attempt. An unavailable device or a serialized DeviceIO call can delay actual persistence; the logs distinguish the attempt and its eventual result. Count checkpoints reset the timer only after successful persistence.

## Verification

Use the controlled FFmpeg build wrapper for Rust checks/tests. Run focused checkpoint, replacement-lock, compact-state and UI capacity regressions, then the full daemon suite and UI build. Record fresh-process measurements and their limitations in a follow-up evidence artifact. Retain generated traces under ignored `target/` directories.

Final verification: 1,424 daemon unit tests and five contract tests passed; cargo check, UI TypeScript checking and production build passed. UI/script tests passed 368 cases with ten skips and two previously documented Windows URL-path failures. Three independent reviewers identified three patches; all were fixed and rechecked. Matched isolated polling reduced peak private memory by 80.52%; the controlled 45,522-track executor with checkpoints and compact polling held around 74 MiB. See [measurement evidence](sync-checkpoint-memory-evidence.md) and [review record](sync-checkpoint-review.md) for limits and logs.

## Suggested Review Order

**Checkpoint ownership and durability**

- Own and drain deferred persistence around every provider sync outcome.
  [sync.rs:2620](../../hifimule-daemon/src/sync.rs#L2620)

- Stage authoritative metadata, flush by count or timer, and log persistence outcomes.
  [checkpoint.rs:47](../../hifimule-daemon/src/device/checkpoint.rs#L47)

- Fence initial and clean commits against the captured physical connection.
  [mod.rs:1648](../../hifimule-daemon/src/device/mod.rs#L1648)

- Coordinate removal through initialization without discarding newer arrivals.
  [mod.rs:1284](../../hifimule-daemon/src/device/mod.rs#L1284)

- Apply provenance through compact indexes while preserving duplicate resident IDs.
  [sync.rs:3999](../../hifimule-daemon/src/sync.rs#L3999)

**Compact state polling**

- Keep legacy responses while offering compact metadata without full manifest allocation.
  [rpc.rs:4334](../../hifimule-daemon/src/rpc.rs#L4334)

- Project counts and capacity directly from authoritative device state.
  [mod.rs:1500](../../hifimule-daemon/src/device/mod.rs#L1500)

- Request compact state from shared UI polling.
  [rpc.ts:132](../../hifimule-ui/src/rpc.ts#L132)

- Consume compact byte totals while retaining legacy array compatibility.
  [syncCapacity.ts:15](../../hifimule-ui/src/state/syncCapacity.ts#L15)

**Storage resilience and verification**

- Retry transient Windows replacement locks and preserve canonical data on failure.
  [device_io.rs:5](../../hifimule-daemon/src/device_io.rs#L5)

- Measure actual execution with checkpoints and periodic state polling.
  [measure-sync-memory.ps1:85](../../scripts/measure-sync-memory.ps1#L85)

- Verify thresholds, timer ownership, recovery and original-connection fences.
  [tests.rs:45](../../hifimule-daemon/src/device/tests.rs#L45)

- Verify large provenance transitions, duplicates and exact checkpoint counts.
  [sync.rs:6100](../../hifimule-daemon/src/sync.rs#L6100)

- Verify compact and legacy state metadata and capacity parity.
  [sync_plan_tests.rs:40](../../hifimule-daemon/src/rpc/sync_plan_tests.rs#L40)

- Verify the UI requests compact polling and honors explicit legacy access.
  [compactDaemonState.test.mjs:10](../../hifimule-ui/tests/compactDaemonState.test.mjs#L10)
