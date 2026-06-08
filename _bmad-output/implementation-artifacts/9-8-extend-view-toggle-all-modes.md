# Story 9.8: Extend Grid/Table Toggle to All Browse Modes and Drill-Down Levels

Status: ready

## Story

As a Ritualist (Arthur),
I want the grid/table toggle to work on every browse page and drill-down level,
So that I can use my preferred view mode consistently across all library content — not just at the Artists/Albums root.

## Acceptance Criteria

1. **Given** any browse mode is active (artists, albums, playlists, genres, recentlyAdded, frequentlyPlayed, recentlyPlayed, favorites) **When** the browse area renders **Then** the view toggle (grid/list) is always visible in the browse-mode bar.

2. **Given** I am drilled into a sub-level (e.g., albums within an artist, tracks within an album) **When** the sub-level content renders **Then** the view toggle remains visible and the active mode (grid or list) applies.

3. **Given** I toggle to list view **When** I switch browse mode or navigate into/out of a sub-level **Then** the global toggle state is preserved — all levels and modes use the same grid/list preference.

4. **Given** list view is active **When** I click a sub-level item (album row drills to tracks; track row adds to basket) **Then** drill-down and basket-add behaviors are identical to grid view.

5. **Given** list view is active on a mode without autoload (playlists, genres, history/favorites, or any sub-level with breadcrumbs) **Then** the list renders what is currently loaded; autoload-on-scroll is not triggered (that behavior remains exclusive to artists/albums root).

## Tasks / Subtasks

- [ ] Task 1: Remove mode/depth guards from `renderViewToggle()` (AC: 1, 2)
  - [ ] In `hifimule-ui/src/library.ts`, locate `renderViewToggle()` (~line 588).
  - [ ] Remove the condition `(state.browseMode === 'artists' || state.browseMode === 'albums') && state.breadcrumbStack.length === 0` from the `showToggle` variable (~lines 593–596).
  - [ ] Keep the `!state.loading` guard — toggle should still be hidden while data is loading.
  - [ ] The toggle now renders for any browse mode at any depth when not loading.

- [ ] Task 2: Remove mode/depth guard from `renderCurrentView()` (AC: 1, 2, 3)
  - [ ] In `hifimule-ui/src/library.ts`, locate `renderCurrentView()` (~line 821).
  - [ ] Remove the `(state.browseMode === 'artists' || state.browseMode === 'albums') && state.breadcrumbStack.length === 0` condition so that list view is used whenever `state.listViewMode === 'list'`, regardless of mode or depth.
  - [ ] `renderGrid` remains the fallback when `listViewMode === 'grid'`.

- [ ] Task 3: Verify autoload is unaffected (AC: 5)
  - [ ] Confirm that `renderList()` at ~line 758 still sets `rootMode` to `null` for any mode that is not `'artists'` or `'albums'`, which already suppresses autoload-on-scroll for those contexts. No code change required here.
  - [ ] Confirm that `loadMoreForListView` is only called when `rootMode !== null` — no change needed.

- [ ] Task 4: TypeScript check (AC: 1–5)
  - [ ] Run `rtk tsc` from `hifimule-ui` — zero new TypeScript errors.

- [ ] Task 5: Smoke test
  - [ ] Toggle grid/list on Playlists tab — list renders rows, no crash.
  - [ ] Toggle grid/list on Genres tab — list renders rows.
  - [ ] Toggle grid/list on Recently Added (or any available history mode) — list renders rows.
  - [ ] Navigate into an artist, toggle to list — albums render as rows with drill-down working.
  - [ ] Navigate into an album's track list, toggle to list — tracks render as rows with basket-add working.
  - [ ] Toggle back to grid from any of the above — instant switch, no re-fetch.
  - [ ] Switch browse mode while in list view — toggle state preserved (still list).
