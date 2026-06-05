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
} from './rpc';
import { MediaCard, BrowseDisplayItem } from './components/MediaCard';
import { basketStore } from './state/basket';
import { t } from './i18n';

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
    scrollCache: Map<string, number>;
    pageCache: Map<string, { items: BrowseDisplayItem[]; total: number }>;
    artistViewTotal: number;
    albumViewTotal: number;
    activeLetter: string | null;
    favoriteTree: FavoriteTree | null;
    listViewModes: Map<BrowseMode, 'grid' | 'list'>;
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
    scrollCache: new Map(),
    pageCache: new Map(),
    artistViewTotal: 0,
    albumViewTotal: 0,
    activeLetter: null,
    favoriteTree: null,
    listViewModes: new Map(),
};

function cacheKey(parentId?: string): string {
    return `${state.browseMode}:${parentId ?? 'root'}`;
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
    // browseMode, availableModes, and listViewModes are intentionally preserved
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
            const inListView = (state.listViewModes.get(state.browseMode) ?? 'grid') === 'list';
            if (inListView) {
                scrollToLetter(letter);
            } else if (isArtists) {
                loadArtistsByLetter(letter);
            } else {
                loadAlbumsByLetter(letter);
            }
        });
        navBar.appendChild(btn);
    }

    return navBar;
}

function renderGrid(items: BrowseDisplayItem[]) {
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

        const card = MediaCard.create(item, 'items', false, () => navigateToBrowseItem(item), selEnabled);
        card.setAttribute('data-name', item.name);
        grid.appendChild(card);
    });

    container.appendChild(grid);

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
    }
}

