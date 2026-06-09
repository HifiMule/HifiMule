// Server Hub (Story 2.11)
// Compact header component listing all configured servers, allowing the user to
// switch the active one, add a new server (inline login), or remove one.

import { rpcCall, serverList, serverSelect, serverRemove, ServerSummary } from '../rpc';
import { basketStore } from '../state/basket';
import { initLoginView } from '../login';
import { t } from '../i18n';

function serverTypeLabel(type: string): string {
    switch (type) {
        case 'jellyfin': return 'Jellyfin';
        case 'openSubsonic': return 'OpenSubsonic';
        case 'subsonic': return 'Subsonic';
        default: return t('server.default');
    }
}

function serverBadgeVariant(type: string): string {
    switch (type) {
        case 'jellyfin': return 'primary';
        case 'openSubsonic': return 'success';
        case 'subsonic': return 'neutral';
        default: return 'neutral';
    }
}

export class ServerHub {
    private container: HTMLElement;
    private servers: ServerSummary[] = [];
    private selectedId: string | null = null;
    /** Invoked after the selected server changes (select/add/remove) so the host
     * can reload the library + basket for the new active server. */
    private onServerChanged: () => void;
    private switchInFlight = false;

    constructor(container: HTMLElement, onServerChanged: () => void) {
        this.container = container;
        this.onServerChanged = onServerChanged;
        this.refresh();
    }

    public async refresh(): Promise<void> {
        try {
            this.servers = await serverList();
            this.selectedId = this.servers.find(s => s.selected)?.id ?? null;
            // Reconcile any legacy composite serverIds in the persisted basket (AC22).
            basketStore.reconcileServerIds(this.servers);
        } catch (e) {
            console.error('[ServerHub] Failed to list servers', e);
            this.servers = [];
            this.selectedId = null;
        }
        this.render();
    }

    private selectedServer(): ServerSummary | undefined {
        return this.servers.find(s => s.id === this.selectedId);
    }

    private render(): void {
        const selected = this.selectedServer();
        const triggerLabel = selected
            ? `${selected.username} @ ${selected.url}`
            : t('serverHub.none_selected');
        const triggerType = selected ? serverTypeLabel(selected.serverType) : '';
        const triggerVariant = selected ? serverBadgeVariant(selected.serverType) : 'neutral';

        const rows = this.servers.map(s => `
            <sl-menu-item class="server-hub-row ${s.id === this.selectedId ? 'active' : ''}"
                          data-id="${this.escape(s.id)}" type="checkbox" ${s.id === this.selectedId ? 'checked' : ''}>
                <sl-badge slot="prefix" variant="${serverBadgeVariant(s.serverType)}" pill>${serverTypeLabel(s.serverType)}</sl-badge>
                <span title="${this.escape(s.url)}">${this.escape(s.username)} @ ${this.escape(s.url)}</span>
                <sl-icon-button slot="suffix" name="trash" class="server-hub-remove"
                                data-id="${this.escape(s.id)}" label="${t('serverHub.remove')}"></sl-icon-button>
            </sl-menu-item>
        `).join('');

        this.container.innerHTML = `
            <div class="server-hub">
                <sl-dropdown placement="bottom-end" hoist>
                    <div slot="trigger" class="server-connection-chip" role="button" tabindex="0" title="${this.escape(triggerLabel)}">
                        ${selected ? `<sl-badge variant="${triggerVariant}" pill>${triggerType}</sl-badge>` : ''}
                        <span>${this.escape(triggerLabel)}</span>
                        <sl-icon name="chevron-down"></sl-icon>
                    </div>
                    <sl-menu>
                        ${rows || `<sl-menu-item disabled>${t('serverHub.empty')}</sl-menu-item>`}
                        <sl-divider></sl-divider>
                        <sl-menu-item class="server-hub-add">
                            <sl-icon slot="prefix" name="plus-lg"></sl-icon>
                            ${t('serverHub.add')}
                        </sl-menu-item>
                        <sl-menu-item class="server-hub-logout">
                            <sl-icon slot="prefix" name="box-arrow-right"></sl-icon>
                            ${t('ui.logout')}
                        </sl-menu-item>
                    </sl-menu>
                </sl-dropdown>
            </div>
        `;

        this.bindEvents();
    }

