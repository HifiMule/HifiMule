---
title: 'Transcoding Handshake via Device Profiles'
slug: 'transcoding-device-profiles-playback-handshake'
created: '2026-03-29'
status: 'review'
stepsCompleted: [1, 2, 3, 4]
tech_stack: ['Rust', 'Tokio', 'reqwest', 'serde_json', 'SQLite/rusqlite']
files_to_modify:
  - 'jellyfinsync-daemon/assets/device-profiles.json'
  - 'jellyfinsync-daemon/src/paths.rs'
  - 'jellyfinsync-daemon/src/transcoding.rs'
  - 'jellyfinsync-daemon/src/api.rs'
  - 'jellyfinsync-daemon/src/device/mod.rs'
  - 'jellyfinsync-daemon/src/db.rs'
  - 'jellyfinsync-daemon/src/sync.rs'
  - 'jellyfinsync-daemon/src/rpc.rs'
  - 'jellyfinsync-daemon/src/main.rs'
code_patterns: []
test_patterns: []
---

# Tech-Spec: Transcoding Handshake via Device Profiles

**Created:** 2026-03-29

## Overview

### Problem Statement

The sync engine currently downloads files as-is via `/Items/{id}/Download`. Devices like iPods running Rockbox or generic MP3 players may not support all source formats (FLAC, Opus, AAC). Without transcoding negotiation, incompatible files are written to the device but will fail to play. The PRD designated this as Post-MVP ("Transcoding Handshake: Dynamic server-side re-encoding via Jellyfin API"), and this spec moves it into the active roadmap.

### Solution

Install a user-editable `device-profiles.json` file into the app data directory on first run (seeded from an embedded default). Each entry contains a Jellyfin `DeviceProfile` payload. When a device manifest has a `transcoding_profile_id` set, the sync engine calls `POST /Items/{id}/PlaybackInfo` with the associated `DeviceProfile` to negotiate the stream. If Jellyfin responds with a `TranscodingUrl`, the engine streams from that URL instead of `/Items/{id}/Download`. If Jellyfin indicates direct play is supported for the profile, it falls back to the existing download path.

### Scope

**In Scope:**
- `device-profiles.json` bundled as binary asset, seeded to app data dir on first run (editable post-install)
- New `transcoding.rs` module: profile types, file loader, first-run seeder
- `get_playback_info_stream_url()` API method: POST `/Items/{id}/PlaybackInfo` returning stream URL
- `transcoding_profile_id: Option<String>` field on `DeviceManifest`
- `transcoding_profile_id` column in SQLite `devices` table (with migration)
- **Profile selection during device initialization**: `device_initialize` RPC accepts optional `transcodingProfileId`; written into the new manifest and stored in the DB
- `initialize_device()` accepts `transcoding_profile_id: Option<String>` and writes it to the manifest
- Sync engine (`execute_sync`) accepts optional `DeviceProfile` and uses PlaybackInfo URL when set
- New RPC method `device_profiles.list` — returns available profiles from JSON file
- New RPC method `device.set_transcoding_profile` — sets profile ID on existing device manifest
- UI-triggered sync (`rpc.rs`) loads device profile and passes to `execute_sync`
- Auto-sync (`main.rs`) loads device profile and passes to `execute_sync`

**Out of Scope:**
- UI for creating or editing profiles (user edits `device-profiles.json` directly)
- Storage projection updates for estimated transcoded size
- Video transcoding
- Multi-quality selection UI
- Changes to the Tauri UI frontend (TypeScript/Shoelace)

---

## Context for Development

### Codebase Patterns

