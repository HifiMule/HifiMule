---
stepsCompleted: ['step-01-init', 'step-02-discovery', 'step-03-success', 'step-04-journeys', 'step-05-domain', 'step-06-innovation', 'step-07-project-type', 'step-08-scoping', 'step-09-functional', 'step-10-nonfunctional', 'step-11-polish']
inputDocuments: ['product-brief-bmad-2026-01-26.md', 'project-context.md', 'architecture.md', 'playback-prd-source-extract.md', 'reconcile-playback-brainstorming.md', 'reconcile-playback-architecture.md']
documentCounts: {briefCount: 1, researchCount: 0, brainstormingCount: 0, projectDocsCount: 0}
classification:
  projectType: 'Desktop App (Rust-based Headless Sync Engine + Detachable UI)'
  strategy: 'Event-Driven Mount Watcher + Manifest Probing'
  domain: 'Media Utility / General'
  complexity: 'Low (Performance-focused)'
  projectContext: 'Brownfield — sync product extended with playback'
workflowType: 'prd'
status: final
updated: '2026-09-19'
playbackUpdate: 'final — ready for staged epic planning; implementation gates remain open'
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
- **Auto-Fill Sync Mode:** Intelligent device-filling using virtual basket slots. Enabling Auto-Fill places a slot in the basket representing remaining capacity; a configurable selection **pipeline** (sources, filters, ordering, memory, budget) runs at sync time, not at basket-build time — defaulting to favorites → play count → creation date when unconfigured. Always uses the freshest library state. Auto-fill can be defined per media server, so multi-server users get an independent fill per server. Can be mixed with manual basket selections.
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
- **FR33:** The system presents a persistent device hub showing all connected managed devices, each identified by its name and icon. The user can switch the active device context at any time. The Playback destination is always available and selected when no physical device is connected (FR55). Without a physical target, physical-device basket and sync actions remain unavailable; playback and library browsing remain usable.
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
- **FR29:** The system can reserve capacity in the sync basket via one or more virtual Auto-Fill slots — one per configured media server (see FR51). At sync time the daemon expands each slot by running that server's configured **auto-fill pipeline** (FR49) against the current library state of the slot's server, up to the device's available capacity or the slot's budget (FR52). When no pipeline is configured, the slot uses the default pipeline — a single ordering stage equivalent to the legacy algorithm (favorites → play count → creation date) — so existing devices behave unchanged.
- **FR34:** The system can add an artist to the sync basket as a single entity reference; at sync time the daemon resolves the artist to its current track list, ensuring tracks added to the artist after basket construction are automatically included.
- **FR30:** The system can automatically trigger synchronization when a known, previously configured device is detected, without requiring user interaction.
- **FR47:** In the virtualized list/table browse view, users can select multiple rows representing artists or albums via per-row checkboxes, Ctrl/Cmd-click toggling, and Shift-click range selection. While at least one row is selected, a bulk action bar shows the selection count and offers: "Add to basket" (adds each selected entity using existing basket semantics, batch-fetching counts/sizes; entities already in the basket are skipped) and "Add to playlist…" (gated on `supports_playlist_write`; opens the existing create-new / add-to-existing playlist flow with all selected item IDs, resolved server-side to tracks). Selection state is keyed by item id, survives virtualization scrolling and autoload-on-scroll, and is cleared on browse-mode change, drill-down navigation, A–Z filter change, view-mode toggle, or Escape.
- **FR48:** Multi-selection and bulk actions extend to individual track rows on both track surfaces: (a) in the virtualized list/table browse view, track rows (e.g., tracks within an album) are selectable exactly like artist and album rows per FR47; (b) in the Tracks dual-panel browse view, track rows support per-row checkboxes, Ctrl/Cmd-click toggling, and Shift-click range selection, with a bulk action bar offering "Add to basket" (tracks already in the basket are skipped; track sizes come from the items themselves — no batch count/size fetch) and "Add to playlist…" (gated on `supports_playlist_write`; opens the existing create-new / add-to-existing playlist flow with all selected track IDs). In the Tracks view, selection is keyed by track id, survives autoload-on-scroll pagination, and is cleared on artist filter change, album filter change, A–Z letter change, leaving the Tracks mode, or Escape.
- **FR49:** The auto-fill expansion is defined by a **configurable pipeline** with ordered stages: **Filter** (genre/tag include-exclude scope) → **Sources** (pools the fill draws from) → **Unit** (track / album / artist selection granularity) → **Ordering** (how candidates are ranked) → **Memory** (repeat/rotation behaviour) → **Budget** (size/duration ceiling). Each device's pipeline configuration is stored per server in the device manifest (`.hifimule.json`). A pipeline is valid with as little as one stage; unconfigured stages use neutral defaults.
- **FR50:** Auto-fill configuration follows **Source × Strategy separation**: a pipeline is an ordered list of `(Source, Picker, share)` entries plus global modifiers and a budget, where *Source* answers "draw from what?" and *Picker* answers "pick how?" as independent axes. The first-class Source types are **Playlist pools** and a **Tag/Genre pre-filter**; per-source **shares** let a fill blend multiple sources (e.g. "70% from 2 playlists, 30% library remainder"). A terminal **fallback chain** entry guarantees every fill reaches its budget target.
- **FR51:** Auto-fill can be defined **independently for each configured media server** on a device (lifting the prior single-slot, single-server limit). Each `(device, portable server id)` pair carries its own pipeline configuration; multiple Auto-Fill slots — at most one per server — may coexist in the basket simultaneously and each expands against its own server at sync time. Per-server pipeline **configuration** lives in the device manifest; transient **runtime state** required by some strategies (e.g. cooldown windows) lives in the daemon database keyed by device+server (machine-local, not portable).
- **FR52:** The auto-fill **Budget** stage supports a size target (bytes), a **duration target** (listening hours, with bytes derived), and a **headroom reserve** (always leave X free). No fill exceeds `capacity − reserve`; the fallback chain ensures the target is reached when primary sources are exhausted.
- **FR53:** Auto-fill supports **Memory / rotation** strategies backed by daemon-stored history: **sync cooldown** (recently-synced tracks ineligible for N weeks, no play data required), **played-track exclusion** (scrobble-aware), **stable-core + delta** (keep X% of the previous fill, refresh the rest), and **rotation tiers** (heavy/medium/gold strata, optionally playlist-backed, cycling at different speeds). A **repeat-tolerance** dial spans "never repeat" ↔ "pin beloved tracks."
- **FR54:** Auto-fill supports advanced **ordering, quality, discovery, and delight** modifiers: quality/version preference (best-version resolution, prefer studio/live), discovery strategies (deep-cuts excavator, acclaimed-classics, community-rating fallback), context-aware fills (time-of-day / energy-curve / seasonal — cheap tag-based versions), an Artist Spotlight unit, and loot-table mechanics (weighted rarity draws, pity timer).

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
- **FR20:** The system can run as a background service (headless) with minimal resource usage. Existing sync-only delivery may use a Tauri-owned sidecar. Playback requires an independent daemon in the signed-in desktop session, surviving UI closure (FR56); startup registration remains user-controlled.
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

