---
baseline_commit: 968209c
---

# Story 11.6: Dual-Panel Playlist Curation View

Status: done

## Story

As a Ritualist (Arthur),
I want a dual-panel view for curating server playlists,
so that I can remove specific artists or albums from a playlist without rebuilding it from scratch.

## Acceptance Criteria

1. **Given** a server playlist is selected for curation **When** I open the curation view **Then** the left panel shows all artists who have tracks in the playlist **And** selecting an artist shows that artist's albums filtered to only those with tracks in the playlist in the right panel.

2. **Given** I click "Remove artist" in the left panel **Then** all tracks by that artist are removed from the playlist via `playlist.removeTracks` **And** the artist disappears from the left panel.

3. **Given** I click "Remove album" in the right panel **Then** all tracks in that album are removed from the playlist via `playlist.removeTracks` **And** the album disappears from the right panel **And** if that artist has no remaining tracks in the playlist the artist also disappears from the left panel.

4. **Given** the curation view is open **Then** a statistics header shows total track count, total duration, and total storage size.

5. **Given** some tracks in the playlist have no `sizeBytes` value **When** the storage size statistic is displayed **Then** those tracks are excluded from the size total (the stat shows 0 B when all sizes are null).

6. **Given** I close the curation view **Then** the server playlist reflects all removals made during the session **And** the playlist list view is restored.

7. **Given** an artist is selected in the left panel **When** the curation view renders or updates **Then** a track panel below the artist/album panels shows all tracks by that artist that are in the playlist **And** each track row shows the track title, duration, and a "Remove track" button.

8. **Given** I click on an album row in the right panel (not the remove button) **Then** the album is highlighted as focused **And** the track panel filters to show only tracks from that album that are in the playlist **And** clicking the focused album again restores the full artist track list.

9. **Given** I click "Remove track" on a track in the track panel **Then** that single track is removed from the playlist via `playlist.removeTracks` **And** the track disappears from the track panel **And** if the artist has no remaining tracks in the playlist the artist also disappears from the left panel **And** the statistics header updates.

## Tasks / Subtasks

- [x] Task 1: Add i18n keys (AC: 1–6)
  - [x] In `hifimule-i18n/catalog.json`, add to the `"en"` block (after the `library.context.*` keys from Story 11.5, around line 108):

    ```json
    "playlist.curation.curate_btn": "Curate",
    "playlist.curation.remove_artist": "Remove artist",
    "playlist.curation.remove_album": "Remove album",
    "playlist.curation.no_artists": "Playlist is empty",
    "playlist.curation.select_artist": "Select an artist to view albums",
    "playlist.curation.error": "Failed to remove tracks: {message}",
    "playlist.curation.close": "Close"
    ```

  - [x] Add the same 7 keys to the `"fr"` and `"es"` blocks (same English values are acceptable — existing pattern).

    **Key notes:**
    - 7 keys × 3 languages = 21 additions total.
    - Maintain valid JSON — no trailing commas on the last key in each language object.

- [x] Task 2: Extend `MediaCard.create()` with optional `onCurate` parameter (AC: 1)
  - [x] Update the `MediaCard.create()` signature in `hifimule-ui/src/components/MediaCard.ts` to add an optional 7th parameter after `supportsPlaylistWrite`:

    ```typescript
    public static create(
        item: JellyfinItem | JellyfinView | BrowseDisplayItem,
        mode: 'libraries' | 'items',
        isSynced: boolean,
        onNavigate: () => void | Promise<void>,
        deviceSelectionEnabled?: boolean,
        supportsPlaylistWrite?: boolean,
        onCurate?: (id: string, name: string) => void,   // NEW
    ): HTMLElement {
    ```

  - [x] Inside `MediaCard.create()`, after the existing `contextmenu` binding block (the one added in Story 11.5 for artist/album cards), add the curate button binding:

    ```typescript
    // Curate button: appears on Playlist cards when playlist write is supported
    if (onCurate) {
        const itemType = 'Type' in item ? (item as JellyfinItem).Type : (item as BrowseDisplayItem).type;
        if (itemType === 'Playlist') {
            const curateBtn = document.createElement('sl-icon-button') as any;
            curateBtn.name = 'pencil-square';
            curateBtn.label = t('playlist.curation.curate_btn');
            curateBtn.style.cssText = 'font-size: 1rem; margin-left: auto;';
            curateBtn.addEventListener('click', (e: MouseEvent) => {
                e.stopPropagation();
                onCurate(itemId, itemName);
            });
            card.appendChild(curateBtn);
        }
    }
    ```

    **Key notes:**
    - `e.stopPropagation()` is **critical** — prevents the click from bubbling to the card's own `click` handler which would call `onNavigate` and navigate into the playlist track view.
    - `itemId` and `itemName` are already extracted earlier in `MediaCard.create()` from the union type — do not re-extract.
    - The `t()` i18n function is already imported at the top of `MediaCard.ts`.
    - `card.appendChild(curateBtn)` appends after the existing basket toggle button — Shoelace card footer renders children in DOM order.
    - `BrowseDisplayItem.type` can be `'Playlist'` (see `mapPlaylists()` in library.ts:227). `JellyfinItem.Type` uses the same string. The guard ensures the button only appears on playlist cards.

