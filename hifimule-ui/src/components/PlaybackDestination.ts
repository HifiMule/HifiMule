import { playbackDescribeOccurrences, playbackListOccurrences, serverList, type OccurrenceDisplay, type PlaybackSessionSnapshot } from '../rpc';
import { t } from '../i18n';
import { playbackStore } from '../state/playback';
import { formatServerIdentity } from '../serverIdentity';

export class PlaybackDestination {
    private disposed = false;
    private unsubscribe?: () => void;
    private snapshot?: PlaybackSessionSnapshot;
    private cursor: string | null = null;
    private nextCursor: string | null = null;
    private previous: Array<string | null> = [];
    private loadIdentity = '';
    private labels = new Map<string, string>();
    private readonly body = document.createElement('div');
    private readonly audition = document.createElement('div');
    private readonly list = document.createElement('ol');
    private readonly previousButton = document.createElement('button');
    private readonly nextButton = document.createElement('button');
    private readonly count = document.createElement('span');
    private readonly error = document.createElement('p');
    private readonly retry = document.createElement('button');
    private rows?: OccurrenceDisplay[];
    private pageRequest = 0;
    private loading = false;
    private retryAttempts = 0;
    private labelAttempts = 0;
    private retryTimer?: ReturnType<typeof setTimeout>;
    private labelTimer?: ReturnType<typeof setTimeout>;

    constructor(
        container: HTMLElement,
        private readonly onBrowseLibrary: () => void,
    ) {
        container.classList.add('playback-destination');
        container.setAttribute('aria-label', t('destination.playback'));
        this.body.className = 'playback-destination__body';
        this.audition.className = 'playback-destination__preview';
        this.audition.hidden = true;
        this.list.className = 'playback-destination__queue';
        const paging = document.createElement('div');
        paging.className = 'playback-destination__paging';
        this.previousButton.type = this.nextButton.type = this.retry.type = 'button';
        this.previousButton.textContent = t('playback.queue.previous');
        this.nextButton.textContent = t('playback.queue.next');
        this.previousButton.addEventListener('click', () => {
            if (this.loading || this.previous.length === 0) return;
            this.cursor = this.previous.pop() ?? null;
            void this.loadPage();
        });
        this.nextButton.addEventListener('click', () => {
            if (this.loading || !this.nextCursor) return;
            this.previous.push(this.cursor);
            this.cursor = this.nextCursor;
            void this.loadPage();
        });
        this.error.setAttribute('role', 'status');
        this.error.hidden = this.retry.hidden = true;
        this.retry.textContent = t('playback.retry');
        this.retry.addEventListener('click', () => {
            this.retryAttempts = 0;
            void this.retryPage();
        });
        paging.append(this.previousButton, this.count, this.nextButton);
        this.body.append(this.audition, this.error, this.retry, this.list, paging);
        this.updatePaging();
        container.replaceChildren(this.heading(), this.body);
        this.unsubscribe = playbackStore.subscribe(snapshot => void this.receive(snapshot));
        void this.loadLabels();
    }

    destroy(): void {
        this.disposed = true;
        this.unsubscribe?.();
        this.cancelRetry();
        if (this.labelTimer !== undefined) clearTimeout(this.labelTimer);
    }

    private async loadLabels(): Promise<void> {
        try {
            const servers = await serverList();
            if (this.disposed) return;
            this.labels = new Map(servers.filter(server => server.serverId)
                .map(server => [server.serverId as string, formatServerIdentity(server).label]));
            this.renderRows();
        } catch {
            if (!this.disposed && this.labelAttempts++ < 3) {
                this.labelTimer = setTimeout(() => void this.loadLabels(), 1000 * 2 ** this.labelAttempts);
            }
        }
    }

    private async receive(snapshot: PlaybackSessionSnapshot): Promise<void> {
        if (this.disposed) return;
        const identity = `${snapshot.instanceId}:${snapshot.sessionId}:${snapshot.queueRevision}`;
        const changed = identity !== this.loadIdentity;
        this.snapshot = snapshot;
        this.renderTransport();
        if (!changed) return;
        this.loadIdentity = identity;
        this.cursor = null; this.nextCursor = null; this.previous = [];
        this.rows = undefined;
        this.list.replaceChildren();
        this.retryAttempts = 0;
        this.cancelRetry();
        await this.loadPage();
    }

    private cancelRetry(): void {
        if (this.retryTimer !== undefined) clearTimeout(this.retryTimer);
        this.retryTimer = undefined;
    }

