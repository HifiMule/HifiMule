# API Contracts — HifiMule Daemon

**Generated:** 2026-05-23 | **Last Updated:** 2026-06-17 | **Scan depth:** Deep | **Protocol:** JSON-RPC 2.0 over HTTP POST to `localhost:19140`

All requests: `Content-Type: application/json`  
All successful responses: `{ "jsonrpc": "2.0", "result": <value>, "id": 1 }`  
All error responses: `{ "jsonrpc": "2.0", "error": { "code": <N>, "message": "<text>" }, "id": 1 }`

**Primary error codes:** JSON-RPC standard `-32601` method not found, `-32602` invalid params, plus app codes `-1` connection failed, `-3` storage error, `-4` not found, `-5` unsupported capability, `-6` sync in progress, `-7` cross-server playlist conflict, `-8` selected server credential unauthorized.

The daemon now exposes a provider-neutral media-server layer. Legacy `jellyfin_*` RPC names remain supported for compatibility, but active Subsonic/OpenSubsonic connections are routed through `MediaProvider` where possible.

Authenticated `daemon.health` includes a sanitized `audioRuntime` object with the
loaded FFmpeg component versions, selected shared endpoint (when opened), and
compressed/PCM high-water counters. It never contains playback request URLs,
headers, credentials, or provider response bodies.

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
# Playback session reconnect contract

Playback session state is owned by the daemon and exposed through authenticated JSON-RPC methods `playback.getSession`, `playback.listOccurrences`, `playback.applySession`, and `playback.retryRestore`. Clients reconnect by calling `playback.getSession` with `schemaVersion: 1` and replacing their local presentation from the returned authoritative snapshot. They must not replay mutations after an instance change.

Queue revisions and state sequences are canonical decimal strings. Occurrence pages are bounded (100 by default, 200 maximum). A page cursor is tied to its session and queue revision; `INVALID_CURSOR`, `SESSION_MISMATCH`, or `QUEUE_REVISION_CONFLICT` means the client must discard that cursor, refresh the snapshot, and fetch pages again. Source availability is metadata and does not remove offline queue entries.

Current selection uses `operation: { "type": "selectCurrent", "occurrenceId": "<UUID>" }`. Conflict `error.data` includes the authoritative `instanceId`, `sessionId`, `queueRevision`, `stateSequence` and `generationId`. An identical retained command returns its original result fields plus `currentMetadata` with those current values; clients must not mistake the original result revision for the latest revision.

`daemon.health` includes a small cached `playback: { restoration, persistence }` status independent of SQLite. Failed restoration remains observable through `getSession` even if the playback schema cannot be read. `retryRestore` validates the entire stored session before accepting it; it cannot replace a valid live session or run after committed shutdown.

After committed Quit, `shutdown.sessionCheckpoint` is `pending`, `failed` or `succeeded` (`notRequired` before participation starts). Pending/failed saves add a `sessionCheckpoint` blocker. A failed save reports `PLAYBACK_CHECKPOINT_FAILED` and retains ownership/admission fencing while sync cancellation and drain continue. Retry uses the authenticated `playback.retryCheckpoint` method with `{ "schemaVersion": 1, "instanceId": "<owner>", "shutdownId": "<observed shutdown>" }`. Only a completed failure may start a retry; concurrent retries join that attempt. Neither the shutdown ID nor its deadline changes. The stopping-method exception is limited to health and this scoped retry; Continue remains available only for a precommit launch-fence failure.

The playback worker is joined after successful preservation and sync drain. A join failure reports `PLAYBACK_OWNER_FAILED` separately; it does not offer a session-save retry that cannot recover worker teardown.
# Playback audio (schema v1)

`playback.applySession` accepts `operation: { type: "playTrack", source: { serverId, trackId } }`. The portable server identity is resolved by the daemon; authenticated URLs and headers are never returned. Admission atomically replaces the queue with one occurrence and returns before source preparation completes.

`playback.control` accepts `{ schemaVersion, instanceId, sessionId, commandId, expectedGenerationId, occurrenceId, action }`, where action is `pause`, `resume`, or `stop`. Stale generation/occurrence controls return a conflict with authoritative session metadata. Command IDs are bounded and idempotent; reuse with another payload is rejected.

`playback.getSession` includes an additive `playback` object with safe metadata, representation, duration, status (`idle`, `loading`, `active`, `paused`, `stopped`, `completed`, `error`) and a sanitized `{ code, retryable }` failure. It never includes provider requests or credentials.


