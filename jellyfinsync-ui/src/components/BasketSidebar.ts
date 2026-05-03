// Basket Sidebar Component
// Displays the list of items selected for synchronization.

import { basketStore, BasketItem, AUTO_FILL_SLOT_ID } from '../state/basket';
import { rpcCall, getImageUrl } from '../rpc';
import { RepairModal } from './RepairModal';
import { InitDeviceModal } from './InitDeviceModal';

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
    pendingDevicePath?: string;
}

interface SyncOperation {
    id: string;
    status: 'running' | 'complete' | 'failed';
    startedAt: string;
    currentFile: string | null;
    bytesCurrent: number;
    bytesTotal: number;
    bytesTransferred: number;
    totalBytes: number;
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
    private lastHydratedDeviceId: string | null = null;
    private syncSnapshotIds: string[] = [];
    // Auto-fill state
    private autoFillEnabled: boolean = false;
    private autoFillMaxBytes: number | null = null;
    private autoSyncOnConnect: boolean = false;
    private etaText: string = 'Calculating\u2026';
    // Multi-device hub state
    private connectedDevices: Array<{ path: string; deviceId: string; name: string; icon?: string | null }> = [];
    private selectedDevicePath: string | null = null;
    private deviceSwitchInFlight: boolean = false;

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
            && (daemonStateResult.value as any)?.dirtyManifest === true;

        if (daemonStateResult.status === 'fulfilled' && daemonStateResult.value) {
            const state = daemonStateResult.value as any;
            // Sync multi-device state so the hub renders correctly on every refreshAndRender,
            // not just during the 2s polling cycle.
            this.connectedDevices = state.connectedDevices ?? this.connectedDevices;
            // Use explicit field-presence check: if field present in response, use it (including null);
            // otherwise keep current. Fixes the null-coalescing bug where selectedDevicePath: null
            // would be ignored by the ?? operator.
            if ('selectedDevicePath' in state) {
                this.selectedDevicePath = state.selectedDevicePath;
            }
            const currentDevice = state.currentDevice;
            if (currentDevice?.deviceId && currentDevice.deviceId !== this.lastHydratedDeviceId) {
                this.lastHydratedDeviceId = currentDevice.deviceId;
                // Load saved auto-fill preferences from manifest
                this.autoFillEnabled = state.autoFill?.enabled ?? false;
                this.autoFillMaxBytes = state.autoFill?.maxBytes ?? null;
                this.autoSyncOnConnect = state.autoSyncOnConnect ?? false;
                // Await basket hydration before triggering auto-fill so that
                // getManualItemIds() and getManualSizeBytes() see the correct state (P1).
                try {
                    const res = await rpcCall('manifest_get_basket') as any;
                    if (res?.basketItems && Array.isArray(res.basketItems)) {
                        basketStore.hydrateFromDaemon(res.basketItems);
                    }
                } catch (err) {
                    console.error("Failed to fetch basket", err);
                }
                if (this.autoFillEnabled && !basketStore.has(AUTO_FILL_SLOT_ID)) {
                    this.insertAutoFillSlot();
                }
            } else if (!currentDevice) {
                if (this.lastHydratedDeviceId !== null) {
                    basketStore.clearForDevice();
                }
                this.lastHydratedDeviceId = null;
                this.autoFillEnabled = false;
                this.autoFillMaxBytes = null;
                this.autoSyncOnConnect = false;
            }
        }

        // Attach to daemon-initiated sync if one is running and we're not already tracking it
        if (!this.isSyncing && !this.showSyncComplete && this.syncErrorMessages === null) {
            if (daemonStateResult.status === 'fulfilled' && daemonStateResult.value) {
                const state = daemonStateResult.value as any;
                const activeOpId = state.activeOperationId as string | null;
                if (activeOpId) {
                    this.isSyncing = true;
                    this.currentOperationId = activeOpId;
                    this.currentOperation = null;
                    this.startPolling();
                    this.render();
                    return;
                }
            }
        }

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

