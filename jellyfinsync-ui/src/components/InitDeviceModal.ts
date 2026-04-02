// Initialize Device Modal Component
// Guides the user through initializing a new unrecognized device for sync.
// Follows the RepairModal.ts pattern: sl-dialog, class-based, open() method, onComplete callback.

import { rpcCall } from '../rpc';

export class InitDeviceModal {
    private dialog: HTMLElement | null = null;
    private onComplete: (() => void) | null = null;

    constructor(_container: HTMLElement, onComplete?: () => void) {
        this.onComplete = onComplete || null;
    }

    async open() {
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
        dialogEl.setAttribute('label', 'Initialize Device');
        dialogEl.innerHTML = `
            <div class="init-device-body">
                <div class="init-device-loading">
                    <sl-spinner style="font-size: 2rem;"></sl-spinner>
                    <p>Loading device info...</p>
                </div>
            </div>
            <sl-button slot="footer" id="init-device-cancel-btn" variant="default">Cancel</sl-button>
            <sl-button slot="footer" id="init-device-confirm-btn" variant="primary" disabled>
                <sl-icon slot="prefix" name="check2-circle"></sl-icon>
                Confirm
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
                : [{ id: 'passthrough', name: 'No Transcoding', description: 'Sync audio files as-is without transcoding.' }];

            const userId = creds?.userId || null;
            this.renderContent(body as HTMLElement, userId, profiles);
        } catch (err) {
            // Tauri invoke rejects with a plain string, not an Error object
            const message = typeof err === 'string' ? err : (err instanceof Error ? err.message : String(err));
            this.renderError(body as HTMLElement, message || 'Unknown error');
        }
    }

    private renderContent(body: HTMLElement, userId: string | null, profiles: any[]) {
        if (!userId) {
            body.innerHTML = `
                <div class="init-device-no-login">
                    <sl-icon name="person-x" style="font-size: 2.5rem; opacity: 0.6;"></sl-icon>
                    <p>Connect to Jellyfin first before initializing a device.</p>
                </div>
            `;
            return;
        }

        const profileOptions = (profiles || []).map(p => 
            `<sl-option value="${this.escapeHtml(p.id)}">${this.escapeHtml(p.name)}</sl-option>`
        ).join('');
        const defaultPassthroughDesc = (profiles || []).find(p => p.id === 'passthrough')?.description || '';

        body.innerHTML = `
            <div class="init-device-form">
                <p style="margin-bottom: 1rem; opacity: 0.8;">
                    A new device has been detected with no sync configuration.
                    Set up the sync folder to get started.
                </p>
                <div style="margin-bottom: 1.25rem;">
                    <label style="font-size: 0.8rem; opacity: 0.7; display: block; margin-bottom: 0.25rem;">
                        Sync Folder Name (optional)
                    </label>
                    <sl-input
                        id="init-folder-input"
                        placeholder="Leave empty to sync to device root"
                        clearable
                    ></sl-input>
                    <div style="font-size: 0.75rem; opacity: 0.55; margin-top: 0.3rem;">
                        Example: "Music" — leave empty to use the entire device
                    </div>
                </div>
                <div style="margin-bottom: 1.25rem;">
                    <label style="font-size: 0.8rem; opacity: 0.7; display: block; margin-bottom: 0.25rem;">
                        Transcoding Profile
                    </label>
                    <sl-select id="init-transcoding-profile" value="passthrough">
                        ${profileOptions}
                    </sl-select>
                    <div style="font-size: 0.75rem; opacity: 0.55; margin-top: 0.3rem;" id="init-transcoding-desc">
                        ${this.escapeHtml(defaultPassthroughDesc)}
                    </div>
                </div>
                <div style="padding: 0.75rem; background: rgba(255,255,255,0.04); border-radius: 6px; border: 1px solid rgba(255,255,255,0.08);">
                    <div style="font-size: 0.75rem; opacity: 0.55; margin-bottom: 0.25rem;">Linked Jellyfin Profile</div>
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
        if (selectEl && descEl) {
            selectEl.addEventListener('sl-change', (e: any) => {
                const selectedId = e.target.value;
                const profile = (profiles || []).find(p => p.id === selectedId);
                if (profile) descEl.innerHTML = this.escapeHtml(profile.description);
            });
        }

        // Enable confirm button once content is rendered
        const confirmBtn = this.dialog?.querySelector('#init-device-confirm-btn') as any;
        if (confirmBtn) {
            confirmBtn.disabled = false;
            confirmBtn.addEventListener('click', () => this.handleConfirm(userId));
        }
    }

    private renderError(body: HTMLElement, message: string) {
        body.innerHTML = `
            <div class="init-device-error">
                <sl-icon name="exclamation-triangle-fill" style="font-size: 2.5rem; color: var(--sl-color-danger-500);"></sl-icon>
                <p>Failed to load device info: ${this.escapeHtml(message)}</p>
                <sl-button id="init-retry-btn" variant="primary" size="small">Retry</sl-button>
            </div>
        `;
        body.querySelector('#init-retry-btn')?.addEventListener('click', () => this.loadCredentials());
    }

    private renderSubmitting(body: HTMLElement) {
        body.innerHTML = `
            <div class="init-device-loading">
                <sl-spinner style="font-size: 2rem;"></sl-spinner>
                <p>Initializing device...</p>
            </div>
        `;
    }

    private renderInitError(body: HTMLElement, message: string) {
        body.innerHTML = `
            <div class="init-device-error">
                <sl-icon name="exclamation-triangle-fill" style="font-size: 2.5rem; color: var(--sl-color-danger-500);"></sl-icon>
                <sl-alert variant="danger" open>
                    <sl-icon slot="icon" name="exclamation-octagon"></sl-icon>
                    <strong>Initialization Failed</strong><br>
                    ${this.escapeHtml(message)}
                </sl-alert>
                <div style="display: flex; gap: 0.5rem; margin-top: 1rem; justify-content: flex-end;">
                    <sl-button id="init-retry-btn" variant="primary" size="small">Retry</sl-button>
                    <sl-button id="init-dismiss-btn" variant="default" size="small">Dismiss</sl-button>
                </div>
            </div>
        `;
        body.querySelector('#init-retry-btn')?.addEventListener('click', () => this.loadCredentials());
        body.querySelector('#init-dismiss-btn')?.addEventListener('click', () => {
            if (this.dialog) (this.dialog as any).hide();
        });
    }

    private async handleConfirm(userId: string) {
        const body = this.dialog?.querySelector('.init-device-body') as HTMLElement | null;
        if (!body) return;

        const folderInput = this.dialog?.querySelector('#init-folder-input') as any;
        const folderPath: string = folderInput?.value?.trim() ?? '';

        const profileSelect = this.dialog?.querySelector('#init-transcoding-profile') as any;
        let transcodingProfileId = profileSelect?.value;

        const confirmBtn = this.dialog?.querySelector('#init-device-confirm-btn') as any;
        if (confirmBtn) confirmBtn.loading = true;

        this.renderSubmitting(body);

        try {
            await rpcCall('device_initialize', {
                folderPath,
                profileId: userId,
                transcodingProfileId,
            });

            if (this.dialog) (this.dialog as any).hide();

            this.onComplete?.();
        } catch (err) {
            const message = typeof err === 'string' ? err : (err instanceof Error ? err.message : String(err));
            this.renderInitError(body, message);
        }
    }

    private escapeHtml(text: string): string {
        const div = document.createElement('div');
        div.textContent = text;
        return div.innerHTML;
    }
}
