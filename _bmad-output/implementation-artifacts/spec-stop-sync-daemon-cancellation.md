---
title: 'Stop Sync – Daemon Cancellation Mechanism'
type: 'feature'
created: '2026-06-05'
status: 'done'
baseline_commit: 'f5a0659'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The UI stop-sync button calls `sync_cancel` RPC but the daemon has no handler for it — the sync loop runs to completion regardless of the user's request.

**Approach:** Add a per-operation `AtomicBool` cancel flag to `SyncOperationManager`; expose a `sync_cancel` RPC that sets the flag; have `execute_sync` and `execute_provider_sync` check the flag at the start of each file iteration and break early; set `status: Cancelled` when the loop exits due to cancellation.

## Boundaries & Constraints

**Always:**
- The manifest **stays dirty** after cancellation — the sync was interrupted, dirty-resume must be able to pick it back up on next sync.
- `SyncStatus::Cancelled` must serialize as `"cancelled"` (serde camelCase default gives `"Cancelled"` — override with `#[serde(rename = "cancelled")]`).
- Cancellation is best-effort: the current in-flight file download/write finishes before the loop breaks (no mid-file abort).
- After cancellation the daemon tray goes back to `DaemonState::Idle`.
- The `sync_cancel` RPC returns success even if the operation already finished (idempotent).

**Ask First:**
- If any existing test exercises `SyncStatus` serialization round-trips, confirm whether adding a new variant breaks the test suite before merging.

**Never:**
- Do not abort an in-progress file write mid-transfer — only check the flag between files.
- Do not clear `manifest.dirty` or `manifest.pending_item_ids` on cancellation.
- Do not remove completed partial progress from the manifest (per-file updates already wrote them — they stay).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Cancel during adds loop | `sync_cancel` called while iterating file downloads | Loop breaks after current file finishes; operation status → `Cancelled`; daemon → Idle | N/A |
| Cancel during deletes loop | `sync_cancel` called while iterating deletions | Loop breaks after current delete finishes | N/A |
| Cancel before sync starts | `sync_cancel` called before `execute_sync` begins | Cancel flag set; loop checks it on first iteration and breaks immediately | N/A |
| Cancel after sync finishes | `sync_cancel` called after status already `Complete`/`Failed` | Returns success; operation status unchanged | No-op |
| Unknown operationId | `sync_cancel` called with an ID not in the map | Returns RPC error `method_not_found`-level; no state change | Error response to UI |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/sync.rs:415` — `SyncStatus` enum — needs `Cancelled` variant
- `hifimule-daemon/src/sync.rs:465` — `SyncOperationManager` struct — needs `cancel_tokens` field
- `hifimule-daemon/src/sync.rs:491` — `create_operation` — must insert cancel token for new op
- `hifimule-daemon/src/sync.rs:1692` — `execute_sync` adds loop — add cancellation check
- `hifimule-daemon/src/sync.rs:2010` — `execute_sync` deletes loop — add cancellation check
- `hifimule-daemon/src/sync.rs:2099` — `execute_sync` id_changes loop — add cancellation check
- `hifimule-daemon/src/sync.rs:2203` — `execute_provider_sync` — add cancellation checks in its loops
- `hifimule-daemon/src/rpc.rs:314` — dispatch table — add `"sync_cancel"` entry
- `hifimule-daemon/src/rpc.rs:3460` — Jellyfin spawned task post-processing — check cancelled before setting Complete/Failed
- `hifimule-daemon/src/rpc.rs:3344` — provider spawned task post-processing — same

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/sync.rs` -- Add `Cancelled` variant to `SyncStatus` with `#[serde(rename = "cancelled")]`; add `cancel_tokens: Arc<RwLock<HashMap<String, Arc<AtomicBool>>>>` to `SyncOperationManager`; initialize in `new()`; in `create_operation` insert `Arc::new(AtomicBool::new(false))` for the op id; add `pub async fn request_cancel(&self, id: &str) -> bool` (sets flag, returns false if id not found); add `pub async fn is_cancelled(&self, id: &str) -> bool` (reads flag) -- wires cancel signal from RPC to sync loop
- [x] `hifimule-daemon/src/sync.rs` -- In `execute_sync` adds loop, deletes loop, and id_changes loop: at the start of each iteration call `if operation_manager.is_cancelled(&operation_id).await { break; }` -- stops file-level processing on cancel
- [x] `hifimule-daemon/src/sync.rs` -- In `execute_provider_sync` adds loop, deletes loop, and id_changes loop: same cancellation check -- stops file-level processing on cancel for provider path
- [x] `hifimule-daemon/src/rpc.rs` -- Add `handle_sync_cancel` async fn: extract `operationId` param, call `state.sync_operation_manager.request_cancel(&id).await`, return `{"cancelled": true}` on success or error if not found; register `"sync_cancel"` in the dispatch table -- exposes the cancel signal to the UI
- [x] `hifimule-daemon/src/rpc.rs` -- In both spawned tasks (Jellyfin path and provider path), after `execute_sync`/`execute_provider_sync` returns, check `op_manager.is_cancelled(&op_id).await` before setting the final status: if true → set `status = Cancelled`, send `DaemonState::Idle`; otherwise → existing Complete/Failed logic unchanged -- ensures terminal status is `Cancelled` not `Complete` when user cancelled

