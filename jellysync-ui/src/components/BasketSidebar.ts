// Basket Sidebar Component
// Displays the list of items selected for synchronization.

import { basketStore, BasketItem } from '../state/basket';
import { IMAGE_PROXY_URL, rpcCall } from '../rpc';

interface StorageInfo {
    totalBytes: number;
    freeBytes: number;
    usedBytes: number;
    devicePath: string;
}

interface FolderInfo {
    name: string;
    relativePath: string;
    isManaged: boolean;
}

interface RootFoldersResponse {
    deviceName: string;
    devicePath: string;
    hasManifest: boolean;
    folders: FolderInfo[];
    managedCount: number;
    unmanagedCount: number;
}

function formatSize(bytes: number): string {
    if (bytes >= 1024 * 1024 * 1024) {
        return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
    }
    return `${Math.round(bytes / (1024 * 1024))} MB`;
}

type CapacityZone = 'green' | 'amber' | 'red';

function getCapacityZone(projectedBytes: number, freeBytes: number, totalBytes: number): CapacityZone {
    if (projectedBytes > freeBytes) return 'red';
    const remainingAfterSync = freeBytes - projectedBytes;
    if (remainingAfterSync < totalBytes * 0.1) return 'amber';
    return 'green';
}

function renderCapacityBar(storageInfo: StorageInfo | null, projectedBytes: number): string {
    if (!storageInfo) {
        // No device state (AC #5)
        if (projectedBytes > 0) {
            return `
                <div class="capacity-section capacity-no-device">
                    <div class="capacity-selection-total">Your selection: <strong>${formatSize(projectedBytes)}</strong></div>
                    <div class="capacity-bar-container capacity-bar-disabled">
                        <div class="capacity-bar">
                            <div class="capacity-segment capacity-grey" style="width: 100%;"></div>
                        </div>
                    </div>
                    <div class="capacity-no-device-label">
                        <sl-icon name="usb-drive" style="font-size: 0.9rem;"></sl-icon>
                        No device connected
                    </div>
                </div>
            `;
        }
        return '';
    }

    const { totalBytes, freeBytes, usedBytes } = storageInfo;
    const zone = getCapacityZone(projectedBytes, freeBytes, totalBytes);

    const usedPct = Math.min((usedBytes / totalBytes) * 100, 100);
    const projectedPct = Math.min((projectedBytes / totalBytes) * 100, 100 - usedPct);
    const freePct = Math.max(100 - usedPct - projectedPct, 0);

    const remaining = freeBytes - projectedBytes;

    let statusMessage = '';
    let statusIcon = '';
    if (zone === 'green') {
        statusMessage = `${formatSize(remaining)} remaining`;
        statusIcon = '<sl-icon name="check-circle" style="color: var(--sl-color-success-600);"></sl-icon>';
    } else if (zone === 'amber') {
        statusMessage = `Tight fit — ${formatSize(remaining)} remaining`;
    } else {
        statusMessage = `Selection exceeds available space by ${formatSize(Math.abs(remaining))}`;
    }

    const projectedColor = zone === 'green' ? 'var(--sl-color-success-500)'
        : zone === 'amber' ? '#EBB334'
            : 'var(--sl-color-danger-500)';

    return `
        <div class="capacity-section capacity-zone-${zone}">
            <div class="capacity-bar-container">
                <div class="capacity-bar">
                    <div class="capacity-segment capacity-used" style="width: ${usedPct}%;"></div>
                    <div class="capacity-segment capacity-projected" style="width: ${projectedPct}%; background: ${projectedColor};"></div>
                    <div class="capacity-segment capacity-free" style="width: ${freePct}%;"></div>
                </div>
            </div>
            <div class="capacity-status">
                ${statusIcon}
                <span>${statusMessage}</span>
            </div>
        </div>
    `;
}

export class BasketSidebar {
    private container: HTMLElement;
    private updateListener: () => void;
    private isDestroyed: boolean = false;
    private storageInfo: StorageInfo | null = null;
    private folderInfo: RootFoldersResponse | null = null;
    private isFoldersExpanded: boolean = false;

    constructor(container: HTMLElement) {
        this.container = container;
        this.updateListener = () => this.refreshAndRender();
        this.init();
    }

    private init() {
        basketStore.addEventListener('update', this.updateListener);
        this.refreshAndRender();
    }

    private async refreshAndRender() {
        const [storageResult, foldersResult] = await Promise.allSettled([
            rpcCall('device_get_storage_info'),
            rpcCall('device_list_root_folders')
        ]);
        this.storageInfo = storageResult.status === 'fulfilled'
            ? storageResult.value as StorageInfo | null
            : null;
        this.folderInfo = foldersResult.status === 'fulfilled'
            ? foldersResult.value as RootFoldersResponse | null
            : null;
        this.render();
    }

    public destroy() {
        this.isDestroyed = true;
        basketStore.removeEventListener('update', this.updateListener);
    }

