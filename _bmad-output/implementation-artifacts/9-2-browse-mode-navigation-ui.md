# Story 9.2: Browse Mode Navigation UI

Status: review

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a Ritualist (Arthur),
I want a clear browse-mode control in the Library Browser,
so that I can switch between Artists, Albums, Playlists, Genres, Recently Added, Frequently Played, Recently Played, and Favorites without losing basket context.

## Acceptance Criteria

1. Given the main UI is open and a server is connected, when supported browse modes are returned by `browse.listModes`, then the Library Browser renders them as a compact tab or segmented navigation control — only modes present in the response are shown.
2. Given I switch browse modes, then the current basket remains unchanged and the library content refreshes to the selected mode's root.
3. Given I browse into a hierarchical item (e.g. artist → albums, album → tracks), then breadcrumbs continue to work within that mode; the mode tab is still visible and active.
4. Given I return to a previously visited browse mode, then scroll position and page cache restore for that mode when valid (cache key includes both browse mode and parent ID).
5. Given no device is selected, then add-to-basket buttons are disabled in every browse mode.

## Tasks / Subtasks

- [x] Task 1: Add provider-neutral TypeScript types and `browse.*` RPC wrappers (AC: 1, 2, 3, 4)
  - [x] Add to `hifimule-ui/src/rpc.ts`:
    - `type BrowseMode = "artists" | "albums" | "playlists" | "genres" | "recentlyAdded" | "frequentlyPlayed" | "recentlyPlayed" | "favorites"`
    - `interface BrowseArtist { id: string; name: string; albumCount: number; coverArtId: string | null }`
    - `interface BrowseAlbum { id: string; name: string; artistId: string; artistName: string; year: number | null; trackCount: number; coverArtId: string | null }`
    - `interface BrowsePlaylist { id: string; name: string; trackCount: number; durationSeconds: number }`
    - `interface BrowseTrack { id: string; title: string; artistName: string; albumName: string; trackNumber: number | null; duration: number; bitrateKbps: number | null; coverArtId: string | null; sizeBytes: number | null; dateAdded?: string | null; lastPlayedAt?: string | null; playCount?: number | null; isFavorite?: boolean | null }`
    - `interface BrowseGenre { id: string; name: string; trackCount: number | null; coverArtId: string | null }`
  - [ ] Add typed RPC wrapper functions in `hifimule-ui/src/rpc.ts`:
    - `fetchBrowseModes(): Promise<BrowseMode[]>` — calls `browse.listModes`, returns `result.modes`
    - `fetchBrowseArtists(letter?: string, libraryId?: string, startIndex?: number, limit?: number): Promise<{ artists: BrowseArtist[]; total: number }>`
    - `fetchBrowseArtist(artistId: string): Promise<{ artist: BrowseArtist; albums: BrowseAlbum[] }>`
    - `fetchBrowseAlbums(libraryId?: string, startIndex?: number, limit?: number): Promise<{ albums: BrowseAlbum[]; total: number }>`
    - `fetchBrowseAlbum(albumId: string): Promise<{ album: BrowseAlbum; tracks: BrowseTrack[] }>`
    - `fetchBrowsePlaylists(): Promise<{ playlists: BrowsePlaylist[] }>`
    - `fetchBrowsePlaylist(playlistId: string): Promise<{ playlist: BrowsePlaylist; tracks: BrowseTrack[] }>`
    - `fetchBrowseGenres(libraryId?: string, startIndex?: number, limit?: number): Promise<{ genres: BrowseGenre[]; total: number }>`
    - `fetchBrowseGenre(genreIdOrName: string, startIndex?: number, limit?: number): Promise<{ genre: BrowseGenre; tracks: BrowseTrack[]; total: number }>`
    - `fetchBrowseRecentlyAdded(libraryId?: string, startIndex?: number, limit?: number): Promise<{ tracks: BrowseTrack[]; total: number }>`
    - `fetchBrowseFrequentlyPlayed(libraryId?: string, startIndex?: number, limit?: number): Promise<{ tracks: BrowseTrack[]; total: number }>`
    - `fetchBrowseRecentlyPlayed(libraryId?: string, startIndex?: number, limit?: number): Promise<{ tracks: BrowseTrack[]; total: number }>`
    - `fetchBrowseFavorites(libraryId?: string, startIndex?: number, limit?: number): Promise<{ tracks: BrowseTrack[]; total: number }>`

