# HifiMule — Integration Architecture

**Generated:** 2026-05-23 | **Scan depth:** Exhaustive

---

## Overview

HifiMule consists of two cooperating processes that run on the same machine. They communicate over a local HTTP connection using JSON-RPC 2.0.

```
┌─────────────────────────────────────────────────────────────┐
│  Tauri 2 Desktop Shell (hifimule-ui)                    │
│  ┌───────────────────────────────────┐                      │
│  │  WebView (TypeScript / Shoelace)  │                      │
│  │  - library.ts                     │                      │
│  │  - BasketSidebar.ts               │                      │
│  │  - rpc.ts: invoke('rpc_proxy')    │                      │
│  └───────────────────────────────────┘                      │
│           │ Tauri invoke IPC (rpc_proxy / image_proxy)      │
│  ┌────────▼──────────────────────────┐                      │
│  │  src-tauri/lib.rs                 │                      │
│  │  - rpc_proxy command              │──── HTTP POST ──────►│
│  │  - image_proxy command            │                      │
│  └───────────────────────────────────┘                      │
└────────────────────────────────────────────────────────────┬┘
                                                             │
                                                     localhost:19140
                                                             │
┌────────────────────────────────────────────────────────────▼┐
│  Daemon (hifimule-daemon)                                │
│  ┌───────────────────────────────────────────────────────┐  │
│  │  Axum HTTP Server (rpc.rs)                            │  │
│  │  POST / → JSON-RPC 2.0 dispatch                      │  │
│  │  GET /jellyfin/image/:id → provider-aware image proxy │  │
│  └──────────────┬────────────────────────────────────────┘  │
│                 │                                            │
│  ┌──────────────▼────────────────────────────────────────┐  │
│  │  AppState                                             │  │
│  │  - JellyfinClient (legacy/direct Jellyfin API path)   │  │
│  │  - provider (MediaProvider: Jellyfin/Subsonic/etc.)   │  │
│  │  - DeviceManager (DeviceManifest + DeviceIO)          │  │
│  │  - DatabaseHandle (SQLite via rusqlite)               │  │
│  │  - SyncOperationManager                               │  │
│  │  - size_cache (RwLock<HashMap>)                       │  │
│  │  - last_connection_check (Mutex, 5s cache)            │  │
│  │  - last_scrobbler_result (RwLock<Option>)             │  │
│  └───────────────────────────────────────────────────────┘  │
│                                                              │
│  Background tasks:                                           │
│  - MSC device observer (1s polling, filesystem)             │
│  - MTP device observer (3s polling, WPD/libmtp)             │
│  - Sync executor (tokio::spawn, per-sync background task)   │
└──────────────────────────────────────────────────────────────┘
                        │
               ┌────────▼─────────┐
               │ Media Server     │
               │ Jellyfin /       │
               │ Subsonic /       │
               │ OpenSubsonic     │
               └──────────────────┘
```

---

## IPC: UI ↔ Daemon

### Why not direct fetch?

Tauri 2 in release mode serves the WebView from `https://tauri.localhost`. A direct `fetch()` to `http://localhost:19140` is blocked by the browser as mixed-content. Instead, all calls go through a Tauri IPC command (`invoke`), which Rust handles and proxies to the daemon over plain HTTP from the Rust process.

### RPC Proxy

```typescript
// ui: src/rpc.ts
export async function rpcCall(method: string, params: any = {}): Promise<any> {
    return await invoke('rpc_proxy', { method, params });
}
```

```rust
// ui: src-tauri/src/lib.rs
#[tauri::command]
async fn rpc_proxy(method: String, params: serde_json::Value) -> Result<serde_json::Value, String> {
    // Constructs JSON-RPC 2.0 body, POSTs to http://127.0.0.1:19140
    // Extracts result or surfaces error.message
}
```

### Image Proxy

Cover art images cannot use the same `invoke` path (CSS `background-image` needs a URL or data URL). Instead:

```typescript
export async function getImageUrl(id: string, maxHeight?: number, quality?: number): Promise<string> {
    return await invoke('image_proxy', { id, maxHeight, quality });
    // Returns: "data:image/jpeg;base64,..."
}
```

The Rust `image_proxy` command fetches `GET http://127.0.0.1:19140/jellyfin/image/:id` from the daemon. The daemon keeps the route name for compatibility, but if a non-Jellyfin provider is active it resolves cover art through `MediaProvider::cover_art_url`.

---

## JSON-RPC 2.0 Protocol

### Request Format

```json
{
  "jsonrpc": "2.0",
  "method": "method_name",
  "params": { ... },
  "id": 1
}
```

### Response Format (success)

