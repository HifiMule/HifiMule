# Sprint Change Proposal: Playlist Curation — Individual Track List & Removal

**Date:** 2026-06-07
**Trigger:** Story 11.6 — Dual-Panel Playlist Curation View (done)
**Proposed by:** Alexis
**Scope:** Minor — direct implementation by Developer agent

---

## 1. Issue Summary

### Problem Statement

Story 11.6 was implemented with a dual-panel curation view: artists on the left, albums for the selected artist on the right. While this allows removing entire artists or albums, there is no way to view or remove individual tracks. For fine-grained playlist editing, users need a track-level panel to see exactly which songs are in the playlist and remove them one by one.

### Discovery Context

Identified by Alexis during post-implementation review of the completed Story 11.6 on the `playlist-edit` branch.

### Root Cause

The original AC for Story 11.6 specified artist and album removal as the granularity targets. The track-level requirement emerged after seeing the implemented view in context — individual track management is a natural next step for a playlist curation tool.

---

## 2. Impact Analysis

### Epic Impact

| Epic | Impact |
|---|---|
| Epic 11 — Playlist Management & Curation | Story 11.6 must be reopened; all other stories unaffected. |
| All other epics | None. |

### Story Impact

| Story | Change |
|---|---|
| 11.6 (done → in-progress) | Add ACs 7–9 for track panel, album focus state, and individual track removal. |

### Artifact Conflicts

| Artifact | Section | Change Required |
|---|---|---|
| `prd.md` | FR38 | Add individual track removal to capability description |
| `ux-design-specification.md` | §5.2 Playlist Curation View | Add track panel and album-focus interaction to description |
| `epics.md` | Story 11.6 | Add AC 7, 8, 9 |
| `11-6-dual-panel-playlist-curation-view-and-stats.md` | Status, Tasks | Reopen story; add Tasks 7–9 |

### Technical Impact

Pure frontend change. No daemon RPCs, no Rust changes. `playlist.removeTracks` already accepts any array of track IDs — removing a single track is a one-element call with no protocol changes.

---

## 3. Recommended Approach

**Option 1 — Direct Adjustment ✅ Selected**

Reopen Story 11.6. Extend `PlaylistCurationView.ts` with:
- A `selectedAlbum` state field
- Album row click-to-focus (highlights album, filters track panel)
- A third panel below artist/album panels showing tracks for the selected artist (optionally filtered by focused album)
- Per-track remove buttons calling `doRemove([track.id])`

**Rationale:** Low effort, low risk. All required RPCs exist. Existing panel rendering and `doRemove` logic is reused unchanged. The layout change is additive (a third section below the existing panels).

---

## 4. Detailed Change Proposals

### 4.1 Proposed Layout

```
┌──────────────────────────────────────────────────────┐
│  ← [Playlist Name]          Stats: N tracks · Xh Ym  │
├──────────────────────┬───────────────────────────────┤
│  Artists             │  Albums (artist filtered)      │
│  • Artist A  [×]     │  • Album 1  [×]               │
│  • Artist B  [×]     │  ▶ Album 2  [×]  ← focused    │
├──────────────────────┴───────────────────────────────┤
│  Tracks  (for selected artist · focused album)        │
│  Song Title 1          3:21                     [×]   │
│  Song Title 2          4:05                     [×]   │
└──────────────────────────────────────────────────────┘
```

### 4.2 `prd.md` — FR38

OLD:
```
- **FR38:** The system provides a dual-panel playlist curation view: artists in the playlist on the left, that artist's albums on the right, filtered to playlist contents. Users can remove an artist or specific albums. A right-click context menu lets users send artists/albums to a playlist from browse views. The view displays playlist statistics (track count, total duration, total storage size). Edits update the server playlist.
```

