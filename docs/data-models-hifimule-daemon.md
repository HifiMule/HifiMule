# Data Models — HifiMule Daemon

**Generated:** 2026-05-23 | **Last Updated:** 2026-09-29 | **Scan depth:** Deep

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
    pub audiobook_path: Option<String>,            // serialized as audiobookPath
    pub podcast_path: Option<String>,              // serialized as podcastPath
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
    pub media_role: MediaRole,       // music, audiobook, or podcast
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
    pub item_type: String,             // music types plus "Book", "PodcastShow", "PodcastEpisode"
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
    pub podcast_retention: PodcastRetention, // Audiobookshelf Podcasts only
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
    pub media_role: MediaRole,
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

---

## Audiobookshelf library scope and media roles (2026-09-27)

`server_config` now has nullable `provider_library_id` and `provider_library_role` columns. Both are required for an Audiobookshelf row and absent for other providers. The role is `audiobook` or `podcast`; each selected upstream library is a distinct configured server. `derive_audiobookshelf_server_id` hashes normalized URL, username, immutable library ID, and role for the portable ID; `server_config.id` remains the machine-local UUID for vault, DB, and provider-cache operations.

`MediaRole` is `Music`, `Audiobook`, or `Podcast` and is serialized in each `SyncedItem` as `mediaRole`. Missing values default to Music for older manifests. `DeviceManifest.audiobook_path` and `.podcast_path` serialize as `audiobookPath` and `podcastPath`; `media_path(role)` falls back to the first managed path when an override is absent. `DesiredItem` and sync add/delete entries preserve role during delta calculation.

The neutral domain adds `PodcastShow { type, id, title, description, coverArtId, episodeCount }`, `PodcastEpisode { type, id, showId, showTitle, title, description, durationSeconds, publishedAt, coverArtId }`, `PodcastShowDetail`, and `PodcastSearchResult`. Books map into existing `Album`/`Song` fields, with `ProviderItemMetadata` carrying stable identity, ordered part IDs, chapter markers, and author/narrator presentation credits. Audiobookshelf IDs encode library, item, media, and part or episode identity; titles and local paths are display data only.

`AutoFillPipeline.podcast_retention` stores the role-specific podcast selection policy with recent count, mode, and selected show IDs. It is ignored for music servers. Playback continuity and verified whole-book mapping are persisted in `playback_book_continuity` and `playback_book_mapping` by `playback/persistence.rs`; ordinary device sync does not write listening progress. See the [Audiobookshelf Implementation Map](./audiobookshelf-implementation.md) for data flow and evidence boundaries.
# Live listening report journal (Story 16.7)

Playback persistence schema v12 adds `playback_live_reports`. A row is keyed by a random operation ID and unique `(session_id, occurrence_id, kind)` where `kind=completed`. It freezes logical session, occurrence, attempt, portable source server, server-local track, terminal reason, bounded heard milliseconds, and optional known duration. No credential or stream URL is stored. Status is `ineligible`, `pending`, `sending`, `confirmed`, `failed`, `ambiguous`, or `unsupported`; `attempt_count`, `next_attempt_at`, creation/update timestamps, and an allowlisted diagnostic explain processing without storing provider response text.

Only known-duration playback with at least `min(ceil(duration/2), 240000 ms)` of generation-fenced consumed audio qualifies for the verified Navidrome contract. Jellyfin and Audiobookshelf occurrences are classified `unsupported` before this rule is applied. Pause, buffering, skipped-over seek duration, and restored cursor add no heard time. Replayed audio counts when consumed again. Technical failure, output loss, and shutdown do not qualify; output loss closes the current audible segment, and a later explicit resume starts new evidence at its cursor. An unsent `ineligible` row can be promoted if that same occurrence subsequently qualifies. The queue holds at most 256 pending/sending operations and the journal at most 2048 rows. Terminal rows older than 90 days are pruned on insertion. A worker sends one completion at a time, prioritizes it over status updates, bounds status and completion requests to 8 and 15 seconds respectively, and defers pre-send verification failure with bounded exponential backoff for at most five claims. A timed-out completion is `ambiguous`, and an interrupted sending row becomes `ambiguous` on restart; neither is automatically replayed. A failed journal write produces at most 32 volatile, inspectable failures without sending.


## Source preference journal and session disposition (Story 16.8)

Playback persistence schema **v13** adds two independent tables. Existing transport, Radio and live-report schema and semantics are retained.

`playback_feedback` is a bounded operational journal, not a taste profile. Its global SQLite `INTEGER PRIMARY KEY AUTOINCREMENT` sequence gives each source-track/account a strictly increasing intent order; operation IDs are unique. Each schema-v1 row freezes session ID, logical session ID, occurrence ID, portable server ID, server-local track ID, hashed authenticated account scope, requested value, status, diagnostic, observed value and timestamps. The hash is private and is never serialized to the UI. No credentials, authenticated URLs, recording equivalences or raw provider responses are stored.

