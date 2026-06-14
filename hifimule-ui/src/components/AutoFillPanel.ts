// Story 12.6: Auto-Fill pipeline-builder configuration panel.
//
// A modal dialog, scoped to a single server, that edits an in-memory `AutoFillPipeline` and on
// confirm hands the produced pipeline back to the caller (which persists it via
// `autoFill.setPipeline`). The default (collapsed) view exposes only the one-click essentials —
// an enable toggle, a single size budget, and (capability permitting) a genre exclude. Everything
// else (multi-source blending, ordering, unit, memory, duration/headroom budget, fallback) lives
// behind an "Advanced" disclosure so the simple path stays one-click (AC6, AC7).

import { t } from '../i18n';
import type { BrowseMode, BrowsePlaylist } from '../rpc';
import { previewAutoFill } from '../rpc';
import {
    AutoFillPipeline,
    OrderingKey,
    ORDERING_KEYS,
    SourceEntry,
    SourceKind,
    TierDef,
    Unit,
    normalizePipeline,
    serializePipeline,
} from '../state/autoFill';

const GB = 1024 * 1024 * 1024;
const ALL_SOURCE_KINDS: SourceKind[] = ['library', 'favorites', 'history', 'playlist'];
const UNITS: Unit[] = ['track', 'album', 'artist'];

