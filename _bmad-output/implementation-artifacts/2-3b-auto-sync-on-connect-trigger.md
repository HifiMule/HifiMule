# Story 2.3b: Auto-Sync on Connect Trigger

Status: done

## Story

As a Convenience Seeker (Sarah),
I want the daemon to automatically start syncing when I plug in a known device with auto-sync enabled,
so that I can plug in and walk away without any interaction.

## Acceptance Criteria

1. **AC1: Auto-Sync Initiation** — Given a known device with `auto_sync_on_connect` enabled in its profile, when the device is detected and profile is loaded, then the daemon automatically initiates a sync operation using auto-fill selection or the last basket configuration.
2. **AC2: Tray Icon Syncing State** — When auto-sync starts, the tray icon transitions to "Syncing" state with updated tooltip. No UI interaction is required.
3. **AC3: Completion Notification** — When auto-sync completes, an OS-native notification is sent: "Sync Complete. Safe to eject." and the tray icon returns to "Idle" state.
4. **AC4: Error Handling** — When auto-sync fails (device disconnected mid-sync, no credentials, server unreachable), the tray icon transitions to "Error" state and an OS-native notification describes the failure.
5. **AC5: Auto-Sync Toggle Persistence** — The `auto_sync_on_connect` flag is stored both in SQLite device profile AND in the `.jellyfinsync.json` manifest on the device.
6. **AC6: RPC Configuration** — A JSON-RPC method allows the UI to enable/disable `auto_sync_on_connect` per device profile.
7. **AC7: Headless Operation** — Auto-sync works identically whether the UI is open or not (daemon-only mode).

## Tasks / Subtasks

