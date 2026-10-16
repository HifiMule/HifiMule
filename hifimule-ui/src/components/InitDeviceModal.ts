// Initialize Device Modal Component
// Guides the user through initializing a new unrecognized device for sync.
// Follows the RepairModal.ts pattern: sl-dialog, class-based, open() method, onComplete callback.

import { rpcCall } from '../rpc';
import { t } from '../i18n';
import catalog from '@hifimule/i18n-catalog';
import { basketStore } from '../state/basket';

interface DeviceProfileSummary {
    id: string;
    name: string;
    description?: string;
    defaultMusicFolder?: string | null;
    defaultPlaylistFolder?: string | null;
}

export class InitDeviceModal {
    private dialog: HTMLElement | null = null;
    private onComplete: (() => void) | null = null;
    private _defaultName: string | undefined = undefined;
    private pendingId: string | undefined;
    private observedDestinationRevision: string | undefined;

    constructor(_container: HTMLElement, onComplete?: () => void) {
        this.onComplete = onComplete || null;
    }

    async open(defaultName?: string, pendingId?: string, observedDestinationRevision?: string) {
        this._defaultName = defaultName;
        this.pendingId = pendingId;
        this.observedDestinationRevision = observedDestinationRevision;
        if (!this.pendingId || !this.observedDestinationRevision) {
            const state = await rpcCall('get_daemon_state');
            const selected = state?.destinations?.find((destination: any) => destination.kind === 'pendingDevice' && destination.selected)
                ?? state?.destinations?.find((destination: any) => destination.kind === 'pendingDevice');
            this.pendingId = selected?.pendingId;
            this.observedDestinationRevision = state?.destinationRevision;
        }
        this.renderDialog();
        await this.showDialog();
        await this.loadCredentials();
    }

    private async showDialog() {
        if (this.dialog) {
            await customElements.whenDefined('sl-dialog');
            await (this.dialog as any).updateComplete;
            (this.dialog as any).show();
        }
    }

    private renderDialog() {
        // Remove any existing dialog first
        document.body.querySelector('#init-device-dialog')?.remove();

        const dialogEl = document.createElement('sl-dialog');
        dialogEl.id = 'init-device-dialog';
        dialogEl.setAttribute('label', t('device.init.title'));
        dialogEl.innerHTML = `
            <div class="init-device-body">
                <div class="init-device-loading">
                    <sl-spinner style="font-size: 2rem;"></sl-spinner>
                    <p>${t('device.init.loading')}</p>
                </div>
            </div>
            <sl-button slot="footer" id="init-device-cancel-btn" variant="default">${t('basket.actions.cancel')}</sl-button>
            <sl-button slot="footer" id="init-device-confirm-btn" variant="primary" disabled>
                <sl-icon slot="prefix" name="check2-circle"></sl-icon>
                ${t('device.init.confirm')}
            </sl-button>
        `;

        document.body.appendChild(dialogEl);
        this.dialog = dialogEl;
        
        dialogEl.addEventListener('sl-after-hide', (e: Event) => {
            if (e.target === dialogEl) {
                dialogEl.remove();
            }
        });

        dialogEl.querySelector('#init-device-cancel-btn')?.addEventListener('click', () => {
            (dialogEl as any).hide();
        });
    }

    private async loadCredentials() {
        const body = this.dialog?.querySelector('.init-device-body');
        if (!body) return;

        try {
            // Use allSettled so a profiles RPC failure doesn't block the modal.
            const [credsResult, profilesResult] = await Promise.allSettled([
                rpcCall('get_credentials') as Promise<any>,
                rpcCall('device_profiles.list') as Promise<any>,
            ]);

            const creds = credsResult.status === 'fulfilled' ? credsResult.value : null;
            const profiles = profilesResult.status === 'fulfilled'
                ? profilesResult.value
                : [{ id: 'passthrough', name: t('basket.profile.no_transcoding'), description: t('basket.profile.no_transcoding_desc') }];

            const userId = creds?.userId || null;
            this.renderContent(body as HTMLElement, userId, profiles);
        } catch (err) {
            // Tauri invoke rejects with a plain string, not an Error object
            const message = typeof err === 'string' ? err : (err instanceof Error ? err.message : String(err));
            this.renderError(body as HTMLElement, message || t('device.init.unknown_error'));
        }
    }

