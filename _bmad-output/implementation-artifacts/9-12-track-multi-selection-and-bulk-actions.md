---
baseline_commit: 7c018bc
---

# Story 9.12: Track Multi-Selection & Bulk Actions

Status: review

## Story

As a Ritualist (Arthur),
I want to select multiple tracks — in an album's track list or in the Tracks browse view — and act on them all at once,
So that I can send a batch of individual songs to my basket or a playlist without clicking every row.

## Acceptance Criteria

1. **Given** the virtualized list view shows track rows (resolved type `Audio`, e.g. tracks within an album)
   **When** a row renders
   **Then** it displays the same leading selection checkbox as artist/album rows, and all Story 9.11 selection mechanics (Ctrl/Cmd-click, Shift-range, bulk bar, virtualization survival, clearing rules, keyboard/ARIA) apply unchanged to track rows.

2. **Given** tracks are selected in the list view and I click "Add to basket"
   **Then** tracks already in the basket are skipped, the remaining tracks are added using their own `sizeBytes`/`sizeTicks` (no count/size batch RPC for tracks), a toast reports added/skipped counts, and the selection clears.

3. **Given** the Tracks dual-panel browse view is active
   **When** a track row renders in the bottom track panel
   **Then** it displays a leading selection checkbox (visible on hover/focus, always visible while any selection is active), alongside the existing per-row (+)/(-) and "Send to playlist…" actions, which continue to work unchanged.

4. **Given** I click a track row's checkbox or Ctrl/Cmd-click the row
   **Then** the row's selection toggles.

5. **Given** a track row is the selection anchor and I Shift-click another track row
   **Then** all track rows between the two indices (inclusive, within the currently loaded track list) become selected.

6. **Given** at least one track is selected in the Tracks view
   **Then** a bulk action bar appears above the track panel showing the selection count (ARIA-live), an "Add to basket" button (disabled when no device is selected), an "Add to playlist…" button (only when `supports_playlist_write` is true), and a "Clear" affordance.

7. **Given** I click "Add to basket" with N tracks selected in the Tracks view
   **Then** tracks already in the basket are skipped, each remaining track is added via `basketStore.add` with its own size metadata, a toast reports added/skipped counts, and the selection clears.

8. **Given** I click "Add to playlist…" with N tracks selected
   **Then** the existing playlist picker dialog opens seeded with all N track ids; existing-playlist and create-new flows behave per Story 9.11, the playlists cache is invalidated on success, a toast confirms, and the selection clears. Cancelling preserves the selection.

9. **Given** tracks are selected in the Tracks view and autoload appends more pages to any panel
   **Then** the selection is preserved (id-keyed).

10. **Given** tracks are selected in the Tracks view
    **When** I change the artist filter, the album filter, or the A–Z letter, leave the Tracks mode, or press Escape
    **Then** the selection and the bulk action bar are cleared.

11. **Given** keyboard-only navigation in the Tracks view
    **Then** checkboxes are focusable and toggleable via Space, bulk bar buttons are reachable in tab order, and the selection count is announced via an ARIA-live region.

## Tasks / Subtasks