    private insertAutoFillSlot(): void {
        const manualSize = basketStore.getManualSizeBytes();
        const available = this.storageInfo
            ? Math.max(this.storageInfo.freeBytes - manualSize, 0)
            : 0;
        const targetBytes = this.autoFillMaxBytes !== null
            ? Math.min(this.autoFillMaxBytes, this.storageInfo ? available : this.autoFillMaxBytes)
            : available;
        basketStore.add({
            id: AUTO_FILL_SLOT_ID,
            name: 'Auto-Fill',
            type: 'AutoFillSlot',
            childCount: 0,
            sizeTicks: 0,
            sizeBytes: targetBytes,
        });
    }

    private async persistAutoFillPrefs() {
        const device = this.folderInfo;
        if (!device) return;
        try {
            await rpcCall('sync.setAutoFill', {
                autoFillEnabled: this.autoFillEnabled,
                maxFillBytes: this.autoFillMaxBytes ?? undefined,
                autoSyncOnConnect: this.autoSyncOnConnect,
            });
        } catch (err) {
            console.error('[AutoFill] Failed to persist preferences:', err);
        }
    }

    private bindAutoFillEvents() {
        const autoFillToggle = this.container.querySelector('#auto-fill-toggle');
        if (autoFillToggle) {
            autoFillToggle.addEventListener('sl-change', (e: Event) => {
                this.autoFillEnabled = (e.target as HTMLInputElement).checked;
                this.persistAutoFillPrefs();
                if (this.autoFillEnabled) {
                    this.insertAutoFillSlot();
                } else {
                    basketStore.remove(AUTO_FILL_SLOT_ID);
                }
                this.render();
            });
        }

        const slider = this.container.querySelector('#auto-fill-slider');
        if (slider) {
            slider.addEventListener('sl-change', (e: Event) => {
                const gb = (e.target as HTMLInputElement).valueAsNumber;
                // Guard against NaN (non-numeric input) and negative values (P10).
                if (isNaN(gb) || gb < 0) return;
                this.autoFillMaxBytes = gb * 1024 * 1024 * 1024;
                this.persistAutoFillPrefs();
                this.insertAutoFillSlot();
            });
        }

        const autoSyncToggle = this.container.querySelector('#auto-sync-toggle');
        if (autoSyncToggle) {
            autoSyncToggle.addEventListener('sl-change', (e: Event) => {
                this.autoSyncOnConnect = (e.target as HTMLInputElement).checked;
                this.persistAutoFillPrefs();
            });
        }
    }

    private bindDeviceHubEvents(): void {
        this.container.querySelectorAll('.device-hub-card').forEach(card => {
            card.addEventListener('click', async () => {
                if (this.deviceSwitchInFlight) return;
                const path = (card as HTMLElement).dataset.path;
                if (!path) return;
                if (path === this.selectedDevicePath) return;
                this.deviceSwitchInFlight = true;
                try {
                    await basketStore.flushPendingSave();
                    await rpcCall('device.select', { path });
                    const basketResult = await rpcCall('manifest_get_basket') as any;
                    basketStore.hydrateFromDaemon(basketResult?.basketItems ?? []);
                    this.refreshAndRender();
                } catch (err) {
                    console.error('[DeviceHub] Failed to switch device:', err);
                } finally {
                    this.deviceSwitchInFlight = false;
                }
            });
        });
    }

