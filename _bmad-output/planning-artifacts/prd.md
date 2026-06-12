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

# Product Requirements Document - HifiMule

**Author:** Alexis
**Date:** 2026-01-26

## Success Criteria

### User Success
- **Friction-Free Bridge:** Users connect hardware and sync with zero confusion or manual file management.
- **Hardware Safety:** Users feel confident that legacy hardware constraints (path-length limits, character sets) are automatically handled.
- **Data Integrity:** Users trust that the "Managed Sync" model will never touch or delete their personal unmanaged files.
- **Ecosystem Continuity:** Listen history from the device is reflected on the media server, making the DAP feel part of the modern library.
- **Server Flexibility:** Users are not locked into a single media server. HifiMule works seamlessly with Jellyfin, Navidrome, Subsonic, and any OpenSubsonic-compatible server.

### Business Success
- **Legacy Ecosystem Essential:** HifiMule becomes the top recommendation for the Rockbox/DAP community regardless of media server choice (Jellyfin, Navidrome, or Subsonic-compatible servers).
- **Cross-Platform Parity:** Identical user experience across Windows, Linux, and macOS with zero feature loss.

### Technical Success
- **Capped Idle Usage:** The Rust engine maintains a < 10MB RAM footprint during 72-hour idle state stress tests.
- **Buffered IO Stability:** Memory-to-disk buffering ensures peak USB write speeds without impacting host system responsiveness.
- **Atomic Manifest Updates:** The `.hifimule.json` state is only committed after successful file verification.

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
- **Conflict-Free Manifest Sync:** Implementation of the `.hifimule.json` logic for managed-folder isolation.
- **Profile Selection:** UI/CLI support for selecting the correct Jellyfin user account for playlist/scrobble routing.
- **Auto-Fill Sync Mode:** Intelligent device-filling using a virtual basket slot. Enabling Auto-Fill places a single slot in the basket representing remaining capacity; the priority algorithm (favorites → play count → creation date) runs at sync time, not at basket-build time. Always uses the freshest library state. Can be mixed with manual basket selections.
- **Auto-Sync on Connect:** Known devices with auto-sync enabled trigger synchronization automatically on detection, requiring zero user interaction.
- **Device Configuration:** During device initialization, users can assign a custom display name, select an icon from a built-in library, choose a transcoding profile, and choose device folders for music and playlists. Profile selection appears before folder entry because profiles can provide default music and playlist folders. Playlist location defaults to the music folder when no profile-specific or user-entered playlist folder is provided. Existing managed devices can later be edited for name, icon, transcoding profile, music folder, and playlist folder, with folder changes surfaced as cleanup/resync work before the next sync.

- **Transcoding Handshake:** Per-device profile selection for server-side re-encoding via Jellyfin PlaybackInfo API. Profiles stored in an editable `device-profiles.json` in the app data directory; passthrough (direct download) is the default. Profiles may optionally define default music and playlist folders, such as Rockbox profiles defaulting to `Music` and `Playlists`, and Garmin music watches defaulting both folders to `Music`.

### Growth Features (Post-MVP)
- **Scrobble Queue & Retry:** Robust handling for offline scrobbling when the server is unreachable during sync.
- **Repair Utility:** Guided GUI-based recovery for interrupted transfers or "de-synced" manifests.

## User Journeys

### Arthur's Weekly Ritual (Legacy Success)
*   **Narrative:** Every Saturday morning, Arthur plugs his beloved 160GB iPod Classic into his Linux desktop. The headless HifiMule engine detects the mount instantly. Arthur opens the UI, switches to Recently Added, reviews the newest music from his server, and adds selected albums or tracks to the basket. He clicks "Sync".
*   **Success Moment:** The sync completes in under 20 seconds using the pre-calculated manifest. Arthur ejects the device, confident that his manual "Voice Memos" folder remains untouched.

### Sarah's Pre-Run Dash (Speed Success)
*   **Narrative:** At 6:00 AM, Sarah plugs in her Garmin watch on her way out the door. The daemon recognizes the device, auto-syncs her favorites and most-played tracks to fill the watch, and a tray notification confirms "Sync Complete" before she's finished tying her shoes.
*   **Success Moment:** She unplugs and leaves. Zero clicks. The auto-fill prioritized her favorite running tracks and the tool handled everything silently in the background.

### The "Silent Engine" (Admin Setup)
*   **Narrative:** Alexis sets up HifiMule on a new Mac Mini. He runs a simple wizard to connect to his media server (Jellyfin or Navidrome), which is auto-detected by type, and selects his primary User Profile.
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
- **HifiMule's Position:** The only tool combining the "Leanness" of a CLI-first utility with the "Richness" of media-server metadata and background automation.

