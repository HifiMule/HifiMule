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

`playback.control` accepts `{ schemaVersion, instanceId, sessionId, commandId, expectedGenerationId, occurrenceId, action }`, where action is `pause`, `resume`, `stop`, `next`, or `retry`. Stale generation/occurrence controls return a conflict with authoritative session metadata. Command IDs are bounded and idempotent; reuse with another payload is rejected. `next` records an explicit local skip and selects the indexed successor while preserving playing/paused intent; it returns `NEXT_UNAVAILABLE` without mutation when no successor exists. `retry` retains the occurrence and committed cursor and starts a newly fenced preparation attempt.

`playback.getSession` includes an additive `playback` object with safe metadata, representation, duration, status (`idle`, `loading`, `active`, `paused`, `stopped`, `completed`, `error`) and a sanitized `{ code, retryable }` failure. It never includes provider requests or credentials.

## Prepared album continuity (internal contract, Story 15.9)

Continuity keeps one native output epoch open for a selected endpoint while decode
ownership moves between adjacent album occurrences. A private handoff token binds
the daemon instance, session, predecessor and successor occurrence IDs, queue
revision, control epoch, preparation generation, and output epoch. Source IDs do
not identify a handoff because an album may contain the same source more than
once. Preparation does not change current metadata, position, disposition, or the
durable cursor.

Only one successor may be retained. Each active or successor slot is limited to
8 MiB compressed input and `min(500 ms, 1 MiB)` frame-aligned PCM; aggregate
limits are 16 MiB compressed input, 2 MiB PCM, and two source/decoder slots.
Metadata and native backend buffers are measured separately. A successor becomes
ready after an authorized decoded head reaches the normal 100 ms target, or clean
EOF leaves at least one complete frame. Preparation retains the existing 60 s
deadline. A zero-frame decode is a failure.

The output format is fixed for the output epoch. Each occurrence is decoded and
resampled independently into that rate and mono/stereo layout, including decoder
and resampler drain. The boundary is the exact output-frame offset after the
predecessor's final valid frame. PCM WAV, FLAC, ALAC/M4A, MP3 with validated
delay/padding metadata and a known source byte length, AAC/M4A with validated container timing, and Opus/Ogg
pre-skip/end trimming form the initial decoder matrix. FFmpeg applies their
validated padding metadata; HifiMule does not add a second trim. Missing or
ambiguous padding metadata never authorizes inferred trimming. Legacy Apple AAC
without declared start padding remains untrimmed and is not continuity-certified:
`iTunNORM` and a filler packet do not establish an exact encoder delay. Raw AAC,
AIFF, Vorbis, and WMA remain ordinary-playback formats until separately qualified.
Unknown-length MP3 streams remain ordinary playback: the controlled FFmpeg 9
nonseekable demuxer does not reliably apply terminal Xing padding without byte
size. HifiMule neither guesses that size nor trims to provider duration.

Owner advancement occurs only after the backend reports that the boundary was
presented. The acknowledgment includes the token, boundary frame, and played
frame so owner latency can set the successor offset without replay. Duplicate or
stale acknowledgments are inert; failure for an attempt dominates delayed
completion. Adoption atomically records predecessor completion and the successor
cursor while preserving queue revision and advancing state sequence, and it does
not dispatch a second audio start.

Pause closes the PCM consumption gate synchronously and requests native pause
immediately. The acknowledged native cancellation point decides a race with a
submitted boundary: if the boundary was already presented, the owner adopts it
before applying Pause to the audible successor; otherwise the successor remains
unadopted and its unpresented PCM is retained or replayed exactly once. Stop,
Next, seek, replacement, output loss, and Quit close the gate and invalidate
unpresented preparation. If a backend cannot establish a coherent cancellation
cutoff, it retires the stream and resumes from the last coherent committed
occurrence. It never guesses from callback consumption or provider duration.

Implementation qualification: WASAPI uses acknowledged `Stop`, exact stopped
padding, `Reset`, and a fixed-capacity submitted-tail ledger; Resume replays the
discarded suffix once before new PCM, preserving any still-queued replay across
repeated pauses. Clock snapshots retain a cumulative origin across native resets.
Pulse waits for server-side cork acknowledgment and refreshed played-frame timing
before the owner reconciles a boundary. A failed acknowledgment retires the pipeline. CoreAudio currently uses the synchronous consumption gate
only: its callback timestamps estimate presentation but do not prove an exact
stop cursor, so HifiMule does not use them to flush/replay native buffers.

Persistence failure after presentation closes the gate, retires or corks the
stream, retains one frozen terminal recovery transaction, and exposes
`PERSISTENCE_FAILED`. Recovery retries that exact transaction without audio and
leaves the adopted occurrence paused. Sound already presented is not described
as rolled back.

## Ordered album playback (schema v1, Story 15.8)

