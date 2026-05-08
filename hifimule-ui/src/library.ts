import { rpcCall } from './rpc';
import { MediaCard, JellyfinItem, JellyfinView } from './components/MediaCard';

const MUSIC_ITEM_TYPES = 'MusicAlbum,Playlist,MusicArtist,Audio,MusicVideo';
const ALLOWED_COLLECTION_TYPES = ['music', 'playlists'];

interface JellyfinItemsResponse {
    Items: JellyfinItem[];
    TotalRecordCount: number;
    StartIndex: number;
}

interface DeviceStatusMap {
    syncedItemIds: string[];
}

interface AppState {
    view: 'libraries' | 'items';
    libraryId?: string; // Root library ID
    parentId?: string; // Current folder ID
    breadcrumbStack: { id: string, name: string }[];
    items: JellyfinItem[];
    pagination: {
        startIndex: number;
        limit: number;
        total: number;
    };
    loading: boolean;
    scrollCache: Map<string, number>;
    pageCache: Map<string, { items: JellyfinItem[]; total: number }>;
    artistViewTotal: number; // Total MusicArtist count for the current parent; 0 when not in artist view
    activeLetter: string | null; // Currently selected quick-nav letter, null = no filter
}

let state: AppState = {
    view: 'libraries',
    breadcrumbStack: [],
    items: [],
    pagination: { startIndex: 0, limit: 50, total: 0 },
    loading: false,
    scrollCache: new Map(),
    pageCache: new Map(),
    artistViewTotal: 0,
    activeLetter: null,
};

export async function fetchViews(): Promise<JellyfinView[]> {
    return await rpcCall('jellyfin_get_views');
}

export async function fetchItems(
    parentId?: string,
    includeItemTypes?: string,
    startIndex?: number,
    limit: number = 50,
    nameStartsWith?: string,
    nameLessThan?: string,
): Promise<JellyfinItemsResponse> {
    return await rpcCall('jellyfin_get_items', {
        parentId,
        includeItemTypes,
        startIndex,
        limit,
        ...(nameStartsWith !== undefined && { nameStartsWith }),
        ...(nameLessThan !== undefined && { nameLessThan }),
    });
}

export async function fetchDeviceStatusMap(): Promise<DeviceStatusMap> {
    return await rpcCall('sync_get_device_status_map');
}

export function clearNavigationCache() {
    state.scrollCache = new Map();
    state.pageCache = new Map();
    state.breadcrumbStack = [];
    state.items = [];
    state.pagination = { startIndex: 0, limit: 50, total: 0 };
    state.artistViewTotal = 0;
    state.activeLetter = null;
}

function createBreadcrumbs(): HTMLElement {
    const nav = document.createElement('nav');
    nav.className = 'breadcrumbs';

    // Home button
    const homeBtn = document.createElement('sl-button') as any;
    homeBtn.variant = 'text';
    homeBtn.size = 'small';
    homeBtn.innerHTML = '<sl-icon slot="prefix" name="house"></sl-icon> Libraries';
    homeBtn.onclick = () => renderLibrarySelection();
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
        // Navigate back to this crumb
        if (index < state.breadcrumbStack.length - 1) {
            btn.onclick = () => navigateToCrumb(index);
        } else {
            btn.disabled = true; // Current page
        }
        nav.appendChild(btn);
    });

    return nav;
}

async function renderLibrarySelection() {
    clearNavigationCache();
    state.view = 'libraries';

    const container = document.getElementById('library-content');
    if (!container) return;

    try {
        const views = await fetchViews();
        // Filter for music and playlists collection types only
        const musicViews = views.filter(view =>
            view.CollectionType && ALLOWED_COLLECTION_TYPES.includes(view.CollectionType.toLowerCase())
        );
        renderGrid(musicViews, 'libraries');
    } catch (e) {
        renderError(e as Error);
    }
}

async function navigateToLibrary(view: JellyfinView) {
    state.view = 'items';
    state.libraryId = view.Id;
    state.parentId = view.Id;
    state.breadcrumbStack = [{ id: view.Id, name: view.Name }];
    state.items = [];
    state.pagination.startIndex = 0;

    await loadItems(true);
}

