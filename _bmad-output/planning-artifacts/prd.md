stepsCompleted: ['step-01-init', 'step-02-discovery', 'step-03-success', 'step-04-journeys', 'step-05-domain', 'step-06-innovation', 'step-07-project-type', 'step-08-scoping', 'step-09-functional', 'step-10-nonfunctional', 'step-11-polish']
inputDocuments: ['product-brief-bmad-2026-01-26.md', 'project-context.md']
documentCounts: {briefCount: 1, researchCount: 0, brainstormingCount: 0, projectDocsCount: 0}
classification:
  projectType: 'Desktop App (Rust-based Headless Sync Engine + Detachable UI)'
  strategy: 'Event-Driven Mount Watcher + Manifest Probing'
  domain: 'Media Utility / General'
  complexity: 'Low (Performance-focused)'
  projectContext: 'Greenfield'
workflowType: 'prd'
---

# Product Requirements Document - JellyfinSync

**Author:** Alexis
**Date:** 2026-01-26

## Success Criteria

### User Success
- **Friction-Free Bridge:** Users connect hardware and sync with zero confusion or manual file management.
- **Hardware Safety:** Users feel confident that legacy hardware constraints (path-length limits, character sets) are automatically handled.
- **Data Integrity:** Users trust that the "Managed Sync" model will never touch or delete their personal unmanaged files.
- **Ecosystem Continuity:** Listen history from the device is reflected on the Jellyfin server, making the DAP feel part of the modern library.

### Business Success
- **Legacy Ecosystem Essential:** JellyfinSync becomes the top recommendation for the Rockbox/DAP community.
- **Cross-Platform Parity:** Identical user experience across Windows, Linux, and macOS with zero feature loss.

### Technical Success
- **Capped Idle Usage:** The Rust engine maintains a < 10MB RAM footprint during 72-hour idle state stress tests.
- **Buffered IO Stability:** Memory-to-disk buffering ensures peak USB write speeds without impacting host system responsiveness.
- **Atomic Manifest Updates:** The `.jellyfinsync.json` state is only committed after successful file verification.

### Measurable Outcomes
- **Time-to-Action:** < 5s from device detection to "Sync Ready" state (including manifest audit).
- **Incremental Efficiency:** < 10s for updates where 90%+ of media is already present on-device.
- **Scrobble Accuracy:** 100% correlation between `.scrobbler.log` entries and Jellyfin server play counts for correctly matched items.

## Product Scope

### MVP - Minimum Viable Product
- **Headless Rust Engine:** Performance-optimized core binary (Win/Linux/Mac).
- **Event-Driven Mount Watcher:** Instant detection of mass storage devices via OS-native notifications.
- **Basic Scrobbling (Direct):** Reading Rockbox `.scrobbler.log` and reporting finished tracks to Jellyfin via the `/Progress` API (one-way, fire-and-forget).
- **Hardware-Aware Validation:** Automated sanity checks for path-length limits (255 chars) and filename character-set compatibility.
- **Destructive Safety Protocol:** Mandatory manual user confirmation for any manifest-repair or cleanup operations exceeding 100MB of data deletion.
- **Conflict-Free Manifest Sync:** Implementation of the `.jellyfinsync.json` logic for managed-folder isolation.
- **Profile Selection:** UI/CLI support for selecting the correct Jellyfin user account for playlist/scrobble routing.
- **Auto-Fill Sync Mode:** Intelligent device-filling using a virtual basket slot. Enabling Auto-Fill places a single slot in the basket representing remaining capacity; the priority algorithm (favorites → play count → creation date) runs at sync time, not at basket-build time. Always uses the freshest library state. Can be mixed with manual basket selections.
- **Auto-Sync on Connect:** Known devices with auto-sync enabled trigger synchronization automatically on detection, requiring zero user interaction.

- **Transcoding Handshake:** Per-device profile selection for server-side re-encoding via Jellyfin PlaybackInfo API. Profiles stored in an editable `device-profiles.json` in the app data directory; passthrough (direct download) is the default.

