# Data Models — HifiMule Daemon

**Generated:** 2026-05-23 | **Last Updated:** 2026-06-17 | **Scan depth:** Deep

---

## DeviceManifest

Stored at `<device-root>/.hifimule.json`. Source of truth for all sync state.

```rust
pub struct DeviceManifest {
    pub device_id: String,                         // UUID v4, generated once on initialize
    pub version: String,                           // manifest schema version
    pub name: Option<String>,                      // human-readable name (max 40 chars)
    pub icon: Option<String>,                      // icon key for UI display
    pub synced_items: Vec<SyncedItem>,             // files confirmed present on device
    pub basket_items: Vec<BasketItem>,             // user's curation for next sync
    pub managed_paths: Vec<String>,                // relative paths owned by HifiMule
    pub playlist_path: Option<String>,             // null = inherit first managed music path
    pub dirty: bool,                               // true if sync was interrupted mid-operation
    pub pending_item_ids: Vec<String>,             // Jellyfin IDs being processed when dirty was set
    pub auto_fill: AutoFillConfig,                    // legacy block or per-server pipeline map
    pub auto_sync_on_connect: bool,
    pub transcoding_profile_id: Option<String>,   // null = passthrough (no transcoding)
    pub last_synced_transcoding_profile_id: Option<String>,
    pub transcoding_profile_dirty: bool,          // true = rewrite matching tracks next sync
    pub playlists: Vec<PlaylistManifestEntry>,
    pub storage_id: Option<String>,                // MTP storage object ID cache
    pub folder_ids: HashMap<String, u32>,          // libmtp folder object ID cache
}
```

**Valid icon values:** `"usb-drive"`, `"phone-fill"`, `"watch"`, `"sd-card"`, `"headphones"`, `"music-note-list"`

---

## SyncedItem

Represents a single file that has been successfully synced to the device.

```rust
pub struct SyncedItem {
    pub jellyfin_id: String,       // provider item ID; serialized in .hifimule.json as providerItemId
    pub name: String,              // track title
    pub album: Option<String>,
    pub artist: Option<String>,
    pub local_path: String,        // relative path on device from device root
    pub size_bytes: u64,
    pub synced_at: String,
    pub original_name: Option<String>,
    pub etag: Option<String>,      // provider version/etag for change detection
    pub provider_album_id: Option<String>,
    pub provider_content_type: Option<String>,
    pub provider_suffix: Option<String>,
}
```

`local_path` is relative to the device root (e.g., `"Music/Artist/Album/01 - Track.mp3"`).
The synced-item ID is written as `providerItemId` in `.hifimule.json`. The Rust field is still named `jellyfin_id` for compatibility, but its semantic meaning is now "active provider item ID."

---

## BasketItem

An item in the user's current sync selection (stored in the manifest, also held in UI state).

```rust
pub struct BasketItem {
    pub id: String,                    // provider item ID (or "__auto_fill_slot__" for the virtual slot)
    pub name: String,
    pub item_type: String,             // "MusicAlbum", "Playlist", "MusicArtist", "MusicGenre", "Audio", "FavoriteArtist", "FavoriteAlbum", "AutoFillSlot"
    pub server_id: Option<String>,
    pub artist: Option<String>,
    pub child_count: u32,              // recursive track count (0 for Audio items)
    pub size_ticks: i64,               // cumulativeRunTimeTicks (used for duration display)
    pub size_bytes: u64,               // total file size in bytes
}
```

---

## AutoFillConfig and AutoFillPipeline

Auto-fill configuration embedded in `DeviceManifest`. New manifests store a map keyed by portable `server_id`; old manifests may still contain the legacy `{ enabled, maxBytes }` block and are migrated when a selected server id is available.

```rust
pub struct AutoFillPrefs {
    pub enabled: bool,
    pub max_bytes: Option<u64>,    // capacity budget; None = use all free space
}

pub struct AutoFillConfig {
    pub pipelines: HashMap<String, AutoFillPipeline>,
    pub legacy: Option<AutoFillPrefs>,
}

pub struct AutoFillPipeline {
    pub enabled: bool,
    pub filter: FilterStage,
    pub sources: Vec<SourceEntry>,
    pub unit: Unit,
    pub ordering: Vec<OrderingKey>,
    pub memory: MemoryStage,
    pub budget: BudgetStage,
    pub fallback: Vec<SourceEntry>,
    pub quality: QualityStage,
    pub rarity: RarityStage,
    pub pity: PityStage,
    pub context: ContextStage,
    pub promotion: PromotionStage,
}
```