    private renderAutoFillControls(): string {
        const hasDevice = this.folderInfo?.hasManifest ?? false;
        if (!hasDevice) return '';

        const sliderMax = this.storageInfo
            ? Math.ceil(this.storageInfo.freeBytes / (1024 * 1024 * 1024))
            : 64;
        const sliderValue = this.autoFillMaxBytes !== null
            ? Math.round(this.autoFillMaxBytes / (1024 * 1024 * 1024))
            : sliderMax;

        // Device is completely full — show feedback instead of a useless zero-width slider (P11).
        const deviceFull = this.storageInfo !== null && this.storageInfo.freeBytes === 0;

        return `
            <div class="auto-fill-controls">
                <div class="auto-fill-toggle-row">
                    <sl-switch id="auto-fill-toggle" size="small" ${this.autoFillEnabled ? 'checked' : ''}>
                        Auto-Fill
                    </sl-switch>
                    <span class="auto-fill-hint" style="font-size:0.75rem; opacity:0.6;">
                        Fill basket automatically
                    </span>
                </div>
                ${this.autoFillEnabled && deviceFull ? `
                    <div class="auto-fill-full-notice" style="margin-top:0.5rem; font-size:0.75rem; opacity:0.7;">
                        Device is full — no space available for Auto-Fill
                    </div>
                ` : ''}
                ${this.autoFillEnabled && !deviceFull ? `
                    <div class="auto-fill-slider-row" style="margin-top:0.5rem;">
                        <label style="font-size:0.75rem; opacity:0.7; display:block; margin-bottom:0.25rem;">
                            Max Fill Size: <strong>${sliderValue} GB</strong>
                        </label>
                        <sl-range id="auto-fill-slider"
                            min="0" max="${sliderMax}" step="1" value="${sliderValue}"
                            style="width:100%;">
                        </sl-range>
                    </div>
                ` : ''}
                <div class="auto-fill-toggle-row" style="margin-top:0.5rem;">
                    <sl-switch id="auto-sync-toggle" size="small" ${this.autoSyncOnConnect ? 'checked' : ''}>
                        Auto-Sync on Connect
                    </sl-switch>
                </div>
                <div style="font-size:0.7rem; opacity:0.55; margin-top:0.2rem; padding-left:0.5rem;">
                    Automatically start syncing when this device is connected. Works with or without the UI open.
                </div>
            </div>
        `;
    }

    private startDaemonStatePolling() {
        if (this.daemonStateInterval !== null) return;
        this.daemonStateInterval = window.setInterval(async () => {
            if (this.isDestroyed || this.isSyncing || this.showSyncComplete || this.syncErrorMessages) return;
            try {
                const daemonStateResult = await rpcCall('get_daemon_state') as any;
                const newDirty = daemonStateResult?.dirtyManifest === true;
                const newPendingPath = daemonStateResult?.pendingDevicePath ?? null;
                const currentHasManifest = this.folderInfo?.hasManifest ?? true;
                const hadPendingDevice = !currentHasManifest && this.folderInfo !== null;
                const hasPendingDevice = newPendingPath !== null;

                const currentDevice = daemonStateResult?.currentDevice;
                const isNewDevice = currentDevice?.deviceId && currentDevice.deviceId !== this.lastHydratedDeviceId;
                const deviceDisconnected = !currentDevice && this.lastHydratedDeviceId !== null;
                const activeOperationId = daemonStateResult?.activeOperationId ?? null;

                // Detect multi-device changes
                const newConnectedDevices: Array<{ path: string; deviceId: string; name: string; icon?: string | null }> =
                    daemonStateResult?.connectedDevices ?? [];
                // Use explicit null check so that selectedDevicePath: null from daemon clears local state
                const newSelectedDevicePath: string | null =
                    'selectedDevicePath' in (daemonStateResult ?? {})
                        ? daemonStateResult.selectedDevicePath
                        : this.selectedDevicePath;
                const deviceCountChanged = newConnectedDevices.length !== this.connectedDevices.length;
                const selectedDeviceChanged = newSelectedDevicePath !== this.selectedDevicePath;
                this.connectedDevices = newConnectedDevices;
                this.selectedDevicePath = newSelectedDevicePath;

                if (newDirty !== this.isDirtyManifest || hasPendingDevice !== hadPendingDevice || isNewDevice || deviceDisconnected || activeOperationId || deviceCountChanged || selectedDeviceChanged) {
                    this.isDirtyManifest = newDirty;
                    if (isNewDevice || deviceDisconnected || activeOperationId || deviceCountChanged || selectedDeviceChanged) {
                        // Let refreshAndRender handle the hydration/attach logic reliably on state change (F5)
                        await this.refreshAndRender();
                    } else {
                        this.refreshAndRender();
                    }
                }
            } catch (err) {
                // Ignore transient errors
            }
        }, 2000);
    }


