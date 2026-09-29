import { playbackListSnapshots } from './rpc';
import { snapshotSaves } from './state/snapshotSaves';
import { withDeadline } from './lifecycleDeadline';

/** A failed local read must not erase access to recoverable user content. */
export async function localContentRoute(serverCount: number): Promise<'main' | 'onboarding' | 'error'> {
    if (serverCount > 0) return 'main';
    try {
        const saved = await withDeadline(playbackListSnapshots(null, 1), 15_000, 'SNAPSHOT_UNKNOWN');
        if (saved.snapshots.length || snapshotSaves.pending) return 'main';
        return snapshotSaves.state?.code === 'SNAPSHOT_RECOVERY_STORAGE' ? 'error' : 'onboarding';
    }
    catch { return 'error'; }
}
