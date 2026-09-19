---
stepsCompleted: ['step-01-init', 'step-02-context', 'step-03-starter', 'step-04-decisions', 'step-05-patterns', 'step-06-structure', 'step-07-validation', 'step-08-complete']
workflowType: 'architecture'
status: 'complete'
completedAt: '2026-01-26'
lastAmended: '2026-09-11'
amendments: ['epic-8-library-browsing-rpc-contract', 'epic-8-provider-layer-type-definitions', 'epic-8-factory-lifecycle-config', 'epic-8-subsonic-auth-scrobble-incremental-sync', 'epic-11-selection-as-playlist-write-trait', 'multi-server-management']
playbackExtension:
  stepsCompleted: [1, 2, 3, 4, 5, 6, 7, 8]
  lastStep: 8
  status: complete
  completedAt: '2026-09-11'
  readiness: 'ready-for-planning; implementation gates open'
  inputDocuments:
    - '_bmad-output/brainstorming/brainstorming-session-2026-09-11-090745.md'
    - '_bmad-output/implementation-artifacts/playback-session-results.md'
    - '_bmad-output/implementation-artifacts/playback-feasibility-results.md'
    - '_bmad-output/implementation-artifacts/playback-windows-vm-results.md'
    - '_bmad-output/implementation-artifacts/playback-linux-vm-results.md'
    - '_bmad-output/planning-artifacts/prd.md'
    - '_bmad-output/planning-artifacts/project-context.md'
---

# Architecture Decision Document

## Project Context Analysis

### Requirements Overview

**Functional Requirements:**
HifiMule requires a robust synchronization core capable of differential manifest management and direct Rockbox log processing. The architecture must support asynchronous device discovery and a detachable communication layer for the UI.

**Non-Functional Requirements:**
Architecture is driven by extreme efficiency (< 10MB RAM) and high stability (Atomic IO). Cross-platform parity is mandatory, requiring abstraction layers for Windows/Linux/macOS filesystem and notification primitives.

**Scale & Complexity:**
- Primary domain: Desktop Utility (Rust)
- Complexity level: Medium
- Estimated architectural components: 4 (Sync Core, Mount Dispatcher, Scrobble Processor, UI Bridge)

### Technical Constraints & Dependencies
- **No Heavy Runtimes:** The core engine cannot depend on Electron or Python runtimes if it is to meet the 10MB memory goal.
- **OS Native IO:** Dual-mode event-driven discovery per platform:
  - **Windows:** `WM_DEVICECHANGE` + `DBT_DEVICEARRIVAL` for MSC (drive letters) and `GUID_DEVINTERFACE_WPD` registration for MTP portable devices, both via `windows-rs`.
  - **Linux:** `udev` for MSC block devices; `udev` USB subsystem + `libmtp` device enumeration for MTP.
  - **macOS:** `DiskArbitration` for MSC; `IOKit` USB matching + `libmtp` notification callbacks for MTP.
- **Manifest-Only Truth:** The `.hifimule.json` file on the target device is the definitive record of "Managed State".

## Starter Template Evaluation

### Primary Technology Domain
Desktop Utility (Rust Daemon + Tauri UI)

### Starter Options Considered
- **Standard Tauri v2 App:** Single-process; harder to manage a persistent background daemon that lives after the window closes.
- **Pure egui/Slint:** Leanest (~5-8MB), but UI development is more rigid and requires more boilerplate for media browsing.
- **[SELECTED] Tauri + Sidecar Workspace:** A Rust Workspace with two members: `hifimule-daemon` (engine) and `hifimule-ui` (Tauri).

### Selected Starter: Custom Tauri Sidecar Workspace

**Rationale for Selection:**
Isolates the sub-10MB headless engine from the active UI runtime. Allows for a rich media-browsing interface using web technologies without compromising the idle performance of the sync daemon.

**Initialization Command:**
```bash
# Workspace setup for multi-process isolation
cargo new hifimule-daemon --bin
npx create-tauri-app@latest hifimule-ui --template vanilla-ts
```

**Architectural Decisions Provided by Foundation:**
- **Language:** Rust 1.75+ (Crates: `tokio` for async daemon, `serde` for serialization).
- **Frontend:** Vanilla TypeScript for the detachable selection UI.
- **Build Tooling:** Cargo Workspace for multi-process coordination.
- **IPC Pattern:** JSON-RPC over Localhost (HTTP) or OS-native Named Pipes.

## Core Architectural Decisions

### Decision Priority Analysis

**Critical Decisions (Block Implementation):**
- **Architecture Style:** Detached Multi-Process (Rust Daemon + Tauri UI).
- **IPC Mechanism:** JSON-RPC over Localhost (HTTP).
- **Secure Storage:** Hardware-bound encryption vault (`machine-uid` + `blake3` + `chacha20poly1305`) — credentials stored as `secrets.enc` in the app data directory, bound to the host machine's hardware fingerprint.

**Important Decisions (Shape Architecture):**
- **Data Persistence:** SQLite (`rusqlite`) for daemon state and scrobble history.
- **Async Runtime:** `tokio` for handling concurrent IO and mount events.

### Daemon Responsibilities
- **Media Provider Layer:** All server communication is mediated through a `MediaProvider` trait (`providers/jellyfin.rs` + `providers/subsonic.rs`). The daemon never calls server APIs directly — it holds an `Arc<dyn MediaProvider>` resolved at connect time based on server type detection.
- **Auto-Fill Pipeline:** Configurable selection engine implemented as pure functions over a provider's library: `Filter → Sources → Unit → Ordering → Memory → Budget`, with a terminal fallback chain. A pipeline config is an ordered list of `(Source, Picker, share)` entries + global modifiers + budget. The legacy favorites → play count → creation date behaviour is expressed as the **default single-Ordering-stage pipeline**. Stages query the `MediaProvider` via existing browse-mode/capability methods (`get_favorites()`, `get_most_played()`, `get_recently_added()`, playlist/genre enumeration); strategies requiring history (cooldown, stable-core) read/write the daemon DB. The engine is provider-agnostic and routed per server via `get_provider_by_server_id`. See "Auto-Fill Pipeline Model" for the data model. (Epic 12/13)
- **Auto-Sync Controller:** Monitors device detection events and triggers sync automatically for configured devices without UI interaction.
- **Transcoding Negotiator:** Provider-specific. Jellyfin: `POST /Items/{id}/PlaybackInfo` with `DeviceProfile` payload. Subsonic: `stream?format=mp3&maxBitRate=192` — delegated to provider's `download_url()`.
- **Multi-Device Tracker:** Maintains a map of all currently connected managed devices; exposes selection API so the UI can switch the active device context at any time. `selectedDevicePath` may be null; when null, the UI enters a locked state (basket empty, add buttons disabled). The device hub is always visible when at least one device is connected.

### Media Provider Layer

All server communication is routed through the `MediaProvider` trait:

```rust
#[async_trait]
pub trait MediaProvider: Send + Sync {
    async fn list_libraries(&self) -> Result<Vec<Library>, ProviderError>;
    async fn list_artists(&self, library_id: Option<&str>) -> Result<Vec<Artist>, ProviderError>;
    async fn get_artist(&self, artist_id: &str) -> Result<ArtistWithAlbums, ProviderError>;
    async fn list_albums(&self, library_id: Option<&str>) -> Result<Vec<Album>, ProviderError>;
    async fn get_album(&self, album_id: &str) -> Result<AlbumWithTracks, ProviderError>;
    async fn list_playlists(&self) -> Result<Vec<Playlist>, ProviderError>;
    async fn get_playlist(&self, playlist_id: &str) -> Result<PlaylistWithTracks, ProviderError>;
    async fn list_genres(&self, library_id: Option<&str>) -> Result<Vec<Genre>, ProviderError>;
    async fn get_genre_tracks(&self, genre_id_or_name: &str) -> Result<Vec<Song>, ProviderError>;
    async fn list_recently_added(&self, library_id: Option<&str>, limit: u32, offset: u32) -> Result<Vec<Song>, ProviderError>;
    async fn list_frequently_played(&self, library_id: Option<&str>, limit: u32, offset: u32) -> Result<Vec<Song>, ProviderError>;
    async fn list_recently_played(&self, library_id: Option<&str>, limit: u32, offset: u32) -> Result<Vec<Song>, ProviderError>;
    async fn list_favorites(&self, library_id: Option<&str>, limit: u32, offset: u32) -> Result<Vec<Song>, ProviderError>;
    async fn list_tracks(&self, filter: TrackListFilter) -> Result<TrackListPage, ProviderError>;
    async fn search(&self, query: &str) -> Result<SearchResult, ProviderError>;
    async fn download_url(&self, song_id: &str, profile: Option<&TranscodeProfile>) -> Result<String, ProviderError>;
    async fn cover_art_url(&self, cover_art_id: &str) -> Result<String, ProviderError>;
    async fn changes_since(&self, token: Option<&str>) -> Result<Vec<ChangeEvent>, ProviderError>;
    async fn scrobble(&self, request: ScrobbleRequest) -> Result<(), ProviderError>;
    fn server_type(&self) -> ServerType;
    fn capabilities(&self) -> &Capabilities;
}

pub enum ServerType { Jellyfin, Subsonic }
```

Domain types (`Song`, `Album`, `Artist`, `Playlist`, `Genre`) live in `domain/models.rs` — independent of API DTOs. DTOs map to domain types via `From` conversions at the adapter boundary.

**Key normalization rules:**
- All IDs: `String` (Navidrome uses MD5 hashes — never `i64`/`u64`)
- Duration: `u32` seconds (`runTimeTicks ÷ 10_000_000` for Jellyfin, direct for Subsonic)
- Bitrate: `u32` kbps (convert Jellyfin bps fields at DTO boundary)
- Cover art ref: `Option<String>` (Subsonic `coverArt` field ≠ song ID)

**Project structure additions:**
```
hifimule-daemon/src/
├── providers/
│   ├── mod.rs      (MediaProvider trait, ProviderError, ServerType)
│   ├── jellyfin.rs (JellyfinProvider — wraps existing api.rs)
│   └── subsonic.rs (SubsonicProvider — opensubsonic crate)
├── domain/
│   └── models.rs   (Song, Album, Artist, Playlist — API-agnostic)
```