    private renderContent(body: HTMLElement, userId: string | null, profiles: DeviceProfileSummary[]) {
        profiles = (profiles || []).map(profile => this.localizeProfile(profile));
        if (!userId) {
            body.innerHTML = `
                <div class="init-device-no-login">
                    <sl-icon name="person-x" style="font-size: 2.5rem; opacity: 0.6;"></sl-icon>
                    <p>${t('device.init.login_required')}</p>
                </div>
            `;
            return;
        }

        const profileOptions = (profiles || []).map(p => 
            `<sl-option value="${this.escapeHtml(p.id)}">${this.escapeHtml(p.name)}</sl-option>`
        ).join('');
        const defaultPassthroughDesc = (profiles || []).find(p => p.id === 'passthrough')?.description || '';
        const defaultPassthrough = (profiles || []).find(p => p.id === 'passthrough');

        let selectedIcon = 'usb-drive';

        body.innerHTML = `
            <div class="init-device-form">
                <p style="margin-bottom: 1rem; opacity: 0.8;">
                    ${t('device.init.intro')}
                </p>
                <div style="margin-bottom: 1.25rem;">
                    <label style="font-size: 0.8rem; opacity: 0.7; display: block; margin-bottom: 0.25rem;">
                        ${t('device.init.name')} <span style="color: var(--sl-color-danger-500);">*</span>
                    </label>
                    <sl-input
                        id="init-device-name-input"
                        placeholder="${this.escapeHtml(t('device.init.default_name'))}"
                        maxlength="40"
                        value="${this.escapeHtml(this._defaultName ?? t('device.init.default_name'))}"
                        clearable
                    ></sl-input>
                    <div style="font-size: 0.75rem; opacity: 0.55; margin-top: 0.3rem;">
                        ${t('device.init.name_hint')}
                    </div>
                </div>
                <div style="margin-bottom: 1.25rem;">
                    <label style="font-size: 0.8rem; opacity: 0.7; display: block; margin-bottom: 0.5rem;">
                        ${t('device.init.icon')}
                    </label>
                    <div id="init-icon-picker" style="display: flex; flex-wrap: wrap; gap: 0.5rem;">
                        ${['usb-drive', 'phone-fill', 'watch', 'sd-card', 'headphones', 'music-note-list'].map(icon => `
                            <div class="init-icon-tile ${icon === 'usb-drive' ? 'selected' : ''}"
                                 data-icon="${icon}"
                                 style="display: flex; flex-direction: column; align-items: center; gap: 0.3rem;
                                        padding: 0.5rem 0.7rem; border-radius: var(--sl-border-radius-medium);
                                        border: 2px solid ${icon === 'usb-drive' ? 'var(--sl-color-primary-500)' : 'var(--sl-color-neutral-200)'};
                                        cursor: pointer; min-width: 52px; text-align: center;
                                        background: ${icon === 'usb-drive' ? 'var(--sl-color-primary-50)' : 'transparent'};
                                        transition: border-color 0.15s, background 0.15s;">
                                <sl-icon name="${icon}" style="font-size: 1.5rem;"></sl-icon>
                                <span style="font-size: 0.65rem; opacity: 0.7;">${this.iconLabel(icon)}</span>
                            </div>
                        `).join('')}
                    </div>
                </div>
                <div style="margin-bottom: 1.25rem;">
                    <label style="font-size: 0.8rem; opacity: 0.7; display: block; margin-bottom: 0.25rem;">
                        ${t('basket.device.transcoding_profile')}
                    </label>
                    <sl-select id="init-transcoding-profile" value="passthrough">
                        ${profileOptions}
                    </sl-select>
                    <div style="font-size: 0.75rem; opacity: 0.55; margin-top: 0.3rem;" id="init-transcoding-desc">
                        ${this.escapeHtml(defaultPassthroughDesc)}
                    </div>
                </div>
                <div style="margin-bottom: 1.25rem;">
                    <label style="font-size: 0.8rem; opacity: 0.7; display: block; margin-bottom: 0.25rem;">
                        ${t('device.init.music_folder')}
                    </label>
                    <sl-input
                        id="init-folder-input"
                        placeholder="${this.escapeHtml(t('device.init.music_placeholder'))}"
                        value="${this.escapeHtml(defaultPassthrough?.defaultMusicFolder ?? '')}"
                        clearable
                    ></sl-input>
                    <div style="font-size: 0.75rem; opacity: 0.55; margin-top: 0.3rem;">
                        ${t('device.init.music_hint')}
                    </div>
                </div>
                <div style="margin-bottom: 1.25rem;">
                    <label style="font-size: 0.8rem; opacity: 0.7; display: block; margin-bottom: 0.25rem;">
                        ${t('device.init.playlist_folder')}
                    </label>
                    <sl-input
                        id="init-playlist-folder-input"
                        placeholder="${this.escapeHtml(t('device.init.playlist_placeholder'))}"
                        value="${this.escapeHtml(defaultPassthrough?.defaultPlaylistFolder ?? '')}"
                        clearable
                    ></sl-input>
                </div>
                <div style="margin-bottom: 1.25rem;">
                    <label style="font-size: 0.8rem; opacity: 0.7; display: block; margin-bottom: 0.25rem;">${t('basket.device.audiobook_folder')}</label>
                    <sl-input id="init-audiobook-folder-input" placeholder="${t('basket.device.media_folder_inherit')}" clearable></sl-input>
                </div>
                <div style="margin-bottom: 1.25rem;">
                    <label style="font-size: 0.8rem; opacity: 0.7; display: block; margin-bottom: 0.25rem;">${t('basket.device.podcast_folder')}</label>
                    <sl-input id="init-podcast-folder-input" placeholder="${t('basket.device.media_folder_inherit')}" clearable></sl-input>
                </div>
                <div style="padding: 0.75rem; background: rgba(255,255,255,0.04); border-radius: 6px; border: 1px solid rgba(255,255,255,0.08);">
                    <div style="font-size: 0.75rem; opacity: 0.55; margin-bottom: 0.25rem;">${t('device.init.linked_profile')}</div>
                    <div style="font-size: 0.85rem;">
                        <sl-icon name="person-fill" style="vertical-align: middle; margin-right: 0.3rem;"></sl-icon>
                        <span id="init-user-display">${this.escapeHtml(userId)}</span>
                    </div>
                </div>
            </div>
        `;

        // Update description when selection changes
        const selectEl = body.querySelector('#init-transcoding-profile') as any;
        const descEl = body.querySelector('#init-transcoding-desc') as HTMLElement;
        const musicFolderInput = body.querySelector('#init-folder-input') as any;
        const playlistFolderInput = body.querySelector('#init-playlist-folder-input') as any;
        let foldersEdited = false;
        musicFolderInput?.addEventListener('sl-input', () => { foldersEdited = true; });
        playlistFolderInput?.addEventListener('sl-input', () => { foldersEdited = true; });
        if (selectEl && descEl) {
            selectEl.addEventListener('sl-change', (e: any) => {
                const selectedId = e.target.value;
                const profile = (profiles || []).find(p => p.id === selectedId);
                if (profile) {
                    descEl.innerHTML = this.escapeHtml(profile.description ?? '');
                    if (!foldersEdited) {
                        musicFolderInput.value = profile.defaultMusicFolder ?? '';
                        playlistFolderInput.value = profile.defaultPlaylistFolder ?? '';
                    }
                }
            });
        }

        // Wire up icon tile click listeners
        body.querySelectorAll('.init-icon-tile').forEach(tile => {
            tile.addEventListener('click', () => {
                selectedIcon = (tile as HTMLElement).dataset.icon ?? 'usb-drive';
                body.querySelectorAll('.init-icon-tile').forEach(t => {
                    const el = t as HTMLElement;
                    const isSelected = el.dataset.icon === selectedIcon;
                    el.style.borderColor = isSelected ? 'var(--sl-color-primary-500)' : 'var(--sl-color-neutral-200)';
                    el.style.background = isSelected ? 'var(--sl-color-primary-50)' : 'transparent';
                });
            });
        });

        // Enable/disable confirm button based on name field value
        const nameInput = body.querySelector('#init-device-name-input') as any;
        const confirmBtn = this.dialog?.querySelector('#init-device-confirm-btn') as any;
        if (confirmBtn) {
            // Clone to drop any stale listeners from previous renders, then wire fresh ones
            const freshBtn = confirmBtn.cloneNode(true) as any;
            confirmBtn.replaceWith(freshBtn);
            freshBtn.disabled = !(nameInput?.value ?? this._defaultName ?? t('device.init.default_name')).trim();
            freshBtn.addEventListener('click', () => this.handleConfirm(userId, selectedIcon, nameInput));
            nameInput?.addEventListener('sl-input', () => {
                freshBtn.disabled = !nameInput.value?.trim();
            });
        }
    }

