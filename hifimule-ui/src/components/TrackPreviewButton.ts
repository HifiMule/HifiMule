import { playbackPreviewTrack } from '../rpc';
import { t } from '../i18n';
import { showToast } from '../toast';

export function createTrackPreviewButton(
    serverId: string | null | undefined,
    trackId: string,
    title: string,
): HTMLElement {
    const button = document.createElement('sl-icon-button') as any;
    const source = serverId ? { serverId, trackId } : null;
    button.name = 'soundwave';
    button.label = t('playback.preview_track', { title });
    button.disabled = !source;
    button.addEventListener('mousedown', (event: Event) => event.stopPropagation());
    button.addEventListener('click', async (event: Event) => {
        event.stopPropagation();
        if (!source) return;
        try {
            await playbackPreviewTrack(source.serverId, source.trackId);
        } catch (error) {
            showToast((error as Error).message, 'danger');
        }
    });
    return button;
}
