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

// Artist/album panels split into an inner scroll area (#tracks-*-scroll) and a
// vertical A-Z strip sidebar (#tracks-*-az).  All content queries target the
// inner scroll div; the outer panel div is only a flex-row shell.
const ARTIST_SCROLL = '#tracks-artist-scroll';
const ALBUM_SCROLL  = '#tracks-album-scroll';
const ARTIST_AZ     = '#tracks-artist-az';
const ALBUM_AZ      = '#tracks-album-az';
const TRACK_PANEL   = '#tracks-track-panel';

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
        // Each left/right panel is a flex-row shell: scrollable content area + vertical A-Z strip.
        // The outer div keeps the curation-* class for sizing; overflow is handled by the inner scroll div.
        this.container.innerHTML = `
            <div style="display:flex; flex-direction:column; height:100%; overflow:hidden;">
                <div class="curation-panels">
                    <div id="tracks-artist-panel" class="curation-artist-panel"
                         style="display:flex; flex-direction:row; overflow:hidden; padding:0;">
                        <div id="tracks-artist-scroll" style="flex:1; overflow-y:auto; padding:0.5rem 0; min-width:0;"></div>
                        <div id="tracks-artist-az"
                             style="display:none; width:2.5rem; overflow-y:auto;
                                    border-left:1px solid var(--surface-border-soft);
                                    background:var(--surface-fill); flex-shrink:0;"></div>
                    </div>
                    <div id="tracks-album-panel" class="curation-album-panel"
                         style="display:flex; flex-direction:row; overflow:hidden; padding:0;">
                        <div id="tracks-album-scroll" style="flex:1; overflow-y:auto; padding:0.5rem 0; min-width:0;"></div>
                        <div id="tracks-album-az"
                             style="display:none; width:2.5rem; overflow-y:auto;
                                    border-left:1px solid var(--surface-border-soft);
                                    background:var(--surface-fill); flex-shrink:0;"></div>
                    </div>
                </div>
                <div id="tracks-track-panel" class="curation-track-panel"
                     style="flex:0 0 55%; min-height:0; max-height:none;"></div>
            </div>
        `;
        this.setupScrollHandlers();
    }

    private setupScrollHandlers(): void {
        const as = this.container.querySelector<HTMLElement>(ARTIST_SCROLL);
        const als = this.container.querySelector<HTMLElement>(ALBUM_SCROLL);
        const tp = this.container.querySelector<HTMLElement>(TRACK_PANEL);

        if (as) {
            this._artistScrollHandler = () => {
                this.artistScrollTop = as.scrollTop;
                if (!this.artistState.loading && !this.artistState.exhausted &&
                    as.scrollTop + as.clientHeight >= as.scrollHeight - SCROLL_NEAR_BOTTOM_PX) {
                    this.fetchArtists(false);
                }
            };
            as.addEventListener('scroll', this._artistScrollHandler);
        }
        if (als) {
            this._albumScrollHandler = () => {
                this.albumScrollTop = als.scrollTop;
                if (!this.albumState.loading && !this.albumState.exhausted &&
                    als.scrollTop + als.clientHeight >= als.scrollHeight - SCROLL_NEAR_BOTTOM_PX) {
                    this.fetchAlbums(false);
                }
            };
            als.addEventListener('scroll', this._albumScrollHandler);
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
        const as = this.container.querySelector<HTMLElement>(ARTIST_SCROLL);
        if (as && this._artistScrollHandler) as.removeEventListener('scroll', this._artistScrollHandler);
        const als = this.container.querySelector<HTMLElement>(ALBUM_SCROLL);
        if (als && this._albumScrollHandler) als.removeEventListener('scroll', this._albumScrollHandler);
        const tp = this.container.querySelector<HTMLElement>(TRACK_PANEL);
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
        else this.appendPanelSpinner(ARTIST_SCROLL);

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
            this.showPanelError(ARTIST_SCROLL, e as Error);
        }
    }

    private async fetchAlbums(reset: boolean): Promise<void> {
        if (this.albumState.loading) return;
        if (!reset && this.albumState.exhausted) return;

        if (reset) this.albumState = makePanelState();
        this.albumState.loading = true;
        const startIndex = this.albumState.startIndex;

        if (reset) this.renderAlbumPanel();
        else this.appendPanelSpinner(ALBUM_SCROLL);

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
            this.showPanelError(ALBUM_SCROLL, e as Error);
        }
    }

    private async fetchTracks(reset: boolean): Promise<void> {
        if (this.trackState.loading) return;
        if (!reset && this.trackState.exhausted) return;

        if (reset) this.trackState = makePanelState();
        this.trackState.loading = true;
        const startIndex = this.trackState.startIndex;

        if (reset) this.renderTrackPanel();
        else this.appendPanelSpinner(TRACK_PANEL);

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
            this.showPanelError(TRACK_PANEL, e as Error);
        }
    }

    // ─── Panel rendering (full rebuild) ───────────────────────────────────────

    private renderArtistPanel(): void {
        const scroll = this.container.querySelector<HTMLElement>(ARTIST_SCROLL);
        if (!scroll) return;
        scroll.innerHTML = '';
        scroll.appendChild(this.buildAllArtistsRow());
        for (const artist of this.artistState.items) {
            scroll.appendChild(this.buildArtistRow(artist));
        }
        if (this.artistState.loading) scroll.appendChild(this.buildSpinner());
        this.syncAzStrip('artist');
    }

    private renderAlbumPanel(): void {
        const scroll = this.container.querySelector<HTMLElement>(ALBUM_SCROLL);
        if (!scroll) return;
        scroll.innerHTML = '';
        scroll.appendChild(this.buildAllAlbumsRow());
        for (const album of this.albumState.items) {
            scroll.appendChild(this.buildAlbumRow(album));
        }
        if (this.albumState.loading) scroll.appendChild(this.buildSpinner());
        this.syncAzStrip('album');
    }

    private renderTrackPanel(): void {
        const panel = this.container.querySelector<HTMLElement>(TRACK_PANEL);
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
        const scroll = this.container.querySelector<HTMLElement>(ARTIST_SCROLL);
        if (!scroll) return;
        scroll.querySelector('.tracks-spinner')?.remove();
        for (const artist of artists) {
            scroll.appendChild(this.buildArtistRow(artist));
        }
        if (this.artistState.loading) scroll.appendChild(this.buildSpinner());
        this.syncAzStrip('artist');
    }

    private appendAlbumRows(albums: BrowseAlbum[]): void {
        const scroll = this.container.querySelector<HTMLElement>(ALBUM_SCROLL);
        if (!scroll) return;
        scroll.querySelector('.tracks-spinner')?.remove();
        for (const album of albums) {
            scroll.appendChild(this.buildAlbumRow(album));
        }
        if (this.albumState.loading) scroll.appendChild(this.buildSpinner());
        this.syncAzStrip('album');
    }

    private appendTrackRows(tracks: BrowseTrack[]): void {
        const panel = this.container.querySelector<HTMLElement>(TRACK_PANEL);
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
        const panel = this.container.querySelector<HTMLElement>(TRACK_PANEL);
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

        const scroll = this.container.querySelector(ARTIST_SCROLL);
        if (scroll) {
            scroll.querySelectorAll<HTMLElement>('.curation-artist-row').forEach(row => {
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

        const scroll = this.container.querySelector(ALBUM_SCROLL);
        if (scroll) {
            scroll.querySelectorAll<HTMLElement>('.curation-album-row').forEach(row => {
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

    // ─── Vertical A-Z strip ───────────────────────────────────────────────────

    // Show/hide and repopulate the sidebar strip based on current state.
    private syncAzStrip(type: 'artist' | 'album'): void {
        const azEl = this.container.querySelector<HTMLElement>(
            type === 'artist' ? ARTIST_AZ : ALBUM_AZ,
        );
        if (!azEl) return;

        const total = type === 'artist' ? this.artistState.total : this.albumState.total;
        const showStrip = total >= AZ_THRESHOLD &&
            (type === 'artist' || this.selectedArtistId === null);

        if (!showStrip) {
            azEl.style.display = 'none';
            return;
        }

        azEl.style.display = 'grid';
        azEl.style.gridTemplateColumns = '1fr 1fr';
        azEl.innerHTML = '';

        const activeLetter = type === 'artist' ? this.artistLetter : this.albumLetter;
        for (const letter of ALL_LETTERS) {
            const btn = document.createElement('button');
            btn.textContent = letter;
            const isActive = letter === activeLetter;
            btn.style.cssText = [
                'width:100%',
                'padding:0.15rem 0',
                'background:none',
                'border:none',
                'cursor:pointer',
                'font-size:0.55rem',
                'font-weight:600',
                'text-align:center',
                'line-height:1',
                isActive
                    ? 'color:var(--sl-color-primary-500)'
                    : 'color:var(--ink-dim,rgba(255,255,255,0.45))',
            ].join(';');
            btn.addEventListener('click', () => {
                if (type === 'artist') this.setArtistLetter(letter);
                else this.setAlbumLetter(letter);
            });
            azEl.appendChild(btn);
        }
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
        const as = this.container.querySelector<HTMLElement>(ARTIST_SCROLL);
        if (as) as.scrollTop = this.artistScrollTop;
        const als = this.container.querySelector<HTMLElement>(ALBUM_SCROLL);
        if (als) als.scrollTop = this.albumScrollTop;
        const tp = this.container.querySelector<HTMLElement>(TRACK_PANEL);
        if (tp) tp.scrollTop = this.trackScrollTop;
    }

    private escapeHtml(s: string): string {
        return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
    }

    private escapeAttr(s: string): string {
        return s.replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
    }
}
