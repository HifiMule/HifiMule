# Sprint Change Proposal — Enhanced Multi-Device Hub & Device Identity

**Date:** 2026-04-03
**Author:** Alexis (with SM agent)
**Status:** Approved (2026-04-03)

---

## 1. Issue Summary

**Problem Statement:** The multi-device UX implemented in Story 2.7 was insufficient after real-world testing. The picker hid itself when only one device was connected, there was no concept of a "no device selected" state, the basket had no locked/empty mode, and device initialization (Story 2.6) provided no way to assign a human-readable name or a visual icon to a device.

**Discovery:** Identified during active use following Story 2.7 delivery. The iTunes mental model — always-visible device management, named and iconized devices, clear locked state when no device is active — was the intended experience but was not captured in the original story.

**Evidence:**
- `BasketSidebar.ts renderDevicePicker()`: guarded by `connectedDevices.length > 1`, making the hub invisible for single-device users.
- `device/mod.rs initialize_device()`: sets `name: None` unconditionally — no name is ever captured from the user.
- `DeviceManifest` struct: has `name: Option<String>` but no `icon` field.
- No "no-device-selected" locked state exists anywhere in the UI or basket rendering logic.

---

## 2. Impact Analysis

### Epic Impact

| Epic | Impact |
|------|--------|
| Epic 2 | Two new stories added (2.8, 2.9); Story 2.7 picker behavior superseded |
| Epic 3 | Library browser add buttons gain a disabled state driven by device selection (scoped within Story 2.8) |
| Epics 1, 4, 5, 6 | Unaffected |

### Story Impact

| Story | Change | Reason |
|-------|--------|--------|
| 2.6 (done) | Superseded in part by 2.9 — init dialog needs name + icon inputs | Initialization never captured device identity |
| 2.7 (done) | Picker behavior superseded by 2.8 | Picker hidden for single device; no locked state |
| 2.8 (new) | **Add** — Enhanced Multi-Device Hub | Always-visible hub, no-selection state, basket locking |
| 2.9 (new) | **Add** — Device Identity (Name & Icon) | Name input + icon picker during init; icon field in manifest |

### Artifact Conflicts

| Artifact | Sections Affected |
|----------|------------------|
| PRD | FR26 updated (init captures name + icon); FR33 updated (always-visible hub, locked state); new MVP bullet for Device Identity |
| Architecture | `DeviceManifest` gains `icon` field; `device.initialize` params extended; `device.list` / `get_daemon_state` return `icon`; Multi-Device Tracker description updated |
| UX Spec | New §5.6 Device Hub; §5.4 gains Device Identity subsection (name input + icon picker) |

### Technical Impact

- **`DeviceManifest`** (`device/mod.rs`): add `icon: Option<String>` with `#[serde(default)]` — backward-compatible with existing manifests.
- **`initialize_device()`** (`device/mod.rs`): accept `name: String` and `icon: Option<String>` params; write both to manifest.
- **`device.initialize` RPC** (`rpc.rs`): extract `name` and `icon` from params alongside existing `folderPath` and `profileId`.
- **`device.list` + `get_daemon_state`** (`rpc.rs`): add `icon` field to each device entry in the response.
- **`BasketSidebar.ts`**: remove `connectedDevices.length > 1` guard on `renderDevicePicker()`; add no-device-selected locked state; disable add buttons when `selectedDevicePath === null`.
- **`InitDeviceModal.ts`**: add `<sl-input>` for device name and icon picker grid.
- No new external dependencies.

---

## 3. Recommended Approach

**Direct Adjustment** — add two stories to Epic 2, update three planning artifacts.

**Rationale:**
- The daemon's `DeviceManager`, `connected_devices` map, `device.select`, and `device.list` RPCs are fully reused — no daemon restructuring needed.
- Story 2.8 is primarily a UI change: remove a guard, add a locked state, always render the hub.
- Story 2.9 is a small manifest extension + init dialog UI addition.
- Stories 2.8 and 2.9 are independently implementable and testable.
- Epic 6 (current sprint) is entirely unaffected.

**Effort:** Low
**Risk:** Low — daemon patterns proven; UI changes are targeted and reduce hidden-state complexity
**Timeline Impact:** Two new stories added to Epic 2 backlog; does not block Epic 6

---

## 4. Detailed Change Proposals

### 4.1 New Story 2.8 — Enhanced Multi-Device Hub

**Story: [2.8] Enhanced Multi-Device Hub**
**Epic:** Epic 2 — Connection & Verification (The Handshake)
**Supersedes:** Story 2.7 picker behavior

*As a System Admin (Alexis) and Ritualist (Arthur),
I want a persistent device hub I can always interact with — switching between connected devices or deselecting one entirely — so that I have full, iTunes-style control over which device I'm working with at all times.*

**Acceptance Criteria:**

*Picker always visible*
- Given the main UI is open
- When 1 or more devices are connected
- Then a device hub is displayed (not hidden when only 1 device is connected)
- And each device shows its name (or device_id if unnamed) and its icon