## Playback Extension — Approved Product Requirements

### Purpose and scope

HifiMule becomes the desktop listening companion for the same curated libraries it synchronizes. The primary outcome is an easy answer to “what should I listen to?” through Play something, alongside faithful album listening and auditioning music before choosing it for a playlist or device. Windows, macOS and Linux are in scope from the start.

These requirements extend the existing sync product. They supersede older no-device locking and UI-owned lifetime assumptions for playback only. Delivery is split into Epic 15 (manual Desktop Playback, Back, compact browse navigation and packaging) and Epic 16 (Radio/Recommendations and the remaining playback roadmap). All requirements below remain product commitments; the first release completes only its mapped subset, not Radio, reporting/exports, adaptation or conditional backoff.

### Listening destinations and controls

- **FR55:** Playback is an always-present destination listed first, with independent source/selection settings and a manually editable queue. Select it when no physical device is connected; do not expose storage, folder or file-sync actions for Playback.
- **FR56:** Music and native media controls remain operational after the main UI closes. Reopening the UI shows the same session and current state without restarting playback.
- **FR57:** Play something starts a fresh Radio using the first eligible result from the shared sync/playback selection engine under Playback settings. Resume continues the existing queue and position. Both actions are available from the desktop app menu without opening the main window when configured sources are available.
- **FR58:** Users can control play/pause, stop, Back (restart the current track or return to the previous track), next, playback position where supported, and output selection. Back restarts after three seconds of main-track playback and selects the previous occurrence at or before three seconds; without a previous occurrence it restarts the current track. During Preview it restarts the audition without altering the preserved main session. UI and supported native Previous/keyboard controls use the same session command, preserving paused intent and output safety. Capability limits and recoverable errors are visible rather than silently ignored.
- **FR59:** A translucent floating playback bar below the media browser remains visible while idle, browsing or working with a device basket, and offers Play something when idle. The bar does not obscure the final list items. Selecting a different server/device does not stop music or change its source; arriving physical devices become selected and unconfigured devices retain a visible setup action. Show detected-device open failures with actionable feedback.
- **FR60:** Full application quit preserves queue, position and logical-session exclusions. Relaunch restores paused. Quit during sync stops audio and requests orderly sync cancellation without marking incomplete device writes successful.

