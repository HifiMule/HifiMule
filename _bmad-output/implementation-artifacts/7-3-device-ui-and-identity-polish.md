# Story 7.3: Device UI & Identity Polish

Status: review

## Story

As a **Convenience Seeker (Sarah)** and **System Admin (Alexis)**,
I want the device initialization flow and device hub to reflect real device identity and surface MTP constraints clearly,
so that the UI never shows stale defaults, silent gaps, or confusing state for MTP devices.

## Acceptance Criteria

1. **Given** an MTP device is detected and its `friendly_name` is stored in `unrecognized_device_friendly_name` **When** the "Initialize Device" dialog opens **Then** the device name input is pre-filled with the MTP `friendly_name` (e.g., "Garmin Forerunner 945") instead of "My Device".

2. **Given** a device manifest where `name` is an empty string (`""`) **When** the device hub resolves the display name **Then** the empty string is filtered out (`.filter(|n| !n.is_empty())`) and the `device_id` fallback is applied correctly.

3. **Given** an MTP device is connected **When** the Device State panel renders `unmanaged_count` **Then** the panel displays "MTP — folder enumeration not available" instead of a silent `0 protected`, so the user understands the limitation.

4. **Given** `broadcast_device_state` runs for a device already present in `connected_devices` **When** the device was previously detected **Then** `handle_device_detected` is NOT re-triggered (duplicate insertion and spurious dirty-state transitions are prevented).

5. **Given** an MTP device is connected **When** the Storage Projection bar calculates available capacity **Then** `free_space()` returns a real value from the MTP device's storage object (not `None`), so capacity is visible in the UI.

6. **Given** `initialize_device` is called **When** the physical device was disconnected and reconnected between the `Unrecognized` event and the user completing initialization **Then** the RPC handler detects the stale `Arc<dyn DeviceIO>` (e.g., via a liveness check) and returns a clear error instead of writing to a dead handle.

7. **Given** `cleanup_tmp_files` runs **When** a `.tmp` file exists at the device root (outside `managed_paths`) **Then** it is included in the cleanup sweep.

8. **Given** `initialize_device` creates a managed path **When** the path has multiple levels **Then** `create_dir_all` is used instead of `create_dir`, avoiding a latent failure for nested paths.

9. **Given** the MTP scrobbler detection path in `scrobbler.rs` **When** `read_file` returns a plain `anyhow` error because no `.scrobbler.log` exists **Then** the "not found" case is detected correctly (not treated as a read error), matching the MSC path behavior.

## Tasks / Subtasks