function escapeHtml(s: string): string {
    return s
        .replace(/&/g, '&amp;')
        .replace(/</g, '&lt;')
        .replace(/>/g, '&gt;')
        .replace(/"/g, '&quot;')
        .replace(/'/g, '&#39;');
}

export interface AutoFillPanelOptions {
    serverId: string;
    serverLabel: string;
    pipeline: Partial<AutoFillPipeline> | null;
    /** Browse modes advertised by the selected server's provider — gates genre + playlist UI. */
    modes: BrowseMode[];
    /** Playlists for the playlist-source picker (empty when `playlists` mode unsupported). */
    playlists: BrowsePlaylist[];
    onSave: (pipeline: AutoFillPipeline) => void;
    /** Manual (non-slot) item ids for this server — passed to the preview as `excludeItemIds` so it
     * dedups against manual selections exactly as sync-time fill does (Story 12.7). */
    excludeItemIds?: string[];
    /** Capacity available for the fill (free − manual), derived identically to the slot-card readout.
     * The preview caps its `maxBytes` by this so it never overstates beyond real free space.
     * `undefined` when no device capacity is known — the daemon then falls back to device free bytes. */
    availableBytes?: number;
    /** The project's byte formatter (reused, not reinvented) for the preview's "~size" readout. */
    formatSize: (bytes: number) => string;
}

export class AutoFillPanel {
    private dialog: any = null;
    private pipeline: AutoFillPipeline;
    private advancedOpen = false;
    private readonly genresSupported: boolean;
    private readonly playlistsSupported: boolean;
    private readonly initialMaxBytes?: number;
    private readonly initialBudgetGbInput: string;
    private readonly initialTargetDurationSecs?: number;
    private readonly initialDurationHoursInput: string;
    private readonly initialHeadroomBytes?: number;
    private readonly initialHeadroomGbInput: string;
    private budgetGbInput: string;
    private excludeGenresInput: string;
    private cooldownInput: string;
    private durationHoursInput: string;
    private headroomGbInput: string;

    // --- Live preview state (Story 12.7) ---
    private previewResult: { count: number; bytes: number } | null = null;
    private previewError: string | null = null;
    /** Set when the fill is capped to zero free space — distinguishes "no room" from "no match". */
    private previewNoSpace = false;
    private previewLoading = false;
    private previewInFlight = false;
    private previewTimer: number | null = null;
    /** Bumped on every preview-invalidating edit; a resolving request whose generation is stale is
     * discarded so it can't repaint a count for a since-edited pipeline. */
    private previewGeneration = 0;

    constructor(private opts: AutoFillPanelOptions) {
        this.pipeline = normalizePipeline(opts.pipeline);
        this.genresSupported = opts.modes.includes('genres');
        this.playlistsSupported = opts.modes.includes('playlists');
        this.initialMaxBytes = this.pipeline.budget.maxBytes;
        this.initialBudgetGbInput = this.bytesToGbInput(this.initialMaxBytes);
        this.initialTargetDurationSecs = this.pipeline.budget.targetDurationSecs;
        this.initialDurationHoursInput = this.secondsToHoursInput(this.initialTargetDurationSecs);
        this.initialHeadroomBytes = this.pipeline.budget.headroomBytes;
        this.initialHeadroomGbInput = this.bytesToGbInput(this.initialHeadroomBytes);
        this.budgetGbInput = this.initialBudgetGbInput;
        this.excludeGenresInput = this.pipeline.filter.excludeGenres.join(', ');
        this.cooldownInput = typeof this.pipeline.memory.cooldownWeeks === 'number'
            ? String(this.pipeline.memory.cooldownWeeks)
            : '';
        this.durationHoursInput = this.initialDurationHoursInput;
        this.headroomGbInput = this.initialHeadroomGbInput;
    }

    public open(): void {
        const dialog = document.createElement('sl-dialog') as any;
        dialog.label = t('basket.autofill.configure_title', { server: this.opts.serverLabel });
        dialog.className = 'auto-fill-panel-dialog';
        this.dialog = dialog;
        document.body.appendChild(dialog);
        this.renderBody();
        dialog.addEventListener('sl-after-hide', (event: Event) => {
            if (event.target !== dialog) return;
            // Cancel a debounced preview so it can't fire (wasted RPC + detached paint) after close.
            this.cancelPreviewTimer();
            this.previewGeneration++; // discard any in-flight request resolving post-close
            dialog.remove();
        });
        dialog.show();
    }

    /** The source kinds available for selection given the provider capabilities. */
    private availableKinds(): SourceKind[] {
        return ALL_SOURCE_KINDS.filter((k) => k !== 'playlist' || this.playlistsSupported);
    }

    private renderBody(): void {
        if (!this.dialog) return;
        // A structural change (toggle/source/ordering edit, advanced disclosure) invalidates any
        // prior preview so we never show a stale count. Text-input edits clear it via their own
        // handler (they don't re-render). Bumping the generation also discards any in-flight
        // request so its result can't repaint a count for this since-edited pipeline.
        this.previewGeneration++;
        this.cancelPreviewTimer();
        this.previewResult = null;
        this.previewError = null;
        this.previewNoSpace = false;
        const p = this.pipeline;

        this.dialog.innerHTML = `
            <div class="auto-fill-panel">
                <div class="auto-fill-panel-row">
                    <sl-switch id="af-enabled" size="small" ${p.enabled ? 'checked' : ''}>
                        ${t('basket.autofill.enable')}
                    </sl-switch>
                </div>
                ${this.renderFilterStage()}

                <div class="auto-fill-advanced">
                    <div id="af-advanced-header" class="device-folders-header" role="button" tabindex="0"
                         aria-expanded="${this.advancedOpen}">
                        <sl-icon name="chevron-right" class="af-advanced-chevron${this.advancedOpen ? ' af-advanced-chevron--open' : ''}"></sl-icon>
                        <span>${t('basket.autofill.advanced')}</span>
                    </div>
                    ${this.advancedOpen ? this.renderAdvanced() : ''}
                </div>
                ${this.renderBudgetStage()}
                <div id="af-preview" class="auto-fill-preview">${this.renderPreviewContent()}</div>
            </div>
            <sl-button slot="footer" variant="default" id="af-preview-btn" ${this.previewLoading ? 'loading' : ''}>
                <sl-icon slot="prefix" name="eye"></sl-icon>
                ${t('basket.autofill.preview')}
            </sl-button>
            <sl-button slot="footer" variant="default" id="af-cancel">${t('basket.actions.cancel')}</sl-button>
            <sl-button slot="footer" variant="primary" id="af-save">
                <sl-icon slot="prefix" name="check2"></sl-icon>
                ${t('basket.actions.save')}
            </sl-button>
        `;
        this.bindEvents();
    }

    /** Renders the current preview state: loading, error, empty-result, or "~N tracks · ~size".
     * Empty string when no preview has run, so the area is invisible until first invoked. */
    private renderPreviewContent(): string {
        if (this.previewLoading) {
            return `<sl-spinner></sl-spinner> <span>${t('basket.autofill.preview_loading')}</span>`;
        }
        if (this.previewError) {
            return `<span class="auto-fill-preview-error">${escapeHtml(this.previewError)}</span>`;
        }
        if (this.previewNoSpace) {
            return `<span class="auto-fill-preview-empty">${t('basket.autofill.preview_no_space')}</span>`;
        }
        if (this.previewResult) {
            if (this.previewResult.count === 0) {
                return `<span class="auto-fill-preview-empty">${t('basket.autofill.preview_empty')}</span>`;
            }
            return `<span class="auto-fill-preview-result">${t('basket.autofill.preview_result', {
                count: this.previewResult.count,
                size: this.opts.formatSize(this.previewResult.bytes),
            })}</span>`;
        }
        return '';
    }

    /** Repaints just the preview area + button loading state without a full re-render, so a
     * resolved preview survives (a full renderBody would clear `previewResult`). */
    private updatePreviewUi(): void {
        const el = this.dialog?.querySelector('#af-preview');
        if (el) el.innerHTML = this.renderPreviewContent();
        const btn = this.dialog?.querySelector('#af-preview-btn') as any;
        if (btn) btn.loading = this.previewLoading;
    }

    private renderStage(label: string, body: string): string {
        return `
            <details class="auto-fill-stage" open>
                <summary class="auto-fill-stage-label">${label}</summary>
                <div class="auto-fill-stage-body">${body}</div>
            </details>
        `;
    }

    private renderFilterStage(): string {
        if (!this.genresSupported) return '';
        return this.renderStage(t('basket.autofill.filter'), `
            <sl-input id="af-exclude-genres" clearable
                label="${t('basket.autofill.exclude_genres')}"
                help-text="${t('basket.autofill.genres_hint')}"
                value="${escapeHtml(this.excludeGenresInput)}"></sl-input>
        `);
    }

    private renderBudgetStage(): string {
        return this.renderStage(t('basket.autofill.budget_advanced'), `
            <sl-input id="af-budget-gb" type="number" min="0" step="any" clearable
                label="${t('basket.autofill.size_budget')}"
                help-text="${t('basket.autofill.size_budget_hint')}"
                value="${escapeHtml(this.budgetGbInput)}"></sl-input>
            ${this.advancedOpen ? `
                <sl-input id="af-duration-hours" type="number" min="0" step="0.5" clearable
                    label="${t('basket.autofill.target_duration_hours')}"
                    value="${escapeHtml(this.durationHoursInput)}"></sl-input>
                <sl-input id="af-headroom-gb" type="number" min="0" step="any" clearable
                    label="${t('basket.autofill.headroom_gb')}"
                    value="${escapeHtml(this.headroomGbInput)}"></sl-input>
            ` : ''}
        `);
    }

    private renderAdvanced(): string {
        return `
            <div class="auto-fill-advanced-body">
                ${this.renderSourcesStage()}
                ${this.renderStage(t('basket.autofill.unit'), `
                    <sl-select id="af-unit" size="small" value="${this.pipeline.unit}">
                        ${UNITS.map((u) => `<sl-option value="${u}">${t('basket.autofill.unit_' + u)}</sl-option>`).join('')}
                    </sl-select>
                `)}
                ${this.renderOrderingSection()}
                ${this.renderMemoryStage()}
            </div>
        `;
    }

    /** Memory stage controls: cooldown + played-exclusion (existing), plus the Story 13.1 stable-core
     * %, repeat-tolerance dial, and rotation-tiers editor. */
    private renderMemoryStage(): string {
        const mem = this.pipeline.memory;
        const corePct = typeof mem.stableCorePct === 'number' ? Math.round(mem.stableCorePct * 100) : 0;
        const tolPct = typeof mem.repeatTolerance === 'number' ? Math.round(mem.repeatTolerance * 100) : 0;
        return this.renderStage(t('basket.autofill.memory'), `
            <sl-input id="af-cooldown" type="number" min="0" step="1" clearable
                label="${t('basket.autofill.cooldown_weeks')}"
                value="${escapeHtml(this.cooldownInput)}"></sl-input>
            <sl-switch id="af-played-exclusion" size="small" ${mem.playedExclusion ? 'checked' : ''}>
                ${t('basket.autofill.played_exclusion')}
            </sl-switch>
            <div class="auto-fill-memory-dial">
                <label class="auto-fill-substage-label">${t('basket.autofill.stable_core_pct')}</label>
                <div class="af-share-cell">
                    <sl-range id="af-stable-core" min="0" max="100" step="5" value="${corePct}"></sl-range>
                    <span class="af-share-value">${corePct}%</span>
                </div>
                <div class="auto-fill-caption">${t('basket.autofill.stable_core_pct_hint')}</div>
            </div>
            <div class="auto-fill-memory-dial">
                <label class="auto-fill-substage-label">${t('basket.autofill.repeat_tolerance')}</label>
                <div class="af-share-cell">
                    <sl-range id="af-repeat-tolerance" min="0" max="100" step="5" value="${tolPct}"></sl-range>
                    <span class="af-share-value">${tolPct}%</span>
                </div>
                <div class="auto-fill-caption">${t('basket.autofill.repeat_tolerance_hint')}</div>
            </div>
            ${this.renderTiersEditor()}
        `);
    }

    /** Ordered rotation-tiers editor (Story 13.1): add/remove playlist-backed or library tiers. */
    private renderTiersEditor(): string {
        const tiers = this.memoryTiers();
        const rows = tiers.map((tier, i) => `
            <div class="auto-fill-source-row" data-index="${i}">
                <sl-select class="af-tier-kind" size="small" value="${tier.kind}" data-index="${i}">
                    <sl-option value="library">${t('basket.autofill.source_library')}</sl-option>
                    ${this.playlistsSupported ? `<sl-option value="playlist">${t('basket.autofill.source_playlist')}</sl-option>` : ''}
                </sl-select>
                ${tier.kind === 'playlist' && this.playlistsSupported ? `
                    <sl-select class="af-tier-ref" size="small" placeholder="${t('basket.autofill.pick_playlist')}"
                        value="${escapeHtml(tier.ref ?? '')}" data-index="${i}">
                        ${this.opts.playlists.map((pl) => `<sl-option value="${escapeHtml(pl.id)}">${escapeHtml(pl.name)}</sl-option>`).join('')}
                    </sl-select>
                ` : ''}
                <sl-icon-button class="af-tier-remove" name="x" label="${t('basket.actions.remove')}" data-index="${i}"></sl-icon-button>
            </div>
        `).join('');
        return `
            <div class="auto-fill-source-list">
                <label class="auto-fill-substage-label">${t('basket.autofill.tiers')}</label>
                ${rows || `<div class="auto-fill-caption">${t('basket.autofill.tiers_hint')}</div>`}
                <sl-button class="af-tier-add" size="small" variant="text">
                    <sl-icon slot="prefix" name="plus-circle"></sl-icon>${t('basket.autofill.tiers_add')}
                </sl-button>
            </div>
        `;
    }

    /** The live tiers array (lazily initialized so editing never dereferences undefined). */
    private memoryTiers(): TierDef[] {
        if (!Array.isArray(this.pipeline.memory.tiers)) this.pipeline.memory.tiers = [];
        return this.pipeline.memory.tiers;
    }

    private renderSourcesStage(): string {
        return this.renderStage(t('basket.autofill.sources'), `
            ${this.renderSourcesList(t('basket.autofill.sources'), this.pipeline.sources, 'source')}
            ${this.renderSourcesList(t('basket.autofill.fallback'), this.pipeline.fallback, 'fallback')}
        `);
    }

    /** Renders a sources OR fallback list. `group` discriminates the two so element ids/handlers
     * don't collide. Share sliders appear only when a list holds more than one entry (AC10). */
    private renderSourcesList(label: string, list: SourceEntry[], group: 'source' | 'fallback'): string {
        const showShares = list.length > 1;
        const kinds = this.availableKinds();
        const rows = list.map((src, i) => {
            const sharePct = typeof src.share === 'number' ? Math.round(src.share * 100) : '';
            return `
                <div class="auto-fill-source-row" data-group="${group}" data-index="${i}">
                    <sl-select class="af-source-kind" size="small" value="${src.kind}" data-group="${group}" data-index="${i}">
                        ${kinds.map((k) => `<sl-option value="${k}">${t('basket.autofill.source_' + k)}</sl-option>`).join('')}
                    </sl-select>
                    ${src.kind === 'playlist' && this.playlistsSupported ? `
                        <sl-select class="af-source-ref" size="small" placeholder="${t('basket.autofill.pick_playlist')}"
                            value="${escapeHtml(src.ref ?? '')}" data-group="${group}" data-index="${i}">
                            ${this.opts.playlists.map((pl) => `<sl-option value="${escapeHtml(pl.id)}">${escapeHtml(pl.name)}</sl-option>`).join('')}
                        </sl-select>
                    ` : ''}
                    ${showShares ? `
                        <div class="af-share-cell">
                            <sl-range class="af-source-share" min="0" max="100" step="5"
                                value="${sharePct === '' ? 0 : sharePct}" data-group="${group}" data-index="${i}"></sl-range>
                            <span class="af-share-value">${sharePct === '' ? '—' : sharePct + '%'}</span>
                        </div>
                    ` : ''}
                    <sl-icon-button class="af-source-remove" name="x" label="${t('basket.actions.remove')}"
                        data-group="${group}" data-index="${i}"></sl-icon-button>
                </div>
            `;
        }).join('');
        return `
            <div class="auto-fill-source-list">
                <label class="auto-fill-substage-label">${label}</label>
                ${rows || `<div class="auto-fill-caption">${t('basket.autofill.no_sources')}</div>`}
                <sl-button class="af-source-add" size="small" variant="text" data-group="${group}">
                    <sl-icon slot="prefix" name="plus-circle"></sl-icon>${t('basket.autofill.add_source')}
                </sl-button>
            </div>
        `;
    }

    private renderOrderingSection(): string {
        const used = this.pipeline.ordering;
        const unused = ORDERING_KEYS.filter((k) => !used.includes(k));
        const rows = used.map((key, i) => `
            <div class="auto-fill-ordering-row" data-index="${i}">
                <span class="af-ordering-label">${i + 1}. ${t('basket.autofill.ordering_' + key)}</span>
                <sl-icon-button class="af-ordering-up" name="chevron-up" label="${t('basket.autofill.move_up')}" data-index="${i}" ${i === 0 ? 'disabled' : ''}></sl-icon-button>
                <sl-icon-button class="af-ordering-down" name="chevron-down" label="${t('basket.autofill.move_down')}" data-index="${i}" ${i === used.length - 1 ? 'disabled' : ''}></sl-icon-button>
                <sl-icon-button class="af-ordering-remove" name="x" label="${t('basket.actions.remove')}" data-index="${i}"></sl-icon-button>
            </div>
        `).join('');
        return this.renderStage(t('basket.autofill.ordering'), `
            ${rows || `<div class="auto-fill-caption">${t('basket.autofill.no_ordering')}</div>`}
            ${unused.length > 0 ? `
                <sl-select id="af-ordering-add" size="small" placeholder="${t('basket.autofill.add_ordering')}" value="">
                    ${unused.map((k) => `<sl-option value="${k}">${t('basket.autofill.ordering_' + k)}</sl-option>`).join('')}
                </sl-select>
            ` : ''}
        `);
    }

    private listFor(group: 'source' | 'fallback'): SourceEntry[] {
        return group === 'source' ? this.pipeline.sources : this.pipeline.fallback;
    }

    private bindEvents(): void {
        const d = this.dialog;
        // --- Default-view controls (mutate live) ---
        d.querySelector('#af-enabled')?.addEventListener('sl-change', (e: Event) => {
            this.pipeline.enabled = (e.target as HTMLInputElement).checked;
        });
        this.bindTextState('#af-budget-gb', (value) => { this.budgetGbInput = value; });
        this.bindTextState('#af-exclude-genres', (value) => { this.excludeGenresInput = value; });
        this.bindTextState('#af-cooldown', (value) => { this.cooldownInput = value; });
        this.bindTextState('#af-duration-hours', (value) => { this.durationHoursInput = value; });
        this.bindTextState('#af-headroom-gb', (value) => { this.headroomGbInput = value; });

        // --- Advanced disclosure toggle ---
        d.querySelector('#af-advanced-header')?.addEventListener('click', () => {
            this.captureInputs();
            this.advancedOpen = !this.advancedOpen;
            this.renderBody();
        });

        // --- Unit ---
        d.querySelector('#af-unit')?.addEventListener('sl-change', (e: Event) => {
            this.pipeline.unit = (e.target as any).value as Unit;
        });
        d.querySelector('#af-played-exclusion')?.addEventListener('sl-change', (e: Event) => {
            this.pipeline.memory.playedExclusion = (e.target as HTMLInputElement).checked;
        });

        // --- Memory: stable-core %, repeat-tolerance dial, rotation tiers (Story 13.1) ---
        d.querySelector('#af-stable-core')?.addEventListener('sl-change', (e: Event) => {
            this.captureInputs();
            const pct = Number((e.target as any).value);
            this.pipeline.memory.stableCorePct = isNaN(pct) || pct <= 0 ? undefined : pct / 100;
            this.renderBody(); // refresh the % readout
        });
        d.querySelector('#af-repeat-tolerance')?.addEventListener('sl-change', (e: Event) => {
            this.captureInputs();
            const pct = Number((e.target as any).value);
            this.pipeline.memory.repeatTolerance = isNaN(pct) || pct <= 0 ? undefined : pct / 100;
            this.renderBody();
        });
        d.querySelectorAll('.af-tier-kind').forEach((el: Element) => {
            el.addEventListener('sl-change', (e: Event) => {
                this.captureInputs();
                const i = Number((e.target as HTMLElement).dataset.index ?? '0');
                const kind = (e.target as any).value as 'library' | 'playlist';
                this.memoryTiers()[i] = kind === 'playlist' ? { kind: 'playlist', ref: '' } : { kind: 'library' };
                this.renderBody(); // a playlist kind reveals the ref picker
            });
        });
        d.querySelectorAll('.af-tier-ref').forEach((el: Element) => {
            el.addEventListener('sl-change', (e: Event) => {
                const i = Number((e.target as HTMLElement).dataset.index ?? '0');
                const tier = this.memoryTiers()[i];
                if (tier && tier.kind === 'playlist') tier.ref = (e.target as any).value || '';
                this.invalidatePreview();
            });
        });
        d.querySelectorAll('.af-tier-remove').forEach((el: Element) => {
            el.addEventListener('click', (e: Event) => {
                this.captureInputs();
                const i = Number((e.currentTarget as HTMLElement).dataset.index ?? '0');
                this.memoryTiers().splice(i, 1);
                this.renderBody();
            });
        });
        d.querySelector('.af-tier-add')?.addEventListener('click', () => {
            this.captureInputs();
            this.memoryTiers().push(this.playlistsSupported ? { kind: 'playlist', ref: '' } : { kind: 'library' });
            this.renderBody();
        });

        // --- Sources / fallback ---
        d.querySelectorAll('.af-source-kind').forEach((el: Element) => {
            el.addEventListener('sl-change', (e: Event) => {
                this.captureInputs();
                const { group, index } = this.rowRef(e.target as HTMLElement);
                this.listFor(group)[index].kind = (e.target as any).value as SourceKind;
                this.renderBody(); // a playlist kind reveals the ref picker
            });
        });
        d.querySelectorAll('.af-source-ref').forEach((el: Element) => {
            el.addEventListener('sl-change', (e: Event) => {
                const { group, index } = this.rowRef(e.target as HTMLElement);
                this.listFor(group)[index].ref = (e.target as any).value || undefined;
            });
        });
        d.querySelectorAll('.af-source-share').forEach((el: Element) => {
            el.addEventListener('sl-change', (e: Event) => {
                this.captureInputs();
                const { group, index } = this.rowRef(e.target as HTMLElement);
                const pct = Number((e.target as any).value);
                this.listFor(group)[index].share = isNaN(pct) ? undefined : pct / 100;
                this.renderBody(); // refresh the % readout
            });
        });
        d.querySelectorAll('.af-source-remove').forEach((el: Element) => {
            el.addEventListener('click', (e: Event) => {
                this.captureInputs();
                const { group, index } = this.rowRef(e.currentTarget as HTMLElement);
                this.listFor(group).splice(index, 1);
                this.renderBody();
            });
        });
        d.querySelectorAll('.af-source-add').forEach((el: Element) => {
            el.addEventListener('click', (e: Event) => {
                this.captureInputs();
                const group = (e.currentTarget as HTMLElement).dataset.group as 'source' | 'fallback';
                this.listFor(group).push({ kind: 'library' });
                this.renderBody();
            });
        });

        // --- Ordering ---
        d.querySelectorAll('.af-ordering-up').forEach((el: Element) => {
            el.addEventListener('click', (e: Event) => {
                this.captureInputs();
                this.moveOrdering(this.idxOf(e), -1);
            });
        });
        d.querySelectorAll('.af-ordering-down').forEach((el: Element) => {
            el.addEventListener('click', (e: Event) => {
                this.captureInputs();
                this.moveOrdering(this.idxOf(e), 1);
            });
        });
        d.querySelectorAll('.af-ordering-remove').forEach((el: Element) => {
            el.addEventListener('click', (e: Event) => {
                this.captureInputs();
                this.pipeline.ordering.splice(this.idxOf(e), 1);
                this.renderBody();
            });
        });
        d.querySelector('#af-ordering-add')?.addEventListener('sl-change', (e: Event) => {
            this.captureInputs();
            const key = (e.target as any).value as OrderingKey;
            if (key && !this.pipeline.ordering.includes(key)) this.pipeline.ordering.push(key);
            this.renderBody();
        });

        // --- Footer ---
        d.querySelector('#af-preview-btn')?.addEventListener('click', () => this.onPreviewClick());
        d.querySelector('#af-cancel')?.addEventListener('click', () => d.hide());
        d.querySelector('#af-save')?.addEventListener('click', () => this.handleSave());
    }

    private bindTextState(selector: string, update: (value: string) => void): void {
        const el = this.dialog.querySelector(selector);
        const listener = (e: Event) => {
            update(String((e.target as any).value ?? ''));
            // Edits invalidate any shown preview (text inputs don't re-render the whole body).
            this.invalidatePreview();
        };
        el?.addEventListener('sl-input', listener);
        el?.addEventListener('sl-change', listener);
    }

    /** Clears a shown preview and discards any pending/in-flight request (used when an input changes
     * without a full re-render). Bumping the generation makes a resolving request no-op rather than
     * repaint a count for the now-edited pipeline; we also drop the loading state so the area resets. */
    private invalidatePreview(): void {
        this.previewGeneration++;
        this.cancelPreviewTimer();
        const wasActive = this.previewLoading || this.previewInFlight;
        if (!wasActive && !this.previewResult && !this.previewError && !this.previewNoSpace) return;
        this.previewLoading = false;
        this.previewResult = null;
        this.previewError = null;
        this.previewNoSpace = false;
        this.updatePreviewUi();
    }

    /** Cancels a pending debounced preview so it can't fire after the dialog closes or the config
     * changes (a fired `runPreview` against a closed dialog would do a wasted RPC + detached paint). */
    private cancelPreviewTimer(): void {
        if (this.previewTimer !== null) {
            clearTimeout(this.previewTimer);
            this.previewTimer = null;
        }
    }

    private captureInputs(): void {
        this.budgetGbInput = this.readInputValue('#af-budget-gb') ?? this.budgetGbInput;
        this.excludeGenresInput = this.readInputValue('#af-exclude-genres') ?? this.excludeGenresInput;
        this.cooldownInput = this.readInputValue('#af-cooldown') ?? this.cooldownInput;
        this.durationHoursInput = this.readInputValue('#af-duration-hours') ?? this.durationHoursInput;
        this.headroomGbInput = this.readInputValue('#af-headroom-gb') ?? this.headroomGbInput;
    }

    private readInputValue(selector: string): string | null {
        const el = this.dialog?.querySelector(selector) as any;
        return el ? String(el.value ?? '') : null;
    }

    private rowRef(el: HTMLElement): { group: 'source' | 'fallback'; index: number } {
        return {
            group: (el.dataset.group as 'source' | 'fallback') ?? 'source',
            index: Number(el.dataset.index ?? '0'),
        };
    }

    private idxOf(e: Event): number {
        return Number((e.currentTarget as HTMLElement).dataset.index ?? '0');
    }

    private moveOrdering(index: number, delta: number): void {
        const target = index + delta;
        const list = this.pipeline.ordering;
        if (target < 0 || target >= list.length) return;
        [list[index], list[target]] = [list[target], list[index]];
        this.renderBody();
    }

    /** Reads the free-text/number inputs into the in-memory model and returns the serialized
     * pipeline. Shared by Save (which persists the result) and Preview (which does not) so the two
     * can never diverge — the preview always reflects the exact config a Save would write. */
    private buildPipeline(): AutoFillPipeline {
        this.captureInputs();
        // Budget (GB → bytes); empty clears the ceiling.
        this.pipeline.budget.maxBytes = this.bytesFromGbInput(
            this.budgetGbInput,
            this.initialBudgetGbInput,
            this.initialMaxBytes,
        );

        // Genre exclude (comma-separated). Only meaningful when the provider supports genres.
        if (this.genresSupported) {
            this.pipeline.filter.excludeGenres = this.excludeGenresInput
                .split(',')
                .map((s) => s.trim())
                .filter((s) => s.length > 0);
        }

        if (this.advancedOpen) {
            const cooldown = this.numberFromInput(this.cooldownInput);
            this.pipeline.memory.cooldownWeeks = cooldown != null && cooldown > 0 ? Math.round(cooldown) : undefined;
            // 12.5 lesson: emit null/omit rather than 0 so a cleared field reads as "unset", not an
            // inert empty fill. serializePipeline drops 0/undefined for duration & headroom.
            this.pipeline.budget.targetDurationSecs = this.secondsFromHoursInput(
                this.durationHoursInput,
                this.initialDurationHoursInput,
                this.initialTargetDurationSecs,
            );
            this.pipeline.budget.headroomBytes = this.bytesFromGbInput(
                this.headroomGbInput,
                this.initialHeadroomGbInput,
                this.initialHeadroomBytes,
            );
        }

        return serializePipeline(this.pipeline);
    }

    /** Reads the free-text/number inputs into the model, then serializes and hands back. */
    private handleSave(): void {
        const out = this.buildPipeline();
        this.opts.onSave(out);
        this.dialog.hide();
    }

    /** Preview entry point (AC2): explicit, debounced (≥300 ms), never fired on open/edit/toggle.
     * A request already in flight is ignored (the button is in its loading state). */
    private onPreviewClick(): void {
        if (this.previewInFlight) return;
        this.cancelPreviewTimer();
        this.previewLoading = true;
        this.previewError = null;
        this.updatePreviewUi();
        this.previewTimer = window.setTimeout(() => { void this.runPreview(); }, 300);
    }

    /** Runs the preview through the shared `basket.autoFill`+serverId seam using the current unsaved
     * pipeline (via `buildPipeline()`), the per-server manual exclude ids, and a capacity-capped
     * `maxBytes`. Surfaces all outcomes (result / empty / error) and always clears loading. */
    private async runPreview(): Promise<void> {
        this.previewTimer = null;
        this.previewLoading = true;
        this.previewInFlight = true;
        const generation = this.previewGeneration;
        try {
            const pipeline = this.buildPipeline();
            // A disabled pipeline contributes nothing at sync time — mirror that instead of querying
            // the provider, so the preview can never imply a disabled pipeline would fill tracks.
            if (!this.pipeline.enabled) {
                this.previewResult = { count: 0, bytes: 0 };
                this.previewError = null;
                this.previewNoSpace = false;
                return;
            }
            // Zero capacity (device full once manual selections are subtracted) is a space problem,
            // not a filter miss — surface it distinctly rather than as "no tracks match".
            const maxBytes = this.previewMaxBytes(pipeline);
            if (maxBytes === 0) {
                this.previewResult = null;
                this.previewError = null;
                this.previewNoSpace = true;
                return;
            }
            const items = await previewAutoFill({
                serverId: this.opts.serverId,
                pipeline,
                excludeItemIds: this.opts.excludeItemIds,
                maxBytes,
            });
            if (generation !== this.previewGeneration) return; // superseded by a later edit
            const bytes = items.reduce((sum, i) => sum + (i.sizeBytes ?? 0), 0);
            this.previewResult = { count: items.length, bytes };
            this.previewError = null;
            this.previewNoSpace = false;
        } catch {
            if (generation !== this.previewGeneration) return; // superseded — don't surface a stale error
            this.previewResult = null;
            this.previewNoSpace = false;
            this.previewError = t('basket.autofill.preview_error');
            window.dispatchEvent(new CustomEvent('toast', {
                detail: { type: 'error', message: t('basket.autofill.preview_error') },
            }));
        } finally {
            // Always release the in-flight latch, even when stale, so later previews aren't blocked.
            this.previewInFlight = false;
            if (generation === this.previewGeneration) {
                this.previewLoading = false;
                this.updatePreviewUi();
            }
        }
    }

    /** The capacity-capped byte ceiling for the preview, mirroring the slot-card readout
     * (`slotSizeBytes`): cap the pipeline's budget by real available capacity, else use all of it.
     * `undefined` available → send the pipeline budget (or let the daemon default to device free). */
    private previewMaxBytes(pipeline: AutoFillPipeline): number | undefined {
        const max = pipeline.budget.maxBytes;
        const available = this.opts.availableBytes;
        if (available == null) return max;
        return typeof max === 'number' ? Math.min(max, available) : available;
    }

    private numberFromInput(raw: string): number | null {
        if (raw === '' || raw == null) return null;
        const n = Number(raw);
        return isNaN(n) || n < 0 ? null : n;
    }

    private bytesFromGbInput(raw: string, initialRaw: string, initialBytes?: number): number | undefined {
        if (raw === initialRaw && typeof initialBytes === 'number') return initialBytes;
        const gb = this.numberFromInput(raw);
        return gb != null && gb > 0 ? Math.round(gb * GB) : undefined;
    }

    private secondsFromHoursInput(raw: string, initialRaw: string, initialSeconds?: number): number | undefined {
        if (raw === initialRaw && typeof initialSeconds === 'number') return initialSeconds;
        const hours = this.numberFromInput(raw);
        return hours != null && hours > 0 ? Math.round(hours * 3600) : undefined;
    }

    private bytesToGbInput(bytes?: number): string {
        return typeof bytes === 'number' && bytes > 0 ? String(bytes / GB) : '';
    }

    private secondsToHoursInput(seconds?: number): string {
        return typeof seconds === 'number' && seconds > 0 ? String(seconds / 3600) : '';
    }
}
