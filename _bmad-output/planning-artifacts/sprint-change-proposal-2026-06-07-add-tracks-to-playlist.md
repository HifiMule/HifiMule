# Sprint Change Proposal: Epic 11 — Add Tracks to Playlist (Browse & Curation)

**Date:** 2026-06-07
**Trigger:** Epic 11 post-implementation review — gap between specced `playlist.addTracks` RPC and missing UI surfaces
**Proposed by:** Alexis
**Scope:** Minor — direct implementation by Developer agent

---

## 1. Issue Summary

### Problem Statement

Epic 11 (Stories 11.1–11.6) implemented playlist write-back end-to-end: the `MediaProvider` trait, Jellyfin/Subsonic adapters, daemon RPCs, basket "Save as playlist", and the dual-panel curation view. However, the `playlist.addTracks` RPC (Story 11.4) has no UI path that calls it. Users can only remove tracks from playlists (curation view) or create new playlists from their basket or from artist/album context menus. There is no way to add individual tracks to an existing playlist from any surface.

### Discovery Context

Identified by Alexis after reviewing all completed Epic 11 stories on the `playlist-edit` branch. The gap became apparent when considering the natural user flow: curating a playlist by adding tracks one-by-one, or right-clicking a track in a browse view and sending it to a playlist.

### Root Cause

The original Epic 11 scope focused on "save basket as playlist" and "remove from playlist" workflows. The `addTracks` RPC was defined as a building block for future use, but no story was scoped to expose it in the UI.

---

## 2. Impact Analysis

### Epic Impact

| Epic | Impact |
|---|---|
| **Epic 11 — Selection-as-Playlist & Curation** | Add new Story 11.7; epic status reopened to `in-progress`. All existing stories (11.1–11.6) unaffected. |
| All other epics | None. |

### Story Impact

| Story | Change |
|---|---|
| 11.7 (new, backlog) | New story covering track-level context menu in browse views + "Add tracks" in curation view. |

### Artifact Conflicts

| Artifact | Section | Change Required |
|---|---|---|
| `prd.md` | FR38 | Add track-add capability description alongside existing track-remove language |
| `ux-design-specification.md` | §5.2 Playlist Curation View | Add "Add tracks" button + search dialog spec |
| `ux-design-specification.md` | §5.2 Context Menu | Extend to cover individual track rows |
| `epics.md` | After Story 11.6 | Add Story 11.7 with full ACs and Technical Notes |
| `sprint-status.yaml` | Epic 11 block | Add `11-7-add-tracks-to-playlist-browse-and-curation: backlog`; set `epic-11: in-progress` |

### Technical Impact

Pure frontend change. No daemon RPCs, no Rust changes, no provider changes needed:
- `playlist.addTracks` is already specced in Story 11.4 and implemented
- Jellyfin and Subsonic adapters already implement `add_to_playlist`
- The curation view's `fetchPlaylist()` → `render()` cycle already handles re-render after mutations

---

## 3. Recommended Approach

**Option 1 — Direct Adjustment ✅ Selected**

Add Story 11.7 to Epic 11. Implement two surfaces in the frontend:

1. **Track-row context menu:** Right-click on any track row in browse views → "Add to playlist…" → pick existing (calls `playlist.addTracks`) or create new (calls `playlist.create`). Capability-gated on `supports_playlist_write`.

2. **Curation view "Add tracks" button:** An "Add tracks" button in the statistics header → search dialog (title/artist/album query) → multi-select results → calls `playlist.addTracks` → view re-fetches and re-renders.

**Rationale:** Low effort, low risk. All backend work is complete. The two surfaces are independent and can be implemented sequentially within a single story. No epic restructuring needed.

---

## 4. Detailed Change Proposals

### 4.1 `prd.md` — FR38

**OLD:**
```
- **FR38:** The system provides a dual-panel playlist curation view: artists in the playlist on the left, that
  artist's albums on the right, filtered to playlist contents. A track list panel below both panels shows
  individual tracks for the selected artist, optionally filtered by a focused album. Users can remove an artist,
  a specific album, or an individual track. A right-click context menu lets users send artists/albums to a
  playlist from browse views. The view displays playlist statistics (track count, total duration, total storage
  size). Edits update the server playlist.
```

**NEW:**
```
- **FR38:** The system provides a dual-panel playlist curation view: artists in the playlist on the left, that
  artist's albums on the right, filtered to playlist contents. A track list panel below both panels shows
  individual tracks for the selected artist, optionally filtered by a focused album. Users can remove an artist,
  a specific album, or an individual track. The curation view provides an "Add tracks" affordance that opens a
  search dialog, allowing users to find and append individual tracks from the library to the playlist via
  `playlist.addTracks`. Individual tracks in any browse view also expose an "Add to playlist…" right-click
  context action — selecting an existing playlist calls `playlist.addTracks`; selecting "New playlist" calls
  `playlist.create`. A right-click context menu lets users send artists/albums to a playlist from browse views.
  The view displays playlist statistics (track count, total duration, total storage size). Edits update the
  server playlist.
```

