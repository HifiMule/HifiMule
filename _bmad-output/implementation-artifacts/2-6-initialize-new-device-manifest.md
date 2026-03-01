# Story 2.6: Initialize New Device Manifest

Status: ready-for-dev

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a Ritualist (Arthur) and Convenience Seeker (Sarah),
I want the application to detect when a connected removable disk has no `.jellysync.json` manifest and guide me through initializing it,
so that I can bring a brand-new device into the managed sync model without manually creating any files.

## Acceptance Criteria

1. **Unrecognized Device Detection:**
   - **Given** a USB mass storage device is connected with no `.jellysync.json` present in its root
   - **When** the daemon completes its device discovery scan
   - **Then** it broadcasts an `on_device_unrecognized` event (via a new `DeviceEvent::Unrecognized { path }` internal event)
   - **And** the daemon state transitions to `DeviceFound(path_string)`
   - **And** `get_daemon_state` RPC returns a `pendingDevicePath` field with the device root path
   - **And** the UI displays an "Initialize Device" banner in the Basket Sidebar's Device State panel

2. **Initialization Dialog:**
   - **Given** the "Initialize Device" banner is visible
   - **When** I click "Initialize"
   - **Then** a `sl-dialog` appears (matching the `RepairModal` pattern) prompting me to:
     - Confirm or change the sync folder name on the device (default: empty = device root)
     - The currently logged-in Jellyfin user is shown as the associated profile (read-only display)
   - **When** I click "Confirm"
   - **Then** the UI sends a `device_initialize` JSON-RPC request to the daemon with `{ "folderPath": "<device_root_or_subfolder>", "profileId": "<user_id>" }`

3. **Daemon Initialization:**
   - **Given** a valid `device_initialize` RPC call
   - **When** the daemon processes the request
   - **Then** it generates a new unique hardware ID (UUID v4)
   - **And** it writes an initial `.jellysync.json` to the device using the **atomic Write-Temp-Rename pattern** (`write_manifest`)
   - **And** the manifest contains: `device_id` (new UUID), `version: "1.0"`, `managed_paths` derived from the chosen folder, `synced_items: []`, `dirty: false`
   - **And** if a non-root folder was specified, the daemon creates that folder on the device if it doesn't exist
   - **And** the device profile mapping is stored in SQLite via `db.upsert_device_mapping`
   - **And** the daemon transitions to the normal recognized state (`DeviceRecognized`)
   - **And** the UI transitions to the sync-ready state (banner disappears, Device Folders panel shows normally)

4. **Error Handling:**
   - **When** the initialization fails (e.g., device is read-only, disk full, or invalid folder name)
   - **Then** the dialog displays a clear error message
   - **And** shows a "Retry" or "Dismiss" option

5. **Removable-Only Detection (Windows):**
   - **Given** a Windows environment with fixed drives (C:\, D:\) and a USB device (e.g., E:\)
   - **When** the observer starts
   - **Then** `DeviceEvent::Unrecognized` is ONLY sent for removable drives (type 2 per `GetDriveTypeW`)
   - **And** fixed/network/system drives are silently skipped even if they have no manifest

## Tasks / Subtasks