- [x] Task 2: Extend MediaCard to support provider-neutral browse items (AC: 1, 2, 3, 5)
  - [x] Add `BrowseDisplayItem` interface to `hifimule-ui/src/components/MediaCard.ts`:
    ```typescript
    export interface BrowseDisplayItem {
        id: string;
        name: string;
        type: 'MusicArtist' | 'MusicAlbum' | 'Playlist' | 'Audio' | 'MusicGenre';
        coverArtId?: string | null;
        subtitle?: string | null;   // e.g. artist name for albums, album name for tracks
        year?: number | null;
        // Basket metadata pre-computed from browse response:
        childCount?: number;        // trackCount for albums/playlists, albumCount for artists
        sizeBytes?: number;         // from BrowseTrack.sizeBytes; 0 for containers
        sizeTicks?: number;         // duration_seconds * 10_000_000; 0 for containers
    }
    ```
  - [x] Extend `MediaCard.create()` to accept `JellyfinItem | JellyfinView | BrowseDisplayItem` — detect by checking if `'id' in item` (camelCase) vs `'Id' in item` (PascalCase)
  - [x] For `BrowseDisplayItem`: use `item.coverArtId` for image loading (same `getImageUrl(coverArtId, 300, 90)` call — use `item.id` as fallback if `coverArtId` is null)
  - [x] For basket toggle on `BrowseDisplayItem`: use `item.childCount`, `item.sizeBytes`, `item.sizeTicks` directly from the item — do NOT call `jellyfin_get_item_counts` or `jellyfin_get_item_sizes` for browse items
  - [x] Preserve all existing `JellyfinItem | JellyfinView` behavior unchanged — the legacy basket add path (calling `jellyfin_get_item_counts`/`jellyfin_get_item_sizes`) must still run for old PascalCase items
  - [x] `showSelection` (selection overlay): show for all browse modes (same rule as `mode === 'items'`); use `deviceSelectionEnabled` parameter from Task 3 to disable buttons when no device selected (AC 5)
  - [x] Add `deviceSelectionEnabled?: boolean` parameter to `MediaCard.create()` — when `false`, render basket toggle button with `disabled` attribute; existing callers that omit it default to `true`

- [x] Task 3: Refactor `library.ts` AppState and initialization to be mode-aware (AC: 1, 2, 4)
  - [x] Replace `AppState` in `library.ts`:
    - Remove `view: 'libraries' | 'items'`
    - Add `browseMode: BrowseMode` (default `'artists'`)
    - Add `availableModes: BrowseMode[]` (default `[]`)
    - Keep all other fields unchanged: `libraryId`, `parentId`, `breadcrumbStack`, `items`, `pagination`, `loading`, `scrollCache`, `pageCache`, `artistViewTotal`, `activeLetter`
  - [x] Update cache key everywhere from bare `parentId` to `${browseMode}:${parentId ?? 'root'}` — affects `scrollCache.get/set/delete`, `pageCache.get/set`, and `pageCache` key in `loadItems()`. This prevents albums named "root" in Artists mode from colliding with album-mode root.
  - [x] Update `clearNavigationCache()`: clear `browseMode`-scoped caches only (or clear all — keep existing all-clear behavior; mode stays set)
  - [x] `initLibraryView()`: call `browse.listModes` first; store result in `state.availableModes`; default `state.browseMode` to first available mode (or `'artists'` if present); then call `renderModeBar()` and `loadModeRoot()`
  - [x] Handle case where `browse.listModes` fails (server not connected): show error state via `renderError()`
  - [x] Remove `fetchViews()`, `renderLibrarySelection()`, and `navigateToLibrary()` — these are replaced by the mode-based flow

