// Repair Modal Component
// Provides a side-by-side discrepancy view for fixing dirty manifests.

import { rpcCall } from '../rpc';

interface DiscrepancyItem {
    jellyfinId: string;
    name: string;
    localPath: string;
    album?: string;
    artist?: string;
}

interface Discrepancies {
    missing: DiscrepancyItem[];
    orphaned: DiscrepancyItem[];
}

export class RepairModal {
    private dialog: HTMLElement | null = null;
    private discrepancies: Discrepancies | null = null;
    private onComplete: (() => void) | null = null;

    constructor(private container: HTMLElement, onComplete?: () => void) {
        this.onComplete = onComplete || null;
    }

    async open() {
        this.renderDialog();
        this.showDialog();

        try {
            this.discrepancies = await rpcCall('manifest_get_discrepancies') as Discrepancies;
            this.renderContent();
        } catch (err) {
            this.renderError((err as Error).message);
        }
    }

    private showDialog() {
        const dialog = this.container.querySelector('#repair-dialog') as any;
        if (dialog) {
            dialog.show();
            this.dialog = dialog;
        }
    }

    private renderDialog() {
        // Remove any existing dialog first
        this.container.querySelector('#repair-dialog')?.remove();

        const dialogEl = document.createElement('sl-dialog');
        dialogEl.id = 'repair-dialog';
        dialogEl.setAttribute('label', 'Manifest Repair');
        dialogEl.setAttribute('style', 'max-width: 90vw; --width: 720px;');
        dialogEl.innerHTML = `
            <div class="repair-body">
                <div class="repair-loading">
                    <sl-spinner style="font-size: 2rem;"></sl-spinner>
                    <p>Scanning device for discrepancies...</p>
                </div>
            </div>
        `;

        this.container.appendChild(dialogEl);

        dialogEl.addEventListener('sl-after-hide', () => {
            dialogEl.remove();
        });
    }

    private renderContent() {
        const body = this.dialog?.querySelector('.repair-body');
        if (!body || !this.discrepancies) return;

        const { missing, orphaned } = this.discrepancies;
        const hasIssues = missing.length > 0 || orphaned.length > 0;

        if (!hasIssues) {
            body.innerHTML = `
                <div class="repair-clean">
                    <sl-icon name="check-circle-fill" style="font-size: 3rem; color: var(--sl-color-success-600);"></sl-icon>
                    <h3>No Discrepancies Found</h3>
                    <p>The manifest matches the files on your device.</p>
                    <sl-button id="repair-clear-dirty-btn" variant="primary" style="margin-top: 1rem;">
                        <sl-icon slot="prefix" name="check"></sl-icon>
                        Clear Dirty Flag
                    </sl-button>
                </div>
            `;
            body.querySelector('#repair-clear-dirty-btn')?.addEventListener('click', () => this.handleClearDirty());
            return;
        }

        body.innerHTML = `
            <div class="repair-columns">
                <div class="repair-column repair-missing-col">
                    <div class="repair-col-header">
                        <sl-icon name="file-earmark-x" style="color: var(--sl-color-danger-500);"></sl-icon>
                        <h4>Missing Files <sl-badge variant="danger" pill>${missing.length}</sl-badge></h4>
                    </div>
                    <p class="repair-col-desc">In manifest but not on device</p>
                    <div class="repair-item-list" id="repair-missing-list">
                        ${missing.length === 0 ? '<div class="repair-no-items">No missing files</div>' :
                missing.map(item => this.renderMissingItem(item)).join('')}
                    </div>
                </div>
                <div class="repair-column repair-orphaned-col">
                    <div class="repair-col-header">
                        <sl-icon name="file-earmark-plus" style="color: var(--sl-color-warning-500);"></sl-icon>
                        <h4>Orphaned Files <sl-badge variant="warning" pill>${orphaned.length}</sl-badge></h4>
                    </div>
                    <p class="repair-col-desc">On device but not in manifest</p>
                    <div class="repair-item-list" id="repair-orphaned-list">
                        ${orphaned.length === 0 ? '<div class="repair-no-items">No orphaned files</div>' :
                orphaned.map(item => this.renderOrphanedItem(item)).join('')}
                    </div>
                </div>
            </div>
            <div class="repair-actions">
                <sl-button id="repair-prune-all-btn" variant="danger" size="small"
                           ${missing.length === 0 ? 'disabled' : ''}>
                    <sl-icon slot="prefix" name="trash"></sl-icon>
                    Prune All Missing (${missing.length})
                </sl-button>
                <sl-button id="repair-done-btn" variant="primary" size="small">
                    <sl-icon slot="prefix" name="check"></sl-icon>
                    Finish & Clear Dirty
                </sl-button>
            </div>
        `;

        this.bindEvents();
    }

    private renderMissingItem(item: DiscrepancyItem): string {
        const pathParts = item.localPath.split('/');
        const filename = pathParts[pathParts.length - 1];
        const folder = pathParts.slice(0, -1).join('/');

        return `
            <div class="repair-item" data-id="${this.escapeAttr(item.jellyfinId)}" data-path="${this.escapeAttr(item.localPath)}">
                <div class="repair-item-info">
                    <span class="repair-item-name" title="${this.escapeAttr(item.name)}">${this.escapeHtml(item.name)}</span>
                    <span class="repair-item-path" title="${this.escapeAttr(item.localPath)}">${this.escapeHtml(folder)}/<strong>${this.escapeHtml(filename)}</strong></span>
                </div>
                <div class="repair-item-actions">
                    <sl-button class="prune-single-btn" size="small" variant="danger" outline
                               data-id="${this.escapeAttr(item.jellyfinId)}"
                               title="Remove from manifest">
                        <sl-icon name="trash" slot="prefix"></sl-icon>
                        Prune
                    </sl-button>
                </div>
            </div>
        `;
    }

