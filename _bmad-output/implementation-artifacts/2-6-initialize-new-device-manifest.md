# Story 2.6: Initialize New Device Manifest

Status: ready-for-dev

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a Ritualist (Arthur) and Convenience Seeker (Sarah),
I want the application to detect when a connected removable disk has no `.jellyfinsync.json` manifest and guide me through initializing it,
so that I can bring a brand-new device into the managed sync model without manually creating any files.

## Acceptance Criteria

1. **Unrecognized Device Detection:**
   - **Given** a USB mass storage device is connected with no `.jellyfinsync.json` present in its root
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
   - **And** it writes an initial `.jellyfinsync.json` to the device using the **atomic Write-Temp-Rename pattern** (`write_manifest`)
   - **And** the manifest contains: `device_id` (new UUID), `version: "1.0"`, `managed_paths` derived from the chosen folder, `synced_items: []`, `dirty: false`
   - **And** if a non-root folder was specified, the daemon creates that folder on the device if it doesn't exist
   - **And** the device profile mapping is stored in SQLite via `db.upsert_device_mapping`
   - **And** the daemon transitions to the normal recognized state (`DeviceRecognized`)
   - **And** the UI transitions to the sync-ready state (banner disappears, Device Folders panel shows normally)

4. **MTP Device Manifest Write (Sprint Change 2026-04-30):**
   - **Given** the target device is an MTP device
   - **When** the daemon writes the initial `.jellyfinsync.json`
   - **Then** it uses `device_io.write_with_verify()` instead of calling `write_manifest` (Write-Temp-Rename) directly
   - **And** the `device.initialize` RPC handler receives `Arc<dyn DeviceIO>` from `DeviceManager` — no direct `std::fs` calls in the handler
   - **Note:** `write_with_verify()` delegates to Write-Temp-Rename for MSC and dirty-marker + overwrite for MTP (defined in Story 4.0)

5. **Error Handling:**
   - **When** the initialization fails (e.g., device is read-only, disk full, or invalid folder name)
   - **Then** the dialog displays a clear error message
   - **And** shows a "Retry" or "Dismiss" option

5. **Removable-Only Detection (Windows):**
   - **Given** a Windows environment with fixed drives (C:\, D:\) and a USB device (e.g., E:\)
   - **When** the observer starts
   - **Then** `DeviceEvent::Unrecognized` is ONLY sent for removable drives (type 2 per `GetDriveTypeW`)
   - **And** fixed/network/system drives are silently skipped even if they have no manifest

## Tasks / Subtasks

