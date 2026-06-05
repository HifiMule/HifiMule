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
        supportsPlaylistWrite?: boolean,
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
                    const resolvedType = bi.basketType ?? bi.type;
                    const CONTAINER_TYPES = ['MusicArtist', 'MusicAlbum', 'MusicGenre', 'Playlist'];
                    const isFavoriteScoped = resolvedType === 'FavoriteArtist' || resolvedType === 'FavoriteAlbum';
                    const needsFetch = CONTAINER_TYPES.includes(resolvedType) && !isFavoriteScoped && (!bi.childCount || !bi.sizeBytes);

                    if (needsFetch) {
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
                            const resolvedId = bi.basketId ?? bi.id;
                            const [metadata, sizeData] = await Promise.all([
                                rpcCall('jellyfin_get_item_counts', { itemIds: [resolvedId] }),
                                rpcCall('jellyfin_get_item_sizes', { itemIds: [resolvedId] }),
                            ]);
                            const info = metadata[0] || { recursiveItemCount: 0, cumulativeRunTimeTicks: 0 };
                            const sizeInfo = sizeData[0] || { totalSizeBytes: 0 };
                            basketStore.add({
                                id: resolvedId,
                                name: bi.name,
                                type: resolvedType,
                                artist: bi.subtitle ?? undefined,
                                childCount: info.recursiveItemCount,
                                sizeTicks: bi.sizeTicks || info.cumulativeRunTimeTicks,
                                sizeBytes: sizeInfo.totalSizeBytes,
                            });
                        } catch (err) {
                            console.error('Failed to fetch item count:', err);
                        } finally {
                            toggleBtn.loading = false;
                            if (overlay) overlay.remove();
                        }
                    } else {
                        basketStore.add({
                            id: bi.basketId ?? bi.id,
                            name: bi.name,
                            type: resolvedType,
                            artist: bi.subtitle ?? undefined,
                            childCount: bi.childCount ?? 0,
                            sizeTicks: bi.sizeTicks ?? 0,
                            sizeBytes: bi.sizeBytes ?? 0,
                        });
                    }
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

        // Context menu: "Send to playlist…" on artist/album cards when supported
        if (supportsPlaylistWrite) {
            const isBrowseDisplayItem = !('Id' in item);
            const isArtistOrAlbum = isBrowseDisplayItem
                ? ((item as BrowseDisplayItem).type === 'MusicArtist' || (item as BrowseDisplayItem).type === 'MusicAlbum')
                : false;

            if (isArtistOrAlbum) {
                card.addEventListener('contextmenu', (e) => {
                    e.preventDefault();
                    MediaCard.showContextMenu(e.clientX, e.clientY, itemId, itemName);
                });
            }
        }

        return card;
    }

    private static activeContextMenu: HTMLElement | null = null;

    static showContextMenu(x: number, y: number, itemId: string, itemName: string): void {
        // Dismiss any existing context menu first
        if (MediaCard.activeContextMenu) {
            MediaCard.activeContextMenu.remove();
            MediaCard.activeContextMenu = null;
        }

        const menu = document.createElement('div');
        menu.className = 'hm-context-menu';
        menu.style.cssText = `
            position: fixed;
            z-index: 9999;
            background: var(--sl-panel-background-color, #fff);
            border: 1px solid var(--sl-color-neutral-200, #e2e8f0);
            border-radius: var(--sl-border-radius-medium, 4px);
            box-shadow: var(--sl-shadow-large);
            padding: 4px 0;
            min-width: 180px;
        `;

        // Clamp position to viewport
        const MARGIN = 8;
        const viewW = window.innerWidth;
        const viewH = window.innerHeight;
        const MENU_W = 200;
        const MENU_H = 44;
        const left = Math.min(x, viewW - MENU_W - MARGIN);
        const top = Math.min(y, viewH - MENU_H - MARGIN);
        menu.style.left = `${left}px`;
        menu.style.top = `${top}px`;

        const sendItem = document.createElement('div');
        sendItem.className = 'hm-context-menu-item';
        sendItem.style.cssText = `
            padding: 8px 16px;
            cursor: pointer;
            font-size: var(--sl-font-size-small, 0.875rem);
            color: var(--sl-color-neutral-900);
            display: flex;
            align-items: center;
            gap: 8px;
        `;
        sendItem.innerHTML = `<sl-icon name="collection-play"></sl-icon> Send to playlist…`;

        sendItem.addEventListener('mouseover', () => {
            sendItem.style.background = 'var(--sl-color-primary-50, #eff6ff)';
        });
        sendItem.addEventListener('mouseout', () => {
            sendItem.style.background = '';
        });

        sendItem.addEventListener('click', () => {
            menu.remove();
            MediaCard.activeContextMenu = null;
            MediaCard.openCreatePlaylistDialog(itemId, itemName);
        });

        menu.appendChild(sendItem);
        document.body.appendChild(menu);
        MediaCard.activeContextMenu = menu;

        // Dismiss on any click outside the menu
        const dismiss = (ev: MouseEvent) => {
            if (!menu.contains(ev.target as Node)) {
                menu.remove();
                MediaCard.activeContextMenu = null;
                document.removeEventListener('click', dismiss, true);
            }
        };
        // Use capture=true so outside clicks register before propagation stops them
        document.addEventListener('click', dismiss, true);
    }

    static openCreatePlaylistDialog(itemId: string, itemName: string): void {
        const dialog = document.createElement('sl-dialog') as any;
        dialog.label = 'Send to New Playlist';
        dialog.innerHTML = `
            <sl-input
                id="ctx-playlist-name"
                placeholder="My playlist…"
                autofocus
                clearable
                value="${MediaCard.escapeHtml(itemName)}"
            ></sl-input>
            <sl-alert id="ctx-playlist-error" variant="danger" closable style="display:none; margin-top: 0.75rem;"></sl-alert>
            <sl-button slot="footer" variant="default" id="ctx-playlist-cancel">Cancel</sl-button>
            <sl-button slot="footer" variant="primary" id="ctx-playlist-create">Create</sl-button>
        `;

        document.body.appendChild(dialog);

        const dismissDialog = () => dialog.hide();
        dialog.querySelector('#ctx-playlist-cancel')?.addEventListener('click', dismissDialog);

        dialog.querySelector('#ctx-playlist-create')?.addEventListener('click', async () => {
            const createBtn = dialog.querySelector('#ctx-playlist-create') as any;
            const errorEl = dialog.querySelector('#ctx-playlist-error') as HTMLElement | null;
            const nameInput = dialog.querySelector('#ctx-playlist-name') as any;
            const name = (nameInput?.value ?? '').trim();
            if (!name) return;

            createBtn.loading = true;
            if (errorEl) errorEl.style.display = 'none';

            try {
                const { rpcCall } = await import('../rpc');
                await rpcCall('playlist.create', { name, itemIds: [itemId] });
                dialog.hide();
            } catch (err) {
                const msg = err instanceof Error ? err.message : String(err);
                if (errorEl) {
                    errorEl.textContent = `Failed to create playlist: ${msg}`;
                    errorEl.style.display = '';
                    (errorEl as any).open = true;
                }
            } finally {
                createBtn.loading = false;
            }
        });

        dialog.addEventListener('sl-after-hide', (event: Event) => {
            if (event.target === dialog) dialog.remove();
        });

        customElements.whenDefined('sl-dialog').then(() => dialog.show());
    }

    private static escapeHtml(text: string): string {
        const div = document.createElement('div');
        div.textContent = text;
        return div.innerHTML;
    }
}
