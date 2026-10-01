import {
    playbackCancelSelectionStart, playbackGetSelectionConfig, playbackSaveSelectionConfig,
    playbackSelectionOptions, playbackStartSelection, serverList,
    type PlaybackSelectionConfig, type PlaybackSelectionKind,
    type PlaybackSelectionOption, type PlaybackSelectionOrdering, type PlaybackSelectionSource,
    type ServerSummary,
} from '../rpc';
import { t } from '../i18n';
import { formatServerIdentity } from '../serverIdentity';

const kinds: PlaybackSelectionKind[] = ['playlist', 'artist', 'genre'];
const orderings: PlaybackSelectionOrdering[] = ['random', 'favorite', 'playCount', 'dateCreated', 'quality', 'excavation', 'rediscovery', 'rarity'];

export class PlaybackSelectionSettings {
    readonly element = document.createElement('section');
    private readonly rows = document.createElement('div');
    private readonly status = document.createElement('p');
    private readonly add = document.createElement('button');
    private readonly save = document.createElement('button');
    private readonly start = document.createElement('button');
    private readonly cancel = document.createElement('button');
    private readonly ordering = document.createElement('select');
    private readonly seed = document.createElement('input');
    private servers: ServerSummary[] = [];
    private config: PlaybackSelectionConfig = { schemaVersion: 1, sources: [], ordering: ['random'], seed: 0, maxTracks: 16 };
    private disposed = false;
    private busy = false;
    private cancelling = false;
    private saving = false;
    private dirty = false;
    private editRevision = 0;
    private request = 0;
    private optionRequests = new WeakMap<HTMLInputElement, number>();
    private searchTimers = new Set<ReturnType<typeof setTimeout>>();
    private static nextPickerId = 0;

    constructor() {
        this.element.className = 'playback-selection-settings';
        const heading = document.createElement('h2');
        heading.textContent = t('playback.selection.title');
        heading.tabIndex = -1;
        const description = document.createElement('p');
        description.textContent = t('playback.selection.description');
        this.rows.className = 'playback-selection-settings__rows';
        this.status.setAttribute('role', 'status');
        this.status.setAttribute('aria-live', 'polite');
        this.add.type = this.save.type = this.start.type = this.cancel.type = 'button';
        this.add.textContent = t('playback.selection.add');
        this.save.textContent = t('playback.selection.save');
        this.start.textContent = t('playback.selection.start');
        this.cancel.textContent = t('playback.selection.cancel');
        this.seed.type = 'number'; this.seed.min = '0'; this.seed.max = '4294967295'; this.seed.step = '1';
        this.seed.addEventListener('input', () => this.markDirty());
        for (const key of orderings) this.ordering.add(new Option(t(`playback.selection.order.${key}`), key));
        this.ordering.addEventListener('change', () => this.markDirty());
        this.add.addEventListener('click', () => this.addSource());
        this.save.addEventListener('click', () => void this.saveConfig());
        this.start.addEventListener('click', () => void this.startSelection());
        this.cancel.addEventListener('click', () => void this.cancelStart());
        const controls = document.createElement('div'); controls.className = 'playback-selection-settings__controls';
        controls.append(this.add, this.save, this.start, this.cancel);
        this.element.append(heading, description, this.rows,
            this.label('playback.selection.order', this.ordering),
            this.label('playback.selection.seed', this.seed), controls, this.status);
        this.updateActions();
        void this.load();
    }

    destroy(): void {
        this.disposed = true;
        ++this.request;
        this.clearSearchTimers();
        if (this.busy) void playbackCancelSelectionStart().catch(() => undefined);
    }

    focus(): void {
        this.element.querySelector<HTMLElement>('h2')?.focus();
    }

    private label(key: string, input: HTMLElement): HTMLLabelElement {
        const label = document.createElement('label');
        label.textContent = t(key);
        label.append(input);
        return label;
    }

    private async load(): Promise<void> {
        try {
            const [config, servers] = await Promise.all([playbackGetSelectionConfig(), serverList()]);
            if (this.disposed) return;
            this.config = config;
            this.servers = servers.filter(server => Boolean(server.serverId) && !server.libraryRole);
            this.ordering.value = config.ordering[0] ?? 'random';
            this.seed.value = String(config.seed);
            this.renderRows();
            this.status.textContent = this.servers.length ? '' : t('playback.selection.no_servers');
        } catch {
            if (!this.disposed) this.status.textContent = t('playback.selection.load_failed');
        }
    }

