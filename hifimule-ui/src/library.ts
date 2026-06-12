import {
    BrowseMode,
    BrowseArtist,
    BrowseAlbum,
    BrowsePlaylist,
    BrowseTrack,
    BrowseGenre,
    fetchBrowseModes,
    fetchBrowseArtists,
    fetchBrowseArtist,
    fetchBrowseAlbums,
    fetchBrowseAlbum,
    fetchBrowsePlaylists,
    fetchBrowsePlaylist,
    fetchBrowseGenres,
    fetchBrowseGenre,
    fetchBrowseRecentlyAdded,
    fetchBrowseFrequentlyPlayed,
    fetchBrowseRecentlyPlayed,
    fetchBrowseFavorites,
    fetchBrowseFavoriteItems,
    getImageUrl,
    rpcCall,
} from './rpc';
import { MediaCard, BrowseDisplayItem } from './components/MediaCard';
import { PlaylistCurationView } from './components/PlaylistCurationView';
import { TracksBrowseView } from './components/TracksBrowseView';
import { basketStore } from './state/basket';
import { t } from './i18n';
import { showToast, ERROR_TOAST_DURATION } from './toast';

let _supportsPlaylistWrite = false;
export function setPlaylistWriteCapability(v: boolean): void {
    _supportsPlaylistWrite = v;
}

function modeLabel(mode: BrowseMode): string {
    return t(`library.mode.${mode}`);
}

const VIRTUAL_ROW_HEIGHT = 56;
const OVERSCAN = 3;

interface AppState {
    browseMode: BrowseMode;
    availableModes: BrowseMode[];
    parentId?: string;
    breadcrumbStack: { id: string; name: string }[];
    items: BrowseDisplayItem[];
    pagination: { startIndex: number; limit: number; total: number };
    loading: boolean;
    listLoading: boolean;
    scrollCache: Map<string, number>;
    pageCache: Map<string, { items: BrowseDisplayItem[]; total: number }>;
    artistViewTotal: number;
    albumViewTotal: number;
    activeLetter: string | null;
    favoriteTree: FavoriteTree | null;
    listViewMode: 'grid' | 'list';
    selectedIds: Set<string>;
    selectionAnchorIdx: number | null;
}

interface FavoriteTree {
    artists: BrowseArtist[];
    albums: BrowseAlbum[];
    tracks: BrowseTrack[];
    favoriteArtistIds: Set<string>;
    favoriteAlbumIds: Set<string>;
}

let state: AppState = {
    browseMode: 'artists',
    availableModes: [],
    breadcrumbStack: [],
    items: [],
    pagination: { startIndex: 0, limit: 50, total: 0 },
    loading: false,
    listLoading: false,
    scrollCache: new Map(),
    pageCache: new Map(),
    artistViewTotal: 0,
    albumViewTotal: 0,
    activeLetter: null,
    favoriteTree: null,
    listViewMode: 'grid',
    selectedIds: new Set(),
    selectionAnchorIdx: null,
};

let _tracksBrowseView: TracksBrowseView | null = null;

function cacheKey(parentId?: string): string {
    return `${state.browseMode}:${parentId ?? 'root'}`;
}

// Drop cached playlist pages so a newly created playlist appears on next navigation.
export function invalidatePlaylistsCache(): void {
    for (const key of Array.from(state.pageCache.keys())) {
        if (key.startsWith('playlists:')) {
            state.pageCache.delete(key);
        }
    }
}

function updatePlaylistNameInCache(playlistId: string, name: string): void {
    for (const cached of state.pageCache.values()) {
        for (const item of cached.items) {
            if (item.type === 'Playlist' && item.id === playlistId) {
                item.name = name;
            }
        }
    }

    for (const item of state.items) {
        if (item.type === 'Playlist' && item.id === playlistId) {
            item.name = name;
        }
    }
}

export function clearNavigationCache() {
    state.scrollCache = new Map();
    state.pageCache = new Map();
    state.breadcrumbStack = [];
    state.items = [];
    state.pagination = { startIndex: 0, limit: 50, total: 0 };
    state.artistViewTotal = 0;
    state.albumViewTotal = 0;
    state.activeLetter = null;
    state.parentId = undefined;
    state.favoriteTree = null;
    state.listLoading = false;
    // browseMode, availableModes, and listViewMode are intentionally preserved
    _tracksBrowseView?.destroy();
    _tracksBrowseView = null;
}

// --- Scroll helpers ---

function saveScroll() {
    const key = cacheKey(state.parentId);
    const content = document.getElementById('library-content') as HTMLElement;
    if (content) {
        state.scrollCache.set(key, content.scrollTop);
    }
}

function restoreScroll(key: string) {
    const cachedScroll = state.scrollCache.get(key);
    if (cachedScroll !== undefined) {
        requestAnimationFrame(() => {
            const content = document.getElementById('library-content') as HTMLElement;
            if (content) content.scrollTop = cachedScroll;
            state.scrollCache.delete(key);
        });
    }
}

async function yieldTick(): Promise<void> {
    await new Promise<void>(resolve => setTimeout(resolve, 0));
}

function showSpinner(container: HTMLElement) {
    if (!container.querySelector('.is-navigating')) {
        container.innerHTML = '<sl-spinner style="font-size: 3rem;"></sl-spinner>';
    }
}

// --- Item mappers ---

function mapArtists(artists: BrowseArtist[]): BrowseDisplayItem[] {
    return artists.map(a => ({
        id: a.id,
        name: a.name,
        type: 'MusicArtist' as const,
        coverArtId: a.coverArtId,
        subtitle: null,
        childCount: a.albumCount,
        sizeBytes: 0,
        sizeTicks: 0,
    }));
}

function mapFavoriteArtists(tree: FavoriteTree): BrowseDisplayItem[] {
    return favoriteArtistsForTree(tree).map(artist => {
        const directFavorite = tree.favoriteArtistIds.has(artist.id);
        const favoriteTracks = tree.tracks.filter(track => track.artistId === artist.id);
        const favoriteAlbumIds = new Set(
            tree.albums
                .filter(album => album.artistId === artist.id)
                .map(album => album.id),
        );
        const favoriteTrackAlbumIds = new Set(
            favoriteTracks
                .map(track => track.albumId)
                .filter((albumId): albumId is string => !!albumId),
        );
        return {
            id: artist.id,
            name: artist.name,
            type: 'MusicArtist' as const,
            basketId: directFavorite ? artist.id : favoriteBasketId('artist', artist.id),
            basketType: directFavorite ? 'MusicArtist' : 'FavoriteArtist',
            coverArtId: artist.coverArtId,
            subtitle: directFavorite ? null : 'Favorite items',
            childCount: directFavorite
                ? (artist.albumCount ?? 0)
                : new Set([...favoriteAlbumIds, ...favoriteTrackAlbumIds]).size,
            sizeBytes: directFavorite ? 0 : sumTrackSizes(favoriteTracks),
            sizeTicks: directFavorite ? 0 : sumTrackTicks(favoriteTracks),
        };
    });
}

function mapAlbums(albums: BrowseAlbum[]): BrowseDisplayItem[] {
    return albums.map(a => ({
        id: a.id,
        name: a.name,
        type: 'MusicAlbum' as const,
        coverArtId: a.coverArtId,
        subtitle: a.artistName,
        year: a.year,
        childCount: a.trackCount,
        sizeBytes: 0,
        sizeTicks: 0,
    }));
}

function mapFavoriteAlbums(
    albums: BrowseAlbum[],
    tree: FavoriteTree,
    artistDirectFavorite: boolean,
): BrowseDisplayItem[] {
    return albums.map(album => {
        const directFavorite = tree.favoriteAlbumIds.has(album.id);
        const scopedFavorite = !directFavorite && !artistDirectFavorite;
        const favoriteTracks = tree.tracks.filter(track => track.albumId === album.id);
        return {
            id: album.id,
            name: album.name,
            type: 'MusicAlbum' as const,
            basketId: scopedFavorite ? favoriteBasketId('album', album.id) : album.id,
            basketType: scopedFavorite ? 'FavoriteAlbum' : 'MusicAlbum',
            coverArtId: album.coverArtId,
            subtitle: album.artistName,
            year: album.year,
            childCount: scopedFavorite ? favoriteTracks.length : album.trackCount,
            sizeBytes: scopedFavorite ? sumTrackSizes(favoriteTracks) : 0,
            sizeTicks: scopedFavorite ? sumTrackTicks(favoriteTracks) : 0,
        };
    });
}

function mapPlaylists(playlists: BrowsePlaylist[]): BrowseDisplayItem[] {
    return playlists.map(p => ({
        id: p.id,
        name: p.name,
        type: 'Playlist' as const,
        coverArtId: null,
        subtitle: null,
        childCount: p.trackCount,
        sizeBytes: 0,
        sizeTicks: p.durationSeconds * 10_000_000,
    }));
}

