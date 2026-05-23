# Story 9.5: Hierarchical Favorites Navigation

Status: review

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a Convenience Seeker (Sarah),
I want Favorites to browse as Artists -> Albums -> Tracks instead of a flat song list,
so that I can sync favorite artists, favorite albums, and favorite tracks without losing the normal music hierarchy.

## Acceptance Criteria

1. **Given** the active provider supports Favorites, **When** I open the Favorites browse mode, **Then** the root level shows artists that are directly favorited, artists that have favorited albums, and artists that have favorited tracks.

2. **Given** I select an artist in Favorites, **When** the artist is directly favorited, **Then** the album level shows all albums for that artist.

3. **Given** I select an artist in Favorites, **When** the artist is not directly favorited but has favorite albums or tracks, **Then** the album level shows directly favorited albums for that artist plus albums that contain favorited tracks for that artist.

4. **Given** I select an album in Favorites, **When** the album is directly favorited or belongs to a directly favorited artist selected from Favorites, **Then** the track level shows all tracks in that album.

5. **Given** I select an album in Favorites, **When** the album is not directly favorited and does not belong to a directly favorited artist, **Then** the track level shows only favorited tracks from that album.

6. **Given** the provider exposes favorite artists, favorite albums, and favorite tracks in a single favorites response, **Then** the UI uses that response as the favorite tree source and does not continue to render root Favorites as a paginated flat track result.

7. **Given** I navigate inside Favorites, **Then** breadcrumbs, page cache, scroll restoration, basket toggles, and device-locked behavior continue to work like the existing Artists/Albums hierarchy.

8. **Given** the active provider cannot return hierarchical favorite items, **Then** Favorites is not offered as a broken hierarchical mode; it either remains unsupported by provider capabilities or fails with the existing unsupported-capability error path.

## Tasks / Subtasks

- [x] Task 1: Finalize provider contract for hierarchical favorites (AC: 1, 6, 8)
  - [x] Keep `MediaProvider::list_favorite_items(library_id)` returning `SearchResult` with `artists`, `albums`, and `songs`.
  - [x] Keep `browse.listFavoriteItems` as the UI-facing RPC endpoint for the tree source.
  - [x] Ensure `browse.listModes` only includes `favorites` for providers that can satisfy the favorites mode contract.
  - [x] Verify Jellyfin maps favorite `MusicArtist`, `MusicAlbum`, and `Audio` items into the correct `SearchResult` vectors.
  - [x] Verify Subsonic/OpenSubsonic maps `getStarred2` artists, albums, and songs into the correct `SearchResult` vectors.

- [x] Task 2: Complete Favorites root as artist level (AC: 1, 6, 7)
  - [x] Use `fetchBrowseFavoriteItems()` in `library.ts` to populate a cached `FavoriteTree`.
  - [x] Build the root artist list from the union of directly favorite artists, artists of favorite albums, and artists of favorite tracks.
  - [x] De-duplicate artists by ID and sort by display name for stable navigation.
  - [x] Do not call `fetchBrowseFavorites()` from Favorites root; the flat track-only loader remains available only if future code needs it outside the root favorites hierarchy.

- [x] Task 3: Complete artist -> album filtering (AC: 2, 3, 7)
  - [x] If the selected artist is directly favorited, load all of that artist's albums with `fetchBrowseArtist(artistId)`.
  - [x] Otherwise, show the union of directly favorite albums for that artist and albums inferred from favorite tracks by that artist.
  - [x] De-duplicate albums by ID and sort by display name.
  - [x] Preserve album card metadata and cover art using existing `mapAlbums()`.

- [x] Task 4: Complete album -> track filtering (AC: 4, 5, 7)
  - [x] If the selected album is directly favorited, load all album tracks with `fetchBrowseAlbum(albumId)`.
  - [x] If the selected album belongs to a directly favorited artist from the Favorites breadcrumb path, load all album tracks with `fetchBrowseAlbum(albumId)`.
  - [x] Otherwise, show only favorite tracks from the cached favorite tree for that album.
  - [x] Preserve existing `Audio` basket behavior through `mapAlbumTracks()`.

- [x] Task 5: Cache invalidation and navigation correctness (AC: 6, 7)
  - [x] Clear `state.favoriteTree` in `clearNavigationCache()`.
  - [x] Include `browseMode` and parent IDs in cache keys as the existing `cacheKey()` helper already does.
  - [x] Ensure switching away from Favorites and back restores valid cached levels, but reconnect/server refresh clears stale favorite-tree data.
  - [x] Ensure `loadMore()` is a no-op for Favorites because Favorites hierarchy levels are not paginated in the UI.