**Crate additions:**
- `jellyfin-sdk = "=0.x.y"` (pin exact pre-1.0 version)
- `opensubsonic = "latest"`
- `async-trait = "0.1"`

### Data Architecture
- **Daemon State:** Managed via a local SQLite database to ensure atomic scrobble commits and robust history tracking.
- **UI Preferences:** Stored in standard JSON configuration files for ease of access from the Tauri frontend.
- **Device Profile Fields:** `auto_fill_enabled BOOLEAN DEFAULT false`, `max_fill_bytes INTEGER NULL` (null = fill to capacity), `auto_sync_on_connect BOOLEAN DEFAULT false`, `transcoding_profile_id TEXT NULL` (references id in `device-profiles.json`; null = passthrough).
- **Manifest Extension:** `.hifimule.json` includes `auto_sync_on_connect` (boolean), `auto_fill` block (`{ "enabled": bool, "maxBytes": number | null }`), `transcoding_profile_id` (string | null), `last_synced_transcoding_profile_id` (string | null), `transcoding_profile_dirty` (boolean), `name` (string | null), `icon` (string | null), `playlist_path` (string | null; defaults to the first managed music path), and `server_id` (string | null — normalized server URL for multi-server manifests). All new fields use `#[serde(default)]` for backward compatibility. When `transcoding_profile_dirty` is true, matching synced tracks are planned as delete+add work so existing files are regenerated under the active profile.
- **device-profiles.json:** Seeded to `{app_data_dir}/device-profiles.json` on first daemon startup from an embedded binary asset (`include_bytes!`). User-editable post-install. Contains named `DeviceProfile` payloads for Jellyfin PlaybackInfo negotiation and optional `defaultMusicFolder` / `defaultPlaylistFolder` strings used to prefill device folder fields in the UI. A `passthrough` profile (`deviceProfile: null`) explicitly disables transcoding.

### DeviceManager Struct
```
connected_devices: HashMap<PathBuf, DeviceManifest>  // all currently connected managed devices
selected_device_path: Option<PathBuf>                // the device targeted by all UI operations
unrecognized_device_path: Option<PathBuf>            // device awaiting initialization
```
`get_current_device()` returns the manifest for `selected_device_path`. All existing callers (basket, sync, manifest, storage) are unchanged. When only one device is connected it is auto-selected.

### Authentication & Security
- **Credential Management:** Server credentials are stored in a hardware-bound encrypted vault (`secrets.enc`) in the app data directory. The encryption key is derived from the host machine's hardware fingerprint (via `machine-uid`) mixed with an app-specific salt using `blake3`, then used with `chacha20poly1305` (AEAD). This protects against offline disk/backup exfiltration; root compromise is out of scope. **Known limitation:** credentials are irrecoverably lost if the hardware fingerprint changes (VM migration, hardware replacement, OS reinstall) — re-authentication is required.
  - **Jellyfin:** Stores a rotatable access token. Re-authenticates on 401.
  - **Subsonic/OpenSubsonic:** Stores the user password (encrypted at rest). Auth is stateless — credentials are sent on every request as `t=md5(password+salt)` + `s=salt`. The password is used only to compute per-request tokens; it is never stored in plaintext.
- **Process Isolation:** The UI and Daemon communicate over a restricted local loopback, minimizing system exposure.

### API & Communication Patterns
- **Internal IPC:** JSON-RPC 2.0 protocol implemented over a local HTTP server within the daemon.
- **Release Mode Proxy:** In release builds, Tauri serves the frontend from `https://tauri.localhost`, which blocks direct `fetch()` to the daemon's `http://localhost:19140` endpoint (mixed content / CORS). All RPC and image requests are proxied through Tauri invoke commands (`rpc_proxy`, `image_proxy`) in the UI's Rust backend, bypassing browser security restrictions. In dev mode, direct HTTP is used.
- **External API:** Direct utilization of the Jellyfin Progressive Sync API for scrobbling and playback reporting.
- **Auto-Fill IPC:** `basket.autoFill` — Preview/debug endpoint for auto-fill calculation. Params: `{ deviceId, maxBytes?, excludeItemIds[] }`. Returns ranked item list. **Not called by the UI to populate the basket** — auto-fill expansion runs inside `sync.start` when the `autoFill` param is present.
- **Auto-Fill Settings IPC:** `sync.setAutoFill` — Persist auto-fill settings per device profile. Params: `{ deviceId, autoFillEnabled, maxFillBytes?, autoSyncOnConnect }`.
- **`sync.start` params (extended):** `{ devicePath: string, itemIds: string[], autoFill?: { enabled: boolean, maxBytes?: number, excludeItemIds: string[] } }` — if `autoFill.enabled`, the daemon calls `run_auto_fill()` and merges the resulting IDs with `itemIds` before executing sync. Mirrors the daemon-initiated auto-sync path (`main.rs:503`).
- **Virtual basket slots:** Two UI-only marker types stored in the basket that represent deferred expansion. `AutoFillSlot` (`id: '__auto_fill_slot__'`) is passed to `sync.start` as the `autoFill` param, not as an `itemId`. `MusicArtist` items are passed as regular `itemIds`; the existing container-expansion logic at `rpc.rs:807–866` resolves them to tracks at sync time.
- **Server Connect IPC:**
  - `server.connect(params: { url: string, serverType: 'jellyfin' | 'subsonic' | 'auto', username: string, password: string })` → `{ ok: true, serverType: string, serverVersion: string }` — when `serverType: 'auto'`, daemon pings the URL: checks `openSubsonic` flag in Subsonic ping response, falls back to Jellyfin `/System/Info` detection. Returns detected type.
  - `get_daemon_state` response gains: `serverType: 'jellyfin' | 'subsonic' | null` and `serverVersion: string | null`.
- **Multi-Device IPC:**
  - `device.list` → `Array<{ path: string, deviceId: string, name: string | null, icon: string | null }>` — all connected managed devices.
  - `device.select(params: { path: string })` → `{ ok: true }` — sets the active device context for all operations.
  - `device.initialize(params: { folderPath: string, playlistFolderPath?: string | null, profileId: string, transcodingProfileId?: string | null, name: string, icon: string | null })` → `{ ok: true }` — writes manifest including name, icon, transcoding profile, music folder, and playlist folder. If `playlistFolderPath` is omitted or null, it resolves to `folderPath`.
  - `device.update_manifest(params: { deviceId: string, name?: string, icon?: string | null, transcodingProfileId?: string | null, musicFolderPath?: string, playlistFolderPath?: string | null })` → `{ ok: true, relocationRequired: boolean, cleanupPreview?: { tracksToRemove: number, playlistsToRemove: number, bytesToRemove: number } }` — edits the selected managed device manifest. Name/icon/profile-only changes are metadata updates. Folder changes are validated as device-relative paths and surface relocation cleanup for the next sync preview. `transcodingProfileId: null` or `passthrough` clears device transcoding.
  - `get_daemon_state` response extended with `connectedDevices: Array<{path, deviceId, name, icon}>` and `selectedDevicePath: string | null`.
- **Transcoding IPC:**
  - `device_profiles.list` → `Array<{ id, name, description, defaultMusicFolder?: string | null, defaultPlaylistFolder?: string | null }>` — reads from `device-profiles.json`. The full `deviceProfile` payload is not returned to the UI.
  - `device.set_transcoding_profile(params: { deviceId: string, profileId: string })` → `{ ok: true }` — persists to manifest (Write-Temp-Rename) and SQLite `devices` table.
- **execute_sync() signature:** `execute_sync(..., transcoding_profile: Option<serde_json::Value>)` — both callers (`rpc.rs` `sync.start` handler and `main.rs` `run_auto_sync`) load the device's profile from the manifest and pass it through.

### Frontend Architecture
- **UI Type:** Webview-based via Tauri v2.
- **State Management:** Local selection state managed within the webview, synchronized with the daemon manifest via RPC.
- **Tauri Commands:** The UI Rust backend exposes `rpc_proxy` (JSON-RPC passthrough), `image_proxy` (Jellyfin artwork as base64 data URLs), and `get_sidecar_status` (daemon lifecycle query) via `tauri::command`. These are required in release mode where browser security blocks direct HTTP to localhost.

## Implementation Patterns & Consistency Rules

### Pattern Categories Defined

**Critical Conflict Points Identified:**
3 areas where AI agents could make different choices (Naming, IPC, Safety).

### Naming Patterns

**Database Naming Conventions:**
- Tables: `snake_case` plural (e.g., `sync_history`, `devices`).
- Columns: `snake_case` (e.g., `play_count`, `last_synced_at`).

**API/IPC Naming Conventions:**
- **External Payload:** `camelCase` for all JSON-RPC fields (e.g., `syncProgress`, `deviceId`).
- **Automated Enforcement:** Use `ts-rs` or equivalent to generate TypeScript interfaces directly from Rust structs with a mandatory `#[serde(rename_all = "camelCase")]` policy.

**Code Naming Conventions:**
- **Rust (Daemon):** Standard `snake_case` for variables/functions.
- **TypeScript (UI):** Standard `camelCase` for variables/functions.

### Structure Patterns

**Project Organization:**
- Rust Workspace with crates: `hifimule-daemon` (engine) and `hifimule-ui` (Tauri).
- **Core Logic:** Extracted into a local `hifimule-core` library crate shared between binary crates if needed.
- **Tests:** Co-located in mod `tests` blocks (Rust) or `*.test.ts` (TypeScript).