`playback.playAlbum` accepts exactly `{ schemaVersion, instanceId, sessionId,
commandId, expectedQueueRevision, expectedGenerationId, source: { serverId,
albumId } }`. Source identities are nonempty portable provider identities bounded
to 1024 UTF-8 bytes. One resolution may be pending per owner and has a 60-second
deadline. The provider album must contain 1–10,000 valid track occurrences;
`ALBUM_EMPTY`, `ALBUM_TOO_LARGE`, `ALBUM_INVALID`, provider failure, timeout,
cancellation or stale admission leaves the previous queue untouched. Success is
one atomic replacement, including albums larger than the ordinary 200-item batch.

Album admission is reserved and committed by the serialized playback owner.
The reservation captures instance/session, queue revision, generation and control
epoch; Pause, Stop, seek, Next, output changes, replacement and shutdown supersede
it. The deadline covers provider acquisition (including its lock/connection) and
album enumeration. Dropping the caller cancels the reservation and releases its
lifecycle guard. Concurrent requests, including an identical pending request,
return `PLAYBACK_BUSY`; reusing the pending command ID with a changed payload
returns `COMMAND_ID_REUSED`.

Successful album receipts are retained for at most 10 minutes and 1024 commands.
An identical replay is recognized before stale revision checks and provider work,
returns the authoritative current snapshot, and never starts audio or remints
occurrences. Album IDs share the apply/control/seek/output command-ID namespace.
The resolved `PlayAlbum { sources }` operation is internal and cannot be submitted
through public `playback.applySession`; ordinary insert limits remain unchanged.

Provider order is captured before sorting. Positive disc numbers are valid;
missing, zero or negative disc numbers use effective disc 1. Within each disc,
positive track numbers precede missing/zero/negative numbers. The full key is
`(effectiveDisc, missingTrack, positiveTrackOrZero, providerOrdinal)`. Titles and
IDs never participate, and repeated source IDs remain distinct occurrences.

Occurrences persist a local disposition (`naturalCompletion`, `explicitSkip`, or
`technicalFailure`). Successor lookup uses indexed `(session_id, ordinal)` data,
never a snapshot page. Natural presentation completion is consumed once per
occurrence/generation after output drain. With a successor it commits that current
occurrence at zero; without one it retains the final occurrence and actual cursor
as Paused/Completed. Decoder EOF, duration metadata and exact-end seek are not
completion signals.

Committed transitions publish one bounded owner effect to the shared
`PlaybackCommandService`, outside owner/database locks and independently of UI
polling or native registration. Effects are fenced by instance, session,
occurrence, generation and control epoch at dequeue, after provider resolution
and before activation. Stop, replacement, seek, output loss, shutdown and newer
generations supersede old work. Paused Next is silent and output loss never routes
to another endpoint. Duplicate/late EOF, progress, metadata and preparation cannot
advance or revive the old occurrence.

Play album invents no outcome; natural completion records `naturalCompletion`;
UI/native Next records `explicitSkip`; source/decode/load failure records
`technicalFailure`, pauses and never skips; Retry targets the same occurrence and
cursor; Stop records no outcome; exact-end seek does not advance. Failed atomic
writes retain the prior coherent state. Schema v2 adds outcomes, and restoration
returns paused with the complete queue and local outcomes intact.

Each new Retry/replay attempt resets its current occurrence disposition to pending
(SQL NULL). The last sanitized failure code is retained until successful recovery.
Terminal disposition, failure code, current occurrence, cursor and checkpoint
sequence commit together. A failed terminal write stops audio and retains one
identity-fenced recovery plan; Next and seek cannot bypass it. Storage Retry
commits that plan once and leaves the result paused (or Completed at final EOF),
without preparing audio. Stop or a replacement discards the plan. Ordinary
source/decode Retry still reopens the same occurrence at its committed cursor.
Next from Stopped retains Stopped status on the successor. Playback persistence
schema v4 migrates the older v3 private album contexts described below. Existing v1/v2
rows migrate with no album context and therefore use exact unity gain.

## Frozen album loudness policy (internal contract, Story 15.10)

Album admission may consume one OpenSubsonic `Child.replayGain` pair:
`albumGain` is a finite dB adjustment in the conventional 89 dB ReplayGain
reference, and `albumPeak` is a positive decoded sample peak where 1.0 is full
scale. The supported bounds are `albumGain ∈ [-60, +30]` and
`albumPeak ∈ (0, 64]`. The complete provider response must declare the requested
album membership and track count; every occurrence must contain a usable pair.
Across occurrences, gain spread must be at most 0.01 dB and peak spread at most
`max(1e-6, 1e-4 × maximumPeak)`. Resolution uses the minimum accepted gain and
maximum accepted peak, so provider order cannot change the result. Inclusive
boundaries allow only f64 subtraction roundoff (four machine epsilons scaled
to the compared magnitudes). The 10,000-occurrence cap is checked before
normalization, which computes extrema with constant additional space.
The returned album ID must equal the requested album ID before non-unity
qualification.