async function navigateToCrumb(index: number) {
    state.activeLetter = null;
    // Save scroll position for the page being left
    const libraryView = document.querySelector('.library-view') as HTMLElement;
    if (libraryView && state.parentId) {
        state.scrollCache.set(state.parentId, libraryView.scrollTop);
    }

    // Truncate stack
    state.breadcrumbStack = state.breadcrumbStack.slice(0, index + 1);
    const target = state.breadcrumbStack[index];
    state.parentId = target.id;
    state.pagination.startIndex = 0;
    state.items = [];
    await loadItems(true);
}

async function navigateToItem(item: JellyfinItem) {
    // Only navigate if container
    // Common Jellyfin container types
    const containerTypes = ['MusicArtist', 'MusicAlbum', 'Playlist', 'Folder', 'CollectionFolder', 'BoxSet', 'Series', 'Season']; // MusicArtist: navigates into artist's albums
    if (containerTypes.includes(item.Type)) {
        state.activeLetter = null;
        // Save scroll position for the page being left
        const libraryView = document.querySelector('.library-view') as HTMLElement;
        if (libraryView && state.parentId) {
            state.scrollCache.set(state.parentId, libraryView.scrollTop);
        }

        state.parentId = item.Id;
        state.breadcrumbStack.push({ id: item.Id, name: item.Name });
        state.pagination.startIndex = 0;
        state.items = [];
        await loadItems(true);
    } else {
        console.log("Clicked leaf item", item);
        // Could trigger play or add to basket here
    }
}

async function loadItems(reset: boolean) {
    const container = document.getElementById('library-content');
    if (!container) return;

    const targetParentId = state.parentId;

    // Cache hit path: render from page cache instantly (no spinner, no items re-fetch)
    if (reset && targetParentId) {
        const cached = state.pageCache.get(targetParentId);
        if (cached) {
            state.loading = true;
            try {
                const deviceStatus = await fetchDeviceStatusMap();
                state.items = cached.items;
                state.pagination.total = cached.total;
                state.pagination.startIndex = 0;
                state.artistViewTotal = state.items[0]?.Type === 'MusicArtist' ? cached.total : 0;
                renderGrid(state.items, 'items', deviceStatus);
                // Restore scroll position after DOM is painted
                const cachedScroll = state.scrollCache.get(targetParentId);
                if (cachedScroll !== undefined) {
                    requestAnimationFrame(() => {
                        const libraryView = document.querySelector('.library-view') as HTMLElement;
                        if (libraryView) libraryView.scrollTop = cachedScroll;
                        state.scrollCache.delete(targetParentId);
                    });
                }
            } catch (e) {
                renderError(e as Error);
            } finally {
                state.loading = false;
            }
            return;
        }
    }

    if (reset) {
        // Yield one tick so any click-feedback (card overlay, opacity) renders before we wipe the container.
        await new Promise<void>(resolve => setTimeout(resolve, 0));
        if (!container.isConnected) return;

        // Only show the big spinner if a card isn't already showing a loading state
        if (!container.querySelector('.is-navigating')) {
            container.innerHTML = '<sl-spinner style="font-size: 3rem;"></sl-spinner>';
        }
    }

    state.loading = true;
    try {
        const [itemsResponse, deviceStatus] = await Promise.all([
            fetchItems(targetParentId, MUSIC_ITEM_TYPES, state.pagination.startIndex, state.pagination.limit),
            fetchDeviceStatusMap()
        ]);

        state.pagination.total = itemsResponse.TotalRecordCount;

        if (reset) {
            state.items = itemsResponse.Items;
            // Track whether this is an artist view so the quick-nav bar visibility
            // survives letter-filtered re-renders (which return fewer than 20 items)
            state.artistViewTotal = state.items[0]?.Type === 'MusicArtist'
                ? state.pagination.total
                : 0;
            // Cache the page for back-navigation
            if (targetParentId) {
                state.pageCache.set(targetParentId, { items: state.items, total: state.pagination.total });
            }
        } else {
            state.items = [...state.items, ...itemsResponse.Items];
        }

        renderGrid(state.items, 'items', deviceStatus);

        // Restore scroll position after DOM is painted (for back-navigation)
        if (reset && targetParentId) {
            const cachedScroll = state.scrollCache.get(targetParentId);
            if (cachedScroll !== undefined) {
                requestAnimationFrame(() => {
                    const libraryView = document.querySelector('.library-view') as HTMLElement;
                    if (libraryView) libraryView.scrollTop = cachedScroll;
                    state.scrollCache.delete(targetParentId);
                });
            }
        }

    } catch (e) {
        renderError(e as Error);
    } finally {
        state.loading = false;
    }
}