function mapGenres(genres: BrowseGenre[]): BrowseDisplayItem[] {
    return genres.map(g => ({
        id: g.id,
        name: g.name,
        type: 'MusicGenre' as const,
        coverArtId: g.coverArtId,
        subtitle: g.trackCount != null ? `${g.trackCount} tracks` : null,
        childCount: g.trackCount ?? 0,
        sizeBytes: 0,
        sizeTicks: 0,
    }));
}

function formatBrowseDate(isoStr: string | null | undefined): string | null {
    if (!isoStr) return null;
    try {
        const d = new Date(isoStr);
        if (isNaN(d.getTime())) return null;
        const now = new Date();
        const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
        const playedDay = new Date(d.getFullYear(), d.getMonth(), d.getDate());
        const diffDays = Math.floor((today.getTime() - playedDay.getTime()) / 86_400_000);
        if (diffDays === 0) return 'Today';
        if (diffDays === 1) return 'Yesterday';
        if (diffDays > 1 && diffDays < 7) return `${diffDays} days ago`;
        return d.toLocaleDateString(undefined, {
            month: 'short',
            day: 'numeric',
            ...(d.getFullYear() !== now.getFullYear() && { year: 'numeric' }),
        });
    } catch {
        return null;
    }
}

function mapFlatTracks(
    tracks: BrowseTrack[],
    mode?: 'frequentlyPlayed' | 'recentlyPlayed' | 'favorites',
): BrowseDisplayItem[] {
    return tracks.map(t => {
        let subtitle = `${t.artistName} — ${t.albumName}`;
        if (mode === 'frequentlyPlayed' && t.playCount != null) {
            subtitle += ` · ${t.playCount} play${t.playCount === 1 ? '' : 's'}`;
        } else if (mode === 'recentlyPlayed') {
            const dateStr = formatBrowseDate(t.lastPlayedAt);
            if (dateStr) subtitle += ` · ${dateStr}`;
        }
        return {
            id: t.id,
            name: t.title,
            type: 'Audio' as const,
            coverArtId: t.coverArtId,
            subtitle,
            sizeBytes: t.sizeBytes ?? 0,
            sizeTicks: t.duration * 10_000_000,
            childCount: 1,
        };
    });
}

function mapAlbumTracks(tracks: BrowseTrack[]): BrowseDisplayItem[] {
    return tracks.map(t => ({
        id: t.id,
        name: t.title,
        type: 'Audio' as const,
        coverArtId: t.coverArtId,
        subtitle: t.artistName,
        sizeBytes: t.sizeBytes ?? 0,
        sizeTicks: t.duration * 10_000_000,
        childCount: 1,
    }));
}

function upsertById<T extends { id: string }>(map: Map<string, T>, item: T) {
    if (!map.has(item.id)) {
        map.set(item.id, item);
    }
}

function favoriteBasketId(kind: 'artist' | 'album', id: string): string {
    return `favorites:${kind}:${id}`;
}

function sumTrackSizes(tracks: BrowseTrack[]): number {
    return tracks.reduce((sum, track) => sum + (track.sizeBytes ?? 0), 0);
}

function sumTrackTicks(tracks: BrowseTrack[]): number {
    return tracks.reduce((sum, track) => sum + (track.duration * 10_000_000), 0);
}

async function ensureFavoriteTree(): Promise<FavoriteTree> {
    if (state.favoriteTree) return state.favoriteTree;

    const result = await fetchBrowseFavoriteItems();
    state.favoriteTree = {
        artists: result.artists,
        albums: result.albums,
        tracks: result.tracks,
        favoriteArtistIds: new Set(result.artists.map(a => a.id)),
        favoriteAlbumIds: new Set(result.albums.map(a => a.id)),
    };
    return state.favoriteTree;
}

function favoriteArtistsForTree(tree: FavoriteTree): BrowseArtist[] {
    const artists = new Map<string, BrowseArtist>();

    tree.artists.forEach(artist => upsertById(artists, artist));

    tree.albums.forEach(album => {
        if (!album.artistId || !album.artistName) return;
        upsertById(artists, {
            id: album.artistId,
            name: album.artistName,
            albumCount: 0,
            coverArtId: album.coverArtId,
        });
    });

    tree.tracks.forEach(track => {
        const artistId = track.artistId ?? undefined;
        if (!artistId || !track.artistName) return;
        upsertById(artists, {
            id: artistId,
            name: track.artistName,
            albumCount: 0,
            coverArtId: track.coverArtId,
        });
    });

    return [...artists.values()].sort((a, b) => a.name.localeCompare(b.name));
}

function favoriteAlbumsForArtist(tree: FavoriteTree, artistId: string): BrowseAlbum[] {
    const albums = new Map<string, BrowseAlbum>();

    tree.albums
        .filter(album => album.artistId === artistId || tree.favoriteArtistIds.has(artistId))
        .forEach(album => upsertById(albums, album));

    tree.tracks.forEach(track => {
        const trackArtistId = track.artistId ?? undefined;
        const albumId = track.albumId ?? undefined;
        if (trackArtistId !== artistId || !albumId || !track.albumName) return;
        upsertById(albums, {
            id: albumId,
            name: track.albumName,
            artistId,
            artistName: track.artistName,
            year: null,
            trackCount: 0,
            coverArtId: track.coverArtId,
        });
    });

    return [...albums.values()].sort((a, b) => a.name.localeCompare(b.name));
}

function favoriteTracksForAlbum(tree: FavoriteTree, albumId: string): BrowseTrack[] {
    return tree.tracks.filter(track => track.albumId === albumId);
}

// --- UI rendering ---

function renderModeBar() {
    const container = document.getElementById('browse-mode-bar');
    if (!container) return;

    const existingBtns = container.querySelectorAll('sl-button[data-mode]');
    if (existingBtns.length > 0) {
        existingBtns.forEach(btn => {
            const mode = (btn as HTMLElement).getAttribute('data-mode') as BrowseMode;
            (btn as any).variant = mode === state.browseMode ? 'primary' : 'default';
            (btn as any).disabled = state.loading;
        });
        renderViewToggle();
        return;
    }

    container.innerHTML = '';
    const bar = document.createElement('div');
    bar.className = 'browse-mode-bar';

    state.availableModes.forEach(mode => {
        const btn = document.createElement('sl-button') as any;
        btn.setAttribute('data-mode', mode);
        btn.variant = mode === state.browseMode ? 'primary' : 'default';
        btn.size = 'small';
        btn.textContent = modeLabel(mode);
        btn.disabled = state.loading;
        btn.addEventListener('click', () => switchMode(mode));
        bar.appendChild(btn);
    });

    container.appendChild(bar);
    renderViewToggle();
}

function createBreadcrumbs(): HTMLElement {
    const nav = document.createElement('nav');
    nav.className = 'breadcrumbs';

    const homeBtn = document.createElement('sl-button') as any;
    homeBtn.variant = 'text';
    homeBtn.size = 'small';
    homeBtn.innerHTML = `<sl-icon slot="prefix" name="house"></sl-icon> ${modeLabel(state.browseMode)}`;
    homeBtn.onclick = () => {
        saveScroll();
        loadModeRoot();
    };
    nav.appendChild(homeBtn);

    state.breadcrumbStack.forEach((crumb, index) => {
        const separator = document.createElement('span');
        separator.className = 'separator';
        separator.textContent = '/';
        nav.appendChild(separator);

        const btn = document.createElement('sl-button') as any;
        btn.variant = 'text';
        btn.size = 'small';
        btn.textContent = crumb.name;
        if (index < state.breadcrumbStack.length - 1) {
            btn.onclick = () => navigateToCrumb(index);
        } else {
            btn.disabled = true;
        }
        nav.appendChild(btn);
    });

    return nav;
}

function renderQuickNav(): HTMLElement | null {
    if (state.breadcrumbStack.length > 0) return null;

    const isArtists = state.browseMode === 'artists';
    const isAlbums = state.browseMode === 'albums';
    if (!isArtists && !isAlbums) return null;

    const viewTotal = isArtists ? state.artistViewTotal : state.albumViewTotal;
    if (viewTotal < 20) return null;

    const allLetters = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ'.split('').concat('#');

    const navBar = document.createElement('div');
    navBar.className = 'quick-nav-bar';

    for (const letter of allLetters) {
        const btn = document.createElement('sl-button') as any;
        btn.size = 'small';
        btn.variant = letter === state.activeLetter ? 'primary' : 'text';
        btn.textContent = letter;
        btn.addEventListener('click', () => {
            if (isArtists) {
                loadArtistsByLetter(letter);
            } else {
                loadAlbumsByLetter(letter);
            }
        });
        navBar.appendChild(btn);
    }

    return navBar;
}

