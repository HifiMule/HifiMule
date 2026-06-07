# Sprint Change Proposal: List View — Autoload on Scroll

**Date:** 2026-06-07
**Trigger:** Story 9.7 — Virtualized List/Table Browse View (done)
**Proposed by:** Alexis
**Scope:** Minor — direct implementation by Developer agent

---

## 1. Issue Summary

### Problem Statement

Story 9.7 was implemented with a "load all before render" strategy: when the user switches to list view, `loadAllForListView()` pages through the entire library in batches of 200, blocking rendering until every item is fetched. For large libraries (e.g. 5 000 artists), this means 25 sequential API calls with a spinner before the list appears — defeating the intent of "rapid scanning without waiting."

### Discovery Context

Discovered during manual smoke-testing of the completed Story 9.7 implementation on a real Jellyfin library. The virtual scroller itself performs correctly; only the data-loading strategy before first render is the problem.

### Root Cause

Story AC 6 was explicitly specced as "remaining pages are fetched from the daemon before the virtual list is rendered." The implementation was correct to that spec, but the spec itself was wrong at scale.

### Additional Design Decision

A–Z in both grid and list view should be a **server-side filter** only, not a quick-nav scroll. The `scrollToLetter()` function (which was only valid when all items were pre-loaded) should be removed. A–Z behavior is now uniform across both views.

---

## 2. Impact Analysis

### Epic Impact

| Epic | Impact |
|---|---|
| Epic 9 — Browse & Library Explore | Story 9.7 must be reopened and its loading strategy revised. All other stories in Epic 9 are unaffected. |
| All other epics | None. |

### Story Impact

| Story | Change |
|---|---|
| 9.7 (done → reopen) | Replace `loadAllForListView()` with autoload-on-scroll. Remove `scrollToLetter()`. Update AC 6 and Technical Notes. |

### Artifact Conflicts

| Artifact | Section | Change Required |
|---|---|---|
| `epics.md` | Story 9.7 AC 6 | Rewrite: autoload on scroll instead of pre-load before render |
| `epics.md` | Story 9.7 Technical Notes | Remove "single scroll surface for the full result set"; remove A–Z scroll-offset note |
| `prd.md` | NFR — List/Table View Rendering | Clarify "avoiding pagination" means no page-boundary UI friction, not no incremental server fetching |
| `ux-design-specification.md` | §5.1 List/Table Browse View | Replace "renders the full result set" with progressive-loading language; update A–Z description |
| `9-7-virtualized-list-table-browse-views.md` | Status, Tasks, Dev Notes | Reopen story; revise Task 2, 3, 4; update Dev Notes |

### Technical Impact

Pure frontend change. No daemon RPCs, no protocol changes, no backend work.

---

## 3. Recommended Approach

**Option 1 — Direct Adjustment** ✅ Selected

Reopen Story 9.7. Revise the loading strategy in `library.ts`. All virtual scroller infrastructure (DOM windowing, row rendering, basket integration, view toggle) remains intact — only the data-fetch trigger changes.

**Rationale:**
- Low risk: the virtual scroller itself is not changing
- Medium effort: ~4 focused changes in `library.ts` + artifact updates
- No epic restructuring or new stories needed
- Fixes the user-visible performance issue directly

---

## 4. Detailed Change Proposals

### 4.1 `epics.md` — Story 9.7

**AC 6:**

OLD:
```
**Given** list view is active and all items are not yet loaded
**When** I switch to list view
**Then** remaining pages are fetched from the daemon (up to `total`) before the virtual list is rendered.
```

NEW:
```
**Given** list view is active
**When** I switch to list view
**Then** the list renders immediately with the currently loaded items.
**And** as I scroll toward the end of the loaded rows, the next page is fetched automatically from the daemon and appended to the list.
**And** this continues until all items (up to `total`) are loaded.
```

**AC for A–Z (replacing the scroll-to-letter AC):**

OLD:
```
**Given** the browse page has a quick-nav (A–Z) control
**When** I am in list/table view
**Then** selecting a letter scrolls the virtualized list to the matching position.
```

NEW:
```
**Given** the browse page has an A–Z filter control
**When** I select a letter in either grid or list view
**Then** the view fetches and displays only items starting with that letter (server-side filter), identical to grid view behavior.
```

**Technical Notes:**

OLD:
```
- Implement windowed/virtualized rendering — no pagination; a single scroll surface for the full result set.
- A–Z quick-nav must drive the virtualized list scroll offset correctly.
```

NEW:
```
- Implement windowed/virtualized rendering with autoload-on-scroll: render immediately with the loaded page; fetch the next page when the user scrolls within ~5 rows of the loaded boundary.
- The scroller element height is set to `state.pagination.total * VIRTUAL_ROW_HEIGHT` from the start, so the scrollbar reflects the full expected size; height updates as the total is refined by responses.
- A–Z is a server-side filter in both grid and list view. There is no client-side scroll-to-letter behavior.
```

---

### 4.2 `prd.md` — NFR List/Table View Rendering

OLD:
```
List and table browse views must use virtualized (windowed) rendering to remain responsive with libraries of thousands of items, avoiding pagination while keeping memory and scroll performance within the app's existing UI responsiveness targets.
```

NEW:
```
List and table browse views must use virtualized (windowed) rendering to remain responsive with libraries of thousands of items. The list view uses autoload-on-scroll (next page fetches automatically as the user approaches the loaded boundary) rather than a "Load More" button, avoiding visible page-boundary friction while keeping memory and scroll performance within the app's existing UI responsiveness targets.
```

---

### 4.3 `ux-design-specification.md` — §5.1 List/Table Browse View

