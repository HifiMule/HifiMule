import {
    isPlaybackQueueConflict, playbackDescribeOccurrences, playbackListOccurrences,
    playbackMoveUpcoming, playbackRemoveUpcoming, serverList,
    type OccurrenceDisplay, type PlaybackOccurrencePage, type PlaybackSessionSnapshot, type PlaybackStatus,
} from '../rpc';
import { t } from '../i18n';
import { playbackStore } from '../state/playback';
import { formatServerIdentity } from '../serverIdentity';

type Section = 'upcoming' | 'history';
const PRESENTED_MAIN_STATUSES: ReadonlySet<PlaybackStatus> = new Set(['loading', 'active', 'paused']);
interface Region {
    section: Section; list: HTMLOListElement; count: HTMLSpanElement;
    previousButton: HTMLButtonElement; nextButton: HTMLButtonElement;
    cursor: string | null; nextCursor: string | null; previous: Array<string | null>;
    page?: PlaybackOccurrencePage; rows?: OccurrenceDisplay[];
}

export class PlaybackDestination {
    private disposed = false;
    private unsubscribe?: () => void;
    private snapshot?: PlaybackSessionSnapshot;
    private loadIdentity = '';
    private labels = new Map<string, string>();
    private readonly body = document.createElement('div');
    private readonly audition = document.createElement('div');
    private readonly current = document.createElement('section');
    private readonly error = document.createElement('p');
    private readonly retry = document.createElement('button');
    private readonly status = document.createElement('p');
    private readonly regions: Record<Section, Region>;
    private pageRequest = 0;
    private loading = 0;
    private retryAttempts = 0;
    private labelAttempts = 0;
    private retryTimer?: ReturnType<typeof setTimeout>;
    private labelTimer?: ReturnType<typeof setTimeout>;
    private focusAfterLoad?: { occurrenceId: string; action: string };

    constructor(container: HTMLElement, private readonly onBrowseLibrary: () => void) {
        container.classList.add('playback-destination');
        container.setAttribute('aria-label', t('destination.playback'));
        this.body.className = 'playback-destination__body';
        this.audition.className = 'playback-destination__preview';
        this.audition.hidden = true;
        this.current.className = 'playback-destination__current';
        this.current.tabIndex = -1;
        this.error.setAttribute('role', 'status');
        this.error.hidden = this.retry.hidden = true;
        this.retry.type = 'button';
        this.retry.textContent = t('playback.retry');
        this.retry.addEventListener('click', () => { this.retryAttempts = 0; void this.retryPages(); });
        this.status.className = 'playback-destination__mutation-status';
        this.status.setAttribute('role', 'status');
        this.status.setAttribute('aria-live', 'polite');
        this.regions = { upcoming: this.createRegion('upcoming'), history: this.createRegion('history') };
        this.body.append(this.audition, this.status, this.error, this.retry, this.current,
            this.regionElement(this.regions.upcoming), this.regionElement(this.regions.history));
        container.replaceChildren(this.heading(), this.body);
        this.unsubscribe = playbackStore.subscribe(snapshot => void this.receive(snapshot));
        void this.loadLabels();
    }

    destroy(): void {
        this.disposed = true; this.unsubscribe?.(); this.cancelRetry();
        if (this.labelTimer !== undefined) clearTimeout(this.labelTimer);
    }

    private createRegion(section: Section): Region {
        const list = document.createElement('ol');
        list.className = `playback-destination__queue playback-destination__queue--${section}`;
        const previousButton = document.createElement('button');
        const nextButton = document.createElement('button');
        const count = document.createElement('span');
        previousButton.type = nextButton.type = 'button';
        previousButton.textContent = t('playback.queue.previous');
        nextButton.textContent = t('playback.queue.next');
        const region: Region = { section, list, count, previousButton, nextButton, cursor: null, nextCursor: null, previous: [] };
        previousButton.addEventListener('click', () => {
            if (this.loading || region.previous.length === 0) return;
            region.cursor = region.previous.pop() ?? null; void this.loadRegion(region);
        });
        nextButton.addEventListener('click', () => {
            if (this.loading || !region.nextCursor) return;
            region.previous.push(region.cursor); region.cursor = region.nextCursor; void this.loadRegion(region);
        });
        return region;
    }

