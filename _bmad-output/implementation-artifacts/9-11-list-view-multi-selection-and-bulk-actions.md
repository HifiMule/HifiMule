---
baseline_commit: fcc2513
---

# Story 9.11: List View Multi-Selection & Bulk Actions

Status: ready-for-dev

## Story

As a Ritualist (Arthur),
I want to select multiple artists or albums in the list view and act on them all at once,
So that I can build my basket or a playlist in seconds instead of clicking every row.

## Acceptance Criteria

1. **Given** the list/table view is active and a row represents an artist or album (resolved type `MusicArtist` or `MusicAlbum`)
   **When** the row renders
   **Then** it displays a leading selection checkbox (visible on hover/focus, and always visible while any selection is active).

2. **Given** I click a row's checkbox or Ctrl/Cmd-click the row
   **Then** the row's selection toggles without navigating into the item.

3. **Given** a row is the selection anchor and I Shift-click another row
   **Then** all selectable rows between the two indices (inclusive) become selected.

4. **Given** at least one row is selected
   **Then** a bulk action bar appears in the browse area showing the selection count, an "Add to basket" button, an "Add to playlist…" button (only when `supports_playlist_write` is true), and a "Clear" affordance.
   **And** all per-row single-item actions continue to work unchanged.

5. **Given** I click "Add to basket" with N items selected
   **Then** items already in the basket are skipped, counts/sizes for the remaining items are fetched in a single batched `jellyfin_get_item_counts` + `jellyfin_get_item_sizes` call pair, each item is added to the basket with existing semantics (artist entity items per Story 3.9), a success toast reports added/skipped counts, and the selection clears.

6. **Given** I click "Add to playlist…" with N items selected
   **Then** the existing playlist picker dialog (Story 11.7) opens; choosing an existing playlist calls `playlist.addItems { playlistId, itemIds: [all N ids] }`, choosing "New playlist" opens the create dialog and calls `playlist.create { name, itemIds: [all N ids] }`; on success the playlists cache is invalidated, a toast confirms, and the selection clears.

7. **Given** no device is selected (`selectedDevicePath === null`)
   **Then** "Add to basket" renders disabled (mirroring per-row (+) behavior); "Add to playlist…" remains available when `supports_playlist_write` is true.

8. **Given** rows are selected and I scroll far enough that selected rows unmount and remount (virtualization), or autoload appends pages
   **Then** selection state is preserved and remounted rows render as selected.

9. **Given** rows are selected
   **When** I change browse mode, drill into an item, change the A–Z filter, toggle to grid view, or press Escape
   **Then** the selection and the bulk action bar are cleared.

10. **Given** keyboard-only navigation
    **Then** checkboxes are focusable and toggleable via Space, the bulk bar buttons are reachable in tab order, and the selection count is announced via an ARIA-live region.

## Tasks / Subtasks

- [ ] **Task 1: Selection state in `library.ts`** (AC: 2, 3, 8, 9)
  - [ ] Add to the `AppState` interface ([library.ts:43](hifimule-ui/src/library.ts:43)–59) and the `state` initializer (line 69):
    ```typescript
    selectedIds: Set<string>;            // init: new Set()
    selectionAnchorIdx: number | null;   // init: null
    ```
  - [ ] Selection key is `item.basketId ?? item.id` — the SAME id `renderListRow` already computes as `itemId` at [library.ts:656](hifimule-ui/src/library.ts:656) and uses for `basketStore.has`. Never key by index or DOM node.
  - [ ] Selectability predicate helper:
    ```typescript
    function isSelectableListItem(item: BrowseDisplayItem): boolean {
        const resolved = item.basketType ?? item.type;
        return resolved === 'MusicArtist' || resolved === 'MusicAlbum';
    }
    ```
    `FavoriteArtist`/`FavoriteAlbum`, `MusicGenre`, `Playlist`, and `Audio` rows are NOT selectable in v1 (no checkbox rendered, skipped by Shift-range).
  - [ ] Add a single `clearSelection()` helper: empties `state.selectedIds`, nulls `selectionAnchorIdx`, updates/removes the bulk bar, and repaints mounted rows (remove all `.media-list-row` then call `(content as any).__listPaint?.()` — same pattern as the basket update handler at [library.ts:819](hifimule-ui/src/library.ts:819)–822). No-op fast path when the set is already empty.