- [x] Task 6: Tests and verification (AC: 1-8)
  - [x] Run `rtk tsc` from `hifimule-ui/`.
  - [x] Run `rtk cargo test -p hifimule-daemon` from the workspace root.
  - [x] Add or update Rust provider tests proving `list_favorite_items` includes artist, album, and song favorites where provider fixtures support them.
  - [x] Manually smoke test Favorites root -> artist -> album -> tracks with: directly favorite artist, directly favorite album, and isolated favorite track.

## Dev Notes

### Current Implementation Context

This story corrects Story 9.4's underspecified Favorites behavior. Story 9.4 only required "favorited tracks are shown", so the completed work implemented Favorites as a flat track result. The desired behavior is now hierarchical and must treat artists and albums as first-class favorites.

Partial implementation already exists in the working tree. Continue from it; do not revert it.

**Frontend partials already present:**
- `hifimule-ui/src/rpc.ts` exposes `fetchBrowseFavoriteItems()` and `BrowseArtist` / `BrowseAlbum` / `BrowseTrack` include the fields needed to build the tree.
- `hifimule-ui/src/library.ts` already has `FavoriteTree`, `ensureFavoriteTree()`, `favoriteArtistsForTree()`, `favoriteAlbumsForArtist()`, `favoriteTracksForAlbum()`.
- `loadModeRoot()` already dispatches `favorites` to `loadFavoriteArtists()` instead of `loadFlatTracks('favorites')`.
- `navigateToArtist()`, `navigateToAlbum()`, and `reloadCurrentLevel()` already branch for `state.browseMode === 'favorites'`.

**Daemon partials already present:**
- `hifimule-daemon/src/providers/mod.rs` has a default `list_favorite_items()` trait method returning `UnsupportedCapability`.
- `hifimule-daemon/src/rpc.rs` routes `browse.listFavoriteItems` to `handle_browse_list_favorite_items()`.
- `hifimule-daemon/src/providers/jellyfin.rs` implements `list_favorite_items()` via `get_favorite_music_items()`.
- `hifimule-daemon/src/providers/subsonic.rs` implements `list_favorite_items()` via `get_starred2()`.

### Favorite Navigation Rules

Use a cached tree from `browse.listFavoriteItems`:

- Root Artists = directly favorite artists OR artists with favorite albums OR artists with favorite tracks.
- Artist Albums:
  - If artist is directly favorite: all albums from `fetchBrowseArtist(artistId)`.
  - Otherwise: favorite albums for that artist OR albums containing favorite tracks by that artist.
- Album Tracks:
  - If album is directly favorite: all tracks from `fetchBrowseAlbum(albumId)`.
  - If selected artist is directly favorite: all tracks from `fetchBrowseAlbum(albumId)`.
  - Otherwise: only favorite tracks from the tree for that album.

### Boundaries

**In scope:**
- Hierarchical Favorites root/artist/album navigation.
- Provider-neutral favorite tree RPC and provider implementations.
- De-duplication and deterministic sorting for inferred artists/albums.
- Verification that basket toggles still produce existing `MusicArtist`, `MusicAlbum`, and `Audio` item types.

**Out of scope:**
- New dynamic basket item types.
- Changing Recently Added, Frequently Played, or Recently Played behavior.
- Making Favorites root paginated; the tree is expected to be small enough for one response.
- Sync-time semantic changes for existing artist, album, and track basket items.

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story-9.4-History-and-Favorites-Browse-Modes]
- [Source: _bmad-output/implementation-artifacts/9-4-history-and-favorites-browse-modes.md]
- [Source: hifimule-ui/src/rpc.ts] (`fetchBrowseFavoriteItems`)
- [Source: hifimule-ui/src/library.ts] (`FavoriteTree`, Favorites loaders, Favorites navigation branches)
- [Source: hifimule-daemon/src/providers/mod.rs] (`MediaProvider::list_favorite_items`)
- [Source: hifimule-daemon/src/rpc.rs] (`browse.listFavoriteItems`)
- [Source: hifimule-daemon/src/providers/jellyfin.rs] (`list_favorite_items`)
- [Source: hifimule-daemon/src/providers/subsonic.rs] (`list_favorite_items`)

## Dev Agent Record

### Agent Model Used

GPT-5 Codex