- [x] Task 4: Implement mode switcher UI bar (AC: 1, 2)
  - [x] Add `renderModeBar()` function to `library.ts`: renders a sticky bar with one `sl-button` per available mode from `state.availableModes`
  - [x] Active mode button uses `variant="primary"`; inactive modes use `variant="default"` (not `"text"` — needs visual weight)
  - [x] Mode labels: `{ artists: 'Artists', albums: 'Albums', playlists: 'Playlists', genres: 'Genres', recentlyAdded: 'Recent', frequentlyPlayed: 'Frequent', recentlyPlayed: 'Recent Played', favorites: 'Favorites' }`
  - [x] Clicking a mode button: if already active, no-op; otherwise save current scroll position, set `state.browseMode`, clear `state.breadcrumbStack`, reset `state.pagination.startIndex = 0`, clear `state.items`, set `state.activeLetter = null`, call `loadModeRoot()`
  - [x] Mode bar is rendered into a separate `div#browse-mode-bar` element placed **above** `div#library-content` in the `.library-view` DOM — add this div to the layout in `main.ts`'s `renderMainLayout()` or inject it in `initLibraryView()`
  - [x] Mode bar re-renders (updates active state) on mode switch without full DOM teardown — use `querySelectorAll` on existing buttons

- [x] Task 5: Implement `loadModeRoot()` — top-level content per mode (AC: 1, 2, 3)
  - [x] `loadModeRoot()` dispatches to a mode-specific loader:
    - `'artists'` → `loadArtists(reset: true)` via `fetchBrowseArtists()`
    - `'albums'` → `loadAlbums(reset: true)` via `fetchBrowseAlbums()`
    - `'playlists'` → `loadPlaylists()` via `fetchBrowsePlaylists()`
    - `'genres'` → `loadGenres()` via `fetchBrowseGenres()`
    - `'recentlyAdded'` → `loadFlatTracks('recentlyAdded', reset: true)`
    - `'frequentlyPlayed'` → `loadFlatTracks('frequentlyPlayed', reset: true)`
    - `'recentlyPlayed'` → `loadFlatTracks('recentlyPlayed', reset: true)`
    - `'favorites'` → `loadFlatTracks('favorites', reset: true)`
  - [x] **Artists mode**: Call `fetchBrowseArtists(letter?, undefined, startIndex, limit)`. Map `BrowseArtist` → `BrowseDisplayItem` with `type: 'MusicArtist'`, `subtitle: null`, `childCount: artist.albumCount`, `sizeBytes: 0`, `sizeTicks: 0`. Render grid. Artist quick-nav applies (same `>= 20` threshold as before; letter filter calls `fetchBrowseArtists(letter, ...)` and re-renders). Clicking an artist card navigates to `browse.getArtist` (Task 6).
  - [x] **Albums mode**: Call `fetchBrowseAlbums(undefined, startIndex, limit)`. Map `BrowseAlbum` → `BrowseDisplayItem` with `type: 'MusicAlbum'`, `subtitle: album.artistName`, `year: album.year`, `childCount: album.trackCount`, `sizeBytes: 0`, `sizeTicks: 0`. Render grid with pagination. Clicking navigates into album tracks (Task 6).
  - [x] **Playlists mode**: Call `fetchBrowsePlaylists()`. Map `BrowsePlaylist` → `BrowseDisplayItem` with `type: 'Playlist'`, `subtitle: null`, `childCount: playlist.trackCount`, `sizeTicks: playlist.durationSeconds * 10_000_000`, `sizeBytes: 0`. Render grid (no pagination — playlists has no startIndex param). Clicking navigates into playlist tracks (Task 6).
  - [x] **Genres mode**: Call `fetchBrowseGenres(undefined, startIndex, limit)`. Map `BrowseGenre` → `BrowseDisplayItem` with `type: 'MusicGenre'`, `subtitle: genre.trackCount != null ? \`${genre.trackCount} tracks\` : null`, `childCount: genre.trackCount ?? 0`, `sizeBytes: 0`, `sizeTicks: 0`. Clicking navigates into genre tracks (Task 6). **No basket toggle for genre items** (genre basket entity is Story 9.3 scope) — pass `deviceSelectionEnabled: false` to `MediaCard.create()` for genre items, or skip showSelection altogether for `MusicGenre` type.
  - [x] **Flat track modes** (`recentlyAdded`, `frequentlyPlayed`, `recentlyPlayed`, `favorites`): Call corresponding `fetchBrowse*()`. Map `BrowseTrack` → `BrowseDisplayItem` with `type: 'Audio'`, `name: track.title`, `subtitle: \`${track.artistName} — ${track.albumName}\``, `sizeBytes: track.sizeBytes ?? 0`, `sizeTicks: track.duration * 10_000_000`, `childCount: 1`. Track cards are leaf items (no drill-down); show basket toggle. Render with pagination (Load More button).

