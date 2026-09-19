import { appendTracksToQueueFromLibrary } from '../state/queue';
import { t } from '../i18n';

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
            await appendTracksToQueueFromLibrary([{ ...source }]);
        } finally {
            button.disabled = false;
        }
    });
    return button;
}
