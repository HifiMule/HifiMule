// Library View - Handles Jellyfin library browsing and media grid display

const RPC_PORT = (import.meta as any).env?.VITE_RPC_PORT || '19140';
const RPC_URL = `http://localhost:${RPC_PORT}`;

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

// RPC Helper
async function rpcCall(method: string, params: any = {}): Promise<any> {
    const response = await fetch(RPC_URL, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
            jsonrpc: '2.0',
            method,
            params,
            id: Date.now()
        })
    });

    const data = await response.json();
    if (data.error) {
        throw new Error(data.error.message);
    }
    return data.result;
}

// Fetch Jellyfin views (libraries)
export async function fetchViews(): Promise<JellyfinView[]> {
    return await rpcCall('jellyfin_get_views');
}

// Fetch items (albums/playlists) from a library
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

// Fetch item details
export async function fetchItemDetails(itemId: string): Promise<JellyfinItem> {
    return await rpcCall('jellyfin_get_item_details', { itemId });
}

// Fetch synced item IDs from device
export async function fetchDeviceStatusMap(): Promise<DeviceStatusMap> {
    return await rpcCall('sync_get_device_status_map');
}

// Render media card
export function createMediaCard(item: JellyfinItem, isSynced: boolean): HTMLElement {
    const card = document.createElement('sl-card');
    card.className = 'media-card';

    // Add synced indicator
    if (isSynced) {
        card.classList.add('synced');
    }

    // Image placeholder (will be replaced with actual Jellyfin image URL)
    const imageUrl = `/api/Items/${item.Id}/Images/Primary?maxHeight=300&quality=90`;

    card.innerHTML = `
        <div class="card-image" style="background-image: url('${imageUrl}');">
            ${isSynced ? '<div class="synced-badge"><sl-icon name="check-circle-fill"></sl-icon></div>' : ''}
            <sl-skeleton effect="sheen" class="image-skeleton"></sl-skeleton>
        </div>
        <div class="card-content">
            <strong>${escapeHtml(item.Name)}</strong>
            ${item.AlbumArtist ? `<div class="card-subtitle">${escapeHtml(item.AlbumArtist)}</div>` : ''}
            ${item.ProductionYear ? `<div class="card-year">${item.ProductionYear}</div>` : ''}
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
        }
    };
    img.src = imageUrl;

    return card;
}

// Render media grid
export async function renderMediaGrid(containerId: string, parentId?: string, includeItemTypes: string = 'MusicAlbum,Playlist') {
    const container = document.getElementById(containerId);
    if (!container) {
        console.error(`Container ${containerId} not found`);
        return;
    }

    try {
        // Show loading state
        container.innerHTML = '<sl-spinner style="font-size: 3rem;"></sl-spinner>';

        // Fetch items and device status in parallel
        const [itemsResponse, deviceStatus] = await Promise.all([
            fetchItems(parentId, includeItemTypes, 0, 50),
            fetchDeviceStatusMap()
        ]);

        const syncedIds = new Set(deviceStatus.syncedItemIds);

        // Clear container and render grid
        container.innerHTML = '';
        const grid = document.createElement('div');
        grid.className = 'media-grid';

        itemsResponse.Items.forEach(item => {
            const isSynced = syncedIds.has(item.Id);
            const card = createMediaCard(item, isSynced);
            grid.appendChild(card);
        });

        container.appendChild(grid);

        // Add pagination info if needed
        if (itemsResponse.TotalRecordCount > itemsResponse.Items.length) {
            const info = document.createElement('p');
            info.className = 'pagination-info';
            info.textContent = `Showing ${itemsResponse.Items.length} of ${itemsResponse.TotalRecordCount} items`;
            container.appendChild(info);
        }

    } catch (error) {
        console.error('Failed to render media grid:', error);
        container.innerHTML = `
            <div class="error-message">
                <sl-icon name="exclamation-triangle" style="font-size: 2rem;"></sl-icon>
                <p>Failed to load library: ${(error as Error).message}</p>
            </div>
        `;
    }
}

// HTML escape utility
function escapeHtml(text: string): string {
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
}

// Initialize library view
export function initLibraryView() {
    console.log('Initializing library view...');

    // Render the default view (Music library)
    renderMediaGrid('library-content', undefined, 'MusicAlbum,Playlist');
}