OLD:
```
A virtualized (windowed) list/table rendering mode for Artist and Album browse pages, available as a user-toggled alternative to the paginated <sl-card> grid. Renders the full result set in a single scrollable surface — no pagination — using only the visible rows in the DOM at any time for smooth performance across libraries of thousands of items. The view-mode toggle (grid vs list) is stored in local UI state per browse mode. The A–Z quick-nav control drives the virtualized scroll offset when in list view. Breadcrumb navigation, synced badges, and basket add interactions are identical to grid view. Data is shared from the existing browse.* RPC layer; switching views does not re-fetch from the daemon.
```

NEW:
```
A virtualized (windowed) list/table rendering mode for Artist and Album browse pages, available as a user-toggled alternative to the paginated <sl-card> grid. Renders immediately with the currently loaded items; as the user scrolls to the bottom, the next page is fetched automatically (autoload-on-scroll) and appended — no "Load More" button. Only visible rows are mounted in the DOM at any time for smooth performance across libraries of thousands of items. The scrollbar reflects the full expected library size from the first render. The view-mode toggle (grid vs list) is stored in local UI state per browse mode. The A–Z control is a server-side filter in both views (no scroll-to-letter). Breadcrumb navigation, synced badges, and basket add interactions are identical to grid view. Data is shared from the existing browse.* RPC layer; switching views does not re-fetch from the daemon.
```

---

### 4.4 `9-7-virtualized-list-table-browse-views.md` — Revised Tasks

**Status:** `done` → `in-progress`

**Task 2 — Virtual scroller (revised):**

The scroll handler gains a near-bottom check:
```typescript
// Near bottom = within LOAD_AHEAD rows of the loaded boundary
const LOAD_AHEAD = 5;
const loadedBoundary = state.items.length * VIRTUAL_ROW_HEIGHT;
if (
    scrollTop + viewportH >= loadedBoundary - (LOAD_AHEAD * VIRTUAL_ROW_HEIGHT) &&
    state.items.length < state.pagination.total &&
    !state.listLoading
) {
    loadMoreForListView(state.browseMode as 'artists' | 'albums');
}
```

The scroller element height uses `state.pagination.total` (not `items.length`):
```typescript
scroller.style.height = `${state.pagination.total * VIRTUAL_ROW_HEIGHT}px`;
```

Store a reference to the scroller element so `loadMoreForListView` can update its height after a page arrives.

**Task 3 — Replace `loadAllForListView` with `loadMoreForListView`:**

```typescript
// Replaces loadAllForListView entirely
async function loadMoreForListView(mode: 'artists' | 'albums') {
    if (state.listLoading || state.items.length >= state.pagination.total) return;
    state.listLoading = true;
    try {
        const startIndex = state.items.length;
        if (mode === 'artists') {
            const r = await fetchBrowseArtists(undefined, undefined, startIndex, 200);
            state.items = [...state.items, ...mapArtists(r.artists)];
            state.pagination.total = r.total;
        } else {
            const r = await fetchBrowseAlbums(undefined, undefined, startIndex, 200);
            state.items = [...state.items, ...mapAlbums(r.albums)];
            state.pagination.total = r.total;
        }
        state.pagination.startIndex = state.items.length;
        // Update scroller height and repaint
        const scroller = document.querySelector<HTMLElement>('.media-list');
        if (scroller) scroller.style.height = `${state.pagination.total * VIRTUAL_ROW_HEIGHT}px`;
        // paint() is called by the stored scrollHandler reference
        (document.getElementById('library-content') as any)?.__listPaint?.();
    } finally {
        state.listLoading = false;
    }
}
```

Add `listLoading: boolean` to `AppState` (reset to `false` in `clearNavigationCache`).

**Task 4 — Remove `scrollToLetter`, unify A–Z behavior:**

- Remove `scrollToLetter()` function entirely.
- Remove the `inListView` branch in `renderQuickNav()` — A–Z always calls `loadArtistsByLetter` / `loadAlbumsByLetter` regardless of view mode.
- When a letter filter is active in list view and the user scrolls to the bottom, `loadMoreForListView` must NOT be called (letter-filtered sets are already fully loaded from the server in one fetch). Guard: check `state.activeLetter === null` before triggering autoload.

**`setViewMode` — simplified:**

Remove the `shouldLoadAll` block entirely. `setViewMode` just updates `listViewModes` and calls `renderCurrentView()`. No pre-loading.

**Remove `ensureRootListItemsLoaded`** — no longer needed.

---

## 5. Implementation Handoff

**Scope Classification:** Minor — Developer agent direct implementation.

**Files to change:**
- `hifimule-ui/src/library.ts` (primary)
- `_bmad-output/planning-artifacts/epics.md`
- `_bmad-output/planning-artifacts/prd.md`
- `_bmad-output/planning-artifacts/ux-design-specification.md`
- `_bmad-output/implementation-artifacts/9-7-virtualized-list-table-browse-views.md`
- `_bmad-output/implementation-artifacts/sprint-status.yaml` (reopen 9.7)

**Success Criteria:**
- Switching to list view on a 5 000-item library renders the first page within the normal load time (same as grid view first page)
- Scrolling to the bottom of the loaded rows triggers a background fetch; rows appear without any button press
- A–Z filter works identically in grid and list views (server-side letter filter)
- Switching between grid and list with data already loaded is instant (no re-fetch)
- Letter-filtered list view does not trigger autoload on scroll (already a full set)
- TypeScript build passes with zero errors

**Handoff to:** Developer agent — implement changes listed in Section 4.4 to `library.ts`, then update artifact documents.
