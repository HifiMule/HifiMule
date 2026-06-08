# HifiMule 0.10.0

Release date: 2026-06-08

## Highlights

- **Playlist editing**: HifiMule can now create, rename, delete, and curate playlists directly on your server. A new dual-panel curation view lets you build a playlist, add or remove tracks, reorder them, and see live stats (track count, total duration, total size). Works for Jellyfin and Subsonic/Navidrome.
- **New "Tracks" browse mode**: A dedicated Tracks browse mode pairs a filter panel with a scrollable, auto-paginating track list, complete with A–Z quick navigation and per-track actions.
- **Grid / Table toggle everywhere**: The grid/table view toggle now applies across every browse mode and every drill-down level.
- **German translation**: HifiMule now ships English, French, Spanish, and German.

---

## Added

### Playlist write support across providers (Epic 11)

A new playlist-write contract was added to the `MediaProvider` trait and implemented for both adapters, so playlists can be modified on the server rather than only read:

- **Jellyfin adapter** (`providers/jellyfin.rs`): create, rename, delete, add tracks, and reorder via the Jellyfin Playlists API.
- **Subsonic / Navidrome adapter** (`providers/subsonic.rs`): the equivalent operations via the OpenSubsonic playlist endpoints.
- Providers that do not support a given operation return a clear "unsupported" result instead of failing silently. UI capability is gated on `setPlaylistWriteCapability`, so write actions are only offered where the connected server supports them.
- New daemon RPCs back the UI: create/save-as-playlist, rename, delete, add tracks, and `move_playlist_item` (reorder). Basket selections are resolved to concrete track lists server-side before being written.

### Save basket selection as a playlist (Story 11.5)

The basket sidebar gains **Save as Playlist** and **Send to Playlist** actions (also available from item context menus). The current selection is resolved to its full track list and written to a new or existing playlist on the server.

### Dual-panel playlist curation view (Stories 11.6, 11.7, 11.10)

A new `PlaylistCurationView` shows the playlist's complete track list alongside a browse/search panel:

- Live stats header: track count, total duration, and total size.
- Add tracks by searching or browsing in the side panel, or from the browse view's context menu.
- Remove tracks from the playlist.
- Tracks are numbered in playback order.

### Playlist rename and delete (Story 11.8)

The curation view header exposes rename and delete actions, with confirmation before deletion.

### Playlist reorder (Stories 11.9, 11.10)

Tracks within a playlist can be reordered; the new order is persisted to the server through the `move_playlist_item` RPC and reflected immediately in the numbered list.

### Tracks browse mode (Stories 9.9, 9.10)

A new `tracks` value joins the browse modes. The daemon gains a `TrackListFilter` / `TrackListPage` contract (filterable by library, artist, album, and starting letter, with paged results) and a matching `fetchBrowseTracks` RPC. The `TracksBrowseView` renders a dual-panel layout:

- A filter panel (artists / albums) on one side and a track list on the other.
- Auto-pagination: more tracks load as you scroll, with a spinner shown during autoload.
- A compact A–Z navigation strip (vertical sidebar, 2-column grid) for jumping through large libraries.
- Per-track actions consistent with the rest of the library browser.

---

## Changed

### Grid / Table toggle extended to all modes (Story 9.8)

The grid/table view toggle, previously limited to a subset of views, now works across all browse modes and drill-down levels (artists, albums, tracks, playlists, genres, history, and favorites).

### Jellyfin artist listing uses AlbumArtists

Artist listing now queries the `/Artists/AlbumArtists` endpoint, and track/artist filtering uses `AlbumArtistIds`. This makes the artist list match what users expect (album artists rather than every credited performer) and fixes tracks not appearing under the correct artist.

### Design polish

Multiple passes over layout, spacing, and styling: track-panel sizing was stabilised so it no longer jumps on load (fixed flex basis, 55% track panel / 45% filters), sticky positioning was removed from the "All artists / All albums" rows, the A–Z strip was reworked into a compact sidebar, and a toast helper (`toast.ts`) was added for transient notifications.

### Internationalization

- German (`de`) translation added to the i18n catalog; the catalog and `hifimule-i18n` were extended with all new playlist-curation, tracks-browse, and toast strings across English, French, Spanish, and German.

---

## Fixed

- **Jellyfin album-artist filtering**: tracks and artists are now filtered by `AlbumArtistIds`, fixing missing or misattributed entries for album artists (e.g. artists with a `/` in their name).
- **Jellyfin playlist editing**: corrected playlist edit behaviour and playlist size display.
- **Track panel height** no longer jumps when content loads.

---

## Internal

- `MediaProvider` trait extended with playlist-write methods (`rename_playlist`, `reorder_playlist`, plus create/delete/add), each with a default "unsupported" implementation so adapters opt in explicitly.
- Substantial `rpc.rs` growth to host the new playlist and tracks RPCs and selection-to-tracks resolution.
- Clippy warnings reduced across the daemon.
</content>
</invoke>