**Packaging & Distribution:**
- **Bundler:** Tauri v2 built-in bundler for platform-native installers (MSI, DMG, AppImage/.deb).
- **Daemon Bundling:** The `hifimule-daemon` binary is included as a Tauri sidecar, bundled alongside the UI.
- **CI/CD:** GitHub Actions matrix build targeting Windows, Linux, and macOS with artifact upload to GitHub Releases.
- **Code Signing:** Platform-specific signing (Windows Authenticode, macOS notarization) deferred to post-MVP unless required for distribution.
- **Daemon Lifecycle — Windows:** The WiX installer registers `hifimule-daemon.exe` as a startup application via `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` (`startup-fragment.wxs`). The daemon starts in the user's interactive session at login, giving it full access to the system tray and user data directory. The UI health-checks port 19140 on launch; if the daemon is already running, no sidecar is spawned and the exit handler does not kill it.
- **Daemon Lifecycle — macOS:** On first launch, the UI writes a launchd user agent `.plist` to `~/Library/LaunchAgents/com.hifimule.daemon.plist` and loads it via `launchctl load`. The daemon then starts automatically at each login in the user's session. The UI health-checks port 19140; if already running (launchd-owned), no sidecar is spawned and the exit handler does not kill the process. The UI RPC `settings.setLaunchOnStartup(bool)` calls `launchctl load/unload` to toggle the agent.
- **Daemon Lifecycle — Linux:** Sidecar model only. systemd user-unit support is deferred (Story 6.4 note preserved).
- **Note — Windows Service:** `service.rs` contains Windows Service scaffolding (`--install-service` / `--service` flags) but is not used by the production installer. The startup application model is sufficient and keeps the daemon in the user session where tray and credential file access work correctly.

### Format Patterns

**API Response Formats:**
- Wrap results in a success/fail envelope: `{ "status": "success", "data": { ... } }` or `{ "status": "error", "message": "...", "code": 102 }`.

### Communication Patterns

**Event System Patterns:**
- **Pattern:** Request-Response-Event.
- The UI requests a "Sync start"; the Daemon returns an immediate "OK" and broadcasts progress via an `on_sync_progress` event stream.

### Process Patterns

**Error Handling Patterns:**
- **Rust Internal:** `thiserror` crate for typed library errors.
- **Rust Top-level:** `anyhow` for binary-level error management.

**Loading State Patterns:**
- Background tasks (Syncing/Discovery) are represented as "Job IDs" in the state, allowing the UI to re-attach to long-running tasks.

### Safety & Atomicity Patterns
- **Atomic Manifest Commitment:** Utilize the "Write-Temp-Rename" pattern for all `.hifimule.json` updates to prevent state corruption during disconnection.
- **Database Consistency:** Mandatory Transaction wrapping for all multi-row scrobble history updates.

### Logging & Diagnostics
- **Release Mode Logging:** In release builds, stdout/stderr are unavailable. Both the daemon (`daemon_log!` macro) and the UI Rust backend (`ui_log` function) write to file-based logs in the OS application data directory (`%APPDATA%/HifiMule/` on Windows).
  - Daemon log: `daemon.log`
  - UI log: `ui.log`
- **Debug Mode:** Standard `println!`/`eprintln!` output to the terminal as usual.

### Device IO Abstraction

All device file operations MUST go through the `DeviceIO` trait. Direct `std::fs` calls targeting device paths are forbidden outside the `MscBackend` implementation.

```rust
trait DeviceIO: Send + Sync {
    fn read_file(&self, path: &str) -> Result<Vec<u8>>;
    fn write_file(&self, path: &str, data: &[u8]) -> Result<()>;
    fn list_files(&self, path: &str) -> Result<Vec<FileEntry>>;
    fn delete_file(&self, path: &str) -> Result<()>;
    fn free_space(&self) -> Result<u64>;
    fn write_with_verify(&self, path: &str, data: &[u8]) -> Result<()>;
}

struct MscBackend { root: PathBuf }        // std::fs — MSC drive path
struct MtpBackend { device: MtpHandle }    // WPD (Win) / libmtp (Linux, macOS)
```

**Atomic writes over MTP:** MTP has no native rename operation. The Write-Temp-Rename pattern is MSC-only. For MTP, `write_with_verify()` writes a `".dirty"` marker object first, overwrites the target in-place, then removes the marker. This provides crash detection (dirty marker present on reconnect) without native atomicity.

**Backend selection:** `DeviceManager` instantiates the correct backend at detection time based on device class (MSC vs MTP) and passes it as `Arc<dyn DeviceIO>` to all downstream callers (sync engine, manifest handler, scrobble reader).

**Enforcement:** All AI agents MUST use `DeviceIO` methods for any read/write targeting the device. Never call `std::fs` with a device path directly.

### Subsonic URL Sanitization (Security Requirement)

Subsonic embeds auth credentials (`u`, `p`, `t`, `s`) as query parameters in every URL, including stream/download URLs. This is a security requirement, not an optimization:

- All Subsonic URLs **MUST** be sanitized via `sanitize_subsonic_url()` before logging.
- The function strips `u`, `p`, `t`, `s` params and replaces with `[REDACTED]`.
- Stream and download URLs must **NEVER** appear in log files with credentials intact.

### Enforcement Guidelines

**All AI Agents MUST:**
- Use the provided `ts-rs` macros to ensure the IPC contract is strictly adhered to.
- Validate filesystem path lengths before attempting write operations on legacy hardware.
- Commit manifest changes ONLY after `sync_all` has returned successfully.
- Use `DeviceIO` trait methods for all device file operations — never `std::fs` directly with a device path.
- Route all media server API calls through `Arc<dyn MediaProvider>` — never call Jellyfin or Subsonic HTTP APIs directly outside of `providers/` module.
- Call `sanitize_subsonic_url()` on any Subsonic URL before passing to `tracing::` macros or file-based logging.
- Use `String` for all item/track/album/artist IDs — never `i64` or `u64`.

## Library Browsing — Multi-Provider RPC Contract

### RPC Method Inventory

Level-specific `browse.*` methods expose the provider hierarchy to the UI. Each maps to exactly one `MediaProvider` call; no generic dispatch exists.

| Method | Params | Returns |
|---|---|---|
| `browse.listLibraries` | — | `{ libraries: Library[] }` |
| `browse.listArtists` | `{ libraryId?: string, letter?: string }` | `{ artists: Artist[], total: number }` |
| `browse.getArtist` | `{ artistId: string }` | `{ artist: Artist, albums: Album[] }` |
| `browse.listAlbums` | `{ libraryId?: string, startIndex?: number, limit?: number }` | `{ albums: Album[], total: number }` |
| `browse.getAlbum` | `{ albumId: string }` | `{ album: Album, tracks: Track[] }` |
| `browse.listPlaylists` | — | `{ playlists: Playlist[] }` |
| `browse.getPlaylist` | `{ playlistId: string }` | `{ playlist: Playlist, tracks: Track[] }` |
| `browse.listModes` | — | `{ modes: BrowseMode[] }` |
| `browse.listGenres` | `{ libraryId?: string, startIndex?: number, limit?: number }` | `{ genres: Genre[], total: number }` |
| `browse.getGenre` | `{ genreIdOrName: string, startIndex?: number, limit?: number }` | `{ genre: Genre, tracks: Track[], total: number }` |
| `browse.listRecentlyAdded` | `{ libraryId?: string, startIndex?: number, limit?: number }` | `{ tracks: Track[], total: number }` |
| `browse.listFrequentlyPlayed` | `{ libraryId?: string, startIndex?: number, limit?: number }` | `{ tracks: Track[], total: number }` |
| `browse.listRecentlyPlayed` | `{ libraryId?: string, startIndex?: number, limit?: number }` | `{ tracks: Track[], total: number }` |
| `browse.listFavorites` | `{ libraryId?: string, startIndex?: number, limit?: number }` | `{ tracks: Track[], total: number }` |
| `browse.listTracks` | `{ libraryId?: string, artistId?: string, albumId?: string, letter?: string, startIndex?: number, limit?: number }` | `{ tracks: Track[], total: number, startIndex: number, limit: number }` |

**Response shapes (camelCase per IPC naming convention):**
```typescript
type Library  = { id: string; name: string }
type Artist   = { id: string; name: string; albumCount: number; coverArtId: string | null }
type Album    = { id: string; name: string; artistId: string; artistName: string;
                  year: number | null; trackCount: number; coverArtId: string | null }
type Track    = { id: string; title: string; artistName: string; albumName: string;
                  trackNumber: number | null; duration: number; bitrateKbps: number | null;
                  coverArtId: string | null; sizeBytes: number | null;
                  dateAdded?: string | null; lastPlayedAt?: string | null;
                  playCount?: number | null; isFavorite?: boolean | null }
type Playlist = { id: string; name: string; trackCount: number; durationSeconds: number }
type Genre    = { id: string; name: string; trackCount: number | null; coverArtId: string | null }
type BrowseMode = "artists" | "albums" | "playlists" | "tracks" | "genres" |
                  "recentlyAdded" | "frequentlyPlayed" | "recentlyPlayed" | "favorites"
```

`browse.listModes` is capability-driven. The daemon returns only modes that the active provider can service reliably. The UI must hide unsupported modes instead of issuing requests that are expected to fail.

### Subsonic Library Level

`SubsonicProvider::list_libraries()` returns one synthetic entry:
```rust
vec![Library { id: "all".into(), name: "All Music".into() }]
```

**UI rule:** when `libraries.length === 1`, the library picker is hidden and `libraryId` is auto-forwarded to `"all"` on all subsequent `browse.*` calls — making the Subsonic single-library experience visually identical to a single-library Jellyfin setup.

`SubsonicProvider::list_artists` ignores `library_id` entirely; Subsonic has no per-library artist scope.

### Alphabetical Quick-Nav — Provider Contract

**Trait amendment** — `list_artists` gains a `letter` parameter:
```rust
async fn list_artists(
    &self,
    library_id: Option<&str>,
    letter: Option<char>,   // None = all; Some('A') = artists whose name starts with A
) -> Result<Vec<Artist>, ProviderError>;
```

**Provider implementations:**
- **JellyfinProvider:** appends `&NameStartsWith={letter}&NameLessThan={next_letter}` to the `/Artists` query. Server-side filter; only matching artists are transferred.
- **SubsonicProvider:** calls `GET /rest/getArtists.view` once (no filter param in the API); filters the returned index array by matching the letter key (`index.iter().find(|i| i.name == letter_str)`). Full artist list is fetched in-process; no caching at the daemon layer.

`browse.listArtists` forwards `letter` (single uppercase char or absent) directly to `provider.list_artists()`.

### Tracks Browse Mode — Provider Contract

