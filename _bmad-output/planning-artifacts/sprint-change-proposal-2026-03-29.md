# Sprint Change Proposal — Multi-Device Selection & Transcoding Normalization

**Date:** 2026-03-29
**Author:** Alexis (with SM agent)
**Status:** Approved (2026-03-29)

---

## 1. Issue Summary

**Problem Statement:** Two gaps were identified during active development. First, when more than one managed device is connected simultaneously, the daemon silently overwrites `current_device` with the last-detected device — there is no mechanism to select or switch between them. Second, the Transcoding Handshake feature was fully implemented via a standalone tech spec (all 13 tasks complete) without a corresponding epic story or PRD update, leaving the planning artifacts out of sync with the codebase.

**Discovery:** Direct testing/use during Epic 2–4 implementation. Transcoding gap identified by reviewing git status showing `transcoding.rs` and `device-profiles.json` as new files against no matching story in `epics.md`.

**Evidence:**
- Multi-device: `DeviceManager` ([device/mod.rs:166-171](../../jellyfinsync-daemon/src/device/mod.rs)) holds a single `current_device` / `current_device_path`; `handle_device_detected` overwrites on every new detection; `get_daemon_state` returns singular `currentDevice` with no list or selection mechanism.
- Transcoding: `tech-spec-transcoding-device-profiles-playback-handshake.md` status `review`, all 13 tasks `[x]`; PRD still lists "Transcoding Handshake" as Phase 2 Post-MVP; `epics.md` has no transcoding story.

---

## 2. Impact Analysis

### Epic Impact

| Epic | Impact |
|------|--------|
| Epic 2 | New Story 2.7 added (Multi-Device Selection) |
| Epic 4 | New Story 4.8 added (Transcoding Handshake — formalizes completed implementation) |
| Epics 1, 3, 5, 6 | Unaffected |

### Story Impact

| Story | Change | Reason |
|-------|--------|--------|
| 2.7 (new) | **Add** — multi-device selection panel, device.list / device.select RPCs, DeviceManager refactor | Second device overwrites first with no recovery path |
| 4.8 (new) | **Add** — transcoding handshake via device profiles (implementation already complete) | Tech spec completed without corresponding epic story |

### Artifact Conflicts

| Artifact | Sections Affected |
|----------|------------------|
| PRD | "Transcoding Handshake" promoted from Post-MVP to MVP; FR31–FR33 added; FR coverage map updated |
| Architecture | DeviceManager struct updated (connected_devices HashMap, selected_device_path); device.list / device.select RPCs added; get_daemon_state schema updated; transcoding RPCs and execute_sync signature documented; device-profiles.json seeding noted |
| UX Spec | §5.4 — Multi-Device Picker component added (sl-select, hidden when single device) |

### Technical Impact

- **DeviceManager refactor (Story 2.7):** `current_device` / `current_device_path` replaced by `connected_devices: HashMap<PathBuf, DeviceManifest>` + `selected_device_path: Option<PathBuf>`. All existing callers of `get_current_device()` remain unchanged — they transparently get the selected device.
- **New RPCs (Story 2.7):** `device.list`, `device.select`. `get_daemon_state` gains `connectedDevices` and `selectedDevicePath` fields.
- **Transcoding (Story 4.8):** Already implemented. No new code. Story is documentation-only closure.
- **No new external dependencies.**

---

## 3. Recommended Approach

**Direct Adjustment** — add two new stories to existing epics; update three planning artifact documents.

**Rationale:**
- No epic restructuring needed. Both stories slot naturally into their respective epics.
- Story 2.7 DeviceManager refactor is self-contained — all downstream operations use `get_current_device()` which just returns the selected slot.
- Story 4.8 has zero implementation cost; it closes the planning gap created by the standalone tech spec workflow.
- Artifact updates are documentation-sync only.

**Effort:** Low
**Risk:** Low — Story 2.7 is a bounded internal refactor with no change to IPC contract shape (only additions); Story 4.8 is zero-effort
**Timeline Impact:** Minimal — one new implementation story; one documentation story

---

## 4. Detailed Change Proposals

### 4.1 New Story 2.7 — Multi-Device Selection Panel

**Story: [2.7] Multi-Device Selection Panel**
**Epic:** Epic 2 — Connection & Verification

*As a System Admin (Alexis) / Ritualist (Arthur),
I want to see all currently connected managed devices and select which one I am working with,
So that I can operate on one specific device without the daemon silently overwriting my context when a second device is plugged in.*

**Acceptance Criteria:**

*Multi-device detection*
- When two or more managed devices are connected simultaneously → the UI displays a device picker listing all connected managed devices (device name from manifest, device_id, path).
- The currently selected device is highlighted. All operations (basket, storage projection, sync, manifest) target the selected device.

