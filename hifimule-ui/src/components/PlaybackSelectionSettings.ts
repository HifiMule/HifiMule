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
    private optionRequests = new WeakMap<HTMLSelectElement, number>();

    constructor() {
        this.element.className = 'playback-selection-settings';
        const heading = document.createElement('h2');
        heading.textContent = t('playback.selection.title');
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
        if (this.busy) void playbackCancelSelectionStart().catch(() => undefined);
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
        this.save.disabled = this.busy || this.saving || !this.dirty || this.config.sources.some(source => !source.ref);
        this.start.disabled = this.busy || this.cancelling || this.saving || this.dirty || this.config.sources.length === 0;
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
        this.rows.replaceChildren();
        this.config.sources.forEach((source, index) => {
            const row = document.createElement('div'); row.className = 'playback-selection-settings__row';
            const server = document.createElement('select');
            for (const item of this.servers) {
                if (item.serverId) server.add(new Option(formatServerIdentity(item).label, item.serverId));
            }
            server.value = source.serverId;
            const kind = document.createElement('select');
            for (const value of kinds) kind.add(new Option(t(`playback.selection.kind.${value}`), value));
            kind.value = source.kind;
            const ref = document.createElement('select'); ref.setAttribute('aria-label', t('playback.selection.input'));
            const more = document.createElement('button'); more.type = 'button';
            more.textContent = t('playback.selection.more'); more.hidden = true;
            const explanation = document.createElement('span'); explanation.className = 'playback-selection-settings__explanation';
            const remove = document.createElement('button'); remove.type = 'button';
            remove.textContent = t('playback.selection.remove');
            remove.setAttribute('aria-label', `${t('playback.selection.remove')} ${index + 1}`);
            server.addEventListener('change', () => { source.serverId = server.value; source.ref = ''; this.markDirty(); void this.loadOptions(source, ref, explanation, more); });
            kind.addEventListener('change', () => { source.kind = kind.value as PlaybackSelectionKind; source.ref = ''; this.markDirty(); void this.loadOptions(source, ref, explanation, more); });
            ref.addEventListener('change', () => { source.ref = ref.value; this.markDirty(); });
            remove.addEventListener('click', () => { this.config.sources.splice(index, 1); this.markDirty(); this.renderRows(); });
            row.append(this.label('playback.selection.server', server), this.label('playback.selection.kind', kind),
                this.label('playback.selection.input', ref), more, remove, explanation);
            this.rows.append(row);
            void this.loadOptions(source, ref, explanation, more);
        });
        this.updateActions();
    }

    private async loadOptions(source: PlaybackSelectionSource, select: HTMLSelectElement, explanation: HTMLElement, more: HTMLButtonElement, offset = 0): Promise<void> {
        const request = (this.optionRequests.get(select) ?? 0) + 1;
        this.optionRequests.set(select, request);
        select.disabled = true;
        more.disabled = true;
        if (offset === 0) {
            more.hidden = true;
            select.replaceChildren(new Option(t('playback.selection.loading'), ''));
        }
        try {
            const result = await playbackSelectionOptions(source.serverId, source.kind, offset);
            if (this.disposed || request !== this.optionRequests.get(select) || !select.isConnected) return;
            if (!result.supported) {
                explanation.textContent = t(result.reason === 'UNSUPPORTED_CAPABILITY'
                    ? 'playback.selection.unsupported' : 'playback.selection.unavailable');
                more.hidden = true;
                return;
            }
            if (offset === 0) select.replaceChildren(new Option(t('playback.selection.choose'), ''));
            for (const option of result.options as PlaybackSelectionOption[]) {
                const existing = Array.from(select.options).find(item => item.value === option.id);
                if (existing) existing.textContent = option.name;
                else select.add(new Option(option.name, option.id));
            }
            if (source.ref && !Array.from(select.options).some(item => item.value === source.ref)) {
                select.add(new Option(source.ref, source.ref));
            }
            select.value = source.ref;
            explanation.textContent = select.options.length > 1 ? '' : t('playback.selection.no_inputs');
            select.disabled = select.options.length <= 1;
            more.hidden = !result.hasMore;
            more.disabled = false;
            more.onclick = () => void this.loadOptions(source, select, explanation, more, offset + result.options.length);
        } catch {
            if (!this.disposed && request === this.optionRequests.get(select) && select.isConnected) {
                explanation.textContent = t('playback.selection.unavailable');
                select.disabled = offset === 0;
                more.disabled = false;
            }
        }
        this.updateActions();
    }

    private async saveConfig(): Promise<void> {
        const seed = Number(this.seed.value);
        if (!Number.isSafeInteger(seed) || seed < 0 || seed > 4294967295 || this.config.sources.some(source => !source.ref)) {
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
                this.config = config; this.dirty = false;
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
                this.status.textContent = message.includes('PLAYBACK_SELECTION_EMPTY') ? t('playback.selection.empty')
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
