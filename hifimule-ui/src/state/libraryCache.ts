import type { BrowseDisplayItem } from '../components/MediaCard';

// Shared data only: callers must not import the library UI to update this state.
export const libraryPageCache = new Map<string, {
    items: BrowseDisplayItem[];
    total: number;
    chapters?: Array<{ startSeconds: number; endSeconds: number }>;
}>();

export let supportsPlaylistWrite = false;

export function setPlaylistWriteCapability(value: boolean): void {
    supportsPlaylistWrite = value;
}

// Keep unrelated browse pages when a playlist is created or edited.
export function invalidatePlaylistsCache(): void {
    for (const key of libraryPageCache.keys()) {
        if (key.startsWith('playlists:')) libraryPageCache.delete(key);
    }
}
