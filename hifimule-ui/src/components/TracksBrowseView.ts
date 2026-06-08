import {
    fetchBrowseArtists,
    fetchBrowseArtist,
    fetchBrowseAlbums,
    fetchBrowseTracks,
    BrowseArtist,
    BrowseAlbum,
    BrowseTrack,
} from '../rpc';
import { MediaCard } from './MediaCard';
import { basketStore } from '../state/basket';
import { t } from '../i18n';

const ARTIST_LIMIT = 200;
const ALBUM_LIMIT = 50;
const TRACK_LIMIT = 200;
const AZ_THRESHOLD = 20;
const ALL_LETTERS = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ'.split('').concat('#');
const SCROLL_NEAR_BOTTOM_PX = 200;

interface PanelState<T> {
    items: T[];
    total: number;
    startIndex: number;
    loading: boolean;
    exhausted: boolean;
}

function makePanelState<T>(): PanelState<T> {
    return { items: [], total: 0, startIndex: 0, loading: false, exhausted: false };
}

export class TracksBrowseView {
    private container: HTMLElement;
    private supportsPlaylistWrite: boolean;

    private selectedArtistId: string | null = null;
    private selectedAlbumId: string | null = null;
    private artistLetter: string | null = null;
    private albumLetter: string | null = null;

    private artistState: PanelState<BrowseArtist> = makePanelState();
    private albumState: PanelState<BrowseAlbum> = makePanelState();
    private trackState: PanelState<BrowseTrack> = makePanelState();

    private artistScrollTop = 0;
    private albumScrollTop = 0;
    private trackScrollTop = 0;

    private basketUnsub: (() => void) | null = null;
    private _artistScrollHandler: ((e: Event) => void) | null = null;
    private _albumScrollHandler: ((e: Event) => void) | null = null;
    private _trackScrollHandler: ((e: Event) => void) | null = null;

    constructor(container: HTMLElement, supportsPlaylistWrite = false) {
        this.container = container;
        this.supportsPlaylistWrite = supportsPlaylistWrite;
    }

    async load(): Promise<void> {
        this.renderLayout();
        await Promise.all([
            this.fetchArtists(true),
            this.fetchAlbums(true),
            this.fetchTracks(true),
        ]);
        this.subscribeBasket();
    }

    remount(): void {
        this.renderLayout();
        this.renderArtistPanel();
        this.renderAlbumPanel();
        this.renderTrackPanel();
        this.restoreScrolls();
        this.subscribeBasket();
    }

    destroy(): void {
        this.teardownScrollHandlers();
        if (this.basketUnsub) {
            this.basketUnsub();
            this.basketUnsub = null;
        }
    }

    // ─── Layout ───────────────────────────────────────────────────────────────

    private renderLayout(): void {
        this.teardownScrollHandlers();
        this.container.innerHTML = `
            <div style="display:flex; flex-direction:column; height:100%; overflow:hidden;">
                <div class="curation-panels">
                    <div id="tracks-artist-panel" class="curation-artist-panel"></div>
                    <div id="tracks-album-panel" class="curation-album-panel"></div>
                </div>
                <div id="tracks-track-panel" class="curation-track-panel"></div>
            </div>
        `;
        this.setupScrollHandlers();
    }

    private setupScrollHandlers(): void {
        const ap = this.container.querySelector<HTMLElement>('#tracks-artist-panel');
        const alp = this.container.querySelector<HTMLElement>('#tracks-album-panel');
        const tp = this.container.querySelector<HTMLElement>('#tracks-track-panel');

        if (ap) {
            this._artistScrollHandler = () => {
                this.artistScrollTop = ap.scrollTop;
                if (!this.artistState.loading && !this.artistState.exhausted &&
                    ap.scrollTop + ap.clientHeight >= ap.scrollHeight - SCROLL_NEAR_BOTTOM_PX) {
                    this.fetchArtists(false);
                }
            };
            ap.addEventListener('scroll', this._artistScrollHandler);
        }
        if (alp) {
            this._albumScrollHandler = () => {
                this.albumScrollTop = alp.scrollTop;
                if (!this.albumState.loading && !this.albumState.exhausted &&
                    alp.scrollTop + alp.clientHeight >= alp.scrollHeight - SCROLL_NEAR_BOTTOM_PX) {
                    this.fetchAlbums(false);
                }
            };
            alp.addEventListener('scroll', this._albumScrollHandler);
        }
        if (tp) {
            this._trackScrollHandler = () => {
                this.trackScrollTop = tp.scrollTop;
                if (!this.trackState.loading && !this.trackState.exhausted &&
                    tp.scrollTop + tp.clientHeight >= tp.scrollHeight - SCROLL_NEAR_BOTTOM_PX) {
                    this.fetchTracks(false);
                }
            };
            tp.addEventListener('scroll', this._trackScrollHandler);
        }
    }