Pipeline config is portable manifest data. Runtime history for cooldowns, rotation, and pity timers is machine-local SQLite data.

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
    pub provider_album_id: Option<String>,
    pub provider_content_type: Option<String>,
    pub provider_suffix: Option<String>,
    pub original_bitrate: Option<u32>,
    pub original_container: Option<String>,
    pub track_number: Option<u32>,
    pub server_id: Option<String>,       // portable server id for routing
}
```

---

## SyncDelta

Output of `calculate_delta`. Describes what needs to change on the device.

```rust
pub struct SyncDelta {
    pub adds: Vec<SyncAddItem>,
    pub deletes: Vec<SyncDeleteItem>,
    pub id_changes: Vec<SyncIdChangeItem>,
    pub unchanged: usize,
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
    pub provider_album_id: Option<String>,
    pub provider_content_type: Option<String>,
    pub provider_suffix: Option<String>,
    pub original_name: Option<String>,
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
    pub warnings: Vec<String>,
}

pub enum SyncStatus {
    Running,
    Complete,
    Failed,
    Cancelled,
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
    pub priority_reason: String,
    pub provider_album_id: Option<String>,
    pub provider_content_type: Option<String>,
    pub provider_suffix: Option<String>,
    pub tier: Option<String>,
    pub max_bitrate_override_kbps: Option<u32>,
}
```

---

## Provider-Domain Models

The provider layer maps Jellyfin, Subsonic, Navidrome, and OpenSubsonic DTOs into a common domain model.

```rust
pub struct Library { pub id: String, pub name: String, pub item_type: ItemType, pub cover_art_id: Option<String> }
pub struct Artist { pub id: String, pub name: String, pub album_count: Option<u32>, pub song_count: Option<u32>, pub cover_art_id: Option<String> }
pub struct Album { pub id: String, pub title: String, pub artist_id: Option<String>, pub artist_name: Option<String>, pub year: Option<u32>, pub song_count: Option<u32>, pub duration_seconds: Option<u32>, pub cover_art_id: Option<String> }
pub struct Song { pub id: String, pub title: String, pub artist_id: Option<String>, pub artist_name: Option<String>, pub album_id: Option<String>, pub album_title: Option<String>, pub duration_seconds: u32, pub bitrate_kbps: Option<u32>, pub track_number: Option<u32>, pub disc_number: Option<u32>, pub cover_art_id: Option<String>, pub date_added: Option<String>, pub last_played_at: Option<String>, pub play_count: Option<u32>, pub is_favorite: Option<bool>, pub content_type: Option<String>, pub suffix: Option<String> }
pub struct Genre { pub id: String, pub name: String, pub song_count: Option<u32>, pub cover_art_id: Option<String> }
pub struct Playlist { pub id: String, pub name: String, pub song_count: Option<u32>, pub duration_seconds: Option<u32>, pub cover_art_id: Option<String> }
```

`ChangeEvent`, `ProviderChangeContext`, and `ProviderSyncedSong` carry provider change metadata for incremental sync:

```rust
pub struct ProviderSyncedSong {
    pub song_id: String,
    pub album_id: Option<String>,
    pub size: Option<u64>,
    pub content_type: Option<String>,
    pub suffix: Option<String>,
    pub version: Option<String>,
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
    pub jellyfin_user_id: Option<String>,          // provider user/profile ID; legacy DB column name
    pub name: Option<String>,
    pub auto_sync_on_connect: bool,
    pub transcoding_profile_id: Option<String>,
    pub sync_rules: Option<String>,                // future use
    pub last_seen_at: Option<String>,
}
```

---

## ServerConfig (SQLite `server_config` table)

Stores the active media-server connection metadata. Secrets are not stored here.

```rust
pub struct ServerConfig {
    pub id: String,                // machine-local UUID primary key
    pub url: String,
    pub server_type: String,       // "jellyfin" | "subsonic" | "openSubsonic"
    pub username: String,
    pub server_version: Option<String>,
    pub name: Option<String>,
    pub icon: Option<String>,
    pub updated_at: i64,
    pub selected: bool,
    pub server_id: Option<String>,          // deterministic portable identity
    pub server_reported_id: Option<String>, // Jellyfin System/Info.Id when known
}
```

`server_id` is derived from `server_type`, normalized URL or server-reported id, and username. It is used in device manifests and basket items so synced media can be routed back to its source provider across remove/re-add and multi-machine scenarios.

---

## Auto-fill Runtime Tables

Machine-local SQLite tables supporting the configurable pipeline:

| Table | Key | Purpose |
|-------|-----|---------|
| `autofill_history` | `(device_id, server_id, track_id)` | Last synced time and optional rotation tier per track |
| `autofill_rotation` | `(device_id, server_id)` | Rotation cursor for Memory tiers |
| `autofill_pity` | `(device_id, server_id)` | Dry-streak counter for pity discovery reserve |

---

## DeviceProfileEntry (transcoding profiles)

Loaded from `device-profiles.json` in the app data dir.

```rust
pub struct DeviceProfileEntry {
    pub id: String,
    pub name: String,
    pub description: String,
    pub device_profile: Option<serde_json::Value>, // null = passthrough
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
| Legacy Jellyfin URL + User ID | `<AppData>/HifiMule/config.json` as `{ "url": "...", "user_id": "..." }` |
| Current server metadata | SQLite `server_config` table |
| Access token / provider secret | OS keyring, service name `"hifimule.github.io"`, username `"secrets"` |
