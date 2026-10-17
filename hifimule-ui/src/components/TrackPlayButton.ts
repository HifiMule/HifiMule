import { playbackPlayTrack } from '../rpc';
import { t } from '../i18n';
import { showToast } from '../toast';

export function createTrackPlayButton(
    serverId: string | null | undefined,
    trackId: string,
    title: string,
    kind: 'track' | 'part' = 'track',
): HTMLElement {
    const button = document.createElement('sl-icon-button') as any;
    const source = serverId ? { serverId, trackId } : null;
    button.name = 'play-fill';
    button.label = t(kind === 'part' ? 'library.books.play_part' : 'playback.play_track', { title });
    button.disabled = !source;
    button.addEventListener('mousedown', (event: Event) => event.stopPropagation());
    button.addEventListener('click', async (event: Event) => {
        event.stopPropagation();
        if (!source || button.disabled) return;
        button.disabled = true;
        button.setAttribute('aria-busy', 'true');
        button.name = 'hourglass-split';
        try {
            await playbackPlayTrack(source.serverId, source.trackId);
        } catch (error) {
            showToast((error as Error).message, 'danger');
        } finally {
            button.disabled = false;
            button.removeAttribute('aria-busy');
            button.name = 'play-fill';
        }
    });
    return button;
}