- Admission commits before any provider write. At most 128 pending/sending rows and 2,048 total rows are retained. Confirmed, failed and reconciled rows expire after 90 days or are pruned oldest-first at capacity. Ambiguous/conflict rows are never silently discarded to admit more work.
- A single atomic sending claim bounds concurrency. Earlier pending/sending/ambiguous/conflict rows block later intents for the same server, track and account. Other accounts/sources remain independent. Completion updates only the claimed operation; old acknowledgements cannot settle a newer row.
- A definite rejection is failed. Lost replies, interrupted sends or failed/mismatching readback are ambiguous. Restart preserves unsent pending work and changes sending rows to ambiguous. Queued work behind uncertainty becomes conflict. If delivery settlement fails, the worker retries local recovery only; it never resends that request.
- A fresh authoritative read can reconcile uncertain rows only when its captured latest sequence is still current and no request is pending/sending for the source-track/account. It records `observedValue` and `valueObserved`, without claiming causation. Queued opposites behind uncertainty are also resolved as observations and are **not sent**. The user must inspect the refreshed value and explicitly choose again if the latest desired preference is absent. An old request may still have executed remotely; reads do not prove cancellation. Normal confirmed/failed operations let subsequent accepted intents proceed in order.

`playback_feedback_dispositions` contains schema-v1 `(session_id, logical_session_id, occurrence_id, rejected=1)` rows, keyed by session and occurrence and bounded to 10,000. Dislike inserts a rejection in its own committed transaction before remote journal insertion, so remote or journal failure does not erase it. Like deletes only that occurrence's rejection; neutral and other occurrences of the same track have no effect. Idempotent operation replay cannot reapply an older Dislike after a newer Like. Paused restoration retains rejections, including departed Preview occurrences. Ordinary queue replacement keeps the logical session and its history; session replacement/deletion and Radio logical-session replacement delete expired dispositions using database triggers. These rows never become Radio exclusions or alter terminal listening outcomes.

`is_playback_occurrence_rejected(session, occurrence)` supports the current feedback view. `rejected_playback_occurrences(session)` returns the bounded set of exact rejected occurrence IDs for Story 16.9 snapshot filtering, including Preview occurrences no longer displayed. Filtering must never infer rejection from the track ID, recording identity, remote preference, or delivery status.


## Immutable listening snapshots (Story 16.9, playback schema v14)

Migration remains in `Database::init_playback_inner`, in the existing transaction.
Versions 1–13 upgrade through their existing migrations; future playback versions
are rejected. The new tables have no live-session or server foreign keys/cleanup
triggers, so clear, replacement, source removal and restart do not remove saves.

| Table | Stored contract |
| --- | --- |
| `playback_listening_snapshots` | `creation_seq` autoincrement primary key; unique `snapshot_id` and `operation_id`; checked schema/policy version 1; canonical request JSON; immutable name and UTC creation time; captured instance/session/optional Radio logical ID; nonnegative queue revision; optional main occurrence; nonnegative entry count |
| `playback_listening_snapshot_entries` | Primary key `(snapshot_id,ordinal)`; unique `(snapshot_id,occurrence_id)`; nonnegative ordinal; original occurrence and portable source IDs; checked history/current/upcoming origin; frozen local label/icon and optional title/artist/album/duration |

Headers have a descending creation-sequence list index; entry primary keys support
ordered keyset paging. There is no uniqueness constraint on track or recording
identity. Count, first ordinal and last ordinal are validated before a committed
artifact is returned: a nonempty artifact must contain exactly ordinals 0..count-1.
Unsupported saved versions and corrupt/gapped counts are explicit errors.
Header/entries are inserted atomically using SQL window ordering, with a final
count check before commit. Trigger-injected failures and process interruption
before commit leave no partial artifact; earlier saves remain intact.

Snapshot policy 1 reads `playback_occurrences`, closed `playback_attempts`, and
`playback_feedback_dispositions` in that transaction. Source configuration is read
only to freeze a safe name/icon, falling back to portable ID. Metadata is copied
only when already available for the main current source; Preview metadata is not
used. No credentials, authenticated URLs, remote reporting rows, full library
metadata or source-copy substitutions are stored. A changing `sourceAvailable`
read badge does not mutate frozen labels or references.

Committed operation identity is retained for the entire lifetime of a snapshot,
independently of the generic playback command cache. No expiry, prune, rename or
delete operation is provided by this story. Blank/omitted requested names share a
canonical null name, while the generated default display name is persisted once.
The detailed policy, errors, recovery rules, paging wire contract and measured
single-connection contention limits are documented in the API contract's Story
16.9 section. The previous `rejected_playback_occurrences` vector helper remains
for other consumers; snapshot capture uses a transaction-local SQL anti-join and
does not use that helper as a separate acceptance read.
# Snapshot server-playlist export journal (persistence v15)

`playback_server_exports` stores the operation UUID, immutable snapshot UUID, validated base name, canonical request, and bounded retention timestamps. `(snapshot_id, name)` is reserved to prevent concurrent duplicate creates.

`playback_server_export_parts` stores one ordered row per frozen portable source with its ordered occurrence-preserving track-ID plan, state, attempt/generation fence, expected and confirmed counts, known remote playlist identity, diagnostic code, and retry boundary. Valid part states are `planned`, `unsupported`, `denied`, `pending`, `creating`, `populating`, `partial`, `succeeded`, `failed`, `ambiguous`, `unresolved`, and `canceled`. Aggregate state is derived from parts.

Bounds are explicit: 480 UTF-8 bytes/120 characters per name, 10,000 occurrences, 32 sources, 1,000 IDs per provider request, and 30-day terminal-operation retention. Snapshot rows remain immutable. Migration v15 is transactional, retains all v1-v14 data, and remains distinct from RPC schema version 1.
