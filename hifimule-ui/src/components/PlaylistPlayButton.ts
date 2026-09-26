import { playbackPlayPlaylist } from '../rpc';
import { t } from '../i18n';
import { showToast } from '../toast';

export function createPlaylistPlayButton(playlistId: string, serverId: string | undefined, title: string, trackCount?: number): HTMLElement {
    const play = document.createElement('sl-icon-button') as HTMLElement & { name: string; label: string; disabled: boolean };
    play.name = 'play-fill';
    play.label = t('playback.play_playlist', { title });
    play.disabled = !serverId || trackCount === 0;
    play.addEventListener('mousedown', event => event.stopPropagation());
    play.addEventListener('click', async event => {
        event.stopPropagation();
        if (!serverId || play.disabled) return;
        play.disabled = true;
        try { await playbackPlayPlaylist(serverId, playlistId); }
        catch (error) { showToast((error as Error).message, 'danger'); }
        finally { play.disabled = false; }
    });
    return play;
}