**Acceptance Criteria:**
- Given a sync is running, when `sync_cancel` is called with the active operation ID, then the operation status transitions to `"cancelled"` within one file-transfer interval and the tray returns to Idle.
- Given `sync_cancel` is called after the operation already reached `"complete"`, then the RPC returns success and the status remains `"complete"`.
- Given `sync_cancel` is called with an unknown operation ID, then the RPC returns a JSON-RPC error.
- Given a sync was cancelled mid-run, when the next sync delta is calculated, then previously-synced items (written before cancellation) are not in the adds list (manifest was updated per-file).
- Given a sync was cancelled, when the daemon restarts, then `manifest.dirty = true` is preserved (dirty-resume logic is not blocked).

## Design Notes

The cancel flag is checked **between files**, not mid-file. This is intentional: aborting a mid-flight write would leave a partial file on the device without a manifest entry, creating an orphan the tool can't track. Completing the current file then stopping is safe.

`request_cancel` and `is_cancelled` use `RwLock::read` (not `write`) for the check — the `AtomicBool` itself does the synchronization. Only the insertion into the map during `create_operation` requires a write lock.

The manifest stays dirty after cancellation so that story 4.4 dirty-resume will offer to resume from the last completed file on the next sync.

## Verification

**Commands:**
- `cd hifimule-daemon && cargo check` -- expected: zero errors
- `cd hifimule-daemon && cargo test 2>&1 | grep -E "FAILED|error"` -- expected: no new failures

## Suggested Review Order

**Cancel signal API**

- RPC dispatch entry — wire `sync_cancel` to the handler
  [`rpc.rs:318`](../../hifimule-daemon/src/rpc.rs#L318)

- `handle_sync_cancel` — extracts operationId, delegates to `request_cancel`, returns success or ERR_NOT_FOUND
  [`rpc.rs:3570`](../../hifimule-daemon/src/rpc.rs#L3570)

- `request_cancel` / `is_cancelled` — store+load an `AtomicBool` under a read-lock; write-lock only on insert
  [`sync.rs:532`](../../hifimule-daemon/src/sync.rs#L532)

- `cancel_tokens` field + initialization in `new()` and `create_operation`
  [`sync.rs:476`](../../hifimule-daemon/src/sync.rs#L476)

**Sync loop guards**

- `execute_sync` adds loop guard — check at top of each file iteration, break on cancel
  [`sync.rs:1726`](../../hifimule-daemon/src/sync.rs#L1726)

- `execute_sync` deletes + id_changes guards
  [`sync.rs:2051`](../../hifimule-daemon/src/sync.rs#L2051)

- `execute_provider_sync` adds loop guard (mirrors execute_sync pattern)
  [`sync.rs:2338`](../../hifimule-daemon/src/sync.rs#L2338)

**Terminal status resolution**

- Provider-path spawned task: check `is_cancelled` before clearing dirty; set `Cancelled` and return
  [`rpc.rs:3348`](../../hifimule-daemon/src/rpc.rs#L3348)

- Jellyfin-path spawned task: same guard, different branch
  [`rpc.rs:3473`](../../hifimule-daemon/src/rpc.rs#L3473)

**Type change**

- `SyncStatus::Cancelled` with explicit serde rename `"cancelled"`
  [`sync.rs:419`](../../hifimule-daemon/src/sync.rs#L419)
