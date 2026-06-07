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
        if (this.selectedArtist === null) return this.tracks; // All artists → full playlist, playlist order
        return this.tracks.filter(track => {
            if ((track.artistName || 'Unknown Artist') !== this.selectedArtist) return false;
            if (this.selectedAlbum !== null && (track.albumName || 'Unknown Album') !== this.selectedAlbum) return false;
            return true;
        });
    }

    private renderStats(): string {
        const count = this.tracks.length;
        const totalSecs = this.tracks.reduce((s, track) => s + (track.duration ?? 0), 0);
        const totalBytes = this.tracks.reduce((s, track) => s + (track.sizeBytes ?? 0), 0);
        return `
            <div class="curation-stats" style="
                padding: 0.5rem 1rem;
                background: var(--sl-color-neutral-50);
                border-bottom: 1px solid var(--sl-color-neutral-200);
                font-size: var(--sl-font-size-small);
                color: var(--sl-color-neutral-600);
                display: flex;
                align-items: center;
                gap: 1.5rem;
            ">
                <span>${count} track${count === 1 ? '' : 's'}</span>
                <span>${formatDuration(totalSecs)}</span>
                <span>${formatBytes(totalBytes)}</span>
                <sl-button
                    id="curation-add-tracks-btn"
                    size="small"
                    variant="default"
                    style="margin-left: auto;"
                >
                    <sl-icon slot="prefix" name="plus-circle"></sl-icon>
                    ${t('playlist.curation.add_tracks')}
                </sl-button>
            </div>
        `;
    }

    private render(): void {
        const artistIndex = this.buildArtistIndex();
        const artists = Array.from(artistIndex.keys()).sort((a, b) => a.localeCompare(b));
        // null = "All artists". A previously-selected artist that no longer exists → fall back to All.
        if (this.selectedArtist !== null && !artistIndex.has(this.selectedArtist)) {
            this.selectedArtist = null;
        }
        const selectedArtist = this.selectedArtist; // null = All

        const albums = selectedArtist !== null
            ? Array.from(artistIndex.get(selectedArtist)!).sort((a, b) => a.localeCompare(b))
            : [];

        // Reset selectedAlbum if it no longer exists for this artist
        if (this.selectedAlbum !== null && !albums.includes(this.selectedAlbum)) {
            this.selectedAlbum = null;
        }

        // Precompute absolute 1-based position map for #N badges (Task 3)
        const positionById = new Map<string, number>();
        this.tracks.forEach((track, i) => positionById.set(track.id, i));

        const panelTracks = this.getTracksForPanel();

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
                    ${this.isRenamingPlaylist
                        ? `<sl-input
                               id="playlist-rename-input"
                               value="${this.escapeAttr(this.playlistName)}"
                               size="small"
                               style="flex: 1; max-width: 300px;"
                           ></sl-input>
                           <sl-icon-button
                               class="playlist-rename-save"
                               name="check"
                               label="${t('playlist.curation.rename_save')}"
                           ></sl-icon-button>
                           <sl-icon-button
                               class="playlist-rename-cancel"
                               name="x"
                               label="${t('playlist.curation.rename_cancel')}"
                           ></sl-icon-button>`
                        : `<span
                               class="playlist-name-title"
                               style="font-weight: var(--sl-font-weight-semibold); font-size: var(--sl-font-size-medium); cursor: pointer; border-bottom: 1px dashed var(--sl-color-neutral-400);"
                               title="${t('playlist.curation.rename_hint')}"
                           >${this.escapeHtml(this.playlistName)}</span>`
                    }
                    ${this.supportsPlaylistWrite
                        ? `<sl-icon-button
                               class="playlist-delete-btn"
                               name="trash"
                               label="${t('playlist.curation.delete_title')}"
                               style="color: var(--sl-color-danger-600); margin-left: auto;"
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
                        ${this.tracks.length === 0
                            ? `<p style="padding: 1rem; color: var(--sl-color-neutral-500);">${t('playlist.curation.no_artists')}</p>`
                            : `<div class="curation-artist-row curation-all-artists${selectedArtist === null ? ' curation-selected' : ''}"
                                     style="
                                        display: flex;
                                        align-items: center;
                                        padding: 0.5rem 0.75rem;
                                        cursor: pointer;
                                        background: ${selectedArtist === null ? 'var(--sl-color-primary-50)' : 'transparent'};
                                        border-left: 3px solid ${selectedArtist === null ? 'var(--sl-color-primary-600)' : 'transparent'};
                                        gap: 0.5rem;
                                     ">
                                    <span style="flex: 1; font-size: var(--sl-font-size-small); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-style: italic;">${t('playlist.curation.all_artists')}</span>
                                </div>
                               ${artists.map(artist => `
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
                            `).join('')}`
                        }
                    </div>
                    <div id="curation-album-panel" style="
                        flex: 1;
                        overflow-y: auto;
                        padding: 0.5rem 0;
                    ">
                        ${selectedArtist === null
                            ? `<p style="padding: 1rem; color: var(--sl-color-neutral-500);">${t('playlist.curation.select_artist')}</p>`
                            : `<div class="curation-album-row curation-all-albums${this.selectedAlbum === null ? ' curation-album-focused' : ''}"
                                     style="
                                        display: flex;
                                        align-items: center;
                                        padding: 0.5rem 0.75rem;
                                        cursor: pointer;
                                        background: ${this.selectedAlbum === null ? 'var(--sl-color-primary-50)' : 'transparent'};
                                        border-left: 3px solid ${this.selectedAlbum === null ? 'var(--sl-color-primary-600)' : 'transparent'};
                                        gap: 0.5rem;
                                     ">
                                    <span style="flex: 1; font-size: var(--sl-font-size-small); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-style: italic;">${t('playlist.curation.all_albums')}</span>
                                </div>
                               ${albums.map(album => `
                                <div class="curation-album-row${album === this.selectedAlbum ? ' curation-album-focused' : ''}"
                                     data-artist="${this.escapeAttr(selectedArtist)}"
                                     data-album="${this.escapeAttr(album)}"
                                     style="
                                        display: flex;
                                        align-items: center;
                                        padding: 0.5rem 0.75rem;
                                        cursor: pointer;
                                        background: ${album === this.selectedAlbum ? 'var(--sl-color-primary-50)' : 'transparent'};
                                        border-left: 3px solid ${album === this.selectedAlbum ? 'var(--sl-color-primary-600)' : 'transparent'};
                                        gap: 0.5rem;
                                     ">
                                    <span style="flex: 1; font-size: var(--sl-font-size-small); overflow: hidden; text-overflow: ellipsis; white-space: nowrap;"
                                          title="${this.escapeAttr(album)}">${this.escapeHtml(album)}</span>
                                    <sl-icon-button
                                        class="curation-remove-album"
                                        name="x-circle"
                                        data-artist="${this.escapeAttr(selectedArtist)}"
                                        data-album="${this.escapeAttr(album)}"
                                        label="${t('playlist.curation.remove_album')}"
                                        style="font-size: 0.9rem; flex-shrink: 0;"
                                    ></sl-icon-button>
                                </div>
                            `).join('')}`
                        }
                    </div>
                </div>
                <div id="curation-track-panel" style="
                    border-top: 1px solid var(--sl-color-neutral-200);
                    overflow-y: auto;
                    max-height: 40%;
                    padding: 0.5rem 0;
                    flex-shrink: 0;
                ">
                    ${panelTracks.length === 0
                        ? `<p style="padding: 0.5rem 1rem; color: var(--sl-color-neutral-500); font-size: var(--sl-font-size-small);">${t('playlist.curation.no_tracks')}</p>`
                        : panelTracks.map((track, panelIdx) => `
                            <div class="curation-track-row" data-panel-index="${panelIdx}"
                                 style="display: flex; align-items: center; padding: 0.35rem 0.75rem; gap: 0.5rem;">
                                <span style="font-size: var(--sl-font-size-x-small); color: var(--sl-color-neutral-400); flex-shrink: 0; min-width: 2.5em; text-align: right;">
                                    #${(positionById.get(track.id) ?? 0) + 1}
                                </span>
                                <span style="flex: 1; font-size: var(--sl-font-size-small); overflow: hidden; text-overflow: ellipsis; white-space: nowrap;"
                                      title="${this.escapeAttr(track.title)}">${this.escapeHtml(track.title)}</span>
                                <span style="font-size: var(--sl-font-size-x-small); color: var(--sl-color-neutral-500); flex-shrink: 0;">
                                    ${formatDuration(track.duration ?? 0)}
                                </span>
                                ${this.supportsPlaylistWrite ? `
                                    <sl-icon-button class="curation-move-up" name="chevron-up"
                                        data-panel-index="${panelIdx}" label="${t('playlist.curation.move_up')}"
                                        ${panelIdx === 0 ? 'disabled' : ''} style="font-size: 0.9rem; flex-shrink: 0;"></sl-icon-button>
                                    <sl-icon-button class="curation-move-down" name="chevron-down"
                                        data-panel-index="${panelIdx}" label="${t('playlist.curation.move_down')}"
                                        ${panelIdx === panelTracks.length - 1 ? 'disabled' : ''} style="font-size: 0.9rem; flex-shrink: 0;"></sl-icon-button>
                                ` : ''}
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
                <sl-alert id="curation-error" variant="danger" closable style="display:none; margin: 0.5rem;"></sl-alert>
            </div>
        `;

        this.bindEvents();

        requestAnimationFrame(() => {
            const artistPanel = this.container.querySelector<HTMLElement>('#curation-artist-panel');
            const selectedRow = artistPanel?.querySelector<HTMLElement>('.curation-artist-row.curation-selected');
            if (artistPanel && selectedRow) {
                artistPanel.scrollTop =
                    selectedRow.getBoundingClientRect().top - artistPanel.getBoundingClientRect().top;
            }
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

        // Rename: save
        this.container.querySelector('.playlist-rename-save')?.addEventListener('click', async () => {
            if (this.isSavingRename) return;
            const input = this.container.querySelector('#playlist-rename-input') as any;
            const newName = input?.value?.trim();
            if (newName && newName !== this.playlistName) {
                this.isSavingRename = true;
                try {
                    await rpcCall('playlist.rename', { playlistId: this.playlistId, name: newName });
                    this.playlistName = newName;
                } catch (err) {
                    const message = err instanceof Error ? err.message : String(err);
                    showToast(t('playlist.curation.rename_error', { message }), 'danger');
                    return; // stay in edit mode so the user can retry
                } finally {
                    this.isSavingRename = false;
                }
            }
            this.isRenamingPlaylist = false;
            this.render();
        });

        // Rename: cancel
        this.container.querySelector('.playlist-rename-cancel')?.addEventListener('click', () => {
            this.isRenamingPlaylist = false;
            this.render();
        });

        // Rename: Escape key
        this.container.querySelector('#playlist-rename-input')?.addEventListener('keydown', (e) => {
            if ((e as KeyboardEvent).key === 'Escape') {
                this.isRenamingPlaylist = false;
                this.render();
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
            try {
                await rpcCall('playlist.delete', { playlistId: this.playlistId });
            } catch (err) {
                const message = err instanceof Error ? err.message : String(err);
                showToast(t('playlist.curation.delete_error', { message }), 'danger');
                return; // leave the dialog open so the user can retry or cancel
            } finally {
                this.isDeleting = false;
            }
            this.onClose();
        });

        this.container.querySelector('.curation-all-artists')?.addEventListener('click', () => {
            this.selectedArtist = null;
            this.selectedAlbum = null;
            this.render();
        });

        this.container.querySelector('.curation-all-albums')?.addEventListener('click', () => {
            this.selectedAlbum = null;
            this.render();
        });

        this.container.querySelectorAll<HTMLElement>('.curation-artist-row').forEach(row => {
            row.addEventListener('click', (e) => {
                if ((e.target as HTMLElement).closest('.curation-remove-artist')) return;
                const artist = row.dataset.artist;
                if (artist) {
                    this.selectedArtist = artist;
                    this.render();
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

        this.container.querySelectorAll<HTMLElement>('.curation-remove-album').forEach(btn => {
            btn.addEventListener('click', async (e) => {
                e.stopPropagation();
                const artist = (btn as any).dataset?.artist ?? btn.closest('[data-artist]')?.getAttribute('data-artist');
                const album = (btn as any).dataset?.album ?? btn.closest('[data-album]')?.getAttribute('data-album');
                if (artist && album) await this.removeAlbum(artist, album);
            });
        });

        // Album row click — toggle album focus to filter the track panel
        this.container.querySelectorAll<HTMLElement>('.curation-album-row').forEach(row => {
            row.addEventListener('click', (e) => {
                if ((e.target as HTMLElement).closest('.curation-remove-album')) return;
                const album = row.dataset.album;
                if (album) {
                    this.selectedAlbum = album === this.selectedAlbum ? null : album;
                    this.render();
                }
            });
        });

        // "Add tracks" button
        this.container.querySelector('#curation-add-tracks-btn')?.addEventListener('click', (e) => {
            e.stopPropagation();
            this.openAddTracksDialog();
        });

        // Track remove buttons
        this.container.querySelectorAll<HTMLElement>('.curation-remove-track').forEach(btn => {
            btn.addEventListener('click', async (e) => {
                e.stopPropagation();
                const trackId = (btn as any).dataset?.trackId
                    ?? btn.closest('[data-track-id]')?.getAttribute('data-track-id');
                if (trackId) await this.doRemove([trackId]);
            });
        });

        // Move up/down buttons
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
        // Optimistic local update — removes from local state before RPC returns
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
        } finally {
            this.isRemoving = false;
        }

        this.render();

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

        // Use object-reference indexOf — duplicate-id safe
        const fullIdxA = this.tracks.indexOf(panel[panelIdx]);
        const fullIdxB = this.tracks.indexOf(panel[neighbourPanelIdx]);
        if (fullIdxA < 0 || fullIdxB < 0) return;

        this.isReordering = true;
        const previousOrder = this.tracks.slice(); // snapshot for rollback
        const next = this.tracks.slice();
        [next[fullIdxA], next[fullIdxB]] = [next[fullIdxB], next[fullIdxA]];
        this.tracks = next; // optimistic
        this.render(); // #N updates immediately

        let errorMsg: string | null = null;
        try {
            await rpcCall('playlist.reorder', {
                playlistId: this.playlistId,
                trackIds: this.tracks.map(track => track.id), // full reordered id list
            });
        } catch (err) {
            errorMsg = err instanceof Error ? err.message : String(err);
            this.tracks = previousOrder; // rollback
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

    private openAddTracksDialog(): void {
        const dialog = document.createElement('sl-dialog') as any;
        dialog.label = t('playlist.curation.add_tracks');
        dialog.innerHTML = `
            <sl-input
                id="add-tracks-query"
                placeholder="${t('playlist.curation.add_tracks_placeholder')}"
                clearable
                autofocus
            ></sl-input>
            <div id="add-tracks-results" style="
                margin-top: 0.75rem;
                max-height: 300px;
                overflow-y: auto;
                display: flex;
                flex-direction: column;
                gap: 0.25rem;
            "></div>
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
                el.innerHTML = `<p style="color: var(--sl-color-neutral-500); font-size: var(--sl-font-size-small);">${t('playlist.curation.no_search_results')}</p>`;
                return;
            }
            for (const track of tracks) {
                const row = document.createElement('div');
                row.style.cssText = `
                    display: flex; align-items: center; gap: 0.5rem;
                    padding: 0.35rem 0.5rem;
                    border-radius: var(--sl-border-radius-small);
                    cursor: pointer;
                    background: ${selectedIds.has(track.id) ? 'var(--sl-color-primary-50)' : 'transparent'};
                    border: 1px solid ${selectedIds.has(track.id) ? 'var(--sl-color-primary-300)' : 'transparent'};
                `;
                row.dataset.trackId = track.id;

                const cb = document.createElement('sl-checkbox') as any;
                cb.checked = selectedIds.has(track.id);
                cb.style.flexShrink = '0';

                const info = document.createElement('div');
                info.style.cssText = 'flex: 1; overflow: hidden;';
                info.innerHTML = `
                    <div style="font-size: var(--sl-font-size-small); overflow: hidden; text-overflow: ellipsis; white-space: nowrap;">${this.escapeHtml(track.title)}</div>
                    <div style="font-size: var(--sl-font-size-x-small); color: var(--sl-color-neutral-500); overflow: hidden; text-overflow: ellipsis; white-space: nowrap;">${this.escapeHtml(track.artistName || '')} — ${this.escapeHtml(track.albumName || '')}</div>
                `;

                row.appendChild(cb);
                row.appendChild(info);

                const updateRow = (selected: boolean) => {
                    row.style.background = selected ? 'var(--sl-color-primary-50)' : 'transparent';
                    row.style.borderColor = selected ? 'var(--sl-color-primary-300)' : 'transparent';
                    cb.checked = selected;
                    const btn = confirmBtn();
                    if (btn) btn.disabled = selectedIds.size === 0;
                };

                // Row click (text area): toggle selection and sync checkbox
                row.addEventListener('click', (e) => {
                    if ((e.target as HTMLElement).closest('sl-checkbox')) return;
                    if (selectedIds.has(track.id)) {
                        selectedIds.delete(track.id);
                    } else {
                        selectedIds.add(track.id);
                    }
                    updateRow(selectedIds.has(track.id));
                });

                // Checkbox click: Shoelace already toggled cb.checked, sync selectedIds to match
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
            // Each search starts a fresh selection (only currently-visible tracks
            // can be confirmed) and supersedes any in-flight request.
            const queryId = ++latestQueryId;
            resetSelection();
            if (!query.trim()) {
                resultsEl().innerHTML = '';
                return;
            }
            resultsEl().innerHTML = '<sl-spinner></sl-spinner>';
            try {
                const result = await fetchBrowseSearch(query);
                if (queryId !== latestQueryId) return; // a newer search superseded this one
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
