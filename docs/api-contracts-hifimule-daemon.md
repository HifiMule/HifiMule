# API Contracts — HifiMule Daemon

**Generated:** 2026-05-23 | **Last Updated:** 2026-06-17 | **Scan depth:** Deep | **Protocol:** JSON-RPC 2.0 over HTTP POST to `localhost:19140`

All requests: `Content-Type: application/json`  
All successful responses: `{ "jsonrpc": "2.0", "result": <value>, "id": 1 }`  
All error responses: `{ "jsonrpc": "2.0", "error": { "code": <N>, "message": "<text>" }, "id": 1 }`

**Primary error codes:** JSON-RPC standard `-32601` method not found, `-32602` invalid params, plus app codes `-1` connection failed, `-3` storage error, `-4` not found, `-5` unsupported capability, `-6` sync in progress, `-7` cross-server playlist conflict, `-8` selected server credential unauthorized.

The daemon now exposes a provider-neutral media-server layer. Legacy `jellyfin_*` RPC names remain supported for compatibility, but active Subsonic/OpenSubsonic connections are routed through `MediaProvider` where possible.

---

## Server Connection & Credentials

### `server.probe`

Detects a server type before authentication.

**Params:** `{ url: string }`  
**Returns:** `{ serverType: "jellyfin" | "subsonic" | "openSubsonic" | null }`

---

### `server.connect`

Connects to Jellyfin, Subsonic, Navidrome, or OpenSubsonic and stores the resulting provider configuration.

**Params:**
```json
{
  "url": "http://localhost:8096",
  "serverType": "auto",
  "username": "alexis",
  "password": "secret"
}
```

`serverType` accepts `"auto"`, `"jellyfin"`, or `"subsonic"`. Navidrome is treated as a Subsonic/OpenSubsonic-compatible server.

**Returns:**
```json
{
  "status": "success",
  "serverType": "jellyfin|subsonic|openSubsonic",
  "serverVersion": "string|null",
  "userId": "string|null"
}
```

---

### `server.logout`

Clears the active in-memory provider connection.

**Params:** none  
**Returns:** `{ "status": "success", "data": { "ok": true } }`

---

### `server.list` / `server.select` / `server.update` / `server.remove`

Multi-server management. Local server row IDs are used for management calls; deterministic portable `serverId` values are returned for basket, manifest, and sync routing.

| Method | Params | Returns |
|--------|--------|---------|
| `server.list` | none | `ServerSummary[]` |
| `server.select` | `{ "id": string }` | `{ "ok": true }` |
| `server.update` | `{ "id": string, "name"?: string, "icon"?: string | null }` | `{ "ok": true }` |
| `server.remove` | `{ "id": string }` | `{ "removedServerId": string, "reselectedServerId": string | null }` |

`ServerSummary = { id, serverId, url, serverType, username, name, icon, selected }`.

---

### `test_connection`

Tests Jellyfin token connectivity. Kept for legacy compatibility.

**Params:** `{ url: string, token: string }`  
**Returns:** `{ status: "ok" }` or error `-32003`

---

### `login`

Authenticates and persists credentials. Current UI uses `server.connect`; `login` now auto-detects compatible Subsonic/OpenSubsonic servers while preserving the legacy Jellyfin response shape where possible.

**Params:** `{ url: string, username: string, password: string }`  
**Returns:**
```json
{
  "AccessToken": "...",
  "User": { "Id": "...", "Name": "..." }
}
```
**Errors:** `-32001` (auth failed)

---

### `save_credentials`

Saves credentials directly (bypasses authentication).

**Params:** `{ url: string, token: string, userId?: string }`  
**Returns:** `true`

---

### `get_credentials`

Returns stored credentials.

**Params:** none  
**Returns:** `{ url: string, token?: string, userId: string | null, serverType?: string }` or `null` if not configured  
**Errors:** `-32004` (storage error, excluding expected "not configured" states which return `null`)

---

## Daemon State

### `daemon.health`

Lightweight health probe.

**Params:** none  
**Returns:** `{ "data": { "status": "ok" } }`

---

### `get_daemon_state`

Returns comprehensive daemon state snapshot. Polled by UI every 2s.

