import { fetchBrowsePlaylist, fetchBrowseSearch, BrowseTrack, rpcCall } from '../rpc';
import { t } from '../i18n';

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
    private isAddingTracks = false;

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
        if (!this.selectedArtist) return [];
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
        const selectedArtist = this.selectedArtist && artistIndex.has(this.selectedArtist)
            ? this.selectedArtist
            : (artists[0] ?? null);
        this.selectedArtist = selectedArtist;

        const albums = selectedArtist
            ? Array.from(artistIndex.get(selectedArtist)!).sort((a, b) => a.localeCompare(b))
            : [];

        // Reset selectedAlbum if it no longer exists for this artist
        if (this.selectedAlbum !== null && !albums.includes(this.selectedAlbum)) {
            this.selectedAlbum = null;
        }

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
                                <div class="curation-album-row${album === this.selectedAlbum ? ' curation-album-focused' : ''}"
                                     data-artist="${this.escapeAttr(selectedArtist!)}"
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
                <div id="curation-track-panel" style="
                    border-top: 1px solid var(--sl-color-neutral-200);
                    overflow-y: auto;
                    max-height: 40%;
                    padding: 0.5rem 0;
                    flex-shrink: 0;
                ">
                    ${panelTracks.length === 0
                        ? `<p style="padding: 0.5rem 1rem; color: var(--sl-color-neutral-500); font-size: var(--sl-font-size-small);">${t('playlist.curation.no_tracks')}</p>`
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
        if (this.isRemoving) return;
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

                row.appendChild(cb);
                row.appendChild(info);
                resultsEl().appendChild(row);
            }
        };

        const doSearch = async (query: string) => {
            if (!query.trim()) {
                resultsEl().innerHTML = '';
                return;
            }
            resultsEl().innerHTML = '<sl-spinner></sl-spinner>';
            try {
                const result = await fetchBrowseSearch(query);
                renderResults(result.tracks);
            } catch (err) {
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
            if (selectedIds.size === 0 || this.isAddingTracks) return;
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
