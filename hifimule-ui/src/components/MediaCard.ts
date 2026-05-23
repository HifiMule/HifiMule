// MediaCard Component
// Handles rendering of media items in the grid with selection support.

import { basketStore } from '../state/basket';
import { getImageUrl, rpcCall } from '../rpc';

export interface JellyfinItem {
    Id: string;
    Name: string;
    Type: string;
    ImageId?: string;
    AlbumArtist?: string;
    ProductionYear?: number;
}

export interface JellyfinView {
    Id: string;
    Name: string;
    Type: string;
    ImageId?: string;
    CollectionType?: string;
}

export interface BrowseDisplayItem {
    id: string;
    name: string;
    type: 'MusicArtist' | 'MusicAlbum' | 'Playlist' | 'Audio' | 'MusicGenre';
    basketId?: string;
    basketType?: string;
    coverArtId?: string | null;
    subtitle?: string | null;
    year?: number | null;
    childCount?: number;
    sizeBytes?: number;
    sizeTicks?: number;
}

export class MediaCard {
    public static create(
        item: JellyfinItem | JellyfinView | BrowseDisplayItem,
        mode: 'libraries' | 'items',
        isSynced: boolean,
        onNavigate: () => void | Promise<void>,
        deviceSelectionEnabled?: boolean,
    ): HTMLElement {
        const isBrowseItem = !('Id' in item);
        const itemId = isBrowseItem ? ((item as BrowseDisplayItem).basketId ?? (item as BrowseDisplayItem).id) : (item as JellyfinItem | JellyfinView).Id;
        const itemName = isBrowseItem ? (item as BrowseDisplayItem).name : (item as JellyfinItem | JellyfinView).Name;
        const selectionAllowed = deviceSelectionEnabled !== false;

        const card = document.createElement('sl-card');
        card.className = 'media-card';
        if (isSynced) card.classList.add('synced');

        const isSelected = basketStore.has(itemId);
        if (isSelected) card.classList.add('is-selected');

        const showSelection = isBrowseItem ? true : mode === 'items';
        const btnDisabled = !selectionAllowed;

        let subtitleHtml = '';
        let yearHtml = '';
        if (isBrowseItem) {
            const bi = item as BrowseDisplayItem;
            if (bi.subtitle) subtitleHtml = `<div class="card-subtitle">${this.escapeHtml(bi.subtitle)}</div>`;
            if (bi.year) yearHtml = `<div class="card-year">${bi.year}</div>`;
        } else {
            const ji = item as JellyfinItem;
            if (ji.AlbumArtist) subtitleHtml = `<div class="card-subtitle">${this.escapeHtml(ji.AlbumArtist)}</div>`;
            if (ji.ProductionYear) yearHtml = `<div class="card-year">${ji.ProductionYear}</div>`;
            if ((item as JellyfinView).Type === 'CollectionFolder') subtitleHtml = '<div class="card-subtitle">Library</div>';
        }

        card.innerHTML = `
            <div class="card-image">
                ${isSynced ? '<div class="synced-badge"><sl-icon name="check-circle-fill"></sl-icon></div>' : ''}

                ${showSelection ? `
                    <div class="selection-overlay">
                        <sl-icon-button
                            name="${isSelected ? 'dash-circle-fill' : 'plus-circle-fill'}"
                            class="basket-toggle-btn"
                            variant="${isSelected ? 'danger' : 'primary'}"
                            label="${isSelected ? 'Remove from basket' : 'Add to basket'}"
                            ${btnDisabled ? 'disabled' : ''}
                        ></sl-icon-button>
                    </div>
                ` : ''}

                <sl-skeleton effect="sheen" class="image-skeleton"></sl-skeleton>
                <div class="track-count-placeholder"></div>
            </div>
            <div class="card-content">
                <strong>${this.escapeHtml(itemName)}</strong>
                ${subtitleHtml}
                ${yearHtml}
            </div>
        `;

        // Load image asynchronously via Tauri proxy
        const cardImage = card.querySelector('.card-image') as HTMLElement;
        let imageId: string;
        if (isBrowseItem) {
            const bi = item as BrowseDisplayItem;
            imageId = bi.coverArtId ?? bi.id;
        } else {
            const ji = item as JellyfinItem | JellyfinView;
            imageId = ji.ImageId || ji.Id;
        }
        getImageUrl(imageId, 300, 90).then(dataUrl => {
            if (cardImage) {
                cardImage.style.backgroundImage = `url('${dataUrl}')`;
                const skeleton = card.querySelector('.image-skeleton') as HTMLElement;
                if (skeleton) skeleton.style.display = 'none';
            }
        }).catch(err => {
            console.warn(`Failed to load image for ${imageId}:`, err);
            const skeleton = card.querySelector('.image-skeleton') as HTMLElement;
            if (skeleton) skeleton.style.display = 'none';
        });

        // Event: Navigation (click on card but NOT on toggle button)
        card.addEventListener('click', async (e) => {
            const path = e.composedPath();
            const isButton = path.some(el => (el as HTMLElement).classList?.contains('basket-toggle-btn'));
            if (!isButton && !card.classList.contains('is-navigating')) {
                card.classList.add('is-navigating');
                try {
                    await onNavigate();
                } finally {
                    card.classList.remove('is-navigating');
                }
            }
        });

        // Event: Toggle Basket
        if (showSelection) {
            const toggleBtn = card.querySelector('.basket-toggle-btn') as any;
            toggleBtn.addEventListener('click', async (e: Event) => {
                e.stopPropagation();
                if (!selectionAllowed) return;

                if (basketStore.has(itemId)) {
                    basketStore.remove(itemId);
                } else if (isBrowseItem) {
                    const bi = item as BrowseDisplayItem;
                    basketStore.add({
                        id: bi.basketId ?? bi.id,
                        name: bi.name,
                        type: bi.basketType ?? bi.type,
                        artist: bi.subtitle ?? undefined,
                        childCount: bi.childCount ?? 0,
                        sizeTicks: bi.sizeTicks ?? 0,
                        sizeBytes: bi.sizeBytes ?? 0,
                    });
                } else {
                    // Fetch metadata (track count + file size) from daemon (Jellyfin-specific)
                    toggleBtn.loading = true;

                    let overlay: HTMLElement | null = null;
                    if (cardImage) {
                        overlay = document.createElement('div');
                        overlay.className = 'nav-loading-overlay';
                        const spinner = document.createElement('sl-spinner');
                        overlay.appendChild(spinner);
                        cardImage.appendChild(overlay);
                    }

                    try {
                        const ji = item as JellyfinItem | JellyfinView;
                        const [metadata, sizeData] = await Promise.all([
                            rpcCall('jellyfin_get_item_counts', { itemIds: [ji.Id] }),
                            rpcCall('jellyfin_get_item_sizes', { itemIds: [ji.Id] }),
                        ]);
                        const info = metadata[0] || { recursiveItemCount: 0, cumulativeRunTimeTicks: 0 };
                        const sizeInfo = sizeData[0] || { totalSizeBytes: 0 };

                        basketStore.add({
                            id: ji.Id,
                            name: ji.Name,
                            type: ji.Type,
                            artist: (ji as JellyfinItem).AlbumArtist,
                            childCount: info.recursiveItemCount,
                            sizeTicks: info.cumulativeRunTimeTicks,
                            sizeBytes: sizeInfo.totalSizeBytes,
                        });
                    } catch (err) {
                        console.error("Failed to fetch item metadata:", err);
                    } finally {
                        toggleBtn.loading = false;
                        if (overlay) {
                            overlay.remove();
                        }
                    }
                }
            });
        }

        // Listen for store updates to update visual state locally
        basketStore.addEventListener('update', () => {
            const selected = basketStore.has(itemId);
            card.classList.toggle('is-selected', selected);
            if (showSelection) {
                const btn = card.querySelector('.basket-toggle-btn') as any;
                if (btn) {
                    btn.name = selected ? 'dash-circle-fill' : 'plus-circle-fill';
                }
            }
        });

        return card;
    }

    private static escapeHtml(text: string): string {
        const div = document.createElement('div');
        div.textContent = text;
        return div.innerHTML;
    }
}