### Growth Features (Post-MVP)
- **Scrobble Queue & Retry:** Robust handling for offline scrobbling when the server is unreachable during sync.
- **Repair Utility:** Guided GUI-based recovery for interrupted transfers or "de-synced" manifests.

## User Journeys

### Arthur's Weekly Ritual (Legacy Success)
*   **Narrative:** Every Saturday morning, Arthur plugs his beloved 160GB iPod Classic into his Linux desktop. The headless JellyfinSync engine detects the mount instantly. Arthur opens the UI, which automatically highlights 50 new tracks in his "Recently Added" Jellyfin playlist. He clicks "Sync".
*   **Success Moment:** The sync completes in under 20 seconds using the pre-calculated manifest. Arthur ejects the device, confident that his manual "Voice Memos" folder remains untouched.

### Sarah's Pre-Run Dash (Speed Success)
*   **Narrative:** At 6:00 AM, Sarah plugs in her Garmin watch on her way out the door. The daemon recognizes the device, auto-syncs her favorites and most-played tracks to fill the watch, and a tray notification confirms "Sync Complete" before she's finished tying her shoes.
*   **Success Moment:** She unplugs and leaves. Zero clicks. The auto-fill prioritized her favorite running tracks and the tool handled everything silently in the background.

### The "Silent Engine" (Admin Setup)
*   **Narrative:** Alexis sets up JellyfinSync on a new Mac Mini. He runs a simple wizard to connect to his Jellyfin server and selects his primary User Profile.
*   **Success Moment:** The Rust engine sits in the system tray, consuming negligible memory (< 10MB) while waiting for the next USB hardware connection.

### The Mid-Sync Eject (Edge Case Recovery)
*   **Narrative:** Arthur's cat trips over the USB cable mid-sync, disconnecting the iPod. The UI immediately displays a warning, and the Rust engine marks the local manifest as "Dirty".
*   **Success Moment:** Arthur reconnects the device. The **Repair Utility** checks the manifest, identifies the partially written files, and offers a one-click resume instead of a full re-sync.

## Innovation & Novel Patterns

### Detected Innovation Areas
- **Headless Engine + Detachable UI:** Distinguishing between the background "Sync Daemon" (Rust) and the "Selection UI" (TBD). This ensures zero-footprint operation while idle.
- **Auto-Pilot Policy (Invisible Sync):** A policy-driven model where the Rust engine automatically triggers sync and eject based on device-specific rules in the manifest, requiring zero user interaction after the first setup.
- **Event-Driven Mount Watcher:** Replacing manual folder-picking with OS-level hotplug detection for "invisible" operation.
- **The Scrobble Bridge:** A novel "History Sync" pattern that reconciles legacy `.scrobbler.log` files with the modern Jellyfin API without direct server-to-device communication.

### Market Context & Competitive Landscape
- **Generic Sync (rsync/Unison):** Low metadata awareness; unable to parse playlists or genres.
- **Heavy Media Managers (iTunes/MediaMonkey):** High resource footprint (150MB+ idle); often lack direct Jellyfin integration.
- **JellyfinSync's Position:** The only tool combining the "Leanness" of a CLI-first utility with the "Richness" of media-server metadata and background automation.

### Validation Approach
- **Sync Stress Test:** Validating the Rust engine against a simulated 10,000-file library across three OS platforms.
- **Memory Soak Test:** 72-hour automated monitoring to confirm zero-leak, < 10MB idle performance.
- **Auto-Pilot Reliability:** Iterative testing of mounting/unmounting events to ensure sync consistently triggers and completes without user intervention.

## Desktop App Specific Requirements

### Project-Type Overview
As a cross-platform desktop application, JellyfinSync consists of a performance-critical Rust-based sync engine (Headless) and a separate (detachable) user interface.

### Technical Architecture Considerations
- **Platform Support:** General support for Windows, Linux, and macOS.
- **Update Strategy:** Manual updates initially; no built-in auto-update mechanism for MVP.
- **Resource Management:** Strict < 10MB idle memory footprint.

