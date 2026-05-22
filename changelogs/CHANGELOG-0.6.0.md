# HifiMule 0.6.0

Release date: 2026-05-22

## Highlights

- **Rich Library Navigation**: eight browse modes are now available — Artists, Albums, Playlists, Genres, Recently Added, Frequently Played, Recently Played, and Favorites. A new tab bar at the top of the Library Browser switches between them; only modes the connected server actually supports are shown.
- **Genre as a Basket Entity**: an entire genre can be added to the sync basket in one click. HifiMule resolves the full genre track list at sync time, and duplicate tracks shared with other basket items are removed automatically.
- **History and Favorites modes**: Recently Added surfaces new albums first; Frequently Played ranks tracks by server play count; Recently Played sorts by last-listened date; Favorites shows your starred tracks. Track cards in these modes include the relevant metadata in their subtitle (play count or last-played date).
- **Faster Genre List on Jellyfin**: the Genres screen now loads in a single request with no sequential art-fetching round-trips.

---

## New Features

### Browse Mode Navigation (Story 9.2)

The Library Browser gains a compact tab/segmented control that lists every browse mode the active server supports. Switching modes refreshes the library content while leaving the current basket untouched. Breadcrumb navigation continues to work within each mode, and scroll position and page cache are restored per mode when navigating back.

Supported browse modes:
- **Artists** — alphabetical artist grid with album counts
- **Albums** — full library album grid
- **Playlists** — server playlists
- **Genres** — genre grid with cover art and track counts
- **Recently Added** — newest albums first
- **Frequently Played** — tracks ranked by server play count (count shown in card subtitle)
- **Recently Played** — tracks sorted by last-listened date (date shown in card subtitle)
- **Favorites** — starred tracks

Jellyfin exposes all eight modes. Subsonic/OpenSubsonic exposes Artists, Albums, Playlists, and Genres.

### Genre Basket Entity (Story 9.3)

Genre cards in the library now have a basket-add toggle, matching Artist and Album cards. Adding a genre places a single `MusicGenre` item in the basket; the sidebar shows it as "Genre · ~N tracks · ~X MB". At sync time the daemon fetches the current track list for that genre from the server so the device always receives the latest matching songs.

Duplicate tracks that appear in both a genre basket item and another item (album, artist, individual track) are removed during sync planning via the existing deduplication pass — no extra configuration required.

### History and Favorites Browse Modes (Story 9.4)

Four flat-track browse modes are now wired end-to-end:

| Mode | Server source | Card subtitle |
|------|--------------|---------------|
| Recently Added | Jellyfin `DateCreated` / Subsonic `getNowPlaying` | — |
| Frequently Played | Jellyfin `UserData.PlayCount` / Subsonic `getNowPlaying` | "Artist — Album · 42 plays" |
| Recently Played | Jellyfin `UserData.LastPlayedDate` | "Artist — Album · May 1" |
| Favorites | Jellyfin `UserData.IsFavorite` / Subsonic `getStarred` | "Artist — Album" |

All four modes paginate correctly; "Load More" fetches additional pages from the server.

---

## Improvements

### Provider Capability Contract (Story 9.1)

The daemon now declares browse capabilities per provider type via a structured contract. Browse mode methods (`list_genres`, `get_genre_tracks`, `list_recently_added`, `list_frequently_played`, `list_recently_played`, `list_favorites`) are explicit typed methods on the `MediaProvider` trait. Unsupported modes return a well-defined `UnsupportedCapability` error rather than a silent empty list. No provider-specific URL construction happens outside the provider layer.

### Faster Jellyfin Genre Loading

- Switched from the generic `/Genres` endpoint to `/MusicGenres`, which returns cover art IDs in the same response. This eliminates the entire sequential art-enrichment pass that previously made the Genres screen slow for large libraries.
- Genre list is now paginated server-side so art enrichment is bounded by page size rather than the full library.
- Remaining per-page art futures are resolved in a single parallel `join_all` instead of serial batches.
- Track counts are now populated from the `SongCount` field returned by `/MusicGenres`, with `RecursiveItemCount` as a fallback.

### Build and CI Reliability

- Reduced Tauri build thread count to prevent OOM failures on GitHub Actions runners.
- Added a macOS library bundling script (`scripts/bundle-macos-libs.mjs`) so dynamic libraries are correctly packaged in release builds without manual steps.
- Unified the sidecar preparation script between local and CI builds.
- Cleaned up `.gitignore` to exclude build artifacts that were accidentally tracked.

### UI Polish

- Improved card layout proportions and spacing across all browse-mode grids.

---

## Bug Fixes

- **Genre pagination not respected**: the genre list handler previously ignored `startIndex`/`limit` and enriched art for every genre before responding. It now slices to the requested page first.
- **Pagination offset field mismatch**: the daemon `browse_pagination` helper was reading `offset` instead of `startIndex` from RPC params, so "Load More" always returned the first page. Fixed to read `startIndex`, matching the TypeScript RPC callers.
- **Genre track counts missing on Jellyfin**: `/MusicGenres` requires `Fields=RecursiveItemCount` in the query to include track counts. The field is now requested explicitly.
- **Tracing logs silently dropped in daemon**: genre timing logs used `tracing::debug!` but no tracing subscriber is initialized. Switched to `daemon_log!` so output goes to stdout and `HifiMule/daemon.log`.

---

## Commits Included

- `bc9c37f` — Fix music genre for jellyfin
- `ec268c1` — fix: request Fields=RecursiveItemCount to populate genre track count
- `08797f2` — fix: populate genre trackCount from SongCount field on /MusicGenres
- `8f2c0f0` — perf: use /MusicGenres endpoint — eliminates art enrichment entirely
- `ee995d3` — fix: switch genre timing logs to daemon_log!
- `3b372cb` — debug: add timing logs to handle_browse_list_genres
- `c5372c7` — perf: run all genre art enrichment futures in a single join_all
- `37e5176` — fix: paginate genre list handler to stop blocking on full-library art enrichment
- `db3b8f0` — Improve card layout
- `485a731` — Review 9.4
- `02d4524` — Dev 9.4
- `4fe9dc6` — Story 9.4
- `d97e648` — Review 9.3
- `4d07b5a` — Dev 9.3
- `7494be0` — Story 9.3
- `48201c8` — Review 9.2
- `d89ed20` — Dev 9.2
- `4f8ff9b` — Story 9.2
- `2c415ee` — Review 9.1
- `0bf4334` — Dev 9.1
- `e80acc5` — Story 9.1
- `45b3659` — Correct course for media browser
- `df4182a` — Reduce thread for tauri and consistent build for tauri and github action
- `b18822d` — Reduce thread for tauri and consistent build for tauri and github action