### Album listening and auditions

- **FR61:** Album playback follows disc/track order and preserves the intended continuity, including silence recorded in the source. Track boundaries introduce no additional gap when both tracks are prepared and their tested formats are supported. No automatic crossfade or silence removal is implied.
- **FR62:** An explicit Preview action, distinct from Play, starts a full-track audition while preserving the main session and position. Another audition replaces the audition, not the preserved main session. Provide an explicit return-to-session action; exact button/context-menu placement is settled in the UI story.
- **FR63:** Natural audition completion restores the main session's previous playing/paused state and position. Explicit audition Stop restores it paused. With no main session, completion leaves playback idle. Failed resumption preserves the session and presents a recoverable error.
- **FR64:** Loss of the selected output pauses playback without automatically sending music to another output. Reconnection leaves playback paused until resumed. Shared output is the default; OS Do Not Disturb remains managed by the OS.

### Radio and selection

- **FR65:** Radio continually replenishes a limited upcoming queue until stopped or no eligible music is available. It uses the same selection engine as sync with separate settings; it does not enqueue the whole library.
- **FR66:** Radio stays with the current artist while eligible unheard tracks remain, then moves to a meaningfully connected artist using available relationship evidence stronger than shared genre alone. The transition reason is explainable.
- **FR67:** When no meaningful artist connection is available, Radio chooses a fresh center using the original Playback settings and labels it as a new starting point. When all eligible tracks have been heard, begin another listening cycle while preserving exclusions; when none are eligible, show an explained waiting state.
- **FR68:** Users can reorder and remove queued tracks. Automatic replenishment appends without reordering existing entries. Removed automatic suggestions remain excluded for the logical session; stale concurrent edits cannot overwrite newer queue changes.
- **FR69:** Skips exclude tracks for the current logical Radio session, including across restoration. A new Radio resets those exclusions. HifiMule does not accumulate a cross-session taste-learning profile.
- **FR70:** Radio may draw from multiple configured sources. Copies confidently identified as the same recording count as one Radio selection, while distinct performances and uncertain matches remain separate. Playback and server actions retain the chosen source identity regardless of the browsed server.

### Quality and reliability

- **FR71:** Playback requests the highest sustainable available quality independently of portable-device transcoding preferences. A brief startup buffer is allowed. Buffer/refill behavior informs automatic quality reduction and conservative recovery, with a discreet explanation of reduced quality.
- **FR72:** Initial quality changes occur at track boundaries. Mid-track replacement is enabled only for provider/format combinations validated for correct resume timing. If no available representation can be sustained, expose buffering/retry rather than promise uninterrupted playback.
- **FR73:** For an unavailable source, Radio may continue with another eligible track; album playback pauses and retries without silently omitting tracks. A technical failure is not treated as a user skip or dislike.
- **FR74:** Radio uses track-level loudness matching; album playback uses consistent album gain to preserve relative levels. Apply peak protection. Leave gain unchanged when usable metadata is absent; automatic dynamic-range compression is not required.
- **FR75:** Playback and device sync normally run together. Reduce sync demand only when needed to protect playback. Switching, skipping or seeking cannot allow obsolete buffered content to become the new session's audio.

### History, preferences and curation

- **FR76:** Distinguish currently-playing status from a completed listen. Avoid counting skipped/interrupted tracks where provider semantics allow; completed auditions count where supported. Do not promise reversal of server-counted plays. Listening eligibility and reporting behavior must be verified for supported providers before enabling reports.
- **FR77:** Explicit Like/Dislike persists on the source server only when a genuine supported equivalent exists. Unsupported actions are unavailable; removing a favorite is not silently mapped to dislike. No local-only durable taste state is created as a fallback.
- **FR78:** Users can save an immutable snapshot of accepted played occurrences, the current occurrence once unless rejected, and upcoming occurrences, omitting skipped/disliked entries. Further playback does not alter the saved snapshot; deliberate repeated queue occurrences remain distinct.
- **FR79:** Saving a mixed-source snapshot creates one playlist per contributing capable server, preserving relative order within each part. Display per-server successes, unsupported parts and failures; retries must not blindly duplicate already-created playlists. The complete cross-server order remains local.
- **FR80:** With a connected physical target, users can explicitly Add the snapshot to its basket or Replace its basket. Without a physical target these actions are unavailable. Sending music to a basket does not itself start synchronization or establish a live link to Radio.
- **FR81:** Maintain recoverable reporting/export operation state across interruption. Reconcile ambiguous results before repeating non-idempotent writes; do not promise exactly-once remote effects where server APIs cannot support them.