    private renderOrphanedItem(item: DiscrepancyItem): string {
        const pathParts = item.localPath.split('/');
        const filename = pathParts[pathParts.length - 1];
        const folder = pathParts.slice(0, -1).join('/');

        return `
            <div class="repair-item orphaned" data-path="${this.escapeAttr(item.localPath)}">
                <div class="repair-item-info">
                    <span class="repair-item-name" title="${this.escapeAttr(item.name)}">${this.escapeHtml(item.name)}</span>
                    <span class="repair-item-path" title="${this.escapeAttr(item.localPath)}">${this.escapeHtml(folder)}/<strong>${this.escapeHtml(filename)}</strong></span>
                </div>
                <div class="repair-item-actions">
                    <sl-button class="relink-btn" size="small" variant="warning" outline
                               data-path="${this.escapeAttr(item.localPath)}"
                               title="Re-link to a missing manifest entry">
                        <sl-icon name="link-45deg" slot="prefix"></sl-icon>
                        Re-link
                    </sl-button>
                </div>
            </div>
        `;
    }

    private renderError(message: string) {
        const body = this.dialog?.querySelector('.repair-body');
        if (!body) return;

        body.innerHTML = `
            <div class="repair-error">
                <sl-icon name="exclamation-triangle-fill" style="font-size: 2.5rem; color: var(--sl-color-danger-500);"></sl-icon>
                <p>Failed to scan device: ${this.escapeHtml(message)}</p>
                <sl-button id="repair-retry-btn" variant="primary" size="small">Retry</sl-button>
            </div>
        `;

        body.querySelector('#repair-retry-btn')?.addEventListener('click', () => this.open());
    }

    private bindEvents() {
        const body = this.dialog?.querySelector('.repair-body');
        if (!body) return;

        // Prune single missing item
        body.querySelectorAll('.prune-single-btn').forEach(btn => {
            btn.addEventListener('click', async (e) => {
                const id = (e.currentTarget as HTMLElement).getAttribute('data-id');
                if (id) await this.handlePrune([id]);
            });
        });

        // Prune all missing items
        body.querySelector('#repair-prune-all-btn')?.addEventListener('click', async () => {
            if (!this.discrepancies) return;
            const ids = this.discrepancies.missing.map(m => m.jellyfinId);
            await this.handlePrune(ids);
        });

        // Re-link orphaned to missing
        body.querySelectorAll('.relink-btn').forEach(btn => {
            btn.addEventListener('click', async (e) => {
                const orphanPath = (e.currentTarget as HTMLElement).getAttribute('data-path');
                if (orphanPath) await this.handleRelinkPrompt(orphanPath);
            });
        });

        // Done / Clear dirty
        body.querySelector('#repair-done-btn')?.addEventListener('click', () => this.handleClearDirty());
    }

    private async handlePrune(ids: string[]) {
        try {
            await rpcCall('manifest_prune', { itemIds: ids });
            // Refresh discrepancies
            this.discrepancies = await rpcCall('manifest_get_discrepancies') as Discrepancies;
            this.renderContent();
        } catch (err) {
            console.error('[Repair] Prune failed:', err);
        }
    }

    private async handleRelinkPrompt(orphanPath: string) {
        if (!this.discrepancies || this.discrepancies.missing.length === 0) {
            return;
        }

        // If there's only one missing item, auto-select it
        if (this.discrepancies.missing.length === 1) {
            await this.handleRelink(this.discrepancies.missing[0].jellyfinId, orphanPath);
            return;
        }

        // Show a selection dropdown for which missing entry to relink to
        const body = this.dialog?.querySelector('.repair-body');
        if (!body) return;

        const orphanedEl = body.querySelector(`.repair-item.orphaned[data-path="${CSS.escape(orphanPath)}"]`);
        if (!orphanedEl) return;

        const actionsDiv = orphanedEl.querySelector('.repair-item-actions');
        if (!actionsDiv) return;

        actionsDiv.innerHTML = `
            <select class="relink-select" style="font-size: 0.75rem; padding: 0.2rem; background: #1e293b; color: #f1f5f9; border: 1px solid rgba(255,255,255,0.2); border-radius: 4px;">
                <option value="">Link to...</option>
                ${this.discrepancies.missing.map(m =>
            `<option value="${this.escapeAttr(m.jellyfinId)}">${this.escapeHtml(m.name)}</option>`
        ).join('')}
            </select>
        `;

        const select = actionsDiv.querySelector('.relink-select') as HTMLSelectElement;
        select.addEventListener('change', async () => {
            const selectedId = select.value;
            if (selectedId) {
                await this.handleRelink(selectedId, orphanPath);
            }
        });
    }

    private async handleRelink(jellyfinId: string, newLocalPath: string) {
        try {
            await rpcCall('manifest_relink', { jellyfinId, newLocalPath });
            // Refresh discrepancies
            this.discrepancies = await rpcCall('manifest_get_discrepancies') as Discrepancies;
            this.renderContent();
        } catch (err) {
            console.error('[Repair] Relink failed:', err);
        }
    }

    private async handleClearDirty() {
        try {
            await rpcCall('manifest_clear_dirty');
            const dialog = this.container.querySelector('#repair-dialog') as any;
            if (dialog) dialog.hide();
            this.onComplete?.();
        } catch (err) {
            console.error('[Repair] Clear dirty failed:', err);
        }
    }

    private escapeHtml(text: string): string {
        const div = document.createElement('div');
        div.textContent = text;
        return div.innerHTML;
    }

    private escapeAttr(text: string): string {
        return text.replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/'/g, '&#39;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
    }
}