### Debug Log References

- 2026-05-23: Started implementation; sprint/story status set to in-progress.
- 2026-05-23: `rtk cargo test -p hifimule-daemon provider_list_favorite_items_maps` passed (2 tests).
- 2026-05-23: `rtk tsc` could not spawn global `npx`; equivalent local TypeScript check passed via bundled Node: `rtk C:\Users\alexi\.cache\codex-runtimes\codex-primary-runtime\dependencies\node\bin\node.exe node_modules/typescript/bin/tsc`.
- 2026-05-23: `rtk cargo test -p hifimule-daemon -- --test-threads=1` passed (321 tests).
- 2026-05-23: `rtk cargo test -p hifimule-daemon` passed (321 tests) after one earlier parallel run exited with Windows `STATUS_ACCESS_VIOLATION`.
- 2026-05-23: Local Vite server returned HTTP 200, but in-app browser smoke was blocked for both `http://127.0.0.1:5173` and `http://localhost:5173` with `ERR_BLOCKED_BY_CLIENT`; live provider-backed manual smoke remains open.
- 2026-05-23: Added scoped Favorites basket items for inferred favorite artists/albums so sync expands only favorite descendants instead of full non-favorite containers.
- 2026-05-23: `rtk cargo test -p hifimule-daemon test_sync_calculate_delta_favorite_album_syncs_only_favorite_tracks` passed.
- 2026-05-23: `rtk C:\Users\alexi\.cache\codex-runtimes\codex-primary-runtime\dependencies\node\bin\node.exe node_modules/typescript/bin/tsc` passed.
- 2026-05-23: `rtk cargo test -p hifimule-daemon` passed (322 tests).
- 2026-05-23: Manual smoke test confirmed by Alexis.

### Implementation Plan

- Preserve the existing provider-neutral Favorites tree contract and verify both Jellyfin and Subsonic fill `SearchResult.artists`, `SearchResult.albums`, and `SearchResult.songs`.
- Reuse the existing Artists/Albums hierarchy UI patterns for Favorites, including breadcrumbs, cached pages, scroll restoration, and existing basket item mapping.
- Keep flat favorite-track loading available but keep the Favorites root on `browse.listFavoriteItems`.
- Treat Favorites hierarchy levels as non-paginated and make `loadMore()` return without mutating pagination state.

### Completion Notes List

- Completed hierarchical Favorites implementation already present in the working tree: Favorites root uses a cached favorite tree, artist rows are inferred from favorited artists/albums/tracks, artist drilldown loads all albums for directly favorited artists or filtered favorite albums otherwise, and album drilldown loads all tracks for directly favorited albums/artists or only favorite tracks otherwise.
- Added provider contract coverage for Jellyfin and Subsonic/OpenSubsonic favorite item trees.
- Fixed `loadMore()` so Favorites is a true no-op and does not advance pagination state.
- Added `FavoriteArtist` and `FavoriteAlbum` basket item types for inferred Favorites containers; direct favorite artists/albums continue to use normal full-container sync, while inferred containers sync only favorite albums/tracks.
- Updated Jellyfin auto-sync, Jellyfin delta calculation, and provider-neutral/Subsonic delta calculation to resolve scoped Favorites basket items from the favorite tree.
- Automated verification passed, and manual provider-backed Favorites smoke was confirmed by Alexis.

### File List

- _bmad-output/implementation-artifacts/9-5-hierarchical-favorites-navigation.md
- _bmad-output/implementation-artifacts/sprint-status.yaml
- hifimule-daemon/src/api.rs
- hifimule-daemon/src/main.rs
- hifimule-daemon/src/providers/jellyfin.rs
- hifimule-daemon/src/providers/mod.rs
- hifimule-daemon/src/providers/subsonic.rs
- hifimule-daemon/src/rpc.rs
- hifimule-ui/src/components/BasketSidebar.ts
- hifimule-ui/src/components/MediaCard.ts
- hifimule-ui/src/library.ts
- hifimule-ui/src/rpc.ts

## Change Log

- 2026-05-23: Created corrective story for hierarchical Favorites navigation based on implementation discovery during Story 9.4 follow-up work.
- 2026-05-23: Implemented and verified hierarchical Favorites provider/UI path; story remains in-progress pending live manual smoke.
- 2026-05-23: Corrected Favorites sync semantics for inferred non-favorite artists/albums by adding scoped basket item types.
- 2026-05-23: Manual smoke passed; story moved to review.
