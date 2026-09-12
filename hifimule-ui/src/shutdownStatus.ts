export type ShutdownProgress = { deadlineExceeded: boolean; phase?: string };

export function shutdownMessageKey(
    shutdown: ShutdownProgress,
    errorCode: string | null | undefined,
): 'lifecycle.shutdown_delayed' | 'lifecycle.shutdown_progress' | 'lifecycle.quit_persistence_failed' | 'lifecycle.fencing_waiting' | 'lifecycle.fencing_delayed' {
    if (errorCode === 'QUIT_PERSISTENCE_FAILED') return 'lifecycle.quit_persistence_failed';
    if (shutdown.phase === 'fencing') return shutdown.deadlineExceeded
        ? 'lifecycle.fencing_delayed' : 'lifecycle.fencing_waiting';
    return shutdown.deadlineExceeded || errorCode === 'SHUTDOWN_TIMEOUT'
        ? 'lifecycle.shutdown_delayed'
        : 'lifecycle.shutdown_progress';
}

/** Prevent overlapping health requests while still allowing explicit refresh. */
export class ShutdownPollGate {
    private pending = false;

    constructor(private readonly request: () => Promise<void>) {}

    async poll(): Promise<void> {
        if (this.pending) return;
        this.pending = true;
        try {
            await this.request();
        } finally {
            this.pending = false;
        }
    }
}

/** One timer, one request, at least one second between starts (including Refresh). */
export class ShutdownPoller {
    private timer: ReturnType<typeof setTimeout> | undefined;
    private pending = false;
    private disposed = false;
    private lastStart: number;
    constructor(private readonly request: () => Promise<void>,
        private readonly now = () => performance.now(),
        // WebKit requires the Window receiver; never store unbound browser timers.
        private readonly schedule = (callback: () => void, delay: number) => globalThis.setTimeout(callback, delay),
        private readonly cancel = (timer: ReturnType<typeof setTimeout>) => globalThis.clearTimeout(timer)) {
        this.lastStart = now();
        this.refresh();
    }
    refresh(): void {
        if (this.disposed || this.pending || this.timer !== undefined) return;
        this.timer = this.schedule(() => {
            this.timer = undefined;
            void this.run();
        }, Math.max(0, 1_000 - (this.now() - this.lastStart)));
    }
    private async run(): Promise<void> {
        if (this.disposed || this.pending) return;
        this.pending = true;
        this.lastStart = this.now();
        try { await this.request(); }
        finally { this.pending = false; this.refresh(); }
    }
    dispose(): void {
        this.disposed = true;
        if (this.timer !== undefined) this.cancel(this.timer);
        this.timer = undefined;
    }
}

export function canRetryQuit(health: { errorCode?: string | null; shutdown?: { phase: string } | null }): boolean {
    return health.shutdown?.phase === 'fenceFailed' && health.errorCode === 'QUIT_PERSISTENCE_FAILED';
}
