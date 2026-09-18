import { playbackPlayAlbum } from '../rpc';
import { t } from '../i18n';
import { showToast } from '../toast';

export function createAlbumPlayButton(albumId: string, serverId: string | undefined, title: string): HTMLElement & { disabled: boolean } {
    const play = document.createElement('sl-icon-button') as HTMLElement & { name: string; label: string; disabled: boolean };
    play.name = 'play-fill';
    play.label = t('playback.play_album', { title });
    play.disabled = !serverId;
    const source = serverId ? { serverId, albumId } : null;
    play.addEventListener('mousedown', event => event.stopPropagation());
    play.addEventListener('click', async event => {
        event.stopPropagation();
        if (!source) return;
        try { await playbackPlayAlbum(source.serverId, source.albumId); }
        catch (error) { showToast((error as Error).message, 'danger'); }
    });
    return play;
}