- **Download today** (`api.rs:635`): `download_item_stream()` calls `GET /Items/{id}/Download` and returns a `bytes_stream()`. No profile, no negotiation.
- **Sync call site** (`sync.rs:472`): `execute_sync()` calls `download_item_stream()` per add-item. Two callers: `rpc.rs:955` (UI-triggered) and `main.rs:641` (auto-sync).
- **DeviceManifest** (`device/mod.rs:39`): The `.jellyfinsync.json` manifest struct. Add `transcoding_profile_id: Option<String>` here — it persists per device on the removable medium.
- **DeviceMapping in DB** (`db.rs:8`): SQLite `devices` table, queried in the sync trigger path. Needs `transcoding_profile_id TEXT` column. Note: existing `handle_set_device_profile` in `rpc.rs:316` uses the confusingly-named `profile_id` parameter to store the Jellyfin *user* ID — NOT a transcoding profile. Our new field is distinct.
- **App data dir** (`paths.rs:4`): `get_app_data_dir()` → `%APPDATA%/JellyfinSync/` (Win), `~/Library/Application Support/JellyfinSync/` (mac), `~/.local/share/JellyfinSync/` (Linux). Config (`config.json`) and DB (`jellyfinsync.db`) already live here. `device-profiles.json` goes here too.
- **Asset embedding pattern**: Icons are embedded via `include_bytes!("../assets/icon.png")` in `main.rs`. Use the same pattern for `device-profiles.json` default.
- **Atomic writes pattern**: All manifest writes use Write-Temp-Rename. Not needed for `device-profiles.json` (read-only from the daemon's perspective after seeding).
- **Error handling**: Anyhow `Result<T>` throughout. Async via Tokio. All API errors are non-fatal at the per-file level in `execute_sync` — follow the existing `errors.push(SyncFileError {...})` pattern for PlaybackInfo failures.
- **SQLite migrations**: Done inline in `db.rs:init()` via `ALTER TABLE` with an `is_ok()` guard (see `auto_sync_on_connect` migration pattern at `db.rs:58`).

### Files to Reference

| File | Purpose |
| ---- | ------- |
| [jellyfinsync-daemon/src/api.rs](jellyfinsync-daemon/src/api.rs) | `JellyfinClient`, `download_item_stream()` at line 635 |
| [jellyfinsync-daemon/src/sync.rs](jellyfinsync-daemon/src/sync.rs) | `execute_sync()` at line 384, download call at line 472 |
| [jellyfinsync-daemon/src/rpc.rs](jellyfinsync-daemon/src/rpc.rs) | sync.start handler at line 950, `handle_set_device_profile` at 316 |
| [jellyfinsync-daemon/src/main.rs](jellyfinsync-daemon/src/main.rs) | `run_auto_sync()` at line 472, `execute_sync` call at line 641 |
| [jellyfinsync-daemon/src/device/mod.rs](jellyfinsync-daemon/src/device/mod.rs) | `DeviceManifest` struct at line 39 |
| [jellyfinsync-daemon/src/db.rs](jellyfinsync-daemon/src/db.rs) | `DeviceMapping` struct at line 8, `init()` migrations at line 43 |
| [jellyfinsync-daemon/src/paths.rs](jellyfinsync-daemon/src/paths.rs) | `get_app_data_dir()` at line 4 |

### Technical Decisions

- **Profile storage in manifest vs DB**: `transcoding_profile_id` is stored in **both** the device manifest (`.jellyfinsync.json` on the device) and the SQLite DB. The manifest is the source of truth (travels with the device); the DB column is for quick lookup in the sync trigger without reading the manifest. On device connect, the manifest value is authoritative.
- **`DeviceProfile` payload type**: Use `serde_json::Value` for the actual `deviceProfile` payload within each profile entry. This allows the user to freely edit the JSON without Rust recompile, and avoids a rigid struct that would break if Jellyfin's schema evolves. Schema reference: OpenAPI `DeviceProfile` (fields: `Name`, `MaxStreamingBitrate`, `MusicStreamingTranscodingBitrate`, `DirectPlayProfiles`, `TranscodingProfiles`, `CodecProfiles`). `TranscodingProfile` fields: `Container`, `Type`, `AudioCodec`, `Protocol`, `EstimateContentLength` (boolean, note: NOT `EstimatedContentLength`), `EnableMpegtsM2TsMode`.
- **PlaybackInfo response handling**: Jellyfin returns `MediaSources[0].SupportsDirectPlay` (boolean) and `MediaSources[0].TranscodingUrl` (path string, not full URL). Construct full URL as `{jellyfin_base_url}{TranscodingUrl}`. If `TranscodingUrl` is absent or `SupportsDirectPlay` is true, fall back to `/Items/{id}/Download`.
- **`passthrough` profile**: A special profile with `"deviceProfile": null` in the JSON means no transcoding (same as having no `transcoding_profile_id`). Include this so users can explicitly reset a device without setting the manifest field to null.
- **`execute_sync` signature**: Add `transcoding_profile: Option<serde_json::Value>` as the last parameter. Both callers (`rpc.rs` and `main.rs`) are updated to load the profile from the device manifest and pass it through.
- **First-run seed**: Embed `device-profiles.json` with `include_bytes!` in `main.rs`. On daemon startup (in `start_daemon_core`), call `transcoding::ensure_profiles_file_exists()` which writes the default if the file isn't present. This runs before the RPC server starts.

---

## Implementation Plan

### Task Checklist

- [x] **Task 1** — Create `jellyfinsync-daemon/assets/device-profiles.json` with 4 default profiles
- [x] **Task 2** — Add `get_device_profiles_path()` to `jellyfinsync-daemon/src/paths.rs`
- [x] **Task 3** — Create `jellyfinsync-daemon/src/transcoding.rs` (types, loader, seeder)
- [x] **Task 4** — Add `get_item_stream()` + `resolve_stream_url()` to `jellyfinsync-daemon/src/api.rs`
- [x] **Task 5** — Add `transcoding_profile_id` to `DeviceManifest` + update `initialize_device()` in `device/mod.rs`
- [x] **Task 6** — Add `transcoding_profile_id` column to SQLite + `set_transcoding_profile()` method in `db.rs`
- [x] **Task 7** — Update `handle_device_initialize` in `rpc.rs` to accept and persist `transcodingProfileId`
- [x] **Task 8** — Add `transcoding_profile` param to `execute_sync()` and swap download call in `sync.rs`
- [x] **Task 9** — Add `device_profiles.list` RPC handler in `rpc.rs`
- [x] **Task 10** — Add `device.set_transcoding_profile` RPC handler in `rpc.rs`
- [x] **Task 11** — Update `sync.start` handler in `rpc.rs` to load profile and pass to `execute_sync`
- [x] **Task 12** — Update `run_auto_sync()` in `main.rs` to load profile and pass to `execute_sync`
- [x] **Task 13** — Seed `device-profiles.json` on daemon startup in `main.rs`

---

### Tasks

#### Task 1 — Create default `device-profiles.json` asset

**File:** `jellyfinsync-daemon/assets/device-profiles.json` (NEW)

Create the bundled default profiles file:

```json
{
  "profiles": [
    {
      "id": "passthrough",
      "name": "No Transcoding (Download Original)",
      "description": "Download files as-is from Jellyfin without transcoding.",
      "deviceProfile": null
    },
    {
      "id": "rockbox-mp3-320",
      "name": "Rockbox / iPod — MP3 320 kbps",
      "description": "For iPods and DAPs running Rockbox firmware. Transcodes non-MP3 to MP3 320 kbps; passes through MP3 and FLAC directly.",
      "deviceProfile": {
        "Name": "JellyfinSync-Rockbox",
        "MaxStreamingBitrate": 320000,
        "MusicStreamingTranscodingBitrate": 320000,
        "DirectPlayProfiles": [
          { "Container": "mp3", "Type": "Audio", "AudioCodec": "mp3" },
          { "Container": "flac", "Type": "Audio", "AudioCodec": "flac" },
          { "Container": "ogg", "Type": "Audio", "AudioCodec": "vorbis" },
          { "Container": "opus", "Type": "Audio", "AudioCodec": "opus" }
        ],
        "TranscodingProfiles": [
          {
            "Container": "mp3",
            "Type": "Audio",
            "AudioCodec": "mp3",
            "Protocol": "http",
            "EstimateContentLength": true,
            "EnableMpegtsM2TsMode": false
          }
        ],
        "CodecProfiles": []
      }
    },
    {
      "id": "rockbox-mp3-192",
      "name": "Rockbox / iPod — MP3 192 kbps",
      "description": "For devices with limited storage. Transcodes everything to MP3 192 kbps.",
      "deviceProfile": {
        "Name": "JellyfinSync-Rockbox-192",
        "MaxStreamingBitrate": 192000,
        "MusicStreamingTranscodingBitrate": 192000,
        "DirectPlayProfiles": [],
        "TranscodingProfiles": [
          {
            "Container": "mp3",
            "Type": "Audio",
            "AudioCodec": "mp3",
            "Protocol": "http",
            "EstimateContentLength": true,
            "EnableMpegtsM2TsMode": false
          }
        ],
        "CodecProfiles": []
      }
    },
    {
      "id": "generic-mp3-player",
      "name": "Generic MP3 Player",
      "description": "For basic MP3 players that only support MP3. Transcodes all audio to MP3 256 kbps.",
      "deviceProfile": {
        "Name": "JellyfinSync-Generic",
        "MaxStreamingBitrate": 256000,
        "MusicStreamingTranscodingBitrate": 256000,
        "DirectPlayProfiles": [
          { "Container": "mp3", "Type": "Audio", "AudioCodec": "mp3" }
        ],
        "TranscodingProfiles": [
          {
            "Container": "mp3",
            "Type": "Audio",
            "AudioCodec": "mp3",
            "Protocol": "http",
            "EstimateContentLength": true,
            "EnableMpegtsM2TsMode": false
          }
        ],
        "CodecProfiles": []
      }
    }
  ]
}
```

---

#### Task 2 — Add `get_device_profiles_path()` to `paths.rs`

**File:** `jellyfinsync-daemon/src/paths.rs`

Add after the existing `get_app_data_dir()` function:

```rust
pub fn get_device_profiles_path() -> Result<PathBuf> {
    Ok(get_app_data_dir()?.join("device-profiles.json"))
}
```

---

#### Task 3 — Create `transcoding.rs` module

**File:** `jellyfinsync-daemon/src/transcoding.rs` (NEW)

```rust
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

/// A single entry in device-profiles.json.
/// `device_profile` is `None` for the passthrough (no-transcode) profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceProfileEntry {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(rename = "deviceProfile")]
    pub device_profile: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct ProfilesFile {
    profiles: Vec<DeviceProfileEntry>,
}

/// Load all profiles from `device-profiles.json` at the given path.
pub fn load_profiles(path: &Path) -> Result<Vec<DeviceProfileEntry>> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| anyhow!("Failed to read device-profiles.json: {}", e))?;
    let file: ProfilesFile = serde_json::from_str(&content)
        .map_err(|e| anyhow!("Failed to parse device-profiles.json: {}", e))?;
    Ok(file.profiles)
}

/// Find a profile by ID. Returns None if not found or if the profile is passthrough (null payload).
pub fn find_device_profile(path: &Path, profile_id: &str) -> Result<Option<Value>> {
    let profiles = load_profiles(path)?;
    let entry = profiles.into_iter().find(|p| p.id == profile_id);
    Ok(entry.and_then(|e| e.device_profile))
}

/// Seed the default device-profiles.json to `dest_path` if it does not already exist.
/// The default content is the embedded asset bytes.
pub fn ensure_profiles_file_exists(dest_path: &Path, default_bytes: &[u8]) -> Result<()> {
    if dest_path.exists() {
        return Ok(());
    }
    if let Some(parent) = dest_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| anyhow!("Failed to create profiles directory: {}", e))?;
    }
    std::fs::write(dest_path, default_bytes)
        .map_err(|e| anyhow!("Failed to write default device-profiles.json: {}", e))?;
    Ok(())
}
```

---

#### Task 4 — Add `get_item_stream()` to `api.rs`

**File:** `jellyfinsync-daemon/src/api.rs`

**Design rationale:** `write_file_streamed` (sync.rs:711) requires `S: Stream + Unpin`. Two separate `impl Stream` return types (from `download_item_stream` vs a new transcoding stream) are different concrete types and cannot be assigned to the same variable without type erasure. Since both code paths ultimately call `response.bytes_stream()` on a `reqwest::Response`, we unify them in a single method that returns one concrete `impl Stream` type.

Add to `impl JellyfinClient`, after `download_item_stream()` (after line 660):

```rust
/// Unified item stream resolver. If `transcoding_profile` is Some, calls
/// POST /Items/{id}/PlaybackInfo to negotiate the stream URL. Falls back to
/// /Items/{id}/Download if direct play is supported or PlaybackInfo returns no
/// transcoding URL. If `transcoding_profile` is None, uses /Download directly.
///
/// Both code paths call `response.bytes_stream()` on a `reqwest::Response`,
/// so the return type is a single concrete impl Stream (no type erasure needed).
pub async fn get_item_stream(
    &self,
    base_url: &str,
    token: &str,
    user_id: &str,
    item_id: &str,
    transcoding_profile: Option<&serde_json::Value>,
) -> Result<impl futures::Stream<Item = std::result::Result<bytes::Bytes, reqwest::Error>>> {
    CredentialManager::validate_url(base_url)?;
    CredentialManager::validate_token(token)?;

    let mut headers = HeaderMap::new();
    headers.insert(
        "X-Emby-Token",
        HeaderValue::from_str(token).map_err(|_| anyhow!("Invalid token format"))?,
    );

    // Resolve the URL to stream from
    let stream_url = if let Some(profile) = transcoding_profile {
        self.resolve_stream_url(base_url, token, user_id, item_id, profile).await?
    } else {
        format!("{}/Items/{}/Download", base_url.trim_end_matches('/'), item_id)
    };

    let response = self
        .client
        .get(&stream_url)
        .headers(headers)
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(anyhow!("Stream returned status: {}", response.status()));
    }

    Ok(response.bytes_stream())
}

/// Calls POST /Items/{itemId}/PlaybackInfo with the given DeviceProfile.
/// Returns the URL to stream from:
///   - TranscodingUrl from PlaybackInfo if server must transcode
///   - /Items/{id}/Download if direct play is supported or no transcoding URL
async fn resolve_stream_url(
    &self,
    base_url: &str,
    token: &str,
    user_id: &str,
    item_id: &str,
    device_profile: &serde_json::Value,
) -> Result<String> {
    let endpoint = format!(
        "{}/Items/{}/PlaybackInfo?userId={}",
        base_url.trim_end_matches('/'),
        item_id,
        user_id
    );

    let mut headers = HeaderMap::new();
    headers.insert(
        "X-Emby-Token",
        HeaderValue::from_str(token).map_err(|_| anyhow!("Invalid token format"))?,
    );

    let body = serde_json::json!({
        "DeviceProfile": device_profile,
        "UserId": user_id,
        "IsPlayback": true,
        "AutoOpenLiveStream": true
    });

    let response = self
        .client
        .post(&endpoint)
        .headers(headers)
        .json(&body)
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(anyhow!("PlaybackInfo returned status: {}", response.status()));
    }

    let json: serde_json::Value = response.json().await?;

    if let Some(source) = json["MediaSources"].as_array().and_then(|a| a.first()) {
        let supports_direct_play = source["SupportsDirectPlay"].as_bool().unwrap_or(false);
        if !supports_direct_play {
            if let Some(transcode_path) = source["TranscodingUrl"].as_str() {
                // TranscodingUrl is a path (e.g. "/Videos/abc/stream.mp3?...")
                // Prepend the base URL to form the full URL
                return Ok(format!(
                    "{}{}",
                    base_url.trim_end_matches('/'),
                    transcode_path
                ));
            }
        }
    }

    // Direct play supported or no TranscodingUrl — use Download endpoint
    Ok(format!(
        "{}/Items/{}/Download",
        base_url.trim_end_matches('/'),
        item_id
    ))
}
```

---

#### Task 5 — Add `transcoding_profile_id` to `DeviceManifest` and `initialize_device()`

**File:** `jellyfinsync-daemon/src/device/mod.rs`

**5a.** In the `DeviceManifest` struct (line 39), add after `auto_fill`:

```rust
/// ID referencing an entry in device-profiles.json. None = no transcoding (passthrough).
#[serde(default)]
pub transcoding_profile_id: Option<String>,
```

**5b.** Update `initialize_device()` signature (line 280) to accept the new parameter:

```rust
pub async fn initialize_device(
    &self,
    folder_path: &str,
    transcoding_profile_id: Option<String>,
) -> Result<DeviceManifest>
```

**5c.** In the `DeviceManifest` construction block (line 317), add the new field:

```rust
let manifest = DeviceManifest {
    device_id,
    name: None,
    version: "1.0".to_string(),
    managed_paths,
    synced_items: vec![],
    dirty: false,
    pending_item_ids: vec![],
    basket_items: vec![],
    auto_sync_on_connect: false,
    auto_fill: AutoFillPrefs::default(),
    transcoding_profile_id,   // NEW — stored in .jellyfinsync.json on the device
};
```

**Note:** The `transcoding_profile_id` is serialized into the `.jellyfinsync.json` manifest on the device. This means if the same device is connected to a different machine or reinstallation, the profile preference travels with the device.

---

#### Task 6 — Add `transcoding_profile_id` to SQLite `devices` table

**File:** `jellyfinsync-daemon/src/db.rs`

**6a.** In `DeviceMapping` struct (line 8), add:
```rust
pub transcoding_profile_id: Option<String>,
```

**6b.** In `init()` (after the `auto_sync_on_connect` migration block, around line 68), add a migration for the new column:
```rust
let has_transcoding_col: bool = conn
    .prepare("SELECT transcoding_profile_id FROM devices LIMIT 0")
    .is_ok();
if !has_transcoding_col {
    conn.execute(
        "ALTER TABLE devices ADD COLUMN transcoding_profile_id TEXT",
        [],
    )
    .map_err(|e| anyhow!("Failed to add transcoding_profile_id column: {}", e))?;
}
```

**6c.** In `get_device_mapping()` (line 91), update the SELECT query to include `transcoding_profile_id`:
```sql
SELECT id, name, jellyfin_user_id, sync_rules, last_seen_at, auto_sync_on_connect, transcoding_profile_id FROM devices WHERE id = ?
```
Update the row mapping to add:
```rust
transcoding_profile_id: row.get(6)?,
```

**6d.** Add a new `set_transcoding_profile()` method to `Database` (after `set_auto_sync_on_connect`):
```rust
pub fn set_transcoding_profile(&self, device_id: &str, profile_id: Option<&str>) -> Result<()> {
    let conn = self.conn.lock().unwrap();
    conn.execute(
        "UPDATE devices SET transcoding_profile_id = ? WHERE id = ?",
        params![profile_id, device_id],
    )
    .map_err(|e| anyhow!("Failed to set transcoding profile: {}", e))?;
    Ok(())
}
```

---

#### Task 7 — Update `handle_device_initialize` in `rpc.rs` to accept transcoding profile

**File:** `jellyfinsync-daemon/src/rpc.rs`

The existing `device_initialize` handler (line 1244) accepts `folderPath` and `profileId` (Jellyfin user ID). Add an optional `transcodingProfileId` parameter for the transcoding device profile.

**7a.** After extracting `profile_id` (line 1260), add:

```rust
// Optional — if not provided, device uses passthrough (no transcoding)
let transcoding_profile_id = params["transcodingProfileId"].as_str().map(|s| s.to_string());

// Validate the transcoding profile ID exists in device-profiles.json (if provided)
if let Some(ref tpid) = transcoding_profile_id {
    let profiles_path = crate::paths::get_device_profiles_path().map_err(|e| JsonRpcError {
        code: ERR_STORAGE_ERROR,
        message: e.to_string(),
        data: None,
    })?;
    let profiles = crate::transcoding::load_profiles(&profiles_path).map_err(|e| JsonRpcError {
        code: ERR_STORAGE_ERROR,
        message: format!("Failed to load device profiles: {}", e),
        data: None,
    })?;
    if !profiles.iter().any(|p| p.id == *tpid) {
        return Err(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: format!("Transcoding profile '{}' not found in device-profiles.json", tpid),
            data: None,
        });
    }
}
```

**7b.** Update the `initialize_device` call (line 1268) to pass the new param:

```rust
let manifest = state
    .device_manager
    .initialize_device(folder_path, transcoding_profile_id.clone())
    .await
    ...
```

**7c.** After `upsert_device_mapping` (line 1278), store the transcoding profile in the DB:

```rust
if let Some(ref tpid) = transcoding_profile_id {
    state
        .db
        .set_transcoding_profile(&manifest.device_id, Some(tpid))
        .map_err(|e| JsonRpcError {
            code: ERR_STORAGE_ERROR,
            message: format!("Failed to store transcoding profile: {}", e),
            data: None,
        })?;
}
```

**7d.** Update the success response to include `transcodingProfileId`:

```rust
Ok(serde_json::json!({
    "status": "success",
    "data": {
        "deviceId": manifest.device_id,
        "version": manifest.version,
        "managedPaths": manifest.managed_paths,
        "transcodingProfileId": manifest.transcoding_profile_id,
    }
}))
```

**Note on existing tests:** The tests `test_rpc_device_initialize_success_root` and `test_rpc_device_initialize_success_subfolder` pass `{"folderPath": "...", "profileId": "..."}` without a `transcodingProfileId`. Since the field is optional, these tests continue to pass without modification. Add a new test `test_rpc_device_initialize_with_transcoding_profile` that passes `"transcodingProfileId": "rockbox-mp3-320"` and verifies the manifest contains the profile ID.

---

#### Task 8 — Modify `execute_sync()` in `sync.rs`

**File:** `jellyfinsync-daemon/src/sync.rs`

**8a.** Add `transcoding_profile: Option<serde_json::Value>` as the last parameter to `execute_sync()` (line 384):

```rust
pub async fn execute_sync(
    delta: &SyncDelta,
    device_path: &Path,
    jellyfin_client: &crate::api::JellyfinClient,
    jellyfin_url: &str,
    jellyfin_token: &str,
    jellyfin_user_id: &str,
    operation_manager: Arc<SyncOperationManager>,
    operation_id: String,
    device_manager: Arc<crate::device::DeviceManager>,
    transcoding_profile: Option<serde_json::Value>,  // NEW
) -> Result<(Vec<crate::device::SyncedItem>, Vec<SyncFileError>)>
```

**8b.** Replace the existing download call (lines 471–486) with a single call to the new `get_item_stream()`:

```rust
// Resolve stream via PlaybackInfo if a profile is set, else direct /Download
let stream_result = jellyfin_client
    .get_item_stream(
        jellyfin_url,
        jellyfin_token,
        jellyfin_user_id,
        &add_item.jellyfin_id,
        transcoding_profile.as_ref(),
    )
    .await;

let stream = match stream_result {
    Ok(stream) => stream,
    Err(e) => {
        errors.push(SyncFileError {
            jellyfin_id: add_item.jellyfin_id.clone(),
            filename: add_item.name.clone(),
            error_message: format!("Failed to get stream: {}", e),
        });
        continue;
    }
};
```

**Note:** No `Box::pin` type erasure needed. Both code paths in `get_item_stream()` call `response.bytes_stream()` on a `reqwest::Response`, so Rust sees one concrete `impl Stream` type. `write_file_streamed` requires `S: Stream + Unpin` — `reqwest`'s byte stream satisfies both bounds.

---

#### Task 9 — Add RPC method `device_profiles.list`

**File:** `jellyfinsync-daemon/src/rpc.rs`

**8a.** In the match dispatch (line 127), add:
```rust
"device_profiles.list" => handle_device_profiles_list().await,
```

**8b.** Add the handler function:
```rust
async fn handle_device_profiles_list() -> Result<Value, JsonRpcError> {
    let path = crate::paths::get_device_profiles_path().map_err(|e| JsonRpcError {
        code: ERR_STORAGE_ERROR,
        message: format!("Failed to get profiles path: {}", e),
        data: None,
    })?;

    let profiles = crate::transcoding::load_profiles(&path).map_err(|e| JsonRpcError {
        code: ERR_STORAGE_ERROR,
        message: format!("Failed to load device profiles: {}", e),
        data: None,
    })?;

    // Return id, name, description only — not the full deviceProfile payload
    let summary: Vec<Value> = profiles
        .iter()
        .map(|p| {
            serde_json::json!({
                "id": p.id,
                "name": p.name,
                "description": p.description,
            })
        })
        .collect();

    Ok(Value::Array(summary))
}
```

---

#### Task 10 — Add RPC method `device.set_transcoding_profile`

**File:** `jellyfinsync-daemon/src/rpc.rs`

**9a.** In the match dispatch, add:
```rust
"device.set_transcoding_profile" => handle_set_transcoding_profile(&state, payload.params).await,
```

**9b.** Add the handler:
```rust
async fn handle_set_transcoding_profile(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let params = params.ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing params".to_string(),
        data: None,
    })?;

    let device_id = params["deviceId"].as_str().ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing deviceId".to_string(),
        data: None,
    })?;

    // profile_id can be null to clear transcoding
    let profile_id = params["profileId"].as_str();

    // Validate profile_id exists in profiles file (unless null/passthrough)
    if let Some(id) = profile_id {
        if id != "passthrough" {
            let path = crate::paths::get_device_profiles_path().map_err(|e| JsonRpcError {
                code: ERR_STORAGE_ERROR,
                message: e.to_string(),
                data: None,
            })?;
            let profiles = crate::transcoding::load_profiles(&path).map_err(|e| JsonRpcError {
                code: ERR_STORAGE_ERROR,
                message: e.to_string(),
                data: None,
            })?;
            if !profiles.iter().any(|p| p.id == id) {
                return Err(JsonRpcError {
                    code: ERR_INVALID_PARAMS,
                    message: format!("Profile '{}' not found in device-profiles.json", id),
                    data: None,
                });
            }
        }
    }

    // Persist to SQLite DB
    state
        .db
        .set_transcoding_profile(device_id, profile_id)
        .map_err(|e| JsonRpcError {
            code: ERR_STORAGE_ERROR,
            message: e.to_string(),
            data: None,
        })?;

    // Update in-memory device manifest
    state
        .device_manager
        .update_manifest(|m| {
            m.transcoding_profile_id = profile_id.map(|s| s.to_string());
        })
        .await
        .map_err(|e| JsonRpcError {
            code: ERR_STORAGE_ERROR,
            message: e.to_string(),
            data: None,
        })?;

    Ok(Value::Bool(true))
}
```

---

#### Task 11 — Update `rpc.rs` sync.start handler to load and pass profile

**File:** `jellyfinsync-daemon/src/rpc.rs`

In the `sync.start` handler's `tokio::spawn` block (around line 954), before calling `execute_sync()`, load the device's transcoding profile:

```rust
// Load transcoding profile from device manifest
let transcoding_profile = {
    let device = state.device_manager.get_current_device().await;
    if let Some(ref manifest) = device {
        if let Some(ref profile_id) = manifest.transcoding_profile_id {
            match crate::paths::get_device_profiles_path()
                .and_then(|p| crate::transcoding::find_device_profile(&p, profile_id))
            {
                Ok(profile) => profile,
                Err(e) => {
                    eprintln!("[Sync] Failed to load transcoding profile '{}': {}", profile_id, e);
                    None
                }
            }
        } else {
            None
        }
    } else {
        None
    }
};
```

Then add `transcoding_profile` as the last argument to `execute_sync()`.

---

#### Task 12 — Update `main.rs` `run_auto_sync()` to load and pass profile

**File:** `jellyfinsync-daemon/src/main.rs`

In `run_auto_sync()`, after building `desired_items` and before calling `execute_sync()` (around line 641), add:

```rust
// Load transcoding profile if set on the device manifest
let transcoding_profile = if let Some(ref profile_id) = manifest.transcoding_profile_id {
    match crate::paths::get_device_profiles_path()
        .and_then(|p| crate::transcoding::find_device_profile(&p, profile_id))
    {
        Ok(profile) => profile,
        Err(e) => {
            daemon_log!("[AutoSync] Failed to load transcoding profile '{}': {}", profile_id, e);
            None
        }
    }
} else {
    None
};
```

Then add `transcoding_profile` as the last argument to `execute_sync()`.

---

#### Task 13 — Seed `device-profiles.json` on daemon startup

**File:** `jellyfinsync-daemon/src/main.rs`

**12a.** Add `mod transcoding;` to the module declarations.

**12b.** In `start_daemon_core()`, immediately after the `let db = ...` block (around line 148), add:

```rust
// Seed default device-profiles.json if not present
let profiles_default = include_bytes!("../assets/device-profiles.json");
if let Ok(profiles_path) = crate::paths::get_device_profiles_path() {
    if let Err(e) = crate::transcoding::ensure_profiles_file_exists(&profiles_path, profiles_default) {
        daemon_log!("Warning: Failed to seed device-profiles.json: {}", e);
        // Non-fatal — transcoding will be unavailable until the file exists
    }
}
```

---

### Acceptance Criteria

#### AC-1: Default profiles file seeded on first run

**Given** the daemon starts for the first time (no `device-profiles.json` in app data dir)
**When** the daemon initializes
**Then** `{app_data_dir}/device-profiles.json` is created containing the 4 default profiles (passthrough, rockbox-mp3-320, rockbox-mp3-192, generic-mp3-player)

**Given** `device-profiles.json` already exists (e.g., user has edited it)
**When** the daemon starts
**Then** the existing file is NOT overwritten

---

#### AC-2: Transcoding profile is written to manifest during device initialization

**Given** a new unrecognized device is connected
**When** a `device_initialize` RPC call is made with `{"folderPath": "Music", "profileId": "user-abc", "transcodingProfileId": "rockbox-mp3-320"}`
**Then** the `.jellyfinsync.json` written to the device contains `"transcoding_profile_id": "rockbox-mp3-320"`
**And** the in-memory manifest has `transcoding_profile_id = Some("rockbox-mp3-320")`
**And** the SQLite `devices` table row has `transcoding_profile_id = "rockbox-mp3-320"`
**And** the RPC response includes `"transcodingProfileId": "rockbox-mp3-320"` in `data`

**Given** `device_initialize` is called WITHOUT `transcodingProfileId`
**When** the device is initialized
**Then** `transcoding_profile_id` is `null`/`None` in both the manifest and the DB
**And** the device syncs using the passthrough (direct download) path

**Given** `device_initialize` is called with a `transcodingProfileId` that does not exist in `device-profiles.json`
**When** the call is processed
**Then** the response returns `ERR_INVALID_PARAMS` and the device is NOT initialized

---

#### AC-3: `device_profiles.list` RPC returns available profiles

**Given** the daemon is running and `device-profiles.json` exists
**When** a JSON-RPC call `{"method": "device_profiles.list"}` is made
**Then** the response contains an array of objects with `id`, `name`, `description` fields
**And** the full `deviceProfile` payload is NOT included in the response

**Given** `device-profiles.json` does not exist or is malformed
**When** `device_profiles.list` is called
**Then** the response contains an error with code matching `ERR_STORAGE_ERROR`

---

#### AC-4: `device.set_transcoding_profile` persists profile to manifest and DB

**Given** a connected device with ID `device-123`
**When** a JSON-RPC call `{"method": "device.set_transcoding_profile", "params": {"deviceId": "device-123", "profileId": "rockbox-mp3-320"}}` is made
**Then** the device manifest in memory has `transcoding_profile_id = "rockbox-mp3-320"`
**And** the SQLite `devices` table row for `device-123` has `transcoding_profile_id = "rockbox-mp3-320"`

**Given** a `profileId` that does not exist in `device-profiles.json`
**When** `device.set_transcoding_profile` is called
**Then** the response contains an error with code `ERR_INVALID_PARAMS`

**Given** `{"profileId": null}` is passed
**When** `device.set_transcoding_profile` is called
**Then** `transcoding_profile_id` is cleared to `null`/`None` on both manifest and DB

---

#### AC-5: Sync engine uses PlaybackInfo when transcoding profile is set

**Given** a device with `transcoding_profile_id = "rockbox-mp3-320"` in its manifest
**And** a sync delta with items to add
**When** `execute_sync()` runs
**Then** for each add item, `POST /Items/{id}/PlaybackInfo` is called with the `rockbox-mp3-320` device profile payload
**And** if the response contains a `TranscodingUrl`, the file is streamed from `{base_url}{TranscodingUrl}`
**And** if the response indicates `SupportsDirectPlay: true` (no transcoding needed), the file is downloaded via `/Items/{id}/Download`

**Given** a device with no `transcoding_profile_id` (or `null`)
**When** `execute_sync()` runs
**Then** `PlaybackInfo` is NOT called; files are downloaded directly via `/Items/{id}/Download` (existing behavior unchanged)

**Given** `PlaybackInfo` returns an HTTP error for a specific item
**When** `execute_sync()` processes that item
**Then** the item is recorded as a `SyncFileError` with an appropriate message
**And** the sync continues processing remaining items (non-fatal, per existing pattern)

---

#### AC-6: Auto-sync path passes device transcoding profile

**Given** a device with `auto_sync_on_connect: true` and `transcoding_profile_id = "generic-mp3-player"`
**When** the device is connected and `run_auto_sync()` triggers
**Then** the sync engine receives the `generic-mp3-player` device profile and uses PlaybackInfo for each downloaded file

---

## Additional Context

### Dependencies

No new Cargo dependencies required. `serde_json::Value` (already a dependency via `serde_json`) handles the flexible DeviceProfile payload. `reqwest` (already in use) handles the POST to PlaybackInfo.

### Testing Strategy

- **`transcoding.rs` unit tests**: Test `load_profiles()` with valid and malformed JSON. Test `find_device_profile()` with known IDs, unknown IDs, and the `passthrough` profile. Test `ensure_profiles_file_exists()` — creates file when absent, does not overwrite when present.
- **`api.rs` tests**: Add `mockito` tests for `get_item_stream()` covering: (a) `transcoding_profile=None` → calls `/Items/{id}/Download` directly; (b) profile set, server returns `TranscodingUrl` → streams from that URL; (c) profile set, server returns `SupportsDirectPlay: true` → falls back to `/Download`; (d) PlaybackInfo returns HTTP error → `Err` propagated. Follow the existing `test_download_item_stream_*` mockito pattern in `api.rs`. Also test `resolve_stream_url()` in isolation with mocked PlaybackInfo responses.
- **`db.rs` tests**: Test `set_transcoding_profile()` sets and clears the value. Test `get_device_mapping()` returns `transcoding_profile_id` correctly.
- **`device/mod.rs` tests**: Add `test_initialize_device_with_transcoding_profile` — verify that `initialize_device("Music", Some("rockbox-mp3-320"))` writes `transcoding_profile_id` to the manifest JSON on disk. Add `test_initialize_device_without_transcoding_profile` — verify `None` produces `null` in the manifest.
- **`rpc.rs` integration tests**: Follow existing `test_rpc_set_device_profile()` pattern for `device.set_transcoding_profile` and `device_profiles.list`. Add `test_rpc_device_initialize_with_transcoding_profile` verifying the profile ID is returned in the response and stored in the DB. Existing `test_rpc_device_initialize_*` tests need `initialize_device` call updated to pass `None` as the new parameter.
- **`execute_sync()` tests**: The existing streaming tests use mock streams. Add a test that verifies `transcoding_profile: Some(...)` causes the PlaybackInfo path to be taken, using a mockito server.

### Notes

- The `device-profiles.json` path on each platform: Windows `%APPDATA%\JellyfinSync\device-profiles.json`, macOS `~/Library/Application Support/JellyfinSync/device-profiles.json`, Linux `~/.local/share/JellyfinSync/device-profiles.json`.
- No `Box::pin` type erasure required. `get_item_stream()` resolves both download and transcode paths via `response.bytes_stream()` on the same `reqwest::Response` type. The return type unifies naturally as one concrete `impl Stream`.
- The Jellyfin `TranscodingUrl` field is a path like `/Videos/abc123/stream.mp3?api_key=...&...`. Prepend the server base URL. The token in the URL query string may duplicate the `X-Emby-Token` header — this is fine; Jellyfin accepts both.
- **Pre-mortem risk — Jellyfin auth on TranscodingUrl**: The `TranscodingUrl` path from Jellyfin typically includes `?api_key=...` in the query string for authentication. Our `get_item_stream()` also sends an `X-Emby-Token` header for the stream request. This is redundant but harmless. If the token is absent from the URL, the header auth is the fallback.
- **Pre-mortem risk — `MediaSources` empty**: If Jellyfin returns an empty `MediaSources` array from PlaybackInfo (e.g., item not found or user has no access), `resolve_stream_url()` falls back to `/Download`. This is the safe behavior.
- `upsert_device_mapping` already has a confusingly-named `profile_id` parameter that actually stores the Jellyfin *user* ID. The new `set_transcoding_profile` is a separate method on `Database` to avoid ambiguity.
- Storage projection (basket byte estimation) is NOT updated in this spec. The basket will continue to show the source file sizes. Actual transcoded files may be smaller. This is a known limitation to address in a future spec.

---

## Dev Agent Record

### Completion Notes

**Date:** 2026-03-29
**Status:** All 13 tasks implemented and verified.

**Summary:**
- Created `jellyfinsync-daemon/assets/device-profiles.json` with 4 profiles: passthrough, rockbox-mp3-320, rockbox-mp3-192, generic-mp3-player
- Added `get_device_profiles_path()` to `paths.rs`
- Created `transcoding.rs` with `DeviceProfileEntry`, `load_profiles()`, `find_device_profile()`, `ensure_profiles_file_exists()` + unit tests (10 tests)
- Added `get_item_stream()` and `resolve_stream_url()` to `api.rs` — unified stream resolver with PlaybackInfo negotiation
- Added `transcoding_profile_id: Option<String>` to `DeviceManifest` struct and updated `initialize_device()` signature
- Added `transcoding_profile_id` column to SQLite `devices` table via inline migration; added `set_transcoding_profile()` to `Database`; updated `get_device_mapping()` query
- Updated `handle_device_initialize` in `rpc.rs` to accept optional `transcodingProfileId`, validate it, write to manifest and DB, return in response
- Updated `execute_sync()` in `sync.rs` to accept `transcoding_profile: Option<serde_json::Value>` and call `get_item_stream()` instead of `download_item_stream()`
- Added `device_profiles.list` and `device.set_transcoding_profile` RPC handlers
- Updated `sync.start` handler and `run_auto_sync()` to load profile from manifest and pass to `execute_sync()`
- Added `mod transcoding;` and startup seeding via `include_bytes!` in `main.rs`
- Fixed all 24+ existing test `DeviceManifest` constructions across `rpc.rs`, `device/tests.rs`, `tests.rs`, `sync.rs` to include the new field
- Fixed all 7 `initialize_device()` test call sites to pass `None` as the new parameter

**Test Results:** 150 tests pass (0 failures, 0 errors).

### File List

- `jellyfinsync-daemon/assets/device-profiles.json` (NEW)
- `jellyfinsync-daemon/src/transcoding.rs` (NEW)
- `jellyfinsync-daemon/src/paths.rs` (modified)
- `jellyfinsync-daemon/src/api.rs` (modified)
- `jellyfinsync-daemon/src/device/mod.rs` (modified)
- `jellyfinsync-daemon/src/db.rs` (modified)
- `jellyfinsync-daemon/src/sync.rs` (modified)
- `jellyfinsync-daemon/src/rpc.rs` (modified)
- `jellyfinsync-daemon/src/main.rs` (modified)
- `jellyfinsync-daemon/src/device/tests.rs` (modified — test fixes)
- `jellyfinsync-daemon/src/tests.rs` (modified — test fixes)

### Change Log

- 2026-03-29: Implemented transcoding handshake via device profiles — all 13 tasks complete, 150 tests passing