    private renderError(body: HTMLElement, message: string) {
        body.innerHTML = `
            <div class="init-device-error">
                <sl-icon name="exclamation-triangle-fill" style="font-size: 2.5rem; color: var(--sl-color-danger-500);"></sl-icon>
                <p>${this.escapeHtml(t('device.init.load_error', { message }))}</p>
                <sl-button id="init-retry-btn" variant="primary" size="small">${t('repair.retry')}</sl-button>
            </div>
        `;
        body.querySelector('#init-retry-btn')?.addEventListener('click', () => this.loadCredentials());
    }

    private renderSubmitting(body: HTMLElement) {
        body.innerHTML = `
            <div class="init-device-loading">
                <sl-spinner style="font-size: 2rem;"></sl-spinner>
                <p>${t('device.init.submitting')}</p>
            </div>
        `;
    }

    private renderInitError(body: HTMLElement, message: string) {
        body.innerHTML = `
            <div class="init-device-error">
                <sl-icon name="exclamation-triangle-fill" style="font-size: 2.5rem; color: var(--sl-color-danger-500);"></sl-icon>
                <sl-alert variant="danger" open>
                    <sl-icon slot="icon" name="exclamation-octagon"></sl-icon>
                    <strong>${t('device.init.failed')}</strong><br>
                    ${this.escapeHtml(message)}
                </sl-alert>
                <div style="display: flex; gap: 0.5rem; margin-top: 1rem; justify-content: flex-end;">
                    <sl-button id="init-retry-btn" variant="primary" size="small">${t('repair.retry')}</sl-button>
                    <sl-button id="init-dismiss-btn" variant="default" size="small">${t('basket.actions.dismiss')}</sl-button>
                </div>
            </div>
        `;
        body.querySelector('#init-retry-btn')?.addEventListener('click', () => this.loadCredentials());
        body.querySelector('#init-dismiss-btn')?.addEventListener('click', () => {
            if (this.dialog) (this.dialog as any).hide();
        });
    }