*Device switching*
- When I click a different device in the picker → the UI switches context to that device (reloads basket from its manifest, updates storage projection).
- The daemon's active device updates via `device.select` RPC.

*Single-device behaviour (unchanged)*
- When only one managed device is connected → no picker is shown; device is auto-selected; all existing behaviour preserved.

*Device disconnection*
- When the currently selected device disconnects → the UI clears device context (no crash, no stale state).
- If other devices remain connected → picker is shown for remaining devices.

**Technical Notes:**
- Daemon: `DeviceManager` gains `connected_devices: HashMap<PathBuf, DeviceManifest>` and `selected_device_path: Option<PathBuf>`. `handle_device_detected` adds to map; `handle_device_removed` removes from map and clears selection if needed.
- `get_current_device()` returns manifest for `selected_device_path` entry; all existing callers unchanged.
- New RPC `device.list` → `Vec<{path, deviceId, name}>` for all connected devices.
- New RPC `device.select(params: {path: string})` → sets `selected_device_path`; no-op for single-device (still sets it silently).
- `get_daemon_state` gains `connectedDevices: Array<{path, deviceId, name}>` and `selectedDevicePath: string | null`.
- UI: `<sl-select>` or device card list in Device State panel header, rendered only when `connectedDevices.length > 1`.

---

### 4.2 New Story 4.8 — Transcoding Handshake via Device Profiles

**Story: [4.8] Transcoding Handshake via Device Profiles**
**Epic:** Epic 4 — The Sync Engine & Self-Healing Core

*As a Ritualist (Arthur) / Convenience Seeker (Sarah),
I want the sync engine to transcode music to a device-compatible format before writing it,
So that tracks play correctly on DAPs that don't support FLAC, Opus, or AAC.*

**Acceptance Criteria:**

*Profile listing*
- `device_profiles.list` RPC returns available profiles (id, name, description) from `device-profiles.json` including: `passthrough`, `rockbox-mp3-320`, `generic-mp3-192`, `generic-aac-256`.

*Profile assignment*
- `device.set_transcoding_profile(params: {deviceId, profileId})` → writes `transcoding_profile_id` to device manifest AND persists to SQLite `devices` table.

*Transcoded sync*
- When `transcoding_profile_id` is set to a non-passthrough profile → engine calls `POST /Items/{id}/PlaybackInfo` with DeviceProfile payload.
- If Jellyfin returns `TranscodingUrl` → streams from `{base_url}{TranscodingUrl}`.
- If Jellyfin returns `SupportsDirectPlay: true` → falls back to `/Items/{id}/Download`.
- If PlaybackInfo call fails → non-fatal; logged in `SyncFileError`; continues with next file.

*Passthrough behaviour (unchanged)*
- When `transcoding_profile_id` is null or `"passthrough"` → existing `/Items/{id}/Download` path used.

*First-run seeding*
- On daemon startup → `transcoding::ensure_profiles_file_exists()` seeds `device-profiles.json` to app data dir from embedded asset before RPC server starts.

**Technical Notes (implementation complete — per tech spec, all tasks [x]):**
- `transcoding.rs`: `DeviceProfileEntry` type, `load_profiles()`, `ensure_profiles_file_exists()`
- `device-profiles.json` embedded via `include_bytes!` in `main.rs`
- `DeviceManifest.transcoding_profile_id: Option<String>` (`device/mod.rs`)
- SQLite `transcoding_profile_id TEXT` column + migration in `db.rs`
- `device_profiles.list` + `device.set_transcoding_profile` handlers in `rpc.rs`
- `get_playback_info_stream_url()` + `resolve_stream_url()` in `api.rs`
- `execute_sync()` extended with `transcoding_profile: Option<serde_json::Value>` param
- Both callers (`rpc.rs` `sync.start`, `main.rs` `run_auto_sync`) load and pass profile

**Status:** Implementation complete. Story added to close planning gap between tech spec and epic record.

---

### 4.3 PRD Updates

**Section: MVP Feature Set — Growth Features (Post-MVP)**

```
OLD (under Growth Features / Post-MVP):
- Transcoding Handshake: Dynamic server-side re-encoding via Jellyfin API
  for storage optimization.

NEW (moved into MVP Feature Set):
- Transcoding Handshake: Per-device profile selection for server-side
  re-encoding via Jellyfin PlaybackInfo API. Profiles stored in an editable
  device-profiles.json in the app data directory; passthrough (direct
  download) is the default.
```

**New Functional Requirements:**

```
FR31: The system can negotiate a transcoded stream URL from the Jellyfin server
  using a device-specific DeviceProfile payload, falling back to direct download
  when direct play is supported or transcoding fails.

FR32: The system can list available device transcoding profiles and assign one
  to a connected device, persisting the selection in both the device manifest
  and the local database.

FR33: When multiple managed devices are connected simultaneously, the system
  presents a device selection UI and allows the user to switch the active
  device context without restarting or reconnecting.
```

