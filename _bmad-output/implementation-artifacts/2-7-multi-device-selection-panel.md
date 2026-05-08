# Story 2.7: Multi-Device Selection Panel

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a System Admin (Alexis) and Ritualist (Arthur),
I want to see all currently connected managed devices and select which one I am working with,
So that I can operate on one specific device without the daemon silently overwriting my context when a second device is plugged in.

## Acceptance Criteria

1. **Multi-Device Picker Display:**
   - **Given** two or more managed devices are connected simultaneously
   - **When** I open the main UI (or when a second device is detected while the UI is open)
   - **Then** the UI displays a device picker listing all connected managed devices (device name from manifest, device_id, path)
   - **And** the currently selected device is highlighted

2. **Device Context Switch:**
   - **Given** the device picker is visible
   - **When** I click a different device
   - **Then** the UI switches context to that device (reloads basket from its manifest, updates storage projection)
   - **And** the daemon's active device updates via the `device.select` RPC

3. **Single-Device Auto-Select (No Picker):**
   - **Given** only one managed device is connected
   - **Then** no picker is shown and behaviour is identical to the current single-device experience (device is auto-selected)

4. **Device Removal — Context Clear:**
   - **Given** the currently selected device is disconnected
   - **When** the daemon fires a device-removed event
   - **Then** the UI clears device context with no crash or stale state
   - **And** if other devices remain connected, the picker is shown for the remaining devices

5. **All Operations Target Selected Device:**
   - **Given** a device is selected via the picker
   - **When** any operation is performed (basket view, storage projection, sync, manifest repair)
   - **Then** all operations target the selected device — the daemon's `get_current_device()` always returns the selected device's manifest

## Tasks / Subtasks