    private renderDeviceFolders(): string {
        if (!this.folderInfo) {
            // No device state (AC #5)
            return `
                <div class="device-folders-panel">
                    <div class="capacity-no-device-label" style="opacity: 0.7;">
                        <sl-icon name="usb-drive" style="font-size: 0.9rem;"></sl-icon>
                        Connect a device to view folders
                    </div>
                </div>
            `;
        }

        const { folders, managedCount, unmanagedCount, hasManifest } = this.folderInfo;

        let content = `
            <div class="device-folders-panel">
                <div class="device-folders-header" id="device-folders-toggle">
                    <h3>Device Folders</h3>
                    <div style="display: flex; align-items: center; gap: 0.5rem;">
                        <span class="device-folders-summary">${managedCount} managed | ${unmanagedCount} protected</span>
                        <sl-icon name="${this.isFoldersExpanded ? 'chevron-up' : 'chevron-down'}" style="font-size: 0.8rem; opacity: 0.5;"></sl-icon>
                    </div>
                </div>
        `;

        if (this.isFoldersExpanded) {
            content += `
                <div class="device-folders-list">
                    ${folders.length === 0 ? '<div style="font-size: 0.8rem; opacity: 0.5; padding: 0.5rem;">No folders found</div>' : ''}
                    ${folders.map(f => `
                        <div class="folder-item ${f.isManaged ? 'folder-managed' : 'folder-protected'}">
                            <sl-icon name="${f.isManaged ? 'unlock' : 'shield-lock'}" class="folder-icon"></sl-icon>
                            <span class="folder-name" title="${this.escapeHtml(f.name)}">${this.escapeHtml(f.name)}</span>
                            <span class="folder-status">${f.isManaged ? 'Managed' : 'Protected'}</span>
                        </div>
                    `).join('')}
                </div>
            `;

            if (!hasManifest) {
                content += `
                    <div class="device-empty-banner">
                        <sl-icon name="info-circle"></sl-icon>
                        <span>No managed sync zone — folders created on first sync.</span>
                    </div>
                `;
            }
        }

        content += `</div>`;
        return content;
    }

    public render() {
        if (this.isDestroyed) return;

        const items = basketStore.getItems();

        if (items.length === 0) {
            this.container.innerHTML = `
                <div class="basket-header">
                    <h2>Basket</h2>
                    <sl-badge variant="neutral" pill>0</sl-badge>
                </div>
                <div class="basket-placeholder">
                    <sl-icon name="basket" style="font-size: 2rem; opacity: 0.5;"></sl-icon>
                    <p style="opacity: 0.5;">Your basket is empty</p>
                </div>
                <div class="basket-footer">
                    ${this.renderDeviceFolders()}
                     <sl-button variant="primary" style="width: 100%;" disabled>
                        <sl-icon slot="prefix" name="cloud-download"></sl-icon>
                        Start Sync
                    </sl-button>
                </div>
            `;

            this.container.querySelector('#device-folders-toggle')?.addEventListener('click', () => {
                this.isFoldersExpanded = !this.isFoldersExpanded;
                this.render();
            });
            return;
        }

        const totalTracks = items.reduce((sum, item) => sum + item.childCount, 0);
        const totalSizeBytes = basketStore.getTotalSizeBytes();
        const zone = this.storageInfo
            ? getCapacityZone(totalSizeBytes, this.storageInfo.freeBytes, this.storageInfo.totalBytes)
            : null;
        const isOverLimit = zone === 'red';
        const overAmount = isOverLimit && this.storageInfo
            ? totalSizeBytes - this.storageInfo.freeBytes
            : 0;

        this.container.innerHTML = `
            <div class="basket-header">
                <h2>Basket</h2>
                <sl-badge variant="primary" pill>${items.length}</sl-badge>
            </div>

            <div class="basket-items-list">
                ${items.map(item => this.renderItem(item)).join('')}
            </div>

            <div class="basket-footer">
                <div class="basket-summary">
                    <span>${totalTracks} tracks | ${formatSize(totalSizeBytes)}</span>
                </div>
                ${renderCapacityBar(this.storageInfo, totalSizeBytes)}
                ${this.renderDeviceFolders()}
                ${isOverLimit ? `
                    <sl-button variant="danger" style="width: 100%;" disabled>
                        <sl-icon slot="prefix" name="exclamation-triangle"></sl-icon>
                        Remove ${formatSize(overAmount)} to fit
                    </sl-button>
                ` : `
                    <sl-button variant="primary" style="width: 100%;">
                        <sl-icon slot="prefix" name="cloud-download"></sl-icon>
                        Start Sync
                    </sl-button>
                `}
                <sl-button variant="text" size="small" class="clear-basket-btn" style="width: 100%; margin-top: 0.5rem;">
                    Clear All
                </sl-button>
            </div>
        `;

        // Bind events
        this.container.querySelectorAll('.remove-item-btn').forEach(btn => {
            btn.addEventListener('click', (e) => {
                const id = (e.currentTarget as HTMLElement).getAttribute('data-id');
                if (id) basketStore.remove(id);
            });
        });

        this.container.querySelector('.clear-basket-btn')?.addEventListener('click', () => {
            basketStore.clear();
        });

        this.container.querySelector('#device-folders-toggle')?.addEventListener('click', () => {
            this.isFoldersExpanded = !this.isFoldersExpanded;
            this.render();
        });
    }

    private renderItem(item: BasketItem): string {
        // We reuse the image proxy from the daemon
        const imageUrl = `${IMAGE_PROXY_URL}/${item.id}?maxHeight=100&quality=80`;

        return `
            <div class="basket-item-card" data-id="${item.id}">
                <div class="basket-item-image" style="background-image: url('${imageUrl}')"></div>
                <div class="basket-item-info">
                    <div class="basket-item-name">${this.escapeHtml(item.name)}</div>
                    <div class="basket-item-meta">${item.childCount} tracks • ${item.type}</div>
                </div>
                <sl-icon-button name="x" class="remove-item-btn" data-id="${item.id}" label="Remove"></sl-icon-button>
            </div>
        `;
    }

    private escapeHtml(text: string): string {
        const div = document.createElement('div');
        div.textContent = text;
        return div.innerHTML;
    }
}
