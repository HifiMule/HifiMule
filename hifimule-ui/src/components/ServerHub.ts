// Server Hub (Story 2.11)
// Compact header component listing all configured servers, allowing the user to
// switch the active one, add a new server (inline login), or remove one.

import { rpcCall, serverList, serverSelect, serverRemove, serverUpdate, ServerSummary } from '../rpc';
import { basketStore } from '../state/basket';
import { initLoginView } from '../login';
import { t } from '../i18n';
import { SERVER_ICON_OPTIONS, formatServerIdentity } from '../serverIdentity';

export class ServerHub {
    private container: HTMLElement;
    private servers: ServerSummary[] = [];
    private selectedId: string | null = null;
    /** Invoked after the selected server changes (select/add/remove) so the host
     * can reload the library + basket for the new active server. */
    private onServerChanged: () => void;
    private switchInFlight = false;
    private refreshGeneration = 0;
    private refreshFailed = false;

    constructor(container: HTMLElement, onServerChanged: () => void) {
        this.container = container;
        this.onServerChanged = onServerChanged;
        this.refresh();
    }

    public async refresh(): Promise<void> {
        const generation = ++this.refreshGeneration;
        try {
            const servers = await serverList();
            if (generation !== this.refreshGeneration) return;
            this.servers = servers;
            this.refreshFailed = false;
            this.selectedId = this.servers.find(s => s.selected)?.id ?? null;
            // Reconcile any legacy composite serverIds in the persisted basket (AC22).
            basketStore.reconcileServerIds(this.servers);
        } catch (e) {
            if (generation !== this.refreshGeneration) return;
            console.error('[ServerHub] Failed to list servers', e);
            this.refreshFailed = true;
        }
        this.render();
    }

    private selectedServer(): ServerSummary | undefined {
        return this.servers.find(s => s.id === this.selectedId);
    }

    private render(): void {
        const selected = this.selectedServer();
        const selectedIdentity = selected ? formatServerIdentity(selected) : null;
        const triggerLabel = selected
            ? selectedIdentity!.label
            : t('serverHub.none_selected');

        const rows = this.servers.map(s => {
            const identity = formatServerIdentity(s);
            return `
            <sl-menu-item class="server-hub-row ${s.id === this.selectedId ? 'active' : ''}"
                          data-id="${this.escape(s.id)}" type="checkbox" ${s.id === this.selectedId ? 'checked' : ''}>
                <sl-icon slot="prefix" name="${this.escape(identity.icon)}"></sl-icon>
                <span class="server-hub-row-text" title="${this.escape(identity.tooltip)}">
                    <span class="server-hub-row-name">${this.escape(identity.label)}</span>
                    <span class="server-hub-row-meta">${this.escape(identity.secondaryText)}</span>
                </span>
                <sl-icon-button slot="suffix" name="pencil" class="server-hub-edit"
                                data-id="${this.escape(s.id)}" label="${t('serverHub.edit')}"></sl-icon-button>
                <sl-icon-button slot="suffix" name="trash" class="server-hub-remove"
                                data-id="${this.escape(s.id)}" label="${t('serverHub.remove')}"></sl-icon-button>
            </sl-menu-item>
        `;
        }).join('');

        this.container.innerHTML = `
            <div class="server-hub">
                <sl-dropdown placement="bottom-end" hoist>
                    <div slot="trigger" class="server-connection-chip" role="button" tabindex="0" title="${this.escape(selectedIdentity?.tooltip ?? triggerLabel)}">
                        ${selectedIdentity ? `<sl-icon name="${this.escape(selectedIdentity.icon)}"></sl-icon>` : ''}
                        <span>${this.escape(triggerLabel)}</span>
                        <sl-icon name="chevron-down"></sl-icon>
                    </div>
                    <sl-menu>
                        ${rows || (!this.refreshFailed ? `<sl-menu-item disabled>${t('serverHub.empty')}</sl-menu-item>` : '')}
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
                ${this.refreshFailed ? `<div role="status" style="max-width:100%;overflow-wrap:anywhere">
                    <sl-button class="server-hub-retry" size="small" style="max-width:100%">${t('serverHub.refresh_failed')}</sl-button>
                </div>` : ''}
            </div>
        `;

        this.bindEvents();
    }