- [x] **Task 1: Surface A — widen the list-view selectability predicate** (AC: 1, 2)
  - [x] In `isSelectableListItem` ([library.ts:665](hifimule-ui/src/library.ts:665)–668), add `|| resolved === 'Audio'` and update the comment above it (lines 663–664: it currently says tracks render no checkbox — now only favorite-scoped containers, genres, and playlists are excluded).
  - [x] That is the ONLY code change Surface A needs. Verify (do not re-implement) that everything downstream now just works for `Audio` rows:
    - `renderListRow` ([library.ts:924](hifimule-ui/src/library.ts:924)–1037) gates checkbox rendering, `is-checked` class, Ctrl/Cmd-toggle, and Shift-range on `isSelectableListItem` — track rows pick all of it up automatically.
    - Plain click on an `Audio` row stays a no-op: `navigateToBrowseItem` has `case 'Audio': break;` ([library.ts:1999](hifimule-ui/src/library.ts:1999)) — no navigation regression possible.
    - `selectRange` ([library.ts:713](hifimule-ui/src/library.ts:713)) and `resolveSelectedItems` ([library.ts:815](hifimule-ui/src/library.ts:815)) both filter via the predicate — ranges and bulk actions include tracks automatically.
    - `addBrowseItemsToBasket` ([library.ts:825](hifimule-ui/src/library.ts:825)–885): `'Audio'` is NOT in `CONTAINER_TYPES` → never enters `needsFetch` → added directly with `item.sizeBytes ?? 0` / `item.sizeTicks ?? 0` / `childCount ?? 0`. All `Audio` list items are mapped with their own sizes and `childCount: 1` by `mapAlbumTracks` ([library.ts:328](hifimule-ui/src/library.ts:328)) and `mapFlatTracks` ([library.ts:303](hifimule-ui/src/library.ts:303)) — AC 2's "no batch RPC" is already true.
    - `bulkAddSelectionToPlaylist` ([library.ts:910](hifimule-ui/src/library.ts:910)) passes `it.id` — for tracks that IS the track id; `playlist.create`/`playlist.addItems` accept track ids natively.
  - [x] Intended blast radius (verify, don't fight it): the predicate widening makes ALL `Audio` rows in list view selectable — album drill-down tracks ([library.ts:1816](hifimule-ui/src/library.ts:1816), [1899](hifimule-ui/src/library.ts:1899)), history modes frequently/recently played ([library.ts:1151](hifimule-ui/src/library.ts:1151)–1163), and favorite tracks ([library.ts:1684](hifimule-ui/src/library.ts:1684)). FR48 wording covers them all ("track rows … in the virtualized list/table browse view"); they all carry their own sizes.

- [x] **Task 2: Surface B — selection state + clearing in `TracksBrowseView`** (AC: 4, 9, 10)
  - [x] Add private fields to the class ([TracksBrowseView.ts:43](hifimule-ui/src/components/TracksBrowseView.ts:43)):
    ```typescript
    private selectedTrackIds: Set<string> = new Set();
    private selectionAnchorIdx: number | null = null;
    ```
    Keyed by `track.id` (same id `basketStore.has` uses). Anchor indexes into `this.trackState.items` — valid across autoload because `fetchTracks` only ever APPENDS ([TracksBrowseView.ts:301](hifimule-ui/src/components/TracksBrowseView.ts:301)) until a reset.
  - [x] Add a `clearSelection()` method: no-op fast path when already empty; empties the set, nulls the anchor, removes the bulk bar, removes `has-selection` from the track panel, and unchecks any rendered rows (`.is-checked` class + checkbox `checked`). Keep it callable before/after panel rebuilds (all DOM lookups null-safe).
  - [x] Clearing choke point: call `this.clearSelection()` at the top of `fetchTracks` when `reset === true` ([TracksBrowseView.ts:280](hifimule-ui/src/components/TracksBrowseView.ts:280)–283). Every AC-10 filter trigger funnels through it: `selectArtist` (line 546), `selectAlbum` (565), `setArtistLetter` (582), `setAlbumLetter` (591) all call `fetchTracks(true)`. This is the proposal's sanctioned hook ("cleared by the same code paths that reset trackState"). First-entry `load()` also resets — harmless no-op.
  - [x] "Leaving the Tracks mode" (AC 10): call `this.clearSelection()` in `destroy()` ([TracksBrowseView.ts:96](hifimule-ui/src/components/TracksBrowseView.ts:96)–102). CRITICAL: the instance is CACHED at module level ([library.ts:91](hifimule-ui/src/library.ts:91)) — `destroy()` runs on mode exit ([library.ts:1224](hifimule-ui/src/library.ts:1224)) but re-entry calls `remount()` on the SAME instance ([library.ts:1274](hifimule-ui/src/library.ts:1274)–1275). If you don't clear in `destroy()`, the old selection resurrects on remount and violates AC 10.
  - [x] Escape (AC 10): document-level `keydown` listener, registered in `load()` and `remount()`, removed in `destroy()` (store the handler ref like the existing `_trackScrollHandler` pattern, lines 68–70; remove-before-add or guard so `load`→`remount` sequences never double-register). Mirror the library.ts handler EXACTLY ([library.ts:728](hifimule-ui/src/library.ts:728)–741): capture phase, return unless `e.key === 'Escape'` and `this.selectedTrackIds.size > 0`, skip when `document.querySelector('sl-dialog[open]')`, skip when `document.querySelector('.hm-context-menu')` — bare selector WITHOUT `.is-open` (9.11 review patch: the class lands a frame after mount; the element only exists while open). The library.ts module Escape listener coexists safely: in Tracks mode `state.selectedIds` is empty (cleared by `switchMode`), so it returns early.

- [x] **Task 3: Surface B — checkbox + click semantics in `buildTrackRow`** (AC: 3, 4, 5, 11)
  - [x] Thread the row index in: `buildTrackRow(track: BrowseTrack, index: number)`. Update both call sites — `renderTrackPanel` loop index ([TracksBrowseView.ts:364](hifimule-ui/src/components/TracksBrowseView.ts:364)–366) and `appendTrackRows` ([TracksBrowseView.ts:394](hifimule-ui/src/components/TracksBrowseView.ts:394)–402) with offset `this.trackState.items.length - tracks.length + i` (it is called AFTER the items are pushed, line 301–312).
  - [x] In `buildTrackRow` ([TracksBrowseView.ts:460](hifimule-ui/src/components/TracksBrowseView.ts:460)–527), prepend (before `info`) a native `<input type="checkbox">` with class `media-list-row__check` (reuse the 9.11 class — Task 5 extends its CSS to this context), `checked = this.selectedTrackIds.has(track.id)`, `aria-label` = `track.title` via DOM property assignment. Native checkbox = free Space/Tab semantics (AC 11). Add `is-checked` class to the row when selected. Note `.curation-track-row` has NO basket `is-selected` class collision (unlike list rows), but keep the class name `is-checked` for CSS sharing.
  - [x] Checkbox `click` handler: `e.stopPropagation()`, then toggle via a new `toggleTrackSelection(track, index, row)` helper (mirror [library.ts:693](hifimule-ui/src/library.ts:693)–709: flip set membership, set `selectionAnchorIdx = index`, sync row class + checkbox + panel `has-selection` class + bulk bar in place — no full repaint).
  - [x] Row `click` handler (the row currently has NONE — you are adding the first; existing buttons already `stopPropagation`):
    - `e.ctrlKey || e.metaKey` → `toggleTrackSelection(...)`, return.
    - `e.shiftKey && this.selectionAnchorIdx !== null` → range-select all indices between anchor and `index` inclusive over `this.trackState.items` (anchor stays put, mirror [library.ts:713](hifimule-ui/src/library.ts:713)–722; every track row is selectable so no type filter needed), then refresh checked state of all rendered rows + bulk bar.
    - Plain click → do nothing (track rows have no navigation today — preserve that).
  - [x] `mousedown` handler: `if (e.shiftKey && this.selectionAnchorIdx !== null) e.preventDefault();` — suppresses the browser text-selection artifact on Shift-click (same as [library.ts:999](hifimule-ui/src/library.ts:999)–1001).
  - [x] Range/re-render refresh: rows are plain appended divs (NOT virtualized — never unmounted until a full `renderTrackPanel` rebuild). After a Shift-range, update rendered rows in place by iterating `panel.querySelectorAll('.curation-track-row')` and syncing `is-checked` + checkbox from the set via `row.dataset.trackId` (already set, line 463). `renderTrackPanel`/`appendTrackRows` read the set at build time, so full rebuilds and autoload appends render correctly for free (AC 9).
  - [x] Per-row (+)/(-), "Send to playlist…", and the contextmenu handler (lines 483–524) must remain byte-for-byte behaviorally unchanged (AC 3).

- [x] **Task 4: Surface B — bulk action bar** (AC: 6, 11)
  - [x] Build a `div.bulk-action-bar` (reuses the existing CSS, [styles.css:1816](hifimule-ui/src/styles.css:1816)–1837) and insert it dynamically into the flex column wrapper rendered by `renderLayout` ([TracksBrowseView.ts:110](hifimule-ui/src/components/TracksBrowseView.ts:110)–133), directly BEFORE `#tracks-track-panel` ("above the track panel", AC 6). Create on selection 0→1, update the count text in place, remove on →0 (mirror `updateBulkBar`, [library.ts:783](hifimule-ui/src/library.ts:783)–813). The wrapper doesn't scroll (each panel scrolls internally), so the bar's sticky positioning is inert — fine.
  - [x] Contents (mirror `renderBulkBar`, [library.ts:743](hifimule-ui/src/library.ts:743)–779, including i18n keys):
    - Count `span.bulk-action-bar__count` with `aria-live="polite"`, text `t('library.selection.count', { count })`.
    - "Add to basket" `<sl-button size="small" variant="primary">` with class **`basket-toggle-btn`** — the Tracks view mounts INSIDE `#library-content` ([library.ts:1271](hifimule-ui/src/library.ts:1271)), so the existing `#library-content.device-locked .basket-toggle-btn` rule ([styles.css:1341](hifimule-ui/src/styles.css:1341)) disables it when no device is selected. That IS AC 6's disabled mechanism — identical to the 9.11 bulk bar; zero new state plumbing. Do NOT change the per-row buttons' existing `!basketStore.getActiveServerId()` gate (9.10 review P7 decision — leave it alone).
    - "Add to playlist…" `<sl-button>` — only when `this.supportsPlaylistWrite`.
    - "Clear" `<sl-button variant="text">` → `this.clearSelection()`.
  - [x] ARIA first-announcement fix (AC 11, 9.11 review patch — bake it in from the start): an aria-live region only announces mutations made after connection. On bar insertion, blank the count then re-assert it in a `requestAnimationFrame` so the 0→1 selection is announced ([library.ts:803](hifimule-ui/src/library.ts:803)–812 is the reference implementation).

- [x] **Task 5: Surface B — bulk handlers** (AC: 7, 8)
  - [x] Factor the per-row (+) handler's `BrowseTrack` → basket-item mapping (currently inline at [TracksBrowseView.ts:496](hifimule-ui/src/components/TracksBrowseView.ts:496)–504) into a private `trackToBasketItem(track)` returning `{ id: track.id, name: track.title, type: 'Audio', artist: track.artistName, childCount: 1, sizeBytes: track.sizeBytes ?? 0, sizeTicks: (track.duration ?? 0) * 10_000_000 }`. Use it from BOTH the per-row handler and the bulk handler — factor, don't fork.
  - [x] Resolve selected tracks in `trackState.items` ORDER (filter items by set membership — never iterate the Set) so playlist insertion order is deterministic and matches the visible list.
  - [x] Bulk "Add to basket": for each resolved track, skip if `basketStore.has(track.id)` (count `skipped`), else `basketStore.add(trackToBasketItem(track))` (count `added`). All local — no RPC, no loading state. Toast `t('library.selection.added_toast', { added })` + `t('library.selection.skipped_suffix', { skipped })` when `skipped > 0`, variant `'success'` (`showToast`, [toast.ts:25](hifimule-ui/src/toast.ts:25)), then `this.clearSelection()`. Note: `basketStore.add` itself toasts and returns early when no active server (basket.ts:271–274) — belt-and-braces under the CSS device gate.
  - [x] Bulk "Add to playlist…": `MediaCard.openAddToPlaylistDialog(ids, t('library.selection.new_playlist_name'), () => this.clearSelection())` — the second arg is forwarded as the create-dialog's suggested name, so it must be the generic localized default, NOT the count string (9.11 review patch). Selection clears ONLY via `onSuccess`; cancelling the dialog preserves it (AC 8). The dialog already invalidates the playlists cache and toasts ([MediaCard.ts:373](hifimule-ui/src/components/MediaCard.ts:373)–536, generalized to `itemIds: string[]` by 9.11 — no MediaCard changes needed).

- [x] **Task 6: CSS — share, don't duplicate** (AC: 1, 3)
  - [x] In [styles.css](hifimule-ui/src/styles.css), comma-extend the existing 9.11 selection rules (lines 1789–1811) to the Tracks-view context instead of writing parallel rules:
    - Checkbox reveal: extend the `.media-list-row:hover`/`:focus-within` reveal selectors with `.curation-track-row:hover .media-list-row__check`, `.curation-track-row:focus-within .media-list-row__check`, and add `.curation-track-panel.has-selection .media-list-row__check { opacity: 1; }` alongside the existing `.media-list.has-selection` rule. The `has-selection` class goes on the `.curation-track-panel` element (`#tracks-track-panel`).
    - Selected tint: extend `.media-list-row.is-checked` (lines 1808–1811) with `.curation-track-row.is-checked` (same tint + inset accent edge).
  - [x] Keep `opacity: 0` for hiding (NEVER `display:none`/`visibility:hidden` — Tab order breaks AC 11). `.bulk-action-bar` needs no changes. Only existing token patterns (`--accent-rgb`, `--surface-*`, `--sl-*`).

- [x] **Task 7: Build gates & manual verification** (AC: all)
  - [x] `npx tsc --noEmit --ignoreDeprecations "6.0"` → zero NEW errors (pre-existing: tsconfig `baseUrl` deprecation, MediaCard TS6133 — not yours).
  - [x] `rtk npm run build` → zero new errors.
  - [x] No test framework exists in `hifimule-ui` — do NOT scaffold one. Manual runtime checklist (hand to review if headless):
    1. List view, album drill-down: track rows show checkboxes; Ctrl/Cmd-click toggles; Shift-range works; plain click still does nothing; per-row (+) and context menu unchanged.
    2. List view: bulk-add a mix of new + already-basketed tracks → instant add (no RPC spinner), toast with added/skipped, basket projection updates, selection clears.
    3. List view history modes (frequently/recently played) and favorite tracks: checkboxes present and functional (intended FR48 blast radius).
    4. List view: virtualization scroll-away/back and autoload append preserve track selections; mixed Shift-range across page boundary.
    5. Tracks view: checkbox reveal on hover/focus; always visible while selection active; per-row (+)/(-) and "Send to playlist…" unchanged.
    6. Tracks view: Ctrl/Cmd-click toggle; Shift-range downward AND upward; range across autoloaded page boundary; selection survives autoload on all three panels (AC 9).
    7. Tracks view bulk bar: appears above track panel on first selection; count updates; "Add to basket" disabled with no device selected; "Add to playlist…" hidden on a non-playlist-write server (classic Subsonic).
    8. Tracks view bulk basket add → added/skipped toast, selection clears; bulk playlist add → picker seeded with N ids, existing + create-new paths, cancel preserves selection, success clears it.
    9. Each AC-10 clearing trigger: artist filter, album filter, artist A–Z, album A–Z, leaving Tracks mode (then re-entering — selection must NOT resurrect), Escape. Escape with an open dialog/context menu closes those and keeps the selection.
    10. Keyboard-only in Tracks view: Tab to checkbox, Space toggles, bulk bar buttons in tab order, ARIA-live count announced including the first (0→1) selection.
    11. Jellyfin AND Navidrome/Subsonic servers: both surfaces behave identically.

## Dev Notes

### Scope Boundary — Pure UI, Two Files of Surgery

All work in `hifimule-ui/src/library.ts` (one-line predicate + comment), `hifimule-ui/src/components/TracksBrowseView.ts` (the real work), and `hifimule-ui/src/styles.css` (selector extension). **No daemon, provider, manifest, or sync-engine changes. No new RPCs. No new i18n keys** — the seven `library.selection.*` keys from 9.11 (including `new_playlist_name` added in its review) exist in all FOUR languages (en/fr/es/de — verified: 28 catalog entries). **Do not touch:** `hifimule-daemon/**`, `MediaCard.ts` (dialogs already take `itemIds: string[]` + `onSuccess` from 9.11), `basket.ts` (consume only), `catalog.json`, `PlaylistCurationView.ts` (its curation list is explicitly out of scope), planning artifacts.

Explicitly OUT of scope (sprint change proposal §2): grid view multi-select; Playlist Curation View main track list (its "Add tracks" dialog already has its own multi-select); bulk REMOVE from basket (bar only adds; basketed tracks are skipped); cross-panel selection in the Tracks view (artist/album panels are filters, not selection sources).

### Why Surface A Is One Line

9.11 built everything behind a single predicate. `isSelectableListItem` ([library.ts:665](hifimule-ui/src/library.ts:665)) is consulted by `renderListRow` (checkbox render + Ctrl/Shift handlers), `selectRange`, and `resolveSelectedItems` — widening it to `'Audio'` activates checkboxes, ranges, bulk basket, and bulk playlist for track rows with zero further changes. The bulk basket path is already track-correct: `'Audio'` ∉ `CONTAINER_TYPES` ([library.ts:826](hifimule-ui/src/library.ts:826)) so tracks never enter the `needsFetch` batch-RPC branch and are added with their own `sizeBytes`/`sizeTicks` (every Audio mapper pre-populates them: [library.ts:303](hifimule-ui/src/library.ts:303)–339). Plain click on Audio is a navigation no-op ([library.ts:1999](hifimule-ui/src/library.ts:1999)). Do not add any Audio special-casing to the handlers — there is nothing to special-case.

### TracksBrowseView Anatomy (READ BEFORE TOUCHING)

- **Lifecycle (the AC-10 trap):** instance cached module-level in library.ts (line 91). Mode exit → `destroy()` (line 1224, also app-teardown at 135); re-entry → `remount()` on the same instance (1274–1278). `destroy()` today only tears down scroll handlers + basket unsub (96–102) — selection clearing and Escape-listener removal MUST be added there, or state resurrects on remount.
- **Panel state:** `trackState: PanelState<BrowseTrack>` (`items`, `total`, `startIndex`, `loading`, `exhausted`, `errored`). `fetchTracks(reset)` (280–320): reset → fresh `makePanelState()` + full `renderTrackPanel()`; append → `items.push(...)` + `appendTrackRows(newItems)`. Generation counter `trackGen` discards stale responses (a reset supersedes in-flight loads — your selection clearing at reset is therefore race-safe).
- **Rendering:** `renderTrackPanel` (348–368) is a full `innerHTML = ''` rebuild iterating `trackState.items`; `appendTrackRows` (394–402) appends only. Rows are plain divs with `dataset.trackId`, `tabindex="0"`, no row click handler, no role. NOT virtualized — rows never unmount between rebuilds, so AC 9 is structural; id-keying still required for rebuild correctness.
- **Filter funnels (AC 10):** `selectArtist` (546, early-returns on same id), `selectAlbum` (565, same), `setArtistLetter` (582), `setAlbumLetter` (591, guarded when an artist is selected) — ALL call `fetchTracks(true)`. Clearing inside `fetchTracks(reset)` covers every one with a single hook and stays correct if future filters are added.
- **Per-row actions (must not regress):** (+)/(-) `sl-icon-button` with `dataset.basketToggle`, disabled via `!basketStore.getActiveServerId()` (490; refreshed by `updateTrackButtons` on every basket `update`, 531–542); playlist button + contextmenu only when `supportsPlaylistWrite` (509–524). All `stopPropagation` already — your new row click handler will not receive their clicks.
- **`BrowseTrack`** ([rpc.ts:190](hifimule-ui/src/rpc.ts:190)): `id`, `title`, `artistName`, `albumName`, `duration` (seconds → ticks ×10_000_000), `sizeBytes: number | null`, `coverArtId`, …. `sizeBytes ?? 0` matches the established per-row mapping; Subsonic providers may report null sizes — 0 is the accepted existing behavior, do not "fix".

### 9.11 Machinery To Mirror (reference implementations, all in library.ts)

- `toggleRowSelection` (693–709) — cheap single-row toggle, no full repaint.
- `selectRange` (713–722) — `[lo, hi]` normalization, anchor stays put, state-array iteration (never DOM).
- `ensureSelectionEscapeListener` (728–741) — capture phase + `sl-dialog[open]` + bare `.hm-context-menu` guards, with comments explaining both review-patched subtleties.
- `renderBulkBar`/`updateBulkBar` (743–813) — bar contents, i18n keys, `basket-toggle-btn` device gate, in-place count update, rAF aria-live re-assertion.
- `bulkAddSelectionToBasket`/`bulkAddSelectionToPlaylist` (887–922) — toast composition, clear-on-success-only, `new_playlist_name` suggested name.
Mirror semantics and naming; state-holder differs (`this.*` component fields vs module `state`). Tracks-view basket adds are synchronous (no RPC) so the `btn.loading` machinery is unnecessary there.

### Device Gating — Two Mechanisms, Use the Right One

The bulk bar's "Add to basket" uses the CSS class gate: `basket-toggle-btn` + `#library-content.device-locked` rule ([styles.css:1341](hifimule-ui/src/styles.css:1341)), toggled by `BasketSidebar` when `selectedDevicePath === null` ([BasketSidebar.ts:852](hifimule-ui/src/components/BasketSidebar.ts:852)). It works in the Tracks view because the view mounts in `#library-content` (library.ts:1271) — and it is the exact mechanism the 9.11 bulk bar uses, so both bars behave identically. The Tracks view's PER-ROW buttons use `!basketStore.getActiveServerId()` (a 9.10 review decision) — leave them untouched; do not unify the two in this story. Known accepted limitation (9.11 review, deferred): the CSS gate doesn't block keyboard Enter/Space activation — accept the same here, do not invent a fix.

### Previous Story Intelligence (9.11 — directly upstream)

- **Review patches to inherit from day one:** (a) aria-live blank-then-rAF re-assert for the 0→1 announcement; (b) Escape guard matches `.hm-context-menu` WITHOUT `.is-open`; (c) playlist suggested name = `t('library.selection.new_playlist_name')`, never the count string.
- **Review false positive to not re-fight:** `it.id` vs `basketId` for playlist ids was examined and refuted — `basketId` differs only for non-selectable favorite containers; tracks have no `basketId`.
- **Deferred items that stay deferred:** partial batch-RPC fallback overcount (N/A to tracks — no RPC); per-row (+) failure toast; bulk-bar sticky offset after resize; Shift-range grow-only/anchor-on-deselect semantics (same minimal semantics are fine here); no already-in-playlist skip for bulk playlist add (daemon resolves server-side).
- **Patterns that prevented bugs:** native `<input type="checkbox">` over `sl-checkbox` (focus + Space for free); selection is app state, DOM repaints re-read it; listeners registered once with paired teardown (9.10 review P3 was a leaked basket listener — your Escape listener add/remove must pair through load/remount/destroy); every user-facing string through `t()` (9.10 review P6).
- **Commit convention:** `Story 9.12` → `Dev 9.12` → `Review 9.12` (matches 7c018bc/70387dc/d870559 for 9.11).

### Project Structure Notes

- **Files to modify (UPDATE, no new files):** `hifimule-ui/src/library.ts` (predicate + comment only), `hifimule-ui/src/components/TracksBrowseView.ts` (selection state, checkbox, bulk bar, handlers, lifecycle), `hifimule-ui/src/styles.css` (comma-extend three rule groups).
- Matches the pure-UI story footprint of 9.7/9.8/9.11 — architecture explicitly unaffected (proposal §2: "No change. Pure UI; no new RPCs, traits, or schemas").
- No test framework in `hifimule-ui` (no vitest/jest config or test script) — build gates + manual checklist per established convention; do not scaffold one.

### References

- [Source: _bmad-output/planning-artifacts/epics.md:2378](_bmad-output/planning-artifacts/epics.md:2378) — Story 9.12 ACs + Technical Notes (authoritative).
- [Source: _bmad-output/planning-artifacts/sprint-change-proposal-2026-06-12-track-multi-selection.md](_bmad-output/planning-artifacts/sprint-change-proposal-2026-06-12-track-multi-selection.md) — origin proposal: evidence, scope boundaries, success criteria, review-attention areas (Shift-range indexing, three filter axes).
- [Source: _bmad-output/planning-artifacts/prd.md:168](_bmad-output/planning-artifacts/prd.md:168) — FR48.
- [Source: _bmad-output/planning-artifacts/ux-design-specification.md] — §5.1 list view + §5.2 "Tracks Browse View" / "List Multi-Select & Bulk Action Bar" (amended for tracks: checkbox reveal rules, bar contents, clearing triggers).
- [Source: _bmad-output/implementation-artifacts/9-11-list-view-multi-selection-and-bulk-actions.md] — previous story: full selection-machinery anatomy, review findings, critical decisions.
- [Source: hifimule-ui/src/library.ts:665](hifimule-ui/src/library.ts:665) — `isSelectableListItem` (Surface A change site).
- [Source: hifimule-ui/src/library.ts:661](hifimule-ui/src/library.ts:661)–922 — complete 9.11 selection machinery (mirror source).
- [Source: hifimule-ui/src/components/TracksBrowseView.ts:43](hifimule-ui/src/components/TracksBrowseView.ts:43)–135 — class state + lifecycle (load/remount/destroy); [:280](hifimule-ui/src/components/TracksBrowseView.ts:280) — `fetchTracks` (clearing choke point); [:460](hifimule-ui/src/components/TracksBrowseView.ts:460) — `buildTrackRow` (checkbox site); [:546](hifimule-ui/src/components/TracksBrowseView.ts:546)–597 — the four filter handlers.
- [Source: hifimule-ui/src/components/MediaCard.ts:373](hifimule-ui/src/components/MediaCard.ts:373)/[:444](hifimule-ui/src/components/MediaCard.ts:444) — plural-id dialogs with `onSuccess` (consume as-is).
- [Source: hifimule-ui/src/styles.css:1789](hifimule-ui/src/styles.css:1789)–1837 — 9.11 checkbox/tint/bulk-bar rules to extend; [:1341](hifimule-ui/src/styles.css:1341) — device-locked gate; [:1499](hifimule-ui/src/styles.css:1499)–1516 — `.curation-track-row`.
- [Source: hifimule-ui/src/rpc.ts:190](hifimule-ui/src/rpc.ts:190) — `BrowseTrack` shape.

## Dev Agent Record

### Agent Model Used

Claude Fable 5 (claude-fable-5)

### Debug Log References

- `npx tsc --noEmit --ignoreDeprecations "6.0"` → "TypeScript: No errors found" (zero errors, none new).
- `npm run build` (vite) → 29 modules transformed, built in 201ms; only pre-existing dynamic/static import chunking warnings (`@tauri-apps/api/core`, `library.ts`) — no new errors.

### Completion Notes List

- **Surface A (list view)**: single-line predicate widening — `isSelectableListItem` now accepts resolved type `Audio`; comment updated. Verified downstream as the story predicted: `renderListRow` picks up checkbox/Ctrl/Shift for track rows automatically; plain click stays a no-op (`case 'Audio': break;` at library.ts:1999); `Audio` ∉ `CONTAINER_TYPES` so bulk basket adds use per-item `sizeBytes`/`sizeTicks` with no batch RPC; `bulkAddSelectionToPlaylist` passes raw track ids. No other library.ts changes.
- **Surface B (TracksBrowseView)**: added `selectedTrackIds: Set<string>` + `selectionAnchorIdx` keyed by `track.id`; `clearSelection()` (no-op fast path, null-safe DOM sync, removes bulk bar + `has-selection`); clearing choke point at top of `fetchTracks(reset=true)` covers all four filter funnels (selectArtist/selectAlbum/setArtistLetter/setAlbumLetter); `destroy()` clears selection + removes Escape listener (module-level instance cache — prevents AC-10 resurrection on remount); document-level capture-phase Escape handler registered in `load()`/`remount()` with double-registration guard, mirroring library.ts guards (`sl-dialog[open]`, bare `.hm-context-menu` without `.is-open`).
- **Row mechanics**: `buildTrackRow(track, index)` — index threaded from `renderTrackPanel` (forEach index) and `appendTrackRows` (offset = items.length − tracks.length, called after push). Native `<input type="checkbox">` with reused `media-list-row__check` class, `aria-label` = track title, `stopPropagation` + toggle. Row click: Ctrl/Cmd → toggle, Shift+anchor → range over `trackState.items` (anchor stays put, no type filter needed), plain click → nothing. `mousedown` Shift suppression for text-selection artifact. Range refresh syncs all rendered rows in place via `dataset.trackId` (rows are plain divs, never unmounted — AC 9 structural). Per-row (+)/(−), "Send to playlist…", contextmenu untouched.
- **Bulk bar**: created 0→1 / updated in place / removed →0, inserted in the flex wrapper directly before `#tracks-track-panel`. Count span `aria-live="polite"` with blank-then-rAF re-assert (9.11 review patch baked in). "Add to basket" carries `basket-toggle-btn` so the existing `#library-content.device-locked` CSS rule disables it (view mounts inside `#library-content`); per-row buttons' `getActiveServerId()` gate untouched. "Add to playlist…" only when `supportsPlaylistWrite`. i18n: existing `library.selection.*` keys only — no new keys.
- **Bulk handlers**: per-row (+) mapping factored into `trackToBasketItem(track): BasketItem` (used by per-row and bulk paths — factored, not forked). Selected tracks resolved in `trackState.items` order (never Set iteration). Bulk basket add: skip `basketStore.has`, add with own size metadata, added/skipped toast, clear; all synchronous, no RPC/loading state. Bulk playlist: `MediaCard.openAddToPlaylistDialog(ids, t('library.selection.new_playlist_name'), onSuccess=clear)` — cancel preserves selection (9.11 review patch: generic suggested name, not count string).
- **CSS**: comma-extended the three 9.11 rule groups (checkbox reveal incl. `.curation-track-panel.has-selection`, `is-checked` tint) — no parallel rules, `opacity: 0` hiding preserved (Tab order, AC 11). `.bulk-action-bar` untouched. `is-checked` tint wins over `.curation-track-row:hover` by source order (same specificity, later rule).
- **Verification**: tsc + vite build gates pass with zero new errors. No test framework exists in `hifimule-ui` (per story: do not scaffold) — the 11-point manual runtime checklist in Task 7 is handed to code review, as the UI requires the Tauri daemon + a connected media server and cannot be exercised headless.

### File List

- `hifimule-ui/src/library.ts` (modified — `isSelectableListItem` predicate + comment only)
- `hifimule-ui/src/components/TracksBrowseView.ts` (modified — selection state, lifecycle clearing, Escape listener, checkbox + click semantics, bulk bar, bulk handlers, `trackToBasketItem` factoring)
- `hifimule-ui/src/styles.css` (modified — comma-extended checkbox reveal + `is-checked` tint rules to `.curation-track-row`/`.curation-track-panel`; comment updated)

## Change Log

- 2026-06-12: Story created from sprint-change-proposal-2026-06-12-track-multi-selection (Epic 9, follow-up to 9.11). Ultimate context engine analysis completed — comprehensive developer guide created.
- 2026-06-12: Implementation complete (Tasks 1–7). Track rows selectable in both surfaces: list view via one-line predicate widening; Tracks dual-panel view via new selection machinery mirroring 9.11 (checkboxes, Ctrl/Shift semantics, bulk bar, bulk basket/playlist handlers, AC-10 clearing via fetchTracks reset + destroy + Escape). All three 9.11 review patches inherited from day one. Build gates green (tsc, vite). Status → review.