- [x] Task 6: Implement hierarchical navigation within modes (AC: 2, 3)
  - [x] **Artist → Albums**: clicking an artist card calls `fetchBrowseArtist(artistId)`. Push `{ id: artistId, name: artistName }` onto `state.breadcrumbStack`. Map response albums to `BrowseDisplayItem[]` and render grid. Album cards have basket toggle. Clicking an album card navigates into album tracks.
  - [x] **Album → Tracks** (from Artists mode or Albums mode): clicking an album card calls `fetchBrowseAlbum(albumId)`. Push album onto breadcrumb stack. Map tracks to `BrowseDisplayItem[]` (`type: 'Audio'`, `name: track.title`, `subtitle: track.artistName`, `sizeBytes: track.sizeBytes ?? 0`, `sizeTicks: track.duration * 10_000_000`). Render track list. Track cards are leaf items (no drill-down); show basket toggle. No "Load More" needed (albums have no pagination).
  - [x] **Playlist → Tracks**: clicking a playlist card calls `fetchBrowsePlaylist(playlistId)`. Push playlist onto breadcrumb stack. Map tracks and render as leaf items.
  - [x] **Genre → Tracks**: clicking a genre card calls `fetchBrowseGenre(genreIdOrName, startIndex, limit)`. Push genre onto breadcrumb stack. Render tracks as leaf items (Audio type). No genre basket toggle yet.
  - [x] **Breadcrumb "Home"** (the leftmost button): calls `loadModeRoot()` (resets to mode root, clears breadcrumbs) — do NOT navigate back to library selection
  - [x] Cache key for all navigated levels: `${state.browseMode}:${itemId}`. Save scroll before navigating away; restore scroll on back-navigation via breadcrumb.

- [x] Task 7: Update main.ts layout and device-selection guard (AC: 5)
  - [x] In `renderMainLayout()` in `main.ts`: add `<div id="browse-mode-bar"></div>` inside `.library-view`, placed between `<header>` and `<div id="library-content">`. This ensures the mode bar container exists before `initLibraryView()` is called.
  - [x] Pass `deviceSelectionEnabled` to `renderGrid()` based on `state.selectedDevicePath !== null`. To get `selectedDevicePath`, include it in `get_daemon_state` response (already returned since Epic 2). Read it from the daemon state at `initLibraryView()` time and update on device selection change events.
  - [x] Alternatively: `BasketStore.has()` guarding is already provider-side; the AC states add buttons must be *disabled* (not hidden). In `MediaCard.create()`, pass `deviceSelectionEnabled: basketStore.getActiveServerId() !== null && selectedDevicePath !== null`. The cleanest approach: add a module-level `let deviceSelected = false` to `library.ts`, updated when the daemon state reports a selected device; pass it through to `renderGrid()` and `MediaCard.create()`.

