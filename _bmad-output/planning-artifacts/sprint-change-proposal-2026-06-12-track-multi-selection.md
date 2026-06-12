# Sprint Change Proposal — Track Multi-Selection & Bulk Actions

**Date:** 2026-06-12
**Author:** Alexis (facilitated by Dev agent, correct-course workflow, batch mode)
**Status:** APPROVED (2026-06-12) — artifact edits applied
**Change scope classification:** Minor (direct implementation by Dev agent)

---

## 1. Issue Summary

### Problem Statement

Story 9.11 introduced multi-selection with bulk "Add to basket" / "Add to playlist…" actions, but only for **artist and album rows** in the virtualized list view. Track rows were explicitly excluded ("Playlist, genre, and track rows do not render checkboxes in v1"), and the Tracks dual-panel browse view (Story 9.10) — the primary surface for track-grain curation — was declared out of scope. A user who wants to send twenty individual tracks to a playlist or the basket is back to O(n) gestures, exactly the friction 9.11 removed for containers.

### Context & Discovery

Identified immediately after Story 9.11 shipped (2026-06-12), during day-to-day use. This is the planned follow-up the 9.11 proposal's Scope Boundaries anticipated: "extending multi-select there can be a future change." New UX requirement, not a defect.

### Evidence

- `isSelectableListItem()` (`hifimule-ui/src/library.ts:665`) returns true only for resolved types `MusicArtist` / `MusicAlbum`; track rows (`Audio`, mapped by `mapAlbumTracks`, `library.ts:328`) render in the same virtualized list with no checkbox.
- `TracksBrowseView.ts` (`buildTrackRow`, line 460) has its own row renderer with per-row (+)/(-) basket and "Send to playlist…" buttons, but no selection state at all.
- The downstream machinery is already track-ready: `addBrowseItemsToBasket` (`library.ts:825`) adds non-container types directly using the item's own `sizeBytes`/`sizeTicks` (no count/size RPC needed for tracks), and `MediaCard.openAddToPlaylistDialog(itemIds: string[])` / `openCreatePlaylistDialog(itemIds: string[])` already accept arrays (generalized by 9.11). `playlist.create` / `playlist.addItems` accept track ids natively. The gap is purely selection UI.

---

## 2. Impact Analysis

### Epic Impact

| Epic | Impact |
|------|--------|
| Epic 9 — Rich Library Navigation | **Still open** (9.11 just completed). New Story 9.12 added (track multi-selection on both track surfaces). No existing story modified; 9.10/9.11 ACs remain valid. |
| Epic 3 — Curation Hub (Basket) | None. Tracks are already first-class basket entities with pre-populated sizes. |
| Epic 11 — Selection-as-Playlist | None. Reuses the 11.7 dialogs and plural `playlist.*` RPCs unchanged. |
| All other epics | None. No resequencing, no invalidated work. |

### Story Impact

- **New:** Story 9.12 — Track Multi-Selection & Bulk Actions (full draft in Section 4).
- **Modified:** none.

### Artifact Conflicts

| Artifact | Impact |
|----------|--------|
| PRD | New **FR48** in §3 Content Selection & Browsing (FR47 was 9.11; 48 is the next free number). FR47 untouched for traceability. |
| Epics (`epics.md`) | New Story 9.12 under Epic 9; FR coverage map gains an FR48 line. |
| UX Design Spec | §5.1 "List/Table Browse View" sentence amended (track rows selectable); §5.2 "Tracks Browse View" amended (selection + bulk bar); §5.2 "List Multi-Select & Bulk Action Bar" amended (covers track rows and the Tracks view). |
| Architecture | **No change.** Pure UI; no new RPCs, traits, or schemas (same classification as 9.7/9.8/9.11). |
| Sprint status (`sprint-status.yaml`) | Add `9-12-track-multi-selection-and-bulk-actions: backlog` (epic-9 already `in-progress`). |

### Technical Impact

- **Surface A — virtualized list view (`library.ts`):** widen `isSelectableListItem` to also accept resolved type `Audio`. Everything else (checkbox rendering, anchor/range, bulk bar, both bulk handlers, virtualization survival, clearing rules, ARIA) is inherited from 9.11 with no changes — `addBrowseItemsToBasket` already branches tracks away from the batch count/size RPCs.
- **Surface B — Tracks dual-panel view (`TracksBrowseView.ts`):** new component-local selection state (`selectedTrackIds: Set<string>` + anchor index into `trackState.items`), a leading checkbox in `buildTrackRow`, Ctrl/Cmd-click and Shift-range handling, and a bulk action bar reusing the 9.11 `.bulk-action-bar` CSS, the `library.selection.*` i18n keys, and the same two bulk handlers' semantics. The track panel is append-on-scroll (not virtualized), so selection survival across autoload is automatic (rows are never unmounted); id-keying still required for correctness across re-renders.
- **No new i18n keys** — the seven `library.selection.*` keys from 9.11 cover both surfaces (en/fr/es already translated).
- No daemon, provider, manifest, or sync-engine changes. No new RPCs.

