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

### Story 2.3: Multi-Device Profile Mapping

As a Convenience Seeker (Sarah),
I want the tool to remember that my Garmin watch belongs to my "Running" Jellyfin profile,
So that my sync rules are applied automatically on connection.

**Acceptance Criteria:**

**Given** a known device (has `.jellyfinsync.json` with a unique ID) is connected
**When** the daemon reads the ID
**Then** it automatically loads the associated Jellyfin User Profile and Sync Rules.

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

### Story 4.5: "Start Sync" UI-to-Engine Trigger

As a Convenience Seeker (Sarah) and Ritualist (Arthur),
I want to click a "Start Sync" button in the Sync Basket sidebar to initiate the synchronization process with the daemon,
So that I can execute my prepared sync selection and monitor real-time progress without leaving the UI.

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