    private async loadPage(): Promise<void> {
        const observed = this.snapshot;
        if (!observed || this.disposed) return;
        const request = ++this.pageRequest;
        const cursor = this.cursor;
        const identity = this.loadIdentity;
        const current = () => !this.disposed && request === this.pageRequest
            && identity === this.loadIdentity && cursor === this.cursor;
        this.loading = true;
        this.updatePaging();
        try {
            const page = await playbackListOccurrences(observed, cursor, 100);
            if (!current()) return;
            const descriptions = page.occurrences.length
                ? await playbackDescribeOccurrences(observed, page.occurrences.map(item => item.occurrenceId)) : [];
            if (!current()) return;
            this.rows = descriptions;
            this.nextCursor = page.nextCursor;
            this.count.textContent = t('playback.queue.count', { count: page.totalOccurrenceCount });
            this.error.hidden = true;
            // Partial/offline metadata stays readable and can be refreshed without
            // reloading it on every transport tick.
            this.retry.hidden = descriptions.every(row => row.status === 'available');
            this.retryAttempts = 0;
            this.cancelRetry();
            this.renderRows();
        } catch {
            if (!current()) return;
            this.cursor = null; this.nextCursor = null; this.previous = [];
            this.renderError();
            if (this.retryAttempts++ < 3) {
                this.retryTimer = setTimeout(() => void this.retryPage(), 1000 * 2 ** this.retryAttempts);
            }
        } finally {
            if (!this.disposed && request === this.pageRequest) {
                this.loading = false;
                this.updatePaging();
            }
        }
    }

    private async retryPage(): Promise<void> {
        if (this.disposed || this.loading) return;
        this.cancelRetry();
        const request = this.pageRequest;
        try {
            const snapshot = await playbackStore.refresh();
            if (this.disposed || request !== this.pageRequest) return;
            // A changed owner/revision already started loading through receive().
            // An equal snapshot is not broadcast by the store, so retry explicitly.
            this.snapshot = snapshot;
        } catch { /* page request retains its bounded recovery/error action */ }
        if (!this.disposed && request === this.pageRequest) await this.loadPage();
    }

    private renderTransport(): void {
        const snapshot = this.snapshot;
        this.audition.hidden = snapshot?.mode !== 'preview';
        this.audition.textContent = snapshot?.mode === 'preview'
            ? t('playback.queue.preview', { title: snapshot.playback.metadata?.title ?? t('playback.nothing_selected') }) : '';
    }

    private renderRows(): void {
        if (!this.rows) return;
        if (this.rows.length === 0) {
            const empty = document.createElement('li');
            empty.className = 'playback-destination__empty';
            empty.textContent = t('playback.queue.empty');
            this.list.replaceChildren(empty);
        } else {
            this.list.replaceChildren(...this.rows.map(row => this.row(row)));
        }
    }

    private updatePaging(): void {
        // Keep native buttons mounted and focusable, including while loading.
        this.previousButton.setAttribute('aria-disabled', String(this.loading || this.previous.length === 0));
        this.nextButton.setAttribute('aria-disabled', String(this.loading || !this.nextCursor));
        this.body.setAttribute('aria-busy', String(this.loading));
    }

    private heading(): HTMLDivElement {
        const heading = document.createElement('div');
        heading.className = 'playback-destination__heading';
        const title = document.createElement('h2');
        title.textContent = t('destination.playback');
        const actions = document.createElement('div');
        actions.className = 'playback-destination__heading-actions';
        const readonly = document.createElement('span');
        readonly.textContent = t('playback.queue.read_only');
        const browse = document.createElement('button');
        browse.type = 'button';
        browse.className = 'playback-destination__browse';
        browse.textContent = t('playback.queue.back_to_library');
        browse.addEventListener('click', this.onBrowseLibrary);
        actions.append(readonly, browse);
        heading.append(title, actions);
        return heading;
    }

    private row(row: OccurrenceDisplay): HTMLLIElement {
        const item = document.createElement('li');
        item.className = 'playback-destination__row';
        const source = this.labels.get(row.source.serverId) ?? t('playback.queue.source_unavailable');
        const title = row.title ?? t(`playback.queue.${row.status}`);
        item.textContent = `${title} — ${row.artist ?? t('playback.queue.unknown_artist')} · ${source}`;
        return item;
    }

    private renderError(): void {
        this.error.hidden = this.retry.hidden = false;
        this.error.textContent = t('playback.queue.recoverable_error');
    }
}
