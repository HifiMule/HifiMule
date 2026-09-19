import { destinationSelect, getDaemonState, type DaemonDestinationState, type Destination } from '../rpc';
import { t } from '../i18n';
import { InitDeviceModal } from './InitDeviceModal';
import { basketStore } from '../state/basket';

export class DestinationHub {
    private disposed = false;
    private timer?: ReturnType<typeof setTimeout>;
    private revision = '-1';
    private refreshRequest = 0;
    private selecting = false;
    private selectionRequest = 0;
    private selectionTail: Promise<void> = Promise.resolve();
    private selectedKey?: string;
    private issueNodes = new Map<string, { element: HTMLDivElement; revision: string }>();
    private buttons = new Map<string, HTMLButtonElement>();
    private readonly list = document.createElement('div');
    private readonly issues = document.createElement('div');
    private readonly announcement = document.createElement('div');
    private readonly selectionError = document.createElement('p');

    constructor(private readonly container: HTMLElement, private readonly onChange: (selected?: Destination) => void) {
        container.className = 'destination-hub';
        container.setAttribute('aria-label', t('destination.group'));
        this.list.className = 'destination-hub__list';
        this.list.setAttribute('role', 'group');
        this.list.setAttribute('aria-label', t('destination.group'));
        this.issues.className = 'destination-hub__issues';
        this.announcement.className = 'sr-only';
        this.announcement.setAttribute('role', 'status');
        this.announcement.setAttribute('aria-live', 'polite');
        this.selectionError.className = 'destination-hub__issue';
        this.selectionError.setAttribute('role', 'alert');
        this.selectionError.hidden = true;
        container.replaceChildren(this.list, this.issues, this.announcement, this.selectionError);
        void this.refresh();
    }

    destroy(): void {
        this.disposed = true;
        if (this.timer !== undefined) clearTimeout(this.timer);
    }

    async refresh(): Promise<void> {
        if (this.disposed || this.selecting) return;
        if (this.timer !== undefined) clearTimeout(this.timer);
        this.timer = undefined;
        const request = ++this.refreshRequest;
        try {
            const state = await getDaemonState();
            if (this.disposed || request !== this.refreshRequest) return;
            const programmatic = this.revision !== '-1' && state.destinationRevision !== this.revision;
            await this.applyState(state, programmatic);
        } catch { /* lifecycle UI owns connection errors */ }
        if (!this.disposed && request === this.refreshRequest) this.timer = globalThis.setTimeout(() => void this.refresh(), 1000);
    }

    async applyState(state: DaemonDestinationState, programmatic = false): Promise<void> {
        if (this.disposed) return;
        const previousSelected = this.selectedKey;
        const ordered = state.destinations.filter(destination => destination.kind !== 'playback');
        const mounted: HTMLButtonElement[] = [];
        for (const destination of ordered) {
            const key = this.key(destination);
            let button = this.buttons.get(key);
            if (!button) {
                button = document.createElement('button');
                button.type = 'button';
                button.dataset.destinationKey = key;
                button.addEventListener('click', () => void this.select(destination));
                this.buttons.set(key, button);
            }
            button.dataset.destinationKind = destination.kind;
            button.className = `destination-hub__item${destination.selected ? ' is-selected' : ''}`;
            button.setAttribute('aria-pressed', String(destination.selected));
            button.textContent = destination.name;
            if (destination.kind === 'pendingDevice') {
                button.textContent = `${destination.name} — ${t('destination.setup')}`;
                button.setAttribute('aria-label', t('destination.setup_named', { name: destination.name }));
            }
            mounted.push(button);
        }
        const mountedSet = new Set(mounted);
        for (const child of [...this.list.children]) {
            if (!mountedSet.has(child as HTMLButtonElement)) child.remove();
        }
        // New observations are already ordered by the daemon's mutation clock. Append
        // only genuinely new nodes so refreshing selection state never detaches the
        // focused button (which would steal keyboard focus in real browsers).
        for (const button of mounted) {
            if (button.parentElement !== this.list) this.list.append(button);
        }
        for (const [key] of this.buttons) if (!ordered.some(item => this.key(item) === key)) this.buttons.delete(key);
        const issueIds = new Set<string>();
        for (const issue of state.deviceDiscoveryIssues ?? []) {
            issueIds.add(issue.discoveryId);
            let node = this.issueNodes.get(issue.discoveryId);
            if (!node) {
                const element = document.createElement('div');
                element.className = 'destination-hub__issue';
                element.setAttribute('role', 'status');
                node = { element, revision: '' };
                this.issueNodes.set(issue.discoveryId, node);
                this.issues.append(element);
            }
            if (node.revision !== issue.revision) {
                node.element.textContent = t(`destination.failure.${issue.code}`, { name: issue.displayName ?? t('destination.device') });
                node.revision = issue.revision;
            }
        }
        for (const [id, node] of this.issueNodes) {
            if (!issueIds.has(id)) { node.element.remove(); this.issueNodes.delete(id); }
        }
        const selected = state.destinations.find(item => item.selected);
        if (programmatic && selected && this.key(selected) !== previousSelected) {
            const name = selected.kind === 'playback' ? t('destination.playback') : selected.name;
            this.announcement.textContent = t('destination.arrival_selected', { name });
        }
        this.selectedKey = selected ? this.key(selected) : undefined;
        this.revision = state.destinationRevision;
        this.container.hidden = mounted.length === 0 && issueIds.size === 0 && this.selectionError.hidden;
        if (selected && this.key(selected) !== previousSelected) this.onChange(selected);
    }

