import { playbackGetSession, type PlaybackSessionSnapshot } from '../rpc';

export type PlaybackSubscriber = (snapshot: PlaybackSessionSnapshot, previous?: PlaybackSessionSnapshot) => void;
export type PlaybackConnection = 'connecting' | 'fresh' | 'disconnected' | 'stale';

export class PlaybackStore {
    private snapshot?: PlaybackSessionSnapshot;
    private subscribers = new Set<PlaybackSubscriber>();
    private heartbeats = new Map<PlaybackSubscriber, () => void>();
    private connectionSubscribers = new Set<(state: PlaybackConnection) => void>();
    private connectionState: PlaybackConnection = 'connecting';
    private timer?: ReturnType<typeof setTimeout>;
    private request = 0;
    private pending?: Promise<PlaybackSessionSnapshot>;
    private lastResponseAt = 0;

    constructor(private readonly load: () => Promise<PlaybackSessionSnapshot> = playbackGetSession) {}

    current(): PlaybackSessionSnapshot | undefined { return this.snapshot; }
    connection(): PlaybackConnection { return this.connectionState; }
    subscribeConnection(subscriber: (state: PlaybackConnection) => void): () => void {
        this.connectionSubscribers.add(subscriber);
        subscriber(this.connectionState);
        return () => { this.connectionSubscribers.delete(subscriber); };
    }
    private setConnection(state: PlaybackConnection): void {
        if (state === this.connectionState) return;
        this.connectionState = state;
        for (const subscriber of this.connectionSubscribers) subscriber(state);
    }

    accept(next: PlaybackSessionSnapshot): boolean {
        const previous = this.snapshot;
        if (previous && next.instanceId === previous.instanceId && next.sessionId === previous.sessionId
            && BigInt(next.stateSequence) <= BigInt(previous.stateSequence)) return false;
        this.snapshot = next;
        for (const subscriber of this.subscribers) subscriber(next, previous);
        return true;
    }

    /** Command replies cannot introduce an owner or replace another occurrence. */
    acceptCommand(next: PlaybackSessionSnapshot, observed: PlaybackSessionSnapshot): boolean {
        const current = this.snapshot;
        if (!current || this.connectionState !== 'fresh') return false;
        const identity = (value: PlaybackSessionSnapshot) => JSON.stringify([
            value.instanceId, value.sessionId, value.generationId, value.current?.occurrenceId,
        ]);
        if (identity(current) !== identity(observed)
            || next.instanceId !== observed.instanceId || next.sessionId !== observed.sessionId
            || next.current?.occurrenceId !== observed.current?.occurrenceId
            || BigInt(next.stateSequence) <= BigInt(current.stateSequence)) return false;
        // Invalidate a read begun before this command result, even if its owner differs.
        ++this.request;
        this.pending = undefined;
        return this.accept(next);
    }

    subscribe(subscriber: PlaybackSubscriber, heartbeat?: () => void): () => void {
        const first = this.subscribers.size === 0;
        this.subscribers.add(subscriber);
        if (heartbeat) this.heartbeats.set(subscriber, heartbeat);
        if (this.snapshot) subscriber(this.snapshot);
        if (first) {
            this.lastResponseAt = Date.now();
            this.setConnection('connecting');
            this.schedule();
            void this.refresh().catch(() => {});
        }
        return () => {
            this.subscribers.delete(subscriber);
            this.heartbeats.delete(subscriber);
            if (this.subscribers.size === 0) {
                if (this.timer !== undefined) clearTimeout(this.timer);
                this.timer = undefined;
                ++this.request;
                this.pending = undefined;
            }
        };
    }

    /** Explicit refresh supersedes an old/hung read. Polls coalesce with the current read. */
    refresh(supersede = true): Promise<PlaybackSessionSnapshot> {
        if (!supersede && this.pending) return this.pending;
        const request = ++this.request;
        const pending: Promise<PlaybackSessionSnapshot> = this.load().then((snapshot): PlaybackSessionSnapshot | Promise<PlaybackSessionSnapshot> => {
            if (request !== this.request) {
                if (this.pending && this.pending !== pending) return this.pending;
                if (this.snapshot) return this.snapshot;
                throw new Error('Playback read superseded');
            }
            this.lastResponseAt = Date.now();
            this.accept(snapshot);
            this.setConnection('fresh');
            for (const heartbeat of this.heartbeats.values()) heartbeat();
            return this.snapshot ?? snapshot;
        }, error => {
            if (request === this.request) this.setConnection('disconnected');
            throw error;
        }).finally(() => {
            if (request === this.request) this.pending = undefined;
        });
        this.pending = pending;
        return pending;
    }

    private schedule(): void {
        this.timer = globalThis.setTimeout(() => {
            this.timer = undefined;
            if (this.subscribers.size === 0) return;
            if (Date.now() - this.lastResponseAt >= 2000 && this.connectionState !== 'disconnected') {
                this.setConnection('stale');
            }
            if (!this.pending) void this.refresh(false).catch(() => {});
            this.schedule();
        }, 500);
    }
}

export const playbackStore = new PlaybackStore();