    private bindEvents(): void {
        // Switch server on row click (ignore clicks on the remove button).
        this.container.querySelectorAll('.server-hub-row').forEach(row => {
            row.addEventListener('click', (e) => {
                if ((e.target as HTMLElement)?.closest('.server-hub-remove')) return;
                const id = (row as HTMLElement).dataset.id;
                if (id && id !== this.selectedId) this.handleSelect(id);
            });
        });
        this.container.querySelectorAll('.server-hub-remove').forEach(btn => {
            btn.addEventListener('click', (e) => {
                e.stopPropagation();
                const id = (btn as HTMLElement).dataset.id;
                if (id) this.handleRemove(id);
            });
        });
        this.container.querySelector('.server-hub-add')?.addEventListener('click', () => this.handleAdd());
        this.container.querySelector('.server-hub-logout')?.addEventListener('click', () => this.handleLogout());
    }

    private async handleSelect(id: string): Promise<void> {
        if (this.switchInFlight) return;
        this.switchInFlight = true;
        try {
            await serverSelect(id);
            this.selectedId = id;
            basketStore.setActiveServerId(id);
            await this.refresh();
            this.onServerChanged();
        } catch (e) {
            const msg = e instanceof Error ? e.message : String(e);
            window.dispatchEvent(new CustomEvent('toast', { detail: { type: 'error', message: msg } }));
        } finally {
            this.switchInFlight = false;
        }
    }

    private handleAdd(): void {
        // Inline add: present the Story 2.5 connection form without a full-screen
        // takeover, then refresh the hub on success (AC4).
        initLoginView(() => {
            this.refresh().then(() => this.onServerChanged());
        }, { mode: 'add' });
    }

    private handleRemove(id: string): void {
        const server = this.servers.find(s => s.id === id);
        if (!server) return;
        const isSelected = id === this.selectedId;

        const dialog = document.createElement('sl-dialog') as any;
        dialog.label = t('serverHub.remove_title');
        const warning = isSelected
            ? `<sl-alert variant="warning" open style="margin-bottom: 0.75rem;">
                 <sl-icon slot="icon" name="exclamation-triangle"></sl-icon>
                 ${t('serverHub.remove_selected_warning')}
               </sl-alert>`
            : '';
        dialog.innerHTML = `
            ${warning}
            <p>${t('serverHub.remove_confirm', { server: `${server.username} @ ${server.url}` })}</p>
            <sl-button slot="footer" variant="default" id="server-remove-cancel">${t('basket.actions.cancel')}</sl-button>
            <sl-button slot="footer" variant="danger" id="server-remove-confirm">${t('serverHub.remove')}</sl-button>
        `;
        document.body.appendChild(dialog);
        dialog.querySelector('#server-remove-cancel')?.addEventListener('click', () => dialog.hide());
        dialog.querySelector('#server-remove-confirm')?.addEventListener('click', async () => {
            try {
                await serverRemove(id);
                // Drop the removed server's basket items and notify (AC7).
                const removed = basketStore.removeItemsForServer(id);
                if (removed > 0) {
                    window.dispatchEvent(new CustomEvent('toast', {
                        detail: {
                            type: 'info',
                            message: t('serverHub.items_removed', {
                                count: removed,
                                server: serverTypeLabel(server.serverType),
                            }),
                        },
                    }));
                }
                dialog.hide();
                await this.refresh();
                this.onServerChanged();
            } catch (e) {
                const msg = e instanceof Error ? e.message : String(e);
                window.dispatchEvent(new CustomEvent('toast', { detail: { type: 'error', message: msg } }));
            }
        });
        dialog.addEventListener('sl-after-hide', (ev: Event) => {
            if (ev.target === dialog) dialog.remove();
        });
        customElements.whenDefined('sl-dialog').then(() => dialog.show());
    }

    private async handleLogout(): Promise<void> {
        try {
            await rpcCall('server.logout');
            basketStore.setActiveServerId(null);
            basketStore.clearLocalOnly();
            this.onServerChanged();
        } catch (e) {
            console.error('[ServerHub] logout failed', e);
        }
    }

    private escape(text: string): string {
        return text
            .replace(/&/g, '&amp;')
            .replace(/</g, '&lt;')
            .replace(/>/g, '&gt;')
            .replace(/"/g, '&quot;');
    }
}