function renderGrid(items: BrowseDisplayItem[], onCurate?: (id: string, name: string) => void) {
    const container = document.getElementById('library-content');
    if (!container) return;

    teardownListScrollHandler();
    container.innerHTML = '';

    if (state.breadcrumbStack.length > 0) {
        container.appendChild(createBreadcrumbs());
    }

    const quickNav = renderQuickNav();
    if (quickNav) container.appendChild(quickNav);

    const grid = document.createElement('div');
    grid.className = 'media-grid';

    items.forEach(item => {
        const selEnabled = true;

        const card = MediaCard.create(item, 'items', false, () => navigateToBrowseItem(item), selEnabled, _supportsPlaylistWrite, onCurate);
        card.setAttribute('data-name', item.name);
        grid.appendChild(card);
    });

    container.appendChild(grid);

    // One delegated basket listener for the whole grid, torn down in
    // teardownListScrollHandler before each re-render. Replaces the former
    // per-card subscription, which leaked one orphaned listener per card on
    // every navigation (innerHTML teardown can't unsubscribe individual cards).
    const gridBasketHandler = () => MediaCard.refreshSelection(grid);
    basketStore.addEventListener('update', gridBasketHandler);
    (container as any).__gridBasketHandler = gridBasketHandler;

    if (state.items.length < state.pagination.total) {
        const loadMoreContainer = document.createElement('div');
        loadMoreContainer.className = 'load-more-container';

        const loadMoreBtn = document.createElement('sl-button') as any;
        loadMoreBtn.className = 'load-more-btn';
        loadMoreBtn.textContent = `Load More (${state.pagination.total - state.items.length} remaining)`;
        loadMoreBtn.onclick = () => loadMore();

        loadMoreContainer.appendChild(loadMoreBtn);
        container.appendChild(loadMoreContainer);
    }
}

function teardownListScrollHandler() {
    const c = document.getElementById('library-content');
    if (c) {
        if ((c as any).__listScrollHandler) {
            c.removeEventListener('scroll', (c as any).__listScrollHandler);
            delete (c as any).__listScrollHandler;
        }
        if ((c as any).__listBasketHandler) {
            basketStore.removeEventListener('update', (c as any).__listBasketHandler);
            delete (c as any).__listBasketHandler;
        }
        if ((c as any).__gridBasketHandler) {
            basketStore.removeEventListener('update', (c as any).__gridBasketHandler);
            delete (c as any).__gridBasketHandler;
        }
        delete (c as any).__listScroller;
        delete (c as any).__listPaint;
        delete (c as any).__listSpinner;
    }
}

function showListSpinner(content: HTMLElement, loadedCount: number) {
    const scroller = (content as any).__listScroller as HTMLElement | undefined;
    if (!scroller) return;
    const existing = (content as any).__listSpinner as HTMLElement | undefined;
    if (existing) return;
    const loader = document.createElement('div');
    loader.className = 'media-list-loader';
    loader.style.top = `${loadedCount * VIRTUAL_ROW_HEIGHT}px`;
    loader.innerHTML = '<sl-spinner></sl-spinner>';
    scroller.appendChild(loader);
    (content as any).__listSpinner = loader;
}

function removeListSpinner(content: HTMLElement) {
    const existing = (content as any).__listSpinner as HTMLElement | undefined;
    if (existing) {
        existing.remove();
        delete (content as any).__listSpinner;
    }
}

function renderViewToggle() {
    const container = document.getElementById('browse-mode-bar');
    if (!container) return;
    const existing = container.querySelector('.view-toggle-group');
    if (existing) existing.remove();
    const showToggle = !state.loading && state.browseMode !== 'tracks';
    if (!showToggle) return;
    const currentMode = state.listViewMode;
    const group = document.createElement('div');
    group.className = 'view-toggle-group';
    const gridBtn = document.createElement('sl-button') as any;
    gridBtn.size = 'small';
    gridBtn.variant = currentMode === 'grid' ? 'primary' : 'default';
    gridBtn.title = t('library.viewToggle.grid');
    gridBtn.innerHTML = '<sl-icon name="grid"></sl-icon>';
    gridBtn.addEventListener('click', () => setViewMode('grid'));
    const listBtn = document.createElement('sl-button') as any;
    listBtn.size = 'small';
    listBtn.variant = currentMode === 'list' ? 'primary' : 'default';
    listBtn.title = t('library.viewToggle.list');
    listBtn.innerHTML = '<sl-icon name="list"></sl-icon>';
    listBtn.addEventListener('click', () => setViewMode('list'));
    group.appendChild(gridBtn);
    group.appendChild(listBtn);
    container.appendChild(group);
}

function setViewMode(mode: 'grid' | 'list') {
    if (state.loading) return;
    clearSelection();
    state.listViewMode = mode;
    renderModeBar();
    renderCurrentView();
}

// --- List multi-selection (Story 9.11) ---

// Only artist/album rows are selectable in v1. Favorite-scoped entries, genres,
// playlists and tracks render no checkbox and are skipped by Shift-ranges.
function isSelectableListItem(item: BrowseDisplayItem): boolean {
    const resolved = item.basketType ?? item.type;
    return resolved === 'MusicArtist' || resolved === 'MusicAlbum';
}

// Repaint mounted rows (selection is app state — remounted rows re-read it)
// and refresh the bulk bar. Same wipe-and-paint mechanism as basket updates.
function syncSelectionUi(): void {
    const content = document.getElementById('library-content');
    if (!content) return;
    const scroller = (content as any).__listScroller as HTMLElement | undefined;
    if (scroller) {
        scroller.classList.toggle('has-selection', state.selectedIds.size > 0);
        scroller.querySelectorAll('.media-list-row').forEach(r => r.remove());
        (content as any).__listPaint?.();
    }
    updateBulkBar(content);
}

function clearSelection(): void {
    if (state.selectedIds.size === 0 && state.selectionAnchorIdx === null) return;
    state.selectedIds = new Set();
    state.selectionAnchorIdx = null;
    syncSelectionUi();
}

// Cheap single-row toggle: updates only the clicked row's class/checkbox and
// the bulk bar — no full repaint (toggles can happen rapidly).
function toggleRowSelection(item: BrowseDisplayItem, index: number, row: HTMLElement): void {
    const itemId = item.basketId ?? item.id;
    if (state.selectedIds.has(itemId)) {
        state.selectedIds.delete(itemId);
    } else {
        state.selectedIds.add(itemId);
    }
    state.selectionAnchorIdx = index;
    const checked = state.selectedIds.has(itemId);
    row.classList.toggle('is-checked', checked);
    const check = row.querySelector<HTMLInputElement>('.media-list-row__check');
    if (check) check.checked = checked;
    const content = document.getElementById('library-content');
    const scroller = content && ((content as any).__listScroller as HTMLElement | undefined);
    if (scroller) scroller.classList.toggle('has-selection', state.selectedIds.size > 0);
    updateBulkBar(content);
}

// Shift-range: ranges come from state.items, never the DOM — virtualized rows
// outside the viewport are unmounted. The anchor stays put.
function selectRange(fromIdx: number, toIdx: number): void {
    const [lo, hi] = fromIdx <= toIdx ? [fromIdx, toIdx] : [toIdx, fromIdx];
    for (let i = lo; i <= hi; i++) {
        const it = state.items[i];
        if (it && isSelectableListItem(it)) {
            state.selectedIds.add(it.basketId ?? it.id);
        }
    }
    syncSelectionUi();
}

// Module-level Escape handler, registered once. Capture phase so it runs
// before the context menu's own capture handler removes the menu — an open
// menu/dialog swallows the Escape and the selection survives.
let _selectionEscapeRegistered = false;
function ensureSelectionEscapeListener(): void {
    if (_selectionEscapeRegistered) return;
    _selectionEscapeRegistered = true;
    document.addEventListener('keydown', (e: KeyboardEvent) => {
        if (e.key !== 'Escape' || state.selectedIds.size === 0) return;
        if (document.querySelector('sl-dialog[open]')) return;
        // Match the menu regardless of `.is-open`: the class is added one frame
        // after the element mounts, so guarding on `.is-open` would miss an
        // Escape pressed in that opening frame. The menu element only exists in
        // the DOM while open (removed on close), so bare `.hm-context-menu` is safe.
        if (document.querySelector('.hm-context-menu')) return;
        clearSelection();
    }, true);
}

function renderBulkBar(): HTMLElement {
    const bar = document.createElement('div');
    bar.className = 'bulk-action-bar';

    const count = document.createElement('span');
    count.className = 'bulk-action-bar__count';
    count.setAttribute('aria-live', 'polite');
    count.textContent = t('library.selection.count', { count: state.selectedIds.size });
    bar.appendChild(count);

    const addBtn = document.createElement('sl-button') as any;
    addBtn.size = 'small';
    addBtn.variant = 'primary';
    // basket-toggle-btn: the existing `#library-content.device-locked` CSS rule
    // disables it when no device is selected — same mechanism as per-row (+).
    addBtn.classList.add('basket-toggle-btn');
    addBtn.textContent = t('library.selection.add_to_basket');
    addBtn.addEventListener('click', () => bulkAddSelectionToBasket(addBtn));
    bar.appendChild(addBtn);

    if (_supportsPlaylistWrite) {
        const plBtn = document.createElement('sl-button') as any;
        plBtn.size = 'small';
        plBtn.textContent = t('library.selection.add_to_playlist');
        plBtn.addEventListener('click', () => bulkAddSelectionToPlaylist());
        bar.appendChild(plBtn);
    }

    const clearBtn = document.createElement('sl-button') as any;
    clearBtn.size = 'small';
    clearBtn.variant = 'text';
    clearBtn.textContent = t('library.selection.clear');
    clearBtn.addEventListener('click', () => clearSelection());
    bar.appendChild(clearBtn);

    return bar;
}