**Trait method:**
```rust
pub struct TrackListFilter {
    pub library_id: Option<String>,
    pub artist_id: Option<String>,
    pub album_id: Option<String>,
    pub letter: Option<char>,
    pub start_index: u32,
    pub limit: u32,
}

pub struct TrackListPage {
    pub tracks: Vec<Track>,
    pub total: u32,
    pub start_index: u32,
    pub limit: u32,
}

async fn list_tracks(&self, filter: TrackListFilter) -> Result<TrackListPage, ProviderError>;
// Default impl returns ProviderError::NotSupported.
```

**Provider implementations:**
- **JellyfinProvider:** `GET /Users/{uid}/Items?IncludeItemTypes=Audio&Recursive=true&SortBy=Name,Album&StartIndex&Limit[&ArtistIds][&AlbumIds][&NameStartsWith]`. When both `artist_id` and `album_id` are supplied, the album filter takes precedence (album implies its artist).
- **SubsonicProvider:** unfiltered enumeration uses `search3?query=&songCount&songOffset`. When `artist_id` is set, the adapter composes from `getArtist` (album list) + `getAlbum` (tracks). When `album_id` is set, `getAlbum` is used directly. Classic Subsonic without `search3` returns `ProviderError::NotSupported` and omits `BrowseMode::Tracks` from `BrowseCapabilities::list_modes`. All Subsonic URL auth sanitization rules apply.

**Capability gating:** `browse.listModes` includes `Tracks` only when `provider.capabilities().browse.list_modes` contains `BrowseMode::Tracks`. A `browse.listTracks` call on a provider lacking the capability returns an RPC error.

### Cover Art Routing

All browse responses carry `coverArtId: string | null`. The UI fetches artwork exclusively via the existing `image_proxy` Tauri command — it never calls `cover_art_url()` directly.

For Subsonic, `coverArtId` is the `coverArt` field from the API response, which is **not** equal to the item ID. `SubsonicProvider` maps this field into the domain type at the adapter boundary. No caller outside `providers/subsonic.rs` is aware of this distinction.

**Enforcement:** All AI agents MUST use `provider.cover_art_url(cover_art_id, size)` to build artwork URLs — never construct Subsonic or Jellyfin artwork URLs manually.

## Epic 8: Provider Layer — Remaining Architectural Decisions

### Provider Type Definitions

All types live in `providers/mod.rs` alongside the `MediaProvider` trait.

**`ProviderError`:**
```rust
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("HTTP {status}: {message}")]
    Http { status: u16, message: String },
    #[error("Authentication failed: {0}")]
    AuthFailed(String),
    #[error("Item not found: {0}")]
    NotFound(String),
    #[error("Capability not supported: {0}")]
    NotSupported(String),
    #[error("Deserialization error: {0}")]
    Deserialization(String),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
```

**`ChangeEvent`, `ItemRef`, `ItemType`:**
```rust
pub enum ChangeEvent {
    Added(ItemRef),
    Updated(ItemRef),
    Removed { id: String, item_type: ItemType },
}
pub struct ItemRef { pub id: String, pub item_type: ItemType }
pub enum ItemType { Song, Album, Artist, Playlist }
```

**`SearchResult`** (domain layer, uses types from `domain/models.rs`):
```rust
pub struct SearchResult {
    pub artists: Vec<Artist>,
    pub albums:  Vec<Album>,
    pub songs:   Vec<Song>,
}
```

**`Capabilities`:**
```rust
pub struct Capabilities {
    pub open_subsonic: bool,                  // OpenSubsonic extension detected
    pub supports_changes_since: bool,          // getIndexes?ifModifiedSince reliable
    pub supports_server_transcoding: bool,     // PlaybackInfo (Jellyfin) / stream params (Subsonic)
}
```
Cached via `std::sync::OnceLock<Capabilities>` in each provider struct, populated on first `server.connect`. Reset only when `server.connect` is called with a new URL (i.e. when the provider is replaced).

### Trait Amendment — `scrobble()`

The `MediaProvider` trait gains one additional method:
```rust
async fn scrobble(&self, track_id: &str, timestamp_ms: u64) -> Result<(), ProviderError>;
```
- **JellyfinProvider:** calls the Progressive Sync API (`POST /Sessions/{sessionId}/Playing/Stopped`).
- **SubsonicProvider:** calls `GET /rest/scrobble.view?id={track_id}&submission=true&time={timestamp_ms}`.

`ScrobbleSubmitter` holds `Arc<dyn MediaProvider>` and calls `provider.scrobble()` exclusively — no `match provider.server_type()` branching outside `providers/`.

### Provider Factory

A free function in `providers/mod.rs`:
```rust
pub async fn connect(
    url: &str,
    creds: &Credentials,
    hint: ServerTypeHint,
) -> Result<Arc<dyn MediaProvider>, ProviderError>

pub enum ServerTypeHint { Auto, Jellyfin, Subsonic }
```

**Auto-detection ping sequence** (when `hint = Auto`):
1. `GET /rest/ping.view` → `openSubsonic: true` in response → `SubsonicProvider` (OpenSubsonic)
2. `GET /rest/ping.view` succeeds, no `openSubsonic` flag → `SubsonicProvider` (classic)
3. `GET /System/Info` succeeds → `JellyfinProvider`
4. All fail → `ProviderError::AuthFailed("Unknown server type at this URL")`

When `hint` is `Jellyfin` or `Subsonic`, the detection step is skipped and the specified provider is instantiated directly.

### Provider Lifecycle

`AppState` holds the active provider:
```rust
pub struct AppState {
    // ...existing fields...
    pub provider: Arc<RwLock<Option<Arc<dyn MediaProvider>>>>,
}
```

- RPC handlers acquire a read lock: `state.provider.read().await` → clone the `Arc` → release lock immediately before any async work.
- `server.connect` acquires a write lock, calls `connect()`, replaces the inner `Option`. The old provider (and any in-memory credentials) is dropped when the `Arc` refcount reaches zero.
- All `browse.*`, `sync.*`, and scrobble RPC handlers that need the provider call a shared helper:
  ```rust
  async fn require_provider(state: &AppState) -> Result<Arc<dyn MediaProvider>, RpcError> {
      state.provider.read().await.clone().ok_or(RpcError::NotConnected)
  }
  ```

### Server Config Persistence

Server URL, detected type, and username are persisted in SQLite so the daemon can reconnect on restart. Credentials remain exclusively in the encrypted local vault (`secrets.enc`).

**Schema:**
```sql
CREATE TABLE IF NOT EXISTS server_config (
    id          INTEGER PRIMARY KEY CHECK (id = 1),  -- single-row enforced
    url         TEXT    NOT NULL,
    server_type TEXT    NOT NULL,  -- 'jellyfin' | 'subsonic'
    username    TEXT    NOT NULL,
    updated_at  INTEGER NOT NULL   -- unix timestamp
);
```

On daemon startup: if a `server_config` row exists, the daemon calls `connect()` with the stored URL, fetches credentials from the encrypted local vault (`secrets.enc`), and restores the active provider before the RPC server starts accepting requests.

### Subsonic Auth Internals

`SubsonicProvider` fetches the password from the encrypted local vault **once at construction time** and holds it in memory for the session lifetime of the struct.

Every outgoing HTTP request appends auth params:
```
u={username}&t={md5(password + salt)}&s={salt}&v=1.16.1&c=hifimule&f=json
```
where `salt` is a freshly generated random alphanumeric string **per request**.

- The raw password **never leaves** `providers/subsonic.rs` — not stored in `AppState`, not passed to callers, not logged.
- When `server.connect` replaces the provider, the old `SubsonicProvider` (and its in-memory password) is dropped with the `Arc`.
- All Subsonic URLs containing auth params MUST be sanitized via `sanitize_subsonic_url()` before any logging (existing enforcement rule).

### Subsonic Incremental Sync — Album-Level Fallback

`SubsonicProvider::changes_since(since)` handles the Navidrome/Subsonic limitation internally. The sync engine always receives a `Vec<ChangeEvent>` and is never aware of the fallback.

**Implementation contract inside `SubsonicProvider`:**
1. Call `GET /rest/getIndexes.view?ifModifiedSince={since_epoch_ms}`.
2. If the response indicates the artist index is unchanged **and** `since > EPOCH` (i.e. not initial sync): re-fetch every album present in the current manifest via `getAlbum` and compare song count + track ID set. Emit `ChangeEvent::Added` / `ChangeEvent::Removed` for any drift detected.
3. If `since == EPOCH` (initial sync): use `search3?query=&songCount=500&songOffset={n}` with pagination to enumerate all tracks instead of the index-based path.

**Enforcement:** All AI agents MUST NOT add album-level drift detection outside `providers/subsonic.rs`. The sync engine calls `provider.changes_since()` and processes the returned `Vec<ChangeEvent>` only.

## Epic 11: Selection-as-Playlist — Architectural Decisions

### Playlist Write Extension to `MediaProvider` Trait

Four new write methods are appended to the `MediaProvider` trait. All are capability-gated — callers **MUST** check `capabilities().supports_playlist_write` before calling.

```rust
// Playlist write operations (capability-gated)
async fn create_playlist(&self, name: &str, track_ids: &[String]) -> Result<String, ProviderError>; // returns server-assigned playlist ID
async fn add_to_playlist(&self, playlist_id: &str, track_ids: &[String]) -> Result<(), ProviderError>;
async fn remove_from_playlist(&self, playlist_id: &str, track_ids: &[String]) -> Result<(), ProviderError>;
async fn delete_playlist(&self, playlist_id: &str) -> Result<(), ProviderError>;
```

**Design decision:** The trait exposes **add/remove operations directly**, not a full-replace `update_playlist`. The daemon RPC layer computes the diff between the current server playlist state and the desired state, then calls `add_to_playlist` / `remove_from_playlist` as needed. Adapters implement each method as a single atomic server call.

### `Capabilities` Amendment

```rust
pub struct Capabilities {
    pub open_subsonic: bool,
    pub supports_changes_since: bool,
    pub supports_server_transcoding: bool,
    pub supports_playlist_write: bool,  // NEW — true for Jellyfin and Subsonic/OpenSubsonic
}
```

`supports_playlist_write` is set to `true` for both providers during `server.connect`. If a write call returns a server-side auth or capability error at runtime, the adapter returns `ProviderError::NotSupported`.

### Adapter Implementations

