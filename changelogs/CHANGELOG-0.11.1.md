# HifiMule 0.11.1

Release date: 2026-06-12

## Highlights

- **Multi-selection & bulk actions**: Select many artists, albums, or tracks at once in the library and act on them together — add them all to your basket or to a playlist in a single click, instead of clicking every row one by one.
- **Works everywhere you browse lists**: Selection is available in the list/table view (artists, albums, and now individual tracks — including album drill-downs, history modes, and favorite tracks) and in the Tracks dual-panel browse view.
- **Familiar selection gestures**: Hover-revealed checkboxes, Ctrl/Cmd-click to toggle a row, and Shift-click to select a whole range. Selection survives scrolling and "load more", and is announced for keyboard and screen-reader users.

---

## Added

### List-view multi-selection & bulk actions (Story 9.11)

- **Selection checkboxes** on artist and album rows in the list/table view. A checkbox appears on hover or keyboard focus, and stays visible while any selection is active.
- **Selection gestures**: click the checkbox or Ctrl/Cmd-click a row to toggle it (without navigating into the item); Shift-click to select every selectable row between the anchor and the clicked row, inclusive.
- **Bulk action bar**: as soon as one row is selected, a sticky bar appears showing the selection count, **Add to basket**, **Add to playlist…** (only on servers that support playlist writing), and a **Clear** button. Per-row actions keep working exactly as before.
- **Bulk add to basket**: items already in the basket are skipped, the rest are added with their counts/sizes fetched in a single batched request, a toast reports how many were added and skipped, and the selection clears.
- **Bulk add to playlist**: opens the existing playlist picker seeded with every selected item — choose an existing playlist or create a new one. The selection clears on success and is preserved if you cancel the dialog.
- **Selection survives** scrolling rows out of view and back, and "load more" pagination; it clears automatically when you switch browse mode, drill in, change the A–Z filter, toggle to grid view, or press Escape.
- **Keyboard & accessibility**: checkboxes are focusable and toggle with Space, the bulk-bar buttons are in tab order, and the selection count is announced via an ARIA-live region (including the first 0→1 selection).
- **Device-aware**: "Add to basket" is disabled when no device is selected (mirroring the per-row add button); "Add to playlist…" stays available.

### Track multi-selection & bulk actions (Story 9.12)

- **Track rows are now selectable** in the list view too — in an album's track list, in the Frequently/Recently Played history modes, and among favorite tracks. They reuse all of the Story 9.11 selection mechanics with no behavior change.
- **Tracks dual-panel browse view** gains its own selection: checkboxes on track rows, Ctrl/Cmd-click toggle, Shift-range selection, and a bulk action bar above the track panel, alongside the existing per-row (+)/(−) and "Send to playlist…" actions, which are unchanged.
- **Bulk add tracks to basket**: tracks already in the basket are skipped and the rest are added instantly with their own size metadata (no extra server round-trip for tracks); a toast reports added/skipped counts and the selection clears.
- **Bulk add tracks to playlist**: opens the playlist picker seeded with all selected track ids; existing-playlist and create-new flows behave as in the list view, clearing on success and preserving the selection on cancel.
- **Selection clears** when you change the artist filter, album filter, or A–Z letter, leave the Tracks mode (and does not resurrect on re-entry), or press Escape.

---

## Changed

- **`MediaCard` playlist dialogs are now plural**: `openAddToPlaylistDialog` and `openCreatePlaylistDialog` accept `itemIds: string[]` (plus an optional `onSuccess` callback) so they can seed a playlist with many items at once. All existing single-item call sites pass one-element arrays — no behavior change for per-row actions.
- **List rows obey the device-locked gate**: a per-row add button on list rows now carries the `basket-toggle-btn` class, so it is disabled when no device is selected — closing a pre-existing gap and making the bulk button's "mirror per-row behavior" rule true.

---

## Fixed

- **Track de-duplication across pages**: in the Tracks view, tracks already loaded are dropped when more pages are appended, and paging now advances by the raw page size — preventing duplicate playlist payloads, desynced row toggles, and re-fetch loops.
- **No false-success bulk basket toast**: bulk "Add to basket" in the Tracks view bails before adding (and the button is disabled) when there is no active server, so it no longer reports a phantom success or wipes the selection.
- **Keyboard can't bypass the device gate**: the Tracks-view bulk "Add to basket" button is properly disabled, so Enter/Space cannot trigger it when no device is selected.
- **Tracks-view listeners register earlier**: the Escape handler and basket subscription are wired up before the view's initial fetches settle, closing an inert-Escape window and a teardown-during-load leak.

---

## Internal

- All work is UI-only — no daemon, provider, manifest, sync-engine, or RPC changes. The daemon playlist contract was already plural (`playlist.create` / `playlist.addItems` take `itemIds: string[]`).
- Files touched: `hifimule-ui/src/library.ts` (selection state, bulk bar, batched basket add, clearing hooks, Escape handling), `hifimule-ui/src/components/TracksBrowseView.ts` (selection machinery, bulk handlers, lifecycle clearing), `hifimule-ui/src/components/MediaCard.ts` (plural dialogs), `hifimule-ui/src/styles.css` (shared checkbox/tint/bulk-bar rules), `hifimule-i18n/catalog.json`.
- Selection is held as id-keyed app state (never DOM state) so it survives virtualization and autoload; bulk basket-add logic is factored into a single helper shared by per-row and bulk paths.
- i18n: seven new `library.selection.*` keys added to all four languages (English, French, Spanish, German); Story 9.12 reuses them with no new keys.
- No test framework exists in `hifimule-ui`; changes were validated via `tsc`/`vite` build gates and a documented manual runtime checklist handed to review.
