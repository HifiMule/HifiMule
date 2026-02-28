// Basket Sidebar Component
// Displays the list of items selected for synchronization.

import { basketStore, BasketItem } from '../state/basket';
import { IMAGE_PROXY_URL, rpcCall } from '../rpc';
import { RepairModal } from './RepairModal';

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

interface SyncOperation {
    id: string;
    status: 'running' | 'complete' | 'failed';
    startedAt: string;
    currentFile: string | null;
    bytesCurrent: number;
    bytesTotal: number;
    filesCompleted: number;
    filesTotal: number;
    errors: Array<{ jellyfinId: string; filename: string; errorMessage: string }>;
}

function getBasename(path: string): string {
    const normalized = path.replace(/\\/g, '/');
    const segments = normalized.split('/');
    return segments[segments.length - 1] || path;
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
    private isSyncing: boolean = false;
    private currentOperationId: string | null = null;
    private currentOperation: SyncOperation | null = null;
    private pollingInterval: number | null = null;
    private daemonStateInterval: number | null = null;
    private showSyncComplete: boolean = false;
    private syncErrorMessages: string[] | null = null;
    private isDirtyManifest: boolean = false;

    constructor(container: HTMLElement) {
        this.container = container;
        this.updateListener = () => this.refreshAndRender();
        this.init();
        this.startDaemonStatePolling();
    }

    private init() {
        basketStore.addEventListener('update', this.updateListener);
        this.refreshAndRender();
    }

    private async refreshAndRender() {
        if (this.isSyncing || this.showSyncComplete || this.syncErrorMessages !== null) {
            this.render();
            return;
        }
        const [storageResult, foldersResult, daemonStateResult] = await Promise.allSettled([
            rpcCall('device_get_storage_info'),
            rpcCall('device_list_root_folders'),
            rpcCall('get_daemon_state')
        ]);
        this.storageInfo = storageResult.status === 'fulfilled'
            ? storageResult.value as StorageInfo | null
            : null;
        this.folderInfo = foldersResult.status === 'fulfilled'
            ? foldersResult.value as RootFoldersResponse | null
            : null;
        this.isDirtyManifest = daemonStateResult.status === 'fulfilled'
            && daemonStateResult.value?.dirtyManifest === true;
        this.render();
    }

    public destroy() {
        this.isDestroyed = true;
        this.stopPolling();
        if (this.daemonStateInterval !== null) {
            clearInterval(this.daemonStateInterval);
            this.daemonStateInterval = null;
        }
        basketStore.removeEventListener('update', this.updateListener);
    }

    private startDaemonStatePolling() {
        if (this.daemonStateInterval !== null) return;
        this.daemonStateInterval = window.setInterval(async () => {
            if (this.isDestroyed || this.isSyncing || this.showSyncComplete || this.syncErrorMessages) return;
            try {
                const daemonStateResult = await rpcCall('get_daemon_state') as any;
                const newDirty = daemonStateResult?.dirtyManifest === true;
                if (newDirty !== this.isDirtyManifest) {
                    this.isDirtyManifest = newDirty;
                    this.refreshAndRender();
                }
            } catch (err) {
                // Ignore transient errors
            }
        }, 2000);
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

        // Show dirty manifest banner if flagged
        if (this.isDirtyManifest) {
            content += `
                <div class="dirty-manifest-banner" id="open-repair-btn" title="Open Manifest Repair">
                    <sl-icon name="exclamation-triangle-fill"></sl-icon>
                    <div class="dirty-manifest-banner-text">
                        <strong>Manifest Dirty</strong>
                        Interrupted sync detected — click to repair
                    </div>
                    <sl-icon name="chevron-right" style="opacity: 0.5;"></sl-icon>
                </div>
            `;
        }

        content += `</div>`;
        return content;
    }

    public render() {
        if (this.isDestroyed) return;
        if (this.showSyncComplete) {
            this.renderSyncComplete();
            return;
        }
        if (this.syncErrorMessages) {
            this.renderSyncError(this.syncErrorMessages);
            return;
        }
        if (this.isSyncing && this.currentOperation) {
            this.renderSyncProgress();
            return;
        }
        if (this.isSyncing) {
            this.container.innerHTML = `
                <div class="basket-header"><h2>Starting...</h2></div>
                <div class="sync-progress-panel" aria-live="polite" aria-label="Sync progress">
                    <sl-spinner style="font-size: 2rem;"></sl-spinner>
                </div>
                <div class="basket-footer">
                    <sl-button variant="primary" style="width: 100%;" disabled loading>
                        Syncing...
                    </sl-button>
                </div>
            `;
            return;
        }

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
            this.container.querySelector('#open-repair-btn')?.addEventListener('click', () => this.openRepairModal());
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
                ` : this.isDirtyManifest ? `
                    <sl-button variant="warning" style="width: 100%;" disabled>
                        <sl-icon slot="prefix" name="exclamation-triangle"></sl-icon>
                        Repair Manifest First
                    </sl-button>
                ` : `
                    <sl-button id="start-sync-btn" variant="primary" style="width: 100%;"
                               ${this.isSyncing ? 'loading disabled' : ''}>
                        <sl-icon slot="prefix" name="cloud-download"></sl-icon>
                        ${this.isSyncing ? 'Syncing...' : 'Start Sync'}
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

        this.container.querySelector('#start-sync-btn')?.addEventListener('click', () => {
            this.handleStartSync();
        });

        this.container.querySelector('#device-folders-toggle')?.addEventListener('click', () => {
            this.isFoldersExpanded = !this.isFoldersExpanded;
            this.render();
        });
        this.container.querySelector('#open-repair-btn')?.addEventListener('click', () => this.openRepairModal());
    }

    private openRepairModal() {
        const modal = new RepairModal(this.container, () => {
            this.isDirtyManifest = false;
            this.refreshAndRender();
        });
        modal.open();
    }

    private async handleStartSync() {
        if (this.isSyncing) return;
        const itemIds = basketStore.getItems().map(i => i.id);
        if (itemIds.length === 0) return;

        try {
            this.isSyncing = true;
            this.showSyncComplete = false;
            this.syncErrorMessages = null;
            this.currentOperation = null;
            this.currentOperationId = null;
            this.render();

            const delta = await rpcCall('sync_calculate_delta', { itemIds });
            const result = await rpcCall('sync_execute', { delta });
            this.currentOperationId = result.operationId as string;

            this.startPolling();
        } catch (err) {
            this.stopPolling();
            this.isSyncing = false;
            this.currentOperationId = null;
            this.currentOperation = null;
            this.showError(`Failed to start sync: ${(err as Error).message}`);
        }
    }

    private startPolling() {
        this.stopPolling();
        this.pollingInterval = window.setInterval(async () => {
            if (!this.currentOperationId) {
                this.stopPolling();
                return;
            }
            try {
                const op = await rpcCall('sync_get_operation_status', {
                    operationId: this.currentOperationId
                }) as SyncOperation;
                this.currentOperation = op;
                this.renderSyncProgress();

                if (op.status === 'complete') {
                    this.stopPolling();
                    this.handleSyncComplete();
                } else if (op.status === 'failed') {
                    this.stopPolling();
                    this.handleSyncFailed(op);
                }
            } catch (err) {
                console.error('[Sync] Progress poll failed:', err);
            }
        }, 500);
    }

    private stopPolling() {
        if (this.pollingInterval !== null) {
            clearInterval(this.pollingInterval);
            this.pollingInterval = null;
        }
    }

    private renderSyncProgress() {
        if (!this.currentOperation || this.isDestroyed) return;

        const op = this.currentOperation;
        const pct = op.filesTotal > 0
            ? Math.round((op.filesCompleted / op.filesTotal) * 100)
            : 0;
        const currentFileName = op.currentFile
            ? getBasename(op.currentFile)
            : 'Preparing...';

        this.container.innerHTML = `
            <div class="basket-header">
                <h2>Syncing</h2>
                <sl-badge variant="primary" pill>${op.filesCompleted}/${op.filesTotal}</sl-badge>
            </div>
            <div class="sync-progress-panel" aria-live="polite" aria-label="Sync progress">
                <sl-progress-bar value="${pct}" style="width: 100%; margin-bottom: 0.75rem;"
                    label="Sync progress: ${pct}%"></sl-progress-bar>
                <div class="sync-current-file">
                    <sl-icon name="arrow-down-circle" style="color: var(--sl-color-primary-600);"></sl-icon>
                    <span title="${this.escapeHtml(op.currentFile || '')}">${this.escapeHtml(currentFileName)}</span>
                </div>
                <div class="sync-file-counter">${op.filesCompleted} of ${op.filesTotal} files</div>
            </div>
            <div class="basket-footer">
                <sl-button variant="primary" style="width: 100%;" disabled loading>
                    <sl-icon slot="prefix" name="cloud-download"></sl-icon>
                    Syncing...
                </sl-button>
            </div>
        `;
    }

    private renderSyncComplete() {
        this.container.innerHTML = `
            <div class="basket-header">
                <h2>Basket</h2>
                <sl-badge variant="neutral" pill>0</sl-badge>
            </div>
            <div class="sync-success-panel">
                <sl-icon name="check-circle-fill"
                    style="font-size: 2.5rem; color: var(--sl-color-success-600);"></sl-icon>
                <p class="sync-status-label">Sync Complete</p>
            </div>
            <div class="basket-footer">
                <sl-button id="sync-done-btn" variant="primary" style="width: 100%;">
                    <sl-icon slot="prefix" name="check"></sl-icon>
                    Done
                </sl-button>
            </div>
        `;

        this.container.querySelector('#sync-done-btn')?.addEventListener('click', () => {
            this.showSyncComplete = false;
            this.render();
        });
    }

    private renderSyncError(errors: string[]) {
        const errorList = errors.length > 0
            ? errors.map(msg => `<li>${this.escapeHtml(msg)}</li>`).join('')
            : '<li>Sync failed - check device connection and try again.</li>';

        this.container.innerHTML = `
            <div class="basket-header">
                <h2>Basket</h2>
            </div>
            <div class="sync-error-panel">
                <sl-icon name="exclamation-triangle-fill"
                    style="font-size: 2.5rem; color: var(--sl-color-danger-500);"></sl-icon>
                <p class="sync-status-label">Sync Failed</p>
                <ul class="sync-error-list">${errorList}</ul>
            </div>
            <div class="basket-footer">
                <sl-button id="sync-dismiss-btn" variant="text" style="width: 100%;">
                    Dismiss
                </sl-button>
            </div>
        `;

        this.container.querySelector('#sync-dismiss-btn')?.addEventListener('click', () => {
            this.syncErrorMessages = null;
            this.render();
        });
    }

    private handleSyncComplete() {
        if (this.isDestroyed) return;
        this.isSyncing = false;
        this.currentOperationId = null;
        this.currentOperation = null;
        this.showSyncComplete = true;
        this.syncErrorMessages = null;
        basketStore.clear();
        this.renderSyncComplete();
    }

    private handleSyncFailed(operation: SyncOperation) {
        if (this.isDestroyed) return;
        this.isSyncing = false;
        this.currentOperationId = null;
        this.currentOperation = null;
        this.showSyncComplete = false;
        this.syncErrorMessages = operation.errors.length > 0
            ? operation.errors.map(e => `${e.filename || e.jellyfinId}: ${e.errorMessage}`)
            : ['Sync failed - check device connection and try again.'];
        this.renderSyncError(this.syncErrorMessages);
    }

    private showError(message: string) {
        this.isSyncing = false;
        this.currentOperation = null;
        this.currentOperationId = null;
        this.showSyncComplete = false;
        this.syncErrorMessages = [message];
        this.renderSyncError(this.syncErrorMessages);
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