**JellyfinProvider:**
- `create_playlist`: `POST /Playlists` → reads `Id` from response body → returns as playlist ID.
- `add_to_playlist`: `POST /Playlists/{id}/Items?Ids={comma-separated track IDs}`.
- `remove_from_playlist`: fetch entry IDs via `GET /Playlists/{id}/Items` (Jellyfin uses per-entry `PlaylistItemId`, not the item ID), match against requested track IDs, then `DELETE /Playlists/{id}/Items?EntryIds={comma-separated entry IDs}`.
- `delete_playlist`: `DELETE /Items/{id}` (Jellyfin playlists are deleted as generic items).

**SubsonicProvider:**
- `create_playlist`: `GET /rest/createPlaylist.view?name={name}&songId[]={ids}` → reads `id` from response → returns as playlist ID.
- `add_to_playlist`: `GET /rest/updatePlaylist.view?playlistId={id}&songIdToAdd[]={ids}`.
- `remove_from_playlist`: fetch current song list via `GET /rest/getPlaylist.view`, resolve positions for the requested IDs, then `GET /rest/updatePlaylist.view?playlistId={id}&songIndexToRemove[]={indices}`.
- `delete_playlist`: `GET /rest/deletePlaylist.view?id={id}`.

### Daemon RPCs

**New RPC methods:**

| Method | Params | Returns |
|---|---|---|
| `playlist.create` | `{ name: string, itemIds: string[] }` | `{ playlistId: string }` |
| `playlist.addTracks` | `{ playlistId: string, trackIds: string[] }` | `{ ok: true }` |
| `playlist.removeTracks` | `{ playlistId: string, trackIds: string[] }` | `{ ok: true }` |
| `playlist.delete` | `{ playlistId: string }` | `{ ok: true }` |

**Selection→tracks resolution:**
- `playlist.create`'s `itemIds` follow the same entity format as `sync.start`'s `itemIds`. The daemon resolves all entity types (albums, artists, genres, individual tracks) to a concrete flat track list using the existing container-expansion logic (`rpc.rs:807–866`).
- Auto-Fill virtual slots (`id: '__auto_fill_slot__'`) are **excluded** from resolution. When present in the basket, the UI surfaces a notice to the user before saving; the daemon silently skips any slot if one is passed.
- Track ordering within a resolved entity is left to implementation.
- `playlist.create` calls `provider.create_playlist()` with the resolved track IDs and returns the server-assigned ID.
- `playlist.addTracks` / `playlist.removeTracks` pass `trackIds` directly to `provider.add_to_playlist()` / `provider.remove_from_playlist()` — no entity resolution; callers supply concrete track IDs only.

### Capability Gating

**Enforcement:** The UI MUST hide or disable all "Save as playlist" affordances when `capabilities().supports_playlist_write == false`. This mirrors the existing capability-driven browse-mode hiding pattern.

**All AI agents MUST:**
- Check `capabilities().supports_playlist_write` before invoking any playlist write RPC or trait method.
- Never construct Jellyfin or Subsonic playlist write URLs outside `providers/jellyfin.rs` and `providers/subsonic.rs`.
- Exclude Auto-Fill slots from all `playlist.create` / `playlist.addTracks` calls.
- Use `String` for playlist IDs — consistent with the project-wide ID type rule.

## Multi-Server Management — Architectural Decisions

### Motivation

The existing single-server architecture (`server_config CHECK (id = 1)`, single `Arc<RwLock<Option<Arc<dyn MediaProvider>>>>` in `AppState`) is replaced by a `ServerManager` that holds multiple server records with lazy per-server provider caching. The `MediaProvider` abstraction and `server_id` manifest field already anticipated this; this amendment activates them at the user-facing level.

### ServerManager — Replaces AppState.provider

```rust
pub struct ServerManager {
    servers: Vec<ServerRecord>,
    selected_server_id: Option<String>,
    providers: HashMap<String, Arc<dyn MediaProvider>>,  // lazy, keyed by server UUID
}

pub struct ServerRecord {
    pub id: String,           // stable UUID
    pub url: String,
    pub server_type: String,  // 'jellyfin' | 'subsonic'
    pub username: String,
    pub name: Option<String>, // user-facing display name
    pub icon: Option<String>, // built-in icon identifier
}

// In AppState — replaces `provider: Arc<RwLock<Option<Arc<dyn MediaProvider>>>>`
pub server_manager: Arc<RwLock<ServerManager>>,
```

**Provider lifecycle (lazy initialization):**
- Providers are instantiated via `providers::connect()` only when a server is first selected (`server.select` or first startup auto-select), not eagerly on daemon startup for all configured servers.
- This preserves the < 10MB idle memory NFR. Only the active provider's HTTP client and in-memory state are live at any time.
- When `server.remove` is called, the evicted provider's `Arc` is dropped from the cache; refcount reaches zero when all in-flight RPC handlers release their clones.

**require_provider() — semantic change:**
```rust
// Returns the selected server's provider, or RpcError::NotConnected if none selected.
async fn require_provider(state: &AppState) -> Result<Arc<dyn MediaProvider>, RpcError> {
    let mgr = state.server_manager.read().await;
    let id = mgr.selected_server_id.as_deref().ok_or(RpcError::NotConnected)?;
    mgr.providers.get(id).cloned().ok_or(RpcError::NotConnected)
}
```
All existing `browse.*`, `sync.*`, scrobble, and playlist RPC handlers continue to call `require_provider()` — no other changes needed at the call sites.

### Database Migration — server_config

The `CHECK (id = 1)` single-row constraint is removed. The table is recreated (SQLite does not support dropping CHECK constraints via ALTER TABLE).

```sql
-- New schema
CREATE TABLE IF NOT EXISTS server_config (
    id          TEXT    PRIMARY KEY,           -- stable UUID (was INTEGER with CHECK id=1)
    url         TEXT    NOT NULL,
    server_type TEXT    NOT NULL,              -- 'jellyfin' | 'subsonic'
    username    TEXT    NOT NULL,
    name        TEXT    NULL,                  -- user-facing display name
    icon        TEXT    NULL,                  -- built-in icon identifier
    selected    INTEGER NOT NULL DEFAULT 0,    -- NEW: 1 for the active server
    updated_at  INTEGER NOT NULL
);

-- Migration (runs on first startup after upgrade):
-- 1. Read existing INTEGER row (if any).
-- 2. Generate UUID for it.
-- 3. Drop old table, create new schema.
-- 4. Re-insert with UUID primary key and selected = 1.
```

On daemon startup: load all rows into `ServerManager.servers`; the row with `selected = 1` becomes `selected_server_id`. If no row has `selected = 1`, `selected_server_id` is `None`.

### Credential Vault — Restructuring

**Old format** (single blob): `Secrets { jellyfin_token: Option<String>, subsonic_password: Option<String> }`

**New format** (multi-server map):
```rust
// Decrypted contents of secrets.enc
type VaultContents = HashMap<String, ServerCredentials>;  // key = server UUID

pub struct ServerCredentials {
    pub token_or_password: String,  // Jellyfin token OR Subsonic password
}
```

**Migration path** (run on first load after upgrade):
1. Decrypt `secrets.enc`.
2. Attempt to deserialize as `VaultContents` (new format). If successful → done.
3. If deserialization fails, attempt as legacy `Secrets` struct.
4. If legacy format detected: obtain the existing server's UUID from `server_config`, wrap as `HashMap { uuid → ServerCredentials { token_or_password } }`, re-encrypt and overwrite `secrets.enc`.
5. If neither format parses, treat vault as empty (credentials lost — existing known limitation for hardware fingerprint changes).

**Known limitation (unchanged):** credentials are irrecoverably lost if the hardware fingerprint changes (VM migration, hardware replacement, OS reinstall). Re-authentication per server is required.

### IPC Contract Changes

**Modified — server.connect now returns serverId:**
```
server.connect(params: { url, serverType, username, password, name?, icon? })
  → { ok: true, serverId: string, serverType: string, serverVersion: string }
```

**New methods:**

| Method | Params | Returns |
|---|---|---|
| `server.list` | — | `Array<{ id, url, serverType, username, selected: boolean }>` |
| `server.select` | `{ id: string }` | `{ ok: true }` |
| `server.remove` | `{ id: string }` | `{ ok: true }` |

**get_daemon_state extended:**
```typescript
{
  // ...all existing fields...
  servers: Array<{ id: string, url: string, serverType: string,
                   username: string, selected: boolean }>,
  selectedServerId: string | null,
}
```

**Server identity amendment:** `server.list` and `get_daemon_state.servers` include `name: string | null` and `icon: string | null` for every server record. `server.connect` accepts optional `name` and `icon` fields. A new `server.update({ id, name?: string, icon?: string | null }) -> { ok: true }` RPC persists identity-only changes without reconnecting credentials or replacing the provider cache. UI labels use configured name/icon first, then fall back to URL host, username plus provider type, and finally provider type.

`server.select` updates both `ServerManager.selected_server_id` (in-memory) and the `selected` column in `server_config` (persist). Lazy-loads the provider if not already cached.

`server.remove` removes from DB, deletes the server's entry from the vault, evicts the provider from `ServerManager.providers`. If the removed server was selected, `selected_server_id` is set to the first remaining server's ID, or `None` if no servers remain.

### Basket Item Model — serverId Field

```typescript
type BasketItem = {
  id: string;
  type: BasketItemType;
  name: string;
  sizeBytes: number | null;
  serverId: string;          // NEW — UUID of originating server; set at add time
  // ...rest unchanged
}
```

- `serverId` is set to `state.selectedServerId` at the moment the item is added to the basket.
- `AutoFillSlot` virtual item also gains `serverId` (set to `selectedServerId` at toggle time). On toggle ON: any existing `__auto_fill_slot__` item is removed first, then a new one is inserted with `serverId = selectedServerId`.
- `basketStore` persists `serverId` per item in the basket manifest section of `.hifimule.json`.
- Items where `item.serverId !== state.selectedServerId` render as locked (CSS class `basket-item--locked`; `(×)` button hidden).

**basket.add / basket.remove RPC changes:**
- Both RPCs gain `serverId` in params.
- Daemon validates that `serverId` exists in `server_config` before accepting.

### sync.start — Multi-Provider Routing

