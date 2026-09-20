---
stepsCompleted: ['step-01-validate-prerequisites', 'step-02-design-epics', 'step-03-create-stories', 'step-04-final-validation']
inputDocuments: ['prd.md', 'architecture.md', 'ux-design-specification.md', 'product-brief-bmad-2026-01-26.md', 'project-context.md']
status: 'complete'
completedAt: '2026-01-27'
lastAmended: '2026-09-19'
amendments: ['epic-11-selection-as-playlist', 'story-9-7-virtualized-list-view', 'multi-server-management', 'server-identity-name-and-icon', 'desktop-playback-release-radio-split']

playbackExtension:
  stepsCompleted: ['step-01-validate-prerequisites', 'step-02-design-epics', 'step-03-create-stories']
  status: 'stories-approved-coverage-reviewed-implementation-gates-open'
  approvedStories: ['15.1', '15.2', '15.3', '15.4', '15.5', '15.6', '15.7', '15.8', '15.9', '15.10', '15.11', '15.12', '15.13', '15.14', '15.15', '15.16', '15.17', '16.1', '16.2', '16.3', '16.4', '16.5', '16.6', '16.7', '16.8', '16.9', '16.10', '16.11', '16.12', '16.13', '16.14']
  updated: '2026-09-19'
  inputDocuments: ['prd.md', 'architecture.md', 'ux-design-specification.md', 'playback-prd-source-extract.md']
---

# HifiMule - Epic Breakdown

## Overview

This document provides the complete epic and story breakdown for HifiMule, decomposing the requirements from the PRD, UX Design, and Architecture requirements into implementable stories.

## Requirements Inventory

### Functional Requirements

FR1: Automatically detect Mass Storage devices (USB) on Windows, Linux, and macOS.
FR2: Manually select a target device folder if automatic detection fails.
FR3: Identify the presence of a `.hifimule.json` manifest on discovery.
FR4: Read persistent hardware identifiers to link devices across different sessions.
FR5: Configure Jellyfin server credentials (URL, username, token).
FR6: Select a specific Jellyfin user profile for syncing.
FR7: Maintain a persistent, encrypted connection state to the Jellyfin server.
FR8: Browse server-supported music navigation modes within the UI: Playlists, Artists, Albums, Genres, Recently Added, Frequently Played, Recently Played, and Favorites.
FR9: Select specific playlists or entities for synchronization.
FR10: Report real-time storage availability on the target device.
FR11: View a preview of "Proposed Changes" (files to add, remove, or update) before starting a sync.
FR12: Perform a differential sync based on the local manifest.
FR13: Protect unmanaged user files from deletion or modification.
FR14: Stream media files directly from the Jellyfin server to the device via memory-to-disk buffering.
FR15: Validate hardware-specific constraints (path length, character sets) before writing files.
FR16: Resume an interrupted sync session without restarting from scratch.
FR17: Detect Rockbox `.scrobbler.log` files on connected devices.
FR18: Report completed track plays to the Jellyfin server via the Progressive Sync API.
FR19: Track which scrobbles have already been submitted to prevent duplication.
FR20: Run as a background service (headless) with minimal resource usage.
FR21: Toggle "Launch on Startup" behavior.
FR22: Provide tray-icon status updates for sync progress and hardware state.
FR23: Send OS-native notifications for sync completion or errors.
FR24: Provide visual feedback (splash screen) during application startup and connection validation.
FR37: The system can persist the current device selection as a media-server playlist — creating a new playlist or updating an existing one. The system reads the current server playlist state before editing (read-fresh) and writes the resulting track set back (write-back). Basket entities are resolved to a concrete ordered track list at save time. The Auto-Fill virtual slot is excluded; when present, the user is notified. Supported on Jellyfin and Subsonic/OpenSubsonic, gated by `supports_playlist_write`.
FR38: The system provides a dual-panel playlist curation view: artists in the playlist on the left, that artist's albums on the right, filtered to playlist contents. Users can remove an artist or specific albums. A right-click context menu lets users send artists/albums to a playlist from browse views. The view displays playlist statistics (track count, total duration, total storage size). Edits update the server playlist.
FR39: The system can present Artist and Album browse pages as virtualized list/table views (in addition to paginated album-art grids), enabling rapid scanning across thousands of items without pagination.

### NonFunctional Requirements

NFR1: Memory footprint < 10MB during idle states.
NFR2: Complete manifest audit and be "ready to sync" in < 5 seconds.
NFR3: Sync throughput limited only by target hardware or network bandwidth.
NFR4: Utilize OS-level file sync primitives (sync_all) for data integrity.
NFR5: Atomic `.hifimule.json` manifest updates.
NFR6: Network interruption handling with at least 3 retry cycles.
NFR7: Graceful "Interrupted" session marking and repair utility trigger on mid-sync disconnect.
NFR8: 100% feature parity between Windows, Linux, and macOS.
NFR9: macOS sandbox compliance (no root/sudo required).
NFR10: Resource usage within 15% delta across OS environments.
NFR11: Encrypted credential storage using hardware-bound encryption (machine-uid + blake3 + ChaCha20-Poly1305). Secrets are stored as `secrets.enc` in the app data directory, bound to the host machine's hardware fingerprint.
NFR12: Pure local media synchronization (zero third-party data transmission).
NFR13: CLI-first architecture for the core sync engine.

### Additional Requirements

- **Multi-process Architecture:** Rust Daemon + Tauri v2 UI (Detachable).
- **IPC Mechanism:** JSON-RPC 2.0 over localhost HTTP.
- **Data Persistence:** SQLite (`rusqlite`) for daemon state and scrobble history.
- **Project Structure:** Rust Cargo Workspace containing `hifimule-daemon` and `hifimule-ui`.
- **UI Framework:** Shoelace Web Components for performance and consistency.
- **Design Layout:** "Basket Centric" (70/30 split view).
- **Managed Safety:** Visual "Managed Zone" shield to isolate personal data.
- **Accessibility:** WCAG 2.1 Level AA compliance.
- **Responsive strategy:** "Detachable Sidebar" for compact monitoring.


### FR Coverage Map

FR1: Epic 2 - Hardware Autodetection
FR2: Epic 2 - Manual Folder Fallback
FR3: Epic 2 - Manifest Presence Check
FR4: Epic 2 - Persistent Hardware ID
FR5: Epic 2 - Server Credential Entry
FR6: Epic 2 - User Profile Select
FR7: Epic 2 - Persistent Server Token (Encrypted Vault)
FR8: Epic 3 - Jellyfin Library Browser; Epic 9 - Rich Library Navigation
FR9: Epic 3 - Entity Selection Logic
FR10: Epic 3 - Real-time Disk Projection
FR11: Epic 3 - Staging Basket (Live Diff)
FR12: Epic 4 - Differential Sync Algorithm
FR13: Epic 3 - Managed Zone Isolation UI
FR14: Epic 4 - Buffered IO Streaming
FR15: Epic 4 - Legacy Hardware Path Validation
FR16: Epic 4 - Self-Healing Core (Core Re-sync/Resume)
FR17: Epic 5 - Rockbox Scrobbler Log Detection
FR18: Epic 5 - Progressive Sync API Submission
FR19: Epic 5 - Scrobble Submission Tracking
FR20: Epic 1 - Headless Background Daemon
FR21: Epic 1 - Toggle Launch on Startup
FR22: Epic 1 - System Tray Lifecycle Hub
FR23: Epic 5 - OS-Native Sync Notifications
FR24: Epic 2 - Startup Splash Screen with Connection Status
FR25: Epic 3 - Music-Only Library Filtering (Story 3.5)
FR27: Epic 6 - Platform-Native Installer Bundling
FR28: Epic 6 - CI/CD Cross-Platform Build Pipeline
FR29: Epic 3 - Auto-Fill Virtual Slot (Story 3.8); generalized to configurable pipeline + multi-server in Epic 12
FR30: Epic 2 - Auto-Sync on Known Device Detection
FR31: Epic 4 - Transcoding Handshake (Story 4.8)
FR32: Epic 4 - Transcoding Profile RPC (Story 4.8)
FR26: Epic 2 - Device Identity (Story 2.9)
FR33: Epic 2 - Enhanced Multi-Device Hub (Story 2.8)
FR34: Epic 3 - Artist Entity Basket Item (Story 3.9)
FR35: Epic 8 - Multi-Provider Server Support (Stories 8.1–8.6)
FR36: Epic 10 - Device Configuration Editing (Stories 10.1-10.2)
FR37: Epic 11 - Selection-as-Playlist (Stories 11.1-11.5)
FR38: Epic 11 - Dual-Panel Curation (Story 11.6)
FR39: Epic 9 - Virtualized List/Table Browse View (Story 9.7)
FR40: Epic 11 - Playlist Track Reordering (Stories 11.9-11.10)
FR41: Epic 9 - Tracks Browse Mode (Stories 9.9-9.10)
FR42: Epic 2 - Multi-Server Hub (Story 2.11)
FR43: Epic 3 - Mixed-Server Basket (Story 3.2, amended)
FR44: Epic 11 - Playlist Server Scope (Stories 11.4, 11.5, amended)
FR45: Epic 2 - Server Identity Name and Icon (Story 2.12)
FR46: Epic 2 - Portable Server Identity (Story 2.13)
FR47: Epic 9 - List View Multi-Selection & Bulk Actions (Story 9.11)
FR48: Epic 9 - Track Multi-Selection & Bulk Actions (Story 9.12)
FR49: Epic 12 - Configurable Auto-Fill Pipeline (Stories 12.1–12.7)
FR50: Epic 12 - Source × Strategy Separation / Playlist & Tag Sources (Stories 12.1, 12.4)
FR51: Epic 12 - Per-Server Auto-Fill / Multi-Slot (Stories 12.2, 12.3, 12.6)
FR52: Epic 12 - Auto-Fill Budget System (Story 12.5)
FR53: Epic 13 - Auto-Fill Memory / Rotation Strategies (Story 13.1)
FR54: Epic 13 - Auto-Fill Quality / Discovery / Delight (Stories 13.2–13.6)

## Epic List


## Epic 1: Foundation & Project Genesis

Establish the robust, multi-process Rust workspace and cross-platform Tray hub.

### Story 1.1: Multi-Process Workspace Initialization

As a System Admin (Alexis),
I want a Rust Cargo workspace containing separate crates for the daemon and the UI,
So that the sync engine can operate under the 10MB memory goal independent of the UI runtime.

**Acceptance Criteria:**

**Given** a clean project directory
**When** I run `cargo build`
**Then** the workspace successfully compiles both `hifimule-daemon` and `hifimule-ui` (Tauri).
**And** `hifimule-daemon` starts as a standalone headless binary.

### Story 1.2: Cross-Platform System Tray Hub

As a Convenience Seeker (Sarah),
I want a persistent system tray icon with status indicators,
So that I can monitor the sync engine's health (Idle/Syncing/Error) without opening the main window.

**Acceptance Criteria:**

**Given** the `hifimule-daemon` is running
**When** I check the system taskbar/menu bar
**Then** I see the HifiMule icon.
**And** the icon provides a "Quit" and "Open UI" menu option.

### Story 1.3: Detachable Tauri UI Skeleton

As a Ritualist (Arthur),
I want a detachable window that can be opened and closed from the tray without killing the sync engine,
So that I can browse my library while the background sync remains active.

**Acceptance Criteria:**

**Given** the daemon is active in the tray
**When** I click "Open UI"
**Then** a Tauri window appears using the "Vibrant Hub" Shoelace foundation.
**When** I close the window
**Then** the daemon remains running in the tray.


## Epic 2: Connection & Verification (The Handshake)

Implement secure Jellyfin authentication and automated hardware identification.

### Story 2.1: Secure Media Server Link

As a System Admin (Alexis),
I want to securely add media server credentials to the encrypted local vault and manage them per server,
So that I don't have to re-enter them and my credentials are safe from other users even when multiple servers are configured.

**Acceptance Criteria:**

**Given** the UI is open in "Settings → Servers"
**When** I enter a server URL, username, and password and click "Add Server"
**Then** the daemon auto-detects the server type (Story 8.4 factory: Subsonic ping → Jellyfin `/System/Info` fallback).
**And** for Jellyfin: authenticates and stores the access token in the vault keyed by the new server's UUID.
**And** for Subsonic/Navidrome: stores the password in the vault keyed by the new server's UUID for per-request MD5 signing.
**And** the connection is validated by a successful ping/library query.
**And** the new server appears in the Server Hub with its detected type and username.
**And** if a server with the same URL already exists, its credentials are updated (upsert by URL — no duplicate entries).
**And** the newly added server becomes the selected server if no server was previously selected.

**Technical Notes:**
- `server.connect` RPC now returns `{ ok, serverId: string, serverType, serverVersion }`.
- Vault key format: server UUID (not URL) so credentials survive URL edits (e.g., adding trailing slash or switching http→https).
- Vault stores `HashMap<String, ServerCredentials>` keyed by server UUID; old single-blob format auto-migrated on first startup after upgrade (see Story 2.11).

### Story 2.2: Mass Storage Heartbeat (Autodetection)

As a Ritualist (Arthur),
I want the daemon to "WAKE UP" the moment I plug in my iPod,
So that I don't have to manually hunt for folder paths.

**Acceptance Criteria:**

**Given** the daemon is running in the tray
**When** a USB Mass Storage device is connected
**Then** the daemon triggers a "Device Detected" event.
**And** it checks for the presence of a `.hifimule.json` manifest in the root directory.

**Note:** This story covers MSC (Mass Storage Class) devices only — USB devices that mount as drive letters. MTP device detection is covered by Story 2.10.

### Story 2.3: Multi-Device Profile Mapping & Auto-Sync Trigger

As a Convenience Seeker (Sarah),
I want the tool to remember that my Garmin watch belongs to my "Running" Jellyfin profile and automatically start syncing,
So that I can plug in and walk away without any interaction.

**Acceptance Criteria:**

**Given** a known device (has `.hifimule.json` with a unique ID) is connected
**When** the daemon reads the ID
**Then** it automatically loads the associated Jellyfin User Profile and Sync Rules.

**Given** a known device with `auto_sync_on_connect` enabled in its profile
**When** the device is detected and profile is loaded
**Then** the daemon automatically initiates a sync operation (using auto-fill selection or the last basket configuration).
**And** the tray icon transitions to "Syncing" state.
**And** no UI interaction is required.

**When** auto-sync completes
**Then** an OS-native notification is sent: "Sync Complete. Safe to eject."
**And** the tray icon returns to "Idle" state.

### Story 2.4: Startup Splash Screen with Connection Status

As a Convenience Seeker (Sarah),
I want to see a splash screen while the app is starting and connecting to my server,
So that I know the application hasn't frozen during its initialization phase.

**Acceptance Criteria:**

**Given** the `hifimule-ui` is launched
**When** the application is initializing (loading daemon, checking connection)
**Then** a native Tauri splash screen featuring the HifiMule logo and name is displayed.
**And** it clearly indicates the current state via status text (e.g., "Initializing Daemon...", "Connecting to Server...").
**When** the daemon is ready and connection is verified
**Then** the splash screen auto-dismisses and the main window appears.
**When** a connection timeout (10 seconds) or initialization error occurs
**Then** the splash screen displays a clear error message with a "Retry" or "Open Settings" option.


### Story 2.5: Interactive Login & Identity Management

As a Ritualist (Arthur),
I want a clear, guided login screen where I can select my server and enter my credentials,
So that I can easily connect to my library without manually copying API tokens.

**Acceptance Criteria:**

**Given** the application is unconfigured or a connection error occurs
**When** the Login View is displayed
**Then** I can enter a server URL.
**And** the UI shows a live type badge (detecting: "Jellyfin" / "Navidrome / Subsonic" / "Unknown") as I type or after I confirm the URL.
**And** I can enter my Username and Password.
**When** I click "Connect"
**Then** the daemon calls the factory (Story 8.4) to auto-detect server type and authenticate.
**And** for Jellyfin: retrieves and stores an access token via `POST /Users/AuthenticateByName`.
**And** for Subsonic: verifies credentials via `GET /rest/ping.view` (no token — stateless auth).
**And** the token or password is securely stored in the encrypted local vault keyed by the new server's UUID.
**And** the UI transitions to the main Library Browser on success with that server selected.
**When** authentication fails
**Then** a clear error message is shown (e.g., "Invalid Credentials", "Server Unreachable", or "Unknown server type at this URL").

**Given** the application already has configured servers
**When** I open Settings → Servers → "Add Server"
**Then** the same connection form is presented inline (not a full-screen takeover).
**And** on success, the new server is appended to the hub without disrupting the currently selected server or basket.

**Given** a previously configured server has an expired or invalid token
**When** the user selects that server in the Server Hub and the library browse RPC returns a 401
**Then** the UI surfaces a re-authentication prompt for that server specifically (not a full-screen login).
**And** re-authentication replaces only that server's credential in the vault.
**And** the other servers and their basket items are unaffected.

**Technical Notes:**
- The Login screen shows a subtle server type badge that updates live (debounced) as the URL is typed
- Auto-detection is primary; a manual server type override is available as an advanced option
- No separate "server type" dropdown in the primary flow
- Full-screen login is shown only on first run (no servers configured); post-first-run server addition is an inline form in the Server Hub settings panel
- Re-auth prompt: inline modal bound to the specific server's URL, not a global state reset

### Story 2.6: Initialize New Device Manifest

As a Ritualist (Arthur) and Convenience Seeker (Sarah),
I want the application to detect when a connected removable disk has no `.hifimule.json` manifest and guide me through initializing it,
So that I can bring a brand-new device into the managed sync model without manually creating any files.

**Acceptance Criteria:**

**Given** a USB mass storage device is connected with no `.hifimule.json` present in its root
**When** the daemon completes its device discovery scan
**Then** it broadcasts an `on_device_unrecognized` event to the UI.
**And** the UI displays an "Initialize Device" banner in the Device State panel.

**Given** the "Initialize Device" banner is visible
**When** I click "Initialize"
**Then** a dialog prompts me to confirm or change the target sync folder path on the device (defaulting to the device root).
**And** I can select the associated Jellyfin user profile for this device.
**When** I click "Confirm"
**Then** the UI sends a `device.initialize` JSON-RPC request to the daemon with the chosen folder path and profile ID.
**And** the daemon writes an initial `.hifimule.json` to the device via `device_io.write_with_verify()`, containing a new unique hardware ID and the selected profile.
**And** the daemon broadcasts an updated device state marking the device as "Managed".
**And** the UI transitions to the normal sync-ready state.

**When** the initialization fails (e.g., device is read-only or disk full)
**Then** the UI displays a clear error message with a "Retry" or "Dismiss" option.

### Story 2.7: Multi-Device Selection Panel

As a System Admin (Alexis) and Ritualist (Arthur),
I want to see all currently connected managed devices and select which one I am working with,
So that I can operate on one specific device without the daemon silently overwriting my context when a second device is plugged in.

**Acceptance Criteria:**

**Given** two or more managed devices are connected simultaneously
**When** I open the main UI (or when a second device is detected while the UI is open)
**Then** the UI displays a device picker listing all connected managed devices (device name from manifest, device_id, path).
**And** the currently selected device is highlighted.
**And** all operations (basket, storage projection, sync, manifest) target the selected device.

**Given** the device picker is visible
**When** I click a different device
**Then** the UI switches context to that device (reloads basket from its manifest, updates storage projection).
**And** the daemon's active device updates via the `device.select` RPC.

**Given** only one managed device is connected
**Then** no picker is shown and behaviour is identical to the current single-device experience (device is auto-selected).

**Given** the currently selected device is disconnected
**When** the daemon fires a device-removed event
**Then** the UI clears device context with no crash or stale state.
**And** if other devices remain connected, the picker is shown for the remaining devices.

**Technical Notes:**
- Daemon: `DeviceManager` gains `connected_devices: HashMap<PathBuf, DeviceManifest>` and `selected_device_path: Option<PathBuf>`. `handle_device_detected` adds to map; `handle_device_removed` removes from map and clears selection if needed.
- `get_current_device()` returns manifest for the `selected_device_path` entry — all existing callers remain unchanged.
- New RPC `device.list` → `Vec<{path, deviceId, name}>` for all connected devices.
- New RPC `device.select(params: {path: string})` → sets `selected_device_path`; silently sets for single-device case.
- `get_daemon_state` gains `connectedDevices: Array<{path, deviceId, name}>` and `selectedDevicePath: string | null`.
- UI: `<sl-select>` or device card list in the Device State panel header, rendered only when `connectedDevices.length > 1`.

**Status:** Picker behavior superseded by Story 2.8; device identity (name/icon) extended by Story 2.9.

### Story 2.8: Enhanced Multi-Device Hub

As a System Admin (Alexis) and Ritualist (Arthur),
I want a persistent device hub I can always interact with — switching between connected devices or deselecting one entirely,
So that I have full, iTunes-style control over which device I'm working with at all times.

**Acceptance Criteria:**

**Given** the main UI is open and 1 or more devices are connected
**Then** the device hub is always displayed (not hidden for single device).
**And** each device shows its name (or device_id fallback) and its icon.

**Given** no device is selected (selectedDevicePath === null)
**Then** the basket shows a placeholder: "Select a device to start curating".
**And** all (+) add buttons in the library browser are disabled.
**And** the "Start Sync" button is disabled.

**Given** I click a device in the hub
**Then** the UI calls `device.select` RPC and loads that device's basket.

**Given** all devices are disconnected
**Then** the no-device-selected locked state is shown.

**Technical Notes:**
- `BasketSidebar.ts renderDevicePicker()`: remove `connectedDevices.length > 1` guard — render hub whenever `connectedDevices.length >= 1`.
- Add no-device check: if `selectedDevicePath === null`, render locked basket placeholder and emit `device-locked` CSS class on library container.
- Library browser add buttons: check shared device-selected state before executing add RPC.
- Daemon: no changes needed — `selectedDevicePath` already supports `null`.

### Story 2.9: Device Identity — Name & Icon

As a System Admin (Alexis),
I want to give each device a custom name and icon when I initialize it,
So that I can instantly recognize my devices in the hub without staring at raw IDs.

**Acceptance Criteria:**

**Given** the "Initialize Device" dialog is open
**Then** a "Device Name" text input is shown (required, max 40 chars, prefilled with volume label or "My Device").
**And** an icon picker is shown with a small library of device-type icons (e.g., iPod Classic, Generic DAP, SD Card, USB Drive, Watch, Phone).
**When** I click "Confirm"
**Then** the name and icon are written to the manifest.

**Given** a device with a name and icon is connected
**When** it appears in the device hub
**Then** its icon is displayed alongside its name.
**And** if no icon is set, a default "USB Drive" icon is shown.
**And** if no name is set, the device_id is shown (existing fallback, unchanged).

**Technical Notes:**
- `DeviceManifest`: add `icon: Option<String>` with `#[serde(default)]` — backward-compatible with existing manifests.
- `device.initialize` RPC params: add `name: String` and `icon: Option<String>`.
- `device/mod.rs initialize_device()`: accept and store name + icon into manifest.
- `device.list` + `get_daemon_state`: add `icon` field to each device entry.
- `InitDeviceModal.ts`: add `<sl-input>` for device name + icon picker grid (~6–8 SVG icons embedded in UI).
- `device_io.write_with_verify()` abstracts the write strategy per backend: Write-Temp-Rename for MSC, dirty-marker + overwrite for MTP.
- The `device.initialize` RPC handler receives the `Arc<dyn DeviceIO>` for the target device from `DeviceManager` — no direct `std::fs` calls in the handler.

### Story 2.10: MTP Device Detection (Cross-Platform)

As a Convenience Seeker (Sarah),
I want the daemon to detect my Garmin watch (or any MTP device) the moment I plug it in,
So that it appears in the device hub without requiring manual steps.

**Acceptance Criteria:**

**Given** the daemon is running on Windows
**When** an MTP device is connected
**Then** the daemon receives a `WM_DEVICECHANGE` event with `GUID_DEVINTERFACE_WPD`.
**And** it enumerates the device via `IPortableDeviceManager` to retrieve its device ID and friendly name.
**And** it checks for a `.hifimule.json` object in the device root storage.
**And** it fires a `on_device_detected` or `on_device_unrecognized` event (identical behavior to MSC Story 2.2).

**Given** the daemon is running on Linux
**When** an MTP device is connected
**Then** the daemon receives a `udev` USB event.
**And** `libmtp` enumerates the device and retrieves its serial/device ID.
**And** it checks for `.hifimule.json` and fires the appropriate event.

**Given** the daemon is running on macOS
**When** an MTP device is connected
**Then** the daemon receives an `IOKit` USB match notification.
**And** `libmtp` enumerates the device and fires the appropriate event.

**Given** an MTP device is detected (any platform)
**When** the daemon creates the device IO backend
**Then** it instantiates `MtpBackend` with the device handle.
**And** passes `Arc<dyn DeviceIO>` to all downstream device operations.

**Technical Notes:**
- Windows: `windows-rs` with WPD COM API (`IPortableDeviceManager`, `IPortableDevice`)
- Linux/macOS: `libmtp-rs` crate or direct FFI to `libmtp`
- `DeviceManager` gains `device_class: DeviceClass { Msc, Mtp }` per connected device entry
- MTP device `path` in `device.list` RPC: use synthetic identifier `mtp://<device_id>` — no real filesystem path exists
- Device enumeration runs in `tokio::task::spawn_blocking` (libmtp is synchronous)

### Story 2.11: Multi-Server Hub

As a System Admin (Alexis) and Ritualist (Arthur),
I want a persistent Server Hub where I can see all configured servers, switch the active one, and add or remove servers,
So that I have full control over which media library I'm curating from at any time.

**Acceptance Criteria:**

**Given** one or more servers are configured
**When** I open the main UI or Settings → Servers
**Then** the Server Hub is displayed listing all configured servers.
**And** each server shows its URL (or a user-friendly display name), detected type badge (Jellyfin / Subsonic), and username.
**And** the currently selected server is highlighted.

**Given** I click a server in the hub that is not currently selected
**When** the UI calls `server.select({ id })`
**Then** the daemon updates `selectedServerId` in `ServerManager` and persists the `selected` flag in `server_config`.
**And** the library browser reloads with the newly selected server's content.
**And** basket items from the previous server become read-only (locked visual state).
**And** basket items from the newly selected server become editable.

**Given** I click "Add Server" in the hub
**Then** the server connection form (Story 2.5) is presented inline.
**And** on success, the new server is appended to the hub without disrupting the currently selected server.

**Given** I click "Remove" on a server that is not currently selected
**When** I confirm removal in a confirmation dialog
**Then** `server.remove({ id })` is called.
**And** the server's credentials are deleted from the vault.
**And** the server is removed from the `server_config` table.
**And** basket items that originated from that server are removed from the basket with a notification: "X items from [server] were removed from your basket."

**Given** I click "Remove" on the currently selected server
**Then** the confirmation dialog warns that the active server will be deselected.
**And** on confirmation, removal proceeds and `selectedServerId` is set to the first remaining server (or null if none remain).
**And** if no servers remain, the UI enters the first-run state (full-screen login).

**Given** no server is selected (`selectedServerId === null`)
**Then** the library browser shows an empty state: "Select a server to browse your library."
**And** all (+) add buttons in the library browser are disabled.
**And** the "Start Sync" button is disabled.

**Technical Notes:**
- `server.list` → `Array<{ id, url, serverType, username, selected: boolean }>`
- `server.select({ id })` → `{ ok: true }` — updates `ServerManager.selected_server_id` + sets `selected = 1` in DB (clears `selected` on all other rows)
- `server.remove({ id })` → `{ ok: true }` — removes row from `server_config`, deletes vault entry for that server UUID, evicts provider from `ServerManager.providers` cache
- `get_daemon_state` gains `servers: Array<{ id, url, serverType, username, selected }>` and `selectedServerId: string | null`
- Provider initialization is lazy: `ServerManager` calls `providers::connect()` only when a server is first selected, not at daemon startup for all servers (preserves < 10MB idle RAM NFR)
- Vault migration: on first daemon startup after upgrade, if `secrets.enc` decrypts to the old single-`Secrets` struct format, it is auto-migrated to `HashMap<serverId, ServerCredentials>` using the existing server's UUID; the migrated vault is re-encrypted and saved
- `server_config` DB migration: recreate table to drop `CHECK (id = 1)` constraint; add `selected INTEGER NOT NULL DEFAULT 0` column; existing single row gets a generated UUID and `selected = 1`
- UI: Server Hub lives in a "Servers" tab in Settings AND as a compact selector in the main layout header (analogous to the device hub in the sidebar)
- Basket item locked rendering: CSS class `basket-item--locked` on items where `item.serverId !== state.selectedServerId`; (×) button hidden for locked items

### Story 2.12: Server Identity Name and Icon

As a System Admin (Alexis) and multi-server user,
I want each configured media server to have a custom display name and icon,
So that I can quickly distinguish servers in the hub, switcher, basket, and playlist flows without relying on provider type.

**Acceptance Criteria:**

**Given** I add a new server
**When** the connection succeeds
**Then** the server is created with a default display name derived from the server URL or detected provider name.
**And** the server receives a default icon based on detected provider type when available.

**Given** I open Settings -> Servers for an existing server
**When** I edit its display name or icon
**Then** `server.update({ id, name, icon })` persists the changes.
**And** the Server Hub, compact switcher, basket badges, and playlist notices update without reconnecting the server.

**Given** I choose a server icon
**Then** the picker offers provider icons plus generic music/audio icons such as music note, headphones, library, album, radio, audiobook/book, and generic server.
**And** unsupported or missing provider logos fall back to a generic server or music icon.

**Given** the basket contains items from multiple servers
**When** basket items render
**Then** each server badge uses the configured server icon and display name.
**And** provider type is shown only as secondary metadata or tooltip text when helpful.

**Given** a legacy server record has no name or icon
**When** it is loaded
**Then** the UI uses a stable fallback label: configured name -> URL host -> username + provider type -> provider type.
**And** no migration blocks startup.

**Technical Notes:**
- `server_config` adds nullable `name` and `icon` columns.
- `ServerRecord` gains `name: Option<String>` and `icon: Option<String>`.
- `server.connect` accepts optional `name` and `icon`, and still works when omitted.
- `server.list` and `get_daemon_state.servers` return `{ id, url, serverType, username, name, icon, selected }`.
- New `server.update({ id, name?: string, icon?: string | null }) -> { ok: true }`.
- UI should share one `formatServerIdentity(server)` helper so hub cards, switcher labels, basket badges, notices, and playlist dialogs do not drift.

### Story 2.13: Portable Server Identity

As a System Admin (Alexis) and multi-server user,
I want each media server to have a stable, machine-independent identity used in device
manifests and sync routing,
So that a device synced on one machine is recognized on another, and removing then
re-adding the same server does not trigger a needless full resync.

**Acceptance Criteria:**

**Given** a server is added or reconnected
**When** `server.connect` / upsert runs
**Then** the daemon derives a deterministic portable `server_id` and persists it in
  `server_config.server_id`, while the existing random `id` is retained as the machine-local id.
**And** the basis is `sha256("v1|" + serverType + "|" + canonicalBaseUrl + "|" + username)`,
  preferring `sha256("v1|" + serverType + "|rid:" + serverReportedId + "|" + username)` when a
  server-reported id is available (Jellyfin `System/Info.Id`); Subsonic/OpenSubsonic uses the
  canonical-URL basis.

**Given** the same logical server/user is configured on two different machines
**Then** both machines derive an identical `server_id`.

**Given** a server is removed and later re-added with the same type/URL/username
**Then** the re-derived `server_id` is identical to the previous one.
**And** existing manifest items tagged with that `server_id` are still recognized — no full resync.

**Given** the credentials vault and provider cache
**Then** they remain keyed by the machine-local `id` (credentials are machine-local).

**Given** sync runs with basket/manifest items tagged by portable `server_id`
**When** the daemon needs a provider for an item
**Then** it resolves `server_id -> local id` and uses the existing per-local-id provider cache.

**Given** a device manifest or UI basket holds items tagged with an old random `server_id`
  (a machine-local UUID written by Story 2.11) or the pre-2.11 composite `type|url|user`
**When** the server is loaded/connected
**Then** those tags are reconciled in place to the new deterministic `server_id`.
**And** reconciliation is idempotent and never blocks startup.

**Given** the canonical base URL changes but a server-reported id is available
**Then** `server_id` remains stable.
**And** if no server-reported id is available, a URL change may yield a new logical identity
  (documented fallback behavior).

**Technical Notes:**
- `server_config` adds `server_id TEXT` (deterministic) and `server_reported_id TEXT NULL`
  (captured at connect for stable re-derivation). Existing `id` becomes the documented
  machine-local id (vault key, provider-cache key, select/remove/update key).
- New daemon helper `derive_server_id(server_type, canonical_base_url, username, reported_id)`.
  Reuse `normalized_server_url()` for the canonical base URL.
- `ServerManager` gains `get_provider_by_server_id(server_id)` mapping portable -> local
  before delegating to the existing `get_provider(local_id)`.
- Device manifest `SyncedItem.server_id` / `BasketItem.server_id` and UI basket `serverId`
  store the portable `server_id`. Extend `reconcileServerIds()` to map random-UUID -> portable.
- A migration backfills `server_id` for existing rows by deriving from stored type/url/username.
- Contracts: `server.list` / `get_daemon_state.servers[]` return both `id` (local) and
  `serverId` (portable); `get_daemon_state` adds `selectedServerPortableId`; `server.connect`
  returns `serverId` (portable) and `localId`. `server.select/remove/update` keep using local `id`.
- Tests: derivation determinism, cross-machine equality, remove/re-add no-resync, manifest +
  basket reconciliation idempotency, schema migration/backfill.

**Prerequisites:** Story 2.11, Story 2.12.

## Epic 3: The Curation Hub (Basket & Library)

Develop the high-confidence Library Browser and Selection Basket with storage projection.

### Story 3.1: Immersive Media Browser (Multi-Server Integration)

As a Ritualist (Arthur),
I want to browse my media server library through familiar music views such as Artists, Albums, Playlists, Genres, Recently Added, Frequently Played, Recently Played, and Favorites,
So that HifiMule feels like a curation surface for my server rather than only an artist/album tree.

**Acceptance Criteria:**

**Given** a successful server link (Jellyfin or Subsonic/Navidrome)
**When** I open the main UI
**Then** I see the "Vibrant Hub" layout with paginated album art grids.
**And** items already on the device are marked with a "Synced" badge.
**And** the Library Browser exposes provider-supported browse modes in a stable navigation control.

**Given** a Subsonic/Navidrome server is connected
**When** the Library Browser loads
**Then** artists are fetched via `provider.list_artists()` and albums via `provider.get_artist()`.
**And** the browse hierarchy (Library → Artist → Album → Tracks) works identically regardless of server type.
**And** hierarchical modes preserve breadcrumb navigation, synced badges, artwork, pagination, and add-to-basket behavior.
**And** history and favorites modes display music-only results sorted by the matching server metadata.
**And** unsupported modes are hidden or marked unavailable based on provider capabilities.
**And** cover art is fetched via `provider.cover_art_url()` — for Subsonic, uses the song's `coverArt` field (not the song ID directly).

**Technical Notes:**
- UI never calls server APIs directly — all data goes through daemon RPC → `MediaProvider`
- Domain `Song` type carries `cover_art_id: Option<String>` — populated from Subsonic's `coverArt` field

### Story 3.2: The Live Selection Basket

As a Convenience Seeker (Sarah),
I want to click items and have them "collect" in a sidebar,
So that I can see exactly what I'm about to sync without committing yet.

**Acceptance Criteria:**

**Given** the Library Browser and a server is selected
**When** I click the `(+)` on an album, artist, playlist, or track
**Then** the item is added to the "Sync Basket" sidebar with its `serverId` set to the currently `selectedServerId`.
**And** the sidebar displays the "Intent Overlay" (e.g., `+12 Tracks`).

**Given** no device is selected
**When** I view the library browser
**Then** all `(+)` add buttons are disabled (greyed out, no click interaction).

**Given** the basket contains items from multiple servers
**When** the basket renders
**Then** items from the selected server render with normal `(+)` / `(×)` controls.
**And** items from non-selected servers render with a server name badge and no remove `(×)` control (read-only / locked state).
**And** a clear informational note is shown when mixed-server items are present: "Items from other servers are read-only until you switch back to that server."

**Given** I switch the selected server via the Server Hub
**When** the basket re-renders
**Then** previously-selected-server items become locked (read-only, no `(×)`).
**And** newly-selected-server items (if any) become editable.

**Given** the basket contains only items from non-selected servers
**Then** the "Start Sync" button remains enabled (sync can execute items from any server).
**And** the storage projection bar includes all items regardless of server.

**Given** sync starts with a mixed-server basket
**When** `sync.start` is called
**Then** the daemon groups `itemIds` by their `serverId`, retrieves the appropriate provider per group via `ServerManager.get_provider(serverId)`, and downloads each file from its correct server.