- [x] **T1: Surface `pendingDeviceFriendlyName` from daemon RPC** (AC: #1)
  - [x] In `jellyfinsync-daemon/src/rpc.rs` `handle_get_daemon_state` (line 386-436): add `pending_device_friendly_name` variable via `state.device_manager.get_unrecognized_device_snapshot().await.and_then(|s| s.friendly_name)`.
  - [x] Add `"pendingDeviceFriendlyName": pending_device_friendly_name` to the final `serde_json::json!({...})` response alongside the existing `"pendingDevicePath"` field.

- [x] **T2: Pre-fill InitDeviceModal name from daemon state** (AC: #1)
  - [x] In `jellyfinsync-ui/src/components/BasketSidebar.ts`: add `pendingDeviceFriendlyName?: string` to the `RootFoldersResponse` interface (it comes from `daemonStateResult`, not `foldersResult`; capture it from `daemonStateResult?.pendingDeviceFriendlyName ?? undefined`).
  - [x] Store as a class field `private pendingDeviceFriendlyName: string | undefined = undefined` alongside `pendingDevicePath`.
  - [x] Update `openInitDeviceModal()` to pass `this.pendingDeviceFriendlyName` to `InitDeviceModal`.
  - [x] In `jellyfinsync-ui/src/components/InitDeviceModal.ts`: change `open()` signature to `async open(defaultName?: string)`. Thread it through to `renderContent()` so the `sl-input#init-device-name-input` uses `value="${this.escapeHtml(defaultName ?? 'My Device')}"` instead of the hardcoded `"My Device"`.
  - [x] Confirm the `confirmBtn.disabled` logic still uses the actual input value (not the default), so an empty override keeps the button disabled.

- [x] **T3: Filter empty-string device names in connected_devices_json** (AC: #2)
  - [x] In `jellyfinsync-daemon/src/rpc.rs` `handle_get_daemon_state` (line ~415): change `m.name.clone().unwrap_or_else(|| m.device_id.clone())` to `m.name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| m.device_id.clone())`.
  - [x] Add a unit test verifying that a manifest with `name: Some("")` falls back to `device_id`.

- [x] **T4: Show MTP constraint label instead of unmanaged_count** (AC: #3)
  - [x] In `jellyfinsync-ui/src/components/BasketSidebar.ts` `renderDeviceFolders()` (line ~503): check `this.folderInfo.devicePath.toLowerCase().startsWith('mtp://')` and, if so, replace `${unmanagedCount} protected` with `MTP — folder enumeration not available` in the summary `<span>`.
  - [x] Ensure the managed folder list still renders (managed paths are returned from daemon for MTP devices).

- [x] **T5: Fix `broadcast_device_state` to not re-trigger device detection** (AC: #4)
  - [x] In `jellyfinsync-daemon/src/rpc.rs`, replace the implementation of `broadcast_device_state` (lines 1307-1319). The current body calls `get_manifest_and_io` + `get_current_device_path` + `handle_device_detected` — this re-runs the detection flow (dirty-marker scan, state writes) even for already-connected devices.
  - [x] New body: call `get_current_device()` (read-only) and build the appropriate `DaemonState` without re-triggering detection.
  - [x] Preserve the existing call sites at lines 1360, 1399, 1413.
  - [x] Add a note (or test) verifying that calling `broadcast_device_state` while a device is already connected does not insert it a second time in `connected_devices`.

- [x] **T6: Wire `storage_id` into MTP backend for `free_space`** (AC: #5)
  - [x] In `jellyfinsync-daemon/src/device/mod.rs` `emit_mtp_probe_event`: accept `dev_info: mtp::MtpDeviceInfo`. After parsing manifest, if `manifest.storage_id.is_some()`, create a second backend via `spawn_blocking` with the storage ID and use it for `DeviceEvent::Detected`. Fall back to the original backend on failure.
  - [x] Caller in `run_mtp_observer` passes `dev.clone()` as `dev_for_probe`.
  - [x] Existing `emit_mtp_probe_event` tests updated to pass the new `dev_info` parameter.

- [x] **T7: Liveness check in `initialize_device`** (AC: #6)
  - [x] In `jellyfinsync-daemon/src/device/mod.rs` `initialize_device`, after obtaining `pending = get_unrecognized_device_snapshot()`: call `device_io.list_files("").await` as a lightweight connectivity probe. Return clear error if it fails.

- [x] **T8: Include device root in `cleanup_tmp_files` sweep** (AC: #7)
  - [x] In `jellyfinsync-daemon/src/device/mod.rs` `cleanup_tmp_files`: prepend `""` (device root) using `std::iter::once("").chain(managed_paths.iter().map(|s| s.as_str()))`.
  - [x] Added two unit tests: root-level .tmp deletion with empty managed_paths, and combined root + managed sweep.

- [x] **T9: Allow multi-level folder paths in `initialize_device`** (AC: #8)
  - [x] In `jellyfinsync-daemon/src/device/mod.rs` `initialize_device`: relaxed path validation to allow `/` and `\` as internal separators, keeping blocks for `..` traversal and absolute paths.
  - [x] Updated existing `test_initialize_device_rejects_path_traversal` test to reflect that multi-level paths now succeed.

- [x] **T10: Verify MTP scrobbler not-found detection** (AC: #9)
  - [x] `is_missing_scrobbler_log_error` in `scrobbler.rs` already handles both MSC (`NotFound`) and MTP WPD (`".scrobbler.log" + "not found"`) cases.
  - [x] `test_process_device_mtp_style_missing_log_is_empty_success` passes — verified with `cargo test`.
  - [x] No code change needed.

- [x] **T11: Run full test suite and validate** (AC: all)
  - [x] Run `rtk cargo test -p jellyfinsync-daemon` — 198 tests pass.
  - [x] Run `rtk cargo clippy -p jellyfinsync-daemon -- -D warnings` — no new warnings (32 pre-existing).
  - [x] Run `rtk tsc` in `jellyfinsync-ui/` — no TypeScript errors.
  - [x] Update story File List.

## Dev Notes

### AC1 — Daemon: `handle_get_daemon_state` in `rpc.rs`

Current state (line 386-436): already returns `pendingDevicePath` by calling `get_unrecognized_device_path()`. Extend it:

```rust
// After the existing pending_device_path block:
let pending_device_friendly_name = state
    .device_manager
    .get_unrecognized_device_snapshot()
    .await
    .and_then(|s| s.friendly_name);
```

Then in the `serde_json::json!` block, add:
```rust
"pendingDeviceFriendlyName": pending_device_friendly_name,
```

`get_unrecognized_device_snapshot()` already exists on `DeviceManager` (line 340, `device/mod.rs`). It returns `Option<UnrecognizedDeviceState>` where `UnrecognizedDeviceState.friendly_name: Option<String>` was added in Story 7.2.

### AC1 — UI: `BasketSidebar.ts` + `InitDeviceModal.ts`

`BasketSidebar.ts` tracks daemon state polling at lines 175-184. `daemonStateResult` already carries `pendingDevicePath`. Add `pendingDeviceFriendlyName` to the same capture:

```typescript
// Around line 410:
const newPendingFriendlyName = daemonStateResult?.pendingDeviceFriendlyName ?? undefined;
this.pendingDeviceFriendlyName = newPendingFriendlyName;
```

`openInitDeviceModal()` at line 770 currently passes no args. Update it to:

```typescript
private openInitDeviceModal() {
    const modal = new InitDeviceModal(this.container, () => {
        this.refreshAndRender();
    });
    modal.open(this.pendingDeviceFriendlyName);
}
```

In `InitDeviceModal.ts`, `open()` signature at line 15:
```typescript
async open(defaultName?: string) {
    this._defaultName = defaultName;
    this.renderDialog();
    await this.showDialog();
    await this.loadCredentials();
}
```

Add `private _defaultName: string | undefined = undefined;` as class field.

In `renderContent()` at line 118, change:
```typescript
// Before:
value="My Device"
// After:
value="${this.escapeHtml(this._defaultName ?? 'My Device')}"
```

The `confirmBtn.disabled` check (line 213) reads from `nameInput?.value` — no change needed; it correctly reads the actual current input value.

### AC2 — Empty-string filter in `rpc.rs`

Current code (line 415):
```rust
"name": m.name.clone().unwrap_or_else(|| m.device_id.clone()),
```

Change to:
```rust
"name": m.name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| m.device_id.clone()),
```

This is a one-line change. A manifest where `name` was stored as `""` (which `initialize_device` now prevents via the `filter(|s| !s.is_empty())` at line 555, but pre-existing manifests may have it) will now fall back to `device_id`.

### AC3 — MTP label in UI

`RootFoldersResponse` already includes `devicePath: string` (line 24, `BasketSidebar.ts`). For MTP devices, `devicePath` is `"mtp://<device_id>"` (set by `list_root_folders` in `device/mod.rs` line 659). In `renderDeviceFolders()`:

```typescript
// Line ~503:
const isMtp = this.folderInfo.devicePath.toLowerCase().startsWith('mtp://');
const unmanagedSummary = isMtp
    ? 'MTP — folder enumeration not available'
    : `${unmanagedCount} protected`;
// Replace ${unmanagedCount} protected with ${unmanagedSummary} in the span
```

### AC4 — Fix `broadcast_device_state`

Current (line 1307-1319):
```rust
async fn broadcast_device_state(state: &AppState) {
    if let Some((device, device_io)) = state.device_manager.get_manifest_and_io().await {
        if let Some(path) = state.device_manager.get_current_device_path().await {
            if let Ok(daemon_state) = state.device_manager
                .handle_device_detected(path, device, device_io).await
            {
                let _ = state.state_tx.send(daemon_state);
            }
        }
    }
}
```

The problem: `handle_device_detected` runs the full detection flow (dirty-marker scan, state writes, unrecognized-slot clear). For an already-connected device, it returns `DaemonState::Idle`, which causes the UI to transition to idle state spuriously.

New body:
```rust
async fn broadcast_device_state(state: &AppState) {
    if let Ok(daemon_state_json) = handle_get_daemon_state(state).await {
        // Derive the legacy DaemonState from the rich JSON response.
        // The simplest broadcast: send the device name if recognized.
        let name = daemon_state_json["currentDevice"]["name"]
            .as_str()
            .map(|s| s.to_string());
        if let Some(name) = name {
            let _ = state.state_tx.send(crate::DaemonState::DeviceFound(name));
        }
    }
}
```

Wait — `handle_get_daemon_state` returns `Result<Value, JsonRpcError>`, not `Result<crate::DaemonState, _>`. The `state_tx` sends `crate::DaemonState`. We need a mapping.

Better approach: extract the state-computation logic into a shared helper, or just use the device name from `get_current_device()`:

```rust
async fn broadcast_device_state(state: &AppState) {
    if let Some(manifest) = state.device_manager.get_current_device().await {
        let name = manifest.name.clone().filter(|n| !n.is_empty())
            .unwrap_or_else(|| manifest.device_id.clone());
        let mapping = state.db.get_device_mapping(&manifest.device_id).unwrap_or(None);
        let daemon_state = if let Some(m) = mapping {
            if let Some(profile_id) = m.jellyfin_user_id {
                crate::DaemonState::DeviceRecognized { name, profile_id }
            } else {
                crate::DaemonState::DeviceFound(name)
            }
        } else {
            crate::DaemonState::DeviceFound(name)
        };
        let _ = state.state_tx.send(daemon_state);
    }
}
```

This reads current state without re-triggering detection. No `write` locks, no dirty-marker scan, no `handle_device_detected` side effects.

### AC5 — Wire `storage_id` into MTP backend for `free_space`

The issue: `run_mtp_observer` always calls `mtp::create_mtp_backend(&dev_clone, None)` (line 1346). The backend's `WpdHandle` has `storage_id: None`, so `free_space()` must enumerate storage objects on every call instead of using the cached ID.

The fix: after reading `.jellyfinsync.json` from the initial backend (in `emit_mtp_probe_event`), if `manifest.storage_id.is_some()`, create a second backend with the `storage_id`:

Restructure `run_mtp_observer` loop body:
1. Create initial backend with `storage_id: None` (probe backend)
2. Read `.jellyfinsync.json` from it
3. Parse the manifest
4. If `manifest.storage_id.is_some()`, create a second backend with `mtp::create_mtp_backend(&dev, manifest.storage_id.clone())` via `spawn_blocking`
5. Use the second backend (or fall back to the first if second creation fails) for `DeviceEvent::Detected`
6. For `DeviceEvent::Unrecognized`, use the original backend

This means inlining what `emit_mtp_probe_event` currently does. `emit_mtp_probe_event` can be kept for the unrecognized case or refactored to return the manifest.

The `MtpDeviceInfo` (`dev`) is still available in the loop scope after the `spawn_blocking` because only `dev_clone` (a clone) was moved into it.

### AC6 — Liveness check in `initialize_device`

In `device/mod.rs` `initialize_device` (~line 540), after getting the `pending` snapshot:
```rust
// Liveness probe: detect stale IO from a device that disconnected and reconnected
// between the Unrecognized event and the user completing initialization.
if let Err(_) = device_io.list_files("").await {
    return Err(anyhow::anyhow!(
        "Device no longer accessible — reconnect the device and try again"
    ));
}
```

Note: `device_io` is `pending.io` from the snapshot. For MSC devices, if the path is gone, `list_files("")` calls `tokio::fs::read_dir` which fails. For MTP, the WPD call fails. This is a best-effort check — a TOCTOU gap remains between the check and the write, but this is acceptable (subsequent writes will also fail with errors).

### AC7 — Device root in `cleanup_tmp_files`

Current (`device/mod.rs` line 137-156): iterates only `managed_paths`.

`managed_paths` never contains `""` — `initialize_device` only adds `folder_path` which is validated non-empty, and the empty-path case results in `vec![]`.

Fix: prepend `""` unconditionally:
```rust
pub async fn cleanup_tmp_files(
    device_io: std::sync::Arc<dyn crate::device_io::DeviceIO>,
    managed_paths: &[String],
) -> Result<usize> {
    let mut count = 0;
    // Sweep device root ("") plus all managed paths
    let paths: Vec<&str> = std::iter::once("")
        .chain(managed_paths.iter().map(|s| s.as_str()))
        .collect();
    for path_str in paths {
        let entries = match device_io.list_files(path_str).await {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries {
            if entry.name.ends_with(".tmp") {
                if device_io.delete_file(&entry.path).await.is_ok() {
                    count += 1;
                }
            }
        }
    }
    Ok(count)
}
```

### AC8 — Multi-level paths in `initialize_device`

Current validation (line 524-535):
```rust
if !folder_path.is_empty() {
    if folder_path.contains("..")
        || folder_path.starts_with('/')
        || folder_path.starts_with('\\')
        || folder_path.contains('/')    // ← too restrictive
        || folder_path.contains('\\')  // ← too restrictive
    { return Err(...); }
}
```

Relaxed validation (keep traversal and absolute-path blocks, remove internal separator blocks):
```rust
if !folder_path.is_empty() {
    let components: Vec<&str> = folder_path.split(&['/', '\\']).collect();
    if components.iter().any(|c| *c == "..") {
        return Err(anyhow::anyhow!("Invalid folder path: path traversal ('..') not allowed"));
    }
    if folder_path.starts_with('/') || folder_path.starts_with('\\') {
        return Err(anyhow::anyhow!("Invalid folder path: absolute paths not allowed"));
    }
}
```

`MscBackend::ensure_dir` uses `tokio::fs::create_dir_all` (already confirmed at `device_io.rs` line 212). `MtpBackend::ensure_dir` is a no-op (MTP auto-creates parent objects). Both handle nested paths correctly — no `device_io.rs` changes needed.

### AC9 — MTP scrobbler not-found detection

`is_missing_scrobbler_log_error` in `scrobbler.rs` (line 80-93) already has two branches:
1. `std::io::Error::NotFound` — covers MSC path traversal
2. Error chain message contains `.scrobbler.log` AND `not found` — covers WPD errors like `"WPD: path component '.scrobbler.log' not found"`

Test `test_process_device_mtp_style_missing_log_is_empty_success` (line ~413) exercises branch 2 with a mock returning `anyhow::anyhow!("WPD: path component '{}' not found", path)`.

Dev task: run `cargo test -p jellyfinsync-daemon test_process_device_mtp_style_missing_log_is_empty_success` and confirm it passes. No code change expected. If the real WPD `path_to_object_id` (in `device/mtp.rs`) generates a different message format for missing files, update the message pattern in `is_missing_scrobbler_log_error`.

### Project Structure Notes

All daemon changes are in:
- `jellyfinsync-daemon/src/rpc.rs` — `handle_get_daemon_state`, `broadcast_device_state`
- `jellyfinsync-daemon/src/device/mod.rs` — `cleanup_tmp_files`, `initialize_device`, `run_mtp_observer`
- `jellyfinsync-daemon/src/scrobbler.rs` — verify only, no expected changes

All UI changes are in:
- `jellyfinsync-ui/src/components/BasketSidebar.ts` — `renderDeviceFolders`, `openInitDeviceModal`, field capture
- `jellyfinsync-ui/src/components/InitDeviceModal.ts` — `open()` signature, `renderContent()`

Existing test files that may need new test cases:
- `jellyfinsync-daemon/src/device/tests.rs`
- `jellyfinsync-daemon/src/device_io.rs` (has existing `msc_ensure_dir_creates_path` test)
- `jellyfinsync-daemon/src/scrobbler.rs` (has existing MTP missing-log test)

### References

- `handle_get_daemon_state`: `rpc.rs` lines 371-437
- `broadcast_device_state`: `rpc.rs` lines 1307-1319 (callers at 1360, 1399, 1413)
- `connected_devices_json` name resolution: `rpc.rs` line 415
- `get_unrecognized_device_snapshot`: `device/mod.rs` lines 340-350
- `UnrecognizedDeviceState.friendly_name`: `device/mod.rs` (added Story 7.2)
- `cleanup_tmp_files`: `device/mod.rs` lines 137-156
- `initialize_device`: `device/mod.rs` lines 515-602
- `run_mtp_observer` / `emit_mtp_probe_event`: `device/mod.rs` lines 1333-1414
- `create_mtp_backend`: `device/mtp.rs` lines 1691-1709
- `WpdHandle.storage_id` and `free_space()`: `device/mtp.rs` lines 252, 1211-1260
- `MscBackend::ensure_dir` uses `create_dir_all`: `device_io.rs` line 212
- `MtpBackend::ensure_dir` is no-op: `device_io.rs` lines 339-341
- `is_missing_scrobbler_log_error`: `scrobbler.rs` lines 80-93
- `test_process_device_mtp_style_missing_log_is_empty_success`: `scrobbler.rs` line ~413
- `openInitDeviceModal`: `BasketSidebar.ts` line 770
- `renderDeviceFolders` summary span: `BasketSidebar.ts` line 503
- `InitDeviceModal.open()`: `InitDeviceModal.ts` line 15
- `renderContent` name input: `InitDeviceModal.ts` line 118

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

### Completion Notes List

- T1+T2 (AC#1): `get_daemon_state` now surfaces `pendingDeviceFriendlyName` from `get_unrecognized_device_snapshot()`. `BasketSidebar` captures this field and passes it to `InitDeviceModal.open()`. The modal pre-fills the device name input with the MTP friendly name (e.g. "Garmin Forerunner 945") instead of the hardcoded "My Device". The `confirmBtn.disabled` check uses `nameInput?.value` (actual input value) — unchanged, correctly reads the live value.
- T3 (AC#2): Empty-string device names in `connected_devices_json` now fall back to `device_id` via `.filter(|n| !n.is_empty())`. Unit test `test_empty_device_name_falls_back_to_device_id` added to `device/tests.rs` covering `Some("")`, `None`, and real-name cases.
- T4 (AC#3): `renderDeviceFolders()` detects MTP devices via `devicePath.startsWith('mtp://')` and shows "MTP — folder enumeration not available" instead of `${unmanagedCount} protected`. Managed folder list still renders.
- T5 (AC#4): `broadcast_device_state` replaced — no longer calls `handle_device_detected` (which re-ran dirty-marker scan and state writes). New body reads current device state via `get_current_device()` (read-only) and sends appropriate `DaemonState`. Call sites at lines 1360, 1399, 1413 preserved unchanged.
- T6 (AC#5): `emit_mtp_probe_event` now accepts `dev_info: mtp::MtpDeviceInfo`. When the parsed manifest has a `storage_id`, a second backend is created via `spawn_blocking(create_mtp_backend(..., Some(storage_id)))`. This storage-aware backend is used for `DeviceEvent::Detected`, enabling `free_space()` and path lookups to skip the DEVICE first-child enumeration. Falls back to original backend if second creation fails.
- T7 (AC#6): Liveness probe added in `initialize_device` after obtaining the unrecognized device snapshot: `device_io.list_files("").await` — fails early with a clear "Device no longer accessible" error if the IO handle is stale.
- T8 (AC#7): `cleanup_tmp_files` now sweeps device root `""` before all managed paths via `std::iter::once("").chain(...)`. Tests added: `test_cleanup_tmp_files_at_device_root` and `test_cleanup_tmp_files_root_and_managed`.
- T9 (AC#8): Path validation in `initialize_device` relaxed to allow `/` and `\` as internal separators; traversal (`..`) and absolute paths still blocked. Existing test updated: multi-level path "Music/SubFolder" now asserts `is_ok()`.
- T10 (AC#9): `is_missing_scrobbler_log_error` in `scrobbler.rs` already covers both MSC (`NotFound`) and MTP WPD (`".scrobbler.log"` + `"not found"` in error chain). `test_process_device_mtp_style_missing_log_is_empty_success` passes. No code change needed.
- T11: All 198 daemon tests pass. 32 pre-existing clippy warnings unchanged (none introduced). TypeScript: no errors.

### File List

- `jellyfinsync-daemon/src/rpc.rs`
- `jellyfinsync-daemon/src/device/mod.rs`
- `jellyfinsync-daemon/src/device/tests.rs`
- `jellyfinsync-ui/src/components/BasketSidebar.ts`
- `jellyfinsync-ui/src/components/InitDeviceModal.ts`
- `_bmad-output/implementation-artifacts/7-3-device-ui-and-identity-polish.md`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`

## Change Log

- 2026-05-08 (claude-sonnet-4-6): Implemented all ACs — surfaced MTP friendly name in daemon RPC and pre-filled InitDeviceModal, filtered empty device names, added MTP constraint label, fixed broadcast_device_state side-effects, wired storage_id into MTP backend, added liveness check and cleanup_tmp root sweep, relaxed multi-level path validation, verified scrobbler detection. 198 daemon tests pass.
