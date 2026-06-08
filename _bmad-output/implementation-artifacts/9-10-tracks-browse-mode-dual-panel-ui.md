---
baseline_commit: 5ca0d07
---

# Story 9.10: Tracks Browse Mode — Dual-Panel UI with Auto-Pagination & Track Actions

Status: review

## Story

As a Ritualist (Arthur),
I want to browse my entire library at the track grain with artist and album filters,
So that I can quickly find and queue individual songs without drilling through albums.

## Acceptance Criteria

1. **Given** the active provider advertises the Tracks mode
   **When** the Library Browser renders the browse-mode bar
   **Then** a "Tracks" mode is shown alongside the existing modes.

2. **Given** I select the Tracks mode
   **Then** the view renders three panels: an artists panel on the left, an albums panel on the right, and a track list panel below.

3. **Given** the Tracks view is rendering
   **Then** the artist panel auto-paginates the full library artist list via `browse.listArtists` with autoload-on-scroll.
   **And** the album panel auto-paginates albums (filtered by the selected artist if any) via `browse.getArtist`/`browse.listAlbums` with autoload-on-scroll.
   **And** the track list auto-paginates via `browse.listTracks` with the active artist/album filters.

4. **Given** the artist panel shows an "All artists" entry at the top
   **When** I select it
   **Then** the album panel shows all library albums (paginated) and the track panel shows all library tracks (paginated).

5. **Given** I select an artist in the left panel
   **Then** the album panel filters to that artist's albums (via `browse.getArtist`).
   **And** the track panel filters to that artist's tracks (paginated).

6. **Given** I select an album in the right panel
   **Then** the track panel filters to that album's tracks (paginated).

7. **Given** the album panel shows an "All albums" entry at the top
   **When** I select it
   **Then** the track panel filter clears its album constraint (artist constraint, if any, remains).

8. **Given** a device is selected
   **When** a track row renders
   **Then** a (+) "Add to basket" control is shown; if the track is already in the basket, a (-) "Remove from basket" control is shown instead.

9. **Given** no device is selected
   **Then** all (+) controls render disabled.

10. **Given** the active provider supports playlist write
    **When** I right-click a track row
    **Then** an "Add to playlist…" context menu appears (per Story 11.7).
    **And** the track row also renders a visible "Send to playlist…" affordance opening the same flow.

11. **Given** the active provider does not support playlist write
    **Then** both the context menu and the "Send to playlist…" affordance are hidden.

12. **Given** I am in Tracks mode
    **Then** the grid/list view toggle is not displayed (the dual-panel layout is the sole rendering).

13. **Given** an A–Z letter strip is available on the artist or album panel
    **When** I select a letter
    **Then** the corresponding panel filters its list and pagination resets.

14. **Given** I switch away from Tracks mode and back
    **Then** the panel selections and scroll positions are restored (consistent with other browse modes).

## Tasks / Subtasks

- [x] **Task 1: Add `fetchBrowseTracks` RPC helper and extend `BrowseMode` union** (AC: 1)
  - [x] In [hifimule-ui/src/rpc.ts:95](hifimule-ui/src/rpc.ts:95), extend `BrowseMode` type:
    ```typescript
    export type BrowseMode = "artists" | "albums" | "playlists" | "tracks" | "genres" | "recentlyAdded" | "frequentlyPlayed" | "recentlyPlayed" | "favorites";
    ```
    Place `"tracks"` after `"playlists"` to mirror the architecture doc's `BrowseMode` union order.
  - [x] Add `fetchBrowseTracks` after `fetchBrowseFavorites` (around line 287, before `fetchBrowseFavoriteItems`):
    ```typescript
    export async function fetchBrowseTracks(filter: {
        libraryId?: string;
        artistId?: string;
        albumId?: string;
        letter?: string;
        startIndex?: number;
        limit?: number;
    }): Promise<{ tracks: BrowseTrack[]; total: number; startIndex: number; limit: number }> {
        return await rpcCall('browse.listTracks', filter);
    }
    ```
    Note the return type is `{ tracks, total, startIndex, limit }` — four fields, NOT just `{ tracks, total }`. The daemon returns all four (per 9.9 implementation).

