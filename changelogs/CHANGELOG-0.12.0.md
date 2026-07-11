# HifiMule 0.12.0

Release date: 2026-07-11

## Highlights

- **Configurable Auto-Fill**: Auto-fill is now a real builder instead of a single on/off filler. You can choose sources, filters, size or duration goals, ordering, memory rules, discovery options, quality preferences, context rules, and album/artist promotion.
- **Smarter discovery**: Auto-fill can rotate through source tiers, avoid recently synced or played tracks, preserve a stable core, surface rarer or less-played music, and guarantee discovery after dry streaks.
- **Better sync behavior**: Auto-fill now works with playlist-based auto-sync, writes a device playlist named `Autofill`, and fixes several cases where sync controls or playlist-only work behaved incorrectly.
- **Device and Jellyfin fixes**: Windows devices prefer WPD over MTP where available, Jellyfin library auto-fill no longer sees an empty pool, and the UI no longer asks for login when the daemon already has usable server credentials.

---

## Added

### Configurable Auto-Fill pipeline

- New per-device, per-server Auto-Fill pipeline stored in the device manifest.
- Auto-Fill configuration UI with a simple path for enabling fill and setting a budget, plus advanced controls for:
  - included sources: library, favorites, listening history, and playlists,
  - source shares and fallback sources,
  - track, album, or artist fill units,
  - ordering by favorites, play count, date added, random, quality, rediscovery, excavation, and rarity,
  - genre exclusions,
  - max size, duration target, reserved headroom, and encoding-from-goals,
  - cooldown, played exclusion, stable core, repeat tolerance, and rotation tiers,
  - best-version selection and preferred versions such as studio, live, remaster, remix, acoustic, and demo,
  - rarity weighting and pity discovery,
  - time/month/date context rules,
  - artist spotlight, album/track ratio, favorite-album promotion, and coherent artist/album ordering.
- Auto-Fill preview support using the same pipeline contract as saved sync configuration.
- Auto-Fill state and RPC contract mirrored in the UI so saved settings round-trip cleanly.
- Machine-local Auto-Fill history, rotation, and pity counters used during sync without storing those counters on the device manifest.

### Auto-Fill playlists

- Sync now writes a synthetic `Autofill` playlist containing the tracks selected by Auto-Fill.
- The playlist is de-duplicated and preserves the selected order.
- Multi-server sync writes Auto-Fill playlists after all provider groups copy their files.
- Legacy Jellyfin Auto-Fill and provider Auto-Fill paths now emit the same playlist.

### Auto-sync improvements

- Auto-sync with manual playlist or basket content can now fill remaining capacity with Auto-Fill.
- Playlist-only auto-sync work is treated as real sync work, even when no audio files need copying.

---

## Changed

- Auto-Fill's original simple behavior is preserved as a fast path when the pipeline is equivalent to the old default.
- Auto-Fill selection now avoids manually selected tracks so filler does not duplicate basket choices.
- Sync-time budget resolution now accounts for configured headroom and duration targets.
- Auto-Fill settings are scoped by portable server id, so the same device can keep separate fill rules for different servers.
- The app design was polished: the status bar was folded away, basket/config interactions were tightened, and the UI styling was cleaned up.
- Frontend build settings were optimized.
- Documentation was expanded with an Auto-Fill deep dive and refreshed architecture/API/data-model references.

---

## Fixed

- Jellyfin Auto-Fill library sources now work: Jellyfin implements `list_all_songs_page`, so the library pool no longer falls back to `UnsupportedCapability`.
- Auto-sync playlist runs now include Auto-Fill instead of syncing only the playlist/basket items.
- The sync stop button now stops running work correctly.
- Starting the daemon from the UI no longer asks for login when stored credentials are already enough.
- Opening Auto-Fill configuration no longer needs two clicks.
- If a sync already exists, the UI displays that existing sync instead of failing.
- Windows device detection now favors WPD over MTP where available.

---

## Internal

- `auto_fill.rs` was split into `auto_fill/mod.rs`, `auto_fill/fetch.rs`, and `auto_fill/pipeline.rs`.
- The pipeline engine is pure and unit-tested; provider I/O and history/counter threading stay outside it.
- Daemon DB schema now includes Auto-Fill history, rotation, and pity tables.
- New and updated tests cover pipeline stages, device manifest round-trips, Jellyfin `list_all_songs_page`, Auto-Fill playlist generation, playlist-only auto-sync work, sync stopping, existing-sync display, and WPD preference.
- i18n catalog expanded for the Auto-Fill builder and related sync messages.
