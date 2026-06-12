# Sprint Change Proposal — List View Multi-Selection & Bulk Actions

**Date:** 2026-06-12
**Author:** Alexis (facilitated by Dev agent, correct-course workflow)
**Status:** APPROVED (2026-06-12) — artifact edits applied
**Change scope classification:** Minor (direct implementation by Dev agent)

---

## 1. Issue Summary

### Problem Statement

The virtualized list/table browse view (Stories 9.7, 9.8) only supports **single-item actions**: each row exposes one (+)/(-) basket toggle and a per-item right-click "Send to playlist…" context menu. A user curating from a large library — exactly the scenario the list view was built for — must repeat the same gesture once per artist or album. There is no way to select several artists or albums and act on the whole selection at once.

### Context & Discovery

Identified during day-to-day use of the list view after Epic 9 (Rich Library Navigation) and Epic 11 (Selection-as-Playlist & Curation) completed. The list view made *scanning* thousands of items fast, but *acting* on them remained O(n) gestures. This is a new UX requirement, not a defect: nothing in the PRD, epics, or UX spec currently specifies multi-selection.

### Evidence

- `renderListRow()` in `hifimule-ui/src/library.ts:655` wires exactly one basket toggle and one context-menu target per row; no selection state exists in the `library.ts` UI state.
- The right-click menu (`MediaCard.showItemContextMenu`, `MediaCard.ts:302`) accepts a single `itemId`.
- Conversely, the daemon side is **already plural**: `playlist.create { name, itemIds: [...] }` and `playlist.addItems { playlistId, itemIds: [...] }` accept arrays (Story 11.4 resolves container entities — artists, albums — to ordered track lists server-side), and `jellyfin_get_item_counts` / `jellyfin_get_item_sizes` are batch RPCs. The gap is purely in the UI.

---

## 2. Impact Analysis

### Epic Impact

| Epic | Impact |
|------|--------|
| Epic 9 — Rich Library Navigation | **Reopened.** New Story 9.11 added (list-view multi-selection + bulk action bar). No existing story is modified; 9.7/9.8 ACs remain valid as-is. |
| Epic 3 — Curation Hub (Basket) | None. The bulk "Add to basket" action reuses `basketStore.add` semantics and the existing batch count/size RPCs. |
| Epic 11 — Selection-as-Playlist | None. The bulk "Add to playlist…" action reuses the Story 11.5/11.7 dialogs and the existing `playlist.create` / `playlist.addItems` RPCs, which already accept `itemIds` arrays and resolve containers to tracks. |
| All other epics | None. |

### Story Impact

