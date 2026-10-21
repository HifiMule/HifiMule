import { escapeHtml, escapeAttr } from '../html';
// Repair Modal Component
// Provides a side-by-side discrepancy view for fixing dirty manifests.

import { rpcCall } from '../rpc';
import { t } from '../i18n';
import { showToast, ERROR_TOAST_DURATION } from '../toast';

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
    /** Serializes manifest mutations so a rapid double-click (or clicking prune
     * while a relink is in flight) can't fire two writes against the manifest. */
    private busy = false;
    private scanGeneration = 0;
    private disabledControls: HTMLElement[] = [];

    constructor(private container: HTMLElement, onComplete?: () => void) {
        this.onComplete = onComplete || null;
    }

    async open() {
        if (this.busy) return;
        this.discrepancies = null;
        this.renderDialog();
        this.showDialog();
        await this.scan();
    }

    private async scan() {
        const generation = ++this.scanGeneration;
        try {
            const discrepancies = await rpcCall('manifest_get_discrepancies') as Discrepancies;
            if (generation !== this.scanGeneration) return;
            this.discrepancies = discrepancies;
            this.renderContent();
        } catch (err) {
            if (generation !== this.scanGeneration) return;
            this.discrepancies = null;
            this.renderError(this.errorMessage(err));
        }
    }

    private errorMessage(error: unknown): string {
        return error instanceof Error ? error.message : String(error);
    }

    private setBusy(busy: boolean) {
        this.busy = busy;
        this.dialog?.setAttribute('aria-busy', String(busy));
        if (busy) {
            this.disabledControls = Array.from(
                this.dialog?.querySelectorAll<HTMLElement>('sl-button:not([disabled]), sl-select:not([disabled])') ?? []
            );
            this.disabledControls.forEach(control => control.setAttribute('disabled', ''));
        } else {
            this.disabledControls.forEach(control => control.removeAttribute('disabled'));
            this.disabledControls = [];
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
        dialogEl.setAttribute('label', t('repair.title'));
        dialogEl.setAttribute('style', 'max-width: 90vw; --width: 720px;');
        dialogEl.innerHTML = `
            <div class="repair-body">
                <div class="repair-loading" role="status">
                    <sl-spinner style="font-size: 2rem;"></sl-spinner>
                    <p>${t('repair.scanning')}</p>
                </div>
            </div>
        `;

        this.container.appendChild(dialogEl);

        dialogEl.addEventListener('sl-request-close', event => {
            if (event.target === dialogEl && this.busy) event.preventDefault();
        });
        dialogEl.addEventListener('sl-hide', event => {
            if (event.target === dialogEl && this.dialog === dialogEl) {
                ++this.scanGeneration;
                this.dialog = null;
                this.discrepancies = null;
            }
        });
        dialogEl.addEventListener('sl-after-hide', event => {
            if (event.target === dialogEl) dialogEl.remove();
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
                    <h3>${t('repair.no_discrepancies')}</h3>
                    <p>${t('repair.manifest_matches')}</p>
                    <sl-button id="repair-clear-dirty-btn" variant="primary" style="margin-top: 1rem;">
                        <sl-icon slot="prefix" name="check"></sl-icon>
                        ${t('repair.clear_dirty_flag')}
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
                        <h4>${t('repair.missing_files')} <sl-badge variant="danger" pill>${missing.length}</sl-badge></h4>
                    </div>
                    <p class="repair-col-desc">${t('repair.missing_desc')}</p>
                    <div class="repair-item-list" id="repair-missing-list">
                        ${missing.length === 0 ? `<div class="repair-no-items">${t('repair.no_missing')}</div>` :
                missing.map(item => this.renderMissingItem(item)).join('')}
                    </div>
                </div>
                <div class="repair-column repair-orphaned-col">
                    <div class="repair-col-header">
                        <sl-icon name="file-earmark-plus" style="color: var(--sl-color-warning-500);"></sl-icon>
                        <h4>${t('repair.orphaned_files')} <sl-badge variant="warning" pill>${orphaned.length}</sl-badge></h4>
                    </div>
                    <p class="repair-col-desc">${t('repair.orphaned_desc')}</p>
                    ${missing.length === 0 && orphaned.length > 0 ? `
                    <p class="repair-orphan-note">
                        <sl-icon name="info-circle"></sl-icon>
                        ${t('repair.orphan_note')}
                    </p>` : ''}
                    <div class="repair-item-list" id="repair-orphaned-list">
                        ${orphaned.length === 0 ? `<div class="repair-no-items">${t('repair.no_orphaned')}</div>` :
                orphaned.map(item => this.renderOrphanedItem(item, missing.length > 0)).join('')}
                    </div>
                </div>
            </div>
            <div class="repair-actions">
                <sl-button id="repair-prune-all-btn" variant="danger" size="small"
                           ${missing.length === 0 ? 'disabled' : ''}>
                    <sl-icon slot="prefix" name="trash"></sl-icon>
                    ${t('repair.prune_all', { count: missing.length })}
                </sl-button>
                <sl-button id="repair-done-btn" variant="primary" size="small">
                    <sl-icon slot="prefix" name="check"></sl-icon>
                    ${t('repair.finish_clear_dirty')}
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
            <div class="repair-item" data-id="${escapeAttr(item.jellyfinId)}" data-path="${escapeAttr(item.localPath)}">
                <div class="repair-item-info">
                    <span class="repair-item-name" title="${escapeAttr(item.name)}">${escapeHtml(item.name)}</span>
                    <span class="repair-item-path" title="${escapeAttr(item.localPath)}">${escapeHtml(folder)}/<strong>${escapeHtml(filename)}</strong></span>
                </div>
                <div class="repair-item-actions">
                    <sl-button class="prune-single-btn" size="small" variant="danger" outline
                               data-id="${escapeAttr(item.jellyfinId)}"
                               title="${escapeAttr(t('repair.remove_from_manifest'))}">
                        <sl-icon name="trash" slot="prefix"></sl-icon>
                        ${t('repair.prune')}
                    </sl-button>
                </div>
            </div>
        `;
    }

    private renderOrphanedItem(item: DiscrepancyItem, hasMissing: boolean): string {
        const pathParts = item.localPath.split('/');
        const filename = pathParts[pathParts.length - 1];
        const folder = pathParts.slice(0, -1).join('/');
        const relinkTitle = escapeAttr(hasMissing
            ? t('repair.relink_title')
            : t('repair.relink_unavailable'));

        return `
            <div class="repair-item orphaned" data-path="${escapeAttr(item.localPath)}">
                <div class="repair-item-info">
                    <span class="repair-item-name" title="${escapeAttr(item.name)}">${escapeHtml(item.name)}</span>
                    <span class="repair-item-path" title="${escapeAttr(item.localPath)}">${escapeHtml(folder)}/<strong>${escapeHtml(filename)}</strong></span>
                </div>
                <div class="repair-item-actions">
                    <sl-button class="relink-btn" size="small" variant="warning" outline
                               data-path="${escapeAttr(item.localPath)}"
                               ${!hasMissing ? 'disabled' : ''}
                               title="${relinkTitle}">
                        <sl-icon name="link-45deg" slot="prefix"></sl-icon>
                        ${t('repair.relink')}
                    </sl-button>
                </div>
            </div>
        `;
    }

    private renderError(message: string) {
        const body = this.dialog?.querySelector('.repair-body');
        if (!body) return;

        body.innerHTML = `
            <div class="repair-error" role="alert">
                <sl-icon name="exclamation-triangle-fill" style="font-size: 2.5rem; color: var(--sl-color-danger-500);"></sl-icon>
                <p>${t('repair.scan_failed', { message: escapeHtml(message) })}</p>
                <sl-button id="repair-retry-btn" variant="primary" size="small">${t('repair.retry')}</sl-button>
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
        if (this.busy) return;
        this.setBusy(true);
        try {
            await rpcCall('manifest_prune', { itemIds: ids });
            // Refresh discrepancies
            await this.scan();
        } catch (err) {
            console.error('[Repair] Prune failed:', err);
            showToast(t('repair.prune_failed', { message: this.errorMessage(err) }), 'danger', ERROR_TOAST_DURATION);
        } finally {
            this.setBusy(false);
        }
    }

    private async handleRelinkPrompt(orphanPath: string) {
        if (this.busy || !this.discrepancies || this.discrepancies.missing.length === 0) {
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
            <sl-select class="relink-select" size="small" hoist label="${escapeAttr(t('repair.link_to'))}" placeholder="${escapeAttr(t('repair.link_to'))}">
                ${this.discrepancies.missing.map(m =>
            `<sl-option value="${escapeAttr(m.jellyfinId)}">${escapeHtml(m.name)}</sl-option>`
        ).join('')}
            </sl-select>
        `;

        const select = actionsDiv.querySelector('.relink-select') as HTMLSelectElement & { updateComplete: Promise<boolean> };
        // Wait for Shoelace to create the internal combobox before moving focus.
        await select.updateComplete;
        if (!select.isConnected) return;
        select.focus();
        select.addEventListener('sl-change', async () => {
            const selectedId = select.value;
            if (selectedId) {
                await this.handleRelink(selectedId, orphanPath);
            }
        });
    }

    private async handleRelink(jellyfinId: string, newLocalPath: string) {
        if (this.busy) return;
        this.setBusy(true);
        try {
            await rpcCall('manifest_relink', { jellyfinId, newLocalPath });
            // Refresh discrepancies
            await this.scan();
        } catch (err) {
            console.error('[Repair] Relink failed:', err);
            showToast(t('repair.relink_failed', { message: this.errorMessage(err) }), 'danger', ERROR_TOAST_DURATION);
        } finally {
            this.setBusy(false);
        }
    }

    private async handleClearDirty() {
        if (this.busy) return;
        this.setBusy(true);
        try {
            await rpcCall('manifest_clear_dirty');
            const dialog = this.dialog as any;
            if (dialog) dialog.hide();
            this.onComplete?.();
        } catch (err) {
            console.error('[Repair] Clear dirty failed:', err);
            showToast(t('repair.clear_dirty_failed', { message: this.errorMessage(err) }), 'danger', ERROR_TOAST_DURATION);
        } finally {
            this.setBusy(false);
        }
    }
}