### Playback quality requirements

- **P-NFR1 — Continuity:** Prepared supported album boundaries introduce zero extra decoded samples of silence or omitted source samples in deterministic fixtures. Physical-output continuity and real-sync coexistence require separate release tests; callback counters alone are insufficient.
- **P-NFR2 — Resource bounds:** Bound upcoming entries, compressed prefetch, decoded PCM and candidate caches separately. Long-session history is paged rather than held as an ever-growing UI list. Measure idle and active memory separately; retain the existing idle target without extending it to active playback. Numeric active budgets and buffer thresholds must be set in the owning implementation story before performance acceptance.
- **P-NFR3 — Cross-platform operation:** Validate installed builds on every shipping architecture, including UI-close/reopen, native transport, output loss, sleep/wake and safe shutdown. Test physical media-key routing separately from OS API delivery.
- **P-NFR4 — State integrity:** Restore only valid persisted state; reject stale queue mutations; prevent obsolete asynchronous work from changing the current session. Failed restoration or preview return must preserve recoverable listening state and explain the problem.
- **P-NFR5 — Privacy:** Use existing source-server credentials and capabilities. Do not expose authenticated stream URLs in the UI or logs. Initial relationship enrichment uses configured-server metadata; no third-party listening or metadata service is introduced.
- **P-NFR6 — Usability/accessibility:** Playback remains keyboard-operable with visible focus, accessible control names and status changes, consistent with existing WCAG 2.1 AA targets. Floating controls and background updates must not steal focus or make bottom rows unreachable.

### User scenarios captured in the playback discussion

- **UJ-P1 — Alexis working with headphones:** Start music without choosing a first artist; Radio replenishes from his curated sources while the main UI is closed. Skip an unwanted track without creating a durable taste profile.
- **UJ-P2 — Alexis listening through his USB audio interface:** Play an album in order with its intended transitions while other applications can still use audio. Disconnecting the output pauses music instead of sending it to laptop speakers.
- **UJ-P3 — Alexis curating portable music:** Audition a complete track, return to the preserved session, then explicitly save a listening snapshot to server playlists or add/replace a connected device basket.

### Success and release evidence

Epic 15 release success is demonstrated by selected-track/album listening, Preview/Return, manual queue, Back, compact accessible browse navigation, durable paused restoration and installed verification in Story 15.17.

Epic 16 success is demonstrated when configured Play something starts listening without opening the UI, album/preview behavior retains the agreed order and position, and users can move discoveries into playlists or a device basket without reconstructing the queue. Counter-metrics are playback underruns, unintended output reroutes, duplicate reporting/export effects, memory growth and unnecessary sync-throughput loss. Report actual measurements; do not replace unknown thresholds with invented latency guarantees.

The isolated playback experiments establish short-run decoding/output and session-control feasibility on tested ARM64 environments. They do not certify streaming adaptation, packaged x64 behavior, metadata coverage or sustained physical playback under real sync load. The architecture lists the remaining gates.

### Non-goals and open implementation decisions

ASIO/exclusive-mode playback, crossfade, cross-session taste learning, automatic external metadata lookup, automatic server-queue synchronization and guaranteed seamless adaptation through an outage are not requirements of this extension. Server playlist saves remain explicit snapshots.

The owning implementation story must settle source-selection ranking, reporting eligibility after seeking, unavailable initial setup, physical-target changes during basket export, buffer budgets and repeat/export reconciliation details before coding that behavior. These are tracked contract questions, not permission to change the approved user outcomes. Owner: implementation-story author, reviewed against this PRD and architecture before the affected stage starts.

### Playback glossary and design reference

Playback destination: the always-available local listening context. Main session: the queue/state preserved while auditioning. Audition: temporary full-track playback. Radio center: the current artist around which selection proceeds. Logical session: listening state that survives restoration and ends when replaced with a new Radio. Recording: musical identity distinct from server copies and queue occurrences. Snapshot: immutable export of the agreed queue/history contents.

Implementation mechanisms, module layout and technology decisions remain in [architecture.md](architecture.md), Playback Extension, rather than being duplicated here.
