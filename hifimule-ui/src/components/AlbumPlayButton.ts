import { playbackPlayAlbum } from '../rpc';
import { t } from '../i18n';
import { playbackAction } from './playbackAction';

export function createAlbumPlayButton(albumId: string, serverId: string | undefined, title: string, kind: 'album' | 'book' = 'album'): HTMLElement & { disabled: boolean } {
    return playbackAction({
        icon: 'play-fill',
        label: t(kind === 'book' ? 'library.books.play_book' : 'playback.play_album', { title }),
        run: serverId ? () => playbackPlayAlbum(serverId, albumId) : null,
    });
}