- [x] **Backend: Add `DeviceEvent::Unrecognized` and observer update** (AC: #1, #5)
  - [x] Add `Unrecognized { path: PathBuf }` variant to `DeviceEvent` enum in `device/mod.rs`
  - [x] Add Windows-only `is_removable_drive(path: &Path) -> bool` helper using `GetDriveTypeW`
  - [x] Modify `run_observer` in `device/mod.rs`: when `DeviceProber::probe` returns `Ok(None)`, check `is_removable_drive` (Windows) or use existing mount-point filtering (macOS/Linux), then send `DeviceEvent::Unrecognized { path }`
  - [x] Handle `DeviceEvent::Unrecognized` in `main.rs`: call `device_manager.handle_device_unrecognized(path)` and send `DaemonState::DeviceFound(path_string)` via `state_tx`

- [x] **Backend: `DeviceManager` changes** (AC: #1, #3)
  - [x] Add `unrecognized_device_path: Arc<RwLock<Option<PathBuf>>>` field to `DeviceManager`
  - [x] Add `handle_device_unrecognized(path: PathBuf) -> DaemonState` method — stores path and returns `DaemonState::DeviceFound(path.to_string_lossy().to_string())`
  - [x] Add `get_unrecognized_device_path() -> Option<PathBuf>` method
  - [x] Modify `handle_device_removed` to also clear `unrecognized_device_path`
  - [x] Modify `list_root_folders` to also work when `current_device_path` is `None` but `unrecognized_device_path` is set (so the init dialog can list folders)
  - [x] Add `initialize_device(folder_path: &str, profile_id: &str) -> Result<DeviceManifest>` method that: generates UUID, constructs `DeviceManifest`, calls `write_manifest`, optionally creates the target folder, clears `unrecognized_device_path`, sets `current_device` and `current_device_path`

- [x] **Backend: RPC changes** (AC: #1, #3, #4)
  - [x] Add `"device_initialize"` branch to `handler` match in `rpc.rs`
  - [x] Implement `handle_device_initialize(state, params)`: extract `folderPath` and `profileId` params, call `device_manager.initialize_device`, call `db.upsert_device_mapping`, send `DaemonState::DeviceRecognized` via `state.state_tx`, return success
  - [x] Modify `handle_get_daemon_state` to include `pendingDevicePath: Option<String>` in response (from `device_manager.get_unrecognized_device_path()`)
  - [x] Add unit tests for `handle_device_initialize` (success, read-only error, invalid folder)

- [x] **Frontend: "Initialize Device" banner in `BasketSidebar`** (AC: #1, #2)
  - [x] Update `RootFoldersResponse` interface to add `pendingDevicePath?: string` (from get_daemon_state polling OR pass through list_root_folders when hasManifest is false)
  - [x] In `renderDeviceFolders()`: when `hasManifest` is `false` (device connected but no manifest), render "Initialize Device" banner with an "Initialize" button (style similar to `dirty-manifest-banner`)
  - [x] Wire up the "Initialize" button click to open the `InitDeviceModal`

- [x] **Frontend: New `InitDeviceModal` component** (AC: #2, #3, #4)
  - [x] Create `jellyfinsync-ui/src/components/InitDeviceModal.ts` following the `RepairModal.ts` pattern
  - [x] Render `sl-dialog` with:
    - Device path display (non-editable)
    - `sl-input` for sync folder name (placeholder: "Leave empty for device root", default: empty)
    - Profile display: show logged-in user ID (from `get_credentials` RPC call)
    - "Confirm" button (calls `device_initialize` RPC)
    - "Cancel" button (closes dialog)
  - [x] On `device_initialize` success: close dialog, call `onComplete` callback to refresh device state
  - [x] On error: show `sl-alert` with error message and Retry/Dismiss options

- [ ] **MTP: Replace direct manifest write with DeviceIO (AC: #4 — Sprint Change 2026-04-30)**
  - [ ] Replace `device::write_manifest(device_root, &manifest)` call in `initialize_device()` with `device_io.write_with_verify(path, &manifest_bytes)`
  - [ ] Pass `Arc<dyn DeviceIO>` into `initialize_device()` from the `device.initialize` RPC handler (retrieve from `DeviceManager` by device path)
  - [ ] Verify existing MSC behavior is unchanged (MscBackend.write_with_verify delegates to Write-Temp-Rename)
  - **Depends on:** Story 4.0 (DeviceIO abstraction layer)

- [x] **Frontend: Refresh after initialization** (AC: #3)
  - [x] Ensure `BasketSidebar.refreshDeviceData()` is called after successful initialization
  - [x] The "Initialize Device" banner must disappear once `hasManifest` becomes `true`

## Dev Notes

### Current Implementation State (as of 2026-05-01)

**All original tasks (ACs #1–3, #5–6) are DONE.** The only remaining work is the MTP manifest write task (AC #4), which depends on Story 4.0.

Key current code locations (updated line numbers):
- `DeviceEvent` enum: `device/mod.rs:154–163`
- `DeviceManager` struct + fields: `device/mod.rs:181–188`
- `handle_device_unrecognized`: `device/mod.rs:242–255`
- `initialize_device()` (current signature): `device/mod.rs:361–440`
  - Calls `write_manifest(&device_root, &manifest).await?` at line 420 — **this is the line to replace for AC #4**
- `run_observer`: `device/mod.rs:884–930`
- `is_removable_drive`: `device/mod.rs:867–882`
- `handle_device_initialize` RPC: `rpc.rs:1387–1468`
  - Calls `device_manager.initialize_device(folder_path, transcoding_profile_id, device_name, device_icon).await` — **this caller must pass `Arc<dyn DeviceIO>` once Story 4.0 adds it**
- `BasketSidebar` banner: `BasketSidebar.ts:480–495`
- `InitDeviceModal`: `jellyfinsync-ui/src/components/InitDeviceModal.ts` (fully implemented)

### Architecture & Pattern Compliance

- **Atomic Manifest Write (MSC):** Current code uses `device::write_manifest()` — Write-Temp-Rename with `sync_all`. Story 4.0 will make `MscBackend::write_with_verify()` wrap this exact pattern. MSC behavior must be preserved unchanged.
- **MTP Write (post-Story 4.0):** `MtpBackend::write_with_verify()` uses dirty-marker + overwrite (no rename op available over MTP).
- **UUID Generation:** `uuid` crate with `v4` feature already in `Cargo.toml`. `uuid::Uuid::new_v4().to_string()` — already used in current `initialize_device()`.
- **camelCase IPC:** All JSON-RPC fields MUST be `camelCase` per architecture mandate.

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

- `e2f9903 Add story for creating .jellyfinsync.json` — This is the story we're implementing
- `3677f2d Done` — Story 5.4 (Visual Manifest Repair Utility) completed
- `067ec1e Review 5.4` / `434197a Code 5.4` — RepairModal.ts was completed in these commits; use it as the UI modal template

The RepairModal.ts pattern (Shoelace `sl-dialog`, class-based, `open()` method, `onComplete` callback) is confirmed working and is the correct template for `InitDeviceModal.ts`.

### File Structure

- `jellyfinsync-daemon/src/device/mod.rs` — Add `DeviceEvent::Unrecognized`, `DeviceManager.unrecognized_device_path`, `handle_device_unrecognized`, `get_unrecognized_device_path`, `initialize_device`, update `run_observer`, update `list_root_folders`, update `handle_device_removed`
- `jellyfinsync-daemon/src/main.rs` — Add `DeviceEvent::Unrecognized` handler arm
- `jellyfinsync-daemon/src/rpc.rs` — Add `device_initialize` dispatch, implement `handle_device_initialize`, update `handle_get_daemon_state`
- `jellyfinsync-ui/src/components/InitDeviceModal.ts` — New component (follow RepairModal.ts pattern)
- `jellyfinsync-ui/src/components/BasketSidebar.ts` — Add `has_manifest: false` banner rendering and InitDeviceModal integration

### MTP Task: Exact Changes Required (post-Story 4.0)

Once Story 4.0 defines `DeviceIO` trait and `DeviceManager` stores `Arc<dyn DeviceIO>` per device:

**1. `DeviceManager` — store IO backend for pending unrecognized device:**
`DeviceManager` will need `unrecognized_device_io: Arc<RwLock<Option<Arc<dyn DeviceIO>>>>` alongside `unrecognized_device_path`. `handle_device_unrecognized` must receive and store the backend created by the detection layer (MSC or MTP).

**2. `initialize_device()` signature change:**
```rust
// Before (current):
pub async fn initialize_device(&self, folder_path: &str, transcoding_profile_id: Option<String>, name: String, icon: Option<String>) -> Result<DeviceManifest>

// After (post-4.0):
pub async fn initialize_device(&self, folder_path: &str, transcoding_profile_id: Option<String>, name: String, icon: Option<String>, device_io: Arc<dyn DeviceIO>) -> Result<DeviceManifest>
```

**3. Replace `write_manifest` with `device_io.write_with_verify()` at `device/mod.rs:420`:**
```rust
// Before:
write_manifest(&device_root, &manifest).await?;

// After:
let manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
device_io.write_with_verify(".jellyfinsync.json", &manifest_bytes)?;
```

**4. `handle_device_initialize` in `rpc.rs:1387`:**
Retrieve `device_io` from `DeviceManager` for the unrecognized device path and pass it to `initialize_device()`.

**Note:** MSC behavior is preserved — `MscBackend::write_with_verify()` calls the same Write-Temp-Rename + `sync_all` that `write_manifest()` does today.

### References

- Architecture — Device IO Abstraction: `_bmad-output/planning-artifacts/architecture.md` (Device IO Abstraction section)
- Architecture — Safety & Atomicity: `_bmad-output/planning-artifacts/architecture.md` (Safety & Atomicity Patterns section)
- `write_manifest()`: `device/mod.rs:87–102`
- `DeviceEvent` enum: `device/mod.rs:154–163`
- `DeviceManager` struct: `device/mod.rs:181–188`
- `initialize_device()`: `device/mod.rs:361–440` — line 420 is the `write_manifest` call to replace
- `run_observer`: `device/mod.rs:884–930`
- `is_removable_drive`: `device/mod.rs:867–882`
- `handle_device_initialize` RPC: `rpc.rs:1387–1468`
- `handle_get_daemon_state`: `rpc.rs:374–428`
- `BasketSidebar` banner render: `BasketSidebar.ts:480–495`
- `InitDeviceModal`: `jellyfinsync-ui/src/components/InitDeviceModal.ts`
- Story 4.0 (DeviceIO definition): `_bmad-output/implementation-artifacts/4-0-device-io-abstraction-layer.md`

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

### Completion Notes List

- Implemented `DeviceEvent::Unrecognized { path }` variant in `device/mod.rs`. The `run_observer` now uses a `match` on `DeviceProber::probe` result: `Ok(Some(manifest))` → Detected, `Ok(None)` → Unrecognized (if removable), `Err(_)` → ignored.
- Added `is_removable_drive()` function with `#[cfg(target_os = "windows")]` guard using `GetDriveTypeW`. Non-Windows returns `true` since mount detection already filters appropriately.
- `DeviceManager` extended with `unrecognized_device_path: Arc<RwLock<Option<PathBuf>>>`. The field is mutually exclusive with `current_device_path`: `handle_device_detected` clears it, `handle_device_removed` clears both.
- `initialize_device()` generates UUID v4, constructs `DeviceManifest` with `version: "1.0"`, writes atomically via `write_manifest`, creates subfolder if specified, and transitions to recognized state.
- `list_root_folders` falls through to `unrecognized_device_path` when `current_device_path` is `None` — enabling the init dialog to show folders on the device.
- RPC `device_initialize` handler extracts `folderPath` + `profileId`, calls `initialize_device`, stores DB mapping, sends `DeviceRecognized` state update.
- `get_daemon_state` now includes `pendingDevicePath: Option<String>` field.
- `BasketSidebar.renderDeviceFolders()` shows "New Device Detected" banner (styled like dirty-manifest-banner) when `!hasManifest`, with "Initialize" button opening `InitDeviceModal`.
- `InitDeviceModal.ts` follows `RepairModal.ts` pattern: `sl-dialog`, `open()` method, loads credentials via `get_credentials` RPC, shows `sl-input` for folder name and read-only userId display, sends `device_initialize` RPC on Confirm, calls `onComplete` on success, shows error with Retry/Dismiss on failure.
- Daemon state polling in `BasketSidebar.startDaemonStatePolling()` updated to also detect `pendingDevicePath` changes and trigger `refreshAndRender()`.
- All 114 tests pass (107 pre-existing + 7 new: 3 device/mod.rs + 4 rpc.rs). TypeScript compiles cleanly.

### File List

- `jellyfinsync-daemon/src/device/mod.rs`
- `jellyfinsync-daemon/src/device/tests.rs`
- `jellyfinsync-daemon/src/main.rs`
- `jellyfinsync-daemon/src/rpc.rs`
- `jellyfinsync-ui/src/components/InitDeviceModal.ts` (new)
- `jellyfinsync-ui/src/components/BasketSidebar.ts`
- `_bmad-output/implementation-artifacts/2-6-initialize-new-device-manifest.md`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`

### Change Log

- 2026-04-30: Reopened — MTP support (Sprint Change 2026-04-30). AC #4 and MTP task added. Requires Story 4.0 (DeviceIO abstraction) to be completed first.
- 2026-03-01: Implemented Story 2.6 — Initialize New Device Manifest. Added unrecognized device detection pipeline (DeviceEvent::Unrecognized, is_removable_drive guard, DeviceManager.handle_device_unrecognized/initialize_device), device_initialize RPC endpoint, pendingDevicePath in get_daemon_state, InitDeviceModal UI component, and Initialize Device banner in BasketSidebar.
- 2026-03-01: Code Review (AI) — Fixed 6 issues: (H1) Added path traversal and single-level folder validation to initialize_device, (M1) handle_device_unrecognized now clears current_device fields enforcing mutual exclusivity, (M2) DeviceRecognized state uses human-readable device path name instead of UUID, (M3) Replaced create_dir_all with create_dir to prevent nested directory creation, (M4) Removed unused _profile_id parameter from DeviceManager::initialize_device, (M5) Added pendingDevicePath test for get_daemon_state, path traversal test, and mutual exclusivity test. 117 tests pass.