- [x] **Task 2: Add i18n keys** (AC: 1, 4, 7, 3)
  - [x] In `hifimule-i18n/catalog.json`, locate the `library.mode.*` block (lines 59–66) and insert after `library.mode.playlists`:
    ```json
    "library.mode.tracks": "Tracks",
    ```
    (French: `"Pistes"`, Spanish: `"Pistas"`)
  - [x] Add the `tracks.view.*` keys as a new block (place near other `tracks.*` or after the `library.*` section):
    ```json
    "tracks.view.all_artists": "All artists",
    "tracks.view.all_albums": "All albums",
    "tracks.view.no_tracks": "No tracks for this selection",
    "tracks.view.loading": "Loading…",
    "tracks.view.send_to_playlist": "Send to playlist…"
    ```
  - [x] Add French and Spanish translations for all new keys (follow the same multi-language structure as the existing catalog).

- [x] **Task 3: Create `TracksBrowseView.ts`** (AC: 2–14)
  - [x] Create new file: `hifimule-ui/src/components/TracksBrowseView.ts`
  - [x] **Component structure** — model on `PlaylistCurationView.ts` but paginated:
    ```typescript
    import { fetchBrowseArtists, fetchBrowseArtist, fetchBrowseAlbums, fetchBrowseTracks, BrowseArtist, BrowseAlbum, BrowseTrack } from '../rpc';
    import { MediaCard } from './MediaCard';
    import { basketStore } from '../state/basket';
    import { t } from '../i18n';

    export class TracksBrowseView {
        private container: HTMLElement;
        private supportsPlaylistWrite: boolean;
        private selectedArtistId: string | null = null;
        private selectedArtistName: string | null = null;
        private selectedAlbumId: string | null = null;
        private selectedAlbumName: string | null = null;
        private artistLetter: string | null = null;
        private albumLetter: string | null = null;
        // Per-panel pagination state
        private artistState: PanelPaginationState;
        private albumState: PanelPaginationState;
        private trackState: TrackPaginationState;
        private basketUnsub: (() => void) | null = null;

        constructor(container: HTMLElement, supportsPlaylistWrite = false) { ... }
        async load(): Promise<void> { /* initial render + first page fetch for all panels */ }
        remount(): void { /* re-render into existing container (used for restore-on-back) */ }
        destroy(): void { /* cleanup basket subscription */ }
    }
    ```
  - [x] **Panel pagination state** — each panel is independently paginated:
    ```typescript
    interface PanelPaginationState {
        items: any[];     // BrowseArtist[] or BrowseAlbum[] or BrowseTrack[]
        total: number;
        startIndex: number;
        loading: boolean;
    }
    ```
  - [x] **Layout** — three-panel layout reusing global `curation-*` CSS classes:
    - Root: flex column, `height:100%`, `overflow:hidden`
    - `.curation-panels` row holds two sub-panels side by side
    - Each sub-panel (`curation-artist-panel` / `curation-album-panel`) is a flex-row shell: an inner scroll div (`#tracks-artist-scroll` / `#tracks-album-scroll`) + a narrow vertical A–Z sidebar (`#tracks-artist-az` / `#tracks-album-az`) on the right
    - Track panel (`#tracks-track-panel`) below, fixed at `flex:0 0 55%` so it is always visible and never jumps on load
  - [x] **Artist panel content** (AC: 4, 5, 13):
    - "All artists" row at top as the **first scrollable item** (`data-all-artists`, `.curation-all-artists` style, selected when `selectedArtistId === null`) — NOT sticky
    - Then loaded artist rows (`data-artist-id`, `.curation-artist-row` style)
    - A–Z strip: a narrow vertical sidebar (`width:2.5rem`) displayed as a 2-column grid of letter buttons, shown on the right of the panel when `artistState.total >= 20`; clicking an active letter deselects it
    - On scroll near bottom: autoload next page with a spinner shown immediately before the fetch
    - On letter click: reset pagination, reload artists, reset `selectedArtistId`
  - [x] **Album panel content** (AC: 7, 6, 13):
    - "All albums" row at top as the **first scrollable item** (`data-all-albums`, selected when `selectedAlbumId === null`) — NOT sticky
    - Then loaded album rows (`data-album-id`)
    - **TWO data sources** — this is critical:
      - `selectedArtistId === null`: paginated `browse.listAlbums({letter, startIndex, limit: 50})` with autoload-on-scroll
      - `selectedArtistId !== null`: single `browse.getArtist(selectedArtistId)` returns all albums at once (bounded by artist discography — same pattern as `loadArtistAlbums` in `library.ts:1498`)
    - A–Z strip: vertical sidebar, shown only when `selectedArtistId === null`; hidden when artist is selected (per-artist list is already bounded)
    - When artist changes: re-fetch albums, reset `selectedAlbumId`
  - [x] **Track panel content** (AC: 3, 8, 9, 10, 11):
    - Show each track row: title, artist name, album name
    - Basket toggle button (+ or -) based on `basketStore.has(track.id)`:
      - Device selected: clickable, adds/removes `{ id: track.id, name: track.title, type: 'Audio', sizeBytes, sizeTicks, childCount: 1 }` from basket
      - Device NOT selected (`basketStore` has no device? — use the same check as `renderListRow` in `library.ts:696`): render button as `disabled`
    - **Context menu** (AC: 10, 11): when `supportsPlaylistWrite`, add `contextmenu` listener calling `MediaCard.showItemContextMenu(x, y, track.id, track.title)`
    - **"Send to playlist…" button** (AC: 10, 11): when `supportsPlaylistWrite`, render a visible `<sl-icon-button name="collection-play" label="${t('tracks.view.send_to_playlist')}">` per track row, calling `MediaCard.openAddToPlaylistDialog(track.id, track.title)` on click
    - Pagination: autoload-on-scroll against `browse.listTracks({artistId?, albumId?, startIndex, limit: 200})`; a spinner is appended to the panel immediately before each autoload fetch so the loading state is always visible
    - Exhaustion check: `state.items.length < state.pagination.total` — for Subsonic unfiltered, `total` equals page length on the last page; use `page.tracks.length < limit` as secondary signal (consistent with 9.9 dev notes)
    - Panel height is fixed at `flex:0 0 55%` (overriding the CSS `max-height:40%`) so the track panel area is always visible and does not jump when content loads
    - Subscribe to `basketStore` events to re-render visible rows when basket changes (same pattern as `library.ts:786`)
  - [x] **"No device selected" detection** — check `basketStore.devicePath` (or the same mechanism `library.ts` uses). Looking at the existing `renderListRow` at [library.ts:646](hifimule-ui/src/library.ts:646), the disabled state for (+) buttons comes from... it does NOT check a device path — it just always renders. The no-device state in `library.ts` is controlled externally by `main.ts` disabling the entire mode bar. For `TracksBrowseView`, check if a `selectedDevicePath` or equivalent is available via the basket store. Looking at the basket store import: `import { basketStore } from '../state/basket';` — check `basket.ts` for a device path field. If none, mirror the disabled-button pattern from `MediaCard` which uses the same mechanism.
  - [x] **Scroll position tracking per panel**: Save the scroll top of each scrollable panel element in the instance (`artistScrollTop`, `albumScrollTop`, `trackScrollTop`). Restore on `remount()`.
  - [x] **autoload-on-scroll per panel**: Each panel `<div>` gets an `'scroll'` listener. On scroll near bottom, load the next page if not already loading. Store teardown refs (like `__scrollHandler` pattern in `library.ts`) to avoid listener leaks on `destroy()`.
  - [x] **Error handling**: On any fetch failure, show an inline `<sl-alert variant="danger">` within the relevant panel (do not crash other panels).