*No-device-selected state*
- Given no device is selected (selectedDevicePath === null)
- Then the basket is displayed as empty with a placeholder message "Select a device to start curating"
- And all (+) add buttons in the library browser are disabled/greyed out
- And the "Start Sync" button is disabled

*Device selection*
- Given one or more devices are shown in the hub
- When I click a device
- Then the UI calls device.select RPC and loads that device's basket
- (existing basket-reload-on-switch behavior from Story 2.7 is unchanged)

*No device connected*
- Given all devices are disconnected
- When the daemon fires device-removed events
- Then the no-device-selected state is shown (basket clears, adds locked)

*Single device auto-select (unchanged)*
- Given exactly one managed device is connected and none was previously selected
- Then it is auto-selected (daemon behavior unchanged)

**Technical Notes:**
- `BasketSidebar.ts renderDevicePicker()`: remove `connectedDevices.length > 1` guard — render hub whenever `connectedDevices.length >= 1`
- Add no-device check: if `selectedDevicePath === null`, render locked basket placeholder and disable add buttons (emit `device-locked` CSS class on library container)
- Library browser add buttons: check shared device-selected state before executing add RPC
- Daemon: no changes needed — `selectedDevicePath` already supports `null`

---

### 4.2 New Story 2.9 — Device Identity (Name & Icon)

**Story: [2.9] Device Identity — Name & Icon**
**Epic:** Epic 2 — Connection & Verification (The Handshake)

*As a System Admin (Alexis),
I want to give each device a custom name and icon when I initialize it,
So that I can instantly recognize my devices in the hub without staring at raw IDs.*

**Acceptance Criteria:**

*Name input during initialization*
- Given the "Initialize Device" dialog (Story 2.6) is open
- Then a "Device Name" text input is shown (required, max 40 chars)
- And it defaults to the device's volume label or "My Device" if unavailable
- When I click "Confirm"
- Then the name is written to the manifest as the `name` field

*Icon selection during initialization*
- Given the "Initialize Device" dialog is open
- Then an icon picker is shown with a small library of device-type icons (e.g., iPod Classic, Generic DAP, SD Card, USB Drive, Watch, Phone)
- When I click an icon
- Then it is visually selected (highlighted border)
- When I click "Confirm"
- Then the icon identifier is written to the manifest as the `icon` field

*Display in hub*
- Given a device with a name and icon is connected
- When it appears in the device hub (Story 2.8)
- Then its icon is displayed alongside its name
- And if no icon is set, a default "USB Drive" icon is shown
- And if no name is set, the device_id is shown (existing fallback, unchanged)

**Technical Notes:**
- `DeviceManifest`: add `icon: Option<String>` with `#[serde(default)]` — backward-compatible with existing manifests
- `device.initialize` RPC params: add `name: String` and `icon: Option<String>`
- `device/mod.rs initialize_device()`: accept and store name + icon into manifest
- `device.list` + `get_daemon_state`: add `icon` field to each device entry
- `InitDeviceModal.ts`: add `<sl-input>` for device name + icon picker grid (~6–8 SVG icons embedded in UI, no external fetch)

---

### 4.3 PRD Updates

**New MVP bullet:**
```
OLD: (no device identity bullet)

ADD:
- Device Identity: During device initialization, users can assign a custom display 
  name and select an icon from a built-in library. The name and icon are stored in 
  the device manifest and displayed in the device hub for instant visual recognition.
```

**FR26:**
```
OLD:
FR26: The system can initialize a new `.hifimule.json` manifest on a connected 
  device that has not previously been managed, capturing a hardware identifier, a 
  designated sync folder path, and an associated Jellyfin user profile.

NEW:
FR26: The system can initialize a new `.hifimule.json` manifest on a connected 
  device that has not previously been managed, capturing a hardware identifier, a 
  designated sync folder path, an associated Jellyfin user profile, a user-provided 
  display name, and an optional icon identifier selected from a built-in library.
```

**FR33:**
```
OLD:
FR33: When multiple managed devices are connected simultaneously, the system presents 
  a device selection UI and allows the user to switch the active device context without 
  restarting or reconnecting.

NEW:
FR33: The system presents a persistent device hub showing all connected managed devices, 
  each identified by its name and icon. The user can switch the active device context at 
  any time. When no device is selected, the basket is empty and adding items is disabled.
```

**FR Coverage Map additions:**
```
FR33: Epic 2 — Enhanced Multi-Device Hub (Story 2.8)
FR26: Epic 2 — Device Identity (Story 2.9)
```

---

### 4.4 Architecture Updates

**Data Architecture — Manifest Extension:**
```
OLD:
Manifest Extension: `.hifimule.json` includes `auto_sync_on_connect` (boolean), 
  `auto_fill` block, and `transcoding_profile_id` (string | null).

NEW:
Manifest Extension: `.hifimule.json` includes `auto_sync_on_connect` (boolean), 
  `auto_fill` block, `transcoding_profile_id` (string | null), `name` (string | null), 
  and `icon` (string | null). Both `name` and `icon` use `#[serde(default)]` for 
  backward compatibility with manifests written before Story 2.9.
