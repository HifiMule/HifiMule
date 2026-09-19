import { destinationSelect, getDaemonState, type DaemonDestinationState, type Destination } from '../rpc';
import { t } from '../i18n';
import { InitDeviceModal } from './InitDeviceModal';
import { basketStore } from '../state/basket';

export class DestinationHub {
    private disposed = false;
    private timer?: ReturnType<typeof setTimeout>;
    private revision = '-1';
    private buttons = new Map<string, HTMLButtonElement>();
    private readonly list = document.createElement('div');
    private readonly issues = document.createElement('div');
    private readonly announcement = document.createElement('div');

    constructor(private readonly container: HTMLElement, private readonly onChange: () => void) {
        container.className = 'destination-hub';
        container.setAttribute('aria-label', t('destination.group'));
        this.list.className = 'destination-hub__list';
        this.list.setAttribute('role', 'group');
        this.list.setAttribute('aria-label', t('destination.group'));
        this.issues.className = 'destination-hub__issues';
        this.announcement.className = 'sr-only';
        this.announcement.setAttribute('role', 'status');
        this.announcement.setAttribute('aria-live', 'polite');
        container.replaceChildren(this.list, this.issues, this.announcement);
        void this.refresh();
    }

    destroy(): void {
        this.disposed = true;
        if (this.timer !== undefined) clearTimeout(this.timer);
    }

    async refresh(): Promise<void> {
        if (this.disposed) return;
        try {
            const state = await getDaemonState();
            const programmatic = this.revision !== '-1' && state.destinationRevision !== this.revision;
            await this.applyState(state, programmatic);
        } catch { /* lifecycle UI owns connection errors */ }
        if (!this.disposed) this.timer = globalThis.setTimeout(() => void this.refresh(), 1000);
    }

    async applyState(state: DaemonDestinationState, programmatic = false): Promise<void> {
        const previousSelected = [...this.buttons.values()].find(button => button.getAttribute('aria-pressed') === 'true')?.dataset.destinationKey;
        const ordered = [...state.destinations].sort((a, b) => a.kind === 'playback' ? -1 : b.kind === 'playback' ? 1 : 0);
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
            button.textContent = destination.kind === 'playback' ? t('destination.playback') : destination.name;
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
        this.issues.replaceChildren(...(state.deviceDiscoveryIssues ?? []).map(issue => {
            const element = document.createElement('div');
            element.className = 'destination-hub__issue';
            element.setAttribute('role', 'status');
            element.textContent = t(`destination.failure.${issue.code}`, { name: issue.displayName ?? t('destination.device') });
            return element;
        }));
        const selected = ordered.find(item => item.selected);
        if (programmatic && selected && this.key(selected) !== previousSelected) {
            const name = selected.kind === 'playback' ? t('destination.playback') : selected.name;
            this.announcement.textContent = t('destination.arrival_selected', { name });
        }
        this.revision = state.destinationRevision;
        if (selected && this.key(selected) !== previousSelected) this.onChange();
    }

    private async select(destination: Destination): Promise<void> {
        if (destination.kind === 'pendingDevice') {
            const modal = new InitDeviceModal(this.container, this.onChange);
            await modal.open(destination.name, destination.pendingId, this.revision);
            return;
        }
        if (destination.kind === 'device') await basketStore.flushPendingSave();
        await destinationSelect(destination.kind === 'playback' ? { kind: 'playback' } : { kind: 'device', path: destination.path });
        if (destination.kind === 'device') {
            const basket = await import('../rpc').then(({ rpcCall }) => rpcCall('manifest_get_basket')) as any;
            basketStore.hydrateFromDaemon(basket?.basketItems ?? []);
        }
        this.onChange();
        await this.refresh();
    }

    private key(destination: Destination): string {
        if (destination.kind === 'playback') return 'playback';
        if (destination.kind === 'device') return `device:${destination.path}`;
        return `pending:${destination.pendingId}`;
    }
}