- **New:** Story 9.11 — List View Multi-Selection & Bulk Actions (full draft in Section 4).
- **Modified:** none. (Story 9.10's Tracks dual-panel view is explicitly out of scope — see Scope Boundaries.)

### Artifact Conflicts

| Artifact | Impact |
|----------|--------|
| PRD | New **FR47** in §3 Content Selection & Browsing (FR42–FR46 are already allocated to multi-server work in the epics coverage map). |
| Epics (`epics.md`) | New Story 9.11 under Epic 9; FR coverage map gains an FR47 line. |
| UX Design Spec | §5.1 "List/Table Browse View" amended; new §5.2 component "List Multi-Select & Bulk Action Bar". |
| Architecture | **No change.** Pure UI rendering/interaction concern; no new daemon RPCs, no trait or schema changes (same classification as Stories 9.7/9.8). |
| Sprint status (`sprint-status.yaml`) | `epic-9` → `in-progress`; add `9-11-list-view-multi-selection-and-bulk-actions: backlog`. |

### Technical Impact

- UI-only change in `hifimule-ui` (`library.ts`, `MediaCard.ts`, CSS, i18n catalogs en/fr/es).
- Selection state must be id-keyed in `library.ts` state (not DOM-based), because virtualized rows are unmounted/remounted on scroll (`paint()` in `renderList()`).
- `MediaCard.openAddToPlaylistDialog` / `openCreatePlaylistDialog` signatures generalize from a single `itemId` to `itemIds: string[]` (existing single-item callers pass a one-element array — no behavior change).
- No daemon, provider, manifest, or sync-engine changes. No new RPCs.

### Scope Boundaries (explicitly out of scope for this change)

1. **Grid view multi-selection** — the request targets the list view; grid cards keep single-item actions.
2. **Tracks dual-panel browse mode (9.10) and Playlist Curation view (11.6)** — these have their own panel-based interaction models; extending multi-select there can be a future change.
3. **Bulk remove from basket** — the bulk bar adds; removing stays per-row or via the basket sidebar. (Selected rows already in the basket are skipped, not duplicated.)

---

## 3. Recommended Approach

**Selected path: Option 1 — Direct Adjustment** (add one story to the existing epic structure).

**Rationale:**

- The daemon contract is already plural; the entire change is UI composition over shipped, tested flows (basket add semantics from Epic 3, playlist write flows from Epic 11). One well-scoped story covers it.
- Rollback (Option 2) is meaningless — nothing needs reverting; Stories 9.7/9.8 are correct, just not yet multi-select-aware.
- MVP review (Option 3) is not applicable — MVP shipped; this is post-MVP UX enhancement and threatens no existing goal.

**Effort estimate:** Low-Medium (1 story; UI state + rendering + two bulk handlers + i18n + tests).
**Risk:** Low. Main risks are virtualization edge cases (selection survival across row unmount/remount and autoload appends) and Shift-range semantics — both contained in `library.ts` and coverable by component tests.
**Timeline impact:** None on other work; Epic 9 reopens for one story.

---

## 4. Detailed Change Proposals

### 4.1 PRD — new FR47 (§3. Content Selection & Browsing)

**OLD:** *(section ends at FR30)*

**NEW (append after FR30):**

> - **FR47:** In the virtualized list/table browse view, users can select multiple rows representing artists or albums via per-row checkboxes, Ctrl/Cmd-click toggling, and Shift-click range selection. While at least one row is selected, a bulk action bar shows the selection count and offers: "Add to basket" (adds each selected entity using existing basket semantics, batch-fetching counts/sizes; entities already in the basket are skipped) and "Add to playlist…" (gated on `supports_playlist_write`; opens the existing create-new / add-to-existing playlist flow with all selected item IDs, resolved server-side to tracks). Selection state is keyed by item id, survives virtualization scrolling and autoload-on-scroll, and is cleared on browse-mode change, drill-down navigation, A–Z filter change, view-mode toggle, or Escape.

**Rationale:** Captures the new capability at requirement level, reusing FR37/FR38/FR39 vocabulary. FR47 is the next free number (FR42–FR46 are allocated in the epics coverage map).

### 4.2 Epics — new Story 9.11 (Epic 9: Rich Library Navigation)

**OLD:** *(Epic 9 ends at Story 9.10)*

**NEW (append):**

> ### Story 9.11: List View Multi-Selection & Bulk Actions
>
> As a Ritualist (Arthur),
> I want to select multiple artists or albums in the list view and act on them all at once,
> So that I can build my basket or a playlist in seconds instead of clicking every row.
>
> **Acceptance Criteria:**
>
> **Given** the list/table view is active and a row represents an artist or album (resolved type `MusicArtist` or `MusicAlbum`)
> **When** the row renders
> **Then** it displays a leading selection checkbox (visible on hover/focus, and always visible while any selection is active).
>
> **Given** I click a row's checkbox or Ctrl/Cmd-click the row
> **Then** the row's selection toggles without navigating into the item.
>
> **Given** a row is the selection anchor and I Shift-click another row
> **Then** all selectable rows between the two indices (inclusive) become selected.
>
> **Given** at least one row is selected
> **Then** a bulk action bar appears in the browse area showing the selection count, an "Add to basket" button, an "Add to playlist…" button (only when `supports_playlist_write` is true), and a "Clear" affordance.
> **And** all per-row single-item actions continue to work unchanged.
>
> **Given** I click "Add to basket" with N items selected
> **Then** items already in the basket are skipped, counts/sizes for the remaining items are fetched in a single batched `jellyfin_get_item_counts` + `jellyfin_get_item_sizes` call pair, each item is added to the basket with existing semantics (artist entity items per Story 3.9), a success toast reports added/skipped counts, and the selection clears.
>
> **Given** I click "Add to playlist…" with N items selected
> **Then** the existing playlist picker dialog (Story 11.7) opens; choosing an existing playlist calls `playlist.addItems { playlistId, itemIds: [all N ids] }`, choosing "New playlist" opens the create dialog and calls `playlist.create { name, itemIds: [all N ids] }`; on success the playlists cache is invalidated, a toast confirms, and the selection clears.
>
> **Given** no device is selected (`selectedDevicePath === null`)
> **Then** "Add to basket" renders disabled (mirroring per-row (+) behavior); "Add to playlist…" remains available when `supports_playlist_write` is true.
>
> **Given** rows are selected and I scroll far enough that selected rows unmount and remount (virtualization), or autoload appends pages
> **Then** selection state is preserved and remounted rows render as selected.
>
> **Given** rows are selected
> **When** I change browse mode, drill into an item, change the A–Z filter, toggle to grid view, or press Escape
> **Then** the selection and the bulk action bar are cleared.
>
> **Given** keyboard-only navigation
> **Then** checkboxes are focusable and toggleable via Space, the bulk bar buttons are reachable in tab order, and the selection count is announced via an ARIA-live region.
>
> **Technical Notes:**
> - Selection state in `library.ts` UI state: `selectedIds: Set<string>` + `selectionAnchorIdx: number | null`, keyed by `item.basketId ?? item.id` (same id used by `basketStore.has` in `renderListRow`, `library.ts:656`). Items are looked up from `state.items` at action time — never from the DOM (virtualized rows unmount).
> - Selectability predicate: `(item.basketType ?? item.type)` is `MusicArtist` or `MusicAlbum`. Playlist, genre, and track rows do not render checkboxes in v1.
> - `renderListRow` renders the checkbox + `is-checked` class from `selectedIds`; the existing `paint()` repaint path makes remounted rows pick up selection state for free.
> - Bulk action bar: a sibling of the list scroller in `#library-content` (sticky, above the list), rendered/torn down by the same code path that manages the list (`renderList` / `teardownListScrollHandler`); re-rendered on selection change.
> - Bulk basket add reuses the per-row add logic factored out of `renderListRow`'s toggle handler — including the container metadata fetch — but with a single batched `itemIds` array for counts/sizes.
> - `MediaCard.openAddToPlaylistDialog(itemIds: string[], label: string)` and `openCreatePlaylistDialog(itemIds: string[], suggestedName: string)` generalize their current single-id signatures; existing callers (context menu, track rows) pass one-element arrays. Daemon-side container→track resolution already exists (Story 11.4) — no RPC changes.
> - Cross-server safety: the browse list only ever shows the active server's items, and `playlist.*` RPCs already enforce server scope (409 on cross-server items, Story 11.4 amendment) — no new handling needed.
> - New i18n keys (en/fr/es): `library.selection.count`, `library.selection.add_to_basket`, `library.selection.add_to_playlist`, `library.selection.clear`, `library.selection.added_toast`, `library.selection.skipped_suffix`.
> - No new daemon RPCs; pure UI concern (same classification as Stories 9.7/9.8).

**Rationale:** One story, since the daemon contract already supports everything and both bulk actions share the selection mechanics.

### 4.3 Epics — FR Coverage Map

**OLD:**
> FR46: Epic 2 - Portable Server Identity (Story 2.13)

**NEW:**
> FR46: Epic 2 - Portable Server Identity (Story 2.13)
> FR47: Epic 9 - List View Multi-Selection & Bulk Actions (Story 9.11)

### 4.4 UX Design Specification — §5.1 List/Table Browse View

**OLD (end of the §5.1 "List/Table Browse View" bullet):**
> Breadcrumb navigation, synced badges, and basket add interactions are identical to grid view. Data is shared from the existing `browse.*` RPC layer; switching views does not re-fetch from the daemon.

**NEW:**
> Breadcrumb navigation, synced badges, and basket add interactions are identical to grid view. Data is shared from the existing `browse.*` RPC layer; switching views does not re-fetch from the daemon. Artist and album rows additionally support **multi-selection** (checkbox, Ctrl/Cmd-click, Shift-click range) with a bulk action bar for adding the whole selection to the basket or to a playlist — see "List Multi-Select & Bulk Action Bar" in §5.2.

### 4.5 UX Design Specification — new §5.2 component

**NEW (append to §5.2 Custom Components):**

> *   **List Multi-Select & Bulk Action Bar:** In the virtualized list view, artist and album rows render a leading selection checkbox (visible on hover/focus; always visible while a selection is active). Ctrl/Cmd-click toggles a row; Shift-click selects the range from the anchor row. While ≥1 row is selected, a sticky bulk action bar appears above the list showing "N selected" (ARIA-live), a **"Add to basket"** button (disabled when no device is selected, mirroring per-row (+) behavior; already-basketed items are skipped with a toast summary), an **"Add to playlist…"** button (shown only when `supports_playlist_write` is true; opens the existing playlist picker / create dialog seeded with all selected item ids), and a **"Clear"** affordance (also Escape). Selection survives scrolling and autoload (id-keyed, virtualization-safe) and clears on browse-mode change, drill-down, A–Z filter change, or toggling back to grid view. Single-row actions remain unchanged alongside multi-select.

---

## 5. Implementation Handoff

**Scope classification: Minor** — direct implementation by the Developer agent. No backlog reorganization beyond reopening Epic 9; no PM/Architect escalation (zero architecture impact).

**Handoff plan:**

| Role | Responsibility |
|------|----------------|
| Dev agent (`create-story`) | Generate the Story 9.11 context file from the epic entry above. |
| Dev agent (`dev-story`) | Implement Story 9.11 in `hifimule-ui` (library.ts selection state + bulk bar, MediaCard dialog generalization, CSS, i18n en/fr/es, component tests). |
| Dev agent (code-review) | Standard adversarial review on completion, with attention to virtualization selection-survival and Shift-range edge cases. |

**Artifact updates on approval (this proposal's executor):**

1. `prd.md` — add FR47 (§4.1).
2. `epics.md` — add Story 9.11 + coverage map line (§4.2, §4.3).
3. `ux-design-specification.md` — amend §5.1, add §5.2 component (§4.4, §4.5).
4. `sprint-status.yaml` — `epic-9: in-progress`; add `9-11-list-view-multi-selection-and-bulk-actions: backlog`.

**Success criteria:**

- A user in list view can select any number of artists/albums (including across autoloaded pages) and add them all to the basket in one action, with correct storage projection.
- The same selection can be sent to a new or existing server playlist in one action, with server-side track resolution.
- No regression in single-row actions, virtualization performance, or grid view.

---

## 6. Checklist Execution Record

| Section | Status | Notes |
|---------|--------|-------|
| 1. Trigger & Context | [x] Done | New stakeholder requirement; evidence from `library.ts` / `MediaCard.ts` single-item wiring vs. plural daemon RPCs. |
| 2. Epic Impact | [x] Done | Epic 9 reopened with Story 9.11; no other epic affected. |
| 3. Artifact Conflicts | [x] Done | PRD FR47; epics + coverage map; UX spec §5.1/§5.2. Architecture: N/A (pure UI). Secondary artifacts (CI, deployment, docs): N/A. |
| 4. Path Forward | [x] Done | Option 1 Direct Adjustment selected (effort Low-Medium, risk Low). Rollback and MVP review not viable/applicable. |
| 5. Proposal Components | [x] Done | Sections 1–5 above. |
| 6. Final Review & Handoff | [!] Action-needed | Awaiting explicit user approval; sprint-status.yaml update deferred until approved. |
