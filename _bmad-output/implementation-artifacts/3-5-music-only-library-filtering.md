# Story 3.5: Music-Only Library Filtering

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a **Ritualist (Arthur)**,
I want the **application to automatically filter out non-music content (movies, series, books) from my Jellyfin library**,
so that **I can focus purely on my music collection for my DAP.**

## Acceptance Criteria

1.  **Strict Music Filtering**: The application MUST retrieve and display only music-related items. This includes `MusicAlbum`, `Playlist`, `MusicArtist`, `Audio`, and `MusicVideo`. (AC: #1)
2.  **Exclusion of Visual Media**: Movies, Series, Seasons, Episodes, and Books MUST be explicitly excluded from all library views. (AC: #2)
3.  **Dynamic Filtering**: The filtering MUST be applied at the API level using Jellyfin's `IncludeItemTypes` parameter to minimize data transfer and ensure consistency. (AC: #3)
4.  **UI Consistency**: The "Vibrant Hub" library browser MUST only show relevant music folders/views even when the server contains mixed libraries. (AC: #4)

## Tasks / Subtasks

- [x] **T1: Daemon - Enhance Jellyfin Client** (AC: #1, #3)
    - [x] Update `hifimule-daemon/src/api.rs`: Modify `get_items` to ensure it correctly handles the `include_item_types` parameter.
    - [x] Define or verify the use of a constant/config for `MUSIC_ITEM_TYPES` (e.g., `"MusicAlbum,Playlist,MusicArtist,Audio,MusicVideo"`).
- [x] **T2: UI - Apply Filtering in Library View** (AC: #2, #4)
    - [x] Update `hifimule-ui/src/library.ts`: Modify `loadItems` to pass the music-only filter string to the `fetchItems` RPC call.
    - [x] Implement UI-side filtering if necessary to ensure `fetchViews` doesn't show non-music libraries (e.g. Movies/TV).
- [x] **T3: Verification & Edge Cases** (AC: #1, #2)
    - [x] Verify that libraries containing mixed content only show the music-related items.
    - [x] Ensure pagination still works correctly with the applied filters.

## Dev Notes

- **Architecture Patterns:**
    - **IPC:** Use existing `jellyfin_get_items` RPC method.
    - **State Management:** The UI already handles item lists; no change to state structure needed.
- **Technical Specifics (Jellyfin API):**
    - The `/Items?userId={userId}` endpoint accepts `IncludeItemTypes`.
    - Recommended filter: `MusicAlbum,Playlist,MusicArtist,Audio,MusicVideo`.
- **Source tree components to touch:**
    - `hifimule-daemon/src/api.rs`: Verify `get_items` and types.
    - `hifimule-ui/src/library.ts`: Update `loadItems` call.

### Project Structure Notes

- Keep logic centralized in `library.ts` to avoid spreading filter hardcoding.

### References

- [Story 3.1 (Library Integration)](file:///c:/Workspaces/HifiMule/_bmad-output/implementation-artifacts/3-1-immersive-media-browser-jellyfin-integration.md)
- [Jellyfin API - Items](https://api.jellyfin.org/#tag/Items/operation/GetItems)

## Dev Agent Record

### Agent Model Used

Antigravity (Workflow Engine)

### Debug Log References

### Completion Notes List
- Implemented `MUSIC_ITEM_TYPES` constant in `api.rs`.
- Added `CollectionType` to `JellyfinView` logic.
- Implemented UI filtering in `library.ts` to show only music/playlist libraries.
- Applied `MUSIC_ITEM_TYPES` filter to `fetchItems` in the UI.
- Verified backend with `cargo test`.

### File List
- `hifimule-daemon/src/api.rs`
- `hifimule-ui/src/components/MediaCard.ts`
- `hifimule-ui/src/library.ts`
