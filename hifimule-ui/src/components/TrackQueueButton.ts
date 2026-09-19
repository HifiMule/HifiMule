import { playbackAppendQueue } from '../rpc';
import { t } from '../i18n';
import { showToast } from '../toast';

export function createTrackQueueButton(
    serverId: string | null | undefined,
    trackId: string,
    title: string,
): HTMLElement {
    const button = document.createElement('sl-icon-button') as any;
    const source = serverId ? { serverId, trackId } : null;
    button.name = 'list-ul';
    button.label = t('playback.add_to_queue', { title });
    button.disabled = !source;
    button.dataset.queueAdd = trackId;
    button.addEventListener('mousedown', (event: Event) => event.stopPropagation());
    button.addEventListener('click', async (event: Event) => {
        event.stopPropagation();
        if (!source || button.disabled) return;
        button.disabled = true;
        try {
            await playbackAppendQueue([{ ...source }]);
            showToast(t('playback.queue_add_success', { count: 1 }), 'success');
        } catch (error) {
            showToast((error as Error).message, 'danger');
        } finally {
            button.disabled = false;
        }
    });
    return button;
}