**Technical Notes:**
- `BasketItem` gains `serverId: string` — populated from `selectedServerId` at add time.
- `basketStore` persists `serverId` per item in the basket manifest section of `.hifimule.json`.
- `sync.start` params: `itemIds` becomes `Array<{ id: string, serverId: string }>`; daemon routes each group to `ServerManager.get_provider(serverId)`.
- `basket.add` / `basket.remove` RPCs gain `serverId` in params; daemon validates `serverId` exists in `server_config` before accepting.
- Read-only rendering: CSS class `basket-item--locked` on items where `item.serverId !== state.selectedServerId`; `(×)` button hidden.

### Story 3.3: High-Confidence Storage Projection

As a Ritualist (Arthur),
I want to know *exactly* how many megabytes my selection will take on my device,
So that I don't trigger a "Disk Full" error mid-sync.

**Acceptance Criteria:**

**Given** items in the Sync Basket
**When** the list changes
**Then** the sidebar calculates the literal byte-size (factoring in target file formats).
**And** displays a "Projected Capacity" bar (Green = Safe, Red = Over Limit).

### Story 3.4: "Managed Zone" Hardware Shielding

As a Ritualist (Arthur),
I want a clear visual indication that my personal folders are protected,
So that I don't accidentally mark them for deletion.

**Acceptance Criteria:**

**Given** a connected device with unmanaged folders (e.g., `Notes/`)
**When** I view the "Device State" in the UI
**Then** unmanaged folders are shown as "Locked/Shielded" and cannot be modified by the tool.


### Story 3.5: Music-Only Library Filtering

As a Ritualist (Arthur),
I want the application to automatically filter out non-music content (movies, series, books) from my Jellyfin library,
So that I can focus purely on my music collection for my DAP.

**Acceptance Criteria:**

**Given** a Jellyfin library with mixed content types
**When** browsing the library in HifiMule
**Then** only MusicAlbums, Playlists, Artists, and MusicVideos (optional) are retrieved.
**And** Movies, Series, and Books are explicitly excluded from the UI views.

### Story 3.6: Auto-Fill Sync Mode (Synchronise All)

As a Convenience Seeker (Sarah),
I want the basket to automatically fill with music from my entire library prioritized by my favorites, most-played, and newest additions,
So that I can fill my device without manually browsing and selecting every album.

**Acceptance Criteria:**

**Given** the Basket sidebar is visible
**When** I enable the "Auto-Fill" toggle
**Then** the daemon queries the Jellyfin library and ranks all music tracks using the priority algorithm: favorites first, then by play count (descending), then by creation date (descending).
**And** the basket populates with tracks up to the device's available capacity or a user-defined size limit.
**And** the Storage Projection bar updates in real-time.

**Given** Auto-Fill is enabled and I have manually added artists/playlists to the basket
**When** the auto-fill algorithm runs
**Then** manual selections take priority and occupy space first.
**And** auto-fill uses the remaining capacity for algorithmically selected tracks.
**And** duplicates between manual and auto-fill selections are excluded.

**Given** Auto-Fill is active
**When** I adjust the optional "Max Fill Size" slider
**Then** the basket recalculates to respect the new limit.
**And** tracks beyond the limit are removed from the basket in reverse priority order.

**Given** Auto-Fill items are displayed in the basket
**When** I view the item list
**Then** auto-filled items show a distinct "Auto" badge to differentiate them from manually added items.
**And** each item shows its priority reason (e.g., "★ Favorite", "▶ 47 plays", "New").

**Technical Notes:**
- Priority algorithm runs daemon-side via Jellyfin API queries (IsFavorite, PlayCount, DateCreated)
- IPC: `basket.autoFill` JSON-RPC method with params: { deviceId, maxBytes?, excludeItemIds[] }
- Response streams items progressively as the daemon calculates
- Device profile stores auto-fill preferences: `auto_fill_enabled`, `max_fill_bytes`
- Post-MVP: allow scoping to specific libraries/collections

**Status:** Superseded by Story 3.8 — lazy virtual slot model replaces eager basket population.

### Story 3.7: Artist View — Cache, Scroll State & Quick Navigation

As a Ritualist (Arthur),
I want the Artist view to remember where I was scrolling and load instantly when I navigate back,
So that browsing a large music library feels snappy and I never lose my place when exploring albums.

**Acceptance Criteria:**

**Given** I have scrolled down the artist/album grid and clicked into a container item
**When** I press the breadcrumb to navigate back
**Then** the grid scrolls back to the exact position I was at before navigating in.
**And** my position is preserved for any level of the breadcrumb stack (library → artist → album).

**Given** I previously loaded a page of items under a parent
**When** I navigate back to that parent via breadcrumb
**Then** the grid renders from cache instantly (no spinner, no re-fetch).
**And** cached data is invalidated if I navigate away to a different branch of the library tree.

**Given** the current folder contains 20 or more items of type `MusicArtist`
**When** the grid renders
**Then** an alphabetical quick-nav bar is displayed (letters A–Z plus `#` for non-alpha).
**And** clicking a letter filters the grid to show only artists whose name starts with that letter (via server-side `NameStartsWith` / `NameLessThan` params); clicking the active letter again clears the filter and restores the full list.
**And** the quick-nav bar is NOT shown for views with fewer than 20 items or for non-artist views.

**Given** a device is disconnected or a different device is selected
**When** the library view re-initialises
**Then** all scroll state and page caches are cleared and the library reloads from scratch.

**Technical Notes:**
- UI-only change (`library.ts` + CSS); zero new RPC methods
- `AppState` extended with `scrollCache: Map<string, number>` and `pageCache: Map<string, { items, total }>`
- `clearNavigationCache()` exported and called on device-change events, matching `clearForDevice()` pattern
- Quick-nav uses server-side filter (not client-side scrollIntoView) — confirmed via user test post-implementation
- Scroll restore uses `requestAnimationFrame` after `renderGrid()` to ensure DOM is painted

### Story 3.8: Lazy Auto-Fill Virtual Slot

As a Convenience Seeker (Sarah),
I want to enable Auto-Fill with a single toggle and have the device fill with my best music at sync time,
So that I don't wait for a slow basket population and always get the freshest track selection when I actually sync.

**Acceptance Criteria:**

**Given** the basket sidebar is visible
**When** I enable the "Auto-Fill" toggle
**Then** a single "Auto-Fill Slot" card appears in the basket (not individual tracks).
**And** the card shows the configured capacity target (e.g. "Fill remaining 12.4 GB" or the user-set max).
**And** no Jellyfin API call is made at this point.

**Given** manual items and the Auto-Fill Slot are in the basket
**When** I view the basket
**Then** manual items appear as individual cards above the Auto-Fill Slot.
**And** the Auto-Fill Slot shows "Will fill ~X GB with top-priority tracks at sync time".
**And** storage projection includes the slot's target bytes in the capacity bar.

**Given** the basket contains the Auto-Fill Slot
**When** I click "Start Sync"
**Then** the daemon runs the priority algorithm (`run_auto_fill`) at the start of the sync job.
**And** expands the slot to real track IDs (favorites first, then play count, then newest).
**And** excludes any track IDs already covered by manual basket items.
**And** the expanded track list is merged with manual items for the sync operation.
**And** the UI shows real-time progress exactly as today (files completed, current filename).

**Given** Auto-Fill is enabled
**When** I toggle it off
**Then** the Auto-Fill Slot is removed from the basket immediately (no API call).

**Given** the basket sidebar is visible and a server is selected
**When** I enable the "Auto-Fill" toggle
**Then** the Auto-Fill Slot card is bound to the currently `selectedServerId`.
**And** if an Auto-Fill Slot already exists (from any server), it is replaced by the new slot.

**Given** an Auto-Fill Slot is in the basket from server A and I switch to server B
**When** the basket renders
**Then** the Auto-Fill Slot from server A is shown as read-only (locked, no toggle affordance) with a server name badge indicating it belongs to server A.
**And** the auto-fill toggle for server B is shown as OFF.

**Given** an Auto-Fill Slot is in the basket from server A and I switch to server B and enable Auto-Fill
**When** the toggle is turned ON
**Then** the server A slot is replaced by a new Auto-Fill Slot bound to server B (silent overwrite, no confirmation prompt).

**Technical Notes:**
- Remove from `BasketSidebar.ts`: `triggerAutoFill()`, `scheduleAutoFill()`, `autoFillInFlight`, `autoFillPendingRetrigger`, `autoFillDebounceTimer`, `isAutoFillLoading`, `basketStore.replaceAutoFilled()`
- Toggle inserts a single `{ id: '__auto_fill_slot__', type: 'AutoFillSlot', maxBytes: N, serverId: selectedServerId }` virtual item into `basketStore`
- Auto-fill toggle state: UI checks `item.serverId === selectedServerId` to determine whether to show the toggle as ON or OFF
- On toggle ON: dispatch removes any existing `__auto_fill_slot__` item first, then inserts new one with `serverId = selectedServerId`
- `basket.autoFill` RPC: retained as preview/debug endpoint, no longer called by UI for basket population
- `sync.start` RPC handler: if request contains `autoFill: { enabled: true, maxBytes?, excludeItemIds[], serverId }`, call `run_auto_fill()` routing to `ServerManager.get_provider(serverId)` — mirrors existing daemon-initiated path (`main.rs:503`)
- `sync.start` params gain: `autoFill?: { enabled: boolean, maxBytes?: number, excludeItemIds: string[], serverId: string }`
- Auto-fill preferences (enabled, maxBytes) continue to be persisted via `sync.setAutoFill`
- Multi-slot auto-fill (one per server simultaneously) is deferred to a future change

### Story 3.9: Artist Entity Basket Item

As a Ritualist (Arthur),
I want to add an artist to my basket as a single entity rather than a snapshot of their tracks,
So that any new albums or tracks added to that artist in Jellyfin are automatically included the next time I sync.

**Acceptance Criteria:**

**Given** I am browsing the Artist view
**When** I click (+) on an artist
**Then** a single "Artist" card appears in the basket (not individual track cards).
**And** the card shows: artist name, approximate track count, and estimated size (from artist entity metadata at add-time).
**And** no per-track child fetch is triggered at add-time.

**Given** an artist card is in the basket
**When** I view it
**Then** it shows "Artist · ~N tracks · ~X MB" (approximate).
**And** storage projection uses this estimate for the capacity bar.
**And** the card has the same remove (×) interaction as any other basket item.

**Given** the basket contains one or more artist cards
**When** sync starts
**Then** the daemon calls `get_child_items_with_sizes` for each artist ID to resolve current tracks (already occurs at `rpc.rs:831` for any container ID).
**And** newly added tracks from that artist (since the basket was built) are included in the sync.

**Given** artist cards and manually added albums/playlists are both in the basket
**Then** duplicate tracks are deduplicated by the daemon at sync time via the existing manifest comparison logic.

**When** I click (×) on an artist card
**Then** the card is removed immediately; no individual track cleanup needed.

**Technical Notes:**
- UI: on artist (+) click, store `{ id: artistId, type: 'MusicArtist', name, sizeBytes: artistTotalBytes, childCount }` in `basketStore` — use artist-level size from metadata, no child fetch
- Daemon `sync.start`: no change required — `rpc.rs:807–866` already expands `MusicArtist` container IDs via `get_child_items_with_sizes`
- `BasketItem.type: 'MusicArtist'` already valid; `sizeBytes` carries artist-level cumulative size
- Story 3.6's eager artist-track expansion at add-time is superseded by this story

## Epic 4: The Sync Engine & Self-Healing Core

Build the performant, atomic sync logic with built-in core resume capabilities.

### Story 4.0: Device IO Abstraction Layer

As a System Admin (Alexis),
I want all device file operations to go through a single abstract interface,
So that the sync engine works identically for both MSC and MTP devices without duplicated IO logic.

**Acceptance Criteria:**

**Given** the `DeviceIO` trait is defined in `hifimule-daemon`
**When** any sync, manifest, or scrobble operation targets a device
**Then** it calls methods on `Arc<dyn DeviceIO>` exclusively — no direct `std::fs` calls with a device path anywhere outside `MscBackend`.

**Given** a connected MSC device
**When** `DeviceManager` instantiates the backend
**Then** it creates `MscBackend { root: PathBuf }`.
**And** `MscBackend::write_with_verify()` uses the Write-Temp-Rename pattern + `sync_all()` (existing behavior, unchanged).

**Given** a connected MTP device
**When** `DeviceManager` instantiates the backend
**Then** it creates `MtpBackend { handle: Arc<MtpHandle> }`.
**And** `MtpBackend::write_file()` transfers data via WPD object creation (Windows) or `libmtp_send_file_from_memory` (Linux/macOS).
**And** `MtpBackend::write_with_verify()` writes a `".dirty"` marker object first, overwrites the target object, then deletes the marker.
**And** `MtpBackend::read_file()` retrieves object data by path lookup.
**And** `MtpBackend::list_files()` enumerates storage objects.
**And** `MtpBackend::delete_file()` removes an object by handle.
**And** `MtpBackend::free_space()` queries device storage capacity.

**Given** the daemon reconnects to a device with a `".dirty"` marker present
**When** device detection completes
**Then** the daemon fires `on_device_dirty` (same as the existing MSC dirty-manifest path).

**Given** all existing callers in `sync.rs`, `rpc.rs`, `device/mod.rs`, and `scrobble.rs`
**When** Story 4.0 is complete
**Then** every direct `std::fs` call targeting a device path has been replaced with the corresponding `DeviceIO` method.
**And** all existing unit tests pass without modification (MSC behavior is unchanged).

**Technical Notes:**
- `device_io.rs`: new file defining `DeviceIO` trait, `FileEntry` struct, `MscBackend`, `MtpBackend`
- Platform gating: `MtpBackend` compiled with `#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]`
- Windows MTP: `windows-rs` crate, `IPortableDevice`, `IPortableDeviceContent`, `IPortableDeviceDataStream`
- Linux/macOS MTP: `libmtp-rs` crate (wraps `libmtp` C library); build script links `libmtp` via `pkg-config`
- `DeviceManager` stores `Arc<dyn DeviceIO>` per connected device, keyed by device path/ID; passed to all downstream callers at construction time
- Existing `execute_sync()` signature gains `device_io: Arc<dyn DeviceIO>` param; both callers (`rpc.rs` `sync.start` and `main.rs` `run_auto_sync`) retrieve it from `DeviceManager`

### Story 4.1: Differential Sync Algorithm (Manifest Comparison)

As a System Admin (Alexis),
I want the engine to calculate exactly which files to add or delete by comparing the Jellyfin server state with the local `.hifimule.json` manifest,
So that only necessary changes are made to the disk, preserving the hardware's life.

**Acceptance Criteria:**

**Given** a Selection Basket with 50 items
**When** the sync engine starts
**Then** it generates a list of "Adds" and "Deletes" based on the `.hifimule.json` record.
**And** it detects if server IDs have changed for existing local files.

### Story 4.2: Atomic Buffered-IO Streaming

As a Convenience Seeker (Sarah),
I want files to be written directly from the Jellyfin server to the USB device using buffered memory,
So that the sync is fast and doesn't consume local temporary disk space.

**Acceptance Criteria:**

**Given** a list of files to sync
**When** a file write begins
**Then** the engine streams data directly into the device buffer.
**And** uses `sync_all` to ensure the directory entry is committed before moving to the next file.

### Story 4.3: Legacy Hardware Constraints (Path & Char Validation)

As a Ritualist (Arthur),
I want the engine to automatically shorten paths or rename files that exceed legacy hardware limits (e.g., FAT32 or Rockbox 255-char limits),
So that my sync never fails due to filesystem errors.

**Acceptance Criteria:**

**Given** a Jellyfin track with a 300-character name
**When** the engine prepares to write to the device
**Then** it automatically truncates or sanitizes the filename to fit hardware constraints.
**And** logs the original-to-sanitized mapping in the manifest.

### Story 4.4: Self-Healing "Dirty Manifest" Resume

As a System Admin (Alexis),
I want the system to detect an interrupted sync and offer to resume from the last successful file,
So that I don't lose progress after an accidental unplug.

**Acceptance Criteria:**

**Given** a sync was interrupted at 60%
**When** the device is reconnected
**Then** the engine detects the "Dirty" manifest flag.
**And** it identifies which files were only partially written and initiates a resume of the remaining delta.

### Story 4.5: "Start Sync" UI-to-Engine & Daemon-Initiated Trigger

As a Convenience Seeker (Sarah) and Ritualist (Arthur),
I want to click a "Start Sync" button in the Sync Basket sidebar or have the daemon automatically trigger sync on device connect,
So that I can either manually execute my selection or enjoy zero-touch automatic synchronization.

**Acceptance Criteria:**

**Given** the Sync Basket is populated with items and storage projection is within safe limits
**When** I click the "Start Sync" button
**Then** the UI sends a `sync.start` JSON-RPC request to the daemon, including the basket's item list (Jellyfin IDs) and target device path.
**And** the daemon responds immediately with `{ "status": "success", "data": { "jobId": "<uuid>" } }`.
**And** the "Start Sync" button transitions to a disabled "Syncing..." state with a Shoelace progress indicator.
**And** the UI subscribes to the `on_sync_progress` event stream and displays real-time progress (files completed, percentage, current filename).

**When** the sync completes successfully
**Then** the UI displays "Sync Complete" status.
**And** the Sync Basket clears and the button resets to its default enabled state.

**When** the daemon returns an error or the device disconnects mid-sync
**Then** the UI displays a clear error message.
**And** the daemon marks the manifest as "Dirty" (per Story 4.4 behaviour).
**And** the UI offers a "Retry" or "Dismiss" option.

**Technical Notes:**
- IPC pattern: JSON-RPC 2.0 · Request: `sync.start` · Response: `{ jobId }` · Events: `on_sync_progress`
- `sync.start` `itemIds` parameter format: `Array<{ id: string, serverId: string }>` — each item carries its originating server UUID so the daemon can route per-item downloads to the correct `MediaProvider`. The daemon groups items by `serverId` and calls `ServerManager.get_provider(serverId)` for each group.
- Follows the architecture's Request-Response-Event communication pattern
- Button must be disabled when: basket is empty, storage projection is Over Limit, or a sync is already in progress
- ARIA-live region required for progress updates (WCAG 2.1 AA)

**Given** a known device is connected with `auto_sync_on_connect` enabled
**When** the daemon detects the device and loads its profile
**Then** the daemon internally triggers `sync.start` using the device's auto-fill configuration (without a UI-initiated RPC call).
**And** the sync follows the same differential algorithm, buffered IO, and manifest update logic as a UI-triggered sync.
**And** if the UI is open, it reflects the in-progress sync state via `on_sync_progress` events.
**And** if the UI is closed, the tray icon and OS notifications provide progress and completion feedback.

### Story 4.6: Sync Progress — Time Remaining Estimation

As a Convenience Seeker (Sarah),
I want to see an estimated time remaining during sync,
So that I know whether to wait by the screen or step away.

**Acceptance Criteria:**

**Given** a sync is in progress
**When** at least 2 polling cycles have completed with non-zero `bytesTransferred`
**Then** a time-remaining estimate is displayed below the progress bar ("~N min left" / "~N sec left" / "Almost done…").
**And** before 2 samples are available, the label shows "Calculating…".

**Given** `bytesTransferred` and `totalBytes` are available
**When** calculating ETA
**Then** ETA = `bytes_remaining / avg_bytes_per_second`, where `avg_bytes_per_second` = `bytesTransferred / elapsed_seconds` (cumulative average since `startedAt`).
**And** format: ≥ 60s → "~N min left"; ≥ 10s → "~N sec left"; < 10s → "Almost done…".

**When** `status === 'complete'`
**Then** the ETA line is replaced by the existing "Sync Complete" panel.

**Technical Notes:**
- Daemon: `SyncOperation` gains `bytes_transferred: u64` (cumulative across completed + in-progress file) and `total_bytes: u64` (pre-computed sum of all file sizes at sync start)
- `total_bytes` written once at start of `execute_sync()`; `bytes_transferred` updated in the per-file progress callback
- ETA calculation and display are UI-side only (`BasketSidebar.ts`)
- Tray tooltip remains "HifiMule: Syncing…" — ETA is UI-only (known variance)

### Story 4.7: Playlist M3U File Generation

As a Ritualist (Arthur) and Convenience Seeker (Sarah),
I want `.m3u` playlist files to be written to my device when I sync a Jellyfin playlist,
So that my DAP or Rockbox player can natively load and play the playlist in the correct order.

**Acceptance Criteria:**

**Given** at least one basket item has `item_type = "Playlist"` and sync runs successfully
**When** sync completes
**Then** a `.m3u` file is written to `manifest.playlistPath` for each playlist.
**And** if `playlistPath` is absent, null, or empty for an older manifest, the daemon writes playlists to `manifest.managed_paths[0]`.
**And** the filename is sanitized via `sanitize_path_component()` and truncated to 255 characters if needed.

**Given** a playlist `.m3u` is being written
**When** generating its contents
**Then** the file begins with `#EXTM3U`.
**And** each track has an `#EXTINF:<seconds>,<Artist> - <Title>` line followed by its relative path (relative to the `.m3u` file location, forward slashes).
**And** duration is `RunTimeTicks ÷ 10,000,000`; absent ticks → `-1`.

**Given** the playlist folder differs from the music folder
**When** generating `.m3u` entries
**Then** each track path is relative from the playlist file location to the synced track file location, using forward slashes.

**Given** a playlist's track list is unchanged from the previous sync (same `trackCount` and `trackIds` hash in manifest)
**Then** the `.m3u` is not rewritten. If changed, it is regenerated atomically (Write-Temp-Rename).

**Given** a playlist was in the previous sync but is absent from the current basket
**Then** its `.m3u` file is deleted and its entry removed from `manifest.playlists`.

**Given** a playlist track's provider item ID is not found in `manifest.synced_items` (`providerItemId` in `.hifimule.json`; internal Rust field `jellyfin_id`)
**Then** that track is omitted from the `.m3u` with a log entry; remaining tracks are still written.

**Technical Notes:**
- `PlaylistTrackInfo` and `PlaylistSyncItem` structs added to `sync.rs`
- `SyncDelta` gains `playlists: Vec<PlaylistSyncItem>` — populated during `sync.start` container expansion
- Manifest extended with `playlists: Vec<PlaylistManifestEntry>` (`jellyfinId`, `filename`, `trackCount`, `trackIds`, `lastModified`)
- All writes use Write-Temp-Rename + `sync_all()` for atomicity
- `run_time_ticks: Option<u64>` added to `JellyfinItem` in `api.rs`

### Story 4.8: Transcoding Handshake via Device Profiles

As a Ritualist (Arthur) and Convenience Seeker (Sarah),
I want the sync engine to transcode music to a device-compatible format before writing it,
So that tracks play correctly on DAPs that don't support FLAC, Opus, or AAC (e.g., iPods running older firmware).

**Acceptance Criteria:**

**Given** `device-profiles.json` is installed in the app data dir (seeded on first run)
**When** I call the `device_profiles.list` RPC
**Then** I receive the list of available profiles (id, name, description) including: `passthrough`, `rockbox-mp3-320`, `generic-mp3-192`, `generic-aac-256`.

**Given** I call `device.set_transcoding_profile` with a profileId
**When** the daemon processes the request
**Then** the `transcoding_profile_id` is written to the device manifest AND persisted in the SQLite `devices` table.

**Given** a device manifest with `transcoding_profile_id` set to a non-passthrough profile
**When** `execute_sync` runs for a file
**Then** the engine calls `POST /Items/{id}/PlaybackInfo` with the `DeviceProfile` payload.
**And** if Jellyfin returns a `TranscodingUrl` → streams from `{base_url}{TranscodingUrl}`.
**And** if Jellyfin returns `SupportsDirectPlay: true` → falls back to `/Items/{id}/Download`.
**And** if the PlaybackInfo call fails → non-fatal; logged in `SyncFileError`; continues with next file.

**Given** a device manifest with `transcoding_profile_id` = null or `"passthrough"`
**When** `execute_sync` runs
**Then** the engine uses the existing `/Items/{id}/Download` path unchanged (Jellyfin) or `/rest/download.view` (Subsonic).

**Given** a Subsonic/OpenSubsonic device profile is selected
**When** `execute_sync` runs for a file
**Then** the engine calls `provider.download_url(track_id, profile)` which returns:
  - Passthrough: `/rest/download.view?id=...&[auth params]`
  - Transcoding: `/rest/stream.view?id=...&format=mp3&maxBitRate=192&[auth params]` where `maxBitRate` is in **kbps** (not bps)
**And** auth params in the URL are sanitized before logging (Story 8.5).

**Technical Notes (Subsonic transcoding):**
- Subsonic uses direct stream URL params — no PlaybackInfo API exists
- Bitrate unit: Jellyfin uses bps (`audioBitRate=192000`), Subsonic uses kbps (`maxBitRate=192`) — conversion inside `SubsonicProvider`
- `execute_sync()` calls `provider.download_url(id, profile)` — provider encapsulates which approach to use

**Given** no `device-profiles.json` in the app data dir
**When** the daemon starts
**Then** `transcoding::ensure_profiles_file_exists()` seeds the default file from the embedded asset before the RPC server starts.

**Technical Notes (implementation complete — per tech spec, all tasks [x]):**
- `transcoding.rs`: `DeviceProfileEntry` type, `load_profiles()`, `ensure_profiles_file_exists()`
- `device-profiles.json` embedded via `include_bytes!` in `main.rs`
- `DeviceManifest.transcoding_profile_id: Option<String>` in `device/mod.rs`
- SQLite `transcoding_profile_id TEXT` column + migration in `db.rs`
- `device_profiles.list` + `device.set_transcoding_profile` handlers in `rpc.rs`
- `get_playback_info_stream_url()` + `resolve_stream_url()` in `api.rs`
- `execute_sync()` extended with `transcoding_profile: Option<serde_json::Value>` param
- Both callers (`rpc.rs` `sync.start`, `main.rs` `run_auto_sync`) load and pass profile
- Reference: `_bmad-output/implementation-artifacts/tech-spec-transcoding-device-profiles-playback-handshake.md`

**Status:** Implementation complete. Story added to formally track completed work.

### Story 4.9: Provider-Neutral Transcoding, Compatibility, and Extension Verification

As a user syncing to a device that only supports MP3,
I want incompatible source formats to be truly transcoded before being written, and skipped when compatible transcoding is unavailable,
So that the device never receives unplayable media.

**Acceptance Criteria:**

**Given** the target device/profile requires MP3 and the source item is FLAC
**When** sync starts against a Subsonic/Navidrome provider
**Then** the daemon requests a provider stream URL with `format=mp3` and `maxBitRate` in kbps.

**Given** the target device/profile requires MP3 and the provider cannot transcode the track to MP3
**When** sync plans or executes that item
**Then** sync skips that track with a clear warning/result entry instead of copying the incompatible source file.
**And** the skipped track is not written to the device manifest.

**Given** transcoding is requested but cannot be negotiated or confirmed
**When** sync executes the item
**Then** sync skips that item with a clear warning/result entry instead of writing source bytes to a target-extension path.

**Given** the provider returns direct/passthrough content
**When** the source suffix/content type is compatible with the active device profile
**Then** the output filename uses the original source suffix, not the requested target extension.

**Given** the provider returns direct/passthrough content whose source suffix/content type is not compatible with the active device profile
**When** sync executes the item
**Then** sync skips the item and keeps it out of the manifest.

**Given** the active provider is Jellyfin
**When** a non-passthrough device profile is selected
**Then** existing PlaybackInfo transcoding behavior remains intact.

**Given** the active provider is Subsonic/OpenSubsonic
**When** sync needs a stream or download URL
**Then** no code outside `providers/subsonic.rs` constructs Subsonic stream URLs directly.

**Technical Notes:**
- Treat profile compatibility as a hard device constraint, not a best-effort preference.
- The safe fallback for unsupported transcoding is omission, not passthrough.
- Tests should cover FLAC-to-MP3 success, compatible direct-download fallback preserving source extension, skipped incompatible direct downloads, skipped tracks when required transcoding cannot be honored, and manifest exclusion for skipped items.

### Story 4.10: Idempotent Managed File Deletion on USB

As a user syncing a USB drive,
I want cleanup to tolerate files that are already missing,
So that stale manifest entries do not turn into noisy sync failures.

**Acceptance Criteria:**

**Given** a managed manifest entry points to a file that is already absent on an MSC device
**When** sync cleanup deletes it
**Then** the delete is treated as successful.
**And** the manifest entry is removed.

**Given** deletion fails because of permission, read-only media, or another real IO error
**When** sync cleanup deletes it
**Then** sync reports the error.
**And** the manifest entry is not silently dropped.

**Given** an MTP backend reports an item missing during delete
**When** the backend can distinguish missing-object errors from real IO errors
**Then** the sync layer treats the missing-object case equivalently to MSC not-found deletion.

**Technical Notes:**
- Missing managed files are expected after manual deletion or prior partial cleanup.
- Keep genuine delete failures visible so hardware or permission problems are not hidden.
- Tests should cover MSC missing-file deletion, a real deletion error, and sync cleanup removing stale manifest entries without failing the operation.

## Epic 5: Ecosystem Lifecycle & Advanced Tools

Complete the scrobble bridge and implement user-facing repair/completion notifications.

### Story 5.1: Rockbox Scrobbler Bridge

As a Ritualist (Arthur),
I want the daemon to automatically find and read the `.scrobbler.log` on my iPod,
So that my on-the-go listening is reflected on my Jellyfin server.

**Acceptance Criteria:**

**Given** a connected device with a Rockbox `.scrobbler.log`
**When** the device is detected
**Then** the engine parses the log file.
**And** it submits the play counts to Jellyfin using the `/PlaybackInfo/Progress` API.

**Given** the connected device is an MTP device
**When** the daemon scans for a `.scrobbler.log`
**Then** it uses `device_io.read_file(".scrobbler.log")` to retrieve the log contents.
**And** parsing and submission logic is identical to the MSC path.

**Given** a `.scrobbler.log` is found and the connected server is Subsonic/Navidrome
**When** the daemon processes scrobble submissions
**Then** for each completed track: calls `POST /rest/scrobble.view?id={id}&submission=true&time={epoch_ms}`.
**And** this is provider-specific: Navidrome only increments play count via explicit scrobble calls, NOT from streaming.

**Given** the connected server is Jellyfin
**When** scrobble submissions are processed
**Then** existing behavior is unchanged (Progressive Sync API / `/PlaybackInfo/Progress`).

