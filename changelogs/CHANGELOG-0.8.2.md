# HifiMule 0.8.2

Release date: 2026-05-26

## Highlights

- **Quality-aware re-sync**: HifiMule now tracks the bitrate of every file it writes to your device. On the next sync it compares the recorded bitrate against what the server currently reports, and automatically re-downloads tracks whose quality has been upgraded. Files synced before 0.8.2 will be re-synced once (to populate the bitrate record), then only re-downloaded again when the server actually has a better version.
- **Missing file recovery**: If you manually delete a synced file from the device, the sync preview now catches it and promotes it from "unchanged" back into the download queue — no need to remove and re-add the track to the basket.
- **Force Sync**: A new "Force Sync" option in the sync button dropdown re-downloads every file currently on the device, bypassing the delta entirely. Useful after a profile change, a storage corruption, or whenever you want a clean slate.
- **M4A/AAC bitrate fix**: Jellyfin often leaves the container-level `Bitrate` field null for M4A files and only exposes the true bitrate inside `MediaStreams`. HifiMule now falls back to the audio stream's `BitRate` value, so M4A tracks are correctly included in the quality-upgrade check.
- **macOS non-ASCII filename fix**: Files with accented or non-Latin characters (e.g. `Ångström.flac`) could be incorrectly flagged as missing on macOS because the OS returns filenames in a different Unicode normalization form (NFD) than what was written (NFC). The missing-file detection now uses a direct filesystem `metadata` lookup on macOS mass-storage devices, which is normalization-insensitive, instead of comparing paths from a directory listing.
- **Album A–Z nav bar disappears on back-navigate**: Returning to the Albums tab after visiting another tab caused the letter navigation bar to vanish. A wrong field assignment in the cache-restore path (`artistViewTotal` instead of `albumViewTotal`) reset the total to 0, hiding the bar.
- **Load More button missing when a letter filter is active**: Selecting a letter on the Albums or Artists tab caused the "Load More" button to disappear entirely, making it impossible to page through results beyond the initial 200. The button now correctly appears for letter-filtered views, and clicking it fetches and appends the next page without clearing the active letter or the nav bar.

---

## New Features

### Quality-Aware Re-Sync (Story 4.11)

The device manifest now records `original_bitrate` and `original_container` alongside each synced file. These fields are written at sync time and used on the next delta calculation to detect quality upgrades.

Upgrade detection logic:
- `server_bitrate > recorded_bitrate` → re-sync (quality improved)
- `server_bitrate present, no local record` → re-sync once (old manifest entry; populates the record)
- `server_bitrate absent` → skip check (no data to compare; leave unchanged)

ID-change events (Jellyfin item-ID migrations) intentionally write `original_bitrate: None` so the next sync re-evaluates quality for the renamed track.

The `original_bitrate` field is `Option<u32>` with `#[serde(default)]` — manifests from older versions deserialize without error.

### Missing File Recovery (Story 4.12 — existence check)

`sync_calculate_delta` now calls `augment_delta_with_existence_check` after the standard delta calculation. It iterates manifest entries whose status would otherwise be "unchanged", checks whether the file is actually present on the device, and promotes any missing files into `delta.adds`. The sync preview reflects these recovered items before the user confirms the sync.

This check runs only in the UI preview path, not inside auto-sync.

### Force Sync (Story 4.12 — force mode)

The sync button in the basket sidebar is now a split-button group. The primary button starts a normal sync; the "Force Sync" item in the dropdown sets a one-shot `forceSyncMode` flag.

When `force: true` is passed to `sync_execute`, the daemon rebuilds the delta before executing: every file currently in the manifest is added to both `delta.deletes` and `delta.adds`, `id_changes` is cleared, and `unchanged` is reset to 0. This causes all synced files to be deleted and re-downloaded in a single pass.

New i18n key `basket.actions.force_sync` added for English, French, and Spanish.

### M4A/AAC Bitrate Fix (spec-fix-m4a-jellyfin-bitrate)

Jellyfin sets `MediaSource.Bitrate` to `null` for many M4A/AAC files and instead reports the audio bitrate in `MediaSource.MediaStreams[].BitRate` (stream type `"Audio"`). A new `MediaStream` struct was added to `api.rs`, wired into `MediaSource.media_streams`, and all three bitrate-extraction sites (`jellyfin_item_to_desired_item` in `rpc.rs`, its inline duplicate in `handle_sync_calculate_delta`, and `to_desired_item` in `main.rs`) now fall back to the audio stream value when the container-level field is absent.

Precedence: container `Bitrate` → audio `MediaStream.BitRate` → item-level `Bitrate` → `None`.

Three regression tests cover the edge cases:
- Fallback to audio stream when container bitrate is null
- Container bitrate takes precedence when both are present
- Returns `None` when only non-audio streams are present

---

## Bug Fixes

- **M4A tracks always recorded `originalBitrate: null`** — the quality-upgrade check never fired for AAC/M4A files because Jellyfin omits the container-level bitrate for these formats. Fixed by reading the audio stream's `BitRate` as a fallback.
- **Non-ASCII filenames incorrectly treated as missing on macOS** — `device_file_exists` used `list_files` and compared paths as strings. On macOS, HFS+/APFS returns filenames in NFD normalization while the path written to the manifest may be NFC, causing the comparison to fail for any filename containing diacritics or non-Latin characters. Fixed by adding a `file_exists` method to the `DeviceIO` trait; `MscBackend` overrides the default with a `tokio::fs::metadata` call that is normalization-insensitive.
- **Album A–Z nav bar vanishes after navigating away and back** — the cache-restore branch in `loadAlbums()` wrote to `state.artistViewTotal` instead of `state.albumViewTotal`, resetting the album count to 0 and suppressing the letter bar. Fixed by assigning `state.albumViewTotal = cached.total`.
- **"Load More" button hidden while a letter filter is active** — `renderGrid()` had an `activeLetter === null` guard on the load-more condition, so the button was never rendered when browsing by letter. Guard removed; a new `appendByLetter()` helper fetches subsequent pages for the active letter and appends them without resetting the letter selection or the nav bar. Both `loadArtistsByLetter()` and `loadAlbumsByLetter()` now record `pagination.total` and `pagination.startIndex` after the initial 200-item fetch so `loadMore()` knows the correct offset.

---

## Internal / Test

- `SyncedItem`, `DesiredItem`, `SyncAddItem` all gain `original_bitrate: Option<u32>` (and `SyncedItem` additionally `original_container: Option<String>`), all with `#[serde(default)]`.
- All test-only struct constructions updated with `original_bitrate: None` / `original_container: None`.
- New public `sync::augment_delta_with_existence_check` helper.
- `handle_sync_execute` in `rpc.rs` now accepts a `force: bool` parameter.
- New `file_exists(&self, path: &str) -> bool` method on the `DeviceIO` trait; default implementation uses `list_files`; `MscBackend` overrides with a direct `tokio::fs::metadata` lookup.
- New `appendByLetter(mode, letter)` async function in `library.ts` handles paginated appends for letter-filtered artist/album views; `loadMore()` routes to it when `state.activeLetter` is set and derives `startIndex` from `state.items.length` instead of incrementing by limit.
