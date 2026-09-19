import { isPlaybackQueueConflict, playbackAppendQueue, RpcError, type PlaybackTrackSource } from '../rpc';
import { t } from '../i18n';
import { showToast, ERROR_TOAST_DURATION } from '../toast';
import { playbackStore } from './playback';

/** One explicit library action; refreshing state never replays its mutation. */
export async function appendTracksToQueueFromLibrary(sources: PlaybackTrackSource[]): Promise<void> {
    const captured = sources.map(source => ({ ...source }));
    try {
        await playbackAppendQueue(captured);
    } catch (error) {
        if (isPlaybackQueueConflict(error)) {
            let refreshed = false;
            try { await playbackStore.refresh(); refreshed = true; } catch { /* Keep the original conflict actionable. */ }
            showToast(t(refreshed ? 'playback.queue.conflict' : 'playback.queue.conflict_refresh_failed'),
                'warning', ERROR_TOAST_DURATION);
        } else {
            const data = error instanceof RpcError && error.data && typeof error.data === 'object'
                ? error.data as Record<string, unknown> : undefined;
            showToast(t(data?.code === 'QUEUE_LIMIT_EXCEEDED'
                ? 'playback.queue.limit_exceeded' : 'playback.command_error'), 'danger', ERROR_TOAST_DURATION);
        }
        return;
    }

    // The append is already committed. A failed read must not suggest adding it again.
    try {
        await playbackStore.refresh();
        showToast(t('playback.queue_add_success', { count: captured.length }), 'success');
    } catch {
        showToast(t('playback.queue.added_refresh_failed', { count: captured.length }),
            'warning', ERROR_TOAST_DURATION);
    }
}