- [x] Task 1: Add `auto_sync_on_connect` field to device profiles (AC: #5)
  - [x] 1.1 Add `auto_sync_on_connect BOOLEAN DEFAULT false` column to `device_profiles` table in `db.rs`
  - [x] 1.2 Add migration logic for existing databases (ALTER TABLE)
  - [x] 1.3 Extend `DeviceManifest` struct to parse/write `autoSyncOnConnect` from `.jellyfinsync.json`
  - [x] 1.4 Ensure manifest atomic write includes the new field via Write-Temp-Rename pattern
- [x] Task 2: Implement auto-sync controller in device event handler (AC: #1, #7)
  - [x] 2.1 In `main.rs` device event handler (`DeviceEvent::Detected`), after `handle_device_detected()`, check if device profile has `auto_sync_on_connect` enabled
  - [x] 2.2 If enabled, resolve sync source: use last basket items from manifest (`basket_items`) or trigger auto-fill if `auto_fill_enabled` is true
  - [x] 2.3 Call `SyncOperationManager` to start sync operation (reuse existing `sync_start` RPC logic)
  - [x] 2.4 Send `DaemonState::Syncing` through `state_tx` channel
- [x] Task 3: Tray icon and notification integration (AC: #2, #3, #4)
  - [x] 3.1 Ensure `DaemonState::Syncing` already triggers tray icon update (verify existing code in `run_interactive`)
  - [x] 3.2 Add OS-native notification on sync completion using `notify-rust` or platform notification API
  - [x] 3.3 Add OS-native notification on sync failure with error description
  - [x] 3.4 Transition tray icon back to Idle on completion, Error on failure
- [x] Task 4: RPC method for auto-sync configuration (AC: #6)
  - [x] 4.1 Add `device.setAutoSyncOnConnect` JSON-RPC method in `rpc.rs` — params: `{ deviceId, enabled }`
  - [x] 4.2 Update both SQLite profile AND device manifest atomically
  - [x] 4.3 Add `device.getProfile` RPC method or extend existing `get_daemon_state` to include auto-sync flag
- [x] Task 5: Sync completion monitoring (AC: #3, #4)
  - [x] 5.1 After spawning auto-sync task, spawn a monitoring task that polls `SyncOperationManager` for completion
  - [x] 5.2 On completion: send notification + update `DaemonState` to Idle
  - [x] 5.3 On failure: send error notification + update `DaemonState` to Error + log details via `daemon_log!`
- [x] Task 6: Testing (All ACs)
  - [x] 6.1 Unit test: `auto_sync_on_connect` DB column CRUD operations
  - [x] 6.2 Unit test: manifest serialization/deserialization with `autoSyncOnConnect` field
  - [x] 6.3 Integration test: device detection → auto-sync trigger flow (mock sync operation)
  - [x] 6.4 Integration test: auto-sync disabled → no sync triggered
  - [x] 6.5 Test: RPC `device.setAutoSyncOnConnect` updates both DB and manifest

## Dev Notes

### Architecture Patterns & Constraints

- **Multi-Process Architecture**: Daemon runs independently of UI. Auto-sync MUST work in daemon-only mode (headless). The UI is optional.
- **IPC**: JSON-RPC 2.0 over HTTP on `localhost:19140`. All JSON payloads use camelCase (`#[serde(rename_all = "camelCase")]`).
- **Async Runtime**: `tokio` — spawn auto-sync as `tokio::spawn` task. Use `tokio::task::spawn_blocking` for any SQLite operations.
- **State Broadcasting**: Send `DaemonState` variants through the existing `state_tx: mpsc::Sender<DaemonState>` channel. Tray icon handler already listens on the receiver.
- **Atomic Manifest Writes**: Always use the existing `DeviceManifest::write()` method which implements Write-Temp-Rename pattern (write to `.jellyfinsync.json.tmp`, `sync_all()`, rename).
- **Error Handling**: Use `thiserror` for typed errors, `anyhow` at binary level. Log errors via `daemon_log!` macro in release builds.
- **Logging**: Release mode writes to `%APPDATA%/JellyfinSync/daemon.log` via `daemon_log!`. No `println!` in release.

### Source Tree Components to Touch

| File | Purpose |
|------|---------|
| `jellyfinsync-daemon/src/db.rs` | Add `auto_sync_on_connect` column, migration, CRUD methods |
| `jellyfinsync-daemon/src/device/mod.rs` | Extend `DeviceManifest` struct with `auto_sync_on_connect` field |
| `jellyfinsync-daemon/src/main.rs` | Add auto-sync trigger logic in device event handler (lines ~179-233) |
| `jellyfinsync-daemon/src/rpc.rs` | Add `device.setAutoSyncOnConnect` RPC method |
| `jellyfinsync-daemon/src/sync.rs` | Reuse `SyncOperationManager` — no changes expected unless API needs exposure |
| `jellyfinsync-daemon/src/tests.rs` | Add integration tests for auto-sync flow |
| `jellyfinsync-daemon/Cargo.toml` | Add `notify-rust` dependency if not already present for OS notifications |

### Critical Implementation Details

1. **Sync Source Resolution Order**: When auto-sync triggers, determine what to sync:
   - If `basket_items` in manifest is non-empty → use those as the sync source (last configured basket)
   - Else if `auto_fill_enabled` is true → run auto-fill algorithm first, then sync
   - Else → log warning "Auto-sync enabled but no basket/auto-fill configured", skip sync, notify user

2. **Race Condition Prevention**: The device event handler already runs in a `tokio::spawn` task. Auto-sync should be spawned as a separate child task to avoid blocking the event handler from processing further events (e.g., device removal during sync).

3. **Device Removal During Sync**: The existing sync engine should handle IO errors gracefully. On `DeviceEvent::Removed`, if a sync is in progress, the `SyncOperationManager` should mark it as failed. Verify this behavior exists or add it.

4. **Duplicate Sync Prevention**: Before starting auto-sync, check `SyncOperationManager` for any active operation on the same device. If one exists, skip (device was likely reconnected quickly).

5. **Notification Library**: Check if `notify-rust` is already in `Cargo.toml`. If not, add it. It supports Windows (toast), macOS (NSUserNotification), and Linux (libnotify). Use title "JellyfinSync" and appropriate body text.

6. **Existing Scrobbler Pattern**: The scrobbler is already auto-triggered on device detection (main.rs lines 197-219). Follow the same pattern: check conditions → spawn async task → update shared state on completion.

### Previous Story Intelligence (2-3: Multi-Device Profile Mapping)

Key learnings from story 2-3 that directly apply:

- **DeviceManager** (`device/mod.rs`) centralizes device state and DB lookups — extend it for auto-sync checks
- **`get_daemon_state` RPC** method exists for UI polling — extend its response to include `autoSyncOnConnect` flag
- **Database** uses `rusqlite` with `spawn_blocking` pattern — follow same for new column operations
- **`paths` utility** (`paths.rs`) manages platform-standard AppData directories — reuse for any new paths
- **Integration tests** in `tests.rs` verify full detection-to-recognition flow — extend for detection-to-sync flow
- Credentials migrated from plaintext JSON to OS-native `keyring` — use `api::CredentialManager::get_credentials()` to check credentials before auto-sync

### Project Structure Notes

- All daemon code lives in `jellyfinsync-daemon/src/`
- Rust naming: `snake_case` for variables/functions
- JSON-RPC payloads: `camelCase` via `#[serde(rename_all = "camelCase")]`
- DB tables: `snake_case` plural (e.g., `device_profiles`)
- DB columns: `snake_case` (e.g., `auto_sync_on_connect`)
- Tests: co-located in `mod tests` blocks or `tests.rs`
- Use `tempfile` crate for database tests

### UX Requirements

- **Auto-sync toggle**: `<sl-switch>` in device profile panel with helper text: "Automatically start syncing when this device is connected. Works with or without the UI open."
- **Headless feedback**: Tray icon animation (syncing state) + OS-native notification on completion
- **With UI open**: Basket should reflect live sync state via `on_sync_progress` events, identical to manual "Start Sync" progress display
- **Sarah's Journey**: Plug in → Daemon detects → Auto-sync triggers → Background IO → OS notification "Sync Complete. Safe to Eject." → Unplug. Zero clicks.

### References

- [Source: _bmad-output/planning-artifacts/epics.md — Epic 2, Story 2.3]
- [Source: _bmad-output/planning-artifacts/architecture.md — Auto-Sync Controller, Device Profile Fields, Data Architecture]
- [Source: _bmad-output/planning-artifacts/ux-design-specification.md — Section 5.4 Device Profile Settings, Section 5.5 Headless Sync Feedback]
- [Source: _bmad-output/implementation-artifacts/2-3-multi-device-profile-mapping.md — DeviceManager patterns, DB patterns, testing approach]
- [Source: jellyfinsync-daemon/src/main.rs — Device event handler lines 179-233, scrobbler auto-trigger pattern lines 197-219]
- [Source: jellyfinsync-daemon/src/device/mod.rs — DeviceManifest struct, atomic write pattern, DeviceEvent enum]
- [Source: jellyfinsync-daemon/src/sync.rs — SyncOperationManager, SyncDelta, SyncOperation]
- [Source: jellyfinsync-daemon/src/rpc.rs — AppState struct, RPC routing]

## Dev Agent Record

### Agent Model Used
Claude Opus 4.6

### Debug Log References
- No debug issues encountered during implementation.

### Completion Notes List
- **Task 1**: Added `auto_sync_on_connect` boolean field to `DeviceMapping` DB struct, `devices` table (with ALTER TABLE migration for existing DBs), and `DeviceManifest` struct. Field defaults to `false`. Added `set_auto_sync_on_connect()` DB method.
- **Task 2**: Implemented auto-sync trigger in `main.rs` device event handler. After device detection, checks both manifest and DB for `auto_sync_on_connect` flag. Resolves basket items via Jellyfin API, calculates delta, and spawns sync operation. Follows existing scrobbler pattern.
- **Task 3**: Tray icon transitions handled via existing `DaemonState` channel (Syncing → Idle/Error). OS-native notifications via `notify-rust` on completion ("Sync Complete. Safe to eject.") and failure (error description).
- **Task 4**: Added `device_set_auto_sync_on_connect` JSON-RPC method (params: `deviceId`, `enabled`). Atomically updates both SQLite profile and device manifest on disk. Extended `get_daemon_state` response with `autoSyncOnConnect` field.
- **Task 5**: Sync completion monitoring integrated into `run_auto_sync` function — directly awaits `execute_sync` result and handles success/failure with notifications, state transitions, and logging via `daemon_log!`.
- **Task 6**: Added 9 new tests covering DB CRUD (3), manifest serde (4), integration detection flow (2), and RPC handler (2). All 134 tests pass with zero regressions.
- Refactored `SyncOperationManager` to be shared between RPC server and device event handler (created in `start_daemon_core`, passed to both consumers).

### Change Log
- 2026-03-18: Implemented Story 2.3b — Auto-Sync on Connect Trigger (all 6 tasks, all ACs satisfied)

### File List
- `jellyfinsync-daemon/src/db.rs` — Added `auto_sync_on_connect` field to `DeviceMapping`, column to `devices` table with migration, `set_auto_sync_on_connect()` method, 3 unit tests
- `jellyfinsync-daemon/src/device/mod.rs` — Added `auto_sync_on_connect` field to `DeviceManifest` struct, updated `initialize_device()`
- `jellyfinsync-daemon/src/device/tests.rs` — Added 4 tests for manifest serde with `auto_sync_on_connect`, updated all existing test struct literals
- `jellyfinsync-daemon/src/main.rs` — Added `run_auto_sync()` function, `to_desired_item()` helper, auto-sync trigger logic in device event handler, shared `SyncOperationManager` initialization
- `jellyfinsync-daemon/src/rpc.rs` — Added `handle_device_set_auto_sync_on_connect()` handler, `device_set_auto_sync_on_connect` route, extended `handle_get_daemon_state()` with `autoSyncOnConnect` field, updated `run_server()` signature to accept shared `SyncOperationManager`, 2 RPC tests
- `jellyfinsync-daemon/src/sync.rs` — Updated test `empty_manifest()` with new field
- `jellyfinsync-daemon/src/tests.rs` — Added 2 integration tests for device detection with auto-sync enabled/disabled, updated existing test struct literal
