# HifiMule 0.16.0

Release date: 2026-09-27

## Highlights

- **Audiobookshelf support**: Connect individual Books and Podcasts libraries as separate servers, then browse, search, play, and sync their media from HifiMule.
- **Long-form listening**: Play ordered audiobook parts and podcast episodes, keep audiobook progress with the source, and seek within supported direct MP3 and AAC/MP4 streams after runtime checks.
- **Audiobook and podcast browsing**: Explore Books by author, series, or collection, and find unfinished podcast episodes in a Recent Episodes tab.
- **Device-ready media**: Add books, authors, podcast shows, or episodes to the basket; use separate audiobook and podcast folders and per-server Auto-Fill policies.
- **Better library search and playback**: Search music libraries across artists, albums, and tracks, and start playback from playlists and audiobook groupings.

---

## Added

### Audiobookshelf connection and libraries

- Add an Audiobookshelf account, choose one accessible Books or Podcasts library, and keep each selected library as its own Server Hub entry. Multiple libraries from the same account can coexist.
- Browse Books and their ordered audio parts, with authors, narrators, artwork, and book details mapped from the selected library. Separate Authors, Series, and Collections tabs keep author selection distinct from read-only source groupings.
- Browse podcast shows and episodes in a Podcasts library. The Recent Episodes tab lists newly published unfinished episodes with paging and the existing play and basket actions.
- Search Audiobookshelf Books and Podcasts libraries using their role-specific results.

### Audiobookshelf playback

- Play a complete book in part order or a podcast episode through the desktop player. Playlists, audiobook series, and collections can also start playback in their source order.
- Keep audiobook listening progress tied to the whole book and write it back to Audiobookshelf from the player, independently of device sync.
- Seek within a book part or podcast episode when its authenticated direct stream, duration, container, and decoder pass the playback checks. Unsupported or changed representations keep ordinary playback available without advertising seek.

### Device sync and Auto-Fill

- Add books, individual book parts, authors, podcast shows, and episodes to the device basket. Author selections expand to the author's eligible books, including coauthored books, without creating a playlist file.
- Set separate managed device folders for audiobooks and podcasts, with the music folder as the default when a role-specific folder is unset.
- Configure per-server audiobook and podcast Auto-Fill, including podcast show selection and recent-episode retention. Auto-sync applies the selected server's saved policy.
- See profile-aware compatibility and block reasons in the sync preview for Audiobookshelf media; uncertain browse-time compatibility remains marked as unknown.

### Music library search

- Search Jellyfin and Subsonic-compatible music libraries from a dedicated Search tab with grouped artist, album, and track results.

---

## Changed

- Audiobookshelf series and collections are browsable and playable in source order but remain read-only in HifiMule.
- Podcast files written to a device include show and publication-date context when available.
- Staged audio now moves to mass-storage and MTP devices through bounded-memory transfers, so syncing a large book or episode no longer requires loading the whole file into daemon memory.

---

## Fixed

- Audiobookshelf book and podcast search now accepts the server's nonempty result shapes, and podcast artwork and recent-episode paging render correctly.
- Audiobook and podcast Auto-Fill selections and episode dates now resolve correctly; automatic sync uses the selected server's saved Auto-Fill settings.
- Seeking in a large file no longer crashes the player.
- Reopening HifiMule from the daemon tray reuses the existing UI, and macOS can reopen the UI after its window exits.
- The refresh button layout no longer shifts unexpectedly.

---

## Internal

- Added library-scoped Audiobookshelf identity, daemon-owned authentication and refresh, role-specific provider models, and offline contract coverage for the validated Audiobookshelf v2.36.1 responses.
- Added guarded book-progress persistence, direct-media admission, source-scoped podcast RPCs, and tests for identity, grouping, search, paging, playback, sync, and compatibility.
- Extended device manifests and sync planning with media roles, managed destination paths, and path-backed writes for mass-storage, Windows WPD, and Unix libmtp devices.
- Expanded localization, architecture and implementation documentation, lifecycle contracts, and platform smoke checks for the new provider and UI reopening behavior.