### System Integration
- **System Tray:** A tray icon for status monitoring (Syncing, Error, Idle).
- **Launch on Startup:** Option to automatically launch the headless engine on system boot.
- **Notifications:** OS-native desktop notifications for Sync Completion and critical Errors.

## Project Scoping & Phased Development

### MVP Strategy & Philosophy
**MVP Approach:** Problem-Solving/Efficiency MVP. The objective is to demonstrate that a lightweight, Rust-based headless engine can manage legacy media synchronization with higher reliability and less friction than manual file management.

**Resource Requirements:** Solo developer with proficiency in Rust and system-level IO.

### MVP Feature Set (Phase 1)
**Core User Journeys Supported:**
- **Arthur's Weekly Ritual (Legacy Success):** Validating the core differential sync logic.
- **The "Silent Engine" (Admin Setup):** Establishing the low-footprint background service.

**Must-Have Capabilities:**
- **Rust Headless Engine:** The core synchronization logic.
- **Event-Driven Mount Watcher:** Automating disk detection (with manual fallback for V1 stability).
- **Conflict-Free Manifest Sync:** Ensuring user-managed files are protected.
- **Basic Scrobbling:** Fire-and-forget submission of `.scrobbler.log` data to Jellyfin.
- **Hardware Validation:** Enforcing path-length and character-set constraints for legacy compatibility.

### Post-MVP Features

**Phase 2 (Growth):**
- **Transcoding Handshake:** Offloading re-encoding tasks to the Jellyfin server.
- **Manifest Repair GUI:** A visual tool for resolving state conflicts.
- **Scrobble Queue & Retry:** Robustness for offline sync sessions.

**Phase 3 (Expansion):**
- **Smart Playlists:** Automatically building on-device collections from server-side favorites.
- **Wi-Fi Sync:** Support for modern, network-enabled DAPs.

## Functional Requirements

### 1. Device Connection & Discovery
- **FR1:** The system can automatically detect Mass Storage devices (USB) on Windows, Linux, and macOS.
- **FR2:** Users can manually select a target device folder if automatic detection fails.
- **FR3:** The system can identify the presence of a `.jellyfinsync.json` manifest on discovery.
- **FR4:** The system can read persistent hardware identifiers to link devices across different sessions. When multiple managed devices are connected simultaneously, the system tracks all of them and allows the user to select the active device context.
- **FR33:** When multiple managed devices are connected simultaneously, the system presents a device selection UI and allows the user to switch the active device context without restarting or reconnecting.
- **FR26:** The system can initialize a new `.jellyfinsync.json` manifest on a connected device that has not previously been managed, capturing a hardware identifier, a designated sync folder path, and an associated Jellyfin user profile.

### 2. Server & Profile Management
- **FR5:** Users can configure Jellyfin server credentials (URL, username, token).
- **FR6:** Users can select a specific Jellyfin user profile for syncing.
- **FR7:** The system can maintain a persistent, encrypted connection state to the Jellyfin server.

### 3. Content Selection & Browsing
- **FR8:** Users can browse Jellyfin Playlists, Genres, and Artists within the UI.
- **FR9:** Users can select specific playlists or entities for synchronization.
- **FR10:** The system can report real-time storage availability on the target device.
- **FR11:** Users can see a preview of "Proposed Changes" (files to add, remove, or update) before starting a sync.
- **FR29:** The system can reserve capacity in the sync basket via a virtual Auto-Fill slot; at sync time the daemon expands the slot by running the priority algorithm (favorites first, then by play count, then by creation date) against the current Jellyfin library state, up to the device's available capacity or a user-defined size limit.
- **FR34:** The system can add an artist to the sync basket as a single entity reference; at sync time the daemon resolves the artist to its current track list, ensuring tracks added to the artist after basket construction are automatically included.
- **FR30:** The system can automatically trigger synchronization when a known, previously configured device is detected, without requiring user interaction.

