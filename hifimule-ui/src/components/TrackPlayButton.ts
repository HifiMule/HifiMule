import { playbackPlayTrack } from '../rpc';
import { t } from '../i18n';
import { playbackAction } from './playbackAction';

export function createTrackPlayButton(
    serverId: string | null | undefined,
    trackId: string,
    title: string,
    kind: 'track' | 'part' = 'track',
): HTMLElement {
    return playbackAction({
        icon: 'play-fill',
        label: t(kind === 'part' ? 'library.books.play_part' : 'playback.play_track', { title }),
        run: serverId ? () => playbackPlayTrack(serverId, trackId) : null,
    });
}
