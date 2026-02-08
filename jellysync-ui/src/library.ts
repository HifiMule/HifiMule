// Library View - Handles Jellyfin library browsing and media grid display

import { rpcCall, IMAGE_PROXY_URL } from './rpc';

interface JellyfinView {
    Id: string;
    Name: string;
    Type: string;
}

interface JellyfinItem {
    Id: string;
    Name: string;
    Type: string;
    AlbumArtist?: string;
    ProductionYear?: number;
}

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

// RPC Helper removed - imported from rpc.ts

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

// ... UI Components ...

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

function createMediaCard(item: JellyfinItem | JellyfinView, isSynced: boolean, onClick: () => void): HTMLElement {
    const card = document.createElement('sl-card');
    card.className = 'media-card';
    card.onclick = onClick;

    if (isSynced) {
        card.classList.add('synced');
    }

    // Use proxy URL - Append timestamp to prevent aggressive caching if needed, but for now ID is enough
    const imageUrl = `${IMAGE_PROXY_URL}/${item.Id}?maxHeight=300&quality=90`;

    card.innerHTML = `
        <div class="card-image" style="background-image: url('${imageUrl}');">
            ${isSynced ? '<div class="synced-badge"><sl-icon name="check-circle-fill"></sl-icon></div>' : ''}
            <sl-skeleton effect="sheen" class="image-skeleton"></sl-skeleton>
        </div>
        <div class="card-content">
            <strong>${escapeHtml(item.Name)}</strong>
            ${(item as JellyfinItem).AlbumArtist ? `<div class="card-subtitle">${escapeHtml((item as JellyfinItem).AlbumArtist!)}</div>` : ''}
            ${(item as JellyfinItem).ProductionYear ? `<div class="card-year">${(item as JellyfinItem).ProductionYear}</div>` : ''}
             ${(item as JellyfinView).Type === 'CollectionFolder' ? '<div class="card-subtitle">Library</div>' : ''}
        </div>
    `;

    // Handle image load
    const img = new Image();
    img.onload = () => {
        const cardImage = card.querySelector('.card-image') as HTMLElement;
        const skeleton = card.querySelector('.image-skeleton') as HTMLElement;
        if (cardImage && skeleton) {
            cardImage.style.backgroundImage = `url('${imageUrl}')`;
            skeleton.style.display = 'none';
        }
    };
    img.onerror = () => {
        const skeleton = card.querySelector('.image-skeleton') as HTMLElement;
        if (skeleton) {
            skeleton.style.display = 'none';
            // cardImage.style.backgroundImage = 'url(/assets/placeholder.png)'; // Optional fallback
        }
    };
    img.src = imageUrl;

    return card;
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

        const card = createMediaCard(item, isSynced, onClick);
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
            <sl-button onclick="initLibraryView()">Retry</sl-button>
        </div>
    `;
}

function escapeHtml(text: string): string {
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
}

// Global scope init for module
// Make initLibraryView global so main.ts can access it if imported as module, 
// but typically main calls it directly if imported.
export function initLibraryView() {
    console.log('Initializing library view...');
    renderLibrarySelection();
}
