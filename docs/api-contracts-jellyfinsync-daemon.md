# API Contracts — HifiMule Daemon

**Generated:** 2026-05-07 | **Scan depth:** Exhaustive | **Protocol:** JSON-RPC 2.0 over HTTP POST to `localhost:19140`

All requests: `Content-Type: application/json`  
All successful responses: `{ "jsonrpc": "2.0", "result": <value>, "id": 1 }`  
All error responses: `{ "jsonrpc": "2.0", "error": { "code": <N>, "message": "<text>" }, "id": 1 }`

**Error codes:** `-32001` invalid credentials, `-32002` invalid params, `-32003` connection/device failed, `-32004` storage error, `-32005` internal error

---

## Auth & Credentials

### `test_connection`

Tests Jellyfin connectivity.

**Params:** `{ url: string, token: string }`  
**Returns:** `{ status: "ok" }` or error `-32003`

---

### `login`

Authenticates with Jellyfin and persists credentials.

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
**Returns:** `{ url: string, token: string, userId: string | null }` or `null` if not configured  
**Errors:** `-32004` (storage error, excluding expected "not configured" states which return `null`)

---

## Daemon State

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
  "selectedDevicePath": string | null
}
```

Note: `serverConnected` is cached for 5 seconds to avoid excessive Jellyfin health checks.

---

## Device Setup

### `device_initialize`

Initializes a newly detected device that has no `.hifimule.json` manifest.

**Params:**
```json
{
  "folderPath": string,              // managed folder name ("Music") or "" for device root
  "profileId": string,               // Jellyfin user ID (used as device profile ID)
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
    "version": number,
    "managedPaths": string[],
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

### `device.select`

Switches the "current device" to the given path (multi-device hub).

**Params:** `{ "path": string }`  
**Returns:** `true`

---

### `set_device_profile`

Updates device's Jellyfin profile ID and optional sync rules.

**Params:** `{ "deviceId": string, "profileId": string, "syncRules"?: string }`  
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

## Jellyfin Browse

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

Returns all Jellyfin item IDs currently synced to the device.

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
  "errors": Array<{ "jellyfinId": string, "filename": string, "errorMessage": string }>
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

## Auto-Fill

### `basket.autoFill`

Runs the auto-fill priority algorithm and returns ranked tracks.

**Params:**
```json
{
  "deviceId"?: string,
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

### `scrobbler.getLastResult`

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
  "jellyfinProfileId"?: string
}>
```

Always includes at least the built-in `"passthrough"` profile (no transcoding).

---

## Image Proxy (HTTP GET)

`GET /jellyfin/image/:id[?maxHeight=N&quality=N]`

Proxies the request to Jellyfin's `GET /Items/:id/Images/Primary` endpoint. Returns the image with its original content-type header. Used by the Rust-side `image_proxy` Tauri command, not directly by the WebView.