NEW:
```
- **FR38:** The system provides a dual-panel playlist curation view: artists in the playlist on the left, that artist's albums on the right, filtered to playlist contents. A track list panel below both panels shows individual tracks for the selected artist, optionally filtered by a focused album. Users can remove an artist, a specific album, or an individual track. A right-click context menu lets users send artists/albums to a playlist from browse views. The view displays playlist statistics (track count, total duration, total storage size). Edits update the server playlist.
```

---

### 4.3 `ux-design-specification.md` — §5.2 Playlist Curation View

OLD:
> The left panel lists all artists who have tracks in the selected playlist; selecting an artist shows that artist's albums filtered to playlist-only contents in the right panel. Remove-artist affordance removes all tracks by that artist from the playlist via `playlist.removeTracks`; the artist then disappears from the left panel. Remove-album affordance removes that album's tracks; if no tracks remain for the artist, they also disappear from the left panel. A statistics header displays total track count, total duration, and total storage size (tracks without `sizeBytes` are excluded from the size total without error). Edits are written to the server playlist in real time; closing the view leaves the playlist in its final edited state.

NEW:
> The left panel lists all artists who have tracks in the selected playlist; selecting an artist shows that artist's albums filtered to playlist-only contents in the right panel. Clicking an album row (not the remove button) focuses it and filters the track panel to that album's tracks. A track panel below both panels shows individual tracks for the selected artist, optionally filtered by the focused album; each track row has a Remove-track affordance. Remove-artist removes all tracks by that artist; Remove-album removes all tracks in that album; Remove-track removes a single track — all via `playlist.removeTracks`. Artists and albums disappear from their panels when they have no remaining tracks in the playlist. A statistics header displays total track count, total duration, and total storage size (tracks without `sizeBytes` are excluded from the size total without error). Edits are written to the server playlist in real time; closing the view leaves the playlist in its final edited state.

---

### 4.4 `epics.md` — Story 11.6 — New Acceptance Criteria

ADD after existing AC 6 ("Given I close the curation view…"):

```
**Given** an artist is selected in the left panel
**When** the curation view renders or updates
**Then** a track panel below the artist/album panels shows all tracks by that artist that are in the playlist.
**And** each track row shows the track title, duration, and a "Remove track" button.

**Given** I click on an album row in the right panel (not the remove button)
**Then** the album is highlighted as focused.
**And** the track panel filters to show only tracks from that album that are in the playlist.

**Given** I click "Remove track" on a track in the track panel
**Then** that single track is removed from the playlist via `playlist.removeTracks`.
**And** the track disappears from the track panel.
**And** if the artist has no remaining tracks in the playlist, the artist disappears from the left panel.
**And** the statistics header updates.
```

---

### 4.5 `11-6-dual-panel-playlist-curation-view-and-stats.md` — Revised Tasks

**Status:** `done` → `in-progress`

**Task 7: Add i18n keys for track removal (AC: 7–9)**

In `hifimule-i18n/catalog.json`, add to `"en"`, `"fr"`, and `"es"` blocks (after existing `playlist.curation.*` keys):
```json
"playlist.curation.remove_track": "Remove track",
"playlist.curation.no_tracks": "No tracks for this selection"
```
2 keys × 3 languages = 6 additions.

**Task 8: Add `selectedAlbum` state and album focus interaction (AC: 8)**

In `PlaylistCurationView.ts`:
- Add `private selectedAlbum: string | null = null`
- In `render()`, album rows get a click handler on the row body (not the remove button): `this.selectedAlbum = album; this.render();`
- Album rows get a highlighted state when `album === this.selectedAlbum` (same left-border accent + background-tint pattern as artist rows)
- In `removeAlbum()`: after removal, if `this.selectedAlbum === albumName`, reset `this.selectedAlbum = null` before calling `this.render()`

**Task 9: Add track panel (AC: 7, 9)**

In `PlaylistCurationView.ts`:

Add helper:
```typescript
private getTracksForPanel(): BrowseTrack[] {
    if (!this.selectedArtist) return [];
    return this.tracks.filter(t => {
        const artist = t.artistName || 'Unknown Artist';
        const album = t.albumName || 'Unknown Album';
        if (artist !== this.selectedArtist) return false;
        if (this.selectedAlbum !== null && album !== this.selectedAlbum) return false;
        return true;
    });
}
```

In `render()`, after the `curation-panels` div, add:
```typescript
<div id="curation-track-panel" style="
    border-top: 1px solid var(--sl-color-neutral-200);
    overflow-y: auto;
    max-height: 40%;
    padding: 0.5rem 0;
">
    ${panelTracks.length === 0
        ? `<p style="padding: 1rem; color: var(--sl-color-neutral-500);">${t('playlist.curation.no_tracks')}</p>`
        : panelTracks.map(track => `
            <div class="curation-track-row"
                 style="display: flex; align-items: center; padding: 0.35rem 0.75rem; gap: 0.5rem;">
                <span style="flex: 1; font-size: var(--sl-font-size-small); overflow: hidden; text-overflow: ellipsis; white-space: nowrap;"
                      title="${escapeAttr(track.title)}">${escapeHtml(track.title)}</span>
                <span style="font-size: var(--sl-font-size-x-small); color: var(--sl-color-neutral-500); flex-shrink: 0;">
                    ${formatDuration(track.duration ?? 0)}
                </span>
                <sl-icon-button
                    class="curation-remove-track"
                    name="x-circle"
                    data-track-id="${escapeAttr(track.id)}"
                    label="${t('playlist.curation.remove_track')}"
                    style="font-size: 0.9rem; flex-shrink: 0;"
                ></sl-icon-button>
            </div>
        `).join('')
    }
</div>
```

In `bindEvents()`, add listener:
```typescript
this.container.querySelectorAll<HTMLElement>('.curation-remove-track').forEach(btn => {
    btn.addEventListener('click', async () => {
        const trackId = (btn as any).dataset?.trackId
            ?? btn.closest('[data-track-id]')?.getAttribute('data-track-id');
        if (trackId) await this.doRemove([trackId]);
    });
});
```

**Key notes:**
- `doRemove` already filters `this.tracks` and calls `this.render()` — no changes needed there.
- After track removal via `doRemove`, `getTracksForPanel()` will naturally return the updated list.
- `selectedAlbum` is reset by `removeAlbum` when the focused album is removed; `selectedArtist` reset by `removeArtist` is already handled in the existing code.
- The track panel `max-height: 40%` means the artist/album panels keep visible height even with long track lists.

---

## 5. Implementation Handoff

**Scope Classification:** Minor — Developer agent direct implementation.

**Files to change:**

| File | Change |
|---|---|
| `hifimule-i18n/catalog.json` | 2 new keys × 3 languages |
| `hifimule-ui/src/components/PlaylistCurationView.ts` | `selectedAlbum` state, album focus, track panel, per-track removal |
| `_bmad-output/planning-artifacts/prd.md` | FR38 |
| `_bmad-output/planning-artifacts/ux-design-specification.md` | §5.2 |
| `_bmad-output/planning-artifacts/epics.md` | Story 11.6 ACs 7–9 |
| `_bmad-output/implementation-artifacts/11-6-dual-panel-playlist-curation-view-and-stats.md` | Reopen + Tasks 7–9 |

**Success criteria:**
- Selecting an artist shows its tracks in the track panel below the artist/album panels
- Clicking an album row (not ×) highlights it and filters the track panel to that album's tracks
- Clicking × on a track removes it; stats update; artist/album disappear from their panels if they have no remaining tracks
- Artist/album × removal continues to work correctly; track panel reflects the changes
- `selectedAlbum` resets when the focused album is removed via Remove-album
- TypeScript compiles with zero errors (`rtk tsc`)

**Handoff to:** Developer agent — implement Tasks 7–9 in `PlaylistCurationView.ts` + i18n + artifact doc updates.