- [x] Task 3: Thread `onCurate` through `renderGrid()` and `loadPlaylists()` (AC: 1)
  - [x] Update `renderGrid()` signature in `hifimule-ui/src/library.ts` (line 508) to accept an optional second parameter:

    ```typescript
    function renderGrid(items: BrowseDisplayItem[], onCurate?: (id: string, name: string) => void) {
    ```

  - [x] Inside `renderGrid()`, update the `MediaCard.create()` call (line 528) to pass `onCurate`:

    ```typescript
    const card = MediaCard.create(
        item, 'items', false,
        () => navigateToBrowseItem(item),
        selEnabled,
        _supportsPlaylistWrite,
        onCurate,    // NEW — undefined for all non-playlist modes
    );
    ```

  - [x] In `loadPlaylists()` (line 1099 and 1118), update both `renderGrid()` calls to pass the curate callback when playlist write is supported and at root depth (not inside a playlist):

    ```typescript
    // Both renderGrid calls in loadPlaylists() (cached path and fresh path):
    const onCurate = _supportsPlaylistWrite ? openCurationView : undefined;
    renderGrid(state.items, onCurate);
    ```

    Replace the bare `renderGrid(state.items);` calls at lines 1099 and 1118 with:
    ```typescript
    renderGrid(state.items, _supportsPlaylistWrite ? openCurationView : undefined);
    ```

    **Key notes:**
    - `openCurationView` is the function added in Task 5 — declare it before `loadPlaylists()` (hoisting doesn't apply to `const`/`let` functions; use `function openCurationView(...)` declaration which is hoisted, or place it before `loadPlaylists()` in the file).
    - All other callers of `renderGrid()` omit the second argument (default `undefined`) — no existing call sites change.
    - The curate button only appears when `_supportsPlaylistWrite` is true at the moment `loadPlaylists()` runs. This is correct: the module variable reflects the current daemon state.

- [x] Task 4: Implement `PlaylistCurationView` component (AC: 1–6)
  - [x] Create new file `hifimule-ui/src/components/PlaylistCurationView.ts`:

    ```typescript
    import { fetchBrowsePlaylist, BrowseTrack } from '../rpc';
    import { rpcCall } from '../rpc';
    import { t } from '../i18n';

    function formatDuration(totalSecs: number): string {
        const h = Math.floor(totalSecs / 3600);
        const m = Math.floor((totalSecs % 3600) / 60);
        const s = totalSecs % 60;
        if (h > 0) return `${h}h ${m}m`;
        if (m > 0) return `${m}m ${s}s`;
        return `${s}s`;
    }

    function formatBytes(bytes: number): string {
        if (bytes <= 0) return '0 B';
        if (bytes < 1024) return `${bytes} B`;
        if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
        if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
        return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
    }

    export class PlaylistCurationView {
        private container: HTMLElement;
        private playlistId: string;
        private playlistName: string;
        private tracks: BrowseTrack[] = [];
        private selectedArtist: string | null = null;
        private onClose: () => void;

        constructor(
            container: HTMLElement,
            playlistId: string,
            playlistName: string,
            onClose: () => void,
        ) {
            this.container = container;
            this.playlistId = playlistId;
            this.playlistName = playlistName;
            this.onClose = onClose;
        }

        async load(): Promise<void> {
            this.container.innerHTML = '<div style="padding: 2rem; text-align: center;"><sl-spinner></sl-spinner></div>';
            try {
                const result = await fetchBrowsePlaylist(this.playlistId);
                this.tracks = result.tracks;
                this.render();
            } catch (e) {
                const msg = e instanceof Error ? e.message : String(e);
                this.container.innerHTML = `<sl-alert variant="danger" open style="margin: 1rem;">${msg}</sl-alert>`;
            }
        }

        private buildArtistIndex(): Map<string, Set<string>> {
            // Returns Map<artistName, Set<albumName>>
            const index = new Map<string, Set<string>>();
            for (const track of this.tracks) {
                const artist = track.artistName || 'Unknown Artist';
                const album = track.albumName || 'Unknown Album';
                if (!index.has(artist)) index.set(artist, new Set());
                index.get(artist)!.add(album);
            }
            return index;
        }

        private getTrackIdsByArtist(artistName: string): string[] {
            return this.tracks
                .filter(t => (t.artistName || 'Unknown Artist') === artistName)
                .map(t => t.id);
        }

        private getTrackIdsByAlbum(artistName: string, albumName: string): string[] {
            return this.tracks
                .filter(t =>
                    (t.artistName || 'Unknown Artist') === artistName &&
                    (t.albumName || 'Unknown Album') === albumName
                )
                .map(t => t.id);
        }

        private renderStats(): string {
            const count = this.tracks.length;
            const totalSecs = this.tracks.reduce((s, t) => s + (t.duration ?? 0), 0);
            const totalBytes = this.tracks.reduce((s, t) => s + (t.sizeBytes ?? 0), 0);
            return `
                <div class="curation-stats" style="
                    padding: 0.5rem 1rem;
                    background: var(--sl-color-neutral-50);
                    border-bottom: 1px solid var(--sl-color-neutral-200);
                    font-size: var(--sl-font-size-small);
                    color: var(--sl-color-neutral-600);
                    display: flex;
                    gap: 1.5rem;
                ">
                    <span>${count} track${count === 1 ? '' : 's'}</span>
                    <span>${formatDuration(totalSecs)}</span>
                    <span>${formatBytes(totalBytes)}</span>
                </div>
            `;
        }

        private render(): void {
            const artistIndex = this.buildArtistIndex();
            const artists = Array.from(artistIndex.keys()).sort((a, b) => a.localeCompare(b));
            const selectedArtist = this.selectedArtist && artistIndex.has(this.selectedArtist)
                ? this.selectedArtist
                : (artists[0] ?? null);
            this.selectedArtist = selectedArtist;

            const albums = selectedArtist
                ? Array.from(artistIndex.get(selectedArtist)!).sort((a, b) => a.localeCompare(b))
                : [];

            this.container.innerHTML = `
                <div class="curation-view" style="display: flex; flex-direction: column; height: 100%; overflow: hidden;">
                    <div class="curation-header" style="
                        display: flex;
                        align-items: center;
                        gap: 0.75rem;
                        padding: 0.75rem 1rem;
                        border-bottom: 1px solid var(--sl-color-neutral-200);
                    ">
                        <sl-icon-button
                            id="curation-close-btn"
                            name="arrow-left"
                            label="${t('playlist.curation.close')}"
                            style="font-size: 1.1rem;"
                        ></sl-icon-button>
                        <span style="font-weight: var(--sl-font-weight-semibold); font-size: var(--sl-font-size-medium);">${this.escapeHtml(this.playlistName)}</span>
                    </div>
                    ${this.renderStats()}
                    <div class="curation-panels" style="
                        display: flex;
                        flex: 1;
                        overflow: hidden;
                        min-height: 0;
                    ">
                        <div id="curation-artist-panel" style="
                            width: 40%;
                            border-right: 1px solid var(--sl-color-neutral-200);
                            overflow-y: auto;
                            padding: 0.5rem 0;
                        ">
                            ${artists.length === 0
                                ? `<p style="padding: 1rem; color: var(--sl-color-neutral-500);">${t('playlist.curation.no_artists')}</p>`
                                : artists.map(artist => `
                                    <div class="curation-artist-row${artist === selectedArtist ? ' curation-selected' : ''}"
                                         data-artist="${this.escapeAttr(artist)}"
                                         style="
                                            display: flex;
                                            align-items: center;
                                            padding: 0.5rem 0.75rem;
                                            cursor: pointer;
                                            background: ${artist === selectedArtist ? 'var(--sl-color-primary-50)' : 'transparent'};
                                            border-left: 3px solid ${artist === selectedArtist ? 'var(--sl-color-primary-600)' : 'transparent'};
                                            gap: 0.5rem;
                                         ">
                                        <span style="flex: 1; font-size: var(--sl-font-size-small); overflow: hidden; text-overflow: ellipsis; white-space: nowrap;"
                                              title="${this.escapeAttr(artist)}">${this.escapeHtml(artist)}</span>
                                        <sl-icon-button
                                            class="curation-remove-artist"
                                            name="x-circle"
                                            data-artist="${this.escapeAttr(artist)}"
                                            label="${t('playlist.curation.remove_artist')}"
                                            style="font-size: 0.9rem; flex-shrink: 0;"
                                        ></sl-icon-button>
                                    </div>
                                `).join('')
                            }
                        </div>
                        <div id="curation-album-panel" style="
                            flex: 1;
                            overflow-y: auto;
                            padding: 0.5rem 0;
                        ">
                            ${!selectedArtist
                                ? `<p style="padding: 1rem; color: var(--sl-color-neutral-500);">${t('playlist.curation.select_artist')}</p>`
                                : albums.map(album => `
                                    <div class="curation-album-row"
                                         data-artist="${this.escapeAttr(selectedArtist!)}"
                                         data-album="${this.escapeAttr(album)}"
                                         style="
                                            display: flex;
                                            align-items: center;
                                            padding: 0.5rem 0.75rem;
                                            gap: 0.5rem;
                                         ">
                                        <span style="flex: 1; font-size: var(--sl-font-size-small); overflow: hidden; text-overflow: ellipsis; white-space: nowrap;"
                                              title="${this.escapeAttr(album)}">${this.escapeHtml(album)}</span>
                                        <sl-icon-button
                                            class="curation-remove-album"
                                            name="x-circle"
                                            data-artist="${this.escapeAttr(selectedArtist!)}"
                                            data-album="${this.escapeAttr(album)}"
                                            label="${t('playlist.curation.remove_album')}"
                                            style="font-size: 0.9rem; flex-shrink: 0;"
                                        ></sl-icon-button>
                                    </div>
                                `).join('')
                            }
                        </div>
                    </div>
                    <sl-alert id="curation-error" variant="danger" closable style="display:none; margin: 0.5rem;"></sl-alert>
                </div>
            `;

            this.bindEvents();
        }

        private bindEvents(): void {
            // Close button
            this.container.querySelector('#curation-close-btn')?.addEventListener('click', () => {
                this.onClose();
            });

            // Artist row click — select artist, re-render album panel only
            this.container.querySelectorAll<HTMLElement>('.curation-artist-row').forEach(row => {
                row.addEventListener('click', (e) => {
                    // Don't select if clicking the remove button
                    if ((e.target as HTMLElement).closest('.curation-remove-artist')) return;
                    const artist = row.dataset.artist;
                    if (artist) {
                        this.selectedArtist = artist;
                        this.render();
                    }
                });
            });

            // Remove artist buttons
            this.container.querySelectorAll<HTMLElement>('.curation-remove-artist').forEach(btn => {
                btn.addEventListener('click', async (e) => {
                    e.stopPropagation();
                    const artist = (btn as any).dataset?.artist ?? btn.closest('[data-artist]')?.getAttribute('data-artist');
                    if (artist) await this.removeArtist(artist);
                });
            });

            // Remove album buttons
            this.container.querySelectorAll<HTMLElement>('.curation-remove-album').forEach(btn => {
                btn.addEventListener('click', async () => {
                    const artist = (btn as any).dataset?.artist ?? btn.closest('[data-artist]')?.getAttribute('data-artist');
                    const album = (btn as any).dataset?.album ?? btn.closest('[data-album]')?.getAttribute('data-album');
                    if (artist && album) await this.removeAlbum(artist, album);
                });
            });
        }

        private async removeArtist(artistName: string): Promise<void> {
            const trackIds = this.getTrackIdsByArtist(artistName);
            if (trackIds.length === 0) return;
            await this.doRemove(trackIds);
            // If the removed artist was selected, clear selection so album panel shows empty
            if (this.selectedArtist === artistName) this.selectedArtist = null;
        }

        private async removeAlbum(artistName: string, albumName: string): Promise<void> {
            const trackIds = this.getTrackIdsByAlbum(artistName, albumName);
            if (trackIds.length === 0) return;
            await this.doRemove(trackIds);
        }

        private async doRemove(trackIds: string[]): Promise<void> {
            const errorEl = this.container.querySelector('#curation-error') as HTMLElement | null;
            if (errorEl) errorEl.style.display = 'none';

            // Optimistic local update first — removes from local state
            const removedSet = new Set(trackIds);
            this.tracks = this.tracks.filter(t => !removedSet.has(t.id));

            try {
                await rpcCall('playlist.removeTracks', {
                    playlistId: this.playlistId,
                    trackIds,
                });
            } catch (err) {
                const msg = err instanceof Error ? err.message : String(err);
                if (errorEl) {
                    errorEl.textContent = t('playlist.curation.error', { message: msg });
                    errorEl.style.display = '';
                    (errorEl as any).open = true;
                }
                // Note: local state is already updated; on error the track is gone from the view
                // but the server still has it. This is a best-effort UI — the user can re-open
                // the view to get a fresh fetch.
            }

            this.render();
        }

        private escapeHtml(s: string): string {
            return s
                .replace(/&/g, '&amp;')
                .replace(/</g, '&lt;')
                .replace(/>/g, '&gt;');
        }

        private escapeAttr(s: string): string {
            return s
                .replace(/&/g, '&amp;')
                .replace(/"/g, '&quot;')
                .replace(/</g, '&lt;')
                .replace(/>/g, '&gt;');
        }
    }
    ```

    **Key notes:**
    - **Optimistic update**: `this.tracks` is updated locally *before* the RPC call returns. This makes the UI feel instant. On RPC failure, the local state is already modified — the user sees the item gone, gets an error message, and can re-open the view to restore from server truth. This matches the "best-effort" nature of a curation tool.
    - **`escapeAttr`** is separate from `escapeHtml` because attribute values require `"` to be escaped (`&quot;`) while inner HTML does not — Story 11.5 review patch applied this same lesson.
    - `sl-icon-button` `data-artist` and `data-album` attributes: The button element is an HTMLElement but the `data-*` attributes can be read via `(btn as any).dataset` or `btn.closest('[data-artist]')`. Use `(btn as any).dataset?.artist` for directness, with `btn.closest(...)` as fallback.
    - `BrowseTrack.artistName` is typed `string` in rpc.ts but the underlying Rust field is `Option<String>` — guard with `|| 'Unknown Artist'` to be safe.
    - `BrowseTrack.albumName` (aliased as `albumName` from `album_title`) is similarly guarded.
    - `t.duration` is seconds — the Rust `Song.duration_seconds` is renamed to `duration` on serialization.
    - `t.sizeBytes` will always be `null` currently (Rust `Song` struct has no `size_bytes` field) — `formatBytes(0)` returns `'0 B'`, which is correct behavior per AC 5.
    - The `onClose` callback invalidates the cached playlist data and restores the playlist list — implemented in Task 5.

- [x] Task 5: Add `openCurationView()` to library.ts and wire cache invalidation (AC: 6)
  - [x] In `hifimule-ui/src/library.ts`, add an import for `PlaylistCurationView` at the top of the file (after existing component imports):

    ```typescript
    import { PlaylistCurationView } from './components/PlaylistCurationView';
    ```

  - [x] Add the `openCurationView` function declaration in library.ts, placed **before** `loadPlaylists()` (so it is in scope when `loadPlaylists` references it):

    ```typescript
    function openCurationView(playlistId: string, playlistName: string): void {
        const container = document.getElementById('library-content');
        if (!container) return;

        saveScroll();

        const view = new PlaylistCurationView(
            container,
            playlistId,
            playlistName,
            () => {
                // On close: invalidate this playlist's track cache and the playlists list cache
                for (const key of Array.from(state.pageCache.keys())) {
                    if (key.includes(playlistId) || key.startsWith('playlists:')) {
                        state.pageCache.delete(key);
                    }
                }
                // Restore the playlist list view
                loadPlaylists();
            },
        );

        view.load();
    }
    ```

    **Key notes:**
    - `saveScroll()` is already defined in library.ts — call it before replacing the container contents so scroll position is saved for when we return.
    - The `onClose` callback deletes two cache key families: the specific playlist's track cache (key contains `playlistId`) and the root playlists list cache (key starts with `'playlists:'`). The playlists list doesn't change during curation, but the specific playlist's track cache is now stale since tracks were removed. Deleting both ensures the next open of this playlist shows fresh data.
    - `loadPlaylists()` is hoisted (it's a `function` declaration), so calling it from within `openCurationView` is safe even though `openCurationView` is defined first.
    - No breadcrumb manipulation is needed: the curation view is not a navigation level — it's a modal-like overlay within the library content div. Returning to the playlist list via `loadPlaylists()` resets the breadcrumb stack to empty naturally (the function unconditionally fetches and renders the root playlist grid).
    - The `PlaylistCurationView` replaces `container.innerHTML` on `load()` — the existing playlist grid is overwritten. This is the same pattern used by all `loadX()` functions via `renderGrid()`.

- [x] Task 6: Verify compilation (AC: all)
  - [x] Run `rtk cargo check` — no new Rust errors (no Rust files changed in this story; verify zero delta).
  - [x] Run `rtk tsc` — zero TypeScript errors. Common pitfalls to check:
    - `PlaylistCurationView` constructor and `load()` method types
    - `onCurate` optional parameter threading through `renderGrid()` → `MediaCard.create()`
    - `(btn as any).dataset?.artist` — verify `any` cast compiles cleanly under strict mode
  - [x] Manual verification checklist (in Tauri dev app):
    - Connect to a Jellyfin or Subsonic server.
    - Navigate to the Playlists browse mode. Confirm each playlist card shows a "Curate" pencil icon button.
    - Click the "Curate" button on a playlist that has tracks from multiple artists. Confirm the curation view opens with the artist panel on the left and the first artist's albums on the right.
    - Confirm the stats header shows track count and duration (storage size shows 0 B — expected).
    - Click a different artist in the left panel. Confirm the right panel updates to show only that artist's albums in the playlist.
    - Click "Remove album". Confirm: the album disappears from the right panel; if the artist has no other albums in the playlist, the artist also disappears from the left panel; the stats update.
    - Click "Remove artist". Confirm: the artist and all their albums disappear; stats update.
    - Click "Close" (back arrow). Confirm the playlist list view is restored.
    - Re-open the curated playlist. Confirm the removed tracks are gone (server reflects changes — requires re-fetching via `browse.getPlaylist`).
    - Click the playlist card body (not the curate button). Confirm it still navigates to the flat track view as before.

- [x] Task 7: Add i18n keys for track removal (AC: 7–9)
  - [ ] In `hifimule-i18n/catalog.json`, add to the `"en"`, `"fr"`, and `"es"` blocks (after existing `playlist.curation.*` keys):

    ```json
    "playlist.curation.remove_track": "Remove track",
    "playlist.curation.no_tracks": "No tracks for this selection"
    ```

    **Key notes:**
    - 2 keys × 3 languages = 6 additions total.
    - Maintain valid JSON — no trailing commas on the last key in each language object.

- [x] Task 8: Add `selectedAlbum` state and album focus interaction (AC: 8)
  - [ ] In `PlaylistCurationView.ts`, add `private selectedAlbum: string | null = null` field.
  - [ ] In `render()`, album rows get a click handler on the row body (separate from the remove button):

    ```typescript
    row.addEventListener('click', (e) => {
        if ((e.target as HTMLElement).closest('.curation-remove-album')) return;
        const album = row.dataset.album;
        if (album) {
            this.selectedAlbum = album === this.selectedAlbum ? null : album;
            this.render();
        }
    });
    ```

    Toggle off on second click (so clicking the focused album again shows all artist tracks).

  - [ ] Album rows get a highlighted state when `album === this.selectedAlbum` — use same left-border accent + background-tint pattern already used for artist rows:

    ```typescript
    background: ${album === this.selectedAlbum ? 'var(--sl-color-primary-50)' : 'transparent'};
    border-left: 3px solid ${album === this.selectedAlbum ? 'var(--sl-color-primary-600)' : 'transparent'};
    ```

  - [ ] In `removeAlbum()`, after filtering `this.tracks` and before calling `this.render()`, reset `selectedAlbum` if the removed album was focused:

    ```typescript
    if (this.selectedAlbum === albumName) this.selectedAlbum = null;
    ```

- [x] Task 9: Add track panel below artist/album panels (AC: 7, 9)
  - [ ] Add `private getTracksForPanel(): BrowseTrack[]` helper to `PlaylistCurationView.ts`:

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

  - [ ] In `render()`, compute `const panelTracks = this.getTracksForPanel();` before building the HTML.
  - [ ] Add the track panel div after the closing `</div>` of `curation-panels` (inside `curation-view`):

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
                          title="${this.escapeAttr(track.title)}">${this.escapeHtml(track.title)}</span>
                    <span style="font-size: var(--sl-font-size-x-small); color: var(--sl-color-neutral-500); flex-shrink: 0;">
                        ${formatDuration(track.duration ?? 0)}
                    </span>
                    <sl-icon-button
                        class="curation-remove-track"
                        name="x-circle"
                        data-track-id="${this.escapeAttr(track.id)}"
                        label="${t('playlist.curation.remove_track')}"
                        style="font-size: 0.9rem; flex-shrink: 0;"
                    ></sl-icon-button>
                </div>
            `).join('')
        }
    </div>
    ```

  - [ ] In `bindEvents()`, add listener for track removal:

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
    - `doRemove` already handles single-element arrays — no changes to `doRemove` itself.
    - `doRemove` calls `render()` after removal, so `getTracksForPanel()` will re-run with the updated `this.tracks`.
    - `max-height: 40%` keeps the artist/album panels visible even with long track lists.
    - `track.title` is the display name — use `escapeHtml`/`escapeAttr` consistently.
    - Album click-to-focus uses toggle semantics (click focused album again → show all artist tracks). This avoids needing a separate "show all" affordance.

### Review Findings

- [x] [Review][Patch] Error alert never shown — `doRemove` calls `render()` unconditionally after the catch block, replacing `#curation-error` with a freshly-rendered hidden state; the error message is never visible to the user [`hifimule-ui/src/components/PlaylistCurationView.ts`]
- [x] [Review][Patch] Cache key substring collision — `key.includes(playlistId)` in the `openCurationView` close callback may evict unrelated cache entries when playlist IDs are short or share substrings with other browse cache keys (e.g. Subsonic numeric IDs) [`hifimule-ui/src/library.ts`]
- [x] [Review][Defer] `basketStore` event listener leak compounded by curation close pattern [`hifimule-ui/src/components/MediaCard.ts`] — deferred, pre-existing
- [x] [Review][Patch] Concurrent `doRemove` race condition — no in-flight guard; rapid clicks on multiple remove buttons cause interleaved optimistic updates and overlapping RPC calls with potentially duplicated/stale trackId sets [`hifimule-ui/src/components/PlaylistCurationView.ts`]
- [x] [Review][Patch] Dead `selectedArtist = null` in `removeArtist` — the null-assignment after `doRemove` never executes because `render()` (called inside `doRemove`) already reassigned `this.selectedArtist` to `artists[0]`; the comment "clear selection so album panel shows empty" is misleading dead code [`hifimule-ui/src/components/PlaylistCurationView.ts:315`]
- [x] [Review][Patch] Orphaned list-scroll handler in `openCurationView` — mounting the curation view replaces `#library-content` without calling `teardownListScrollHandler()`; if the user navigated from artists/albums in list mode, the stale scroll handler remains attached and fires on curation-view scroll events [`hifimule-ui/src/library.ts:1095`]
- [x] [Review][Patch] `formatDuration` silently drops seconds when hours > 0 — `${h}h ${m}m` omits `s`; a 1h 0m 45s track displays as "1h 0m", causing the stats duration to be systematically under-reported [`hifimule-ui/src/components/PlaylistCurationView.ts:8`]
- [x] [Review][Patch] Filter parameter `t` shadows module-level i18n `t()` — `.filter(t => ...)` and `.reduce((s, t) => ...)` callbacks in `getTrackIdsByArtist`, `getTrackIdsByAlbum`, `getTracksForPanel`, and `renderStats` shadow the imported `t` function; calling `t('key')` inside any of those lambdas would invoke the BrowseTrack object as a function and throw a TypeError [`hifimule-ui/src/components/PlaylistCurationView.ts`]
- [x] [Review][Patch] Missing `e.stopPropagation()` on remove-track button — all other remove buttons (artist, album) call `e.stopPropagation()`; the track remove handler does not, inconsistent with the defensive event pattern used throughout `bindEvents` [`hifimule-ui/src/components/PlaylistCurationView.ts:302`]
- [x] [Review][Defer] `t()` i18n return values interpolated directly into `innerHTML` template strings without `escapeHtml` — pre-existing pattern across the codebase; systemic fix required, not scoped to this story [`hifimule-ui/src/components/PlaylistCurationView.ts`] — deferred, pre-existing
- [x] [Review][Defer] Empty playlist shows simultaneous "Playlist is empty" (artist panel) and "No tracks for this selection" (track panel) empty-state messages — no spec coverage for this edge case [`hifimule-ui/src/components/PlaylistCurationView.ts`] — deferred, pre-existing
- [x] [Review][Defer] `listViewMode` simplified from per-mode `Map<BrowseMode, 'grid' | 'list'>` to a single global value — switching to list mode in artists now affects albums too; part of approved sprint-change-proposal-2026-06-07 (autoload-on-scroll) [`hifimule-ui/src/library.ts`] — deferred, pre-existing

## Dev Notes

### UI-only story — no Rust changes

This story touches **4 files**, all in the UI/i18n packages:

| File | Change |
|------|--------|
| `hifimule-i18n/catalog.json` | 7 new i18n keys across 3 language blocks |
| `hifimule-ui/src/components/MediaCard.ts` | Add optional `onCurate` 7th param; render curate button on Playlist cards |
| `hifimule-ui/src/library.ts` | `renderGrid()` accepts `onCurate`; `loadPlaylists()` passes it; add `openCurationView()` |
| `hifimule-ui/src/components/PlaylistCurationView.ts` | New component — dual-panel curation logic |

No Rust/daemon changes. No Cargo.toml or package.json changes. No new RPC handlers needed — all RPCs (`browse.getPlaylist`, `playlist.removeTracks`) were implemented in earlier stories.

### Available RPCs (no new handlers needed)

All required RPCs exist:

| RPC | Params | Returns | Implemented in |
|-----|--------|---------|----------------|
| `browse.getPlaylist` | `{ playlistId: string }` | `{ playlist: BrowsePlaylist; tracks: BrowseTrack[] }` | Story 9.x / rpc.rs:651 |
| `playlist.removeTracks` | `{ playlistId: string; trackIds: string[] }` | `{ ok: true }` | Story 11.4 / rpc.rs:937 |

TypeScript wrappers in `rpc.ts`:
- `fetchBrowsePlaylist(playlistId)` → line 197-201
- `rpcCall('playlist.removeTracks', {...})` — use `rpcCall` directly (no named wrapper exists for write RPCs)

### `BrowseTrack.sizeBytes` is always null in practice

The Rust `Song` struct (`hifimule-daemon/src/domain/models.rs:26-48`) does not have a `size_bytes` field. Serialization therefore never emits `sizeBytes`. On the TypeScript side, `BrowseTrack.sizeBytes` will be `undefined` (not `null`) from the JSON parser — the `?? 0` fallback in `formatBytes` handles both. The storage stat will show `0 B` for all playlists. Per AC 5, this is correct behavior: tracks without `sizeBytes` are excluded from the total.

### Artist/album grouping from track fields

The curation view groups tracks by `artistName` and `albumName` on the `BrowseTrack`:
- `artistName` ← Rust `Song.artist_name: Option<String>`, serialized as `artistName`
- `albumName` ← Rust `Song.album_title: Option<String>`, serialized as `albumName` (via `#[serde(rename = "albumName")]`)

Both fields are `Option<String>` in Rust, meaning they CAN be null in JSON. The TypeScript `BrowseTrack` interface declares them as `string` (non-nullable) — this is a type lie. Always guard with `|| 'Unknown Artist'` and `|| 'Unknown Album'` before using as Map keys.

### `MediaCard.create()` current signature (post-Story-11.5)

```typescript
public static create(
    item: JellyfinItem | JellyfinView | BrowseDisplayItem,
    mode: 'libraries' | 'items',
    isSynced: boolean,
    onNavigate: () => void | Promise<void>,
    deviceSelectionEnabled?: boolean,
    supportsPlaylistWrite?: boolean,   // added Story 11.5
): HTMLElement
```

Story 11.6 adds `onCurate?` as the 7th parameter. All existing call sites pass 5–6 arguments; the 7th defaults to `undefined` — no existing callers break.

### Optimistic vs. pessimistic removal

The `doRemove()` method updates local state *before* the RPC call. Reasons:
1. The RPC is expected to succeed (capability-gated; provider supports it).
2. Instant visual feedback is more important than consistency for this use case.
3. On failure, the user sees an error and can re-open the view to restore from server truth (fresh `browse.getPlaylist` fetch in `load()`).

If pessimistic semantics were required (rollback on failure), the track list would need to be saved before removal and restored on error. This adds complexity not justified by the spec.

### `_supportsPlaylistWrite` propagation

`_supportsPlaylistWrite` is the module-level variable in `library.ts` set by `BasketSidebar` via `setPlaylistWriteCapability()` (Story 11.5 — `library.ts:29-32`). The curate button is only shown when this is `true`, meaning the connected provider supports playlist write. When the user disconnects or switches to a provider without write support, the next `loadPlaylists()` call passes `undefined` for `onCurate` and the button doesn't appear.

### Cache key structure

Cache keys in library.ts follow the pattern `${state.browseMode}:${id}` via `cacheKey()`. For the playlists mode:
- Root playlists list: `'playlists:'` (empty id)
- Specific playlist's track list: `'playlists:abc123'` (playlist ID)

The `openCurationView` close callback deletes both by filtering on `key.includes(playlistId)` (catches the track list) and `key.startsWith('playlists:')` (catches the root list, though the root doesn't change during curation — kept for safety). The `invalidatePlaylistsCache()` function exported from library.ts (line 87-93) only clears `playlists:*` keys — use the inline filter in `openCurationView` to also clear the specific playlist's track cache.

### Why not use the existing `navigateToBrowseItem` flow

`navigateToBrowseItem()` for a Playlist item calls `navigateToPlaylist()` → `loadPlaylistTracks()` → flat grid of tracks. The curation view is a different interaction: it opens directly from the `onCurate` callback which bypasses navigation state. This is intentional: the curation view is not a navigation level (no breadcrumb added). Returning via `onClose` → `loadPlaylists()` is a full page replacement, not a breadcrumb pop.

### `sl-icon-button` event propagation in MediaCard cards

In `MediaCard.create()`, the card element has a `click` handler for `onNavigate`. The curate `sl-icon-button` must call `e.stopPropagation()` to prevent the button click from bubbling to the card's click handler. This is the same pattern used by the basket toggle button in `MediaCard.ts` — verify the existing toggle button also stops propagation (it does, via the toggle element consuming the click natively in Shoelace).

### Story 11.5 review findings relevant to this story

From Story 11.5 review:
- **Attribute injection**: Always use `escapeAttr()` (escaping `"`) for values interpolated into HTML attribute strings, not `escapeHtml()`. The `PlaylistCurationView` uses a separate `escapeAttr()` method for `data-artist` and `data-album` attributes.
- **Capture-phase click listeners**: Not used in this story — no outside-click dismiss pattern.
- **Double-submit**: The remove buttons are not disabled during the async RPC call. The optimistic update removes the track from `this.tracks` immediately and then calls `render()` which replaces the DOM — the button that was clicked no longer exists. This naturally prevents double-submission.
- **Enter key**: Not applicable (no text inputs in the curation view).

### Project Structure Notes

- New component file path: `hifimule-ui/src/components/PlaylistCurationView.ts` — consistent with `MediaCard.ts`, `BasketSidebar.ts`, etc.
- Import in library.ts: `import { PlaylistCurationView } from './components/PlaylistCurationView';` — follows existing component import pattern.
- No barrel file (`index.ts`) exists in `components/` — import directly.

### References

- Epic 11 Story 11.6 spec: `_bmad-output/planning-artifacts/epics.md:2252–2289`
- UX spec §5.2 (Playlist Curation View): `_bmad-output/planning-artifacts/ux-design-specification.md`
- `browse.getPlaylist` RPC handler: `hifimule-daemon/src/rpc.rs:651-670`
- `playlist.removeTracks` RPC handler: `hifimule-daemon/src/rpc.rs:937-976`
- `BrowseTrack` TypeScript interface: `hifimule-ui/src/rpc.ts:121-137`
- `fetchBrowsePlaylist()`: `hifimule-ui/src/rpc.ts:197-201`
- `Song` domain model (no `size_bytes` field): `hifimule-daemon/src/domain/models.rs:26-48`
- `MediaCard.create()` current signature: `hifimule-ui/src/components/MediaCard.ts:39-46`
- `renderGrid()` current implementation: `hifimule-ui/src/library.ts:508-547`
- `loadPlaylists()` current implementation: `hifimule-ui/src/library.ts:1088-1126`
- `navigateToPlaylist()`: `hifimule-ui/src/library.ts:1631-1638`
- `_supportsPlaylistWrite` module variable and `setPlaylistWriteCapability()`: `hifimule-ui/src/library.ts:29-32`
- `invalidatePlaylistsCache()`: `hifimule-ui/src/library.ts:87-93`
- `saveScroll()`: already present in library.ts
- `mapPlaylists()` — `type: 'Playlist'` assignment: `hifimule-ui/src/library.ts:225-236`
- `sumTrackSizes()` utility (handles null via `?? 0`): `hifimule-ui/src/library.ts:321-323`
- Story 11.5 (previous) — MediaCard context menu, `_supportsPlaylistWrite` wiring: `_bmad-output/implementation-artifacts/11-5-save-selection-as-playlist-ui-and-context-menu.md`
- Story 11.4 — `playlist.removeTracks` RPC implementation: `_bmad-output/implementation-artifacts/11-4-playlist-rpcs-and-selection-to-tracks-resolution.md`

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

### Completion Notes List

- Added 7 i18n keys × 3 languages (en/fr/es) to catalog.json: `playlist.curation.*`
- (Tasks 7–9, 2026-06-07) Added 2 new i18n keys (`playlist.curation.remove_track`, `playlist.curation.no_tracks`) × 3 languages to catalog.json.
- Added `selectedAlbum: string | null` field to `PlaylistCurationView`. Album rows now have click-to-focus handler (toggle: click focused album clears filter); focused album gets left-border accent + background tint identical to selected artist rows. `removeAlbum()` resets `selectedAlbum` when the focused album is removed. `render()` auto-resets `selectedAlbum` when it no longer exists in the current artist's album set.
- Added `getTracksForPanel()` helper — filters `this.tracks` by `selectedArtist` (always) and `selectedAlbum` (when set). Added `curation-track-panel` div below the artist/album panels: lists title + duration for each panel track, each with a `curation-remove-track` sl-icon-button. `bindEvents()` wires track buttons to `doRemove([trackId])`. Track panel shows "No tracks for this selection" when empty.
- TypeScript: zero errors (`npx tsc --noEmit`). catalog.json: valid JSON.
- Extended `MediaCard.create()` with optional 7th param `onCurate?(id, name)`. On Playlist-typed cards the callback wires a `pencil-square` sl-icon-button with `e.stopPropagation()` to prevent bubble-through to the card's `onNavigate` handler.
- Updated `renderGrid(items, onCurate?)` and both `renderGrid` call sites in `loadPlaylists()` to pass `openCurationView` when `_supportsPlaylistWrite` is true.
- Created `PlaylistCurationView` class: loads tracks via `fetchBrowsePlaylist`, renders dual-panel (artist list 40% / album panel flex-1), stats header (track count + duration + bytes), optimistic removal via `playlist.removeTracks` RPC, HTML-escaping via separate `escapeHtml`/`escapeAttr` methods per Story 11.5 review learnings.
- Added `openCurationView(id, name)` function to library.ts before `loadPlaylists()`: calls `saveScroll()`, mounts the view, on-close invalidates both the specific playlist's track cache and all `playlists:*` cache entries, then restores the playlist list via `loadPlaylists()`.
- TypeScript: zero errors (`rtk tsc` → "No errors found"). JSON catalog: valid.

### File List

- hifimule-i18n/catalog.json
- hifimule-ui/src/components/MediaCard.ts
- hifimule-ui/src/library.ts
- hifimule-ui/src/components/PlaylistCurationView.ts
- _bmad-output/planning-artifacts/prd.md
- _bmad-output/planning-artifacts/ux-design-specification.md
- _bmad-output/planning-artifacts/epics.md
- _bmad-output/planning-artifacts/sprint-change-proposal-2026-06-07-playlist-curation-track-list.md
- _bmad-output/implementation-artifacts/sprint-status.yaml

## Change Log

- 2026-06-06: Story 11.6 created — dual-panel playlist curation view ready for dev.
- 2026-06-06: Implementation complete — all 6 tasks done, TypeScript compiles clean, story ready for review.
- 2026-06-07: Story reopened — Sprint Change Proposal approved. Tasks 7–9 added for track panel, album focus state, and individual track removal.
- 2026-06-07: Tasks 7–9 implemented. TypeScript compiles clean. Story moved to review.

## Status

done