    private regionElement(region: Region): HTMLElement {
        const section = document.createElement('section');
        section.className = `playback-destination__region playback-destination__region--${region.section}`;
        const heading = document.createElement('h3');
        heading.textContent = t(`playback.queue.${region.section}`); heading.tabIndex = -1;
        const paging = document.createElement('div'); paging.className = 'playback-destination__paging';
        paging.append(region.previousButton, region.count, region.nextButton);
        section.append(heading, region.list, paging); return section;
    }

    private async loadLabels(): Promise<void> {
        try {
            const servers = await serverList(); if (this.disposed) return;
            this.labels = new Map(servers.filter(server => server.serverId)
                .map(server => [server.serverId as string, formatServerIdentity(server).label]));
            this.renderRegions();
        } catch {
            if (!this.disposed && this.labelAttempts++ < 3)
                this.labelTimer = setTimeout(() => void this.loadLabels(), 1000 * 2 ** this.labelAttempts);
        }
    }

    private async receive(snapshot: PlaybackSessionSnapshot): Promise<void> {
        if (this.disposed) return;
        const identity = `${snapshot.instanceId}:${snapshot.sessionId}:${snapshot.queueRevision}:${snapshot.mainCurrent?.occurrenceId ?? '-'}`;
        const changed = identity !== this.loadIdentity;
        this.snapshot = snapshot; this.renderTransport(); if (!changed) return;
        this.loadIdentity = identity;
        for (const region of Object.values(this.regions)) this.resetRegion(region);
        this.retryAttempts = 0; this.cancelRetry(); await this.loadPages();
    }

    private resetRegion(region: Region): void {
        region.cursor = null; region.nextCursor = null; region.previous = [];
        region.page = undefined; region.rows = undefined; region.list.replaceChildren();
    }

    private async loadPages(): Promise<void> {
        await Promise.all([this.loadRegion(this.regions.upcoming), this.loadRegion(this.regions.history)]);
    }

    private async loadRegion(region: Region, aroundOccurrenceId: string | null = null): Promise<void> {
        const observed = this.snapshot; if (!observed || this.disposed) return;
        const request = ++this.pageRequest; const identity = this.loadIdentity;
        const cursor = aroundOccurrenceId ? null : region.cursor;
        this.loading += 1; this.updatePaging();
        try {
            const page = await playbackListOccurrences(observed, cursor, 100, { section: region.section, aroundOccurrenceId });
            if (this.disposed || identity !== this.loadIdentity || request < this.pageRequest - 1) return;
            const rows = page.occurrences.length
                ? await playbackDescribeOccurrences(observed, page.occurrences.map(item => item.occurrenceId)) : [];
            if (this.disposed || identity !== this.loadIdentity) return;
            region.page = page; region.rows = rows; region.nextCursor = page.nextCursor;
            region.count.textContent = t('playback.queue.count', { count: page.sectionCount });
            this.error.hidden = true; this.retry.hidden = rows.every(row => row.status === 'available');
            if (region.section === 'upcoming') { this.retryAttempts = 0; this.cancelRetry(); }
            this.renderRegion(region); this.restoreFocus();
        } catch (error) {
            if (this.disposed || identity !== this.loadIdentity) return;
            this.renderError();
            if (region.section === 'upcoming' && this.retryAttempts++ < 3)
                this.retryTimer = setTimeout(() => void this.retryPages(), 1000 * 2 ** this.retryAttempts);
        } finally { this.loading = Math.max(0, this.loading - 1); this.updatePaging(); }
    }