- [ ] **Backend: Add `DeviceEvent::Unrecognized` and observer update** (AC: #1, #5)
  - [ ] Add `Unrecognized { path: PathBuf }` variant to `DeviceEvent` enum in `device/mod.rs`
  - [ ] Add Windows-only `is_removable_drive(path: &Path) -> bool` helper using `GetDriveTypeW`
  - [ ] Modify `run_observer` in `device/mod.rs`: when `DeviceProber::probe` returns `Ok(None)`, check `is_removable_drive` (Windows) or use existing mount-point filtering (macOS/Linux), then send `DeviceEvent::Unrecognized { path }`
  - [ ] Handle `DeviceEvent::Unrecognized` in `main.rs`: call `device_manager.handle_device_unrecognized(path)` and send `DaemonState::DeviceFound(path_string)` via `state_tx`

- [ ] **Backend: `DeviceManager` changes** (AC: #1, #3)
  - [ ] Add `unrecognized_device_path: Arc<RwLock<Option<PathBuf>>>` field to `DeviceManager`
  - [ ] Add `handle_device_unrecognized(path: PathBuf) -> DaemonState` method — stores path and returns `DaemonState::DeviceFound(path.to_string_lossy().to_string())`
  - [ ] Add `get_unrecognized_device_path() -> Option<PathBuf>` method
  - [ ] Modify `handle_device_removed` to also clear `unrecognized_device_path`
  - [ ] Modify `list_root_folders` to also work when `current_device_path` is `None` but `unrecognized_device_path` is set (so the init dialog can list folders)
  - [ ] Add `initialize_device(folder_path: &str, profile_id: &str) -> Result<DeviceManifest>` method that: generates UUID, constructs `DeviceManifest`, calls `write_manifest`, optionally creates the target folder, clears `unrecognized_device_path`, sets `current_device` and `current_device_path`

- [ ] **Backend: RPC changes** (AC: #1, #3, #4)
  - [ ] Add `"device_initialize"` branch to `handler` match in `rpc.rs`
  - [ ] Implement `handle_device_initialize(state, params)`: extract `folderPath` and `profileId` params, call `device_manager.initialize_device`, call `db.upsert_device_mapping`, send `DaemonState::DeviceRecognized` via `state.state_tx`, return success
  - [ ] Modify `handle_get_daemon_state` to include `pendingDevicePath: Option<String>` in response (from `device_manager.get_unrecognized_device_path()`)
  - [ ] Add unit tests for `handle_device_initialize` (success, read-only error, invalid folder)

- [ ] **Frontend: "Initialize Device" banner in `BasketSidebar`** (AC: #1, #2)
  - [ ] Update `RootFoldersResponse` interface to add `pendingDevicePath?: string` (from get_daemon_state polling OR pass through list_root_folders when hasManifest is false)
  - [ ] In `renderDeviceFolders()`: when `hasManifest` is `false` (device connected but no manifest), render "Initialize Device" banner with an "Initialize" button (style similar to `dirty-manifest-banner`)
  - [ ] Wire up the "Initialize" button click to open the `InitDeviceModal`

- [ ] **Frontend: New `InitDeviceModal` component** (AC: #2, #3, #4)
  - [ ] Create `jellysync-ui/src/components/InitDeviceModal.ts` following the `RepairModal.ts` pattern
  - [ ] Render `sl-dialog` with:
    - Device path display (non-editable)
    - `sl-input` for sync folder name (placeholder: "Leave empty for device root", default: empty)
    - Profile display: show logged-in user ID (from `get_credentials` RPC call)
    - "Confirm" button (calls `device_initialize` RPC)
    - "Cancel" button (closes dialog)
  - [ ] On `device_initialize` success: close dialog, call `onComplete` callback to refresh device state
  - [ ] On error: show `sl-alert` with error message and Retry/Dismiss options

- [ ] **Frontend: Refresh after initialization** (AC: #3)
  - [ ] Ensure `BasketSidebar.refreshDeviceData()` is called after successful initialization
  - [ ] The "Initialize Device" banner must disappear once `hasManifest` becomes `true`

## Dev Notes

### Architecture & Pattern Compliance

- **Atomic Manifest Write:** MUST use the existing `device::write_manifest(device_root, &manifest)` function — it already implements the Write-Temp-Rename pattern with `sync_all`. Do NOT write the manifest any other way.
- **UUID Generation:** `uuid` crate with `v4` feature is already in `jellysync-daemon/Cargo.toml`. Use `uuid::Uuid::new_v4().to_string()` for `device_id`.
- **RPC Pattern:** Follow the exact pattern in `rpc.rs` — `handle_device_initialize` is an `async fn(state: &AppState, params: Option<Value>) -> Result<Value, JsonRpcError>`. Return `Ok(serde_json::json!({"status": "success", "data": {...}}))` on success.
- **State Updates from RPC:** `state.state_tx.send(DaemonState::...)` is the approved channel — the field exists in `AppState` (see `rpc.rs:90`).
- **UI Modal Pattern:** Follow `RepairModal.ts` exactly — create an `sl-dialog` element, append to container, call `.show()`. Use `sl-spinner` for loading and `sl-alert` for errors.
- **camelCase IPC:** All JSON-RPC fields MUST be `camelCase` (e.g., `folderPath`, `profileId`, `pendingDevicePath`) per architecture mandate. Rust structs must use `#[serde(rename_all = "camelCase")]`.

### Critical Windows Guardrail

On Windows, `get_mounts()` returns ALL drive letters (A:\ through Z:\) via `GetLogicalDrives`. Before sending `DeviceEvent::Unrecognized`, the observer MUST call `GetDriveTypeW` to filter out non-removable drives. Only drives returning type `2` (DRIVE_REMOVABLE) should trigger the event. C:\ (type 3 = fixed) must NEVER trigger `DeviceEvent::Unrecognized`.

```rust
// Windows-only guard in run_observer
#[cfg(target_os = "windows")]
fn is_removable_drive(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDriveTypeW;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    unsafe { GetDriveTypeW(wide.as_ptr()) == 2 } // DRIVE_REMOVABLE
}
#[cfg(not(target_os = "windows"))]
fn is_removable_drive(_path: &Path) -> bool { true } // macOS/Linux already filtered by mount detection
```

### `managed_paths` Semantics

The `managed_paths` field in `DeviceManifest` stores relative folder names (e.g., `["Music"]`), not full paths. The logic for `initialize_device`:
- If `folder_path` param is empty or equals device root → `managed_paths = []` (treat as root-level, all files belong to managed zone)
- If `folder_path` is a subfolder name (e.g., `"Music"`) → `managed_paths = ["Music"]` and create `E:\Music` if it doesn't exist

### `get_daemon_state` Extension

The new `pendingDevicePath` field in the RPC response bridges the detection → initialization gap. The UI should check:
```typescript
if (!state.currentDevice && state.pendingDevicePath) {
    // Show "Initialize Device" banner
}
```

### DeviceManager Field Addition

The `unrecognized_device_path` field is parallel to `current_device_path` — both are `Arc<RwLock<Option<PathBuf>>>`. Key rule: **they are mutually exclusive**. When `handle_device_detected` succeeds, `current_device_path` is set and `unrecognized_device_path` must be `None` (and vice versa). `handle_device_removed` must clear BOTH.

### Profile ID Simplification

The current architecture supports a single Jellyfin user login. The "profile selector" in the dialog should:
1. Call `get_credentials` to get the current `userId` and `serverUrl`
2. Display the userId as the "currently linked profile" (non-interactive, read-only)
3. Pass this `userId` as `profileId` in the `device_initialize` RPC call

If no user is logged in, the "Initialize" button should show a message: "Connect to Jellyfin first".

### `list_root_folders` for Unrecognized Devices

Modify `DeviceManager::list_root_folders` to fall through to `unrecognized_device_path` when `current_device_path` is `None`:
```rust
let device_path = match self.get_current_device_path().await {
    Some(p) => p,
    None => match self.get_unrecognized_device_path().await {
        Some(p) => p,
        None => return Ok(None),
    }
};
```
This allows the init dialog to list existing folders on the device before writing the manifest.

### Previous Story Intelligence

From Story 2.5 (`2-5-interactive-login-and-identity-management.md`) implementation notes:
- Login stores `AccessToken` in OS keyring via `CredentialManager::save_credentials`
- Config (`config.json`) stores `serverUrl` and `userId` (from `AuthenticationResult`)
- `CredentialManager::get_credentials()` returns `(url, token, Option<user_id>)` — the `user_id` is the `Option<String>`
- The UI uses `rpcCall('get_credentials')` to retrieve credentials for display
- **Device ID pattern:** A persistent `device_id` was added to `api.rs` config for JellyfinSync's own client identity — the new manifest `device_id` is DIFFERENT (it's the target hardware's ID, not JellyfinSync's client ID)

From Story 2.2 (`2-2-mass-storage-heartbeat-autodetection.md`): The device observer pattern in `device/mod.rs` is established. The `DeviceProber::probe` → `DeviceEvent::Detected` flow is the template to extend.

### Git Intelligence (Recent Commits)

- `e2f9903 Add story for creating .jellysync.json` — This is the story we're implementing
- `3677f2d Done` — Story 5.4 (Visual Manifest Repair Utility) completed
- `067ec1e Review 5.4` / `434197a Code 5.4` — RepairModal.ts was completed in these commits; use it as the UI modal template

The RepairModal.ts pattern (Shoelace `sl-dialog`, class-based, `open()` method, `onComplete` callback) is confirmed working and is the correct template for `InitDeviceModal.ts`.

### File Structure

- `jellysync-daemon/src/device/mod.rs` — Add `DeviceEvent::Unrecognized`, `DeviceManager.unrecognized_device_path`, `handle_device_unrecognized`, `get_unrecognized_device_path`, `initialize_device`, update `run_observer`, update `list_root_folders`, update `handle_device_removed`
- `jellysync-daemon/src/main.rs` — Add `DeviceEvent::Unrecognized` handler arm
- `jellysync-daemon/src/rpc.rs` — Add `device_initialize` dispatch, implement `handle_device_initialize`, update `handle_get_daemon_state`
- `jellysync-ui/src/components/InitDeviceModal.ts` — New component (follow RepairModal.ts pattern)
- `jellysync-ui/src/components/BasketSidebar.ts` — Add `has_manifest: false` banner rendering and InitDeviceModal integration

### References

- Architecture: [Source: _bmad-output/planning-artifacts/architecture.md#Safety & Atomicity Patterns]
- Write-Temp-Rename: `device/mod.rs:write_manifest` (lines 41-56)
- DeviceEvent enum: `device/mod.rs:108-115`
- run_observer: `device/mod.rs:623-656`
- get_mounts Windows: `device/mod.rs:674-685`
- DeviceManager.handle_device_detected: `device/mod.rs:147-178`
- DeviceManager.handle_device_removed: `device/mod.rs:181-187`
- DeviceManager.list_root_folders: `device/mod.rs:217-287`
- AppState / run_server: `rpc.rs:75-117`
- handler dispatch: `rpc.rs:119-172`
- handle_get_daemon_state: `rpc.rs:341-361`
- RepairModal pattern: `jellysync-ui/src/components/RepairModal.ts`
- BasketSidebar device rendering: `jellysync-ui/src/components/BasketSidebar.ts:203-265`
- DB upsert: `db.rs:153-173` — `upsert_device_mapping(id, name, user_id, rules)`
- uuid v4: `jellysync-daemon/Cargo.toml:30` — already available
- windows_sys GetDriveTypeW: already imported in `device/mod.rs` via `GetLogicalDrives` (same module)

## Dev Agent Record

### Agent Model Used

{{agent_model_name_version}}

### Debug Log References

### Completion Notes List

### File List