    private markDirty(): void {
        ++this.editRevision;
        this.dirty = true;
        this.updateActions();
    }

    private updateActions(): void {
        this.add.disabled = this.busy || this.saving || this.config.sources.length >= 8 || this.servers.length === 0;
        this.save.disabled = this.busy || this.saving || !this.dirty || this.config.sources.some(source => source.kind !== 'library' && !source.ref);
        this.start.disabled = this.busy || this.cancelling || this.saving || this.dirty;
        this.cancel.disabled = !this.busy || this.cancelling;
    }

    private addSource(): void {
        const serverId = this.servers[0]?.serverId;
        if (!serverId || this.config.sources.length >= 8) return;
        this.config.sources.push({ serverId, kind: 'playlist', ref: '' });
        this.markDirty();
        this.renderRows();
    }

    private renderRows(): void {
        this.clearSearchTimers();
        this.rows.replaceChildren();
        this.config.sources.forEach((source, index) => {
            const row = document.createElement('div'); row.className = 'playback-selection-settings__row';
            const server = document.createElement('select');
            for (const item of this.servers) {
                if (item.serverId) server.add(new Option(formatServerIdentity(item).label, item.serverId));
            }
            server.value = source.serverId;
            const kind = document.createElement('select');
            for (const value of source.kind === 'library' ? ['library', ...kinds] : kinds) kind.add(new Option(t(`playback.selection.kind.${value}`), value));
            kind.value = source.kind;
            const ref = document.createElement('input'); ref.type = 'text'; ref.autocomplete = 'off';
            ref.setAttribute('role', 'combobox'); ref.setAttribute('aria-autocomplete', 'list');
            ref.setAttribute('aria-expanded', 'false');
            ref.placeholder = t('playback.selection.choose');
            const picker = document.createElement('div'); picker.className = 'playback-selection-settings__picker';
            const suggestions = document.createElement('div'); suggestions.setAttribute('role', 'listbox');
            suggestions.id = `radio-source-${++PlaybackSelectionSettings.nextPickerId}`;
            suggestions.setAttribute('aria-label', t('playback.selection.input'));
            suggestions.className = 'playback-selection-settings__suggestions'; suggestions.hidden = true;
            ref.setAttribute('aria-controls', suggestions.id);
            let choices: PlaybackSelectionOption[] = [];
            let active = -1;
            let query = '';
            let timer: ReturnType<typeof setTimeout> | undefined;
            const more = document.createElement('button'); more.type = 'button';
            more.textContent = t('playback.selection.more'); more.hidden = true;
            const explanation = document.createElement('span'); explanation.className = 'playback-selection-settings__explanation';
            const remove = document.createElement('button'); remove.type = 'button';
            remove.textContent = t('playback.selection.remove');
            remove.setAttribute('aria-label', `${t('playback.selection.remove')} ${index + 1}`);
            const show = (visible: boolean) => {
                suggestions.hidden = !visible || choices.length === 0;
                ref.setAttribute('aria-expanded', String(!suggestions.hidden));
            };
            const highlight = () => {
                Array.from(suggestions.children).forEach((item, i) => item.setAttribute('aria-selected', String(i === active)));
                ref.setAttribute('aria-activedescendant', active < 0 ? '' : `${suggestions.id}-${active}`);
                suggestions.children[active]?.scrollIntoView?.({ block: 'nearest' });
            };
            const select = (option: PlaybackSelectionOption) => {
                source.ref = option.id; ref.value = option.name; query = ''; active = -1;
                this.optionRequests.set(ref, (this.optionRequests.get(ref) ?? 0) + 1);
                if (timer) { clearTimeout(timer); this.searchTimers.delete(timer); }
                inputBusy(false); more.hidden = true;
                highlight(); show(false); this.markDirty();
            };
            const renderChoices = (options: PlaybackSelectionOption[]) => {
                choices = options; active = -1; suggestions.replaceChildren();
                const names = new Map<string, number>();
                for (const option of choices) names.set(option.name, (names.get(option.name) ?? 0) + 1);
                choices.forEach((option, i) => {
                    const item = document.createElement('button'); item.type = 'button'; item.tabIndex = -1;
                    item.id = `${suggestions.id}-${i}`; item.setAttribute('role', 'option');
                    item.textContent = (names.get(option.name) ?? 0) > 1 ? `${option.name} (${option.id})` : option.name;
                    item.addEventListener('mousedown', event => event.preventDefault());
                    item.addEventListener('click', () => { select(option); ref.focus(); });
                    suggestions.append(item);
                });
                highlight(); show(document.activeElement === ref);
            };
            const inputBusy = (busy: boolean) => ref.setAttribute('aria-busy', String(busy));
            const load = (offset = 0, resolveSelected = false) => void this.loadOptions(source, ref, explanation, more, query, offset, resolveSelected,
                options => renderChoices(offset === 0 ? options : [...choices, ...options.filter(option => !choices.some(existing => existing.id === option.id))]),
                next => load(next));
            const reset = () => {
                if (timer) { clearTimeout(timer); this.searchTimers.delete(timer); }
                source.ref = ''; ref.value = ''; query = ''; this.markDirty();
                ref.disabled = source.kind === 'library';
                if (ref.disabled) {
                    this.optionRequests.set(ref, (this.optionRequests.get(ref) ?? 0) + 1);
                    inputBusy(false); more.hidden = true; renderChoices([]);
                    explanation.textContent = '';
                    ref.value = t('playback.selection.kind.library');
                } else load();
            };
            server.addEventListener('change', () => { source.serverId = server.value; reset(); });
            kind.addEventListener('change', () => { source.kind = kind.value as PlaybackSelectionKind; reset(); });
            ref.addEventListener('input', () => {
                this.optionRequests.set(ref, (this.optionRequests.get(ref) ?? 0) + 1);
                if (timer) { clearTimeout(timer); this.searchTimers.delete(timer); }
                query = ref.value.trim(); source.ref = ''; this.markDirty();
                inputBusy(false);
                choices = []; active = -1; suggestions.replaceChildren(); highlight(); show(false); more.hidden = true;
                timer = setTimeout(() => { this.searchTimers.delete(timer!); load(); }, 250);
                this.searchTimers.add(timer);
            });
            ref.addEventListener('focus', () => show(true));
            ref.addEventListener('blur', () => show(false));
            ref.addEventListener('keydown', event => {
                if (event.key === 'Escape') { show(false); return; }
                if (event.key === 'Enter' && active >= 0 && !suggestions.hidden) {
                    event.preventDefault(); select(choices[active]); return;
                }
                if ((event.key === 'ArrowDown' || event.key === 'ArrowUp') && choices.length) {
                    event.preventDefault(); show(true);
                    active = event.key === 'ArrowDown' ? (active + 1) % choices.length : (active <= 0 ? choices.length : active) - 1;
                    highlight();
                }
            });
            remove.addEventListener('click', () => { this.config.sources.splice(index, 1); this.markDirty(); this.renderRows(); });
            picker.append(this.label('playback.selection.input', ref), suggestions);
            row.append(this.label('playback.selection.server', server), this.label('playback.selection.kind', kind),
                picker, more, remove, explanation);
            this.rows.append(row);
            if (source.kind === 'library') {
                ref.disabled = true;
                ref.value = t('playback.selection.kind.library');
            } else {
                load(0, true);
            }
        });
        this.updateActions();
    }