    private async retryPages(): Promise<void> {
        if (this.disposed || this.loading) return; this.cancelRetry();
        try { this.snapshot = await playbackStore.refresh(); } catch { /* retain bounded retry */ }
        if (!this.disposed) await this.loadPages();
    }
    private cancelRetry(): void { if (this.retryTimer !== undefined) clearTimeout(this.retryTimer); this.retryTimer = undefined; }

    private renderTransport(): void {
        const snapshot = this.snapshot;
        this.audition.hidden = snapshot?.mode !== 'preview';
        this.audition.textContent = snapshot?.mode === 'preview'
            ? t('playback.queue.preview', { title: snapshot.playback.metadata?.title ?? t('playback.nothing_selected') }) : '';
        const main = snapshot?.mainCurrent;
        const playbackStatus = snapshot?.playback.status;
        if (!main || !playbackStatus || !PRESENTED_MAIN_STATUSES.has(playbackStatus)) {
            this.current.textContent = t('playback.queue.no_current');
            return;
        }
        const metadata = main.source && snapshot?.mode === 'main'
            && snapshot.playback.metadata?.source?.serverId === main.source.serverId
            && snapshot.playback.metadata?.source?.trackId === main.source.trackId
            ? snapshot.playback.metadata : undefined;
        if (!metadata) {
            this.current.textContent = t('playback.queue.no_current');
            return;
        }
        const source = main.source
            ? this.labels.get(main.source.serverId) ?? t('playback.queue.source_unavailable')
            : t('playback.queue.source_unavailable');
        this.current.textContent = `${t('playback.queue.current_occurrence', { id: metadata.title })} — ${metadata.artist ?? t('playback.queue.unknown_artist')} · ${source}`;
    }

    private renderRegions(): void { this.renderRegion(this.regions.upcoming); this.renderRegion(this.regions.history); }
    private renderRegion(region: Region): void {
        if (!region.rows || !region.page) return;
        if (region.rows.length === 0) {
            const empty = document.createElement('li'); empty.className = 'playback-destination__empty';
            empty.textContent = t(`playback.queue.${region.section}_empty`); region.list.replaceChildren(empty); return;
        }
        region.list.replaceChildren(...region.rows.map((row, index) => this.row(region, row, index)));
    }

    private row(region: Region, row: OccurrenceDisplay, index: number): HTMLLIElement {
        const item = document.createElement('li'); item.className = 'playback-destination__row'; item.dataset.occurrenceId = row.occurrenceId;
        const label = document.createElement('span');
        const source = this.labels.get(row.source.serverId) ?? t('playback.queue.source_unavailable');
        const title = row.title ?? t(`playback.queue.${row.status}`);
        label.textContent = `${title} — ${row.artist ?? t('playback.queue.unknown_artist')} · ${source}`; item.append(label);
        if (region.section === 'upcoming') {
            const actions = document.createElement('span'); actions.className = 'playback-destination__row-actions';
            actions.append(
                this.action(row.occurrenceId, 'moveUp', t('playback.queue.move_up'), () => this.move(region, index, -1)),
                this.action(row.occurrenceId, 'moveDown', t('playback.queue.move_down'), () => this.move(region, index, 1)),
                this.action(row.occurrenceId, 'remove', t('playback.queue.remove'), () => this.remove(region, index)));
            item.append(actions);
        }
        return item;
    }

    private action(occurrenceId: string, action: string, label: string, run: () => Promise<void>): HTMLButtonElement {
        const button = document.createElement('button'); button.type = 'button'; button.dataset.queueAction = action;
        button.dataset.occurrenceId = occurrenceId; button.textContent = label; button.addEventListener('click', () => void run()); return button;
    }

