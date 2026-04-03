stepsCompleted: ['step-01-validate-prerequisites', 'step-02-design-epics', 'step-03-create-stories', 'step-04-final-validation']
inputDocuments: ['prd.md', 'architecture.md', 'ux-design-specification.md', 'product-brief-bmad-2026-01-26.md', 'project-context.md']
status: 'complete'
completedAt: '2026-01-27'

# JellyfinSync - Epic Breakdown

## Overview

This document provides the complete epic and story breakdown for JellyfinSync, decomposing the requirements from the PRD, UX Design, and Architecture requirements into implementable stories.

## Requirements Inventory

### Functional Requirements

FR1: Automatically detect Mass Storage devices (USB) on Windows, Linux, and macOS.
FR2: Manually select a target device folder if automatic detection fails.
FR3: Identify the presence of a `.jellyfinsync.json` manifest on discovery.
FR4: Read persistent hardware identifiers to link devices across different sessions.
FR5: Configure Jellyfin server credentials (URL, username, token).
FR6: Select a specific Jellyfin user profile for syncing.
FR7: Maintain a persistent, encrypted connection state to the Jellyfin server.
FR8: Browse Jellyfin Playlists, Genres, and Artists within the UI.
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

### NonFunctional Requirements

NFR1: Memory footprint < 10MB during idle states.
NFR2: Complete manifest audit and be "ready to sync" in < 5 seconds.
NFR3: Sync throughput limited only by target hardware or network bandwidth.
NFR4: Utilize OS-level file sync primitives (sync_all) for data integrity.
NFR5: Atomic `.jellyfinsync.json` manifest updates.
NFR6: Network interruption handling with at least 3 retry cycles.
NFR7: Graceful "Interrupted" session marking and repair utility trigger on mid-sync disconnect.
NFR8: 100% feature parity between Windows, Linux, and macOS.
NFR9: macOS sandbox compliance (no root/sudo required).
NFR10: Resource usage within 15% delta across OS environments.
NFR11: Encrypted credential storage via OS-native vaults using the `keyring` crate.
NFR12: Pure local media synchronization (zero third-party data transmission).
NFR13: CLI-first architecture for the core sync engine.

### Additional Requirements

- **Multi-process Architecture:** Rust Daemon + Tauri v2 UI (Detachable).
- **IPC Mechanism:** JSON-RPC 2.0 over localhost HTTP.
- **Data Persistence:** SQLite (`rusqlite`) for daemon state and scrobble history.
- **Project Structure:** Rust Cargo Workspace containing `jellyfinsync-daemon` and `jellyfinsync-ui`.
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
FR7: Epic 2 - Persistent Server Token (Keyring)
FR8: Epic 3 - Jellyfin Library Browser
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
FR27: Epic 6 - Platform-Native Installer Bundling
FR28: Epic 6 - CI/CD Cross-Platform Build Pipeline
FR29: Epic 3 - Auto-Fill Virtual Slot (Story 3.8)
FR30: Epic 2 - Auto-Sync on Known Device Detection
FR31: Epic 4 - Transcoding Handshake (Story 4.8)
FR32: Epic 4 - Transcoding Profile RPC (Story 4.8)
FR26: Epic 2 - Device Identity (Story 2.9)
FR33: Epic 2 - Enhanced Multi-Device Hub (Story 2.8)
FR34: Epic 3 - Artist Entity Basket Item (Story 3.9)

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
**Then** the workspace successfully compiles both `jellyfinsync-daemon` and `jellyfinsync-ui` (Tauri).
**And** `jellyfinsync-daemon` starts as a standalone headless binary.

### Story 1.2: Cross-Platform System Tray Hub

As a Convenience Seeker (Sarah),
I want a persistent system tray icon with status indicators,
So that I can monitor the sync engine's health (Idle/Syncing/Error) without opening the main window.

**Acceptance Criteria:**

**Given** the `jellyfinsync-daemon` is running
**When** I check the system taskbar/menu bar
**Then** I see the JellyfinSync icon.
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

### Story 2.1: Secure Jellyfin Server Link

As a System Admin (Alexis),
I want to securely store my Jellyfin URL and credentials in the OS-native keyring,
So that I don't have to re-enter them and my tokens are safe from other users.

**Acceptance Criteria:**

**Given** the UI is open in "Settings"
**When** I enter a valid Jellyfin URL and User Token
**Then** the daemon validates the connection via the `/System/Info` API.
**And** the token is encrypted and stored in the system Keyring (Windows Credential Manager / macOS Keychain).

### Story 2.2: Mass Storage Heartbeat (Autodetection)

As a Ritualist (Arthur),
I want the daemon to "WAKE UP" the moment I plug in my iPod,
So that I don't have to manually hunt for folder paths.

**Acceptance Criteria:**

**Given** the daemon is running in the tray
**When** a USB Mass Storage device is connected
**Then** the daemon triggers a "Device Detected" event.
**And** it checks for the presence of a `.jellyfinsync.json` manifest in the root directory.

### Story 2.3: Multi-Device Profile Mapping & Auto-Sync Trigger

