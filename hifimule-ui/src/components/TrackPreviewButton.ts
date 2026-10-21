import { playbackPreviewTrack } from '../rpc';
import { t } from '../i18n';
import { playbackAction } from './playbackAction';

export function createTrackPreviewButton(
    serverId: string | null | undefined,
    trackId: string,
    title: string,
): HTMLElement {
    return playbackAction({
        icon: 'soundwave',
        label: t('playback.preview_track', { title }),
        run: serverId ? () => playbackPreviewTrack(serverId, trackId) : null,
    });
}