    private async move(region: Region, index: number, delta: -1 | 1): Promise<void> {
        const snapshot = this.snapshot; const page = region.page; if (!snapshot || !page) return;
        const occurrences = page.occurrences; const target = occurrences[index]; if (!target) return;
        let before: string | null;
        if (delta < 0) { before = index > 0 ? occurrences[index - 1].occurrenceId : page.precedingOccurrenceId; if (!before) return; }
        else if (index + 2 < occurrences.length) before = occurrences[index + 2].occurrenceId;
        else {
            const offset = Math.max(0, index + 2 - occurrences.length);
            before = page.followingOccurrenceIds[offset] ?? null;
            if (index === occurrences.length - 1 && page.endOfSection) return;
        }
        this.focusAfterLoad = { occurrenceId: target.occurrenceId, action: delta < 0 ? 'moveUp' : 'moveDown' };
        await this.mutate(() => playbackMoveUpcoming(snapshot, target.occurrenceId, before), target.occurrenceId, t('playback.queue.moved'));
    }

    private async remove(region: Region, index: number): Promise<void> {
        const snapshot = this.snapshot; const page = region.page; if (!snapshot || !page) return;
        const target = page.occurrences[index]; if (!target) return;
        const survivor = page.occurrences[index + 1]?.occurrenceId ?? page.followingOccurrenceIds[0]
            ?? page.occurrences[index - 1]?.occurrenceId ?? page.precedingOccurrenceId;
        if (survivor) this.focusAfterLoad = { occurrenceId: survivor, action: 'remove' };
        await this.mutate(() => playbackRemoveUpcoming(snapshot, [target.occurrenceId]), survivor ?? null, t('playback.queue.removed'));
    }

    private async mutate(operation: () => Promise<unknown>, around: string | null, success: string): Promise<void> {
        this.body.setAttribute('aria-busy', 'true');
        try {
            await operation(); this.status.textContent = success;
            const refreshed = await playbackStore.refresh(); this.snapshot = refreshed;
            this.loadIdentity = `${refreshed.instanceId}:${refreshed.sessionId}:${refreshed.queueRevision}:${refreshed.mainCurrent?.occurrenceId ?? '-'}`;
            await this.loadRegion(this.regions.upcoming, around); await this.loadRegion(this.regions.history);
        } catch (error) {
            if (isPlaybackQueueConflict(error)) { this.status.textContent = t('playback.queue.conflict'); await playbackStore.refresh(); }
            else this.status.textContent = (error as Error).message;
        } finally { this.body.setAttribute('aria-busy', 'false'); }
    }

    private restoreFocus(): void {
        const desired = this.focusAfterLoad; if (!desired) return;
        const escaped = CSS.escape(desired.occurrenceId);
        const action = this.body.querySelector<HTMLElement>(`[data-occurrence-id="${escaped}"][data-queue-action="${desired.action}"]`)
            ?? this.body.querySelector<HTMLElement>(`[data-occurrence-id="${escaped}"] button`);
        if (action?.isConnected) action.focus(); else this.current.focus(); this.focusAfterLoad = undefined;
    }

    private updatePaging(): void {
        for (const region of Object.values(this.regions)) {
            region.previousButton.setAttribute('aria-disabled', String(this.loading > 0 || region.previous.length === 0));
            region.nextButton.setAttribute('aria-disabled', String(this.loading > 0 || !region.nextCursor));
        }
        this.body.setAttribute('aria-busy', String(this.loading > 0));
    }

    private heading(): HTMLDivElement {
        const heading = document.createElement('div'); heading.className = 'playback-destination__heading';
        const title = document.createElement('h2'); title.textContent = t('destination.playback');
        const browse = document.createElement('button'); browse.type = 'button'; browse.className = 'playback-destination__browse';
        browse.textContent = t('playback.queue.back_to_library'); browse.addEventListener('click', this.onBrowseLibrary);
        heading.append(title, browse); return heading;
    }
    private renderError(): void { this.error.hidden = this.retry.hidden = false; this.error.textContent = t('playback.queue.recoverable_error'); }
}
