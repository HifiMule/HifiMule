import { playbackGetSession, type PlaybackSessionSnapshot } from '../rpc';

export type PlaybackSubscriber = (snapshot: PlaybackSessionSnapshot, previous?: PlaybackSessionSnapshot) => void;

export class PlaybackStore {
    private snapshot?: PlaybackSessionSnapshot;
    private subscribers = new Set<PlaybackSubscriber>();
    private heartbeats = new Map<PlaybackSubscriber, () => void>();
    private timer?: ReturnType<typeof setTimeout>;
    private polling = false;

    constructor(private readonly load: () => Promise<PlaybackSessionSnapshot> = playbackGetSession) {}

    current(): PlaybackSessionSnapshot | undefined { return this.snapshot; }

    accept(next: PlaybackSessionSnapshot): boolean {
        const previous = this.snapshot;
        if (previous && next.instanceId === previous.instanceId && next.sessionId === previous.sessionId
            && BigInt(next.stateSequence) <= BigInt(previous.stateSequence)) return false;
        this.snapshot = next;
        for (const subscriber of this.subscribers) subscriber(next, previous);
        return true;
    }

    subscribe(subscriber: PlaybackSubscriber, heartbeat?: () => void): () => void {
        this.subscribers.add(subscriber);
        if (heartbeat) this.heartbeats.set(subscriber, heartbeat);
        if (this.snapshot) subscriber(this.snapshot);
        if (!this.polling) void this.poll();
        return () => {
            this.subscribers.delete(subscriber);
            this.heartbeats.delete(subscriber);
            if (this.subscribers.size === 0 && this.timer !== undefined) {
                clearTimeout(this.timer);
                this.timer = undefined;
                this.polling = false;
            }
        };
    }

    async refresh(): Promise<PlaybackSessionSnapshot> {
        const snapshot = await this.load();
        this.accept(snapshot);
        for (const heartbeat of this.heartbeats.values()) heartbeat();
        return this.snapshot ?? snapshot;
    }

    private async poll(): Promise<void> {
        this.polling = true;
        try { await this.refresh(); } catch { /* lifecycle UI owns daemon availability */ }
        if (this.subscribers.size > 0) {
            this.timer = globalThis.setTimeout(() => void this.poll(), 500);
        } else {
            this.polling = false;
        }
    }
}

export const playbackStore = new PlaybackStore();
