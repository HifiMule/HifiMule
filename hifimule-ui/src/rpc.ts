import { invoke } from '@tauri-apps/api/core';
import { t } from './i18n';

export const RPC_PORT = (import.meta as any).env?.VITE_RPC_PORT || '19140';
export const RPC_URL = `http://localhost:${RPC_PORT}`;
export const IMAGE_PROXY_URL = `${RPC_URL}/jellyfin/image`;

function getErrorMessage(error: unknown): string {
    const localized = localizeKnownRpcError(error);
    if (localized) return localized;

    if (error instanceof Error && error.message.trim()) {
        return error.message;
    }

    if (typeof error === 'string' && error.trim()) {
        return error;
    }

    if (error && typeof error === 'object') {
        const record = error as Record<string, unknown>;
        for (const key of ['message', 'error', 'details']) {
            const value = record[key];
            if (typeof value === 'string' && value.trim()) {
                return value;
            }
        }

        try {
            const serialized = JSON.stringify(error);
            if (serialized && serialized !== '{}') {
                return serialized;
            }
        } catch {
            // Fall through to generic message.
        }
    }

    return t('error.unknown_rpc');
}

function localizeKnownRpcError(error: unknown): string | null {
    const message = rawErrorMessage(error);
    if (!message) return null;

    if (
        message === 'Unknown server type at this URL'
        || message === 'provider capability is unsupported: Unknown server type at this URL'
    ) {
        return t('error.unknown_server_type');
    }

    return null;
}

function rawErrorMessage(error: unknown): string | null {
    if (error instanceof Error && error.message.trim()) {
        return error.message;
    }
    if (typeof error === 'string' && error.trim()) {
        return error;
    }
    if (error && typeof error === 'object') {
        const record = error as Record<string, unknown>;
        for (const key of ['message', 'error', 'details']) {
            const value = record[key];
            if (typeof value === 'string' && value.trim()) {
                return value;
            }
        }
    }
    return null;
}

export async function rpcCall(method: string, params: any = {}): Promise<any> {
    console.log(`RPC Call: ${method}`, params);
    // Use Tauri invoke to proxy RPC calls through the Rust backend.
    // Direct fetch from the webview to http://localhost is blocked in release mode
    // because Tauri serves pages from https://tauri.localhost (mixed content).
    try {
        return await invoke('rpc_proxy', { method, params });
    } catch (error) {
        throw new Error(getErrorMessage(error));
    }
}

/// Fetches a Jellyfin image via the Tauri backend, returning a data URL.
/// Works in both dev and release mode by bypassing browser mixed-content restrictions.
export async function getImageUrl(id: string, maxHeight?: number, quality?: number): Promise<string> {
    return await invoke('image_proxy', { id, maxHeight: maxHeight ?? null, quality: quality ?? null });
}

// --- Provider-neutral browse types ---

export type BrowseMode = "artists" | "albums" | "playlists" | "tracks" | "genres" | "recentlyAdded" | "frequentlyPlayed" | "recentlyPlayed" | "favorites";

export interface BrowseArtist {
    id: string;
    name: string;
    albumCount: number;
    coverArtId: string | null;
}

export interface BrowseAlbum {
    id: string;
    name: string;
    artistId: string;
    artistName: string;
    year: number | null;
    trackCount: number;
    coverArtId: string | null;
}

export interface BrowsePlaylist {
    id: string;
    name: string;
    trackCount: number;
    durationSeconds: number;
}

export interface BrowseTrack {
    id: string;
    title: string;
    artistId?: string | null;
    artistName: string;
    albumId?: string | null;
    albumName: string;
    trackNumber: number | null;
    duration: number;
    bitrateKbps: number | null;
    coverArtId: string | null;
    sizeBytes: number | null;
    dateAdded?: string | null;
    lastPlayedAt?: string | null;
    playCount?: number | null;
    isFavorite?: boolean | null;
}

export interface BrowseGenre {
    id: string;
    name: string;
    trackCount: number | null;
    coverArtId: string | null;
}

// --- browse.* RPC wrapper functions ---

export async function fetchBrowseModes(): Promise<BrowseMode[]> {
    const result = await rpcCall('browse.listModes');
    return result.modes;
}

export async function fetchBrowseArtists(
    letter?: string,
    libraryId?: string,
    startIndex?: number,
    limit?: number,
): Promise<{ artists: BrowseArtist[]; total: number }> {
    return await rpcCall('browse.listArtists', {
        ...(letter !== undefined && { letter }),
        ...(libraryId !== undefined && { libraryId }),
        ...(startIndex !== undefined && { startIndex }),
        ...(limit !== undefined && { limit }),
    });
}