### Validation Approach
- **Sync Stress Test:** Validating the Rust engine against a simulated 10,000-file library across three OS platforms.
- **Memory Soak Test:** 72-hour automated monitoring to confirm zero-leak, < 10MB idle performance.
- **Auto-Pilot Reliability:** Iterative testing of mounting/unmounting events to ensure sync consistently triggers and completes without user intervention.

## Desktop App Specific Requirements

### Project-Type Overview
As a cross-platform desktop application, HifiMule consists of a performance-critical Rust-based sync engine (Headless) and a separate (detachable) user interface.

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
- **FR1:** The system can automatically detect Mass Storage (USB MSC) and MTP (Media Transfer Protocol) devices on Windows, Linux, and macOS.
- **FR2:** Users can manually select a target device folder if automatic detection fails. Manual fallback applies to Mass Storage devices only; MTP devices must be detected automatically via the OS device manager.
- **FR3:** The system can identify the presence of a `.hifimule.json` manifest on discovery.
- **FR4:** The system can read persistent hardware identifiers to link devices across different sessions. When multiple managed devices are connected simultaneously, the system tracks all of them and allows the user to select the active device context.
- **FR33:** The system presents a persistent device hub showing all connected managed devices, each identified by its name and icon. The user can switch the active device context at any time. When no device is selected, the basket is empty and adding items is disabled.
- **FR26:** The system can initialize a new `.hifimule.json` manifest on a connected device that has not previously been managed, capturing a hardware identifier, a designated music sync folder path, a playlist folder path that defaults from the selected device profile or otherwise the music folder, an associated media-server user profile, a user-provided display name, and an optional icon identifier selected from a built-in library.

### 2. Server & Profile Management
- **FR5:** Users can configure media server credentials (URL, server type, username, and either an API token for Jellyfin or username+password for Subsonic/OpenSubsonic servers). The system auto-detects the server type by pinging the URL when the user enters it.
- **FR6:** Users can select a specific user profile from the connected media server for syncing.
- **FR7:** The system can maintain a persistent, encrypted connection state to the configured media server. For Jellyfin, the access token is stored. For Subsonic/OpenSubsonic, the user password is stored (encrypted) and used to sign each request stateless-style.
- **FR45:** Users can assign each configured media server a custom display name and icon from a built-in icon library that includes provider icons (Jellyfin, Navidrome/Subsonic/OpenSubsonic where available) plus music and audiobook-oriented icons. The UI uses the configured name and/or icon as the primary server identity in the Server Hub, compact server switcher, basket server badges, playlist notices, and other multi-server contexts. Provider type remains visible as secondary metadata when useful.
- **FR46:** Each configured media server has a stable, machine-independent logical identity ("portable server id") used to tag synced items and basket items in the device manifest (`.hifimule.json`) and to route sync. The portable id is derived deterministically from the server's identity (server type, canonical base URL, username; preferring a server-reported id when available), so the same logical server/user resolves to the same id on any machine and across remove/re-add cycles. A separate machine-local id continues to key local storage, credentials vault, and the provider cache. Changing this identity is invisible to users; it only affects manifest portability and avoidance of unnecessary re-sync.