    selectPlayback(onSelected: () => void): Promise<void> {
        return this.select({ kind: 'playback', id: 'playback', selected: false }, onSelected);
    }

    private select(destination: Destination, onSelected?: () => void): Promise<void> {
        if (this.disposed) return Promise.resolve();
        const request = ++this.selectionRequest;
        this.selecting = true;
        ++this.refreshRequest;
        if (this.timer !== undefined) clearTimeout(this.timer);
        this.timer = undefined;
        // Serialize mutations, but discard superseded work before each await's
        // result can start another mutation or change the visible surface.
        const selection = this.selectionTail.then(() => this.performSelection(destination, request, onSelected));
        this.selectionTail = selection;
        return selection;
    }

    private async performSelection(destination: Destination, request: number, onSelected?: () => void): Promise<void> {
        const current = () => !this.disposed && request === this.selectionRequest;
        if (!current()) return;
        this.selectionError.hidden = true;
        this.selectionError.textContent = '';
        try {
            const state = await getDaemonState();
            if (!current()) return;
            const selected = state.destinations.find(item => item.selected);
            await this.applyState(state);
            if (!current()) return;
            if (selected?.kind === 'device') await basketStore.flushPendingSave();
            if (!current()) return;
            if (destination.kind === 'pendingDevice') {
                const modal = new InitDeviceModal(this.container, () => {
                    if (current()) void this.refresh();
                });
                await modal.open(destination.name, destination.pendingId, this.revision);
                return;
            }
            if (destination.kind !== 'playback' || selected?.kind !== 'playback') {
                await destinationSelect(destination.kind === 'playback' ? { kind: 'playback' } : { kind: 'device', path: destination.path });
            }
            if (!current()) return;
            if (destination.kind === 'device') {
                const basket = await import('../rpc').then(({ rpcCall }) => rpcCall('manifest_get_basket')) as any;
                if (!current()) return;
                basketStore.hydrateFromDaemon(basket?.basketItems ?? []);
            }
            const updated = await getDaemonState();
            if (!current()) return;
            await this.applyState(updated);
            if (!current()) return;
            const confirmed = updated.destinations.find(item => item.selected);
            if (confirmed && this.key(confirmed) === this.key(destination)) {
                this.onChange(confirmed);
                onSelected?.();
            }
        } catch {
            if (current()) {
                this.selectionError.textContent = t('destination.selection_failed');
                this.selectionError.hidden = false;
                this.container.hidden = false;
            }
        } finally {
            if (current()) {
                this.selecting = false;
                await this.refresh();
            }
        }
    }

    private key(destination: Destination): string {
        if (destination.kind === 'playback') return 'playback';
        if (destination.kind === 'device') return `device:${destination.path}`;
        return `pending:${destination.pendingId}`;
    }
}
