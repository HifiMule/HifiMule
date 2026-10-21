import { deviceIconLabel } from '../deviceIcon';
import { escapeAttr } from '../html';
// Initialize Device Modal Component
// Guides the user through initializing a new unrecognized device for sync.
// Follows the RepairModal.ts pattern: sl-dialog, class-based, open() method, onComplete callback.

import { rpcCall } from '../rpc';
import { t } from '../i18n';
import english from 'virtual:hifimule-locale/en';
import { basketStore } from '../state/basket';
import { withDeadline } from '../lifecycleDeadline';

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
    private credentialsLoadingBody: HTMLElement | null = null;

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
        const dialog = this.dialog;
        const body = dialog?.querySelector('.init-device-body') as HTMLElement | null;
        if (!body || this.credentialsLoadingBody === body) return;
        this.credentialsLoadingBody = body;
        const restoreFocus = body.contains(document.activeElement);
        const current = () => dialog === this.dialog && body.isConnected;
        const focusControl = async (selector: string) => {
            if (!restoreFocus) return;
            const control = body.querySelector(selector) as any;
            if (!control) return;
            await control.updateComplete;
            if (current()) control.focus();
        };
        const confirm = dialog?.querySelector('#init-device-confirm-btn') as any;
        if (confirm) confirm.disabled = true;
        body.setAttribute('aria-busy', 'true');
        body.innerHTML = `<div class="init-device-loading" role="status"><sl-spinner></sl-spinner><p>${t('device.init.loading')}</p></div>`;
        try {
            // Bound each read independently so profile failures can use the fallback.
            const [credsResult, profilesResult] = await Promise.allSettled([
                withDeadline(rpcCall('get_credentials'), 15_000, t('device.init.load_timeout')),
                withDeadline(rpcCall('device_profiles.list'), 15_000, t('device.init.load_timeout')),
            ]);
            if (!current()) return;
            if (credsResult.status === 'rejected') throw credsResult.reason;
            const profiles = profilesResult.status === 'fulfilled'
                ? profilesResult.value
                : [{ id: 'passthrough', name: t('basket.profile.no_transcoding'), description: t('basket.profile.no_transcoding_desc') }];
            this.renderContent(body, credsResult.value?.userId || null, profiles);
            await focusControl('sl-input');
        } catch (err) {
            if (!current()) return;
            const message = typeof err === 'string' ? err : (err instanceof Error ? err.message : '');
            if (confirm) confirm.disabled = true;
            this.renderError(body, message || t('device.init.unknown_error'));
            await focusControl('#init-retry-btn');
        } finally {
            if (this.credentialsLoadingBody === body) this.credentialsLoadingBody = null;
            body.removeAttribute('aria-busy');
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
            `<sl-option value="${escapeAttr(p.id)}">${escapeAttr(p.name)}</sl-option>`
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
                    <sl-input
                        id="init-device-name-input" label="${t('device.init.name')}" required
                        placeholder="${escapeAttr(t('device.init.default_name'))}"
                        help-text="${escapeAttr(t('device.init.name_hint'))}"
                        maxlength="40"
                        value="${escapeAttr(this._defaultName ?? t('device.init.default_name'))}"
                        clearable
                    ></sl-input>
                </div>
                <div style="margin-bottom: 1.25rem;">
                    <label style="font-size: 0.8rem; opacity: 0.7; display: block; margin-bottom: 0.5rem;">
                        ${t('device.init.icon')}
                    </label>
                    <div id="init-icon-picker" role="group" aria-label="${t('device.init.icon')}" style="display: flex; flex-wrap: wrap; gap: 0.5rem;">
                        ${['usb-drive', 'phone-fill', 'watch', 'sd-card', 'headphones', 'music-note-list'].map(icon => `
                            <button type="button" aria-pressed="${icon === 'usb-drive'}" class="init-icon-tile ${icon === 'usb-drive' ? 'selected' : ''}"
                                 data-icon="${icon}"
                                 style="display: flex; flex-direction: column; align-items: center; gap: 0.3rem;
                                        padding: 0.5rem 0.7rem; border-radius: var(--sl-border-radius-medium);
                                        border: 2px solid ${icon === 'usb-drive' ? 'var(--sl-color-primary-500)' : 'var(--sl-color-neutral-200)'};
                                        cursor: pointer; min-width: 52px; text-align: center; color: inherit; font: inherit;
                                        background: ${icon === 'usb-drive' ? 'var(--sl-color-primary-50)' : 'transparent'};
                                        transition: border-color 0.15s, background 0.15s;">
                                <sl-icon name="${icon}" style="font-size: 1.5rem;"></sl-icon>
                                <span style="font-size: 0.65rem; opacity: 0.7;">${deviceIconLabel(icon)}</span>
                            </button>
                        `).join('')}
                    </div>
                </div>
                <div style="margin-bottom: 1.25rem;">
                    <sl-select id="init-transcoding-profile" label="${t('basket.device.transcoding_profile')}" value="passthrough">
                        ${profileOptions}
                        <span slot="help-text" id="init-transcoding-desc">${escapeAttr(defaultPassthroughDesc)}</span>
                    </sl-select>
                </div>
                <div style="margin-bottom: 1.25rem;">
                    <sl-input
                        id="init-folder-input" label="${t('device.init.music_folder')}"
                        help-text="${escapeAttr(t('device.init.music_hint'))}"
                        placeholder="${escapeAttr(t('device.init.music_placeholder'))}"
                        value="${escapeAttr(defaultPassthrough?.defaultMusicFolder ?? '')}"
                        clearable
                    ></sl-input>
                </div>
                <div style="margin-bottom: 1.25rem;">
                    <sl-input
                        id="init-playlist-folder-input" label="${t('device.init.playlist_folder')}"
                        placeholder="${escapeAttr(t('device.init.playlist_placeholder'))}"
                        value="${escapeAttr(defaultPassthrough?.defaultPlaylistFolder ?? '')}"
                        clearable
                    ></sl-input>
                </div>
                <div style="margin-bottom: 1.25rem;">
                    <sl-input id="init-audiobook-folder-input" label="${t('basket.device.audiobook_folder')}" placeholder="${t('basket.device.media_folder_inherit')}" clearable></sl-input>
                </div>
                <div style="margin-bottom: 1.25rem;">
                    <sl-input id="init-podcast-folder-input" label="${t('basket.device.podcast_folder')}" placeholder="${t('basket.device.media_folder_inherit')}" clearable></sl-input>
                </div>
                <div style="margin-bottom: 1.25rem;">
                    <sl-checkbox id="init-device-reconcile" help-text="${escapeAttr(t('basket.device.reconcile_hint'))}">${t('basket.device.reconcile_before_sync')}</sl-checkbox>
                </div>
                <div style="margin-bottom: 1.25rem;">
                    <sl-checkbox id="init-device-artwork" help-text="${escapeAttr(t('basket.device.artwork_hint'))}">${t('basket.device.artwork')}</sl-checkbox>
                    <div id="init-device-artwork-additions" hidden style="margin-top: 0.75rem; padding-left: 1.5rem;">
                        ${(['windows', 'macos', 'kde', 'gnome'] as const).map(kind => `<sl-checkbox id="init-device-artwork-${kind}" style="display: block; margin-bottom: 0.75rem;" help-text="${escapeAttr(t(`basket.device.artwork_${kind}_hint`))}">${t(`basket.device.artwork_${kind}`)}</sl-checkbox>`).join('')}
                    </div>
                </div>
                <div style="padding: 0.75rem; background: rgba(255,255,255,0.04); border-radius: 6px; border: 1px solid rgba(255,255,255,0.08);">
                    <div style="font-size: 0.75rem; opacity: 0.55; margin-bottom: 0.25rem;">${t('device.init.linked_profile')}</div>
                    <div style="font-size: 0.85rem;">
                        <sl-icon name="person-fill" style="vertical-align: middle; margin-right: 0.3rem;"></sl-icon>
                        <span id="init-user-display">${escapeAttr(userId)}</span>
                    </div>
                </div>
            </div>
        `;

        const artwork = body.querySelector('#init-device-artwork') as any;
        const artworkAdditions = body.querySelector('#init-device-artwork-additions') as HTMLElement;
        artwork?.addEventListener('sl-change', () => {
            artworkAdditions.hidden = !artwork.checked;
        });

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
                    descEl.innerHTML = escapeAttr(profile.description ?? '');
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
                    el.setAttribute('aria-pressed', String(isSelected));
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
                <p role="alert">${escapeAttr(t('device.init.load_error', { message }))}</p>
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
                    ${escapeAttr(message)}
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

        const reconcileBeforeSync = (body.querySelector('#init-device-reconcile') as any)?.checked === true;
        const folderArtwork = Object.fromEntries(['enabled', 'windows', 'macos', 'kde', 'gnome'].map(kind => [
            kind,
            (body.querySelector(kind === 'enabled' ? '#init-device-artwork' : `#init-device-artwork-${kind}`) as any)?.checked === true,
        ]));

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
                reconcileBeforeSync,
                folderArtwork,
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
        const dialog = this.dialog;
        if (!dialog) return;
        dialog.setAttribute('label', t('device.scan.title'));
        dialog.querySelector('#init-device-confirm-btn')?.remove();
        dialog.querySelector('#init-device-cancel-btn')?.remove();
        dialog.addEventListener('sl-request-close', (event: Event) => event.preventDefault());
        // Keep announcement nodes mounted across every state transition.
        body.innerHTML = `<p id="scan-announcement" class="sr-only" role="status" aria-live="polite" aria-atomic="true"></p>
            <p id="scan-error" role="alert" aria-atomic="true" style="overflow-wrap:anywhere"></p>
            <div id="scan-content"></div>`;
        const announcement = body.querySelector('#scan-announcement') as HTMLElement;
        const error = body.querySelector('#scan-error') as HTMLElement;
        const content = body.querySelector('#scan-content') as HTMLElement;
        let revision = 0;
        let closed = false;
        let timer: ReturnType<typeof setInterval> | undefined;
        const current = () => !closed && body.isConnected && this.dialog === dialog;
        const stopPolling = () => { if (timer !== undefined) clearInterval(timer); timer = undefined; };
        dialog.addEventListener('sl-after-hide', stopPolling, { once: true });
        const finish = () => { stopPolling(); if (current()) { closed = true; (dialog as any).hide(); this.onComplete?.(); } };
        const focus = async (selector: string) => {
            const target = content.querySelector(selector) as any;
            await target?.updateComplete;
            if (current() && target?.isConnected) target.focus();
        };
        const showError = (key: string, detail: unknown) => {
            error.textContent = `${t(key)} ${detail instanceof Error ? detail.message : String(detail)}`;
        };
        const render = () => {
            const view = ++revision;
            content.innerHTML = `<p>${t('device.scan.prompt')}</p><p style="color:var(--ink-dim)">${t('device.scan.ownership')}</p>
                <div style="display:flex;flex-wrap:wrap;gap:var(--space-xs);justify-content:flex-end">
                <sl-button id="scan-dismiss" style="display:none">${t('basket.actions.dismiss')}</sl-button><sl-button id="scan-no">${t('device.scan.skip')}</sl-button><sl-button id="scan-yes" variant="primary">${t('device.scan.start')}</sl-button></div>`;
            void focus('#scan-yes');
            const dismiss = content.querySelector('#scan-dismiss') as HTMLElement;
            dismiss.addEventListener('click', finish);
            const skip = content.querySelector('#scan-no') as any;
            const start = content.querySelector('#scan-yes') as any;
            skip.addEventListener('click', async () => {
                if (skip.disabled) return;
                skip.disabled = start.disabled = true;
                error.textContent = '';
                announcement.textContent = t('device.scan.cancelling');
                try {
                    const outcome = await withDeadline(rpcCall('device.music_scan_cancel', { scanId }), 15_000, t('device.scan.requestTimeout'));
                    if (!current() || view !== revision) return;
                    if (outcome?.cancelled !== true) throw new Error(t('device.scan.cancelNotConfirmed'));
                    finish();
                } catch (cause) {
                    if (!current() || view !== revision) return;
                    announcement.textContent = '';
                    showError('device.scan.cancelFailed', cause);
                    skip.disabled = start.disabled = false;
                    // An expired scan or disconnected device must not trap the dialog.
                    dismiss.style.display = '';
                    void focus('#scan-no');
                }
            });
            start.addEventListener('click', async () => {
                if (start.disabled) return;
                start.disabled = true;
                const run = ++revision;
                const active = () => current() && revision === run;
                error.textContent = '';
                announcement.textContent = t('device.scan.scanning');
                content.innerHTML = `<p id="scan-progress">${t('device.scan.scanning')}</p><p id="scan-stale" role="status"></p>
                    <sl-button id="scan-cancel">${t('device.scan.cancel')}</sl-button>`;
                void focus('#scan-cancel');
                const cancel = content.querySelector('#scan-cancel') as any;
                const progress = content.querySelector('#scan-progress') as HTMLElement;
                const stale = content.querySelector('#scan-stale') as HTMLElement;
                let cancellation: Promise<boolean> | undefined;
                let cancelPending = false;
                let cancelAccepted = false;
                cancel.addEventListener('click', () => {
                    if (!active() || cancelPending || cancelAccepted) return;
                    cancelPending = true;
                    cancel.disabled = true;
                    cancel.textContent = t('device.scan.cancelling');
                    announcement.textContent = t('device.scan.cancelling');
                    error.textContent = '';
                    cancellation = (async () => {
                        try {
                            const outcome = await withDeadline(rpcCall('device.music_scan_cancel', { scanId }), 15_000, t('device.scan.requestTimeout'));
                            if (!active()) return outcome?.cancelled === true;
                            if (outcome?.cancelled !== true) throw new Error(t('device.scan.cancelNotConfirmed'));
                            cancelAccepted = true;
                            return true;
                        } catch (cause) {
                            if (active()) {
                                showError('device.scan.cancelFailed', cause);
                                announcement.textContent = '';
                                cancel.disabled = false;
                                cancel.textContent = t('device.scan.cancel');
                                void focus('#scan-cancel');
                            }
                            return false;
                        } finally { cancelPending = false; }
                    })();
                });
                let polling = false, failures = 0, lastAnnouncement = Date.now();
                timer = setInterval(async () => {
                    if (!active()) { stopPolling(); return; }
                    if (polling) return;
                    polling = true;
                    try {
                        const status = await withDeadline(rpcCall('device.music_scan_status', { scanId }), 5_000, t('device.scan.requestTimeout'));
                        if (!active()) return;
                        failures = 0;
                        stale.textContent = '';
                        progress.textContent = t('device.scan.progress', { count: status.scanned, total: status.total });
                        // Do not interrupt reading every second, or overwrite cancellation feedback.
                        if (!cancelPending && !cancelAccepted && Date.now() - lastAnnouncement >= 10_000) {
                            announcement.textContent = progress.textContent;
                            lastAnnouncement = Date.now();
                        }
                    } catch {
                        if (active() && ++failures >= 3) stale.textContent = t('device.scan.stale');
                    } finally { polling = false; }
                }, 1000);
                try {
                    const result = await rpcCall('device.music_scan_start', { scanId });
                    if (!active()) return;
                    ++revision; // Late progress/cancel replies cannot overwrite completion.
                    stopPolling();
                    error.textContent = '';
                    const summary = t('device.scan.result', { imported: result.imported, unmatched: result.unmatched, ambiguous: result.ambiguous, incompatible: result.incompatible });
                    announcement.textContent = summary;
                    content.innerHTML = `<p>${escapeAttr(summary)}</p><sl-button id="scan-done" variant="primary">${t('device.scan.done')}</sl-button>`;
                    void focus('#scan-done');
                    const done = content.querySelector('#scan-done') as any;
                    done.addEventListener('click', async () => {
                        if (done.disabled) return;
                        done.disabled = true;
                        error.textContent = '';
                        try {
                            const basket = await withDeadline(rpcCall('manifest_get_basket'), 15_000, t('device.scan.requestTimeout'));
                            if (!current()) return;
                            if (basket?.targetDeviceId === deviceId) {
                                basketStore.setDaemonContext(deviceId, basket.basketHash ?? null);
                                basketStore.hydrateFromDaemon(basket.basketItems ?? []);
                            }
                            finish();
                        } catch (cause) {
                            if (current()) showError('device.scan.refreshFailed', cause);
                        } finally {
                            done.disabled = false;
                            if (current()) void focus('#scan-done');
                        }
                    });
                } catch (cause) {
                    stopPolling();
                    const cancelled = cancellation ? await cancellation : false;
                    if (!active()) return;
                    if (cancelled) { finish(); return; }
                    announcement.textContent = '';
                    render();
                    showError('device.scan.failed', cause);
                } finally { stopPolling(); }
            });
        };
        render();
    }

    private localizeProfile(profile: DeviceProfileSummary): DeviceProfileSummary {
        // Translate stock text only: saved profiles can contain customer overrides.
        if (profile.id === 'passthrough') {
            return {
                ...profile,
                name: profile.name === 'No Transcoding (Download Original)' || profile.name === english['basket.profile.no_transcoding']
                    ? t('basket.profile.no_transcoding') : profile.name,
                description: profile.description === 'Download files as-is from Jellyfin without transcoding.' || profile.description === english['basket.profile.no_transcoding_desc']
                    ? t('basket.profile.no_transcoding_desc') : profile.description,
            };
        }
        const nameKey = `device.profile.${profile.id}.name`;
        const descriptionKey = `device.profile.${profile.id}.description`;
        return {
            ...profile,
            name: english[nameKey] && profile.name === english[nameKey] ? t(nameKey) : profile.name,
            description: english[descriptionKey] && profile.description === english[descriptionKey]
                ? t(descriptionKey) : profile.description,
        };
    }
}
