# HifiMule Daemon — Architecture

**Part:** `hifimule-daemon` | **Generated:** 2026-05-07 | **Scan depth:** Exhaustive

---

## Process Architecture

The daemon is a single-process Rust binary. Due to macOS requiring the main thread for GUI event loops, the architecture separates concerns:

```
OS Main Thread
  ├─ (macOS) tao event loop for system tray
  └─ start_daemon_core() ──► Spawns background OS thread
                                  └─ Tokio multi-thread runtime
                                       ├─ Axum HTTP server (port 19140)
                                       ├─ MSC device observer loop (1s polling)
                                       ├─ MTP device observer loop (3s polling)
                                       └─ Auto-sync spawner (on device connect)
```

`start_daemon_core()` returns an `Arc<AtomicBool>` shutdown flag and a `watch::Receiver<DaemonState>` channel that the tray icon listener subscribes to for updating the tray menu.

### DaemonState Enum

```rust
enum DaemonState {
    Idle,
    Syncing,
    Error,
    DeviceRecognized { name: String, profile_id: String },
    DeviceConnected(String),
    DeviceDisconnected,
}
```

Transitions are published via `tokio::sync::watch::Sender<DaemonState>` stored in `AppState.state_tx`.

---

## AppState

The central state object, wrapped in `Arc<AppState>` and shared across all Axum route handlers:

```rust
pub struct AppState {
    pub jellyfin_client: JellyfinClient,
    pub device_manager: Arc<DeviceManager>,
    pub db: DatabaseHandle,
    pub sync_operation_manager: Arc<SyncOperationManager>,
    pub last_scrobbler_result: RwLock<Option<ScrobblerResult>>,
    pub last_connection_check: Mutex<Option<(Instant, bool)>>,  // 5s cache
    pub size_cache: RwLock<HashMap<String, u64>>,               // item size cache
    pub state_tx: watch::Sender<DaemonState>,
}
```

---

## RPC Server (`rpc.rs`)

- **Axum 0.8** HTTP server bound to `0.0.0.0:19140`
- Single `POST /` handler dispatches on `method` field
- `GET /jellyfin/image/:id` proxies Jellyfin images
- CORS allows `https://tauri.localhost` and `http://localhost:1420`

### Method Dispatch (34 methods)

| Category | Methods |
|----------|---------|
| Auth | `test_connection`, `login`, `save_credentials`, `get_credentials` |
| Daemon | `get_daemon_state` |
| Device setup | `device_initialize`, `device_set_auto_sync_on_connect`, `device.select` |
| Device info | `device_get_storage_info`, `device_list_root_folders`, `set_device_profile` |
| Jellyfin browse | `jellyfin_get_views`, `jellyfin_get_items`, `jellyfin_get_item_details`, `jellyfin_get_item_counts`, `jellyfin_get_item_sizes` |
| Manifest | `manifest_get_basket`, `manifest_save_basket`, `manifest_get_discrepancies`, `manifest_prune`, `manifest_relink`, `manifest_clear_dirty` |
| Sync | `sync_get_device_status_map`, `sync_calculate_delta`, `sync_execute`, `sync_get_operation_status`, `sync_get_resume_state`, `sync.setAutoFill` |
| Auto-fill | `basket.autoFill` |
| Scrobbler | `scrobbler.getLastResult` |
| Transcoding | `device_profiles.list` |

---

## Device Management (`device/mod.rs`)

### DeviceManifest

The manifest is the source of truth for all sync state. It lives at `<device-root>/.hifimule.json`:

```rust
pub struct DeviceManifest {
    pub device_id: String,                      // UUID v4
    pub version: u32,                           // manifest schema version
    pub name: Option<String>,                   // human-readable name (40 char max)
    pub icon: Option<String>,                   // icon key ("usb-drive", "phone-fill", etc.)
    pub synced_items: Vec<SyncedItem>,          // files confirmed on device
    pub basket_items: Vec<BasketItem>,          // user's selection for next sync
    pub managed_paths: Vec<String>,             // folders owned by HifiMule
    pub dirty: bool,                            // true if sync was interrupted
    pub pending_item_ids: Vec<String>,          // IDs being synced when dirty was set
    pub auto_fill: AutoFillPrefs,               // auto-fill configuration
    pub auto_sync_on_connect: bool,
    pub transcoding_profile_id: Option<String>,
}
```