```json
{
  "jsonrpc": "2.0",
  "result": { ... },
  "id": 1
}
```

### Response Format (error)

```json
{
  "jsonrpc": "2.0",
  "error": {
    "code": -32003,
    "message": "No device connected"
  },
  "id": 1
}
```

### Error Codes

| Code | Constant | Meaning |
|------|----------|---------|
| -32001 | `ERR_INVALID_CREDENTIALS` | Auth failed with the active media server |
| -32002 | `ERR_INVALID_PARAMS` | Missing or invalid request parameters |
| -32003 | `ERR_CONNECTION_FAILED` | Cannot reach the media server or no device connected |
| -32004 | `ERR_STORAGE_ERROR` | Database, keyring, or filesystem write failed |
| -32005 | `ERR_INTERNAL_ERROR` | Unexpected internal error |

### CORS

The Axum server allows requests from `https://tauri.localhost` and `http://localhost:1420` (dev Vite server). This is set via `tower-http` CORS middleware in `rpc.rs`.

---

## Daemon ↔ Media Server

The daemon communicates with media servers over HTTP through a provider layer. `JellyfinClient` in `api.rs` still owns the Jellyfin REST client and legacy compatibility path. The `MediaProvider` trait in `providers/mod.rs` normalizes Jellyfin, Subsonic, Navidrome, and OpenSubsonic behavior into common library, artist, album, playlist, song, genre, search, change, download, cover-art, transcoding, and scrobble operations.

### Authentication Flow

1. User enters server URL + credentials in the UI login form.
2. UI probes the URL with `server.probe` and submits `server.connect` with `serverType: "auto"` unless the user chose a specific type.
3. The daemon tries Subsonic/OpenSubsonic first for compatible servers, otherwise authenticates with Jellyfin.
4. The daemon stores URL, server type, username, and server version in SQLite `server_config`; provider secrets live in the OS keyring. The legacy Jellyfin `config.json` path remains for compatibility.
5. Subsequent browse/sync calls use the active `MediaProvider`. Legacy `jellyfin_*` RPCs fall through to `JellyfinClient` only when the active provider is Jellyfin.

### Key Jellyfin API Calls

| Purpose | Endpoint |
|---------|----------|
| Auth | `POST /Users/AuthenticateByName` |
| Library views | `GET /Users/{userId}/Views` |
| Browse items | `GET /Items?userId=...&parentId=...` |
| Item details | `GET /Items/{itemId}` |
| Item stream URL | `POST /Items/{itemId}/PlaybackInfo` |
| Download stream | `GET <stream-url-from-PlaybackInfo>` |
| Report played | `POST /Users/{userId}/PlayedItems/{itemId}` |
| Image | `GET /Items/{itemId}/Images/Primary` |
| Search | `GET /Items?SearchTerm=...` |

### Key Subsonic/OpenSubsonic API Calls

| Purpose | Endpoint |
|---------|----------|
| Probe/auth | `GET /rest/ping.view` |
| Artists | `GET /rest/getArtists.view`, `GET /rest/getArtist.view` |
| Albums | `GET /rest/getAlbumList2.view`, `GET /rest/getAlbum.view` |
| Songs | `GET /rest/getSong.view`, `GET /rest/search3.view` |
| Playlists | `GET /rest/getPlaylists.view`, `GET /rest/getPlaylist.view` |
| Genres | `GET /rest/getGenres.view`, `GET /rest/getSongsByGenre.view` |
| Favorites | `GET /rest/getStarred2.view` |
| Downloads/streams | `GET /rest/download.view`, `GET /rest/stream.view` |
| Cover art | `GET /rest/getCoverArt.view` |
| Scrobble | `GET /rest/scrobble.view` |

OpenSubsonic-capable servers expose additional reliable history semantics. HifiMule advertises `recentlyAdded`, `frequentlyPlayed`, and `recentlyPlayed` only when provider capabilities say those modes are reliable.

---

## Daemon ↔ Device

### Device Detection

Two concurrent polling loops run in background tasks:

| Observer | Interval | Protocol |
|----------|----------|----------|
| MSC observer (`run_observer`) | 1 second | Scans OS mount points; checks for `.hifimule.json` |
| MTP observer (`run_mtp_observer`) | 3 seconds | Polls WPD (Windows) or libmtp (Unix) for connected MTP devices |

When a device is detected:
1. Check if a `.hifimule.json` manifest exists at the device root
2. If yes: load manifest → add to `connected_devices` HashMap → notify UI via `DaemonState`
3. If no: add to `unrecognized_device` pending slot → UI shows "Initialize" banner

### DeviceManifest Location