- [x] **Task 4: Wire `TracksBrowseView` into `library.ts`** (AC: 1, 2, 12, 14)
  - [x] **Import** at top of `library.ts`:
    ```typescript
    import { TracksBrowseView } from './components/TracksBrowseView';
    ```
    And import `fetchBrowseTracks` from `'./rpc'`.
  - [x] **Module-level instance** — add after the `state` declaration:
    ```typescript
    let _tracksBrowseView: TracksBrowseView | null = null;
    ```
  - [x] **`clearNavigationCache()`** — add `_tracksBrowseView?.destroy(); _tracksBrowseView = null;` (the view holds a basket subscription that must be cleaned up).
  - [x] **`loadModeRoot()`** — add the tracks case to the switch at [library.ts:932](hifimule-ui/src/library.ts:932):
    ```typescript
    case 'tracks':
        loadTracksView();
        break;
    ```
  - [x] **Add `loadTracksView()` function** — similar to `openCurationView` ([library.ts:1144](hifimule-ui/src/library.ts:1144)):
    ```typescript
    function loadTracksView(): void {
        const container = document.getElementById('library-content');
        if (!container) return;
        teardownListScrollHandler();
        if (_tracksBrowseView) {
            _tracksBrowseView.remount(); // restore in-place with saved scroll/selection
        } else {
            _tracksBrowseView = new TracksBrowseView(container, _supportsPlaylistWrite);
            _tracksBrowseView.load();
        }
    }
    ```
  - [x] **`renderViewToggle()`** — suppress the toggle when in tracks mode.
  - [x] **`modeLabel()`** — no code change needed — the new i18n key `library.mode.tracks` handles it automatically.
  - [x] **`listAutoloadSupported()`** — do NOT add `'tracks'` here. The tracks view manages its own scroll/load internally via `TracksBrowseView`.