// Create the bar when the selection goes 0→1, update the count in place while
// it lives, remove it when the selection empties.
function updateBulkBar(content: HTMLElement | null = document.getElementById('library-content')): void {
    if (!content) return;
    const existing = content.querySelector<HTMLElement>(':scope > .bulk-action-bar');
    if (state.selectedIds.size === 0) {
        existing?.remove();
        return;
    }
    if (existing) {
        const count = existing.querySelector('.bulk-action-bar__count');
        if (count) count.textContent = t('library.selection.count', { count: state.selectedIds.size });
        return;
    }
    const scroller = (content as any).__listScroller as HTMLElement | undefined;
    if (!scroller || !scroller.isConnected) return;
    const bar = renderBulkBar();
    // The quick-nav is sticky at top: 0 — stick the bar right below it so the
    // two don't overlap while scrolled.
    const qn = content.querySelector<HTMLElement>('.quick-nav-bar');
    if (qn) bar.style.top = `${qn.offsetHeight}px`;
    content.insertBefore(bar, scroller);
    // AC 10: an aria-live region only announces mutations made after it is
    // connected. renderBulkBar populates the count before insertion (silent), so
    // re-assert it on the next frame to announce the first (0→1) selection too.
    const count = bar.querySelector<HTMLElement>('.bulk-action-bar__count');
    if (count) {
        count.textContent = '';
        requestAnimationFrame(() => {
            count.textContent = t('library.selection.count', { count: state.selectedIds.size });
        });
    }
}

function resolveSelectedItems(): BrowseDisplayItem[] {
    return state.items.filter(
        it => isSelectableListItem(it) && state.selectedIds.has(it.basketId ?? it.id)
    );
}

// Shared basket-add used by both the per-row (+) toggle and the bulk action.
// Preserves the per-row semantics exactly: already-basketed items are skipped,
// container items missing count/size get ONE batched RPC pair, and responses
// are mapped by id (response order is not guaranteed to match request order).
async function addBrowseItemsToBasket(items: BrowseDisplayItem[]): Promise<{ added: number; skipped: number }> {
    const CONTAINER_TYPES = ['MusicArtist', 'MusicAlbum', 'MusicGenre', 'Playlist'];
    const toAdd: BrowseDisplayItem[] = [];
    const needsFetch = new Set<BrowseDisplayItem>();
    let skipped = 0;
    for (const item of items) {
        const itemId = item.basketId ?? item.id;
        if (basketStore.has(itemId)) {
            skipped++;
            continue;
        }
        toAdd.push(item);
        const resolvedType = item.basketType ?? item.type;
        const isFavoriteScoped = resolvedType === 'FavoriteArtist' || resolvedType === 'FavoriteAlbum';
        if (CONTAINER_TYPES.includes(resolvedType) && !isFavoriteScoped && (!item.childCount || !item.sizeBytes)) {
            needsFetch.add(item);
        }
    }

    const countById = new Map<string, { recursiveItemCount?: number; cumulativeRunTimeTicks?: number }>();
    const sizeById = new Map<string, { totalSizeBytes?: number }>();
    if (needsFetch.size > 0) {
        const itemIds = [...needsFetch].map(it => it.basketId ?? it.id);
        const [metadata, sizeData] = await Promise.all([
            rpcCall('jellyfin_get_item_counts', { itemIds }),
            rpcCall('jellyfin_get_item_sizes', { itemIds }),
        ]);
        for (const m of metadata ?? []) countById.set(m.id, m);
        for (const s of sizeData ?? []) sizeById.set(s.id, s);
    }

    for (const item of toAdd) {
        const itemId = item.basketId ?? item.id;
        const resolvedType = item.basketType ?? item.type;
        if (needsFetch.has(item)) {
            const info = countById.get(itemId) ?? { recursiveItemCount: 0, cumulativeRunTimeTicks: 0 };
            const sizeInfo = sizeById.get(itemId) ?? { totalSizeBytes: 0 };
            basketStore.add({
                id: itemId,
                name: item.name,
                type: resolvedType,
                artist: item.subtitle ?? undefined,
                childCount: info.recursiveItemCount ?? 0,
                sizeTicks: item.sizeTicks || (info.cumulativeRunTimeTicks ?? 0),
                sizeBytes: sizeInfo.totalSizeBytes ?? 0,
            });
        } else {
            basketStore.add({
                id: itemId,
                name: item.name,
                type: resolvedType,
                artist: item.subtitle ?? undefined,
                childCount: item.childCount ?? 0,
                sizeTicks: item.sizeTicks ?? 0,
                sizeBytes: item.sizeBytes ?? 0,
            });
        }
    }

    return { added: toAdd.length, skipped };
}

async function bulkAddSelectionToBasket(btn: any): Promise<void> {
    if (btn.loading) return;
    // Snapshot up front — adds are idempotent via the skip check, so a
    // selection cleared mid-flight cannot double-add.
    const selected = resolveSelectedItems();
    if (selected.length === 0) return;
    btn.loading = true;
    try {
        const { added, skipped } = await addBrowseItemsToBasket(selected);
        let msg = t('library.selection.added_toast', { added });
        if (skipped > 0) msg += t('library.selection.skipped_suffix', { skipped });
        showToast(msg, 'success');
        clearSelection();
    } catch (err) {
        console.error('Bulk add to basket failed:', err);
        const msg = err instanceof Error ? err.message : String(err);
        showToast(msg, 'danger', ERROR_TOAST_DURATION);
        // Selection survives so the user can retry.
    } finally {
        btn.loading = false;
    }
}

function bulkAddSelectionToPlaylist(): void {
    const selected = resolveSelectedItems();
    if (selected.length === 0) return;
    const itemIds = selected.map(it => it.id);
    // Selection clears only on success; cancelling the dialog keeps it.
    // The label is forwarded as the "New playlist" suggested name, so pass a
    // generic localized default rather than the "N selected" count string.
    MediaCard.openAddToPlaylistDialog(
        itemIds,
        t('library.selection.new_playlist_name'),
        () => clearSelection()
    );
}