function renderViewToggle() {
    const container = document.getElementById('browse-mode-bar');
    if (!container) return;
    const existing = container.querySelector('.view-toggle-group');
    if (existing) existing.remove();
    const showToggle =
        (state.browseMode === 'artists' || state.browseMode === 'albums') &&
        state.breadcrumbStack.length === 0 &&
        !state.loading;
    if (!showToggle) return;
    const currentMode = state.listViewModes.get(state.browseMode) ?? 'grid';
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

async function setViewMode(mode: 'grid' | 'list') {
    if (state.loading) return;
    state.listViewModes.set(state.browseMode, mode);
    if (
        mode === 'list' &&
        (state.browseMode === 'artists' || state.browseMode === 'albums') &&
        state.items.length < state.pagination.total &&
        !state.activeLetter
    ) {
        state.loading = true;
        renderModeBar();
        try {
            await loadAllForListView(state.browseMode as 'artists' | 'albums');
        } finally {
            state.loading = false;
            renderModeBar();
        }
    }
    renderCurrentView();
}

function scrollToLetter(letter: string) {
    const container = document.getElementById('library-content');
    if (!container) return;
    const isHash = letter === '#';
    const idx = state.items.findIndex(item => {
        const first = item.name.charAt(0).toUpperCase();
        return isHash ? /[0-9]/.test(first) : first === letter;
    });
    if (idx >= 0) container.scrollTop = idx * VIRTUAL_ROW_HEIGHT;
}

function renderListRow(item: BrowseDisplayItem, index: number): HTMLElement {
    const itemId = item.basketId ?? item.id;
    const isSelected = basketStore.has(itemId);
    const row = document.createElement('div');
    row.className = 'media-list-row';
    if (isSelected) row.classList.add('is-selected');
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
    toggleBtn.label = isSelected ? 'Remove from basket' : 'Add to basket';
    toggleBtn.style.fontSize = '1.25rem';
    toggleBtn.addEventListener('click', (e: Event) => {
        e.stopPropagation();
        if (basketStore.has(itemId)) {
            basketStore.remove(itemId);
        } else {
            const resolvedType = item.basketType ?? item.type;
            const CONTAINER_TYPES = ['MusicArtist', 'MusicAlbum', 'MusicGenre', 'Playlist'];
            const isFavoriteScoped = resolvedType === 'FavoriteArtist' || resolvedType === 'FavoriteAlbum';
            const needsFetch = CONTAINER_TYPES.includes(resolvedType) && !isFavoriteScoped && (!item.childCount || !item.sizeBytes);
            if (!needsFetch) {
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
    });
    row.addEventListener('click', (e) => {
        const path = e.composedPath();
        const isBtn = path.some(el => (el as HTMLElement).tagName === 'SL-ICON-BUTTON');
        if (!isBtn) navigateToBrowseItem(item);
    });
    row.appendChild(thumb);
    row.appendChild(info);
    row.appendChild(toggleBtn);
    return row;
}

function renderList(items: BrowseDisplayItem[]) {
    const container = document.getElementById('library-content');
    if (!container) return;
    teardownListScrollHandler();
    container.innerHTML = '';
    if (state.breadcrumbStack.length > 0) container.appendChild(createBreadcrumbs());
    const qn = renderQuickNav();
    if (qn) container.appendChild(qn);
    const totalHeight = items.length * VIRTUAL_ROW_HEIGHT;
    const scroller = document.createElement('div');
    scroller.className = 'media-list';
    scroller.style.height = `${totalHeight}px`;
    function paint() {
        const scrollTop = container.scrollTop;
        const viewportH = container.clientHeight;
        const first = Math.max(0, Math.floor(scrollTop / VIRTUAL_ROW_HEIGHT) - OVERSCAN);
        const last = Math.min(items.length - 1, Math.ceil((scrollTop + viewportH) / VIRTUAL_ROW_HEIGHT) + OVERSCAN);
        scroller.querySelectorAll<HTMLElement>('.media-list-row').forEach(row => {
            const idx = Number(row.dataset.idx);
            if (idx < first || idx > last) row.remove();
        });
        const existing = new Set(
            [...scroller.querySelectorAll<HTMLElement>('.media-list-row')].map(r => Number(r.dataset.idx))
        );
        for (let i = first; i <= last; i++) {
            if (!existing.has(i)) scroller.appendChild(renderListRow(items[i], i));
        }
    }
    const scrollHandler = () => paint();
    container.addEventListener('scroll', scrollHandler);
    (container as any).__listScrollHandler = scrollHandler;
    const basketUpdateHandler = () => {
        scroller.querySelectorAll('.media-list-row').forEach(r => r.remove());
        paint();
    };
    basketStore.addEventListener('update', basketUpdateHandler);
    (container as any).__listBasketHandler = basketUpdateHandler;
    container.appendChild(scroller);
    paint();
}

async function loadAllForListView(mode: 'artists' | 'albums') {
    const container = document.getElementById('library-content');
    if (!container) return;
    container.innerHTML = '<sl-spinner style="font-size: 3rem;"></sl-spinner>';
    while (state.items.length < state.pagination.total) {
        const startIndex = state.items.length;
        if (mode === 'artists') {
            const r = await fetchBrowseArtists(undefined, undefined, startIndex, 200);
            state.items = [...state.items, ...mapArtists(r.artists)];
            state.pagination.total = r.total;
        } else {
            const r = await fetchBrowseAlbums(undefined, undefined, startIndex, 200);
            state.items = [...state.items, ...mapAlbums(r.albums)];
            state.pagination.total = r.total;
        }
    }
}

function renderCurrentView() {
    const mode = state.listViewModes.get(state.browseMode) ?? 'grid';
    if (
        mode === 'list' &&
        (state.browseMode === 'artists' || state.browseMode === 'albums') &&
        state.breadcrumbStack.length === 0
    ) {
        renderList(state.items);
    } else {
        renderGrid(state.items);
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

    saveScroll();
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
            state.pagination.startIndex = 0;
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

    if (state.activeLetter === letter) {
        state.activeLetter = null;
        await loadArtists(true);
        return;
    }

    state.activeLetter = letter;
    state.loading = true;
    renderModeBar();

    container.innerHTML = '<sl-spinner style="font-size: 3rem;"></sl-spinner>';
    await yieldTick();
    if (!container.isConnected) {
        state.loading = false;
        return;
    }

    try {
        const result = await fetchBrowseArtists(letter, undefined, 0, 200);
        state.items = mapArtists(result.artists);
        state.pagination.total = result.total;
        state.pagination.startIndex = result.artists.length;
        renderCurrentView();
    } catch (e) {
        renderError(e as Error);
    } finally {
        state.loading = false;
        renderModeBar();
    }
}

async function loadAlbumsByLetter(letter: string) {
    const container = document.getElementById('library-content');
    if (!container || state.loading) return;

    if (state.activeLetter === letter) {
        state.activeLetter = null;
        await loadAlbums(true);
        return;
    }

    state.activeLetter = letter;
    state.loading = true;
    renderModeBar();

    container.innerHTML = '<sl-spinner style="font-size: 3rem;"></sl-spinner>';
    await yieldTick();
    if (!container.isConnected) {
        state.loading = false;
        return;
    }

    try {
        const result = await fetchBrowseAlbums(letter, undefined, 0, 200);
        state.items = mapAlbums(result.albums);
        state.pagination.total = result.total;
        state.pagination.startIndex = result.albums.length;
        renderCurrentView();
    } catch (e) {
        renderError(e as Error);
    } finally {
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
            state.pagination.startIndex = 0;
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

async function loadPlaylists() {
    const container = document.getElementById('library-content');
    if (!container) return;

    const key = cacheKey(undefined);

    const cached = state.pageCache.get(key);
    if (cached) {
        state.items = cached.items;
        state.pagination.total = cached.total;
        state.artistViewTotal = 0;
        renderGrid(state.items);
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
        renderGrid(state.items);
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
            renderGrid(state.items);
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

        renderGrid(state.items);
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
            renderGrid(state.items);
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

        renderGrid(state.items);
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
            renderGrid(state.items);
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

        renderGrid(state.items);
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
        renderGrid(state.items);
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
        renderGrid(state.items);
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
        renderGrid(state.items);
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
        renderGrid(state.items);
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
        renderGrid(state.items);
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
        renderGrid(state.items);
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
        renderGrid(state.items);
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
        renderGrid(state.items);
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
        renderGrid(state.items);
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
        renderGrid(state.items);
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

    const key = cacheKey(playlistId);

    const cached = state.pageCache.get(key);
    if (cached) {
        state.items = cached.items;
        state.pagination.total = cached.total;
        state.pagination.startIndex = cached.total;
        state.artistViewTotal = 0;
        renderGrid(state.items);
        restoreScroll(key);
        return;
    }

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
        state.pageCache.set(key, { items: tracks, total: tracks.length });
        renderGrid(state.items);
        restoreScroll(key);
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
            renderGrid(state.items);
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

        renderGrid(state.items);
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
    saveScroll();
    state.breadcrumbStack.push({ id: playlistId, name: playlistName });
    state.parentId = playlistId;
    state.pagination.startIndex = 0;
    state.items = [];
    await loadPlaylistTracks(playlistId);
}

async function navigateToGenre(genreIdOrName: string, genreName: string) {
    saveScroll();
    state.breadcrumbStack.push({ id: genreIdOrName, name: genreName });
    state.parentId = genreIdOrName;
    state.pagination.startIndex = 0;
    state.items = [];
    await loadGenreTracks(genreIdOrName, true);
}

async function navigateToCrumb(index: number) {
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
        renderGrid(state.items);
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
