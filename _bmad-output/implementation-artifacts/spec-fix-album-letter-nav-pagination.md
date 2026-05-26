---
title: 'Fix album letter nav disappearing + no load-more when filtering by letter'
type: 'bugfix'
created: '2026-05-26'
status: 'done'
baseline_commit: '3fc069a'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** On the Albums tab, the A–Z letter navigation bar disappears after leaving and returning to the tab, because the cache restore path sets the wrong total field. On both Albums and Artists tabs, selecting a letter makes the "Load More" button disappear entirely, so users with more items than the initial 200-item batch cannot page through the rest.

**Approach:** Fix the wrong field assignment in `loadAlbums`'s cache path; remove the `activeLetter === null` guard from the load-more render condition; update both letter loaders to track total and startIndex; add a letter-aware append path inside `loadMore`.

## Boundaries & Constraints

**Always:**
- Keep the initial letter fetch at limit=200 (existing behaviour, avoids multiple round trips for typical library sizes)
- Subsequent "load more" pages after a letter selection use `state.pagination.limit` (50)
- `state.pagination.startIndex` must always equal `state.items.length` before `loadMore` increments it, so use `state.items.length` instead of `+= limit`
- Do not alter any logic for genres, playlists, recentlyAdded, frequentlyPlayed, or recentlyPlayed modes

**Ask First:** None — all decisions are self-contained bug fixes with clear correct behaviour

**Never:**
- Do not change the letter-bar visibility threshold (< 20 items hides the bar)
- Do not add server-side pagination support to `fetchBrowseArtists` / `fetchBrowseAlbums` (already supported; just use it)
- Do not reset `state.activeLetter` inside the new append path

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Albums tab revisit (cache hit) | Navigate away then back to Albums tab | Letter nav bar renders; `albumViewTotal` = cached.total | — |
| Artist letter selected, all fit | Click "A", server returns 50/50 total | Items shown, no load-more button | — |
| Artist letter selected, more exist | Click "A", server returns 200/340 total | Items shown, "Load More (140 remaining)" button visible | — |
| Artist "Load More" after letter | Click "Load More" with activeLetter="A", startIndex=200 | Fetches artists A startIndex=200 limit=50, appends results | renderError on fetch failure |
| Album letter selected, more exist | Click "B", server returns 200/250 total | Items shown, "Load More (50 remaining)" button visible | — |
| Letter toggle (deselect) | Click same letter again | Clears filter, reloads full artist/album list | — |

</frozen-after-approval>

## Code Map

- `hifimule-ui/src/library.ts` -- all state, rendering, and loader functions for the library browser (the only file changed)