function renderListRow(item: BrowseDisplayItem, index: number, onCurate?: (id: string, name: string) => void): HTMLElement {
    const itemId = item.basketId ?? item.id;
    const isSelected = basketStore.has(itemId);
    const selectable = isSelectableListItem(item);
    const row = document.createElement('div');
    row.className = 'media-list-row';
    if (isSelected) row.classList.add('is-selected');
    if (selectable && state.selectedIds.has(itemId)) row.classList.add('is-checked');
    row.dataset.idx = String(index);
    row.style.top = `${index * VIRTUAL_ROW_HEIGHT}px`;
    const thumb = document.createElement('div');
    thumb.className = 'media-list-row__thumb';
    getImageUrl(item.coverArtId ?? item.id, 64, 90).then(url => {
        thumb.style.backgroundImage = `url('${url}')`;
    }).catch(() => {});
    const info = document.createElement('div');
    info.className = 'media-list-row__info';
    const nameEl = document.createElement('div');
    nameEl.className = 'media-list-row__name';
    nameEl.textContent = item.name;
    info.appendChild(nameEl);
    if (item.subtitle) {
        const subtitleEl = document.createElement('div');
        subtitleEl.className = 'media-list-row__subtitle';
        subtitleEl.textContent = item.subtitle;
        info.appendChild(subtitleEl);
    }
    const toggleBtn = document.createElement('sl-icon-button') as any;
    toggleBtn.name = isSelected ? 'dash-circle-fill' : 'plus-circle-fill';
    toggleBtn.label = isSelected ? t('tracks.view.remove_from_basket') : t('tracks.view.add_to_basket');
    toggleBtn.style.fontSize = '1.25rem';
    // device-locked CSS rule (#library-content.device-locked .basket-toggle-btn)
    // disables this button when no device is selected, same as grid cards.
    toggleBtn.classList.add('basket-toggle-btn');
    toggleBtn.addEventListener('click', async (e: Event) => {
        e.stopPropagation();
        if (basketStore.has(itemId)) {
            basketStore.remove(itemId);
            return;
        }
        toggleBtn.loading = true;
        try {
            await addBrowseItemsToBasket([item]);
        } catch (err) {
            console.error('Failed to fetch item count:', err);
        } finally {
            toggleBtn.loading = false;
        }
    });
    row.addEventListener('click', async (e) => {
        const path = e.composedPath();
        const isBtn = path.some(el => {
            const tag = (el as HTMLElement).tagName;
            return tag === 'SL-ICON-BUTTON' || tag === 'INPUT';
        });
        if (isBtn) return;
        if (selectable && (e.ctrlKey || e.metaKey)) {
            toggleRowSelection(item, index, row);
            return;
        }
        if (selectable && e.shiftKey && state.selectionAnchorIdx !== null) {
            selectRange(state.selectionAnchorIdx, index);
            return;
        }
        if (!row.classList.contains('is-navigating')) {
            row.classList.add('is-navigating');
            try {
                await navigateToBrowseItem(item);
            } finally {
                row.classList.remove('is-navigating');
            }
        }
    });
    // Shift-click extends the selection — suppress the browser's text-range
    // selection artifact for that gesture only.
    row.addEventListener('mousedown', (e) => {
        if (e.shiftKey && state.selectionAnchorIdx !== null) e.preventDefault();
    });
    // Context menu for artist/album/track rows
    if (_supportsPlaylistWrite && (item.type === 'MusicArtist' || item.type === 'MusicAlbum' || item.type === 'Audio')) {
        row.addEventListener('contextmenu', (e) => {
            e.preventDefault();
            MediaCard.showItemContextMenu(e.clientX, e.clientY, item.id, item.name);
        });
    }
    if (selectable) {
        const check = document.createElement('input');
        check.type = 'checkbox';
        check.className = 'media-list-row__check';
        check.checked = state.selectedIds.has(itemId);
        check.setAttribute('aria-label', item.name);
        check.addEventListener('click', (e) => {
            e.stopPropagation();
            toggleRowSelection(item, index, row);
        });
        row.appendChild(check);
    }
    row.appendChild(thumb);
    row.appendChild(info);
    // Curate button: appears on Playlist rows when playlist write is supported (mirrors MediaCard grid behavior)
    if (onCurate && item.type === 'Playlist') {
        const curateBtn = document.createElement('sl-icon-button') as any;
        curateBtn.name = 'pencil-square';
        curateBtn.label = t('playlist.curation.curate_btn');
        curateBtn.style.fontSize = '1.25rem';
        curateBtn.addEventListener('click', (e: MouseEvent) => {
            e.stopPropagation();
            onCurate(item.id, item.name);
        });
        row.appendChild(curateBtn);
    }
    row.appendChild(toggleBtn);
    return row;
}

const LOAD_AHEAD = 5;

function renderList(items: BrowseDisplayItem[], onCurate?: (id: string, name: string) => void) {
    const container = document.getElementById('library-content');
    if (!container) return;
    const content = container;
    teardownListScrollHandler();
    content.innerHTML = '';
    if (state.breadcrumbStack.length > 0) content.appendChild(createBreadcrumbs());
    const qn = renderQuickNav();
    if (qn) content.appendChild(qn);
    const expectedTotal = state.pagination.total > 0 ? state.pagination.total : items.length;
    const scroller = document.createElement('div');
    scroller.className = 'media-list';
    scroller.style.height = `${expectedTotal * VIRTUAL_ROW_HEIGHT}px`;
    (content as any).__listScroller = scroller;
    function paint() {
        const currentItems = state.items;
        const scrollTop = content.scrollTop;
        const viewportH = content.clientHeight;
        const first = Math.max(0, Math.floor(scrollTop / VIRTUAL_ROW_HEIGHT) - OVERSCAN);
        const last = Math.min(currentItems.length - 1, Math.ceil((scrollTop + viewportH) / VIRTUAL_ROW_HEIGHT) + OVERSCAN);
        scroller.querySelectorAll<HTMLElement>('.media-list-row').forEach(row => {
            const idx = Number(row.dataset.idx);
            if (idx < first || idx > last) row.remove();
        });
        const existing = new Set(
            [...scroller.querySelectorAll<HTMLElement>('.media-list-row')].map(r => Number(r.dataset.idx))
        );
        for (let i = first; i <= last; i++) {
            if (!existing.has(i)) scroller.appendChild(renderListRow(currentItems[i], i, onCurate));
        }
    }
    (content as any).__listPaint = paint;
    const scrollHandler = () => {
        if (state.listLoading) {
            const maxScroll = state.items.length * VIRTUAL_ROW_HEIGHT;
            if (content.scrollTop > maxScroll) content.scrollTop = maxScroll;
        }
        paint();
        if (
            !state.listLoading &&
            listAutoloadSupported() &&
            state.items.length < state.pagination.total
        ) {
            const loadedBoundary = (state.items.length - LOAD_AHEAD) * VIRTUAL_ROW_HEIGHT;
            if (content.scrollTop + content.clientHeight >= loadedBoundary) {
                loadMoreForListView();
            }
        }
    };
    content.addEventListener('scroll', scrollHandler);
    (content as any).__listScrollHandler = scrollHandler;
    const basketUpdateHandler = () => {
        scroller.querySelectorAll('.media-list-row').forEach(r => r.remove());
        paint();
    };
    basketStore.addEventListener('update', basketUpdateHandler);
    (content as any).__listBasketHandler = basketUpdateHandler;
    ensureSelectionEscapeListener();
    // Selection is usually cleared before a re-render, but if one is active
    // (e.g. a repaint without a clearing trigger) restore its UI.
    scroller.classList.toggle('has-selection', state.selectedIds.size > 0);
    content.appendChild(scroller);
    updateBulkBar(content);
    paint();
}

// Which (mode, depth) combinations loadMoreForListView() can actually paginate.
// Single source of truth shared with the scroll handler so it never fires for
// modes that fall through to the no-op branch (avoids churn / stuck triggers).
function listAutoloadSupported(): boolean {
    const depth = state.breadcrumbStack.length;
    switch (state.browseMode) {
        case 'artists':
        case 'albums':
        case 'recentlyAdded':
        case 'frequentlyPlayed':
        case 'recentlyPlayed':
            return depth === 0;
        case 'genres':
            return depth === 0 || (depth === 1 && !!state.parentId);
        default:
            return false;
    }
}

async function loadMoreForListView() {
    if (state.listLoading || !listAutoloadSupported() || state.items.length >= state.pagination.total) return;
    state.listLoading = true;
    const content = document.getElementById('library-content');
    if (content) showListSpinner(content, state.items.length);
    try {
        const startIndex = state.items.length;
        const letter = state.activeLetter ?? undefined;
        const depth = state.breadcrumbStack.length;
        const mode = state.browseMode;

        if (mode === 'artists' && depth === 0) {
            const r = await fetchBrowseArtists(letter, undefined, startIndex, 200);
            state.items = [...state.items, ...mapArtists(r.artists)];
            state.pagination.total = r.total;
        } else if (mode === 'albums' && depth === 0) {
            const r = await fetchBrowseAlbums(letter, undefined, startIndex, 200);
            state.items = [...state.items, ...mapAlbums(r.albums)];
            state.pagination.total = r.total;
        } else if (mode === 'genres' && depth === 0) {
            const r = await fetchBrowseGenres(undefined, startIndex, state.pagination.limit);
            state.items = [...state.items, ...mapGenres(r.genres)];
            state.pagination.total = r.total;
        } else if (mode === 'genres' && depth === 1 && state.parentId) {
            const r = await fetchBrowseGenre(state.parentId, startIndex, state.pagination.limit);
            state.items = [...state.items, ...mapFlatTracks(r.tracks)];
            state.pagination.total = r.total;
        } else if (mode === 'recentlyAdded' && depth === 0) {
            const r = await fetchBrowseRecentlyAdded(undefined, startIndex, state.pagination.limit);
            state.items = [...state.items, ...mapAlbums(r.albums)];
            state.pagination.total = r.total;
        } else if (mode === 'frequentlyPlayed' && depth === 0) {
            const r = await fetchBrowseFrequentlyPlayed(undefined, startIndex, state.pagination.limit);
            state.items = [...state.items, ...mapFlatTracks(r.tracks, 'frequentlyPlayed')];
            state.pagination.total = r.total;
        } else if (mode === 'recentlyPlayed' && depth === 0) {
            const r = await fetchBrowseRecentlyPlayed(undefined, startIndex, state.pagination.limit);
            state.items = [...state.items, ...mapFlatTracks(r.tracks, 'recentlyPlayed')];
            state.pagination.total = r.total;
        } else {
            return;
        }

        state.pagination.startIndex = state.items.length;
        if (content) {
            const scroller = (content as any).__listScroller as HTMLElement | undefined;
            if (scroller) scroller.style.height = `${state.pagination.total * VIRTUAL_ROW_HEIGHT}px`;
            (content as any).__listPaint?.();
        }
    } catch (e) {
        console.error('loadMoreForListView failed:', e);
    } finally {
        state.listLoading = false;
        if (content) removeListSpinner(content);
    }
}