    private teardownScrollHandlers(): void {
        const ap = this.container.querySelector<HTMLElement>('#tracks-artist-panel');
        if (ap && this._artistScrollHandler) ap.removeEventListener('scroll', this._artistScrollHandler);
        const alp = this.container.querySelector<HTMLElement>('#tracks-album-panel');
        if (alp && this._albumScrollHandler) alp.removeEventListener('scroll', this._albumScrollHandler);
        const tp = this.container.querySelector<HTMLElement>('#tracks-track-panel');
        if (tp && this._trackScrollHandler) tp.removeEventListener('scroll', this._trackScrollHandler);
        this._artistScrollHandler = null;
        this._albumScrollHandler = null;
        this._trackScrollHandler = null;
    }

    // ─── Data fetching ────────────────────────────────────────────────────────

    private async fetchArtists(reset: boolean): Promise<void> {
        if (this.artistState.loading) return;
        if (!reset && this.artistState.exhausted) return;

        if (reset) this.artistState = makePanelState();
        this.artistState.loading = true;
        const startIndex = this.artistState.startIndex;

        if (reset) this.renderArtistPanel();
        else this.appendPanelSpinner('#tracks-artist-panel');

        try {
            const result = await fetchBrowseArtists(
                this.artistLetter ?? undefined,
                undefined,
                startIndex,
                ARTIST_LIMIT,
            );
            if (!this.container.isConnected) return;

            const newItems = result.artists;
            this.artistState.items.push(...newItems);
            this.artistState.total = result.total;
            this.artistState.startIndex = this.artistState.items.length;
            this.artistState.exhausted =
                this.artistState.items.length >= result.total || newItems.length < ARTIST_LIMIT;
            this.artistState.loading = false;

            if (reset) {
                this.renderArtistPanel();
            } else {
                this.appendArtistRows(newItems);
            }
        } catch (e) {
            if (!this.container.isConnected) return;
            this.artistState.loading = false;
            this.showPanelError('#tracks-artist-panel', e as Error);
        }
    }

    private async fetchAlbums(reset: boolean): Promise<void> {
        if (this.albumState.loading) return;
        if (!reset && this.albumState.exhausted) return;

        if (reset) this.albumState = makePanelState();
        this.albumState.loading = true;
        const startIndex = this.albumState.startIndex;

        if (reset) this.renderAlbumPanel();
        else this.appendPanelSpinner('#tracks-album-panel');

        try {
            if (this.selectedArtistId !== null) {
                const result = await fetchBrowseArtist(this.selectedArtistId);
                if (!this.container.isConnected) return;
                this.albumState.items = result.albums;
                this.albumState.total = result.albums.length;
                this.albumState.startIndex = result.albums.length;
                this.albumState.exhausted = true;
            } else {
                const result = await fetchBrowseAlbums(
                    this.albumLetter ?? undefined,
                    undefined,
                    startIndex,
                    ALBUM_LIMIT,
                );
                if (!this.container.isConnected) return;
                const newItems = result.albums;
                this.albumState.items.push(...newItems);
                this.albumState.total = result.total;
                this.albumState.startIndex = this.albumState.items.length;
                this.albumState.exhausted =
                    this.albumState.items.length >= result.total || newItems.length < ALBUM_LIMIT;
            }
            this.albumState.loading = false;

            if (reset) {
                this.renderAlbumPanel();
            } else {
                this.appendAlbumRows(this.albumState.items.slice(startIndex));
            }
        } catch (e) {
            if (!this.container.isConnected) return;
            this.albumState.loading = false;
            this.showPanelError('#tracks-album-panel', e as Error);
        }
    }

