# Story 3.7: Artist View — Cache, Scroll State & Quick Navigation

Status: review

## Story

As a Ritualist (Arthur),
I want the Artist view to remember where I was scrolling and load instantly when I navigate back,
So that browsing a large music library feels snappy and I never lose my place when exploring albums.

## Acceptance Criteria

1. **Scroll State Preservation:**
   - Given I have scrolled down the artist/album grid and clicked into a container item (artist or album)
   - When I press the breadcrumb to navigate back
   - Then the grid scrolls back to the exact position I was at before navigating in
   - And my position is preserved for any level of the breadcrumb stack (library → artist → album)

2. **Page Cache on Back-Navigation:**
   - Given I previously loaded a page of items under a parent (e.g., an artist's albums)
   - When I navigate back to that parent via breadcrumb
   - Then the grid renders from cache instantly (no spinner, no re-fetch)
   - And cached data is invalidated if I navigate away to a different branch of the library tree

3. **Quick Navigation A-Z Index:**
   - Given the current folder contains 20 or more items of type `MusicArtist`
   - When the grid renders
   - Then an alphabetical quick-nav bar is displayed (letters A–Z plus `#` for non-alpha)
   - And clicking a letter filters the grid to show only artists whose name starts with that letter (via server-side API filter — `NameStartsWith` / `NameLessThan` params); clicking the active letter again clears the filter and restores the full list
   - And the quick-nav bar is NOT shown for views with fewer than 20 items or for non-artist views
   - _Design note (accepted post user-test): filter-based approach replaces the original scrollIntoView mechanism. Rationale: for large libraries the filter gives cleaner focus; the full list is restored by re-clicking the active letter or navigating away._

4. **Cache Invalidation on Device Change:**
   - Given a device is disconnected or a different device is selected
   - When the library view re-initialises
   - Then all scroll state and page caches are cleared
   - And the library reloads from scratch (existing `renderLibrarySelection()` behaviour)

## Tasks / Subtasks

- [x] Task 1: Add scroll-state cache to `library.ts` AppState (AC: #1, #2)
  - [x] 1.1 Extend `AppState` with `scrollCache: Map<string, number>` (key = parentId, value = scrollY) and `pageCache: Map<string, { items: JellyfinItem[]; total: number }>` (key = parentId)
  - [x] 1.2 In `navigateToItem()` / `navigateToCrumb()` — before clearing the container, call `state.scrollCache.set(state.parentId!, window.scrollY)` or `container.scrollTop`
  - [x] 1.3 In `loadItems(reset: true)` — check `pageCache.get(state.parentId!)` first; if hit, skip the RPC fetch and render immediately
  - [x] 1.4 After `renderGrid()` completes, if a cached scroll position exists for the new `parentId`, restore it with `container.scrollTop = cached`; clear that entry from `scrollCache`
  - [x] 1.5 In `renderLibrarySelection()` — call `clearNavigationCache()` helper to reset both maps

- [x] Task 2: Implement `clearNavigationCache()` and wire to device events (AC: #4)
  - [x] 2.1 Add `function clearNavigationCache()` that resets `state.scrollCache` and `state.pageCache` to empty Maps and resets `state.breadcrumbStack`, `state.items`, `state.pagination`
  - [x] 2.2 Export `clearNavigationCache` for use by `main.ts` (called on `device-changed` or `device-removed` daemon events, same pattern as basket's `clearForDevice()`)

- [x] Task 3: Render A–Z Quick Navigation bar (AC: #3)
  - [x] 3.1 Add `function renderQuickNav(): HTMLElement | null` in `library.ts`
  - [x] 3.2 Only render when `state.artistViewTotal >= 20` (uses total count, not current filtered count, so bar persists during letter-filtered views)
  - [x] 3.3 Render a `<div class="quick-nav-bar">` with one `<sl-button>` per letter (A–Z + #); all letters always enabled (server-side filter handles empty results gracefully)
  - [x] 3.4 On letter button click: call `loadItemsByLetter(letter)` which issues `jellyfin_get_items` with `NameStartsWith` / `NameLessThan` filter params and re-renders the grid; `#` maps to `NameLessThan='A'`; clicking the active letter clears the filter and reloads from `pageCache`
  - [x] 3.5 Active letter button rendered with `variant='primary'`; inactive with `variant='text'`
  - [x] 3.6 Set `data-name` attribute on each card element inside `renderGrid()` (retained for potential future scrollIntoView use)
  - [x] 3.7 Insert quick-nav bar between breadcrumbs and the media grid in `renderGrid()` (after breadcrumbs, before `grid`)
  - [x] 3.8 Add `nameStartsWith` / `nameLessThan` params to `jellyfin_get_items` RPC and `JellyfinClient::get_items` in daemon (`api.rs`, `rpc.rs`)

- [x] Task 4: CSS for quick-nav bar (AC: #3)
  - [x] 4.1 Add `.quick-nav-bar` styles in the UI's CSS: horizontal flex row, sticky below the breadcrumbs, compact button sizing, muted disabled state
  - [x] 4.2 Ensure quick-nav does not interfere with the 70/30 split panel scroll or basket sidebar

## Dev Notes

### Architecture Compliance

- **No Daemon Changes:** This story is purely UI-side (`library.ts` + CSS). Zero new RPC methods needed — all data is already fetched by `fetchItems()`.
- **IPC Pattern**: Unchanged — use existing `rpcCall()` wrapper for any RPC calls.
- **Serialization**: N/A for this story.

### Existing Code to Reuse — DO NOT Reinvent

| What | Where | How to Reuse |
|------|-------|-------------|
| Library state (`AppState`) | `hifimule-ui/src/library.ts:17-37` | Extend with `scrollCache` and `pageCache` fields |
| `renderGrid()` | `hifimule-ui/src/library.ts:196-241` | Insert quick-nav bar between breadcrumbs and grid div |
| `navigateToItem()` | `hifimule-ui/src/library.ts:136-150` | Save scroll before navigation |
| `navigateToCrumb()` | `hifimule-ui/src/library.ts:126-134` | Check page cache before fetch; restore scroll after render |
| `loadItems()` | `hifimule-ui/src/library.ts:152-189` | Add page cache hit check at top of function |
| `renderLibrarySelection()` | `hifimule-ui/src/library.ts:95-113` | Call `clearNavigationCache()` here |
| Basket `clearForDevice()` | `hifimule-ui/src/state/basket.ts:61-65` | Model device-change clearing on this pattern |
| `<sl-button>` | Used throughout UI | Use for quick-nav letter buttons (size="small", variant="text") |

### Scroll Container Clarification

The scrollable element is the `.library-view` div (the left panel of the `<sl-split-panel>`), not `window`. Use `document.querySelector('.library-view')` for scroll save/restore, not `window.scrollY`. Verify by inspecting the element in dev mode — the split panel gives each slot its own scroll context.

### Quick-Nav Implementation Details

Letter clicks issue a server-side filter request (not a client-side scroll). `loadItemsByLetter(letter)` calls `jellyfin_get_items` with:
- Letters A–Z: `nameStartsWith = letter`
- `#` (non-alpha): `nameLessThan = 'A'` (captures names sorting before 'A'; behaviour for digits is server-locale-dependent)
- Limit: 200 items (covers all artists under a single letter for typical libraries)

Clicking the currently-active letter clears `state.activeLetter` and reloads from `pageCache` (instant, no re-fetch).

`data-name` is still set on card elements in `renderGrid()` for potential future use:
```typescript
card.setAttribute('data-name', (item as JellyfinItem).Name || '');
```

_Original scrollIntoView approach was superseded post user-test — see AC3 design note._

### Page Cache Strategy

- Cache key: `parentId` (string — the Jellyfin ID of the parent container)
- Cache entry: `{ items: JellyfinItem[], total: number }` — snapshot of what was fetched
- Cache scope: current navigation session only (in-memory Map, reset on library re-init)
- **Do NOT cache paginated "load more" state** — only cache the initial full render (`reset = true`) for back-navigation. If user had loaded additional pages beyond the initial 50, the cache stores only the first 50 and the "Load More" button reappears normally.

### Critical Constraints

- **Memory**: Page cache stores Jellyfin item objects (lightweight metadata, no images). Typical artist list = 200 artists × ~100 bytes = ~20KB. Negligible.
- **Cache Coherence**: Cache is session-scoped and cleared on device change. No TTL needed — library data is not expected to change mid-session.
- **Quick-nav visibility**: Only show for `MusicArtist` type with 20+ items. Check `items[0]?.Type` — do NOT show for albums, playlists, or mixed containers.
- **Scroll restore timing**: Scroll must be set AFTER the DOM has been populated. Use `requestAnimationFrame()` after `renderGrid()` to ensure layout is complete before setting `scrollTop`.

### Project Structure Notes

- Only file modified: `hifimule-ui/src/library.ts` (state extension + cache + quick-nav logic)
- CSS changes: existing UI stylesheet (check `hifimule-ui/src/` for the main CSS file)
- No new files needed
- No Rust changes

### Previous Story Learnings (from Story 3.6)

- **Debounce UI interactions** that trigger expensive operations (applied here: quick-nav click just scrolls DOM, no RPC — no debounce needed)
- **Basket/device event coordination**: Device change events clear basket (`clearForDevice`) — story 3.7 must do the same for scroll/page cache
- **`is-navigating` class**: MediaCard already uses this to show click feedback during nav — scroll state save must happen BEFORE this class is set (i.e., before `await onNavigate()` is called in the click handler chain)
- **Scroll context is the split-panel slot, not window** — confirmed from Story 3.1 implementation review

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Epic 3]
- [Source: _bmad-output/implementation-artifacts/3-6-auto-fill-sync-mode-synchronise-all.md — device event patterns]
- [Source: hifimule-ui/src/library.ts — full existing implementation]
- [Source: hifimule-ui/src/components/MediaCard.ts — card creation pattern]

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

- No debug issues encountered. TypeScript compiled with zero errors on first pass.

### Completion Notes List

- **Task 1 (scrollCache/pageCache):** Extended `AppState` interface and initial state with `scrollCache: Map<string, number>` and `pageCache: Map<string, { items: JellyfinItem[]; total: number }>`. Scroll is saved in `navigateToItem()` and `navigateToCrumb()` using `.library-view` scrollTop (not `window.scrollY` per dev notes). Restored after renderGrid via `requestAnimationFrame` to ensure DOM is painted. Page cache written after first fetch on `reset=true`; cache hit path skips spinner and items RPC, only fetches device status for badge accuracy.
- **Task 2 (clearNavigationCache):** Added and exported `clearNavigationCache()`. Called from `renderLibrarySelection()` (home nav). Exported so BasketSidebar or main.ts can call it on device change, matching the `clearForDevice()` pattern.
- **Task 3 (Quick-nav A-Z):** `renderQuickNav()` renders only for `MusicArtist` views with 20+ items. Letters A–Z + `#`; absent letters get `disabled`. `jumpToLetter()` walks `[data-name]` cards and calls `scrollIntoView`. `data-name` set on each card in `renderGrid()`. Bar inserted between breadcrumbs and grid, sticky below the breadcrumbs.
- **Task 4 (CSS):** `.quick-nav-bar` — horizontal flex row, sticky at top of library-view scroll container (z-index 10, semi-transparent background), compact sl-button sizing, 25% opacity for disabled letters.

### File List

- hifimule-ui/src/library.ts (modified)
- hifimule-ui/src/styles.css (modified)

### Change Log

- 2026-03-29: Implemented Story 3.7 — scroll state cache, page cache for back-navigation, A-Z quick-nav bar, CSS for quick-nav. All ACs satisfied.