export async function fetchBrowseArtist(
    artistId: string,
): Promise<{ artist: BrowseArtist; albums: BrowseAlbum[] }> {
    return await rpcCall('browse.getArtist', { artistId });
}

export async function fetchBrowseAlbums(
    letter?: string,
    libraryId?: string,
    startIndex?: number,
    limit?: number,
): Promise<{ albums: BrowseAlbum[]; total: number }> {
    return await rpcCall('browse.listAlbums', {
        ...(letter !== undefined && { letter }),
        ...(libraryId !== undefined && { libraryId }),
        ...(startIndex !== undefined && { startIndex }),
        ...(limit !== undefined && { limit }),
    });
}

export async function fetchBrowseAlbum(
    albumId: string,
): Promise<{ album: BrowseAlbum; tracks: BrowseTrack[] }> {
    return await rpcCall('browse.getAlbum', { albumId });
}

export async function fetchBrowsePlaylists(): Promise<{ playlists: BrowsePlaylist[] }> {
    return await rpcCall('browse.listPlaylists');
}

export async function fetchBrowsePlaylist(
    playlistId: string,
): Promise<{ playlist: BrowsePlaylist; tracks: BrowseTrack[] }> {
    return await rpcCall('browse.getPlaylist', { playlistId });
}

export async function fetchBrowseGenres(
    libraryId?: string,
    startIndex?: number,
    limit?: number,
): Promise<{ genres: BrowseGenre[]; total: number }> {
    return await rpcCall('browse.listGenres', {
        ...(libraryId !== undefined && { libraryId }),
        ...(startIndex !== undefined && { startIndex }),
        ...(limit !== undefined && { limit }),
    });
}

export async function fetchBrowseGenre(
    genreIdOrName: string,
    startIndex?: number,
    limit?: number,
): Promise<{ genre: BrowseGenre; tracks: BrowseTrack[]; total: number }> {
    return await rpcCall('browse.getGenre', {
        genreId: genreIdOrName,
        ...(startIndex !== undefined && { startIndex }),
        ...(limit !== undefined && { limit }),
    });
}

export async function fetchBrowseRecentlyAdded(
    libraryId?: string,
    startIndex?: number,
    limit?: number,
): Promise<{ albums: BrowseAlbum[]; total: number }> {
    return await rpcCall('browse.listRecentlyAdded', {
        ...(libraryId !== undefined && { libraryId }),
        ...(startIndex !== undefined && { startIndex }),
        ...(limit !== undefined && { limit }),
    });
}

export async function fetchBrowseFrequentlyPlayed(
    libraryId?: string,
    startIndex?: number,
    limit?: number,
): Promise<{ tracks: BrowseTrack[]; total: number }> {
    return await rpcCall('browse.listFrequentlyPlayed', {
        ...(libraryId !== undefined && { libraryId }),
        ...(startIndex !== undefined && { startIndex }),
        ...(limit !== undefined && { limit }),
    });
}

export async function fetchBrowseRecentlyPlayed(
    libraryId?: string,
    startIndex?: number,
    limit?: number,
): Promise<{ tracks: BrowseTrack[]; total: number }> {
    return await rpcCall('browse.listRecentlyPlayed', {
        ...(libraryId !== undefined && { libraryId }),
        ...(startIndex !== undefined && { startIndex }),
        ...(limit !== undefined && { limit }),
    });
}

export async function fetchBrowseFavorites(
    libraryId?: string,
    startIndex?: number,
    limit?: number,
): Promise<{ tracks: BrowseTrack[]; total: number }> {
    return await rpcCall('browse.listFavorites', {
        ...(libraryId !== undefined && { libraryId }),
        ...(startIndex !== undefined && { startIndex }),
        ...(limit !== undefined && { limit }),
    });
}

export async function fetchBrowseTracks(filter: {
    libraryId?: string;
    artistId?: string;
    albumId?: string;
    letter?: string;
    startIndex?: number;
    limit?: number;
}): Promise<{ tracks: BrowseTrack[]; total: number; startIndex: number; limit: number }> {
    return await rpcCall('browse.listTracks', filter);
}

export async function fetchBrowseFavoriteItems(
    libraryId?: string,
): Promise<{ artists: BrowseArtist[]; albums: BrowseAlbum[]; tracks: BrowseTrack[] }> {
    return await rpcCall('browse.listFavoriteItems', {
        ...(libraryId !== undefined && { libraryId }),
    });
}

export async function fetchBrowseSearch(
    query: string,
): Promise<{ tracks: BrowseTrack[] }> {
    return await rpcCall('browse.search', { query });
}