    private clearSearchTimers(): void {
        for (const timer of this.searchTimers) clearTimeout(timer);
        this.searchTimers.clear();
    }

    private async loadOptions(source: PlaybackSelectionSource, input: HTMLInputElement, explanation: HTMLElement, more: HTMLButtonElement,
        query: string, offset: number, resolveSelected: boolean, render: (options: PlaybackSelectionOption[]) => void, next: (offset: number) => void): Promise<void> {
        const request = (this.optionRequests.get(input) ?? 0) + 1;
        this.optionRequests.set(input, request);
        input.setAttribute('aria-busy', 'true');
        more.disabled = true;
        if (offset === 0) {
            more.hidden = true;
            render([]);
            explanation.textContent = t('playback.selection.loading');
        }
        try {
            const result = await playbackSelectionOptions(source.serverId, source.kind, offset, query, resolveSelected ? source.ref : undefined);
            if (this.disposed || request !== this.optionRequests.get(input) || !input.isConnected) return;
            if (!result.supported) {
                explanation.textContent = t(result.reason === 'UNSUPPORTED_CAPABILITY'
                    ? 'playback.selection.unsupported' : 'playback.selection.unavailable');
                more.hidden = true;
                return;
            }
            if (resolveSelected && source.ref) {
                const selected = result.selected ?? result.options.find(option => option.id === source.ref);
                input.value = selected?.name ?? source.ref;
            }
            render(result.options);
            explanation.textContent = result.options.length > 0 ? '' : t('playback.selection.no_inputs');
            more.hidden = !result.hasMore;
            more.disabled = false;
            more.onclick = () => { input.focus(); next(offset + result.options.length); };
        } catch {
            if (!this.disposed && request === this.optionRequests.get(input) && input.isConnected) {
                explanation.textContent = t('playback.selection.unavailable');
                more.disabled = false;
            }
        } finally {
            if (request === this.optionRequests.get(input)) input.setAttribute('aria-busy', 'false');
        }
        this.updateActions();
    }

