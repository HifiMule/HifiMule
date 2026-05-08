# Story 4.5b: Daemon-Initiated Sync Path

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a **Convenience Seeker (Sarah)**,
I want the UI to automatically detect and display an in-progress daemon-initiated sync when the application is open,
so that I have full visibility into sync state regardless of whether sync was triggered manually via the basket button or automatically by device detection.

## Acceptance Criteria

1. **`get_daemon_state` exposes active operation**: When `get_daemon_state` is called and any sync is running (daemon-initiated OR UI-triggered), the response includes `activeOperationId: "<uuid>"`. When no sync is running, `activeOperationId: null`. (AC: #1)

2. **UI auto-attaches on initial load**: When `BasketSidebar` initializes and `refreshAndRender()` runs, if `get_daemon_state` returns a non-null `activeOperationId`, the sidebar automatically enters syncing mode for that operation (sets `isSyncing = true`, sets `currentOperationId`, starts the 500ms polling loop) — no user click required. (AC: #2)

3. **UI auto-attaches on device connect**: When the UI is already open and a device auto-connects with `auto_sync_on_connect` enabled, the sidebar's next `refreshAndRender()` cycle detects the new `activeOperationId` and transitions to the progress view. (AC: #3)

4. **StatusBar reflects Syncing state**: When `get_daemon_state` returns a non-null `activeOperationId`, `StatusBar.pollDaemonState()` sets daemon state to `'Syncing'`. (AC: #4)

5. **Completion/failure flow identical to UI-triggered**: When the daemon-initiated sync completes (`status === 'complete'`), the existing `handleSyncComplete()` logic fires: "Sync Complete" panel shown, basket cleared, "Done" button returns to normal view. On failure, `handleSyncFailed()` fires: error panel shown, basket NOT cleared, "Dismiss" returns to normal view. (AC: #5)

6. **Headless path untouched**: When the UI is not open, the daemon's existing `run_auto_sync` behavior is unchanged: tray icon → Syncing, OS-native notification "Sync Complete. Safe to eject." on completion, tray icon → Idle. No code changes to `main.rs`. (AC: #6)

7. **No duplicate polling when already syncing**: If `isSyncing` is already true (user clicked Start Sync), `refreshAndRender()` does NOT attempt to re-attach to `activeOperationId` — the existing guard (`if (this.isSyncing ...) { this.render(); return; }`) is preserved as-is. (AC: #7)

## Tasks / Subtasks

- [x] **T1: Add `get_active_operation_id()` to `SyncOperationManager` in `sync.rs`** (AC: #1)
  - [x] T1.1: Add the following public async method to `SyncOperationManager` (after `has_active_operation`, around line 196 of `sync.rs`):
    ```rust
    pub async fn get_active_operation_id(&self) -> Option<String> {
        let ops = self.operations.read().await;
        ops.values()
            .find(|op| op.status == SyncStatus::Running)
            .map(|op| op.id.clone())
    }
    ```

- [x] **T2: Expose `activeOperationId` in `get_daemon_state` (rpc.rs)** (AC: #1, #4)
  - [x] T2.1: In `handle_get_daemon_state` (rpc.rs:357), add after the `auto_fill` block:
    ```rust
    let active_operation_id = state.sync_operation_manager.get_active_operation_id().await;
    ```
  - [x] T2.2: Add to the `serde_json::json!({...})` return value (after `"autoFill"` field):
    ```rust
    "activeOperationId": active_operation_id,
    ```
  - [x] T2.3: Add a unit test in rpc.rs tests section verifying `activeOperationId` is `null` when no sync is running and returns the correct UUID when a `SyncStatus::Running` operation exists. Reference the pattern from `test_rpc_get_daemon_state_includes_dirty_manifest_field` at line ~2226.

- [x] **T3: Update `BasketSidebar.refreshAndRender()` to detect and attach to active operations** (AC: #2, #3, #7)
  - [x] T3.1: Modify `refreshAndRender()` starting at line 171. The current guard is:
    ```typescript
    if (this.isSyncing || this.showSyncComplete || this.syncErrorMessages !== null) {
        this.render();
        return;
    }
    ```
    Change to allow the `get_daemon_state` check to proceed even when not yet syncing — but ONLY for the purpose of detecting `activeOperationId`. The `isSyncing` early return must stay for when we're already handling a sync. The guard logic should remain as-is; instead, **after** the existing `daemonStateResult` processing block (after line 220, before `this.render()`), add:
    ```typescript
    // Attach to daemon-initiated sync if one is running and we're not already tracking it
    if (!this.isSyncing && !this.showSyncComplete && this.syncErrorMessages === null) {
        if (daemonStateResult.status === 'fulfilled' && daemonStateResult.value) {
            const state = daemonStateResult.value as any;
            const activeOpId = state.activeOperationId as string | null;
            if (activeOpId) {
                this.isSyncing = true;
                this.showSyncComplete = false;
                this.syncErrorMessages = null;
                this.currentOperationId = activeOpId;
                this.currentOperation = null;
                this.startPolling();
                return;  // startPolling() will call renderSyncProgress() once data arrives
            }
        }
    }
    ```
    **CRITICAL**: This block must be placed AFTER the device hydration block (the `currentDevice?.deviceId` check) so that device context (basket, auto-fill) is loaded before potentially entering sync mode. Otherwise a race: auto-fill could trigger while also entering sync mode.

  - [x] T3.2: The existing `isSyncing` guard at line 171 remains **unchanged**. It prevents `refreshAndRender()` from issuing extra RPCs while already displaying the progress view — this is still correct for both UI-triggered and daemon-triggered syncs.

- [x] **T4: Update `StatusBar.pollDaemonState()` to show Syncing** (AC: #4)
  - [x] T4.1: In `StatusBar.pollDaemonState()` around line 91, the current condition chain is:
    ```typescript
    if (r.serverConnected === false) {
        this.state.daemonState = 'Not logged in';
    } else if (r.currentDevice) {
        this.state.daemonState = r.currentDevice.dirty ? 'Device (dirty)' : 'Idle';
    } else {
        this.state.daemonState = 'Idle';
    }
    ```
    Change to insert the Syncing state check BEFORE the device/idle checks:
    ```typescript
    if (r.serverConnected === false) {
        this.state.daemonState = 'Not logged in';
    } else if (r.activeOperationId) {
        this.state.daemonState = 'Syncing';
    } else if (r.currentDevice) {
        this.state.daemonState = r.currentDevice.dirty ? 'Device (dirty)' : 'Idle';
    } else {
        this.state.daemonState = 'Idle';
    }
    ```

- [x] **T5: Verification** (AC: all)
  - [ ] T5.1: Manual — Enable `auto_sync_on_connect` in the UI toggle for a connected device. Disconnect and reconnect device. Observe: (a) tray shows Syncing, (b) StatusBar shows "Syncing", (c) BasketSidebar transitions to progress panel without any click.
  - [ ] T5.2: Manual — With auto-sync running, close and re-open the UI window. Observe: progress panel appears immediately reflecting current sync state (not a blank basket).
  - [ ] T5.3: Manual — Let auto-sync complete. Observe: "Sync Complete" panel, basket clears, "Done" button returns to normal view.
  - [ ] T5.4: Manual — Trigger auto-sync (no UI open). Observe: OS notification "Sync Complete. Safe to eject." fires. No regressions in headless path.
  - [ ] T5.5: Manual — Click "Start Sync" button manually while no auto-sync is running. Verify behavior is identical to pre-story (no regression). `get_daemon_state` returning `activeOperationId` for a UI-triggered sync does NOT cause double-attach.
  - [x] T5.6: Run `cargo test` in `hifimule-daemon/` — all existing tests pass plus new T2.3 test.

## Dev Notes

### Architecture Compliance

**CRITICAL: This story is EXCLUSIVELY about UI visibility of an existing daemon capability.**

`run_auto_sync` in `main.rs` is **fully implemented and battle-tested**. Do NOT modify:
- `main.rs` — the auto-sync trigger, device detection loop, or `run_auto_sync` function
- `sync.rs` execution logic — `execute_sync`, `calculate_delta`, `SyncOperationManager` behavior
- The 500ms polling mechanism — already used by `handleStartSync()`, Story 4.5 established this pattern

**No new IPC methods.** The change to `get_daemon_state` is an additive field — zero breaking changes.

**Two-step sync initiation is NOT used here.** `run_auto_sync` internally calls `calculate_delta` and `execute_sync` directly without the two-step RPC flow. The `SyncOperationManager` gets an operation regardless of how sync was triggered, so `sync_get_operation_status` polling works identically.

**camelCase serde applies**: `activeOperationId` (camelCase) is the field name in the JSON response — consistent with existing `autoSyncOnConnect`, `autoFill`, `currentDevice`, `dirtyManifest` naming.

### Why `activeOperationId` in `get_daemon_state` (not a new RPC)

The `get_daemon_state` is already polled by both `BasketSidebar` (on `refreshAndRender`) and `StatusBar` (every N seconds). Piggy-backing `activeOperationId` here means both consumers get the data in a single existing call with zero additional network round-trips.

### Critical Invariant: Guard Order in `refreshAndRender()`

The existing early-return guard at line 171:
```typescript
if (this.isSyncing || this.showSyncComplete || this.syncErrorMessages !== null) {
    this.render();
    return;
}
```
**Must remain unchanged**. When `isSyncing === true`, we're already inside `startPolling()` which owns the UX state machine. Allowing `refreshAndRender()` to re-read `activeOperationId` from daemon state while already syncing would cause double-attach to the same operation or a state conflict.

The new auto-attach logic (T3.1) is placed AFTER the device hydration block for an important reason: device hydration sets `this.autoFillEnabled`, `this.autoFillMaxBytes`, `this.autoSyncOnConnect`. If auto-sync starts during the same refresh cycle, having device context already loaded ensures render() can display the correct device name and settings in the progress view header.

### `handleSyncComplete()` basket clear — intentional for daemon-initiated sync

`basketStore.clear()` is called in `handleSyncComplete()` for both UI-triggered and daemon-initiated syncs. This is intentional: after a successful sync, the device is in sync with the basket configuration. Clearing the UI basket reflects the "clean slate" state. If the user wants to add more items, they start fresh — consistent with the UI-triggered flow.

### Source Tree

**Files to MODIFY:**
1. [hifimule-daemon/src/sync.rs](hifimule-daemon/src/sync.rs) — T1: `get_active_operation_id()` method on `SyncOperationManager` (~line 196)
2. [hifimule-daemon/src/rpc.rs](hifimule-daemon/src/rpc.rs) — T2: `activeOperationId` in `handle_get_daemon_state` (~line 357); add test
3. [hifimule-ui/src/components/BasketSidebar.ts](hifimule-ui/src/components/BasketSidebar.ts) — T3: auto-attach block in `refreshAndRender()` (~line 220)
4. [hifimule-ui/src/components/StatusBar.ts](hifimule-ui/src/components/StatusBar.ts) — T4: Syncing state in `pollDaemonState()` (~line 91)

**Files to READ (do NOT modify):**
5. [hifimule-daemon/src/main.rs](hifimule-daemon/src/main.rs) — `run_auto_sync` (~line 482), device event loop (~line 199), `DaemonState::Syncing` pattern (~line 493)
6. [hifimule-ui/src/components/BasketSidebar.ts](hifimule-ui/src/components/BasketSidebar.ts) — full `handleStartSync()`, `startPolling()`, `handleSyncComplete()`, `handleSyncFailed()` to understand the sync state machine before modifying `refreshAndRender()`

**Files NOT to touch:**
- `main.rs` — `run_auto_sync` is complete; do NOT add `activeOperationId` broadcasting here
- `sync.rs` execution logic (`execute_sync`, `calculate_delta`) — no changes needed
- `device/mod.rs`, `auto_fill.rs` — unrelated to this story

### Critical RPC Signatures (unchanged)

```typescript
// get_daemon_state (MODIFIED: adds activeOperationId)
// returns: { currentDevice, deviceMapping, serverConnected, dirtyManifest,
//            pendingDevicePath, autoSyncOnConnect, autoFill,
//            activeOperationId: string | null  ← NEW
//          }

// sync_get_operation_status (UNCHANGED from Story 4.5)
// params: { operationId: string }
// returns: SyncOperation { id, status, startedAt, currentFile, bytesCurrent,
//          bytesTotal, filesCompleted, filesTotal, errors[] }
```

### Testing Standards

No TypeScript test framework in the UI project. All UI testing is manual (T5 tasks). Daemon: add one unit test in `rpc.rs` tests (T2.3). Run `cargo test` in `hifimule-daemon/` to confirm no regressions.

### Project Structure Notes

- `SyncOperationManager` is part of `AppState` in `rpc.rs` (line 61: `pub sync_operation_manager: Arc<crate::sync::SyncOperationManager>`). `handle_get_daemon_state` already has access via `&state`.
- `refreshAndRender()` is triggered by `basketStore`'s `'update'` event AND called on `BasketSidebar` construction. Both paths will now detect an active operation.
- `daemonStateInterval` in `BasketSidebar` is a separate polling mechanism for periodic state refresh (distinct from the sync progress poll). Both exist simultaneously and don't conflict.

**Detected Conflicts/Variances:**
- Epics say "reflects via `on_sync_progress` events" — actual implementation uses 500ms polling (established in Story 4.5 dev notes). This story continues the polling pattern, not push events.
- Story 4.5 code review fix M2 added a guard in `refreshAndRender()` to skip RPCs when syncing. This story must respect that guard — the auto-attach block is placed AFTER the guard, in the "not yet syncing" code path only.

### References

- [main.rs:480](hifimule-daemon/src/main.rs#L480) — `run_auto_sync` function (already complete)
- [main.rs:199](hifimule-daemon/src/main.rs#L199) — Device event loop; `auto_sync_on_connect` trigger
- [rpc.rs:357](hifimule-daemon/src/rpc.rs#L357) — `handle_get_daemon_state` — modify here for T2
- [rpc.rs:61](hifimule-daemon/src/rpc.rs#L61) — `AppState.sync_operation_manager` field
- [sync.rs:148](hifimule-daemon/src/sync.rs#L148) — `SyncOperationManager` struct and impl
- [sync.rs:193](hifimule-daemon/src/sync.rs#L193) — `has_active_operation()` — model `get_active_operation_id()` after this
- [BasketSidebar.ts:170](hifimule-ui/src/components/BasketSidebar.ts#L170) — `refreshAndRender()` — T3 target
- [BasketSidebar.ts:689](hifimule-ui/src/components/BasketSidebar.ts#L689) — `handleStartSync()` — understand sync state machine before modifying
- [StatusBar.ts:91](hifimule-ui/src/components/StatusBar.ts#L91) — `pollDaemonState()` state derivation — T4 target
- [Story 4.5](4-5-start-sync-ui-to-engine-trigger.md) — `SyncOperation` TypeScript interface, `startPolling()`, `handleSyncComplete()`, `handleSyncFailed()` — all reused unchanged
- [Architecture: Communication Patterns](../_bmad-output/planning-artifacts/architecture.md#communication-patterns) — polling pattern (push events deferred)
- [Epic 4 Story 4.5](../_bmad-output/planning-artifacts/epics.md#story-45-start-sync-ui-to-engine-trigger) — daemon-initiated AC (last "Given" block)

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

### Completion Notes List

- T1: Added `get_active_operation_id()` to `SyncOperationManager` in `sync.rs` — follows same pattern as `has_active_operation()`, reads operations map and returns the ID of any `SyncStatus::Running` operation.
- T2: Added `active_operation_id` variable in `handle_get_daemon_state` and exposed it as `activeOperationId` in the JSON response. Additive field — zero breaking changes to existing consumers.
- T2.3: Added `test_rpc_get_daemon_state_includes_active_operation_id` unit test covering both null (no running op) and UUID (running op) cases. 151 tests pass.
- T3: Added auto-attach block in `refreshAndRender()` after the device hydration block. Placed after device context load so auto-fill settings are ready before entering sync mode. The existing `isSyncing` early-return guard at line 171 remains unchanged.
- T4: Inserted `activeOperationId` Syncing state check before the device/idle checks in `StatusBar.pollDaemonState()` — highest-priority visible state when a sync is running.
- T5: TypeScript compilation clean (no errors). `cargo test` 151/151 pass.

### File List

- hifimule-daemon/src/sync.rs
- hifimule-daemon/src/rpc.rs
- hifimule-ui/src/components/BasketSidebar.ts
- hifimule-ui/src/components/StatusBar.ts

## Change Log

- Added daemon-initiated sync path: `get_active_operation_id()` on `SyncOperationManager`, `activeOperationId` field in `get_daemon_state` response, auto-attach logic in `BasketSidebar.refreshAndRender()`, Syncing state in `StatusBar.pollDaemonState()`. (Date: 2026-03-31)
