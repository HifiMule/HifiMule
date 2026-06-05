// Basket Sidebar Component
// Displays the list of items selected for synchronization.

import { basketStore, BasketItem, AUTO_FILL_SLOT_ID } from '../state/basket';
import { rpcCall, getImageUrl } from '../rpc';
import { RepairModal } from './RepairModal';
import { InitDeviceModal } from './InitDeviceModal';
import { t } from '../i18n';

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
    pendingDeviceFriendlyName?: string;
}

interface ConnectedDeviceSummary {
    path: string;
    deviceId: string;
    name: string;
    icon?: string | null;
    managedPaths?: string[];
    playlistPath?: string | null;
    transcodingProfileId?: string | null;
}

interface DeviceProfileSummary {
    id: string;
    name: string;
    description?: string;
    defaultMusicFolder?: string | null;
    defaultPlaylistFolder?: string | null;
}

interface SyncOperation {
    id: string;
    status: 'running' | 'complete' | 'failed' | 'cancelled';
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
                    <div class="capacity-selection-total">${t('basket.capacity.selection', { size: formatSize(projectedBytes) })}</div>
                    <div class="capacity-bar-container capacity-bar-disabled">
                        <div class="capacity-bar">
                            <div class="capacity-segment capacity-grey" style="width: 100%;"></div>
                        </div>
                    </div>
                    <div class="capacity-no-device-label">
                        <sl-icon name="usb-drive" style="font-size: 0.9rem;"></sl-icon>
                        ${t('basket.no_device_connected')}
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
        statusMessage = t('basket.capacity.remaining', { size: formatSize(remaining) });
        statusIcon = '<sl-icon name="check-circle" style="color: var(--sl-color-success-600);"></sl-icon>';
    } else if (zone === 'amber') {
        statusMessage = t('basket.capacity.tight_fit', { size: formatSize(remaining) });
    } else {
        statusMessage = t('basket.capacity.exceeds', { size: formatSize(Math.abs(remaining)) });
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
    private serverType: string | null = null;
    private currentServerId: string | null = null;
    private syncSnapshotIds: string[] = [];
    // Auto-fill state
    private autoFillEnabled: boolean = false;
    private autoFillMaxBytes: number | null = null;
    private autoSyncOnConnect: boolean = false;
    private etaText: string = t('basket.sync.calculating');
    // Multi-device hub state
    private connectedDevices: ConnectedDeviceSummary[] = [];
    private selectedDevicePath: string | null = null;
    private deviceSwitchInFlight: boolean = false;
    private pendingDeviceFriendlyName: string | undefined = undefined;
    private currentDevice: any = null;
    private syncPreviewCleanupDevicePath: string | null = null;
    private forceSyncMode: boolean = false;
    // Cancel state
    private isCancelling: boolean = false;
    // Transfer stats captured at completion for the sync-complete screen
    private completedFilesCount: number = 0;
    private completedBytesCount: number = 0;

    constructor(container: HTMLElement) {
        this.container = container;
        this.updateListener = () => this.refreshAndRender();
        // Custom clickable divs (device cards, folder toggle, repair banner) are
        // rendered as role="button". One delegated handler makes them keyboard-
        // operable; it lives on the persistent container, so it survives the
        // innerHTML re-renders that replace those elements.
        this.container.addEventListener('keydown', this.keyActivateHandler);
        this.init();
        this.startDaemonStatePolling();
    }

    // Enter/Space activates a synthetic (role="button") div, matching native
    // button semantics. Skips real interactive children so we never double-fire.
    private keyActivateHandler = (event: KeyboardEvent): void => {
        if (event.key !== 'Enter' && event.key !== ' ' && event.key !== 'Spacebar') return;
        const target = event.target as HTMLElement | null;
        const btn = target?.closest('[role="button"]') as HTMLElement | null;
        if (!btn || !this.container.contains(btn)) return;
        if (target !== btn && target?.closest('sl-button, sl-icon-button, button, a, input')) return;
        event.preventDefault();
        btn.click();
    };

    private init() {
        basketStore.addEventListener('update', this.updateListener);
        this.refreshAndRender();
    }

    private getCurrentDeviceId(currentDevice: any): string | null {
        return currentDevice?.deviceId ?? currentDevice?.device_id ?? null;
    }

    private getAutoSyncOnConnect(state: any): boolean {
        return state?.autoSyncOnConnect
            ?? state?.currentDevice?.autoSyncOnConnect
            ?? state?.currentDevice?.auto_sync_on_connect
            ?? false;
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
            this.serverType = state.serverType ?? null;
            this.currentServerId = state.currentServer?.serverId ?? null;
            basketStore.setActiveServerId(this.currentServerId);
            // Sync multi-device state so the hub renders correctly on every refreshAndRender,
            // not just during the 2s polling cycle.
            this.connectedDevices = state.connectedDevices ?? this.connectedDevices;
            this.pendingDeviceFriendlyName = state.pendingDeviceFriendlyName ?? undefined;
            // Use explicit field-presence check: if field present in response, use it (including null);
            // otherwise keep current. Fixes the null-coalescing bug where selectedDevicePath: null
            // would be ignored by the ?? operator.
            if ('selectedDevicePath' in state) {
                this.selectedDevicePath = state.selectedDevicePath;
            }
            const currentDevice = state.currentDevice;
            this.currentDevice = currentDevice ?? null;
            const currentDeviceId = this.getCurrentDeviceId(currentDevice);
            if (currentDeviceId && currentDeviceId !== this.lastHydratedDeviceId) {
                this.lastHydratedDeviceId = currentDeviceId;
                // Load saved auto-fill preferences from manifest
                this.autoFillEnabled = state.autoFill?.enabled ?? false;
                this.autoFillMaxBytes = state.autoFill?.maxBytes ?? null;
                this.autoSyncOnConnect = this.getAutoSyncOnConnect(state);
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
            } else if (currentDeviceId) {
                this.autoFillEnabled = state.autoFill?.enabled ?? false;
                this.autoFillMaxBytes = state.autoFill?.maxBytes ?? null;
                this.autoSyncOnConnect = this.getAutoSyncOnConnect(state);
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
        this.container.removeEventListener('keydown', this.keyActivateHandler);
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
            name: t('basket.autofill.name'),
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
            (autoFillToggle as any).checked = this.autoFillEnabled;
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
                const gb = (e.target as any).value as number;
                // Guard against NaN (non-numeric input) and negative values (P10).
                if (isNaN(gb) || gb < 0) return;
                this.autoFillMaxBytes = gb * 1024 * 1024 * 1024;
                this.persistAutoFillPrefs();
                this.insertAutoFillSlot();
            });
        }

        const autoSyncToggle = this.container.querySelector('#auto-sync-toggle');
        if (autoSyncToggle) {
            (autoSyncToggle as any).checked = this.autoSyncOnConnect;
            autoSyncToggle.addEventListener('sl-change', (e: Event) => {
                this.autoSyncOnConnect = (e.target as HTMLInputElement).checked;
                this.persistAutoFillPrefs();
            });
        }
    }

    private bindDeviceHubEvents(): void {
        this.container.querySelectorAll('.device-hub-card').forEach(card => {
            card.addEventListener('click', async (event) => {
                if ((event.target as HTMLElement | null)?.closest('.device-settings-btn')) return;
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
        this.container.querySelectorAll('.device-settings-btn').forEach(btn => {
            btn.addEventListener('click', (event) => {
                event.stopPropagation();
                void this.openDeviceSettings();
            });
        });
    }

    private selectedDeviceSummary(): ConnectedDeviceSummary | null {
        return this.connectedDevices.find(d => d.path === this.selectedDevicePath) ?? this.connectedDevices[0] ?? null;
    }

    private async openDeviceSettings(): Promise<void> {
        const selected = this.selectedDeviceSummary();
        const current = this.currentDevice ?? {};
        if (!selected) return;

        let profiles: DeviceProfileSummary[] = [];
        try {
            profiles = await rpcCall('device_profiles.list') as DeviceProfileSummary[];
        } catch (err) {
            profiles = [{ id: 'passthrough', name: t('basket.profile.no_transcoding'), description: t('basket.profile.no_transcoding_desc') }];
        }

        const musicFolder = selected.managedPaths?.[0]
            ?? current.managed_paths?.[0]
            ?? current.managedPaths?.[0]
            ?? '';
        const playlistFolder = selected.playlistPath
            ?? current.playlistPath
            ?? current.playlist_path
            ?? '';
        let selectedIcon = selected.icon || 'usb-drive';
        const selectedProfileId = selected.transcodingProfileId
            ?? current.transcodingProfileId
            ?? current.transcoding_profile_id
            ?? 'passthrough';
        const profileOptions = profiles
            .map(profile => `<sl-option value="${this.escapeHtml(profile.id)}">${this.escapeHtml(profile.name)}</sl-option>`)
            .join('');
        const selectedProfile = profiles.find(profile => profile.id === selectedProfileId)
            ?? profiles.find(profile => profile.id === 'passthrough');
        const dialog = document.createElement('sl-dialog') as any;
        dialog.label = t('basket.device.settings');
        dialog.className = 'device-settings-dialog';
        dialog.innerHTML = `
            <div class="device-settings-form">
                <sl-input id="device-settings-name" label="${t('basket.device.name')}" value="${this.escapeHtml(selected.name || selected.deviceId)}"></sl-input>
                <div>
                    <label class="device-settings-label">${t('basket.device.icon')}</label>
                    <div id="device-settings-icon-picker" class="device-settings-icon-picker">
                        ${['usb-drive', 'phone-fill', 'watch', 'sd-card', 'headphones', 'music-note-list'].map(icon => `
                            <div class="init-icon-tile ${icon === selectedIcon ? 'selected' : ''}"
                                 data-icon="${icon}">
                                <sl-icon name="${icon}"></sl-icon>
                                <span>${this.iconLabel(icon)}</span>
                            </div>
                        `).join('')}
                    </div>
                </div>
                <sl-select id="device-settings-transcoding-profile" label="${t('basket.device.transcoding_profile')}" value="${this.escapeHtml(selectedProfileId)}">
                    ${profileOptions}
                </sl-select>
                <div id="device-settings-transcoding-desc" class="device-settings-description">
                    ${this.escapeHtml(selectedProfile?.description ?? '')}
                </div>
                <sl-input id="device-settings-music" label="${t('basket.device.music_folder')}" value="${this.escapeHtml(musicFolder)}"></sl-input>
                <sl-input id="device-settings-playlist" label="${t('basket.device.playlist_folder')}" placeholder="${this.escapeHtml(musicFolder)}" value="${this.escapeHtml(playlistFolder ?? '')}"></sl-input>
                <sl-alert id="device-settings-error" variant="danger" closable style="display:none;"></sl-alert>
            </div>
            <sl-button slot="footer" variant="default" id="device-settings-cancel">${t('basket.actions.cancel')}</sl-button>
            <sl-button slot="footer" variant="primary" id="device-settings-save">
                <sl-icon slot="prefix" name="check2"></sl-icon>
                ${t('basket.actions.save')}
            </sl-button>
        `;
        document.body.appendChild(dialog);
        dialog.querySelectorAll('.init-icon-tile').forEach((tile: Element) => {
            tile.addEventListener('click', () => {
                selectedIcon = (tile as HTMLElement).dataset.icon ?? 'usb-drive';
                dialog.querySelectorAll('.init-icon-tile').forEach((t: Element) => {
                    const el = t as HTMLElement;
                    const isSelected = el.dataset.icon === selectedIcon;
                    el.classList.toggle('selected', isSelected);
                });
            });
        });
        const profileSelect = dialog.querySelector('#device-settings-transcoding-profile') as any;
        const profileDesc = dialog.querySelector('#device-settings-transcoding-desc') as HTMLElement | null;
        const musicInput = dialog.querySelector('#device-settings-music') as any;
        const playlistInput = dialog.querySelector('#device-settings-playlist') as any;
        let foldersEdited = false;
        musicInput?.addEventListener('sl-input', () => { foldersEdited = true; });
        playlistInput?.addEventListener('sl-input', () => { foldersEdited = true; });
        profileSelect?.addEventListener('sl-change', (event: any) => {
            const profile = profiles.find(p => p.id === event.target.value);
            if (profileDesc) profileDesc.textContent = profile?.description ?? '';
            if (!foldersEdited && profile) {
                musicInput.value = profile.defaultMusicFolder ?? musicInput.value ?? '';
                playlistInput.value = profile.defaultPlaylistFolder ?? playlistInput.value ?? '';
                playlistInput.placeholder = profile.defaultMusicFolder ?? musicInput.value ?? '';
            }
        });
        dialog.querySelector('#device-settings-cancel')?.addEventListener('click', () => dialog.hide());
        dialog.querySelector('#device-settings-save')?.addEventListener('click', async () => {
            const saveButton = dialog.querySelector('#device-settings-save') as any;
            const error = dialog.querySelector('#device-settings-error') as HTMLElement | null;
            if (saveButton) saveButton.loading = true;
            if (error) error.style.display = 'none';
            try {
                const musicFolderValue = ((dialog.querySelector('#device-settings-music') as any)?.value ?? '').trim();
                const playlistFolderValue = ((dialog.querySelector('#device-settings-playlist') as any)?.value ?? '').trim();
                const payload: Record<string, unknown> = {
                    deviceId: selected.deviceId,
                    name: (dialog.querySelector('#device-settings-name') as any)?.value ?? '',
                    icon: selectedIcon || null,
                    transcodingProfileId: (dialog.querySelector('#device-settings-transcoding-profile') as any)?.value ?? 'passthrough',
                    playlistFolderPath: playlistFolderValue,
                };
                if (musicFolderValue !== '') {
                    payload.musicFolderPath = musicFolderValue;
                }
                const result = await rpcCall('device.update_manifest', payload) as any;
                this.syncPreviewCleanupDevicePath = result?.relocationRequired === true ? selected.path : null;
                dialog.hide();
                await this.refreshAndRender();
            } catch (err) {
                const message = typeof err === 'string' ? err : (err instanceof Error ? err.message : String(err));
                if (error) {
                    error.textContent = message;
                    error.style.display = '';
                    (error as any).open = true;
                }
            } finally {
                if (saveButton) saveButton.loading = false;
            }
        });
        dialog.addEventListener('sl-after-hide', (event: Event) => {
            if (event.target === dialog) {
                dialog.remove();
            }
        });
        await customElements.whenDefined('sl-dialog');
        await customElements.whenDefined('sl-select');
        await (dialog as any).updateComplete;
        dialog.show();
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
                        ${t('basket.autofill.name')}
                    </sl-switch>
                    <span class="auto-fill-hint" style="font-size:0.75rem; opacity:0.6;">
                        ${t('basket.autofill.hint')}
                    </span>
                </div>
                ${this.autoFillEnabled && deviceFull ? `
                    <div class="auto-fill-full-notice" style="margin-top:0.5rem; font-size:0.75rem; opacity:0.7;">
                        ${t('basket.autofill.full')}
                    </div>
                ` : ''}
                ${this.autoFillEnabled && !deviceFull ? `
                    <div class="auto-fill-slider-row" style="margin-top:0.5rem;">
                        <label style="font-size:0.75rem; opacity:0.7; display:block; margin-bottom:0.25rem;">
                            ${t('basket.autofill.max_fill_size', { size: `${sliderValue} GB` })}
                        </label>
                        <sl-range id="auto-fill-slider"
                            min="0" max="${sliderMax}" step="1" value="${sliderValue}"
                            style="width:100%;">
                        </sl-range>
                    </div>
                ` : ''}
                <div class="auto-fill-toggle-row" style="margin-top:0.5rem;">
                    <sl-switch id="auto-sync-toggle" size="small" ${this.autoSyncOnConnect ? 'checked' : ''}>
                        ${t('basket.autofill.auto_sync_on_connect')}
                    </sl-switch>
                </div>
                <div style="font-size:0.7rem; opacity:0.55; margin-top:0.2rem; padding-left:0.5rem;">
                    ${t('basket.autofill.auto_sync_hint')}
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
                this.currentDevice = currentDevice ?? null;
                this.serverType = daemonStateResult?.serverType ?? null;
                this.currentServerId = daemonStateResult?.currentServer?.serverId ?? null;
                basketStore.setActiveServerId(this.currentServerId);
                const currentDeviceId = this.getCurrentDeviceId(currentDevice);
                const isNewDevice = currentDeviceId && currentDeviceId !== this.lastHydratedDeviceId;
                const deviceDisconnected = !currentDevice && this.lastHydratedDeviceId !== null;
                const activeOperationId = daemonStateResult?.activeOperationId ?? null;

                // Detect multi-device changes
                const newConnectedDevices: ConnectedDeviceSummary[] = daemonStateResult?.connectedDevices ?? [];
                // Use explicit null check so that selectedDevicePath: null from daemon clears local state
                const newSelectedDevicePath: string | null =
                    'selectedDevicePath' in (daemonStateResult ?? {})
                        ? daemonStateResult.selectedDevicePath
                        : this.selectedDevicePath;
                const deviceCountChanged = newConnectedDevices.length !== this.connectedDevices.length;
                const selectedDeviceChanged = newSelectedDevicePath !== this.selectedDevicePath;
                if (selectedDeviceChanged || deviceDisconnected || activeOperationId) {
                    this.syncPreviewCleanupDevicePath = null;
                }
                const autoPrefsChanged = currentDevice
                    && (
                        (daemonStateResult?.autoFill?.enabled ?? false) !== this.autoFillEnabled
                        || (daemonStateResult?.autoFill?.maxBytes ?? null) !== this.autoFillMaxBytes
                        || this.getAutoSyncOnConnect(daemonStateResult) !== this.autoSyncOnConnect
                    );
                this.connectedDevices = newConnectedDevices;
                this.selectedDevicePath = newSelectedDevicePath;
                this.pendingDeviceFriendlyName = daemonStateResult?.pendingDeviceFriendlyName ?? undefined;

                if (newDirty !== this.isDirtyManifest || hasPendingDevice !== hadPendingDevice || isNewDevice || deviceDisconnected || activeOperationId || deviceCountChanged || selectedDeviceChanged || autoPrefsChanged) {
                    this.isDirtyManifest = newDirty;
                    if (isNewDevice || deviceDisconnected || activeOperationId || deviceCountChanged || selectedDeviceChanged || autoPrefsChanged) {
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
                             data-path="${this.escapeHtml(d.path)}"
                             role="button" tabindex="0"
                             aria-pressed="${d.path === this.selectedDevicePath ? 'true' : 'false'}">
                            <sl-icon name="${this.escapeHtml(d.icon || 'usb-drive')}"
                                     class="device-hub-icon"></sl-icon>
                            <span class="device-hub-name">${this.escapeHtml(d.name || d.deviceId)}</span>
                            ${d.path === this.selectedDevicePath ? `
                                <sl-icon-button name="gear" label="${t('basket.device.settings')}" class="device-settings-btn"></sl-icon-button>
                            ` : ''}
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
                        ${t('basket.device.connect_to_view_folders')}
                    </div>
                </div>
            `;
        }

        const { folders, managedCount, unmanagedCount, hasManifest } = this.folderInfo;

        // Show Initialize Device banner when device is connected but has no manifest
        if (!hasManifest) {
            return `
                <div class="device-folders-panel">
                    <div class="dirty-manifest-banner" id="open-init-device-btn" title="${t('basket.device.initialize_title')}">
                        <sl-icon name="usb-drive"></sl-icon>
                        <div class="dirty-manifest-banner-text">
                            <strong>${t('basket.device.new_detected')}</strong>
                            ${t('basket.device.click_initialize')}
                        </div>
                        <sl-button size="small" variant="primary" id="init-device-btn">${t('basket.device.initialize')}</sl-button>
                    </div>
                </div>
            `;
        }

        const relocationBanner = this.syncPreviewCleanupDevicePath === this.folderInfo.devicePath ? `
            <div class="dirty-manifest-banner device-relocation-banner">
                <sl-icon name="arrow-repeat"></sl-icon>
                <div class="dirty-manifest-banner-text">
                    <strong>${t('basket.device.folder_layout_changed')}</strong>
                    ${t('basket.device.relocation_hint')}
                </div>
            </div>
        ` : '';

        const isMtp = this.folderInfo.devicePath.toLowerCase().startsWith('mtp://');
        const unmanagedSummary = isMtp
            ? t('basket.device.mtp_no_folder_enum')
            : t('basket.device.protected_count', { count: unmanagedCount });

        let content = `
            <div class="device-folders-panel">
                ${relocationBanner}
                <div class="device-folders-header" id="device-folders-toggle"
                     role="button" tabindex="0" aria-expanded="${this.isFoldersExpanded ? 'true' : 'false'}">
                    <h3>${t('basket.device.folders')}</h3>
                    <div style="display: flex; align-items: center; gap: 0.5rem;">
                        <span class="device-folders-summary">${t('basket.device.managed_count', { count: managedCount })} | ${unmanagedSummary}</span>
                        <sl-icon name="${this.isFoldersExpanded ? 'chevron-up' : 'chevron-down'}" style="font-size: 0.8rem; opacity: 0.5;"></sl-icon>
                    </div>
                </div>
        `;

        if (this.isFoldersExpanded) {
            content += `
                <div class="device-folders-list">
                    ${folders.length === 0 ? `<div style="font-size: 0.8rem; opacity: 0.5; padding: 0.5rem;">${t('basket.device.no_folders_found')}</div>` : ''}
                    ${folders.map(f => `
                        <div class="folder-item ${f.isManaged ? 'folder-managed' : 'folder-protected'}">
                            <sl-icon name="${f.isManaged ? 'unlock' : 'shield-lock'}" class="folder-icon"></sl-icon>
                            <span class="folder-name" title="${this.escapeHtml(f.name)}">${this.escapeHtml(f.name)}</span>
                            <span class="folder-status">${f.isManaged ? t('basket.device.managed') : t('basket.device.protected')}</span>
                        </div>
                    `).join('')}
                </div>
            `;
        }

        // Show dirty manifest banner if flagged
        if (this.isDirtyManifest) {
            content += `
                <div class="dirty-manifest-banner" id="open-repair-btn" title="${t('basket.manifest.open_repair')}"
                    role="button" tabindex="0" aria-label="${t('basket.manifest.open_repair')}">
                    <sl-icon name="exclamation-triangle-fill"></sl-icon>
                    <div class="dirty-manifest-banner-text">
                        <strong>${t('basket.manifest.dirty')}</strong>
                        ${t('basket.manifest.interrupted')}
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
                    <span>${t('basket.sync.proposed')}</span>
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
                <h2>${t('basket.title')}</h2>
                <sl-badge variant="neutral" pill>0</sl-badge>
            </div>
            <div class="basket-placeholder">
                <sl-icon name="usb-drive" style="font-size: 2rem; opacity: 0.5;"></sl-icon>
                <p style="opacity: 0.5;">${t('basket.select_device')}</p>
            </div>
            <div class="basket-footer">
                ${this.renderDeviceHub()}
                ${this.renderDeviceFolders()}
            </div>
            <div class="basket-actions">
                <sl-button id="start-sync-btn" variant="primary" style="width: 100%;" disabled>
                    <sl-icon slot="prefix" name="box-arrow-in-down"></sl-icon>
                    ${t('basket.actions.start_sync')}
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
                <div class="basket-header"><h2>${t('basket.sync.starting')}</h2></div>
                <div class="sync-progress-panel" aria-live="polite" aria-label="${t('basket.sync.progress')}">
                    <sl-spinner style="font-size: 2rem;"></sl-spinner>
                </div>
                <div class="basket-footer">
                    <sl-button id="cancel-sync-btn" variant="default" style="width: 100%;"
                               ${this.isCancelling ? 'loading disabled' : ''}>
                        <sl-icon slot="prefix" name="x-circle"></sl-icon>
                        ${this.isCancelling ? t('basket.sync.cancelling') : t('basket.actions.cancel_sync')}
                    </sl-button>
                </div>
            `;
            this.container.querySelector('#cancel-sync-btn')?.addEventListener('click', () => {
                this.handleCancelSync();
            });
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
                    <h2>${t('basket.title')}</h2>
                    <sl-badge variant="neutral" pill>0</sl-badge>
                </div>
                <div class="basket-placeholder">
                    <sl-icon name="basket" style="font-size: 2rem; opacity: 0.5;"></sl-icon>
                    <p style="opacity: 0.5;">${t('basket.empty')}</p>
                </div>
                <div class="basket-footer">
                    ${this.renderAutoFillControls()}
                    ${this.renderStatusZone()}
                    ${this.renderDeviceHub()}
                    ${this.renderDeviceFolders()}
                </div>
                <div class="basket-actions">
                    <sl-button id="start-sync-btn" variant="primary" style="width: 100%;" ${(!basketStore.isDirty() && !this.autoFillEnabled && !(this.currentDevice?.synced_items?.length > 0)) || !this.selectedDevicePath ? 'disabled' : ''}>
                        <sl-icon slot="prefix" name="box-arrow-in-down"></sl-icon>
                        ${t('basket.actions.start_sync')}
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
                <h2>${t('basket.title')}</h2>
                <sl-badge variant="primary" pill>${items.length}</sl-badge>
            </div>

            <div class="basket-items-list">
                ${items.map(item => this.renderItem(item)).join('')}
            </div>

            <div class="basket-footer">
                <div class="basket-summary">
                    <span>${t('basket.summary.tracks_size', { count: totalTracks, size: formatSize(totalSizeBytes) })}</span>
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
                        ${t('basket.actions.remove_to_fit', { size: formatSize(overAmount) })}
                    </sl-button>
                ` : this.isDirtyManifest ? `
                    <sl-button variant="warning" style="width: 100%;" disabled>
                        <sl-icon slot="prefix" name="exclamation-triangle"></sl-icon>
                        ${t('basket.actions.repair_manifest_first')}
                    </sl-button>
                ` : `
                    <sl-button-group style="width: 100%;">
                        <sl-button id="start-sync-btn" variant="primary" style="flex: 1;"
                                   ${!this.selectedDevicePath ? 'disabled' : this.isSyncing ? 'loading disabled' : ''}>
                            <sl-icon slot="prefix" name="box-arrow-in-down"></sl-icon>
                            ${this.isSyncing ? t('basket.sync.syncing') : t('basket.actions.start_sync')}
                        </sl-button>
                        <sl-dropdown id="sync-mode-dropdown" placement="bottom-end" ${!this.selectedDevicePath || this.isSyncing ? 'disabled' : ''}>
                            <sl-button slot="trigger" variant="primary" caret ${!this.selectedDevicePath || this.isSyncing ? 'disabled' : ''}></sl-button>
                            <sl-menu>
                                <sl-menu-item id="force-sync-item">
                                    <sl-icon slot="prefix" name="arrow-repeat"></sl-icon>
                                    ${t('basket.actions.force_sync')}
                                </sl-menu-item>
                            </sl-menu>
                        </sl-dropdown>
                    </sl-button-group>
                `}
                <sl-button variant="text" size="small" class="clear-basket-btn" style="width: 100%;">
                    ${t('basket.actions.clear_all')}
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
            this.confirmClearAll();
        });

        this.container.querySelector('#start-sync-btn')?.addEventListener('click', () => {
            this.handleStartSync();
        });

        this.container.querySelector('#force-sync-item')?.addEventListener('click', () => {
            this.forceSyncMode = true;
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
        modal.open(this.pendingDeviceFriendlyName);
    }

    private async handleStartSync() {
        if (this.isSyncing) return;

        // Disable the button immediately so a second click can't slip through
        // while the async daemon-state check or delta calculation is in flight.
        this.isSyncing = true;
        this.isCancelling = false;
        this.showSyncComplete = false;
        this.syncErrorMessages = null;
        this.currentOperation = null;
        this.currentOperationId = null;
        this.etaText = t('basket.sync.calculating');
        this.render();

        // Check daemon for a sync started outside this window (e.g. auto-sync on connect).
        // If one is already running, attach to it instead of starting a new one.
        try {
            const daemonState = await rpcCall('get_daemon_state') as any;
            const activeOpId = daemonState?.activeOperationId as string | null;
            if (activeOpId) {
                this.currentOperationId = activeOpId;
                this.startPolling();
                return;
            }
        } catch {
            // Ignore — if daemon state can't be fetched, let the sync attempt proceed and
            // the server-side guard will reject it if a concurrent sync is truly running.
        }

        const currentItems = basketStore.getItems();

        // Detect and extract the auto-fill slot
        const autoFillSlot = currentItems.find(i => i.id === AUTO_FILL_SLOT_ID);
        const manualIds = currentItems.filter(i => i.id !== AUTO_FILL_SLOT_ID).map(i => i.id);

        // Take snapshot for race-safe dirty reset (exclude virtual slot — it won't appear in manifest)
        this.syncSnapshotIds = [...manualIds].sort();

        // Build delta request params
        const syncItemIds = await this.itemIdsWithIncrementalChanges(manualIds);
        const deltaParams: Record<string, unknown> = {
            itemIds: syncItemIds,
            basketItems: currentItems.filter(i => i.id !== AUTO_FILL_SLOT_ID),
        };
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
                excludeItemIds: syncItemIds,
            };
        }

        try {

            const delta = await rpcCall('sync_calculate_delta', deltaParams);
            const rawCleanupCount = (delta as any)?.destructiveCleanupCount;
            const deleteCount = typeof rawCleanupCount === 'number'
                ? rawCleanupCount
                : Array.isArray((delta as any)?.deletes) ? (delta as any).deletes.length : 0;
            const rawThreshold = (delta as any)?.destructiveCleanupThreshold;
            const destructiveThreshold = typeof rawThreshold === 'number' ? rawThreshold : Number.POSITIVE_INFINITY;
            const changeReasons = this.changeReasonSummary(delta);
            const confirmDestructiveCleanup = deleteCount > destructiveThreshold
                ? await this.confirmDestructiveCleanup(deleteCount, changeReasons)
                : false;
            if (deleteCount > destructiveThreshold && !confirmDestructiveCleanup) {
                this.stopPolling();
                this.isSyncing = false;
                this.currentOperationId = null;
                this.currentOperation = null;
                this.etaText = '';
                this.render();
                return;
            }
            const force = this.forceSyncMode;
            this.forceSyncMode = false;
            const result = await rpcCall('sync_execute', { delta, confirmDestructiveCleanup, force });
            this.currentOperationId = result.operationId as string;

            this.startPolling();
        } catch (err) {
            this.stopPolling();
            this.isSyncing = false;
            this.currentOperationId = null;
            this.currentOperation = null;
            this.showError(t('basket.sync.failed_to_start', { message: (err as Error).message }));
        }
    }

    private changeReasonSummary(delta: unknown): Array<{ reason: string; count: number }> {
        const raw = (delta as any)?.changeReasons;
        if (!Array.isArray(raw)) return [];
        return raw
            .map((entry) => ({
                reason: typeof entry?.reason === 'string' ? entry.reason : '',
                count: typeof entry?.count === 'number' ? entry.count : 0,
            }))
            .filter((entry) => entry.reason && entry.count > 0);
    }

    private confirmDestructiveCleanup(
        count: number,
        reasons: Array<{ reason: string; count: number }> = [],
    ): Promise<boolean> {
        return new Promise((resolve) => {
            const dialog = document.createElement('sl-dialog') as any;
            const reasonList = reasons.length > 0
                ? `<p><strong>${t('basket.confirm.reason_summary')}</strong></p>
                <ul class="cleanup-reasons">${reasons.map((entry) => `
                    <li><strong>${entry.count}</strong> ${this.escapeHtml(entry.reason)}</li>
                `).join('')}</ul>`
                : '';
            dialog.innerHTML = `
                <p>${t('basket.confirm.remove_managed_files', { count })}</p>
                ${reasonList}
                <sl-button slot="footer" variant="default" id="cleanup-cancel">${t('basket.actions.cancel')}</sl-button>
                <sl-button slot="footer" variant="danger" id="cleanup-confirm">${t('basket.actions.start_sync')}</sl-button>
            `;
            document.body.appendChild(dialog);
            let confirmed = false;
            dialog.querySelector('#cleanup-cancel')?.addEventListener('click', () => dialog.hide());
            dialog.querySelector('#cleanup-confirm')?.addEventListener('click', () => {
                confirmed = true;
                dialog.hide();
            });
            dialog.addEventListener('sl-after-hide', () => {
                dialog.remove();
                resolve(confirmed);
            }, { once: true });
            customElements.whenDefined('sl-dialog').then(() => dialog.show());
        });
    }

    private isSubsonicServer(): boolean {
        return this.serverType === 'subsonic' || this.serverType === 'openSubsonic';
    }

    private syncTokenStorageKey(): string | null {
        return this.lastHydratedDeviceId
            ? `hifimule-subsonic-sync-token:${this.lastHydratedDeviceId}`
            : null;
    }

    private async itemIdsWithIncrementalChanges(manualIds: string[]): Promise<string[]> {
        if (!this.isSubsonicServer()) return manualIds;
        const key = this.syncTokenStorageKey();
        if (!key) return manualIds;
        const syncToken = localStorage.getItem(key);
        if (!syncToken) return manualIds;

        const changes = await rpcCall('sync_detect_changes', { syncToken }) as Array<{
            id?: string;
            itemType?: string;
            changeType?: string;
        }>;
        const merged = new Set(manualIds);
        for (const change of changes) {
            if (
                change.itemType === 'song'
                && (change.changeType === 'created' || change.changeType === 'updated')
                && typeof change.id === 'string'
                && change.id.length > 0
            ) {
                merged.add(change.id);
            }
        }
        return Array.from(merged);
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
                } else if (op.status === 'cancelled' || this.isCancelling) {
                    this.stopPolling();
                    this.handleSyncCancelled();
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
        if (op.totalBytes <= 0 || op.bytesTransferred <= 0) return t('basket.sync.calculating');

        const elapsedSeconds = (Date.now() - new Date(op.startedAt).getTime()) / 1000;
        if (elapsedSeconds <= 0 || isNaN(elapsedSeconds)) return t('basket.sync.calculating');

        const totalRate = op.bytesTransferred / elapsedSeconds;
        if (totalRate <= 0) return t('basket.sync.calculating');

        const remaining = Math.max(0, op.totalBytes - op.bytesTransferred);
        if (remaining <= 0) return t('basket.sync.almost_done');

        const etaSeconds = remaining / totalRate;

        if (etaSeconds < 10) return t('basket.sync.almost_done');
        if (etaSeconds < 60) return t('basket.sync.seconds_left', { count: Math.round(etaSeconds) });
        return t('basket.sync.minutes_left', { count: Math.round(etaSeconds / 60) });
    }

    private renderSyncProgress() {
        if (!this.currentOperation || this.isDestroyed) return;

        const op = this.currentOperation;
        const pct = op.filesTotal > 0
            ? Math.round((op.filesCompleted / op.filesTotal) * 100)
            : 0;
        const currentFileName = op.currentFile
            ? getBasename(op.currentFile)
            : t('basket.sync.preparing');

        this.etaText = this.computeEta(op);

        this.container.innerHTML = `
            <div class="basket-header">
                <h2>${t('basket.sync.syncing_title')}</h2>
                <sl-badge variant="primary" pill>${op.filesCompleted}/${op.filesTotal}</sl-badge>
            </div>
            <div class="sync-progress-panel" aria-live="polite" aria-label="${t('basket.sync.progress')}">
                <sl-progress-bar value="${pct}" style="width: 100%; margin-bottom: 0.75rem;"
                    label="${t('basket.sync.progress_percent', { pct })}"></sl-progress-bar>
                <div class="sync-current-file">
                    <sl-icon name="arrow-down-circle" style="color: var(--sl-color-primary-600);"></sl-icon>
                    <span title="${this.escapeHtml(op.currentFile || '')}">${this.escapeHtml(currentFileName)}</span>
                </div>
                <div class="sync-file-counter">${t('basket.sync.file_counter', { completed: op.filesCompleted, total: op.filesTotal })}</div>
                <div class="sync-eta">${this.escapeHtml(this.etaText)}</div>
            </div>
            <div class="basket-footer">
                <sl-button id="cancel-sync-btn" variant="default" style="width: 100%;"
                           ${this.isCancelling ? 'loading disabled' : ''}>
                    <sl-icon slot="prefix" name="x-circle"></sl-icon>
                    ${this.isCancelling ? t('basket.sync.cancelling') : t('basket.actions.cancel_sync')}
                </sl-button>
            </div>
        `;

        this.container.querySelector('#cancel-sync-btn')?.addEventListener('click', () => {
            this.handleCancelSync();
        });
    }

    private renderSyncComplete() {
        const summary = this.completedFilesCount > 0
            ? t('basket.sync.complete_summary', {
                files: this.completedFilesCount,
                size: formatSize(this.completedBytesCount),
              })
            : '';
        this.container.innerHTML = `
            <div class="basket-header">
                <h2>${t('basket.title')}</h2>
                <sl-badge variant="neutral" pill>0</sl-badge>
            </div>
            <div class="sync-success-panel">
                <sl-icon name="check-circle-fill"
                    style="font-size: 2.5rem; color: var(--sl-color-success-600);"></sl-icon>
                <p class="sync-status-label">${t('basket.sync.complete')}</p>
                ${summary ? `<p class="sync-summary-label">${this.escapeHtml(summary)}</p>` : ''}
            </div>
            <div class="basket-footer">
                <sl-button id="sync-done-btn" variant="primary" style="width: 100%;">
                    <sl-icon slot="prefix" name="check"></sl-icon>
                    ${t('basket.actions.done')}
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
            : `<li>${t('basket.sync.failed_retry')}</li>`;

        this.container.innerHTML = `
            <div class="basket-header">
                <h2>${t('basket.title')}</h2>
            </div>
            <div class="sync-error-panel">
                <sl-icon name="exclamation-triangle-fill"
                    style="font-size: 2.5rem; color: var(--sl-color-danger-500);"></sl-icon>
                <p class="sync-status-label">${t('basket.sync.failed')}</p>
                <ul class="sync-error-list">${errorList}</ul>
            </div>
            <div class="basket-footer">
                <sl-button id="sync-retry-btn" variant="primary" style="width: 100%; margin-bottom: 0.5rem;">
                    <sl-icon slot="prefix" name="arrow-repeat"></sl-icon>
                    ${t('basket.actions.retry_sync')}
                </sl-button>
                <sl-button id="sync-dismiss-btn" variant="text" style="width: 100%;">
                    ${t('basket.actions.dismiss')}
                </sl-button>
            </div>
        `;

        this.container.querySelector('#sync-retry-btn')?.addEventListener('click', () => {
            this.syncErrorMessages = null;
            this.handleStartSync();
        });
        this.container.querySelector('#sync-dismiss-btn')?.addEventListener('click', () => {
            this.syncErrorMessages = null;
            this.refreshAndRender();
        });
    }

    private async handleSyncComplete() {
        if (this.isDestroyed) return;
        this.isSyncing = false;

        // Capture transfer stats before clearing the operation reference
        this.completedFilesCount = this.currentOperation?.filesCompleted ?? 0;
        this.completedBytesCount = this.currentOperation?.bytesTransferred ?? 0;

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
        this.syncPreviewCleanupDevicePath = null;
        this.etaText = t('basket.sync.calculating');
        const tokenKey = this.syncTokenStorageKey();
        if (this.isSubsonicServer() && tokenKey) {
            localStorage.setItem(tokenKey, Date.now().toString());
        }

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
        this.isCancelling = false;
        this.currentOperationId = null;
        this.currentOperation = null;
        this.showSyncComplete = false;
        this.etaText = t('basket.sync.calculating');
        this.syncErrorMessages = operation.errors.length > 0
            ? operation.errors.map(e => {
                const target = e.filename || e.jellyfinId || t('basket.sync.unknown_file');
                const message = e.errorMessage || t('basket.sync.unknown_file_error');
                return `${target}: ${message}`;
            })
            : [t('basket.sync.failed_retry')];
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

    private itemTypeLabel(type: string): string {
        if (type === 'MusicAlbum' || type === 'FavoriteAlbum') return t('basket.item.type.album');
        if (type === 'Playlist') return t('basket.item.type.playlist');
        if (type === 'FavoriteArtist') return t('basket.item.type.favorites');
        return type;
    }

    private renderPriorityLabel(reason: string): string {
        if (reason === 'favorite') return t('basket.priority.favorite');
        if (reason.startsWith('playCount:')) {
            const count = reason.split(':')[1];
            return t('basket.priority.plays', { count });
        }
        if (reason === 'new' || reason === '') return t('basket.priority.new');
        return t('basket.priority.auto');
    }

    private renderAutoFillSlotCard(item: BasketItem): string {
        return `
            <div class="basket-item-card basket-item-auto-fill-slot" data-id="${AUTO_FILL_SLOT_ID}">
                <div class="basket-item-auto-fill-icon">
                    <sl-icon name="stars"></sl-icon>
                </div>
                <div class="basket-item-info">
                    <div class="basket-item-name">${t('basket.autofill.slot')}</div>
                    <div class="basket-item-meta">
                        ${t('basket.autofill.slot_meta', { size: formatSize(item.sizeBytes) })}
                    </div>
                </div>
                <sl-icon-button name="x" class="remove-item-btn" data-id="${AUTO_FILL_SLOT_ID}" label="${t('basket.actions.remove')}"></sl-icon-button>
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
                        ${t('basket.item.artist_meta', { count: item.childCount ?? 0, size: formatSize(item.sizeBytes ?? 0) })}
                    </div>
                </div>
                <sl-icon-button name="x" class="remove-item-btn" data-id="${this.escapeHtml(item.id)}" label="${t('basket.actions.remove')}"></sl-icon-button>
            </div>
        `;
    }

    private renderGenreCard(item: BasketItem): string {
        return `
            <div class="basket-item-card basket-item-genre" data-id="${this.escapeHtml(item.id)}">
                <div class="basket-item-genre-icon">
                    <sl-icon name="music-note-beamed"></sl-icon>
                </div>
                <div class="basket-item-info">
                    <div class="basket-item-name">${this.escapeHtml(item.name)}</div>
                    <div class="basket-item-meta">
                        ${t('basket.item.genre_meta', { count: item.childCount ?? 0, size: formatSize(item.sizeBytes ?? 0) })}
                    </div>
                </div>
                <sl-icon-button name="x" class="remove-item-btn" data-id="${this.escapeHtml(item.id)}" label="${t('basket.actions.remove')}"></sl-icon-button>
            </div>
        `;
    }

    private renderItem(item: BasketItem): string {
        if (item.id === AUTO_FILL_SLOT_ID) {
            return this.renderAutoFillSlotCard(item);
        }
        if (item.type === 'MusicGenre') {
            return this.renderGenreCard(item);
        }
        if (item.type === 'MusicArtist') {
            return this.renderArtistCard(item);
        }
        const autoBadge = item.autoFilled
            ? `<span class="basket-item-auto-badge" title="${t('basket.autofill.added_by')}">${t('basket.priority.auto')}</span>`
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
                        ${t('basket.item.meta', { label: this.itemTypeLabel(item.type), count: item.childCount ?? 0, size: formatSize(item.sizeBytes ?? 0) })}
                        ${priorityLabel}
                    </div>
                </div>
                <sl-icon-button name="x" class="remove-item-btn" data-id="${item.id}" label="${t('basket.actions.remove')}"></sl-icon-button>
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

    private async handleCancelSync(): Promise<void> {
        if (this.isCancelling) return;
        this.isCancelling = true;
        this.renderSyncProgress(); // show Cancelling... state immediately
        if (this.currentOperationId) {
            try {
                await rpcCall('sync_cancel', { operationId: this.currentOperationId });
            } catch (err) {
                console.error('[Sync] Cancel request failed:', err);
            }
        }
        // The polling loop will detect the terminal status and call handleSyncCancelled
    }

    private handleSyncCancelled(): void {
        if (this.isDestroyed) return;
        this.isSyncing = false;
        this.isCancelling = false;
        this.currentOperationId = null;
        this.currentOperation = null;
        this.showSyncComplete = false;
        this.syncErrorMessages = null;
        this.etaText = t('basket.sync.calculating');
        this.refreshAndRender();
    }

    private confirmClearAll(): void {
        const count = basketStore.getItems().length;
        if (count === 0) return;
        const dialog = document.createElement('sl-dialog') as any;
        dialog.label = t('basket.actions.clear_all');
        dialog.innerHTML = `
            <p>${t('basket.confirm.clear_all', { count })}</p>
            <sl-button slot="footer" variant="default" id="clear-cancel">${t('basket.actions.cancel')}</sl-button>
            <sl-button slot="footer" variant="danger" id="clear-confirm">${t('basket.actions.clear_all')}</sl-button>
        `;
        document.body.appendChild(dialog);
        dialog.querySelector('#clear-cancel')?.addEventListener('click', () => dialog.hide());
        dialog.querySelector('#clear-confirm')?.addEventListener('click', () => {
            basketStore.clear();
            dialog.hide();
        });
        dialog.addEventListener('sl-after-hide', (event: Event) => {
            if (event.target === dialog) dialog.remove();
        });
        customElements.whenDefined('sl-dialog').then(() => dialog.show());
    }

    private escapeHtml(text: string): string {
        const div = document.createElement('div');
        div.textContent = text;
        return div.innerHTML.replace(/"/g, '&quot;');
    }

    private iconLabel(icon: string): string {
        const labels: Record<string, string> = {
            'usb-drive': t('basket.icon.usb_drive'),
            'phone-fill': t('basket.icon.phone'),
            'watch': t('basket.icon.watch'),
            'sd-card': t('basket.icon.sd_card'),
            'headphones': t('basket.icon.headphones'),
            'music-note-list': t('basket.icon.music_player'),
        };
        return labels[icon] ?? icon;
    }
}
