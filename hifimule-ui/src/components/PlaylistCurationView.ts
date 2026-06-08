import { fetchBrowsePlaylist, fetchBrowseSearch, BrowseTrack, rpcCall } from '../rpc';
import { t } from '../i18n';
import { showToast } from '../toast';

function formatDuration(totalSecs: number): string {
    const h = Math.floor(totalSecs / 3600);
    const m = Math.floor((totalSecs % 3600) / 60);
    const s = totalSecs % 60;
    if (h > 0) return `${h}h ${m}m ${s}s`;
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
    private selectedAlbum: string | null = null;
    private onClose: () => void;
    private isRemoving = false;
    private isReordering = false;
    private isAddingTracks = false;
    /** True while any optimistic track-list mutation (remove/reorder/add) has an RPC in flight. */
    private get isMutating(): boolean {
        return this.isRemoving || this.isReordering || this.isAddingTracks;
    }
    private supportsPlaylistWrite: boolean = false;
    private isRenamingPlaylist = false;
    private isSavingRename = false;
    private isDeleting = false;

    constructor(
        container: HTMLElement,
        playlistId: string,
        playlistName: string,
        onClose: () => void,
        supportsPlaylistWrite = false,
    ) {
        this.container = container;
        this.playlistId = playlistId;
        this.playlistName = playlistName;
        this.onClose = onClose;
        this.supportsPlaylistWrite = supportsPlaylistWrite;
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
            .filter(track => (track.artistName || 'Unknown Artist') === artistName)
            .map(track => track.id);
    }

    private getTrackIdsByAlbum(artistName: string, albumName: string): string[] {
        return this.tracks
            .filter(track =>
                (track.artistName || 'Unknown Artist') === artistName &&
                (track.albumName || 'Unknown Album') === albumName
            )
            .map(track => track.id);
    }

    private getTracksForPanel(): BrowseTrack[] {
        if (this.selectedArtist === null) return this.tracks;
        return this.tracks.filter(track => {
            if ((track.artistName || 'Unknown Artist') !== this.selectedArtist) return false;
            if (this.selectedAlbum !== null && (track.albumName || 'Unknown Album') !== this.selectedAlbum) return false;
            return true;
        });
    }

    // ─── Panel HTML builders ───────────────────────────────────────────────────

    private buildArtistPanelHtml(artists: string[], selectedArtist: string | null): string {
        if (this.tracks.length === 0) {
            return `<p class="curation-empty-state">${t('playlist.curation.no_artists')}</p>`;
        }
        return `<div class="curation-artist-row curation-all-artists${selectedArtist === null ? ' curation-selected' : ''}"
                     role="button" tabindex="0"
                     aria-pressed="${selectedArtist === null ? 'true' : 'false'}">
                    <span class="curation-row-label curation-row-label--italic">${t('playlist.curation.all_artists')}</span>
                </div>
                ${artists.map(artist => `
                 <div class="curation-artist-row${artist === selectedArtist ? ' curation-selected' : ''}"
                      data-artist="${this.escapeAttr(artist)}"
                      role="button" tabindex="0"
                      aria-pressed="${artist === selectedArtist ? 'true' : 'false'}">
                     <span class="curation-row-label" title="${this.escapeAttr(artist)}">${this.escapeHtml(artist)}</span>
                     <sl-icon-button
                         class="curation-remove-artist curation-row-action"
                         name="x-circle"
                         data-artist="${this.escapeAttr(artist)}"
                         label="${t('playlist.curation.remove_artist')}"
                         ${this.isMutating ? 'disabled' : ''}
                     ></sl-icon-button>
                 </div>
             `).join('')}`;
    }

    private buildAlbumPanelHtml(albums: string[], selectedArtist: string | null): string {
        if (selectedArtist === null) {
            return `<p class="curation-empty-state">${t('playlist.curation.select_artist')}</p>`;
        }
        return `<div class="curation-album-row curation-all-albums${this.selectedAlbum === null ? ' curation-album-focused' : ''}"
                     role="button" tabindex="0"
                     aria-pressed="${this.selectedAlbum === null ? 'true' : 'false'}">
                    <span class="curation-row-label curation-row-label--italic">${t('playlist.curation.all_albums')}</span>
                </div>
                ${albums.map(album => `
                 <div class="curation-album-row${album === this.selectedAlbum ? ' curation-album-focused' : ''}"
                      data-artist="${this.escapeAttr(selectedArtist)}"
                      data-album="${this.escapeAttr(album)}"
                      role="button" tabindex="0"
                      aria-pressed="${album === this.selectedAlbum ? 'true' : 'false'}">
                     <span class="curation-row-label" title="${this.escapeAttr(album)}">${this.escapeHtml(album)}</span>
                     <sl-icon-button
                         class="curation-remove-album curation-row-action"
                         name="x-circle"
                         data-artist="${this.escapeAttr(selectedArtist)}"
                         data-album="${this.escapeAttr(album)}"
                         label="${t('playlist.curation.remove_album')}"
                         ${this.isMutating ? 'disabled' : ''}
                     ></sl-icon-button>
                 </div>
             `).join('')}`;
    }

    private buildTrackPanelHtml(panelTracks: BrowseTrack[], positionById: Map<string, number>): string {
        if (panelTracks.length === 0) {
            return `<p class="curation-empty-state">${t('playlist.curation.no_tracks')}</p>`;
        }
        return panelTracks.map((track, panelIdx) => `
            <div class="curation-track-row" data-panel-index="${panelIdx}" tabindex="-1">
                <span class="curation-track-pos">#${(positionById.get(track.id) ?? 0) + 1}</span>
                <span class="curation-row-label" title="${this.escapeAttr(track.title)}">${this.escapeHtml(track.title)}</span>
                <span class="curation-track-duration">${formatDuration(track.duration ?? 0)}</span>
                ${this.supportsPlaylistWrite ? `
                    <sl-icon-button class="curation-move-up curation-row-action" name="chevron-up"
                        data-panel-index="${panelIdx}" label="${t('playlist.curation.move_up')}"
                        ${panelIdx === 0 || this.isMutating ? 'disabled' : ''}></sl-icon-button>
                    <sl-icon-button class="curation-move-down curation-row-action" name="chevron-down"
                        data-panel-index="${panelIdx}" label="${t('playlist.curation.move_down')}"
                        ${panelIdx === panelTracks.length - 1 || this.isMutating ? 'disabled' : ''}></sl-icon-button>
                ` : ''}
                <sl-icon-button
                    class="curation-remove-track curation-row-action"
                    name="x-circle"
                    data-track-id="${this.escapeAttr(track.id)}"
                    label="${t('playlist.curation.remove_track')}"
                    ${this.isMutating ? 'disabled' : ''}
                ></sl-icon-button>
            </div>
        `).join('');
    }

    private renderStats(): string {
        const count = this.tracks.length;
        const totalSecs = this.tracks.reduce((s, track) => s + (track.duration ?? 0), 0);
        const totalBytes = this.tracks.reduce((s, track) => s + (track.sizeBytes ?? 0), 0);
        return `
            <div class="curation-stats">
                <span>${count} track${count === 1 ? '' : 's'}</span>
                <span>${formatDuration(totalSecs)}</span>
                <span>${formatBytes(totalBytes)}</span>
                <sl-button
                    id="curation-add-tracks-btn"
                    size="small"
                    variant="default"
                    class="curation-stats-action"
                >
                    <sl-icon slot="prefix" name="plus-circle"></sl-icon>
                    ${t('playlist.curation.add_tracks')}
                </sl-button>
            </div>
        `;
    }

    // ─── Full render ───────────────────────────────────────────────────────────
    // Used for: initial load, rename/delete operations, track mutations (add/remove).
    // Artist/album selection and track reorder use the targeted partial methods below.

    private render(): void {
        const artistIndex = this.buildArtistIndex();
        const artists = Array.from(artistIndex.keys()).sort((a, b) => a.localeCompare(b));
        if (this.selectedArtist !== null && !artistIndex.has(this.selectedArtist)) {
            this.selectedArtist = null;
        }
        const selectedArtist = this.selectedArtist;
        const albums = selectedArtist !== null
            ? Array.from(artistIndex.get(selectedArtist)!).sort((a, b) => a.localeCompare(b))
            : [];
        if (this.selectedAlbum !== null && !albums.includes(this.selectedAlbum)) {
            this.selectedAlbum = null;
        }
        const positionById = new Map<string, number>();
        this.tracks.forEach((track, i) => positionById.set(track.id, i));
        const panelTracks = this.getTracksForPanel();

        this.container.innerHTML = `
            <div class="curation-view">
                <div class="curation-header">
                    <sl-icon-button
                        id="curation-close-btn"
                        name="arrow-left"
                        label="${t('playlist.curation.close')}"
                    ></sl-icon-button>
                    ${this.isRenamingPlaylist
                        ? `<sl-input
                               id="playlist-rename-input"
                               value="${this.escapeAttr(this.playlistName)}"
                               size="small"
                               class="curation-rename-input"
                           ></sl-input>
                           ${this.isSavingRename
                               ? '<sl-spinner class="curation-row-action"></sl-spinner>'
                               : `<sl-icon-button
                                      class="playlist-rename-save curation-row-action"
                                      name="check"
                                      label="${t('playlist.curation.rename_save')}"
                                  ></sl-icon-button>`
                           }
                           <sl-icon-button
                               class="playlist-rename-cancel curation-row-action"
                               name="x"
                               label="${t('playlist.curation.rename_cancel')}"
                               ${this.isSavingRename ? 'disabled' : ''}
                           ></sl-icon-button>`
                        : `<span
                               class="playlist-name-title curation-playlist-name"
                               role="button"
                               tabindex="0"
                               aria-label="${t('playlist.curation.rename_hint')}"
                               title="${t('playlist.curation.rename_hint')}"
                           >${this.escapeHtml(this.playlistName)}</span>`
                    }
                    ${this.supportsPlaylistWrite
                        ? `<sl-icon-button
                               class="playlist-delete-btn curation-delete-btn curation-row-action"
                               name="trash"
                               label="${t('playlist.curation.delete_title')}"
                           ></sl-icon-button>`
                        : ''
                    }
                </div>
                <sl-dialog id="playlist-delete-dialog" label="${t('playlist.curation.delete_title')}">
                    <p>${t('playlist.curation.delete_body', { name: this.escapeHtml(this.playlistName) })}</p>
                    <sl-button slot="footer" class="playlist-delete-cancel" variant="default">
                        ${t('playlist.curation.delete_cancel_btn')}
                    </sl-button>
                    <sl-button slot="footer" class="playlist-delete-confirm" variant="danger">
                        ${t('playlist.curation.delete_confirm')}
                    </sl-button>
                </sl-dialog>
                ${this.renderStats()}
                <div class="curation-panels">
                    <div id="curation-artist-panel" class="curation-artist-panel">
                        ${this.buildArtistPanelHtml(artists, selectedArtist)}
                    </div>
                    <div id="curation-album-panel" class="curation-album-panel">
                        ${this.buildAlbumPanelHtml(albums, selectedArtist)}
                    </div>
                </div>
                <div id="curation-track-panel" class="curation-track-panel">
                    ${this.buildTrackPanelHtml(panelTracks, positionById)}
                </div>
                <sl-alert id="curation-error" variant="danger" closable style="display:none; margin: 0.5rem;"></sl-alert>
            </div>
        `;

        this.bindEvents();

        requestAnimationFrame(() => {
            this.container.querySelector<HTMLElement>('.curation-artist-row.curation-selected')?.scrollIntoView({ block: 'nearest' });
        });
    }

    // ─── Partial re-render methods ─────────────────────────────────────────────
    // Each method updates only the DOM regions that change for a given interaction,
    // leaving the header, stats bar, dialog, and error alert untouched.

    /** Artist row clicked: update selected state in artist panel, rebuild album + track panels. */
    private updateAlbumsAndTracks(): void {
        const artistIndex = this.buildArtistIndex();
        const selectedArtist = this.selectedArtist;
        const albums = selectedArtist !== null
            ? Array.from(artistIndex.get(selectedArtist)!).sort((a, b) => a.localeCompare(b))
            : [];
        if (this.selectedAlbum !== null && !albums.includes(this.selectedAlbum)) {
            this.selectedAlbum = null;
        }
        const positionById = new Map<string, number>();
        this.tracks.forEach((track, i) => positionById.set(track.id, i));
        const panelTracks = this.getTracksForPanel();

        // Toggle selected class on artist rows without rebuilding the panel
        this.container.querySelectorAll<HTMLElement>('.curation-artist-row').forEach(row => {
            const isAll = row.classList.contains('curation-all-artists');
            const isSelected = isAll ? selectedArtist === null : row.dataset.artist === selectedArtist;
            row.classList.toggle('curation-selected', isSelected);
            row.setAttribute('aria-pressed', isSelected ? 'true' : 'false');
        });

        const albumPanel = this.container.querySelector('#curation-album-panel');
        if (albumPanel) {
            albumPanel.innerHTML = this.buildAlbumPanelHtml(albums, selectedArtist);
            this.bindAlbumPanelEvents();
        }

        const trackPanel = this.container.querySelector('#curation-track-panel');
        if (trackPanel) {
            trackPanel.innerHTML = this.buildTrackPanelHtml(panelTracks, positionById);
            this.bindTrackPanelEvents();
        }

        requestAnimationFrame(() => {
            this.container.querySelector<HTMLElement>('.curation-artist-row.curation-selected')?.scrollIntoView({ block: 'nearest' });
        });
    }

    /** Album row clicked: update selected state in album panel, rebuild track panel only. */
    private updateTracksPanel(): void {
        const positionById = new Map<string, number>();
        this.tracks.forEach((track, i) => positionById.set(track.id, i));
        const panelTracks = this.getTracksForPanel();

        this.container.querySelectorAll<HTMLElement>('.curation-album-row').forEach(row => {
            const isAll = row.classList.contains('curation-all-albums');
            const isSelected = isAll ? this.selectedAlbum === null : row.dataset.album === this.selectedAlbum;
            row.classList.toggle('curation-album-focused', isSelected);
            row.setAttribute('aria-pressed', isSelected ? 'true' : 'false');
        });

        const trackPanel = this.container.querySelector('#curation-track-panel');
        if (trackPanel) {
            trackPanel.innerHTML = this.buildTrackPanelHtml(panelTracks, positionById);
            this.bindTrackPanelEvents();
        }
    }

    /** Track order changed: rebuild track panel only (stats and panels unchanged). */
    private updateTrackPanelOnly(): void {
        const positionById = new Map<string, number>();
        this.tracks.forEach((track, i) => positionById.set(track.id, i));
        const panelTracks = this.getTracksForPanel();

        const trackPanel = this.container.querySelector('#curation-track-panel');
        if (trackPanel) {
            trackPanel.innerHTML = this.buildTrackPanelHtml(panelTracks, positionById);
            this.bindTrackPanelEvents();
        }
    }

    // ─── Event binding ─────────────────────────────────────────────────────────

    private bindArtistPanelEvents(): void {
        this.container.querySelector('.curation-all-artists')?.addEventListener('click', () => {
            this.selectedArtist = null;
            this.selectedAlbum = null;
            this.updateAlbumsAndTracks();
        });

        this.container.querySelectorAll<HTMLElement>('.curation-artist-row').forEach(row => {
            row.addEventListener('click', (e) => {
                if ((e.target as HTMLElement).closest('.curation-remove-artist')) return;
                const artist = row.dataset.artist;
                if (artist) {
                    this.selectedArtist = artist;
                    this.updateAlbumsAndTracks();
                }
            });
            row.addEventListener('keydown', (e) => {
                if ((e as KeyboardEvent).key === 'Enter' || (e as KeyboardEvent).key === ' ') {
                    e.preventDefault();
                    row.click();
                }
            });
        });

        this.container.querySelectorAll<HTMLElement>('.curation-remove-artist').forEach(btn => {
            btn.addEventListener('click', async (e) => {
                e.stopPropagation();
                const artist = (btn as any).dataset?.artist ?? btn.closest('[data-artist]')?.getAttribute('data-artist');
                if (artist) await this.removeArtist(artist);
            });
        });
    }

    private bindAlbumPanelEvents(): void {
        this.container.querySelector('.curation-all-albums')?.addEventListener('click', () => {
            this.selectedAlbum = null;
            this.updateTracksPanel();
        });

        this.container.querySelectorAll<HTMLElement>('.curation-album-row').forEach(row => {
            row.addEventListener('click', (e) => {
                if ((e.target as HTMLElement).closest('.curation-remove-album')) return;
                const album = row.dataset.album;
                if (album) {
                    this.selectedAlbum = album === this.selectedAlbum ? null : album;
                    this.updateTracksPanel();
                }
            });
            row.addEventListener('keydown', (e) => {
                if ((e as KeyboardEvent).key === 'Enter' || (e as KeyboardEvent).key === ' ') {
                    e.preventDefault();
                    row.click();
                }
            });
        });

        this.container.querySelectorAll<HTMLElement>('.curation-remove-album').forEach(btn => {
            btn.addEventListener('click', async (e) => {
                e.stopPropagation();
                const artist = (btn as any).dataset?.artist ?? btn.closest('[data-artist]')?.getAttribute('data-artist');
                const album = (btn as any).dataset?.album ?? btn.closest('[data-album]')?.getAttribute('data-album');
                if (artist && album) await this.removeAlbum(artist, album);
            });
        });
    }

    private bindTrackPanelEvents(): void {
        this.container.querySelectorAll<HTMLElement>('.curation-remove-track').forEach(btn => {
            btn.addEventListener('click', async (e) => {
                e.stopPropagation();
                const trackId = (btn as any).dataset?.trackId
                    ?? btn.closest('[data-track-id]')?.getAttribute('data-track-id');
                if (trackId) await this.doRemove([trackId]);
            });
        });

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
    }

    private bindEvents(): void {
        this.container.querySelector('#curation-close-btn')?.addEventListener('click', () => {
            this.onClose();
        });

        // Rename: click title → enter edit mode
        this.container.querySelector('.playlist-name-title')?.addEventListener('click', () => {
            this.isRenamingPlaylist = true;
            this.render();
            const input = this.container.querySelector('#playlist-rename-input') as any;
            if (input) input.focus();
        });
        this.container.querySelector('.playlist-name-title')?.addEventListener('keydown', (e) => {
            if ((e as KeyboardEvent).key === 'Enter' || (e as KeyboardEvent).key === ' ') {
                e.preventDefault();
                this.isRenamingPlaylist = true;
                this.render();
                const input = this.container.querySelector('#playlist-rename-input') as any;
                if (input) input.focus();
            }
        });

        // Rename: save
        this.container.querySelector('.playlist-rename-save')?.addEventListener('click', async () => {
            if (this.isSavingRename) return;
            const input = this.container.querySelector('#playlist-rename-input') as any;
            const newName = input?.value?.trim();
            if (!newName || newName === this.playlistName) {
                this.isRenamingPlaylist = false;
                this.render();
                return;
            }
            this.isSavingRename = true;
            this.render();
            try {
                await rpcCall('playlist.rename', { playlistId: this.playlistId, name: newName });
                this.playlistName = newName;
                this.isRenamingPlaylist = false;
            } catch (err) {
                const message = err instanceof Error ? err.message : String(err);
                showToast(t('playlist.curation.rename_error', { message }), 'danger');
            } finally {
                this.isSavingRename = false;
            }
            this.render();
        });

        // Rename: cancel
        this.container.querySelector('.playlist-rename-cancel')?.addEventListener('click', () => {
            this.isRenamingPlaylist = false;
            this.render();
        });

        // Rename: Escape cancels, Enter confirms
        this.container.querySelector('#playlist-rename-input')?.addEventListener('keydown', (e) => {
            const key = (e as KeyboardEvent).key;
            if (key === 'Escape') {
                this.isRenamingPlaylist = false;
                this.render();
            } else if (key === 'Enter') {
                this.container.querySelector<HTMLElement>('.playlist-rename-save')?.click();
            }
        });

        // Delete: open dialog
        this.container.querySelector('.playlist-delete-btn')?.addEventListener('click', () => {
            (this.container.querySelector('#playlist-delete-dialog') as any)?.show();
        });

        // Delete: cancel
        this.container.querySelector('.playlist-delete-cancel')?.addEventListener('click', () => {
            (this.container.querySelector('#playlist-delete-dialog') as any)?.hide();
        });

        // Delete: confirm
        this.container.querySelector('.playlist-delete-confirm')?.addEventListener('click', async () => {
            if (this.isDeleting) return;
            this.isDeleting = true;
            const confirmBtn = this.container.querySelector('.playlist-delete-confirm') as any;
            const cancelBtn = this.container.querySelector('.playlist-delete-cancel') as any;
            if (confirmBtn) { confirmBtn.loading = true; confirmBtn.disabled = true; }
            if (cancelBtn) { cancelBtn.disabled = true; }
            try {
                await rpcCall('playlist.delete', { playlistId: this.playlistId });
            } catch (err) {
                const message = err instanceof Error ? err.message : String(err);
                showToast(t('playlist.curation.delete_error', { message }), 'danger');
                return;
            } finally {
                this.isDeleting = false;
                if (confirmBtn) { confirmBtn.loading = false; confirmBtn.disabled = false; }
                if (cancelBtn) { cancelBtn.disabled = false; }
            }
            this.onClose();
        });

        // "Add tracks" button
        this.container.querySelector('#curation-add-tracks-btn')?.addEventListener('click', (e) => {
            e.stopPropagation();
            this.openAddTracksDialog();
        });

        this.bindArtistPanelEvents();
        this.bindAlbumPanelEvents();
        this.bindTrackPanelEvents();
    }

    // ─── Actions ───────────────────────────────────────────────────────────────

    private async removeArtist(artistName: string): Promise<void> {
        const trackIds = this.getTrackIdsByArtist(artistName);
        if (trackIds.length === 0) return;
        await this.doRemove(trackIds);
    }

    private async removeAlbum(artistName: string, albumName: string): Promise<void> {
        const trackIds = this.getTrackIdsByAlbum(artistName, albumName);
        if (trackIds.length === 0) return;
        if (this.selectedAlbum === albumName) this.selectedAlbum = null;
        await this.doRemove(trackIds);
    }

    private async doRemove(trackIds: string[]): Promise<void> {
        if (this.isMutating) return;
        this.isRemoving = true;
        const previousTracks = this.tracks.slice();
        const removedSet = new Set(trackIds);
        this.tracks = this.tracks.filter(track => !removedSet.has(track.id));

        let errorMsg: string | null = null;
        try {
            await rpcCall('playlist.removeTracks', {
                playlistId: this.playlistId,
                trackIds,
            });
        } catch (err) {
            errorMsg = err instanceof Error ? err.message : String(err);
            this.tracks = previousTracks;
        } finally {
            this.isRemoving = false;
        }

        // Full render: stats, artist panel, album panel, and track panel all potentially changed.
        this.render();

        requestAnimationFrame(() => {
            const firstRow = this.container.querySelector<HTMLElement>('.curation-track-row');
            firstRow?.focus();
        });

        if (errorMsg !== null) {
            const errorEl = this.container.querySelector('#curation-error') as HTMLElement | null;
            if (errorEl) {
                errorEl.textContent = t('playlist.curation.error', { message: errorMsg });
                errorEl.style.display = '';
                (errorEl as any).open = true;
            }
        }
    }

    private async moveTrack(panelIdx: number, direction: -1 | 1): Promise<void> {
        if (this.isMutating) return;
        const panel = this.getTracksForPanel();
        const neighbourPanelIdx = panelIdx + direction;
        if (panelIdx < 0 || panelIdx >= panel.length) return;
        if (neighbourPanelIdx < 0 || neighbourPanelIdx >= panel.length) return;

        const fullIdxA = this.tracks.indexOf(panel[panelIdx]);
        const fullIdxB = this.tracks.indexOf(panel[neighbourPanelIdx]);
        if (fullIdxA < 0 || fullIdxB < 0) return;

        this.isReordering = true;
        const previousOrder = this.tracks.slice();
        const next = this.tracks.slice();
        [next[fullIdxA], next[fullIdxB]] = [next[fullIdxB], next[fullIdxA]];
        this.tracks = next;

        // Optimistic: show new order with buttons disabled.
        // Only the track panel changes — header, stats, and artist/album panels are untouched.
        this.updateTrackPanelOnly();

        const newPanelIdx = panelIdx + direction;
        requestAnimationFrame(() => {
            this.container.querySelector<HTMLElement>(
                `.curation-track-row[data-panel-index="${newPanelIdx}"]`
            )?.focus();
        });

        let errorMsg: string | null = null;
        try {
            await rpcCall('playlist.reorder', {
                playlistId: this.playlistId,
                trackIds: this.tracks.map(track => track.id),
            });
        } catch (err) {
            errorMsg = err instanceof Error ? err.message : String(err);
            this.tracks = previousOrder;
        } finally {
            this.isReordering = false;
            // Re-render track panel: re-enables buttons in the success path,
            // shows rolled-back order in the error path.
            this.updateTrackPanelOnly();
            const focusIdx = errorMsg !== null ? panelIdx : newPanelIdx;
            requestAnimationFrame(() => {
                this.container.querySelector<HTMLElement>(
                    `.curation-track-row[data-panel-index="${focusIdx}"]`
                )?.focus();
            });
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

    private openAddTracksDialog(): void {
        const dialog = document.createElement('sl-dialog') as any;
        dialog.label = t('playlist.curation.add_tracks');
        dialog.classList.add('curation-add-tracks-dialog');
        dialog.innerHTML = `
            <sl-input
                id="add-tracks-query"
                placeholder="${t('playlist.curation.add_tracks_placeholder')}"
                clearable
                autofocus
            ></sl-input>
            <div id="add-tracks-results" class="curation-add-tracks-results"></div>
            <sl-alert id="add-tracks-error" variant="danger" closable style="display:none; margin-top: 0.75rem;"></sl-alert>
            <sl-button slot="footer" variant="default" id="add-tracks-cancel">${t('basket.actions.cancel')}</sl-button>
            <sl-button slot="footer" variant="primary" id="add-tracks-confirm" disabled>${t('playlist.curation.add_tracks_confirm')}</sl-button>
        `;

        document.body.appendChild(dialog);

        const selectedIds = new Set<string>();
        let searchTimeout: ReturnType<typeof setTimeout> | null = null;

        const queryInput = () => dialog.querySelector('#add-tracks-query') as any;
        const resultsEl = () => dialog.querySelector('#add-tracks-results') as HTMLElement;
        const errorEl = () => dialog.querySelector('#add-tracks-error') as HTMLElement | null;
        const confirmBtn = () => dialog.querySelector('#add-tracks-confirm') as any;

        const renderResults = (tracks: BrowseTrack[]) => {
            const el = resultsEl();
            el.innerHTML = '';
            if (tracks.length === 0) {
                el.innerHTML = `<p class="curation-empty-state">${t('playlist.curation.no_search_results')}</p>`;
                return;
            }
            for (const track of tracks) {
                const row = document.createElement('div');
                row.classList.add('curation-track-result');
                row.classList.toggle('curation-track-result--selected', selectedIds.has(track.id));
                row.dataset.trackId = track.id;

                const cb = document.createElement('sl-checkbox') as any;
                cb.checked = selectedIds.has(track.id);

                const info = document.createElement('div');
                info.className = 'curation-track-result-info';
                info.innerHTML = `
                    <div class="curation-track-result-title">${this.escapeHtml(track.title)}</div>
                    <div class="curation-track-result-meta">${this.escapeHtml(track.artistName || '')} · ${this.escapeHtml(track.albumName || '')}</div>
                `;

                row.appendChild(cb);
                row.appendChild(info);

                const updateRow = (selected: boolean) => {
                    row.classList.toggle('curation-track-result--selected', selected);
                    cb.checked = selected;
                    const btn = confirmBtn();
                    if (btn) btn.disabled = selectedIds.size === 0;
                };

                row.addEventListener('click', (e) => {
                    if ((e.target as HTMLElement).closest('sl-checkbox')) return;
                    if (selectedIds.has(track.id)) {
                        selectedIds.delete(track.id);
                    } else {
                        selectedIds.add(track.id);
                    }
                    updateRow(selectedIds.has(track.id));
                });

                cb.addEventListener('sl-change', () => {
                    if (cb.checked) {
                        selectedIds.add(track.id);
                    } else {
                        selectedIds.delete(track.id);
                    }
                    updateRow(cb.checked);
                });

                el.appendChild(row);
            }
        };

        let latestQueryId = 0;
        const resetSelection = () => {
            selectedIds.clear();
            const btn = confirmBtn();
            if (btn) btn.disabled = true;
        };

        const doSearch = async (query: string) => {
            const queryId = ++latestQueryId;
            resetSelection();
            if (!query.trim()) {
                resultsEl().innerHTML = '';
                return;
            }
            resultsEl().innerHTML = '<sl-spinner></sl-spinner>';
            try {
                const result = await fetchBrowseSearch(query);
                if (queryId !== latestQueryId) return;
                renderResults(result.tracks);
            } catch (err) {
                if (queryId !== latestQueryId) return;
                const msg = err instanceof Error ? err.message : String(err);
                resultsEl().innerHTML = '';
                const err2 = errorEl();
                if (err2) {
                    err2.textContent = msg;
                    err2.style.display = '';
                    (err2 as any).open = true;
                }
            }
        };

        dialog.querySelector('#add-tracks-cancel')?.addEventListener('click', () => dialog.hide());

        dialog.querySelector('#add-tracks-confirm')?.addEventListener('click', async () => {
            if (selectedIds.size === 0 || this.isMutating) return;
            this.isAddingTracks = true;
            const btn = confirmBtn();
            if (btn) { btn.loading = true; btn.disabled = true; }
            const err2 = errorEl();
            if (err2) err2.style.display = 'none';
            try {
                await rpcCall('playlist.addTracks', {
                    playlistId: this.playlistId,
                    trackIds: Array.from(selectedIds),
                });
                dialog.hide();
                const result = await fetchBrowsePlaylist(this.playlistId);
                this.tracks = result.tracks;
                this.render();
                showToast(t('playlist.context.added_success'), 'success');
            } catch (err) {
                const msg = err instanceof Error ? err.message : String(err);
                if (err2) {
                    err2.textContent = t('playlist.curation.add_tracks_error', { message: msg });
                    err2.style.display = '';
                    (err2 as any).open = true;
                }
                if (btn) { btn.loading = false; btn.disabled = false; }
            } finally {
                this.isAddingTracks = false;
            }
        });

        customElements.whenDefined('sl-dialog').then(() => {
            dialog.show();
            const inputEl = queryInput();
            inputEl?.addEventListener('sl-input', () => {
                if (searchTimeout) clearTimeout(searchTimeout);
                searchTimeout = setTimeout(() => doSearch(inputEl.value ?? ''), 300);
            });
        });

        dialog.addEventListener('sl-after-hide', (event: Event) => {
            if (event.target === dialog) {
                if (searchTimeout) clearTimeout(searchTimeout);
                dialog.remove();
            }
        });
    }

    // ─── Helpers ───────────────────────────────────────────────────────────────

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