    private async saveConfig(): Promise<void> {
        const seed = Number(this.seed.value);
        if (!Number.isSafeInteger(seed) || seed < 0 || seed > 4294967295 || this.config.sources.some(source => source.kind !== 'library' && !source.ref)) {
            this.status.textContent = t('playback.selection.invalid'); return;
        }
        const ordering = this.ordering.value === this.config.ordering[0]
            ? [...this.config.ordering]
            : [this.ordering.value as PlaybackSelectionOrdering];
        const config: PlaybackSelectionConfig = { schemaVersion: 1, sources: this.config.sources.map(source => ({ ...source })),
            ordering, seed, maxTracks: this.config.maxTracks };
        const revision = this.editRevision;
        this.saving = true; this.updateActions();
        try {
            await playbackSaveSelectionConfig(config);
            if (this.disposed) return;
            if (revision === this.editRevision) {
                this.config = { ...config, sources: this.config.sources }; this.dirty = false;
                this.status.textContent = t('playback.selection.saved');
            } else {
                this.status.textContent = t('playback.selection.save_again');
            }
        } catch {
            if (!this.disposed) this.status.textContent = t('playback.selection.save_failed');
        }
        this.saving = false; this.updateActions();
    }

    private async startSelection(): Promise<void> {
        const request = ++this.request;
        this.busy = true; this.updateActions();
        this.status.textContent = t('playback.selection.starting');
        try {
            await playbackStartSelection();
            if (!this.disposed && request === this.request) this.status.textContent = t('playback.selection.started');
        } catch (error) {
            if (!this.disposed && request === this.request) {
                const message = error instanceof Error ? error.message : String(error);
                this.status.textContent = message.includes('PLAYBACK_SELECTION_SAVE_FAILED') ? t('playback.selection.load_failed')
                    : message.includes('PLAYBACK_SELECTION_PREPARATION_FAILED') ? t('playback.selection.preparation_failed')
                    : message.includes('PLAYBACK_BUSY') ? t('playback.selection.busy')
                    : message.includes('OUTPUT_') ? t('playback.output.choose')
                    : message.includes('PLAYBACK_SELECTION_EMPTY') ? t('playback.selection.empty')
                    : message.includes('PLAYBACK_SELECTION_NO_MUSIC_SERVER') ? t('playback.selection.no_music_server')
                    : message.includes('PLAYBACK_SELECTION_SETUP') ? t('playback.selection.invalid')
                    : message.includes('PLAYBACK_SELECTION_CANCELLED') ? t('playback.selection.cancelled')
                    : t('playback.selection.unavailable');
            }
        } finally {
            if (request === this.request) { this.busy = false; this.updateActions(); }
        }
    }

    private async cancelStart(): Promise<void> {
        const request = this.request;
        this.cancelling = true; this.updateActions();
        try {
            await playbackCancelSelectionStart();
            if (!this.disposed && request === this.request && this.busy) {
                ++this.request;
                this.busy = false;
                this.status.textContent = t('playback.selection.cancelled');
            }
        } catch {
            if (!this.disposed) this.status.textContent = t('playback.selection.cancel_failed');
        } finally {
            this.cancelling = false; this.updateActions();
        }
    }
}
