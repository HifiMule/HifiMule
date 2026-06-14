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
import {
    AutoFillPipeline,
    OrderingKey,
    ORDERING_KEYS,
    SourceEntry,
    SourceKind,
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
            if (event.target === dialog) dialog.remove();
        });
        dialog.show();
    }

    /** The source kinds available for selection given the provider capabilities. */
    private availableKinds(): SourceKind[] {
        return ALL_SOURCE_KINDS.filter((k) => k !== 'playlist' || this.playlistsSupported);
    }

    private renderBody(): void {
        if (!this.dialog) return;
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
            </div>
            <sl-button slot="footer" variant="default" id="af-cancel">${t('basket.actions.cancel')}</sl-button>
            <sl-button slot="footer" variant="primary" id="af-save">
                <sl-icon slot="prefix" name="check2"></sl-icon>
                ${t('basket.actions.save')}
            </sl-button>
        `;
        this.bindEvents();
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
                ${this.renderStage(t('basket.autofill.memory'), `
                    <sl-input id="af-cooldown" type="number" min="0" step="1" clearable
                        label="${t('basket.autofill.cooldown_weeks')}"
                        value="${escapeHtml(this.cooldownInput)}"></sl-input>
                    <sl-switch id="af-played-exclusion" size="small" ${this.pipeline.memory.playedExclusion ? 'checked' : ''}>
                        ${t('basket.autofill.played_exclusion')}
                    </sl-switch>
                `)}
            </div>
        `;
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
        d.querySelector('#af-cancel')?.addEventListener('click', () => d.hide());
        d.querySelector('#af-save')?.addEventListener('click', () => this.handleSave());
    }

    private bindTextState(selector: string, update: (value: string) => void): void {
        const el = this.dialog.querySelector(selector);
        const listener = (e: Event) => update(String((e.target as any).value ?? ''));
        el?.addEventListener('sl-input', listener);
        el?.addEventListener('sl-change', listener);
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

    /** Reads the free-text/number inputs into the model, then serializes and hands back. */
    private handleSave(): void {
        const d = this.dialog;
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

        const out = serializePipeline(this.pipeline);
        this.opts.onSave(out);
        d.hide();
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