**Params:** none  
**Returns:**
```json
{
  "currentDevice": DeviceManifest | null,
  "deviceMapping": { "deviceProfileId": string, "autoSyncOnConnect": bool, "transcodingProfileId": string | null } | null,
  "serverConnected": bool,
  "dirtyManifest": bool,
  "pendingDevicePath": string | null,
  "autoSyncOnConnect": bool,
  "autoFill": { "enabled": bool, "maxBytes": number | null } | null,
  "activeOperationId": string | null,
  "connectedDevices": Array<{ "path": string, "deviceId": string, "name": string, "icon": string | null, "deviceClass": "msc" | "mtp" }>,
  "selectedDevicePath": string | null,
  "serverType": "jellyfin" | "subsonic" | "openSubsonic" | null,
  "servers": ServerSummary[],
  "selectedServerId": string | null,
  "selectedServerPortableId": string | null,
  "currentServer": ServerSummary | null
}
```

Note: `serverConnected` is cached for 5 seconds to avoid excessive media-server health checks.

---

## Device Setup

### `device_initialize`

Initializes a newly detected device that has no `.hifimule.json` manifest.

**Params:**
```json
{
  "folderPath": string,              // managed folder name ("Music") or "" for device root
  "playlistFolderPath": string | null, // optional playlist folder; defaults to folderPath
  "profileId": string,               // provider user/profile ID
  "transcodingProfileId": string | null,
  "name": string,                    // 1–40 characters
  "icon": string | null              // one of: "usb-drive", "phone-fill", "watch", "sd-card", "headphones", "music-note-list"
}
```
**Returns:**
```json
{
  "status": "success",
  "data": {
    "deviceId": string,
    "version": string,
    "managedPaths": string[],
    "playlistPath": string | null,
    "transcodingProfileId": string | null
  }
}
```
**Errors:** `-32002` (invalid name/icon/profile), `-32004` (no unrecognized device pending init or write failed)

---

### `device_set_auto_sync_on_connect`

Enables or disables automatic sync when this device is connected.

**Params:** `{ "deviceId": string, "enabled": bool }`  
**Returns:** `{ "status": "success", "autoSyncOnConnect": bool }`

---

### `device.update_manifest`

Updates the selected managed device manifest from Device Settings. Identity and transcoding profile changes are metadata-only. Folder changes return a relocation signal for the next sync preview.

**Params:**
```json
{
  "deviceId": string,
  "name": string,
  "icon": string | null,
  "transcodingProfileId": string | null,
  "musicFolderPath": string,
  "playlistFolderPath": string | null
}
```
`transcodingProfileId` must match `device-profiles.json`; `null` or `"passthrough"` clears device transcoding.

**Returns:**
```json
{
  "ok": true,
  "relocationRequired": bool,
  "cleanupPreview": {
    "tracksToRemove": number,
    "playlistsToRemove": number,
    "bytesToRemove": number
  }
}
```

**Errors:** `-32602` (invalid device, icon, profile, or folder path), `-3` (manifest/profile persistence failed)

---

### `device.select`

Switches the "current device" to the given path (multi-device hub).

**Params:** `{ "path": string }`  
**Returns:** `true`

---

### `device.list`

Returns connected managed devices.

**Params:** none  
**Returns:** `{ "status": "success", "data": Array<{ path: string, deviceId: string, name: string, icon: string | null, managedPaths: string[], playlistPath: string | null, transcodingProfileId: string | null, deviceClass: "msc" | "mtp" }> }`

---

### `set_device_profile`

Updates the device's provider profile ID and optional sync rules.

**Params:** `{ "deviceId": string, "profileId": string, "syncRules"?: string }`  
**Returns:** `true`

---

### `device.set_transcoding_profile`

Updates the current device's transcoding profile.

**Params:** `{ "deviceId": string, "profileId": string | null }`  
**Returns:** `true`

---

## Device Info

### `device_get_storage_info`

Returns storage statistics for the currently selected device.

**Params:** none  
**Returns:**
```json
{
  "totalBytes": number,
  "freeBytes": number,
  "usedBytes": number,
  "devicePath": string
}
```
or `null` if no device connected.

---

### `device_list_root_folders`

Lists root folders on the current device with managed/protected classification.

**Params:** none  
**Returns:**
```json
{
  "deviceName": string,
  "devicePath": string,
  "hasManifest": bool,
  "folders": Array<{ "name": string, "relativePath": string, "isManaged": bool }>,
  "managedCount": number,
  "unmanagedCount": number,
  "pendingDevicePath": string | null
}
```
or `null` if no device.

---

## Provider-Neutral Browse

Browse responses use camelCase provider-domain DTOs:

```typescript
type BrowseArtist = { id: string; name: string; albumCount?: number; coverArtId?: string | null };
type BrowseAlbum = { id: string; name: string; artistId?: string | null; artistName?: string | null; year?: number | null; trackCount?: number; coverArtId?: string | null };
type BrowsePlaylist = { id: string; name: string; trackCount?: number; durationSeconds?: number | null; coverArtId?: string | null };
type BrowseTrack = { id: string; title: string; artistId?: string | null; artistName?: string | null; albumId?: string | null; albumName?: string | null; duration: number; bitrateKbps?: number | null; coverArtId?: string | null; sizeBytes?: number | null; dateAdded?: string | null; lastPlayedAt?: string | null; playCount?: number | null; isFavorite?: boolean | null };
type BrowseGenre = { id: string; name: string; trackCount?: number | null; coverArtId?: string | null };
```

### `browse.listModes`

Returns browse modes supported by the active provider.

**Params:** none  
**Returns:** `{ "modes": Array<"artists" | "albums" | "playlists" | "tracks" | "genres" | "recentlyAdded" | "frequentlyPlayed" | "recentlyPlayed" | "favorites"> }`

---

### `browse.listArtists`

Lists artists for the active provider.

**Params:** `{ "libraryId"?: string, "letter"?: string, "startIndex"?: number, "limit"?: number }`  
**Returns:** `{ "artists": BrowseArtist[], "total": number }`

---

### `browse.getArtist`

Returns one artist plus albums.

**Params:** `{ "artistId": string }`  
**Returns:** `{ "artist": BrowseArtist, "albums": BrowseAlbum[] }`

---

### `browse.listAlbums`

Lists albums for the active provider.

**Params:** `{ "libraryId"?: string, "letter"?: string, "startIndex"?: number, "limit"?: number }`  
**Returns:** `{ "albums": BrowseAlbum[], "total": number }`

---

### `browse.getAlbum`

Returns one album plus tracks.

**Params:** `{ "albumId": string }`  
**Returns:** `{ "album": BrowseAlbum, "tracks": BrowseTrack[] }`

---

### `browse.listPlaylists` / `browse.getPlaylist`

Lists playlists and loads playlist tracks.

**Params:** none for `browse.listPlaylists`; `{ "playlistId": string }` for `browse.getPlaylist`  
**Returns:** `{ "playlists": BrowsePlaylist[] }` or `{ "playlist": BrowsePlaylist, "tracks": BrowseTrack[] }`

---

### `browse.listGenres` / `browse.getGenre`

Lists genres and loads tracks for a genre.

**Params:** `{ "libraryId"?: string, "startIndex"?: number, "limit"?: number }` or `{ "genreId": string, "startIndex"?: number, "limit"?: number }`  
**Returns:** `{ "genres": BrowseGenre[], "total": number }` or `{ "genre": BrowseGenre, "tracks": BrowseTrack[], "total": number }`

---

### History and Favorites Browse

| Method | Params | Returns |
|--------|--------|---------|
| `browse.listRecentlyAdded` | `{ "libraryId"?: string, "startIndex"?: number, "limit"?: number }` | `{ "albums": BrowseAlbum[], "total": number }` |
| `browse.listFrequentlyPlayed` | `{ "libraryId"?: string, "startIndex"?: number, "limit"?: number }` | `{ "tracks": BrowseTrack[], "total": number }` |
| `browse.listRecentlyPlayed` | `{ "libraryId"?: string, "startIndex"?: number, "limit"?: number }` | `{ "tracks": BrowseTrack[], "total": number }` |
| `browse.listFavorites` | `{ "libraryId"?: string, "startIndex"?: number, "limit"?: number }` | `{ "tracks": BrowseTrack[], "total": number }` |
| `browse.listFavoriteItems` | `{ "libraryId"?: string }` | `{ "artists": BrowseArtist[], "albums": BrowseAlbum[], "tracks": BrowseTrack[] }` |

Classic Subsonic returns `-32002`/unsupported capability for history modes that are not advertised by `browse.listModes`.

---

## Playlist Editing

Playlist methods require provider playlist-write capability. Cross-server basket items are rejected for playlist creation with app error `-7`.

| Method | Params | Returns |
|--------|--------|---------|
| `playlist.create` | `{ "name": string, "itemIds": string[], "items"?: Array<{ id: string, serverId?: string }> }` | `{ "playlistId": string, "skippedItemIds": string[] }` |
| `playlist.addItems` | `{ "playlistId": string, "itemIds": string[] }` | `{ "ok": true }` |
| `playlist.addTracks` | `{ "playlistId": string, "trackIds": string[] }` | `{ "ok": true }` |
| `playlist.removeTracks` | `{ "playlistId": string, "trackIds": string[] }` | `{ "ok": true }` |
| `playlist.delete` | `{ "playlistId": string }` | `{ "ok": true }` |
| `playlist.rename` | `{ "playlistId": string, "name": string }` | `{ "ok": true }` |
| `playlist.reorder` | `{ "playlistId": string, "trackIds": string[] }` | `{ "ok": true }` |