# Explicit playback outputs (schema v1, Story 15.5)

`playback.listOutputs` accepts exactly `{ "schemaVersion": 1 }`. Its `data`
contains `instanceId`, canonical decimal `outputRevision`, up to 256 `outputs`,
`output`, and a sanitized discovery `error`. Discovery runs on one owned worker;
refreshes coalesce. A stalled enumeration is reported after five seconds and
remains owned until it returns. An incomplete inventory reports truncation.

Descriptors expose `outputId` (opaque identity digest), `displayName`, `detail`,
`backend`, `available`, `isDefault`, `identityConfidence`, and `isVirtual`.
Names and default annotations are presentation metadata. They never identify a
restored endpoint. Virtual outputs cannot certify downstream physical routing.

Linux physical sinks are selectable only when exactly one reported port matches
the active port. Multi-port or unknown physical routes have `available: false`
and `identityConfidence: "unsupported"`: pinning a sink cannot prevent its server
from switching headphone audio to a speaker port. This policy is rechecked on
the opened sink before it is uncorked. Virtual endpoints remain explicitly labeled.
`OUTPUT_DISCOVERY_PARTIAL` and `OUTPUT_ENUMERATION_TRUNCATED` report incomplete
inventories without invalidating independently resolved endpoints. Duplicate IDs
invalidate their matching descriptors; total discovery failure still gates playback.

`playback.selectOutput` accepts exactly:

```json
{
  "schemaVersion": 1,
  "instanceId": "<owner UUID>",
  "sessionId": "<session UUID>",
  "commandId": "<new UUID>",
  "expectedOutputRevision": "0",
  "expectedGenerationId": "<observed generation UUID>",
  "outputId": "<opaque output ID from listOutputs>",
  "replaceInvalidConfig": false
}
```

The response `data` is the authoritative session snapshot. Acceptance does not
promise audible success. Replays share the Apply/Control command namespace and
do not repeat persistence or audio effects. Changed payloads and stale identity,
output revision or generation conflict. Output selection never changes queue
revision, session identity or current occurrence and works with an empty queue.

Every snapshot adds `output: { revision, selected, pending, active, status, error }`.
`selected` projects the committed `playback.json` preference; `pending` represents
an accepted save/switch; `active` describes the opened stream. Available output
without an active stream is valid. Status is `unselected`, `available`,
`switching`, `unavailable` or `error`. UI must render these fields rather than
assuming the last clicked choice is active.

On first launch only, missing configuration selects and persists the one available
concrete system-default endpoint after discovery completes. An explicit null
configuration remains unselected. Invalid or future configuration is retained and
requires an explicit reset action followed by selection with
`replaceInvalidConfig: true`; the original is archived before atomic replacement.
Startup never opens a stream or resumes. Play retains its queue selection when no
output is selected; Resume requires an available concrete selected output.

Windows/macOS use exact CPAL 0.18.2 endpoint identity; CoreAudio opens enumerated
concrete devices rather than default-following units. Linux uses a named Pulse
sink with movement prohibited and bounded server buffering. Reconnection and
system-default changes do not resume playback or change the saved preference.
Failures report sanitized output codes independently of the queue and transport.
Installed platform verification remains required; source checks are not evidence
of physical routing, latency or shared-mode behavior.

# Native playback controls (Story 15.6)

Native Play, Pause, Toggle and Stop are private daemon ingress, not public RPC.
They enter the same bounded serialized playback owner as `playback.control`.
Toggle and current occurrence resolve at owner execution; Resume uses the same
provider resolution, selected-output policy, preparation deadline, audio effect,
generation fencing and failure publication as RPC Resume. Unsupported actions
(Next, Previous, Seek, SetPosition, OpenUri, Raise and Quit) are not advertised
and are rejected without mutation.

The daemon owns one SMTC, MPRemoteCommandCenter or MPRIS registration for its
lifetime. UI close/reopen does not register controls. Now-playing metadata is a
replacement projection from authoritative playback state and never triggers a
provider listening report. Explicit Quit disables commands, clears metadata and
detaches registration before the native owner is dropped.
## Playback seek contract (schema v1, Story 15.7)

`playback.seek` accepts the strict playback identity envelope (`schemaVersion`,
`instanceId`, `sessionId`, `commandId`, `expectedGenerationId`, `occurrenceId`)
plus an absolute integer `positionMs`. The value must be a nonnegative
JavaScript-safe integer and no greater than the current positive, verified
duration. Unknown fields, fractions, negative values, values beyond duration,
and stale identities are rejected before transport changes. Zero is valid.
Exactly duration commits the terminal cursor, pauses and silences the current
occurrence, and never repeats or advances it.

