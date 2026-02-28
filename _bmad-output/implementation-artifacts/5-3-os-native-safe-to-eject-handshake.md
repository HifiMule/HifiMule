# Story 5.3: OS-Native "Safe to Eject" Handshake

Status: ready-for-dev

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a **Convenience Seeker (Sarah)**,
I want a system notification the second my sync is done,
so that I can unplug and leave without checking the app.

## Acceptance Criteria

1. **OS-native notification on success**: When the final atomic manifest rename completes (sync operation status becomes `Complete`), the daemon fires an OS-native notification with the message **"Sync Complete. Ready to Run."** (AC: #1)

2. **Tray icon returns to Idle**: Immediately after the sync operation reaches `Complete` or `Failed` status, the tray icon returns to the green Idle state (or Error state on failure). During sync, the tray shows the Syncing state. (AC: #2)

3. **No notification on failure**: If sync fails, no "Sync Complete" notification is sent. The tray transitions to `Error` state instead of `Idle`. (AC: #3)

4. **Non-fatal notification errors**: If the OS notification system is unavailable (no dbus, locked screen, permission denied), the failure is logged but does NOT crash the daemon or affect sync result. (AC: #4)

## Tasks / Subtasks

- [ ] **T1: Add `notify-rust` dependency** (AC: #1)
  - [ ] T1.1: In `Cargo.toml` (workspace), add: `notify-rust = "~4.12"`
  - [ ] T1.2: In `jellysync-daemon/Cargo.toml`, add: `notify-rust.workspace = true`

- [ ] **T2: Add `state_tx` to `AppState` and wire tray state transitions** (AC: #2)
  - [ ] T2.1: In `rpc.rs`, add field to `AppState`:
    ```rust
    pub state_tx: std::sync::mpsc::Sender<crate::DaemonState>,
    ```
    Place after `last_scrobbler_result` for logical grouping.
  - [ ] T2.2: Update `run_server()` signature in `rpc.rs` to accept `state_tx`:
    ```rust
    pub async fn run_server(
        port: u16,
        db: Arc<crate::db::Database>,
        device_manager: Arc<crate::device::DeviceManager>,
        last_scrobbler_result: Arc<tokio::sync::RwLock<Option<crate::scrobbler::ScrobblerResult>>>,
        state_tx: std::sync::mpsc::Sender<crate::DaemonState>,
    ) {
    ```
  - [ ] T2.3: In `run_server()`, include `state_tx` in the `AppState { ... }` constructor literal.
  - [ ] T2.4: In `main.rs`, clone `state_tx` before the RPC spawn:
    ```rust
    let state_tx_rpc = state_tx.clone();
    tokio::spawn(async move {
        rpc::run_server(19140, db_clone, dm_clone, scrobbler_result_rpc, state_tx_rpc).await;
    });
    ```
    **CRITICAL**: `state_tx` is already moved into the daemon tokio block. Clone it BEFORE the `tokio::spawn` that runs `run_server` (line ~96 in main.rs), alongside the other `Arc::clone` calls already there.

- [ ] **T3: Implement `send_sync_complete_notification()` helper** (AC: #1, #4)
  - [ ] T3.1: Add a free function in `rpc.rs` (NOT in impl block):
    ```rust
    fn send_sync_complete_notification() {
        if let Err(e) = notify_rust::Notification::new()
            .summary("Sync Complete. Ready to Run.")
            .show()
        {
            eprintln!("[Notification] Failed to show OS notification: {}", e);
        }
    }
    ```
    Non-fatal: error is logged, not propagated. No return value.

- [ ] **T4: Wire state transitions and notification into `handle_sync_execute`** (AC: #1, #2, #3)
  - [ ] T4.1: At the top of `handle_sync_execute`, after extracting the device path and before spawning the background task, send Syncing state:
    ```rust
    let _ = state.state_tx.send(crate::DaemonState::Syncing);
    ```
    Place this immediately before the `tokio::spawn(async move { ... })` block.
  - [ ] T4.2: In the `tokio::spawn` background task, capture `state_tx` via clone:
    ```rust
    let state_tx = state.state_tx.clone();
    ```
    Add alongside the existing clones (`jellyfin_client`, `op_manager`, etc.) on lines ~854-857 in rpc.rs.
  - [ ] T4.3: In the `Ok((_synced_items, errors))` success arm, after updating operation status, add:
    ```rust
    // Notify OS and return tray to Idle — Story 5.3
    if errors.is_empty() {
        tokio::task::spawn_blocking(send_sync_complete_notification);
    }
    let _ = state_tx.send(crate::DaemonState::Idle);
    ```
    **Order matters**: notification fires only if `errors.is_empty()` (true Complete, not Failed-with-errors). Tray ALWAYS returns to Idle after the operation ends (even if there were partial errors).
  - [ ] T4.4: In the `Err(e)` failure arm, after marking operation as Failed, add:
    ```rust
    let _ = state_tx.send(crate::DaemonState::Error);
    ```
    On hard failure (no Ok), tray goes to Error state (not Idle).

- [ ] **T5: Verification** (AC: all)
  - [ ] T5.1: `cargo build` in workspace root — zero errors (confirms new dependency resolves and API compiles correctly).
  - [ ] T5.2: `cargo test` in `jellysync-daemon/` — all existing 96 tests pass, zero regressions.
  - [ ] T5.3: Manual — run a sync, verify: (a) tray icon changes to Syncing during sync, (b) OS notification "Sync Complete. Ready to Run." appears on completion, (c) tray returns to Idle (green).
  - [ ] T5.4: Manual — simulate a sync failure (disconnect device mid-sync), verify: (a) no notification, (b) tray shows Error state.

## Dev Notes

### Critical Architecture Constraints

**`state_tx` threading model — MANDATORY:**
- `state_tx` is `std::sync::mpsc::Sender<DaemonState>` — it is `Send + Clone`, safe to clone into async tasks.
- Do NOT convert to `tokio::sync::mpsc` — the receiver is on the synchronous tao event loop main thread. Using tokio channels would require the main thread to await, which is incompatible with the blocking tao event loop.
- Pattern: clone before spawn, send is fire-and-forget (`let _ = state_tx.send(...)` — ignore send errors gracefully, as they only occur if the event loop has already shut down).

**`notify-rust` threading model — MANDATORY:**
- `notify_rust::Notification::show()` is SYNCHRONOUS and may block waiting for the notification daemon (especially on Linux with D-Bus).
- MUST use `tokio::task::spawn_blocking(send_sync_complete_notification)` to avoid blocking the tokio async runtime thread.
- Fire-and-forget: do NOT `.await` the JoinHandle. The notification is best-effort; we don't need to wait for the OS to display it before returning.

**No new async or `await` on notification path**: The notification function itself is `fn` (not `async fn`). Only the `spawn_blocking` wrapper is async.

### Notification Library Decision: `notify-rust` v4.12.0

`notify-rust` is the standard cross-platform Rust notification crate:
- **Linux**: D-Bus/libnotify — works natively in daemon context.
- **macOS**: NSUserNotification (or UserNotifications framework on macOS 11+).
- **Windows**: WinRT toast notifications via `tauri-winrt-notification` (bundled dependency).

**API used (minimal, no platform-specific features):**
```rust
notify_rust::Notification::new()
    .summary("Sync Complete. Ready to Run.")
    .show()
```
Do NOT use `.body()`, `.icon()`, `.hint()`, or `.timeout()` — these have platform-specific behavior and some don't compile on Windows. The one-liner `summary`-only call is cross-platform and sufficient for this story.

**Why not Tauri notification plugin?**: The notification is fired from the daemon process (`jellysync-daemon`), not from the Tauri UI. Using `notify-rust` directly in the daemon is the correct pattern — no Tauri dependency needed.

### `DaemonState` Tray Transitions for Sync (Story 5.3)

The existing tray state machine in `main.rs` already handles all needed states. Story 5.3 wires up the missing transitions:

| Event | State Sent | Tray Result |
|-------|-----------|-------------|
| `sync_execute` called | `DaemonState::Syncing` | Spinning icon, "Syncing..." tooltip |
| Sync completes (no errors) | `DaemonState::Idle` | Green icon, "Idle" tooltip |
| Sync completes (partial errors but `Ok`) | `DaemonState::Idle` | Green icon, "Idle" tooltip |
| Sync fails (`Err`) | `DaemonState::Error` | Error icon, "Error!" tooltip |

Note: `DaemonState::Syncing` already exists in `main.rs` and the event loop already handles it (line 210-212 in main.rs). No changes needed to main.rs event loop — only wiring the send calls.

### Source Tree Components to Touch

**Files to MODIFY (4 files):**
1. [Cargo.toml](Cargo.toml) — Add `notify-rust = "~4.12"` to `[workspace.dependencies]`
2. [jellysync-daemon/Cargo.toml](jellysync-daemon/Cargo.toml) — Add `notify-rust.workspace = true` to `[dependencies]`
3. [jellysync-daemon/src/rpc.rs](jellysync-daemon/src/rpc.rs) — Add `state_tx` to `AppState`, update `run_server()` signature, add `send_sync_complete_notification()`, wire state sends + notification in `handle_sync_execute`
4. [jellysync-daemon/src/main.rs](jellysync-daemon/src/main.rs) — Clone `state_tx` for RPC server, pass to `run_server()`

**Files NOT to modify:**
- `sync.rs`, `db.rs`, `scrobbler.rs`, `api.rs`, `device/mod.rs`, `paths.rs` — no changes needed
- Any TypeScript / frontend files — OS notification is daemon-only
- `tests.rs` — no new integration tests needed (existing 96 tests verify no regression)

### Precise Code Location References

**main.rs changes:**
- Line 96-101 (tokio::spawn for RPC server): Clone `state_tx` here and pass to `run_server()`. The existing structure is:
  ```rust
  let db_clone = Arc::clone(&db);
  let dm_clone = Arc::clone(&device_manager);
  let scrobbler_result_rpc = Arc::clone(&last_scrobbler_result);
  tokio::spawn(async move {
      rpc::run_server(19140, db_clone, dm_clone, scrobbler_result_rpc).await;  // ← add state_tx_rpc arg
  });
  ```
  Add `let state_tx_rpc = state_tx.clone();` before this block.

**rpc.rs AppState (line 54-63):**
  ```rust
  pub struct AppState {
      pub jellyfin_client: JellyfinClient,
      pub db: Arc<crate::db::Database>,
      pub device_manager: Arc<crate::device::DeviceManager>,
      pub last_connection_check: Arc<tokio::sync::Mutex<Option<(std::time::Instant, bool)>>>,
      pub size_cache: Arc<tokio::sync::RwLock<HashMap<String, u64>>>,
      pub sync_operation_manager: Arc<crate::sync::SyncOperationManager>,
      pub last_scrobbler_result: Arc<tokio::sync::RwLock<Option<crate::scrobbler::ScrobblerResult>>>,
      // ← add: pub state_tx: std::sync::mpsc::Sender<crate::DaemonState>,
  }
  ```

**rpc.rs `handle_sync_execute` — background task (lines 859-910):**
  The existing clones block at lines ~854-857:
  ```rust
  let jellyfin_client = state.jellyfin_client.clone();
  let op_manager = state.sync_operation_manager.clone();
  let op_id = operation_id.clone();
  let device_manager = state.device_manager.clone();
  // ← add: let state_tx = state.state_tx.clone();
  ```
  After all existing clones, add `let _ = state.state_tx.send(crate::DaemonState::Syncing);` (before `tokio::spawn`).

  In success arm (after `op_manager.update_operation` call, ~line 894):
  ```rust
  // Notify OS and return tray to Idle — Story 5.3
  if errors.is_empty() {
      tokio::task::spawn_blocking(send_sync_complete_notification);
  }
  let _ = state_tx.send(crate::DaemonState::Idle);
  ```

  In error arm (after `op_manager.update_operation` call, ~line 907):
  ```rust
  let _ = state_tx.send(crate::DaemonState::Error);
  ```

### Testing Standards Summary

- **No new unit tests**: The tray transition and OS notification are integration-level behaviors that require a running event loop and OS notification subsystem. These can't be unit-tested in the existing `mod tests { }` pattern.
- **Regression guarantee**: `cargo test` with all 96 existing tests passing is the quality gate. The changes are additive (new field in AppState, new function, new sends) with zero changes to existing logic.
- **Compile-time verification**: `cargo build` is the primary quality signal — if `notify-rust` API is misused (wrong method names, wrong feature gates), it fails at compile time.
- **`spawn_blocking` doesn't need test**: It's a standard tokio utility; testing it would be testing tokio itself.

### Previous Story Intelligence (Story 5.2 → 5.3)

From Story 5.2 completion notes and review:
- **96 tests pass** as of Story 5.2 review completion (`5ed8dbc Review 5.2`). Story 5.3 target: 96 tests still pass (no new unit tests added).
- **Non-fatal error pattern established**: `Ok(false)` → proceed; `Err(e)` → log + proceed. Story 5.3 follows the same philosophy for notification failures: log the error, don't propagate.
- **`AppState` extension pattern**: Story 5.2 showed that `AppState` is only modified in `rpc.rs` — it is NOT shared with scrobbler.rs or other modules. Adding `state_tx` continues this pattern.
- **Fire-and-forget background tasks**: The scrobbler spawn in `main.rs` (line 129) and the sync execute spawn in `rpc.rs` (line 859) both use `tokio::spawn` without awaiting. The notification `spawn_blocking` follows the same fire-and-forget philosophy.
- **No TypeScript changes**: Story 5.2 established that scrobbler changes are daemon-only. Story 5.3 is also daemon-only. The UI already handles sync completion display via polling (`BasketSidebar.ts` `handleSyncComplete()` at ~line 500). OS notification is a parallel, additive UX layer.

### Git Intelligence

Recent commits:
- `5ed8dbc Review 5.2` — Story 5.2 complete. 96 tests pass. `db.rs` and `scrobbler.rs` are the modified files. Source tree is in final reviewed state.
- `cea7f93 Dev 5.2` — Implementation commit. Established dedup pre-check pattern.
- `ecb5aca Story 5.2` — Story file created. Established Story 5.2 story doc format.

No open technical debt affecting Story 5.3.

### Project Structure Notes

**Alignment with Unified Structure:**
- `send_sync_complete_notification()` as a free function (not `pub`, not `async`) follows the established `now_iso8601()` pattern in `sync.rs`.
- `state_tx: std::sync::mpsc::Sender<crate::DaemonState>` in `AppState` follows the `Arc<...>` wrapping pattern already used for `last_connection_check` and `sync_operation_manager`.
- The `state_tx` clone pattern in `main.rs` mirrors the existing `let state_tx_clone = state_tx.clone();` at line 104.

**No structural conflicts**: Story 5.3 is purely additive. No existing function signatures, method behaviors, or test expectations change.

### References

- [Source: epics.md#story-53-os-native-safe-to-eject-handshake] — Story requirements and AC
- [Source: epics.md#epic-5-ecosystem-lifecycle--advanced-tools] — Epic 5 objectives (FR23: OS-Native Sync Notifications)
- [Source: architecture.md#api--communication-patterns] — JSON-RPC 2.0, Request-Response-Event pattern
- [Source: architecture.md#process-patterns] — Error handling: `anyhow` at binary level
- [jellysync-daemon/src/main.rs:28-35](jellysync-daemon/src/main.rs#L28-L35) — `DaemonState` enum — `Syncing` and `Idle` variants already defined
- [jellysync-daemon/src/main.rs:40](jellysync-daemon/src/main.rs#L40) — `state_tx` creation point
- [jellysync-daemon/src/main.rs:96-101](jellysync-daemon/src/main.rs#L96-L101) — RPC server spawn — add `state_tx` clone and pass here
- [jellysync-daemon/src/main.rs:206-213](jellysync-daemon/src/main.rs#L206-L213) — Existing Syncing/Idle tray handlers — no changes needed
- [jellysync-daemon/src/rpc.rs:54-63](jellysync-daemon/src/rpc.rs#L54-L63) — `AppState` struct — add `state_tx` field
- [jellysync-daemon/src/rpc.rs:65-80](jellysync-daemon/src/rpc.rs#L65-L80) — `run_server()` — add `state_tx` parameter
- [jellysync-daemon/src/rpc.rs:854-870](jellysync-daemon/src/rpc.rs#L854-L870) — `handle_sync_execute` clones + spawn — add `state_tx` clone + Syncing send
- [jellysync-daemon/src/rpc.rs:873-895](jellysync-daemon/src/rpc.rs#L873-L895) — Sync success arm — add notification + Idle send
- [jellysync-daemon/src/rpc.rs:897-909](jellysync-daemon/src/rpc.rs#L897-L909) — Sync failure arm — add Error send
- [notify-rust docs](https://docs.rs/notify-rust/4.12.0/notify_rust/) — v4.12.0 API reference

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

### Completion Notes List

### File List