---

## Legacy Jellyfin-Compatible Browse

### `jellyfin_get_views`

Returns the user's Jellyfin media library roots.

**Params:** none  
**Returns:** `Array<JellyfinView>` where `JellyfinView = { Id, Name, Type, CollectionType? }`

---

### `jellyfin_get_items`

Returns paginated items from a parent container.

**Params:**
```json
{
  "parentId"?: string,
  "includeItemTypes"?: string,        // e.g. "MusicAlbum,Playlist,Audio"
  "startIndex"?: number,
  "limit"?: number,
  "nameStartsWith"?: string,          // single alpha char (quick-nav filter)
  "nameLessThan"?: string             // single alpha char (quick-nav '#' filter)
}
```
**Returns:** `{ "Items": JellyfinItem[], "TotalRecordCount": number, "StartIndex": number }`

---

### `jellyfin_get_item_details`

Returns full details for a single item.

**Params:** `{ "itemId": string }`  
**Returns:** `JellyfinItem` (with `MediaSources`, `UserData`, `RunTimeTicks`, etc.)

---

### `jellyfin_get_item_counts`

Returns recursive item counts and cumulative runtime for a list of items. Used to show "N tracks" in basket cards.

**Params:** `{ "itemIds": string[] }`  
**Returns:** `Array<{ "id": string, "recursiveItemCount": number, "cumulativeRunTimeTicks": number }>`

---

### `jellyfin_get_item_sizes`

Returns total file sizes (in bytes) for a list of items. Results are cached in-memory.

**Params:** `{ "itemIds": string[] }`  
**Returns:** `Array<{ "id": string, "totalSizeBytes": number }>`

---

## Manifest Operations

### `manifest_get_basket`

Returns the basket items stored in the current device's manifest.

**Params:** none  
**Returns:** `{ "basketItems": BasketItem[] }`

---

### `manifest_save_basket`

Persists basket items to the current device's manifest.

**Params:** `{ "basketItems": BasketItem[] }`  
**Returns:** `true`

---

### `manifest_get_discrepancies`

Scans the device to find discrepancies between manifest and actual files.

**Params:** none  
**Returns:**
```json
{
  "missing": Array<{ "jellyfinId": string, "name": string, "localPath": string, "album"?: string, "artist"?: string }>,
  "orphaned": Array<{ "jellyfinId": string, "name": string, "localPath": string, "album"?: string, "artist"?: string }>
}
```
**Errors:** `-32003` (no device), `-32004` (scan failed)

---

### `manifest_prune`

Removes items from the manifest (used to clean up missing files).

**Params:** `{ "itemIds": string[] }`  
**Returns:** `{ "removed": number }`

---

### `manifest_relink`

Re-links a missing manifest entry to an orphaned file's path.

**Params:** `{ "jellyfinId": string, "newLocalPath": string }`  
**Returns:** `{ "success": bool }`

---

### `manifest_clear_dirty`

Clears the manifest's dirty flag after manual repair.

**Params:** none  
**Returns:** `{ "success": true }`

---

## Sync

### `sync_get_device_status_map`

Returns all provider item IDs currently synced to the device.

**Params:** none  
**Returns:** `{ "syncedItemIds": string[] }`

---

### `sync_calculate_delta`

Calculates the sync delta between the basket and the current device manifest.

**Params:**
```json
{
  "itemIds": string[],             // manually selected item IDs (may include containers)
  "autoFill"?: {
    "enabled": bool,
    "maxBytes"?: number,
    "excludeItemIds": string[]
  }
}
```

Behavior:
- Container IDs (albums, playlists, artists) are expanded to constituent tracks
- Favorite group IDs (`favorites:artist:<id>`, `favorites:album:<id>`) expand only the favorite subset selected in the favorites browse tree
- Auto-fill items are fetched via the priority algorithm and merged
- If any item fetch fails, the whole delta is aborted (to prevent accidental deletes)

**Returns:** `SyncDelta`
```json
{
  "adds": Array<DesiredItem>,
  "deletes": Array<SyncedItem>,
  "idChanges": Array<{ "oldJellyfinId": string, "newJellyfinId": string, "oldLocalPath": string }>,
  "playlists": Array<PlaylistSyncItem>
}
```

