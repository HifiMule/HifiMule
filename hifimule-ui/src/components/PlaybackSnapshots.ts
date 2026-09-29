import { playbackListSnapshots, playbackListSnapshotEntries, playbackPlanSnapshotPlaylistExport, playbackStartSnapshotPlaylistExport, playbackRetrySnapshotPlaylistExport, playbackReconcileSnapshotPlaylistExport, playbackListSnapshotPlaylistExports, type SnapshotPlaylistExport, type ListeningSnapshotSummary, type PlaybackSessionSnapshot } from '../rpc';
import { t } from '../i18n';
import { withDeadline } from '../lifecycleDeadline';
import { playbackStore } from '../state/playback';
import { snapshotSaves, snapshotErrorCode, type SnapshotSaveState } from '../state/snapshotSaves';

function button(key: string, action: () => void): HTMLButtonElement {
    const value = document.createElement('button'); value.type = 'button'; value.textContent = t(key);
    value.addEventListener('click', action); return value;
}
function message(code?: string): string {
    const keys: Record<string, string> = {
        INVALID_SNAPSHOT_NAME: 'invalidName', INVALID_SNAPSHOT_REQUEST: 'invalidRequest', INVALID_SNAPSHOT_CURSOR: 'invalidCursor',
        STALE_INSTANCE: 'stale', STALE_SESSION: 'stale', QUEUE_CONFLICT: 'stale', STALE_MAIN_OCCURRENCE: 'stale',
        SNAPSHOT_OPERATION_REUSED: 'operationReused', SNAPSHOT_NOT_FOUND: 'notFound', SNAPSHOT_STORAGE_FAILED: 'storage',
        SNAPSHOT_CORRUPT: 'corrupt', UNSUPPORTED_SNAPSHOT_VERSION: 'version', PLAYBACK_BUSY: 'busy', TERMINAL_PENDING: 'terminal',
        RESTORE_FAILED: 'restore', DAEMON_STOPPED: 'shutdown', SNAPSHOT_RECOVERY_STORAGE: 'recoveryStorage', SNAPSHOT_NOT_SAVED: 'notSaved',
    };
    return t(`playback.snapshots.${keys[code ?? ''] ?? 'loadError'}`);
}

/** Saved pages have their own identity fence; live polling never reloads them. */
export class PlaybackSnapshots {
    readonly element = document.createElement('section');
    private readonly name = document.createElement('input');
    private readonly save = button('playback.snapshots.save', () => {
        if (this.observed && !this.save.disabled) void snapshotSaves.save(this.observed, this.name.value);
    });
    private readonly browse = button('playback.snapshots.browse', () => this.showList());
    private readonly explanation = document.createElement('p');
    private readonly saveStatus = document.createElement('p');
    private readonly recover = button('playback.snapshots.recover', () => void snapshotSaves.recover());
    private readonly refresh = button('playback.retry', () => void this.refreshLive());
    private readonly openResult = button('playback.snapshots.viewSaved', () => {
        if (this.saveState.snapshot) this.showEntries(this.saveState.snapshot);
    });
    private readonly panel = document.createElement('section');
    private readonly heading = document.createElement('h3');
    private readonly details = document.createElement('p');
    private readonly list = document.createElement('ol');
    private readonly pageStatus = document.createElement('p');
    private readonly back = button('playback.snapshots.live', () => this.showLive());
    private readonly savedList = button('playback.snapshots.browse', () => this.showList());
    private readonly exportName = document.createElement('input');
    private readonly exportButton = button('playback.snapshots.export.action', () => void this.confirmExport());
    private readonly exportStatus = document.createElement('div');
    private readonly retry = button('playback.retry', () => void this.loadPage());
    private readonly first = button('playback.snapshots.first', () => { this.cursor = null; this.previous = []; void this.loadPage(); });
    private readonly prev = button('playback.queue.previous', () => { this.cursor = this.previous.pop() ?? null; void this.loadPage(); });
    private readonly next = button('playback.queue.next', () => {
        if (!this.nextCursor) return;
        this.previous.push(this.cursor); if (this.previous.length > 64) this.previous.shift();
        this.cursor = this.nextCursor; void this.loadPage();
    });
    private observed?: PlaybackSessionSnapshot;
    private saveState: SnapshotSaveState = snapshotSaves.state;
    private selection?: ListeningSnapshotSummary;
    private cursor: string | null = null;
    private nextCursor: string | null = null;
    private previous: (string | null)[] = [];
    private request = 0;
    private loading = false;
    private disposed = false;
    private requiresRefresh = false;
    private readonly unsubscribers: (() => void)[] = [];