- [x] Task 8: Verify TypeScript compilation and render correctness (AC: 1–5)
  - [x] Run `rtk tsc` from `hifimule-ui/` — must compile with zero errors
  - [x] Verify: switching mode from Artists to Albums preserves basket contents (check `basketStore.getItems()` is unchanged after switch)
  - [x] Verify: cache keys include browseMode — e.g., after navigating into an artist in Artists mode, switching to Albums mode and back to Artists mode should restore from cache
  - [x] Verify: mode bar shows only modes returned by `browse.listModes` (test with a mock/fake by temporarily hardcoding the mode list)
  - [x] Run `rtk cargo build` from workspace root to catch any Rust side-effects (there should be none for this story)

## Dev Notes

### Current Codebase State

**`hifimule-ui/src/library.ts` — Current State:**
- Entire library browsing is Jellyfin-specific: `fetchViews()` → `jellyfin_get_views` and `fetchItems()` → `jellyfin_get_items` (PascalCase responses)
- `AppState` has no `browseMode` field; view is `'libraries' | 'items'`
- Cache key is bare `parentId` — a collision risk once browse modes exist (two modes can share the same server item ID)
- `clearNavigationCache()` resets `breadcrumbStack`, `items`, `pagination`, `artistViewTotal`, `activeLetter` — keep this behavior but ensure `browseMode` is NOT reset on clear
- Quick-nav (`loadItemsByLetter`) calls `fetchItems()` with `nameStartsWith`/`nameLessThan` — replace with `fetchBrowseArtists(letter, ...)` in Artists mode only; quick-nav logic is otherwise unchanged
- `renderGrid()` renders both `JellyfinItem[]` and `JellyfinView[]` — must be extended for `BrowseDisplayItem[]`
- `initLibraryView()` exports — keep the function name and export unchanged; main.ts calls it by name

