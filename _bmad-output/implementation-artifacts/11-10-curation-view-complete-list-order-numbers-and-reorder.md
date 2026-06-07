---
baseline_commit: dc52e27
---

# Story 11.10: Curation View — Complete Track List, Order Numbers & Reorder

Status: ready-for-dev

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a Ritualist (Arthur),
I want to see my whole playlist in order and nudge tracks up or down,
so that I can fine-tune track sequence directly in the editor.

## Acceptance Criteria

1. **Given** the curation view is open **When** the artist panel renders **Then** an "All artists" entry appears at the top; selecting it shows every playlist track in the track panel, **in playlist order**. **And** when a specific artist is selected, an "All albums" entry in the album panel lets that artist's tracks all show together (no album filter).

2. **Given** the track panel renders **Then** each track row shows its **absolute 1-based position** (#N) in the full playlist, regardless of any active artist/album filter.

3. **Given** the active provider supports playlist write **When** the track panel renders **Then** each row shows up (↑) and down (↓) controls; ↑ is disabled on the **first visible row** and ↓ on the **last visible row** of the current panel.

4. **Given** I click ↑ or ↓ on a track **Then** it swaps playlist position with the previous/next **currently-visible** track **And** the change is applied **optimistically** and persisted via `playlist.reorder` with the **full reordered track-id list** **And** the #N order numbers update to reflect the new sequence.

5. **Given** the reorder RPC fails **Then** an inline error is shown (`#curation-error`) **and** the prior order is restored (rollback to the pre-swap `this.tracks`).

6. **Given** the provider does not support playlist write **Then** the up/down controls are hidden (order numbers #N are **still shown**).

## Tasks / Subtasks

> **Scope: frontend-only.** This story touches exactly two files: [PlaylistCurationView.ts](hifimule-ui/src/components/PlaylistCurationView.ts) and [catalog.json](hifimule-i18n/catalog.json). **No daemon/Rust changes** — the `playlist.reorder` RPC already exists (Story 11.9, baseline `dc52e27`). **No new `rpc.ts` helper** — call `rpcCall('playlist.reorder', …)` directly (matches how `playlist.rename`/`playlist.delete`/`playlist.removeTracks` are already called in this file).

### Task 1: "All artists" selection state — make `selectedArtist = null` mean "All" (AC: #1)

**File:** [PlaylistCurationView.ts](hifimule-ui/src/components/PlaylistCurationView.ts)

The current `render()` (lines 132–135) **coerces** `null` → `artists[0]`:
```ts
const selectedArtist = this.selectedArtist && artistIndex.has(this.selectedArtist)
    ? this.selectedArtist
    : (artists[0] ?? null);
this.selectedArtist = selectedArtist;
```
Change the semantics so **`null` is a legitimate "All artists" state** (the default), and a *removed/invalid* artist falls back to `null` (All), not to `artists[0]`:
```ts
// null = "All artists". A previously-selected artist that no longer exists → fall back to All.
if (this.selectedArtist !== null && !artistIndex.has(this.selectedArtist)) {
    this.selectedArtist = null;
}
const selectedArtist = this.selectedArtist; // null = All
```
- Initial load now defaults to **All artists** (the constructor leaves `selectedArtist = null`, line 27 — keep it).
- Downstream code that does `selectedArtist!` (lines 254, 270) only runs inside the per-artist album branch — guard it on `selectedArtist !== null` (see Task 4 rendering).

### Task 2: `getTracksForPanel()` — All artists returns the full playlist in order (AC: #1, #2)

**File:** [PlaylistCurationView.ts](hifimule-ui/src/components/PlaylistCurationView.ts) (current method lines 89–96)

```ts
private getTracksForPanel(): BrowseTrack[] {
    if (this.selectedArtist === null) return this.tracks; // All artists → full playlist, playlist order
    return this.tracks.filter(track => {
        if ((track.artistName || 'Unknown Artist') !== this.selectedArtist) return false;
        if (this.selectedAlbum !== null && (track.albumName || 'Unknown Album') !== this.selectedAlbum) return false;
        return true;
    });
}
```
- `this.tracks` is the **server playlist order** as returned by `browse.getPlaylist` (`fetchBrowsePlaylist`, [rpc.ts:197](hifimule-ui/src/rpc.ts#L197)). `Array.prototype.filter` preserves order, so a specific artist's panel is also in playlist order. **Do not sort tracks** — order is the whole point of this story.

### Task 3: Absolute #N order numbers (AC: #2, #6)

**File:** [PlaylistCurationView.ts](hifimule-ui/src/components/PlaylistCurationView.ts) — inside `render()`, before building `panelTracks`’ HTML.

Precompute an id→absolute-index map once per render:
```ts
const positionById = new Map<string, number>();
this.tracks.forEach((track, i) => positionById.set(track.id, i));
```
In each track row (current template lines 289–305), prepend the position. `#N` = `positionById.get(track.id)! + 1`:
```ts
<span style="font-size: var(--sl-font-size-x-small); color: var(--sl-color-neutral-400); flex-shrink: 0; min-width: 2.5em; text-align: right;">
    #${(positionById.get(track.id) ?? 0) + 1}
</span>
```
- **#N is absolute** — it reflects the position in `this.tracks`, *not* the row's index within the filtered panel (AC2 explicitly: "regardless of any active artist/album filter").
- #N is shown in **all** cases — capability-gated or not (AC6: order numbers still shown when write unsupported).
- **Duplicate-id caveat:** if the same track id appears twice in a playlist, the map collapses to the last index. This is acceptable for #N display; the **swap logic (Task 5) must use panel position, not id**, to stay duplicate-safe.

### Task 4: "All albums" entry + ↑/↓ controls in the track panel (AC: #1, #3, #6)

**File:** [PlaylistCurationView.ts](hifimule-ui/src/components/PlaylistCurationView.ts)

**4a — "All albums" row** (album panel, current lines 250–277). When a **specific** artist is selected, prepend an "All albums" row above the album list. It is focused when `selectedAlbum === null` (the existing rows highlight on `album === this.selectedAlbum`). Clicking it sets `this.selectedAlbum = null` and re-renders. When `selectedArtist === null` (All artists), the album panel shows the existing `select_artist` hint / no album rows (do **not** render "All albums" with no artist context).
```ts
${selectedArtist === null
    ? `<p style="padding: 1rem; color: var(--sl-color-neutral-500);">${t('playlist.curation.select_artist')}</p>`
    : `<div class="curation-album-row curation-all-albums${this.selectedAlbum === null ? ' curation-album-focused' : ''}"
            style="/* same row styling; highlight when selectedAlbum === null */">
           <span style="flex:1; font-size: var(--sl-font-size-small); font-style: italic;">${t('playlist.curation.all_albums')}</span>
       </div>
       ${albums.map(album => /* existing album row markup */).join('')}`
}
```

**4b — ↑/↓ controls** on each track row, **only when `this.supportsPlaylistWrite`** (AC6). Compute the **panel index** for disabled-state and for duplicate-safe identification. Iterate `panelTracks` with index so each row carries `data-panel-index`:
```ts
${panelTracks.map((track, panelIdx) => `
    <div class="curation-track-row" data-panel-index="${panelIdx}" style="display: flex; align-items: center; padding: 0.35rem 0.75rem; gap: 0.5rem;">
        <span style="/* #N badge from Task 3 */">#${(positionById.get(track.id) ?? 0) + 1}</span>
        <span style="flex:1; /* title — existing */" title="${this.escapeAttr(track.title)}">${this.escapeHtml(track.title)}</span>
        <span style="/* duration — existing */">${formatDuration(track.duration ?? 0)}</span>
        ${this.supportsPlaylistWrite ? `
            <sl-icon-button class="curation-move-up" name="chevron-up"
                data-panel-index="${panelIdx}" label="${t('playlist.curation.move_up')}"
                ${panelIdx === 0 ? 'disabled' : ''} style="font-size: 0.9rem; flex-shrink: 0;"></sl-icon-button>
            <sl-icon-button class="curation-move-down" name="chevron-down"
                data-panel-index="${panelIdx}" label="${t('playlist.curation.move_down')}"
                ${panelIdx === panelTracks.length - 1 ? 'disabled' : ''} style="font-size: 0.9rem; flex-shrink: 0;"></sl-icon-button>
        ` : ''}
        <sl-icon-button class="curation-remove-track" name="x-circle" data-track-id="${this.escapeAttr(track.id)}"
            label="${t('playlist.curation.remove_track')}" style="font-size: 0.9rem; flex-shrink: 0;"></sl-icon-button>
    </div>
`).join('')}
```
- ↑ disabled on `panelIdx === 0`; ↓ disabled on `panelIdx === panelTracks.length - 1` (AC3 — *first/last **visible** row*).
- The remove-track button (existing, lines 297–303) stays. Move controls are siblings, gated on capability.

### Task 5: Reorder logic — swap visible neighbours, optimistic + persist + rollback (AC: #4, #5)

**File:** [PlaylistCurationView.ts](hifimule-ui/src/components/PlaylistCurationView.ts)

Add an `isReordering` guard field (next to `isRemoving`, line 30) and a move handler. **Operate on panel position, not track id** (duplicate-safe), and swap the corresponding entries in the **full `this.tracks`** array:

```ts
private isReordering = false;

private async moveTrack(panelIdx: number, direction: -1 | 1): Promise<void> {
    if (this.isReordering) return;
    const panel = this.getTracksForPanel();
    const neighbourPanelIdx = panelIdx + direction;
    if (panelIdx < 0 || panelIdx >= panel.length) return;
    if (neighbourPanelIdx < 0 || neighbourPanelIdx >= panel.length) return; // first/last visible row

    // Map panel rows → absolute positions in this.tracks via object reference (duplicate-id safe).
    const fullIdxA = this.tracks.indexOf(panel[panelIdx]);
    const fullIdxB = this.tracks.indexOf(panel[neighbourPanelIdx]);
    if (fullIdxA < 0 || fullIdxB < 0) return;

    this.isReordering = true;
    const previousOrder = this.tracks.slice();                 // snapshot for rollback
    const next = this.tracks.slice();
    [next[fullIdxA], next[fullIdxB]] = [next[fullIdxB], next[fullIdxA]];
    this.tracks = next;                                        // optimistic
    this.render();                                             // #N updates immediately

    let errorMsg: string | null = null;
    try {
        await rpcCall('playlist.reorder', {
            playlistId: this.playlistId,
            trackIds: this.tracks.map(track => track.id),       // FULL reordered id list
        });
    } catch (err) {
        errorMsg = err instanceof Error ? err.message : String(err);
        this.tracks = previousOrder;                           // rollback
        this.render();
    } finally {
        this.isReordering = false;
    }

    if (errorMsg !== null) {
        const errorEl = this.container.querySelector('#curation-error') as HTMLElement | null;
        if (errorEl) {
            errorEl.textContent = t('playlist.curation.reorder_error', { message: errorMsg });
            errorEl.style.display = '';
            (errorEl as any).open = true;
        }
    }
}
```
**Bind in `bindEvents()`** (alongside the existing `.curation-remove-track` binding, lines 446–453):
```ts
this.container.querySelectorAll<HTMLElement>('.curation-move-up').forEach(btn => {
    btn.addEventListener('click', async (e) => {
        e.stopPropagation();
        const idx = Number((btn as HTMLElement).dataset.panelIndex);
        if (!Number.isNaN(idx)) await this.moveTrack(idx, -1);
    });
});
this.container.querySelectorAll<HTMLElement>('.curation-move-down').forEach(btn => {
    btn.addEventListener('click', async (e) => {
        e.stopPropagation();
        const idx = Number((btn as HTMLElement).dataset.panelIndex);
        if (!Number.isNaN(idx)) await this.moveTrack(idx, 1);
    });
});
```
**Critical notes:**
- This mirrors the exact optimistic-update + `#curation-error` pattern in `doRemove` (lines 469–498): snapshot → mutate `this.tracks` → `render()` → on failure restore + show inline `#curation-error`. **Reuse, don't reinvent.**
- The `#curation-error` `<sl-alert>` already exists in the template (line 308) and is recreated on every `render()` (display:none) — so set its text/`open` **after** the final `render()`, exactly as `doRemove` does.
- "Swap with previous/next **currently-visible** track" (AC4): when a specific artist/album filter is active, the neighbour is the adjacent track *in the filtered panel*, which may be many positions away in the full playlist. `indexOf` on the panel's object references resolves both absolute positions correctly. The persisted `trackIds` is always the **entire** `this.tracks` in its new order (matches the 11.9 backend contract — it always receives the full live id list).
- `isReordering` blocks concurrent moves (rapid ↑/↑ clicks) just like `isRemoving` blocks concurrent removes.

### Task 6: i18n keys × en/fr/es (AC: #1, #3, #5)

**File:** [catalog.json](hifimule-i18n/catalog.json)

Add these keys into **each** of the three language blocks (`en`, `fr`, `es` — top-level keys), next to the other `playlist.curation.*` keys (en block starts ~line 173; fr/es blocks repeat the same flat keys):

| Key | en | fr | es |
|-----|----|----|----|
| `playlist.curation.all_artists` | `All artists` | `Tous les artistes` | `Todos los artistas` |
| `playlist.curation.all_albums` | `All albums` | `Tous les albums` | `Todos los álbumes` |
| `playlist.curation.move_up` | `Move up` | `Monter` | `Subir` |
| `playlist.curation.move_down` | `Move down` | `Descendre` | `Bajar` |
| `playlist.curation.reorder_error` | `Failed to reorder tracks: {message}` | `Échec de la réorganisation des pistes : {message}` | `Error al reordenar las pistas: {message}` |

- The epic tech note lists `all_artists`, `all_albums`, `move_up`, `move_down`. **`reorder_error` is added beyond that list** because AC5 requires a distinct inline error — reusing `playlist.curation.error` ("Failed to **remove** tracks…") would be misleading. Keep the `{message}` placeholder, consistent with `playlist.curation.error`/`add_tracks_error`/`rename_error`.
- After editing, validate the JSON is well-formed (`python3 -c "import json; json.load(open('hifimule-i18n/catalog.json'))"`).

### Task 7: Artist-panel "All artists" entry (AC: #1)

**File:** [PlaylistCurationView.ts](hifimule-ui/src/components/PlaylistCurationView.ts) — artist panel template (current lines 218–243).

Prepend an "All artists" row above the sorted artist list; highlight it when `selectedArtist === null`. Clicking sets `this.selectedArtist = null; this.selectedAlbum = null; this.render()`:
```ts
${this.tracks.length === 0
    ? `<p style="padding: 1rem; color: var(--sl-color-neutral-500);">${t('playlist.curation.no_artists')}</p>`
    : `<div class="curation-artist-row curation-all-artists${selectedArtist === null ? ' curation-selected' : ''}"
            style="/* same artist-row styling; highlight when selectedArtist === null */">
           <span style="flex:1; font-size: var(--sl-font-size-small); font-style: italic;">${t('playlist.curation.all_artists')}</span>
       </div>
       ${artists.map(artist => /* existing artist-row markup, unchanged */).join('')}`
}
```
**Bind** the All-artists click in `bindEvents()` (the existing `.curation-artist-row` handler at lines 399–408 reads `row.dataset.artist`; the All row has **no** `data-artist`, so add a dedicated handler):
```ts
this.container.querySelector('.curation-all-artists')?.addEventListener('click', () => {
    this.selectedArtist = null;
    this.selectedAlbum = null;
    this.render();
});
```
- The All row has **no** remove-artist button (it is not a real artist).
- Keep the existing per-artist rows and their remove buttons unchanged.
- The "All albums" row (Task 4a) needs an analogous click binding: `this.container.querySelector('.curation-all-albums')?.addEventListener('click', () => { this.selectedAlbum = null; this.render(); });`

### Task 8: Verify build & manual check (AC: all)

- `rtk tsc` (or the project's typecheck) — **zero new errors**. Note a pre-existing `TS5101 baseUrl deprecated` warning exists (per Story 11.8) — that is not new.
- **No `cargo` work** — backend untouched. Do not run/modify Rust.
- Manual smoke (if a dev environment is available): open a server playlist → curation view shows "All artists" selected by default with every track in order and #N badges; select an artist → "All albums" entry shows; ↑/↓ reorder a track and confirm #N updates and persists (re-open playlist); with a read-only provider, ↑/↓ are hidden but #N still shows.

## Dev Notes

### Scope boundary — frontend only

Story 11.10 is the **frontend half** of the playlist-reorder feature. Story 11.9 (baseline `dc52e27`, "Dev 11.9") already shipped the backend: the `reorder_playlist` trait method, Jellyfin (selection-sort via `Items/Move`) and Subsonic (`createPlaylist` set-order) adapters, and the **`playlist.reorder` RPC** (`{ playlistId: string, trackIds: string[] }` → `{ ok: true }`). **Do not touch any Rust file.** This story changes only:

| File | Change |
|------|--------|
| [PlaylistCurationView.ts](hifimule-ui/src/components/PlaylistCurationView.ts) | "All artists"/"All albums" entries, `null`=All semantics, `getTracksForPanel` All-mode, #N absolute order numbers, ↑/↓ controls + `moveTrack` (optimistic + `playlist.reorder` + rollback), `isReordering` guard |
| [catalog.json](hifimule-i18n/catalog.json) | 5 new `playlist.curation.*` keys × en/fr/es |

No new files. No `rpc.ts` change (call `rpcCall('playlist.reorder', …)` directly). No `library.ts` change — `supportsPlaylistWrite` is already threaded into the constructor (Story 11.8).

### The reorder RPC contract (from Story 11.9 — already implemented)

```
playlist.reorder  →  params { playlistId: string, trackIds: string[] }  →  returns { ok: true }
```
- `trackIds` MUST be the **full** track-id list in the desired final order (same set, new sequence). The backend sets the playlist to exactly that order. Sending the entire `this.tracks.map(t => t.id)` after the optimistic swap satisfies this.
- On the incapable-provider path the daemon returns `ERR_UNSUPPORTED_CAPABILITY` — but the UI **hides** ↑/↓ when `supportsPlaylistWrite` is false (AC6), so a well-behaved client never calls reorder on a read-only provider. The error rollback (AC5) is for genuine server/network failures.
- Backend edge behavior to rely on: an already-correct order issues zero work; a single-track or empty playlist is a no-op. The selection-sort (Jellyfin) and `createPlaylist` set-order (Subsonic) preserve the track set.

### Existing code being modified — current state & what must be preserved

**`PlaylistCurationView` ([PlaylistCurationView.ts](hifimule-ui/src/components/PlaylistCurationView.ts)) — current behavior:**
- `this.tracks: BrowseTrack[]` is loaded once via `fetchBrowsePlaylist` (`browse.getPlaylist`) in playlist order and is the single source of truth. `add`/`remove` flows mutate it optimistically and re-render. Reorder follows the **same** mutate-then-render model.
- `render()` (lines 129–322) rebuilds the entire DOM and re-binds events every call — there is **no incremental DOM update**. After a swap, `render()` recomputes artists (alphabetical), albums (alphabetical), and `panelTracks` (playlist order). **Preserve** this full-render model; do not introduce surgical DOM mutation.
- `selectedArtist` currently auto-coerces to the first artist (lines 132–135). Task 1 changes this to allow `null` = All. **Preserve** the removed-artist fallback — but it now falls back to `null` (All), not `artists[0]`.
- `getTracksForPanel()` (lines 89–96) returns `[]` when no artist; Task 2 changes the `null` case to return the full list. **Preserve** the artist+album filter for the specific-artist case.
- `doRemove()` (lines 469–498) is the **canonical optimistic-update + `#curation-error`** template: snapshot → mutate `this.tracks` → `render()` → on failure restore + reveal the `#curation-error` `<sl-alert>` (line 308). `moveTrack` must mirror it exactly, including setting the alert **after** the final render. **Preserve** the `isRemoving`/`isReordering` re-entrancy guards.
- The header (rename/delete, Story 11.8), the stats bar + "Add tracks" dialog (Story 11.7), and remove-artist/album/track flows (Story 11.6) must keep working unchanged. Reorder only **adds** controls to track rows and **adds** the All-artists/All-albums entries.

**`escapeHtml`/`escapeAttr` (lines 682–695):** continue using `escapeAttr` for any HTML attribute value (e.g. `title`, `data-*` carrying user text) and `escapeHtml` for text content. The new All-artists/All-albums rows use static i18n labels (no escaping needed); track titles already use `escapeHtml`/`escapeAttr`.

### #N vs panel index — the one subtle correctness point

There are **two** distinct indices in this story:
- **#N (display):** absolute 1-based position in the *full* `this.tracks` — from the `positionById` map (Task 3). Shown on every row regardless of filter.
- **panel index (logic):** the row's position within the *filtered* `panelTracks` — used for ↑/↓ enable/disable and to pick the visible neighbour to swap with (Task 5). Carried on the row as `data-panel-index`.

Do **not** conflate them. The swap operates on visible neighbours (panel index) but persists the full-list order (#N changes as a consequence). Using **object-reference `indexOf`** on `panelTracks` entries to find their absolute positions keeps the swap correct even if duplicate track ids exist (the `positionById` map alone would be ambiguous for duplicates — that is why the swap uses references, not the map).

### Testing standards

- This codebase keeps Rust tests in `#[cfg(test)]` modules; the **frontend has no unit-test harness** for components (verify via `tsc` typecheck + manual smoke). No new test framework should be introduced for this story.
- The 11.x review cycle runs **Blind Hunter + Edge Case Hunter + Acceptance Auditor**. Pre-empt edge-case findings by handling: empty playlist (no All-artists row needed / 0 tracks), single-track playlist (both ↑ and ↓ disabled), first/last visible row (↑/↓ disabled), reorder while a filter is active (swap visible neighbours, persist full list), reorder RPC failure (rollback + inline error), rapid double-clicks (`isReordering` guard), and read-only provider (controls hidden, #N still shown).

### Previous story intelligence

- **Story 11.9 (`dc52e27`):** delivered the backend `playlist.reorder`. Its dev notes confirm the contract this story consumes — frontend always sends the **full** ordered id list; a track-id absent from the server playlist surfaces an error (treated as a real desync). Our optimistic model keeps `this.tracks` authoritative, so the sent list always matches what the user sees.
- **Story 11.8 (rename & delete):** established the header rename/delete UI and threaded `supportsPlaylistWrite` through the `PlaylistCurationView` constructor and the `library.ts` call site — **already wired**, reuse it for the AC6 capability gate. 11.8 also confirmed the i18n workflow: add keys to **all three** language blocks in `catalog.json` and validate JSON.
- **Story 11.7 (add tracks):** the "Add tracks" dialog re-fetches the playlist and re-renders; the stats header is in `renderStats()`. Reorder does **not** need a re-fetch — it mutates `this.tracks` locally and persists; the server order is authoritative only on the next full `load()`.
- **Story 11.6 (dual-panel curation):** the artist/album/track panel structure, the alphabetical artist/album sort, and `getTracksForPanel()` filtering all originate here. Reorder preserves that structure and adds the "All" entries + order controls.

### Git intelligence

Recent commits (`dc52e27 Dev 11.9`, `161a1d3 Story 11.9`, `7a63bb5 Review 11.8`) confirm the Story → Dev → Review rhythm on the `playlist-edit` branch. 11.9 touched only the daemon (`providers/mod.rs`, `api.rs`, `providers/jellyfin.rs`, `providers/subsonic.rs`, `rpc.rs`). 11.10 touches only the UI (`PlaylistCurationView.ts`, `catalog.json`) — **disjoint file sets**, so no merge interaction between the two halves. Baseline for this story: `dc52e27` (11.9 must be merged/done before 11.10 reorder calls resolve at runtime).

### Project Structure Notes

- Frontend lives in `hifimule-ui/src/`; the curation component is `components/PlaylistCurationView.ts`. RPC plumbing is `rpc.ts` (`rpcCall` → Tauri `rpc_proxy`). i18n catalog is the standalone `hifimule-i18n/catalog.json` aliased as `@hifimule/i18n-catalog` ([tsconfig.json:13](hifimule-ui/tsconfig.json#L13), [vite.config.ts:34](hifimule-ui/vite.config.ts#L34)); `t(key, replacements)` ([i18n.ts:26](hifimule-ui/src/i18n.ts#L26)) does `{placeholder}` substitution and falls back to `en` then the raw key.
- No structural conflicts; the change is additive within an existing component and the existing catalog.

### References

- Epic 11.10 definition: [epics.md:2450](_bmad-output/planning-artifacts/epics.md#L2450)
- FR40 (playlist reordering, Stories 11.9–11.10): [epics.md:116](_bmad-output/planning-artifacts/epics.md#L116)
- Previous story (reorder backend — the RPC this story calls): [11-9-playlist-reorder-provider-and-rpc.md](_bmad-output/implementation-artifacts/11-9-playlist-reorder-provider-and-rpc.md)
- Curation view to modify: [PlaylistCurationView.ts](hifimule-ui/src/components/PlaylistCurationView.ts) — `render()` lines 129–322, `getTracksForPanel()` 89–96, `doRemove()` 469–498 (optimistic-update template), `bindEvents()` 324–454, `#curation-error` alert line 308
- RPC entry point: `rpcCall` [rpc.ts:75](hifimule-ui/src/rpc.ts#L75); `BrowseTrack` type [rpc.ts:121](hifimule-ui/src/rpc.ts#L121); `fetchBrowsePlaylist` [rpc.ts:197](hifimule-ui/src/rpc.ts#L197)
- i18n: catalog [catalog.json](hifimule-i18n/catalog.json) (existing `playlist.curation.*` keys ~line 173 en); `t()` [i18n.ts:26](hifimule-ui/src/i18n.ts#L26)
- Capability threading precedent: [11-8-playlist-rename-and-delete.md](_bmad-output/implementation-artifacts/11-8-playlist-rename-and-delete.md)

## Dev Agent Record

### Agent Model Used

{{agent_model_name_version}}

### Debug Log References

### Completion Notes List

### File List

## Change Log

- 2026-06-07: Story 11.10 created — curation-view complete-list reorder UI: "All artists"/"All albums" entries, absolute #N order numbers, ↑/↓ move controls (capability-gated), optimistic swap persisted via `playlist.reorder` with full-list rollback on failure. Frontend-only (`PlaylistCurationView.ts` + `catalog.json`); consumes Story 11.9's backend RPC. Status → ready-for-dev.