The common scalar is resolved once in f64 with zero preamp:

```text
ceiling = 10^(-1 / 20)
requested = 10^(albumGain / 20)
effective = min(requested, ceiling / albumPeak)
```

The runtime f32 value is rounded downward when needed so representation rounding
cannot exceed that declared sample-reference ceiling. One scalar applies to all
original album occurrences. Track gain, track peak, fallback gain, embedded
ReplayGain, R128, Sound Check and dynamic limiting are not applied. Missing,
partial, malformed, non-finite, inconsistent or unsupported evidence freezes
exact unity; metadata failure does not reject otherwise playable media and unity
makes no clipping-safety claim.

The v1 qualification is OpenSubsonic metadata for original PCM-in-WAV, FLAC,
AAC/ALAC-in-M4A and MP3. Missing or contradictory suffix/content type, Opus,
Vorbis, WMA, an alternative/transcoded representation, and any later codec or
container contradiction keep unity or fail preparation before non-unity audio.
Each non-unity member retains its canonical admission suffix by occurrence
ordinal. Fresh initial, seek, retry and successor descriptions must match it;
matching a newly reported suffix against itself is insufficient. Preparation
contradictions report retryable `SOURCE_UNAVAILABLE`, preserving the policy and
allowing explicit Retry. FFmpeg confirms the opened container/codec before
adjusted PCM is queued. The
peak claim concerns the provider's declared decoded sample domain. Resampling,
downmix, inter-sample peaks, OS processing, interface volume and analog output
remain outside that claim.

The private persisted context contains policy version, portable server and album
identity, original occurrence count, membership digest, ordered canonical
member suffixes for non-unity policies, selected gain/peak bits, scalar bits,
provenance reason and fallback reason. Queue and context commit in one SQLite
transaction. Original members retain the policy across seek, Retry, Next,
prepared handoff, Pause/Resume, output recreation and paused restart. Appended
occurrences use unity; successful Clear, ReplaceQueue, PlayTrack or PlayAlbum
replaces the context, while a failed replacement preserves the prior queue and
policy. Corrupt or future policy records fail restoration rather than being
reinterpreted as unity. The v3→v4 transaction fills a missing legacy membership digest from the validated
ordered original occurrence prefix; it never replaces an existing digest. It
preserves queue, cursor, outcomes and scalar. Legacy non-unity records without
format evidence receive an explicit `unverified` format marker per member.
Those sessions restore paused and permit starting a new album, but adjusted
playback of an unverified member fails preparation until a new album admission
provides the evidence. No format or replacement gain is guessed. Unity legacy
records need no format evidence. Missing fields in v4 records remain restoration
errors; this compatibility conversion runs only when upgrading v3.

The worker multiplies packed f32 output once after swresample and before each
bounded PCM insertion, including resampler drain. Exact unity bypasses the
multiplication bit-for-bit. Native callbacks, submitted-tail replay and boundary
reconciliation do not perform gain work, allocate, fetch metadata, touch SQLite,
or acquire sync locks. Replay therefore cannot square the scalar, and gain does
not change frame counts, padding decisions or album boundaries.

Native Next resolves owner state at execution. `canGoNext` is true only for a real
successor while lifecycle state permits it; delivery while false has no effect. A
qualified MPRIS relative seek strictly beyond duration uses the same Next path
when a successor exists. Equality stays an exact-end seek, and an unsupported
seek capability cannot bypass admission.


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

Native Play, Pause, Toggle, Stop and capability-gated Next are private daemon ingress, not public RPC.
They enter the same bounded serialized playback owner as `playback.control`.
Toggle and current occurrence resolve at owner execution; Resume uses the same
provider resolution, selected-output policy, preparation deadline, audio effect,
generation fencing and failure publication as RPC Resume. Unsupported actions
(Previous, Seek, SetPosition, OpenUri, Raise and Quit) are not advertised
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
| Jellyfin | Vorbis, AIFF, WMA, unsupported codec/container pairing or transcoded/changed representation | None qualified | — | — | — | Disabled; ordinary playback retained |
| Navidrome | Original raw WAV, M4A AAC/ALAC, Ogg Opus, MP3 or FLAC after an authenticated ping reports `type=navidrome`; authenticated validated byte ranges | Same FFmpeg mechanism and bounded pre-roll as Jellyfin | Audio stream start time and time base, reconciled to output frame zero | ≤ 50 ms | Same platform accounting as Jellyfin | Runtime enabled after per-track and server verification; installed acceptance remains pending |
| Other Subsonic/OpenSubsonic | Raw stream, any format | None qualified | — | — | — | Disabled; ordinary playback retained |

`range_supported`, filename extension, `Accept-Ranges`, or one successful HTTP
206 response never enables seeking by itself. The provider must identify the
qualified original candidate and FFmpeg must verify the actual container/codec stream.
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