```
<device-root>/
└── .hifimule.json      DeviceManifest — source of truth for sync state
```

### DeviceIO Abstraction

All device I/O goes through the `DeviceIO` async trait:

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

Two implementations:
- **`MscBackend`**: standard `std::fs` + write-temp-rename atomicity
- **`MtpBackend`**: wraps an `Arc<dyn MtpHandle>` in `spawn_blocking` for sync-to-async bridging

---

## UI State Management

The UI has no global state store framework. State is managed at two levels:

### `BasketStore` (singleton, `state/basket.ts`)

- Holds the collection of items selected for sync
- Backed by `localStorage` for session persistence
- Syncs to daemon via `manifest_save_basket` (debounced 1s write)
- Hydrates from daemon's `manifest_get_basket` when a device connects
- Emits `CustomEvent('update')` for all subscribers

### `BasketSidebar` (component, owns UI refresh lifecycle)

- Polls `get_daemon_state` every **2 seconds** to detect:
  - New device connected / disconnected
  - Active sync operation (attach to progress)
  - Dirty manifest flag
  - Multi-device changes
- Polls `sync_get_operation_status` every **500ms** during an active sync
- `StatusBar` polls `get_daemon_state` every **3 seconds** independently via direct fetch (not invoke, because it was written before the mixed-content constraint was fully appreciated)

## Provider-Neutral Browse Flow

The current browser does not hard-code server type. On library initialization:

1. `library.ts` calls `browse.listModes`.
2. The daemon returns `MediaProvider::capabilities().browse.list_modes`.
3. The UI renders mode buttons for only those modes.
4. Each mode calls a provider-neutral RPC:
   - `browse.listArtists` / `browse.getArtist`
   - `browse.listAlbums` / `browse.getAlbum`
   - `browse.listPlaylists` / `browse.getPlaylist`
   - `browse.listGenres` / `browse.getGenre`
   - `browse.listRecentlyAdded`
   - `browse.listFrequentlyPlayed`
   - `browse.listRecentlyPlayed`
   - `browse.listFavorites` / `browse.listFavoriteItems`

Jellyfin currently advertises the full mode set. OpenSubsonic/Navidrome advertises artists, albums, playlists, genres, favorites, and reliable history modes. Classic Subsonic keeps history modes hidden and returns `UnsupportedCapability` if a hidden mode is called directly.

---

## Sync Flow (End-to-End)

```
User clicks "Start Sync"
        │
        ▼
BasketSidebar.handleStartSync()
  ├─ Extract manual item IDs from basket
  ├─ Optionally add autoFill params
  ├─ rpcCall('sync_calculate_delta', { itemIds, autoFill? })
  │     │
  │     ▼ Daemon: handle_sync_calculate_delta
  │       ├─ Resolve the active provider
  │       ├─ Expand containers (albums/playlists/artists/favorite groups) → tracks
  │       ├─ If autoFill and Jellyfin-backed: run_auto_fill() → merge results
  │       ├─ calculate_delta(desired_items, manifest) → SyncDelta
  │       └─ Return SyncDelta {adds, deletes, id_changes, playlists}
  │
  ├─ rpcCall('sync_execute', { delta })
  │     │
  │     ▼ Daemon: handle_sync_execute
  │       ├─ Generate operation_id (UUID)
  │       ├─ Mark manifest dirty (pending_item_ids set)
  │       ├─ tokio::spawn background sync task
  │       └─ Return { operationId }
  │
  ├─ Start 500ms polling: rpcCall('sync_get_operation_status', { operationId })
  │     │
  │     ▼ Background sync task (sync.rs: execute_sync)
  │       ├─ For each ADD: download from active provider → write to device → update manifest
  │       ├─ For each DELETE: remove file from device → update manifest
  │       ├─ For each ID_CHANGE: download new → delete old → update manifest
  │       ├─ Generate M3U playlists
  │       ├─ Process scrobbles (parse .scrobbler.log → submit through provider)
  │       └─ Clear dirty flag on success
  │
  └─ On status=complete: reset basket dirty flag, show "Sync Complete"
```

---

## Auto-Sync Flow (No UI Required)

```
Device connected (MSC or MTP observer detects)
        │
        ▼
DeviceManager.handle_device_detected()
  └─ Check db.get_device_mapping().auto_sync_on_connect == true
        │
        ▼
main.rs: run_auto_sync()
  ├─ Load manifest (basket_items as desired_items)
  ├─ Resolve desired sync items through the active provider
  ├─ calculate_delta()
  ├─ execute_sync() (same as manual sync)
  └─ send_sync_complete_notification() → OS desktop notification
```