    constructor(private readonly onSavedView: (show: boolean) => void) {
        this.element.className = 'playback-snapshots';
        const label = document.createElement('label'); label.textContent = t('playback.snapshots.name');
        this.name.type = 'text'; this.name.value = `${t('playback.snapshots.defaultName')} ${new Date().toLocaleString()}`;
        this.name.setAttribute('aria-label', t('playback.snapshots.name')); label.append(this.name);
        const toolbar = document.createElement('div'); toolbar.className = 'playback-snapshots__actions';
        toolbar.append(label, this.save, this.browse);
        for (const status of [this.saveStatus, this.pageStatus]) { status.setAttribute('role', 'status'); status.setAttribute('aria-live', 'polite'); }
        this.explanation.className = 'playback-snapshots__explanation';
        this.panel.hidden = true; this.panel.className = 'playback-snapshots__panel'; this.heading.tabIndex = -1;
        this.list.className = 'playback-destination__queue playback-snapshots__list';
        const paging = document.createElement('div'); paging.className = 'playback-destination__paging';
        paging.append(this.first, this.prev, this.next);
        const exportLabel=document.createElement('label');exportLabel.textContent=t('playback.snapshots.export.name');this.exportName.maxLength=120;this.exportName.setAttribute('aria-label',t('playback.snapshots.export.name'));exportLabel.append(this.exportName);
        this.exportStatus.setAttribute('role','status');this.exportStatus.setAttribute('aria-live','polite');this.exportStatus.className='playback-snapshots__export-status';
        this.panel.append(this.back, this.savedList, this.heading, this.details, exportLabel, this.exportButton, this.exportStatus, this.pageStatus, this.retry, this.list, paging);
        this.element.append(toolbar, this.explanation, this.saveStatus, this.recover, this.refresh, this.openResult, this.panel);
        this.element.addEventListener('keydown', event => { if (event.key === 'Escape' && !this.panel.hidden) { event.preventDefault(); this.showLive(); } });
        this.unsubscribers.push(playbackStore.subscribe(s => { this.observed = s; this.renderSave(); }));
        this.unsubscribers.push(playbackStore.subscribeConnection(() => this.renderSave()));
        this.unsubscribers.push(snapshotSaves.subscribe(state => {
            this.saveState = state;
            if (state.kind === 'error' && ['STALE_INSTANCE', 'STALE_SESSION', 'QUEUE_CONFLICT', 'STALE_MAIN_OCCURRENCE', 'SNAPSHOT_NOT_SAVED'].includes(state.code ?? ''))
                void this.refreshLive();
            this.renderSave();
        }));
        if (snapshotSaves.pending) void snapshotSaves.recover();
    }
    destroy(): void { this.disposed = true; ++this.request; for (const unsubscribe of this.unsubscribers) unsubscribe(); }
    focus(): boolean { if (this.panel.hidden || this.disposed) return false; this.heading.focus(); return true; }
    private async refreshLive(): Promise<void> {
        this.requiresRefresh = true; this.renderSave();
        try { await playbackStore.refresh(); this.requiresRefresh = false; }
        catch { /* Keep new saves disabled until an explicit refresh succeeds. */ }
        if (!this.disposed) this.renderSave();
    }
    private renderSave(): void {
        if (this.disposed) return;
        const fresh = playbackStore.connection() === 'fresh';
        this.save.disabled = !fresh || this.requiresRefresh || !this.observed || !snapshotSaves.canSave() || this.saveState.kind === 'saving';
        this.name.disabled = !!snapshotSaves.pending;
        this.explanation.textContent = !fresh ? t('playback.snapshots.stale')
            : this.observed?.mode === 'preview' ? t(this.observed.mainCurrent ? 'playback.snapshots.previewMain' : 'playback.snapshots.previewOnly')
                : t('playback.snapshots.localOnly');
        const state = this.saveState;
        const statusText = state.kind === 'idle' ? '' : state.kind === 'saved'
            ? t('playback.snapshots.saved', { name: state.snapshot?.name ?? '', count: state.snapshot?.entryCount ?? '' })
            : state.kind === 'error' ? message(state.code)
                : state.kind === 'empty' ? t(`playback.snapshots.${state.reason}`) : t(`playback.snapshots.${state.kind}`);
        if (this.saveStatus.textContent !== statusText) this.saveStatus.textContent = statusText;
        this.recover.hidden = !snapshotSaves.pending; this.recover.disabled = state.kind === 'saving';
        this.refresh.hidden = fresh && state.kind !== 'error'; this.openResult.hidden = !state.snapshot;
    }
    private reset(): void { ++this.request; this.cursor = null; this.nextCursor = null; this.previous = []; this.list.replaceChildren(); }
    private showLive(): void { ++this.request; this.panel.hidden = true; this.onSavedView(false); this.browse.focus(); }
    showList(): void {
        if (this.disposed) return;
        this.selection = undefined; this.reset(); this.panel.hidden = false; this.savedList.hidden = true;
        this.heading.textContent = t('playback.snapshots.browse'); this.details.textContent = '';
        this.onSavedView(true); this.heading.focus(); void this.loadPage();
    }
    private showEntries(snapshot: ListeningSnapshotSummary): void {
        this.selection = snapshot; this.reset(); this.panel.hidden = false; this.savedList.hidden = false;
        this.heading.textContent = snapshot.name;
        this.details.textContent = `${new Date(snapshot.createdAt).toLocaleString()} · ${t('playback.snapshots.count', { count: snapshot.entryCount })}`;
        this.exportName.value=snapshot.name;this.exportStatus.replaceChildren();
        this.onSavedView(true); this.heading.focus(); void this.loadPage(); void playbackListSnapshotPlaylistExports(snapshot.snapshotId).then(v=>{if(v[0])this.renderExport(v[0]);}).catch(()=>{});
    }
    private async confirmExport():Promise<void>{
        const snapshot=this.selection;if(!snapshot||this.disposed)return;const request=++this.request;this.exportButton.disabled=true;this.exportStatus.textContent=t('playback.snapshots.export.planning');
        try{const plan=await playbackPlanSnapshotPlaylistExport(snapshot.snapshotId,this.exportName.value);if(this.disposed||request!==this.request||this.selection?.snapshotId!==snapshot.snapshotId)return;const parts=plan.parts.map(p=>`${p.sourceLabel}: ${p.expectedCount}`).join('\n');if(!window.confirm(t('playback.snapshots.export.confirm',{name:plan.name,parts})))return;this.renderExport(await playbackStartSnapshotPlaylistExport(snapshot.snapshotId,plan.name));}
        catch(error){this.exportStatus.textContent=message(snapshotErrorCode(error));}finally{if(!this.disposed&&request===this.request)this.exportButton.disabled=false;}
    }
    private renderExport(operation:SnapshotPlaylistExport):void{
        if(this.disposed||operation.snapshotId!==this.selection?.snapshotId)return;const list=document.createElement('ul');for(const part of operation.parts){const row=document.createElement('li');row.append(`${part.sourceLabel}: ${t(`playback.snapshots.export.state.${part.state}`)} · ${part.confirmedCount}/${part.expectedCount}${part.playlistId?` · ${part.playlistId}`:''}`);if(part.safeToRetry&&operation.operationId)row.append(button('playback.retry',()=>void playbackRetrySnapshotPlaylistExport(operation.operationId!,part.serverId).then(v=>this.renderExport(v))));if(part.state==='ambiguous'&&operation.operationId)row.append(button('playback.snapshots.export.reconcile',()=>void playbackReconcileSnapshotPlaylistExport(operation.operationId!,part.serverId).then(v=>this.renderExport(v))));list.append(row);}this.exportStatus.replaceChildren(document.createTextNode(t(`playback.snapshots.export.aggregate.${operation.status}`)),list);
    }
    private paging(): void {
        this.first.disabled = this.loading || this.cursor === null;
        this.prev.disabled = this.loading || !this.previous.length; this.next.disabled = this.loading || !this.nextCursor;
    }
    private async loadPage(): Promise<void> {
        const request = ++this.request; const snapshotId = this.selection?.snapshotId;
        const current = () => !this.disposed && !this.panel.hidden && request === this.request && snapshotId === this.selection?.snapshotId;
        this.loading = true; this.retry.hidden = true; this.pageStatus.textContent = t('playback.snapshots.loading'); this.paging();
        try {
            const rows: HTMLLIElement[] = [];
            if (snapshotId) {
                const page = await withDeadline(playbackListSnapshotEntries(snapshotId, this.cursor, 50), 15_000, 'SNAPSHOT_UNKNOWN'); if (!current()) return;
                if (page.snapshotId !== snapshotId) throw new Error('snapshot mismatch');
                this.nextCursor = page.nextCursor;
                for (const entry of page.entries) {
                    const row = document.createElement('li'); row.className = 'playback-destination__row'; row.dataset.occurrenceId = entry.occurrenceId;
                    const info = document.createElement('div'); info.className = 'playback-snapshots__track';
                    const title = document.createElement('strong'); title.textContent = entry.title ?? `${t('playback.unknown_track')} · ${entry.source.trackId}`;
                    const detail = document.createElement('span'); detail.textContent = [entry.artist, entry.album].filter(Boolean).join(' · ');
                    const badge = document.createElement('span'); badge.className = 'playback-destination__source'; badge.textContent = entry.sourceLabel;
                    if (entry.sourceIcon) { const icon = document.createElement('sl-icon'); icon.setAttribute('name', entry.sourceIcon); icon.setAttribute('aria-hidden', 'true'); badge.append(icon); }
                    const ordinal = document.createElement('span'); ordinal.textContent = (BigInt(entry.ordinal) + 1n).toString(); info.append(title, detail, badge);
                    if (!entry.sourceAvailable) { const missing = document.createElement('span'); missing.textContent = t('playback.snapshots.sourceUnavailable'); info.append(missing); }
                    row.append(ordinal, info); rows.push(row);
                }
            } else {
                const page = await withDeadline(playbackListSnapshots(this.cursor, 50), 15_000, 'SNAPSHOT_UNKNOWN'); if (!current()) return; this.nextCursor = page.nextCursor;
                for (const snapshot of page.snapshots) {
                    const row = document.createElement('li'); const open = button('playback.snapshots.viewSaved', () => this.showEntries(snapshot));
                    open.textContent = `${snapshot.name} · ${t('playback.snapshots.count', { count: snapshot.entryCount })} · ${new Date(snapshot.createdAt).toLocaleString()}`;
                    row.append(open); rows.push(row);
                }
            }
            this.list.replaceChildren(...rows); this.pageStatus.textContent = rows.length ? '' : t('playback.snapshots.none');
        } catch (error) {
            if (!current()) return;
            this.nextCursor = null;
            this.list.replaceChildren(); this.pageStatus.textContent = message(snapshotErrorCode(error)); this.retry.hidden = false;
        } finally { if (current()) { this.loading = false; this.paging(); } }
    }
}
