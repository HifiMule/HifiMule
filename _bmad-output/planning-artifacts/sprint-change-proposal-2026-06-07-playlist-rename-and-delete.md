# Sprint Change Proposal: Playlist Curation — Rename & Delete

**Date:** 2026-06-07
**Trigger:** Epic 11 post-implementation review — two UX gaps in the curation view header
**Proposed by:** Alexis
**Scope:** Minor — Developer agent direct implementation

---

## 1. Issue Summary

### Problem Statement

After completing Epic 11 (Stories 11.1–11.7), the playlist curation view header displays the playlist name as a read-only title. There is no way to rename a playlist directly from the curation view, nor any way to delete a playlist from the UI — even though the `playlist.delete` daemon RPC has been live since Story 11.4.

### Discovery Context

Identified by Alexis during post-implementation review of the completed Epic 11 on the `playlist-edit` branch.

### Root Cause

The original Epic 11 scope focused on track-level write operations (create, add, remove). Rename was not part of the original API surface — `rename_playlist` does not yet exist in the `MediaProvider` trait, either provider, or the daemon RPC layer. Delete was specced as a daemon RPC (11.4) but no UI surface was ever scoped to expose it.

---

## 2. Impact Analysis

### Epic Impact

| Epic | Impact |
|---|---|
| Epic 11 — Playlist Management & Curation | Reopen to `in-progress`; add Story 11.8. All other stories (11.1–11.7) unaffected. |
| All other epics | None. |

### Story Impact

| Story | Change |
|---|---|
| 11.8 (new, backlog) | Rename + Delete affordances in the curation view, including backend `rename_playlist` support. |

### Artifact Conflicts

| Artifact | Section | Change Required |
|---|---|---|
| `prd.md` | FR38 | Add rename and delete affordances to the curation view description |
| `ux-design-specification.md` | §5.2 Playlist Curation View | Add rename (inline edit of header name) and delete (button + confirm dialog) patterns |
| `epics.md` | After Story 11.7 | Add Story 11.8 with full ACs and Technical Notes |
| `sprint-status.yaml` | Epic 11 block | Set `epic-11: in-progress`; add `11-8-playlist-rename-and-delete: backlog` |

### Technical Impact

**Rename (new backend work):**
- `MediaProvider` trait: new `rename_playlist(id, new_name)` method
- `JellyfinProvider`: 2-step — `GET /Users/{uid}/Items/{id}` to fetch item JSON, then `POST /Items/{id}` with `Name` updated
- `SubsonicProvider`: single-step — `GET /rest/updatePlaylist.view?playlistId={id}&name={new_name}`
- Daemon RPC: new `playlist.rename({ playlistId, name })` handler

**Delete (UI-only):**
- `playlist.delete({ playlistId })` RPC from Story 11.4 already works — no backend changes

---

## 3. Recommended Approach

**Option 1 — Direct Adjustment ✅ Selected**

Add Story 11.8 to Epic 11. Implement in three layers:

1. **Backend (rename only):** Extend `MediaProvider` trait, implement in Jellyfin and Subsonic providers, wire up a new `playlist.rename` daemon RPC.
2. **Curation view — Rename:** The playlist name in the header becomes editable when clicked — renders an `<sl-input>` pre-filled with the current name, plus Save and Cancel affordances. On save, calls `playlist.rename`; updates the header title on success.
3. **Curation view — Delete:** A trash-icon button in the header (visible only when `supports_playlist_write` is true). Clicking it opens an `<sl-dialog>` for confirmation showing the playlist name. On confirm, calls `playlist.delete` and navigates back to the playlist browser; on cancel the dialog closes.

**Rationale:** Low effort. The rename backend is one new method across three files with well-established patterns from Stories 11.2/11.3. The delete UI is additive with no new RPCs needed. Both changes are isolated to the curation view header.

---

## 4. Detailed Change Proposals

### 4.1 `prd.md` — FR38

**OLD:**
```
- **FR38:** The system provides a dual-panel playlist curation view: artists in the playlist on the left, that artist's albums on the right, filtered to playlist contents. A track list panel below both panels shows individual tracks for the selected artist, optionally filtered by a focused album. Users can remove an artist, a specific album, or an individual track. The curation view provides an "Add tracks" affordance that opens a search dialog, allowing users to find and append individual tracks from the library to the playlist via `playlist.addTracks`. Individual tracks in any browse view also expose an "Add to playlist…" right-click context action — selecting an existing playlist calls `playlist.addTracks`; selecting "New playlist" calls `playlist.create`. A right-click context menu lets users send artists/albums to a playlist from browse views. The view displays playlist statistics (track count, total duration, total storage size). Edits update the server playlist.
```

**NEW:**
```
- **FR38:** The system provides a dual-panel playlist curation view: artists in the playlist on the left, that artist's albums on the right, filtered to playlist contents. A track list panel below both panels shows individual tracks for the selected artist, optionally filtered by a focused album. Users can remove an artist, a specific album, or an individual track. The curation view provides an "Add tracks" affordance that opens a search dialog, allowing users to find and append individual tracks from the library to the playlist via `playlist.addTracks`. Individual tracks in any browse view also expose an "Add to playlist…" right-click context action — selecting an existing playlist calls `playlist.addTracks`; selecting "New playlist" calls `playlist.create`. A right-click context menu lets users send artists/albums to a playlist from browse views. The view displays playlist statistics (track count, total duration, total storage size). The playlist name in the curation view header is editable inline; saving calls `playlist.rename`. A delete affordance in the header opens a confirmation dialog before calling `playlist.delete` and returning to the playlist browser. Edits update the server playlist.
```

