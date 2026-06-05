# Story 9.7: Virtualized List/Table Browse Views

Status: ready-for-dev

## Story

As a Ritualist (Arthur),
I want to browse Artists and Albums as a list/table view in addition to the current grid,
So that I can scan libraries of thousands of items quickly without waiting for pagination.

## Acceptance Criteria

1. **Given** the Artist or Album browse page is open (at root level, no breadcrumbs) **When** I toggle to list/table view **Then** all items render in a single scrollable list using virtualized windowed rendering **And** scroll performance remains smooth for libraries of thousands of items.

2. **Given** the list view is active **When** I scroll **Then** only the rows currently visible in the viewport are mounted in the DOM at any time (windowed rendering, not full-DOM).

3. **Given** the browse page has a quick-nav (A–Z) control **When** I am in list/table view **And** I click a letter **Then** the virtualized list scrolls to the first item whose name starts with that letter (client-side scroll, no daemon re-fetch).

4. **Given** I am in list/table view **When** I click an item **Then** drill-down, breadcrumb, and basket-add behaviors are identical to grid view.

5. **Given** I toggle between grid and list view **When** data has already been fetched **Then** the view switches instantly without re-fetching from the daemon.

6. **Given** list view is active and all items are not yet loaded **When** I switch to list view **Then** remaining pages are fetched from the daemon (up to `total`) before the virtual list is rendered.

7. **Given** I switch browse mode (e.g., Artists → Albums) **When** I return to Artists **Then** the view mode (grid or list) for Artists is remembered independently.

8. **Given** I am drilled into an artist's albums or an album's tracks **Then** the view toggle is hidden — it only appears at the Artists/Albums root level.

## Tasks / Subtasks

- [ ] Task 1: Extend AppState and add view-mode toggle (AC: 1, 5, 7, 8)
  - [ ] Add `listViewModes: Map<BrowseMode, 'grid' | 'list'>` to `AppState` in `library.ts`.
  - [ ] Add `VIRTUAL_ROW_HEIGHT = 56` constant (pixels, fixed-height rows required for offset math).
  - [ ] Add `renderViewToggle()` that appends a grid/list icon button pair to `#browse-mode-bar`; show only when `browseMode` is `artists` or `albums` AND `breadcrumbStack.length === 0`.
  - [ ] Wire toggle buttons to call `setViewMode(mode: 'grid' | 'list')` which updates `listViewModes`, re-renders the toggle, and calls `renderCurrentView()`.
  - [ ] Add i18n keys `library.viewToggle.grid` and `library.viewToggle.list` to `hifimule-i18n/catalog.json` (en/fr/es).

- [ ] Task 2: Implement virtual scroller (AC: 1, 2, 6)
  - [ ] Add `renderList(items: BrowseDisplayItem[])` to `library.ts` (parallel to `renderGrid`).
  - [ ] Structure: prepend breadcrumbs and quick-nav (same as `renderGrid`), then create `div.media-list` (relative-positioned, `overflow: hidden`, height = `items.length * VIRTUAL_ROW_HEIGHT`px).
  - [ ] On scroll of `#library-content`, compute `firstVisible = Math.floor(scrollTop / VIRTUAL_ROW_HEIGHT)` and `lastVisible = Math.ceil((scrollTop + viewportHeight) / VIRTUAL_ROW_HEIGHT)`.
  - [ ] Render only rows `[firstVisible, lastVisible + overscan]` (overscan = 3); position each row absolutely at `top: index * VIRTUAL_ROW_HEIGHT`.
  - [ ] Remove the existing scroll event listener before attaching a new one each time `renderList` is called; store the handler reference so it can be torn down on `renderGrid` / mode switch.
  - [ ] Implement `renderListRow(item, index)` → `div.media-list-row` with small thumbnail (32×32, via `getImageUrl`), name, subtitle, basket-add button, and click handler for `navigateToBrowseItem`. Mirror the basket-add logic from `MediaCard` — call `basketStore.add/remove` identically.

- [ ] Task 3: Full-dataset load for list view (AC: 6)
  - [ ] Add `async function loadAllForListView(mode: 'artists' | 'albums')` that pages through the daemon until `state.items.length >= state.pagination.total`, using `fetchBrowseArtists` or `fetchBrowseAlbums` with incrementing `startIndex`.
  - [ ] Call this before `renderList` when switching to list view and `state.items.length < state.pagination.total`.
  - [ ] Respect letter filter: if `state.activeLetter` is set, the items are already a letter-filtered full set — no additional loading needed.