### 3. Content Selection & Browsing
- **FR8:** Users can browse music from the connected media server through server-supported navigation modes: Playlists, Artists, Albums, Tracks, Genres, Recently Added, Frequently Played, Recently Played, and Favorites. The provider abstraction normalizes these browse modes across Jellyfin, Navidrome, Subsonic, and OpenSubsonic-compatible servers. Unsupported modes are hidden or clearly unavailable based on provider capabilities.
- **FR9:** Users can select specific server playlists or entities (artists, albums, genres, tracks) for synchronization (read path). Persisting a selection back to the server as a playlist is covered by FR37.
- **FR10:** The system can report real-time storage availability on the target device.
- **FR11:** Users can see a preview of "Proposed Changes" (files to add, remove, or update) before starting a sync.
- **FR29:** The system can reserve capacity in the sync basket via a virtual Auto-Fill slot; at sync time the daemon expands the slot by running the priority algorithm (favorites first, then by play count, then by creation date) against the current Jellyfin library state, up to the device's available capacity or a user-defined size limit.
- **FR34:** The system can add an artist to the sync basket as a single entity reference; at sync time the daemon resolves the artist to its current track list, ensuring tracks added to the artist after basket construction are automatically included.
- **FR30:** The system can automatically trigger synchronization when a known, previously configured device is detected, without requiring user interaction.
- **FR47:** In the virtualized list/table browse view, users can select multiple rows representing artists or albums via per-row checkboxes, Ctrl/Cmd-click toggling, and Shift-click range selection. While at least one row is selected, a bulk action bar shows the selection count and offers: "Add to basket" (adds each selected entity using existing basket semantics, batch-fetching counts/sizes; entities already in the basket are skipped) and "Add to playlist…" (gated on `supports_playlist_write`; opens the existing create-new / add-to-existing playlist flow with all selected item IDs, resolved server-side to tracks). Selection state is keyed by item id, survives virtualization scrolling and autoload-on-scroll, and is cleared on browse-mode change, drill-down navigation, A–Z filter change, view-mode toggle, or Escape.
- **FR48:** Multi-selection and bulk actions extend to individual track rows on both track surfaces: (a) in the virtualized list/table browse view, track rows (e.g., tracks within an album) are selectable exactly like artist and album rows per FR47; (b) in the Tracks dual-panel browse view, track rows support per-row checkboxes, Ctrl/Cmd-click toggling, and Shift-click range selection, with a bulk action bar offering "Add to basket" (tracks already in the basket are skipped; track sizes come from the items themselves — no batch count/size fetch) and "Add to playlist…" (gated on `supports_playlist_write`; opens the existing create-new / add-to-existing playlist flow with all selected track IDs). In the Tracks view, selection is keyed by track id, survives autoload-on-scroll pagination, and is cleared on artist filter change, album filter change, A–Z letter change, leaving the Tracks mode, or Escape.

### 4. Synchronization Engine
- **FR12:** The system can perform a differential sync based on the local manifest.
- **FR13:** The system can protect unmanaged user files from deletion or modification.
- **FR14:** The system can stream media files directly from the Jellyfin server to the device via memory-to-disk buffering, using the appropriate device IO backend (filesystem writes for MSC devices, WPD/libmtp object transfers for MTP devices).
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
- **FR24:** The system provides visual feedback (splash screen) during application startup and connection validation.
- **FR25:** The system retrieves and displays only music-centric content (Playlists, Albums, Artists, Tracks). For Jellyfin: applies `IncludeItemTypes` filter to exclude movies, series, and books. For Subsonic/Navidrome: uses music-specific endpoints (`getArtists`, `getAlbum`, `getPlaylists`) which are inherently music-only.
- **FR35:** The system supports Jellyfin, Navidrome, Subsonic, and any OpenSubsonic-compatible media server. Server type is auto-detected at connection time by pinging the server URL. Detected capability extensions (OpenSubsonic) are cached and used to enable per-server features.
- **FR36:** The system can edit an existing managed device manifest, allowing users to change device name, icon, transcoding profile, music folder, and playlist folder. Folder changes are reflected in the next sync preview and trigger managed relocation cleanup before new items are written.

### 7. Packaging & Distribution
- **FR27:** The system can be packaged into platform-native installers (MSI for Windows, DMG for macOS, AppImage/.deb for Linux) using the Tauri v2 bundler.
- **FR28:** The build pipeline can produce signed, distributable artifacts for all three target platforms from a single CI workflow.

