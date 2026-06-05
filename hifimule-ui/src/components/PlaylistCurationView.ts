import { fetchBrowsePlaylist, BrowseTrack, rpcCall } from '../rpc';
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
        if (this.selectedArtist === artistName) this.selectedArtist = null;
    }

    private async removeAlbum(artistName: string, albumName: string): Promise<void> {
        const trackIds = this.getTrackIdsByAlbum(artistName, albumName);
        if (trackIds.length === 0) return;
        await this.doRemove(trackIds);
    }

    private async doRemove(trackIds: string[]): Promise<void> {
        // Optimistic local update — removes from local state before RPC returns
        const removedSet = new Set(trackIds);
        this.tracks = this.tracks.filter(t => !removedSet.has(t.id));

        let errorMsg: string | null = null;
        try {
            await rpcCall('playlist.removeTracks', {
                playlistId: this.playlistId,
                trackIds,
            });
        } catch (err) {
            errorMsg = err instanceof Error ? err.message : String(err);
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