- [ ] **Task 2: Checkbox + selection interactions in `renderListRow`** (AC: 1, 2, 3, 4, 8, 10)
  - [ ] In `renderListRow` ([library.ts:655](hifimule-ui/src/library.ts:655)–764), for selectable items only, prepend a leading native `<input type="checkbox">` with class `media-list-row__check`, `checked = state.selectedIds.has(itemId)`, and `aria-label` = item name (use `escapeHtml`-safe DOM assignment, not innerHTML). Native checkboxes are focusable and Space-toggleable for free (AC 10).
  - [ ] Add `is-checked` class to the row when selected (alongside the existing basket `is-selected` class at line 660 — do NOT reuse `is-selected`, it means "in basket").
  - [ ] Checkbox `click` handler: `e.stopPropagation()` (so the row click navigation at lines 729–740 never fires), then toggle `state.selectedIds`, set `state.selectionAnchorIdx = index`, sync row class + bulk bar.
  - [ ] Extend the existing row click handler (lines 729–740) BEFORE the navigation branch:
    - `e.ctrlKey || e.metaKey` → toggle selection, set anchor to this index, `return` (no navigation).
    - `e.shiftKey && state.selectionAnchorIdx !== null` → select all selectable rows in `state.items` between anchor and this index inclusive (anchor stays put), `return` (no navigation).
    - Plain click keeps current navigation behavior unchanged.
    - The existing `isBtn` composedPath guard (line 731) must also skip `INPUT` elements so checkbox clicks never navigate (defense-in-depth with stopPropagation).
  - [ ] Suppress text-selection artifacts on Shift-click: add a `mousedown` listener on the row that calls `e.preventDefault()` when `e.shiftKey` and a selection anchor exists (or set `user-select: none` on `.media-list-row` in CSS — pick one).
  - [ ] Shift-range looks up items ONLY from `state.items` (virtualized rows unmount — the DOM never holds the full list). Indices are stable because autoload is append-only.
  - [ ] Virtualization survival (AC 8) comes for free: `paint()` ([library.ts:782](hifimule-ui/src/library.ts:782)–798) recreates rows via `renderListRow`, which reads `state.selectedIds`. No extra wiring. Verify, don't re-implement.

- [ ] **Task 3: Bulk action bar** (AC: 4, 7, 10)
  - [ ] New `renderBulkBar()` in `library.ts`: a `div.bulk-action-bar` mounted as a sibling of the `.media-list` scroller inside `#library-content`, inserted by `renderList` ([library.ts:768](hifimule-ui/src/library.ts:768)–827) after the quick-nav (line 776) and before the scroller. `#library-content` is the scroll container, so `position: sticky; top: 0` keeps the bar pinned above the list.
  - [ ] Contents:
    - Count span with `aria-live="polite"` — text from `t('library.selection.count', { count })` (AC 10).
    - "Add to basket" `<sl-button>` with class **`basket-toggle-btn`** — this makes the existing rule `#library-content.device-locked .basket-toggle-btn` ([styles.css:1341](hifimule-ui/src/styles.css:1341)) disable it automatically when no device is selected (`BasketSidebar` toggles `device-locked` on `#library-content` at [BasketSidebar.ts:852](hifimule-ui/src/components/BasketSidebar.ts:852) when `selectedDevicePath === null`). That IS the "mirroring per-row (+) behavior" mechanism of AC 7. Do not invent a new device-state probe.
    - "Add to playlist…" `<sl-button>` — rendered ONLY when `_supportsPlaylistWrite` is true (module flag set via `setPlaylistWriteCapability`, [library.ts:31](hifimule-ui/src/library.ts:31)–34). NOT gated by device-locked (AC 7).
    - "Clear" affordance (`<sl-button variant="text">` or icon-button) → `clearSelection()`.
  - [ ] Re-render/update the bar on every selection change (create when size goes 0→1, update count text in place, remove when size→0). Keep it cheap — selection toggles happen rapidly.
  - [ ] Teardown: `renderList` already wipes `#library-content` (`content.innerHTML = ''`, line 773) so the bar dies with the list on re-render; no leak risk (the bar holds no document-level listeners). Escape handling is Task 5.
  - [ ] AC 4 regression guard: per-row (+)/(−) toggle, row navigation, context menu, and playlist curate button must behave exactly as before when no selection is active.