    private async handleConfirm(userId: string, selectedIcon: string, nameInputEl: any) {
        const body = this.dialog?.querySelector('.init-device-body') as HTMLElement | null;
        if (!body) return;

        const deviceName: string = nameInputEl?.value?.trim() ?? t('device.init.default_name');

        const folderInput = this.dialog?.querySelector('#init-folder-input') as any;
        const folderPath: string = folderInput?.value?.trim() ?? '';
        const playlistFolderInput = this.dialog?.querySelector('#init-playlist-folder-input') as any;
        const playlistFolderPath: string = playlistFolderInput?.value?.trim() ?? '';
        const audiobookFolderPath: string = (this.dialog?.querySelector('#init-audiobook-folder-input') as any)?.value?.trim() ?? '';
        const podcastFolderPath: string = (this.dialog?.querySelector('#init-podcast-folder-input') as any)?.value?.trim() ?? '';

        const profileSelect = this.dialog?.querySelector('#init-transcoding-profile') as any;
        let transcodingProfileId = profileSelect?.value;

        const confirmBtn = this.dialog?.querySelector('#init-device-confirm-btn') as any;
        if (confirmBtn) confirmBtn.loading = true;

        this.renderSubmitting(body);

        try {
            const initialized = await rpcCall('device_initialize', {
                pendingId: this.pendingId,
                observedDestinationRevision: this.observedDestinationRevision,
                folderPath,
                playlistFolderPath,
                audiobookFolderPath,
                podcastFolderPath,
                profileId: userId,
                transcodingProfileId,
                name: deviceName,
                icon: selectedIcon || null,
            });

            const scanWarning = initialized?.data?.musicScanWarning;
            if (scanWarning) {
                this.dialog?.querySelector('#init-device-confirm-btn')?.remove();
                body.textContent = `${t('device.scan.discoveryWarning')} ${scanWarning}`;
                this.onComplete?.();
                return;
            }
            const scanId = initialized?.data?.musicScanId;
            if (scanId) { this.showMusicScan(body, scanId, initialized.data.deviceId); return; }
            if (this.dialog) (this.dialog as any).hide();
            this.onComplete?.();
        } catch (err) {
            const message = typeof err === 'string' ? err : (err instanceof Error ? err.message : String(err));
            this.renderInitError(body, message);
        }
    }