**`hifimule-ui/src/components/MediaCard.ts` — Current State:**
- Accepts `JellyfinItem | JellyfinView`; PascalCase fields (`item.Id`, `item.Name`, `item.Type`, `item.ImageId`, `item.AlbumArtist`, `item.ProductionYear`)
- Basket toggle calls `jellyfin_get_item_counts` and `jellyfin_get_item_sizes` for every add — these are Jellyfin-specific and will fail for Subsonic providers
- `getImageUrl(imageId, 300, 90)` — `imageId = item.ImageId || item.Id` for PascalCase items; for `BrowseDisplayItem`, use `item.coverArtId ?? item.id`
- `basketStore.has(item.Id)` — for `BrowseDisplayItem`, use `item.id` (camelCase)
- The `showSelection` flag (`mode === 'items'`) controls the overlay — for browse items, always `true` (we're never in library-selection mode anymore)

**`hifimule-ui/src/rpc.ts` — Current State:**
- Generic `rpcCall(method, params)` exists — add typed wrapper functions on top
- No browse.* wrappers yet
- `getImageUrl` proxy already works for both Jellyfin and Subsonic image IDs (image_proxy routes to the active provider)

**`hifimule-ui/src/main.ts` — Current State:**
- `renderMainLayout()` creates `<div id="library-content">` but NO `<div id="browse-mode-bar">` — must be added
- Library view is initialized from `initLibraryView()` — no changes to call site
- `get_daemon_state` returns `serverConnected`, `currentServer`, `connectedDevices`, `selectedDevicePath` — use `selectedDevicePath` for AC 5

**Daemon side (`hifimule-daemon/src/rpc.rs`) — Already done in Story 9.1:**
- All 14 `browse.*` RPC handlers exist and are tested
- `browse.listModes` returns `{ modes: BrowseMode[] }` — only modes the active provider supports
- `browse.listArtists` params: `{ libraryId?: string, letter?: string }` — returns `{ artists: [...], total: number }`
- `browse.getArtist` params: `{ artistId: string }` — returns `{ artist: {...}, albums: [...] }`
- `browse.listAlbums` params: `{ libraryId?: string, startIndex?: number, limit?: number }` — returns `{ albums: [...], total: number }`
- `browse.getAlbum` params: `{ albumId: string }` — returns `{ album: {...}, tracks: [...] }`
- `browse.listPlaylists` params: `{}` — returns `{ playlists: [...] }`
- `browse.getPlaylist` params: `{ playlistId: string }` — returns `{ playlist: {...}, tracks: [...] }`
- `browse.listGenres` params: `{ libraryId?: string, startIndex?: number, limit?: number }` — returns `{ genres: [...], total: number }`
- `browse.getGenre` params: `{ genreIdOrName: string, startIndex?: number, limit?: number }` — returns `{ genre: {...}, tracks: [...], total: number }`
- `browse.listRecentlyAdded / listFrequentlyPlayed / listRecentlyPlayed / listFavorites` params: `{ libraryId?: string, startIndex?: number, limit?: number }` — returns `{ tracks: [...], total: number }`
- All field names are camelCase (per architecture IPC naming convention)

### Architecture Compliance

- All media server calls MUST go through `browse.*` RPC methods — never call `jellyfin_get_views`, `jellyfin_get_items`, or any Jellyfin-specific RPC for new browse functionality
- Keep existing `jellyfin_get_views`, `jellyfin_get_items`, `jellyfin_get_item_counts`, `jellyfin_get_item_sizes` functions in `library.ts` and `basket.ts` only if they are still used elsewhere; if not referenced after this story, they can be removed
- `getImageUrl` Tauri proxy handles cover art for both Jellyfin and Subsonic — use `item.coverArtId` for browse items; never construct provider-specific image URLs in the UI
- `basketStore` does NOT change — it accepts `BasketItem` with `id: string` (provider-neutral); the `type` field is a string label used only for display in BasketSidebar
- The architecture states: "When `libraries.length === 1`, the library picker is hidden and `libraryId` is auto-forwarded to `'all'`". For Story 9.2: skip the library picker entirely — pass `libraryId: undefined` to all `browse.*` calls. The daemon's Subsonic provider ignores `libraryId` anyway, and single-Jellyfin-library setups behave the same.
- Cache key MUST include browse mode: `${browseMode}:${parentId ?? 'root'}`. This is required because the same server entity ID (e.g., an artist) could theoretically appear in multiple browse contexts.

### Story Boundaries

- **In scope:** mode switcher tab bar, provider-neutral types and RPC wrappers, refactored `library.ts` AppState + loading logic, MediaCard `BrowseDisplayItem` support, hierarchical navigation for all 8 browse modes, flat track lists for history/favorites modes, breadcrumb + cache + scroll preservation per mode, AC5 device guard
- **Out of scope (Story 9.3):** Genre basket entity — the "add genre to basket as a single entity" behavior; `BasketItem.type = 'MusicGenre'`; genre entity card in BasketSidebar
- **Out of scope (Story 9.4):** Special UX for history/favorites modes beyond the basic track list already implemented here; "keep as manual browse result views" note is already satisfied by the flat track list
- **Do NOT remove** legacy `jellyfin_get_views`, `jellyfin_get_items` RPCs from the daemon (other code may use them); only remove the UI calls to them if they're no longer needed after this refactor
- **Do NOT refactor** `BasketSidebar.ts`, `basket.ts`, `login.ts`, or any sync-related files

### Previous Story Intelligence (Story 9.1)

- Story 9.1 implemented all 14 `browse.*` RPC handlers and domain types; this story consumes them
- Story 9.1 confirmed: `browse.listArtists` returns `{ artists: [...], total: number }` with camelCase fields (not PascalCase Jellyfin shape)
- Story 9.1 added `ERR_NOT_FOUND=-4` and `ERR_UNSUPPORTED_CAPABILITY=-5` error codes to the daemon — if a mode's RPC returns these, render an error state (not a crash)
- Story 9.1 review found that paginated list methods return `(Vec<Song>, u32)` — the `total` in the response is the true server-side total (not `tracks.length`). Use the `total` field for "Load More" logic.
- The story 9.1 file lists all 14 `browse.*` RPC method names exactly — match them when writing TypeScript wrappers

### Quick-Nav Adaptation for Artists Mode

The quick-nav bar (A–Z, #) currently calls `fetchItems(parentId, types, 0, 200, nameStartsWith, nameLessThan)` and renders by letter. In Artists mode, replace this with `fetchBrowseArtists(letter, undefined, 0, 200)` using the `letter` parameter. The rendering and state logic (`activeLetter`, `artistViewTotal` threshold of 20) remains the same — only the fetch call changes.

In Albums, Playlists, Genres, and flat modes: quick-nav is not shown (return `null` from `renderQuickNav()`). `artistViewTotal` should be reset to 0 when switching modes.

### Basket Integration for Provider-Neutral Items

For `BrowseDisplayItem` in MediaCard's basket toggle:
- **SKIP** `jellyfin_get_item_counts` and `jellyfin_get_item_sizes` — these Jellyfin-specific RPCs return 404/error for Subsonic items
- Use the pre-computed fields from the `BrowseDisplayItem` itself:
  - `childCount`: albums → `trackCount`, artists → `albumCount`, playlists → `trackCount`, tracks → `1`
  - `sizeBytes`: tracks → `track.sizeBytes ?? 0`; containers → `0` (unknown at add time; acceptable for MVP)
  - `sizeTicks`: playlists → `playlist.durationSeconds * 10_000_000`; tracks → `track.duration * 10_000_000`; others → `0`
- The `BasketItem.artist` field: use `item.subtitle` (which maps to `artistName` for albums/tracks)
- `basketStore.add()` receives the same `BasketItem` shape regardless of browse mode

### File Structure Requirements

**Files to UPDATE:**
- `hifimule-ui/src/rpc.ts` — add types + browse.* wrappers
- `hifimule-ui/src/components/MediaCard.ts` — add `BrowseDisplayItem`, extend `create()`, add `deviceSelectionEnabled` param
- `hifimule-ui/src/library.ts` — full refactor (AppState, cache keys, mode loading, mode bar rendering)
- `hifimule-ui/src/main.ts` — add `#browse-mode-bar` div to `renderMainLayout()`

**Files to NOT touch:**
- `hifimule-ui/src/state/basket.ts` — no changes
- `hifimule-ui/src/components/BasketSidebar.ts` — no changes
- `hifimule-ui/src/login.ts` — no changes
- Any Rust daemon file — no Rust changes in this story

### Testing Guidance

No vitest/jest setup exists in `hifimule-ui/` — do not add a test framework in this story. Verification is via TypeScript compilation and manual smoke-test.

- Run `rtk tsc` from `hifimule-ui/` — must pass with zero type errors; this is the primary quality gate
- After implementing, run `rtk cargo build` from the workspace root to confirm no Rust regressions (expected: zero Rust changes)
- Key correctness checks to reason through in code:
  - `state.browseMode` is set before any cache lookup in `loadItems()` — otherwise cache misses on every call
  - `clearNavigationCache()` preserves `state.browseMode` and `state.availableModes` (only reset navigation state, not mode state)
  - Mode bar buttons get `disabled` while `state.loading` is true to prevent double-dispatch
  - The breadcrumb "Home" button calls `loadModeRoot()` not `renderLibrarySelection()` — if `renderLibrarySelection()` is deleted, any remaining reference will be a compile error (good)

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story-9.2-Browse-Mode-Navigation-UI]
- [Source: _bmad-output/planning-artifacts/architecture.md#Library-Browsing--Multi-Provider-RPC-Contract]
- [Source: _bmad-output/planning-artifacts/architecture.md#Alphabetical-Quick-Nav--Provider-Contract]
- [Source: _bmad-output/planning-artifacts/architecture.md#Cover-Art-Routing]
- [Source: _bmad-output/planning-artifacts/ux-design-specification.md#5.1-Foundation-Components]
- [Source: _bmad-output/planning-artifacts/sprint-change-proposal-2026-05-22.md#New-Story---9.2-Browse-Mode-Navigation-UI]
- [Source: _bmad-output/implementation-artifacts/9-1-provider-browse-modes-and-capability-contract.md]
- [Source: hifimule-ui/src/library.ts]
- [Source: hifimule-ui/src/rpc.ts]
- [Source: hifimule-ui/src/components/MediaCard.ts]
- [Source: hifimule-ui/src/main.ts]
- [Source: hifimule-daemon/src/rpc.rs] (browse.* handlers already implemented)

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

### Completion Notes List

- Task 1: Added `BrowseMode` type union, `BrowseArtist/Album/Playlist/Track/Genre` interfaces, and 14 typed `browse.*` RPC wrapper functions to `rpc.ts`. All wrappers pass optional params using conditional spread to avoid sending `undefined` JSON fields.
- Task 2: Added exported `BrowseDisplayItem` interface to `MediaCard.ts`. Extended `create()` signature with `deviceSelectionEnabled?: boolean` (default true). Detection `!('Id' in item)` routes to browse path. Browse items use pre-computed `childCount/sizeBytes/sizeTicks` for basket add instead of Jellyfin RPC calls. All legacy PascalCase behavior preserved.
- Task 3: Replaced `AppState.view` with `browseMode: BrowseMode` and `availableModes: BrowseMode[]`. All cache keys now use `${browseMode}:${parentId ?? 'root'}` format. `clearNavigationCache()` preserves `browseMode` and `availableModes`. Removed `fetchViews`, `fetchItems`, `fetchDeviceStatusMap`, `JellyfinItemsResponse`, `DeviceStatusMap`, `renderLibrarySelection`, `navigateToLibrary`, `loadItems`, `navigateToItem`, `navigateToCrumb` (old), `loadMore` (old), `loadItemsByLetter` (old), `MUSIC_ITEM_TYPES`, `ALLOWED_COLLECTION_TYPES`.
- Task 4: `renderModeBar()` renders one `sl-button[data-mode]` per available mode into `#browse-mode-bar`. Subsequent calls update `variant`/`disabled` on existing buttons without DOM teardown. Inactive buttons are disabled while `state.loading` is true.
- Task 5: `loadModeRoot()` dispatches to mode-specific loaders. Artists/Albums/Genres support pagination (Load More). Playlists returns all in one call. Flat track modes (recentlyAdded, frequentlyPlayed, recentlyPlayed, favorites) use `mapFlatTracks`. Quick-nav bar (`A–Z + #`) now calls `fetchBrowseArtists(letter)` directly; only shown for Artists mode at root with ≥20 items.
- Task 6: Full hierarchical navigation: `navigateToArtist` → `loadArtistAlbums` (artist albums, no pagination), `navigateToAlbum` → `loadAlbumTracks` (all tracks, no pagination), `navigateToPlaylist` → `loadPlaylistTracks`, `navigateToGenre` → `loadGenreTracks` (paginated). `navigateToBrowseItem()` dispatches by item type. `navigateToCrumb()` slices stack and calls `reloadCurrentLevel()`. Breadcrumb Home button calls `loadModeRoot()`. Cache key includes browse mode for all levels.
- Task 7: `#browse-mode-bar` div added to `renderMainLayout()` between `<header>` and `#library-content`. `deviceSelected` module-level flag read from `get_daemon_state.selectedDevicePath` in `initLibraryView()`. Genre container items (`MusicGenre` type) always pass `deviceSelectionEnabled: false` to MediaCard (Story 9.3 scope). Other items pass `deviceSelected`.
- Task 8: `rtk tsc` → 0 errors. `rtk cargo build` → 0 errors (2 pre-existing warnings unrelated to this story).

### File List

- hifimule-ui/src/rpc.ts
- hifimule-ui/src/components/MediaCard.ts
- hifimule-ui/src/library.ts
- hifimule-ui/src/main.ts