- [x] **Task 5: "No device selected" basket integration** (AC: 8, 9)
  - [x] Read `hifimule-ui/src/state/basket.ts` — `basketStore` exposes no `devicePath`. `renderListRow` in library.ts does NOT disable basket buttons based on device selection; the mode bar is disabled globally by main.ts when no device is connected. `TracksBrowseView` mirrors this behavior: basket toggle is always enabled per-row.

- [x] **Task 6: i18n type declaration update** (AC: all)
  - [x] `hifimule-ui/src/i18n-catalog.d.ts` uses `Record<string, string>` catch-all — no change needed.
  - [x] `rtk tsc --noEmit` — only the pre-existing baseUrl deprecation warning, zero new errors.

- [x] **Task 7: Build and test gates**
  - [x] `rtk tsc --noEmit` — zero new errors (pre-existing baseUrl deprecation only).
  - [x] `npm run build` — only pre-existing `MediaCard.ts` activeContextMenu error; zero new errors.
  - [x] No lint script configured for this project (no pnpm, no eslint script in package.json).

## Dev Notes

### Scope Boundary — UI Only

This story is **UI only** (TypeScript). All daemon work — `BrowseMode::Tracks`, `TrackListFilter`/`TrackListPage`, `MediaProvider::list_tracks`, `browse.listTracks` RPC handler, `error.tracks_mode_unsupported` i18n key — is **Story 9.9** and is already **done** (baseline commit `5ca0d07`).

- ✅ In scope: `BrowseMode` TS union update, `fetchBrowseTracks` helper, `TracksBrowseView.ts`, `library.ts` wiring, `catalog.json` new keys.
- ❌ Out of scope: Any Rust file, any daemon-side change, any modifications to `hifimule-daemon/`.

### Current Code Anatomy (READ BEFORE TOUCHING)

#### `hifimule-ui/src/rpc.ts`

- **`BrowseMode` type (line 95)** — union string literal. Currently does NOT include `"tracks"`. Add it after `"playlists"`.
- **`BrowseTrack` interface (line 121–137)** — already fully defined with all fields the daemon returns (`id`, `title`, `artistId`, `artistName`, `albumId`, `albumName`, `trackNumber`, `duration`, `bitrateKbps`, `coverArtId`, `sizeBytes`, etc.). No new interface needed.
- **`fetchBrowseArtists` (line 153–165)** — accepts `letter?, libraryId?, startIndex?, limit?`. Returns `{ artists: BrowseArtist[], total: number }`.
- **`fetchBrowseArtist` (line 167–171)** — accepts `artistId`. Returns `{ artist: BrowseArtist, albums: BrowseAlbum[] }`. Use this for "artist selected → get their albums" (complete list, no pagination needed).
- **`fetchBrowseAlbums` (line 173–185)** — accepts `letter?, libraryId?, startIndex?, limit?`. **DOES NOT accept `artistId`** — use `fetchBrowseArtist` instead when an artist is selected. Returns `{ albums: BrowseAlbum[], total: number }`.
- **No existing `fetchBrowseTracks`** — add it (Task 1).

