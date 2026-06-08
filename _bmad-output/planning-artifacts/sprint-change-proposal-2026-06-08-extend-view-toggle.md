# Sprint Change Proposal: Extend Grid/Table Toggle to All Browse Modes and Drill-Down Levels

**Date:** 2026-06-08
**Author:** Alexis
**Scope:** Minor
**Handoff:** Developer agent

---

## 1. Issue Summary

After completing Story 9.7 (Virtualized List/Table Browse Views), the grid/table toggle was observed to work well on the Artists and Albums root views. The decision was made to extend the toggle to all navigation tabs (Playlists, Genres, Recently Added, Frequently Played, Recently Played, Favorites) and to sub-levels (albums within an artist, tracks within an album).

Story 9.7 AC #8 had explicitly restricted the toggle to Artists/Albums root only. This restriction was a scoping decision for the first implementation pass, not a permanent design intent. FR39 in the PRD also narrowly described the feature as applying to "Artist and Album browse pages" only.

The user preference is a **single global toggle** — one grid/list state shared across all modes and depths.

---

## 2. Impact Analysis

### Epic Impact
- **Epic 9 (Rich Library Navigation):** Story 9.7 remains done. New **Story 9.8** added to define the extension.
- **No other epics affected.**

### Story Impact
- **Story 9.7:** Task 7 manual verification item "toggle is hidden when drilled into artist albums" annotated as superseded. Technical note about per-mode view state annotated as superseded.
- **Story 9.8:** New story added to Epic 9.

### Artifact Conflicts Resolved
| Artifact | Section | Change |
|---|---|---|
| PRD | FR39 | Broadened from "Artist and Album browse pages" to all modes and drill-down levels |
| UX Design Spec | §5.1 List/Table Browse View | Scope expanded; global toggle specified; autoload clarified as artists/albums-only optimization |
| Story 9.7 impl. artifact | Task 7 verification | Superseded item annotated |
| Epics | Story 9.7 Technical Notes | Per-mode note annotated as superseded; Story 9.8 added |

### Technical Impact
- Pure UI rendering concern; no daemon RPCs, no new basket types, no state structure change
- `state.listViewMode` is already a single global value — no refactor needed
- Code delta: two guard removals in `library.ts`

---

## 3. Recommended Approach

**Direct Adjustment** — add Story 9.8, update three artifact sections.

- **Effort:** Low
- **Risk:** Low
- **Timeline impact:** None (existing stories unaffected; Story 9.8 is a new, self-contained story)
- **Rationale:** The virtualized renderer already handles any `BrowseDisplayItem` regardless of mode or depth. The two guard removals unlock the behavior immediately. The `loadMoreForListView` autoload path is already suppressed for non-artists/albums contexts via the `rootMode` variable — no additional logic required.

---

## 4. Detailed Change Proposals

### PRD FR39

**OLD:**
> FR39: The system can present Artist and Album browse pages as virtualized list/table views (in addition to paginated album-art grids), enabling rapid scanning across thousands of items without pagination.

**NEW:**
> FR39: The system can present any browse page or drill-down level as a virtualized list/table view (in addition to the paginated album-art grid), enabling rapid scanning of artists, albums, playlists, genres, history, and favorites — and sub-levels such as albums within an artist or tracks within an album. A single global grid/list toggle in the browse-mode bar applies uniformly across all browse modes and navigation depths.

---

### UX Design Spec §5.1

**OLD:**
> A virtualized (windowed) list/table rendering mode for Artist and Album browse pages... The view-mode toggle (grid vs list) is stored in local UI state per browse mode.

**NEW:**
> A virtualized (windowed) list/table rendering mode available on all browse pages and drill-down levels... The view-mode toggle (grid vs list) is a **single global UI state value** that applies uniformly across all browse modes and navigation depths. Autoload-on-scroll applies only to Artists and Albums root; other modes and sub-levels render what is already loaded.

---

### Story 9.7 Superseded Items

- Task 7 verification: "toggle is hidden when drilled into artist albums" → annotated as superseded by Story 9.8
- Technical note: "View mode is stored per browse mode" → annotated as superseded by Story 9.8

---

### New Story 9.8

See `_bmad-output/implementation-artifacts/9-8-extend-view-toggle-all-modes.md`

---

## 5. Implementation Handoff

**Scope classification:** Minor — direct implementation by Developer agent.

**Developer agent deliverables:**
1. In `hifimule-ui/src/library.ts`:
   - Remove the `(state.browseMode === 'artists' || state.browseMode === 'albums') && state.breadcrumbStack.length === 0` guard from `renderViewToggle()` (lines ~593–596)
   - Remove the matching guard from `renderCurrentView()` (lines ~823–826)
2. Verify `rtk tsc` — zero new TypeScript errors
3. Manual smoke test: toggle grid/list on Playlists, Genres, a history mode, and while drilled into artist albums

**Success criteria:**
- View toggle visible on all browse tabs at all drill-down levels
- Toggling to list view works on Playlists, Genres, Recently Added, Frequently Played, Recently Played, Favorites
- Toggling to list view works when drilled into albums-within-artist and tracks-within-album
- Switching browse mode or navigating in/out of sub-levels preserves the toggle state
- No autoload triggered on modes/levels that previously had none