```

**Multi-Device IPC:**
```
OLD:
device.list → Array<{ path: string, deviceId: string, name: string | null }>
device.initialize(params: { folderPath: string, profileId: string })

NEW:
device.list → Array<{ path: string, deviceId: string, name: string | null, 
  icon: string | null }>
device.initialize(params: { folderPath: string, profileId: string, 
  name: string, icon: string | null })
get_daemon_state connectedDevices: Array<{path, deviceId, name, icon}>
```

**Multi-Device Tracker (Daemon Responsibilities):**
```
OLD:
Multi-Device Tracker: Maintains a map of all currently connected managed devices; 
  exposes selection API so the UI can switch the active device context without restart.

NEW:
Multi-Device Tracker: Maintains a map of all currently connected managed devices; 
  exposes selection API so the UI can switch the active device context at any time. 
  selectedDevicePath may be null; when null, the UI enters a locked state (basket empty, 
  add buttons disabled). The device hub is always visible when at least one device is connected.
```

---

### 4.5 UX Spec Updates

**New Section 5.6 — Device Hub:**
```
### 5.6 Device Hub

The device hub is a persistent panel displayed whenever at least one managed device 
is connected. It replaces the conditional <sl-select> picker from Story 2.7.

Device cards:
- Each connected device is shown as a compact card containing:
  - Its icon (from the built-in icon library; fallback: generic USB Drive icon)
  - Its display name (fallback: device_id if no name is set)
- The currently selected device card is highlighted with an active border/accent
- Clicking any card calls device.select and reloads the basket for that device

No-device-selected state:
- When selectedDevicePath === null, the hub shows a placeholder:
  "Select a device to start curating"
- The basket renders as empty with no items and no storage projection bar
- All (+) add buttons in the library browser render as disabled (greyed out, 
  no click interaction)
- The "Start Sync" button is disabled

Single device:
- The hub is still visible with a single device (not hidden)
- The single device is auto-selected by the daemon; its card renders as active
```

**Section 5.4 addition — Device Identity:**
```
ADD under 5.4:
Device Identity (shown in the Initialize Device dialog — Story 2.9):
- <sl-input> labelled "Device Name" — required, max 40 chars, prefilled with 
  volume label or "My Device"
- Icon picker: a grid of ~6–8 icon options (iPod Classic, Generic DAP, SD Card, 
  USB Drive, Watch, Phone, etc.) rendered as selectable tiles with a highlighted 
  border on selection
- Selected icon and name are confirmed with the existing "Confirm" button and 
  written to the manifest
```

---

## 5. Implementation Handoff

### Change Scope: Minor

All changes are within existing epics. No new epics. Epic 6 (current sprint) unaffected.

| Recipient | Responsibility |
|-----------|---------------|
| **Dev** | Implement Story 2.8: remove picker guard in `BasketSidebar.ts`; add no-device locked state; disable add buttons when no device selected |
| **Dev** | Implement Story 2.9: add `icon` field to `DeviceManifest`; extend `initialize_device()` and `device.initialize` RPC; extend `device.list` + `get_daemon_state`; update `InitDeviceModal.ts` |
| **SM / Architect** | Update `epics.md`: add Stories 2.8 and 2.9 to Epic 2 |
| **SM / Architect** | Update `prd.md`: revise FR26, FR33; add Device Identity MVP bullet; update FR coverage map |
| **Architect** | Update `architecture.md`: manifest extension, Multi-Device IPC, Multi-Device Tracker description |
| **SM** | Update `ux-design-specification.md`: new §5.6 Device Hub; §5.4 Device Identity addition |
| **SM** | Update `sprint-status.yaml`: add `2-8` and `2-9` as `backlog` under epic-2 |

### Success Criteria

- [ ] Story 2.8: device hub visible with a single connected device (not hidden)
- [ ] Story 2.8: `selectedDevicePath === null` → basket shows placeholder, add buttons disabled, sync disabled
- [ ] Story 2.8: clicking a device in the hub switches context and reloads basket
- [ ] Story 2.8: `BasketSidebar.ts` has no `connectedDevices.length > 1` guard on hub rendering
- [ ] Story 2.9: Initialize Device dialog includes name input and icon picker
- [ ] Story 2.9: initialized manifest contains `name` and `icon` fields
- [ ] Story 2.9: `device.list` and `get_daemon_state` return `icon` in each device entry
- [ ] Story 2.9: old manifests without `icon` field load without error (serde default)
- [ ] PRD: FR26 mentions name + icon; FR33 describes always-visible hub + locked state
- [ ] Architecture: manifest extension documents `icon`; Multi-Device IPC shows updated params
- [ ] UX Spec: §5.6 describes hub cards, locked state, single-device behavior