#### `hifimule-ui/src/library.ts`

- **`BrowseMode` import (line 2)** — import from `'./rpc'`. Once `rpc.ts` is updated, the type flows automatically.
- **`renderViewToggle()` (line 588–613)** — renders the grid/list toggle. Must suppress in tracks mode (Task 4).
- **`loadModeRoot()` switch (line 932–947)** — no `'tracks'` case exists yet. The `default:` clause is missing (fall-through would be a no-op since TypeScript's exhaustive check doesn't apply here). Add `case 'tracks': loadTracksView(); break;`.
- **`clearNavigationCache()` (line 98–111)** — call `_tracksBrowseView?.destroy()` here; the view holds a `basketStore` event listener that leaks if not cleaned up.
- **`openCurationView` (line 1144–1163)** — template for how a full-container view is mounted. Follow this exact pattern: `teardownListScrollHandler()`, `saveScroll()` (optional — tracks view handles its own scroll), instantiate view, call `load()`. The key difference: we keep the instance alive across mode switches (unlike curation views which are one-shot per playlist).
- **`listAutoloadSupported()` (line 799–813)** — DO NOT add `'tracks'`. The view manages its own scroll internally.
- **`modeLabel()` (line 35–37)** — calls `t('library.mode.${mode}')`. Works automatically once the i18n key is added.
- **`renderModeBar()` (line 407–439)** — calls `renderViewToggle()`. No direct change here, but `renderViewToggle()` will check the mode.

#### `hifimule-ui/src/components/PlaylistCurationView.ts`

The canonical reference implementation. Key patterns to reuse:

- **Three-panel layout** with `curation-panels` (flex row for artist+album), `curation-artist-panel`, `curation-album-panel`, `curation-track-panel` — these CSS classes already exist in the stylesheet. Check if they're scoped to `.curation-view` (bad) or global (good). If scoped, use the same class names inside a `tracks-view` root, or define parallel `tracks-*` classes.
- **"All artists" / "All albums" rows** — exact HTML pattern at [PlaylistCurationView.ts:109](hifimule-ui/src/components/PlaylistCurationView.ts:109) and [line 135](hifimule-ui/src/components/PlaylistCurationView.ts:135). Reuse the same class names and `role="button" tabindex="0" aria-pressed` pattern.
- **`escapeHtml` / `escapeAttr`** helpers — copy them directly into `TracksBrowseView.ts`.
- **Error display** via `<sl-alert variant="danger">` — reuse the pattern.
- Key difference: `PlaylistCurationView` holds ALL tracks in memory (loaded once). `TracksBrowseView` holds only loaded pages per panel — each panel independently paginates.

#### `hifimule-ui/src/components/MediaCard.ts`

- **`MediaCard.showItemContextMenu(x, y, itemId, itemName)`** (line 291) — already wired to open "Add to playlist…" dialog. Call from track row `contextmenu` event. Guard with `_supportsPlaylistWrite`.
- **`MediaCard.openAddToPlaylistDialog(trackId, trackName)`** (line 436) — opens the playlist picker dialog with "New playlist…" option. This is the exact function to call from the per-row "Send to playlist…" button.

#### `hifimule-ui/src/state/basket.ts`

- **Read this file** before Task 5. Understand how `basketStore.has(id)`, `basketStore.add(item)`, `basketStore.remove(id)` work, and how to subscribe to updates (`basketStore.addEventListener('update', handler)` — see library.ts:786).
- For the disabled state when no device is selected: look for a `devicePath` or `deviceId` field. If absent, track rows should reflect the same behavior as `MediaCard` grid cards and `renderListRow` in library.ts — both show the toggle button but the actual disabled state may come from a CSS class on a parent element (check `main.ts` for when the library panel gets a `no-device` class or similar).

#### `hifimule-i18n/catalog.json`