`itemIds` param changes type:
```typescript
// Old
itemIds: string[]

// New
itemIds: Array<{ id: string, serverId: string }>
```

The daemon groups `itemIds` by `serverId`, calls `ServerManager.get_provider(serverId)` per group, and runs the existing container-expansion logic (`rpc.rs:807–866`) and download pipeline per group. Groups execute concurrently (one provider per server, bounded by the existing async task model).

`autoFill` param gains `serverId: string`; `run_auto_fill()` routes to `ServerManager.get_provider(serverId)` instead of the single global provider.

### Auto-Fill Pipeline Model (Epic 12/13)

Generalizes the single-slot, single-algorithm auto-fill (Stories 3.6/3.8) into a configurable pipeline definable **per `(device, portable serverId)` pair**. Lifting the one-slot-per-basket limit and storing a per-slot pipeline config are the same change.

**Config (manifest, portable, per `(device, portable serverId)`):**
```
manifest.autoFill : Map<serverId, AutoFillPipeline>     // replaces the single autoFill block
AutoFillPipeline {
  enabled: bool,
  filter:   { includeTags: [], excludeTags: [], includeGenres: [], excludeGenres: [] },
  sources:  [ { kind: "playlist"|"library"|"favorites"|"history"|..., ref?: id, share?: f32 } ],
  unit:     "track" | "album" | "artist",
  ordering: [ "favorite", "playCount", "dateCreated", "random", "quality", ... ],   // ordered keys
  memory:   { cooldownWeeks?: u32, playedExclusion?: bool, stableCorePct?: f32, repeatTolerance?: f32, tiers?: [...] },
  budget:   { maxBytes?: u64, targetDurationSecs?: u64, headroomBytes?: u64 },
  fallback: [ ... ]   // ordered terminal sources guaranteeing full fill
}
// Backward compat: a legacy { enabled, maxBytes } block is read as
// { ordering: ["favorite","playCount","dateCreated"], budget: { maxBytes } }.
```

**Runtime state (daemon DB, machine-local, keyed by device+serverId):**
```
autofill_history(device_id, server_id, track_id, last_synced_at, tier, ...)   // cooldown, stable-core, pity-timer
```

**Expansion (sync time):**
```
for each AutoFillSlot in basket (one per server):
    provider = get_provider_by_server_id(slot.serverId)
    tracks   = run_pipeline(provider, pipeline, db_history, manual_exclude_ids)
    merge into desired_items  (manual items still win dedup)
```

**Contract amendments (additive; legacy params still accepted):**
- `AutoFillSlot` is **one per server**: toggling auto-fill for the selected server inserts/updates *that server's* slot without removing other servers' slots. Slots for non-selected servers render read-locked.
- `sync.start` carries an **array** of per-server auto-fill descriptors `{ serverId, pipeline-or-default, excludeItemIds }`; the daemon runs `run_pipeline` per descriptor, routed via `get_provider_by_server_id`.
- `sync.setAutoFill` is **superseded by** `autoFill.setPipeline { deviceId, serverId, pipeline }`. `autoSyncOnConnect` stays on the device (server-independent). Legacy `sync.setAutoFill` params are still accepted and mapped to the default pipeline.
- `basket.autoFill` preview endpoint gains `serverId` + optional inline `pipeline`; returns ranked items for that server.

### Server Identity Model — Portable vs Machine-Local (Story 2.13)

**Problem:** Stories 2.11/2.12 used a single random `Uuid::new_v4()` (`server_config.id`) for
*everything* — DB row PK, vault key, provider-cache key, AND the manifest/basket/sync `serverId`.
Because that id is machine-local and random, `.hifimule.json` is not portable across machines,
and remove/re-add of the same logical server mints a new id → spurious full resync and orphaned
manifest items. (Pre-2.11 used a deterministic composite `type|url|user`; 2.11 regressed it.)

**Resolution — two distinct identities:**

| Identity | Column | Used for | Stability |
|---|---|---|---|
| `local_id` | `server_config.id` (unchanged) | DB row PK, **vault key**, **provider-cache key**, `server.select/remove/update` | Random UUID, machine-local |
| `server_id` (portable) | `server_config.server_id` (NEW) | device manifest `SyncedItem.server_id` / `BasketItem.server_id`, UI basket `serverId`, **sync routing** | Deterministic, identical across machines & re-adds |

**Derivation (daemon, at connect/upsert):**
```rust
fn derive_server_id(
    server_type: &str,
    canonical_base_url: &str,   // normalized_server_url(): scheme+host+port+path, lowercased host, no trailing slash
    username: &str,
    server_reported_id: Option<&str>,  // Jellyfin System/Info.Id; None for Subsonic/OpenSubsonic
) -> String {
    let basis = match server_reported_id {
        Some(rid) if !rid.is_empty() => format!("v1|{server_type}|rid:{rid}|{username}"),
        _                            => format!("v1|{server_type}|url:{canonical_base_url}|{username}"),
    };
    sha256_hex(basis.as_bytes())   // lowercase hex
}
```
- The `v1|` prefix and `rid:` / `url:` basis tags allow future versioning without collisions.
- `server_reported_id` is captured at connect into a new `server_config.server_reported_id TEXT NULL`
  so the basis is recomputable and stable. Jellyfin populates it from `System/Info.Id`; Subsonic/
  OpenSubsonic has no server-id concept → URL basis. **Consequence:** for URL-basis servers, a base-URL
  change yields a new logical identity (documented fallback); for `rid`-basis servers, identity survives URL changes.

**Schema amendment:**
```sql
ALTER TABLE server_config ADD COLUMN server_id           TEXT;  -- deterministic portable id
ALTER TABLE server_config ADD COLUMN server_reported_id  TEXT;  -- nullable; basis input
-- Backfill on migration: server_id = derive_server_id(server_type, url, username, NULL) for existing rows.
```

**ServerRecord amendment:**
```rust
pub struct ServerRecord {
    pub id: String,                      // machine-local id (was "stable UUID")
    pub server_id: String,               // NEW — deterministic portable id
    pub server_reported_id: Option<String>, // NEW
    pub url: String,
    pub server_type: String,
    pub username: String,
    pub name: Option<String>,
    pub icon: Option<String>,
}
```

**Routing translation:** vault and provider cache stay keyed by `local_id`. Manifest/basket/sync carry
`server_id`. `ServerManager` gains:
```rust
// Resolve a portable server_id to the local record, then reuse the existing per-local-id cache.
pub async fn get_provider_by_server_id(state: &AppState, server_id: &str)
    -> Result<Arc<dyn MediaProvider>, RpcError>;
```
`sync.start` grouping and `run_auto_fill()` route via `get_provider_by_server_id` instead of
`get_provider(local_id)`. On a single machine `server_id ↔ local_id` is 1:1 (upsert-by-URL prevents dupes).

**Reconciliation (idempotent, on startup/connect — no spurious resync):**
- Device manifests: rewrite any `synced_items[].server_id` / `basket_items[].server_id` that equals a known
  `local_id` (2.11 random UUID) **or** the pre-2.11 composite `type|url|user` → that server's portable `server_id`.
- UI: extend `reconcileServerIds()` to map `local_id → server_id` in addition to the existing composite mapping.
- Because re-deriving yields the same `server_id`, remove/re-add leaves manifest tags valid → delta sees items as unchanged.

**Contract amendments (additive — existing fields preserved):**
- `server.connect` → adds `serverId` (portable) and `localId` to the existing response.
- `server.list` / `get_daemon_state.servers[]` → each record adds `serverId` (portable) alongside `id` (local).
- `get_daemon_state` → adds `selectedServerPortableId`. `selectedServerId` keeps its current meaning (local id).
- `server.select` / `server.remove` / `server.update` → unchanged; continue to key on local `id`.
- UI basket `setActiveServerId()` switches to compare against `selectedServerPortableId`; basket items tag with portable `serverId`.

**Enforcement additions:**
- Never write a `local_id` into the device manifest or basket `serverId` — always the portable `server_id`.
- Keep the vault and provider cache keyed by `local_id`; translate portable→local at the routing boundary.
- `derive_server_id` is the single source of truth for portable identity — do not reconstruct the basis ad hoc.

### Enforcement — All AI Agents MUST

- Never access `AppState.provider` — that field no longer exists. Use `require_provider(state)` or `state.server_manager` directly.
- Never hardcode `server_config` queries that assume a single row or INTEGER primary key.
- Always pass `serverId` when adding items to the basket RPC.
- Group `sync.start` `itemIds` by `serverId` and route each group to its correct provider — never assume all items belong to the active provider.
- Never re-encrypt the vault with the legacy `Secrets` struct format — always use `HashMap<String, ServerCredentials>`.
- Treat auto-fill as **one slot per server**, never a global singleton; never remove another server's slot when toggling auto-fill for the selected server.
- Route every auto-fill pipeline expansion through `get_provider_by_server_id(slot.serverId)`; never assume the active provider.
- Store auto-fill pipeline **config** in the manifest (portable `server_id`-keyed); store cooldown/rotation **history** in the daemon DB. Never put runtime history in the manifest, never put user config in the DB.
- Evict provider cache entry on `server.remove` before returning `{ ok: true }`.

## Playback Extension — Approved Context (2026-09-11)

Playback architecture workflow: context approved; existing application foundation retained; core decisions in progress. The completed base architecture remains authoritative outside explicit playback amendments. No production code changes are authorized by this design document alone.

### Inputs and precedence

- User-approved playback brainstorming and subsequent conversation decisions.
- `../brainstorming/brainstorming-session-2026-09-11-090745.md`: accepted listening behaviors; early feasibility statements are superseded by completed reports.
- `../implementation-artifacts/playback-session-results.md`: completed lifecycle/native-controls proof and limitations.
- `../implementation-artifacts/playback-feasibility-results.md`, `playback-windows-vm-results.md`, `playback-linux-vm-results.md`: decoder/output evidence.
- Existing PRD and architecture: provider boundaries, portable server identities, managed-device safety, RPC conventions and idle resource goals. Earlier no-device UI locking requires a playback-specific amendment.

### Requirements overview

