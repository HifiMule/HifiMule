import {
    isPlaybackQueueConflict, playbackDescribeOccurrences, playbackListOccurrences,
    playbackMoveUpcoming, playbackRemoveUpcoming, serverList,
    type OccurrenceDisplay, type PlaybackOccurrencePage, type PlaybackSessionSnapshot,
} from '../rpc';
import { t } from '../i18n';
import { playbackStore } from '../state/playback';
import { formatServerIdentity } from '../serverIdentity';

type Section = 'upcoming' | 'history';
interface PageLocation { cursor: string | null; aroundOccurrenceId: string | null }
interface Region {
    section: Section; list: HTMLOListElement; count: HTMLSpanElement; heading: HTMLHeadingElement;
    previousButton: HTMLButtonElement; nextButton: HTMLButtonElement;
    location: PageLocation; nextCursor: string | null; previous: PageLocation[];
    request: number; loading: boolean; failed: boolean; retryNeeded: boolean;
    retryAttempts: number; retryTimer?: ReturnType<typeof setTimeout>;
    nodes: Map<string, HTMLLIElement>;
    page?: PlaybackOccurrencePage; rows?: OccurrenceDisplay[];
}

export class PlaybackDestination {
    private disposed = false;
    private unsubscribe?: () => void;
    private snapshot?: PlaybackSessionSnapshot;
    private loadIdentity = '';
    private labels = new Map<string, string>();
    private readonly body = document.createElement('div');
    private readonly error = document.createElement('p');
    private readonly retry = document.createElement('button');
    private readonly status = document.createElement('p');
    private readonly regions: Record<Section, Region>;
    private mutating = false;
    private deferPageLoads = false;
    private refreshInFlight?: Promise<boolean>;
    private labelAttempts = 0;
    private labelTimer?: ReturnType<typeof setTimeout>;
    private focusAfterLoad?: { occurrenceId: string | null; action: string };

    constructor(container: HTMLElement) {
        container.classList.add('playback-destination');
        container.setAttribute('aria-label', t('destination.playback'));
        this.body.className = 'playback-destination__body';
        this.error.setAttribute('role', 'status');
        this.error.hidden = this.retry.hidden = true;
        this.retry.type = 'button';
        this.retry.textContent = t('playback.retry');
        this.retry.addEventListener('click', () => {
            for (const region of Object.values(this.regions)) region.retryAttempts = 0;
            void this.retryPages();
        });
        this.status.className = 'playback-destination__mutation-status';
        this.status.setAttribute('role', 'status');
        this.status.setAttribute('aria-live', 'polite');
        this.regions = { upcoming: this.createRegion('upcoming'), history: this.createRegion('history') };
        this.body.append(this.status, this.error, this.retry,
            this.regionElement(this.regions.upcoming), this.regionElement(this.regions.history));
        container.replaceChildren(this.body);
        this.unsubscribe = playbackStore.subscribe(snapshot => void this.receive(snapshot));
        void this.loadLabels();
    }

    destroy(): void {
        this.disposed = true; this.unsubscribe?.();
        for (const region of Object.values(this.regions)) this.cancelRetry(region);
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
        const heading = document.createElement('h3');
        heading.textContent = t(`playback.queue.${section}`); heading.tabIndex = -1;
        const region: Region = { section, list, count, heading, previousButton, nextButton,
            location: { cursor: null, aroundOccurrenceId: null }, nextCursor: null, previous: [],
            request: 0, loading: false, failed: false, retryNeeded: false, retryAttempts: 0, nodes: new Map() };
        previousButton.addEventListener('click', () => {
            if (region.loading || this.mutating) return;
            const prior = region.previous.pop();
            const preceding = region.page?.precedingOccurrenceId;
            if (!prior && !preceding) return;
            region.location = prior ?? { cursor: null, aroundOccurrenceId: preceding! };
            void this.loadRegion(region);
        });
        nextButton.addEventListener('click', () => {
            if (region.loading || this.mutating || !region.nextCursor) return;
            region.previous.push({ ...region.location });
            region.location = { cursor: region.nextCursor, aroundOccurrenceId: null };
            void this.loadRegion(region);
        });
        return region;
    }

