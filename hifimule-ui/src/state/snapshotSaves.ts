import { playbackGetSession, playbackGetSnapshot, playbackSaveSnapshot,
    type ListeningSnapshotSummary, type PlaybackSessionSnapshot, type SaveSnapshotRequest, type SaveSnapshotResult } from '../rpc';
import { withDeadline } from '../lifecycleDeadline';

const KEY = 'hifimule.snapshotSave.v1';
export type SnapshotSaveState = { kind: 'idle' | 'saving' | 'unknown' | 'saved' | 'empty' | 'error';
    snapshot?: ListeningSnapshotSummary; code?: string; reason?: string };
export function snapshotErrorCode(error: unknown): string {
    const data = (error as { data?: { code?: string; errorCode?: string } })?.data;
    return data?.code ?? data?.errorCode ?? 'SNAPSHOT_UNKNOWN';
}
function validRequest(p: SaveSnapshotRequest): boolean {
    const uuid = (s: unknown) => typeof s === 'string' && /^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$/i.test(s);
    return !!p && p.schemaVersion === 1 && uuid(p.operationId) && uuid(p.instanceId) && uuid(p.sessionId)
        && (p.expectedMainOccurrenceId === null || uuid(p.expectedMainOccurrenceId))
        && typeof p.expectedQueueRevision === 'string' && /^(0|[1-9][0-9]{0,18})$/.test(p.expectedQueueRevision)
        && BigInt(p.expectedQueueRevision) <= 9223372036854775807n
        && (p.name === undefined || typeof p.name === 'string' && [...p.name.trim()].length <= 120 && !/[\u0000-\u001f\u007f-\u009f]/u.test(p.name))
        && Object.keys(p).every(k => ['schemaVersion', 'operationId', 'instanceId', 'sessionId', 'expectedQueueRevision', 'expectedMainOccurrenceId', 'name'].includes(k));
}

/** One bounded, durable intent. Completion belongs to this coordinator, not a view. */
export class SnapshotSaveCoordinator {
    pending: SaveSnapshotRequest | null = null;
    state: SnapshotSaveState = { kind: 'idle' };
    private listeners = new Set<(state: SnapshotSaveState) => void>();
    private inFlight?: Promise<void>;
    private storageBroken = false;
    private readonly storage?: Pick<Storage, 'getItem' | 'setItem' | 'removeItem'>;
    constructor(storage?: Pick<Storage, 'getItem' | 'setItem' | 'removeItem'>) {
        try {
            this.storage = storage ?? globalThis.localStorage;
            const raw = this.storage.getItem(KEY);
            if (raw) {
                if (raw.length > 4096) throw new Error('oversized recovery');
                const record = JSON.parse(raw);
                if (record.version !== 1 || !validRequest(record.request)) throw new Error('invalid recovery');
                this.pending = record.request;
                this.state = { kind: 'unknown' };
            }
        } catch { this.storageBroken = true; this.state = { kind: 'error', code: 'SNAPSHOT_RECOVERY_STORAGE' }; }
    }
    canSave(): boolean { return !this.storageBroken && !this.pending; }
    subscribe(listener: (state: SnapshotSaveState) => void): () => void {
        this.listeners.add(listener); listener(this.state); return () => this.listeners.delete(listener);
    }
    private publish(state: SnapshotSaveState): void { this.state = state; for (const listener of this.listeners) listener(state); }
    private run(work: () => Promise<void>): Promise<void> {
        if (this.inFlight) return this.inFlight;
        const job = work(); this.inFlight = job;
        void job.finally(() => { if (this.inFlight === job) this.inFlight = undefined; });
        return job;
    }
    private clear(): boolean {
        try { this.storage!.removeItem(KEY); this.pending = null; return true; }
        catch { return false; } // A committed success is still success; keep its recoverable identity.
    }
    private complete(result: SaveSnapshotResult): void {
        this.clear();
        this.publish(result.status === 'saved' ? { kind: 'saved', snapshot: result.snapshot }
            : { kind: 'empty', reason: result.reason });
    }
    save(observed: PlaybackSessionSnapshot, name: string): Promise<void> {
        if (this.pending || this.storageBroken || this.inFlight) return Promise.resolve();
        const request: SaveSnapshotRequest = { schemaVersion: 1, operationId: crypto.randomUUID(), instanceId: observed.instanceId,
            sessionId: observed.sessionId, expectedQueueRevision: observed.queueRevision,
            expectedMainOccurrenceId: observed.mainCurrent?.occurrenceId ?? null, name };
        if (!validRequest(request)) { this.publish({ kind: 'error', code: 'INVALID_SNAPSHOT_NAME' }); return Promise.resolve(); }
        request.name = name.trim();
        try { this.storage!.setItem(KEY, JSON.stringify({ version: 1, request })); }
        catch { this.publish({ kind: 'error', code: 'SNAPSHOT_RECOVERY_STORAGE' }); return Promise.resolve(); }
        this.pending = request;
        return this.run(() => this.send(request));
    }
    private async send(request: SaveSnapshotRequest): Promise<void> {
        this.publish({ kind: 'saving' });
        try { this.complete(await withDeadline(playbackSaveSnapshot(request), 15_000, 'SNAPSHOT_UNKNOWN')); }
        catch (error) {
            const code = snapshotErrorCode(error);
            // These are definitive owner/validation rejections, following the
            // committed-operation lookup. Busy and transport/storage failures
            // cannot establish that a previous, disconnected call did not commit.
            if (['INVALID_SNAPSHOT_REQUEST', 'INVALID_SNAPSHOT_NAME', 'STALE_INSTANCE', 'STALE_SESSION',
                'QUEUE_CONFLICT', 'STALE_MAIN_OCCURRENCE', 'RESTORE_FAILED', 'TERMINAL_PENDING', 'SNAPSHOT_OPERATION_REUSED'].includes(code)) {
                this.clear(); this.publish({ kind: 'error', code });
            } else this.publish({ kind: 'unknown', code });
        }
    }
    recover(): Promise<void> {
        return this.run(async () => {
            const request = this.pending; if (!request) return;
            this.publish({ kind: 'saving' });
            try {
                try {
                    const snapshot = await withDeadline(playbackGetSnapshot({ operationId: request.operationId }), 15_000, 'SNAPSHOT_UNKNOWN');
                    this.complete({ schemaVersion: 1, status: 'saved', snapshot }); return;
                } catch (error) { if (snapshotErrorCode(error) !== 'SNAPSHOT_NOT_FOUND') throw error; }
                const live = await withDeadline(playbackGetSession(), 15_000, 'SNAPSHOT_UNKNOWN');
                if (live.instanceId !== request.instanceId) {
                    // Recheck on the observed new daemon: the first lookup may
                    // have preceded a late commit by the previous owner.
                    try {
                        const snapshot = await withDeadline(playbackGetSnapshot({ operationId: request.operationId }), 15_000, 'SNAPSHOT_UNKNOWN');
                        this.complete({ schemaVersion: 1, status: 'saved', snapshot });
                    } catch (error) {
                        if (snapshotErrorCode(error) !== 'SNAPSHOT_NOT_FOUND') throw error;
                        this.clear(); this.publish({ kind: 'error', code: 'SNAPSHOT_NOT_SAVED' });
                    }
                } else await this.send(request); // Exact identity, never a fresh implicit save.
            } catch (error) { this.publish({ kind: 'unknown', code: snapshotErrorCode(error) }); }
        });
    }
}
export const snapshotSaves = new SnapshotSaveCoordinator();