    private async fetchTracks(reset: boolean): Promise<void> {
        if (this.trackState.loading) return;
        if (!reset && this.trackState.exhausted) return;

        if (reset) this.trackState = makePanelState();
        this.trackState.loading = true;
        const startIndex = this.trackState.startIndex;

        if (reset) this.renderTrackPanel();
        else this.appendPanelSpinner('#tracks-track-panel');

        try {
            const result = await fetchBrowseTracks({
                artistId: this.selectedArtistId ?? undefined,
                albumId: this.selectedAlbumId ?? undefined,
                startIndex,
                limit: TRACK_LIMIT,
            });
            if (!this.container.isConnected) return;

            const newItems = result.tracks;
            this.trackState.items.push(...newItems);
            this.trackState.total = result.total;
            this.trackState.startIndex = this.trackState.items.length;
            // Subsonic unfiltered: total == page length on last page; use page length < limit as secondary signal
            this.trackState.exhausted =
                this.trackState.items.length >= result.total || newItems.length < TRACK_LIMIT;
            this.trackState.loading = false;

            if (reset) {
                this.renderTrackPanel();
            } else {
                this.appendTrackRows(newItems);
            }
        } catch (e) {
            if (!this.container.isConnected) return;
            this.trackState.loading = false;
            this.showPanelError('#tracks-track-panel', e as Error);
        }
    }

    // ─── Panel rendering (full rebuild) ───────────────────────────────────────

    private renderArtistPanel(): void {
        const panel = this.container.querySelector<HTMLElement>('#tracks-artist-panel');
        if (!panel) return;
        panel.innerHTML = '';
        panel.appendChild(this.buildAllArtistsRow());

        if (this.artistState.total >= AZ_THRESHOLD) {
            panel.appendChild(this.buildAzStrip('artist'));
        }
        for (const artist of this.artistState.items) {
            panel.appendChild(this.buildArtistRow(artist));
        }
        if (this.artistState.loading) panel.appendChild(this.buildSpinner());
    }

    private renderAlbumPanel(): void {
        const panel = this.container.querySelector<HTMLElement>('#tracks-album-panel');
        if (!panel) return;
        panel.innerHTML = '';
        panel.appendChild(this.buildAllAlbumsRow());

        if (this.selectedArtistId === null && this.albumState.total >= AZ_THRESHOLD) {
            panel.appendChild(this.buildAzStrip('album'));
        }
        for (const album of this.albumState.items) {
            panel.appendChild(this.buildAlbumRow(album));
        }
        if (this.albumState.loading) panel.appendChild(this.buildSpinner());
    }

    private renderTrackPanel(): void {
        const panel = this.container.querySelector<HTMLElement>('#tracks-track-panel');
        if (!panel) return;
        panel.innerHTML = '';

        if (this.trackState.loading && this.trackState.items.length === 0) {
            panel.appendChild(this.buildSpinner());
            return;
        }
        if (!this.trackState.loading && this.trackState.items.length === 0) {
            const empty = document.createElement('p');
            empty.className = 'curation-empty-state';
            empty.textContent = t('tracks.view.no_tracks');
            panel.appendChild(empty);
            return;
        }
        for (const track of this.trackState.items) {
            panel.appendChild(this.buildTrackRow(track));
        }
        if (this.trackState.loading) panel.appendChild(this.buildSpinner());
    }

    // ─── Append helpers (for autoload-on-scroll) ──────────────────────────────

    private appendArtistRows(artists: BrowseArtist[]): void {
        const panel = this.container.querySelector<HTMLElement>('#tracks-artist-panel');
        if (!panel) return;
        panel.querySelector('.tracks-spinner')?.remove();

        if (this.artistState.total >= AZ_THRESHOLD && !panel.querySelector('.tracks-az-strip')) {
            const allRow = panel.querySelector<HTMLElement>('[data-all-artists]');
            if (allRow) allRow.after(this.buildAzStrip('artist'));
        }
        for (const artist of artists) {
            panel.appendChild(this.buildArtistRow(artist));
        }
        if (this.artistState.loading) panel.appendChild(this.buildSpinner());
    }