- [ ] Task 4: Quick-nav integration for list view (AC: 3)
  - [ ] In `renderQuickNav()`, attach a different handler when list view is active: `scrollToLetter(letter)` instead of `loadArtistsByLetter / loadAlbumsByLetter`.
  - [ ] `scrollToLetter(letter)`: scan `state.items` for the first item with `name.toUpperCase().startsWith(letter === '#' ? '0123456789' : letter)`; compute `targetScrollTop = foundIndex * VIRTUAL_ROW_HEIGHT`; set `#library-content.scrollTop = targetScrollTop`.
  - [ ] For `#` (non-alpha): match names that start with a digit (0–9) — consistent with existing provider behavior.
  - [ ] Do NOT call `loadArtistsByLetter` / `loadAlbumsByLetter` when in list view (those re-fetch from daemon and replace the full dataset).

- [ ] Task 5: CSS for list view (AC: 1, 4)
  - [ ] Add `.media-list` — `position: relative; width: 100%;` (height set inline).
  - [ ] Add `.media-list-row` — `position: absolute; left: 0; right: 0; height: 56px; display: flex; align-items: center; gap: 0.75rem; padding: 0 0.5rem; cursor: pointer; border-bottom: 1px solid var(--surface-border-soft)` with hover state.
  - [ ] Add `.media-list-row__thumb` — `width: 36px; height: 36px; border-radius: var(--sl-border-radius-small); background-size: cover; background-position: center; flex-shrink: 0; background-color: var(--sl-color-neutral-800)`.
  - [ ] Add `.media-list-row__info` — `flex: 1; min-width: 0; overflow: hidden`.
  - [ ] Add `.media-list-row__name` — `font-weight: 500; white-space: nowrap; overflow: hidden; text-overflow: ellipsis`.
  - [ ] Add `.media-list-row__subtitle` — `font-size: 0.8rem; color: var(--ink-dim); white-space: nowrap; overflow: hidden; text-overflow: ellipsis`.
  - [ ] Add `.view-toggle-group` — button group container in browse-mode-bar (right-aligned).

- [ ] Task 6: View-mode persistence and `renderCurrentView()` helper (AC: 5, 7)
  - [ ] Add `function renderCurrentView()` that calls `renderGrid(state.items)` or `renderList(state.items)` based on `listViewModes.get(state.browseMode)`.
  - [ ] Replace all direct `renderGrid(state.items)` calls in artists/albums loaders with `renderCurrentView()` — so switching view mode mid-browse is instant.
  - [ ] Keep `renderGrid` direct calls in non-artists/albums modes (genres, playlists, history, favorites) unchanged — list view only applies to artists/albums root.

- [ ] Task 7: Verification (AC: 1–8)
  - [ ] Run `rtk tsc` from `hifimule-ui` — zero TypeScript errors.
  - [ ] Manually smoke test: Artists grid → switch to list → scroll a 500+ artist library smooth.
  - [ ] Manually verify: A–Z click in list view scrolls to correct position (no spinner, no re-fetch).
  - [ ] Manually verify: clicking artist in list view drills down to albums grid (or list if albums view mode is list).
  - [ ] Manually verify: basket-add button in list row adds/removes item (visual update when store changes).
  - [ ] Manually verify: toggle between grid and list — no re-fetch, instant switch.
  - [ ] Manually verify: toggle is hidden when drilled into artist albums.
  - [ ] Manually verify: Albums list view works identically to Artists list view.

## Dev Notes

### Current Code Anatomy (READ BEFORE TOUCHING)

**`hifimule-ui/src/library.ts`** — entire browse surface. Key facts:

- `state: AppState` holds all browse state. `AppState.items: BrowseDisplayItem[]` is the current page's items; `AppState.pagination` tracks `startIndex / limit / total`.
- `renderGrid(items)` is called by every loader — it builds `div.media-grid` (CSS `repeat(auto-fill, minmax(190px, 1fr))`). `renderGrid` also calls `createBreadcrumbs()` and `renderQuickNav()`.
- `renderModeBar()` manages `#browse-mode-bar`. It is called frequently (loading start/end). The view toggle should be rendered via an appended `div.view-toggle-group` within this function — only when mode is `artists` or `albums` and `breadcrumbStack.length === 0`.
- `loadArtists(reset)` / `loadAlbums(reset)` end with `renderGrid(state.items)` — replace these calls with `renderCurrentView()`.
- `loadArtistsByLetter(letter)` / `loadAlbumsByLetter(letter)` re-fetch from the daemon and replace `state.items` with the letter subset. These must NOT be called in list view for the quick-nav (scroll-only behavior instead). `renderQuickNav()` already knows the active mode — pass view-mode context to determine handler.
- `clearNavigationCache()` resets all state except `browseMode` and `availableModes` — add `listViewModes` to the preserved state (view mode should survive server reconnects and mode switches).
- Scroll cache uses `#library-content.scrollTop`. The virtual scroller reads this same property. No conflict.
- `navigateToBrowseItem(item)` handles drill-down for all types — call this from list row click handlers (identical to grid).

**`hifimule-ui/src/components/MediaCard.ts`** — creates `<sl-card>` for the grid. Do **NOT** use `MediaCard` for list rows — its layout assumes square aspect ratio and card chrome. Implement list rows directly in `library.ts` with `div.media-list-row`. Replicate the basket toggle logic: call `basketStore.has(itemId)` for initial state, call `basketStore.add/remove` on click, and subscribe to `basketStore.addEventListener('update', ...)` to sync visual state. See `MediaCard.ts:137–250` for the exact pattern.

**`hifimule-ui/src/main.ts:169`** — `#browse-mode-bar` is a plain `<div>`. `renderModeBar()` in `library.ts` owns its content. The view toggle belongs here.

**`hifimule-ui/index.html:37`** — `#library-content` has CSS `overflow-y: auto`. The virtual scroller attaches its scroll handler to this element. `getComputedStyle(container).height` or `container.clientHeight` gives viewport height for windowed rendering.

**`hifimule-i18n/catalog.json`** — add `library.viewToggle.grid` and `library.viewToggle.list` to all three language objects (`en`, `fr`, `es`).

### Virtual Scroller Implementation Pattern

```typescript
const VIRTUAL_ROW_HEIGHT = 56; // px — fixed; changing breaks offset math
const OVERSCAN = 3;            // extra rows above/below viewport

function renderList(items: BrowseDisplayItem[]) {
    const container = document.getElementById('library-content');
    if (!container) return;

    container.innerHTML = '';
    if (state.breadcrumbStack.length > 0) container.appendChild(createBreadcrumbs());
    const qn = renderQuickNav(); if (qn) container.appendChild(qn);

    const totalHeight = items.length * VIRTUAL_ROW_HEIGHT;
    const scroller = document.createElement('div');
    scroller.className = 'media-list';
    scroller.style.height = `${totalHeight}px`;

    function paint() {
        const scrollTop = container.scrollTop;
        const viewportH = container.clientHeight;
        const first = Math.max(0, Math.floor(scrollTop / VIRTUAL_ROW_HEIGHT) - OVERSCAN);
        const last  = Math.min(items.length - 1, Math.ceil((scrollTop + viewportH) / VIRTUAL_ROW_HEIGHT) + OVERSCAN);

        // Remove rows outside window
        scroller.querySelectorAll<HTMLElement>('.media-list-row').forEach(row => {
            const idx = Number(row.dataset.idx);
            if (idx < first || idx > last) row.remove();
        });

        // Add rows inside window
        const existing = new Set(
            [...scroller.querySelectorAll<HTMLElement>('.media-list-row')].map(r => Number(r.dataset.idx))
        );
        for (let i = first; i <= last; i++) {
            if (!existing.has(i)) scroller.appendChild(renderListRow(items[i], i));
        }
    }

    const scrollHandler = () => paint();
    container.addEventListener('scroll', scrollHandler);
    // Store reference for teardown on next renderGrid/renderList call
    (container as any).__listScrollHandler = scrollHandler;

    container.appendChild(scroller);
    paint(); // initial render
}

// Teardown: call in renderGrid() before clearing innerHTML
function teardownListScrollHandler() {
    const c = document.getElementById('library-content');
    if (c && (c as any).__listScrollHandler) {
        c.removeEventListener('scroll', (c as any).__listScrollHandler);
        delete (c as any).__listScrollHandler;
    }
}
```

`renderGrid` should call `teardownListScrollHandler()` at its start to clean up any live list handler.

### Quick-Nav in List View