- [ ] **Task 4: Bulk "Add to basket" handler** (AC: 5, 7)
  - [ ] Factor the per-row add logic out of `renderListRow`'s toggle handler ([library.ts:684](hifimule-ui/src/library.ts:684)–728) into a reusable async helper, e.g. `addBrowseItemsToBasket(items: BrowseDisplayItem[]): Promise<{added: number; skipped: number}>`, then have BOTH the single-row toggle and the bulk handler call it. Do not duplicate the logic.
  - [ ] Helper semantics (preserve EXACTLY the current per-row behavior, lines 686–726):
    - Skip items where `basketStore.has(basketId ?? id)` → count as `skipped`.
    - Same `needsFetch` predicate per item: resolved type in `['MusicArtist','MusicAlbum','MusicGenre','Playlist']`, not favorite-scoped, and `(!item.childCount || !item.sizeBytes)`.
    - ONE batched call pair for all needs-fetch items: `Promise.all([rpcCall('jellyfin_get_item_counts', { itemIds }), rpcCall('jellyfin_get_item_sizes', { itemIds })])`. Responses are arrays of `{ id, recursiveItemCount, cumulativeRunTimeTicks }` / `{ id, totalSizeBytes }` — build `Map`s keyed by `id`; do NOT assume response order matches request order (the per-row code uses `metadata[0]` because it sends one id — that shortcut does not generalize).
    - `basketStore.add(...)` per item with the identical field mapping as lines 702–710 / 717–725 (`id: basketId ?? id`, `name`, `type: resolvedType`, `artist: item.subtitle ?? undefined`, `childCount`, `sizeTicks: item.sizeTicks || fetched`, `sizeBytes`). Artist-entity semantics (Story 3.9) flow through `item.basketType` — no special-casing needed beyond the existing mapping.
  - [ ] Bulk handler: resolve selected items from `state.items` by id (never from DOM), call the helper, show `showToast(...)` ([toast.ts:25](hifimule-ui/src/toast.ts:25)) with `t('library.selection.added_toast', { added })` + `t('library.selection.skipped_suffix', { skipped })` appended when `skipped > 0`, variant `'success'`, then `clearSelection()`.
  - [ ] While the RPC pair is in flight, set the bulk button's `loading = true` and ignore re-clicks (mirror the per-row `toggleBtn.loading` pattern, lines 694/714). On RPC failure: `console.error` + danger toast, do NOT clear the selection (user can retry).
  - [ ] Note: `basketStore.add` fires an `update` event per item → the list's basket handler (line 819) wipes and repaints rows after each add. This is acceptable for v1 (N is human-scale); do not refactor basketStore batching in this story.