    private appendAlbumRows(albums: BrowseAlbum[]): void {
        const panel = this.container.querySelector<HTMLElement>('#tracks-album-panel');
        if (!panel) return;
        panel.querySelector('.tracks-spinner')?.remove();

        if (this.selectedArtistId === null && this.albumState.total >= AZ_THRESHOLD &&
            !panel.querySelector('.tracks-az-strip')) {
            const allRow = panel.querySelector<HTMLElement>('[data-all-albums]');
            if (allRow) allRow.after(this.buildAzStrip('album'));
        }
        for (const album of albums) {
            panel.appendChild(this.buildAlbumRow(album));
        }
        if (this.albumState.loading) panel.appendChild(this.buildSpinner());
    }

    private appendTrackRows(tracks: BrowseTrack[]): void {
        const panel = this.container.querySelector<HTMLElement>('#tracks-track-panel');
        if (!panel) return;
        panel.querySelector('.tracks-spinner')?.remove();
        for (const track of tracks) {
            panel.appendChild(this.buildTrackRow(track));
        }
        if (this.trackState.loading) panel.appendChild(this.buildSpinner());
    }

    // ─── Row builders ─────────────────────────────────────────────────────────

    private buildAllArtistsRow(): HTMLElement {
        const row = document.createElement('div');
        row.className = `curation-artist-row curation-all-artists${this.selectedArtistId === null ? ' curation-selected' : ''}`;
        row.dataset.allArtists = '';
        row.setAttribute('role', 'button');
        row.setAttribute('tabindex', '0');
        row.setAttribute('aria-pressed', this.selectedArtistId === null ? 'true' : 'false');
        row.style.cssText = 'position:sticky; top:0; z-index:5; background:var(--sl-panel-background-color,#1e293b);';
        row.innerHTML = `<span class="curation-row-label curation-row-label--italic">${t('tracks.view.all_artists')}</span>`;
        row.addEventListener('click', () => this.selectArtist(null));
        row.addEventListener('keydown', (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); this.selectArtist(null); } });
        return row;
    }

    private buildAllAlbumsRow(): HTMLElement {
        const row = document.createElement('div');
        row.className = `curation-album-row curation-all-albums${this.selectedAlbumId === null ? ' curation-album-focused' : ''}`;
        row.dataset.allAlbums = '';
        row.setAttribute('role', 'button');
        row.setAttribute('tabindex', '0');
        row.setAttribute('aria-pressed', this.selectedAlbumId === null ? 'true' : 'false');
        row.style.cssText = 'position:sticky; top:0; z-index:5; background:var(--sl-panel-background-color,#1e293b);';
        row.innerHTML = `<span class="curation-row-label curation-row-label--italic">${t('tracks.view.all_albums')}</span>`;
        row.addEventListener('click', () => this.selectAlbum(null));
        row.addEventListener('keydown', (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); this.selectAlbum(null); } });
        return row;
    }

    private buildArtistRow(artist: BrowseArtist): HTMLElement {
        const row = document.createElement('div');
        const isSelected = artist.id === this.selectedArtistId;
        row.className = `curation-artist-row${isSelected ? ' curation-selected' : ''}`;
        row.dataset.artistId = artist.id;
        row.setAttribute('role', 'button');
        row.setAttribute('tabindex', '0');
        row.setAttribute('aria-pressed', isSelected ? 'true' : 'false');
        row.innerHTML = `<span class="curation-row-label" title="${this.escapeAttr(artist.name)}">${this.escapeHtml(artist.name)}</span>`;
        row.addEventListener('click', () => this.selectArtist(artist.id));
        row.addEventListener('keydown', (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); this.selectArtist(artist.id); } });
        return row;
    }

    private buildAlbumRow(album: BrowseAlbum): HTMLElement {
        const row = document.createElement('div');
        const isSelected = album.id === this.selectedAlbumId;
        row.className = `curation-album-row${isSelected ? ' curation-album-focused' : ''}`;
        row.dataset.albumId = album.id;
        row.setAttribute('role', 'button');
        row.setAttribute('tabindex', '0');
        row.setAttribute('aria-pressed', isSelected ? 'true' : 'false');
        row.innerHTML = `<span class="curation-row-label" title="${this.escapeAttr(album.name)}">${this.escapeHtml(album.name)}</span>`;
        row.addEventListener('click', () => this.selectAlbum(album.id));
        row.addEventListener('keydown', (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); this.selectAlbum(album.id); } });
        return row;
    }

    private buildTrackRow(track: BrowseTrack): HTMLElement {
        const row = document.createElement('div');
        row.className = 'curation-track-row';
        row.dataset.trackId = track.id;
        row.setAttribute('tabindex', '0');

        const info = document.createElement('div');
        info.style.cssText = 'flex:1; min-width:0; display:flex; flex-direction:column;';

        const title = document.createElement('span');
        title.className = 'curation-row-label';
        title.style.fontWeight = 'var(--sl-font-weight-semibold, 600)';
        title.textContent = track.title;
        title.title = track.title;

        const meta = document.createElement('span');
        meta.style.cssText = 'font-size:0.75rem; opacity:0.7; overflow:hidden; text-overflow:ellipsis; white-space:nowrap;';
        meta.textContent = [track.artistName, track.albumName].filter(Boolean).join(' · ');

        info.appendChild(title);
        info.appendChild(meta);
        row.appendChild(info);

        const isInBasket = basketStore.has(track.id);
        const toggleBtn = document.createElement('sl-icon-button') as any;
        toggleBtn.name = isInBasket ? 'dash-circle-fill' : 'plus-circle-fill';
        toggleBtn.label = isInBasket ? 'Remove from basket' : 'Add to basket';
        toggleBtn.style.fontSize = '1.1rem';
        toggleBtn.dataset.basketToggle = track.id;
        toggleBtn.addEventListener('click', (e: Event) => {
            e.stopPropagation();
            if (basketStore.has(track.id)) {
                basketStore.remove(track.id);
            } else {
                basketStore.add({
                    id: track.id,
                    name: track.title,
                    type: 'Audio',
                    artist: track.artistName,
                    childCount: 1,
                    sizeBytes: track.sizeBytes ?? 0,
                    sizeTicks: (track.duration ?? 0) * 10_000_000,
                });
            }
        });
        row.appendChild(toggleBtn);

        if (this.supportsPlaylistWrite) {
            const playlistBtn = document.createElement('sl-icon-button') as any;
            playlistBtn.name = 'collection-play';
            playlistBtn.label = t('tracks.view.send_to_playlist');
            playlistBtn.style.fontSize = '1.1rem';
            playlistBtn.addEventListener('click', (e: Event) => {
                e.stopPropagation();
                MediaCard.openAddToPlaylistDialog(track.id, track.title);
            });
            row.appendChild(playlistBtn);

            row.addEventListener('contextmenu', (e: MouseEvent) => {
                e.preventDefault();
                MediaCard.showItemContextMenu(e.clientX, e.clientY, track.id, track.title);
            });
        }

        return row;
    }

    // ─── Basket button refresh on store update ─────────────────────────────────

    private updateTrackButtons(): void {
        const panel = this.container.querySelector<HTMLElement>('#tracks-track-panel');
        if (!panel) return;
        panel.querySelectorAll<HTMLElement>('[data-basket-toggle]').forEach(btn => {
            const trackId = (btn as any).dataset.basketToggle;
            const isInBasket = basketStore.has(trackId);
            (btn as any).name = isInBasket ? 'dash-circle-fill' : 'plus-circle-fill';
            (btn as any).label = isInBasket ? 'Remove from basket' : 'Add to basket';
        });
    }

    // ─── Selection handlers ───────────────────────────────────────────────────

    private async selectArtist(artistId: string | null): Promise<void> {
        if (this.selectedArtistId === artistId) return;
        this.selectedArtistId = artistId;
        this.selectedAlbumId = null;
        this.albumLetter = null;

        const panel = this.container.querySelector('#tracks-artist-panel');
        if (panel) {
            panel.querySelectorAll<HTMLElement>('.curation-artist-row').forEach(row => {
                const isAll = 'allArtists' in row.dataset;
                const isSelected = isAll ? artistId === null : row.dataset.artistId === artistId;
                row.classList.toggle('curation-selected', isSelected);
                row.setAttribute('aria-pressed', isSelected ? 'true' : 'false');
            });
        }

        await Promise.all([this.fetchAlbums(true), this.fetchTracks(true)]);
    }

    private async selectAlbum(albumId: string | null): Promise<void> {
        if (this.selectedAlbumId === albumId) return;
        this.selectedAlbumId = albumId;

        const panel = this.container.querySelector('#tracks-album-panel');
        if (panel) {
            panel.querySelectorAll<HTMLElement>('.curation-album-row').forEach(row => {
                const isAll = 'allAlbums' in row.dataset;
                const isSelected = isAll ? albumId === null : row.dataset.albumId === albumId;
                row.classList.toggle('curation-album-focused', isSelected);
                row.setAttribute('aria-pressed', isSelected ? 'true' : 'false');
            });
        }

        await this.fetchTracks(true);
    }

    private async setArtistLetter(letter: string): Promise<void> {
        const newLetter = this.artistLetter === letter ? null : letter;
        this.artistLetter = newLetter;
        this.selectedArtistId = null;
        this.selectedAlbumId = null;
        await Promise.all([this.fetchArtists(true), this.fetchAlbums(true), this.fetchTracks(true)]);
    }

    private async setAlbumLetter(letter: string): Promise<void> {
        if (this.selectedArtistId !== null) return;
        const newLetter = this.albumLetter === letter ? null : letter;
        this.albumLetter = newLetter;
        this.selectedAlbumId = null;
        await Promise.all([this.fetchAlbums(true), this.fetchTracks(true)]);
    }

    // ─── A-Z strip ────────────────────────────────────────────────────────────

    private buildAzStrip(type: 'artist' | 'album'): HTMLElement {
        const strip = document.createElement('div');
        strip.className = 'tracks-az-strip';
        strip.style.cssText = 'display:flex; flex-wrap:wrap; gap:0.1rem; padding:0.2rem; border-bottom:1px solid var(--surface-border-soft); background:var(--surface-fill); position:sticky; top:2.25rem; z-index:4;';

        const activeLetter = type === 'artist' ? this.artistLetter : this.albumLetter;
        for (const letter of ALL_LETTERS) {
            const btn = document.createElement('sl-button') as any;
            btn.size = 'small';
            btn.variant = letter === activeLetter ? 'primary' : 'text';
            btn.textContent = letter;
            btn.addEventListener('click', () => {
                if (type === 'artist') this.setArtistLetter(letter);
                else this.setAlbumLetter(letter);
            });
            strip.appendChild(btn);
        }
        return strip;
    }

    // ─── Utilities ────────────────────────────────────────────────────────────

    private buildSpinner(): HTMLElement {
        const el = document.createElement('div');
        el.className = 'tracks-spinner';
        el.style.cssText = 'text-align:center; padding:0.75rem;';
        el.innerHTML = '<sl-spinner></sl-spinner>';
        return el;
    }

    private appendPanelSpinner(selector: string): void {
        const panel = this.container.querySelector<HTMLElement>(selector);
        if (!panel || panel.querySelector('.tracks-spinner')) return;
        panel.appendChild(this.buildSpinner());
    }

    private showPanelError(selector: string, err: Error): void {
        const panel = this.container.querySelector<HTMLElement>(selector);
        if (!panel) return;
        const alert = document.createElement('sl-alert') as any;
        alert.variant = 'danger';
        alert.open = true;
        alert.style.margin = '0.5rem';
        alert.textContent = err.message;
        panel.appendChild(alert);
    }

    private subscribeBasket(): void {
        if (this.basketUnsub) this.basketUnsub();
        const handler = () => this.updateTrackButtons();
        basketStore.addEventListener('update', handler);
        this.basketUnsub = () => basketStore.removeEventListener('update', handler);
    }

    private restoreScrolls(): void {
        const ap = this.container.querySelector<HTMLElement>('#tracks-artist-panel');
        if (ap) ap.scrollTop = this.artistScrollTop;
        const alp = this.container.querySelector<HTMLElement>('#tracks-album-panel');
        if (alp) alp.scrollTop = this.albumScrollTop;
        const tp = this.container.querySelector<HTMLElement>('#tracks-track-panel');
        if (tp) tp.scrollTop = this.trackScrollTop;
    }

    private escapeHtml(s: string): string {
        return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
    }

    private escapeAttr(s: string): string {
        return s.replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
    }
}
