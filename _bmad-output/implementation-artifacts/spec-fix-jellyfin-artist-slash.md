---
spec_id: fix-jellyfin-artist-slash
title: Fix Jellyfin artist-slash bug (ArtistIds → AlbumArtistIds)
status: done
branch: playlist-edit
created: 2026-06-08
---

## Problem

When browsing tracks or albums for a Jellyfin artist whose name contains a `/` (e.g. AC/DC):

1. **Track filter broken** — selecting the artist returned an unfiltered track list instead of that artist's tracks.
2. **Album list empty** — the albums panel stayed empty (separate pre-existing Jellyfin metadata issue, not a HifiMule code bug).
3. **Name display** — the `/` appeared as `_` in the UI. Investigation showed this is Jellyfin server behaviour (file tag normalisation), not a HifiMule bug.

Root cause for (1): `list_tracks` in the Jellyfin provider called `get_items` with `ArtistIds`, which filters by *contributing artist* UUID. `MusicArtist` entities in Jellyfin represent *album artists*, so the correct filter is `AlbumArtistIds`.

---

## Changes

### `hifimule-daemon/src/api.rs`

| What | Why |
|---|---|
| Renamed `artist_ids` param → `album_artist_ids` in `get_items` | Clarifies semantics; no callers used this for contributing-artist filtering |
| Changed `ArtistIds={}` → `AlbumArtistIds={}` in query string | Core fix — MusicArtist entities are album artists |
| Added `replace('#', "%23")` to `NameStartsWith` and `NameLessThan` values | `#` is a URL fragment separator; unencoded it truncates the query string for non-alpha browse (e.g. browsing artists starting with `#`) |
| Removed 7 leftover `println!("DEBUG: Jellyfin Response …")` statements | Debug noise |

### `hifimule-daemon/src/providers/jellyfin.rs`

| What | Why |
|---|---|
| Renamed local `artist_ids` → `album_artist_ids` in `list_tracks` | Matches renamed `get_items` parameter |
| Added test `provider_list_tracks_by_artist_uses_album_artist_ids` | Regression guard: verifies `AlbumArtistIds` query param appears when filtering tracks by artist |

### `hifimule-daemon/src/rpc.rs`

| What | Why |
|---|---|
| Route `/jellyfin/image/{id}` → `/jellyfin/image/{*id}` (wildcard) | Defensive: non-Jellyfin providers (Subsonic/Navidrome) may use path-based cover art IDs containing `/`; previous single-segment param silently 404'd |

---

## Acceptance Criteria

- **AC1**: Given an artist named "AC/DC" exists in Jellyfin, When the user selects that artist in the Tracks view, Then only tracks whose album artist is AC/DC are returned.
- **AC2**: Given `letter = "#"` is passed to `list_artists` or `list_tracks`, When the Jellyfin API request is built, Then `NameStartsWith=%23` (percent-encoded) appears in the query string.
- **AC3**: Given a cover art ID containing `/` is requested, When the image proxy endpoint is called, Then the request is routed correctly and the image is returned.
- **AC4**: All 425 existing tests pass.

---

## Out of Scope

- Album display for AC/DC: Jellyfin may store "AC/DC" and "AC_DC" as separate entities linked to different album sets. This is a Jellyfin metadata issue to fix on the server side (re-scan / fix tags).
- Artist name display showing `_` instead of `/`: Jellyfin server normalisation behaviour.