---

### `sync_detect_changes`

Detects provider-side changes since a sync token. Used by Subsonic/OpenSubsonic-aware refresh flows and by providers that can map manifest state into change context.

**Params:** `{ "syncToken"?: string | null }`  
**Returns:**
```json
[
  {
    "id": "provider-item-id",
    "itemType": "song|album|artist|playlist|library",
    "changeType": "created|updated|deleted",
    "version": "provider-version-or-null",
    "providerAlbumId": "album-id-or-null",
    "providerSize": 3000,
    "providerContentType": "audio/flac",
    "providerSuffix": "flac"
  }
]
```

Subsonic/OpenSubsonic change metadata may be derived from album-level fallbacks when a server lacks an item-level changes feed.

---

### `sync_execute`

Starts an asynchronous sync operation. Returns immediately with an operation ID.

**Params:** `{ "delta": SyncDelta }`  
**Returns:** `{ "operationId": string }`

The sync runs as a `tokio::spawn` background task. Progress is queryable via `sync_get_operation_status`.

---

### `sync_get_operation_status`

Returns the current status of a sync operation.

**Params:** `{ "operationId": string }`  
**Returns:**
```json
{
  "id": string,
  "status": "running" | "complete" | "failed",
  "startedAt": string,                   // ISO timestamp
  "currentFile": string | null,
  "bytesCurrent": number,                // bytes transferred for current file
  "bytesTotal": number,                  // total bytes for current file
  "bytesTransferred": number,            // cumulative bytes transferred
  "totalBytes": number,                  // total bytes for entire operation
  "filesCompleted": number,
  "filesTotal": number,
  "errors": Array<{ "jellyfinId": string, "filename": string, "errorMessage": string }>,
  "warnings": string[]
}
```

---

### `sync_get_resume_state`

Returns dirty-manifest info and counts cleaned temp files. Call on device connect to detect interrupted syncs.

**Params:** none  
**Returns:**
```json
{
  "isDirty": bool,
  "pendingItemIds": string[],
  "cleanedTmpFiles": number
}
```

---

### `sync.setAutoFill`

Persists auto-fill preferences to the device manifest.

**Params:** `{ "autoFillEnabled": bool, "maxFillBytes"?: number, "autoSyncOnConnect": bool }`  
**Returns:** `true`

---

### `autoFill.setPipeline`

Persists the full configurable pipeline for a portable server id on the current device manifest.

**Params:** `{ "serverId": string, "pipeline": AutoFillPipeline }`  
**Returns:** `true`

---

## Auto-Fill

### `basket.autoFill`

Runs auto-fill and returns ranked tracks. With `serverId` and/or `pipeline`, routes through the provider-backed configurable pipeline. Without a pipeline, the legacy favorites → play count → newest path remains available for compatibility.

**Params:**
```json
{
  "deviceId"?: string,
  "serverId"?: string,
  "pipeline"?: AutoFillPipeline,
  "maxBytes"?: number,               // defaults to device free bytes
  "excludeItemIds": string[]         // IDs of manually selected items (expanded to track IDs)
}
```
**Returns:** `Array<AutoFillItem>`
```json
[
  {
    "id": string,
    "name": string,
    "album"?: string,
    "artist"?: string,
    "sizeBytes": number,
    "priorityReason": "favorite" | "playCount:N" | "new"
  }
]
```

---

## Scrobbler

### `scrobbler_get_last_result`

Returns the result of the most recent scrobble submission.

**Params:** none  
**Returns:**
```json
{
  "status": "success" | "partial" | "error" | "none",
  "message": string,
  "submitted": number,
  "skipped": number,
  "errors": number
}
```
or `{ "status": "none", "message": "No scrobble submission has been performed yet." }` if no submission has occurred.

---

## Transcoding Profiles

### `device_profiles.list`

Returns available transcoding profiles.

**Params:** none  
**Returns:**
```json
Array<{
  "id": string,
  "name": string,
  "description": string,
  "deviceProfile": object | null
}>
```

Always includes at least the built-in `"passthrough"` profile (no transcoding).

---

## Image Proxy (HTTP GET)

`GET /jellyfin/image/:id[?maxHeight=N&quality=N]`

Proxies cover art for the active provider. Jellyfin uses `GET /Items/:id/Images/Primary`; Subsonic/OpenSubsonic uses provider `cover_art_url`. Returns the image with its original content-type header. Used by the Rust-side `image_proxy` Tauri command, not directly by the WebView.