- The file is structured as a flat JSON object. The `library.mode.*` keys are at lines 59–66. Insert `"library.mode.tracks"` immediately after `"library.mode.playlists"` (line 61).
- The catalog includes all three languages (en/fr/es) in a unified flat structure. Follow the existing pattern — search for `"library.mode.artists"` to find the English entry and check if there is a separate fr/es section or if all languages are in the same key namespace. **Verify the catalog structure before editing.**

### Album Panel Data Source — Critical Decision

**THERE IS NO `artistId` FILTER ON `browse.listAlbums`.** The architecture only defines:
```
browse.listAlbums: { libraryId?, startIndex?, limit? } → { albums, total }
```

To get an artist's albums, use `browse.getArtist(artistId)` → `{ artist, albums: Album[] }`. This returns ALL albums for the artist (not paginated). This is the same pattern used in `library.ts::loadArtistAlbums` (line 1498–1537).

**Implementation decision:**
- `selectedArtistId === null` → album panel uses `fetchBrowseAlbums({letter?, startIndex, limit: 50})` with autoload-on-scroll
- `selectedArtistId !== null` → album panel calls `fetchBrowseArtist(selectedArtistId)` once, gets complete album list, no pagination needed (artist discography is bounded; typical artists have <50 albums)

This avoids needing a phantom `artistId` filter on `listAlbums` that doesn't exist.

### Track Panel — Exhaustion Detection

Story 9.9 established that for Subsonic unfiltered `list_tracks`, `total` equals the page length (not global library total). The UI uses "page length < limit" as a secondary exhaustion signal. In `TracksBrowseView`, after each `fetchBrowseTracks` call:

```typescript
const isExhausted = page.tracks.length < limit || trackState.startIndex + page.tracks.length >= page.total;
```

Use `isExhausted` to stop attempting further pagination.

### Page Cache / Restore-on-Back

The simplest restore pattern (per tech notes: "per-panel state lives in the component"):

```typescript
let _tracksBrowseView: TracksBrowseView | null = null;
```

- On `switchMode('tracks')`: `loadTracksView()` checks `_tracksBrowseView !== null` → calls `remount()` (re-render existing state)
- On `switchMode(other)` and then `switchMode('tracks')`: `_tracksBrowseView` still exists → `remount()` restores
- On `clearNavigationCache()`: `_tracksBrowseView?.destroy()` then `= null`

`remount()` on `TracksBrowseView` re-renders the current state into `container`, restoring the saved scroll positions for each panel.

### CSS — No New Stylesheet File

This story does NOT create a new CSS file. Use existing `curation-*` CSS classes (they are already available). If the existing curation styles don't cover the tracks view layout adequately (e.g., if they are scoped via `.curation-view` parent selectors), add minimal targeted rules inline or in a `<style>` tag inside the component (not a new stylesheet). Avoid scope creep into the CSS.

### Files to Touch

**Create (NEW):**
- `hifimule-ui/src/components/TracksBrowseView.ts` — new component (~250–350 lines)

**Modify (UPDATE):**
- `hifimule-ui/src/rpc.ts` — add `"tracks"` to `BrowseMode`, add `fetchBrowseTracks` (~15 lines)
- `hifimule-ui/src/library.ts` — import `TracksBrowseView` + `fetchBrowseTracks`, add `_tracksBrowseView` var, update `clearNavigationCache`, `loadModeRoot`, `loadTracksView`, `renderViewToggle` (~30 lines)
- `hifimule-i18n/catalog.json` — add 6 new i18n keys in en/fr/es (~18 lines)
- `hifimule-ui/src/i18n-catalog.d.ts` — if it exists and enumerates keys, add the 6 new keys

**Do not touch:**
- Any `hifimule-daemon/**` file — that's Story 9.9 (done).
- `hifimule-ui/src/components/PlaylistCurationView.ts` — read it as a reference; do not modify it.
- `hifimule-ui/src/components/MediaCard.ts` — read it as a reference; do not modify it.
- PRD, architecture, UX, or epics docs — already updated in the sprint change proposal commit.

### Previous Story Intelligence (from 9.9 and 9.8)

Relevant forward from recent review findings:

- **`BrowseMode::Tracks` variant is live** in the daemon since commit `5ca0d07`. The daemon's `browse.listModes` will return `"tracks"` for Jellyfin and OpenSubsonic providers. No daemon changes needed.
- **TypeScript baseUrl deprecation warning** (`tsconfig.json`) is pre-existing and expected — do NOT count it as a new error introduced by this story.
- **`println!("DEBUG: Jellyfin Response ...")` at `api.rs:343`** is pre-existing — irrelevant to this UI-only story, but noted for awareness.
- **Stale-mode race in `loadMoreForListView`** (9.8 review, deferred): when `state.browseMode` changes mid-load, the `loadMoreForListView` may apply results for the wrong mode. Since `TracksBrowseView` manages its own state and is NOT wired into `loadMoreForListView`, this race does not affect Story 9.10. However, be aware that `_tracksBrowseView.load()` makes async calls that could return after a mode switch. Guard async callbacks with a check that the view is still mounted (e.g., `if (!this.container.isConnected) return;`).
- **`get_songs_by_genre` 10k cap** (9.8 review): not relevant here since we use the server-paginated `listTracks` endpoint.
- **Subsonic total undercount**: For Subsonic unfiltered track listing, `total` equals page length. The UI uses "page length < limit" as exhaustion signal. Document this in `TracksBrowseView`'s track-panel autoload logic.

### Git Intelligence

Commit pattern for this project: `Story X.Y` commit → `Dev X.Y` commit → `Review X.Y` commit.

1. This story file lands as `Story 9.10` commit under `_bmad-output/implementation-artifacts/`.
2. Implementation as `Dev 9.10` commit.
3. Review as `Review 9.10` commit.

### Wire Contract Summary

`browse.listTracks` request (as established in 9.9):
```json
{
  "method": "browse.listTracks",
  "params": {
    "artistId": "abc123" | null,
    "albumId": "xyz789" | null,
    "letter": "A" | null,
    "startIndex": 0,
    "limit": 200
  }
}
```

Response:
```json
{
  "tracks": [ /* BrowseTrack[] */ ],
  "total": 1234,
  "startIndex": 0,
  "limit": 200
}
```

The TypeScript `BrowseTrack` interface in `rpc.ts` (lines 121–137) already maps this correctly — no new wire type needed.

### References

- [Source: _bmad-output/planning-artifacts/epics.md:2012](_bmad-output/planning-artifacts/epics.md:2012) — Story 9.10 ACs and Technical Notes.
- [Source: _bmad-output/planning-artifacts/architecture.md:298](_bmad-output/planning-artifacts/architecture.md:298) — `browse.*` RPC table (all methods + return shapes).
- [Source: _bmad-output/planning-artifacts/architecture.md:335](_bmad-output/planning-artifacts/architecture.md:335) — `BrowseMode` TS union (authoritative order).
- [Source: _bmad-output/planning-artifacts/architecture.md:369](_bmad-output/planning-artifacts/architecture.md:369) — Tracks Browse Mode provider contract.
- [Source: _bmad-output/planning-artifacts/ux-design-specification.md:93](_bmad-output/planning-artifacts/ux-design-specification.md:93) — "Tracks Browse View (dual-panel, paginated)" UX spec entry (§5.2).
- [Source: _bmad-output/planning-artifacts/sprint-change-proposal-2026-06-08-tracks-browse-mode.md](_bmad-output/planning-artifacts/sprint-change-proposal-2026-06-08-tracks-browse-mode.md) — proposal that introduced this story.
- [Source: _bmad-output/implementation-artifacts/9-9-tracks-browse-mode-provider-contract-and-daemon-rpc.md](_bmad-output/implementation-artifacts/9-9-tracks-browse-mode-provider-contract-and-daemon-rpc.md) — Story 9.9 (daemon dependency, all tasks done).
- [Source: hifimule-ui/src/rpc.ts:95](hifimule-ui/src/rpc.ts:95) — `BrowseMode` type (Task 1 target).
- [Source: hifimule-ui/src/library.ts:35](hifimule-ui/src/library.ts:35) — `modeLabel()`.
- [Source: hifimule-ui/src/library.ts:588](hifimule-ui/src/library.ts:588) — `renderViewToggle()` (Task 4 target — suppress for tracks mode).
- [Source: hifimule-ui/src/library.ts:799](hifimule-ui/src/library.ts:799) — `listAutoloadSupported()` (do NOT add tracks here).
- [Source: hifimule-ui/src/library.ts:923](hifimule-ui/src/library.ts:923) — `loadModeRoot()` switch (Task 4 target).
- [Source: hifimule-ui/src/library.ts:1144](hifimule-ui/src/library.ts:1144) — `openCurationView()` (mount pattern to follow).
- [Source: hifimule-ui/src/components/PlaylistCurationView.ts](hifimule-ui/src/components/PlaylistCurationView.ts) — canonical reference for three-panel layout patterns.
- [Source: hifimule-ui/src/components/MediaCard.ts:291](hifimule-ui/src/components/MediaCard.ts:291) — `showItemContextMenu()` (right-click dispatcher).
- [Source: hifimule-ui/src/components/MediaCard.ts:436](hifimule-ui/src/components/MediaCard.ts:436) — `openAddToPlaylistDialog()` (per-row "Send to playlist…" dispatcher).
- [Source: hifimule-ui/src/state/basket.ts](hifimule-ui/src/state/basket.ts) — `basketStore` (basket add/remove/has + device-selected state).

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