---

### 4.2 `ux-design-specification.md` — §5.2 Playlist Curation View (addition)

ADD at the end of the Playlist Curation View bullet, before the closing sentence about edits being written in real time:

> The playlist name in the header is editable: clicking it replaces the title with an `<sl-input>` pre-filled with the current name, accompanied by Save (checkmark) and Cancel (×) icon-buttons. On save, `playlist.rename` is called and the header title updates to reflect the new name; on cancel or Escape, the input is dismissed with no change. A delete icon-button (trash) is shown in the header when `supports_playlist_write` is `true`; clicking it opens an `<sl-dialog>` confirming deletion of the named playlist. On confirm, `playlist.delete` is called and the UI navigates back to the playlist browser; on cancel the dialog closes.

---

### 4.3 `epics.md` — New Story 11.8

```
### Story 11.8: Playlist Rename and Delete — Curation View Header

As a Ritualist (Arthur),
I want to rename and delete a playlist directly from the curation view,
So that I can manage my library's playlist catalogue without leaving the edit context.

**Acceptance Criteria:**

**Given** the curation view is open for a playlist
**When** I click the playlist name in the header
**Then** the name becomes an inline `<sl-input>` pre-filled with the current name.
**And** Save and Cancel affordances appear alongside the input.

**Given** the inline name input is open
**When** I edit the name and click Save
**Then** `playlist.rename({ playlistId, name: newName })` is called.
**And** the header title updates to the new name.
**And** the input is dismissed.

**Given** the inline name input is open
**When** I press Escape or click Cancel
**Then** the input is dismissed with no RPC call.

**Given** the active provider supports playlist write
**When** the curation view renders
**Then** a delete icon-button (trash) is visible in the header.

**Given** I click the delete icon-button
**Then** an `<sl-dialog>` opens showing the playlist name and asking for confirmation.

**Given** the confirmation dialog is open and I confirm
**Then** `playlist.delete({ playlistId })` is called.
**And** the UI navigates back to the playlist browser.

**Given** the confirmation dialog is open and I cancel
**Then** the dialog closes with no RPC call.

**Given** the active provider does not support playlist write
**Then** the delete icon-button is hidden.

**Technical Notes:**
- `rename_playlist(id, new_name)` is a new method on the `MediaProvider` trait in `providers/mod.rs`.
- JellyfinProvider: 2-step — `GET /Users/{uid}/Items/{id}` to fetch current item JSON, update `Name`, then `POST /Items/{id}` with the full body.
- SubsonicProvider: single-step — `GET /rest/updatePlaylist.view?playlistId={id}&name={encoded_name}`.
- Daemon: new `playlist.rename({ playlistId, name })` RPC handler calling `provider.rename_playlist`.
- Frontend: editable name state in `PlaylistCurationView.ts`; delete affordance reuses the existing `sl-dialog` pattern from Story 11.5's "Save as playlist" flow.
- `playlist.delete` RPC from Story 11.4 is reused unchanged.
- New i18n keys: `playlist.curation.rename_save`, `playlist.curation.rename_cancel`, `playlist.curation.delete_title`, `playlist.curation.delete_body`, `playlist.curation.delete_confirm`, `playlist.curation.delete_cancel_btn`.
```

---

### 4.4 `sprint-status.yaml`

```yaml
  # epic-11: done  →  epic-11: in-progress
  # Add after 11-7-add-tracks-to-playlist-browse-and-curation: done
  11-8-playlist-rename-and-delete: backlog
```

---

## 5. Implementation Handoff

**Scope Classification:** Minor — Developer agent direct implementation.

**Files to change:**

| File | Change |
|---|---|
| `hifimule-daemon/src/providers/mod.rs` | Add `rename_playlist(id, new_name)` to `MediaProvider` trait |
| `hifimule-daemon/src/providers/jellyfin.rs` | Implement `rename_playlist`: GET item JSON → POST `/Items/{id}` with updated `Name` |
| `hifimule-daemon/src/providers/subsonic.rs` | Implement `rename_playlist`: `updatePlaylist.view?playlistId={id}&name={new_name}` |
| Daemon RPC playlist handler | Add `playlist.rename` handler |
| `hifimule-ui/src/components/PlaylistCurationView.ts` | Inline name editing + delete button + confirmation dialog |
| `hifimule-i18n/catalog.json` | 6 new keys × 3 languages |
| `_bmad-output/planning-artifacts/prd.md` | FR38 update |
| `_bmad-output/planning-artifacts/ux-design-specification.md` | §5.2 addition |
| `_bmad-output/planning-artifacts/epics.md` | Story 11.8 added |
| `_bmad-output/implementation-artifacts/sprint-status.yaml` | Epic 11 in-progress + 11.8 backlog |

**Success criteria:**
- Clicking the playlist name in the curation view header opens an inline `<sl-input>`; saving calls `playlist.rename` and updates the title
- Pressing Escape or Cancel dismisses the input with no RPC
- A trash icon-button is visible in the header when `supports_playlist_write` is true
- Clicking it opens a confirmation dialog with the playlist name; confirming calls `playlist.delete` and navigates back; cancelling closes the dialog
- Delete button is hidden when `supports_playlist_write` is false
- `rename_playlist` correctly updates the name on both Jellyfin (`POST /Items/{id}`) and Subsonic (`updatePlaylist.view?name=`)
- TypeScript compiles with zero errors (`rtk tsc`)

**Handoff to:** Developer agent — implement Story 11.8 (`11-8-playlist-rename-and-delete.md`).