function renderCurrentView() {
    const mode = state.listViewMode;
    const onCurate = state.browseMode === 'playlists' && _supportsPlaylistWrite
        ? openCurationView
        : undefined;
    if (mode === 'list') {
        renderList(state.items, onCurate);
    } else {
        renderGrid(state.items, onCurate);
    }
}


function renderError(error: Error) {
    const container = document.getElementById('library-content');
    if (!container) return;

    container.innerHTML = `
        <div class="error-message">
            <sl-icon name="exclamation-triangle" style="font-size: 2rem;"></sl-icon>
            <p>Error: ${error.message}</p>
            <sl-button id="retry-library-btn">Retry</sl-button>
        </div>
    `;

    container.querySelector('#retry-library-btn')?.addEventListener('click', () => {
        initLibraryView();
    });
}

// --- Mode switching ---

async function switchMode(mode: BrowseMode) {
    if (mode === state.browseMode || state.loading) return;

    clearSelection();
    saveScroll();
    // Leaving Tracks mode: tear down the view's basket subscription and scroll
    // handlers. The instance is kept (not nulled) so re-entry can remount and
    // restore prior selection/scroll; only clearNavigationCache fully discards it.
    if (state.browseMode === 'tracks' && mode !== 'tracks') {
        _tracksBrowseView?.destroy();
    }
    state.browseMode = mode;
    state.breadcrumbStack = [];
    state.pagination.startIndex = 0;
    state.items = [];
    state.activeLetter = null;
    state.artistViewTotal = 0;
    state.albumViewTotal = 0;
    state.parentId = undefined;

    renderModeBar();
    await loadModeRoot();
}

// --- Mode root loading ---

async function loadModeRoot() {
    clearSelection();
    state.breadcrumbStack = [];
    state.pagination.startIndex = 0;
    state.items = [];
    state.activeLetter = null;
    state.artistViewTotal = 0;
    state.albumViewTotal = 0;
    state.parentId = undefined;

    switch (state.browseMode) {
        case 'artists': await loadArtists(true); break;
        case 'albums': await loadAlbums(true); break;
        case 'playlists': await loadPlaylists(); break;
        case 'tracks': loadTracksView(); break;
        case 'genres': await loadGenres(true); break;
        case 'recentlyAdded':
            await loadRecentlyAddedAlbums(true);
            break;
        case 'frequentlyPlayed':
        case 'recentlyPlayed':
            await loadFlatTracks(state.browseMode, true);
            break;
        case 'favorites':
            await loadFavoriteArtists();
            break;
    }
}

function loadTracksView(): void {
    const container = document.getElementById('library-content');
    if (!container) return;
    teardownListScrollHandler();
    if (_tracksBrowseView) {
        _tracksBrowseView.remount();
    } else {
        _tracksBrowseView = new TracksBrowseView(container, _supportsPlaylistWrite);
        _tracksBrowseView.load();
    }
}

// --- Mode-specific loaders ---

async function loadArtists(reset: boolean) {
    const container = document.getElementById('library-content');
    if (!container) return;

    const key = cacheKey(undefined);

    if (reset) {
        const cached = state.pageCache.get(key);
        if (cached) {
            state.items = cached.items;
            state.pagination.total = cached.total;
            state.pagination.startIndex = cached.items.length;
            state.artistViewTotal = cached.total;
            renderCurrentView();
            restoreScroll(key);
            return;
        }

        await yieldTick();
        if (!container.isConnected) return;
        showSpinner(container);
    }

    state.loading = true;
    renderModeBar();
    try {
        const startIndex = reset ? 0 : state.pagination.startIndex;
        const result = await fetchBrowseArtists(undefined, undefined, startIndex, state.pagination.limit);

        state.pagination.total = result.total;
        const mapped = mapArtists(result.artists);

        if (reset) {
            state.items = mapped;
            state.artistViewTotal = result.total;
            state.pageCache.set(key, { items: state.items, total: result.total });
        } else {
            state.items = [...state.items, ...mapped];
        }

        renderCurrentView();
        if (reset) restoreScroll(key);
    } catch (e) {
        renderError(e as Error);
    } finally {
        state.loading = false;
        renderModeBar();
    }
}

async function loadArtistsByLetter(letter: string) {
    const container = document.getElementById('library-content');
    if (!container || state.loading) return;

    clearSelection();
    if (state.activeLetter === letter) {
        state.activeLetter = null;
        await loadArtists(true);
        return;
    }

    const inListView = (state.listViewMode) === 'list';

    state.activeLetter = letter;
    state.loading = true;
    renderModeBar();

    if (inListView) {
        state.listLoading = true;
        showListSpinner(container, state.items.length);
    } else {
        container.innerHTML = '<sl-spinner style="font-size: 3rem;"></sl-spinner>';
    }

    await yieldTick();
    if (!container.isConnected) {
        state.loading = false;
        state.listLoading = false;
        return;
    }

    try {
        const result = await fetchBrowseArtists(letter, undefined, 0, 200);
        state.items = mapArtists(result.artists);
        state.pagination.total = result.total;
        state.pagination.startIndex = result.artists.length;
        renderCurrentView();
        if (inListView) container.scrollTop = 0;
    } catch (e) {
        renderError(e as Error);
    } finally {
        state.listLoading = false;
        state.loading = false;
        renderModeBar();
    }
}

async function loadAlbumsByLetter(letter: string) {
    const container = document.getElementById('library-content');
    if (!container || state.loading) return;

    clearSelection();
    if (state.activeLetter === letter) {
        state.activeLetter = null;
        await loadAlbums(true);
        return;
    }

    const inListView = (state.listViewMode) === 'list';

    state.activeLetter = letter;
    state.loading = true;
    renderModeBar();

    if (inListView) {
        state.listLoading = true;
        showListSpinner(container, state.items.length);
    } else {
        container.innerHTML = '<sl-spinner style="font-size: 3rem;"></sl-spinner>';
    }

    await yieldTick();
    if (!container.isConnected) {
        state.loading = false;
        state.listLoading = false;
        return;
    }

    try {
        const result = await fetchBrowseAlbums(letter, undefined, 0, 200);
        state.items = mapAlbums(result.albums);
        state.pagination.total = result.total;
        state.pagination.startIndex = result.albums.length;
        renderCurrentView();
        if (inListView) container.scrollTop = 0;
    } catch (e) {
        renderError(e as Error);
    } finally {
        state.listLoading = false;
        state.loading = false;
        renderModeBar();
    }
}

async function loadAlbums(reset: boolean) {
    const container = document.getElementById('library-content');
    if (!container) return;

    const key = cacheKey(undefined);

    if (reset) {
        const cached = state.pageCache.get(key);
        if (cached) {
            state.items = cached.items;
            state.pagination.total = cached.total;
            state.pagination.startIndex = cached.items.length;
            state.albumViewTotal = cached.total;
            renderCurrentView();
            restoreScroll(key);
            return;
        }

        await yieldTick();
        if (!container.isConnected) return;
        showSpinner(container);
    }

    state.loading = true;
    renderModeBar();
    try {
        const startIndex = reset ? 0 : state.pagination.startIndex;
        const result = await fetchBrowseAlbums(undefined, undefined, startIndex, state.pagination.limit);

        state.pagination.total = result.total;
        const mapped = mapAlbums(result.albums);

        if (reset) {
            state.items = mapped;
            state.albumViewTotal = result.total;
            state.pageCache.set(key, { items: state.items, total: result.total });
        } else {
            state.items = [...state.items, ...mapped];
        }

        renderCurrentView();
        if (reset) restoreScroll(key);
    } catch (e) {
        renderError(e as Error);
    } finally {
        state.loading = false;
        renderModeBar();
    }
}

function openCurationView(playlistId: string, playlistName: string): void {
    const container = document.getElementById('library-content');
    if (!container) return;

    teardownListScrollHandler();
    saveScroll();

    const view = new PlaylistCurationView(
        container,
        playlistId,
        playlistName,
        () => {
            // On close: invalidate the playlists list cache and restore the list view
            invalidatePlaylistsCache();
            loadPlaylists();
        },
        _supportsPlaylistWrite,
        updatePlaylistNameInCache,
    );

    view.load();
}

async function loadPlaylists() {
    const container = document.getElementById('library-content');
    if (!container) return;

    const key = cacheKey(undefined);

    const cached = state.pageCache.get(key);
    if (cached) {
        state.items = cached.items;
        state.pagination.total = cached.total;
        state.artistViewTotal = 0;
        renderCurrentView();
        restoreScroll(key);
        return;
    }

    await yieldTick();
    if (!container.isConnected) return;
    showSpinner(container);

    state.loading = true;
    renderModeBar();
    try {
        const result = await fetchBrowsePlaylists();
        const mapped = mapPlaylists(result.playlists);
        state.items = mapped;
        state.pagination.total = mapped.length;
        state.pagination.startIndex = mapped.length;
        state.artistViewTotal = 0;
        state.pageCache.set(key, { items: state.items, total: mapped.length });
        renderCurrentView();
        restoreScroll(key);
    } catch (e) {
        renderError(e as Error);
    } finally {
        state.loading = false;
        renderModeBar();
    }
}