```typescript
// In renderQuickNav(), determine handler based on active view mode:
const inListView = (listViewModes.get(state.browseMode) ?? 'grid') === 'list';

btn.addEventListener('click', () => {
    if (inListView) {
        scrollToLetter(letter);  // NEW: client-side scroll
    } else {
        if (isArtists) loadArtistsByLetter(letter);
        else loadAlbumsByLetter(letter);
    }
});

function scrollToLetter(letter: string) {
    const container = document.getElementById('library-content');
    if (!container) return;
    const isHash = letter === '#';
    const idx = state.items.findIndex(item => {
        const first = item.name.charAt(0).toUpperCase();
        return isHash ? /[0-9]/.test(first) : first === letter;
    });
    if (idx >= 0) container.scrollTop = idx * VIRTUAL_ROW_HEIGHT;
}
```

### i18n Additions

Add to `hifimule-i18n/catalog.json` under all three language keys:
```json
"library.viewToggle.grid": "Grid view",
"library.viewToggle.list": "List view"
```
French: `"Grille"` / `"Liste"`. Spanish: `"Cuadrícula"` / `"Lista"`.

### No New Daemon RPCs

This is a **pure UI rendering story**. No changes to `hifimule-daemon`. No new RPC methods. All data comes from existing `browse.listArtists` / `browse.listAlbums` calls.

### Loading All Items for List View

When switching to list view while `state.items.length < state.pagination.total`:

```typescript
async function loadAllForListView(mode: 'artists' | 'albums') {
    // Page through until fully loaded; show spinner in container
    while (state.items.length < state.pagination.total) {
        const startIndex = state.items.length;
        if (mode === 'artists') {
            const r = await fetchBrowseArtists(state.activeLetter ?? undefined, undefined, startIndex, 200);
            state.items = [...state.items, ...mapArtists(r.artists)];
            state.pagination.total = r.total;
        } else {
            const r = await fetchBrowseAlbums(state.activeLetter ?? undefined, undefined, startIndex, 200);
            state.items = [...state.items, ...mapAlbums(r.albums)];
            state.pagination.total = r.total;
        }
    }
}
```

Use limit=200 (same as letter-filter fetch calls in the existing code, see `library.ts:657,692`).

### Basket Toggle in List Rows

For simplicity, list row basket-add only needs the non-fetch path: all artists and albums in the list view already have `childCount` set from the daemon response. See `mapArtists()`/`mapAlbums()` in `library.ts:120–176` — `childCount` is populated from `albumCount`/`trackCount`. The `needsFetch` branch in `MediaCard` fires only when `childCount` or `sizeBytes` is missing — for browse items in this context that won't happen. Still, implement the same guard for safety.

### Previous Story Patterns (9.6)

- 9.6 was daemon-only (subsonic provider). No UI precedent from that story.
- The relevant UI patterns come from earlier stories: `renderGrid` / `MediaCard` from Stories 3.x/9.x.
- `rtk tsc` from `hifimule-ui` sometimes fails if `npx` is not on PATH — use `./node_modules/.bin/tsc` as fallback (see 9.6 debug log).

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story-9.7]
- [Source: _bmad-output/planning-artifacts/sprint-change-proposal-2026-06-05.md]
- [Source: _bmad-output/planning-artifacts/ux-design-specification.md#5.1-Component-Strategy]
- [Source: hifimule-ui/src/library.ts] (full browse surface — primary file for this story)
- [Source: hifimule-ui/src/components/MediaCard.ts] (basket-add logic to replicate in list rows)
- [Source: hifimule-ui/src/styles.css] (`.media-grid`, `.quick-nav-bar` — reference for new CSS)
- [Source: hifimule-ui/src/main.ts:169] (`#browse-mode-bar` div)
- [Source: hifimule-i18n/catalog.json] (i18n catalog — add new keys here)

## Dev Agent Record

### Agent Model Used

_To be filled by dev agent_

### Debug Log References

_To be filled by dev agent_

### Completion Notes List

_To be filled by dev agent_

### File List

- hifimule-ui/src/library.ts
- hifimule-ui/src/styles.css
- hifimule-i18n/catalog.json
- _bmad-output/implementation-artifacts/9-7-virtualized-list-table-browse-views.md
- _bmad-output/implementation-artifacts/sprint-status.yaml

## Change Log

- 2026-06-05: Story created from approved sprint-change-proposal-2026-06-05 (Selection-as-Playlist & List Curation Views).
