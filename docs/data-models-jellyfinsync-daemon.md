# Data Models — JellyfinSync Daemon

**Generated:** 2026-05-07 | **Scan depth:** Exhaustive

---

## DeviceManifest

Stored at `<device-root>/.jellyfinsync.json`. Source of truth for all sync state.

```rust
pub struct DeviceManifest {
    pub device_id: String,                         // UUID v4, generated once on initialize
    pub version: u32,                              // manifest schema version
    pub name: Option<String>,                      // human-readable name (max 40 chars)
    pub icon: Option<String>,                      // icon key for UI display
    pub synced_items: Vec<SyncedItem>,             // files confirmed present on device
    pub basket_items: Vec<BasketItem>,             // user's curation for next sync
    pub managed_paths: Vec<String>,                // relative paths owned by JellyfinSync
    pub dirty: bool,                               // true if sync was interrupted mid-operation
    pub pending_item_ids: Vec<String>,             // Jellyfin IDs being processed when dirty was set
    pub auto_fill: AutoFillPrefs,
    pub auto_sync_on_connect: bool,
    pub transcoding_profile_id: Option<String>,   // null = passthrough (no transcoding)
}
```

**Valid icon values:** `"usb-drive"`, `"phone-fill"`, `"watch"`, `"sd-card"`, `"headphones"`, `"music-note-list"`

---

## SyncedItem

Represents a single file that has been successfully synced to the device.

```rust
pub struct SyncedItem {
    pub jellyfin_id: String,       // Jellyfin item ID
    pub name: String,              // track title
    pub album: Option<String>,
    pub artist: Option<String>,
    pub local_path: String,        // relative path on device from device root
    pub etag: Option<String>,      // Jellyfin etag for change detection
}
```

`local_path` is relative to the device root (e.g., `"Music/Artist/Album/01 - Track.mp3"`).

---

## BasketItem

An item in the user's current sync selection (stored in the manifest, also held in UI state).

```rust
pub struct BasketItem {
    pub id: String,                    // Jellyfin item ID (or "__auto_fill_slot__" for the virtual slot)
    pub name: String,
    pub item_type: String,             // "MusicAlbum", "Playlist", "MusicArtist", "Audio", "AutoFillSlot"
    pub artist: Option<String>,
    pub child_count: u32,              // recursive track count (0 for Audio items)
    pub size_ticks: i64,               // cumulativeRunTimeTicks (used for duration display)
    pub size_bytes: u64,               // total file size in bytes
    pub auto_filled: Option<bool>,     // true for auto-fill algorithm items
    pub priority_reason: Option<String>, // "favorite" | "playCount:N" | "new"
}
```

---

## AutoFillPrefs

Auto-fill configuration embedded in `DeviceManifest`.

```rust
pub struct AutoFillPrefs {
    pub enabled: bool,
    pub max_bytes: Option<u64>,    // capacity budget; None = use all free space
}
```

---

## DeviceClass

Discriminator for device protocol type.

```rust
pub enum DeviceClass {
    Msc,    // Mass Storage Class (USB filesystem)
    Mtp,    // Media Transfer Protocol
}
```

---

## DesiredItem

Input to `calculate_delta` — represents an item the user wants on the device.

```rust
pub struct DesiredItem {
    pub jellyfin_id: String,
    pub name: String,
    pub album: Option<String>,
    pub artist: Option<String>,
    pub size_bytes: u64,
    pub etag: Option<String>,
}
```

---

## SyncDelta

Output of `calculate_delta`. Describes what needs to change on the device.

```rust
pub struct SyncDelta {
    pub adds: Vec<DesiredItem>,
    pub deletes: Vec<SyncedItem>,
    pub id_changes: Vec<IdChangeItem>,
    pub playlists: Vec<PlaylistSyncItem>,
}

pub struct IdChangeItem {
    pub old_jellyfin_id: String,
    pub new_jellyfin_id: String,
    pub old_local_path: String,
    // Metadata for constructing the new download path:
    pub name: String,
    pub album: Option<String>,
    pub artist: Option<String>,
    pub size_bytes: u64,
    pub etag: Option<String>,
}
```

---

## PlaylistSyncItem

A Jellyfin playlist that should be written as a `.m3u` file on the device.