1. The local Rust daemon owns audio and native controls independently of the detachable UI. Playback and sync normally run together; conditional backoff protects playback when actual contention appears.
2. Sessions support ordered gapless albums, continually replenished Radio, and full-track previews preserving the original queue and position. A preview's natural end resumes the preserved listening session. Restored sessions remain paused on launch.
3. Playback reuses the selection engine with independent settings, bounded upcoming tracks and cross-server identities. Radio remains close to its current artist and moves through meaningful relationships, not shared genre alone. Same-recording duplicates must not merge distinct performances.
4. Session-scoped skipped-track exclusions survive restoration of that session and reset with a new Radio. There is no cross-session local taste model. Durable preferences belong to capable underlying servers.
5. Playback chooses the highest sustainable quality, using buffering to inform automatic adaptation. Album continuity and recorded silence are preserved. Loudness matching respects Radio versus album context. Output-device loss pauses playback without silently switching speakers.
6. Playback is the first, always-present virtual destination, selected when no physical device is connected. Physical-device arrival selects its basket without stopping music; unconfigured devices retain their configuration affordance. Floating controls and Play something support low-friction listening.
7. Explicit session snapshots retain accepted played tracks and upcoming tracks, omitting skipped/disliked tracks. Server export splits a mixed-source snapshot into ordered per-server playlists. Connected-device export offers distinct Add and Replace actions.

### Non-functional constraints

Windows, macOS and Linux are required. Shared audio is the default; OS Do Not Disturb remains outside HifiMule. Callback delivery must be isolated from blocking I/O, allocations and sync-held locks. Compressed prefetch, decoded PCM and candidate discovery each need independent bounds. The existing less-than-10-MB requirement concerns idle operation; active playback needs measured resource budgets rather than an invented promise.

Native FFmpeg decoding and callback delivery passed short generated-fixture tests on the three tested ARM64 environments. Those results do not establish streaming adaptation, real-library relationship coverage, mixed-rate physical gaplessness, x64 parity or prolonged real-sync coexistence. Ubuntu Wayland teardown warnings and physical Windows/Linux keyboard routing remain integration checks.

### Existing foundation retained

This is an extension of the current Rust daemon, Tokio runtime, native Tao tray loop, SQLite persistence, provider abstraction, JSON-RPC bridge and Tauri/TypeScript UI. No starter generation, application reinitialization or framework migration is required. The existing completed starter selection is inherited; the next work concerns playback-specific design decisions.

### Architectural concerns to resolve

- Separate playback destination identity from mounted-device identity and destructive sync operations.
- Define session/queue/source identity, restoration, preview state and snapshot consistency.
- Keep audio ownership and native registration in the interactive desktop session with one daemon instance.
- Bound queue growth and library candidate work while sharing selection behavior with sync.
- Extend provider capabilities for streams, reporting, preferences and metadata without leaking provider APIs into playback/UI.
- Define quality adaptation and reporting behavior that can honestly degrade when providers lack necessary capabilities.
- Package and verify the actual audio runtime on each target; preserve the project's deployment and credential boundaries.

### Playback State and Ownership — Approved

The Playback destination owns selection settings independently of physical devices. It is a typed playback destination, never a simulated mount or a target for filesystem synchronization.

A daemon session manager owns queue order, current position, Radio state and temporary audition state. UI and native media controls address that same manager. The audio engine consumes buffered PCM independently of UI and sync work.

Use the existing SQLite database for durable session restoration. Restore paused, including the current logical Radio session's skipped-track exclusions. Starting a new Radio clears those exclusions; restoration does not create a cross-session taste model. Playback configuration remains separate from physical-device settings; its persistence format will be specified with implementation patterns.

Each queued occurrence has its own queue-entry identity. Source identity is the portable server ID plus provider track ID. Recording identity is separate and supports conservative cross-server deduplication without collapsing repeated queue entries or distinct performances. The shared selection engine currently lacks an explicit server dimension in Candidate/SourceKey: its playback integration must preserve source identity through a deliberate typed boundary rather than merge raw provider IDs.

Maintain one preserved main session during preview. A subsequent preview replaces the current audition without nesting preserved sessions. Natural completion resumes the preserved session according to its retained state. Explicit stop and preview without a prior session require state-machine definitions before implementation.

Playback architecture workflow: state/ownership category approved; audio/streaming category next. Existing completed base-workflow steps remain unchanged.

### Playback Audio Pipeline — Approved

Use native FFmpeg decoding and CPAL output, building on the verified experiment. Package a controlled FFmpeg runtime; do not rely on arbitrary system versions. The experiment used CPAL 0.16.0 and ffmpeg-next 9.0.0 with FFmpeg 9 libraries. Newer CPAL versions require renewed platform validation before adoption; exact release packaging is a later implementation gate.

Pipeline: provider stream → bounded compressed prefetch → FFmpeg decoding and audio processing → bounded PCM queue → native shared output. Network, decoding and preparation run outside the audio callback; the callback must not perform blocking I/O, allocate, or acquire locks held by sync. Keep decoder objects on their owning workers.

Maintain a continuous output stream and prepare the next track before the current track completes. Preserve recorded silence, remove only known encoder padding, and perform explicit sample-rate/channel conversion when needed without reopening the output between tracks. Resampler continuity and mixed-format boundaries need validation beyond the existing same-rate fixture proof.

Bound compressed and decoded buffering separately. Keep decoded buffering small enough for responsive controls and compressed prefetch large enough to absorb network variation within measured resource limits. Select concrete sizes and thresholds from streaming measurements rather than fixed untested promises.

Prefer the best available sustainable quality. Use buffer duration and refill behavior to predict starvation; reduce quality automatically and recover conservatively to prevent oscillation. Initially switch quality at track boundaries. Mid-track replacement requires provider-specific seeking/timing validation; support for a transcode offset alone does not prove seamless switching. If the connection cannot sustain any available representation, surface buffering/retry rather than promise uninterrupted output.

Recovery respects listening mode: Radio may bypass unavailable tracks, whereas album playback pauses and retries without silently omitting tracks. Output-device loss pauses playback and does not silently reroute to another device. Sync backoff remains conditional on actual playback risk.

References checked during design: https://docs.rs/crate/cpal/latest and https://opensubsonic.netlify.app/docs/extensions/transcodeoffset/. Versions used in the experiment are evidence baselines, not claims about the latest release.

Playback architecture workflow: audio/streaming category approved; provider integration next.

### Playback Provider Integration — Approved

Extend the existing MediaProvider boundary for playback. Providers return a playback description covering source, representation/quality, authentication and seeking capabilities, rather than exposing only a download URL. Detect streaming, reporting, feedback and metadata capabilities independently per server; adapter behavior must be verified against supported server versions.

Route each playback, feedback, reporting and export operation through the track's portable source-server identity, independent of the currently browsed server. Credentials and authenticated stream URLs remain inside the daemon; controls receive metadata and status. Provider-specific URLs and API semantics stay in provider adapters.

Separate now-playing notifications from completed-listen submissions. Avoid submitting skipped tracks where server behavior allows it, and count completed previews. Exact completion/seek eligibility and server-side counting side effects require a reporting contract and integration tests. Never promise that a reported play can be undone. OpenSubsonic distinguishes now-playing from submission through its scrobble API: https://opensubsonic.netlify.app/docs/endpoints/scrobble/.

Expose Like/Dislike only when an adapter has a genuine supported server equivalent. Removing a favorite is not an implicit dislike. Unsupported feedback must not become a hidden HifiMule-only preference model.

Normalize recording identifiers, artist relationships and loudness metadata with provenance and explicit missing values. Availability of an API or server brand does not establish completeness or correctness of its metadata.

Playlist export freezes one local snapshot and splits it by contributing server, preserving relative order within each part. Track each server's success/failure independently and retain returned playlist identities for safe retry/reconciliation. Reporting retries require durable tracking; remote exactly-once effects cannot be promised without server idempotency support or reliable reconciliation.

Playback architecture workflow: provider integration approved; Radio selection next.

### Playback Radio Selection — Approved

Reuse the existing pure selection engine with playback-owned settings, candidate pools, session exclusions and a track-count budget. Preserve physical-device sync behavior. Play something uses the engine's first eligible selection and establishes that artist as the initial Radio center.

Stay with the current artist while eligible unheard tracks remain, then prefer an artist connected by supported relationship metadata. Retain the transition explanation and metadata provenance. Initially use metadata supplied by configured servers; no external metadata service is introduced by this decision.

If the current artist is exhausted and no meaningful relationship is available, select a fresh center using the original playback selection settings and explicitly label the transition as a new starting point. Do not present a shared genre or unsupported relationship as evidence of a musical connection. This fallback does not itself settle total candidate exhaustion or permission to repeat heard tracks.

Keep a bounded upcoming-track lookahead and replenish incrementally. Bound candidate fetching/index work separately; do not reload the entire library for each refill. Replenishment appends without reordering existing queue entries. Manually removed automatic suggestions stay excluded for the logical session, including across restoration.

Deduplicate conservatively at the recording level while retaining distinct source-server copies for playback-source resolution. Uncertain matches remain separate. Track occurrence identity, recording identity and source identity remain distinct.

Playback architecture workflow: Radio selection approved, including fresh-center fallback; UI/session-control integration next.

### Playback UI and Session Control — Approved

The daemon owns the authoritative playback session. UI actions, native media keys and app-menu commands use common command handlers. Browsing context and playback context are independent: changing the selected server or physical device does not change the playing source or stop playback.

Playback is the first always-present destination and is selected when no physical device is connected. A physical-device arrival selects that device's context and exposes initialization/configuration when needed. Floating playback controls remain available across views; reserve sufficient scroll space to keep final list items accessible.

Extend the existing JSON-RPC bridge with playback commands and versioned state. On reconnect, fetch an authoritative session snapshot. Queue edits carry a revision expectation; reject stale edits and refresh instead of overwriting newer changes. Define snapshot/event ordering and reconnect behavior in implementation patterns.

Animate progress locally between authoritative daemon updates, correcting after pause, seek or reconnect. Interpolation is presentation only: consumed audio state in the daemon determines position and reporting. Avoid constant high-frequency polling solely to animate the UI.

Closing the UI leaves playback running. Explicit Quit HifiMule saves session state and stops the daemon; a later launch restores paused. Production shutdown must coordinate existing sync work and managed-device safety. Play something starts a new Radio, while Resume continues the existing saved session.

