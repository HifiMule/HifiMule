// Library View - Handles Jellyfin library browsing and media grid display

import { rpcCall } from './rpc';
import { MediaCard, JellyfinItem, JellyfinView } from './components/MediaCard';

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
}

let state: AppState = {
    view: 'libraries',
    breadcrumbStack: [],
    items: [],
    pagination: { startIndex: 0, limit: 50, total: 0 },
    loading: false
};

export async function fetchViews(): Promise<JellyfinView[]> {
    return await rpcCall('jellyfin_get_views');
}

export async function fetchItems(
    parentId?: string,
    includeItemTypes?: string,
    startIndex?: number,
    limit: number = 50
): Promise<JellyfinItemsResponse> {
    return await rpcCall('jellyfin_get_items', {
        parentId,
        includeItemTypes,
        startIndex,
        limit
    });
}

export async function fetchDeviceStatusMap(): Promise<DeviceStatusMap> {
    return await rpcCall('sync_get_device_status_map');
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
    state.view = 'libraries';
    state.breadcrumbStack = [];
    state.items = [];

    const container = document.getElementById('library-content');
    if (!container) return;

    container.innerHTML = '<sl-spinner style="font-size: 3rem;"></sl-spinner>';

    try {
        const views = await fetchViews();
        renderGrid(views, 'libraries');
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
    const containerTypes = ['MusicAlbum', 'Playlist', 'Folder', 'CollectionFolder', 'BoxSet', 'Series', 'Season'];
    if (containerTypes.includes(item.Type)) {
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

    if (reset) {
        container.innerHTML = '<sl-spinner style="font-size: 3rem;"></sl-spinner>';
    }

    state.loading = true;
    try {
        const [itemsResponse, deviceStatus] = await Promise.all([
            fetchItems(state.parentId, undefined, state.pagination.startIndex, state.pagination.limit),
            fetchDeviceStatusMap()
        ]);

        state.pagination.total = itemsResponse.TotalRecordCount;

        if (reset) {
            state.items = itemsResponse.Items;
        } else {
            state.items = [...state.items, ...itemsResponse.Items];
        }

        renderGrid(state.items, 'items', deviceStatus);

    } catch (e) {
        renderError(e as Error);
    } finally {
        state.loading = false;
    }
}

async function loadMore() {
    state.pagination.startIndex += state.pagination.limit;
    await loadItems(false);
}

function renderGrid(items: (JellyfinItem | JellyfinView)[], mode: 'libraries' | 'items', deviceStatus?: DeviceStatusMap) {
    const container = document.getElementById('library-content');
    if (!container) return;

    container.innerHTML = '';

    // Breadcrumbs
    if (mode === 'items') {
        container.appendChild(createBreadcrumbs());
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
        grid.appendChild(card);
    });

    container.appendChild(grid);

    // Pagination controls
    if (mode === 'items' && state.items.length < state.pagination.total) {
        const loadMoreContainer = document.createElement('div');
        loadMoreContainer.className = 'load-more-container';

        const loadMoreBtn = document.createElement('sl-button') as any;
        loadMoreBtn.className = 'load-more-btn';
        loadMoreBtn.textContent = `Load More (${state.pagination.total - state.items.length} remaining)`;
        loadMoreBtn.onclick = () => loadMore();

        if (state.loading) {
            loadMoreBtn.loading = true;
        }

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
