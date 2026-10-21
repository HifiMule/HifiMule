import { appendTracksToQueueFromLibrary } from '../state/queue';
import { t } from '../i18n';
import { playbackAction } from './playbackAction';

export function createTrackQueueButton(
    serverId: string | null | undefined,
    trackId: string,
    title: string,
): HTMLElement {
    const button = playbackAction({
        icon: 'list-ul',
        label: t('playback.add_to_queue', { title }),
        run: serverId ? () => appendTracksToQueueFromLibrary([{ serverId, trackId }]) : null,
    });
    button.dataset.queueAdd = trackId;
    return button;
}