    private regionElement(region: Region): HTMLElement {
        const section = document.createElement('section');
        section.className = `playback-destination__region playback-destination__region--${region.section}`;
        const paging = document.createElement('div'); paging.className = 'playback-destination__paging';
        paging.append(region.previousButton, region.count, region.nextButton);
        section.append(region.heading, region.list, paging); return section;
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

    private identity(snapshot: PlaybackSessionSnapshot): string {
        return `${snapshot.instanceId}:${snapshot.sessionId}:${snapshot.queueRevision}:${snapshot.mainCurrent?.occurrenceId ?? '-'}`;
    }

    private async receive(snapshot: PlaybackSessionSnapshot): Promise<void> {
        if (this.disposed) return;
        const identity = this.identity(snapshot);
        const changed = identity !== this.loadIdentity;
        this.snapshot = snapshot;
        if (!changed) return;
        this.loadIdentity = identity;
        for (const region of Object.values(this.regions)) this.resetRegion(region);
        if (!this.deferPageLoads) await this.loadPages();
    }

    private resetRegion(region: Region): void {
        ++region.request; region.loading = false;
        region.location = { cursor: null, aroundOccurrenceId: null };
        region.nextCursor = null; region.previous = [];
        region.page = undefined; region.rows = undefined;
        region.failed = region.retryNeeded = false; region.retryAttempts = 0;
        this.cancelRetry(region);
        // Keep keyed nodes until the replacement page is accepted so refreshes
        // do not detach a focused action while metadata is still loading.
    }

    private async loadPages(): Promise<void> {
        await Promise.all(Object.values(this.regions).map(region => this.loadRegion(region)));
    }

    private async loadRegion(region: Region, recoverConflict = true): Promise<void> {
        const observed = this.snapshot; if (!observed || this.disposed) return;
        const request = ++region.request; const identity = this.loadIdentity;
        const location = { ...region.location };
        const current = () => !this.disposed && request === region.request && identity === this.loadIdentity;
        region.loading = true; this.updatePaging();
        try {
            const page = await playbackListOccurrences(observed, location.cursor, 100,
                { section: region.section, aroundOccurrenceId: location.aroundOccurrenceId });
            if (!current()) return;
            if (page.section !== region.section
                || page.mainCurrentOccurrenceId !== (observed.mainCurrent?.occurrenceId ?? null)) {
                throw new Error('Occurrence page no longer matches the requested section');
            }
            const rows = page.occurrences.length
                ? await playbackDescribeOccurrences(observed, page.occurrences.map(item => item.occurrenceId)) : [];
            if (!current()) return;
            region.page = page; region.rows = rows; region.nextCursor = page.nextCursor;
            region.count.textContent = t('playback.queue.count', { count: page.sectionCount });
            region.failed = false; region.retryNeeded = rows.some(row => row.status !== 'available');
            region.retryAttempts = 0; this.cancelRetry(region);
            this.renderRegion(region);
            if (region.section === 'upcoming' && !this.mutating) this.restoreFocus();
        } catch (error) {
            if (!current()) return;
            region.failed = region.retryNeeded = true;
            if (isPlaybackQueueConflict(error)) {
                this.status.textContent = t('playback.queue.conflict');
                // A survivor may have advanced out of this region even under
                // the refreshed anchor. Never keep retrying its invalid locator.
                region.location = { cursor: null, aroundOccurrenceId: null };
                region.previous = []; region.nextCursor = null; region.page = undefined;
                if (region.section === 'upcoming' && this.focusAfterLoad)
                    this.focusAfterLoad.occurrenceId = null;
                // One immediate reconciliation and first-page reload, including
                // equal snapshots. A second conflict uses bounded timed retries.
                if (recoverConflict && !this.refreshInFlight) {
                    const refreshed = await this.refreshAuthoritative();
                    if (!current()) return;
                    if (refreshed) { await this.loadRegion(region, false); return; }
                }
            }
            if (region.retryAttempts++ < 3) {
                this.cancelRetry(region);
                region.retryTimer = setTimeout(() => void this.retryRegion(region), 1000 * 2 ** region.retryAttempts);
            }
        } finally {
            if (current()) { region.loading = false; this.updateRecovery(); this.updatePaging(); }
        }
    }

    private async refreshAuthoritative(): Promise<boolean> {
        if (this.refreshInFlight) return this.refreshInFlight;
        const refresh = (async () => {
            try { await this.receive(await playbackStore.refresh()); return true; }
            catch {
                if (!this.disposed) this.status.textContent = t('playback.queue.conflict_refresh_failed');
                return false;
            }
        })();
        this.refreshInFlight = refresh;
        try { return await refresh; } finally { this.refreshInFlight = undefined; }
    }

    private async retryRegion(region: Region): Promise<void> {
        if (this.disposed || region.loading || this.mutating) return;
        this.cancelRetry(region);
        const request = region.request;
        await this.refreshAuthoritative();
        if (!this.disposed && request === region.request) await this.loadRegion(region);
    }

    private async retryPages(): Promise<void> {
        if (this.disposed || this.mutating) return;
        await Promise.all(Object.values(this.regions).filter(region => region.retryNeeded).map(region => this.retryRegion(region)));
    }

    private cancelRetry(region: Region): void {
        if (region.retryTimer !== undefined) clearTimeout(region.retryTimer);
        region.retryTimer = undefined;
    }

    private updateRecovery(): void {
        const regions = Object.values(this.regions);
        this.error.hidden = !regions.some(region => region.failed);
        this.error.textContent = this.error.hidden ? '' : t('playback.queue.recoverable_error');
        this.retry.hidden = !regions.some(region => region.retryNeeded);
    }

    private renderRegions(): void { this.renderRegion(this.regions.upcoming); this.renderRegion(this.regions.history); }
    private renderRegion(region: Region): void {
        if (!region.rows || !region.page) return;
        const retained = new Set(region.rows.map(row => row.occurrenceId));
        for (const [id, node] of region.nodes) {
            if (!retained.has(id)) { node.remove(); region.nodes.delete(id); }
        }
        if (region.rows.length === 0) {
            const empty = document.createElement('li'); empty.className = 'playback-destination__empty';
            empty.textContent = t(`playback.queue.${region.section}_empty`); region.list.replaceChildren(empty); return;
        }
        for (const child of Array.from(region.list.children)) {
            if (!retained.has((child as HTMLElement).dataset.occurrenceId ?? '')) child.remove();
        }
        for (const [index, row] of region.rows.entries()) {
            let item = region.nodes.get(row.occurrenceId);
            if (!item) {
                item = this.row(region, row.occurrenceId);
                region.nodes.set(row.occurrenceId, item);
            }
            const label = item.firstElementChild as HTMLElement;
            const source = this.labels.get(row.source.serverId) ?? t('playback.queue.source_unavailable');
            label.textContent = `${row.title ?? t(`playback.queue.${row.status}`)} — ${row.artist ?? t('playback.queue.unknown_artist')} · ${source}`;
            if (region.list.children[index] !== item) {
                const focused = document.activeElement as HTMLElement | null;
                const restore = focused && item.contains(focused);
                region.list.insertBefore(item, region.list.children[index] ?? null);
                if (restore && focused.isConnected) focused.focus();
            }
        }
    }

    private row(region: Region, occurrenceId: string): HTMLLIElement {
        const item = document.createElement('li'); item.className = 'playback-destination__row'; item.dataset.occurrenceId = occurrenceId;
        item.append(document.createElement('span'));
        if (region.section === 'upcoming') {
            const actions = document.createElement('span'); actions.className = 'playback-destination__row-actions';
            const index = () => region.page?.occurrences.findIndex(row => row.occurrenceId === occurrenceId) ?? -1;
            actions.append(
                this.action(occurrenceId, 'moveUp', t('playback.queue.move_up'), () => this.move(region, index(), -1)),
                this.action(occurrenceId, 'moveDown', t('playback.queue.move_down'), () => this.move(region, index(), 1)),
                this.action(occurrenceId, 'remove', t('playback.queue.remove'), () => this.remove(region, index())));
            item.append(actions);
        }
        return item;
    }

    private action(occurrenceId: string, action: 'moveUp' | 'moveDown' | 'remove', label: string, run: () => Promise<void>): HTMLElement {
        const button = document.createElement('button'); button.type = 'button'; button.dataset.queueAction = action;
        button.dataset.occurrenceId = occurrenceId;
        button.className = 'playback-destination__icon-action';
        button.setAttribute('aria-label', label);
        const icon = document.createElement('sl-icon');
        icon.setAttribute('name', { moveUp: 'arrow-up', moveDown: 'arrow-down', remove: 'x-lg' }[action]);
        icon.setAttribute('aria-hidden', 'true');
        const accessibleLabel = document.createElement('span');
        accessibleLabel.className = 'sr-only';
        accessibleLabel.textContent = label;
        button.append(icon, accessibleLabel);
        button.addEventListener('click', () => void run());
        const hint = document.createElement('sl-tooltip');
        hint.setAttribute('content', label);
        hint.setAttribute('placement', 'top');
        hint.setAttribute('hoist', '');
        hint.append(button);
        return hint;
    }

    private async move(region: Region, index: number, delta: -1 | 1): Promise<void> {
        const snapshot = this.snapshot; const page = region.page; if (!snapshot || !page || this.mutating || region.loading) return;
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
        const snapshot = this.snapshot; const page = region.page; if (!snapshot || !page || this.mutating || region.loading) return;
        const target = page.occurrences[index]; if (!target) return;
        const survivor = page.occurrences[index + 1]?.occurrenceId ?? page.followingOccurrenceIds[0]
            ?? page.occurrences[index - 1]?.occurrenceId ?? page.precedingOccurrenceId;
        this.focusAfterLoad = { occurrenceId: survivor ?? null, action: 'remove' };
        await this.mutate(() => playbackRemoveUpcoming(snapshot, [target.occurrenceId]), survivor ?? null, t('playback.queue.removed'));
    }

    private async mutate(operation: () => Promise<unknown>, around: string | null, success: string): Promise<void> {
        if (this.mutating || this.disposed) return;
        this.mutating = true; this.deferPageLoads = true; this.updatePaging();
        let accepted = false;
        let conflicted = false;
        try {
            await operation(); accepted = true; this.status.textContent = success;
        } catch (error) {
            conflicted = isPlaybackQueueConflict(error);
            this.status.textContent = conflicted ? t('playback.queue.conflict') : t('playback.command_error');
        }
        try {
            await this.receive(await playbackStore.refresh());
            if (this.disposed) return;
            const upcoming = this.regions.upcoming;
            upcoming.location = { cursor: null, aroundOccurrenceId: accepted ? around : null };
            if (!accepted) upcoming.previous = [];
            this.deferPageLoads = false;
            await this.loadPages();
        } catch {
            if (!this.disposed) {
                if (conflicted) this.status.textContent = t('playback.queue.conflict_refresh_failed');
                this.regions.upcoming.failed = this.regions.upcoming.retryNeeded = true;
                this.updateRecovery();
            }
        } finally {
            this.mutating = false; this.deferPageLoads = false;
            if (!this.disposed) {
                if (!this.regions.upcoming.loading) this.restoreFocus();
                this.updatePaging();
            }
        }
    }

    private restoreFocus(): void {
        const desired = this.focusAfterLoad; if (!desired) return;
        const region = this.regions.upcoming;
        const item = desired.occurrenceId ? region.nodes.get(desired.occurrenceId) : undefined;
        const action = item?.querySelector<HTMLElement>(`[data-queue-action="${desired.action}"]`);
        if (action?.isConnected) action.focus(); else region.heading.focus();
        this.focusAfterLoad = undefined;
    }

    private updatePaging(): void {
        for (const region of Object.values(this.regions)) {
            region.previousButton.setAttribute('aria-disabled', String(this.mutating || region.loading
                || region.previous.length === 0 && !region.page?.precedingOccurrenceId));
            region.nextButton.setAttribute('aria-disabled', String(this.mutating || region.loading || !region.nextCursor));
        }
        this.body.setAttribute('aria-busy', String(this.mutating || Object.values(this.regions).some(region => region.loading)));
    }

}