    private renderDeviceHub(): string {
        if (this.connectedDevices.length === 0) return '';
        return `
            <div class="device-hub-panel">
                <div class="device-hub-cards">
                    ${this.connectedDevices.map(d => `
                        <div class="device-hub-card ${d.path === this.selectedDevicePath ? 'active' : ''}"
                             data-path="${this.escapeHtml(d.path)}">
                            <sl-icon name="${this.escapeHtml(d.icon || 'usb-drive')}"
                                     class="device-hub-icon"></sl-icon>
                            <span class="device-hub-name">${this.escapeHtml(d.name || d.deviceId)}</span>
                        </div>
                    `).join('')}
                </div>
            </div>
        `;
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

        // Show Initialize Device banner when device is connected but has no manifest
        if (!hasManifest) {
            return `
                <div class="device-folders-panel">
                    <div class="dirty-manifest-banner" id="open-init-device-btn" title="Initialize this device for syncing">
                        <sl-icon name="usb-drive"></sl-icon>
                        <div class="dirty-manifest-banner-text">
                            <strong>New Device Detected</strong>
                            Click Initialize to set up sync
                        </div>
                        <sl-button size="small" variant="primary" id="init-device-btn">Initialize</sl-button>
                    </div>
                </div>
            `;
        }

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

    private renderStatusZone(): string {
        if (!basketStore.isDirty()) {
            return '<div class="basket-status-zone"></div>';
        }

        return `
            <div class="basket-status-zone">
                <div class="sync-proposed-banner">
                    <sl-icon name="arrow-repeat"></sl-icon>
                    <span>Sync Proposed — Basket changed</span>
                </div>
            </div>
        `;
    }

    private updateDeviceLockState(): void {
        const libraryContent = document.getElementById('library-content');
        if (libraryContent) {
            libraryContent.classList.toggle('device-locked', this.selectedDevicePath === null);
        }
    }

    private renderLockedBasket(): void {
        this.container.innerHTML = `
            <div class="basket-header">
                <h2>Basket</h2>
                <sl-badge variant="neutral" pill>0</sl-badge>
            </div>
            <div class="basket-placeholder">
                <sl-icon name="usb-drive" style="font-size: 2rem; opacity: 0.5;"></sl-icon>
                <p style="opacity: 0.5;">Select a device to start curating</p>
            </div>
            <div class="basket-footer">
                ${this.renderDeviceHub()}
                ${this.renderDeviceFolders()}
            </div>
            <div class="basket-actions">
                <sl-button id="start-sync-btn" variant="primary" style="width: 100%;" disabled>
                    <sl-icon slot="prefix" name="cloud-download"></sl-icon>
                    Start Sync
                </sl-button>
            </div>
        `;
        this.updateDeviceLockState();
        this.bindDeviceHubEvents();
        this.container.querySelector('#device-folders-toggle')?.addEventListener('click', () => {
            this.isFoldersExpanded = !this.isFoldersExpanded;
            this.render();
        });
        this.container.querySelector('#init-device-btn')?.addEventListener('click', () => this.openInitDeviceModal());
    }

    public render() {
        if (this.isDestroyed) return;
        if (this.showSyncComplete) {
            this.updateDeviceLockState();
            this.renderSyncComplete();
            return;
        }
        if (this.syncErrorMessages) {
            this.updateDeviceLockState();
            this.renderSyncError(this.syncErrorMessages);
            return;
        }
        if (this.isSyncing && this.currentOperation) {
            this.updateDeviceLockState();
            this.renderSyncProgress();
            return;
        }
        if (this.isSyncing) {
            this.updateDeviceLockState();
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

        // Locked state: no device selected (includes all-disconnected case) → show placeholder
        if (this.selectedDevicePath === null) {
            this.renderLockedBasket();
            return;
        }

        this.updateDeviceLockState();

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
                    ${this.renderAutoFillControls()}
                    ${this.renderStatusZone()}
                    ${this.renderDeviceHub()}
                    ${this.renderDeviceFolders()}
                </div>
                <div class="basket-actions">
                    <sl-button id="start-sync-btn" variant="primary" style="width: 100%;" ${(!basketStore.isDirty() && !this.autoFillEnabled) || !this.selectedDevicePath ? 'disabled' : ''}>
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
            this.container.querySelector('#init-device-btn')?.addEventListener('click', () => this.openInitDeviceModal());
            this.container.querySelector('#start-sync-btn')?.addEventListener('click', () => this.handleStartSync());
            this.bindAutoFillEvents();
            this.bindDeviceHubEvents();
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
                ${this.renderAutoFillControls()}
                ${this.renderStatusZone()}
                ${this.renderDeviceHub()}
                ${this.renderDeviceFolders()}
            </div>
            <div class="basket-actions">
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
                               ${!this.selectedDevicePath ? 'disabled' : this.isSyncing ? 'loading disabled' : ''}>
                        <sl-icon slot="prefix" name="cloud-download"></sl-icon>
                        ${this.isSyncing ? 'Syncing...' : 'Start Sync'}
                    </sl-button>
                `}
                <sl-button variant="text" size="small" class="clear-basket-btn" style="width: 100%;">
                    Clear All
                </sl-button>
            </div>
        `;

        // Load basket item images asynchronously
        this.loadBasketImages();

        // Bind events
        this.container.querySelectorAll('.remove-item-btn').forEach(btn => {
            btn.addEventListener('click', (e) => {
                const id = (e.currentTarget as HTMLElement).getAttribute('data-id');
                if (id) {
                    if (id === AUTO_FILL_SLOT_ID) {
                        this.autoFillEnabled = false;
                        this.persistAutoFillPrefs();
                    }
                    basketStore.remove(id);
                }
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
        this.container.querySelector('#init-device-btn')?.addEventListener('click', () => this.openInitDeviceModal());
        this.bindAutoFillEvents();
        this.bindDeviceHubEvents();
    }

    private openRepairModal() {
        const modal = new RepairModal(this.container, () => {
            this.isDirtyManifest = false;
            this.refreshAndRender();
        });
        modal.open();
    }

    private openInitDeviceModal() {
        const modal = new InitDeviceModal(this.container, () => {
            this.refreshAndRender();
        });
        modal.open();
    }

    private async handleStartSync() {
        if (this.isSyncing) return;
        const currentItems = basketStore.getItems();

        // Detect and extract the auto-fill slot
        const autoFillSlot = currentItems.find(i => i.id === AUTO_FILL_SLOT_ID);
        const manualIds = currentItems.filter(i => i.id !== AUTO_FILL_SLOT_ID).map(i => i.id);

        // Take snapshot for race-safe dirty reset (exclude virtual slot — it won't appear in manifest)
        this.syncSnapshotIds = [...manualIds].sort();

        // Build delta request params
        const deltaParams: Record<string, unknown> = { itemIds: manualIds };
        if (autoFillSlot) {
            // Recompute fill budget fresh from current storage state — the slot's
            // sizeBytes may be stale (e.g. set to 0 when storageInfo was null).
            const manualSize = basketStore.getManualSizeBytes();
            const maxFillBytes = this.storageInfo
                ? Math.max(this.storageInfo.freeBytes - manualSize, 0)
                : autoFillSlot.sizeBytes || 0;
            deltaParams.autoFill = {
                enabled: true,
                maxBytes: maxFillBytes > 0 ? maxFillBytes : undefined,
                excludeItemIds: manualIds,
            };
        }

        try {
            this.isSyncing = true;
            this.showSyncComplete = false;
            this.syncErrorMessages = null;
            this.currentOperation = null;
            this.currentOperationId = null;
            this.etaText = 'Calculating\u2026';
            this.render();

            const delta = await rpcCall('sync_calculate_delta', deltaParams);
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
        let consecutiveFailures = 0;
        this.pollingInterval = window.setInterval(async () => {
            if (!this.currentOperationId) {
                this.stopPolling();
                return;
            }
            try {
                const op = await rpcCall('sync_get_operation_status', {
                    operationId: this.currentOperationId
                }) as SyncOperation;
                consecutiveFailures = 0;
                this.currentOperation = op;
                this.renderSyncProgress();

                if (op.status === 'complete') {
                    this.stopPolling();
                    await this.handleSyncComplete();
                } else if (op.status === 'failed') {
                    this.stopPolling();
                    this.handleSyncFailed(op);
                }
            } catch (err) {
                console.error('[Sync] Progress poll failed:', err);
                consecutiveFailures++;
                if (consecutiveFailures >= 3) {
                    this.stopPolling();
                    this.isSyncing = false;
                    this.currentOperationId = null;
                    this.currentOperation = null;
                    this.render();
                }
            }
        }, 500);
    }

    private stopPolling() {
        if (this.pollingInterval !== null) {
            clearInterval(this.pollingInterval);
            this.pollingInterval = null;
        }
    }

    private computeEta(op: SyncOperation): string {
        if (op.totalBytes <= 0 || op.bytesTransferred <= 0) return 'Calculating\u2026';

        const elapsedSeconds = (Date.now() - new Date(op.startedAt).getTime()) / 1000;
        if (elapsedSeconds <= 0 || isNaN(elapsedSeconds)) return 'Calculating\u2026';

        const totalRate = op.bytesTransferred / elapsedSeconds;
        if (totalRate <= 0) return 'Calculating\u2026';

        const remaining = Math.max(0, op.totalBytes - op.bytesTransferred);
        if (remaining <= 0) return 'Almost done\u2026';

        const etaSeconds = remaining / totalRate;

        if (etaSeconds < 10) return 'Almost done\u2026';
        if (etaSeconds < 60) return `~${Math.round(etaSeconds)} sec left`;
        return `~${Math.round(etaSeconds / 60)} min left`;
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

        this.etaText = this.computeEta(op);

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
                <div class="sync-eta">${this.escapeHtml(this.etaText)}</div>
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
            this.refreshAndRender();
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
            this.refreshAndRender();
        });
    }

    private async handleSyncComplete() {
        if (this.isDestroyed) return;
        this.isSyncing = false;

        // Fetch fresh storage info so capacity bar is accurate immediately after sync
        try {
            this.storageInfo = await rpcCall('device_get_storage_info');
        } catch (err) {
            console.error("Failed to refresh storage info after sync", err);
        }

        this.currentOperationId = null;
        this.currentOperation = null;
        this.showSyncComplete = true;
        this.syncErrorMessages = null;
        this.etaText = 'Calculating\u2026';

        // Reset dirty if current items match snapshot (no mid-sync changes)
        const currentIds = basketStore.getItems().filter(i => i.id !== AUTO_FILL_SLOT_ID).map(i => i.id).sort();
        if (JSON.stringify(currentIds) === JSON.stringify(this.syncSnapshotIds)) {
            console.log("Sync complete, basket unchanged during sync. Resetting dirty flag.");
            basketStore.resetDirty();
        } else {
            console.log("Sync complete, but basket changed during sync. Keeping dirty flag.");
        }

        this.renderSyncComplete();
    }

    private handleSyncFailed(operation: SyncOperation) {
        if (this.isDestroyed) return;
        this.isSyncing = false;
        this.currentOperationId = null;
        this.currentOperation = null;
        this.showSyncComplete = false;
        this.etaText = 'Calculating\u2026';
        this.syncErrorMessages = operation.errors.length > 0
            ? operation.errors.map(e => {
                const target = e.filename || e.jellyfinId || 'Unknown file';
                const message = e.errorMessage || 'Unknown file error';
                return `${target}: ${message}`;
            })
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

    private renderPriorityLabel(reason: string): string {
        if (reason === 'favorite') return '★ Favorite';
        if (reason.startsWith('playCount:')) {
            const count = reason.split(':')[1];
            return `▶ ${count} plays`;
        }
        if (reason === 'new' || reason === '') return 'New';
        return 'Auto';
    }

    private renderAutoFillSlotCard(item: BasketItem): string {
        return `
            <div class="basket-item-card basket-item-auto-fill-slot" data-id="${AUTO_FILL_SLOT_ID}">
                <div class="basket-item-auto-fill-icon">
                    <sl-icon name="stars"></sl-icon>
                </div>
                <div class="basket-item-info">
                    <div class="basket-item-name">Auto-Fill Slot</div>
                    <div class="basket-item-meta">
                        Will fill ~${formatSize(item.sizeBytes)} with top-priority tracks at sync time
                    </div>
                </div>
                <sl-icon-button name="x" class="remove-item-btn" data-id="${AUTO_FILL_SLOT_ID}" label="Remove"></sl-icon-button>
            </div>
        `;
    }

    private renderArtistCard(item: BasketItem): string {
        return `
            <div class="basket-item-card basket-item-artist" data-id="${this.escapeHtml(item.id)}">
                <div class="basket-item-artist-icon">
                    <sl-icon name="person-fill"></sl-icon>
                </div>
                <div class="basket-item-info">
                    <div class="basket-item-name">${this.escapeHtml(item.name)}</div>
                    <div class="basket-item-meta">
                        Artist · ~${item.childCount ?? 0} tracks · ~${formatSize(item.sizeBytes ?? 0)}
                    </div>
                </div>
                <sl-icon-button name="x" class="remove-item-btn" data-id="${this.escapeHtml(item.id)}" label="Remove"></sl-icon-button>
            </div>
        `;
    }

    private renderItem(item: BasketItem): string {
        if (item.id === AUTO_FILL_SLOT_ID) {
            return this.renderAutoFillSlotCard(item);
        }
        if (item.type === 'MusicArtist') {
            return this.renderArtistCard(item);
        }
        const autoBadge = item.autoFilled
            ? `<span class="basket-item-auto-badge" title="Added by Auto-Fill">Auto</span>`
            : '';
        // Always show priority label for auto-filled items; fall back to empty string
        // so renderPriorityLabel returns 'Auto' for items hydrated from older manifests (P13).
        const priorityLabel = item.autoFilled
            ? `<span class="basket-item-priority-reason">${this.escapeHtml(this.renderPriorityLabel(item.priorityReason ?? ''))}</span>`
            : '';

        return `
            <div class="basket-item-card ${item.autoFilled ? 'basket-item-auto' : ''}" data-id="${item.id}">
                <div class="basket-item-image" data-image-id="${item.id}"></div>
                <div class="basket-item-info">
                    <div class="basket-item-name">
                        ${autoBadge}
                        ${this.escapeHtml(item.name)}
                    </div>
                    <div class="basket-item-meta">
                        ${item.childCount} tracks • ${item.type}
                        ${priorityLabel}
                    </div>
                </div>
                <sl-icon-button name="x" class="remove-item-btn" data-id="${item.id}" label="Remove"></sl-icon-button>
            </div>
        `;
    }

    /** Load basket item images asynchronously after HTML is in the DOM. */
    private loadBasketImages(): void {
        const imageEls = this.container.querySelectorAll<HTMLElement>('.basket-item-image[data-image-id]');
        for (const el of imageEls) {
            const id = el.dataset.imageId;
            if (!id) continue;
            getImageUrl(id, 100, 80).then(dataUrl => {
                el.style.backgroundImage = `url('${dataUrl}')`;
            }).catch(() => { /* image load failed, leave blank */ });
        }
    }

    private escapeHtml(text: string): string {
        const div = document.createElement('div');
        div.textContent = text;
        return div.innerHTML.replace(/"/g, '&quot;');
    }
}