### Scope Boundaries (explicitly out of scope for this change)

1. **Grid view multi-selection** — unchanged from 9.11's boundary.
2. **Playlist Curation View (11.6) main track list** — its interaction model is remove/reorder-oriented, and its "Add tracks" dialog already has its own checkbox multi-select. Not touched.
3. **Bulk remove from basket** — the bulk bar adds; removal stays per-row or via the basket sidebar (selected tracks already in the basket are skipped, not duplicated).
4. **Mixed cross-panel selection in the Tracks view** — only track rows are selectable; the artist/album filter panels are filters, not selection sources.

---

## 3. Recommended Approach

**Selected path: Option 1 — Direct Adjustment** (add one story to the existing epic structure).

**Rationale:**

- The entire feature is composition over shipped, tested parts: the 9.11 selection model and bulk handlers, the track-ready basket add path, and the plural playlist dialogs. One story covers both surfaces because they share semantics, CSS, and i18n; only the state-holder differs (global `library.ts` state vs. component-local).
- Rollback (Option 2) is not viable — nothing needs reverting; 9.11 is correct, just scoped to containers.
- MVP review (Option 3) is not applicable — post-MVP UX enhancement, threatens no goal.

**Effort estimate:** Low (predicate widening on Surface A; selection state + checkbox + bulk bar wiring on Surface B; tests).
**Risk:** Low. Main risks are Shift-range correctness against `trackState.items` indices in the Tracks view and selection-clearing on the three filter axes (artist, album, A–Z) — all contained in `TracksBrowseView.ts`.
**Timeline impact:** None on other work; Epic 9 remains open for one more story.

---

## 4. Detailed Change Proposals

### 4.1 PRD — new FR48 (§3. Content Selection & Browsing)

**OLD:** *(section ends at FR47)*

**NEW (append after FR47):**

> - **FR48:** Multi-selection and bulk actions extend to individual track rows on both track surfaces: (a) in the virtualized list/table browse view, track rows (e.g., tracks within an album) are selectable exactly like artist and album rows per FR47; (b) in the Tracks dual-panel browse view, track rows support per-row checkboxes, Ctrl/Cmd-click toggling, and Shift-click range selection, with a bulk action bar offering "Add to basket" (tracks already in the basket are skipped; track sizes come from the items themselves — no batch count/size fetch) and "Add to playlist…" (gated on `supports_playlist_write`; opens the existing create-new / add-to-existing playlist flow with all selected track IDs). In the Tracks view, selection is keyed by track id, survives autoload-on-scroll pagination, and is cleared on artist filter change, album filter change, A–Z letter change, leaving the Tracks mode, or Escape.

**Rationale:** Captures track-grain selection at requirement level without rewording FR47 (which stays mapped 1:1 to Story 9.11 in the coverage map).

### 4.2 Epics — new Story 9.12 (Epic 9: Rich Library Navigation)

**OLD:** *(Epic 9 ends at Story 9.11)*

**NEW (append after Story 9.11, before Epic 10):**