### 4. Synchronization Engine
- **FR12:** The system can perform a differential sync based on the local manifest.
- **FR13:** The system can protect unmanaged user files from deletion or modification.
- **FR14:** The system can stream media files directly from the Jellyfin server to the device via memory-to-disk buffering.
- **FR15:** The system can validate hardware-specific constraints (path length, character sets) before writing files.
- **FR16:** The system can resume an interrupted sync session without restarting from scratch.
- **FR31:** The system can negotiate a transcoded stream URL from the Jellyfin server using a device-specific DeviceProfile payload, falling back to direct download when direct play is supported or transcoding fails.
- **FR32:** The system can list available device transcoding profiles and assign one to a connected device, persisting the selection in both the device manifest and the local database.

### 5. Scrobble Management
- **FR17:** The system can detect Rockbox `.scrobbler.log` files on connected devices.
- **FR18:** The system can report completed track plays to the Jellyfin server via the Progressive Sync API.
- **FR19:** The system can track which scrobbles have already been submitted to prevent duplication.

### 6. Service & System Integration
- **FR20:** The system can run as a background service (headless) with minimal resource usage. MVP: Tauri sidecar process. Post-MVP: OS-native user-session daemon (Windows startup application, systemd user unit, launchd agent).
- **FR21:** Users can toggle "Launch on Startup" behavior. Post-MVP: Fulfilled natively by platform-specific mechanisms (Windows Registry Run key, systemd user unit enable/disable, launchd agent load/unload).
- **FR22:** The system can provide tray-icon status updates for sync progress and hardware state.
- **FR23:** The system can send OS-native notifications for sync completion or errors.
- **FR25:** The system retrieves and displays only music-centric content (Playlists, Albums, Artists, Tracks), automatically filtering out movies, series, and books from Jellyfin views.

### 7. Packaging & Distribution
- **FR27:** The system can be packaged into platform-native installers (MSI for Windows, DMG for macOS, AppImage/.deb for Linux) using the Tauri v2 bundler.
- **FR28:** The build pipeline can produce signed, distributable artifacts for all three target platforms from a single CI workflow.

## Quality & Non-Functional Requirements

### 1. Performance & Efficiency
- **Memory Footprint:** The headless Rust engine must consume < 10MB of RAM during idle states.
- **Sync Overhead:** The system must complete a manifest audit and be "ready to sync" in < 5 seconds.
- **Throughput:** Sync operations should be limited only by the target hardware's write speed or the network bandwidth to the Jellyfin server.

### 2. Reliability & Stability
- **Write-Verify-Commit:** The system must utilize OS-level file sync primitives (e.g., `sync_all`) to ensure the directory structure and data are physically flushed to the device before marking a sync as complete in the manifest.
- **Atomic Manifest Updates:** The `.jellyfinsync.json` manifest must be updated atomically to prevent corruption during unexpected power loss or disconnection.
- **Robust Connection:** The system must handle network interruptions during buffered streaming, attempting to resume for at least 3 retry cycles.
- **Hardware Disconnect:** Mid-sync ejections must not result in unbootable or unmountable media; the system must gracefully mark the session as "Interrupted" and trigger the Repair Utility on reconnection.

### 3. Cross-Platform Parity & Compliance
- **Feature Equality:** 100% feature parity between Windows, Linux, and macOS distributions.
- **macOS Sandbox Compliance:** The application must adhere to modern macOS filesystem permission models, ensuring functionality without requiring root/sudo privileges.
- **Resource Consistency:** Memory and CPU usage should remain within a 15% delta across all supported OS environments.

### 4. Security & Privacy
- **Credential Storage:** Jellyfin server tokens must be stored using OS-native secure storage (e.g., Windows Credential Manager, macOS Keychain).
- **Data Privacy:** All media synchronization occurs locally between the Jellyfin server and the target device; zero user data is transmitted to third-party secondary servers.

### 5. Maintainability
- **CLI-First Architecture:** The core engine must remain fully functional and testable via CLI independent of the detached UI.
- **standard Tooling:** The project should follow established Rust workspace patterns for ease of future contribution.