As a Convenience Seeker (Sarah),
I want the tool to remember that my Garmin watch belongs to my "Running" Jellyfin profile and automatically start syncing,
So that I can plug in and walk away without any interaction.

**Acceptance Criteria:**

**Given** a known device (has `.jellyfinsync.json` with a unique ID) is connected
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

**Given** the `jellyfinsync-ui` is launched
**When** the application is initializing (loading daemon, checking connection)
**Then** a native Tauri splash screen featuring the JellyfinSync logo and name is displayed.
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
**Then** I can enter a Jellyfin URL (with optional "Auto-detect" for local servers).
**And** I can enter my Username and Password.
**When** I click "Connect"
**Then** the daemon attempts to authenticate and retrieve a session token.
**And** the token is securely stored in the system Keyring (replacing any existing token).
**And** the UI transitions to the main Library Browser on success.
**When** authentication fails
**Then** a clear error message is shown (e.g., "Invalid Credentials" or "Server Unreachable").

### Story 2.6: Initialize New Device Manifest

As a Ritualist (Arthur) and Convenience Seeker (Sarah),
I want the application to detect when a connected removable disk has no `.jellyfinsync.json` manifest and guide me through initializing it,
So that I can bring a brand-new device into the managed sync model without manually creating any files.

**Acceptance Criteria:**

**Given** a USB mass storage device is connected with no `.jellyfinsync.json` present in its root
**When** the daemon completes its device discovery scan
**Then** it broadcasts an `on_device_unrecognized` event to the UI.
**And** the UI displays an "Initialize Device" banner in the Device State panel.

**Given** the "Initialize Device" banner is visible
**When** I click "Initialize"
**Then** a dialog prompts me to confirm or change the target sync folder path on the device (defaulting to the device root).
**And** I can select the associated Jellyfin user profile for this device.
**When** I click "Confirm"
**Then** the UI sends a `device.initialize` JSON-RPC request to the daemon with the chosen folder path and profile ID.
**And** the daemon writes an initial `.jellyfinsync.json` to the device using the atomic Write-Temp-Rename pattern, containing a new unique hardware ID and the selected profile.
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

## Epic 3: The Curation Hub (Basket & Library)

Develop the high-confidence Library Browser and Selection Basket with storage projection.

### Story 3.1: Immersive Media Browser (Jellyfin Integration)

As a Ritualist (Arthur),
I want to browse my Jellyfin playlists and albums with high-quality artwork,
So that I can enjoy the curation process as I do on the server.

**Acceptance Criteria:**

**Given** a successful server link
**When** I open the main UI
**Then** I see the "Vibrant Hub" layout with paginated album art grids.
**And** items already on the device are marked with a "Synced" badge.

### Story 3.2: The Live Selection Basket

As a Convenience Seeker (Sarah),
I want to click items and have them "collect" in a sidebar,
So that I can see exactly what I'm about to sync without committing yet.

**Acceptance Criteria:**

**Given** the Library Browser
**When** I click the `(+)` on an album or playlist
**Then** the item is added to the "Sync Basket" sidebar.
**And** the sidebar displays the "Intent Overlay" (e.g., `+12 Tracks`).

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
**When** browsing the library in JellyfinSync
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

**Technical Notes:**
- Remove from `BasketSidebar.ts`: `triggerAutoFill()`, `scheduleAutoFill()`, `autoFillInFlight`, `autoFillPendingRetrigger`, `autoFillDebounceTimer`, `isAutoFillLoading`, `basketStore.replaceAutoFilled()`
- Toggle inserts a single `{ id: '__auto_fill_slot__', type: 'AutoFillSlot', maxBytes: N }` virtual item into `basketStore`
- `basket.autoFill` RPC: retained as preview/debug endpoint, no longer called by UI for basket population
- `sync.start` RPC handler: if request contains `autoFill: { enabled: true, maxBytes?, excludeItemIds[] }`, call `run_auto_fill()` and merge results with `itemIds` before executing — mirrors existing daemon-initiated path (`main.rs:503`)
- `sync.start` params gain: `autoFill?: { enabled: boolean, maxBytes?: number, excludeItemIds: string[] }`
- Auto-fill preferences (enabled, maxBytes) continue to be persisted via `sync.setAutoFill`

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

### Story 4.1: Differential Sync Algorithm (Manifest Comparison)

As a System Admin (Alexis),
I want the engine to calculate exactly which files to add or delete by comparing the Jellyfin server state with the local `.jellyfinsync.json` manifest,
So that only necessary changes are made to the disk, preserving the hardware's life.

**Acceptance Criteria:**

**Given** a Selection Basket with 50 items
**When** the sync engine starts
**Then** it generates a list of "Adds" and "Deletes" based on the `.jellyfinsync.json` record.
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
- Tray tooltip remains "JellyfinSync: Syncing…" — ETA is UI-only (known variance)

### Story 4.7: Playlist M3U File Generation

As a Ritualist (Arthur) and Convenience Seeker (Sarah),
I want `.m3u` playlist files to be written to my device when I sync a Jellyfin playlist,
So that my DAP or Rockbox player can natively load and play the playlist in the correct order.