    private bindEvents(): void {
        this.container.querySelector('.server-hub-retry')?.addEventListener('click', () => void this.refresh());
        // Switch server on row click (ignore clicks on the remove button).
        this.container.querySelectorAll('.server-hub-row').forEach(row => {
            row.addEventListener('click', (e) => {
                if ((e.target as HTMLElement)?.closest('.server-hub-remove, .server-hub-edit')) return;
                const id = (row as HTMLElement).dataset.id;
                if (id && id !== this.selectedId) this.handleSelect(id);
            });
        });
        this.container.querySelectorAll('.server-hub-edit').forEach(btn => {
            btn.addEventListener('click', (e) => {
                e.stopPropagation();
                const id = (btn as HTMLElement).dataset.id;
                if (id) this.handleEdit(id);
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

    private handleEdit(id: string): void {
        const server = this.servers.find(s => s.id === id);
        if (!server) return;
        const identity = formatServerIdentity(server);
        let selectedIcon = identity.icon;

        const dialog = document.createElement('sl-dialog') as any;
        dialog.className = 'server-identity-dialog';
        dialog.label = t('serverHub.identity_title');
        dialog.innerHTML = `
            <div class="server-identity-form">
                <sl-input
                    id="server-identity-name"
                    label="${t('serverHub.identity_name')}"
                    maxlength="40"
                    required
                    value="${this.escape(identity.label)}">
                </sl-input>
                <div class="device-settings-description">${this.escape(identity.secondaryText)}</div>
                <label class="device-settings-label">${t('serverHub.identity_icon')}</label>
                <div class="device-settings-icon-picker server-identity-icon-picker">
                    ${SERVER_ICON_OPTIONS.map(icon => `
                        <button type="button" class="init-icon-tile ${icon === selectedIcon ? 'selected' : ''}" data-icon="${this.escape(icon)}" title="${this.escape(icon)}" aria-label="${this.escape(icon)}" aria-pressed="${icon === selectedIcon}">
                            <sl-icon name="${this.escape(icon)}"></sl-icon>
                        </button>
                    `).join('')}
                </div>
                <sl-alert id="server-identity-error" variant="danger" closable style="display:none;"></sl-alert>
            </div>
            <sl-button slot="footer" variant="default" id="server-identity-cancel">${t('basket.actions.cancel')}</sl-button>
            <sl-button slot="footer" variant="primary" id="server-identity-save">${t('basket.actions.save')}</sl-button>
        `;
        document.body.appendChild(dialog);

        const setSelectedIcon = (icon: string) => {
            selectedIcon = icon;
            dialog.querySelectorAll('.init-icon-tile').forEach((tile: Element) => {
                tile.classList.toggle('selected', (tile as HTMLElement).dataset.icon === icon);
                tile.setAttribute('aria-pressed', String((tile as HTMLElement).dataset.icon === icon));
            });
        };
        dialog.querySelectorAll('.init-icon-tile').forEach((tile: Element) => {
            tile.addEventListener('click', () => {
                const icon = (tile as HTMLElement).dataset.icon;
                if (icon) setSelectedIcon(icon);
            });
        });
        const cancel = dialog.querySelector('#server-identity-cancel') as any;
        const save = dialog.querySelector('#server-identity-save') as any;
        cancel?.addEventListener('click', () => { if (!save.loading) dialog.hide(); });
        dialog.addEventListener('sl-request-close', (event: Event) => {
            if (event.target === dialog && save.loading) event.preventDefault();
        });
        dialog.querySelector('#server-identity-save')?.addEventListener('click', async () => {
            const saveBtn = dialog.querySelector('#server-identity-save') as any;
            if (saveBtn.loading) return;
            const errorEl = dialog.querySelector('#server-identity-error') as HTMLElement | null;
            const nameInput = dialog.querySelector('#server-identity-name') as any;
            const name = (nameInput?.value ?? '').trim();
            if (!name) {
                if (errorEl) {
                    errorEl.textContent = t('serverHub.identity_name_required');
                    errorEl.style.display = 'block';
                    (errorEl as any).open = true;
                }
                return;
            }
            try {
                saveBtn.loading = true;
                cancel.disabled = true;
                await serverUpdate({ id, name, icon: selectedIcon });
                dialog.hide();
                await this.refresh();
                this.onServerChanged();
            } catch (e) {
                const msg = e instanceof Error ? e.message : String(e);
                if (errorEl) {
                    errorEl.textContent = t('serverHub.identity_error', { message: msg });
                    errorEl.style.display = 'block';
                    (errorEl as any).open = true;
                }
            } finally {
                saveBtn.loading = false;
                cancel.disabled = false;
            }
        });
        dialog.addEventListener('sl-after-hide', (ev: Event) => {
            if (ev.target === dialog) dialog.remove();
        });
        customElements.whenDefined('sl-dialog').then(() => dialog.show());
    }

    private async handleSelect(id: string): Promise<void> {
        if (this.switchInFlight) return;
        this.switchInFlight = true;
        try {
            await serverSelect(id);
            this.selectedId = id;
            // server.select keys on the local id, but the basket's active-server key
            // is the PORTABLE id (Story 2.13) so own items never render locked.
            const selected = this.servers.find(s => s.id === id);
            const portableId = selected?.serverId ?? null;
            if (selected && !portableId) {
                console.warn('[ServerHub] selected server has no portable serverId; tagging will be disabled until daemon backfills', { localId: id });
            }
            basketStore.setActiveServerId(portableId);
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
        const identity = formatServerIdentity(server);
        const warning = isSelected
            ? `<sl-alert variant="warning" open style="margin-bottom: 0.75rem;">
                 <sl-icon slot="icon" name="exclamation-triangle"></sl-icon>
                 ${t('serverHub.remove_selected_warning')}
               </sl-alert>`
            : '';
        dialog.innerHTML = `
            ${warning}
            <p style="overflow-wrap:anywhere">${this.escape(t('serverHub.remove_confirm', { server: identity.label }))}</p>
            <sl-button slot="footer" variant="default" id="server-remove-cancel">${t('basket.actions.cancel')}</sl-button>
            <sl-button slot="footer" variant="danger" id="server-remove-confirm">${t('serverHub.remove')}</sl-button>
        `;
        document.body.appendChild(dialog);
        const cancel = dialog.querySelector('#server-remove-cancel') as any;
        const remove = dialog.querySelector('#server-remove-confirm') as any;
        cancel?.addEventListener('click', () => { if (!remove.loading) dialog.hide(); });
        dialog.addEventListener('sl-request-close', (event: Event) => {
            if (event.target === dialog && remove.loading) event.preventDefault();
        });
        dialog.querySelector('#server-remove-confirm')?.addEventListener('click', async () => {
            const confirm = dialog.querySelector('#server-remove-confirm') as any;
            if (confirm.loading) return;
            confirm.loading = true;
            cancel.disabled = true;
            try {
                await serverRemove(id);
                // Drop the removed server's basket items and notify (AC7). Basket
                // items are tagged with the PORTABLE id (Story 2.13) — sweep that
                // first; also sweep the local id to catch any pre-reconciliation
                // legacy tags that slipped through.
                let removed = 0;
                if (server.serverId) {
                    removed += basketStore.removeItemsForServer(server.serverId);
                }
                if (!server.serverId || server.serverId !== id) {
                    removed += basketStore.removeItemsForServer(id);
                }
                if (removed > 0) {
                    window.dispatchEvent(new CustomEvent('toast', {
                        detail: {
                            type: 'info',
                            message: t('serverHub.items_removed', {
                                count: removed,
                                server: identity.label,
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
            } finally {
                confirm.loading = false;
                cancel.disabled = false;
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
            const message = e instanceof Error ? e.message : String(e);
            window.dispatchEvent(new CustomEvent('toast', { detail: { type: 'error', message } }));
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