### Multi-Device Support

`DeviceManager` maintains:
```rust
connected_devices: RwLock<HashMap<PathBuf, (DeviceManifest, DeviceClass)>>
selected_device_path: RwLock<Option<PathBuf>>
unrecognized_device: Mutex<Option<(PathBuf, Arc<dyn DeviceIO>)>>
```

The "current device" concept maps to `selected_device_path` → lookup in `connected_devices`. The `device.select` RPC method changes `selected_device_path`.

### Device Detection

**MSC** (`run_observer`): polls `get_mounts()` every 1s. Per platform:
- **Windows**: reads registry `HKLM\SYSTEM\MountedDevices` + `GetLogicalDrives`
- **macOS**: scans `/Volumes/`
- **Linux**: reads `/proc/mounts`

For each mount, checks for `.hifimule.json` to identify managed devices.

**MTP** (`run_mtp_observer`): polls every 3s via `enumerate_mtp_devices()`. Platform dispatch:
- **Windows**: WPD COM API (`IPortableDeviceManager`)
- **Unix**: libmtp FFI `LIBMTP_Get_Connected_Devices()`

---

## MTP Backends (`device/mtp.rs`)

### Windows: `WpdHandle`

Uses `IPortableDevice` COM interface from WPD (Windows Portable Devices API):
- Opens a new COM session per operation (session-per-operation pattern for reliability)
- Garmin devices require a "shell copy" fallback due to WPD quirks
- `split_path_components()` validates path segments to prevent traversal

### Unix: `LibmtpHandle`

Uses FFI to the C `libmtp` library:
- Wraps a raw `LIBMTP_mtpdevice_t` pointer in a `Mutex` for thread safety
- `spawn_blocking` used in `MtpBackend` to run blocking calls off the async thread pool

---

## DeviceIO Abstraction (`device_io.rs`)

```rust
#[async_trait]
pub trait DeviceIO: Send + Sync {
    async fn read_file(&self, path: &str) -> Result<Vec<u8>>;
    async fn write_file(&self, path: &str, data: &[u8]) -> Result<()>;
    async fn delete_file(&self, path: &str) -> Result<()>;
    async fn create_dir(&self, path: &str) -> Result<()>;
    async fn list_files(&self, path: &str) -> Result<Vec<FileEntry>>;
    async fn file_exists(&self, path: &str) -> Result<bool>;
}
```

Two production implementations:
- **`MscBackend`**: `std::fs` operations; write uses `.tmp` → rename atomicity; `check_relative()` guards against path traversal
- **`MtpBackend`**: wraps `Arc<dyn MtpHandle>`; all calls use `spawn_blocking` for sync→async bridging

---

## Sync Engine (`sync.rs`)

### Delta Calculation

```rust
pub fn calculate_delta(desired: &[DesiredItem], manifest: &DeviceManifest) -> SyncDelta
```

Produces three lists:
- **`adds`**: items in `desired` not present in `synced_items` by `jellyfin_id`
- **`deletes`**: items in `synced_items` not in `desired`
- **`id_changes`**: items present by metadata match (name+artist+album) but with changed `jellyfin_id` (server re-scanned library)

ID-change detection prevents unnecessary re-downloads when Jellyfin regenerates item IDs.

### Sync Execution

`execute_sync()` runs as a `tokio::spawn` background task:

1. For each **add**: `JellyfinClient::get_item_stream()` → download bytes → `DeviceIO::write_file()` → per-file manifest update
2. For each **delete**: `DeviceIO::delete_file()` → manifest update
3. For each **id_change**: download new → delete old → manifest update
4. Generate M3U playlists for Rockbox
5. Process scrobbles (`scrobbler::process_device_scrobbles`)
6. Clear dirty flag on success

Progress is reported via `SyncOperationManager` — the operation object is updated per-file with `filesCompleted`, `bytesTransferred`, `currentFile`, etc.

### Path Construction & Sanitization

