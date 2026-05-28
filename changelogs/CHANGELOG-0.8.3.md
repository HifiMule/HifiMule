# HifiMule 0.8.3

Release date: 2026-05-28

## Highlights

- **Correct track-number prefixes in synced filenames**: When syncing via Navidrome, Subsonic, or Jellyfin without transcoding, every output filename was prefixed `00 - <title>` regardless of the actual track number. Filenames now use the real track number from the server (e.g. `03 - Title.flac`), and only fall back to `00` when no track number is available.
- **Basket items show track count and file size**: Albums, playlists, and other collections added to the basket now display their approximate track count and total file size. If that data isn't already on the card, HifiMule fetches it from the server the moment you add the item — a spinner on the card image indicates the fetch is in progress.

---

## Bug Fixes

### Provider sync track-number prefix always `"00 -"` (spec-fix-provider-sync-track-number-prefix)

The `construct_desired_file_path` function hardcoded `"00"` as the track-number prefix for every filename produced by the provider sync path (Navidrome/Subsonic, and Jellyfin without transcoding). The Jellyfin direct-download path was already reading `index_number` correctly via a separate function and was not affected.

**Fix:** A `track_number: Option<u32>` field was added to both `DesiredItem` and `SyncAddItem` (both with `#[serde(default)]` for backward manifest compatibility). The field is populated from `Song.track_number` wherever `SyncAddItem` is constructed from a `DesiredItem` — including the missing-file recovery path and the initial adds path. `construct_desired_file_path` now formats the prefix as a zero-padded two-digit number when the field is `Some`, and keeps the `"00"` fallback when it is `None`.

Track number mapping:
- `Some(3)` → `"03 - Title.flac"`
- `Some(12)` → `"12 - Title.flac"`
- `None` → `"00 - Title.flac"` (unchanged behaviour)

Force-add relocations re-constructed from `SyncedItem` entries (which do not carry track-number data) correctly fall through to the `"00"` fallback.

---

## Improvements

### Basket item description — lazy count and size loading

Previously, basket item descriptions showed a static track count and type label only when the card already carried that metadata. Collections for which the card hadn't pre-loaded counts (common for albums and playlists reached via browse) showed `0 tracks`.

**Change:** When a container type (`MusicArtist`, `MusicAlbum`, `MusicGenre`, `Playlist`) is added to the basket and either `childCount` or `sizeBytes` is missing, `MediaCard` now fires two parallel RPC calls — `jellyfin_get_item_counts` and `jellyfin_get_item_sizes` — before dispatching the basket `add` action. A loading spinner is shown on the card image during the fetch and cleared when it resolves. Favorite-scoped types (`FavoriteArtist`, `FavoriteAlbum`) skip the fetch because their counts are resolved at sync time rather than browse time.

The basket sidebar description was unified under a single `basket.item.meta` i18n key (`{label} - ~{count} tracks - ~{size}`) that covers all container types. A new `itemTypeLabel()` helper maps internal type strings to localised display names (Album, Playlist, Favorites). The key and its display-name companions (`basket.item.type.album`, `basket.item.type.playlist`, `basket.item.type.favorites`) are added in English, French, and Spanish.

---

## Internal / Test

- `DesiredItem` and `SyncAddItem` gain `track_number: Option<u32>` with `#[serde(default)]`.
- All test-only `DesiredItem` and `SyncAddItem` constructions updated with `track_number: None`.
- New `device/tests.rs` entries and `device/mod.rs` re-export to cover the track-number propagation path.
- New i18n keys in `catalog.json`: `basket.item.meta`, `basket.item.type.album`, `basket.item.type.playlist`, `basket.item.type.favorites` (EN/FR/ES).
