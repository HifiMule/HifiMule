export type ShutdownProgress = { deadlineExceeded: boolean };

export function shutdownMessageKey(
    shutdown: ShutdownProgress,
    errorCode: string | null | undefined,
): 'lifecycle.shutdown_delayed' | 'lifecycle.shutdown_progress' | 'lifecycle.quit_persistence_failed' {
    if (errorCode === 'QUIT_PERSISTENCE_FAILED') return 'lifecycle.quit_persistence_failed';
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
