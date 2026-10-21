import { playbackPlayPlaylist } from '../rpc';
import { t } from '../i18n';
import { playbackAction } from './playbackAction';

export function createPlaylistPlayButton(playlistId: string, serverId: string | undefined, title: string, trackCount?: number): HTMLElement {
    return playbackAction({
        icon: 'play-fill',
        label: t('playback.play_playlist', { title }),
        run: serverId && trackCount !== 0 ? () => playbackPlayPlaylist(serverId, playlistId) : null,
    });
}