    private showMusicScan(body: HTMLElement, scanId: string, deviceId: string) {
        this.dialog?.setAttribute('label', t('device.scan.title'));
        const finish = () => { if (this.dialog) (this.dialog as any).hide(); this.onComplete?.(); };
        // Remove obsolete initialization controls: component styles can override
        // the native hidden attribute, leaving their stale click handlers active.
        this.dialog?.querySelector('#init-device-confirm-btn')?.remove();
        this.dialog?.querySelector('#init-device-cancel-btn')?.remove();
        this.dialog?.addEventListener('sl-request-close', (event: Event) => event.preventDefault());
        const render = () => {
            body.innerHTML = `<p>${t('device.scan.prompt')}</p><p style="color:var(--ink-dim)">${t('device.scan.ownership')}</p>
                <div style="display:flex;gap:var(--spacing-sm,8px);justify-content:flex-end">
                <sl-button id="scan-no">${t('device.scan.skip')}</sl-button><sl-button id="scan-yes" variant="primary">${t('device.scan.start')}</sl-button></div>`;
            body.querySelector('#scan-no')?.addEventListener('click', async () => { try { await rpcCall('device.music_scan_cancel', { scanId }); } catch {} finally { finish(); } });
            body.querySelector('#scan-yes')?.addEventListener('click', async () => {
                body.innerHTML = `<sl-spinner></sl-spinner><p id="scan-progress">${t('device.scan.scanning')}</p><sl-button id="scan-cancel">${t('device.scan.cancel')}</sl-button>`;
                let cancellation: Promise<any> | undefined;
                body.querySelector('#scan-cancel')?.addEventListener('click', async () => { try { cancellation = rpcCall('device.music_scan_cancel', {scanId}); await cancellation; } catch {} });
                let polling = false;
                const timer = window.setInterval(async () => {
                    if (polling) return; polling = true;
                    try { const status = await rpcCall('device.music_scan_status', {scanId}); const label = body.querySelector('#scan-progress'); if (label) label.textContent = t('device.scan.progress', {count:status.scanned,total:status.total}); } catch {} finally { polling = false; }
                }, 1000);
                try {
                    const result = await rpcCall('device.music_scan_start', {scanId});
                    body.innerHTML = `<p>${t('device.scan.result', { imported:result.imported, unmatched:result.unmatched, ambiguous:result.ambiguous, incompatible:result.incompatible })}</p><sl-button id="scan-done" variant="primary">${t('device.scan.done')}</sl-button>`;
                    const done = body.querySelector('#scan-done') as any;
                    done?.addEventListener('click', async () => {
                        if (done.loading) return;
                        done.loading = true;
                        done.disabled = true;
                        body.querySelector('#scan-refresh-error')?.remove();
                        try {
                            const basket = await rpcCall('manifest_get_basket');
                            // A different destination may have been selected while
                            // the modal was open. Never replace its basket with ours.
                            if (basket?.targetDeviceId === deviceId) {
                                basketStore.setDaemonContext(deviceId, basket.basketHash ?? null);
                                basketStore.hydrateFromDaemon(basket.basketItems ?? []);
                            }
                            finish();
                        } catch (error) {
                            const message = document.createElement('p');
                            message.id = 'scan-refresh-error';
                            message.setAttribute('role', 'alert');
                            message.textContent = String(error);
                            body.prepend(message);
                        } finally {
                            done.loading = false;
                            done.disabled = false;
                        }
                    });
                } catch (error) {
                    if (cancellation) { try { const outcome = await cancellation; if (outcome?.cancelled === true) { finish(); return; } } catch {} }
                    render(); const message = document.createElement('p'); message.textContent = String(error); body.prepend(message);
                } finally { window.clearInterval(timer); }
            });
        };
        render();
    }

    private localizeProfile(profile: DeviceProfileSummary): DeviceProfileSummary {
        // Translate stock text only: saved profiles can contain customer overrides.
        if (profile.id === 'passthrough') {
            return {
                ...profile,
                name: profile.name === 'No Transcoding (Download Original)' || profile.name === catalog.en['basket.profile.no_transcoding']
                    ? t('basket.profile.no_transcoding') : profile.name,
                description: profile.description === 'Download files as-is from Jellyfin without transcoding.' || profile.description === catalog.en['basket.profile.no_transcoding_desc']
                    ? t('basket.profile.no_transcoding_desc') : profile.description,
            };
        }
        const nameKey = `device.profile.${profile.id}.name`;
        const descriptionKey = `device.profile.${profile.id}.description`;
        return {
            ...profile,
            name: catalog.en[nameKey] && profile.name === catalog.en[nameKey] ? t(nameKey) : profile.name,
            description: catalog.en[descriptionKey] && profile.description === catalog.en[descriptionKey]
                ? t(descriptionKey) : profile.description,
        };
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

    private escapeHtml(text: string): string {
        const div = document.createElement('div');
        div.textContent = text;
        return div.innerHTML;
    }
}
