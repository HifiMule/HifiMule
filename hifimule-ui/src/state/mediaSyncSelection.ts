import type { BrowseTrack, PodcastEpisode } from '../rpc';
import type { BasketItem } from './basket';

const PODCAST_ESTIMATED_BYTES_PER_SECOND = 16_000;
const UNKNOWN_PODCAST_DURATION_SECONDS = 3_600;

function bookPartEstimatedBytes(track: BrowseTrack): number {
    return track.sizeBytes ?? (track.duration > 0 ? track.duration : 3_600) * PODCAST_ESTIMATED_BYTES_PER_SECOND;
}

export function bookBasketItem(id: string, name: string, serverId: string | undefined,
    artist: string | undefined, tracks: BrowseTrack[]): BasketItem {
    return {
        id, name, type: 'Book', serverId, artist,
        childCount: tracks.length,
        sizeTicks: tracks.reduce((sum, track) => sum + track.duration * 10_000_000, 0),
        sizeBytes: tracks.reduce((sum, track) => sum + bookPartEstimatedBytes(track), 0),
    };
}

export function podcastEpisodeBasketItem(episode: PodcastEpisode, serverId: string): BasketItem {
    const duration = episode.durationSeconds && episode.durationSeconds > 0
        ? episode.durationSeconds : UNKNOWN_PODCAST_DURATION_SECONDS;
    return {
        id: episode.id, name: episode.title, type: 'PodcastEpisode', serverId,
        childCount: 1,
        sizeTicks: duration * 10_000_000,
        sizeBytes: duration * PODCAST_ESTIMATED_BYTES_PER_SECOND,
    };
}

export function podcastShowBasketItem(id: string, name: string, serverId: string,
    episodes: PodcastEpisode[]): BasketItem {
    const duration = episodes.reduce((sum, episode) => sum + (
        episode.durationSeconds && episode.durationSeconds > 0
            ? episode.durationSeconds : UNKNOWN_PODCAST_DURATION_SECONDS
    ), 0);
    return {
        id, name, type: 'PodcastShow', serverId,
        childCount: episodes.length,
        sizeTicks: duration * 10_000_000,
        sizeBytes: duration * PODCAST_ESTIMATED_BYTES_PER_SECOND,
    };
}