**Technical Notes (multi-provider scrobble):**
- `ScrobbleSubmitter` becomes provider-aware: Jellyfin path unchanged; Subsonic path uses `SubsonicProvider`
- IDs for Subsonic scrobble must be server-side track IDs (from manifest's `server_id`-keyed record)
- Navidrome streaming does NOT increment play count — only explicit `scrobble?submission=true` does

### Story 5.2: Scrobble Submission Tracking (Deduplication)

As a System Admin (Alexis),
I want the engine to keep track of which log entries have already been submitted,
So that I don't get duplicate play entries on my server.

**Acceptance Criteria:**

**Given** 100 entries in the `.scrobbler.log`
**When** a submission is successful
**Then** the engine records the timestamp/ID in the local SQLite database.
**And** future scans skip these records.

### Story 5.3: OS-Native "Safe to Eject" Handshake

As a Convenience Seeker (Sarah),
I want a system notification the second my sync is done,
So that I can unplug and leave without checking the app.

**Acceptance Criteria:**

**Given** an active sync operation
**When** the final atomic manifest rename is complete
**Then** the system triggers a native OS notification: *"Sync Complete. Ready to Run."*
**And** the Tray icon returns to the "Idle" (Green) state.

### Story 5.4: Visual Manifest Repair Utility

As a Ritualist (Arthur),
I want a guided UI tool to help me fix a corrupted device manifest,
So that I can recover my "Managed" status without a full wipe.

**Acceptance Criteria:**

**Given** a "Dirty" manifest that needs manual intervention
**When** I open the Repair UI
**Then** the tool shows a side-by-side view of "Actual Files" vs "Manifest Record".
**And** allows me to click "Re-link" or "Prune" to fix the state.

## Epic 6: Packaging & Distribution

Package HifiMule into platform-native installers and establish automated cross-platform build pipelines.

### Story 6.1: Tauri Bundler Configuration & Sidecar Packaging

As a System Admin (Alexis),
I want the Tauri bundler configured to include the `hifimule-daemon` binary as a sidecar,
So that a single installer delivers both the UI and the headless engine as a cohesive application.

**Acceptance Criteria:**

**Given** the Cargo workspace with both crates built
**When** I run `cargo tauri build`
**Then** the output produces a platform-native installer containing both the Tauri UI and the daemon sidecar.
**And** the installed application can launch the daemon from the bundled sidecar path.
**And** the application icon, name ("HifiMule"), and metadata are correctly embedded.

### Story 6.2: Windows Installer (MSI)

As a Ritualist (Arthur),
I want a standard Windows MSI installer,
So that I can install HifiMule like any other desktop application on my Windows PC.

**Acceptance Criteria:**

**Given** a successful `cargo tauri build` on Windows
**When** I run the generated MSI
**Then** HifiMule is installed to Program Files with Start Menu shortcuts.
**And** the daemon sidecar is placed alongside the main executable.
**And** uninstallation via "Add/Remove Programs" cleanly removes all installed files.

**Post-MVP: Daemon as Windows Startup Application**
**Given** the MSI installation completes
**When** the installer registers the startup entry
**Then** `hifimule-daemon` is registered as a startup application via a Registry `Run` key (`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`).
**And** the daemon launches automatically when the user logs in, running in the user session with full tray icon and notification support.
**And** the UI detects the running daemon via a health-check RPC call instead of spawning a sidecar.
**And** if the daemon is not running, the UI attempts to launch it directly.
**And** uninstallation removes the Registry `Run` entry.

### Story 6.3: macOS Installer (DMG)

As a Convenience Seeker (Sarah),
I want a macOS DMG with drag-to-Applications install,
So that I can install HifiMule following standard macOS conventions.

**Acceptance Criteria:**

**Given** a successful `cargo tauri build` on macOS
**When** I open the generated DMG
**Then** I see the HifiMule app bundle with a drag-to-Applications prompt.
**And** the app runs without requiring root/sudo privileges (macOS sandbox compliance — NFR9).
**And** the daemon sidecar is embedded within the .app bundle.

**Post-MVP: Daemon as launchd Agent**
**Given** the application is installed to /Applications
**When** first launch completes setup
**Then** a launchd user agent `.plist` is installed to `~/Library/LaunchAgents/`.
**And** the daemon starts automatically on user login.
**And** the UI detects the running daemon via a health-check RPC call instead of spawning a sidecar.
**And** if the agent is not running, the UI attempts `launchctl load` to start it.
**And** app removal cleans up the `.plist` from LaunchAgents.

### Story 6.4: Linux Packages (AppImage & .deb)

As a System Admin (Alexis),
I want AppImage and .deb packages for Linux,
So that I can install HifiMule on both Debian-based systems and any Linux distro via AppImage.

**Acceptance Criteria:**

**Given** a successful `cargo tauri build` on Linux
**When** I run the AppImage
**Then** HifiMule launches without requiring installation.
**When** I install the .deb package
**Then** HifiMule is installed with a desktop entry and can be launched from the application menu.
**And** both formats include the daemon sidecar binary.

**Post-MVP: Daemon as systemd User Service**
**Given** the .deb package is installed
**When** the post-install script runs
**Then** a systemd user service unit is installed and enabled via `systemctl --user enable hifimule-daemon`.
**And** the daemon starts automatically on user login.
**And** the UI detects the running daemon via a health-check RPC call instead of spawning a sidecar.
**And** if the service is not running, the UI attempts `systemctl --user start hifimule-daemon`.
**And** package removal disables and removes the service unit.
**Note:** AppImage cannot register services; it falls back to the sidecar model.

### Story 6.5: CI/CD Cross-Platform Build Pipeline

As a System Admin (Alexis),
I want an automated GitHub Actions workflow that builds and publishes installers for all three platforms,
So that every release produces verified, downloadable artifacts without manual per-platform builds.

**Acceptance Criteria:**

**Given** a tagged release commit (e.g., `v0.1.0`) is pushed
**When** the GitHub Actions workflow triggers
**Then** it builds HifiMule on Windows, macOS, and Linux runners in parallel.
**And** each build produces the platform-native installer (MSI, DMG, AppImage, .deb).
**And** all artifacts are uploaded to a GitHub Release draft.
**And** the workflow fails clearly if any platform build fails.

**Given** the Linux and macOS build runners
**When** the workflow runs
**Then** `libmtp` and its development headers are installed before `cargo build`
  (e.g. `sudo apt-get install -y libmtp-dev` on Ubuntu; `brew install libmtp` on macOS).
**And** `pkg-config` can resolve `libmtp` for the build script.
**And** the Windows runner requires no additional system libraries (`windows-rs` WPD bindings are pure Rust).

### Story 6.6: Installation Smoke Tests

As a System Admin (Alexis),
I want basic smoke tests that verify each installer produces a working application,
So that I can catch packaging regressions before releasing.

**Acceptance Criteria:**

**Given** a freshly built installer for any platform
**When** the smoke test runs (install → launch → verify daemon starts → uninstall)
**Then** each step completes successfully.
**And** the test verifies the daemon sidecar is reachable and responds to a health-check RPC call.
**And** failures produce clear diagnostic output identifying which step failed.

**Given** the smoke test environment has no physical MTP device
**When** the test suite runs
**Then** MTP device IO is verified by unit tests in `device_io.rs` (mock `MtpBackend` returning fixture data).
**And** the smoke test explicitly notes: "MTP end-to-end detection requires manual hardware verification on each platform."

### Story 6.7: macOS Daemon as launchd User Agent

As a Convenience Seeker (Sarah),
I want the HifiMule daemon to start automatically when I log in on macOS,
So that auto-sync fires when I connect my device even if I haven't opened the app.

**Acceptance Criteria:**

**Given** HifiMule is installed to /Applications on macOS
**When** the UI is launched for the first time (or after an upgrade where the plist is absent)
**Then** the UI writes a launchd user agent `.plist` to `~/Library/LaunchAgents/com.hifimule.daemon.plist`, templated with the resolved absolute path to the bundled daemon binary.
**And** the UI runs `launchctl load ~/Library/LaunchAgents/com.hifimule.daemon.plist` so the agent is active immediately.
**And** subsequent user logins start the daemon automatically with no UI interaction required.

**Given** the UI launches and the daemon is already running (started by launchd)
**When** the UI performs its health-check on port 19140
**Then** the UI attaches to the running daemon (status = "startup") without spawning a sidecar.
**And** when the UI window is closed or exits, the daemon is NOT killed (only a sidecar-spawned child held in `DaemonProcess` is killed on exit).

**Given** the user toggles "Launch on Startup" OFF in Settings
**When** the UI calls the `settings.setLaunchOnStartup(false)` RPC
**Then** the UI backend runs `launchctl unload` on the plist.
**And** the daemon continues running for the current session.
**When** the user toggles "Launch on Startup" ON
**Then** the UI backend reinstalls and `launchctl load`s the plist.

**Technical Notes:**
- `.plist` template is embedded in `lib.rs` as a string constant; the `{DAEMON_PATH}` placeholder is replaced at runtime with the path found by scanning `current_exe().parent()` for an entry whose name starts with `"hifimule-daemon-"` (same scan used for quarantine clearance in spec-fix-macos-daemon-launch).
- LaunchAgents dir is created with `create_dir_all` if absent.
- `launchctl` invocations use `std::process::Command` — no elevated privileges.
- `RunEvent::Exit` kill block in `lib.rs`: already gated on `daemon_proc.take()` returning `Some` — no change needed; a launchd-owned daemon was never stored in `DaemonProcess`, so it won't be killed.
- New RPC `settings.setLaunchOnStartup(enabled: bool)` handled in the UI Rust backend (not the daemon), gated `#[cfg(target_os = "macos")]`.
- `service.rs` and Windows logic are untouched.
- Mirror of the Windows startup application pattern already shipped in `startup-fragment.wxs`.

**Status:** backlog

## Epic 7: Technical Hardening & Deferred Fixes

Address the accumulated technical debt and deferred code-review findings from Epics 2–6. Items are grouped by impact area; stories can be worked in parallel where team capacity allows.

### Story 7.1: MTP IO & WPD Hardening

As a System Admin (Alexis),
I want the MTP IO layer to be reliable and efficient across all device types,
So that bulk syncs to MTP devices are fast, atomic, and free of latent data-loss risks.

**Acceptance Criteria:**

**Given** `read_file`, `delete_file`, and `list_files` in `mtp.rs`
**When** Story 7.1 is complete
**Then** `path_to_object_id` accepts `&IPortableDeviceContent` and all three callers pass a single handle — no double `Content()` acquisition exists anywhere in the module.

**Given** the `write_file` stream write loop
**When** `IStream::Write` returns `S_FALSE`
**Then** the loop treats it as a partial write and retries or returns an explicit error (not silently `Ok(())`).

**Given** `ensure_dir_chain` creates a new directory via `CreateObjectWithPropertiesOnly`
**When** `PWSTR::to_string()` fails after the COM call
**Then** `CoTaskMemFree` is called before the `?` propagation (scopeguard or inline free).

**Given** a device with multiple storage objects (e.g., internal + SD card)
**When** `ensure_dir_chain` or `free_space` is called
**Then** the target storage object is selected by the device manifest's `storage_id` rather than always using the first enumerated child.

**Given** `shell_copy_to_device` is invoked
**When** the call is made from a multi-threaded async runtime
**Then** it is executed on a dedicated STA thread (not the MTA `spawn_blocking` pool), resolving the `IShellFolder::EnumObjects` threading constraint.

**Given** a sync operation for many files
**When** `write_file` runs
**Then** directory creation and file copy share a single Shell session per sync job (not one open/close per file), reducing teardown/reconnect overhead.

**Given** a temp file is created during `write_file`
**When** the filename is generated
**Then** it uses a UUID or `tempfile` crate rather than nanosecond timestamps to guarantee uniqueness under concurrent writes.

**Given** the dirty-marker unit test `mtp_dirty_marker_detected_on_reconnect`
**When** the test runs
**Then** it asserts the sentinel content is `b"\x00"` (not just presence and call order).

**Given** a WPD write fails and the Shell fallback is attempted
**When** Shell copy succeeds
**Then** the original WPD error is logged at `warn` level before the fallback succeeds.

**Given** `collect_files_recursive` encounters a directory that fails to enumerate
**When** the error occurs
**Then** the failure is surfaced as a structured warning in the sync result rather than silently skipped.

**Given** `has_msc_drive_for_device` matches a volume label
**When** two connected drives share the same label (e.g., both named "BACKUP")
**Then** the match uses hardware-level GUIDs (`SetupDi` or `DeviceIoControl`) rather than a case-insensitive volume label comparison, so MTP registration is not incorrectly suppressed. [`device/mod.rs`, `has_msc_drive_for_device`]

**Given** `write_file` deletes the existing object before creating the replacement
**When** `CreateObjectWithPropertiesAndData` or the write stream fails after the delete
**Then** the dirty-marker written by `write_with_verify` is present on the device, allowing the repair utility (Story 5.4) to detect and recover the lost file on next connect.
**And** the error path logs the deleted object ID so the incomplete state is diagnosable. [`mtp.rs`, `write_file`]

**Given** `ensure_dir_chain` checks for an existing child object and finds none
**When** a concurrent process creates the same directory between the check and the `CreateObjectWithPropertiesOnly` call
**Then** the resulting COM error is caught and treated as "directory already exists" (tolerated), not a hard failure. [`mtp.rs`, `ensure_dir_chain`]

**Given** `path_to_object_id` is refactored to accept `&IPortableDeviceContent`
**When** unit tests run
**Then** at least one test exercises the traversal logic using a mock `IPortableDeviceContent` fixture, covering the path-component splitting and recursive child lookup without a physical device. [`mtp.rs`, `path_to_object_id`]

**Technical Notes:**
- Scope limited to `mtp.rs` and `device_io.rs`; no new RPC methods
- `storage_id` storage selection requires `DeviceManifest` to carry an optional `storage_id: Option<String>` (set during MTP device init)
- Shell session batching can be introduced as a `ShellSession` RAII guard passed through `execute_sync`
- `has_msc_drive_for_device` hardware-ID comparison: use `CM_Get_Device_ID` or `SetupDiGetDeviceInstanceId` to obtain a stable identifier per drive letter

### Story 7.2: DeviceManager Concurrency Refactor

As a System Admin (Alexis),
I want the DeviceManager to be free of lock-order inversion, partial-state windows, and silent retry-suppression bugs,
So that multi-device scenarios are reliable and concurrent operations never deadlock.

**Acceptance Criteria:**

**Given** `unrecognized_device_path`, `unrecognized_device_io`, and `unrecognized_device_friendly_name`
**When** Story 7.2 is complete
**Then** these three fields are consolidated into a single `RwLock<Option<UnrecognizedDeviceState>>` struct, eliminating the partial-write window between sequential lock acquisitions.

**Given** `update_manifest` (acquires `selected_device_path` → `connected_devices`) and `select_device` (acquires in reverse order)
**When** both run concurrently
**Then** they use a consistent lock acquisition order (or a single combined lock) that makes a 3-thread circular wait impossible.

**Given** `handle_device_removed` removes the currently selected device
**When** at least one other managed device remains connected
**Then** `selected_device_path` is automatically set to one of the remaining devices (rather than `None`).

**Given** `run_mtp_observer` detects a device
**When** the async manifest probe fails transiently
**Then** `known_ids` is NOT inserted until the probe succeeds, allowing the device to be re-detected on the next physical reconnect without requiring a manual intervention.

**Given** `libmtp` is used on Linux/macOS
**When** `LIBMTP_Get_Files_And_Folders` is called with `storage_id=0`
**Then** behavior is verified against the libmtp API docs and either confirmed as "enumerate all storages" or replaced with explicit storage ID iteration.

**Given** concurrent `MtpBackend` IO operations
**When** dispatched as independent `spawn_blocking` tasks
**Then** a per-device serialization mechanism (channel or mutex) ensures MTP operations execute sequentially for each device.

**Given** concurrent event tests for `unrecognized_device_*`
**When** tests run
**Then** at least one test exercises simultaneous set/clear events to verify the consolidated lock holds invariants under concurrency.

**Given** `MscBackend::new(path)` is constructed in `handle_device_unrecognized`
**When** a re-probe arrives before `connected_devices.remove(&path)` completes
**Then** the `DeviceManager` ensures only one live `Arc<dyn DeviceIO>` exists per path at any point (e.g., by removing the old entry before constructing the new backend). [`device/mod.rs`, `handle_device_unrecognized`]

**Given** `get_mounts` calls `canonicalize` then `is_mount_point` sequentially for a volume
**When** the volume is remounted between the two calls
**Then** the stale entry is skipped or re-evaluated on the next hotplug event rather than producing a hard error. [`device/mod.rs`, `get_mounts`]

**Given** a daemon upgrade is performed without restarting the daemon
**When** the new boot-volume exclusion guard in `get_mounts` runs
**Then** any boot-volume path already present in `known_mounts` from the pre-fix binary is evicted on the next mount-scan cycle (e.g., by clearing `known_mounts` on config reload or daemon restart detection). [`device/mod.rs`, `get_mounts`]

**Technical Notes:**
- `UnrecognizedDeviceState` struct: `{ path: PathBuf, io: Arc<dyn DeviceIO>, friendly_name: Option<String> }`
- Lock consolidation is a breaking internal refactor; keep the public RPC interface unchanged
- Auto-reselect in `handle_device_removed`: select the first entry remaining in `connected_devices`
- `known_mounts` eviction: clearing on daemon version mismatch is acceptable given daemon restarts are fast

### Story 7.3: Device UI & Identity Polish

As a Convenience Seeker (Sarah) and System Admin (Alexis),
I want the device initialization flow and device hub to reflect real device identity and surface MTP constraints clearly,
So that the UI never shows stale defaults, silent gaps, or confusing state for MTP devices.

**Acceptance Criteria:**

**Given** an MTP device is detected and its `friendly_name` is stored in `unrecognized_device_friendly_name`
**When** the "Initialize Device" dialog opens
**Then** the device name input is pre-filled with the MTP `friendly_name` (e.g., "Garmin Forerunner 945") instead of "My Device".

**Given** a device manifest where `name` is an empty string (`""`)
**When** the device hub resolves the display name
**Then** the empty string is filtered out (`.filter(|n| !n.is_empty())`) and the `friendly_name` fallback chain is applied correctly.

**Given** an MTP device is connected
**When** the Device State panel renders `unmanaged_count`
**Then** the panel displays "MTP — folder enumeration not available" instead of a silent `0`, so the user understands the limitation.

**Given** `broadcast_device_state` runs for a device already present in `connected_devices`
**When** the device was previously detected
**Then** `handle_device_detected` is NOT re-triggered (duplicate insertion and spurious dirty-state transitions are prevented).

**Given** an MTP device is connected
**When** the Storage Projection bar calculates available capacity
**Then** `free_space()` returns a real value from the MTP device's storage object (not `None`), so capacity is visible in the UI.

**Given** `initialize_device` is called
**When** the physical device was disconnected and reconnected between the `Unrecognized` event and the user completing initialization
**Then** the RPC handler detects the stale `Arc<dyn DeviceIO>` (e.g., via a liveness check) and returns a clear error instead of writing to a dead handle.

**Given** `cleanup_tmp_files` runs
**When** a `.tmp` file exists at the device root (outside `managed_paths`)
**Then** it is included in the cleanup sweep.

**Given** `initialize_device` creates a managed path
**When** the path has multiple levels
**Then** `create_dir_all` is used instead of `create_dir`, avoiding a latent failure for nested paths.

**Given** the MTP scrobbler detection path in `scrobble.rs`
**When** `read_file` returns a plain `anyhow` error because no `.scrobbler.log` exists
**Then** the "not found" case is detected correctly (not treated as a read error), matching the MSC path behavior.

**Technical Notes:**
- `get_daemon_state` RPC gains `pendingDeviceFriendlyName: Option<String>` for the UI pre-fill
- `BasketSidebar.ts` `openInitDeviceModal`: read `pendingDeviceFriendlyName` and set as default value
- MTP `free_space`: wire `DeviceManager`'s `storage_id` (from Story 7.1) into `get_device_storage`

### Story 7.4: Packaging & CI/CD Hardening

As a System Admin (Alexis),
I want the CI/CD pipeline and installers to be reproducible, supply-chain-safe, and properly declare runtime dependencies,
So that every release artifact is verifiable, installable on clean machines, and not broken by upstream floating versions.

**Acceptance Criteria:**

**Given** the Linux `.deb` package
**When** installed on a machine without `libmtp` pre-installed
**Then** the package declares `libmtp9` (or the appropriate soname) as a runtime `Depends` entry, so `apt` installs it automatically.

**Given** the Linux AppImage
**When** built
**Then** the `libmtp.so` shared library is bundled inside the AppImage (linuxdeploy or `--appimage-extract-and-run`), so it runs on distros without `libmtp` in the system library path.

**Given** the macOS DMG
**When** distributed to a machine without Homebrew or `libmtp`
**Then** the `.app` bundle includes the `libmtp.dylib` (and its transitive deps) via `@rpath` or `otool -L` fixup, so the app launches on a clean macOS install.

**Given** `.github/workflows/release.yml`
**When** reviewed
**Then** `pnpm/action-setup` is pinned to a specific version (e.g., `v4.1.0`), `node-version` is pinned to `"20"`, and `tauri-apps/tauri-action` is pinned to a commit SHA.

**Given** `scripts/prepare-sidecar.mjs`
**When** `copyFileSync` fails mid-execution
**Then** any partially-written sidecar is removed before the script exits with a non-zero code (atomic swap or cleanup-on-failure).

**Given** `scripts/prepare-sidecar.mjs`
**When** a build for a new architecture runs
**Then** stale sidecar binaries from previous architectures in `sidecars/` are removed before the new copy is written.

**Given** `scripts/prepare-sidecar.mjs`
**When** `rustc -vV` output format changes
**Then** the parsing logic degrades gracefully (returns an error) rather than silently producing an `undefined` triple.

**Given** a CI runner or fresh clone
**When** `prepare-sidecar.mjs` executes
**Then** it verifies `node_modules` is populated (or runs `npm install`) before invoking `npm run build`.

**Given** `beforeBuildCommand` in `tauri.conf.json`
**When** triggered by `cargo tauri build` on Windows and Linux
**Then** the command resolves relative paths correctly regardless of whether the CWD is the workspace root or the `hifimule-ui` directory.

**Given** the boot-volume exclusion guard in `get_mounts` (`device/mod.rs:968-975`)
**When** unit tests run
**Then** at least one test covers the `canonicalize`-based root check with a mocked filesystem path.

**Given** the installation smoke tests
**When** run in CI
**Then** the macOS step uses `find "${MOUNT_POINT}" -name "*.app" -maxdepth 1` rather than hardcoding `HifiMule.app`, so a `productName` change in `tauri.conf.json` does not silently break the test.

**Given** the smoke test workflow
**When** a release is published
**Then** the workflow also has a `workflow_call` trigger so it can be invoked programmatically from the release pipeline.

**Given** `tauri.conf.json` sets `minimumSystemVersion`
**When** a new macOS-specific dependency is added that raises the minimum OS floor
**Then** `minimumSystemVersion` is updated in the same PR so it never silently advertises false compatibility.
**And** a CI lint step (or PR checklist item) reminds contributors to verify the value when adding macOS deps.

**Given** the Linux `.deb` package built by Story 6.4
**When** installed via `sudo dpkg -i` on a clean VM and launched from the application menu
**Then** the daemon starts and responds to a health-check RPC at `localhost:19140` (the functional test deferred from Story 6.4 AC2/AC4/AC5 is now verified in this story's acceptance run).

**Given** the Xvfb display server is started on `:99` in the Linux smoke test
**When** another process already occupies `:99` on the shared runner
**Then** the smoke test script auto-selects the next available display number (e.g., `Xvfb -displayfd 1` or iterating `:99`→`:100`) rather than failing silently with a wrong display. [`.github/workflows/smoke-tests.yml`]

**Given** the Windows smoke test searches for the installed executable
**When** the MSI `INSTALLDIR` is customized or a NSIS target is added
**Then** the smoke test resolves the install path via the registry (`HKLM\SOFTWARE\HifiMule\InstallDir`) rather than the hardcoded `C:\Program Files\HifiMule`. [`.github/workflows/smoke-tests.yml`]

**Technical Notes:**
- `.deb` runtime deps: add `"deb": { "depends": ["libmtp9"] }` in `tauri.conf.json` bundle config
- AppImage bundling: add `linuxdeploy-plugin-qt` or equivalent `--library` flags in the Linux CI step
- macOS dylib: use `otool`/`install_name_tool` or `dylibbundler` post-build step in `prepare-sidecar.mjs`
- CI version pins: use `actions/gh-actions-toolset` digests for `tauri-action`
- Xvfb display: `Xvfb -displayfd fd` writes the chosen display number to a file descriptor — avoids hardcoded `:99`
- Windows install path registry key: written by the WiX installer via `<RegistryValue>` in the product `.wxs`

### Story 7.5: Machine-Bound Credential Vault (Replace Keyring)

As a System Admin (Alexis),
I want media credentials to be stored in a hardware-bound encrypted file rather than the OS-native keyring,
So that the daemon works reliably in headless and minimal Linux environments without D-Bus or Secret Service dependencies.

**Acceptance Criteria:**

**Given** `CredentialManager::save_credentials(server_id, token_or_password)` is called for a Jellyfin token or Subsonic password
**When** the write completes
**Then** a `secrets.enc` file is written to `get_app_data_dir()` (alongside `config.json`).
**And** no OS credential vault (Keychain, Credential Manager, Secret Service) is accessed.
**And** the vault stores `HashMap<String, ServerCredentials>` where the key is the server UUID and the value contains `token_or_password`.

**Given** `CredentialManager::load_credentials(server_id)` is called on a machine that saved the vault
**When** the read completes
**Then** the `ServerCredentials` for that server UUID is returned.
**And** if the server UUID does not exist in the vault, `None` is returned (not an error).

**Given** `secrets.enc` contains a legacy single-`Secrets` blob (pre-multi-server upgrade)
**When** `load_all_credentials()` is called on first daemon startup
**Then** the old blob is detected (by attempting deserialization as `Secrets`), converted to `HashMap<existing_server_uuid, credentials>`, and re-encrypted as the new format.
**And** the migrated vault replaces the old file atomically.

**Given** `secrets.enc` is copied to a different machine
**When** `load_all_credentials()` is called on the second machine
**Then** decryption fails (hardware fingerprint mismatch) and an empty `HashMap` is returned.

**Given** `CredentialManager::remove_credentials(server_id)` is called
**When** the call completes
**Then** the entry for that server UUID is removed from the vault map and the file re-encrypted.

**Given** `CredentialManager::clear_all_credentials()` is called
**When** the call completes
**Then** `secrets.enc` is deleted from disk.
**And** `config.json` is also removed (existing behaviour unchanged).

**Given** the test suite runs (`cargo test`)
**When** the `#[cfg(test)]` mock seam is active
**Then** all `CredentialManager` tests pass without modification.

**Technical Notes:**
- New module `hifimule-daemon/src/vault.rs`: `derive_key(app_salt)` (machine-uid + blake3) + `encrypt_file(path, plaintext, salt)` + `decrypt_file(path, salt)` (ChaCha20-Poly1305)
- Nonce: **random per write** via `OsRng` (not a deterministic hash) — prepended as first 12 bytes of `secrets.enc`
- Key material wrapped in `secrecy::Secret<[u8; 32]>` and zeroized after use
- Remove `keyring = "2.3"` from `Cargo.toml`; add `machine-uid = "0.5"`, `blake3 = "1.5"`, `chacha20poly1305 = "0.10"`, `secrecy = { version = "0.8", features = ["serde"] }`, and `rand` (if not already present) for `OsRng`
- `VAULT_APP_SALT: &str = "hifimule.github.io/secrets/v1"`
- Reference: `_bmad-output/planning-artifacts/research/encryption.md` + `sprint-change-proposal-2026-05-30-keyring-to-machine-bound-encryption.md`

## Epic 8: Multi-Provider Media Server Support

> **Prerequisite for modified Stories 2.1, 2.5, 3.1, 4.8, 5.1.**
> Stories 8.1–8.4 must be completed before implementing the modified versions of those stories.
> Epic 8 introduces zero user-visible behavior change for existing Jellyfin users — it is a pure refactor + addition at the provider layer.
>
> **Recommended sequencing:**
> - Phase A: 8.1 → 8.2 → 8.3 → 8.4 → 8.5 (8.6 can run parallel to 8.3)
> - Phase B: Modified existing stories 2.1, 2.5, 3.1, 4.8, 5.1 (in their original epic order)

Introduce a provider abstraction layer enabling HifiMule to connect to Jellyfin, Navidrome, Subsonic, and any OpenSubsonic-compatible media server.

### Story 8.1: MediaProvider Trait & Domain Models

As a System Admin (Alexis),
I want all server communication routed through a shared `MediaProvider` trait,
So that the sync engine, auto-fill, and scrobble bridge never depend on server-specific API details.

**Acceptance Criteria:**

**Given** `hifimule-daemon` is built
**When** any module needs to browse, download, or query the media server
**Then** it calls methods on `Arc<dyn MediaProvider>` — no direct HTTP calls outside the `providers/` module.

**Given** the `domain/models.rs` module is defined
**When** a provider returns library data
**Then** it returns domain types (`Song`, `Album`, `Artist`, `Playlist`) with all fields normalized:
- IDs: always `String` (never `i64`/`u64`)
- Duration: `u32` seconds
- Bitrate: `u32` kbps
- Cover art ref: `Option<String>` (separate from item ID for Subsonic)

**Technical Notes:**
- `providers/mod.rs`: `MediaProvider` trait (async-trait), `ProviderError`, `ServerType`, `Capabilities`
- `domain/models.rs`: `Song`, `Album`, `Artist`, `Playlist`, `SearchResult`, `ChangeEvent`
- Add `async-trait = "0.1"` to `hifimule-daemon` dependencies
- Newtype wrappers at DTO boundaries to prevent unit conversion bugs reaching the domain layer

### Story 8.2: JellyfinProvider Adapter

As a System Admin (Alexis),
I want the existing Jellyfin API client wrapped as a `JellyfinProvider` implementing `MediaProvider`,
So that no existing functionality regresses and all callers use the unified interface.

**Acceptance Criteria:**

**Given** `JellyfinProvider` wraps the existing `api.rs` logic
**When** any caller accesses the library, downloads a track, or fetches cover art
**Then** it uses `provider.get_album()`, `provider.download_url()`, `provider.cover_art_url()` etc.
**And** all Jellyfin-specific DTO→domain conversions happen at the adapter boundary (`RunTimeTicks ÷ 10_000_000 → seconds`, bps → kbps).

**Given** `provider.changes_since(timestamp)` is called on `JellyfinProvider`
**Then** it queries `GET /Items?minDateLastSaved={ISO}` for item-level incremental sync.

**Technical Notes:**
- Rename/move `api.rs` → `providers/jellyfin.rs`; `JellyfinProvider` struct wraps existing `JellyfinClient`
- All existing callers (`sync.rs`, `rpc.rs`, `scrobble.rs`) updated to use `Arc<dyn MediaProvider>`
- `jellyfin-sdk = "=0.x.y"` pinned exact version (pre-1.0)
- Unit tests: 100% coverage for `JellyfinItem → Song`, `RunTimeTicks` conversion, UUID ID passthrough

### Story 8.3: SubsonicProvider Adapter

As a Ritualist (Arthur) and Convenience Seeker (Sarah),
I want HifiMule to connect to Navidrome and any Subsonic/OpenSubsonic-compatible server,
So that users who prefer Navidrome can use HifiMule without switching media servers.

**Acceptance Criteria:**

**Given** `SubsonicProvider` implements `MediaProvider`
**When** connected to a Navidrome or Subsonic server
**Then** `list_artists()` calls `GET /rest/getArtists.view`, `get_album()` calls `GET /rest/getAlbum.view`, `list_playlists()` calls `GET /rest/getPlaylists.view`.
**And** all `SubsonicSong → Song` conversions are correct: `duration` (seconds, no conversion), `bitRate` → kbps, `id` → `String`.

**Given** the server supports OpenSubsonic (`openSubsonic: true` in ping response)
**When** `provider.capabilities()` is called
**Then** the `Capabilities` struct reflects detected extension support (cached via `std::sync::Once`).

**Given** `provider.changes_since(timestamp)` is called on `SubsonicProvider`
**Then** it calls `GET /rest/getIndexes.view?ifModifiedSince={epoch_ms}`.
**And** if the artist list changed, re-fetches affected albums via `getAlbum` to detect song-level changes.

**Given** `provider.download_url()` is called with a `TranscodeProfile`
**Then** for passthrough: returns `/rest/download.view?id={id}&...auth_params`
**Then** for transcoding: returns `/rest/stream.view?id={id}&format=mp3&maxBitRate=192&...auth_params`
**And** `maxBitRate` is in **kbps** (not bps).

**Technical Notes:**
- `opensubsonic = "latest"` crate (full v1.16.1, OpenSubsonic extensions, async, JSON)
- Auth: per-request MD5 token+salt signing (`t=md5(password+salt)`, `s=salt`) — stateless
- All Subsonic URLs MUST be sanitized via `sanitize_subsonic_url()` before logging
- Unit tests: `SubsonicSong → Song` (duration passthrough, kbps passthrough, string ID), cover art ID ≠ song ID
- HTTP mock tests with `wiremock`; snapshot tests with `insta` for Navidrome + classic Subsonic fixtures

### Story 8.4: Runtime Server-Type Detection Factory

As a System Admin (Alexis),
I want the daemon to auto-detect the server type when I enter a URL,
So that I don't need to manually specify "Jellyfin" or "Navidrome" during setup.

**Acceptance Criteria:**

**Given** a user enters a server URL in setup
**When** `server.connect` RPC is called with `serverType: 'auto'`
**Then** the daemon pings the URL in order:
1. Subsonic `GET /rest/ping.view` — if response contains `openSubsonic: true` → `ServerType::Subsonic` with OpenSubsonic capabilities
2. Subsonic ping without OpenSubsonic flag → `ServerType::Subsonic` (classic)
3. Jellyfin `GET /System/Info` → `ServerType::Jellyfin`
4. All fail → returns error "Unknown server type at this URL"
**And** the detected `ServerType` is persisted in the daemon config.
**And** the correct `MediaProvider` implementation is instantiated and held as `Arc<dyn MediaProvider>`.

**Given** `server.connect` is called with `serverType: 'jellyfin'` or `serverType: 'subsonic'`
**Then** the detection step is skipped and the specified provider type is used directly.

**Technical Notes:**
- Factory function: `async fn connect(url: &str, creds: &Credentials, hint: ServerTypeHint) -> Result<Box<dyn MediaProvider>>`
- `ServerTypeHint::Auto | ServerTypeHint::Jellyfin | ServerTypeHint::Subsonic`
- IPC: `server.connect` params gain `serverType: 'jellyfin' | 'subsonic' | 'auto'`
- `get_daemon_state` response gains `serverType: string | null`

### Story 8.5: Subsonic URL Credential Sanitization

As a System Admin (Alexis),
I want Subsonic auth credentials to never appear in log files,
So that my server password is not exposed in application logs.

**Acceptance Criteria:**

**Given** any Subsonic request URL is logged
**When** the URL contains `u=`, `p=`, `t=`, or `s=` query parameters
**Then** those parameters are replaced with `[REDACTED]` before the log line is written.
**And** this sanitization applies to ALL log outputs: `daemon.log`, `ui.log`, `tracing` spans, and debug output.

**Given** `download_url()` or `cover_art_url()` on `SubsonicProvider` returns a URL
**When** the sync engine logs progress (file name, URL)
**Then** the logged URL has auth params stripped.

**Technical Notes:**
- `sanitize_subsonic_url(url: &Url) -> String` utility in `providers/subsonic.rs`
- All `tracing::debug!` / `tracing::info!` calls in sync path that log URLs must use `sanitize_subsonic_url()`
- Unit test: verify sanitization removes `u`, `p`, `t`, `s` params and leaves other params intact

### Story 8.6: Incremental Sync — Subsonic Album-Level Fallback

As a Ritualist (Arthur),
I want incremental sync to correctly detect new songs added to existing albums on Navidrome,
So that a new track added to an album I have synced is picked up on the next incremental sync.

**Acceptance Criteria:**

**Given** Navidrome reports via `getIndexes?ifModifiedSince` that the artist list has NOT changed
**When** a new song was added to an existing album since the last sync
**Then** the sync engine still detects the new song by re-fetching affected albums (fallback strategy).

**Given** the full library dump is requested (initial sync)
**When** `changes_since(EPOCH)` is called on `SubsonicProvider`
**Then** it uses `search3?query=&songCount=500&songOffset={n}` with pagination to enumerate all tracks.

**Technical Notes:**
- `SubsonicProvider.changes_since()`: when `getIndexes` returns "not modified", re-fetch all albums in the current manifest and compare song lists (by count + track IDs)
- No ETag equivalent in Subsonic — compare `size` + `contentType` + `suffix` as a change signal
- This story can be implemented in parallel with Story 8.3

## Epic 9: Rich Library Navigation

Expand the Library Browser from artist/album hierarchy into a Jellyfin-like curation surface with provider-supported navigation modes for genres, recently added, frequently played, recently played, and favorites.

### Story 9.1: Provider Browse Modes and Capability Contract

As a System Admin (Alexis),
I want the daemon provider layer to expose supported browse modes explicitly,
So that the UI can show Jellyfin-like navigation without hardcoding server-specific API behavior.

**Acceptance Criteria:**

**Given** a provider is connected
**When** the UI requests available browse modes
**Then** the daemon returns the modes supported by that provider: artists, albums, playlists, genres, recentlyAdded, frequentlyPlayed, recentlyPlayed, favorites.

**Given** a browse mode is unsupported by the active provider
**Then** the UI does not offer it as an active navigation path.

**Given** browse data is requested
**Then** all calls go through `Arc<dyn MediaProvider>` and no UI or RPC handler constructs server-specific URLs directly.

**Technical Notes:**
- Extend domain models with `Genre` and optional browse metadata fields such as `dateAdded`, `lastPlayedAt`, `playCount`, and `isFavorite`.
- Extend `Capabilities` or add `BrowseCapabilities` with boolean support per mode.
- Add explicit trait methods rather than a generic string dispatch:
  - `list_genres(library_id: Option<&str>)`
  - `get_genre_tracks(genre_id_or_name: &str)`
  - `list_recently_added(library_id: Option<&str>, limit: u32, offset: u32)`
  - `list_frequently_played(library_id: Option<&str>, limit: u32, offset: u32)`
  - `list_recently_played(library_id: Option<&str>, limit: u32, offset: u32)`
  - `list_favorites(library_id: Option<&str>, limit: u32, offset: u32)`
- Preserve the architecture rule that server API details stay inside `providers/`.

### Story 9.2: Browse Mode Navigation UI

As a Ritualist (Arthur),
I want a clear browse-mode control in the Library Browser,
So that I can switch between Artists, Albums, Playlists, Genres, Recently Added, Frequently Played, Recently Played, and Favorites without losing basket context.

**Acceptance Criteria:**

**Given** the main UI is open and a server is connected
**When** supported browse modes are returned by the daemon
**Then** the Library Browser renders them as a compact tab or segmented navigation control.

**Given** I switch browse modes
**Then** the current basket remains unchanged and the library content refreshes to the selected mode.

**Given** I browse into a hierarchical item
**Then** breadcrumbs continue to work within that mode.

**Given** I return to a previous browse mode
**Then** scroll and page cache restore for that mode when valid.

**Given** no device is selected
**Then** add buttons are disabled in every browse mode.

**Technical Notes:**
- Refactor `library.ts` state to include `browseMode`.
- Cache key should include both browse mode and parent ID, e.g. `${browseMode}:${parentId ?? 'root'}`.
- Keep artist quick-nav, and consider applying the same pattern to genres if result count is large.
- Preserve existing `MediaCard` selection overlay and synced badge behavior.

### Story 9.3: Genre Browsing and Genre Entity Basket Item

As a Ritualist (Arthur),
I want to browse by genre and add a genre to the basket as a single entity,
So that my device can receive a dynamic genre-based selection without manually picking every album.

**Acceptance Criteria:**

**Given** the active provider supports genres
**When** I open the Genres browse mode
**Then** I see a music-only list/grid of genres.

**Given** I click a genre
**Then** I can view the tracks or albums associated with that genre.

**Given** I click (+) on a genre
**Then** a single Genre card is added to the basket.

**Given** sync starts with a Genre card in the basket
**Then** the daemon resolves the current track list for that genre at sync time.

**Given** artist, album, playlist, track, and genre selections overlap
**Then** duplicates are removed during sync planning.

**Technical Notes:**
- Add `BasketItem.type: 'MusicGenre'`.
- Use the existing Artist entity pattern as the model.
- Genre entity size and track count may be estimates at add time, then resolved exactly at sync time.

### Story 9.4: History and Favorites Browse Modes

As a Convenience Seeker (Sarah),
I want quick access to Recently Added, Frequently Played, Recently Played, and Favorites,
So that I can build a device basket from the music I am most likely to want offline.

**Acceptance Criteria:**

**Given** the active provider supports Recently Added
**When** I open Recently Added
**Then** the newest music items are shown first.

**Given** the active provider supports Frequently Played
**When** I open Frequently Played
**Then** items are sorted by server play count descending.

**Given** the active provider supports Recently Played
**When** I open Recently Played
**Then** items are sorted by last played date descending.

**Given** the active provider supports Favorites
**When** I open Favorites
**Then** favorited music items are shown.

**Given** a mode returns tracks directly
**Then** track cards can be added to the basket with existing metadata and size calculation behavior.

**Technical Notes:**
- Keep these as manual browse result views, not dynamic basket slots.
- Auto-Fill remains the only dynamic priority slot for this change.
- Display relevant metadata when available: play count, last played date, date added, favorite state.

### Story 9.5: Hierarchical Favorites Navigation

As a Convenience Seeker (Sarah),
I want Favorites to browse as Artists -> Albums -> Tracks instead of a flat song list,
So that I can sync favorite artists, favorite albums, and favorite tracks without losing the normal music hierarchy.

**Acceptance Criteria:**

**Given** the active provider supports Favorites
**When** I open the Favorites browse mode
**Then** the root level shows artists that are directly favorited, artists that have favorited albums, and artists that have favorited tracks.

**Given** I select an artist in Favorites
**When** the artist is directly favorited
**Then** the album level shows all albums for that artist.

**Given** I select an artist in Favorites
**When** the artist is not directly favorited but has favorite albums or tracks
**Then** the album level shows directly favorited albums for that artist plus albums that contain favorited tracks for that artist.

**Given** I select an album in Favorites
**When** the album is directly favorited or belongs to a directly favorited artist selected from Favorites
**Then** the track level shows all tracks in that album.

**Given** I select an album in Favorites
**When** the album is not directly favorited and does not belong to a directly favorited artist
**Then** the track level shows only favorited tracks from that album.

**Given** the provider exposes favorite artists, favorite albums, and favorite tracks in a single favorites response
**Then** the UI uses that response as the favorite tree source and does not continue to render root Favorites as a paginated flat track result.

**Technical Notes:**
- Add or keep a provider-neutral `list_favorite_items(library_id)` contract returning favorite artists, albums, and songs.
- Use a cached UI favorite tree to derive the three navigation levels.
- Preserve existing artist, album, and audio basket item types; do not add a new dynamic favorites basket entity.
- Favorites hierarchy is not paginated in the UI; `Load More` remains a no-op for this mode.

### Story 9.6: Navidrome/Subsonic Browse Parity Hardening

As a Navidrome/Subsonic user,
I want the browse surface to expose every history/navigation mode the active server can support,
So that switching from Jellyfin does not remove core curation workflows.

**Acceptance Criteria:**

**Given** a Navidrome/OpenSubsonic server exposes a reliable Recently Added album endpoint
**When** `browse.listModes` is called
**Then** `recentlyAdded` is included.
**And** `browse.listRecentlyAdded` returns newest albums first.

**Given** a Navidrome/OpenSubsonic server exposes reliable frequent listening data
**When** `browse.listModes` is called
**Then** `frequentlyPlayed` is included.
**And** `browse.listFrequentlyPlayed` returns tracks sorted by server play count descending.

**Given** a Navidrome/OpenSubsonic server exposes reliable recent listening data
**When** `browse.listModes` is called
**Then** `recentlyPlayed` is included.
**And** `browse.listRecentlyPlayed` returns tracks sorted by last played date descending.

**Given** classic Subsonic cannot support a mode reliably
**When** capabilities are calculated
**Then** the mode remains hidden.
**And** the daemon returns `UnsupportedCapability` if the mode is called directly.

**Given** Albums mode is open and the result count warrants quick navigation
**When** the active provider is Subsonic/Navidrome
**Then** alphabetic filtering works consistently with Jellyfin album quick navigation.

**Given** provider support differs by server
**When** the UI renders browse modes
**Then** all capability decisions are made in `SubsonicProvider`, not in the UI.

**Technical Notes:**
- Keep the UI provider-neutral; it should consume `browse.listModes` and existing `browse.*` RPCs only.
- Prefer OpenSubsonic/Navidrome endpoints that expose ordered recent/frequent data reliably; hide modes rather than synthesizing misleading lists.
- Tests should cover capability lists for OpenSubsonic/Navidrome versus classic Subsonic, recently added sorting, frequently played sorting, recently played sorting, and album letter filtering.

### Story 9.7: Virtualized List/Table Browse View

As a Ritualist (Arthur),
I want to browse Artists and Albums as a list/table view in addition to the current grid,
So that I can scan libraries of thousands of items quickly without waiting for pagination.

**Acceptance Criteria:**

**Given** the Artist or Album browse page is open
**When** I toggle to list/table view
**Then** the list renders immediately with the currently loaded items using virtualized windowed rendering.
**And** scroll performance remains smooth for libraries of thousands of items.

**Given** I scroll the list
**Then** only visible rows are mounted in the DOM at any time.

**Given** I scroll toward the end of the loaded rows in list view
**Then** the next page is fetched automatically from the daemon and appended to the list.
**And** this continues until all items (up to `total`) are loaded.

**Given** the browse page has an A–Z filter control
**When** I select a letter in either grid or list view
**Then** the view fetches and displays only items starting with that letter (server-side filter), identical in both views.

**Given** I am in list/table view
**When** I click an item
**Then** drill-down, breadcrumb, and basket-add behaviors are identical to grid view.

**Given** I toggle between grid and list view
**When** data has already been fetched
**Then** the view switches without re-fetching from the daemon.

**Technical Notes:**
- Implement windowed/virtualized rendering with autoload-on-scroll: render immediately with the loaded page; fetch the next page (200 items) when the user scrolls within 5 rows of the loaded boundary.
- The scroller element height is set to `state.pagination.total × VIRTUAL_ROW_HEIGHT` from the start so the scrollbar reflects the full expected size; height is updated as the total is refined by responses.
- View mode (grid vs list) is stored in local UI state per browse mode. **Note: superseded by Story 9.8** — the toggle is now a single global value.
- A–Z is a server-side filter in both grid and list view. There is no client-side scroll-to-letter behavior.
- When an A–Z letter filter is active in list view, autoload-on-scroll is suppressed (the filtered set is already complete).
- No new daemon RPCs or basket entity types; this is a pure UI rendering concern.
- Both views share the same data model from the existing `browse.*` RPC layer.

### Story 9.8: Extend Grid/Table Toggle to All Browse Modes and Drill-Down Levels

As a Ritualist (Arthur),
I want the grid/table toggle to work on every browse page and drill-down level,
So that I can use my preferred view mode consistently across all library content — not just at the Artists/Albums root.

**Acceptance Criteria:**

**Given** any browse mode is active (artists, albums, playlists, genres, recentlyAdded, frequentlyPlayed, recentlyPlayed, favorites)
**When** the browse area renders
**Then** the view toggle (grid/list) is always visible in the browse-mode bar.

**Given** I am drilled into a sub-level (e.g., albums within an artist, tracks within an album)
**When** the sub-level content renders
**Then** the view toggle remains visible and the active mode (grid or list) applies.

**Given** I toggle to list view
**When** I switch browse mode or navigate into/out of a sub-level
**Then** the global toggle state is preserved — all levels and modes use the same grid/list preference.

**Given** list view is active
**When** I click a sub-level item (album row drills to tracks; track row adds to basket)
**Then** drill-down and basket-add behaviors are identical to grid view.

**Given** list view is active on a mode without autoload (playlists, genres, history/favorites, or any sub-level with breadcrumbs)
**Then** the list renders what is currently loaded; autoload-on-scroll is not triggered (that behavior remains exclusive to artists/albums root).

**Technical Notes:**
- Remove the `(state.browseMode === 'artists' || state.browseMode === 'albums') && state.breadcrumbStack.length === 0` guard from `renderViewToggle()` in `library.ts:593–596`.
- Remove the matching mode+breadcrumb guard from `renderCurrentView()` at `library.ts:823–826`.
- No state structure change needed: `state.listViewMode` is already a single global value.
- `loadMoreForListView` continues to operate only for artists/albums root; the `rootMode` variable in `renderList()` already returns `null` for other modes/levels, which suppresses autoload — no change needed there.
- No new daemon RPCs; pure UI rendering concern.

### Story 9.9: Tracks Browse Mode — Provider Contract & Daemon RPC

As a System Admin (Alexis),
I want the daemon to expose a flat, paginated, filterable track listing,
So that the UI can present a library-wide Tracks browse mode for both Jellyfin and OpenSubsonic-class servers.

**Acceptance Criteria:**

**Given** a provider that implements `list_tracks`
**When** `browse.listTracks({ startIndex: 0, limit: 200 })` is called
**Then** the daemon returns the first page of library tracks along with `total`.

**Given** `browse.listTracks` is called with `artistId`
**Then** the response is filtered to tracks whose artist matches.

**Given** `browse.listTracks` is called with `albumId`
**Then** the response is filtered to tracks within that album.

**Given** both `artistId` and `albumId` are provided
**Then** the album filter takes precedence (album implies its artist).

**Given** a Subsonic provider without `search3` support
**When** `browse.listModes` is called
**Then** `Tracks` is not present in the returned list.

**Given** a provider that does not advertise `Tracks`
**When** `browse.listTracks` is called anyway
**Then** an RPC error indicating unsupported capability is returned.

**Given** A–Z letter filtering is implemented (optional v1)
**When** `letter` is provided
**Then** only tracks whose title starts with that letter are returned.

**Technical Notes:**
- `BrowseMode::Tracks` added to `providers/mod.rs`.
- New `TrackListFilter` and `TrackListPage` types; default `list_tracks` trait impl returns `ProviderError::NotSupported`.
- Jellyfin: `GET /Users/{uid}/Items?IncludeItemTypes=Audio&Recursive=true&SortBy=Name,Album&StartIndex&Limit[&ArtistIds][&AlbumIds][&NameStartsWith]`.
- Subsonic/OpenSubsonic: `search3?query=&songCount&songOffset` for unfiltered enumeration; `getArtist`+`getAlbum` aggregation when `artistId`/`albumId` is set; classic Subsonic without `search3` returns `NotSupported` and omits `Tracks` from `list_modes`.
- All Subsonic URL auth sanitization rules apply.
- Daemon RPC: `browse.listTracks` handler dispatches to `provider.list_tracks`; capability-absent requests are rejected.
- Tests: unfiltered page, artist filter, album filter, combined filters, letter filter, `NotSupported` path.

### Story 9.10: Tracks Browse Mode — Dual-Panel UI with Auto-Pagination & Track Actions

As a Ritualist (Arthur),
I want to browse my entire library at the track grain with artist and album filters,
So that I can quickly find and queue individual songs without drilling through albums.

**Acceptance Criteria:**

**Given** the active provider advertises the Tracks mode
**When** the Library Browser renders the browse-mode bar
**Then** a "Tracks" mode is shown alongside the existing modes.

**Given** I select the Tracks mode
**Then** the view renders three panels: an artists panel on the left, an albums panel on the right, and a track list panel below.

**Given** the Tracks view is rendering
**Then** the artist panel auto-paginates the full library artist list via `browse.listArtists` with autoload-on-scroll.
**And** the album panel auto-paginates albums (filtered by the selected artist if any) via `browse.listAlbums` with autoload-on-scroll.
**And** the track list auto-paginates via `browse.listTracks` with the active artist/album filters.

**Given** the artist panel shows an "All artists" entry at the top
**When** I select it
**Then** the album panel shows all library albums (paginated) and the track panel shows all library tracks (paginated).

**Given** I select an artist in the left panel
**Then** the album panel filters to that artist's albums (paginated).
**And** the track panel filters to that artist's tracks (paginated).

**Given** I select an album in the right panel
**Then** the track panel filters to that album's tracks (paginated).

**Given** the album panel shows an "All albums" entry at the top
**When** I select it
**Then** the track panel filter clears its album constraint (artist constraint, if any, remains).

**Given** a device is selected
**When** a track row renders
**Then** a (+) "Add to basket" control is shown; if the track is already in the basket, a (-) "Remove from basket" control is shown instead.

**Given** no device is selected
**Then** all (+) controls render disabled.

**Given** the active provider supports playlist write
**When** I right-click a track row
**Then** an "Add to playlist…" context menu appears (per Story 11.7).
**And** the track row also renders a visible "Send to playlist…" affordance opening the same flow.

**Given** the active provider does not support playlist write
**Then** both the context menu and the "Send to playlist…" affordance are hidden.

**Given** I am in Tracks mode
**Then** the grid/list view toggle is not displayed (the dual-panel layout is the sole rendering).

**Given** an A–Z letter strip is available on the artist or album panel
**When** I select a letter
**Then** the corresponding panel filters its list and pagination resets.

**Given** I switch away from Tracks mode and back
**Then** the panel selections and scroll positions are restored from the page cache (consistent with other browse modes).

**Technical Notes:**
- New `TracksBrowseView.ts` modeled on `PlaylistCurationView.ts`, but each panel manages its own paginated list state (re-using the autoload-on-scroll logic from `library.ts` artist/album root path).
- `BrowseMode` TS union in `rpc.ts` extended with `"tracks"`.
- New `fetchBrowseTracks(filter)` helper in `rpc.ts`.
- Per-panel state lives in the component, not in the global `library.ts` state, since this view's pagination is multi-axis. Page-cache key: `tracks:${artistId ?? '*'}:${albumId ?? '*'}:${letter ?? '*'}`.
- The grid/list global toggle is read but ignored in this view's renderer; the toggle button is hidden when `state.browseMode === 'tracks'`.
- Track-row right-click context menu re-uses the dispatcher wired in Story 11.7; per-row "Send to playlist…" is a visible button/icon calling the same dispatcher.
- New i18n keys (en/fr/es): `library.mode.tracks`, `tracks.view.all_artists`, `tracks.view.all_albums`, `tracks.view.no_tracks`, `tracks.view.loading`, `tracks.view.send_to_playlist`.
- Depends on Story 9.9.

### Story 9.11: List View Multi-Selection & Bulk Actions

As a Ritualist (Arthur),
I want to select multiple artists or albums in the list view and act on them all at once,
So that I can build my basket or a playlist in seconds instead of clicking every row.

**Acceptance Criteria:**

**Given** the list/table view is active and a row represents an artist or album (resolved type `MusicArtist` or `MusicAlbum`)
**When** the row renders
**Then** it displays a leading selection checkbox (visible on hover/focus, and always visible while any selection is active).

**Given** I click a row's checkbox or Ctrl/Cmd-click the row
**Then** the row's selection toggles without navigating into the item.

**Given** a row is the selection anchor and I Shift-click another row
**Then** all selectable rows between the two indices (inclusive) become selected.

**Given** at least one row is selected
**Then** a bulk action bar appears in the browse area showing the selection count, an "Add to basket" button, an "Add to playlist…" button (only when `supports_playlist_write` is true), and a "Clear" affordance.
**And** all per-row single-item actions continue to work unchanged.

**Given** I click "Add to basket" with N items selected
**Then** items already in the basket are skipped, counts/sizes for the remaining items are fetched in a single batched `jellyfin_get_item_counts` + `jellyfin_get_item_sizes` call pair, each item is added to the basket with existing semantics (artist entity items per Story 3.9), a success toast reports added/skipped counts, and the selection clears.

**Given** I click "Add to playlist…" with N items selected
**Then** the existing playlist picker dialog (Story 11.7) opens; choosing an existing playlist calls `playlist.addItems { playlistId, itemIds: [all N ids] }`, choosing "New playlist" opens the create dialog and calls `playlist.create { name, itemIds: [all N ids] }`; on success the playlists cache is invalidated, a toast confirms, and the selection clears.

**Given** no device is selected (`selectedDevicePath === null`)
**Then** "Add to basket" renders disabled (mirroring per-row (+) behavior); "Add to playlist…" remains available when `supports_playlist_write` is true.

**Given** rows are selected and I scroll far enough that selected rows unmount and remount (virtualization), or autoload appends pages
**Then** selection state is preserved and remounted rows render as selected.

**Given** rows are selected
**When** I change browse mode, drill into an item, change the A–Z filter, toggle to grid view, or press Escape
**Then** the selection and the bulk action bar are cleared.

**Given** keyboard-only navigation
**Then** checkboxes are focusable and toggleable via Space, the bulk bar buttons are reachable in tab order, and the selection count is announced via an ARIA-live region.

**Technical Notes:**
- Selection state in `library.ts` UI state: `selectedIds: Set<string>` + `selectionAnchorIdx: number | null`, keyed by `item.basketId ?? item.id` (same id used by `basketStore.has` in `renderListRow`). Items are looked up from `state.items` at action time — never from the DOM (virtualized rows unmount).
- Selectability predicate: `(item.basketType ?? item.type)` is `MusicArtist` or `MusicAlbum`. Playlist, genre, and track rows do not render checkboxes in v1.
- `renderListRow` renders the checkbox + `is-checked` class from `selectedIds`; the existing `paint()` repaint path makes remounted rows pick up selection state for free.
- Bulk action bar: a sibling of the list scroller in `#library-content` (sticky, above the list), rendered/torn down by the same code path that manages the list (`renderList` / `teardownListScrollHandler`); re-rendered on selection change.
- Bulk basket add reuses the per-row add logic factored out of `renderListRow`'s toggle handler — including the container metadata fetch — but with a single batched `itemIds` array for counts/sizes.
- `MediaCard.openAddToPlaylistDialog(itemIds: string[], label: string)` and `openCreatePlaylistDialog(itemIds: string[], suggestedName: string)` generalize their current single-id signatures; existing callers (context menu, track rows) pass one-element arrays. Daemon-side container→track resolution already exists (Story 11.4) — no RPC changes.
- Cross-server safety: the browse list only ever shows the active server's items, and `playlist.*` RPCs already enforce server scope (409 on cross-server items, Story 11.4 amendment) — no new handling needed.
- New i18n keys (en/fr/es): `library.selection.count`, `library.selection.add_to_basket`, `library.selection.add_to_playlist`, `library.selection.clear`, `library.selection.added_toast`, `library.selection.skipped_suffix`.
- No new daemon RPCs; pure UI concern (same classification as Stories 9.7/9.8).
- Out of scope: grid view multi-select, Tracks dual-panel mode (9.10), Playlist Curation view (11.6), bulk remove from basket.

### Story 9.12: Track Multi-Selection & Bulk Actions

As a Ritualist (Arthur),
I want to select multiple tracks — in an album's track list or in the Tracks browse view — and act on them all at once,
So that I can send a batch of individual songs to my basket or a playlist without clicking every row.

**Acceptance Criteria:**

**Given** the virtualized list view shows track rows (resolved type `Audio`, e.g. tracks within an album)
**When** a row renders
**Then** it displays the same leading selection checkbox as artist/album rows, and all Story 9.11 selection mechanics (Ctrl/Cmd-click, Shift-range, bulk bar, virtualization survival, clearing rules, keyboard/ARIA) apply unchanged to track rows.

**Given** tracks are selected in the list view and I click "Add to basket"
**Then** tracks already in the basket are skipped, the remaining tracks are added using their own `sizeBytes`/`sizeTicks` (no count/size batch RPC for tracks), a toast reports added/skipped counts, and the selection clears.

**Given** the Tracks dual-panel browse view is active
**When** a track row renders in the bottom track panel
**Then** it displays a leading selection checkbox (visible on hover/focus, always visible while any selection is active), alongside the existing per-row (+)/(-) and "Send to playlist…" actions, which continue to work unchanged.

**Given** I click a track row's checkbox or Ctrl/Cmd-click the row
**Then** the row's selection toggles.

**Given** a track row is the selection anchor and I Shift-click another track row
**Then** all track rows between the two indices (inclusive, within the currently loaded track list) become selected.

**Given** at least one track is selected in the Tracks view
**Then** a bulk action bar appears above the track panel showing the selection count (ARIA-live), an "Add to basket" button (disabled when no device is selected), an "Add to playlist…" button (only when `supports_playlist_write` is true), and a "Clear" affordance.

**Given** I click "Add to basket" with N tracks selected in the Tracks view
**Then** tracks already in the basket are skipped, each remaining track is added via `basketStore.add` with its own size metadata, a toast reports added/skipped counts, and the selection clears.

**Given** I click "Add to playlist…" with N tracks selected
**Then** the existing playlist picker dialog opens seeded with all N track ids; existing-playlist and create-new flows behave per Story 9.11, the playlists cache is invalidated on success, a toast confirms, and the selection clears. Cancelling preserves the selection.

**Given** tracks are selected in the Tracks view and autoload appends more pages to any panel
**Then** the selection is preserved (id-keyed).

**Given** tracks are selected in the Tracks view
**When** I change the artist filter, the album filter, or the A–Z letter, leave the Tracks mode, or press Escape
**Then** the selection and the bulk action bar are cleared.

**Given** keyboard-only navigation in the Tracks view
**Then** checkboxes are focusable and toggleable via Space, bulk bar buttons are reachable in tab order, and the selection count is announced via an ARIA-live region.

**Technical Notes:**
- Surface A is a one-line predicate widening: `isSelectableListItem` (`library.ts:665`) accepts resolved type `Audio` in addition to `MusicArtist`/`MusicAlbum`. `addBrowseItemsToBasket` already handles `Audio` outside `CONTAINER_TYPES` (no batch RPC); the bulk playlist handler already passes raw ids.
- Surface B selection state lives in `TracksBrowseView`: `selectedTrackIds: Set<string>` + `selectionAnchorIdx: number | null` indexed into `trackState.items`. Cleared by the same code paths that reset `trackState` (filter changes, mode exit).
- `buildTrackRow` renders the checkbox and a selected-row class; reuse the 9.11 checkbox/bulk-bar CSS (`.media-list-row__check`, `.bulk-action-bar`) — extract shared row-check styles to apply to `.curation-track-row` rather than duplicating rules.
- Bulk handlers in `TracksBrowseView` mirror the 9.11 handlers: basket add maps `BrowseTrack` → `basketStore.add({ id, type: 'Audio', sizeBytes, sizeTicks, … })` (same mapping as the per-row (+) handler, factored out and looped); playlist add calls `MediaCard.openAddToPlaylistDialog(ids, label, onSuccess)`.
- Reuse the existing `library.selection.*` i18n keys (en/fr/es) — no new keys.
- Track panel is append-rendered (not virtualized), so no unmount/remount concerns; keep selection id-keyed anyway for re-render correctness.
- No new daemon RPCs; pure UI concern.

## Epic 10: Device Configuration Editing

Allow existing managed devices to be edited after initialization, including identity and folder configuration. Support separate playlist output folders for devices such as Rockbox players, while preserving managed-file safety and backward compatibility with existing manifests.

### Story 10.1: Device Manifest Editing - Identity and Folder Settings

As a System Admin (Alexis),
I want to edit an existing managed device manifest,
So that I can correct device identity and folder layout without reinitializing the device.

**Acceptance Criteria:**

**Given** a managed device is selected
**When** I open Device Settings
**Then** I can edit name, icon, transcoding profile, music folder, and playlist folder.

**Given** I change only name, icon, or transcoding profile
**When** I save
**Then** no sync relocation is required.
**And** the device hub refreshes without requiring device reconnect.

**Given** I clear playlist folder
**When** I save
**Then** the daemon stores it as null/omitted and resolves it to the music folder.

**Given** I change music folder or playlist folder
**When** I save
**Then** the daemon returns `relocationRequired = true`.
**And** the UI marks the next sync preview as requiring cleanup/resync.

**Given** an invalid folder path is entered
**When** I save
**Then** the daemon rejects absolute paths, parent traversal, root-only unsafe values, and empty path components.

**Given** an MTP device uses cached folder IDs
**When** a folder path changes
**Then** stale folder ID cache entries for affected paths are cleared or recomputed.

**Technical Notes:**
- `DeviceManifest` gains `playlist_path: Option<String>` with `#[serde(default)]`; JSON uses `playlistPath`.
- Add `device.update_manifest` RPC for editable manifest fields, including `transcodingProfileId`.
- All manifest writes use `DeviceIO::write_with_verify()` through `device::write_manifest()`.
- Folder paths must remain device-relative and must not contain absolute roots or parent traversal.
- Name/icon/profile-only updates should not dirty sync state or trigger cleanup.

### Story 10.2: Separate Playlist Folder and Relocation Cleanup

As a Rockbox/DAP user,
I want playlist files to be written to the folder my device expects,
So that native playlist browsing works while HifiMule still manages music safely.

**Acceptance Criteria:**

**Given** a manifest has no `playlistPath`
**When** playlist sync runs
**Then** playlist files are written to `managed_paths[0]` as before.

**Given** `playlistPath` is set
**When** playlist sync runs
**Then** `.m3u` files are written to that folder.

**Given** `playlistPath` differs from the music folder
**When** playlist content is generated
**Then** M3U track entries are relative from the playlist folder to the track files, using forward slashes.

**Given** the music folder changes
**When** sync preview is calculated
**Then** existing manifest-owned tracks outside the new music folder are shown as cleanup/removal before rewrite.

**Given** the playlist folder changes
**When** sync preview is calculated
**Then** existing manifest-owned playlist files outside the new playlist folder are shown as cleanup/removal before rewrite.

**Given** cleanup deletes a manifest-owned file that is already missing
**When** sync cleanup runs
**Then** deletion is treated as successful and the stale manifest entry is removed.

**Given** cleanup would delete more than the configured destructive safety threshold
**When** sync starts
**Then** the UI requires explicit confirmation before sync proceeds.

**Given** unmanaged files exist in old or new folders
**When** relocation cleanup runs
**Then** they are not deleted or modified.

**Technical Notes:**
- Reuse existing idempotent managed-delete behavior from Story 4.10.
- M3U generation must calculate relative paths from `playlistPath` to each track's `local_path`, not assume both live under the same folder.
- Sync preview must distinguish managed relocation cleanup from ordinary content removal.
- Auto-sync must not perform threshold-exceeding relocation cleanup without prior user confirmation.

### Story 10.3: Profile-Based Default Folders and Setup Ordering

As a System Admin (Alexis),
I want device profiles to prefill recommended music and playlist folders before I edit folder paths,
So that Rockbox and Garmin-style devices start with sensible folder layouts without manual typing.

**Acceptance Criteria:**

**Given** `device-profiles.json` contains `defaultMusicFolder` and `defaultPlaylistFolder`
**When** the UI lists device profiles
**Then** `device_profiles.list` returns those default folder fields along with id, name, and description.

**Given** a Rockbox profile is selected during device initialization
**When** the folder fields have not been manually edited
**Then** music folder defaults to `Music` and playlist folder defaults to `Playlists`.

**Given** the Garmin Music Watch profile is selected during device initialization
**When** the folder fields have not been manually edited
**Then** music folder defaults to `Music` and playlist folder defaults to `Music`.

**Given** Device Settings is opened for an existing managed device
**Then** the profile selector appears before music and playlist folder inputs.

**Given** the user changes profile in Device Settings without editing folder fields
**Then** the UI applies the selected profile's folder defaults.

**Given** the user has edited either folder field
**When** the selected profile changes
**Then** the UI preserves the user's folder edits.

**Technical Notes:**
- Extend `DeviceProfileEntry` with optional `default_music_folder` / `default_playlist_folder` mapped to camelCase JSON.
- Built-in defaults: Rockbox profiles use `Music` + `Playlists`; Garmin Music Watch uses `Music` + `Music`.
- Keep `device-profiles.json` user-editable and backward compatible when fields are absent.

## Epic 11: Selection-as-Playlist & Curation

Extend HifiMule with server playlist write-back: users can save their device selection as a server playlist, curate it through a dual-panel artist/album view, and send individual items to playlists via right-click. Backed by a new `MediaProvider` write contract and capability-gated throughout.

### Story 11.1: MediaProvider Playlist-Write Trait Amendment

As a System Admin (Alexis),
I want the MediaProvider trait to expose playlist write operations,
So that the daemon can create, modify, and delete server playlists in a provider-neutral way.

**Acceptance Criteria:**

**Given** a provider is connected
**When** capabilities are queried
**Then** `capabilities().supports_playlist_write` is returned — `true` for Jellyfin and Subsonic/OpenSubsonic.

**Given** the active provider supports playlist write
**When** `create_playlist(name, track_ids)` is called
**Then** the server creates a new playlist with those tracks and returns the server-assigned playlist ID as a `String`.

**Given** the active provider supports playlist write
**When** `add_to_playlist(playlist_id, track_ids)` is called
**Then** the specified tracks are appended to the playlist.

**Given** the active provider supports playlist write
**When** `remove_from_playlist(playlist_id, track_ids)` is called
**Then** the specified tracks are removed from the playlist.

**Given** the active provider supports playlist write
**When** `delete_playlist(playlist_id)` is called
**Then** the playlist is deleted from the server.

**Given** a provider does not support playlist write
**When** any write method is called
**Then** `ProviderError::NotSupported` is returned.

**Technical Notes:**
- Four new methods added to `MediaProvider` in `providers/mod.rs`: `create_playlist`, `add_to_playlist`, `remove_from_playlist`, `delete_playlist`.
- `Capabilities` gains `supports_playlist_write: bool`.
- Callers MUST check `capabilities().supports_playlist_write` before invoking any write method.
- Selection→tracks resolution lives in the daemon RPC layer (Story 11.4), not in the trait.
- No UI changes in this story.
- Tests must cover capability `true` and the `NotSupported` path for both providers.

### Story 11.2: JellyfinProvider Playlist Write Adapter

As a System Admin (Alexis),
I want Jellyfin playlist create/add/remove/delete to work correctly,
So that my Jellyfin server playlists reflect my HifiMule selections.

**Acceptance Criteria:**

**Given** a Jellyfin provider is connected
**When** `create_playlist(name, track_ids)` is called
**Then** `POST /Playlists` is issued with MediaType "Audio" and the name in the request body.
**And** the `Id` field from the response is returned as the playlist ID.

**Given** a Jellyfin provider is connected
**When** `add_to_playlist(playlist_id, track_ids)` is called
**Then** `POST /Playlists/{id}/Items?Ids={comma-separated IDs}` is issued.

**Given** a Jellyfin provider is connected
**When** `remove_from_playlist(playlist_id, track_ids)` is called
**Then** `GET /Playlists/{id}/Items` is called first to resolve the Jellyfin `PlaylistItemId` entries matching the given track IDs.
**And** `DELETE /Playlists/{id}/Items?EntryIds={comma-separated PlaylistItemIds}` removes them.

**Given** a Jellyfin provider is connected
**When** `delete_playlist(playlist_id)` is called
**Then** `DELETE /Items/{id}` is issued (Jellyfin deletes playlists as generic items).

**Given** `supports_playlist_write` is queried for a Jellyfin provider
**Then** it is `true`.

**Technical Notes:**
- All four methods implemented in `providers/jellyfin.rs`.
- `remove_from_playlist` is a 2-step operation: GET to resolve `PlaylistItemId` entries, then DELETE.
- All requests go through the provider's existing authenticated HTTP client.
- Tests mock HTTP responses for each operation and verify correct URL and body construction.

### Story 11.3: SubsonicProvider Playlist Write Adapter

As a System Admin (Alexis),
I want Subsonic playlist create/add/remove/delete to work correctly,
So that my Navidrome/Subsonic server playlists reflect my HifiMule selections regardless of provider.

**Acceptance Criteria:**

**Given** a Subsonic provider is connected
**When** `create_playlist(name, track_ids)` is called
**Then** `GET /rest/createPlaylist.view?name={name}&songId[]={ids}` is issued.
**And** the `id` field from the response is returned as the playlist ID.

**Given** a Subsonic provider is connected
**When** `add_to_playlist(playlist_id, track_ids)` is called
**Then** `GET /rest/updatePlaylist.view?playlistId={id}&songIdToAdd[]={ids}` is issued.

**Given** a Subsonic provider is connected
**When** `remove_from_playlist(playlist_id, track_ids)` is called
**Then** `GET /rest/getPlaylist.view?id={id}` fetches the current track list to resolve song index positions.
**And** `GET /rest/updatePlaylist.view?playlistId={id}&songIndexToRemove[]={indices}` removes them.

**Given** a Subsonic provider is connected
**When** `delete_playlist(playlist_id)` is called
**Then** `GET /rest/deletePlaylist.view?id={id}` is issued.

**Given** any playlist write URL contains Subsonic auth params
**When** the URL appears in logs
**Then** auth params are stripped via `sanitize_subsonic_url()` before logging.

**Given** `supports_playlist_write` is queried for a Subsonic or OpenSubsonic provider
**Then** it is `true`.

**Technical Notes:**
- All four methods implemented in `providers/subsonic.rs`.
- `remove_from_playlist` is a 2-step operation: `getPlaylist` to resolve index positions, then `updatePlaylist`.
- All existing Subsonic auth sanitization rules apply to playlist write URLs.
- Tests cover both classic Subsonic and OpenSubsonic response shapes.

### Story 11.4: Daemon RPCs — playlist.create / addTracks / removeTracks / delete

As a System Admin (Alexis),
I want the daemon to expose playlist management RPCs,
So that the UI can create and edit server playlists from the device selection basket.

**Acceptance Criteria:**

**Given** the active provider supports playlist write
**When** `playlist.create({ name, itemIds })` is called
**Then** the daemon resolves all basket entities (albums, artists, genres, individual tracks) in `itemIds` to a concrete flat track list using the existing container-expansion logic.
**And** Auto-Fill virtual slots (`id: '__auto_fill_slot__'`) are silently excluded from the resolved list.
**And** `provider.create_playlist(name, resolved_track_ids)` is called.
**And** the response returns `{ playlistId: string }` with the server-assigned ID.

**Given** a playlist exists
**When** `playlist.addTracks({ playlistId, trackIds })` is called
**Then** `provider.add_to_playlist(playlistId, trackIds)` is called with the provided track IDs directly (no entity resolution).
**And** `{ ok: true }` is returned.

**Given** a playlist exists
**When** `playlist.removeTracks({ playlistId, trackIds })` is called
**Then** `provider.remove_from_playlist(playlistId, trackIds)` is called.
**And** `{ ok: true }` is returned.

**Given** a playlist exists
**When** `playlist.delete({ playlistId })` is called
**Then** `provider.delete_playlist(playlistId)` is called.
**And** `{ ok: true }` is returned.

**Given** the active provider does not support playlist write
**When** any playlist write RPC is called
**Then** an RPC error indicating the capability is unsupported is returned.

**Given** `playlist.create({ name, itemIds })` is called
**When** the daemon resolves the `itemIds` to track IDs
**Then** it verifies all resolved tracks' `serverId` fields match the current `selectedServerId`.
**And** if any track's `serverId` does not match `selectedServerId`, the RPC returns an error (code 409): "Playlist creation requires all items to be from the selected server. Switch server or remove cross-server items."
**And** no playlist is created on the server.

**Technical Notes:**
- Container-expansion reuses the existing `rpc.rs:807–866` path.
- Auto-Fill slots are silently skipped; callers do not need to pre-filter them.
- Track ordering within a resolved entity is left to implementation.
- All four RPCs call `require_provider()` before dispatch — `require_provider()` routes through `ServerManager.selected_server_id`.
- `playlist.addTracks` and `playlist.removeTracks` pass `trackIds` directly — no entity resolution.
- Server scope validation in `playlist.create`: daemon checks each resolved track's originating `serverId` against `selectedServerId` before calling `provider.create_playlist`; 409 error prevents partial cross-server playlists.

### Story 11.5: Basket "Save as Playlist" and "Send to Playlist" UI

As a Ritualist (Arthur),
I want to save the basket selection as a server playlist and send items to playlists from browse views,
So that I can persist and reuse my curated selections across sessions.

**Acceptance Criteria:**

**Given** the active provider supports playlist write and the basket is non-empty
**When** the basket header is visible
**Then** a "Save selection as playlist" action is shown.

**Given** the active provider does not support playlist write
**Then** the "Save selection as playlist" action is hidden.

**Given** I click "Save selection as playlist" and the basket contains only manual selections
**When** the dialog opens
**Then** I can enter a name to create a new server playlist.

**Given** I click "Save selection as playlist" and the basket contains an Auto-Fill slot
**When** the dialog opens
**Then** an inline notice informs me that Auto-Fill tracks are resolved at sync time and will not be saved to the playlist.
**And** I can still proceed to save the manual selections.

**Given** I right-click an artist or album in a browse view
**Then** a context menu appears with a "Send to playlist…" option.

**Given** I select "Send to playlist…" from a context menu
**Then** I can create a new playlist with that item as the initial content.

**Given** I confirm playlist creation
**Then** `playlist.create` is called with the resolved item IDs.
**And** the created playlist becomes available in the server playlist browser.

**Given** I click "Save selection as playlist" and the basket contains items from non-selected servers
**When** the dialog opens
**Then** an inline notice informs me: "Only items from [selected server name] will be saved. Items from other servers are not included."
**And** the item count shown in the dialog reflects only the selected-server items.
**And** I can proceed; cross-server items are silently pre-filtered from the `itemIds` sent to `playlist.create`.

**Technical Notes:**
- Capability-gating: check `capabilities().supports_playlist_write` before rendering any playlist-write affordances.
- Auto-Fill exclusion notice is informational only; the save proceeds for manual items.
- Context menus appear on artists and albums in browse views.
- Cross-server pre-filter: UI filters `basketStore` items to `serverId === selectedServerId` before building the `itemIds` array for `playlist.create` — this prevents the 409 error from surfacing in normal flow.

### Story 11.6: Dual-Panel Playlist Curation View

As a Ritualist (Arthur),
I want a dual-panel view for curating server playlists,
So that I can remove specific artists or albums from a playlist without rebuilding it from scratch.

**Acceptance Criteria:**

**Given** a server playlist is selected for curation
**When** I open the curation view
**Then** the left panel shows all artists who have tracks in the playlist.
**And** selecting an artist shows that artist's albums filtered to only those with tracks in the playlist in the right panel.

**Given** I click "Remove artist" in the left panel
**Then** all tracks by that artist are removed from the playlist via `playlist.removeTracks`.
**And** the artist disappears from the left panel.

**Given** I click "Remove album" in the right panel
**Then** all tracks in that album are removed from the playlist via `playlist.removeTracks`.
**And** the album disappears from the right panel.
**And** if that artist has no remaining tracks in the playlist, the artist also disappears from the left panel.

**Given** the curation view is open
**Then** a statistics header shows total track count, total duration, and total storage size.

**Given** some tracks in the playlist have no `sizeBytes` value
**When** the storage size statistic is displayed
**Then** those tracks are excluded from the size total.

**Given** I close the curation view
**Then** the server playlist reflects all removals made during the session.

**Given** an artist is selected in the left panel
**When** the curation view renders or updates
**Then** a track panel below the artist/album panels shows all tracks by that artist that are in the playlist.
**And** each track row shows the track title, duration, and a "Remove track" button.

**Given** I click on an album row in the right panel (not the remove button)
**Then** the album is highlighted as focused.
**And** the track panel filters to show only tracks from that album that are in the playlist.

**Given** I click "Remove track" on a track in the track panel
**Then** that single track is removed from the playlist via `playlist.removeTracks`.
**And** the track disappears from the track panel.
**And** if the artist has no remaining tracks in the playlist, the artist disappears from the left panel.
**And** the statistics header updates.

**Technical Notes:**
- The view fetches the current playlist via `browse.getPlaylist` to build initial curation state.
- Artist and album grouping is derived from `Track.artistName` and `Track.albumName` in the playlist response.
- Storage size uses `Track.sizeBytes`; tracks without a value are excluded from the total without error.
- This view edits the server playlist only — it does not trigger a device sync.

### Story 11.7: Add Tracks to Playlist — Browse Context Menu & Curation View

As a Ritualist (Arthur),
I want to add individual tracks to a server playlist from browse views and from within the curation view,
So that I can build and expand playlists track-by-track without needing to create a new playlist each time.

**Acceptance Criteria:**

**Given** the active provider supports playlist write
**When** I right-click an individual track row in any browse view
**Then** an "Add to playlist…" option appears in the context menu.

**Given** I select "Add to playlist…" from a track's context menu
**When** the sub-menu or dialog opens
**Then** a list of existing server playlists is shown alongside a "New playlist…" option.

**Given** I select an existing playlist from the "Add to playlist…" dialog
**Then** `playlist.addTracks({ playlistId, trackIds: [track.id] })` is called.
**And** a success notification is shown confirming the track was added.

**Given** I select "New playlist…" from the "Add to playlist…" dialog
**Then** I am prompted for a playlist name.
**And** `playlist.create({ name, itemIds: [track.id] })` is called.
**And** the created playlist becomes available in the server playlist browser.

**Given** the active provider does not support playlist write
**Then** the "Add to playlist…" context action is hidden on track rows.

**Given** the curation view is open for a playlist
**When** I click the "Add tracks" button in the statistics header
**Then** a search dialog opens that accepts a query (title, artist, or album).

**Given** I enter a query in the "Add tracks" search dialog
**Then** matching tracks from the library are displayed as a selectable list.

**Given** I select one or more tracks in the search dialog and confirm
**Then** `playlist.addTracks({ playlistId, trackIds: selectedIds })` is called.
**And** the curation view re-fetches the playlist and re-renders all panels.
**And** the statistics header updates to reflect the new track count, duration, and storage size.

**Given** I open the "Add tracks" search dialog and cancel without selecting
**Then** no RPC is called and the curation view is unchanged.

**Technical Notes:**
- `playlist.addTracks` and `playlist.create` RPCs are already specced in Story 11.4 — no new daemon work.
- No provider changes needed; Jellyfin and Subsonic adapters already implement `add_to_playlist`.
- The track context menu reuses the same `supports_playlist_write` capability gate as Story 11.5.
- The "Add tracks" search dialog should call an existing browse RPC (e.g., `browse.getTracks` or equivalent)
  filtered by the user's query — no new daemon endpoint needed if a track search RPC exists.
- The curation view refresh after adding reuses the existing `fetchPlaylist()` → `render()` cycle.
- Track context menus apply wherever track rows are rendered: album detail views, artist track listings,
  and server playlist browse views.

### Story 11.8: Playlist Rename and Delete — Curation View Header

As a Ritualist (Arthur),
I want to rename and delete a playlist directly from the curation view,
So that I can manage my library's playlist catalogue without leaving the edit context.

**Acceptance Criteria:**

**Given** the curation view is open for a playlist
**When** I click the playlist name in the header
**Then** the name becomes an inline `<sl-input>` pre-filled with the current name.
**And** Save and Cancel affordances appear alongside the input.

**Given** the inline name input is open
**When** I edit the name and click Save
**Then** `playlist.rename({ playlistId, name: newName })` is called.
**And** the header title updates to the new name.
**And** the input is dismissed.

**Given** the inline name input is open
**When** I press Escape or click Cancel
**Then** the input is dismissed with no RPC call.

**Given** the active provider supports playlist write
**When** the curation view renders
**Then** a delete icon-button (trash) is visible in the header.

**Given** I click the delete icon-button
**Then** an `<sl-dialog>` opens showing the playlist name and asking for confirmation.

**Given** the confirmation dialog is open and I confirm
**Then** `playlist.delete({ playlistId })` is called.
**And** the UI navigates back to the playlist browser.

**Given** the confirmation dialog is open and I cancel
**Then** the dialog closes with no RPC call.

**Given** the active provider does not support playlist write
**Then** the delete icon-button is hidden.

**Technical Notes:**
- `rename_playlist(id, new_name)` is a new method on the `MediaProvider` trait in `providers/mod.rs`.
- JellyfinProvider: 2-step — `GET /Users/{uid}/Items/{id}` to fetch current item JSON, update `Name`, then `POST /Items/{id}` with the full body.
- SubsonicProvider: single-step — `GET /rest/updatePlaylist.view?playlistId={id}&name={encoded_name}`.
- Daemon: new `playlist.rename({ playlistId, name })` RPC handler calling `provider.rename_playlist`.
- Frontend: editable name state in `PlaylistCurationView.ts`; delete affordance reuses the existing `sl-dialog` pattern from Story 11.5's "Save as playlist" flow.
- `playlist.delete` RPC from Story 11.4 is reused unchanged.
- New i18n keys: `playlist.curation.rename_save`, `playlist.curation.rename_cancel`, `playlist.curation.delete_title`, `playlist.curation.delete_body`, `playlist.curation.delete_confirm`, `playlist.curation.delete_cancel_btn`.

### Story 11.9: MediaProvider Reorder Contract — Trait, Adapters & RPC

As a Ritualist (Arthur),
I want the daemon to reorder tracks in a server playlist,
So that my curated playlist plays in the sequence I intend.

**Acceptance Criteria:**

**Given** the active provider supports playlist write
**When** `reorder_playlist(playlist_id, ordered_track_ids)` is called
**Then** the playlist's tracks are set to exactly that order (same track set, only the sequence changes).

**Given** a Jellyfin provider
**When** `reorder_playlist` is called
**Then** current playlist entries are fetched, and `POST /Playlists/{id}/Items/{playlistItemId}/Move/{index}` is issued per out-of-place entry until the playlist matches `ordered_track_ids`.
**And** no entries are removed or re-created (item identity is preserved).

**Given** a Subsonic/OpenSubsonic provider
**When** `reorder_playlist` is called
**Then** `createPlaylist?playlistId={id}&songId=…` is issued with the song IDs in the requested order, replacing the playlist contents in that order.

**Given** a provider that does not support playlist write
**When** `reorder_playlist` is called
**Then** `ProviderError::NotSupported` is returned.

**Given** the daemon receives `playlist.reorder({ playlistId, trackIds })`
**Then** it verifies `supports_playlist_write`, calls `reorder_playlist`, and returns success; capability-absent requests are rejected.

**Technical Notes:**
- New `reorder_playlist` method on `MediaProvider` (`providers/mod.rs`); default impl returns `NotSupported`.
- Jellyfin (`providers/jellyfin.rs`): reuse `get_playlist_items` to map track_id→PlaylistItemId; selection-sort with Items/Move. Move count is O(n); acceptable for typical playlist sizes.
- Subsonic (`providers/subsonic.rs`): new client call sending `createPlaylist` with existing `playlistId` + ordered `songId` params.
- Daemon RPC (`rpc.rs`): `"playlist.reorder" => handle_playlist_reorder`; params `{ playlistId, trackIds }`.
- Reuses existing `supports_playlist_write` capability — no new capability flag.
- Tests: order-set correctness for both adapters + `NotSupported` path.

### Story 11.10: Curation View — Complete Track List, Order Numbers & Reorder

As a Ritualist (Arthur),
I want to see my whole playlist in order and nudge tracks up or down,
So that I can fine-tune track sequence directly in the editor.

**Acceptance Criteria:**

**Given** the curation view is open
**When** the artist panel renders
**Then** an "All artists" entry appears at the top; selecting it shows every playlist track in the track panel, in playlist order.
**And** an "All albums" entry lets a selected artist's tracks all show together.

**Given** the track panel renders
**Then** each track row shows its absolute 1-based position (#N) in the full playlist, regardless of any active artist/album filter.

**Given** the active provider supports playlist write
**When** the track panel renders
**Then** each row shows up (↑) and down (↓) controls; ↑ is disabled on the first visible row and ↓ on the last visible row.

**Given** I click ↑ or ↓ on a track
**Then** it swaps playlist position with the previous/next currently-visible track.
**And** the change is applied optimistically and persisted via `playlist.reorder` with the full reordered track-id list.
**And** the #N order numbers update to reflect the new sequence.

**Given** the reorder RPC fails
**Then** an inline error is shown and the prior order is restored.

**Given** the provider does not support playlist write
**Then** the up/down controls are hidden (order numbers still shown).

**Technical Notes:**
- `PlaylistCurationView.ts`: allow `selectedArtist = null` ("All"); precompute id→index map for #N; ↑/↓ swap within `getTracksForPanel()` neighbors against full `this.tracks`; `isReordering` guard; reuse `#curation-error` + optimistic-update pattern from `doRemove`.
- New i18n keys: `playlist.curation.all_artists`, `playlist.curation.all_albums`, `playlist.curation.move_up`, `playlist.curation.move_down` (× en/fr/es).
- New rpc.ts helper or direct `rpcCall('playlist.reorder', …)`.

## Epic 12: Configurable Auto-Fill Pipeline & Multi-Server Auto-Fill

Replace the fixed single-server auto-fill algorithm (Stories 3.6/3.8) with a configurable, composable selection pipeline (Source × Strategy), and let auto-fill be defined independently per media server. This epic delivers the foundation plus MVP strategies (playlist/tag sources, budget). It builds on the shipped multi-server foundation (Epics 2/8 — `ServerManager`, portable `server_id`, `get_provider_by_server_id`) and the provider capability contract (Epic 9). MVP scope is unaffected — this is an additive Growth feature. Source: [sprint-change-proposal-2026-06-14-configurable-auto-fill.md](sprint-change-proposal-2026-06-14-configurable-auto-fill.md), from brainstorming-session-2026-06-12-1.

**Unifying model:** an auto-fill definition is one pipeline config per `(device, portable serverId)` pair. Pipeline **configuration** lives in the device manifest (portable); transient **runtime state** (cooldown windows, stable-core, pity-timer) lives in the daemon DB keyed by device+server. Today's hardcoded algorithm becomes the default single-Ordering-stage pipeline, so existing devices behave unchanged with zero migration.

### Story 12.1: Auto-Fill Pipeline Domain Model & Pure-Function Engine

As a developer,
I want the auto-fill pipeline expressed as pure functions over a `MediaProvider` (filter → source → unit → order → dedupe-vs-memory → budget),
So that selection logic is testable without UI or network and validated against real user needs before any UI is built.

**Acceptance Criteria (to be expanded by create-story):**
- `run_pipeline` implemented as composable pure functions over a provider's library.
- Model validated against the 4 brainstorm personas (Claire/Antoine/Léo/Nadia) with no special cases ("four personas, one model").
- Full unit-test coverage; no UI changes.

### Story 12.2: Auto-Fill Manifest Schema & DB History Scaffolding

As a developer,
I want `manifest.autoFill` to become `Map<serverId, AutoFillPipeline>` and a daemon DB `autofill_history` table to exist,
So that per-server pipeline config persists portably and strategies have a place to store runtime state.

**Acceptance Criteria (to be expanded by create-story):**
- Manifest `autoFill` becomes a per-server map; legacy `{ enabled, maxBytes }` is read as the default pipeline (backward compatible, migration-free).
- New daemon DB `autofill_history` table (schema only; consumed by Epic 13).

### Story 12.3: Multi-Slot Sync-Time Expansion & Lift Single-Slot Limit

As a multi-server user,
I want each server's auto-fill slot to expand against its own server at sync time,
So that I can define auto-fill for several servers at once instead of one overwriting another.

**Acceptance Criteria (to be expanded by create-story):**
- `sync.start` accepts an array of per-server auto-fill descriptors; daemon runs `run_pipeline` per slot via `get_provider_by_server_id`.
- Manual items win dedup; single global `run_auto_fill` is replaced.
- Multiple slots (one per server) coexist; enabling auto-fill on one server never removes another's slot.

### Story 12.4: PlaylistSource, Tag/Genre Filter & Per-Source Shares

As a curator,
I want to draw fills from specific playlists and pre-filter by genre/tag, blending multiple sources by share,
So that I can express "70% from 2 playlists, 30% library remainder, no Christmas music."

**Acceptance Criteria (to be expanded by create-story):**
- First-class `PlaylistSource` and Filter (tag/genre include-exclude) stages.
- Per-source share blending; capability-gated on genre/playlist enumeration.

### Story 12.5: Budget Stage — Headroom Reserve, Duration Target & Fallback Chain

As a user who trusts the tool with my whole device,
I want size/duration budgets with a headroom reserve and a guaranteed full fill,
So that no fill exceeds capacity minus reserve and the target is always reached.

**Acceptance Criteria (to be expanded by create-story):**
- Headroom reserve; duration-as-budget (bytes derived); terminal fallback chain guaranteeing the target.

### Story 12.6: Auto-Fill Configuration UI & Coexisting Multi-Server Slot Cards

As a user,
I want a pipeline-builder configuration surface and a separate auto-fill slot card per server,
So that I can configure and see each server's fill independently in one basket.

**Acceptance Criteria (to be expanded by create-story):**
- Pipeline-builder panel (stage sections + Advanced disclosure + Default simple state) replacing the toggle+slider.
- One slot card per server; non-selected-server slots render read-locked per the existing pattern.

### Story 12.7: Auto-Fill RPC/State Contract & i18n

As a developer,
I want the daemon contract and UI state wired for per-server pipelines,
So that config persists and is exposed consistently.

**Acceptance Criteria (to be expanded by create-story):**
- `autoFill.setPipeline`, `basket.autoFill` (+serverId), `get_daemon_state` exposes per-server configs.
- `autoSyncOnConnect` stays server-independent; en/fr/es i18n keys added.

## Epic 13: Advanced Auto-Fill Strategies

The delight and depth layers from the brainstorm catalog, built on the Epic 12 pipeline and DB-history scaffolding. Each strategy is an additive Picker/modifier; every smart strategy is offered alongside its cheap playlist/tag-based equivalent (ambition tiers). Source: [sprint-change-proposal-2026-06-14-configurable-auto-fill.md](sprint-change-proposal-2026-06-14-configurable-auto-fill.md). Consciously cut (not deferred): listener profiles (#22), smart refill triggers, skip-based negative feedback.

### Story 13.1: Memory & Rotation Strategies

Sync cooldown (#4), played-track exclusion (#5), stable-core + delta (#24), rotation tiers / playlist-backed tiers (#25/#26), repeat-tolerance dial (#23). DB-history backed.

### Story 13.2: Quality & Version Ordering

Best-version resolution (#11), quality-ordering modifier (#13), version preference (#34).

### Story 13.3: Curation & Discovery Sources

Deep-cuts excavator (#14), acclaimed-classics (#16), community-rating fallback (#15), musical-memories cheap version (#31).

### Story 13.4: Delight — Rarity Draws & Pity Timer

Weighted rarity draws (#29), pity timer (#30).

### Story 13.5: Context & Encoding-From-Goals

Time-of-day (#3), energy-curve (#17), seasonal drift cheap version (#32); encoding computed from size/duration goals (#20, depends on transcode-on-sync).

### Story 13.6: Advanced Units & Promotion

Artist Spotlight (#33), album/track space ratio (#8), affinity-triggered album promotion (#9), coherence-optimized fill (#27).


## Epic 14: Sync Throughput Pipeline

Existing epic, tracked in [its approved sprint change proposal](sprint-change-proposal-2026-07-11-sync-throughput-pipeline.md) and [sprint-status.yaml](../implementation-artifacts/sprint-status.yaml). Its implementation stories 14.1–14.3 and existing statuses are unchanged.

## Playback Extension — Requirements Inventory

Status: requirements and the Epic 15/16 split approved on 2026-09-19; completed foundation retained, new and moved work remains backlog. The completed baseline above remains intact. PRD FR20 and FR33 are amended by the playback lifecycle and always-available destination rules. Existing security, accessibility, device-integrity and sync requirements remain inherited constraints.

### Functional Requirements

- **FR55:** Playback is an always-present destination listed first, with independent source/selection settings and a manually editable queue. Select it when no physical device is connected; do not expose storage, folder or file-sync actions for Playback.
- **FR56:** Music and native media controls remain operational after the main UI closes. Reopening the UI shows the same session and current state without restarting playback.
- **FR57:** Play something starts a fresh Radio using the first eligible result from the shared sync/playback selection engine under Playback settings. Resume continues the existing queue and position. Both actions are available from the desktop app menu without opening the main window when configured sources are available.
- **FR58:** Users can control play/pause, stop, Back (restart the current track or return to the previous track), next, playback position where supported, and output selection. Back restarts after three seconds of main-track playback and selects the previous occurrence at or before three seconds; without a previous occurrence it restarts the current track. During Preview it restarts the audition without altering the preserved main session. UI and supported native Previous/keyboard controls use the same session command, preserving paused intent and output safety. Capability limits and recoverable errors are visible rather than silently ignored.
- **FR59:** A translucent floating playback bar below the media browser remains visible while idle, browsing or working with a device basket, and offers Play something when idle. The bar does not obscure the final list items. Selecting a different server/device does not stop music or change its source; arriving physical devices become selected and unconfigured devices retain a visible setup action. Show detected-device open failures with actionable feedback.
- **FR60:** Full application quit preserves queue, position and logical-session exclusions. Relaunch restores paused. Quit during sync stops audio and requests orderly sync cancellation without marking incomplete device writes successful.
- **FR61:** Album playback follows disc/track order and preserves the intended continuity, including silence recorded in the source. Track boundaries introduce no additional gap when both tracks are prepared and their tested formats are supported. No automatic crossfade or silence removal is implied.
- **FR62:** An explicit Preview action, distinct from Play, starts a full-track audition while preserving the main session and position. Another audition replaces the audition, not the preserved main session. Provide an explicit return-to-session action; exact button/context-menu placement is settled in the UI story.
- **FR63:** Natural audition completion restores the main session's previous playing/paused state and position. Explicit audition Stop restores it paused. With no main session, completion leaves playback idle. Failed resumption preserves the session and presents a recoverable error.
- **FR64:** Loss of the selected output pauses playback without automatically sending music to another output. Reconnection leaves playback paused until resumed. Shared output is the default; OS Do Not Disturb remains managed by the OS.
- **FR65:** Radio continually replenishes a limited upcoming queue until stopped or no eligible music is available. It uses the same selection engine as sync with separate settings; it does not enqueue the whole library.
- **FR66:** Radio stays with the current artist while eligible unheard tracks remain, then moves to a meaningfully connected artist using available relationship evidence stronger than shared genre alone. The transition reason is explainable.
- **FR67:** When no meaningful artist connection is available, Radio chooses a fresh center using the original Playback settings and labels it as a new starting point. When all eligible tracks have been heard, begin another listening cycle while preserving exclusions; when none are eligible, show an explained waiting state.
- **FR68:** Users can reorder and remove queued tracks. Automatic replenishment appends without reordering existing entries. Removed automatic suggestions remain excluded for the logical session; stale concurrent edits cannot overwrite newer queue changes.
- **FR69:** Skips exclude tracks for the current logical Radio session, including across restoration. A new Radio resets those exclusions. HifiMule does not accumulate a cross-session taste-learning profile.
- **FR70:** Radio may draw from multiple configured sources. Copies confidently identified as the same recording count as one Radio selection, while distinct performances and uncertain matches remain separate. Playback and server actions retain the chosen source identity regardless of the browsed server.
- **FR71:** Playback requests the highest sustainable available quality independently of portable-device transcoding preferences. A brief startup buffer is allowed. Buffer/refill behavior informs automatic quality reduction and conservative recovery, with a discreet explanation of reduced quality.
- **FR72:** Initial quality changes occur at track boundaries. Mid-track replacement is enabled only for provider/format combinations validated for correct resume timing. If no available representation can be sustained, expose buffering/retry rather than promise uninterrupted playback.
- **FR73:** For an unavailable source, Radio may continue with another eligible track; album playback pauses and retries without silently omitting tracks. A technical failure is not treated as a user skip or dislike.
- **FR74:** Radio uses track-level loudness matching; album playback uses consistent album gain to preserve relative levels. Apply peak protection. Leave gain unchanged when usable metadata is absent; automatic dynamic-range compression is not required.
- **FR75:** Playback and device sync normally run together. Reduce sync demand only when needed to protect playback. Switching, skipping or seeking cannot allow obsolete buffered content to become the new session's audio.
- **FR76:** Distinguish currently-playing status from a completed listen. Avoid counting skipped/interrupted tracks where provider semantics allow; completed auditions count where supported. Do not promise reversal of server-counted plays. Listening eligibility and reporting behavior must be verified for supported providers before enabling reports.
- **FR77:** Explicit Like/Dislike persists on the source server only when a genuine supported equivalent exists. Unsupported actions are unavailable; removing a favorite is not silently mapped to dislike. No local-only durable taste state is created as a fallback.
- **FR78:** Users can save an immutable snapshot of accepted played occurrences, the current occurrence once unless rejected, and upcoming occurrences, omitting skipped/disliked entries. Further playback does not alter the saved snapshot; deliberate repeated queue occurrences remain distinct.
- **FR79:** Saving a mixed-source snapshot creates one playlist per contributing capable server, preserving relative order within each part. Display per-server successes, unsupported parts and failures; retries must not blindly duplicate already-created playlists. The complete cross-server order remains local.
- **FR80:** With a connected physical target, users can explicitly Add the snapshot to its basket or Replace its basket. Without a physical target these actions are unavailable. Sending music to a basket does not itself start synchronization or establish a live link to Radio.
- **FR81:** Maintain recoverable reporting/export operation state across interruption. Reconcile ambiguous results before repeating non-idempotent writes; do not promise exactly-once remote effects where server APIs cannot support them.

### NonFunctional Requirements

- **P-NFR1 — Continuity:** Prepared supported album boundaries introduce zero extra decoded samples of silence or omitted source samples in deterministic fixtures. Physical-output continuity and real-sync coexistence require separate release tests; callback counters alone are insufficient.
- **P-NFR2 — Resource bounds:** Bound upcoming entries, compressed prefetch, decoded PCM and candidate caches separately. Long-session history is paged rather than held as an ever-growing UI list. Measure idle and active memory separately; retain the existing idle target without extending it to active playback. Numeric active budgets and buffer thresholds must be set in the owning implementation story before performance acceptance.
- **P-NFR3 — Cross-platform operation:** Validate installed builds on every shipping architecture, including UI-close/reopen, native transport, output loss, sleep/wake and safe shutdown. Test physical media-key routing separately from OS API delivery.
- **P-NFR4 — State integrity:** Restore only valid persisted state; reject stale queue mutations; prevent obsolete asynchronous work from changing the current session. Failed restoration or preview return must preserve recoverable listening state and explain the problem.
- **P-NFR5 — Privacy:** Use existing source-server credentials and capabilities. Do not expose authenticated stream URLs in the UI or logs. Initial relationship enrichment uses configured-server metadata; no third-party listening or metadata service is introduced.
- **P-NFR6 — Usability/accessibility:** Playback remains keyboard-operable with visible focus, accessible control names and status changes, consistent with existing WCAG 2.1 AA targets. Floating controls and background updates must not steal focus or make bottom rows unreachable.

### Additional Requirements

- P-AR1: Extend the existing brownfield workspace; no starter scaffold or replacement of completed sync epics. Playback runs in one signed-in-user Rust daemon independently of UI lifetime on Windows, macOS and Linux.
- P-AR2: Before lifecycle implementation, specify single-instance ownership, authenticated local command access, startup coordination and shutdown deadlines; reuse the desktop event loop and preserve safe sync cancellation.
- P-AR3: Before dependent implementation, define versioned command/event/snapshot schemas, wire units, occurrence/source/recording identities, queue revisions, bounded command-ID retention and reconnect recovery.
- P-AR4: Persist versioned session/history in SQLite with migrations; store validated Playback settings locally, independently of portable-device manifests. Restore paused and page history; keep persistence outside audio callbacks.
- P-AR5: Use the tested native FFmpeg and CPAL approach with a controlled packaged runtime. Resolve exact dependencies, licensing/distribution and installed-build validation for every shipping architecture before release; ARM64 VM evidence is not x64 certification.
- P-AR6: Maintain separate bounded compressed and PCM buffers, continuous output and next-track preparation. No allocation, blocking IO or sync locks in the output callback; fence obsolete asynchronous work after seek, skip or session changes.
- P-AR7: Preserve recorded silence and trim only known codec padding; define channel/rate conversion and loudness/peak behavior with deterministic fixtures plus physical-output tests.
- P-AR8: Extract one pure selection engine for sync and Radio with separate consumer contexts; carry explicit portable server identities, confident recording deduplication and independent queue occurrence IDs.
- P-AR9: Extend provider capabilities for source-routed authenticated streaming, quality alternatives, seek, reporting, genuine feedback and playlist writes. Keep credentials and authenticated URLs daemon-side; use configured-server relationship metadata only.
- P-AR10: Serialize session commands; queue revisions must not advance merely for progress. Publish authoritative snapshots/events and permit UI position interpolation without a second player state owner.
- P-AR11: Implement immutable per-server export operations with partial results, durable recovery and ambiguity reconciliation before retrying non-idempotent writes. Resolve physical-target changes before basket mutation.
- P-AR12: Before affected acceptance, measure resource/streaming thresholds, reporting eligibility and source quality ranking. Initial adaptation switches at boundaries; enable mid-track resume only for validated provider/format pairs.
- P-AR13: Deliver Epic 15 lifecycle/player/albums/manual UI, Back and compact browse navigation, then packaged manual verification (15.17); follow with Epic 16 Radio, reporting/exports, adaptation and sustained installed validation (16.14). Cross-platform checks and failure handling accompany every stage.
- P-AR14: Validate real sync coexistence with conditional backoff, sustained memory, sleep/wake, output loss, installed lifecycle and native controls. Distinguish API command delivery from physical key routing and callback counters from audible gaplessness.

### UX Design Requirements

- P-UX-DR1: List Playback first and retain it with no physical device. Device arrival selects its basket and exposes setup for blank devices, while preserving the listening session; display open/detection failures.
- P-UX-DR2: Add a translucent floating playback bar under the media browser across views, including idle Play something. Include Back with restart/previous behavior and supported native Previous/keyboard delivery. Reserve reachable space for final rows; do not intercept their controls or steal focus.
- P-UX-DR3: Provide separately named Play and Preview actions; settle preview button/context-menu placement near existing curation affordances. Show audition state and an explicit return to the preserved session.
- P-UX-DR4: Expose Play something and Resume in the desktop menu without requiring the main window; distinguish fresh Radio from resumed state and provide actionable missing-setup feedback.
- P-UX-DR5: Provide an editable Playback queue with occurrence-based reorder/removal, bounded upcoming entries and paged history; append Radio suggestions without resetting user order or selection.
- P-UX-DR6: Provide Playback-specific source/selection settings using suitable existing auto-fill controls; omit physical storage/capacity/manifest settings and do not share device preference values.
- P-UX-DR7: Communicate artist transition reasons, fresh-center fallback, exhausted eligibility, buffering, reduced quality, output loss and recoverable errors with accessible statuses.
- P-UX-DR8: Show Like/Dislike and server-save actions only for genuine provider capabilities; report split-server export success/failure/unsupported parts individually and preserve retry context.
- P-UX-DR9: Offer explicit Add to basket and Replace basket only with a connected physical target; preserve source identity indicators and prevent writes to a changed or unavailable target.
- P-UX-DR10: Reuse Shoelace tokens, purple #52348B, amber #EBB334, midnight #1A1A2E, Outfit headings, Inter data and glass overlays; integrate with the established library/basket layout.
- P-UX-DR11: Preserve virtualized grid/list browsing, no-refetch view toggles, identity-based multi-selection, Ctrl/Cmd/Shift ranges, Escape clearing and accessible sticky selection counts when adding playback actions.
- P-UX-DR12: Adapt controls and queue to existing narrow (<600px), medium (600–1000px) and wide (>1000px) layouts; retain control labels/targets and avoid overlap with the basket.
- P-UX-DR13: Provide keyboard operation, including Back and compact icon-and-small-label browse navigation, visible focus, accessible names and ARIA-live status updates consistent with WCAG 2.1 AA; verify with accessibility tooling and OS-theme visual checks.
- P-UX-DR14: Preserve capability-driven navigation, server names/icons and source badges. Browsing a different server must not reroute playback or its reporting/feedback.
- P-UX-DR15: Keep existing live server-playlist editing semantics distinct from local Radio and explicit snapshot export. Existing no-device placeholders and device-only hub visibility are superseded only where Playback requires availability.

### FR Coverage Map

Epics 15 and 16 jointly retain FR55–81, P-NFR1–6, P-AR1–14 and P-UX-DR1–15. Story 15.16 additionally refines FR8 navigation. See [playback-epic-validation.md](playback-epic-validation.md) for fully qualified story mappings. A first release does not imply completion of the entire extension.

| Requirements | Delivery ownership |
|---|---|
| FR8 | Existing browsing; 15.16 compact navigation, verified in 15.17. |
| FR55, FR59 | 15.12–15.14 manual destination/queue/bar; 16.1 selection settings and 16.6 idle Play something. |
| FR56, FR58, FR64 | Epic 15 lifecycle, native transport, Back (15.15) and output safety. |
| FR60 | Epic 15 safe Quit and paused restoration; 16.2–16.3 Radio identity/exclusions. |
| FR57, FR65–70 | Epic 16 Radio; Epic 15 supplies Resume, manual queue and source identity. |
| FR61–63 | Epic 15 albums and auditions. |
| FR71–72, FR75 | Epic 15 initial selection, audio bounds, stale-work fencing and ordinary coexistence; 16.12–16.14 adaptation/backoff and sustained validation. |
| FR73–74 | Epic 15 album retry/gain; 16.2 Radio failure handling and 16.5 track gain. |
| FR76–81 | 16.7–16.11 reporting, preferences and immutable exports. |

### Quality, Architecture and UX Coverage

Every relevant story retains platform, state-integrity, privacy and accessibility checks. Epic 15 establishes daemon ownership, schemas, occurrence/source identities, native runtime, album continuity, manual queue and bar usability. Story 15.15 extends transport; 15.16 refines compact browsing. Story 15.17 verifies the installed manual release, including measured resources and ordinary real-sync coexistence.

Epic 16 extends the same contracts with bounded selection/Radio, recording identity, provider reporting/preferences, immutable exports, quality adaptation and conditional backoff. Story 16.14 owns sustained and installed validation of those additions. P-AR8/P-AR11 and the Radio/export portions of UX requirements remain Epic 16 work. See the detailed coverage table for every P-NFR, P-AR and P-UX owner.

### Epic List

#### Epic 15: Desktop Playback

Deliver daemon-owned manual desktop listening on Windows, macOS and Linux: paused restoration, selected-track/album playback, output safety, continuity/gain, auditions, editable queue, persistent bar with Back, compact browse navigation, and verified installed packages.

**Dependencies:** Existing sync/provider product. No dependency on Epic 16.

**Sequence:** completed foundation 15.1–15.14; Back 15.15; compact browse navigation 15.16; manual release packaging 15.17. Story 15.16 has no technical dependency on Back but is scheduled before packaging.

#### Epic 16: Radio/Recommendations

Deliver automatic selection, bounded Radio, explainable artist transitions, recording deduplication and track loudness, followed by source-server reporting/preferences, immutable snapshots/exports, adaptive quality, conditional sync protection and sustained installed verification.

**Dependencies:** Delivered Epic 15 foundation and existing auto-fill engine. Sequence: Radio 16.1–16.6; reporting/preferences/exports 16.7–16.11; adaptation/protection/validation 16.12–16.14.

**Implementation gates:** Close each story's applicable schemas, provider semantics, resource budgets and runtime/platform contracts before implementation or acceptance. Later verification never substitutes for earlier checks. No dead controls are exposed for unimplemented features.

### Grouping Rationale and Approval State

Alexis approved the 2026-09-19 course correction after Story 15.14: release manual playback with Back and compact browse navigation, retaining the remaining roadmap in Radio/Recommendations. Existing Epics 1–14 and completed Stories 15.1–15.14 remain intact. The [approved proposal](sprint-change-proposal-2026-09-19.md) records the old-to-new ID map and supersedes the earlier one-epic/29-story grouping. There are now 31 stories across these two epics; approval is planning approval, not runtime certification.

## Epic 15: Desktop Playback

Deliver the manual Desktop Playback release described above. Close through Story 15.17 without requiring Epic 16 features.

### Story 15.1: Close and reopen the UI without restarting the daemon

As a HifiMule user,
I want closing and reopening the UI to reconnect to the same background daemon,
So that ongoing work remains available and repeated launches do not create competing sessions.

**Requirements:** FR20 amendment; lifecycle foundation for FR56; P-AR1–2 and lifecycle portion of P-AR10; P-NFR3–5 where applicable. This story does not claim audible playback, native media keys or paused playback restoration; those require subsequent stories.

**Dependencies:** Existing application only. No new audio dependency or playback database tables are required.

**Acceptance Criteria:**

**Given** an installed build in a signed-in desktop session with no daemon running,
**When** the user launches HifiMule,
**Then** exactly one daemon starts in that user session and the UI connects to it,
**And** startup failures produce an actionable error rather than an indefinite connecting state.

**Given** a healthy daemon with existing background work,
**When** the user closes the UI and later reopens it,
**Then** the daemon process and ongoing work survive the window closure,
**And** the reopened UI obtains authoritative current state through the existing application communication boundary without restarting the daemon or replaying prior commands.

**Given** simultaneous UI, tray or configured startup launches for the same signed-in user,
**When** they attempt to acquire daemon ownership,
**Then** only one process becomes the owner and the others reconnect to it or report a bounded connection failure,
**And** the solution does not create another native event loop or enable login startup without the user's existing preference.

**Given** stale ownership metadata after an abnormal daemon exit,
**When** the user launches HifiMule again,
**Then** verified stale ownership can be recovered and one new daemon can start,
**And** a slow but live owner or a reused process identifier is not mistaken for a dead owner.

**Given** a local client lacking the required application credentials or user-session access,
**When** it attempts to control the daemon through the ownership/reconnect mechanism,
**Then** access is rejected without exposing credentials in UI messages or logs,
**And** a protocol-incompatible daemon produces an actionable compatibility error rather than a second competing daemon.

**Given** an idle daemon with the UI closed,
**When** the user selects the existing explicit Quit action,
**Then** the daemon exits and releases its ownership resources,
**And** no abandoned launcher automatically restarts it. Active-sync shutdown coordination is covered by the next lifecycle story and must retain existing protections meanwhile.

**Given** installed builds on Windows, macOS and Linux,
**When** launch, close/reopen, concurrent launch, crash recovery, rejected access and idle Quit checks run,
**Then** each scenario records process identity, connection outcome and cleanup evidence,
**And** the platform/architecture tested is explicit; a VM result is not presented as certification of an untested architecture.

**Implementation gate:** Before coding, record the selected ownership/liveness mechanism, local access model, protocol compatibility check and bounded startup timeout against the existing launch and RPC paths. Resolve that contract within this story's preparation; do not substitute a test-only probe or leave the production behavior unspecified. Preserve managed-device write safety throughout.

**Scope check:** One lifecycle integration story with existing daemon/UI entry points. Safe quit during active sync and versioned paused playback restoration are separate subsequent stories. No production code changes are authorized by this planning artifact itself.


### Story 15.2: Quit safely while a device sync is running

As a HifiMule user,
I want explicit Quit to stop background work in an orderly way,
So that HifiMule exits without leaving my device falsely marked as successfully synchronized.

**Requirements:** Shutdown foundation for FR60 and FR75; P-AR2; P-NFR3–4; inherited managed-device integrity and interrupted-sync recovery requirements.

**Dependencies:** Story 15.1. This story uses current sync operations; it does not require future audio playback. Audio stop/checkpoint integration is added when playback state exists.

**Acceptance Criteria:**

**Given** an idle daemon,
**When** the user selects Quit HifiMule,
**Then** new work is rejected, the daemon releases its ownership resources and exits,
**And** closing only the UI continues to leave the daemon running as established by Story 15.1.

**Given** one or more active device sync operations,
**When** the user selects Quit HifiMule,
**Then** shutdown requests cancellation of each active operation and prevents new sync work from starting,
**And** the UI or remaining desktop status surface reports that shutdown is waiting for device writes to finish safely.

**Given** cancellation reaches a transfer, conversion or managed metadata write,
**When** the operation stops,
**Then** incomplete work is not recorded as completed,
**And** existing atomic manifest and managed-file integrity guarantees are preserved without altering unmanaged files.

**Given** the user requests Quit repeatedly or reopens the UI during shutdown,
**When** those requests arrive,
**Then** they observe the same shutdown operation without launching another daemon or starting new work,
**And** a reopened UI displays the authoritative shutdown state.

**Given** a device disconnects or an operation fails during shutdown,
**When** cleanup proceeds,
**Then** affected work retains an interrupted or failed outcome with recoverable state,
**And** cleanup of other operations continues without reporting the failed operation as successful.

**Given** an operation has not reached a safe cancellation point within the documented shutdown deadline,
**When** that deadline is reached,
**Then** HifiMule exposes an actionable shutdown failure or waiting state according to the specified contract,
**And** it does not silently force termination, discard recovery state or claim a clean exit. The contract must define which blocked operations may safely be abandoned and which require continued waiting.

**Given** a shutdown interrupted a sync before completion,
**When** HifiMule is relaunched and the device reconnects,
**Then** the existing interrupted-sync reconciliation or repair path can identify the incomplete work,
**And** the device is not treated as fully synchronized solely because shutdown completed.

**Given** Windows, macOS and Linux builds,
**When** idle Quit, active-transfer cancellation, metadata-write cancellation, repeated Quit, disconnect and stalled-operation scenarios are exercised,
**Then** tests verify exit or explicit unresolved status, ownership cleanup when exited, and persisted device integrity,
**And** injected failure checks complement a real-device smoke check without being presented as equivalent evidence.

**Implementation gate:** Before coding, specify shutdown states, cancellation ownership, safe write boundaries, deadline values and blocked-operation behavior using the existing sync implementation. Document how the later audio stop and session checkpoint participate in this contract. Do not add playback tables or implement playback itself in this story.


### Story 15.3: Preserve and restore a paused listening session

As a HifiMule user,
I want HifiMule to retain my listening queue and position between launches,
So that I can resume deliberately without reconstructing my session or being surprised by automatic playback.

**Requirements:** Session-state foundation for FR56, FR58 and FR60; P-NFR2, P-NFR4–5; P-AR3–4 and P-AR10. Radio exclusions and preview restoration extend this foundation in their own stories.

**Dependencies:** Stories 15.1–15.2. This story delivers the production session-state and persistence contract through the daemon command boundary. It is testable using command-driven queue/position fixtures without requiring the future decoder or UI player; it does not claim audible Resume is delivered yet.

**Acceptance Criteria:**

**Given** a queue containing source-server track references and deliberate repeated entries,
**When** the session is checkpointed and the daemon restarts,
**Then** queue order, each occurrence identity, current occurrence and its last checkpointed position are restored,
**And** playback is paused regardless of the previous playing state; server credentials and authenticated stream URLs are not stored in the session record.

**Given** an empty queue or an explicitly cleared session,
**When** HifiMule restarts,
**Then** it restores an idle state without reviving an older queue.

**Given** a valid restored session whose server is temporarily unavailable,
**When** the daemon loads its local state,
**Then** it retains the queue and position without requiring an online server,
**And** it exposes source availability separately without silently deleting entries or choosing another recording.

**Given** an unsupported persistence version, invalid current occurrence or malformed session record,
**When** restoration runs,
**Then** it exposes a recoverable restoration error and retains the stored evidence without overwriting it with an empty session,
**And** known supported versions migrate transactionally while failed migration leaves the previous data recoverable.

**Given** concurrent session commands carrying the specified queue revision and command identity,
**When** the daemon processes them,
**Then** one serialized owner accepts valid changes, rejects stale queue mutations and returns the authoritative revision,
**And** progress-only changes do not invalidate queue edits; repeated command IDs have documented bounded deduplication behavior.

**Given** the UI reconnects after missing state updates,
**When** it requests the current session,
**Then** it receives a versioned authoritative snapshot with the queue revision and position units defined by the contract,
**And** reconnection does not replay mutations or create a second session owner.

**Given** position updates arrive while a session is active,
**When** periodic checkpointing or orderly Quit occurs,
**Then** persistence runs outside any future real-time audio callback and orderly Quit attempts a final checkpoint before exiting,
**And** a checkpoint failure is surfaced through the shutdown contract rather than reported as successful preservation. Abrupt process termination restores at most the last committed checkpoint.

**Given** a long queue/history and an interrupted database write,
**When** state is queried or the daemon restarts,
**Then** reads use bounded pages and the last committed state remains internally consistent,
**And** the story defines the checkpoint interval and page limits before acceptance rather than inventing an active-playback memory guarantee.

**Given** Windows, macOS and Linux builds,
**When** round-trip restoration, idle restoration, duplicate occurrences, stale mutations, reconnect, migration failure and interrupted-checkpoint checks run,
**Then** they verify the same state contract and paused/idle outcomes on each tested platform.

**Implementation gate:** Before coding, settle the minimum session schema, source/occurrence identity, position units, snapshot/command versions, queue revision rules, command-ID retention, migration strategy and checkpoint limits. Create only session data needed here; defer Radio, preview and export entities until their stories. Expose a narrow progress-update boundary for later audio integration without implementing audio in this story.


### Story 15.4: Play a selected library track through the daemon

As a HifiMule user,
I want to play a track from a configured music server,
So that I can listen directly in HifiMule without opening another player.

**Requirements:** Initial audible path for FR56 and FR58; initial source-quality foundation for FR71; output and state-safety foundations for FR64 and FR75; P-NFR2–5; P-AR5–6 and streaming portion of P-AR9; applicable P-UX-DR11, P-UX-DR13–14.

**Dependencies:** Stories 15.1–15.3. Scope is one selected track on the current shared default output, with a minimal accessible Play/Pause/Stop path in the existing browser. Output selection/native keys, seeking and queue advancement are subsequent stories. This story is complete and usable without those controls.

**Acceptance Criteria:**

**Given** a playable track on a configured server,
**When** the user selects its explicit Play action,
**Then** the daemon resolves the track using its server identity and existing credentials, buffers and decodes it with the controlled native runtime, and plays it through shared audio output,
**And** the UI shows the selected track and actual loading/playing/error state without receiving an authenticated stream URL. Browsing another server does not redirect the request.

**Given** the provider exposes original or alternative audio representations,
**When** playback chooses the initial source,
**Then** it uses the documented playback quality ranking independently of device sync transcoding settings,
**And** unsupported representations or server capability limits are reported accurately. Automatic quality adaptation is deferred, not silently claimed by this initial player.

**Given** a playing track,
**When** the user pauses and resumes through the UI,
**Then** playback holds and continues the same session position without creating a new occurrence,
**And** Stop halts output and records the documented stopped-state position behavior through the session manager.

**Given** a track is loading or playing,
**When** the user plays another track or stops,
**Then** obsolete fetch/decode work cannot later emit audio or replace the current session state,
**And** only one session owns the output stream.

**Given** a selected track is playing,
**When** the user closes and reopens the main UI,
**Then** audio continues in the daemon and the UI reconnects to its authoritative track/state/position,
**And** UI controls remain keyboard-operable with accessible names and do not break existing browser selection behavior.

**Given** a track reaches its natural end, loses its source connection or cannot be decoded,
**When** the daemon processes that outcome,
**Then** it transitions to an explicit completed, buffering or error state as specified by the transport contract,
**And** it does not silently repeat the track, count the error as a user dislike, or grow compressed/PCM storage without bounds.

**Given** the active output disappears,
**When** the audio backend reports the loss,
**Then** playback pauses or enters a recoverable paused error state without automatically routing music to another output,
**And** this safety behavior is present even before the later output-selection story.

**Given** playback and an existing device sync are active,
**When** the user selects Quit HifiMule,
**Then** audio stops, the session is checkpointed using Story 15.3 and sync cancellation follows Story 15.2,
**And** relaunch remains paused. Audio callbacks perform no blocking IO, persistence or acquisition of sync locks.

**Given** installed Windows, macOS and Linux builds with the controlled decoder runtime,
**When** supported-format fixtures and a configured-server playback smoke test exercise Play/Pause/Stop, replacement during loading, UI closure and shutdown,
**Then** output and state transitions are verified, actual loaded runtime versions are recorded and compressed/PCM bounds are checked,
**And** unsupported provider/format/platform combinations remain explicit rather than being implied by a successful local-file probe.

**Implementation gate:** Before coding, pin the runtime/dependencies and packaging approach for the tested builds, define provider stream resolution and initial quality ranking, buffer bounds, transport completion/Stop semantics and the minimal command/UI contract. Reuse established session identities and generation fencing. Limit implementation to this vertical single-track path; no Radio, full Playback destination, adaptive replacement or album-gapless claim is included. If provider differences cannot fit one implementation session, split provider enablement into ordered stories before execution while retaining a usable first-provider path.


### Story 15.5: Choose an audio output and recover safely from disconnection

As a HifiMule user,
I want to choose my headphones or audio interface as the playback output,
So that music plays where I intend and never unexpectedly moves to another speaker after a disconnection.

**Requirements:** Output selection portion of FR58; FR64; P-NFR3–4 and P-NFR6; output lifecycle portion of P-AR14; relevant P-UX-DR7 and P-UX-DR13.

**Dependencies:** Stories 15.1–15.4. Reuses the working single-track player and session owner. Native media-key controls and queue transport remain separate stories.

**Acceptance Criteria:**

**Given** one or more available shared audio outputs,
**When** the user opens the output selector,
**Then** HifiMule presents accessible output names and identifies the selected output,
**And** device identity distinguishes outputs with identical display names without exposing raw implementation identifiers as the primary label.

**Given** a track is playing or paused,
**When** the user explicitly selects another available output,
**Then** HifiMule opens that output in shared mode and preserves the logical track position and previous playing/paused state after a successful switch,
**And** it prevents simultaneous playback on the old and new outputs; a brief explicit-switch interruption is allowed without claiming gapless device switching.

**Given** the selected output disconnects or becomes unusable,
**When** the backend detects the loss,
**Then** HifiMule preserves the current session and position in a paused recoverable state,
**And** it explains the output problem without automatically routing music to another device.

**Given** playback paused because the selected output disappeared,
**When** that output reconnects or the system default changes,
**Then** playback remains paused until the user explicitly resumes,
**And** selecting a replacement output while paused does not itself start music.

**Given** a previously selected output preference was saved,
**When** HifiMule restarts,
**Then** it restores the preference using the platform's supported stable identity and keeps playback paused,
**And** if the output is absent or cannot be identified confidently, it shows an unavailable selection and requires an explicit available choice rather than silently substituting another output.

**Given** an output switch fails or races with Stop, another selection or device removal,
**When** asynchronous backend operations complete,
**Then** stale operations cannot replace the latest selection or resume stopped audio,
**And** the user sees the actual active/unavailable output and recoverable error without losing the listening queue.

**Given** other applications need audio access,
**When** HifiMule plays through the selected output,
**Then** it requests shared-mode access and does not change OS Do Not Disturb settings,
**And** ASIO and exclusive-mode controls are not required for this story.

**Given** Windows, macOS and Linux builds,
**When** explicit switching, unplug/replug, missing output at startup, failed open, duplicate names and sleep/wake are exercised,
**Then** tests verify output ownership and preserved paused/playing state as appropriate,
**And** a physical-output smoke check confirms that disconnection does not unexpectedly send sound to another speaker; virtual-device evidence is labeled separately.

**Implementation gate:** Before coding, settle output identity persistence and migration, platform enumeration/hotplug behavior, explicit-switch state transitions and handling of default-output changes while the selected output remains available. Define backend limits and actionable error states. Preserve the approved no-automatic-reroute rule on every platform.


### Story 15.6: Control playback through the operating system with the window closed

As a HifiMule user,
I want native media controls to operate the same player as the UI,
So that I can pause and resume music while working without reopening HifiMule.

**Requirements:** Native controls portion of FR56 and FR58; Resume portion of FR57; P-NFR3–4; native event-loop portion of P-AR1 and P-AR10; applicable P-UX-DR4 and P-UX-DR13.

**Dependencies:** Stories 15.1–15.5. Uses existing Play/Pause/Stop behavior. Native Next and seek are advertised only when their later transport stories enable them; this story does not create placeholder functionality.

**Acceptance Criteria:**

**Given** a playing or paused track,
**When** a supported native Play, Pause, Toggle or Stop command is delivered,
**Then** it executes through the same serialized session owner as the equivalent UI command,
**And** native and UI state reflect the actual result rather than maintaining separate transport states.

**Given** a track is playing and the main UI closes,
**When** the user uses the operating system's media controls,
**Then** Pause and Resume continue to control the daemon without opening a window,
**And** reopening the UI shows the resulting state and position without replaying the native command.

**Given** an existing paused session has a playable current track and usable selected output,
**When** the user chooses Resume from the desktop app menu,
**Then** HifiMule resumes that session without opening the main window or creating a fresh Radio,
**And** missing source/output or restoration errors remain paused and are reported accessibly. If there is no resumable session, the action is unavailable or gives a clear explanation.

**Given** the current session changes track, pauses, buffers, fails or ends,
**When** its authoritative state changes,
**Then** supported native now-playing fields and transport availability update consistently,
**And** unavailable metadata remains absent rather than showing information from a previous track. Local OS metadata updates do not themselves submit listening reports to the media server.

**Given** an unsupported transport command or a Play command while the selected output remains unavailable,
**When** the operating system delivers that command,
**Then** HifiMule neither fabricates success nor silently reroutes output,
**And** unsupported native actions are disabled or omitted where the platform permits.

**Given** native commands arrive during track replacement, output switching or shutdown,
**When** they reach the session owner,
**Then** the established generation and shutdown rules determine the result,
**And** commands cannot restart audio after Quit or activate obsolete session work.

**Given** HifiMule starts, closes its UI, reopens it and finally quits,
**When** native controls are registered and released,
**Then** their lifetime follows the daemon using the existing native event loop,
**And** repeated UI launches do not create duplicate registrations; explicit Quit clears the app's native playback registration and stale metadata as supported by the OS.

**Given** installed Windows, macOS and Linux builds,
**When** native API delivery and physical media-key routing are checked separately with the UI open and closed,
**Then** evidence identifies the OS, architecture, desktop session and command path actually tested,
**And** successful API delivery is not reported as proof of physical-key routing. Where the desktop does not deliver a key to HifiMule, the limitation is recorded without installing a global keyboard hook to seize it from other applications.

**Implementation gate:** Before coding, specify platform adapter ownership, supported command/metadata mappings, native registration cleanup and Resume menu behavior. Reuse the existing daemon event loop and proven native-control feasibility work; do not introduce a second player, cross-application media priority override or new server reporting behavior.


### Story 15.7: Seek within a track and see the actual playback position

As a HifiMule user,
I want to move to another point in a track when its source supports seeking,
So that I can replay a passage or continue from a chosen position without restarting the whole track.

**Requirements:** Position-control portion of FR58; seeking integrity portion of FR75; P-NFR4 and P-NFR6; seek portions of P-AR3, P-AR6, P-AR9 and P-AR10; applicable P-UX-DR7 and P-UX-DR13.

**Dependencies:** Stories 15.1–15.6. Uses existing single-track playback, provider stream resolution, native adapters and checkpointed state. Queue advancement and album continuity are separate stories.

**Acceptance Criteria:**

**Given** a loaded track with a known duration and validated seek support for its provider/representation,
**When** the user chooses a valid target position through the UI,
**Then** the daemon moves to that position within the documented and tested timing tolerance,
**And** it preserves the previous playing/paused state after the seek completes. The displayed position reflects the actual result rather than an unconfirmed requested target.

**Given** a selected source lacks reliable seeking or a duration required by the seek UI,
**When** controls are displayed,
**Then** unsupported seek actions are unavailable with a clear accessible explanation,
**And** ordinary playback remains usable; server byte-range support alone is not taken as proof of correct media-time seeking.

**Given** an invalid, non-finite, negative or beyond-duration seek target,
**When** a command reaches the daemon,
**Then** it is validated according to the explicit boundary contract without corrupting position or queue state,
**And** seeking exactly to the end uses the documented completion behavior and cannot accidentally repeat the track.

**Given** a seek is pending,
**When** a newer seek, track replacement or Stop supersedes it,
**Then** obsolete fetch/decode completions cannot emit old-position audio or overwrite current session state,
**And** temporary compressed and decoded buffers remain bounded during repeated seeks.

**Given** a seek fails because the source disconnects or returns an invalid response,
**When** recovery is attempted,
**Then** HifiMule preserves recoverable session state and reports the actual paused, buffering or playing outcome,
**And** it does not claim the requested position was reached or treat the failure as a user skip/dislike.

**Given** a supported native seek command is delivered,
**When** the native adapter forwards it,
**Then** it uses the same capability checks, time units and serialized command path as UI seeking,
**And** native position metadata updates from authoritative playback state. Unsupported native seek actions are omitted where the platform permits.

**Given** playback is running, paused, buffering or newly restored,
**When** the UI renders elapsed position,
**Then** it uses authoritative daemon updates with interpolation only while playback is advancing,
**And** pause, seek completion, reconnect and source changes correct interpolation without modifying queue revision merely for progress.

**Given** a successful seek is followed by a checkpoint and application restart,
**When** the session is restored,
**Then** it restores paused at the last committed actual position rather than a failed or superseded request,
**And** seeking itself does not submit a completed-listen report; provider reporting eligibility is defined in the later reporting story.

**Given** supported provider/format combinations on Windows, macOS and Linux,
**When** forward/backward seeks, paused seeks, exact-end targets, repeated seeks and failed-source cases are tested,
**Then** decoded-position fixtures establish the documented accuracy tolerance and integration checks verify transport state,
**And** combinations without evidence retain disabled seeking rather than inheriting support from another format or provider.

**Implementation gate:** Before coding, define position/duration units, supported provider/representation seek mechanisms, accuracy tolerances, invalid-target behavior, exact-end semantics and failure recovery. Reuse the existing generation and session contracts. Seeking within the current representation does not enable mid-track quality switching, which requires separate validation in the adaptation stories.


### Story 15.8: Play an album in order and advance through its tracks

As a HifiMule user,
I want to start an album and hear its tracks in their intended order,
So that I can listen to the complete album without starting every track individually.

**Requirements:** Album-order portion of FR61; Next portion of FR58; album failure behavior of FR73; session continuity portions of FR56 and FR60; P-NFR4–6; applicable P-AR3–4 and P-AR10; P-UX-DR11, P-UX-DR13–14.

**Dependencies:** Stories 15.1–15.7. Delivers ordered queue transport using the working single-track player. Prepared gapless boundaries and album loudness handling are separate following stories; this story does not claim them complete.

**Acceptance Criteria:**

**Given** an album with source-provided disc and track ordering,
**When** the user chooses Play album,
**Then** HifiMule creates the main session queue in disc/track order and starts its first playable attempt,
**And** each queue occurrence retains its source identity independently of the currently browsed server. This explicit Play action replaces the main listening queue; it is not the later Preview action.

**Given** an album has multiple discs, missing ordering fields or tied track numbers,
**When** its queue is constructed,
**Then** valid disc/track ordering is honored and missing/tied values use a documented deterministic fallback based on available provider ordering,
**And** the daemon does not silently substitute arbitrary title sorting or drop distinct occurrences.

**Given** an album track reaches natural completion,
**When** the daemon processes its terminal event,
**Then** it advances exactly once to the next occurrence,
**And** delayed or duplicate completion events from the old generation cannot skip an additional track or modify a replaced session.

**Given** a current occurrence has a successor,
**When** the user selects Next in the UI or through a supported native control,
**Then** the daemon moves once to the successor through the same queue command path and preserves the current playing/paused intent,
**And** the previous occurrence is recorded as explicitly skipped rather than naturally completed. Server reporting is not enabled by this local disposition record.

**Given** the current occurrence is the last in the album,
**When** it finishes naturally,
**Then** playback becomes idle/completed with the queue retained for inspection and restoration,
**And** it does not automatically repeat or start Radio. Next without a successor is unavailable where the control surface permits, and a delivered unsupported command cannot mutate the queue.

**Given** an album track is unavailable, fails to load or encounters an unrecoverable decode error,
**When** that track is reached,
**Then** HifiMule pauses at the affected occurrence and offers an explicit retry with its error visible,
**And** it does not silently omit the track or mark the technical failure as a user skip/dislike. The user may explicitly choose Next when a successor exists.

**Given** an album is playing or paused,
**When** the user closes/reopens the UI or quits/relaunches HifiMule,
**Then** UI reconnection retains the active occurrence and full queue, while application relaunch restores paused at the last committed position,
**And** temporary server unavailability does not discard remaining album entries.

**Given** album playback is available in the existing browser,
**When** controls and current-track metadata update,
**Then** Play album and Next have accessible names and keyboard operation, native Next availability matches the actual queue, and source badges remain accurate,
**And** existing grid/list selection and playlist/basket actions continue to work without requiring the later full Playback destination.

**Given** Windows, macOS and Linux builds,
**When** ordered multi-disc fixtures, missing/tied numbering, duplicate completion events, paused Next, final-track completion, failed-track retry and restoration scenarios run,
**Then** they verify the expected occurrence sequence and transport state,
**And** these checks are not reported as evidence of gapless physical output, which has its own story.

**Implementation gate:** Before coding, define provider album ordering/fallback rules, queue completion states, Next behavior and retry semantics. Extend persisted occurrence disposition only as needed for natural completion, explicit skip and technical failure; retain the distinctions for later snapshot/reporting stories. Reuse serialized queue revisions and generation fences.


### Story 15.9: Preserve album continuity across prepared track boundaries

As a HifiMule user,
I want album tracks to join without player-added gaps,
So that live recordings, continuous compositions and intentional pauses sound as recorded.

**Requirements:** Continuity portion of FR61; applicable FR73 and FR75; P-NFR1–4; P-AR5–7 and continuity evidence portion of P-AR14.

**Dependencies:** Stories 15.1–15.8. Builds on ordered album transport. Loudness gain is a following story; this story preserves continuity at unchanged gain. It does not guarantee uninterrupted playback when the next source cannot be prepared in time.

**Acceptance Criteria:**

**Given** adjacent supported album tracks with the next track prepared before the boundary,
**When** the current track finishes,
**Then** decoded output continues through the same active output stream without inserting silence, omitting valid samples or duplicating boundary samples,
**And** exactly one queue advancement occurs at the audible handoff, with track metadata and position associated with the correct occurrence rather than the prefetch start.

**Given** source audio contains intentional silence at the beginning or end of a track,
**When** it crosses an album boundary,
**Then** that recorded silence is preserved,
**And** HifiMule does not apply silence detection, automatic crossfade or overlap to conceal a boundary.

**Given** a supported encoded format provides validated encoder-delay or end-padding information,
**When** it is decoded,
**Then** only known non-musical padding is removed according to that format's verified behavior,
**And** absent or ambiguous padding metadata does not justify guessing or trimming source samples; any resulting continuity limitation is recorded explicitly.

**Given** adjacent tracks differ in sample rate or channel layout,
**When** both are supported by the selected output conversion policy,
**Then** they are converted into the established output format without reopening the output stream at every track boundary,
**And** tests account for resampling duration and delay with a reference conversion, rather than comparing unconverted input sample counts. Unsupported transitions produce an explicit recoverable state rather than corrupt output.

**Given** the next track is being fetched and decoded in advance,
**When** playback proceeds,
**Then** compressed prefetch and decoded PCM remain separately bounded,
**And** the audio callback performs no allocation, blocking IO or synchronization with device-sync locks. Preparation cannot grow into an unbounded album download or decode.

**Given** a prepared successor becomes obsolete after Next, seek, Stop, queue replacement or output loss,
**When** its asynchronous work completes,
**Then** generation checks prevent its audio or completion event from entering the current session,
**And** stopping or pausing at the boundary does not leak the first samples of the successor.

**Given** the successor is not ready in time or cannot be loaded,
**When** the current track ends,
**Then** HifiMule exposes buffering or the established album pause/retry state without silently skipping the successor,
**And** the event is not claimed as a successful gapless transition or classified as a user rejection.

**Given** deterministic adjacent-track fixtures for the supported format matrix,
**When** their combined decoded output is checked,
**Then** same-format boundaries contain zero extra silence, missing valid samples or duplicate valid samples relative to the expected reference,
**And** fixtures include known padding, intentional silence, different rates/layouts and cancellation around the boundary. Exact supported combinations and runtime versions accompany the results.

**Given** Windows, macOS and Linux builds with prepared album boundaries,
**When** physical-output continuity is checked separately from decoder fixtures and callback counters,
**Then** captured-output comparison or a documented equivalent measurement checks for player-added boundary gaps,
**And** listening checks supplement that evidence. VM output and API counters are labeled separately; sustained real-sync stress remains part of later reliability acceptance.

**Implementation gate:** Before coding, specify the continuous-output handoff, prefetch bounds, supported padding interpretation, fixed-output conversion policy and sample-reference method, using the feasibility findings and controlled runtime. Do not imply that successful decoding of one format proves gapless handling for every representation or platform.


### Story 15.10: Preserve an album's relative loudness with consistent gain

As a HifiMule user,
I want an album's tracks to share one appropriate loudness adjustment,
So that quiet and loud passages retain their intended relationship while usable metadata helps avoid excessive playback levels.

**Requirements:** Album portion of FR74; continuity regression for FR61; P-NFR1–2 and P-NFR4; loudness portion of P-AR7. Radio track-level gain is implemented in the Radio group.

**Dependencies:** Stories 15.1–15.9. Applies to the ordered album session and continuous audio path. Does not introduce a metadata analysis service, dynamic-range compression or a new user taste setting.

**Acceptance Criteria:**

**Given** an album has valid, consistent album loudness and peak metadata under the supported metadata policy,
**When** its playback session is prepared,
**Then** HifiMule derives one album gain and applies it consistently across its tracks,
**And** it does not normalize individual tracks to the same loudness or otherwise erase their relative level differences.

**Given** the metadata-based gain would exceed the permitted output peak,
**When** the effective gain is calculated,
**Then** a documented static peak-protection rule reduces the common gain sufficiently for the supported peak reference,
**And** protection does not apply a different gain to each track, introduce a limiter or compress the signal dynamically.

**Given** album loudness or peak metadata is missing, malformed, non-finite, contradictory or expressed in an unsupported convention,
**When** HifiMule evaluates it,
**Then** unusable metadata is rejected and gain remains unchanged where a safe consistent album adjustment cannot be established,
**And** it does not substitute per-track normalization, guess an album gain, or claim metadata-based clipping protection when the necessary peak evidence is absent.

**Given** source metadata uses supported gain units and reference loudness conventions,
**When** it is normalized into the playback model,
**Then** conversion and precedence rules yield a documented deterministic result,
**And** provider metadata and embedded tags cannot cause gain to be applied twice.

**Given** an album transitions between tracks, seeks within a track, pauses/resumes or restores after restart,
**When** playback continues,
**Then** the same resolved album gain policy remains in effect,
**And** track handoff does not create an unintended gain step, extra silence or duplicate samples. Late-arriving metadata cannot silently change gain on each new track.

**Given** conversion to the established output format and album gain are both required,
**When** samples reach the output path,
**Then** gain and peak calculations use the documented signal scale and conversion order,
**And** bounded processing stays outside blocking or allocating callback work. Any claim concerning inter-sample peaks is limited to what the implementation actually validates.

**Given** deterministic albums with known relative levels, valid and invalid tags, excessive suggested gain and intentional silence,
**When** adjusted samples are compared against reference calculations,
**Then** tests verify the common gain, preserved relative levels, fallback behavior and peak limit for supported metadata,
**And** no dynamic compression or boundary artifacts are introduced.

**Given** Windows, macOS and Linux builds,
**When** the same fixtures run through the playback pipeline,
**Then** results satisfy the documented numerical tolerance and retain Story 15.9 continuity checks,
**And** the evidence distinguishes digital pipeline behavior from volume adjustments or processing applied by the OS or external audio interface.

**Implementation gate:** Before coding, define supported loudness/peak tags and units, reference loudness, metadata precedence, consistency checks, static peak margin, conversion order and incomplete-album fallback. Resolve how common gain is established with bounded metadata access before playback; do not require decoding or downloading the entire album. Missing usable metadata must preserve unchanged gain rather than introduce an unapproved normalization policy.


### Story 15.11: Preview a full track without losing the main listening session

As a HifiMule user,
I want to audition a track while keeping my current listening session,
So that I can assess music for a playlist or basket and then return to where I was listening.

**Requirements:** FR62–63; applicable FR58, FR60 and FR64; P-NFR2, P-NFR4–6; session portions of P-AR3–4 and P-AR10; P-UX-DR3, P-UX-DR11 and P-UX-DR13–14.

**Dependencies:** Stories 15.1–15.10. Uses existing single-track/album transport, output safety and restoration. Preview is usable in the existing browser before the later floating Playback bar. This story does not enable server listening reports or basket/playlist export.

**Acceptance Criteria:**

**Given** a main session is playing or paused,
**When** the user invokes an explicit Preview action on a library track,
**Then** HifiMule preserves the main queue, current occurrence, actual position, playing/paused intent and album gain context before auditioning the selected full track,
**And** Preview is clearly distinct from Play and ordinary playlist/basket actions, with its source identity retained independently of browsing context.

**Given** an audition is active,
**When** the user previews another track,
**Then** the new audition replaces only the previous audition,
**And** the original main session remains preserved without nesting another saved session or accumulating unbounded audio buffers.

**Given** an audition finishes naturally with a preserved main session,
**When** HifiMule returns to that session,
**Then** it restores its queue, occurrence and position with the previous playing/paused intent,
**And** no audition track is inserted into the main queue or advances its album sequence.

**Given** an audition is active,
**When** the user explicitly chooses Return to session,
**Then** the audition ends and the preserved main session is restored with its previous playing/paused intent,
**And** the action is accessible by keyboard and clearly labeled. This action is distinct from Stop, which restores the main session paused.

**Given** an audition is active,
**When** the user selects Stop through the UI or native transport,
**Then** the audition ends and the preserved main session is restored paused,
**And** Stop never unexpectedly starts the main session. With no preserved main session, it leaves playback idle.

**Given** an audition has no preserved main session,
**When** it completes naturally,
**Then** playback becomes idle without starting Radio or repeating the track,
**And** the UI does not offer a misleading return-to-session action.

**Given** audition loading or return to the main session fails,
**When** the daemon handles the error,
**Then** it retains recoverable main-session state and explains the failed operation,
**And** it does not discard the queue, silently choose another output, or repeatedly alternate between failed preview and resume attempts. Output loss retains the existing pause-until-explicit-resume rule.

**Given** preview replacement, Stop, return or ordinary Play races with pending fetch/decode work,
**When** asynchronous work completes,
**Then** only the current transport generation can emit audio or change state,
**And** an explicit ordinary Play replaces the listening session under its established contract rather than allowing a later audition completion to resurrect the old session.

**Given** the UI closes while previewing,
**When** it reopens,
**Then** it reflects the same audition and preserved-main state from the daemon,
**And** native commands target the active audition. If the application quits and restarts, restoration remains paused; the persistence contract must preserve the main session and define whether the pending audition is retained or dismissed without losing it.

**Given** preview transport records local outcomes,
**When** an audition finishes, is stopped, is replaced or fails,
**Then** its disposition remains distinct from main-queue occurrences and technical errors,
**And** later reporting can distinguish a fully heard audition from an interrupted one without reporting it to the server in this story.

**Given** Windows, macOS and Linux builds,
**When** tests cover playing/paused/no-main previews, successive auditions, natural completion, Stop, explicit return, failed return, output loss and restart,
**Then** they verify exactly one active audio owner and preservation of the expected main queue, position and intent,
**And** browser controls preserve selection behavior, accessible names and visible audition status.

**Implementation gate:** Before coding, define preview/main state transitions, explicit Return intent, preview failure recovery, active-audition persistence on restart and native Next behavior during audition. Resolve exact Preview placement within existing browser/context-menu patterns. Create only persistence fields needed for one preserved main session and one audition; use existing generation fencing and output safety rules.


### Story 15.12: Access Playback as an always-available destination

As a HifiMule user,
I want Playback listed alongside my physical devices even when none are connected,
So that I can listen and curate my local listening queue without attaching a sync device.

**Requirements:** Destination and independent configuration portions of FR55; device navigation portion of FR59; FR33 amendment; P-NFR4–6; configuration portion of P-AR4; P-UX-DR1, P-UX-DR6, P-UX-DR10, P-UX-DR12–15.

**Dependencies:** Stories 15.1–15.11. Uses the existing playable main session. Full queue editing and the floating bar follow in separate stories; this story exposes the current queue read-only through the destination. Automatic-selection settings are added with Radio, not shown as nonfunctional controls.

**Acceptance Criteria:**

**Given** HifiMule is open with any number of physical devices,
**When** destinations are displayed,
**Then** Playback is always present and listed first,
**And** it is identified as a local listening context rather than a mounted or synchronizable device.

**Given** no physical device is connected,
**When** the UI opens or the selected physical device disconnects,
**Then** Playback is selected and library browsing and playback remain usable,
**And** only physical-target basket and sync actions remain unavailable; the old global no-device lock is not applied to listening.

**Given** the user selects Playback,
**When** its destination view is displayed,
**Then** it shows the daemon's current listening queue and state, or an actionable empty state pointing to manual library playback,
**And** it does not expose storage capacity, mount paths, folder layout, device transcoding, manifest, sync or repair actions.

**Given** a physical device arrives while Playback or another destination is selected,
**When** device detection completes,
**Then** the UI selects the arriving device's basket and visibly identifies it,
**And** a blank/unconfigured device retains an accessible configuration action. Device-open failure is shown with recovery guidance rather than appearing as an unexplained absent device.

**Given** music or an audition is active,
**When** the user selects a destination, browses another server, or a physical device arrives/disconnects,
**Then** listening continues from the same source and main/preview state unless the actual audio output is lost,
**And** navigation changes alone neither replace the listening queue nor reroute server commands.

**Given** destination configuration is loaded or changed,
**When** HifiMule stores Playback-specific values needed by existing playback behavior,
**Then** those values are stored locally under a typed Playback configuration with validated defaults,
**And** device/server auto-fill settings and portable manifests are neither copied implicitly into Playback nor modified. Source/selection controls not yet implemented remain absent.

**Given** the UI reconnects or configuration cannot be loaded,
**When** the destination is rendered,
**Then** queue/state comes from the authoritative daemon and invalid configuration produces a recoverable explanation,
**And** failure does not remove Playback from navigation or overwrite valid device settings.

**Given** narrow, medium and wide supported layouts,
**When** Playback and physical devices are navigated by keyboard or assistive technology,
**Then** the active destination, empty state and configuration action have accessible names and visible focus using existing design tokens,
**And** device-arrival selection is announced without stealing keyboard focus from the user's current control or obscuring essential actions.

**Given** Windows, macOS and Linux builds,
**When** no-device launch, managed/blank-device arrival, selected-device removal, open failure, server navigation and UI reconnection are exercised,
**Then** tests verify the expected selected destination and unchanged daemon listening identity,
**And** physical sync and basket restrictions remain intact.

**Implementation gate:** Before coding, define the typed destination discriminator, navigation fallback, multiple-arrival ordering and minimal local configuration version/defaults. Reuse existing device detection, source badges and responsive design components. Keep Playback out of physical-device manifests and sync enumeration; add only configuration fields needed by delivered behavior.


### Story 15.13: Edit the upcoming listening queue

As a HifiMule user,
I want to add, reorder and remove upcoming tracks in Playback,
So that I can shape what I hear next without interrupting the current track.

**Requirements:** Manual queue portion of FR55 and FR68; occurrence-preservation foundation for FR78; P-NFR2, P-NFR4–6; queue portions of P-AR3–4 and P-AR10; P-UX-DR5, P-UX-DR11–14. Radio replenishment and exclusion behavior extend this story later.

**Dependencies:** Stories 15.1–15.12. Uses the existing Playback destination, queue transport and persistence. Server playlist writes and basket exports are not triggered by local queue editing.

**Acceptance Criteria:**

**Given** tracks are selected in the library,
**When** the user explicitly adds them to the Playback queue,
**Then** the daemon appends occurrences in the specified selection order, retaining their source identities,
**And** deliberately adding the same recording again creates a distinct occurrence rather than silently deduplicating it. Adding tracks does not interrupt current playback or automatically start an idle session.

**Given** upcoming occurrences are displayed,
**When** the user reorders them through an accessible queue action,
**Then** their new order becomes authoritative and persists across reconnection,
**And** the currently playing occurrence, its position and past history are unchanged. Keyboard operation must provide an alternative to drag-and-drop.

**Given** an upcoming occurrence is selected,
**When** the user removes it,
**Then** only that occurrence is removed and playback continues,
**And** removal of one repeated entry does not remove every occurrence of the recording or alter the source server's library or playlists.

**Given** a queue edit invalidates a prefetched successor,
**When** pending preparation completes,
**Then** generation/revision checks prevent the obsolete successor from playing,
**And** the next boundary uses the latest accepted queue order with bounded preparation buffers.

**Given** playback advances while a queue edit is being submitted,
**When** the edit reaches the daemon with a stale revision or targets an occurrence that is no longer upcoming,
**Then** it is rejected with current authoritative state rather than silently removing/reordering the active track,
**And** the UI explains the conflict and refreshes without blindly replaying the mutation. Progress-only updates do not cause such conflicts.

**Given** an album queue is manually changed,
**When** the edit is accepted,
**Then** subsequent playback follows the explicit user order,
**And** unchanged album playback still follows disc/track order. The session's album-versus-manual mode and gain policy are resolved explicitly before implementation rather than silently applying album assumptions to mixed material.

**Given** a preview is active with a preserved main queue,
**When** the user edits upcoming main-session occurrences,
**Then** the edits apply to that preserved queue without replacing the audition or its saved current occurrence,
**And** returning from preview exposes the accepted edited queue.

**Given** a long listening history and queue,
**When** the destination is opened or scrolled,
**Then** history is paged and queue rendering is bounded or virtualized with stable occurrence identities,
**And** local loading, empty and mutation-error states remain accessible without resetting unrelated browser multi-selection. Manual-queue limits are documented separately from later Radio lookahead limits.

**Given** the UI closes or the application restarts after accepted edits,
**When** state is restored,
**Then** the accepted queue order and repeated occurrences are retained, with application relaunch paused,
**And** no remote playlist or device basket has changed as a side effect.

**Given** Windows, macOS and Linux builds,
**When** append/reorder/remove, repeated occurrences, edit-versus-advance races, preview editing, keyboard operations and long-history rendering are tested,
**Then** results verify occurrence identity, preserved current playback and bounded display behavior,
**And** the existing provider capability and source-identity rules remain intact.

**Implementation gate:** Before coding, define batch selection ordering, queue edit commands, manual-queue limits, persisted revision behavior and album-to-manual transition/gain semantics. Scope remove/reorder to upcoming entries; current-track transport and historical rejection retain their separate meanings. Reuse existing UI selection and virtualization patterns without treating the Playback queue as a live server playlist.


### Story 15.14: Control listening from a floating playback bar across views

As a HifiMule user,
I want playback controls to remain visible while I browse music or work with a device basket,
So that I can manage listening without navigating away from what I am doing.

**Requirements:** Floating controls portion of FR59; presentation of FR58 and FR62–63; P-NFR6; UI snapshot/interpolation portion of P-AR10; P-UX-DR2–3, P-UX-DR7, P-UX-DR10–14. The idle Play something action is completed with Radio.

**Dependencies:** Stories 15.1–15.13. Consolidates already working transport, output selection and preview return controls into a shared bar. Does not introduce another playback state owner or nonfunctional Radio controls.

**Acceptance Criteria:**

**Given** any library or destination view is open,
**When** the user navigates between servers, Playback and physical-device baskets,
**Then** a translucent floating playback bar remains visible below the media browser using the established design tokens,
**And** navigation neither recreates the session nor changes the source of its controls.

**Given** the current session is playing, paused or stopped,
**When** the bar renders,
**Then** it exposes the implemented transport, position and output controls with accurate capability availability,
**And** track identity and elapsed position come from authoritative daemon state with interpolation only while playback advances.

**Given** playback is idle,
**When** the bar renders before Radio is implemented,
**Then** it remains visible with an actionable manual-listening empty state and Resume only when a resumable session exists,
**And** it does not display a dead Play something button. The later Radio story must replace this interim idle action with working Play something to complete FR59.

**Given** an audition is active,
**When** the bar renders,
**Then** it clearly identifies preview playback and exposes Return to session only when a main session exists,
**And** Return restores the previously approved playing/paused intent while Stop restores the main session paused.

**Given** playback is loading, buffering, disconnected or in a recoverable error state,
**When** the bar updates,
**Then** it communicates the actual state and relevant recovery action without claiming that audio is playing,
**And** unsupported controls are unavailable with an accessible explanation where needed. Later quality-adaptation messages use this same status area.

**Given** a long list or basket reaches its final rows,
**When** the user scrolls to the bottom or focuses a bottom-row action,
**Then** every row and action can be brought fully above the floating controls,
**And** the translucent treatment does not reduce text/control contrast below the existing accessibility target or intercept interactions with visible content outside the bar.

**Given** narrow, medium and wide supported layouts,
**When** the window resizes or text is enlarged,
**Then** essential transport and error/recovery actions remain reachable without overlapping the basket,
**And** secondary controls can use accessible overflow patterns without clipping labels or forcing horizontal page scrolling.

**Given** the user is interacting with another control,
**When** playback metadata, time or status changes in the background,
**Then** the bar does not steal focus or announce every elapsed-time tick,
**And** meaningful state/error changes are announced appropriately while all bar controls have keyboard operation, accessible names and visible focus.

**Given** the UI reconnects after missing events,
**When** the bar receives a fresh snapshot,
**Then** it corrects displayed track, position and control availability without replaying commands,
**And** pending interactions cannot apply to an obsolete occurrence through stale UI state.

**Given** Windows, macOS and Linux UI builds,
**When** view navigation, preview return, output errors, idle state, long-list bottom rows, keyboard navigation, zoom and responsive layouts are verified,
**Then** the bar remains usable and consistent with the daemon,
**And** accessibility checks and visual review cover supported OS themes and translucent-background contrast.

**Implementation gate:** Before coding, define responsive bar layout, reserved scroll/focus space, status announcement policy and supported-control overflow behavior using existing UI components. Preserve the approved placement under the browser and the authoritative shared playback state. Keep Radio, quality adaptation and exports in their own stories while retaining explicit completion coverage for their future controls.


### Story 15.15: Restart the current track or return to the previous track

As a HifiMule user,
I want a Back button in the playing bar and equivalent supported keyboard/media control,
So that I can restart a track or return to the previous track without rebuilding my queue.

**Requirements:** amended FR58; FR59 transport presentation; FR60 persistence and FR61 ordering constraints; P-NFR3–6; P-AR3–4, P-AR6 and P-AR10; P-UX-DR2 and P-UX-DR12–14.

**Dependencies:** Stories 15.1–15.14. No Radio prerequisite.

**Acceptance criteria:**

1. **Restart after three seconds.** Given a main track with authoritative position greater than 3,000 ms, when Back is accepted, then the current track returns to its beginning. The decision uses daemon position rather than interpolated UI time. Playing remains playing and paused remains paused, subject to existing output/error inhibition.
2. **Previous near the beginning.** Given a main track at 0–3,000 ms inclusive and an available preceding occurrence in the retained main-session playback order, when Back is accepted, then that preceding track becomes current at its beginning. Preserve its source identity, accepted forward order and deliberate repeated entries. Back followed by forward progression returns through the same sequence without silently dropping or duplicating queued selections.
3. **Beginning and unavailable-source boundaries.** Given no preceding occurrence, Back restarts the current track where supported, without wrapping to the queue end. With no current track, it is unavailable. If the requested restart/previous source cannot be prepared, preserve recoverable state and expose an error; do not silently choose a different track or output.
4. **Preview isolation.** Given an active audition, Back restarts that audition and never navigates the preserved main history. The saved main occurrence, queue, position and return intent remain intact. Existing Return and Stop semantics are unchanged.
5. **Shared UI/native command.** Given an available Back action, clicking its bar button, activating the focused button with Enter/Space, or receiving the OS Previous/media-key command invokes the same daemon operation. Support native Previous/keyboard delivery wherever the platform integration provides it, including with the UI closed. Advertise truthful native availability; document unsupported delivery. Do not introduce an arbitrary global key combination or hijack text-editing, seek-slider or browser-navigation keys.
6. **Accessible bar integration.** Given any library/Playing/device view and supported layout, Back remains reachable beside transport controls with a localized accessible name, tooltip/help explaining restart versus previous behavior, visible focus and truthful disabled state. Preserve the current two-row layout, timeline, bottom-row reachability and focus during updates.
7. **Ordering and history integrity.** Given repeated commands or a race with automatic advance, queue edits, seek, preview, source replacement or Quit, the daemon serializes decisions, deduplicates retried command identities and fences obsolete preparation. Stale UI commands cannot rewind a newly selected session. Replaying earlier music must not overwrite already-recorded outcomes; specify replay occurrence identity and forward-cursor behavior before coding.
8. **Persistence and regression evidence.** Given a successful Back followed by orderly Quit/relaunch, restore the accepted current track, queue and position paused. Verify the 0, 3,000 and 3,001 ms boundaries; first-track fallback; repeated source IDs; manual and album queues; paused state; Preview with/without a main session; unavailable sources; output loss; concurrent commands; and restoration. Record UI/native API and physical keyboard/media-key results separately on shipping platforms.

**Implementation gate:** Inspect the existing history, queue cursor and persistence model; freeze replay identity, forward progression, stopped/completed-state behavior, command preconditions/error codes and per-platform native capability mapping. Restart may reuse seek or reopen from zero when safely supported; unsupported behavior must be explicit. Use existing command admission, generation fences and provider routing. Back is not a skip/dislike or an automatic-selection request. Future 16.7 reporting must account for replay without inferring completed listens merely from cursor movement.

The three-second boundary and audition restart rule were approved in the 2026-09-19 course correction; this backlog story does not claim existing implementation.

### Story 15.16: Compact the library browse-mode bar with icons and smaller labels

As a HifiMule user,
I want a compact library navigation bar with recognizable icons and smaller text,
So that I can switch between Tracks, Albums, Recently Added and other browse modes while leaving more room for music.

**Requirements:** FR8; P-NFR6; applicable P-UX-DR10–14; existing provider-capability and browse-state contracts.

**Dependencies:** Existing browse-mode navigation and Story 15.14 library/Playing integration. No dependency on Back or Radio. Packaging 15.17 verifies the completed redesign.

**Scope:** The library browse-mode selector (Tracks, Albums, Recently Added, Artists, Playlists, Genres, Frequently Played, Recently Played and Favorites where supported). This is the navigation bar identified by the user, not the playing transport bar or per-track basket action buttons.

**Acceptance criteria:**

1. **Compact icon-and-label controls.** Given the available library browse modes, when the bar renders, each mode has a consistent recognizable icon and a smaller visible localized text label. Reduce visual bulk and spacing relative to the current bar while keeping labels readable and pointer targets usable. Record before/after measurements at matching viewport, language and available-mode count; merely shrinking text without improving the layout is insufficient.
2. **Preserved navigation.** Given any supported mode, activating its redesigned control opens the same view and uses existing loading, breadcrumb, selection-reset and cached-view behavior. Retain capability filtering, loading/disabled state and clear current-mode indication. Do not introduce duplicate fetches or change playback, selected source or basket contents as a presentation side effect.
3. **Responsive layout.** Given narrow, medium and wide library columns, divider changes, long translated labels or 200% text scaling, every available mode remains reachable without clipped controls, overlap or horizontal page overflow. Choose and document wrapping or accessible overflow during story preparation; keep the selected mode identifiable and any overflow keyboard-operable. Preserve the grid/list toggle and its existing mode-specific availability.
4. **Keyboard and accessible naming.** Given keyboard or assistive-technology navigation, each control has its full localized accessible name, visible focus and programmatically exposed selection state. Icons do not cause duplicate announcements. Enter/Space retain button activation, and metadata/loading updates do not unexpectedly move focus. Any abbreviated visible label has a full hover/focus hint; tooltips are not the sole accessible name.
5. **Consistent visual treatment.** Given supported themes and enabled/selected/disabled states, the bar uses existing Shoelace tokens and icon conventions with sufficient text/icon contrast. Compact styling does not shrink unrelated application buttons or alter track/album content typography.
6. **Visual and behavior verification.** Given representative supported-provider mode sets and EN/FR/ES/DE labels, verify the actual rendered layout, mode switching, active state, grid/list toggle, keyboard focus and constrained-width/text-scale behavior. Record checks on supported installed platforms or retain explicit unverified entries for packaging 15.17; DOM-only assertions do not certify rendered compactness.

**Implementation gate:** Confirm the current bar boundaries, icon mapping, label sizing, spacing, target sizes and responsive policy against the existing library layout before coding. Use a focused visual comparison to settle these details; preserve existing navigation semantics rather than creating a new mode model.

### Story 15.17: Ship verified playback builds for Windows, macOS and Linux

As a HifiMule user,
I want the installed application to provide the tested playback behavior on my supported platform,
So that listening works without a development environment or manually installed decoder libraries.

**Requirements:** P-NFR3 and integrated P-NFR1–6 regression; P-AR5 and P-AR14; release evidence for implemented portions of FR55–64, FR68, FR71, FR73–75, new Back behavior and FR8 compact browse navigation; applicable P-UX-DR1–15. Deferred Epic 16 features are not claimed complete.

**Dependencies:** Stories 15.1–15.16. Epic 16 is not a prerequisite. Completes packaged-build integration and release verification; earlier stories remain responsible for their own cross-platform checks. Does not itself publish a release or certify untested architectures.

**Acceptance Criteria:**

**Given** the project's explicitly enumerated shipping OS/architecture matrix,
**When** release artifacts are built,
**Then** each artifact contains or resolves the controlled playback runtime through the documented distribution mechanism,
**And** exact decoder/audio dependency versions, licensing obligations and runtime loading paths are recorded. A development-machine library must not silently substitute for the shipped runtime.

**Given** an installed artifact on a clean supported environment,
**When** the user starts HifiMule and plays supported source formats,
**Then** streaming, decoding and shared output work without development tools or manually locating native libraries,
**And** packaging/signing/permission behavior follows the existing platform distribution model without requiring elevated privileges for ordinary playback.

**Given** an existing installation with device and server configuration,
**When** it is upgraded to the playback build,
**Then** supported configuration/session migrations preserve existing sync settings and credentials,
**And** an interrupted or failed migration produces recoverable state instead of erasing device configuration. Unsupported downgrade behavior is documented without claiming automatic rollback safety.

**Given** each installed platform build,
**When** UI close/reopen, simultaneous launches, native transport, output loss, sleep/wake, paused restoration and safe Quit during sync are exercised,
**Then** results verify the production daemon lifetime and selected-output behavior,
**And** native API delivery and physical media-key routing have separate evidence entries.

**Given** configured supported providers and representative library metadata,
**When** selected-track/album playback, manual queue edits, Preview/Return, bar transport including Back/Next/seek, compact browse navigation, output selection and existing device-sync workflows run,
**Then** their results match the approved capability-dependent behavior and source routing,
**And** unsupported provider features remain accurately unavailable rather than appearing successful. Manual idle browsing/Resume remain useful; unimplemented Radio actions are not advertised.

**Given** prepared albums and representative playback-plus-sync workloads,
**When** release evidence is assembled,
**Then** deterministic continuity, physical-output checks, bounded-resource, recovery and ordinary real-sync-coexistence measurements for shipped manual playback from compatible builds are linked with versions and environments,
**And** changed packaged dependencies trigger the affected checks rather than inheriting incompatible probe evidence.

**Given** the UI is used across supported themes, widths, text scaling and keyboard navigation,
**When** the shipped manual Playback experience is reviewed,
**Then** the floating bar including Back, compact browse-mode bar, queue, preview and error states retain accessible names, visible focus, sufficient contrast and reachable bottom-row actions,
**And** background updates do not steal focus or overwhelm assistive technology with time ticks.

**Given** a platform, architecture or hardware interaction has not been tested or has a failing check,
**When** the release decision is recorded,
**Then** that entry remains an explicit blocker or accurately scoped unsupported capability,
**And** ARM64 VM results do not certify x64, API calls do not certify physical keys, and callback counters do not certify physical gaplessness. Existing Linux teardown warnings must be resolved or assessed with reproducible evidence before acceptance.

**Given** the verification matrix is complete,
**When** the story is reviewed for acceptance,
**Then** each shipping entry links its artifact, loaded runtime versions, test outcomes and material limitations,
**And** no failed required check is hidden by a global pass. Actual release publication remains outside this story's validation action.

**Given** completed Stories 15.1–15.14 retain outstanding checks or deferred defects,
**When** the release evidence is reconciled,
**Then** inventory applicable full-application/installed checks and recorded issues, including Story 15.12 R16 and Story 15.14 R8/R9, and resolve release blockers or record an explicit scoped disposition with evidence,
**And** preserve accurate unverified rows and reconcile stale status prose without rewriting historical results as passes.

**Implementation gate:** Before execution, enumerate shipping platforms/architectures and provider capability expectations from the actual release configuration, define clean-install/upgrade environments and evidence ownership, and settle controlled-runtime licensing/signing requirements. Close applicable architecture gates with recorded evidence. Set manual-playback verification workloads and resource budgets before acceptance. This release makes no adaptive-quality or conditional-backoff claim; retain implemented source selection and buffering/retry. Stories 16.12–16.14 own those later features and full Radio soak validation. Mid-track quality switching remains disabled without separate validation.


## Epic 16: Radio/Recommendations

Deliver the retained automatic-selection, curation and reliability roadmap after the Epic 15 manual release. Historical story IDs are mapped in the approved course correction.

### Story 16.1: Configure Playback selection and start its first selected track

As a HifiMule user,
I want independent Playback source and selection settings using HifiMule's existing selection engine,
So that I can start listening from my curated music without choosing the first track manually or changing device sync preferences.

**Requirements:** Source/selection settings portion of FR55; first-result selection foundation for FR57 and FR65; shared-engine portion of P-AR8; local configuration portion of P-AR4; P-NFR2, P-NFR4–6; P-UX-DR6, P-UX-DR13–14.

**Dependencies:** The delivered Epic 15 foundation (Stories 15.1–15.17) and the existing auto-fill engine. This story provides a working selection-settings action that plays its first eligible result. It does not label this finite behavior as ongoing Radio or expose final Play something yet; bounded replenishment follows in the next story.

**Acceptance Criteria:**

**Given** configured music servers,
**When** the user opens Playback selection settings,
**Then** the user can choose participating sources and supported playlist/artist/genre selection inputs using suitable existing selection controls,
**And** capability-dependent inputs are unavailable with an explanation rather than silently ignored. Physical capacity, folder layout and device transcoding controls are absent.

**Given** Playback selection settings are edited and saved,
**When** the UI reconnects or HifiMule restarts,
**Then** the validated settings remain available from local Playback configuration,
**And** no physical device/server settings or portable manifests are changed. Existing device preferences are not implicitly adopted as Playback defaults.

**Given** equivalent eligible candidates, ordering settings and an explicit deterministic seed where the strategy needs one,
**When** sync and Playback request selection through their respective consumer contexts,
**Then** both invoke the same pure selection implementation and obtain equivalent ordering before consumer-specific constraints,
**And** Playback has no copied selection algorithm or dependence on a device mount/capacity. Existing sync selection behavior remains unchanged under its existing inputs.

**Given** valid Playback settings produce at least one eligible track,
**When** the user explicitly starts the first selected track from the settings view,
**Then** the daemon plays the first eligible result in the engine's order using its retained source identity,
**And** it uses the existing main-session Play behavior rather than an audition. This deliberate action starts listening; merely saving settings does not replace an active session.

**Given** selection inputs include multiple servers or identical server-local track IDs,
**When** candidates are normalized and selected,
**Then** every candidate retains a portable server-plus-track identity through the shared engine boundary,
**And** source routing is unaffected by the browsed server. Cross-copy recording deduplication is added separately and is not inferred from matching local IDs or titles.

**Given** no sources are configured, a required source is unavailable or no eligible track is found,
**When** the selection action runs,
**Then** the user receives an actionable setup, source or empty-selection explanation,
**And** the existing main session is preserved if no replacement can be prepared. Failed selection is not treated as a user skip or taste signal.

**Given** selection is running,
**When** the user changes settings and starts a newer request or cancels the pending request,
**Then** obsolete results cannot replace the current session,
**And** candidate retrieval/cache use is bounded independently of device sync limits. No entire-library playback queue is created.

**Given** Playback is active while sync selection runs,
**When** either consumer uses its settings,
**Then** the consumers do not mutate each other's configuration, random state or selection memory,
**And** Playback introduces no durable cross-session taste profile.

**Given** Windows, macOS and Linux builds,
**When** settings persistence, shared-engine equivalence, source identity collisions, empty/unavailable selection, concurrent requests and keyboard interaction are tested,
**Then** selection produces the expected first track and preserves the existing sync behavior,
**And** controls use the existing responsive design and accessible names/status feedback.

**Implementation gate:** Before coding, define the pure selection interface, consumer-specific eligibility/context, source identity and settings schema/defaults, deterministic testing inputs, candidate bounds and invalid-setting behavior. Preserve all existing sync selection semantics. Keep continuous replenishment, artist transitions, logical-session exclusions and final Play something menu/bar wiring assigned to subsequent Radio stories.


### Story 16.2: Replenish Radio within a bounded upcoming queue

As a HifiMule user,
I want my listening queue to replenish as I listen while respecting my edits and skips,
So that music can continue without assembling the whole library into a playlist or repeatedly suggesting tracks I rejected in this session.

**Requirements:** FR65, FR68–69; logical-session restoration portion of FR60; exhaustion foundation for FR67; Radio error behavior of FR73; P-NFR2 and P-NFR4–5; relevant P-AR3–4, P-AR6, P-AR8 and P-AR10; P-UX-DR5 and P-UX-DR7.

**Dependencies:** The delivered Epic 15 foundation (Stories 15.1–15.17) and Story 16.1. Implements the replenishment/session policy behind an explicit settings-view start action. Until artist-transition policy is delivered, automatic selection stays within the initial artist and enters an explained waiting state when that artist is exhausted. Final Play something in the bar/menu follows after artist transitions; this intermediate behavior is not presented as the completed Radio experience.

**Acceptance Criteria:**

**Given** the first selected track establishes an initial artist and valid Playback settings,
**When** the user starts the replenishing session,
**Then** the daemon creates a logical Radio session and fills a documented bounded number of upcoming automatic occurrences from eligible unheard tracks of that artist,
**And** it never converts the whole library into the playback queue. Ambiguous or missing artist identity uses a documented conservative waiting state until the relationship policy is available.

**Given** the automatic lookahead falls below its refill threshold,
**When** replenishment runs,
**Then** new suggestions append without reordering existing occurrences,
**And** only one accepted refill result affects the current queue revision; stale or duplicate work cannot add duplicate automatic suggestions.

**Given** the user adds, reorders or removes upcoming entries,
**When** replenishment resumes,
**Then** accepted manual order is preserved and removed automatic suggestions are excluded for the logical session,
**And** automatic lookahead limits do not silently truncate manual additions. Manual queue, candidate cache, compressed prefetch and PCM limits remain independently defined.

**Given** the user explicitly skips a Radio track,
**When** the session records that outcome,
**Then** the track is excluded from automatic selection for the current logical session,
**And** a technical fetch/decode failure is recorded separately rather than as a skip or dislike. This story uses source-qualified track identity; the later recording-deduplication story extends exclusions across confident copies.

**Given** a suggested track fails to load,
**When** the daemon attempts another eligible suggestion,
**Then** Radio can bypass the failed source while preserving the failure explanation,
**And** attempts and retry eligibility are bounded so an unavailable artist/source cannot cause a busy loop. Album pause/retry behavior remains unchanged.

**Given** no eligible unheard track remains within the currently implemented artist scope,
**When** replenishment cannot fill the queue,
**Then** it exposes an explained waiting state and allows already queued tracks to finish,
**And** it does not improvise genre-only drift, clear exclusions or silently restart a cycle. The next artist-transition story completes those explicit continuation rules.

**Given** Radio is paused, stopped, previewed or replaced,
**When** refill work completes,
**Then** it cannot restart stopped audio or change a replaced session,
**And** preview preserves the logical main Radio session. The refill contract bounds background preparation and distinguishes Pause, Stop and replacement without resetting exclusions implicitly.

**Given** the application quits and restarts during Radio,
**When** its session is restored,
**Then** the queue, logical-session identity, heard set and skip/removal exclusions restore paused,
**And** resuming retains those exclusions. Explicitly starting a new Radio resets them; no preference state is accumulated across independent Radio sessions.

**Given** a lengthy Radio session,
**When** history and exclusions grow,
**Then** durable session records and paged reads prevent unbounded in-memory history,
**And** lookahead and candidate caches remain within their documented limits while preserving the full logical-session exclusion behavior.

**Given** Windows, macOS and Linux builds,
**When** tests exercise refill thresholds, manual edits, duplicate refill completion, skips, source failures, pause/preview/replacement and restart,
**Then** they verify bounded preparation, stable queue order and retained session exclusions,
**And** no result is presented as proof that artist drift or cross-server recording deduplication is complete.

**Implementation gate:** Before coding, define lookahead/refill thresholds, source-qualified eligibility/identity, heard semantics, retry bounds, pause/Stop refill behavior and session persistence limits. Reuse the shared selection engine and revision/generation contracts. Artist transitions, repeat cycles, recording-level deduplication and final entry-point wiring remain explicit subsequent work.


### Story 16.3: Continue Radio through meaningful artist connections

As a HifiMule user,
I want Radio to stay close to the current artist before moving to a clearly related artist,
So that its progression feels understandable within my curated libraries rather than drifting on genre alone.

**Requirements:** FR66–67; continuation of FR65; P-NFR2, P-NFR4–5; relationship portions of P-AR8–9; P-UX-DR7.

**Dependencies:** The delivered Epic 15 foundation (Stories 15.1–15.17) and Stories 16.1–16.2. Completes artist progression and cycle renewal on the bounded replenishment path. Cross-server recording deduplication, Radio gain and final Play something entry points remain subsequent stories.

**Acceptance Criteria:**

**Given** the current artist has eligible unheard tracks under the logical session's settings and exclusions,
**When** Radio selects more automatic suggestions,
**Then** those tracks take precedence over changing artist,
**And** existing manual queue order and bounded lookahead remain unchanged.

**Given** the current artist has no remaining eligible unheard tracks,
**When** configured-server metadata supports a meaningful connection to another eligible artist,
**Then** Radio selects that artist as its new center using the documented relationship ranking,
**And** it exposes a concise reason supported by the actual metadata rather than inventing a relationship. Shared genre alone is insufficient evidence of connection.

**Given** no supported meaningful connection leads to eligible unheard music,
**When** Radio needs a new center,
**Then** it uses the original Playback selection settings to choose a fresh eligible starting point,
**And** labels the transition as a new starting point rather than a related-artist recommendation. It retains logical-session exclusions and heard state.

**Given** all eligible tracks under the session's selection scope have been heard,
**When** further listening is requested by the running Radio,
**Then** it begins another listening cycle with renewed cycle-level heard eligibility,
**And** retains skip/removal exclusions for the logical session instead of replaying rejected suggestions. This renewal does not create a new Radio session or reset its user edits.

**Given** exclusions or source availability leave no eligible music,
**When** a refill or cycle renewal is attempted,
**Then** Radio enters an explained waiting state without a tight retry loop,
**And** it does not silently broaden the source settings, discard exclusions or start an external recommendation service.

**Given** artist metadata is missing, ambiguous, contradictory or available only as a shared genre,
**When** relationship eligibility is evaluated,
**Then** unsupported links are not presented as meaningful connections,
**And** the fresh-center fallback remains usable. Candidate retrieval and relationship caches stay within documented bounds.

**Given** sources are temporarily unavailable,
**When** the engine evaluates exhaustion,
**Then** it distinguishes unknown availability from verified absence of eligible unheard music,
**And** it does not clear cycle heard state solely because a metadata request failed. Retry behavior preserves the current queue and explains the limitation.

**Given** a center transition or cycle renewal races with a queue edit, new Radio or settings change,
**When** its result is applied,
**Then** the session revision/generation contract rejects obsolete work,
**And** current center, cycle identity and explanation reflect the accepted transition only. Saving settings alone does not silently rewrite the active session's original selection context.

**Given** the application restarts after a transition or cycle renewal,
**When** the logical session is restored,
**Then** its center, cycle-level heard state and logical-session exclusions restore consistently and paused,
**And** resuming neither repeats a transition because of lost state nor starts a new taste profile.

**Given** deterministic metadata fixtures on Windows, macOS and Linux,
**When** tests cover current-artist priority, meaningful links, genre-only evidence, no-link fallback, cycle renewal, total exclusion, unavailable sources and restart,
**Then** each expected selection and user-facing reason is verified against the fixture evidence,
**And** relationships are derived only from configured-server metadata. Claims about real-library coverage are supported separately from synthetic fixtures.

**Implementation gate:** Before coding, inventory relationship metadata actually available from supported providers and define accepted evidence types, ranking/tie-breaking, artist identity, original-settings snapshot semantics, cycle exhaustion and retry bounds. Missing provider relationship support must use the approved fresh-center fallback; it is not permission to introduce third-party enrichment or genre-only links.


### Story 16.4: Avoid duplicate Radio recordings across configured servers

As a HifiMule user,
I want Radio to recognize confident copies of the same recording across my servers,
So that duplicate library copies do not repeat unnecessarily while distinct performances remain available.

**Requirements:** FR70; recording-level eligibility portions of FR66–69; identity foundation for FR78–79; P-NFR2 and P-NFR4–5; P-AR8 and source-routing portion of P-AR9; P-UX-DR14.

**Dependencies:** The delivered Epic 15 foundation (Stories 15.1–15.17) and Stories 16.1–16.3. Extends existing source-qualified candidates and logical Radio state. Does not deduplicate manual queue occurrences or write to server libraries. Feedback and export remain subsequent stories.

**Acceptance Criteria:**

**Given** candidates from configured servers have evidence meeting the documented same-recording confidence policy,
**When** Radio evaluates automatic suggestions,
**Then** those copies count as one recording for heard eligibility and automatic queue selection,
**And** a duplicate copy cannot consume another automatic suggestion slot merely because its source ID differs.

**Given** candidates have only matching titles, ambiguous metadata or evidence of different performances,
**When** recording identity is resolved,
**Then** uncertain matches and distinct performances remain separate,
**And** live versions, covers or materially distinct recordings are not merged solely on artist/title similarity or duration proximity.

**Given** a recording has several eligible source copies,
**When** Radio selects one for an occurrence,
**Then** the chosen source follows a documented deterministic availability/quality ranking and retains its server-plus-track identity,
**And** the queue distinguishes recording identity from occurrence identity. Duplicate recognition does not itself authorize silent mid-track source replacement.

**Given** a Radio recording is heard, skipped or excluded by removing an automatic suggestion,
**When** another confident copy is considered within the same cycle or logical session as applicable,
**Then** recording-level eligibility respects the corresponding heard or exclusion state,
**And** a new logical Radio resets session exclusions while cycle renewal retains them. Uncertain copies retain independent eligibility.

**Given** the user deliberately queues the same recording more than once,
**When** those manual occurrences coexist with Radio,
**Then** each deliberate occurrence is preserved with its own position and source reference,
**And** recording deduplication affects automatic suggestions without collapsing explicit repetitions or rewriting accepted queue entries.

**Given** metadata identity evidence changes or conflicting identifiers are discovered,
**When** the identity resolver updates its view,
**Then** existing occurrence source references remain stable and reconciliation follows a documented conservative policy,
**And** it does not delete uncertain tracks, rewrite previously played sources or silently erase exclusions. Identity state required for restoration is versioned.

**Given** a source is unavailable or lacks useful identity metadata,
**When** Radio gathers candidates,
**Then** it can continue using eligible available sources under the established selection/failure rules,
**And** missing evidence does not become a confident match. A technical source failure is not recorded as a rejection of every copy.

**Given** the user browses another server or the application restarts,
**When** a queued occurrence plays or its state is restored,
**Then** its chosen source, recording and occurrence identities remain distinct and recoverable,
**And** future reporting/export uses that chosen source rather than the currently browsed server.

**Given** a large multi-server candidate set,
**When** identity matching and lookahead run,
**Then** candidate retrieval and identity caches remain bounded with persistence/paging where required,
**And** no third-party lookup or full-library audio fingerprint download is introduced.

**Given** deterministic multi-server fixtures on Windows, macOS and Linux,
**When** tests cover confident copies, colliding local IDs, uncertain matches, distinct performances, manual repeats, cross-copy exclusions, metadata conflicts and restart,
**Then** the expected recording groups and occurrence sources are verified,
**And** real-library identity coverage is reported separately from synthetic correctness tests.

**Implementation gate:** Before coding, define accepted recording evidence, confidence/conflict rules, source-copy ranking, persisted identity versioning and reconciliation of existing source-qualified heard/exclusion state. Use metadata from configured servers only. Preserve uncertain recordings rather than broadening heuristics to inflate deduplication coverage.


### Story 16.5: Match Radio track loudness using available metadata

As a HifiMule user,
I want Radio to use available track loudness metadata,
So that music from different albums has more consistent listening levels without compressing its dynamics.

**Requirements:** Radio portion of FR74; P-NFR1–2 and P-NFR4; loudness portion of P-AR7.

**Dependencies:** The delivered Epic 15 foundation (Stories 15.1–15.17) and Stories 16.1–16.4. Reuses Story 15.10 metadata normalization and static gain/peak protection. Adds the Radio policy rather than a second audio gain implementation.

**Acceptance Criteria:**

**Given** a Radio occurrence has valid track loudness and usable peak metadata,
**When** it is prepared for playback,
**Then** HifiMule derives track-level gain under the documented reference loudness and peak policy,
**And** applies that gain once to the chosen source's decoded audio without dynamic compression or automatic crossfade.

**Given** the suggested track gain would exceed the supported peak limit,
**When** effective gain is calculated,
**Then** the existing static peak-protection rule reduces it as necessary,
**And** the protection claim is limited to the validated peak convention and signal conversion path.

**Given** a track lacks usable loudness metadata or sufficient peak evidence for a safe adjustment,
**When** Radio prepares it,
**Then** gain remains unchanged according to the established fallback,
**And** the player does not guess loudness, substitute unrelated album tags or analyze/download an entire library to manufacture metadata.

**Given** consecutive Radio tracks require different gains,
**When** prepared playback crosses their boundary,
**Then** each gain applies to the correct occurrence's samples without leaking into the previous or next track,
**And** there is no additional silence, overlap or dropped sample introduced by the gain switch. The story does not promise equal perceived loudness when metadata is unavailable or inaccurate.

**Given** an explicit album session is active instead of Radio,
**When** its tracks play,
**Then** the common album gain policy remains in force,
**And** Radio's per-track policy does not replace it. Session mode, rather than the currently browsed album or server, selects the applicable policy.

**Given** a Radio occurrence is replaced, sought, paused, resumed or restored,
**When** audio preparation completes,
**Then** gain belongs to the active occurrence and chosen source under the existing generation contract,
**And** stale metadata or gain calculations cannot alter a newer track. Restored playback remains paused until explicitly resumed.

**Given** an audition interrupts Radio and later returns,
**When** the preserved session resumes,
**Then** its Radio gain context is restored without double application,
**And** the audition's gain policy is explicitly defined using existing standalone playback behavior rather than accidentally inheriting the main track's gain.

**Given** deterministic tracks with known gain/peak tags, invalid tags, missing metadata and different source copies,
**When** reference comparisons run on Windows, macOS and Linux,
**Then** tests verify gain selection, safe fallback, correct occurrence boundaries and preservation of album behavior,
**And** processing remains bounded and introduces no blocking or allocating work into the audio callback.

**Implementation gate:** Before coding, confirm track metadata conventions and precedence against the album implementation, define Radio/album/standalone-preview policy dispatch and freeze gain per prepared occurrence. Reuse the existing conversion and static peak-protection primitives; do not add user taste memory or dynamic-range processing.


### Story 16.6: Start Radio with Play something without opening the main window

As a HifiMule user,
I want a Play something action in the desktop menu and idle playback bar,
So that I can start an ongoing Radio from my configured libraries with one action instead of facing an empty listening page.

**Requirements:** FR57; idle Play something completion of FR59; entry-point integration of FR55 and FR65–70; P-NFR4–6; P-AR10; P-UX-DR2, P-UX-DR4 and P-UX-DR7.

**Dependencies:** The delivered Epic 15 foundation (Stories 15.1–15.17) and Stories 16.1–16.5. Wires existing selection, replenishment, artist progression, deduplication and Radio gain to final user entry points. Does not create a new selection implementation.

**Acceptance Criteria:**

**Given** Playback settings contain usable configured sources,
**When** the user selects Play something from the desktop app menu with the main UI closed,
**Then** the daemon starts a fresh logical Radio with the first eligible result from the shared selection engine under those settings,
**And** playback proceeds without opening the main window or requiring a physical sync device.

**Given** playback is idle and the floating bar is visible,
**When** the user activates Play something,
**Then** it starts the same daemon operation as the desktop menu,
**And** the interim manual-listening empty state is replaced by the working action with accessible loading, success and failure feedback.

**Given** a resumable existing session is present,
**When** the user chooses Resume instead,
**Then** its queue, position and logical-session exclusions are retained,
**And** Resume does not rerun initial selection or create a fresh Radio. Play something remains a distinct explicit new-session action.

**Given** another session or audition exists,
**When** Play something successfully prepares a replacement session,
**Then** it becomes the new main Radio and starts a new logical-session exclusion scope,
**And** obsolete audition or selection work cannot later restore the replaced session. Saving settings alone never triggers this replacement.

**Given** no sources are configured, settings are invalid, sources are unavailable or no eligible first track can be prepared,
**When** Play something is requested,
**Then** it presents the specific setup or recoverable failure with an actionable route to the relevant settings,
**And** it preserves any existing session when replacement cannot be prepared. An error does not silently open the main window; opening settings remains an explicit user action.

**Given** the selected audio output is unavailable,
**When** Play something is requested,
**Then** it respects the existing output-loss pause rule and explains the required output choice,
**And** it does not send music to an automatically substituted output merely to satisfy one-action startup.

**Given** repeated menu/UI activation or a pending start superseded by Resume, Stop or another session command,
**When** selection and preparation complete,
**Then** the documented command ordering and generation policy prevents duplicate starts and obsolete replacement,
**And** loading feedback reflects the accepted request rather than a stale one.

**Given** Radio starts successfully,
**When** the user continues listening with the UI closed,
**Then** bounded replenishment, current-artist priority, meaningful transitions or labeled fresh-center fallback, recording deduplication and cycle/exclusion behavior continue in the daemon,
**And** native controls remain available. Reopening the UI shows the same Radio and current transition or waiting explanation.

**Given** saved Playback settings change during an existing Radio,
**When** the user saves those settings,
**Then** the current session retains its original selection context,
**And** the next explicit Play something uses the updated settings. The UI explains this distinction without silently rebuilding the active queue.

**Given** Windows, macOS and Linux installed builds,
**When** Play something is exercised from the platform-appropriate desktop menu and floating bar, including a closed UI, missing setup, unavailable output, repeated activation and existing paused Radio,
**Then** checks verify the expected first track, fresh-versus-resumed session identity and continued replenishment,
**And** the native menu placement and tested platform are recorded without assuming macOS Dock behavior maps identically to Windows or Linux tray menus.

**Implementation gate:** Before coding, define each platform's menu placement, windowless setup/error presentation and start-command supersession rules. Reuse existing generation, output and replacement contracts. Keep first-track selection deterministic under controlled test inputs and avoid adding a confirmation dialog to the normal configured one-action start path.


### Story 16.7: Report listening accurately to the source server

As a HifiMule user,
I want supported listening activity recorded on the server that supplied the track,
So that its listening history reflects what I heard without HifiMule creating its own taste profile.

**Requirements:** FR76; reporting portion of FR81; P-NFR4–5; reporting portions of P-AR9 and P-AR12; source identity portion of P-UX-DR14.

**Dependencies:** The delivered Epic 15 foundation (Stories 15.1–15.17) and Stories 16.1–16.6. Uses occurrence identities, actual playback progress, preview outcomes and durable session state. Like/Dislike and exports remain separate stories.

**Acceptance Criteria:**

**Given** a provider supports now-playing status separately from completed-listen reporting,
**When** audible playback starts or its relevant state changes,
**Then** HifiMule sends the supported status to the occurrence's source server,
**And** prefetch, queue insertion, local native metadata updates and merely restoring a paused session do not themselves submit a completed listen.

**Given** a track reaches the provider's verified completed-listen eligibility conditions,
**When** its outcome is evaluated,
**Then** HifiMule submits the appropriate report through that provider's documented semantics,
**And** eligibility is derived from actual listening and transport events rather than assuming that requesting or downloading a full stream means it was heard.

**Given** a user skips, stops or replaces a track before it qualifies,
**When** the reporting policy evaluates it,
**Then** HifiMule avoids submitting a completed-listen report where the provider permits,
**And** it does not promise to reverse a play already counted by the server or treat technical failures as dislikes.

**Given** the user seeks forward, repeats a passage, pauses, buffers or resumes after restart,
**When** listening eligibility is calculated,
**Then** the provider-specific tested policy handles those events explicitly,
**And** wall-clock time, skipped-over duration and repeated progress messages cannot accidentally manufacture a completed listen.

**Given** an audition finishes or is interrupted,
**When** its outcome is evaluated,
**Then** a fully heard audition counts where supported and an interrupted one follows the same provider eligibility rules,
**And** returning to the main session does not count its preserved occurrence a second time solely because it resumed.

**Given** a queue spans multiple servers or contains repeated occurrences,
**When** reporting is submitted,
**Then** each qualifying occurrence targets its chosen source using existing credentials,
**And** browsing another server does not reroute it. Deliberate repeat occurrences remain distinguishable from retries of one report.

**Given** the network fails, the server rejects a report or the daemon exits during submission,
**When** the reporting operation is recovered,
**Then** durable operation state distinguishes pending, confirmed, failed and ambiguous outcomes as needed,
**And** retries use verified provider idempotency or reconciliation where available. An ambiguous non-idempotent write is not blindly repeated or represented as guaranteed exactly-once delivery.

**Given** reporting is unsupported or an operation cannot be safely reconciled,
**When** its status is exposed,
**Then** HifiMule makes that limitation or unresolved result inspectable without interrupting playback,
**And** stored operation evidence and logs do not expose credentials or authenticated stream URLs.

**Given** pending reporting state grows during a server outage,
**When** recovery and retries run,
**Then** backoff, persistence, retention and in-memory work are bounded under a documented policy,
**And** bookkeeping is not performed in the audio callback and does not become a cross-session recommendation/taste model.

**Given** supported providers and Windows, macOS and Linux builds,
**When** tests cover completion, early skip, seeking, preview completion, main-session return, duplicate events, repeated occurrences, restart and ambiguous responses,
**Then** provider request fixtures verify routing and eligibility, and configured-server integration checks verify actual play-count/status effects,
**And** unsupported semantics remain disabled or explicitly limited rather than inferred from another provider's behavior.

**Implementation gate:** Before coding, verify each provider's now-playing, completion, automatic play-count, seek/resume and idempotency semantics; define eligibility thresholds and durable operation identity/reconciliation/retention. No universal counting threshold is assumed. Enable only verified reporting behavior; if provider integrations exceed one implementation session, split them into ordered provider-specific stories before execution.


### Story 16.8: Save supported Like and Dislike preferences on the source server

As a HifiMule user,
I want explicit Like or Dislike actions to update my source server's track preference,
So that the preference is available outside HifiMule rather than becoming a separate local taste profile.

**Requirements:** FR77; rejected-occurrence foundation for FR78; P-NFR4–6; feedback portion of P-AR9; P-UX-DR8 and P-UX-DR13–14.

**Dependencies:** The delivered Epic 15 foundation (Stories 15.1–15.17) and Stories 16.1–16.7. Uses provider capabilities, occurrence/source identity and existing operation recovery patterns. Playlist/basket snapshots are subsequent stories.

**Acceptance Criteria:**

**Given** a source provider exposes a verified equivalent of Like or Dislike for the relevant user and track,
**When** playback feedback controls are displayed,
**Then** HifiMule exposes only the supported actions with accessible names and the server's known current state,
**And** unknown preference state is distinguishable from an explicit neutral or negative state.

**Given** a provider supports favorites but no genuine Dislike equivalent,
**When** controls are displayed or an unsupported command is submitted,
**Then** Dislike is unavailable and the daemon rejects unsupported feedback,
**And** removing a favorite is never silently interpreted as Dislike. Any mapping from favorite to Like must be explicitly verified and documented.

**Given** the user explicitly invokes a supported feedback action on an occurrence,
**When** the operation is sent,
**Then** it targets that occurrence's server and track using existing credentials,
**And** a browsing-context change or confident duplicate on another server does not cause the preference to be written to other sources.

**Given** a feedback request is in flight,
**When** it succeeds, fails or returns an ambiguous result,
**Then** the UI distinguishes pending, confirmed and unresolved/error states,
**And** it does not present an unconfirmed write as a server-confirmed preference. Retry/reconciliation follows verified provider semantics without silently toggling a value twice.

**Given** the user submits opposing feedback or changes tracks while a request is pending,
**When** responses arrive out of order,
**Then** results remain associated with their original source/track operation and the latest accepted intent is resolved explicitly,
**And** a stale response cannot change the displayed preference of the new current track.

**Given** the user explicitly dislikes a listening occurrence,
**When** local session disposition is recorded,
**Then** that occurrence is marked rejected for the later listening snapshot,
**And** this session record is distinguishable from server confirmation and does not become a durable cross-session recommendation profile. Failed remote persistence remains visible; it must not erase the user's explicit rejection from the current session.

**Given** Like/Dislike is applied during playback or preview,
**When** the preference operation completes,
**Then** it does not implicitly issue transport commands, replace the queue or undo a previously counted listen,
**And** any logical-session Radio exclusion behavior is explicitly defined before implementation rather than inferred as a new global taste-learning rule.

**Given** feedback state is refreshed from the server after reconnect,
**When** the provider returns an authoritative value,
**Then** HifiMule reconciles its displayed state and pending operations according to the documented conflict policy,
**And** local operational caching does not claim authority over preferences changed by another client.

**Given** Windows, macOS and Linux builds and each enabled provider mapping,
**When** supported/unsupported actions, pending failures, ambiguous writes, rapid opposing actions, source changes and preview feedback are tested,
**Then** checks verify correct server scope, accessible feedback states and retained session rejection,
**And** configured-server integration evidence confirms actual preference semantics rather than relying solely on endpoint names.

**Implementation gate:** Before coding, define provider equivalence mappings, read/write capabilities, pending-operation identity and reconciliation, feedback-versus-transport behavior and local rejection semantics including explicit later preference reversal. Use bounded operational persistence only; no local-only durable taste fallback or cross-server preference propagation is introduced.


### Story 16.9: Save an immutable local listening snapshot

As a HifiMule user,
I want to save the accepted history and upcoming queue from my listening session locally,
So that I can keep what I discovered without ongoing playback changing the saved selection.

**Requirements:** FR78; local cross-server order portion of FR79; P-NFR2 and P-NFR4–6; snapshot portion of P-AR11; P-UX-DR15.

**Dependencies:** The delivered Epic 15 foundation (Stories 15.1–15.17) and Stories 16.1–16.8. Saves and exposes a read-only local snapshot using existing queue presentation. Server playlist and physical basket export follow in separate stories; this story does not write to either destination.

**Acceptance Criteria:**

**Given** a main session has accepted played occurrences, a current occurrence and upcoming entries,
**When** the user explicitly saves its listening snapshot,
**Then** the saved sequence contains accepted played occurrences in order, the current occurrence once unless rejected, and upcoming occurrences in their accepted order,
**And** skipped/disliked entries are omitted without duplicating the current occurrence at the history/queue boundary.

**Given** the same recording was deliberately queued more than once,
**When** the snapshot is built,
**Then** each included occurrence is retained separately in order,
**And** recording-level Radio deduplication does not collapse deliberate repetitions in the saved snapshot.

**Given** playback advances or the queue is edited while saving,
**When** the daemon captures the snapshot,
**Then** it represents one consistent session revision and disposition boundary,
**And** later playback, feedback or edits do not mutate the saved sequence. A duplicate save request with the same operation identity follows the documented deduplication contract.

**Given** an audition is active over a preserved main session,
**When** the user saves the listening snapshot,
**Then** it captures the preserved main listening history/current/upcoming sequence,
**And** temporary audition playback does not silently insert an extra occurrence. The UI identifies which session is being saved.

**Given** the snapshot spans several servers,
**When** it is saved and displayed,
**Then** its global order, occurrence identities and chosen server/track references remain local and intact,
**And** saving neither merges copies across servers nor requires each server to support playlist writes.

**Given** a track was technically interrupted or its remote feedback/reporting remains unresolved,
**When** snapshot inclusion is evaluated,
**Then** the documented local listening-disposition policy determines inclusion without misclassifying technical failure as an explicit rejection,
**And** server report success is not used as a substitute for whether the user accepted or skipped the occurrence.

**Given** a saved snapshot exists,
**When** the user opens it or restarts HifiMule,
**Then** the same saved selection is available for inspection with source labels and stable ordering,
**And** temporarily unavailable sources do not remove entries. History/snapshot reads are paged rather than requiring the entire result in UI memory.

**Given** the user saves an empty eligible selection or persistence fails,
**When** the operation finishes,
**Then** the UI explains that no snapshot was saved or reports the recoverable error,
**And** no partial snapshot is presented as complete and existing saved snapshots remain intact.

**Given** Windows, macOS and Linux builds,
**When** tests cover current/history boundary races, manual repeats, skips/dislikes, preview, mixed sources, unavailable sources and interrupted persistence,
**Then** they verify the exact expected immutable sequence and recovery behavior,
**And** saving produces no provider writes or device basket changes.

**Implementation gate:** Before coding, define snapshot identity and atomic capture boundary, local naming/listing access, disposition inclusion rules, duplicate-request retention and paged persistence. Resolve incomplete/technically failed occurrence treatment and preview-only sessions explicitly. Preserve the approved accepted-history/current/upcoming formula and keep snapshots distinct from live server playlists or an automatically synchronized queue.


### Story 16.10: Save a listening snapshot as playlists on its source servers

As a HifiMule user,
I want to save a listening snapshot to playlists on its contributing servers,
So that I can reuse my discoveries in other clients while understanding what was saved on each server.

**Requirements:** FR79; export portion of FR81; P-NFR2 and P-NFR4–6; P-AR11 and playlist-write portion of P-AR9; P-UX-DR8 and P-UX-DR14–15.

**Dependencies:** The delivered Epic 15 foundation (Stories 15.1–15.17) and Stories 16.1–16.9. Uses immutable local snapshots and existing provider playlist capabilities. Physical basket export remains a separate story.

**Acceptance Criteria:**

**Given** a saved snapshot contains tracks from one playlist-capable server,
**When** the user explicitly chooses Save to server playlists and supplies a valid name,
**Then** HifiMule creates a playlist containing that server's included occurrences in snapshot order,
**And** deliberate repeated occurrences are preserved where supported. If the provider cannot represent them faithfully, that limitation is disclosed rather than silently claiming an exact save.

**Given** a snapshot contains tracks from multiple servers,
**When** export runs,
**Then** it creates one playlist per contributing capable server containing only that server's referenced tracks in relative snapshot order,
**And** the complete cross-server order remains in the local snapshot. No tracks are copied or uploaded between servers.

**Given** a contributing server lacks playlist creation or required write permission,
**When** export capabilities are evaluated,
**Then** its part is shown as unsupported or denied with the reason,
**And** other capable parts can proceed without misrepresenting the overall export as fully successful.

**Given** the user confirms the explicit save action,
**When** the daemon submits provider requests,
**Then** each request uses the snapshot's chosen source identities and existing credentials,
**And** browsing another server or continued listening cannot redirect or change the exported content. Naming collisions do not overwrite an existing playlist implicitly.

**Given** one server succeeds and another fails,
**When** results are displayed,
**Then** each part shows its actual success, failure, unsupported or unresolved state and any known created playlist identity,
**And** retry targets unfinished work without recreating already confirmed playlists.

**Given** creation or population times out after a request may have reached the server,
**When** recovery evaluates the operation,
**Then** durable state records the ambiguity and attempts only provider-supported idempotency or reliable reconciliation,
**And** it does not blindly create another playlist or promise exactly-once remote effects. If reconciliation cannot establish the result, the UI exposes the uncertainty and an explicit recovery path.

**Given** a provider requires separate create and populate operations or batched track additions,
**When** an intermediate step fails,
**Then** HifiMule retains the known playlist identity and confirmed progress with an accurate partial result,
**And** resumption avoids duplicating confirmed entries. If that cannot be established safely, it remains unresolved instead of guessing. No remote playlist is deleted as an implicit rollback.

**Given** the application exits or loses network connectivity during export,
**When** it restarts or reconnects,
**Then** it recovers the persisted operation state tied to the same immutable snapshot,
**And** retry backoff, batch sizes and in-memory work are bounded without blocking audio callbacks or playback controls.

**Given** referenced source tracks disappear or provider limits prevent an exact export,
**When** export encounters the limitation,
**Then** it exposes the affected part and any partial contents accurately,
**And** it does not silently substitute another server copy, drop entries or reorder the result while calling it complete.

**Given** Windows, macOS and Linux builds and enabled playlist providers,
**When** tests cover mixed sources, repeats, naming collisions, unsupported writes, partial population, ambiguous responses and restart,
**Then** request fixtures verify order/routing and configured-server checks verify actual saved contents and safe retry behavior,
**And** logs and UI never expose credentials or authenticated stream URLs.

**Implementation gate:** Before coding, verify provider create/populate limits, duplicate handling, naming policy, operation identity, batch checkpoints and ambiguity reconciliation. Define user-visible partial-result recovery. Reuse immutable snapshot storage and existing playlist provider methods; split provider enablement before execution if needed to retain single-session story scope.


### Story 16.11: Add a listening snapshot to a connected device basket or replace it

As a HifiMule user,
I want to add my saved listening selection to a connected device basket or replace that basket,
So that I can take the music with me using HifiMule's existing sync workflow.

**Requirements:** FR80; applicable operation integrity portion of FR81; P-NFR4–6; physical-target portion of P-AR11; P-UX-DR9 and P-UX-DR14–15.

**Dependencies:** The delivered Epic 15 foundation (Stories 15.1–15.17) and Stories 16.1–16.10 and existing device-basket behavior. Uses the immutable snapshot from Story 16.9; it does not start sync or write music to the device.

**Acceptance Criteria:**

**Given** a saved snapshot and an eligible connected physical device,
**When** basket export actions are displayed,
**Then** the user can explicitly choose Add to basket or Replace basket with the intended device clearly identified,
**And** Playback itself is not offered as a physical target. With no connected eligible device, both actions are unavailable with an explanation.

**Given** the user chooses Add to basket,
**When** the operation is accepted,
**Then** the snapshot's compatible entries are incorporated using the documented existing basket identity/order rules while retaining existing basket content,
**And** any representational limits, including duplicate handling, are disclosed before claiming a faithful transfer. The immutable snapshot remains unchanged.

**Given** the user chooses Replace basket,
**When** the operation is accepted,
**Then** the intended basket selection is replaced with the validated snapshot selection as one consistent mutation,
**And** it does not append accidentally, affect other device baskets or delete files from the connected device. The action label and target make the replacement scope explicit.

**Given** a snapshot spans sources or contains entries the target basket cannot represent,
**When** export is validated,
**Then** the existing basket/provider constraints are checked before any replacement,
**And** unsupported content is explained without silent source substitution or partial replacement presented as complete. This story does not bypass server-specific basket locks.

**Given** the target disconnects, changes identity, becomes unconfigured or the UI selects another device while export is pending,
**When** the mutation is about to commit,
**Then** the daemon revalidates the originally selected physical target and applicable basket state,
**And** it rejects an invalid target rather than writing to the newly selected device or treating a stale mount path as proof of identity.

**Given** another action modifies the target basket during export preparation,
**When** the snapshot mutation is submitted,
**Then** concurrency validation prevents Replace from overwriting an unseen newer selection,
**And** the user receives a recoverable conflict rather than an automatically replayed destructive mutation. Add follows the documented safe concurrency policy.

**Given** basket mutation fails or the daemon exits before its result is acknowledged,
**When** the operation is recovered or retried,
**Then** its result can be determined from the existing basket persistence/operation contract,
**And** an Add retry does not blindly duplicate entries or leave a partially applied replacement. Failure preserves the last valid basket state.

**Given** export succeeds,
**When** the basket and listening session are inspected,
**Then** the intended basket reflects the action and provides visible completion feedback,
**And** playback and the saved snapshot remain unchanged. Sync starts only through the user's separate existing sync action; later Radio changes do not update the basket automatically.

**Given** Windows, macOS and Linux builds,
**When** tests cover Add, Replace, no device, blank device, incompatible sources, target switch/disconnection, concurrent edits and retry after interruption,
**Then** they verify the intended basket contents and preservation of all other baskets and device files,
**And** actions remain keyboard-accessible with clear target and error announcements.

**Implementation gate:** Before coding, inspect existing basket representation, source locks, duplicates/order semantics and mutation persistence. Define target identity/revalidation and concurrency policy without changing approved device safety guarantees. Resolve unsupported snapshot content explicitly; do not quietly expand this story into cross-server basket redesign or automatic synchronization.


### Story 16.12: Adapt playback quality at track boundaries using buffer health

As a HifiMule user,
I want playback to choose the best quality my connection can sustain,
So that I can keep listening with minimal interruption without manually tuning streaming settings.

**Requirements:** FR71; boundary adaptation and insufficient-bandwidth portions of FR72; P-NFR2 and P-NFR4–6; quality portions of P-AR9 and P-AR12; quality-status portion of P-UX-DR7.

**Dependencies:** The delivered Epic 15 foundation (Stories 15.1–15.17) and Stories 16.1–16.11. Extends initial source ranking and bounded buffering. Mid-track representation replacement remains disabled unless separately validated; seeking support alone does not enable it. Sync backoff and sustained release validation follow separately.

**Acceptance Criteria:**

**Given** a source exposes verified playable quality alternatives,
**When** a track is prepared,
**Then** HifiMule selects the highest sustainable available representation using the documented quality ranking and measured refill/buffer evidence,
**And** portable-device transcoding preferences do not influence that choice. Unknown connection capacity follows the documented startup policy with bounded buffering rather than an invented throughput estimate.

**Given** buffer depletion and refill measurements show that current quality is unsustainable,
**When** the next track's representation is selected,
**Then** the daemon chooses a lower supported sustainable alternative when available,
**And** it uses hysteresis and a defined observation window to avoid oscillating on isolated samples. Reduced quality has a discreet accessible explanation.

**Given** sustained connection recovery provides enough evidence for a higher quality,
**When** a later track is prepared,
**Then** HifiMule upgrades conservatively under the documented recovery threshold,
**And** one brief fast transfer does not force an immediate upgrade that repeatedly empties the buffer.

**Given** an active track is playing at one representation,
**When** the estimator changes its recommendation,
**Then** this story does not replace that representation mid-track,
**And** the next boundary applies the new choice without corrupting position, padding, gain or occurrence identity. Existing prepared album continuity remains required when both tracks are ready.

**Given** the next track was already prefetched at a different quality,
**When** a revised choice is considered,
**Then** the implementation follows a documented preparation cutoff and bounded replacement policy,
**And** it does not discard ready audio so late that it unnecessarily creates a gap or allow obsolete preparation to win a generation race.

**Given** a provider has no lower representation or none can be sustained,
**When** the buffer cannot support continued playback,
**Then** HifiMule exposes buffering/retry and the actual limitation,
**And** it neither promises uninterrupted playback nor silently skips an album track. Radio retains its established unavailable-source policy, with technical failures separate from user rejection.

**Given** compressed downloads are bursty, cached, served by different sources or require server transcoding startup,
**When** the estimator processes measurements,
**Then** it separates relevant source/representation observations and accounts for startup/cached samples under the documented policy,
**And** it does not confuse decoded PCM depth, device-sync write bandwidth or a fast cache hit with sustained server delivery capacity.

**Given** adaptation runs during seek, preview, output loss, Stop or session replacement,
**When** measurement and preparation work completes,
**Then** the existing generation and paused-state rules still govern output,
**And** adaptation cannot restart stopped audio, reroute outputs or submit an extra completed-listen report for the same occurrence.

**Given** controlled network profiles representing stable fast service, sustained slowdown, recovery, jitter and outage,
**When** deterministic and provider integration tests run on Windows, macOS and Linux,
**Then** results record selected representations, buffer bounds, switching frequency, buffering events and actual recovery behavior against defined thresholds,
**And** improved reliability is demonstrated rather than inferred from a quality label alone. Memory stays within separately defined compressed/PCM limits.

**Implementation gate:** Before coding, verify provider alternatives and quality ordering; define startup assumptions, estimator scope, observation windows, depletion/recovery thresholds, preparation cutoff and maximum buffer budgets from measurements. Keep authenticated URLs daemon-side. Do not claim codec bitrate alone establishes comparative quality, and do not enable mid-track replacement without a separate provider/format validation decision.


### Story 16.13: Protect playback during sync without unnecessary throttling

As a HifiMule user,
I want music playback and device synchronization to run together,
So that preparing a device does not interrupt listening or slow down unnecessarily when resources are sufficient.

**Requirements:** FR75; P-NFR1–4; coexistence portions of P-AR6 and P-AR14.

**Dependencies:** The delivered Epic 15 foundation (Stories 15.1–15.17) and Stories 16.1–16.12 and the existing sync scheduler/cancellation boundaries. Reuses playback buffer measurements. This story adjusts sync demand conditionally; it does not replace adaptive streaming or change device write-integrity rules.

**Acceptance Criteria:**

**Given** playback buffers are healthy while a device sync runs,
**When** the sync scheduler admits work,
**Then** normal sync throughput/concurrency is retained,
**And** starting playback alone does not impose a permanent bandwidth cap, pause sync or classify slow device writes as a server-streaming bottleneck.

**Given** sustained measured contention puts playback delivery at risk,
**When** the documented protection threshold is reached,
**Then** the scheduler reduces applicable sync demand at safe scheduling boundaries,
**And** the control does not acquire sync locks from the audio callback or interrupt an atomic device metadata write.

**Given** protection is active and playback health recovers,
**When** the recovery observation window is satisfied,
**Then** sync demand returns toward its normal level under the documented recovery policy,
**And** hysteresis prevents repeated throttle/release oscillation. Pausing or ending playback cannot leave a stale restriction indefinitely.

**Given** the limiting resource is slow device write bandwidth and playback remains healthy,
**When** coexistence is monitored,
**Then** no playback-driven backoff is applied solely because sync progress is slow,
**And** diagnostics distinguish source delivery, decoding/output health and physical-device throughput without claiming causality from one metric alone.

**Given** the source connection is too slow even without sync contention,
**When** backoff cannot improve delivery,
**Then** playback retains its quality-adaptation/buffering behavior and the protection policy avoids indefinite unjustified suppression of sync,
**And** it does not promise uninterrupted audio or silently cancel synchronization.

**Given** several sync jobs or sources are active,
**When** protection is required,
**Then** the documented policy targets relevant resource demand and preserves fair progress where possible,
**And** unrelated jobs are not blindly stopped. Existing cancellation, retry and managed-file integrity contracts continue to apply.

**Given** the user seeks, changes output, stops playback or quits while backoff is active,
**When** those transitions complete,
**Then** stale protection signals cannot revive a replaced session or leave the scheduler permanently restricted,
**And** Quit still follows safe audio checkpoint and sync cancellation behavior.

**Given** reproducible scenarios with a slow device, constrained server delivery, CPU contention and sufficient resources,
**When** real playback and real sync are measured together on Windows, macOS and Linux,
**Then** evidence records underruns/buffering, sync throughput, protection decisions and recovery against playback-only and sync-only baselines,
**And** a healthy device-limited scenario shows no unnecessary playback-triggered throttling. Claims of improved continuity require output evidence, not just an enabled protection flag.

**Implementation gate:** Before coding, identify the existing safe scheduler controls and define risk/recovery thresholds, resource attribution, fairness, minimum progress and stale-signal expiry from measured scenarios. Set acceptable throughput impact for healthy coexistence before acceptance. Do not introduce blocking coordination into the audio callback or weaken device integrity to meet a throughput target.


### Story 16.14: Keep long listening sessions bounded and recoverable

As a HifiMule user,
I want long album and Radio sessions to remain responsive and recoverable,
So that leaving music playing through a workday does not progressively consume memory or lose my session after interruptions.

**Requirements:** Sustained evidence for P-NFR1–4; FR60 and FR65–75 regression; resource portions of P-AR12 and P-AR14.

**Dependencies:** The delivered Epic 15 foundation (Stories 15.1–15.17) and Stories 16.1–16.13. Validates integrated runtime behavior and fixes bounded resource/recovery defects found within this scope. Reuse the packaging process established in Story 15.17 and own the expanded installed shipping-platform matrix for Epic 16. Larger unrelated defects become explicitly linked blockers rather than silently expanding this story.

**Acceptance Criteria:**

**Given** documented active-playback resource budgets and a representative long-session workload,
**When** album and Radio playback run for the defined soak duration,
**Then** compressed prefetch, decoded PCM, upcoming automatic entries, candidate caches and in-memory history stay within their respective bounds,
**And** measurements distinguish idle from active memory rather than applying the existing idle target to an active decoder.

**Given** Radio cycles, skips, removals and manual queue edits accumulate over a long session,
**When** history and logical-session exclusions grow,
**Then** the full required exclusion behavior persists through bounded/paged access,
**And** neither the daemon nor reopened UI loads the whole history into memory. Storage growth and cleanup policy remain explicit without erasing an active logical session's exclusions.

**Given** repeated seeks, previews, track replacements and output changes occur during the soak,
**When** obsolete work is cancelled,
**Then** decoder tasks, output streams, file handles and pending requests are released under documented limits,
**And** no obsolete audio or state appears after the accepted session generation changes.

**Given** source outages, slow responses and reporting/export failures are introduced,
**When** playback and operation recovery continue,
**Then** retry work and in-memory operation queues remain bounded while durable unresolved state is retained according to policy,
**And** restoration or reconciliation does not blindly duplicate remote writes or turn technical failures into user rejections.

**Given** the system sleeps/wakes or the output disconnects during a long session,
**When** HifiMule resumes handling events,
**Then** it reconciles actual output/source availability and preserves recoverable session state,
**And** output loss never causes automatic rerouting or an unrequested restart of paused audio. Sleep-specific transport behavior is defined and verified on each tested OS.

**Given** a running session experiences orderly Quit, an abrupt daemon exit or interrupted checkpointing,
**When** HifiMule restarts,
**Then** it restores valid committed state paused, retaining Radio identity/exclusions and the applicable preview/main recovery behavior,
**And** corrupt or unsupported state remains recoverable and visibly diagnosed rather than silently overwritten.

**Given** prepared album playback runs with a representative real-device sync workload,
**When** sustained continuity and sync protection are measured,
**Then** the evidence includes physical-output continuity, underruns/buffering, sync throughput and protection recovery,
**And** callback counters alone are not used to certify audible continuity.

**Given** Windows, macOS and Linux test environments,
**When** the same documented workload and fault schedule run,
**Then** results record duration, hardware/VM status, build/runtime versions, resource time series and recovery outcomes against budgets fixed before acceptance,
**And** a failed budget or untested scenario remains an explicit blocker or limitation rather than being hidden by a short successful run.

**Given** installed builds on every shipping platform/architecture,
**When** automatic selection, Radio, reporting/feedback, snapshots, playlist/basket exports, adaptation and conditional sync protection are exercised,
**Then** record capability-dependent source routing, accessibility, failure/recovery and runtime-version evidence for all added workflows,
**And** changed dependencies or affected workflows receive fresh checks. Story 15.17 evidence does not certify new features; actual publication is separate. Split platform execution tasks during preparation if needed.

**Implementation gate:** Before execution, set soak duration, sampling interval, memory/task/handle budgets, workload sizes, fault schedule, acceptable growth and evidence method using earlier measurements. Investigate reproducible growth or recovery failures, rerun affected scenarios after fixes, and retain raw evidence locations. Do not invent passing thresholds after seeing the results.


## Epic 17: Audiobookshelf Integration

Add Audiobookshelf as a multi-server provider for independently selected Books and Podcasts
libraries. This epic delivers the complete phased roadmap: audiobook catalog and direct playback;
player-centric progress continuity; podcast catalog and direct playback; local synchronization and
Autofill; then read-only series/collection grouping and compatibility feedback. Audiobooks and
podcasts remain distinct catalogue models, server roles, budgets, and selection behavior.

**Dependencies:** Epic 8’s provider/multi-server foundation and Epic 15’s direct album/track
playback. Epic 15 release verification and Epic 16 Radio work remain independently sequenced.

**Out of scope:** editable remote collections, remote collection write-back, and
Audiobookshelf-specific folder or collection filtering.

### Story 17.1: Validate the Audiobookshelf integration contract

As a developer, I want a versioned, fixture-backed Audiobookshelf integration contract,
So that implementation does not depend on inferred endpoint or item-identity behavior.

**Acceptance Criteria:** Validate authentication, library discovery/type, Books and Podcasts
catalog/search pagination, book/part ordering, author/narrator/artwork fields, stable IDs, stream
and transcode behavior, progress endpoints, and failure semantics against supported server
versions. Store redacted fixtures and record unsupported or ambiguous behavior. Do not enable a
user-visible provider before this contract exists.

### Story 17.2: Connect Audiobookshelf libraries as independent servers

As a user, I want to authenticate to Audiobookshelf and select one library for a server,
So that Books and Podcasts libraries become independent HifiMule servers.

**Acceptance Criteria:** Credentials use the encrypted vault; endpoint type, library ID, and role
persist with the stable HifiMule server identity; Books create audiobook servers and Podcasts
create podcast servers; multiple libraries from one endpoint are supported; restart, upsert,
authentication failure, and sanitized logging follow existing provider rules. No folder or
collection picker appears.

### Story 17.3: Map Audiobookshelf books faithfully into the HifiMule catalog

As a listener, I want books represented as ordered albums with accurate credits,
So that I can find and play long-form audio naturally.

**Acceptance Criteria:** Each book maps to an album; ordered parts/chapters map to deterministic
tracks; a single-file book maps to one track; artwork, author-primary and narrator-secondary
credits are preserved; stable remote IDs are retained as provider metadata; pagination/search and
remote removal use normal provider conventions; fixtures cover ordering and metadata edge cases.

### Story 17.4: Browse and search an Audiobookshelf audiobook server

As a listener, I want to browse and search my audiobook library,
So that I can choose books without confusing them with music or podcasts.

**Acceptance Criteria:** Reuse existing browse/search/RPC capability conventions; provide
accessible book/chapter labels and author/narrator hierarchy; explain empty, loading,
unavailable, and stale-source states; preserve existing provider behavior. Do not introduce
podcast views or series/collection playlists in this story.

### Story 17.5: Directly play Audiobookshelf audiobooks through HifiMule

As a listener, I want to play a selected book or chapter through the existing playback path,
So that Audiobookshelf delivers immediate listening value without device synchronization.

**Acceptance Criteria:** Direct playback honors ordered tracks and existing session/output safety;
authenticated URLs stay daemon-side; only verified compatible delivery paths are used; unavailable
or incompatible media returns a truthful recoverable error. Do not write progress, transfer media,
or run Autofill yet.

### Story 17.6: Preserve Audiobookshelf listening continuity safely

As a listener, I want HifiMule playback to resume and report a book at the correct whole-book position,
So that I can move safely between HifiMule and Audiobookshelf.

**Acceptance Criteria:** Persist stable item identity plus whole-item offset; read remote position
when HifiMule begins playback; translate whole-item and chapter/track positions; report position
or completion only while HifiMule plays a proven matching item; reject missing/stale mappings; do
not add background-sync progress import or propagation.

### Story 17.7: Add Audiobookshelf podcast servers and direct playback

As a listener, I want to select a Podcasts library and browse/play shows and episodes,
So that podcasts work without being forced into audiobook or album semantics.

**Acceptance Criteria:** A selected Podcasts library has an independent role and budget;
show/episode mapping and presentation are distinct from book mapping; browse/search/direct
playback use existing safety contracts; audiobook and podcast catalogues never leak into one
another.

### Story 17.8: Synchronize Audiobookshelf media with independent policies

As a portable-listening user, I want audiobooks and podcasts to sync under appropriate rules,
So that durable books and changing episode feeds fit my device.

**Acceptance Criteria:** Audiobook servers reuse existing per-server selection, budget, and remote
removal reconciliation. Podcast servers use a capacity-managed recent/unplayed retention policy as
an existing Autofill source. Block incompatible media before transfer with an explanation; give each
server an independent budget; do not promise protection for unobservable external playback.

### Story 17.9: Refine Audiobookshelf grouping and compatibility feedback

As a listener, I want source groupings and compatibility feedback that make the integration clear,
So that I can curate and synchronize confidently as the library changes.

**Acceptance Criteria:** Import series and collections as read-only playlists; progressively
disclose direct/transcoded compatibility where probing is cheap and reliable, otherwise explain
eligibility during sync planning; preserve responsive browsing. Collection editing/write-back and
source-specific folder selection remain unavailable.

## Playback Story Coverage and Readiness

All 31 stories across 15.1–15.17 and 16.1–16.14 have planning approval under the [2026-09-19 amendment](sprint-change-proposal-2026-09-19.md). See [playback-epic-validation.md](playback-epic-validation.md) for requirement coverage, dependency checks and readiness limits. Stories 15.1–15.14 remain done; new and moved stories remain backlog. Implementation contracts and actual installed evidence must still be completed by their owners.