- [x] **Backend: Refactor `DeviceManager` struct for multi-device tracking** (AC: #1, #3, #4, #5)
  - [x] Replace `current_device: Arc<RwLock<Option<DeviceManifest>>>` and `current_device_path: Arc<RwLock<Option<PathBuf>>>` with:
    - `connected_devices: Arc<RwLock<HashMap<PathBuf, DeviceManifest>>>` — all currently connected managed devices
    - `selected_device_path: Arc<RwLock<Option<PathBuf>>>` — the device targeted by all UI operations
  - [x] Update `DeviceManager::new()` to initialize both new fields
  - [x] Update `get_current_device()`: acquire read lock on `connected_devices`, read `selected_device_path`, return clone of the matching entry (or `None`)
  - [x] Update `get_current_device_path()`: acquire read lock on `selected_device_path`, return clone
  - [x] Update `update_manifest()`: acquire write lock on `connected_devices`, mutate the entry at `selected_device_path`, write to disk via `write_manifest`
  - [x] Update `get_device_storage()`: call `get_current_device_path()` as before — no change needed at call site

- [x] **Backend: Update `handle_device_detected`** (AC: #1, #3)
  - [x] Acquire write lock on `connected_devices`, insert `(path, manifest.clone())`
  - [x] Read `selected_device_path`; if it is `None`, set `selected_device_path = Some(path)` (auto-select first/only device)
  - [x] If `selected_device_path` is already `Some(_)` (another device selected), do NOT change selection — just add to map
  - [x] Clear `unrecognized_device_path` (preserve mutual exclusivity with unrecognized path)
  - [x] Return `DaemonState::DeviceRecognized` / `DeviceFound` based on DB mapping as before

- [x] **Backend: Update `handle_device_removed`** (AC: #4)
  - [x] Acquire write lock on `connected_devices`, remove the entry for the removed `path`
  - [x] Read `selected_device_path`; if it matches removed path, clear it: `selected_device_path = None`
  - [x] If after removal exactly one device remains, auto-select it (`selected_device_path = Some(remaining_path)`)
  - [x] Clear `unrecognized_device_path` as before
  - [x] Callers in `main.rs` send `DaemonState::Idle` when no devices remain — no change needed there

- [x] **Backend: Update `handle_device_unrecognized`** (AC: no direct change, but mutual exclusivity)
  - [x] This method currently clears `current_device` and `current_device_path` — update to clear `connected_devices` entry for the path and `selected_device_path` if it matches
  - [x] Actually: `Unrecognized` means device has no manifest yet — it should NOT be in `connected_devices`. Just ensure `unrecognized_device_path` is set and no entry exists in `connected_devices` for that path

- [x] **Backend: Add helper `get_connected_devices()`** (AC: #1)
  - [x] `pub async fn get_connected_devices(&self) -> Vec<(PathBuf, DeviceManifest)>`
  - [x] Returns snapshot of all entries in `connected_devices` HashMap
  - [x] Used by `device.list` RPC and `get_daemon_state`

- [x] **Backend: New RPC `device.list`** (AC: #1)
  - [x] Add `"device.list"` arm to `handler` match in `rpc.rs`
  - [x] Implement `handle_device_list(state: &AppState) -> Result<Value, JsonRpcError>`
  - [x] Call `state.device_manager.get_connected_devices().await`
  - [x] Return `Ok(json!({ "status": "success", "data": [ { "path": ..., "deviceId": ..., "name": ... }, ... ] }))`
  - [x] `name` = `manifest.name.clone().unwrap_or_else(|| manifest.device_id.clone())`

- [x] **Backend: New RPC `device.select`** (AC: #2)
  - [x] Add `"device.select"` arm to `handler` match in `rpc.rs`
  - [x] Implement `handle_device_select(state: &AppState, params: Option<Value>) -> Result<Value, JsonRpcError>`
  - [x] Extract `path: String` from params; return `ERR_INVALID_PARAMS` if missing
  - [x] Convert to `PathBuf`; verify it exists in `connected_devices` — if not, return error `{ "code": 404, "message": "Device not connected" }`
  - [x] Acquire write lock on `selected_device_path`, set to `Some(PathBuf::from(path))`
  - [x] Return `Ok(json!({ "status": "success", "data": { "ok": true } }))`
  - [x] Add unit test: select valid path → ok; select unknown path → error

- [x] **Backend: Extend `get_daemon_state` response** (AC: #1, #4)
  - [x] In `handle_get_daemon_state`, call `state.device_manager.get_connected_devices().await`
  - [x] Read `selected_device_path` from `state.device_manager.get_current_device_path().await`
  - [x] Add to response JSON:
    ```json
    "connectedDevices": [{ "path": "...", "deviceId": "...", "name": "..." }],
    "selectedDevicePath": "E:\\" | null
    ```
  - [x] All existing fields (`currentDevice`, `pendingDevicePath`, etc.) remain unchanged

- [x] **Frontend: `startDaemonStatePolling` — track multi-device state** (AC: #1, #4)
  - [x] Add instance variables `connectedDevices: Array<{path, deviceId, name}>` and `selectedDevicePath: string | null` to `BasketSidebar`
  - [x] In polling callback, read `daemonStateResult.connectedDevices` and `daemonStateResult.selectedDevicePath`
  - [x] Detect changes: if device count changes OR `selectedDevicePath` changes → call `refreshAndRender()`
  - [x] Update stored values after comparison

- [x] **Frontend: Device picker in `renderDeviceFolders` / separate render method** (AC: #1, #2, #3)
  - [x] Create `private renderDevicePicker(): string` method
  - [x] If `connectedDevices.length <= 1` → return empty string (no picker)
  - [x] If `connectedDevices.length > 1` → render a `<sl-select>` positioned above the Device Folders panel
  - [x] Wire the `sl-select` `sl-change` event: on change, call `device.select` RPC with `{ path: event.target.value }`, then call `refreshAndRender()`
  - [x] Call `renderDevicePicker()` from both render paths (narrow and standard layouts) — insert ABOVE `renderDeviceFolders()`

- [x] **Frontend: Basket reload on device switch** (AC: #2)
  - [x] After `device.select` RPC succeeds, call `manifest_get_basket` RPC to reload basket items for the newly selected device
  - [x] Update `basketStore` with the new basket items via `hydrateFromDaemon`
  - [x] Call `refreshAndRender()` to reflect the new device's state in the full UI

- [x] **Unit tests** (AC: all)
  - [x] `device/mod.rs`: test `handle_device_detected` with two sequential devices → both in `connected_devices`, first is auto-selected
  - [x] `device/mod.rs`: test `handle_device_removed` for selected device with another remaining → remaining device is auto-selected
  - [x] `device/mod.rs`: test `handle_device_removed` for non-selected device → selection unchanged
  - [x] `rpc.rs`: test `device.list` returns all connected devices
  - [x] `rpc.rs`: test `device.select` with valid path → ok, with unknown path → error

## Dev Notes

### Critical Architecture Change: DeviceManager Refactor

The `DeviceManager` currently has `current_device` and `current_device_path` as its primary device fields. These must be **replaced** (not added to) by `connected_devices` and `selected_device_path`. All existing public API (`get_current_device`, `get_current_device_path`, `update_manifest`) must continue to work by reading from the new structure — **no callers change**.

```rust
pub struct DeviceManager {
    db: Arc<Database>,
    // REPLACED: current_device + current_device_path → connected_devices + selected_device_path
    connected_devices: Arc<RwLock<HashMap<PathBuf, DeviceManifest>>>,
    selected_device_path: Arc<RwLock<Option<PathBuf>>>,
    unrecognized_device_path: Arc<RwLock<Option<PathBuf>>>,
}
```

### Locking Order (Deadlock Prevention)

When acquiring multiple locks simultaneously, ALWAYS acquire in this order:
1. `connected_devices` write lock
2. `selected_device_path` write lock
3. `unrecognized_device_path` write lock

Never hold a read lock on one field while acquiring a write lock on another.

### `update_manifest` Refactor

The current implementation takes a write lock on `current_device` and a read lock on `current_device_path`. With the new structure:

```rust
pub async fn update_manifest<F>(&self, mutation: F) -> Result<()>
where
    F: FnOnce(&mut DeviceManifest),
{
    let selected_path = self.selected_device_path.read().await.clone();
    let path = selected_path.ok_or_else(|| anyhow::anyhow!("No device connected"))?;
    let mut devices = self.connected_devices.write().await;
    let manifest = devices
        .get_mut(&path)
        .ok_or_else(|| anyhow::anyhow!("Selected device not in connected map"))?;
    mutation(manifest);
    crate::device::write_manifest(&path, manifest).await?;
    Ok(())
}
```

### `handle_device_removed` — Auto-Select on Single Remaining

When the selected device is removed and one device remains, auto-select it:

```rust
pub async fn handle_device_removed(&self, removed_path: &PathBuf) {
    {
        let mut devices = self.connected_devices.write().await;
        devices.remove(removed_path);
    }
    {
        let mut sel = self.selected_device_path.write().await;
        if sel.as_ref() == Some(removed_path) {
            *sel = None;
            // Auto-select if exactly one device remains
            let devices = self.connected_devices.read().await;
            if devices.len() == 1 {
                *sel = devices.keys().next().cloned();
            }
        }
    }
    {
        let mut unrecognized = self.unrecognized_device_path.write().await;
        *unrecognized = None;
    }
}
```

Note: `handle_device_removed` in `main.rs` currently calls it without a path argument. The event is `DeviceEvent::Removed(path)` — pass that path through.

### `main.rs` Event Handler — Pass Path to `handle_device_removed`

Current signature is `handle_device_removed(&self)` with no path arg. This must change to `handle_device_removed(&self, removed_path: &PathBuf)`. Update the call site in `main.rs`:

```rust
device::DeviceEvent::Removed(path) => {
    device_manager.handle_device_removed(&path).await;
    // ... existing state_tx.send(DaemonState::Idle) logic unchanged
}
```

### RPC Naming Convention

Per architecture mandate: dot-notation RPC names use dots (e.g., `device.list`, `device.select`). This is consistent with existing `basket.autoFill`, `sync.setAutoFill`, `device_profiles.list` — note the mixed convention. New methods follow `device.list` / `device.select` pattern (lowercase dot-notation). Match in handler dispatch must be exact string.

### `get_daemon_state` Extension

```rust
let connected_devices_snapshot = state.device_manager.get_connected_devices().await;
let selected_device_path = state.device_manager.get_current_device_path().await
    .map(|p| p.to_string_lossy().to_string());

let connected_devices_json: Vec<_> = connected_devices_snapshot.iter().map(|(p, m)| {
    serde_json::json!({
        "path": p.to_string_lossy(),
        "deviceId": m.device_id,
        "name": m.name.clone().unwrap_or_else(|| m.device_id.clone()),
    })
}).collect();

// Add to existing json!({ ... }) response:
// "connectedDevices": connected_devices_json,
// "selectedDevicePath": selected_device_path,
```

### Frontend: `sl-select` Event Handling in Lit/Vanilla TS

`BasketSidebar` uses innerHTML-based rendering (not Lit). After calling `refreshAndRender()`, wire events in `addEventListeners()` (or the equivalent post-render hook already used for other buttons). Find the select element and attach:

```typescript
const devicePicker = this.container.querySelector('sl-select.device-picker') as any;
if (devicePicker) {
    devicePicker.addEventListener('sl-change', async (e: CustomEvent) => {
        const newPath = (e.target as any).value;
        await rpcCall('device.select', { path: newPath });
        // Reload basket for new device
        const basketResult = await rpcCall('manifest_get_basket') as any;
        basketStore.setItems(basketResult?.basketItems ?? []);
        this.refreshAndRender();
    });
}
```

Look at how the existing `init-device-btn` and `open-repair-btn` listeners are wired — follow the same `container.querySelector` + `addEventListener` pattern in the post-render event binding.

### Frontend: `truncatePath` Helper

Path display in picker options should truncate long paths. Implement a simple inline helper or reuse any existing path display logic in `BasketSidebar`:

```typescript
private truncatePath(path: string, maxLen = 30): string {
    if (path.length <= maxLen) return path;
    return '...' + path.slice(path.length - maxLen + 3);
}
```

### UX: Picker Placement

Per UX spec (section 5.4): "Positioned above the Device State panel (above the Managed Zone shield)." In the rendered HTML, `renderDevicePicker()` must be called **before** `renderDeviceFolders()` in both render paths. The picker is hidden entirely (empty string) when only one device is connected.

### Previous Story Intelligence (Story 2.6)

From Story 2.6 implementation:
- `handle_device_unrecognized` currently clears `current_device` and `current_device_path` to enforce mutual exclusivity. After refactor, it must instead ensure: the `unrecognized_device_path` is set, and the path is NOT present in `connected_devices` (it shouldn't be — unrecognized devices have no manifest). Do NOT change `selected_device_path` when an unrecognized device arrives, since other recognized devices might still be connected.
- `get_daemon_state` already returns `pendingDevicePath` — keep this field; just ADD the two new fields alongside it.
- `InitDeviceModal.ts` triggers `device_initialize` RPC on success, which calls `DeviceManager::initialize_device`. After refactor, `initialize_device` must: write manifest, add device to `connected_devices`, set `selected_device_path = Some(path)`, clear `unrecognized_device_path`. Review `initialize_device` implementation and update accordingly.
- RepairModal and InitDeviceModal patterns are confirmed working — do not change them.

### File Structure

- `hifimule-daemon/src/device/mod.rs` — Replace `current_device` + `current_device_path` fields with `connected_devices` + `selected_device_path`; update `new()`, `handle_device_detected`, `handle_device_removed`, `handle_device_unrecognized`, `initialize_device`, `get_current_device`, `get_current_device_path`, `update_manifest`, `get_device_storage`; add `get_connected_devices()`
- `hifimule-daemon/src/main.rs` — Update `DeviceEvent::Removed` handler to pass path to `handle_device_removed`
- `hifimule-daemon/src/rpc.rs` — Add `device.list` dispatch + handler; add `device.select` dispatch + handler; extend `handle_get_daemon_state` with `connectedDevices` + `selectedDevicePath`
- `hifimule-ui/src/components/BasketSidebar.ts` — Add `connectedDevices` + `selectedDevicePath` instance vars; add `renderDevicePicker()`; update polling; wire `sl-change` event; reload basket on device switch

### References

- Architecture DeviceManager spec: `_bmad-output/planning-artifacts/architecture.md` (DeviceManager Struct section)
- Multi-Device IPC spec: `_bmad-output/planning-artifacts/architecture.md` (Multi-Device IPC section)
- UX picker spec: `_bmad-output/planning-artifacts/ux-design-specification.md` (section 5.4)
- Current `DeviceManager` struct: `hifimule-daemon/src/device/mod.rs:178–193`
- `handle_device_detected`: `device/mod.rs:195–231`
- `handle_device_removed`: `device/mod.rs:255–262`
- `handle_device_unrecognized`: `device/mod.rs:233–249`
- `initialize_device`: search `device/mod.rs` for `initialize_device`
- `update_manifest`: `device/mod.rs:276–289`
- `handler` dispatch: `rpc.rs:123–172`
- `handle_get_daemon_state`: `rpc.rs:357–400`
- `DeviceEvent::Removed` in `main.rs`: `main.rs:288–319`
- `BasketSidebar.startDaemonStatePolling`: `BasketSidebar.ts:438–468`
- `BasketSidebar.renderDeviceFolders`: `BasketSidebar.ts:470–543`
- Post-render event wiring pattern: search `BasketSidebar.ts` for `querySelector('.*btn')`

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

- Updated existing test `test_handle_device_unrecognized_clears_current_device` → renamed to `test_handle_device_unrecognized_preserves_recognized_device` because Story 2.7 explicitly changes this behavior: unrecognized device no longer clears the selected recognized device.
- Fixed two existing call sites of `handle_device_removed()` (in `device/tests.rs:813` and `src/tests.rs:93`) that were using the old zero-arg signature.

### Completion Notes List

- `DeviceManager` refactored: `current_device` + `current_device_path` replaced by `connected_devices: HashMap<PathBuf, DeviceManifest>` + `selected_device_path: Option<PathBuf>`. All public API (`get_current_device`, `get_current_device_path`, `update_manifest`) remains unchanged for callers.
- `handle_device_detected` now inserts into `connected_devices` map and auto-selects only if no device is currently selected (first/only device).
- `handle_device_removed` takes a path argument, removes from map, clears/auto-selects as specified. `main.rs` updated to pass the event path.
- `handle_device_unrecognized` no longer clears `selected_device_path` — recognized devices remain available when an unrecognized device arrives.
- `initialize_device` updated to insert into `connected_devices` and set `selected_device_path`.
- New `get_connected_devices()` and `select_device()` helpers added to `DeviceManager`.
- New RPCs `device.list` and `device.select` added to `rpc.rs`.
- `get_daemon_state` extended with `connectedDevices` and `selectedDevicePath` fields.
- `BasketSidebar.ts`: `connectedDevices` + `selectedDevicePath` instance vars; `renderDevicePicker()` renders `<sl-select>` above Device Folders when 2+ devices connected; `bindDevicePickerEvents()` wires `sl-change` → `device.select` RPC + basket reload; both render paths updated.
- 163 Rust tests pass (0 failures). TypeScript compiles clean.

### File List

- `hifimule-daemon/src/device/mod.rs`
- `hifimule-daemon/src/device/tests.rs`
- `hifimule-daemon/src/main.rs`
- `hifimule-daemon/src/rpc.rs`
- `hifimule-daemon/src/tests.rs`
- `hifimule-ui/src/components/BasketSidebar.ts`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`

### Change Log

- 2026-04-02: Implemented Story 2.7 — Multi-Device Selection Panel. Backend DeviceManager refactored for concurrent device tracking; new `device.list` and `device.select` RPCs; `get_daemon_state` extended; frontend device picker added to BasketSidebar with event wiring and basket reload on device switch. 163 tests pass.