**Acceptance Criteria:**

**Given** at least one basket item has `item_type = "Playlist"` and sync runs successfully
**When** sync completes
**Then** a `.m3u` file is written to the managed sync folder for each playlist.
**And** the filename is sanitized via `sanitize_path_component()` and truncated to 255 characters if needed.

**Given** a playlist `.m3u` is being written
**When** generating its contents
**Then** the file begins with `#EXTM3U`.
**And** each track has an `#EXTINF:<seconds>,<Artist> - <Title>` line followed by its relative path (relative to the `.m3u` file location, forward slashes).
**And** duration is `RunTimeTicks ÷ 10,000,000`; absent ticks → `-1`.

**Given** a playlist's track list is unchanged from the previous sync (same `trackCount` and `trackIds` hash in manifest)
**Then** the `.m3u` is not rewritten. If changed, it is regenerated atomically (Write-Temp-Rename).

**Given** a playlist was in the previous sync but is absent from the current basket
**Then** its `.m3u` file is deleted and its entry removed from `manifest.playlists`.

**Given** a playlist track's `jellyfin_id` is not found in `manifest.synced_items` (e.g. download failed)
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
**Then** the engine uses the existing `/Items/{id}/Download` path unchanged.

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

Package JellyfinSync into platform-native installers and establish automated cross-platform build pipelines.

### Story 6.1: Tauri Bundler Configuration & Sidecar Packaging

As a System Admin (Alexis),
I want the Tauri bundler configured to include the `jellyfinsync-daemon` binary as a sidecar,
So that a single installer delivers both the UI and the headless engine as a cohesive application.

**Acceptance Criteria:**

**Given** the Cargo workspace with both crates built
**When** I run `cargo tauri build`
**Then** the output produces a platform-native installer containing both the Tauri UI and the daemon sidecar.
**And** the installed application can launch the daemon from the bundled sidecar path.
**And** the application icon, name ("JellyfinSync"), and metadata are correctly embedded.

### Story 6.2: Windows Installer (MSI)

As a Ritualist (Arthur),
I want a standard Windows MSI installer,
So that I can install JellyfinSync like any other desktop application on my Windows PC.

**Acceptance Criteria:**

**Given** a successful `cargo tauri build` on Windows
**When** I run the generated MSI
**Then** JellyfinSync is installed to Program Files with Start Menu shortcuts.
**And** the daemon sidecar is placed alongside the main executable.
**And** uninstallation via "Add/Remove Programs" cleanly removes all installed files.

**Post-MVP: Daemon as Windows Startup Application**
**Given** the MSI installation completes
**When** the installer registers the startup entry
**Then** `jellyfinsync-daemon` is registered as a startup application via a Registry `Run` key (`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`).
**And** the daemon launches automatically when the user logs in, running in the user session with full tray icon and notification support.
**And** the UI detects the running daemon via a health-check RPC call instead of spawning a sidecar.
**And** if the daemon is not running, the UI attempts to launch it directly.
**And** uninstallation removes the Registry `Run` entry.

### Story 6.3: macOS Installer (DMG)

As a Convenience Seeker (Sarah),
I want a macOS DMG with drag-to-Applications install,
So that I can install JellyfinSync following standard macOS conventions.

**Acceptance Criteria:**

**Given** a successful `cargo tauri build` on macOS
**When** I open the generated DMG
**Then** I see the JellyfinSync app bundle with a drag-to-Applications prompt.
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
So that I can install JellyfinSync on both Debian-based systems and any Linux distro via AppImage.

**Acceptance Criteria:**

**Given** a successful `cargo tauri build` on Linux
**When** I run the AppImage
**Then** JellyfinSync launches without requiring installation.
**When** I install the .deb package
**Then** JellyfinSync is installed with a desktop entry and can be launched from the application menu.
**And** both formats include the daemon sidecar binary.

**Post-MVP: Daemon as systemd User Service**
**Given** the .deb package is installed
**When** the post-install script runs
**Then** a systemd user service unit is installed and enabled via `systemctl --user enable jellyfinsync-daemon`.
**And** the daemon starts automatically on user login.
**And** the UI detects the running daemon via a health-check RPC call instead of spawning a sidecar.
**And** if the service is not running, the UI attempts `systemctl --user start jellyfinsync-daemon`.
**And** package removal disables and removes the service unit.
**Note:** AppImage cannot register services; it falls back to the sidecar model.

### Story 6.5: CI/CD Cross-Platform Build Pipeline

As a System Admin (Alexis),
I want an automated GitHub Actions workflow that builds and publishes installers for all three platforms,
So that every release produces verified, downloadable artifacts without manual per-platform builds.

**Acceptance Criteria:**

**Given** a tagged release commit (e.g., `v0.1.0`) is pushed
**When** the GitHub Actions workflow triggers
**Then** it builds JellyfinSync on Windows, macOS, and Linux runners in parallel.
**And** each build produces the platform-native installer (MSI, DMG, AppImage, .deb).
**And** all artifacts are uploaded to a GitHub Release draft.
**And** the workflow fails clearly if any platform build fails.

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