---

### 4.2 `ux-design-specification.md` — §5.2 Playlist Curation View

**OLD:**
> A dedicated view for editing server playlists. [...] Remove-artist removes all tracks by that artist; Remove-album removes all tracks in that album; Remove-track removes a single track — all via `playlist.removeTracks`. Artists and albums disappear from their panels when they have no remaining tracks in the playlist. A statistics header displays total track count, total duration, and total storage size [...] Edits are written to the server playlist in real time; closing the view leaves the playlist in its final edited state.

**NEW (key addition):**
> The statistics header includes an "Add tracks" button (shown only when `supports_playlist_write` is `true`); clicking it opens a search dialog that queries the library for tracks by title, artist, or album, and allows the user to select one or more results to append to the playlist via `playlist.addTracks`. After adding, the curation view re-fetches the playlist and re-renders.

---

### 4.3 `ux-design-specification.md` — §5.2 Context Menu

**OLD:**
> A right-click context menu that appears on artist and album cards in browse views and in the curation view. Primary action: "Send to playlist…" — opens a sub-menu or dialog to create a new playlist seeded with that item, or send it to an existing managed playlist. Available only when `supports_playlist_write` is `true`. Remove actions are shown in the curation view context.

**NEW:**
> A right-click context menu that appears on artist cards, album cards, and individual track rows in browse views and in the curation view. On artist/album cards: primary action is "Send to playlist…" — opens a sub-menu or dialog to create a new playlist seeded with that item, or add it to an existing managed playlist. On individual track rows: primary action is "Add to playlist…" — opens a sub-menu or dialog to pick an existing playlist (calls `playlist.addTracks`) or create a new one (calls `playlist.create`). All playlist write actions are available only when `supports_playlist_write` is `true`. Remove actions are shown in the curation view context.

---

### 4.4 `epics.md` — New Story 11.7

Added after Story 11.6. Full ACs and Technical Notes in `epics.md`.

**Story summary:**
- AC 1–5: Track-level "Add to playlist…" context menu — pick existing playlist or create new
- AC 6–9: Curation view "Add tracks" button — search dialog, multi-select, `playlist.addTracks`, re-render
- Technical Notes: no daemon/provider work; reuses existing RPC, capability gate, and render cycle

---

## 5. Implementation Handoff

**Scope Classification:** Minor — Developer agent direct implementation.

**Files to change:**

| File | Change |
|---|---|
| `hifimule-ui/src/...` (track context menu) | Add "Add to playlist…" context action on track rows; fetch playlist list; call `playlist.addTracks` or `playlist.create` |
| `hifimule-ui/src/components/PlaylistCurationView.ts` | Add "Add tracks" button to stats header; implement search dialog; call `playlist.addTracks`; re-fetch + re-render |
| `hifimule-i18n/catalog.json` | New i18n keys: `playlist.context.add_to_playlist`, `playlist.curation.add_tracks`, `playlist.curation.add_tracks_dialog_placeholder`, `playlist.curation.add_tracks_confirm`, `playlist.context.new_playlist` |
| `_bmad-output/planning-artifacts/prd.md` | FR38 updated ✅ |
| `_bmad-output/planning-artifacts/ux-design-specification.md` | §5.2 Curation View + Context Menu updated ✅ |
| `_bmad-output/planning-artifacts/epics.md` | Story 11.7 added ✅ |
| `_bmad-output/implementation-artifacts/sprint-status.yaml` | Epic 11 in-progress + 11.7 backlog ✅ |

**Success criteria:**
- Right-clicking a track row in any browse view shows "Add to playlist…" when `supports_playlist_write` is true
- Picking an existing playlist from the dialog appends the track via `playlist.addTracks`; a success notification appears
- Picking "New playlist…" prompts for a name and calls `playlist.create`
- "Add to playlist…" is hidden when `supports_playlist_write` is false
- The curation view statistics header shows an "Add tracks" button
- Typing a query in the search dialog returns matching tracks from the library
- Selecting tracks and confirming calls `playlist.addTracks`; the view re-renders with the new tracks visible
- Cancelling the dialog makes no RPC call
- TypeScript compiles with zero errors (`rtk tsc`)

**Handoff to:** Developer agent — implement Story 11.7 (`11-7-add-tracks-to-playlist-browse-and-curation.md`).
