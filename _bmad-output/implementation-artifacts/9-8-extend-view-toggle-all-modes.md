# Story 9.8: Extend Grid/Table Toggle to All Browse Modes and Drill-Down Levels

Status: ready-for-dev

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
  - [ ] In `hifimule-ui/src/library.ts`, locate `renderViewToggle()` (line ~588).
  - [ ] Remove the condition `(state.browseMode === 'artists' || state.browseMode === 'albums') && state.breadcrumbStack.length === 0` from the `showToggle` variable (lines 593–596).
  - [ ] Keep the `!state.loading` guard — toggle should still be hidden while data is loading.
  - [ ] Result: `const showToggle = !state.loading;`

- [ ] Task 2: Remove mode/depth guard from `renderCurrentView()` (AC: 1, 2, 3)
  - [ ] In `hifimule-ui/src/library.ts`, locate `renderCurrentView()` (line ~821).
  - [ ] Remove the `(state.browseMode === 'artists' || state.browseMode === 'albums') && state.breadcrumbStack.length === 0` condition so that list view is used whenever `state.listViewMode === 'list'`, regardless of mode or depth.
  - [ ] Result: `if (mode === 'list') { renderList(state.items); } else { renderGrid(state.items); }`
  - [ ] `renderGrid` remains the fallback when `listViewMode === 'grid'`.

- [ ] Task 3: Verify autoload is unaffected (AC: 5)
  - [ ] Confirm that `renderList()` at line ~758 still sets `rootMode` to `null` for any mode that is not `'artists'` or `'albums'`, which already suppresses autoload-on-scroll for those contexts. No code change required here.
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

## Dev Notes

### Current Code Anatomy (READ BEFORE TOUCHING)

**`hifimule-ui/src/library.ts`** — entire browse surface. Two functions need guard removals:

**`renderViewToggle()` (line ~588):**
```typescript
// CURRENT — guards toggle to artists/albums root only:
const showToggle =
    (state.browseMode === 'artists' || state.browseMode === 'albums') &&
    state.breadcrumbStack.length === 0 &&
    !state.loading;

// TARGET — show toggle on any mode/depth, only hide while loading:
const showToggle = !state.loading;
```

**`renderCurrentView()` (line ~821):**
```typescript
// CURRENT — applies list view to artists/albums root only:
if (
    mode === 'list' &&
    (state.browseMode === 'artists' || state.browseMode === 'albums') &&
    state.breadcrumbStack.length === 0
) {
    renderList(state.items);
} else {
    renderGrid(state.items);
}

// TARGET — apply list view to any mode/depth:
if (mode === 'list') {
    renderList(state.items);
} else {
    renderGrid(state.items);
}
```

**`AppState.listViewMode`** — already a single global `'grid' | 'list'` value (line 57). Story 9.7 dev notes specified a per-mode `Map<BrowseMode, 'grid' | 'list'>` in its initial spec, but the actual implementation uses a single scalar — confirmed at `library.ts:57` and `library.ts:82`. No state structure change needed.

**`renderList()` / `loadMoreForListView` — autoload behavior (line ~758):**
```typescript
// renderList() captures rootMode at render time:
const rootMode = state.browseMode === 'artists' || state.browseMode === 'albums'
    ? state.browseMode as 'artists' | 'albums'
    : null;

// scroll handler only calls loadMoreForListView when rootMode is non-null:
if (rootMode && !state.listLoading && state.items.length < state.pagination.total) {
    // ... triggers autoload
}
```
`rootMode` is computed inside `renderList()` from `state.browseMode` at the time of rendering. When the user is on playlists, genres, favorites, history modes, or any sub-level with breadcrumbs, `rootMode` is `null` and autoload-on-scroll is automatically suppressed. **No change needed here** — AC 5 is satisfied for free.

### What `renderList` Does When Called on Non-artists/albums Modes

When list view is triggered for playlists, genres, recently added, etc.:
- `state.items` contains whatever was already loaded (the current page)
- `renderList(state.items)` renders them as virtual rows (same row layout as artists/albums)
- `rootMode` evaluates to `null` inside `renderList` → no autoload
- Users see what's loaded; there is no load-more trigger

This means for paginated modes (recentlyAdded, etc.), only the first loaded page will show in list view. That is acceptable per the AC — list view on modes without autoload renders what is currently loaded.

### Files to Touch

- `hifimule-ui/src/library.ts` — only file requiring code changes (2 guard removals, ~4 lines total)

No new files. No daemon changes. No CSS changes. No i18n changes.

### Story 9.7 Learnings Relevant to This Story

- `state.listViewMode` is a scalar, not a Map — the per-mode Map was in the spec but not implemented; actual code uses a single global value. Do not introduce a Map.
- The `teardownListScrollHandler()` in `renderGrid()` already handles cleanup when switching from list to grid — no changes needed for cleanup behavior.
- `rtk tsc` from `hifimule-ui` may show 1 pre-existing warning (`tsconfig baseUrl deprecation`). That is expected and pre-existing — not caused by your changes.
- Context menu for playlist write support is already wired in `renderListRow()` — works for any item type.

### Project Structure Notes

- This is a pure UI rendering concern: no daemon RPCs, no new state fields, no new basket entity types.
- Both `renderViewToggle()` and `renderCurrentView()` are located in `hifimule-ui/src/library.ts` — the single file for the entire browse surface.
- The two guard conditions being removed are symmetric: both reference the same `(state.browseMode === 'artists' || state.browseMode === 'albums') && state.breadcrumbStack.length === 0` pattern.

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story-9.8]
- [Source: _bmad-output/planning-artifacts/sprint-change-proposal-2026-06-08-extend-view-toggle.md]
- [Source: _bmad-output/implementation-artifacts/9-7-virtualized-list-table-browse-views.md] (previous story — list view foundation)
- [Source: hifimule-ui/src/library.ts:588] (`renderViewToggle` — Task 1 target)
- [Source: hifimule-ui/src/library.ts:821] (`renderCurrentView` — Task 2 target)
- [Source: hifimule-ui/src/library.ts:758] (`renderList` rootMode — Task 3 verify)

## Dev Agent Record

### Agent Model Used

_to be filled by dev agent_

### Debug Log References

### Completion Notes List

### File List