Key functions:
- `renderQuickNav()` (line ~448) -- renders A–Z bar; reads `state.artistViewTotal` / `state.albumViewTotal`
- `renderGrid()` (line ~478) -- renders items grid + conditional load-more button
- `loadAlbums()` (line ~702) -- loads albums; cache path bug here (line ~714)
- `loadArtistsByLetter()` (line ~635) -- loads filtered artists; missing total/startIndex update
- `loadAlbumsByLetter()` (line ~668) -- loads filtered albums; missing startIndex update
- `loadMore()` (line ~1360) -- dispatches next-page loads; ignores active letter

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-ui/src/library.ts` -- In `loadAlbums()`, cache-hit branch (around line 714): change `state.artistViewTotal = 0` to `state.albumViewTotal = cached.total` -- fixes letter bar vanishing on back-navigate
- [x] `hifimule-ui/src/library.ts` -- In `renderGrid()` (line ~504): remove `state.activeLetter === null &&` from the load-more condition so the button shows regardless of active letter -- enables load-more visibility
- [x] `hifimule-ui/src/library.ts` -- In `loadArtistsByLetter()`: after successful fetch, add `state.pagination.total = result.total` and `state.pagination.startIndex = result.artists.length` -- gives loadMore correct position
- [x] `hifimule-ui/src/library.ts` -- In `loadAlbumsByLetter()`: after successful fetch, add `state.pagination.startIndex = result.albums.length` (total already set) -- gives loadMore correct position
- [x] `hifimule-ui/src/library.ts` -- In `loadMore()`: replace `state.pagination.startIndex += state.pagination.limit` with `state.pagination.startIndex = state.items.length`; route `artists` and `albums` cases through a new `appendByLetter` helper when `state.activeLetter` is set -- enables paginated letter-filtered loads
- [x] `hifimule-ui/src/library.ts` -- Add `async function appendByLetter(mode: 'artists' | 'albums', letter: string)`: fetches the next page for the current letter using `state.pagination.startIndex` / `state.pagination.limit`, appends mapped results to `state.items`, updates `state.pagination.total`, calls `renderGrid` -- implements the actual load-more-by-letter logic

**Acceptance Criteria:**
- Given the user is on the Albums tab with ≥20 albums and navigates to another tab and back, when the Albums tab loads from cache, then the A–Z letter nav bar is visible
- Given the user selects a letter on the Artists tab and the server has more results than the initial 200, when the grid renders, then a "Load More (N remaining)" button is visible
- Given the user selects a letter on the Albums tab and the server has more results than the initial 200, when the grid renders, then a "Load More (N remaining)" button is visible
- Given the user clicks "Load More" while a letter filter is active, when the load completes, then the new items are appended to the existing list without clearing the letter selection or the nav bar
- Given the user clicks a letter that has ≤ the initial 200 limit results, when the grid renders, then no "Load More" button is shown
- Given the user clicks the same letter again (deselect), when the grid renders, then the full unfiltered list reloads with no active letter

## Design Notes

The `appendByLetter` function mirrors the structure of `loadArtists(false)` / `loadAlbums(false)` but passes the active letter and appends results:

```typescript
async function appendByLetter(mode: 'artists' | 'albums', letter: string) {
    const container = document.getElementById('library-content');
    if (!container) return;
    state.loading = true;
    renderModeBar();
    try {
        const startIndex = state.pagination.startIndex;
        const limit = state.pagination.limit;
        if (mode === 'artists') {
            const result = await fetchBrowseArtists(letter, undefined, startIndex, limit);
            state.items = [...state.items, ...mapArtists(result.artists)];
            state.pagination.total = result.total;
        } else {
            const result = await fetchBrowseAlbums(letter, undefined, startIndex, limit);
            state.items = [...state.items, ...mapAlbums(result.albums)];
            state.pagination.total = result.total;
        }
        renderGrid(state.items);
    } catch (e) {
        renderError(e as Error);
    } finally {
        state.loading = false;
        renderModeBar();
    }
}
```

## Verification

**Commands:**
- `cd hifimule-ui && npm run build` -- expected: zero TypeScript errors, clean build

**Manual checks (if no CLI):**
- Navigate to Albums tab → confirm letter bar appears
- Navigate away (e.g. Artists tab) → navigate back to Albums → confirm letter bar still appears
- Click any letter on Albums → confirm load-more appears if total > fetched count
- Click any letter on Artists → confirm load-more appears if total > fetched count
- Click load-more while letter is active → confirm items append, letter bar stays, button count updates

## Spec Change Log

## Suggested Review Order

**Albums letter bar fix (cache restore path)**

- Wrong field fixed: `artistViewTotal → albumViewTotal`; root of the disappearing nav bar
  [`library.ts:717`](../../hifimule-ui/src/library.ts#L717)

**Load-more visibility**

- Guard removed: `activeLetter === null &&` was suppressing the button on all letter views
  [`library.ts:504`](../../hifimule-ui/src/library.ts#L504)

**Letter loader state tracking**

- Artists letter loader: now records `total` and `startIndex` after first 200-item fetch
  [`library.ts:659`](../../hifimule-ui/src/library.ts#L659)

- Albums letter loader: now records `startIndex` after first 200-item fetch (`total` was already set)
  [`library.ts:695`](../../hifimule-ui/src/library.ts#L695)

**Paginated letter append (new function)**

- `appendByLetter`: fetches next page for the active letter, appends results, guards against stale DOM and advances `startIndex`
  [`library.ts:1363`](../../hifimule-ui/src/library.ts#L1363)

**`loadMore` dispatch**

- `startIndex` now derived from `state.items.length`; routes to `appendByLetter` when a letter is active
  [`library.ts:1396`](../../hifimule-ui/src/library.ts#L1396)
