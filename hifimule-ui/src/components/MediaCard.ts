// MediaCard Component
// Handles rendering of media items in the grid with selection support.

import { basketStore } from '../state/basket';
import { getImageUrl, rpcCall } from '../rpc';
import { t } from '../i18n';
import { showToast } from '../toast';

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
        onCurate?: (id: string, name: string) => void,
    ): HTMLElement {
        const isBrowseItem = !('Id' in item);
        const itemId = isBrowseItem ? ((item as BrowseDisplayItem).basketId ?? (item as BrowseDisplayItem).id) : (item as JellyfinItem | JellyfinView).Id;
        const itemName = isBrowseItem ? (item as BrowseDisplayItem).name : (item as JellyfinItem | JellyfinView).Name;
        const selectionAllowed = deviceSelectionEnabled !== false;

        const card = document.createElement('sl-card');
        card.className = 'media-card';
        // Identifies the card for the grid's single delegated basket listener
        // (see MediaCard.refreshSelection). Avoids a per-card store subscription.
        card.dataset.basketId = itemId;
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
            if ((item as JellyfinView).Type === 'CollectionFolder') subtitleHtml = `<div class="card-subtitle">${t('ui.library.title')}</div>`;
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
                            label="${isSelected ? t('tracks.view.remove_from_basket') : t('tracks.view.add_to_basket')}"
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

        // Selection state is synced by MediaCard.refreshSelection, driven by a
        // single delegated basketStore listener the grid renderer owns. Cards do
        // not self-subscribe: innerHTML teardown can't unsubscribe them, so a
        // per-card listener leaks one orphaned subscription per card per render.

        // Context menu: "Add to playlist…" on artist/album/track cards
        if (supportsPlaylistWrite) {
            const isBrowseDisplayItem = !('Id' in item);
            const itemType = isBrowseDisplayItem ? (item as BrowseDisplayItem).type : null;

            if (itemType === 'MusicArtist' || itemType === 'MusicAlbum' || itemType === 'Audio') {
                const serverItemId = (item as BrowseDisplayItem).id;
                card.addEventListener('contextmenu', (e) => {
                    e.preventDefault();
                    MediaCard.showItemContextMenu(e.clientX, e.clientY, serverItemId, itemName);
                });
            }
        }

        // Curate button: appears on Playlist cards when playlist write is supported
        if (onCurate) {
            const itemType = 'Type' in item ? (item as JellyfinItem).Type : (item as BrowseDisplayItem).type;
            if (itemType === 'Playlist') {
                const curateBtn = document.createElement('sl-icon-button') as any;
                curateBtn.name = 'pencil-square';
                curateBtn.label = t('playlist.curation.curate_btn');
                curateBtn.style.cssText = 'font-size: 1rem; margin-left: auto;';
                curateBtn.addEventListener('click', (e: MouseEvent) => {
                    e.stopPropagation();
                    onCurate(itemId, itemName);
                });
                card.appendChild(curateBtn);
            }
        }

        return card;
    }

    /**
     * Sync every media card under `root` to the current basket state.
     * Drive this from one delegated basketStore 'update' listener owned by the
     * grid renderer (one listener per grid), not one subscription per card.
     */
    static refreshSelection(root: HTMLElement): void {
        root.querySelectorAll<HTMLElement>('.media-card[data-basket-id]').forEach(card => {
            const id = card.dataset.basketId;
            if (!id) return;
            const selected = basketStore.has(id);
            card.classList.toggle('is-selected', selected);
            const btn = card.querySelector('.basket-toggle-btn') as any;
            if (btn) btn.name = selected ? 'dash-circle-fill' : 'plus-circle-fill';
        });
    }

    private static dismissActiveMenu: (() => void) | null = null;

    static showItemContextMenu(x: number, y: number, itemId: string, itemName: string): void {
        if (MediaCard.dismissActiveMenu) {
            MediaCard.dismissActiveMenu();
        }

        const menu = document.createElement('div');
        menu.className = 'hm-context-menu';
        menu.setAttribute('role', 'menu');
        menu.setAttribute('aria-label', itemName);

        const menuItem = document.createElement('div');
        menuItem.className = 'hm-context-menu-item';
        menuItem.setAttribute('role', 'menuitem');
        menuItem.setAttribute('tabindex', '0');
        menuItem.innerHTML = `<sl-icon name="collection-play"></sl-icon> ${t('playlist.context.add_to_playlist')}`;

        menu.appendChild(menuItem);
        document.body.appendChild(menu);

        // Use offsetWidth/offsetHeight (layout size, unaffected by transforms) so
        // the viewport-clamp math stays correct when the entrance animation runs.
        const MARGIN = 8;
        const left = Math.max(MARGIN, Math.min(x, window.innerWidth - menu.offsetWidth - MARGIN));
        const top = Math.max(MARGIN, Math.min(y, window.innerHeight - menu.offsetHeight - MARGIN));
        menu.style.left = `${left}px`;
        menu.style.top = `${top}px`;

        // Reveal + focus in the same frame so the animation starts visible.
        requestAnimationFrame(() => {
            menu.classList.add('is-open');
            menuItem.focus();
        });

        const cleanup = () => {
            menu.remove();
            MediaCard.dismissActiveMenu = null;
            document.removeEventListener('click', onDocClick, true);
            window.removeEventListener('scroll', cleanup, true);
            window.removeEventListener('resize', cleanup);
            document.removeEventListener('keydown', onKeyDown, true);
        };
        MediaCard.dismissActiveMenu = cleanup;

        const onDocClick = (ev: MouseEvent) => {
            if (!menu.contains(ev.target as Node)) cleanup();
        };
        const onKeyDown = (ev: KeyboardEvent) => {
            if (ev.key === 'Escape') {
                cleanup();
            } else if (ev.key === 'Tab') {
                ev.preventDefault();
                cleanup();
            } else if (ev.key === 'Enter' || ev.key === ' ') {
                if (document.activeElement === menuItem) {
                    ev.preventDefault();
                    menuItem.click();
                }
            }
        };

        menuItem.addEventListener('click', () => {
            cleanup();
            MediaCard.openAddToPlaylistDialog([itemId], itemName);
        });

        document.addEventListener('click', onDocClick, true);
        window.addEventListener('scroll', cleanup, true);
        window.addEventListener('resize', cleanup);
        document.addEventListener('keydown', onKeyDown, true);
    }

    static openCreatePlaylistDialog(itemIds: string[], suggestedName: string, onSuccess?: () => void): void {
        const dialog = document.createElement('sl-dialog') as any;
        dialog.label = t('library.context.create_playlist_title');
        dialog.innerHTML = `
            <sl-input
                id="ctx-playlist-name"
                placeholder="${t('basket.playlist.name_placeholder')}"
                autofocus
                clearable
                value="${MediaCard.escapeHtml(suggestedName)}"
            ></sl-input>
            <sl-alert id="ctx-playlist-error" variant="danger" closable style="display:none; margin-top: 0.75rem;"></sl-alert>
            <sl-button slot="footer" variant="default" id="ctx-playlist-cancel">${t('basket.actions.cancel')}</sl-button>
            <sl-button slot="footer" variant="primary" id="ctx-playlist-create">${t('library.context.create_btn')}</sl-button>
        `;

        document.body.appendChild(dialog);

        const dismissDialog = () => dialog.hide();
        dialog.querySelector('#ctx-playlist-cancel')?.addEventListener('click', dismissDialog);

        const submit = async () => {
            const createBtn = dialog.querySelector('#ctx-playlist-create') as any;
            const errorEl = dialog.querySelector('#ctx-playlist-error') as HTMLElement | null;
            const nameInput = dialog.querySelector('#ctx-playlist-name') as any;
            const name = (nameInput?.value ?? '').trim();
            if (!name) return;
            if (createBtn?.loading) return; // guard against double-submit

            createBtn.loading = true;
            createBtn.disabled = true;
            if (errorEl) errorEl.style.display = 'none';

            try {
                const { rpcCall } = await import('../rpc');
                await rpcCall('playlist.create', { name, itemIds });
                const { invalidatePlaylistsCache } = await import('../library');
                invalidatePlaylistsCache();
                dialog.hide();
                showToast(t('playlist.context.created_success'), 'success');
                onSuccess?.();
            } catch (err) {
                const msg = err instanceof Error ? err.message : String(err);
                if (errorEl) {
                    errorEl.textContent = t('basket.playlist.error', { message: msg });
                    errorEl.style.display = '';
                    (errorEl as any).open = true;
                }
            } finally {
                createBtn.loading = false;
                createBtn.disabled = false;
            }
        };

        dialog.querySelector('#ctx-playlist-create')?.addEventListener('click', submit);
        dialog.querySelector('#ctx-playlist-name')?.addEventListener('keydown', (e: KeyboardEvent) => {
            if (e.key === 'Enter') {
                e.preventDefault();
                submit();
            }
        });

        dialog.addEventListener('sl-after-hide', (event: Event) => {
            if (event.target === dialog) dialog.remove();
        });

        customElements.whenDefined('sl-dialog').then(() => dialog.show());
    }



    static openAddToPlaylistDialog(itemIds: string[], label: string, onSuccess?: () => void): void {
        const dialog = document.createElement('sl-dialog') as any;
        dialog.label = t('playlist.context.pick_playlist_title');
        dialog.innerHTML = `
            <div id="ctx-track-playlist-list" style="display:flex; flex-direction:column; gap:0.5rem; max-height:300px; overflow-y:auto;">
                <sl-spinner></sl-spinner>
            </div>
            <sl-alert id="ctx-track-error" variant="danger" closable style="display:none; margin-top: 0.75rem;"></sl-alert>
            <sl-button slot="footer" variant="default" id="ctx-track-cancel">${t('basket.actions.cancel')}</sl-button>
        `;

        document.body.appendChild(dialog);

        dialog.querySelector('#ctx-track-cancel')?.addEventListener('click', () => dialog.hide());
        dialog.addEventListener('sl-after-hide', (event: Event) => {
            if (event.target === dialog) dialog.remove();
        });

        customElements.whenDefined('sl-dialog').then(async () => {
            dialog.show();
            const listEl = dialog.querySelector('#ctx-track-playlist-list') as HTMLElement;
            const errorEl = dialog.querySelector('#ctx-track-error') as HTMLElement | null;

            try {
                const { rpcCall } = await import('../rpc');
                const result = await rpcCall('browse.listPlaylists');
                const playlists: Array<{ id: string; name: string }> = result.playlists ?? [];

                listEl.innerHTML = '';

                const newBtn = document.createElement('sl-button') as any;
                newBtn.variant = 'default';
                newBtn.style.cssText = 'width: 100%; text-align: left;';
                newBtn.innerHTML = `<sl-icon slot="prefix" name="plus-circle"></sl-icon> ${t('playlist.context.new_playlist')}`;
                newBtn.addEventListener('click', () => {
                    dialog.addEventListener('sl-after-hide', () => {
                        MediaCard.openCreatePlaylistDialog(itemIds, label, onSuccess);
                    }, { once: true });
                    dialog.hide();
                });
                listEl.appendChild(newBtn);

                if (playlists.length > 0) {
                    const divider = document.createElement('sl-divider') as any;
                    listEl.appendChild(divider);
                }

                for (const pl of playlists) {
                    const btn = document.createElement('sl-button') as any;
                    btn.variant = 'default';
                    btn.style.cssText = 'width: 100%; text-align: left;';
                    btn.textContent = pl.name;
                    btn.dataset.plId = pl.id;
                    btn.addEventListener('click', async () => {
                        btn.loading = true;
                        btn.disabled = true;
                        if (errorEl) errorEl.style.display = 'none';
                        try {
                            await rpcCall('playlist.addItems', { playlistId: pl.id, itemIds });
                            dialog.hide();
                            showToast(t('playlist.context.added_success'), 'success');
                            onSuccess?.();
                        } catch (err) {
                            const msg = err instanceof Error ? err.message : String(err);
                            if (errorEl) {
                                errorEl.textContent = t('playlist.curation.add_tracks_error', { message: msg });
                                errorEl.style.display = '';
                                (errorEl as any).open = true;
                            }
                            btn.loading = false;
                            btn.disabled = false;
                        }
                    });
                    listEl.appendChild(btn);
                }

                if (playlists.length === 0) {
                    const emptyNote = document.createElement('p');
                    emptyNote.style.cssText = 'color: var(--sl-color-neutral-500); font-size: var(--sl-font-size-small); padding: 0.5rem 0;';
                    emptyNote.textContent = t('playlist.context.no_playlists_yet');
                    listEl.appendChild(emptyNote);
                }
            } catch (err) {
                const msg = err instanceof Error ? err.message : String(err);
                listEl.innerHTML = '';
                if (errorEl) {
                    errorEl.textContent = msg;
                    errorEl.style.display = '';
                    (errorEl as any).open = true;
                }
            }
        });
    }

    private static escapeHtml(text: string): string {
        const div = document.createElement('div');
        div.textContent = text;
        // Escape double-quotes too — value is interpolated into a quoted HTML attribute.
        return div.innerHTML.replace(/"/g, '&quot;');
    }
}