```rust
pub struct PlaylistSyncItem {
    pub jellyfin_id: String,
    pub name: String,
    pub tracks: Vec<PlaylistTrackInfo>,
}

pub struct PlaylistTrackInfo {
    pub jellyfin_id: String,
    pub artist: Option<String>,
    pub run_time_seconds: i64,    // -1 if unknown
}
```

---

## SyncOperation

Tracks progress of an in-flight or completed sync.

```rust
pub struct SyncOperation {
    pub id: String,                     // UUID v4
    pub status: SyncStatus,
    pub started_at: String,             // ISO 8601 timestamp
    pub current_file: Option<String>,
    pub bytes_current: u64,             // bytes transferred for current file
    pub bytes_total: u64,               // size of current file
    pub bytes_transferred: u64,         // cumulative bytes for entire operation
    pub total_bytes: u64,               // total bytes for entire operation
    pub files_completed: u32,
    pub files_total: u32,
    pub errors: Vec<SyncFileError>,
}

pub enum SyncStatus {
    Running,
    Complete,
    Failed,
}

pub struct SyncFileError {
    pub jellyfin_id: String,
    pub filename: String,
    pub error_message: String,
}
```

---

## AutoFillItem

Output of the auto-fill algorithm.

```rust
pub struct AutoFillItem {
    pub id: String,
    pub name: String,
    pub album: Option<String>,
    pub artist: Option<String>,
    pub size_bytes: u64,
    pub priority_reason: String,       // "favorite" | "playCount:N" | "new"
}
```

---

## JellyfinItem

Represents a Jellyfin library item. Used for both API responses and delta computation.

```rust
pub struct JellyfinItem {
    pub id: String,
    pub name: String,
    pub item_type: String,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub artists: Option<Vec<String>>,
    pub index_number: Option<u32>,
    pub container: Option<String>,             // audio container (mp3, flac, etc.)
    pub production_year: Option<u32>,
    pub recursive_item_count: Option<u32>,
    pub cumulative_run_time_ticks: Option<i64>,
    pub run_time_ticks: Option<i64>,
    pub media_sources: Option<Vec<MediaSource>>,
    pub etag: Option<String>,
    pub user_data: Option<JellyfinUserData>,
    pub date_created: Option<String>,
}

pub struct MediaSource {
    pub size: Option<i64>,                     // file size in bytes; -1 = unknown
    pub container: Option<String>,
}

pub struct JellyfinUserData {
    pub is_favorite: bool,
    pub play_count: u32,
}
```

---

## DeviceMapping (SQLite `devices` table)

Per-device settings persisted in SQLite.

```rust
pub struct DeviceMapping {
    pub device_id: String,                         // PRIMARY KEY
    pub device_profile_id: Option<String>,         // Jellyfin user ID
    pub auto_sync_on_connect: bool,
    pub transcoding_profile_id: Option<String>,
    pub sync_rules: Option<String>,                // future use
}
```

---

## DeviceProfileEntry (transcoding profiles)

Loaded from `device-profiles.json` in the app data dir.

```rust
pub struct DeviceProfileEntry {
    pub id: String,
    pub name: String,
    pub description: String,
    pub jellyfin_profile_id: Option<String>,       // links to Jellyfin's device profile
}
```

The `"passthrough"` entry (id `"passthrough"`) means no transcoding — stream the original file directly.

---

## StorageInfo

Device storage statistics.

```rust
pub struct StorageInfo {
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub used_bytes: u64,
    pub device_path: String,
}
```

---

## FileEntry (`device_io.rs`)

Result of `DeviceIO::list_files()`.

```rust
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
}
```

---

## ScrobblerResult

Result of a scrobble submission, stored in `AppState.last_scrobbler_result`.

```rust
pub struct ScrobblerResult {
    pub status: String,         // "success" | "partial" | "error" | "none"
    pub message: String,
    pub submitted: u32,
    pub skipped: u32,
    pub errors: u32,
}
```

---

## Credential Storage

Credentials are split across two locations:

| Data | Location |
|------|----------|
| Server URL + User ID | `<AppData>/JellyfinSync/config.json` as `{ "url": "...", "user_id": "..." }` |
| Access Token | OS keyring, service name `"jellyfinsync"`, username `"token"` |
