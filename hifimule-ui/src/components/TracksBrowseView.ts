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
import { basketStore, BasketItem } from '../state/basket';
import { showToast } from '../toast';
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
    errored: boolean;
}

function makePanelState<T>(): PanelState<T> {
    return { items: [], total: 0, startIndex: 0, loading: false, exhausted: false, errored: false };
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

    // Per-panel request generation. Bumped on every fetch; a resolved request
    // whose captured generation no longer matches is stale and is discarded,
    // so a reset (selection/letter change) always supersedes an in-flight load.
    private artistGen = 0;
    private albumGen = 0;
    private trackGen = 0;

    private artistScrollTop = 0;
    private albumScrollTop = 0;
    private trackScrollTop = 0;

    private basketUnsub: (() => void) | null = null;
    private _artistScrollHandler: ((e: Event) => void) | null = null;
    private _albumScrollHandler: ((e: Event) => void) | null = null;
    private _trackScrollHandler: ((e: Event) => void) | null = null;

    // Track multi-selection (Story 9.12). Keyed by track.id (the same id
    // basketStore.has uses). The anchor indexes into trackState.items — valid
    // across autoload because fetchTracks only ever appends until a reset.
    private selectedTrackIds: Set<string> = new Set();
    private selectionAnchorIdx: number | null = null;
    private _escapeHandler: ((e: KeyboardEvent) => void) | null = null;

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
        this.ensureEscapeListener();
    }

    remount(): void {
        this.renderLayout();
        this.renderArtistPanel();
        this.renderAlbumPanel();
        this.renderTrackPanel();
        this.restoreScrolls();
        this.subscribeBasket();
        this.ensureEscapeListener();
    }

    destroy(): void {
        // The instance is cached module-level and remounted on re-entry: clear
        // the selection here or it resurrects when the user returns (AC 10),
        // and remove the document-level Escape listener so it doesn't leak.
        this.clearSelection();
        this.removeEscapeListener();
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
                if (!this.artistState.loading && !this.artistState.exhausted && !this.artistState.errored &&
                    as.scrollTop + as.clientHeight >= as.scrollHeight - SCROLL_NEAR_BOTTOM_PX) {
                    this.fetchArtists(false);
                }
            };
            as.addEventListener('scroll', this._artistScrollHandler);
        }
        if (als) {
            this._albumScrollHandler = () => {
                this.albumScrollTop = als.scrollTop;
                if (!this.albumState.loading && !this.albumState.exhausted && !this.albumState.errored &&
                    als.scrollTop + als.clientHeight >= als.scrollHeight - SCROLL_NEAR_BOTTOM_PX) {
                    this.fetchAlbums(false);
                }
            };
            als.addEventListener('scroll', this._albumScrollHandler);
        }
        if (tp) {
            this._trackScrollHandler = () => {
                this.trackScrollTop = tp.scrollTop;
                if (!this.trackState.loading && !this.trackState.exhausted && !this.trackState.errored &&
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
        // Autoload is skipped while loading, exhausted, or in an error state; a
        // reset always proceeds and supersedes any in-flight load via the gen token.
        if (!reset && (this.artistState.loading || this.artistState.exhausted || this.artistState.errored)) return;

        if (reset) this.artistState = makePanelState();
        const gen = ++this.artistGen;
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
            if (!this.container.isConnected || gen !== this.artistGen) return;

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
            if (!this.container.isConnected || gen !== this.artistGen) return;
            this.artistState.loading = false;
            this.artistState.errored = true;
            this.showPanelError(ARTIST_SCROLL, e as Error);
        }
    }

    private async fetchAlbums(reset: boolean): Promise<void> {
        if (!reset && (this.albumState.loading || this.albumState.exhausted || this.albumState.errored)) return;

        if (reset) this.albumState = makePanelState();
        const gen = ++this.albumGen;
        this.albumState.loading = true;
        const startIndex = this.albumState.startIndex;

        if (reset) this.renderAlbumPanel();
        else this.appendPanelSpinner(ALBUM_SCROLL);

        try {
            if (this.selectedArtistId !== null) {
                const result = await fetchBrowseArtist(this.selectedArtistId);
                if (!this.container.isConnected || gen !== this.albumGen) return;
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
                if (!this.container.isConnected || gen !== this.albumGen) return;
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
            if (!this.container.isConnected || gen !== this.albumGen) return;
            this.albumState.loading = false;
            this.albumState.errored = true;
            this.showPanelError(ALBUM_SCROLL, e as Error);
        }
    }

    private async fetchTracks(reset: boolean): Promise<void> {
        if (!reset && (this.trackState.loading || this.trackState.exhausted || this.trackState.errored)) return;

        // Every filter change (artist, album, either A-Z letter) funnels through
        // a reset — the single choke point that clears the selection (AC 10).
        if (reset) this.clearSelection();
        if (reset) this.trackState = makePanelState();
        const gen = ++this.trackGen;
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
            if (!this.container.isConnected || gen !== this.trackGen) return;

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
            if (!this.container.isConnected || gen !== this.trackGen) return;
            this.trackState.loading = false;
            this.trackState.errored = true;
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
        this.trackState.items.forEach((track, i) => {
            panel.appendChild(this.buildTrackRow(track, i));
        });
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
        // Called after the new items were pushed onto trackState.items, so the
        // first appended row sits at (length - tracks.length).
        const offset = this.trackState.items.length - tracks.length;
        tracks.forEach((track, i) => {
            panel.appendChild(this.buildTrackRow(track, offset + i));
        });
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

    private buildTrackRow(track: BrowseTrack, index: number): HTMLElement {
        const row = document.createElement('div');
        row.className = 'curation-track-row';
        if (this.selectedTrackIds.has(track.id)) row.classList.add('is-checked');
        row.dataset.trackId = track.id;
        row.setAttribute('tabindex', '0');

        // Leading multi-select checkbox (Story 9.12). Native input: focus and
        // Space-toggle semantics come for free (AC 11).
        const check = document.createElement('input');
        check.type = 'checkbox';
        check.className = 'media-list-row__check';
        check.checked = this.selectedTrackIds.has(track.id);
        check.setAttribute('aria-label', track.title);
        check.addEventListener('click', (e) => {
            e.stopPropagation();
            this.toggleTrackSelection(track, index, row);
        });
        row.appendChild(check);

        // Ctrl/Cmd-click toggles, Shift-click range-selects from the anchor;
        // plain click stays a no-op (track rows have no navigation). The
        // per-row buttons all stopPropagation, so they never reach this.
        row.addEventListener('click', (e) => {
            if (e.ctrlKey || e.metaKey) {
                this.toggleTrackSelection(track, index, row);
                return;
            }
            if (e.shiftKey && this.selectionAnchorIdx !== null) {
                this.selectTrackRange(this.selectionAnchorIdx, index);
            }
        });
        // Shift-click extends the selection — suppress the browser's text-range
        // selection artifact for that gesture only.
        row.addEventListener('mousedown', (e) => {
            if (e.shiftKey && this.selectionAnchorIdx !== null) e.preventDefault();
        });

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
        toggleBtn.label = isInBasket ? t('tracks.view.remove_from_basket') : t('tracks.view.add_to_basket');
        toggleBtn.style.fontSize = '1.1rem';
        toggleBtn.dataset.basketToggle = track.id;
        // AC 9: (+) controls render disabled when no device (server) is selected.
        toggleBtn.disabled = !basketStore.getActiveServerId();
        toggleBtn.addEventListener('click', (e: Event) => {
            e.stopPropagation();
            if (basketStore.has(track.id)) {
                basketStore.remove(track.id);
            } else {
                basketStore.add(this.trackToBasketItem(track));
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
                MediaCard.openAddToPlaylistDialog([track.id], track.title);
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
        const noDevice = !basketStore.getActiveServerId();
        panel.querySelectorAll<HTMLElement>('[data-basket-toggle]').forEach(btn => {
            const trackId = (btn as any).dataset.basketToggle;
            const isInBasket = basketStore.has(trackId);
            (btn as any).name = isInBasket ? 'dash-circle-fill' : 'plus-circle-fill';
            (btn as any).label = isInBasket ? t('tracks.view.remove_from_basket') : t('tracks.view.add_to_basket');
            (btn as any).disabled = noDevice;
        });
    }

    // ─── Track multi-selection (Story 9.12) ───────────────────────────────────

    // Shared BrowseTrack → basket-item mapping, used by both the per-row (+)
    // handler and the bulk "Add to basket" action.
    private trackToBasketItem(track: BrowseTrack): BasketItem {
        return {
            id: track.id,
            name: track.title,
            type: 'Audio',
            artist: track.artistName,
            childCount: 1,
            sizeBytes: track.sizeBytes ?? 0,
            sizeTicks: (track.duration ?? 0) * 10_000_000,
        };
    }

    private clearSelection(): void {
        if (this.selectedTrackIds.size === 0 && this.selectionAnchorIdx === null) return;
        this.selectedTrackIds = new Set();
        this.selectionAnchorIdx = null;
        const panel = this.container.querySelector<HTMLElement>(TRACK_PANEL);
        if (panel) {
            panel.classList.remove('has-selection');
            panel.querySelectorAll<HTMLElement>('.curation-track-row.is-checked').forEach(row => {
                row.classList.remove('is-checked');
                const check = row.querySelector<HTMLInputElement>('.media-list-row__check');
                if (check) check.checked = false;
            });
        }
        this.updateBulkBar();
    }

    // Cheap single-row toggle: updates only the clicked row's class/checkbox
    // and the bulk bar — no full repaint (toggles can happen rapidly).
    private toggleTrackSelection(track: BrowseTrack, index: number, row: HTMLElement): void {
        if (this.selectedTrackIds.has(track.id)) {
            this.selectedTrackIds.delete(track.id);
        } else {
            this.selectedTrackIds.add(track.id);
        }
        this.selectionAnchorIdx = index;
        const checked = this.selectedTrackIds.has(track.id);
        row.classList.toggle('is-checked', checked);
        const check = row.querySelector<HTMLInputElement>('.media-list-row__check');
        if (check) check.checked = checked;
        const panel = this.container.querySelector<HTMLElement>(TRACK_PANEL);
        if (panel) panel.classList.toggle('has-selection', this.selectedTrackIds.size > 0);
        this.updateBulkBar();
    }

    // Shift-range: indices come from trackState.items, never the DOM. The
    // anchor stays put. Every track row is selectable, so no type filter.
    private selectTrackRange(fromIdx: number, toIdx: number): void {
        const [lo, hi] = fromIdx <= toIdx ? [fromIdx, toIdx] : [toIdx, fromIdx];
        for (let i = lo; i <= hi; i++) {
            const it = this.trackState.items[i];
            if (it) this.selectedTrackIds.add(it.id);
        }
        this.refreshRenderedSelection();
    }

    // Rows are plain appended divs (never unmounted between rebuilds), so after
    // a range change sync every rendered row's checked state from the set.
    private refreshRenderedSelection(): void {
        const panel = this.container.querySelector<HTMLElement>(TRACK_PANEL);
        if (panel) {
            panel.classList.toggle('has-selection', this.selectedTrackIds.size > 0);
            panel.querySelectorAll<HTMLElement>('.curation-track-row').forEach(row => {
                const checked = !!row.dataset.trackId && this.selectedTrackIds.has(row.dataset.trackId);
                row.classList.toggle('is-checked', checked);
                const check = row.querySelector<HTMLInputElement>('.media-list-row__check');
                if (check) check.checked = checked;
            });
        }
        this.updateBulkBar();
    }

    // Document-level Escape clears the selection (AC 10). Capture phase so it
    // runs before the context menu's own capture handler removes the menu — an
    // open menu/dialog swallows the Escape and the selection survives.
    // Registered on load/remount, removed in destroy (the instance is cached
    // module-level, so a leaked listener would outlive the mode).
    private ensureEscapeListener(): void {
        if (this._escapeHandler) return;
        this._escapeHandler = (e: KeyboardEvent) => {
            if (e.key !== 'Escape' || this.selectedTrackIds.size === 0) return;
            if (document.querySelector('sl-dialog[open]')) return;
            // Match the menu regardless of `.is-open`: the class is added one
            // frame after the element mounts, so guarding on it would miss an
            // Escape pressed in that opening frame. The menu element only
            // exists in the DOM while open, so the bare selector is safe.
            if (document.querySelector('.hm-context-menu')) return;
            this.clearSelection();
        };
        document.addEventListener('keydown', this._escapeHandler, true);
    }

    private removeEscapeListener(): void {
        if (!this._escapeHandler) return;
        document.removeEventListener('keydown', this._escapeHandler, true);
        this._escapeHandler = null;
    }

    private renderBulkBar(): HTMLElement {
        const bar = document.createElement('div');
        bar.className = 'bulk-action-bar';

        const count = document.createElement('span');
        count.className = 'bulk-action-bar__count';
        count.setAttribute('aria-live', 'polite');
        count.textContent = t('library.selection.count', { count: this.selectedTrackIds.size });
        bar.appendChild(count);

        const addBtn = document.createElement('sl-button') as any;
        addBtn.size = 'small';
        addBtn.variant = 'primary';
        // basket-toggle-btn: the view mounts inside #library-content, so the
        // existing `#library-content.device-locked` CSS rule disables it when
        // no device is selected — same mechanism as the 9.11 bulk bar.
        addBtn.classList.add('basket-toggle-btn');
        addBtn.textContent = t('library.selection.add_to_basket');
        addBtn.addEventListener('click', () => this.bulkAddToBasket());
        bar.appendChild(addBtn);

        if (this.supportsPlaylistWrite) {
            const plBtn = document.createElement('sl-button') as any;
            plBtn.size = 'small';
            plBtn.textContent = t('library.selection.add_to_playlist');
            plBtn.addEventListener('click', () => this.bulkAddToPlaylist());
            bar.appendChild(plBtn);
        }

        const clearBtn = document.createElement('sl-button') as any;
        clearBtn.size = 'small';
        clearBtn.variant = 'text';
        clearBtn.textContent = t('library.selection.clear');
        clearBtn.addEventListener('click', () => this.clearSelection());
        bar.appendChild(clearBtn);

        return bar;
    }

    // Create the bar when the selection goes 0→1 (inserted directly above the
    // track panel), update the count in place while it lives, remove it on →0.
    private updateBulkBar(): void {
        const existing = this.container.querySelector<HTMLElement>('.bulk-action-bar');
        if (this.selectedTrackIds.size === 0) {
            existing?.remove();
            return;
        }
        if (existing) {
            const count = existing.querySelector('.bulk-action-bar__count');
            if (count) count.textContent = t('library.selection.count', { count: this.selectedTrackIds.size });
            return;
        }
        const panel = this.container.querySelector<HTMLElement>(TRACK_PANEL);
        if (!panel || !panel.parentElement) return;
        const bar = this.renderBulkBar();
        panel.parentElement.insertBefore(bar, panel);
        // An aria-live region only announces mutations made after it is
        // connected. renderBulkBar populates the count before insertion
        // (silent), so re-assert it on the next frame to announce the first
        // (0→1) selection too (AC 11).
        const count = bar.querySelector<HTMLElement>('.bulk-action-bar__count');
        if (count) {
            count.textContent = '';
            requestAnimationFrame(() => {
                count.textContent = t('library.selection.count', { count: this.selectedTrackIds.size });
            });
        }
    }

    // Resolve in trackState.items order (never iterate the Set) so playlist
    // insertion order is deterministic and matches the visible list.
    private resolveSelectedTracks(): BrowseTrack[] {
        return this.trackState.items.filter(tr => this.selectedTrackIds.has(tr.id));
    }

    // All local — no RPC, no loading state. basketStore.add itself toasts and
    // returns early when no active server (belt-and-braces under the CSS gate).
    private bulkAddToBasket(): void {
        const selected = this.resolveSelectedTracks();
        if (selected.length === 0) return;
        let added = 0;
        let skipped = 0;
        for (const track of selected) {
            if (basketStore.has(track.id)) {
                skipped++;
            } else {
                basketStore.add(this.trackToBasketItem(track));
                added++;
            }
        }
        let msg = t('library.selection.added_toast', { added });
        if (skipped > 0) msg += t('library.selection.skipped_suffix', { skipped });
        showToast(msg, 'success');
        this.clearSelection();
    }

    private bulkAddToPlaylist(): void {
        const selected = this.resolveSelectedTracks();
        if (selected.length === 0) return;
        // Selection clears only on success; cancelling the dialog keeps it.
        // The label is forwarded as the "New playlist" suggested name, so pass
        // the generic localized default rather than the "N selected" string.
        MediaCard.openAddToPlaylistDialog(
            selected.map(tr => tr.id),
            t('library.selection.new_playlist_name'),
            () => this.clearSelection()
        );
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
        this.albumLetter = null;
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

        const activeLetter = type === 'artist' ? this.artistLetter : this.albumLetter;
        const total = type === 'artist' ? this.artistState.total : this.albumState.total;
        // Keep the strip when a letter filter is active even if the filtered total
        // is below threshold, so the user can always clear or change the letter.
        const showStrip = (total >= AZ_THRESHOLD || activeLetter !== null) &&
            (type === 'artist' || this.selectedArtistId === null);

        if (!showStrip) {
            azEl.style.display = 'none';
            return;
        }

        azEl.style.display = 'grid';
        azEl.style.gridTemplateColumns = '1fr 1fr';
        azEl.innerHTML = '';
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
        panel.querySelector('.tracks-spinner')?.remove();
        panel.querySelector('.tracks-error')?.remove();
        const alert = document.createElement('sl-alert') as any;
        alert.className = 'tracks-error';
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
