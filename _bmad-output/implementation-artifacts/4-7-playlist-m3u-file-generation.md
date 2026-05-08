# Story 4.7: Playlist M3U File Generation

**Status: done**

## Story

As a **Ritualist (Arthur)** and **Convenience Seeker (Sarah)**,
I want `.m3u` playlist files to be written to my device when I sync a Jellyfin playlist,
so that my DAP or Rockbox player can natively load and play the playlist in the correct order.

## Acceptance Criteria

1. **M3U file written on sync**: When at least one basket item has `item_type = "Playlist"` and sync runs successfully, a `.m3u` file is written to the managed sync folder (`manifest.managed_paths[0]`, e.g. `device_path/Music`) for each playlist. (AC: #1)

2. **Filename sanitization**: The `.m3u` filename is derived from the playlist name by calling the existing `sanitize_path_component()` function, then appending `.m3u`. If the result exceeds 255 characters, it is truncated using `truncate_filename(base, "m3u", 255)`. (AC: #2)

3. **Extended M3U format**: The file begins with `#EXTM3U`. Each track is preceded by an `#EXTINF:<seconds>,<Artist> - <Title>` line (or `#EXTINF:<seconds>,<Title>` if artist is absent), followed by the track's relative path. (AC: #3)

4. **Relative track paths**: Paths in the `.m3u` are relative to the `.m3u` file location (`managed_path`). For a track whose `local_path` is `Music/Artist/Album/01 - Name.flac` (relative to `device_path`) and the `.m3u` sits in `Music/`, the path entry is `Artist/Album/01 - Name.flac` using forward slashes — the managed subfolder prefix is stripped. (AC: #4)

5. **Duration from RunTimeTicks**: The `#EXTINF` seconds value is `RunTimeTicks ÷ 10,000,000`, cast to `i64`. If `RunTimeTicks` is absent (zero or None), the value is `-1` (standard M3U convention for unknown duration). No additional Jellyfin API calls are made. (AC: #5)

6. **Differential sync — no rewrite if unchanged**: A playlist's `.m3u` is not rewritten if its track list is unchanged. "Unchanged" means the manifest's `playlists` entry for this `jellyfinId` exists and has the same `trackCount` and `trackIds` hash as the current basket tracks. (AC: #6)

7. **Regeneration on change**: If a playlist's track list has changed (different tracks, count, or order), the `.m3u` is regenerated using the Write-Temp-Rename atomic pattern. (AC: #7)

8. **Cleanup on playlist removal**: If a playlist was in the previous sync but is no longer in the current basket, its `.m3u` file is deleted from the device and its entry removed from `manifest.playlists`. (AC: #8)

9. **Atomic write**: All `.m3u` writes use the Write-Temp-Rename pattern: write to `<filename>.m3u.tmp`, call `sync_all()`, then rename to `<filename>.m3u`. (AC: #9)

10. **Manifest tracking**: After M3U generation, `manifest.playlists` is updated with a `PlaylistManifestEntry` for each written playlist: `jellyfinId`, `filename`, `trackCount`, `trackIds` (Vec of track jellyfin IDs in order), `lastModified` (ISO 8601 timestamp). (AC: #10)

11. **Track not yet synced**: If a playlist track's `jellyfin_id` is not found in the manifest `synced_items` (e.g., it failed to download), that track is omitted from the `.m3u` with a log entry. The remaining tracks are still written. (AC: #11)

## Tasks / Subtasks

### T1: Add `run_time_ticks` to `JellyfinItem` in api.rs (AC: #5)

- [x] **T1.1**: In `api.rs`, find the `JellyfinItem` struct (around line 64). Add the field:
  ```rust
  #[serde(default)]
  pub run_time_ticks: Option<u64>,
  ```
  Place it after `cumulative_run_time_ticks`. The Jellyfin API serializes it as `RunTimeTicks` — the `#[serde(rename_all = "PascalCase")]` on the struct handles this automatically.

### T2: New structs in sync.rs (AC: #1, #6, #10)

- [x] **T2.1**: Add `PlaylistTrackInfo` struct near the top of `sync.rs` (after the existing item structs, around line 100):
  ```rust
  /// Metadata for a single track within a playlist, for M3U generation.
  #[derive(Debug, Serialize, Deserialize, Clone)]
  #[serde(rename_all = "camelCase")]
  pub struct PlaylistTrackInfo {
      pub jellyfin_id: String,
      pub artist: Option<String>,
      pub run_time_seconds: i64,  // RunTimeTicks / 10_000_000; -1 if unknown
  }
  ```

- [x] **T2.2**: Add `PlaylistSyncItem` struct immediately after `PlaylistTrackInfo`:
  ```rust
  /// A Jellyfin playlist from the basket, with its ordered track list for M3U generation.
  #[derive(Debug, Serialize, Deserialize, Clone)]
  #[serde(rename_all = "camelCase")]
  pub struct PlaylistSyncItem {
      pub jellyfin_id: String,
      pub name: String,
      pub tracks: Vec<PlaylistTrackInfo>,
  }
  ```

- [x] **T2.3**: Add `playlists` field to the `SyncDelta` struct (around line 99):
  ```rust
  #[derive(Debug, Serialize, Deserialize, Clone)]
  #[serde(rename_all = "camelCase")]
  pub struct SyncDelta {
      pub adds: Vec<SyncAddItem>,
      pub deletes: Vec<SyncDeleteItem>,
      pub id_changes: Vec<SyncIdChangeItem>,
      pub unchanged: usize,
      #[serde(default)]
      pub playlists: Vec<PlaylistSyncItem>,  // ← NEW: playlist basket items with ordered tracks
  }
  ```
  Use `#[serde(default)]` so that existing callers sending a delta without `playlists` still deserialize correctly (zero-risk backward compat).

### T3: New struct in device/mod.rs (AC: #10)

- [x] **T3.1**: Add `PlaylistManifestEntry` struct near the top of `device/mod.rs` (before `DeviceManifest`):
  ```rust
  #[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
  #[serde(rename_all = "camelCase")]
  pub struct PlaylistManifestEntry {
      pub jellyfin_id: String,
      pub filename: String,
      pub track_count: u32,
      pub track_ids: Vec<String>,   // ordered Jellyfin IDs — used for change detection
      pub last_modified: String,    // ISO 8601 timestamp
  }
  ```

- [x] **T3.2**: Add `playlists` field to `DeviceManifest` (after `transcoding_profile_id`):
  ```rust
  #[serde(default)]
  pub playlists: Vec<PlaylistManifestEntry>,
  ```
  Use `#[serde(default)]` so existing manifest files without this field deserialize cleanly.

### T4: Capture playlist basket items in rpc.rs `sync.calculate_delta` handler (AC: #1, #5)

- [x] **T4.1**: In `rpc.rs`, inside the `sync.calculate_delta` handler (around line 747), find the loop that processes basket items and expands containers via `get_child_items_with_sizes`. Add a `playlist_sync_items: Vec<sync::PlaylistSyncItem>` collection before the loop.

- [x] **T4.2**: When the current basket item has `item_type == "Playlist"`, after calling `get_child_items_with_sizes` to get child tracks, build a `PlaylistSyncItem` and push it:
  ```rust
  if item.item_type == "Playlist" {
      let tracks: Vec<sync::PlaylistTrackInfo> = children
          .iter()
          .filter(|c| is_downloadable_item_type(&c.item_type))
          .map(|c| sync::PlaylistTrackInfo {
              jellyfin_id: c.id.clone(),
              artist: c.album_artist.clone(),
              run_time_seconds: c.run_time_ticks
                  .map(|t| (t / 10_000_000) as i64)
                  .unwrap_or(-1),
          })
          .collect();
      playlist_sync_items.push(sync::PlaylistSyncItem {
          jellyfin_id: item.id.clone(),
          name: item.name.clone(),
          tracks,
      });
  }
  ```
  **IMPORTANT**: This runs in the same loop that expands children. The `children` variable is already fetched for `get_child_items_with_sizes`. Do NOT call Jellyfin again.

- [x] **T4.3**: After the loop, set `delta.playlists = playlist_sync_items` before returning the delta in the RPC response.

### T5: Implement `generate_m3u_files()` in sync.rs (AC: #3, #4, #6, #7, #8, #9, #11)

- [x] **T5.1**: Add a new private async function `generate_m3u_files` in `sync.rs`:

  ```rust
  /// Generates, regenerates, or cleans up .m3u files for playlists in the sync basket.
  ///
  /// Called once per sync run, after all file transfers complete.
  /// Uses Write-Temp-Rename (atomic write) for all .m3u writes.
  async fn generate_m3u_files(
      playlist_items: &[PlaylistSyncItem],
      device_path: &Path,
      all_synced_items: &[crate::device::SyncedItem],   // combined: manifest items + newly synced
      manifest: &mut crate::device::DeviceManifest,
  ) -> Vec<String> {   // returns list of warnings (e.g., missing tracks)
      let mut warnings: Vec<String> = Vec::new();

      // Build a lookup: jellyfin_id → local_path (relative to device_path)
      let path_lookup: HashMap<&str, &str> = all_synced_items
          .iter()
          .map(|i| (i.jellyfin_id.as_str(), i.local_path.as_str()))
          .collect();

      // Track which playlist jellyfin IDs are still active (for cleanup)
      let active_ids: HashSet<&str> = playlist_items
          .iter()
          .map(|p| p.jellyfin_id.as_str())
          .collect();

      // --- CLEANUP: remove .m3u for playlists no longer in basket ---
      let to_remove: Vec<PlaylistManifestEntry> = manifest
          .playlists
          .iter()
          .filter(|e| !active_ids.contains(e.jellyfin_id.as_str()))
          .cloned()
          .collect();
      for entry in &to_remove {
          let m3u_path = device_path.join(&entry.filename);
          if m3u_path.exists() {
              if let Err(e) = tokio::fs::remove_file(&m3u_path).await {
                  warnings.push(format!("[M3U] Failed to delete {}: {}", entry.filename, e));
              } else {
                  daemon_log!("[M3U] Deleted removed playlist: {}", entry.filename);
              }
          }
          manifest.playlists.retain(|e2| e2.jellyfin_id != entry.jellyfin_id);
      }

      // --- GENERATE / REGENERATE for each playlist in basket ---
      for playlist in playlist_items {
          let track_ids: Vec<String> = playlist.tracks.iter().map(|t| t.jellyfin_id.clone()).collect();

          // Determine if regeneration is needed
          let existing = manifest.playlists.iter().find(|e| e.jellyfin_id == playlist.jellyfin_id);
          let needs_write = match existing {
              None => true,
              Some(e) => e.track_ids != track_ids,
          };

          // Build .m3u filename
          let sanitized_name = sanitize_path_component(&playlist.name);
          let m3u_filename = truncate_filename(&sanitized_name, "m3u", 255);
          let m3u_path = device_path.join(&m3u_filename);

          if !needs_write {
              daemon_log!("[M3U] Playlist unchanged, skipping: {}", m3u_filename);
              continue;
          }

          // Build M3U content
          let mut lines: Vec<String> = vec!["#EXTM3U".to_string()];
          let mut included_count = 0u32;
          for track in &playlist.tracks {
              match path_lookup.get(track.jellyfin_id.as_str()) {
                  None => {
                      warnings.push(format!(
                          "[M3U] Track {} not in manifest — omitted from {}",
                          track.jellyfin_id, m3u_filename
                      ));
                  }
                  Some(rel_path) => {
                      // #EXTINF:<seconds>,<Artist> - <Name>  OR  #EXTINF:<seconds>,<Name>
                      // Note: 'name' is not in PlaylistTrackInfo (we use local_path's filename as display)
                      // We use artist only for the label; track title is embedded in the path
                      let label = match &track.artist {
                          Some(a) => format!("{} - {}", a, extract_display_name(rel_path)),
                          None => extract_display_name(rel_path).to_string(),
                      };
                      lines.push(format!("#EXTINF:{},{}", track.run_time_seconds, label));
                      // Convert path separators to forward slash for cross-platform DAP compat
                      lines.push(rel_path.replace('\\', "/"));
                      included_count += 1;
                  }
              }
          }

          if included_count == 0 {
              warnings.push(format!("[M3U] No tracks resolved for playlist {} — skipping write", playlist.name));
              continue;
          }

          let content = lines.join("\n") + "\n";

          // Write-Temp-Rename (atomic)
          let tmp_path = device_path.join(format!("{}.tmp", m3u_filename));
          match write_m3u_atomic(&tmp_path, &m3u_path, content.as_bytes()).await {
              Ok(()) => {
                  daemon_log!("[M3U] Wrote {}: {} tracks", m3u_filename, included_count);
                  let now = now_iso8601();
                  // Update manifest.playlists entry
                  manifest.playlists.retain(|e| e.jellyfin_id != playlist.jellyfin_id);
                  manifest.playlists.push(crate::device::PlaylistManifestEntry {
                      jellyfin_id: playlist.jellyfin_id.clone(),
                      filename: m3u_filename,
                      track_count: included_count,
                      track_ids,
                      last_modified: now,
                  });
              }
              Err(e) => {
                  warnings.push(format!("[M3U] Failed to write {}: {}", m3u_filename, e));
              }
          }
      }

      warnings
  }
  ```

- [x] **T5.2**: Add a helper `write_m3u_atomic` in `sync.rs`:
  ```rust
  async fn write_m3u_atomic(tmp_path: &Path, final_path: &Path, content: &[u8]) -> Result<()> {
      if let Some(parent) = tmp_path.parent() {
          tokio::fs::create_dir_all(parent).await?;
      }
      let mut file = tokio::fs::File::create(tmp_path).await?;
      file.write_all(content).await?;
      file.sync_all().await?;
      drop(file);
      tokio::fs::rename(tmp_path, final_path).await?;
      Ok(())
  }
  ```

- [x] **T5.3**: Add a helper `extract_display_name` in `sync.rs` (extracts filename stem for EXTINF label):
  ```rust
  fn extract_display_name(rel_path: &str) -> &str {
      let path = std::path::Path::new(rel_path);
      path.file_stem()
          .and_then(|s| s.to_str())
          .unwrap_or(rel_path)
  }
  ```

### T6: Call `generate_m3u_files()` at end of `execute_sync` (AC: #1, #6, #7, #8)

- [x] **T6.1**: In `execute_sync()`, after the deletes loop completes (final stage of the sync), add the M3U generation step:

  ```rust
  // --- M3U Playlist Generation ---
  if !delta.playlists.is_empty() {
      // Build combined view: existing synced items from manifest + newly synced items this run
      let device_guard = device_manager.get_device(&device_path).await;
      if let Some(manifest_guard) = device_guard {
          let mut current_manifest = manifest_guard.clone();

          // Merge newly synced items into manifest view (they may not be written yet if
          // manifest is only updated at the end; check current_manifest.synced_items)
          // The manifest is updated incrementally by execute_sync as each file completes —
          // so current_manifest.synced_items already contains all just-synced items.
          let all_synced = &current_manifest.synced_items;

          let warnings = generate_m3u_files(
              &delta.playlists,
              device_path,
              all_synced,
              &mut current_manifest,
          ).await;

          for w in &warnings {
              daemon_log!("{}", w);
          }

          // Persist updated manifest (playlists array updated)
          if let Err(e) = crate::device::write_manifest(device_path, &current_manifest).await {
              daemon_log!("[M3U] Failed to persist manifest after M3U update: {}", e);
          }
      }
  }
  ```

  **CRITICAL**: The manifest is already updated incrementally throughout `execute_sync` (dirty-resume). By the time M3U generation runs, `manifest.synced_items` contains all current items including newly synced ones. Do NOT reload the manifest from disk — use the in-memory version via `device_manager`.

- [x] **T6.2**: Verify that the `device_manager.get_device()` API returns a cloneable manifest. If the device manager uses `Arc<RwLock<HashMap<PathBuf, DeviceManifest>>>`, use a read lock here.

### T7: Tests (AC: all)

- [x] **T7.1**: In `sync.rs` tests block, add `test_generate_m3u_basic`:
  - Creates two `PlaylistSyncItem`s, each with 3 tracks
  - Creates `SyncedItem`s with `local_path = "Music/Artist/Album/01 - Name.flac"`
  - Calls `generate_m3u_files()` with a temp dir as `device_path`
  - Asserts `.m3u` files exist with correct content (starts with `#EXTM3U`, contains `#EXTINF`, relative paths with forward slashes)

- [x] **T7.2**: Add `test_generate_m3u_no_rewrite_if_unchanged`:
  - Calls `generate_m3u_files()` once, records file mtime
  - Calls again with same track_ids → verifies the file is NOT rewritten (same mtime)

- [x] **T7.3**: Add `test_generate_m3u_cleanup`:
  - Pre-populates `manifest.playlists` with a playlist entry + corresponding `.m3u` file in temp dir
  - Calls `generate_m3u_files()` with `playlist_items = []` (empty basket)
  - Asserts the `.m3u` file was deleted and manifest entry removed

- [x] **T7.4**: Add `test_generate_m3u_missing_track_omitted`:
  - Creates playlist with 3 tracks, but only 2 exist in `all_synced_items`
  - Asserts the `.m3u` contains 2 track entries and one warning is returned

- [x] **T7.5**: Run `cargo test` in `hifimule-daemon/` — all existing 151 tests pass.

## Dev Notes

### Architecture Compliance

- **Do NOT** add new Jellyfin API calls. All data (track list, duration) comes from `get_child_items_with_sizes()` already called during `sync.calculate_delta`. (Sprint change proposal constraint: "No additional API calls")
- **Do NOT** modify `calculate_delta()` function itself. The playlist data flows through `SyncDelta.playlists` (populated in the RPC handler, not in the delta algorithm).
- **Do NOT** modify `sync.execute` RPC handler params — `delta` already contains `playlists`. The handler passes it straight to `execute_sync`.
- **Additive only**: All new struct fields use `#[serde(default)]`. No existing serialized fields are changed. Zero breaking changes to `SyncOperation`, `DeviceManifest`, or any existing RPC response.
- **Do NOT** touch `calculate_delta()`, `write_file_streamed()`, `auto_fill.rs`, `transcoding.rs`, `scrobbler.rs`, or `db.rs`.

### Data Flow Summary

```
sync.calculate_delta RPC:
  basket items (includes Playlist type)
  → expand Playlist via get_child_items_with_sizes (already done)
  → capture PlaylistSyncItem { jellyfin_id, name, tracks: [{ jellyfin_id, artist, run_time_seconds }] }
  → set delta.playlists = [PlaylistSyncItem...]
  → return SyncDelta (including playlists field) to UI

sync.execute RPC:
  receives SyncDelta (playlists field round-trips from UI)
  → spawns execute_sync(delta, ...) in background
  → execute_sync processes adds/deletes/id_changes as before
  → at end: calls generate_m3u_files(delta.playlists, device_path, managed_path, manifest.synced_items, &mut manifest)
  → write_manifest() persists updated playlists array
```

### M3U Path Construction Detail

The `.m3u` is written to `managed_path` (the first entry of `manifest.managed_paths`, e.g. `device_path/Music`), NOT the device root. `managed_path` is resolved at runtime from `device_manager.get_current_device()`, with `"Music"` as a fallback if `managed_paths` is empty.

Track entries use `SyncedItem.local_path` (relative to `device_path`), stripped of the `managed_path` prefix so the path is relative to the `.m3u` file's location.

```
device_path/
  Music/                        ← managed_path (managed_paths[0])
    Mixes Playlist.m3u          ← written here
    Pink Floyd/
      The Wall/
        01 - In the Flesh.flac   ← local_path = "Music/Pink Floyd/The Wall/01 - In the Flesh.flac"
```

The M3U entry for the track above:
```
#EXTINF:210,Pink Floyd - 01 - In the Flesh
Pink Floyd/The Wall/01 - In the Flesh.flac
```

Strip the managed subfolder prefix with `device_path.join(rel_path).strip_prefix(managed_path)`, then normalize backslashes to forward slashes for DAP/Rockbox compatibility.

### PlaylistSyncItem Track Name Resolution

`PlaylistTrackInfo` does not store the track name — it uses `extract_display_name()` on `local_path` to derive the title for the `#EXTINF` label. This avoids duplicating the name in two places and uses the already-sanitized filename as ground truth.

**Example**: `local_path = "Music/Artist/Album/01 - Track Name.flac"` → `extract_display_name()` → `"01 - Track Name"`.

### `now_iso8601()` Already Exists

`now_iso8601()` is used in `execute_sync()` (line ~560 for `synced_at`). Reuse it for `last_modified`. No new import needed.

### `sanitize_path_component()` and `truncate_filename()` Are Private

These are defined in `sync.rs` and are not `pub`. Since `generate_m3u_files()` lives in the same file, they are accessible directly. No visibility change needed.

### Manifest Update Pattern

`generate_m3u_files()` mutates `&mut DeviceManifest` directly and then the caller (T6.1) calls `write_manifest()`. This is consistent with how execute_sync handles manifest updates:
- During file sync: `device_manager.update_operation()`
- After M3U: `device::write_manifest()` (Write-Temp-Rename, already used for manifests)

### Source Tree

**Files to MODIFY:**
1. [hifimule-daemon/src/api.rs](hifimule-daemon/src/api.rs) — T1: add `run_time_ticks` to `JellyfinItem`
2. [hifimule-daemon/src/sync.rs](hifimule-daemon/src/sync.rs) — T2: new structs + `playlists` on `SyncDelta`; T5: `generate_m3u_files()` + helpers; T6: call site at end of `execute_sync`; T7: tests
3. [hifimule-daemon/src/device/mod.rs](hifimule-daemon/src/device/mod.rs) — T3: `PlaylistManifestEntry` struct + `playlists` field on `DeviceManifest`
4. [hifimule-daemon/src/rpc.rs](hifimule-daemon/src/rpc.rs) — T4: capture playlist items in `sync.calculate_delta` handler, set `delta.playlists`

**Files NOT to touch:**
- `main.rs` (auto-sync path — playlists are still expanded to tracks for download; M3U generation is a sync.execute concern)
- `auto_fill.rs`, `transcoding.rs`, `scrobbler.rs`, `db.rs`
- Any UI file — no UI changes required for this story

### Critical RPC Contracts

```
// sync.calculate_delta response (MODIFIED: delta gains `playlists` field)
// {
//   adds: [...],
//   deletes: [...],
//   idChanges: [...],
//   unchanged: N,
//   playlists: [        ← NEW (empty array if no playlist basket items)
//     {
//       jellyfinId: "abc123",
//       name: "My Running Mixes",
//       tracks: [
//         { jellyfinId: "t1", artist: "Daft Punk", runTimeSeconds: 210 },
//         ...
//       ]
//     }
//   ]
// }

// sync.execute params: same SyncDelta (playlists round-trips from UI unchanged)
// All other RPCs: UNCHANGED
```

### Testing Standards

Following the existing pattern (Story 4.6 baseline: 151 tests in `hifimule-daemon/`):
- Co-located in `mod tests` block at bottom of `sync.rs`
- Use `tempfile::tempdir()` for file system tests (already a dev-dependency)
- `assert!`, `assert_eq!` for assertions
- No mock required — functions take concrete types
- Run `cargo test` in `hifimule-daemon/` after implementation

### Previous Story Learnings (4.6)

From Story 4.6 dev notes:
- **`Arc<AtomicU64>` pattern** is the established way to share mutable counters across async closures — M3U generation does not need this (no concurrent closures), but keep the pattern in mind for any future iteration
- **`#[serde(rename_all = "camelCase")]`** is the project-wide default for all RPC-facing structs — apply to `PlaylistSyncItem`, `PlaylistTrackInfo`, `PlaylistManifestEntry`
- **151 tests** pass in daemon — preserve this baseline
- **`device_manager.get_operation()` / `update_operation()`** pattern is used for progress state; the manifest state is a separate concern managed via `device_manager.get_device()` + `write_manifest()`

### Potential Gotchas

- **Playlist ordering**: `get_child_items_with_sizes` returns tracks in the order Jellyfin provides. For playlists, Jellyfin respects the user-defined order. Do NOT sort the tracks — preserve insertion order for M3U generation.
- **Playlist in auto-sync path**: `main.rs` also expands playlist items (for track downloads). The `generate_m3u_files()` call is added to `execute_sync` which is called from both paths — M3U generation happens automatically for auto-sync too. The auto-sync path calls `execute_sync` directly with `delta.playlists = vec![]` (because `main.rs` does not populate this field). Use the `if !delta.playlists.is_empty()` guard (T6.1) — no M3U generation if no playlist context passed.
- **Empty track list**: If a playlist has zero downloadable tracks, skip writing the `.m3u` (no point in an empty playlist file). Guard with `if included_count == 0 { continue; }` (already in T5.1).
- **`device_path` vs `managed_path`**: `.m3u` files go to `managed_path` (`manifest.managed_paths[0]`, e.g. `device_path/Music`), not the device root. Audio tracks also live under `managed_path`. Track paths in the `.m3u` are relative to `managed_path` (the managed subfolder prefix is stripped from `SyncedItem.local_path`). `managed_path` is read from `device_manager.get_current_device()` at the start of M3U generation, with `"Music"` as a fallback.
- **`write_m3u_atomic` tmp naming**: Use `format!("{}.tmp", m3u_filename)`. Since `m3u_filename` already ends in `.m3u`, this produces `"My Playlist.m3u.tmp"`.

### Source References

- Sprint change proposal: [_bmad-output/planning-artifacts/sprint-change-proposal-2026-03-28.md](_bmad-output/planning-artifacts/sprint-change-proposal-2026-03-28.md) §4.4
- Path sanitization: [hifimule-daemon/src/sync.rs](hifimule-daemon/src/sync.rs) lines 325–386 (`sanitize_path_component`, `truncate_component`, `truncate_filename`)
- Manifest write pattern: [hifimule-daemon/src/device/mod.rs](hifimule-daemon/src/device/mod.rs) `write_manifest()` lines 72–87
- Atomic file write: [hifimule-daemon/src/sync.rs](hifimule-daemon/src/sync.rs) `write_file_streamed()` lines 742–805
- Playlist expansion in RPC: [hifimule-daemon/src/rpc.rs](hifimule-daemon/src/rpc.rs) `sync.calculate_delta` handler lines 747–885
- `JellyfinItem` struct: [hifimule-daemon/src/api.rs](hifimule-daemon/src/api.rs) lines 64–94
- `DeviceManifest` struct: [hifimule-daemon/src/device/mod.rs](hifimule-daemon/src/device/mod.rs) lines 39–61
- `SyncDelta` struct: [hifimule-daemon/src/sync.rs](hifimule-daemon/src/sync.rs) lines 99–106
- `execute_sync` signature: [hifimule-daemon/src/sync.rs](hifimule-daemon/src/sync.rs) lines 394–427

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

_None_

### Completion Notes List

- Implemented full M3U playlist generation pipeline from scratch (no prior code existed).
- Added `run_time_ticks: Option<u64>` to `JellyfinItem` in api.rs; deserialized automatically via `PascalCase` rename.
- Added `PlaylistTrackInfo`, `PlaylistSyncItem` structs and `playlists: Vec<PlaylistSyncItem>` to `SyncDelta` with `#[serde(default)]` for backward compat.
- Added `PlaylistManifestEntry` struct and `playlists: Vec<PlaylistManifestEntry>` to `DeviceManifest` with `#[serde(default)]`.
- Modified `handle_sync_calculate_delta` in rpc.rs to capture `PlaylistSyncItem`s while expanding playlist children (no extra API calls).
- Implemented `generate_m3u_files()` with cleanup (AC #8), differential skip (AC #6), atomic write via `write_m3u_atomic()` (AC #9), missing-track omission (AC #11), Extended M3U format (AC #3), RunTimeTicks duration (AC #5). .m3u files written to `managed_path` (read from `manifest.managed_paths[0]`); track paths are relative to `managed_path` (managed subfolder prefix stripped from `SyncedItem.local_path`) (AC #4).
- Called `generate_m3u_files()` at end of `execute_sync()` behind `if !delta.playlists.is_empty()` guard.
- M3U generation uses `update_manifest` pattern to persist updated playlists array back through `DeviceManager`.
- All `DeviceManifest` struct literals in tests updated with `playlists: vec![]`; `JellyfinItem` literals updated with `run_time_ticks: None`.
- 155 tests pass (151 baseline + 4 new M3U tests: basic, no-rewrite, cleanup, missing-track).

### File List

- `hifimule-daemon/src/api.rs` — Added `run_time_ticks: Option<u64>` to `JellyfinItem`
- `hifimule-daemon/src/sync.rs` — Added `PlaylistTrackInfo`, `PlaylistSyncItem` structs; `playlists` field on `SyncDelta`; `generate_m3u_files()`, `write_m3u_atomic()`, `extract_display_name()` functions; M3U call in `execute_sync()`; 4 new tests
- `hifimule-daemon/src/device/mod.rs` — Added `PlaylistManifestEntry` struct; `playlists` field on `DeviceManifest`; `playlists: vec![]` in `initialize_device()`
- `hifimule-daemon/src/rpc.rs` — Modified `handle_sync_calculate_delta` to capture `PlaylistSyncItem`s; updated `DeviceManifest` test literals
- `hifimule-daemon/src/device/tests.rs` — Added `playlists: vec![]` to all `DeviceManifest` test literals
- `hifimule-daemon/src/auto_fill.rs` — Added `run_time_ticks: None` and `playlists: vec![]` to test literals
- `hifimule-daemon/src/tests.rs` — Added `playlists: vec![]` to `DeviceManifest` test literals

## Change Log

- 2026-04-01: Story created by SM (create-story workflow). Source: sprint-change-proposal-2026-03-28.md §4.4. No prior M3U code existed in codebase — full new implementation.
- 2026-04-01: Implemented by dev agent (claude-sonnet-4-6). All 11 ACs satisfied. 155 tests pass. Status → review.
