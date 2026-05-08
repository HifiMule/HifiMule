stepsCompleted: ['step-01-init', 'step-02-context', 'step-03-starter', 'step-04-decisions', 'step-05-patterns', 'step-06-structure', 'step-07-validation', 'step-08-complete']
workflowType: 'architecture'
status: 'complete'
completedAt: '2026-01-26'
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
- **Secure Storage:** `keyring` crate for OS-native credential management.

**Important Decisions (Shape Architecture):**
- **Data Persistence:** SQLite (`rusqlite`) for daemon state and scrobble history.
- **Async Runtime:** `tokio` for handling concurrent IO and mount events.

### Daemon Responsibilities
- **Media Provider Layer:** All server communication is mediated through a `MediaProvider` trait (`providers/jellyfin.rs` + `providers/subsonic.rs`). The daemon never calls server APIs directly — it holds an `Arc<dyn MediaProvider>` resolved at connect time based on server type detection.
- **Auto-Fill Algorithm:** Priority-based music selection engine (favorites → play count → creation date) querying the active `MediaProvider` via `get_favorites()`, `get_most_played()`, `get_recently_added()`.
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
    async fn get_artist(&self, id: &str) -> Result<ArtistWithAlbums, ProviderError>;
    async fn get_album(&self, id: &str) -> Result<AlbumWithTracks, ProviderError>;
    async fn search(&self, query: &str) -> Result<SearchResult, ProviderError>;
    fn download_url(&self, track_id: &str, profile: &TranscodeProfile) -> Result<Url, ProviderError>;
    fn cover_art_url(&self, item_id: &str, size: u32) -> Result<Url, ProviderError>;
    async fn list_playlists(&self) -> Result<Vec<Playlist>, ProviderError>;
    async fn get_playlist(&self, id: &str) -> Result<PlaylistWithTracks, ProviderError>;
    async fn changes_since(&self, since: SystemTime) -> Result<Vec<ChangeEvent>, ProviderError>;
    fn server_type(&self) -> ServerType;
    fn capabilities(&self) -> &Capabilities;
}

pub enum ServerType { Jellyfin, Subsonic }
```

Domain types (`Song`, `Album`, `Artist`, `Playlist`) live in `domain/models.rs` — independent of API DTOs. DTOs map to domain types via `From` conversions at the adapter boundary.

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
- **Manifest Extension:** `.hifimule.json` includes `auto_sync_on_connect` (boolean), `auto_fill` block (`{ "enabled": bool, "maxBytes": number | null }`), `transcoding_profile_id` (string | null), `name` (string | null), `icon` (string | null), and `server_id` (string | null — normalized server URL for multi-server manifests). All new fields use `#[serde(default)]` for backward compatibility.
- **device-profiles.json:** Seeded to `{app_data_dir}/device-profiles.json` on first daemon startup from an embedded binary asset (`include_bytes!`). User-editable post-install. Contains named `DeviceProfile` payloads for Jellyfin PlaybackInfo negotiation. A `passthrough` profile (`deviceProfile: null`) explicitly disables transcoding.

### DeviceManager Struct
```
connected_devices: HashMap<PathBuf, DeviceManifest>  // all currently connected managed devices
selected_device_path: Option<PathBuf>                // the device targeted by all UI operations
unrecognized_device_path: Option<PathBuf>            // device awaiting initialization
```
`get_current_device()` returns the manifest for `selected_device_path`. All existing callers (basket, sync, manifest, storage) are unchanged. When only one device is connected it is auto-selected.

### Authentication & Security
- **Credential Management:** Server credentials are stored in the OS-native secure vault (Windows Credential Manager, macOS Keychain, Linux Secret Service) using the `keyring` crate.
  - **Jellyfin:** Stores a rotatable access token. Re-authenticates on 401.
  - **Subsonic/OpenSubsonic:** Stores the user password (encrypted). Auth is stateless — credentials are sent on every request as `t=md5(password+salt)` + `s=salt`. The password is used only to compute per-request tokens; it is never stored in plaintext.
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
  - `device.initialize(params: { folderPath: string, profileId: string, name: string, icon: string | null })` → `{ ok: true }` — writes manifest including name and icon.
  - `get_daemon_state` response extended with `connectedDevices: Array<{path, deviceId, name, icon}>` and `selectedDevicePath: string | null`.
- **Transcoding IPC:**
  - `device_profiles.list` → `Array<{ id, name, description, deviceProfile: object | null }>` — reads from `device-profiles.json`.
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
