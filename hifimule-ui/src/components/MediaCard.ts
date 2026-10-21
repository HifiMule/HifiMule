import { escapeAttr } from '../html';
import { invalidatePlaylistsCache } from '../state/libraryCache';
import './cardArtwork';
import type { CardArtwork } from './cardArtwork';
// MediaCard Component
// Handles rendering of media items in the grid with selection support.

import { basketStore } from '../state/basket';
import { fetchBrowseAlbum, getImageUrl, rpcCall } from '../rpc';
import { t } from '../i18n';
import { showToast } from '../toast';
import { createAlbumPlayButton } from './AlbumPlayButton';
import { createPlaylistPlayButton } from './PlaylistPlayButton';
import { createRadioPlayButton } from './RadioPlayButton';
import { createTrackPlayButton } from './TrackPlayButton';
import { createTrackPreviewButton } from './TrackPreviewButton';
import { createTrackQueueButton } from './TrackQueueButton';
import { bookBasketItem } from '../state/mediaSyncSelection';

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
    serverId?: string;
    name: string;
    type: 'MusicArtist' | 'MusicAlbum' | 'Book' | 'BookPart' | 'Playlist' | 'Audio' | 'MusicGenre';
    basketId?: string;
    basketType?: string;
    coverArtId?: string | null;
    subtitle?: string | null;
    year?: number | null;
    childCount?: number;
    playablePlaylist?: boolean;
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
        card.dataset.basketLabel = itemName;
        if (isSynced) card.classList.add('synced');

        const isSelected = basketStore.has(itemId);
        if (isSelected) card.classList.add('is-selected');

        const showSelection = isBrowseItem || mode === 'items';
        const btnDisabled = !selectionAllowed;

        let subtitleHtml = '';
        let yearHtml = '';
        if (isBrowseItem) {
            const bi = item as BrowseDisplayItem;
            if (bi.subtitle) subtitleHtml = `<div class="card-subtitle">${escapeAttr(bi.subtitle)}</div>`;
            if (bi.year) yearHtml = `<div class="card-year">${bi.year}</div>`;
        } else {
            const ji = item as JellyfinItem;
            if (ji.AlbumArtist) subtitleHtml = `<div class="card-subtitle">${escapeAttr(ji.AlbumArtist)}</div>`;
            if (ji.ProductionYear) yearHtml = `<div class="card-year">${ji.ProductionYear}</div>`;
            if ((item as JellyfinView).Type === 'CollectionFolder') subtitleHtml = `<div class="card-subtitle">${t('ui.library.title')}</div>`;
        }

        card.innerHTML = `
            <hm-card-artwork class="card-image">
                ${isSynced ? `<div class="synced-badge"><sl-icon name="check-circle-fill" aria-hidden="true"></sl-icon></div>` : ''}

                ${showSelection ? `
                    <div class="selection-overlay">
                        <sl-icon-button
                            name="${isSelected ? 'dash-circle-fill' : 'plus-circle-fill'}"
                            class="basket-toggle-btn"
                            variant="${isSelected ? 'danger' : 'primary'}"
                            label="${escapeAttr(`${t(isSelected ? 'tracks.view.remove_from_basket' : 'tracks.view.add_to_basket')} — ${itemName}`)}"
                            ${btnDisabled ? 'disabled' : ''}
                        ></sl-icon-button>
                    </div>
                ` : ''}

                <sl-skeleton effect="sheen" class="image-skeleton"></sl-skeleton>
                <div class="track-count-placeholder"></div>
            </hm-card-artwork>
            <div class="card-content">
                <button type="button" class="card-open" title="${escapeAttr(itemName)}">${escapeAttr(itemName)}${isSynced ? `<span class="sr-only"> — ${t('library.media.synced')}</span>` : ''}</button>
                ${subtitleHtml}
                ${yearHtml}
            </div>
        `;

        if (isBrowseItem && ['Audio', 'BookPart'].includes((item as BrowseDisplayItem).type)) {
            const audio = item as BrowseDisplayItem;
            const isPart = audio.type === 'BookPart';
            const play = createTrackPlayButton(audio.serverId, audio.id, itemName, isPart ? 'part' : 'track');
            card.querySelector('.card-content')?.appendChild(play);
            if (!isPart) {
                card.querySelector('.card-content')?.appendChild(
                    createTrackPreviewButton(audio.serverId, audio.id, itemName),
                );
                card.querySelector('.card-content')?.appendChild(
                    createTrackQueueButton(audio.serverId, audio.id, itemName),
                );
            }
        }

        if (isBrowseItem && ['MusicAlbum', 'Book'].includes((item as BrowseDisplayItem).type)) {
            const album = item as BrowseDisplayItem;
            const play = createAlbumPlayButton(album.id, album.serverId, itemName, album.type === 'Book' ? 'book' : 'album');
            card.querySelector('.card-content')?.appendChild(play);
        }
        if (isBrowseItem && (item as BrowseDisplayItem).playablePlaylist) {
            const playlist = item as BrowseDisplayItem;
            card.querySelector('.card-content')?.appendChild(
                createPlaylistPlayButton(playlist.id, playlist.serverId, itemName, playlist.childCount),
            );
        }

        // Load image asynchronously via Tauri proxy
        if (isBrowseItem) {
            const radio = createRadioPlayButton(item as BrowseDisplayItem);
            if (radio) {
                card.classList.add('media-card--radio');
                radio.setAttribute('placement', 'top');
                card.querySelector('.card-content')?.appendChild(radio);
            }
        }

        const cardImage = card.querySelector('.card-image') as HTMLElement;
        let imageId: string;
        if (isBrowseItem) {
            const bi = item as BrowseDisplayItem;
            imageId = bi.coverArtId ?? bi.id;
        } else {
            const ji = item as JellyfinItem | JellyfinView;
            imageId = ji.ImageId || ji.Id;
        }
        (cardImage as CardArtwork).load = () => getImageUrl(imageId, 300, 90).then(dataUrl => {
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
            let pending = false;
            toggleBtn.addEventListener('click', async (e: Event) => {
                e.stopPropagation();
                if (!selectionAllowed || pending) return;
                if (!basketStore.admitPhysicalTargetMutation()) return;

                if (basketStore.has(itemId)) {
                    basketStore.remove(itemId);
                    return;
                }
                pending = true;
                toggleBtn.setAttribute('aria-disabled', 'true');
                toggleBtn.setAttribute('aria-busy', 'true');
                toggleBtn.shadowRoot?.querySelector('button')?.setAttribute('aria-disabled', 'true');
                const overlay = document.createElement('div');
                overlay.className = 'nav-loading-overlay';
                // Pointer repeats still reach the guarded toggle, not card navigation.
                overlay.style.pointerEvents = 'none';
                overlay.appendChild(document.createElement('sl-spinner'));
                cardImage.appendChild(overlay);
                try {
                    if (isBrowseItem) {
                        const bi = item as BrowseDisplayItem;
                        if (bi.serverId && basketStore.getActiveServerId() !== bi.serverId) return;
                        const resolvedType = bi.basketType ?? bi.type;
                        if (resolvedType === 'Book') {
                            try {
                                const detail = await fetchBrowseAlbum(bi.id);
                                if (bi.serverId && basketStore.getActiveServerId() !== bi.serverId) return;
                                basketStore.add(bookBasketItem(bi.id, bi.name, bi.serverId, bi.subtitle ?? undefined, detail.tracks));
                            } catch (err) {
                                showToast(err instanceof Error ? err.message : String(err), 'danger');
                            }
                            return;
                        }
                        const CONTAINER_TYPES = ['MusicArtist', 'BookAuthor', 'MusicAlbum', 'MusicGenre', 'Playlist'];
                        const isFavoriteScoped = resolvedType === 'FavoriteArtist' || resolvedType === 'FavoriteAlbum';
                        const needsFetch = CONTAINER_TYPES.includes(resolvedType) && !isFavoriteScoped && (!bi.childCount || !bi.sizeBytes);

                        if (needsFetch) {
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
                                    serverId: bi.serverId,
                                    artist: bi.subtitle ?? undefined,
                                    childCount: info.recursiveItemCount,
                                    sizeTicks: info.cumulativeRunTimeTicks ?? bi.sizeTicks ?? 0,
                                    sizeBytes: sizeInfo.totalSizeBytes,
                                });
                            } catch (err) {
                                console.error('Failed to fetch item count:', err);
                                showToast(err instanceof Error ? err.message : String(err), 'danger');
                            }
                        } else {
                            basketStore.add({
                                id: bi.basketId ?? bi.id,
                                name: bi.name,
                                type: resolvedType,
                                serverId: bi.serverId,
                                artist: bi.subtitle ?? undefined,
                                childCount: bi.childCount ?? 0,
                                sizeTicks: bi.sizeTicks ?? 0,
                                sizeBytes: bi.sizeBytes ?? 0,
                            });
                        }
                    } else {
                        // Fetch metadata (track count + file size) from daemon (Jellyfin-specific)
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
                            showToast(err instanceof Error ? err.message : String(err), 'danger');
                        }
                    }
                } finally {
                    pending = false;
                    toggleBtn.removeAttribute('aria-disabled');
                    toggleBtn.removeAttribute('aria-busy');
                    toggleBtn.shadowRoot?.querySelector('button')?.removeAttribute('aria-disabled');
                    overlay.remove();
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
                const trigger = card.querySelector<HTMLElement>('.card-open');
                if (trigger) MediaCard.bindPlaylistMenuKeyboard(trigger, serverItemId, itemName);
                // A normal Tab stop also exposes this action on platforms where
                // the OS consumes the context-menu shortcut before the webview.
                card.querySelector('.card-content')?.appendChild(MediaCard.createPlaylistAction(serverItemId, itemName));
                card.addEventListener('contextmenu', (e) => {
                    e.preventDefault();
                    MediaCard.showItemContextMenu(e.clientX, e.clientY, serverItemId, itemName, card.querySelector<HTMLElement>('.card-open') ?? undefined);
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
            if (btn) {
                btn.name = selected ? 'dash-circle-fill' : 'plus-circle-fill';
                btn.label = `${t(selected ? 'tracks.view.remove_from_basket' : 'tracks.view.add_to_basket')} — ${card.dataset.basketLabel ?? ''}`;
            }
        });
    }

    private static dismissActiveMenu: (() => void) | null = null;

    static createPlaylistAction(itemId: string, itemName: string): HTMLElement {
        const action = document.createElement('sl-icon-button') as any;
        action.name = 'collection-play';
        action.label = `${t('playlist.context.add_to_playlist')} ${itemName}`;
        action.addEventListener('click', (event: MouseEvent) => {
            event.stopPropagation();
            MediaCard.openAddToPlaylistDialog([itemId], itemName);
        });
        return action;
    }

    /** WebKit does not synthesize contextmenu for these keyboard shortcuts. */
    static bindPlaylistMenuKeyboard(trigger: HTMLElement, itemId: string, itemName: string): void {
        trigger.setAttribute('aria-haspopup', 'menu');
        trigger.addEventListener('keydown', (event) => {
            if (event.key !== 'ContextMenu' && !(event.key === 'F10' && event.shiftKey)) return;
            event.preventDefault();
            event.stopPropagation();
            const bounds = trigger.getBoundingClientRect();
            MediaCard.showItemContextMenu(bounds.left, bounds.bottom, itemId, itemName, trigger);
        });
    }

    static showItemContextMenu(x: number, y: number, itemId: string, itemName: string, trigger?: HTMLElement): void {
        if (MediaCard.dismissActiveMenu) {
            MediaCard.dismissActiveMenu();
        }
        let returnFocus = trigger ?? (document.activeElement as HTMLElement | null);
        while (returnFocus?.shadowRoot?.activeElement) {
            returnFocus = returnFocus.shadowRoot.activeElement as HTMLElement;
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
            if (!menu.isConnected) return;
            menu.classList.add('is-open');
            menuItem.focus();
        });

        const cleanup = () => {
            const hadFocus = menu.contains(document.activeElement);
            menu.remove();
            if (hadFocus && returnFocus?.isConnected) returnFocus.focus();
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
                // Restore the invoker before the browser performs normal
                // forward/backward tab navigation from that point.
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

    private static setPlaylistWriting(dialog: any, writing: boolean): void {
        const closeButton = dialog.shadowRoot?.querySelector('[part="close-button"]');
        if (closeButton) closeButton.disabled = writing;
        if (writing) {
            dialog.setAttribute('aria-busy', 'true');
            dialog.panel?.setAttribute('aria-busy', 'true');
            // The submit button becomes disabled. Keep focus inside the modal.
            dialog.panel?.focus();
        } else {
            dialog.removeAttribute('aria-busy');
            dialog.panel?.removeAttribute('aria-busy');
        }
    }

    static openCreatePlaylistDialog(itemIds: string[], suggestedName: string, onSuccess?: () => void): void {
        const dialog = document.createElement('sl-dialog') as any;
        dialog.className = 'playlist-write-dialog';
        dialog.label = t('library.context.create_playlist_title');
        dialog.innerHTML = `
            <sl-input
                id="ctx-playlist-name"
                label="${t('basket.playlist.name_label')}"
                required
                placeholder="${t('basket.playlist.name_placeholder')}"
                autofocus
                clearable
                value="${escapeAttr(suggestedName)}"
            ></sl-input>
            <sl-alert id="ctx-playlist-error" variant="danger" closable style="display:none; margin-top: 0.75rem;"></sl-alert>
            <sl-button slot="footer" variant="default" id="ctx-playlist-cancel">${t('basket.actions.cancel')}</sl-button>
            <sl-button slot="footer" variant="primary" id="ctx-playlist-create">${t('library.context.create_btn')}</sl-button>
        `;

        document.body.appendChild(dialog);

        let writing = false;
        const cancelBtn = dialog.querySelector('#ctx-playlist-cancel') as any;
        const dismissDialog = () => { if (!writing) dialog.hide(); };
        dialog.querySelector('#ctx-playlist-cancel')?.addEventListener('click', dismissDialog);
        dialog.addEventListener('sl-request-close', (event: Event) => {
            if (event.target === dialog && writing) event.preventDefault();
        });

        const submit = async () => {
            if (writing) return;
            const createBtn = dialog.querySelector('#ctx-playlist-create') as any;
            const errorEl = dialog.querySelector('#ctx-playlist-error') as HTMLElement | null;
            const nameInput = dialog.querySelector('#ctx-playlist-name') as any;
            const name = (nameInput?.value ?? '').trim();
            if (!name) {
                nameInput.setCustomValidity(t('basket.playlist.name_required'));
                nameInput.reportValidity();
                nameInput.focus();
                return;
            }
            nameInput.setCustomValidity('');
            writing = true;
            cancelBtn.disabled = true;
            nameInput.disabled = true;
            MediaCard.setPlaylistWriting(dialog, true);
            createBtn.loading = true;
            createBtn.disabled = true;
            if (errorEl) errorEl.style.display = 'none';

            try {
                const { rpcCall } = await import('../rpc');
                await rpcCall('playlist.create', { name, itemIds });
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
                writing = false;
                cancelBtn.disabled = false;
                nameInput.disabled = false;
                MediaCard.setPlaylistWriting(dialog, false);
                createBtn.loading = false;
                createBtn.disabled = false;
                await createBtn.updateComplete;
                if (dialog.open) createBtn.focus();
            }
        };

        dialog.querySelector('#ctx-playlist-name')?.addEventListener('sl-input', () => {
            (dialog.querySelector('#ctx-playlist-name') as any).setCustomValidity('');
        });
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
        dialog.className = 'playlist-write-dialog';
        dialog.label = t('playlist.context.pick_playlist_title');
        dialog.innerHTML = `
            <div id="ctx-track-playlist-list" style="display:flex; flex-direction:column; gap:0.5rem; max-height:300px; overflow-y:auto;">
                <sl-spinner></sl-spinner>
            </div>
            <sl-alert id="ctx-track-error" variant="danger" closable style="display:none; margin-top: 0.75rem;"></sl-alert>
            <sl-button slot="footer" variant="default" id="ctx-track-cancel">${t('basket.actions.cancel')}</sl-button>
        `;

        document.body.appendChild(dialog);

        let writing = false;
        const cancelBtn = dialog.querySelector('#ctx-track-cancel') as any;
        cancelBtn.addEventListener('click', () => { if (!writing) dialog.hide(); });
        dialog.addEventListener('sl-request-close', (event: Event) => {
            if (event.target === dialog && writing) event.preventDefault();
        });
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
                    if (writing) return;
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
                        if (writing) return;
                        writing = true;
                        MediaCard.setPlaylistWriting(dialog, true);
                        cancelBtn.disabled = true;
                        listEl.querySelectorAll('sl-button').forEach(button => { (button as any).disabled = true; });
                        btn.loading = true;
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
                        } finally {
                            writing = false;
                            MediaCard.setPlaylistWriting(dialog, false);
                            cancelBtn.disabled = false;
                            listEl.querySelectorAll('sl-button').forEach(button => { (button as any).disabled = false; });
                            btn.loading = false;
                            await btn.updateComplete;
                            if (dialog.open) btn.focus();
                        }
                    });
                    listEl.appendChild(btn);
                }

                if (playlists.length === 0) {
                    const emptyNote = document.createElement('p');
                    emptyNote.style.cssText = 'color: var(--ink-dim); font-size: var(--sl-font-size-small); padding: 0.5rem 0;';
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
}