> ### Story 9.12: Track Multi-Selection & Bulk Actions
>
> As a Ritualist (Arthur),
> I want to select multiple tracks — in an album's track list or in the Tracks browse view — and act on them all at once,
> So that I can send a batch of individual songs to my basket or a playlist without clicking every row.
>
> **Acceptance Criteria:**
>
> **Given** the virtualized list view shows track rows (resolved type `Audio`, e.g. tracks within an album)
> **When** a row renders
> **Then** it displays the same leading selection checkbox as artist/album rows, and all Story 9.11 selection mechanics (Ctrl/Cmd-click, Shift-range, bulk bar, virtualization survival, clearing rules, keyboard/ARIA) apply unchanged to track rows.
>
> **Given** tracks are selected in the list view and I click "Add to basket"
> **Then** tracks already in the basket are skipped, the remaining tracks are added using their own `sizeBytes`/`sizeTicks` (no count/size batch RPC for tracks), a toast reports added/skipped counts, and the selection clears.
>
> **Given** the Tracks dual-panel browse view is active
> **When** a track row renders in the bottom track panel
> **Then** it displays a leading selection checkbox (visible on hover/focus, always visible while any selection is active), alongside the existing per-row (+)/(-) and "Send to playlist…" actions, which continue to work unchanged.
>
> **Given** I click a track row's checkbox or Ctrl/Cmd-click the row
> **Then** the row's selection toggles.
>
> **Given** a track row is the selection anchor and I Shift-click another track row
> **Then** all track rows between the two indices (inclusive, within the currently loaded track list) become selected.
>
> **Given** at least one track is selected in the Tracks view
> **Then** a bulk action bar appears above the track panel showing the selection count (ARIA-live), an "Add to basket" button (disabled when no device is selected), an "Add to playlist…" button (only when `supports_playlist_write` is true), and a "Clear" affordance.
>
> **Given** I click "Add to basket" with N tracks selected in the Tracks view
> **Then** tracks already in the basket are skipped, each remaining track is added via `basketStore.add` with its own size metadata, a toast reports added/skipped counts, and the selection clears.
>
> **Given** I click "Add to playlist…" with N tracks selected
> **Then** the existing playlist picker dialog opens seeded with all N track ids; existing-playlist and create-new flows behave per Story 9.11, the playlists cache is invalidated on success, a toast confirms, and the selection clears. Cancelling preserves the selection.
>
> **Given** tracks are selected in the Tracks view and autoload appends more pages to any panel
> **Then** the selection is preserved (id-keyed).
>
> **Given** tracks are selected in the Tracks view
> **When** I change the artist filter, the album filter, or the A–Z letter, leave the Tracks mode, or press Escape
> **Then** the selection and the bulk action bar are cleared.
>
> **Given** keyboard-only navigation in the Tracks view
> **Then** checkboxes are focusable and toggleable via Space, bulk bar buttons are reachable in tab order, and the selection count is announced via an ARIA-live region.
>
> **Technical Notes:**
> - Surface A is a one-line predicate widening: `isSelectableListItem` (`library.ts:665`) accepts resolved type `Audio` in addition to `MusicArtist`/`MusicAlbum`. `addBrowseItemsToBasket` already handles `Audio` outside `CONTAINER_TYPES` (no batch RPC); the bulk playlist handler already passes raw ids.
> - Surface B selection state lives in `TracksBrowseView`: `selectedTrackIds: Set<string>` + `selectionAnchorIdx: number | null` indexed into `trackState.items`. Cleared by the same code paths that reset `trackState` (filter changes, mode exit).
> - `buildTrackRow` renders the checkbox and a selected-row class; reuse the 9.11 checkbox/bulk-bar CSS (`.media-list-row__check`, `.bulk-action-bar`) — extract shared row-check styles to apply to `.curation-track-row` rather than duplicating rules.
> - Bulk handlers in `TracksBrowseView` mirror the 9.11 handlers: basket add maps `BrowseTrack` → `basketStore.add({ id, type: 'Audio', sizeBytes, sizeTicks, … })` (same mapping as the per-row (+) handler, factored out and looped); playlist add calls `MediaCard.openAddToPlaylistDialog(ids, label, onSuccess)`.
> - Reuse the existing `library.selection.*` i18n keys (en/fr/es) — no new keys.
> - Track panel is append-rendered (not virtualized), so no unmount/remount concerns; keep selection id-keyed anyway for re-render correctness.
> - No new daemon RPCs; pure UI concern.

**Rationale:** One story: both surfaces share the 9.11 selection semantics, handlers, CSS, and i18n; Surface A alone is too small to stand as a story.

### 4.3 Epics — FR Coverage Map

**OLD:**
> FR47: Epic 9 - List View Multi-Selection & Bulk Actions (Story 9.11)

**NEW:**
> FR47: Epic 9 - List View Multi-Selection & Bulk Actions (Story 9.11)
> FR48: Epic 9 - Track Multi-Selection & Bulk Actions (Story 9.12)

### 4.4 UX Design Specification — §5.1 List/Table Browse View

**OLD (end of the §5.1 "List/Table Browse View" bullet):**
> Artist and album rows additionally support **multi-selection** (checkbox, Ctrl/Cmd-click, Shift-click range) with a bulk action bar for adding the whole selection to the basket or to a playlist — see "List Multi-Select & Bulk Action Bar" in §5.2.

**NEW:**
> Artist, album, and track rows additionally support **multi-selection** (checkbox, Ctrl/Cmd-click, Shift-click range) with a bulk action bar for adding the whole selection to the basket or to a playlist — see "List Multi-Select & Bulk Action Bar" in §5.2.