Playback architecture workflow: UI/session-control integration approved; deployment and implementation sequence next.

### Playback Deployment and Implementation Sequence — Approved

Run playback in the signed-in user's daemon on Windows, macOS and Linux. Reuse the existing daemon native event loop for media controls. Introduce single-instance coordination so UI and startup launches cannot create competing players. Package a controlled FFmpeg runtime and verify actual loaded library versions in release validation. Test every shipping architecture; the ARM64 VM experiments do not establish Windows/Linux x64 compatibility.

Implementation sequence (2026-09-19 approved release split):
1. Retain delivered lifecycle, player/native output, albums/previews and manual Playback UI (15.1–15.14).
2. Add shared Back transport (15.15) and compact icon-and-small-label browse navigation (15.16), then verify/package the manual release (15.17).
3. Add automatic-selection settings and Radio (16.1–16.6).
4. Add source-server reporting/preferences and immutable exports (16.7–16.11).
5. Add adaptation, conditional backoff and sustained/installed validation of the expanded release (16.12–16.14).

Reuse early streaming measurements when preparing adaptation; the manual release does not claim adaptive quality or conditional backoff. The stages are dependency order, not permission to omit cross-platform support or delay basic error handling until release.

Playback architecture workflow: core decision categories approved; implementation contracts and edge cases under review.

### Playback Implementation Contracts — Approved

Back uses one daemon-owned command for the bar and supported native Previous/keyboard delivery. Use authoritative main-track position: above 3,000 ms restart; at or below 3,000 ms select the preceding retained occurrence, falling back to current-track restart without wrapping. During Preview restart only the audition. Preserve paused intent, output-loss inhibition, source identity, accepted forward order and immutable outcomes; specify replay identity, persistence and command admission in Story 15.15. Generation fences prevent obsolete preparation from publishing audio. Compact browse navigation in 15.16 changes presentation in the existing library mode bar, not playback ownership or browse semantics.

One session manager serializes mutations from UI, native controls and background work. Queue edits carry the expected queue revision; stale edits return a conflict and current revision so the caller refreshes before retrying. Position updates do not increment the queue-edit revision. Command IDs suppress duplicate queue mutations within a documented retention window; this is not a promise of indefinite or remote exactly-once execution.

Preview completion restores the main session's saved position and previous playing/paused state. Explicit preview Stop restores the main session paused. Without a main session, preview completion leaves playback idle. Output reconnection does not automatically resume audio.

When Radio exhausts unheard eligible tracks, begin another listening cycle while retaining logical-session skipped/removed exclusions. If no eligible track remains, enter an explained waiting state rather than loop or discard exclusions. Technical failures remain distinct from explicit user skips/dislikes.

Quit stops audio, checkpoints playback, and requests orderly sync cancellation before daemon exit, respecting existing managed-device integrity rules. Shutdown deadlines and failure behavior must be implemented without pretending that interrupted writes completed.

Save/export operates on an immutable snapshot and does not track subsequent queue changes. Include the current track once unless explicitly rejected; omit skipped/disliked entries. Queue occurrence identity prevents accidentally including the current occurrence twice without collapsing deliberate repeated occurrences.

Keep existing Rust/SQL snake_case and JSON camelCase conventions. Distinguish session IDs, queue-entry IDs, recording IDs and source identities in types. Store playback configuration in a versioned local configuration file; store recoverable session state in SQLite. Persist transitions and periodic position checkpoints outside the audio callback.

Use explicit idle, buffering, playing, paused and stopping states with structured reasons such as output loss or unavailable source. Test transition behavior, command races, restart recovery and immutable snapshots independently of audio hardware. Revision and timing values need explicit wire representations and units when schemas are finalized.

Playback architecture workflow: implementation contracts approved; project structure next.

### Playback Project Structure — Approved

Keep playback inside the existing daemon with these modules:

```text
hifimule-daemon/src/playback/
  mod.rs          Public commands and session handle
  model.rs        Typed identities, states and snapshots
  session.rs      Queue, previews and command ordering
  audio.rs        Output stream and PCM callback
  decoder.rs      FFmpeg ownership and conversion
  streaming.rs    Fetching, prefetch and quality adaptation
  radio.rs        Replenishment through shared selection
  persistence.rs  Session checkpoints and restoration
  config.rs       Versioned Playback settings
  reporting.rs    Listening eligibility and submission
  export.rs       Playlist and basket snapshots
  native.rs       Media-control integration

hifimule-ui/src/
  state/playback.ts
  components/PlaybackBar.ts
  components/PlaybackQueue.ts
  components/PlaybackSettings.ts
```

Existing boundaries remain: auto_fill/ is the shared selection engine and radio.rs supplies playback-specific inputs; providers/ owns server APIs, capabilities and normalized metadata; db.rs owns migrations and playback persistence uses its tables; main.rs owns lifecycle and the native event loop; rpc.rs validates and forwards commands; sync.rs owns sync cancellation and conditional resource backoff. Native platform details may be split beneath playback/native/ when warranted, without introducing another application event loop.

Existing browse/basket components expose playback actions through shared playback state. Extend existing daemon/UI manifests and build/release workflows for audio dependencies rather than scaffold another application. The Tauri RPC proxy remains the release-mode communication boundary. Keep tests beside relevant Rust modules, with cross-platform integration coverage for lifecycle, streaming and packaged audio. Retain experiments/playback-probe as a regression reference.

Playback architecture workflow: project structure approved; architecture validation next.

### Playback Validation Refinements — Approved

For playback, these amendments supersede incompatible older statements in this document:
- No-device locking applies to physical-device basket/sync actions, not library playback or its destination.
- Multiple source providers may be active for playback/prefetch; provider lifetime follows in-flight work and bounded cache policy, not the browsed server alone.
- Album playback must preserve disc/track sequence; entity expansion ordering is not discretionary for this use case.
- Physical auto-fill configuration remains in device manifests; Playback configuration lives in its versioned local file. Session/runtime state remains in SQLite.
- The idle-memory target must not be presented as a validated active-playback memory limit.

Every asynchronous playback operation carries a generation identity. Skip, seek, preview replacement and session replacement invalidate superseded work; late fetch/decode completions cannot publish PCM or mutate the current session. Cancellation and stale-result rejection are separate obligations.

Store the long-running Radio journey in SQLite and page history into the UI. Bound candidate retrieval/index caches, compressed prefetch, PCM and upcoming entries independently. Retain session-scoped exclusions without materializing an unbounded UI list or repeatedly rebuilding the whole library.

Preserve main-track resume information during previews and reopen the source when necessary. A failed return preserves the main session and exposes a recoverable error; it must not silently discard the queue or claim a successful resume. Seek/reopen accuracy requires provider integration tests.

Use track loudness gain for Radio and consistent album gain for album playback, with peak protection. When usable metadata is missing, leave gain unchanged. Do not infer dynamic-range compression or launch background loudness analysis from this requirement.

### Playback Architecture Validation Results

**Coherence:** Approved responsibilities, ownership, identities and module boundaries fit the existing Rust/Tauri/provider architecture. The explicit precedence rules above resolve older no-device, single-provider, configuration and ordering statements. Experimental compatibility is evidence for the tested versions/platforms only.

**Coverage:** Listening/preview, album continuity, Radio, virtual destination, restart recovery, multi-server routing, reporting, feedback, export, native controls and sync coexistence have architectural owners. Privacy boundaries retain server communication inside providers and introduce no external metadata service. Performance is addressed through independent bounds and measurement gates, not an unverified memory claim.

**Implementation readiness:** Ready to create implementation stories, not a complete executable contract for every feature. Exact RPC schemas/error codes, database migrations, wire units, event/snapshot recovery, command-dedup retention and packaged dependency versions remain to be specified. Scope these before coding the affected stage.

**Critical implementation gates:**
- Define the lifecycle/single-instance mechanism, authenticated local command access as appropriate to the existing application boundary, and safe shutdown/cancellation contract before stage 1 implementation.
- Finalize versioned state/command schemas, generation fencing, persistence migrations and configuration validation for the first consuming story.
- Pin and verify shipping decoder/output/native-control versions and the controlled FFmpeg build for each supported architecture.

**Important validation gates:** streaming/seek behavior and reporting semantics on supported servers; buffer/resource tuning; loudness metadata behavior; cross-server recording/relationship coverage; physical gapless and real-sync coexistence; output loss, sleep/wake and extended-run reliability; Windows/Linux physical media keys and Wayland teardown.

Checklist (architecture-wide implementation readiness, not workflow completion):
- [x] Project context analyzed
- [x] Scale and complexity assessed: existing desktop application with real-time audio and multi-provider integration
- [x] Technical constraints identified
- [x] Cross-cutting concerns mapped
- [ ] All critical implementation decisions documented with final versions
- [ ] Shipping technology/runtime configuration fully specified and verified
- [x] Integration boundaries defined
- [x] Performance considerations addressed
- [x] Naming conventions established
- [x] Module structure patterns defined
- [ ] Exact communication schemas and recovery contracts specified
- [x] Process behavior and recovery principles documented
- [x] Playback directory structure defined
- [x] Component boundaries established
- [x] Integration points mapped
- [x] Feature responsibilities mapped to modules

**Formal readiness: NOT READY for unrestricted implementation while critical gates above remain open.** The architecture design workflow is complete and ready for staged story planning. This status does not invalidate the completed feasibility experiments or prevent writing the first scoped implementation spec.

### Playback Implementation Handoff

Continue stories in the approved Epic 15/16 sequence above; Story 15.17 closes manual-release packaging and Story 16.14 owns installed validation of later features. Each story must close its applicable contract/version gate and define meaningful acceptance checks before execution. The lifecycle foundation is delivered; prepare Back (15.15) next and preserve physical sync safety and existing provider routing. Carry all accepted product behavior into the playback requirements/epics so older PRD assumptions cannot override this extension.

Keep experiments/playback-probe as the audio regression reference. No production playback code was added by this architecture workflow. Do not treat prior ARM64 VM tests as shipping-architecture certification.

Playback architecture workflow complete: context, decisions, implementation patterns, structure and validation approved. Outstanding implementation gates are explicit above.