async function loadMore() {
    if (state.loading) return;
    state.pagination.startIndex += state.pagination.limit;
    await loadItems(false);
}

async function loadItemsByLetter(letter: string) {
    const container = document.getElementById('library-content');
    if (!container || state.loading) return;

    // Clicking the active letter clears the filter and reloads all artists
    if (state.activeLetter === letter) {
        state.activeLetter = null;
        await loadItems(true);
        return;
    }
    state.activeLetter = letter;
    state.loading = true;

    // '#' = non-alpha names (sort before 'A' in Jellyfin)
    const nameStartsWith = letter === '#' ? undefined : letter;
    const nameLessThan = letter === '#' ? 'A' : undefined;

    container.innerHTML = '<sl-spinner style="font-size: 3rem;"></sl-spinner>';
    // Yield to the event loop so any pending mouseup from the letter-button click
    // fires on the spinner before we render cards, preventing click-through navigation.
    await new Promise<void>(resolve => setTimeout(resolve, 0));
    if (!container.isConnected) {
        state.loading = false;
        return;
    }

    try {
        const [itemsResponse, deviceStatus] = await Promise.all([
            fetchItems(state.parentId, MUSIC_ITEM_TYPES, 0, 200, nameStartsWith, nameLessThan),
            fetchDeviceStatusMap()
        ]);
        state.items = itemsResponse.Items;
        // Do not overwrite state.pagination — Load More is suppressed while activeLetter is set
        renderGrid(state.items, 'items', deviceStatus);
    } catch (e) {
        renderError(e as Error);
    } finally {
        state.loading = false;
    }
}

function renderQuickNav(): HTMLElement | null {
    if (state.artistViewTotal < 20) return null;

    const allLetters = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ'.split('').concat('#');

    const navBar = document.createElement('div');
    navBar.className = 'quick-nav-bar';

    for (const letter of allLetters) {
        const btn = document.createElement('sl-button') as any;
        btn.size = 'small';
        btn.variant = letter === state.activeLetter ? 'primary' : 'text';
        btn.textContent = letter;
        btn.addEventListener('click', () => loadItemsByLetter(letter));
        navBar.appendChild(btn);
    }

    return navBar;
}

function renderGrid(items: (JellyfinItem | JellyfinView)[], mode: 'libraries' | 'items', deviceStatus?: DeviceStatusMap) {
    const container = document.getElementById('library-content');
    if (!container) return;

    container.innerHTML = '';

    // Breadcrumbs
    if (mode === 'items') {
        container.appendChild(createBreadcrumbs());
        // Quick-nav bar (only for MusicArtist views with 20+ total items)
        const quickNav = renderQuickNav();
        if (quickNav) container.appendChild(quickNav);
    }

    const grid = document.createElement('div');
    grid.className = 'media-grid';

    const syncedIds = new Set(deviceStatus?.syncedItemIds || []);

    items.forEach(item => {
        const isSynced = syncedIds.has(item.Id);
        const onClick = mode === 'libraries'
            ? () => navigateToLibrary(item as JellyfinView)
            : () => navigateToItem(item as JellyfinItem);

        const card = MediaCard.create(item, mode, isSynced, onClick);
        card.setAttribute('data-name', (item as JellyfinItem).Name || '');
        grid.appendChild(card);
    });

    container.appendChild(grid);

    // Pagination controls — hidden during letter-filtered views
    if (mode === 'items' && state.activeLetter === null && state.items.length < state.pagination.total) {
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

// Global scope init for module
// Make initLibraryView global so main.ts can access it if imported as module,
// but typically main calls it directly if imported.
export function initLibraryView() {
    console.log('Initializing library view...');
    renderLibrarySelection();
}