async function loadGenres(reset: boolean) {
    const container = document.getElementById('library-content');
    if (!container) return;

    const key = cacheKey(undefined);

    if (reset) {
        const cached = state.pageCache.get(key);
        if (cached) {
            state.items = cached.items;
            state.pagination.total = cached.total;
            state.pagination.startIndex = 0;
            state.artistViewTotal = 0;
            renderCurrentView();
            restoreScroll(key);
            return;
        }

        await yieldTick();
        if (!container.isConnected) return;
        showSpinner(container);
    }

    state.loading = true;
    renderModeBar();
    try {
        const startIndex = reset ? 0 : state.pagination.startIndex;
        const result = await fetchBrowseGenres(undefined, startIndex, state.pagination.limit);

        state.pagination.total = result.total;
        const mapped = mapGenres(result.genres);

        if (reset) {
            state.items = mapped;
            state.artistViewTotal = 0;
            state.pageCache.set(key, { items: state.items, total: result.total });
        } else {
            state.items = [...state.items, ...mapped];
        }

        renderCurrentView();
        if (reset) restoreScroll(key);
    } catch (e) {
        renderError(e as Error);
    } finally {
        state.loading = false;
        renderModeBar();
    }
}

async function loadRecentlyAddedAlbums(reset: boolean) {
    const container = document.getElementById('library-content');
    if (!container) return;

    const key = cacheKey(undefined);

    if (reset) {
        const cached = state.pageCache.get(key);
        if (cached) {
            state.items = cached.items;
            state.pagination.total = cached.total;
            state.pagination.startIndex = 0;
            renderCurrentView();
            restoreScroll(key);
            return;
        }

        await yieldTick();
        if (!container.isConnected) return;
        showSpinner(container);
    }

    state.loading = true;
    renderModeBar();
    try {
        const startIndex = reset ? 0 : state.pagination.startIndex;
        const result = await fetchBrowseRecentlyAdded(undefined, startIndex, state.pagination.limit);

        state.pagination.total = result.total;
        const mapped = mapAlbums(result.albums);

        if (reset) {
            state.items = mapped;
            state.pageCache.set(key, { items: state.items, total: result.total });
        } else {
            state.items = [...state.items, ...mapped];
        }

        renderCurrentView();
        if (reset) restoreScroll(key);
    } catch (e) {
        renderError(e as Error);
    } finally {
        state.loading = false;
        renderModeBar();
    }
}

async function loadFlatTracks(
    mode: 'frequentlyPlayed' | 'recentlyPlayed' | 'favorites',
    reset: boolean,
) {
    const container = document.getElementById('library-content');
    if (!container) return;

    const key = cacheKey(undefined);

    if (reset) {
        const cached = state.pageCache.get(key);
        if (cached) {
            state.items = cached.items;
            state.pagination.total = cached.total;
            state.pagination.startIndex = 0;
            state.artistViewTotal = 0;
            renderCurrentView();
            restoreScroll(key);
            return;
        }

        await yieldTick();
        if (!container.isConnected) return;
        showSpinner(container);
    }

    state.loading = true;
    renderModeBar();
    try {
        const startIndex = reset ? 0 : state.pagination.startIndex;
        let result: { tracks: BrowseTrack[]; total: number };

        switch (mode) {
            case 'frequentlyPlayed':
                result = await fetchBrowseFrequentlyPlayed(undefined, startIndex, state.pagination.limit);
                break;
            case 'recentlyPlayed':
                result = await fetchBrowseRecentlyPlayed(undefined, startIndex, state.pagination.limit);
                break;
            case 'favorites':
                result = await fetchBrowseFavorites(undefined, startIndex, state.pagination.limit);
                break;
        }

        state.pagination.total = result.total;
        const mapped = mapFlatTracks(result.tracks, mode);

        if (reset) {
            state.items = mapped;
            state.artistViewTotal = 0;
            state.pageCache.set(key, { items: state.items, total: result.total });
        } else {
            state.items = [...state.items, ...mapped];
        }

        renderCurrentView();
        if (reset) restoreScroll(key);
    } catch (e) {
        renderError(e as Error);
    } finally {
        state.loading = false;
        renderModeBar();
    }
}

async function loadFavoriteArtists() {
    const container = document.getElementById('library-content');
    if (!container) return;

    const key = cacheKey(undefined);
    const cached = state.pageCache.get(key);
    if (cached) {
        state.items = cached.items;
        state.pagination.total = cached.total;
        state.pagination.startIndex = cached.total;
        renderCurrentView();
        restoreScroll(key);
        return;
    }

    await yieldTick();
    if (!container.isConnected) return;
    showSpinner(container);

    state.loading = true;
    renderModeBar();
    try {
        const tree = await ensureFavoriteTree();
        const artists = mapFavoriteArtists(tree);
        state.items = artists;
        state.pagination.total = artists.length;
        state.pagination.startIndex = artists.length;
        state.artistViewTotal = 0;
        state.albumViewTotal = 0;
        state.pageCache.set(key, { items: artists, total: artists.length });
        renderCurrentView();
        restoreScroll(key);
    } catch (e) {
        renderError(e as Error);
    } finally {
        state.loading = false;
        renderModeBar();
    }
}

async function loadFavoriteArtistAlbums(artistId: string) {
    const container = document.getElementById('library-content');
    if (!container) return;

    const key = cacheKey(artistId);
    const cached = state.pageCache.get(key);
    if (cached) {
        state.items = cached.items;
        state.pagination.total = cached.total;
        state.pagination.startIndex = cached.total;
        renderCurrentView();
        restoreScroll(key);
        return;
    }

    await yieldTick();
    if (!container.isConnected) return;
    showSpinner(container);

    state.loading = true;
    renderModeBar();
    try {
        const tree = await ensureFavoriteTree();
        const artistDirectFavorite = tree.favoriteArtistIds.has(artistId);
        const albums = artistDirectFavorite
            ? (await fetchBrowseArtist(artistId)).albums
            : favoriteAlbumsForArtist(tree, artistId);
        const mapped = mapFavoriteAlbums(albums, tree, artistDirectFavorite);
        state.items = mapped;
        state.pagination.total = mapped.length;
        state.pagination.startIndex = mapped.length;
        state.pageCache.set(key, { items: mapped, total: mapped.length });
        renderCurrentView();
        restoreScroll(key);
    } catch (e) {
        renderError(e as Error);
    } finally {
        state.loading = false;
        renderModeBar();
    }
}

async function loadFavoriteAlbumTracks(albumId: string) {
    const container = document.getElementById('library-content');
    if (!container) return;

    const key = cacheKey(albumId);
    const cached = state.pageCache.get(key);
    if (cached) {
        state.items = cached.items;
        state.pagination.total = cached.total;
        state.pagination.startIndex = cached.total;
        renderCurrentView();
        restoreScroll(key);
        return;
    }

    await yieldTick();
    if (!container.isConnected) return;
    showSpinner(container);

    state.loading = true;
    renderModeBar();
    try {
        const tree = await ensureFavoriteTree();
        const artistId = state.breadcrumbStack[0]?.id;
        const showFullAlbum =
            tree.favoriteAlbumIds.has(albumId) ||
            (artistId != null && tree.favoriteArtistIds.has(artistId));
        const tracks = showFullAlbum
            ? (await fetchBrowseAlbum(albumId)).tracks
            : favoriteTracksForAlbum(tree, albumId);
        const mapped = mapAlbumTracks(tracks);
        state.items = mapped;
        state.pagination.total = mapped.length;
        state.pagination.startIndex = mapped.length;
        state.pageCache.set(key, { items: mapped, total: mapped.length });
        renderCurrentView();
        restoreScroll(key);
    } catch (e) {
        renderError(e as Error);
    } finally {
        state.loading = false;
        renderModeBar();
    }
}

// --- Hierarchical navigation loaders ---

async function loadArtistAlbums(artistId: string) {
    const container = document.getElementById('library-content');
    if (!container) return;

    const key = cacheKey(artistId);

    const cached = state.pageCache.get(key);
    if (cached) {
        state.items = cached.items;
        state.pagination.total = cached.total;
        state.pagination.startIndex = cached.total;
        state.artistViewTotal = 0;
        renderCurrentView();
        restoreScroll(key);
        return;
    }

    await yieldTick();
    if (!container.isConnected) return;
    showSpinner(container);

    state.loading = true;
    renderModeBar();
    try {
        const result = await fetchBrowseArtist(artistId);
        const albums = mapAlbums(result.albums);
        state.items = albums;
        state.pagination.total = albums.length;
        state.pagination.startIndex = albums.length;
        state.artistViewTotal = 0;
        state.pageCache.set(key, { items: albums, total: albums.length });
        renderCurrentView();
        restoreScroll(key);
    } catch (e) {
        renderError(e as Error);
    } finally {
        state.loading = false;
        renderModeBar();
    }
}

