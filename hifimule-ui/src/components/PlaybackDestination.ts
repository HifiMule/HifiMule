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

    constructor(private readonly container: HTMLElement) {
        container.classList.add('playback-destination');
        container.setAttribute('aria-label', t('destination.playback'));
        this.unsubscribe = playbackStore.subscribe(snapshot => void this.receive(snapshot));
        void this.loadLabels();
    }

    destroy(): void { this.disposed = true; this.unsubscribe?.(); }

    private async loadLabels(): Promise<void> {
        try {
            const servers = await serverList();
            this.labels = new Map(servers.filter(server => server.serverId)
                .map(server => [server.serverId as string, formatServerIdentity(server).label]));
        } catch { /* rows retain a localized source fallback */ }
    }

    private async receive(snapshot: PlaybackSessionSnapshot): Promise<void> {
        const identity = `${snapshot.instanceId}:${snapshot.sessionId}:${snapshot.queueRevision}`;
        if (this.loadIdentity && identity !== this.loadIdentity) {
            this.cursor = null; this.nextCursor = null; this.previous = [];
        }
        this.snapshot = snapshot;
        this.loadIdentity = identity;
        await this.loadPage();
    }

    private async loadPage(): Promise<void> {
        const observed = this.snapshot;
        if (!observed || this.disposed) return;
        const identity = this.loadIdentity;
        try {
            const page = await playbackListOccurrences(observed, this.cursor, 100);
            if (this.disposed || identity !== this.loadIdentity) return;
            const descriptions = page.occurrences.length
                ? await playbackDescribeOccurrences(observed, page.occurrences.map(item => item.occurrenceId)) : [];
            if (this.disposed || identity !== this.loadIdentity) return;
            this.nextCursor = page.nextCursor;
            this.render(observed, descriptions, page.totalOccurrenceCount);
        } catch {
            this.cursor = null; this.nextCursor = null; this.previous = [];
            this.renderError();
            try { await playbackStore.refresh(); } catch { /* explanation is already visible */ }
        }
    }

    private render(snapshot: PlaybackSessionSnapshot, rows: OccurrenceDisplay[], total: number): void {
        const heading = document.createElement('div');
        heading.className = 'playback-destination__heading';
        const title = document.createElement('h2');
        title.textContent = t('destination.playback');
        const readonly = document.createElement('span');
        readonly.textContent = t('playback.queue.read_only');
        heading.append(title, readonly);
        const audition = document.createElement('div');
        audition.className = 'playback-destination__preview';
        audition.hidden = snapshot.mode !== 'preview';
        audition.textContent = snapshot.mode === 'preview'
            ? t('playback.queue.preview', { title: snapshot.playback.metadata?.title ?? t('playback.nothing_selected') }) : '';
        const list = document.createElement('ol');
        list.className = 'playback-destination__queue';
        if (rows.length === 0) {
            const empty = document.createElement('li');
            empty.className = 'playback-destination__empty';
            empty.textContent = t('playback.queue.empty');
            list.append(empty);
        } else {
            for (const row of rows) list.append(this.row(row));
        }
        const paging = document.createElement('div');
        paging.className = 'playback-destination__paging';
        const previous = document.createElement('button');
        previous.type = 'button'; previous.textContent = t('playback.queue.previous');
        previous.disabled = this.previous.length === 0;
        previous.addEventListener('click', () => {
            this.cursor = this.previous.pop() ?? null;
            void this.loadPage();
        });
        const next = document.createElement('button');
        next.type = 'button'; next.textContent = t('playback.queue.next');
        next.disabled = !this.nextCursor;
        next.addEventListener('click', () => {
            const cursor = this.nextCursor;
            if (!cursor) return;
            this.previous.push(this.cursor);
            this.cursor = cursor;
            void this.loadPage();
        });
        const count = document.createElement('span');
        count.textContent = t('playback.queue.count', { count: total });
        paging.append(previous, count, next);
        this.container.replaceChildren(heading, audition, list, paging);
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
        const error = document.createElement('p');
        error.setAttribute('role', 'status');
        error.textContent = t('playback.queue.recoverable_error');
        this.container.replaceChildren(error);
    }
}