### 4.5 UX Design Specification — §5.2 Tracks Browse View

**OLD (end of the "Tracks Browse View (dual-panel, paginated)" bullet):**
> A–Z filter controls remain available on the artist and album panels. The mode is hidden when the active provider does not advertise the Tracks capability (e.g., classic Subsonic without `search3`).

**NEW:**
> A–Z filter controls remain available on the artist and album panels. The mode is hidden when the active provider does not advertise the Tracks capability (e.g., classic Subsonic without `search3`). Track rows additionally support **multi-selection** with the same checkbox / Ctrl/Cmd-click / Shift-range mechanics and bulk action bar as the list view (see "List Multi-Select & Bulk Action Bar"); selection survives autoload pagination and clears on artist/album filter change, A–Z change, leaving the mode, or Escape.

### 4.6 UX Design Specification — §5.2 List Multi-Select & Bulk Action Bar

**OLD (start of the bullet):**
> *   **List Multi-Select & Bulk Action Bar:** In the virtualized list view, artist and album rows render a leading selection checkbox (visible on hover/focus; always visible while a selection is active).

**NEW:**
> *   **List Multi-Select & Bulk Action Bar:** In the virtualized list view, artist, album, and track rows render a leading selection checkbox (visible on hover/focus; always visible while a selection is active); the same component also applies to track rows in the Tracks Browse View's track panel.

*(remainder of the bullet unchanged; its basket note "already-basketed items are skipped with a toast summary" and playlist gating apply identically to tracks — track sizes come from the items themselves, with no batch count/size fetch.)*

---

## 5. Implementation Handoff

**Scope classification: Minor** — direct implementation by the Developer agent. No backlog reorganization beyond adding one story to the already-open Epic 9; no PM/Architect escalation (zero architecture impact).

**Handoff plan:**

| Role | Responsibility |
|------|----------------|
| Dev agent (`create-story`) | Generate the Story 9.12 context file from the epic entry above. |
| Dev agent (`dev-story`) | Implement Story 9.12 in `hifimule-ui` (predicate widening in `library.ts`; selection state, checkboxes, and bulk bar in `TracksBrowseView.ts`; shared CSS extraction; tests). |
| Dev agent (code-review) | Standard adversarial review, with attention to Shift-range indexing against `trackState.items` and selection-clearing on all three filter axes. |

**Artifact updates on approval (this proposal's executor):**

1. `prd.md` — add FR48 (§4.1).
2. `epics.md` — add Story 9.12 + coverage map line (§4.2, §4.3).
3. `ux-design-specification.md` — amend §5.1 and the two §5.2 bullets (§4.4–§4.6).
4. `sprint-status.yaml` — add `9-12-track-multi-selection-and-bulk-actions: backlog`.

**Success criteria:**

- In an album's list view, any number of tracks can be selected and added to the basket or a playlist in one action.
- In the Tracks browse view, tracks selected across autoloaded pages (and across filter narrowing sessions, one batch at a time) can be bulk-added to the basket or a playlist, with correct skip-already-basketed behavior.
- No regression in per-row track actions, Tracks-view pagination/filtering, or 9.11 container selection.

---

## 6. Checklist Execution Record

| Section | Status | Notes |
|---------|--------|-------|
| 1. Trigger & Context | [x] Done | Follow-up to Story 9.11 (its Scope Boundaries pre-announced this change); new stakeholder requirement; evidence from `library.ts:665` predicate and `TracksBrowseView.ts` selection-less rows vs. track-ready bulk machinery. |
| 2. Epic Impact | [x] Done | Epic 9 (already in-progress) gains Story 9.12; no other epic affected; no resequencing. |
| 3. Artifact Conflicts | [x] Done | PRD FR48; epics + coverage map; UX spec §5.1 + two §5.2 bullets. Architecture: N/A (pure UI). Secondary artifacts (CI, deployment, docs): N/A; no new i18n keys. |
| 4. Path Forward | [x] Done | Option 1 Direct Adjustment selected (effort Low, risk Low). Rollback not viable; MVP review not applicable. |
| 5. Proposal Components | [x] Done | Sections 1–5 above. |
| 6. Final Review & Handoff | [x] Done | Approved by Alexis 2026-06-12. Artifact edits applied: prd.md (FR48), epics.md (Story 9.12 + coverage map), ux-design-specification.md (§5.1 + two §5.2 bullets), sprint-status.yaml (9-12 backlog entry). |