async function loadAlbumTracks(albumId: string) {
    const container = document.getElementById('library-content');
    if (!container) return;

    const key = cacheKey(albumId);

    const cached = state.pageCache.get(key);
    if (cached) {
        state.items = cached.items;
        state.pagination.total = cached.total;
        state.pagination.startIndex = cached.total;
        state.artistViewTotal = 0;
        renderCurrentView();
        restoreScroll(key);
        return;
    }

    await yieldTick();
    if (!container.isConnected) return;
    showSpinner(container);

    state.loading = true;
    renderModeBar();
    try {
        const result = await fetchBrowseAlbum(albumId);
        const tracks = mapAlbumTracks(result.tracks);
        state.items = tracks;
        state.pagination.total = tracks.length;
        state.pagination.startIndex = tracks.length;
        state.artistViewTotal = 0;
        state.pageCache.set(key, { items: tracks, total: tracks.length });
        renderCurrentView();
        restoreScroll(key);
    } catch (e) {
        renderError(e as Error);
    } finally {
        state.loading = false;
        renderModeBar();
    }
}

async function loadPlaylistTracks(playlistId: string) {
    const container = document.getElementById('library-content');
    if (!container) return;

    await yieldTick();
    if (!container.isConnected) return;
    showSpinner(container);

    state.loading = true;
    renderModeBar();
    try {
        const result = await fetchBrowsePlaylist(playlistId);
        const tracks = mapFlatTracks(result.tracks);
        state.items = tracks;
        state.pagination.total = tracks.length;
        state.pagination.startIndex = tracks.length;
        state.artistViewTotal = 0;
        renderCurrentView();
    } catch (e) {
        renderError(e as Error);
    } finally {
        state.loading = false;
        renderModeBar();
    }
}

async function loadGenreTracks(genreIdOrName: string, reset: boolean) {
    const container = document.getElementById('library-content');
    if (!container) return;

    const key = cacheKey(genreIdOrName);

    if (reset) {
        const cached = state.pageCache.get(key);
        if (cached) {
            state.items = cached.items;
            state.pagination.total = cached.total;
            state.pagination.startIndex = 0;
            state.artistViewTotal = 0;
            renderCurrentView();
            restoreScroll(key);
            return;
        }

        await yieldTick();
        if (!container.isConnected) return;
        showSpinner(container);
    }

    state.loading = true;
    renderModeBar();
    try {
        const startIndex = reset ? 0 : state.pagination.startIndex;
        const result = await fetchBrowseGenre(genreIdOrName, startIndex, state.pagination.limit);

        state.pagination.total = result.total;
        const mapped = mapFlatTracks(result.tracks);

        if (reset) {
            state.items = mapped;
            state.artistViewTotal = 0;
            state.pageCache.set(key, { items: state.items, total: result.total });
        } else {
            state.items = [...state.items, ...mapped];
        }

        renderCurrentView();
        if (reset) restoreScroll(key);
    } catch (e) {
        renderError(e as Error);
    } finally {
        state.loading = false;
        renderModeBar();
    }
}

// --- Navigation ---

async function navigateToBrowseItem(item: BrowseDisplayItem) {
    switch (item.type) {
        case 'MusicArtist': await navigateToArtist(item.id, item.name); break;
        case 'MusicAlbum': await navigateToAlbum(item.id, item.name); break;
        case 'Playlist': await navigateToPlaylist(item.id, item.name); break;
        case 'MusicGenre': await navigateToGenre(item.id, item.name); break;
        case 'Audio': break; // leaf item — no drill-down
    }
}

async function navigateToArtist(artistId: string, artistName: string) {
    clearSelection();
    saveScroll();
    state.breadcrumbStack.push({ id: artistId, name: artistName });
    state.parentId = artistId;
    state.pagination.startIndex = 0;
    state.items = [];
    if (state.browseMode === 'favorites') {
        await loadFavoriteArtistAlbums(artistId);
        return;
    }
    await loadArtistAlbums(artistId);
}

async function navigateToAlbum(albumId: string, albumName: string) {
    clearSelection();
    saveScroll();
    state.breadcrumbStack.push({ id: albumId, name: albumName });
    state.parentId = albumId;
    state.pagination.startIndex = 0;
    state.items = [];
    if (state.browseMode === 'favorites') {
        await loadFavoriteAlbumTracks(albumId);
        return;
    }
    await loadAlbumTracks(albumId);
}

async function navigateToPlaylist(playlistId: string, playlistName: string) {
    clearSelection();
    saveScroll();
    state.breadcrumbStack.push({ id: playlistId, name: playlistName });
    state.parentId = playlistId;
    state.pagination.startIndex = 0;
    state.items = [];
    await loadPlaylistTracks(playlistId);
}

async function navigateToGenre(genreIdOrName: string, genreName: string) {
    clearSelection();
    saveScroll();
    state.breadcrumbStack.push({ id: genreIdOrName, name: genreName });
    state.parentId = genreIdOrName;
    state.pagination.startIndex = 0;
    state.items = [];
    await loadGenreTracks(genreIdOrName, true);
}

async function navigateToCrumb(index: number) {
    clearSelection();
    state.activeLetter = null;
    saveScroll();

    state.breadcrumbStack = state.breadcrumbStack.slice(0, index + 1);
    const target = state.breadcrumbStack[index];
    state.parentId = target.id;
    state.pagination.startIndex = 0;
    state.items = [];

    await reloadCurrentLevel();
}

async function reloadCurrentLevel() {
    const depth = state.breadcrumbStack.length;
    const parentId = state.parentId;

    if (depth === 0 || !parentId) {
        await loadModeRoot();
        return;
    }

    switch (state.browseMode) {
        case 'artists':
            if (depth === 1) await loadArtistAlbums(parentId);
            else await loadAlbumTracks(parentId);
            break;
        case 'favorites':
            if (depth === 1) await loadFavoriteArtistAlbums(parentId);
            else await loadFavoriteAlbumTracks(parentId);
            break;
        case 'albums':
            await loadAlbumTracks(parentId);
            break;
        case 'playlists':
            await loadPlaylistTracks(parentId);
            break;
        case 'genres':
            await loadGenreTracks(parentId, true);
            break;
        default:
            await loadModeRoot();
    }
}

// --- Load More ---

async function appendByLetter(mode: 'artists' | 'albums', letter: string) {
    const container = document.getElementById('library-content');
    if (!container) return;
    state.loading = true;
    renderModeBar();
    try {
        const startIndex = state.pagination.startIndex;
        const limit = state.pagination.limit;
        if (mode === 'artists') {
            const result = await fetchBrowseArtists(letter, undefined, startIndex, limit);
            if (!container.isConnected) return;
            state.items = [...state.items, ...mapArtists(result.artists)];
            state.pagination.total = result.total;
        } else {
            const result = await fetchBrowseAlbums(letter, undefined, startIndex, limit);
            if (!container.isConnected) return;
            state.items = [...state.items, ...mapAlbums(result.albums)];
            state.pagination.total = result.total;
        }
        state.pagination.startIndex = state.items.length;
        renderCurrentView();
    } catch (e) {
        renderError(e as Error);
    } finally {
        state.loading = false;
        renderModeBar();
    }
}

async function loadMore() {
    if (state.loading) return;
    if (state.browseMode === 'favorites') return;

    state.pagination.startIndex = state.items.length;

    const depth = state.breadcrumbStack.length;
    if (depth === 0) {
        switch (state.browseMode) {
            case 'artists':
                if (state.activeLetter) await appendByLetter('artists', state.activeLetter);
                else await loadArtists(false);
                break;
            case 'albums':
                if (state.activeLetter) await appendByLetter('albums', state.activeLetter);
                else await loadAlbums(false);
                break;
            case 'genres': await loadGenres(false); break;
            case 'recentlyAdded':
                await loadRecentlyAddedAlbums(false);
                break;
            case 'frequentlyPlayed':
            case 'recentlyPlayed':
                await loadFlatTracks(state.browseMode, false);
                break;
        }
    } else if (state.browseMode === 'genres' && depth === 1 && state.parentId) {
        await loadGenreTracks(state.parentId, false);
    }
}

// --- Entry point ---

export async function initLibraryView() {
    console.log('Initializing library view...');

    clearNavigationCache();

    const container = document.getElementById('library-content');
    if (container) {
        container.innerHTML = '<sl-spinner style="font-size: 3rem;"></sl-spinner>';
    }

    try {
        const modesResult = await fetchBrowseModes();

        state.availableModes = modesResult;
        const defaultMode: BrowseMode = modesResult.includes('artists')
            ? 'artists'
            : (modesResult[0] ?? 'artists');
        state.browseMode = defaultMode;
    } catch (e) {
        renderError(e as Error);
        return;
    }

    renderModeBar();
    await loadModeRoot();
}