_none_

### Completion Notes List

- Created `TracksBrowseView.ts`: three-panel layout reusing `curation-*` CSS classes with independent pagination per panel, autoload-on-scroll with immediate spinner before each fetch, A-Z strips (threshold: 20), basket subscribe/unsub, per-panel error handling, scroll position save/restore.
- A–Z strips are vertical 2-column grid sidebars (`width:2.5rem`, `grid-template-columns:1fr 1fr`) on the right edge of the artist/album panels — not horizontal strips inside the scroll flow. Each panel is a flex-row shell with an inner scroll div and the sidebar alongside it.
- Track panel fixed at `flex:0 0 55%` (overrides `curation-track-panel` CSS `max-height:40%`) so it is always the same height and never jumps on load.
- "All artists" and "All albums" rows are regular first items in their scroll lists — not sticky.
- Extended `BrowseMode` union to include `"tracks"` in `rpc.ts`; added `fetchBrowseTracks` helper with four-field return type `{ tracks, total, startIndex, limit }`.
- Wired `TracksBrowseView` into `library.ts`: import, `_tracksBrowseView` module-level instance, `clearNavigationCache` destroy hook, `case 'tracks'` in `loadModeRoot`, `loadTracksView()` function, `renderViewToggle` mode guard.
- Added 6 i18n keys in EN/FR/ES: `library.mode.tracks` and `tracks.view.{all_artists,all_albums,no_tracks,loading,send_to_playlist}`.
- No-device basket: mirrored existing `renderListRow` behavior — basket toggle always enabled per-row.
- Pre-existing `MediaCard.ts` TS6133 error confirmed unchanged; zero new TypeScript errors introduced.

### File List

- `hifimule-ui/src/components/TracksBrowseView.ts` (NEW)
- `hifimule-ui/src/rpc.ts` (modified: BrowseMode + fetchBrowseTracks)
- `hifimule-ui/src/library.ts` (modified: import, _tracksBrowseView, clearNavigationCache, loadModeRoot, loadTracksView, renderViewToggle)
- `hifimule-i18n/catalog.json` (modified: 6 keys × 3 languages)

### Review Findings

_to be filled by reviewer_

## Change Log

- 2026-06-08: Story created. Daemon dependency (Story 9.9) is complete at baseline commit `5ca0d07`. Ultimate context engine analysis completed — comprehensive developer guide created.
- 2026-06-08: Implementation complete. TracksBrowseView component created; rpc.ts, library.ts, and catalog.json updated. All ACs satisfied. TypeScript: zero new errors.
- 2026-06-08: Post-review refinements — autoload spinner added before each fetch; A–Z strips moved to vertical 2-column grid sidebar on panel right edge; track panel fixed at 55% height; "All artists"/"All albums" rows changed from sticky to regular scrollable items.