Admission and completion are distinct. An admitted response contains
`playback.pendingSeek` with an operation ID, requested target and prior
committed cursor. `positionMs` remains the last actual committed cursor. A
matching generation- and control-epoch-fenced decoder result clears the pending
operation and publishes `playback.seekOutcome.status = "succeeded"` with its
actual landed cursor. Failure retains the previous committed cursor, pauses the
occurrence and publishes a sanitized retryable `SEEK_FAILED`. Superseded work
cannot commit or overwrite the later operation. Seeking never changes the
session, queue revision, queue order, occurrence or source.

The same bounded command-ID namespace is shared by apply, control, output and
seek mutations. An exact replay returns the authoritative prior admission and
does not repeat audio work; reuse with different payload returns
`COMMAND_ID_REUSED`. Successful committed cursors use the existing transactional
checkpoint path. Pending, failed and superseded targets are never persisted.

### Initial candidate capability matrix

| Provider | Representation | Mechanism | Timestamp origin | Decoded landing tolerance | Presentation allowance | Status |
| --- | --- | --- | --- | ---: | --- | --- |
| Jellyfin | Original WAV (`pcm_s16le`, `pcm_s24le`, `pcm_s32le`), M4A (`aac`, `alac`), Ogg (`opus`), MP3 or FLAC verified after FFmpeg opens the stream; authenticated validated byte ranges | FFmpeg post-open media-time seek, decoder/resampler reset and bounded pre-roll trim | Audio stream start time and time base, reconciled to output frame zero | ≤ 50 ms | CPAL callback accounting ≈25 ms; Pulse played-frame accounting ≈5 ms; owner sample 250 ms + snapshot 500 ms + repaint 100 ms | Runtime enabled after per-track verification; installed acceptance remains pending by target |
| Jellyfin | FLAC, MP3, AAC, ALAC, Opus, Vorbis, AIFF, WMA or transcoded/changed representation | None qualified | — | — | — | Disabled; ordinary playback retained |
| Subsonic/OpenSubsonic | Raw stream, any format | None qualified | — | — | — | Disabled; ordinary playback retained |

`range_supported`, filename extension, `Accept-Ranges`, or one successful HTTP
206 response never enables seeking by itself. The provider must identify the
qualified original candidate and FFmpeg must verify the actual WAV/PCM stream.
Runtime format recognition is necessary but does not certify a platform. The
implemented Jellyfin PCM-WAV path is exposed only after FFmpeg verifies the
opened stream and its media duration. Installed observations still determine
which target rows satisfy release acceptance; runtime success does not by itself
promote a Windows, Linux, or macOS row to qualified evidence.

For PCM WAV candidates, FFmpeg stream duration/time base supplies the millisecond
end cursor. Whole-second provider metadata is reconciled when the difference is
less than 1000 ms; a difference of 1000 ms or more prevents qualification and
fails an attempted media seek. Exact-end admission uses the validated cursor,
not the provider's rounded seconds. Restoring that cursor and explicitly resuming
restarts at zero, including progress admission and backend position accounting.
Zero/missing duration, changed validators, ignored/wrong ranges and truncated
responses disable or fail seeking explicitly.

Native relative commands retain signed microseconds until checked conversion.
MPRIS negative overshoot normalizes to zero and relative overshoot to the
single-track terminal cursor; stale `SetPosition` track IDs and out-of-range
absolute positions are ignored. Windows uses checked signed `TimeSpan`
conversion and a 10-second FastForward/Rewind step. macOS accepts only finite,
nonnegative public position-change values. All normalized commands enter the
same owner admission and effect path as UI seeking. A native `Seeked(i64)` is a
success-driven discontinuity acknowledgement, never an admission receipt or a
routine progress signal. Publication tracks the committed operation identity,
so distinct seeks to the same position still produce distinct acknowledgements.

Transport race outcomes are fixed: a later admitted seek wins; Pause changes
the post-commit intent to paused; Stop, track replacement, output switch/loss
and Quit invalidate preparation; failure keeps the last committed cursor and
requires an explicit retry. Compressed storage remains capped at 8 MiB including
network and scratch allowances, PCM at 1 MiB/500 ms, and the existing 60-second
total preparation deadline is not restarted by a seek stage.