`construct_file_path()` builds paths in format `<managed-path>/<Artist>/<Album>/<Track>.<ext>`:
- Removes or replaces FAT32-illegal characters: `\ / : * ? " < > |`
- Truncates components to 255 bytes (FAT32 limit)
- Ensures total path ≤ 250 characters (Windows MAX_PATH safety margin)

### M3U Generation

`generate_m3u_files()` writes one `.m3u` file per Jellyfin playlist. Format matches Rockbox (relative paths, `#EXTINF` headers with duration in seconds).

---

## API Client (`api.rs`)

### `JellyfinClient`

Async HTTP client wrapping `reqwest::Client`. All methods are reentrant (no internal mutable state):

| Method | Endpoint |
|--------|----------|
| `authenticate_by_name` | `POST /Users/AuthenticateByName` |
| `test_connection` | `GET /System/Info/Public` |
| `get_views` | `GET /Users/{userId}/Views` |
| `get_items` | `GET /Items` (with filters) |
| `get_items_by_ids` | `GET /Items?Ids=...` |
| `get_child_items_with_sizes` | `GET /Items?parentId=...&Fields=MediaSources` |
| `get_item_details` | `GET /Items/{itemId}` |
| `get_item_sizes` | Parallel fetches of `get_item_details` for size extraction |
| `get_item_stream` | `POST /Items/{itemId}/PlaybackInfo` → stream URL |
| `get_image` | `GET /Items/{itemId}/Images/Primary` |
| `report_item_played` | `POST /Users/{userId}/PlayedItems/{itemId}` |
| `search_items` | `GET /Items?SearchTerm=...` |

### `CredentialManager`

- `save_credentials(url, token, user_id?)`: writes `url` + `user_id` to `config.json`; stores `token` in OS keyring
- `get_credentials()` → `(url, token, user_id?)`
- `validate_url(url)`: rejects non-HTTP/HTTPS or `localhost:19140` (SSRF guard)
- `validate_token(token)`: enforces max length

---

## Database (`db.rs`)

SQLite via `rusqlite` (statically bundled):

### Schema

```sql
CREATE TABLE devices (
    device_id TEXT PRIMARY KEY,
    device_profile_id TEXT,
    auto_sync_on_connect INTEGER NOT NULL DEFAULT 0,
    transcoding_profile_id TEXT,
    sync_rules TEXT
);

CREATE TABLE scrobble_history (
    item_id TEXT NOT NULL,
    played_at TEXT NOT NULL,
    device_id TEXT NOT NULL,
    UNIQUE(item_id, played_at, device_id)
);
```

Runtime migrations add new columns if absent (ALTER TABLE ADD COLUMN).

---

## Auto-Fill (`auto_fill.rs`)

Fetches Audio tracks from Jellyfin pre-sorted by:
```
SortBy=IsFavoriteOrLiked,PlayCount,DateCreated
SortOrder=Descending,Descending,Descending
```

Pages in 500-item batches, stops as soon as cumulative bytes exceed `max_fill_bytes`. Max 200 pages guard.

`rank_and_truncate()` — testable pure function — implements break-on-first-oversized semantics.

`expand_exclude_ids()` — expands container IDs to constituent track IDs for correct `ExcludeItemIds` filtering.

---

## Scrobbler (`scrobbler.rs`)

Parses Rockbox `.scrobbler.log` (AudioScrobbler 1.1, tab-separated). Matching strategy:
1. `GET /Items?SearchTerm=<title>&Artists=<artist>` → candidates
2. Filter by duration ±10 seconds
3. `POST /Users/{userId}/PlayedItems/{itemId}?DatePlayed=<timestamp>` on match
4. `INSERT OR IGNORE` into `scrobble_history` for deduplication

---

## Windows Service (`service.rs`)

Uses `windows-service` crate. Service name: `"hifimule-daemon"`.

- `install()`: creates/updates SCM entry with `AutoStart`; starts immediately
- `uninstall()`: stop + delete
- `run()` → `daemon_service_main()` → `run_service()`: registers SCM handler, reports status transitions, calls `start_daemon_core()`, polls for shutdown

---

## Logging

- **Debug**: stdout/stderr
- **Release**: `<AppData>/HifiMule/daemon.log` and `ui.log`, 1 MB cap, truncated on overflow
- `daemon_log!` macro is aware of `#[cfg(debug_assertions)]`