- [ ] **Task 5: Selection-clearing hooks + Escape** (AC: 9)
  - [ ] Call `clearSelection()` (the cheap no-op-when-empty helper) in:
    - `switchMode()` — [library.ts:937](hifimule-ui/src/library.ts:937)–958 (browse-mode change)
    - `loadModeRoot()` — [library.ts:962](hifimule-ui/src/library.ts:962)–988 (covers programmatic resets)
    - `navigateToArtist` / `navigateToAlbum` / `navigateToPlaylist` / `navigateToGenre` — [library.ts:1721](hifimule-ui/src/library.ts:1721)–1763 (drill-down)
    - `navigateToCrumb` — [library.ts:1765](hifimule-ui/src/library.ts:1765)–1776 (breadcrumb jumps)
    - `loadArtistsByLetter` / `loadAlbumsByLetter` — [library.ts:1054](hifimule-ui/src/library.ts:1054)–1144 (A–Z filter change, including letter-clear)
    - `setViewMode()` — [library.ts:648](hifimule-ui/src/library.ts:648)–653 (toggle to grid)
  - [ ] Escape: register ONE module-level `document.addEventListener('keydown', ...)` (guard with a module flag so repeated `renderList` calls don't stack listeners). Handler: only act when `e.key === 'Escape'` and `state.selectedIds.size > 0`; skip when a Shoelace dialog is open (`document.querySelector('sl-dialog[open]')`) or a context menu is open (`.hm-context-menu.is-open`) so Escape closes those first and the selection survives a cancelled dialog.

- [ ] **Task 6: Generalize MediaCard playlist dialogs to `itemIds: string[]`** (AC: 6)
  - [ ] `MediaCard.openAddToPlaylistDialog` ([MediaCard.ts:443](hifimule-ui/src/components/MediaCard.ts:443)) → `openAddToPlaylistDialog(itemIds: string[], label: string, onSuccess?: () => void)`. Inside, the existing `rpcCall('playlist.addItems', { playlistId, itemIds: [trackId] })` (line 501) becomes `{ playlistId, itemIds }`. Invoke `onSuccess?.()` after the success toast.
  - [ ] `MediaCard.openCreatePlaylistDialog` ([MediaCard.ts:373](hifimule-ui/src/components/MediaCard.ts:373)) → `openCreatePlaylistDialog(itemIds: string[], suggestedName: string, onSuccess?: () => void)`. `rpcCall('playlist.create', { name, itemIds: [itemId] })` (line 408) becomes `{ name, itemIds }`. Keep the existing `invalidatePlaylistsCache()` + success toast (lines 409–412); invoke `onSuccess?.()` after.
  - [ ] Update ALL existing call sites to pass one-element arrays (no behavior change):
    - [MediaCard.ts:364](hifimule-ui/src/components/MediaCard.ts:364) — context menu → `openAddToPlaylistDialog([itemId], itemName)`
    - [MediaCard.ts:479](hifimule-ui/src/components/MediaCard.ts:479) — picker's "New playlist…" fallback → `openCreatePlaylistDialog(itemIds, trackName)` (thread the array through)
    - [TracksBrowseView.ts:516](hifimule-ui/src/components/TracksBrowseView.ts:516) — per-track "Send to playlist…" → `openAddToPlaylistDialog([track.id], track.title)`
  - [ ] `MediaCard.showItemContextMenu` stays single-item — the per-row context menu is untouched by this story ([MediaCard.ts:302](hifimule-ui/src/components/MediaCard.ts:302); callers at MediaCard.ts:260, TracksBrowseView.ts:522, library.ts:745).
  - [ ] Bulk handler in `library.ts`: `MediaCard.openAddToPlaylistDialog(selectedIdsArray, t('library.selection.count', { count }), () => clearSelection())` — selection clears only on success (AC 6); cancelling the dialog keeps the selection.

- [ ] **Task 7: CSS** (AC: 1, 4)
  - [ ] In `hifimule-ui/src/styles.css` next to the `.media-list-row` block (lines 1764–1826):
    - `.media-list-row__check` — leading checkbox, hidden by default (`opacity: 0`), visible on `.media-list-row:hover` and `.media-list-row:focus-within`, and ALWAYS visible while any selection is active. Implement "selection active" with a `has-selection` class toggled on the `.media-list` scroller by selection changes: `.media-list.has-selection .media-list-row__check { opacity: 1; }`. Keep the checkbox focusable even at opacity 0 (use opacity, never `display:none`/`visibility:hidden`, or Tab order breaks AC 10).
    - `.media-list-row.is-checked` — selected-row tint, visually distinct from the basket `is-selected` tint (line 1782).
    - `.bulk-action-bar` — sticky (`position: sticky; top: 0;`), surface background + border, flex row, gap, `z-index` above rows but below the context menu (menu is z-index 800, [styles.css:1835](hifimule-ui/src/styles.css:1835)).
  - [ ] Follow existing token usage in styles.css (CSS variables like `--sl-*`/ink colors — match neighboring rules; no hardcoded hex that the file doesn't already use).

- [ ] **Task 8: i18n keys — en/fr/es/de (FOUR languages)** (AC: 4, 5, 10)
  - [ ] Add to `hifimule-i18n/catalog.json` in **all four** language objects (`en`, `fr`, `es`, `de` — the catalog has 267 keys per language and German is live in the app; the proposal text says en/fr/es but de parity is mandatory, see [i18n.ts:11](hifimule-ui/src/i18n.ts:11)):
    ```
    library.selection.count            "{count} selected"
    library.selection.add_to_basket    "Add to basket"
    library.selection.add_to_playlist  "Add to playlist…"
    library.selection.clear            "Clear"
    library.selection.added_toast      "{added} added to basket"
    library.selection.skipped_suffix   " ({skipped} already in basket)"
    ```
  - [ ] Placeholders use the existing `{name}` convention — `t(key, { count })` interpolation is supported ([i18n.ts:47](hifimule-ui/src/i18n.ts:47)–60).
  - [ ] Place keys near the existing `library.*` block; keep all four language objects key-complete (same 6 keys each).

- [ ] **Task 9: Pre-existing list-row device-locked gap (1-line fix, required for AC 7 coherence)**
  - [ ] The `device-locked` CSS only matches `.basket-toggle-btn`, which grid `MediaCard` buttons have ([MediaCard.ts:89](hifimule-ui/src/components/MediaCard.ts:89)) but the list row's `toggleBtn` ([library.ts:680](hifimule-ui/src/library.ts:680)) does NOT — so list (+) buttons are currently never disabled when no device is selected. AC 7 defines the bulk button as "mirroring per-row (+) behavior"; for the mirror to be true, add `toggleBtn.classList.add('basket-toggle-btn')` to `renderListRow` so both per-row and bulk buttons obey the same rule. (Same 1-line treatment for the curate button is NOT needed — curation is not device-gated.)

- [ ] **Task 10: Build gates & manual verification** (AC: all)
  - [ ] `rtk tsc --noEmit` — zero NEW errors. Pre-existing: `tsconfig.json` baseUrl deprecation warning + `MediaCard.ts` TS6133 (`activeContextMenu`) — do not count these, do not "fix" them here.
  - [ ] `npm run build` — zero new errors.
  - [ ] No test framework exists in `hifimule-ui` (no vitest/jest config, no test script in package.json) — do NOT scaffold one in this story. Manually verify instead, in artists AND albums list view:
    1. Checkbox hover/focus reveal; always-visible while selection active.
    2. Ctrl/Cmd-click toggles without navigating; plain click still navigates.
    3. Shift-click range from anchor (downward AND upward).
    4. Scroll selected rows out of view and back → still checked (virtualization).
    5. Scroll to trigger autoload append → selection intact, new rows selectable, Shift-range across the page boundary works.
    6. Bulk add to basket with a mix of new + already-basketed items → toast shows added/skipped, basket sidebar projection updates, selection clears.
    7. Bulk add to playlist → picker opens, both existing-playlist and new-playlist paths work, selection clears on success but survives dialog cancel.
    8. No device selected (`device-locked`) → bulk "Add to basket" disabled, per-row (+) disabled, "Add to playlist…" still enabled.
    9. Each clearing trigger: mode switch, drill-down, breadcrumb, A–Z letter, grid toggle, Escape.
    10. Keyboard-only: Tab to checkbox, Space toggles, Tab reaches bulk bar buttons; screen-reader-visible live count (inspect `aria-live` region updates).
    11. Subsonic/Navidrome server: genres/playlists/favorites rows show NO checkbox; artists/albums work the same as Jellyfin.

## Dev Notes

### Scope Boundary — UI Only

UI-only story in `hifimule-ui` + `hifimule-i18n/catalog.json`. **No daemon, provider, manifest, or sync-engine changes. No new RPCs. Do not touch any `hifimule-daemon/**` file.** The daemon contract is already plural: `playlist.create` / `playlist.addItems` take `itemIds: string[]` and resolve containers (artists/albums) to ordered track lists server-side (Story 11.4); `jellyfin_get_item_counts` / `jellyfin_get_item_sizes` are batch RPCs.

Explicitly OUT of scope (sprint change proposal §2): grid view multi-select; Tracks dual-panel mode (`TracksBrowseView`, Story 9.10); Playlist Curation view (11.6); bulk REMOVE from basket (bulk bar only adds; already-basketed rows are skipped, not toggled).

### Cross-Server Safety — Already Handled

The browse list only ever shows the active server's items, and `playlist.*` RPCs enforce server scope daemon-side (409 on cross-server items, Story 11.4 amendment). `basketStore.add` stamps `serverId` from the active server itself ([basket.ts:270](hifimule-ui/src/state/basket.ts:270)–281). No new cross-server handling needed.

### Current Code Anatomy (READ BEFORE TOUCHING)

#### `hifimule-ui/src/library.ts` — the main surgery site

- **`AppState`** (lines 43–59) + `state` (line 69): fields include `browseMode`, `items`, `pagination`, `listLoading`, `activeLetter`, `listViewMode`, caches. Selection fields go here.
- **`renderList()`** (768–827): wipes `#library-content`, appends breadcrumbs + quick-nav + `.media-list` scroller (absolute-positioned rows, height = `total * VIRTUAL_ROW_HEIGHT` (56px)). Internal **`paint()`** (782–798) mounts/unmounts rows for the visible window ± `OVERSCAN` (3). Refs stashed on the container: `__listScroller`, `__listPaint`, `__listScrollHandler`, `__listBasketHandler`.
- **`renderListRow()`** (655–764): `itemId = item.basketId ?? item.id` (656); basket `is-selected` class (660); `sl-icon-button` toggle with async count/size fetch for container types (684–728); row-click navigation guarded by composedPath `SL-ICON-BUTTON` check + `is-navigating` class (729–740); context menu for MusicArtist/MusicAlbum/Audio when `_supportsPlaylistWrite` (742–747); curate button on Playlist rows (751–761).
- **Basket repaint pattern** (819–824): on `basketStore` `update`, remove all rows + `paint()` — remounted rows re-read state. Selection rendering rides the same mechanism.
- **`teardownListScrollHandler()`** (579–598): removes scroll + basket listeners, clears stashed refs. The bulk bar needs no entry here (it has no document-level listeners and dies with `content.innerHTML = ''`), but the Escape listener is module-level and registered once — NOT per-render.
- **Autoload**: `listAutoloadSupported()` (832–846) gates `loadMoreForListView()` (848–903), which APPENDS to `state.items` and repaints. Append-only ⇒ indices stable ⇒ `selectionAnchorIdx` stays valid across autoloads.
- **No existing document-level Escape/keyboard handler in library.ts** — the one you add is the first; guard against double-registration.
- **`_supportsPlaylistWrite`** module flag via `setPlaylistWriteCapability` (31–34) — already used to gate context menus; reuse for the bulk playlist button.

#### Device-gating mechanism (AC 7) — get this right

`selectedDevicePath` lives in `BasketSidebar` ([BasketSidebar.ts:187](hifimule-ui/src/components/BasketSidebar.ts:187)), which toggles `device-locked` on `#library-content` when it is null (line 852). CSS rule [styles.css:1341](hifimule-ui/src/styles.css:1341): `#library-content.device-locked .basket-toggle-btn { opacity: .3; pointer-events: none; }`. The bulk "Add to basket" button just needs the `basket-toggle-btn` class and to live inside `#library-content` — zero new state plumbing. (Task 9 closes the pre-existing gap where list rows' toggle buttons lack this class.) Do NOT use `basketStore.getActiveServerId()` here — that probes the active *server*, not the selected *device*; it was the right tool in `TracksBrowseView` (9.10 review P7) but the list view has the CSS-class mechanism.

#### `hifimule-ui/src/components/MediaCard.ts`

- `showItemContextMenu(x, y, itemId, itemName)` (302–371): builds the fixed-position menu, calls `openAddToPlaylistDialog(itemId, itemName)` at 364. **Stays single-item.**
- `openAddToPlaylistDialog(trackId, trackName)` (443–534): fetches `browse.listPlaylists` (468), renders "New playlist…" (→ `openCreatePlaylistDialog`, 479) + per-playlist buttons → `rpcCall('playlist.addItems', { playlistId, itemIds: [trackId] })` (501) → success toast `playlist.context.added_success` (503).
- `openCreatePlaylistDialog(itemId, itemName)` (373–439): name prompt → `rpcCall('playlist.create', { name, itemIds: [itemId] })` (408) → `invalidatePlaylistsCache()` (409, imported from library.ts) + success toast (412).
- Only the TWO dialog functions generalize to `itemIds: string[]` (+ optional `onSuccess`). The complete call-site inventory is in Task 6 — there are exactly three.

#### `hifimule-ui/src/state/basket.ts`

`BasketItem` (8–19): `{ id, name, type, serverId?, artist?, childCount, sizeTicks, sizeBytes, autoFilled?, priorityReason? }`. `has(id)` (266), `add(item)` (270–281, stamps serverId + persists + fires `update`), `remove(id)` (283–290). Subscribe via `basketStore.addEventListener('update', h)`.

#### Toast

`showToast(message, variant, duration)` in [toast.ts:25](hifimule-ui/src/toast.ts:25)–43 (sl-alert based; variants `success`/`danger`/etc.). Already imported in MediaCard; import in library.ts if not present.

#### i18n — FOUR languages

`catalog.json` shape: `{ "en": {flat keys}, "fr": {...}, "es": {...}, "de": {...} }`, 267 keys each, parity enforced by convention. `t(key, replacements)` does `{name}` substitution ([i18n.ts:47](hifimule-ui/src/i18n.ts:47)–60) and falls back fr/es/de→en→raw key. **The sprint change proposal lists en/fr/es, but `de` exists and ships ([i18n.ts:11](hifimule-ui/src/i18n.ts:11)) — add all 6 keys to all 4 languages or the catalog drifts.**

### Critical Implementation Decisions (decided — do not re-litigate)

1. **Selection is id-keyed app state, never DOM state.** Virtualized rows unmount; `state.selectedIds` + lookup from `state.items` at action time is the only correct model (epics tech note; proposal §2 Technical Impact).
2. **`is-checked` ≠ `is-selected`.** `is-selected` already means "in basket" on list rows (library.ts:660). Selection gets its own class and its own tint.
3. **Batched metadata fetch must map by response `id`,** not array position — the single-item `metadata[0]` shortcut in the per-row handler does not generalize.
4. **Factor, don't fork, the basket-add logic.** One helper serves both the per-row toggle and the bulk action; this is the epics tech note's explicit instruction and prevents the two paths drifting.
5. **Dialog success callback** (`onSuccess?: () => void`) is how "selection clears on success" (AC 6) reaches library.ts — the dialogs are fire-and-forget today and have no return channel. Cancel ⇒ selection survives.
6. **Native `<input type="checkbox">`,** not `<sl-checkbox>`: zero shadow-DOM focus quirks inside absolutely-positioned virtual rows, native Space/Tab semantics (AC 10), trivial styling.
7. **Bulk bar is rebuilt by `renderList` and updated in place on selection change** — it is NOT part of the virtualized scroller and must not be (sticky sibling, epics tech note).

### Previous Story Intelligence (9.10, 9.8, 9.7 reviews)

- **Guard async handlers against staleness**: 9.10 review found stale-fetch races; the bulk basket handler is async — re-resolve items from `state.items` and tolerate the selection having been cleared mid-flight (check `state.selectedIds.size` after await, or snapshot ids up front and proceed — snapshot is fine since adds are idempotent via the skip check).
- **Don't stack listeners**: 9.10 review P3 was a leaked `basketStore` listener on mode switch. The list's `teardownListScrollHandler` handles its listeners; your Escape listener is module-level-once; the bulk bar holds only element-local listeners.
- **i18n every user-facing string**: 9.10 review P6 flagged hardcoded English aria-labels. Checkbox `aria-label`, bulk bar buttons, count, toasts — all through `t()`.
- **Honor ACs literally on disabled states**: 9.10 review P7 — AC 7's disabled state must be real and verifiable, not "the mode bar is disabled anyway".
- **Stale-mode race in `loadMoreForListView`** (9.8 review, deferred): still open; selection clearing on mode switch (Task 5) means a late append can't resurrect a cleared selection — keep it that way.
- **Pre-existing build noise**: tsconfig `baseUrl` deprecation warning + MediaCard TS6133 — known, not yours.

### Git Intelligence

Commit pattern: `Story 9.11` (this file) → `Dev 9.11` (implementation) → `Review 9.11` (post-review patches). Recent head: `fcc2513 Correct course multi-select` (the approved sprint change proposal + artifact edits — PRD FR47, epics 9.11 entry, UX spec §5.1/§5.2 already applied; do NOT re-edit planning artifacts).

### Project Structure Notes

- All work in `hifimule-ui/src/` (library.ts, components/MediaCard.ts, styles.css, possibly components/TracksBrowseView.ts call-site touch-up) + `hifimule-i18n/catalog.json`. Matches the established UI-story footprint of 9.7/9.8/9.10.
- **Files to modify (UPDATE):** `hifimule-ui/src/library.ts`, `hifimule-ui/src/components/MediaCard.ts`, `hifimule-ui/src/components/TracksBrowseView.ts` (one call site), `hifimule-ui/src/styles.css`, `hifimule-i18n/catalog.json`. **No new files.**
- **Do not touch:** `hifimule-daemon/**`, `hifimule-ui/src/state/basket.ts` (consume only), `PlaylistCurationView.ts`, planning artifacts.

### References

- [Source: _bmad-output/planning-artifacts/epics.md:2324](_bmad-output/planning-artifacts/epics.md:2324) — Story 9.11 ACs + Technical Notes (authoritative).
- [Source: _bmad-output/planning-artifacts/sprint-change-proposal-2026-06-12-list-view-multi-selection.md](_bmad-output/planning-artifacts/sprint-change-proposal-2026-06-12-list-view-multi-selection.md) — origin proposal: evidence, scope boundaries, UX spec amendments.
- [Source: _bmad-output/planning-artifacts/ux-design-specification.md] — §5.2 "List Multi-Select & Bulk Action Bar" component spec (checkbox reveal rules, bar contents, clearing triggers).
- [Source: _bmad-output/planning-artifacts/prd.md] — FR47.
- [Source: _bmad-output/implementation-artifacts/9-10-tracks-browse-mode-dual-panel-ui.md] — previous story: review findings P1–P7, build-gate conventions.
- [Source: hifimule-ui/src/library.ts:655](hifimule-ui/src/library.ts:655) — `renderListRow` (checkbox + click semantics target).
- [Source: hifimule-ui/src/library.ts:768](hifimule-ui/src/library.ts:768) — `renderList` / `paint` virtualization (bulk bar mount point).
- [Source: hifimule-ui/src/components/MediaCard.ts:373](hifimule-ui/src/components/MediaCard.ts:373) / [:443](hifimule-ui/src/components/MediaCard.ts:443) — dialog functions to generalize.
- [Source: hifimule-ui/src/components/BasketSidebar.ts:852](hifimule-ui/src/components/BasketSidebar.ts:852) + [hifimule-ui/src/styles.css:1341](hifimule-ui/src/styles.css:1341) — device-locked gating mechanism.
- [Source: hifimule-ui/src/i18n.ts:47](hifimule-ui/src/i18n.ts:47) — `t()` interpolation; [i18n.ts:11](hifimule-ui/src/i18n.ts:11) — 4 supported languages.

## Dev Agent Record

### Agent Model Used

### Debug Log References

### Completion Notes List

### File List

## Change Log

- 2026-06-12: Story created from sprint-change-proposal-2026-06-12-list-view-multi-selection (Epic 9 reopened). Ultimate context engine analysis completed — comprehensive developer guide created.
