# Story 2.9: Device Identity — Name & Icon

Status: done

## Story

As a System Admin (Alexis),
I want to give each device a custom name and icon when I initialize it,
So that I can instantly recognize my devices in the hub without staring at raw IDs.

## Acceptance Criteria

1. **Initialize Device Dialog — Identity Fields:**
   - **Given** the "Initialize Device" dialog is open
   - **Then** a "Device Name" text input is shown (required, max 40 chars, prefilled with "My Device")
   - **And** an icon picker grid is shown with ~6 device-type icon options (USB Drive, Phone, Watch, SD Card, Headphones, Music Player)
   - **And** the Confirm button is disabled while the name field is empty
   - **When** I click "Confirm"
   - **Then** the name and icon are written to the manifest alongside existing fields (folder path, transcoding profile)

2. **Device Hub Shows Identity:**
   - **Given** a device with a name and icon is connected
   - **When** it appears in the device hub (rendered by Story 2.8's `renderDeviceHub()`)
   - **Then** its icon is displayed alongside its name (already working via `d.icon ?? 'usb-drive'` fallback)
   - **And** if no icon is set, the default "usb-drive" icon is shown (Story 2.8 fallback, unchanged)
   - **And** if no name is set, the device_id is shown (Story 2.8 fallback `name || deviceId`, unchanged)

## Tasks / Subtasks

- [x] **Daemon: `device/mod.rs` — Add `icon` to `DeviceManifest` + fix `name` serde(default)** (AC: #1, #2)
  - [x] Add `#[serde(default)]` to the existing `name: Option<String>` field (line ~51) — **this is missing and breaks deserialization of old manifests that lack the `name` key**
  - [x] Add `icon: Option<String>` with `#[serde(default)]` immediately below the `name` field:
    ```rust
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    ```
  - [x] Update `initialize_device()` signature (line ~358) to accept `name: String` and `icon: Option<String>`:
    ```rust
    pub async fn initialize_device(
        &self,
        folder_path: &str,
        transcoding_profile_id: Option<String>,
        name: String,
        icon: Option<String>,
    ) -> Result<DeviceManifest>
    ```
  - [x] In the `DeviceManifest { ... }` construction inside `initialize_device()`, set:
    ```rust
    name: Some(name).filter(|s| !s.is_empty()),
    icon,
    ```
    (The existing `name: None` line at ~line 401 must be replaced)

- [x] **Daemon: `rpc.rs` — Extract `name`/`icon` in `handle_device_initialize()`** (AC: #1)
  - [x] After extracting `transcoding_profile_id` (~line 1408), extract `name` and `icon`:
    ```rust
    let device_name = params["name"].as_str().ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing name".to_string(),
        data: None,
    })?.to_string();
    if device_name.len() > 40 {
        return Err(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Device name exceeds 40 characters".to_string(),
            data: None,
        });
    }
    let device_icon = params["icon"].as_str().map(|s| s.to_string());
    ```
  - [x] Update the `initialize_device()` call (~line 1433) to pass the new params:
    ```rust
    .initialize_device(folder_path, transcoding_profile_id.clone(), device_name, device_icon)
    ```

- [x] **Daemon: `rpc.rs` — Add `icon` to `handle_device_list()` and `connectedDevices` in `handle_get_daemon_state()`** (AC: #2)
  - [x] In `handle_device_list()` (~line 1855), add `"icon": m.icon.clone()` to the JSON object:
    ```rust
    serde_json::json!({
        "path": p.to_string_lossy(),
        "deviceId": m.device_id,
        "name": m.name.clone().unwrap_or_else(|| m.device_id.clone()),
        "icon": m.icon.clone(),   // NEW
    })
    ```
  - [x] In `handle_get_daemon_state()` (~line 409), add `"icon": m.icon.clone()` to `connected_devices_json`:
    ```rust
    serde_json::json!({
        "path": p.to_string_lossy(),
        "deviceId": m.device_id,
        "name": m.name.clone().unwrap_or_else(|| m.device_id.clone()),
        "icon": m.icon.clone(),   // NEW
    })
    ```

- [x] **Frontend: `InitDeviceModal.ts` — Add name input + icon picker to `renderContent()`** (AC: #1)
  - [x] In `renderContent()`, add the name input ABOVE the folder input section
  - [x] Add `private iconLabel(icon: string): string` helper method
  - [x] Add a `selectedIcon` closure variable (initialized to `'usb-drive'`) in `renderContent()` before the HTML assignment
  - [x] After the HTML assignment, wire up icon tile click listeners
  - [x] Disable/enable the Confirm button based on name field value; replaced existing `confirmBtn.disabled = false` block
  - [x] Update `handleConfirm()` signature and body to accept and use name/icon

- [x] **Verify TypeScript compiles cleanly** (AC: all)
  - [x] `rtk tsc` passes with 0 errors after all changes

- [x] **Verify Rust compiles cleanly** (AC: all)
  - [x] `rtk cargo build` passes with 0 errors after all changes

## Dev Notes

### What Story 2.8 Already Built (Do NOT Touch)

Story 2.8 is fully implemented. These already work correctly and require **zero changes**:

- **`BasketSidebar.ts`**: `connectedDevices` type already includes `icon?: string | null`. The `renderDeviceHub()` method already renders `d.icon ?? 'usb-drive'`. **No changes to `BasketSidebar.ts` or `styles.css`.**
- **`renderDeviceHub()`**: Fully functional device hub with icon rendering from Story 2.8. Story 2.9 only needs the daemon to return the `icon` field — the frontend is already wired.
- **Daemon `DeviceManager`**: `connected_devices`, `selected_device_path`, `get_multi_device_snapshot()`, `select_device()` — all unchanged.
- **`device.list` RPC routing**: Exists at `rpc.rs:167`. Story 2.9 only adds the `icon` field to the response JSON.

### Critical: RPC Method Name Is `device_initialize` (Underscore)

The architecture doc mentions `device.initialize` but the actual routing key is:
```
"device_initialize" => handle_device_initialize(&state, payload.params).await,
```
And the frontend calls: `rpcCall('device_initialize', { ... })`. Do **NOT** change the method name or create a `device.initialize` variant.

### `DeviceManifest` — `name` Field Is Missing `#[serde(default)]`

The current `name: Option<String>` field at line ~51 of `device/mod.rs` lacks `#[serde(default)]`. This means manifests written before Story 2.7 that don't have a `name` key will fail to deserialize. Story 2.9 **must** add `#[serde(default)]` to `name` (even if it seems unrelated to the new `icon` feature).

### `initialize_device()` Currently Sets `name: None`

The current `DeviceManifest` construction inside `initialize_device()` has `name: None` (line ~401). Story 2.9 changes this to accept `name` and `icon` as parameters and stores them. Use `Some(name).filter(|s| !s.is_empty())` to avoid storing empty strings as `Some("")`.

### `handleConfirm()` Refactor Pattern

The current `handleConfirm(userId: string)` signature must become `handleConfirm(userId: string, selectedIcon: string, nameInputEl: any)`. The `selectedIcon` and `nameInputEl` are closure variables in `renderContent()` — they are passed into `handleConfirm()` directly, avoiding class-level state.

The existing confirm button wiring is:
```typescript
// EXISTING (replace entirely):
if (confirmBtn) {
    confirmBtn.disabled = false;
    confirmBtn.addEventListener('click', () => this.handleConfirm(userId));
}
```
Replace with the new validation-aware version described in the task above.

### Icon Picker: Shoelace `<sl-icon>` Not Custom SVGs

The architecture mentions "~6–8 SVG icons embedded in UI" but given the codebase already uses Shoelace `<sl-icon name="...">` for device icons in the hub (Story 2.8), use the same `<sl-icon name="...">` approach. The `icon` value stored in the manifest is the Shoelace Bootstrap Icons name string (e.g., `"usb-drive"`, `"phone-fill"`).

The 6 supported icon names (all available in Bootstrap Icons via Shoelace):
- `usb-drive` — USB Drive (default, already used as fallback)
- `phone-fill` — Phone
- `watch` — Watch
- `sd-card` — SD Card
- `headphones` — Headphones
- `music-note-list` — DAP / generic music player

### Icon Picker Selected State: Inline Styles (No CSS File Changes)

Story 2.8 already added all needed CSS to `styles.css`. The icon picker selection state in `InitDeviceModal.ts` uses **inline styles** (no CSS file changes needed) to toggle the selected tile border/background. This follows the existing pattern in `InitDeviceModal.ts` which uses inline styles throughout.

### Confirm Button Wiring: `sl-input` Event, Not `input`

The existing `InitDeviceModal.ts` uses Shoelace components exclusively. For `<sl-input>`, use the `sl-input` event (not native `input`) to detect value changes:
```typescript
nameInput?.addEventListener('sl-input', () => {
    if (confirmBtn) confirmBtn.disabled = !nameInput.value?.trim();
});
```

### Name Validation Location

- **Frontend**: Confirm button disabled when name is empty (UX guard)
- **Daemon**: 40-char server-side validation in `handle_device_initialize()` (security guard)
- Do NOT add name validation to `initialize_device()` in `device/mod.rs` — keep business logic in the RPC handler

### `handleConfirm` `selectedIcon` Closure Variable

`selectedIcon` starts as `'usb-drive'` (the default icon, also the first tile rendered as selected). If the user never clicks an icon tile, `'usb-drive'` is submitted, which is correct and intentional.

If the user clears the name field and the Confirm button is re-disabled, `selectedIcon` retains its last-selected value — this is correct (icon selection is independent of name validation).

### File Structure

**Only these files change:**
- `hifimule-daemon/src/device/mod.rs` — `DeviceManifest.name` serde(default), new `icon` field, `initialize_device()` signature
- `hifimule-daemon/src/rpc.rs` — `handle_device_initialize()` params, `handle_device_list()` response, `handle_get_daemon_state()` connectedDevices
- `hifimule-ui/src/components/InitDeviceModal.ts` — name input, icon picker, confirm wiring
- `hifimule-ui/src/components/BasketSidebar.ts` — wire `#init-device-btn` click → `openInitDeviceModal()` (was rendered in Story 2.8 but never connected)

**These files do NOT change:**
- `hifimule-ui/src/styles.css` — no new classes needed
- `hifimule-daemon/src/db.rs` — no DB schema changes
- `hifimule-daemon/src/main.rs` — no changes

### References

- Previous story: `_bmad-output/implementation-artifacts/2-8-enhanced-multi-device-hub.md`
- `DeviceManifest` struct: `hifimule-daemon/src/device/mod.rs:49–73`
- `initialize_device()`: `hifimule-daemon/src/device/mod.rs:358–430`
- `handle_device_initialize()`: `hifimule-daemon/src/rpc.rs:1385–1483`
- `handle_device_list()`: `hifimule-daemon/src/rpc.rs:1850–1863`
- `handle_get_daemon_state()` connectedDevices JSON: `hifimule-daemon/src/rpc.rs:406–415`
- `InitDeviceModal.ts` `renderContent()`: `hifimule-ui/src/components/InitDeviceModal.ts:89–162`
- `InitDeviceModal.ts` `handleConfirm()`: `hifimule-ui/src/components/InitDeviceModal.ts:205–234`
- `BasketSidebar.ts` `connectedDevices` type (already has `icon`): `hifimule-ui/src/components/BasketSidebar.ts` (Story 2.8)
- Architecture (Manifest Extension with name/icon): `_bmad-output/planning-artifacts/architecture.md` line ~80
- Architecture (Multi-Device IPC, device.initialize params): `_bmad-output/planning-artifacts/architecture.md` line ~106
- UX spec (Device Identity in Initialize Dialog): `_bmad-output/planning-artifacts/ux-design-specification.md` lines ~92–95

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

None — all changes compiled cleanly on first attempt.

### Completion Notes List

- Added `#[serde(default)]` to `name: Option<String>` in `DeviceManifest` to fix backward-compat deserialization of old manifests.
- Added `icon: Option<String>` with `#[serde(default)]` to `DeviceManifest` struct.
- Updated `initialize_device()` to accept `name: String` and `icon: Option<String>`; stores name as `Some(name).filter(|s| !s.is_empty())` to avoid empty strings.
- Added `device_name` (required, max 40 chars by char count, empty-string rejected) and `device_icon` (optional, whitelist-validated against 6 known icons, empty string filtered to None) extraction in `handle_device_initialize()` with full server-side validation.
- Added `"icon": m.icon.clone()` to JSON responses in both `handle_device_list()` and `handle_get_daemon_state()` `connected_devices_json`.
- Added Device Name text input (required, prefilled "My Device", max 40 chars, `sl-input` validation) above folder input in `InitDeviceModal.ts renderContent()`.
- Added icon picker grid with 6 Shoelace Bootstrap Icons tiles (usb-drive default); inline-style selection state toggled on click via closure variable.
- Replaced old `confirmBtn.disabled = false` + click handler with validation-aware version; confirm button disabled while name is empty, re-enabled via `sl-input` event.
- Updated `handleConfirm()` to accept `selectedIcon` and `nameInputEl` params; sends `name` and `icon` in `device_initialize` RPC payload.
- Added `iconLabel()` private helper for human-readable icon tile labels.
- Rust: 0 errors, 4 pre-existing warnings (unchanged). TypeScript: 0 errors.

### File List

- `hifimule-daemon/src/device/mod.rs`
- `hifimule-daemon/src/rpc.rs`
- `hifimule-ui/src/components/InitDeviceModal.ts`
- `hifimule-ui/src/components/BasketSidebar.ts`

## Change Log

- 2026-04-04: Story created — Device Identity (name + icon) for InitDeviceModal and daemon manifest/RPC extension.
- 2026-04-04: Implementation complete — all tasks done, Rust and TypeScript compile cleanly.
- 2026-04-05: Code review complete — 7 patches applied: char-count name validation, empty-name server guard, icon whitelist + empty-string filter, confirmBtn initial state uses `.value` not `getAttribute`, listener deduplication via clone+replace (fixed: sl-input now references live button), icon label "Music Player". Spec amended to include BasketSidebar.ts (Story 2.8 had rendered #init-device-btn without wiring it).