**FR Coverage Map additions:**
```
FR31: Epic 4 — Transcoding Handshake (Story 4.8)
FR32: Epic 4 — Transcoding Profile RPC (Story 4.8)
FR33: Epic 2 — Multi-Device Selection (Story 2.7)
```

---

### 4.4 Architecture Updates

**DeviceManager struct (replace single-slot with multi-device model):**

```
OLD:
  current_device: Arc<RwLock<Option<DeviceManifest>>>
  current_device_path: Arc<RwLock<Option<PathBuf>>>

NEW:
  connected_devices: Arc<RwLock<HashMap<PathBuf, DeviceManifest>>>
  selected_device_path: Arc<RwLock<Option<PathBuf>>>
  unrecognized_device_path: Arc<RwLock<Option<PathBuf>>>   (unchanged)
```

**New RPCs (Multi-Device):**

```
device.list
  → Vec<{ path: string, deviceId: string, name: string | null }>
  Returns all currently connected managed devices.

device.select
  params: { path: string }
  → { ok: true }
  Sets the active device context. All downstream operations (basket, sync,
  storage projection, manifest) use the selected device.
```

**get_daemon_state response additions:**

```
OLD: { currentDevice, deviceMapping, serverConnected, dirtyManifest,
       pendingDevicePath, autoSyncOnConnect, autoFill }

NEW: { currentDevice, deviceMapping, serverConnected, dirtyManifest,
       pendingDevicePath, autoSyncOnConnect, autoFill,
       connectedDevices: Array<{path, deviceId, name}>,   [NEW]
       selectedDevicePath: string | null }                [NEW]
```

**Transcoding RPCs:**

```
device_profiles.list
  → Array<{ id, name, description, deviceProfile: object | null }>
  Reads from device-profiles.json in app data dir.

device.set_transcoding_profile
  params: { deviceId: string, profileId: string }
  → { ok: true }
  Persists to device manifest (Write-Temp-Rename) and SQLite devices table.
```

**execute_sync() signature:**

```
OLD: execute_sync(client, device_path, manifest, item_ids, ...)
NEW: execute_sync(client, device_path, manifest, item_ids, ...,
                  transcoding_profile: Option<serde_json::Value>)
```

**Data Architecture — Device Profile Fields (update):**

```
Added to DeviceManifest (.jellyfinsync.json):
  transcoding_profile_id: Option<String>   // references id in device-profiles.json

Added to SQLite devices table:
  transcoding_profile_id TEXT              // mirrors manifest for quick lookup

device-profiles.json:
  Seeded to {app_data_dir}/device-profiles.json on first daemon startup.
  Embedded as binary asset via include_bytes! — user-editable post-install.
  passthrough profile (deviceProfile: null) resets a device to direct download.
```

---

### 4.5 UX Spec Update

**Section 5.4 — Device Profile Settings: add Multi-Device Picker**

```
Multi-Device Picker (rendered only when connectedDevices.length > 1):
- Positioned above the Device State panel (above the Managed Zone shield).
- Component: <sl-select> or compact device card list.
- Each option: device name (from manifest), truncated device path.
- Selected device highlighted. Switching calls device.select RPC and reloads
  basket + storage projection for the newly selected device.
- Single device connected: picker hidden; layout and behaviour unchanged.
```

---

## 5. Implementation Handoff

### Change Scope: Minor

All changes are within existing epics or are documentation-only.

| Recipient | Responsibility |
|-----------|---------------|
| **Dev** | Implement Story 2.7: DeviceManager HashMap refactor, `device.list` + `device.select` RPCs, `get_daemon_state` schema additions, UI device picker component |
| **Dev** | Story 4.8: already complete — mark as done when epics.md is updated |
| **Architect / SM** | Update `epics.md`: add Stories 2.7 and 4.8 |
| **Architect / SM** | Update `prd.md`: promote transcoding to MVP, add FR31–FR33, update coverage map |
| **Architect** | Update `architecture.md`: DeviceManager struct, new RPCs, get_daemon_state schema, transcoding additions |
| **SM** | Update `ux-design-specification.md`: §5.4 Multi-Device Picker component |

### Success Criteria

- [ ] Story 2.7: second device no longer silently overwrites first; UI picker appears with 2+ devices; `device.select` switches all operations to chosen device
- [ ] Story 2.7: single-device behaviour 100% unchanged (no regression)
- [ ] Story 4.8: marked complete in epics.md; transcoding handshake tested against Jellyfin with rockbox-mp3-320 profile
- [ ] PRD: transcoding listed in MVP section; FR31–FR33 present with coverage map entries
- [ ] Architecture: DeviceManager, new RPCs, transcoding additions documented
- [ ] UX Spec: §5.4 Multi-Device Picker section present
