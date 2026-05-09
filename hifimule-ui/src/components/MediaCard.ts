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

export class MediaCard {
    public static create(
        item: JellyfinItem | JellyfinView,
        mode: 'libraries' | 'items',
        isSynced: boolean,
        onNavigate: () => void | Promise<void>
    ): HTMLElement {
        const card = document.createElement('sl-card');
        card.className = 'media-card';
        if (isSynced) card.classList.add('synced');

        const isSelected = basketStore.has(item.Id);
        if (isSelected) card.classList.add('is-selected');

        // Selection overlay (only for items, not root libraries)
        const showSelection = mode === 'items';

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
                        ></sl-icon-button>
                    </div>
                ` : ''}

                <sl-skeleton effect="sheen" class="image-skeleton"></sl-skeleton>
                <div class="track-count-placeholder"></div>
            </div>
            <div class="card-content">
                <strong>${this.escapeHtml(item.Name)}</strong>
                ${(item as JellyfinItem).AlbumArtist ? `<div class="card-subtitle">${this.escapeHtml((item as JellyfinItem).AlbumArtist!)}</div>` : ''}
                ${(item as JellyfinItem).ProductionYear ? `<div class="card-year">${(item as JellyfinItem).ProductionYear}</div>` : ''}
                 ${(item as JellyfinView).Type === 'CollectionFolder' ? '<div class="card-subtitle">Library</div>' : ''}
            </div>
        `;

        // Load image asynchronously via Tauri proxy
        const cardImage = card.querySelector('.card-image') as HTMLElement;
        const imageId = item.ImageId || item.Id;
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

                if (basketStore.has(item.Id)) {
                    basketStore.remove(item.Id);
                } else {
                    // Fetch metadata (track count + file size) from daemon
                    toggleBtn.loading = true;

                    // Add large spinner to card image
                    let overlay: HTMLElement | null = null;
                    if (cardImage) {
                        overlay = document.createElement('div');
                        overlay.className = 'nav-loading-overlay';
                        const spinner = document.createElement('sl-spinner');
                        overlay.appendChild(spinner);
                        cardImage.appendChild(overlay);
                    }

                    try {
                        const [metadata, sizeData] = await Promise.all([
                            rpcCall('jellyfin_get_item_counts', { itemIds: [item.Id] }),
                            rpcCall('jellyfin_get_item_sizes', { itemIds: [item.Id] }),
                        ]);
                        const info = metadata[0] || { recursiveItemCount: 0, cumulativeRunTimeTicks: 0 };
                        const sizeInfo = sizeData[0] || { totalSizeBytes: 0 };

                        basketStore.add({
                            id: item.Id,
                            name: item.Name,
                            type: item.Type,
                            artist: (item as JellyfinItem).AlbumArtist,
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
            const selected = basketStore.has(item.Id);
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