### 8. Playlist Management & Curation
- **FR37:** The system can persist the current device selection as a media-server playlist — creating a new playlist or updating an existing one. The system reads the current server playlist state before editing (read-fresh) and writes the resulting track set back (write-back). Basket entities are resolved to a concrete ordered track list at save time. The Auto-Fill virtual slot is excluded; when present, the user is notified. Supported on Jellyfin and Subsonic/OpenSubsonic, gated by `supports_playlist_write`.
- **FR38:** The system provides a dual-panel playlist curation view: artists in the playlist on the left, that artist's albums on the right, filtered to playlist contents. A track list panel below both panels shows individual tracks for the selected artist, optionally filtered by a focused album. Users can remove an artist, a specific album, or an individual track. The curation view provides an "Add tracks" affordance that opens a search dialog, allowing users to find and append individual tracks from the library to the playlist via `playlist.addTracks`. Individual tracks in any browse view also expose an "Add to playlist…" right-click context action — selecting an existing playlist calls `playlist.addTracks`; selecting "New playlist" calls `playlist.create`. A right-click context menu lets users send artists/albums to a playlist from browse views. The view displays playlist statistics (track count, total duration, total storage size). The playlist name in the curation view header is editable inline; saving calls `playlist.rename`. A delete affordance in the header opens a confirmation dialog before calling `playlist.delete` and returning to the playlist browser. Edits update the server playlist. The track list panel shows each track's absolute playlist position (1-based) and supports an "All artists / All albums" selection so the complete playlist can be viewed in order.
- **FR39:** The system can present any browse page or drill-down level as a virtualized list/table view (in addition to the paginated album-art grid), enabling rapid scanning of artists, albums, playlists, genres, history, and favorites — and sub-levels such as albums within an artist or tracks within an album. A single global grid/list toggle in the browse-mode bar applies uniformly across all browse modes and navigation depths.
- **FR40:** The system can reorder tracks within an existing server playlist. The curation view exposes per-track up/down controls; moving a track rewrites the playlist's track order via a new provider write operation (`reorder_playlist`) exposed as the `playlist.reorder` RPC. Reordering is gated by `supports_playlist_write` (Jellyfin via Items/Move; Subsonic/OpenSubsonic via ordered createPlaylist replace). A track's absolute playlist position is shown in the curation track list, so reordering remains meaningful while an artist or album filter is active.
- **FR41:** The system can present the entire library as a flat Tracks browse mode with a dual-panel artist/album filter layout. The artist filter panel, album filter panel, and track list are each independently paginated with autoload-on-scroll, so libraries with thousands of artists/albums/tracks remain responsive. "All artists" and "All albums" filter entries are provided so the unfiltered global track list is reachable. Track rows expose the standard basket add/remove actions and the "Add to playlist…" context menu (capability-gated on `supports_playlist_write`), plus a per-row "Send to playlist…" affordance opening the same flow. The mode is gated by provider capability: providers that cannot enumerate library-wide tracks (e.g., classic Subsonic without `search3`) do not advertise this mode.

## Quality & Non-Functional Requirements

### 1. Performance & Efficiency
- **Memory Footprint:** The headless Rust engine must consume < 10MB of RAM during idle states.
- **Sync Overhead:** The system must complete a manifest audit and be "ready to sync" in < 5 seconds.
- **Throughput:** Sync operations should be limited only by the target hardware's write speed or the network bandwidth to the Jellyfin server.
- **List/Table View Rendering:** List and table browse views must use virtualized (windowed) rendering to remain responsive with libraries of thousands of items. The list view uses autoload-on-scroll (next page fetches automatically as the user approaches the loaded boundary) rather than a "Load More" button, avoiding visible page-boundary friction while keeping memory and scroll performance within the app's existing UI responsiveness targets.

### 2. Reliability & Stability
- **Write-Verify-Commit:** The system must utilize OS-level file sync primitives (e.g., `sync_all`) to ensure the directory structure and data are physically flushed to the device before marking a sync as complete in the manifest.
- **Atomic Manifest Updates:** The `.hifimule.json` manifest must be updated atomically to prevent corruption during unexpected power loss or disconnection.
- **Robust Connection:** The system must handle network interruptions during buffered streaming, attempting to resume for at least 3 retry cycles.
- **Hardware Disconnect:** Mid-sync ejections must not result in unbootable or unmountable media; the system must gracefully mark the session as "Interrupted" and trigger the Repair Utility on reconnection.

### 3. Cross-Platform Parity & Compliance
- **Feature Equality:** 100% feature parity between Windows, Linux, and macOS distributions.
- **macOS Sandbox Compliance:** The application must adhere to modern macOS filesystem permission models, ensuring functionality without requiring root/sudo privileges.
- **Resource Consistency:** Memory and CPU usage should remain within a 15% delta across all supported OS environments.

### 4. Security & Privacy
- **Credential Storage:** Server credentials (Jellyfin access token, Subsonic password) must be stored in a hardware-bound encrypted vault (`secrets.enc`) in the application data directory. The encryption key is derived from the host machine's hardware fingerprint (machine-uid + blake3) and used with ChaCha20-Poly1305 AEAD, protecting against offline disk exfiltration. Credentials are irrecoverably lost if the hardware fingerprint changes (VM migration, hardware replacement) — re-authentication is required.
- **Data Privacy:** All media synchronization occurs locally between the configured media server and the target device; zero user data is transmitted to third-party secondary servers.
- **Playlist Write Security:** Playlist write operations (FR37) target only the user's configured media server using existing stored credentials (Jellyfin token / Subsonic per-request token). No new credential scope is introduced and zero data is transmitted to third-party servers.

### 5. Maintainability
- **CLI-First Architecture:** The core engine must remain fully functional and testable via CLI independent of the detached UI.
- **standard Tooling:** The project should follow established Rust workspace patterns for ease of future contribution.
