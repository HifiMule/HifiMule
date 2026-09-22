import { invoke } from '@tauri-apps/api/core';
import { t } from './i18n';
import type { AutoFillPipeline } from './state/autoFill';

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
    if (error && typeof error === 'object') {
        const data = (error as Record<string, unknown>).data;
        if (data && typeof data === 'object') {
            const key = (data as Record<string, unknown>).i18nKey;
            if (typeof key === 'string' && key.trim()) return t(key);
        }
    }
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

/** JSON-RPC error code for an expired/invalid server credential (daemon ERR_UNAUTHORIZED). */
export const ERR_UNAUTHORIZED = -8;

export class RpcError extends Error {
    constructor(message: string, public readonly code: number | null, public readonly data: unknown, public readonly causeValue: unknown) {
        super(message);
        this.name = 'RpcError';
    }
}

/** Extracts the JSON-RPC error code from a structured rpc_proxy rejection, if present. */
function rpcErrorCode(error: unknown): number | null {
    if (error && typeof error === 'object') {
        const code = (error as Record<string, unknown>).code;
        if (typeof code === 'number') return code;
    }
    return null;
}

/** Browse/library RPCs whose 401 should trigger a server-scoped re-auth (AC11).
 * Excludes server.* methods so the re-auth flow itself never re-triggers. */
function isBrowseMethod(method: string): boolean {
    return method.startsWith('browse.') || method.startsWith('jellyfin_');
}

export async function rpcCall(method: string, params: any = {}): Promise<any> {
    // Never log params: several RPCs carry passwords, tokens, opaque setup IDs,
    // or future credential fields. Method-only logging is safe and useful.
    console.log(`RPC Call: ${method}`);
    // Use Tauri invoke to proxy RPC calls through the Rust backend.
    // Direct fetch from the webview to http://localhost is blocked in release mode
    // because Tauri serves pages from https://tauri.localhost (mixed content).
    try {
        return await invoke('rpc_proxy', { method, params });
    } catch (error) {
        // AC11: an expired/invalid credential on a browse RPC surfaces a scoped
        // re-auth prompt. Dispatch a global event a central handler reacts to.
        if (rpcErrorCode(error) === ERR_UNAUTHORIZED && isBrowseMethod(method)) {
            window.dispatchEvent(
                new CustomEvent('hifimule:server-unauthorized', { detail: { method } })
            );
        }
        const record = error && typeof error === 'object' ? error as Record<string, unknown> : null;
        throw new RpcError(getErrorMessage(error), rpcErrorCode(error), record?.data, error);
    }
}

// --- Multi-server (Story 2.11) ---

export interface ServerSummary {
    id: string;
    /** Deterministic, machine-independent portable id (Story 2.13). Used for basket
     * tagging, active-server tracking, and sync routing. `server.select/remove/update`
     * still key on the local `id`. */
    serverId?: string | null;
    url: string;
    serverType: string;
    username: string;
    name: string | null;
    icon: string | null;
    selected: boolean;
    libraryRole?: 'audiobook' | 'podcast' | null;
}

export interface AudiobookshelfLibraryChoice {
    choiceId: string;
    name: string;
    role: 'audiobook' | 'podcast';
}

export interface AudiobookshelfSetup {
    setupId: string;
    libraries: AudiobookshelfLibraryChoice[];
}

export async function audiobookshelfDiscover(params: {
    url: string;
    username: string;
    password: string;
}): Promise<AudiobookshelfSetup> {
    return await rpcCall('server.audiobookshelf.discover', params) as AudiobookshelfSetup;
}

export async function audiobookshelfCommit(params: {
    setupId: string;
    choiceId: string;
    name?: string;
    icon?: string;
}): Promise<void> {
    await rpcCall('server.audiobookshelf.commit', params);
}

export async function audiobookshelfCancelSetup(setupId: string): Promise<void> {
    await rpcCall('server.audiobookshelf.cancelSetup', { setupId });
}

export async function serverReauthenticate(id: string, password: string): Promise<void> {
    await rpcCall('server.reauthenticate', { id, password });
}

/** Lists all configured servers (AC1/AC20). */
export async function serverList(): Promise<ServerSummary[]> {
    return (await rpcCall('server.list')) as ServerSummary[];
}

/** Selects the active server (AC2). */
export async function serverSelect(id: string): Promise<void> {
    await rpcCall('server.select', { id });
}

export async function serverUpdate(params: {
    id: string;
    name?: string;
    icon?: string | null;
}): Promise<void> {
    await rpcCall('server.update', params);
}

/** Removes a server; returns the removed id and the reselected id (if any) (AC6/AC8). */
export async function serverRemove(
    id: string
): Promise<{ removedServerId: string; reselectedServerId: string | null }> {
    return (await rpcCall('server.remove', { id })) as {
        removedServerId: string;
        reselectedServerId: string | null;
    };
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
    serverId?: string;
    name: string;
    artistId: string;
    artistName: string;
    year: number | null;
    trackCount: number;
    coverArtId: string | null;
    /** Additive public presentation hint; absent for legacy music providers. */
    presentationCredits?: Array<{ name: string; role: 'author' | 'narrator' }>;
}

export interface BrowsePlaylist {
    id: string;
    name: string;
    trackCount: number;
    durationSeconds: number;
}

export interface BrowseTrack {
    id: string;
    serverId?: string;
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

export type PlaybackStatus = 'idle' | 'loading' | 'active' | 'paused' | 'stopped' | 'completed' | 'error';
export interface PlaybackOutput {
    outputId: string; displayName: string; detail: string; backend: string;
    available: boolean; isDefault: boolean; identityConfidence: string; isVirtual: boolean;
}
export interface PlaybackOutputState {
    revision: string; selected: PlaybackOutput | null; pending: PlaybackOutput | null;
    active: PlaybackOutput | null; status: string; error?: { code: string; retryable: boolean } | null;
}
export interface PlaybackSessionSnapshot {
    schemaVersion: number; instanceId: string; sessionId: string; queueRevision: string;
    stateSequence: string; generationId: string; mode: 'main' | 'preview'; queueKind: 'album' | 'manual';
    preview: { auditionId: string; hasMainSession: boolean; savedMainOccurrenceId: string | null;
        savedMainPositionMs: number; savedMainIntent: string; resumeInhibited: boolean } | null;
    state: string; positionMs: number;
    totalOccurrenceCount: number;
    occurrences: PlaybackOccurrence[];
    nextCursor: string | null;
    current: { occurrenceId: string; source: { serverId: string; trackId: string } } | null;
    mainCurrent: { occurrenceId: string; ordinal: number; source: { serverId: string; trackId: string }; availability: 'unknown' | 'notConfigured' } | null;
    playback: { status: PlaybackStatus; canGoNext: boolean; canGoBack: boolean; backUnavailableReason?: string | null; metadata: { title: string; artist?: string | null; source: { serverId: string; trackId: string } } | null; durationMs?: number | null;
        seek: { available: boolean; reason?: string | null; mechanism?: string | null; decodedLandingToleranceMs?: number | null };
        pendingSeek?: { operationId: string; requestedPositionMs: number; priorCommittedPositionMs: number } | null;
        seekOutcome?: { operationId: string; requestedPositionMs: number; actualPositionMs?: number | null; status: string; error?: { code: string; retryable: boolean } | null } | null;
        pendingBack?: { operationId: string } | null;
        backOutcome?: { operationId: string; status: 'committed' | 'failed'; error?: { code: string; retryable: boolean } | null } | null;
        error?: { code: string; retryable: boolean } | null };
    output: PlaybackOutputState;
}

export interface PlaybackOccurrence {
    occurrenceId: string;
    ordinal: number;
    source: { serverId: string; trackId: string };
    availability: 'unknown' | 'notConfigured';
}
export interface PlaybackOccurrencePage {
    occurrences: PlaybackOccurrence[];
    nextCursor: string | null;
    totalOccurrenceCount: number;
    section: 'all' | 'upcoming' | 'history';
    sectionCount: number;
    mainCurrentOccurrenceId: string | null;
    precedingOccurrenceId: string | null;
    followingOccurrenceIds: string[];
    endOfSection: boolean;
}
export interface OccurrenceDisplay {
    occurrenceId: string;
    source: { serverId: string; trackId: string };
    title: string | null;
    artist: string | null;
    album: string | null;
    durationMs: number | null;
    status: 'available' | 'sourceUnavailable' | 'trackUnavailable';
}

export type Destination =
    | { kind: 'playback'; id: 'playback'; selected: boolean }
    | { kind: 'device'; path: string; deviceId: string; name: string; icon?: string | null; selected: boolean }
    | { kind: 'pendingDevice'; pendingId: string; name: string; selected: boolean };
export interface DeviceDiscoveryIssue {
    discoveryId: string; code: 'DEVICE_OPEN_FAILED' | 'DEVICE_READ_FAILED';
    displayName?: string | null; retryable: boolean; revision: string;
}
export interface DaemonDestinationState {
    destinationRevision: string;
    destinations: Destination[];
    deviceDiscoveryIssues: DeviceDiscoveryIssue[];
}

export async function getDaemonState(): Promise<DaemonDestinationState & Record<string, unknown>> {
    return await rpcCall('get_daemon_state');
}

export async function destinationSelect(selection: { kind: 'playback' } | { kind: 'device'; path: string } | { kind: 'pendingDevice'; pendingId: string }): Promise<void> {
    await rpcCall('destination.select', selection);
}

export async function playbackListOutputs(): Promise<{ instanceId: string; outputRevision: string; outputs: PlaybackOutput[]; output: PlaybackOutputState; error?: { code: string; retryable: boolean } | null }> {
    return (await rpcCall('playback.listOutputs', { schemaVersion: 1 })).data;
}

export async function playbackSelectOutput(outputId: string, observed: PlaybackSessionSnapshot, replaceInvalidConfig = false): Promise<PlaybackSessionSnapshot> {
    return (await rpcCall('playback.selectOutput', {
        schemaVersion: 1, instanceId: observed.instanceId, sessionId: observed.sessionId,
        commandId: crypto.randomUUID(), expectedOutputRevision: observed.output.revision,
        expectedGenerationId: observed.generationId, outputId, replaceInvalidConfig,
    })).data;
}

export async function playbackGetSession(): Promise<PlaybackSessionSnapshot> {
    const result = await rpcCall('playback.getSession', { schemaVersion: 1 });
    return result.data;
}

export async function playbackListOccurrences(
    observed: Pick<PlaybackSessionSnapshot, 'sessionId' | 'queueRevision'> & Partial<Pick<PlaybackSessionSnapshot, 'mainCurrent'>>,
    cursor: string | null = null,
    limit = 100,
    options: { section?: 'all' | 'upcoming' | 'history'; aroundOccurrenceId?: string | null } = {},
): Promise<PlaybackOccurrencePage> {
    if (!Number.isSafeInteger(limit) || limit < 1 || limit > 200) throw new RangeError('limit must be between 1 and 200');
    return (await rpcCall('playback.listOccurrences', {
        schemaVersion: 1, sessionId: observed.sessionId,
        expectedQueueRevision: observed.queueRevision,
        section: options.section ?? 'all',
        cursor,
        aroundOccurrenceId: options.aroundOccurrenceId ?? null,
        expectedMainOccurrenceId: options.section && options.section !== 'all'
            ? observed.mainCurrent?.occurrenceId ?? null
            : null,
        limit,
    })).data;
}

export type PlaybackTrackSource = { serverId: string; trackId: string };
export type PlaybackQueueOperation =
    | { type: 'appendQueue'; sources: PlaybackTrackSource[] }
    | { type: 'removeUpcoming'; occurrenceIds: string[] }
    | { type: 'moveUpcoming'; occurrenceId: string; beforeOccurrenceId: string | null };

export interface PlaybackApplyResult {
    sessionId: string;
    queueRevision: string;
    stateSequence: string;
    generationId: string;
    assignedOccurrences: PlaybackOccurrence[];
}

export async function playbackApplyQueueOperation(
    observed: Pick<PlaybackSessionSnapshot, 'instanceId' | 'sessionId' | 'queueRevision'>,
    operation: PlaybackQueueOperation,
    commandId = crypto.randomUUID(),
): Promise<PlaybackApplyResult> {
    return (await rpcCall('playback.applySession', {
        schemaVersion: 1,
        instanceId: observed.instanceId,
        sessionId: observed.sessionId,
        commandId,
        expectedQueueRevision: observed.queueRevision,
        operation,
    })).data;
}

export async function playbackAppendQueue(
    sources: PlaybackTrackSource[],
    observed?: PlaybackSessionSnapshot,
): Promise<PlaybackApplyResult> {
    if (sources.length > 200) throw new RangeError('at most 200 tracks may be added at once');
    for (const source of sources) {
        if (!source.serverId || !source.trackId) throw new TypeError('portable track sources are required');
    }
    return playbackApplyQueueOperation(observed ?? await playbackGetSession(), {
        type: 'appendQueue',
        sources: sources.map(source => ({ ...source })),
    });
}

export async function playbackRemoveUpcoming(
    observed: PlaybackSessionSnapshot,
    occurrenceIds: string[],
): Promise<PlaybackApplyResult> {
    if (occurrenceIds.length < 1 || occurrenceIds.length > 200) {
        throw new RangeError('between 1 and 200 upcoming occurrences are required');
    }
    return playbackApplyQueueOperation(observed, { type: 'removeUpcoming', occurrenceIds: [...occurrenceIds] });
}

export async function playbackMoveUpcoming(
    observed: PlaybackSessionSnapshot,
    occurrenceId: string,
    beforeOccurrenceId: string | null,
): Promise<PlaybackApplyResult> {
    return playbackApplyQueueOperation(observed, { type: 'moveUpcoming', occurrenceId, beforeOccurrenceId });
}

export function isPlaybackQueueConflict(error: unknown): boolean {
    if (!(error instanceof RpcError) || !error.data || typeof error.data !== 'object') return false;
    const data = error.data as Record<string, unknown>;
    if (error.code === 409) {
        return ['QUEUE_REVISION_CONFLICT', 'QUEUE_CONFLICT', 'OCCURRENCE_NOT_UPCOMING',
            'INVALID_CURSOR', 'INSTANCE_MISMATCH', 'SESSION_MISMATCH']
            .includes(String(data.code ?? ''));
    }
    if (error.code === -7 && data.code === 'QUEUE_CONFLICT') {
        return typeof data.authoritative === 'object' && data.authoritative !== null;
    }
    return false;
}

export async function playbackDescribeOccurrences(
    observed: Pick<PlaybackSessionSnapshot, 'sessionId' | 'queueRevision'>,
    occurrenceIds: string[],
): Promise<OccurrenceDisplay[]> {
    if (occurrenceIds.length < 1 || occurrenceIds.length > 200) throw new RangeError('one bounded occurrence page is required');
    return (await rpcCall('playback.describeOccurrences', {
        schemaVersion: 1, sessionId: observed.sessionId,
        expectedQueueRevision: observed.queueRevision, occurrenceIds,
    })).data.occurrences;
}

export async function playbackPlayTrack(serverId: string, trackId: string): Promise<void> {
    const current = await playbackGetSession();
    await rpcCall('playback.applySession', {
        schemaVersion: 1, instanceId: current.instanceId, sessionId: current.sessionId,
        commandId: crypto.randomUUID(), expectedQueueRevision: current.queueRevision,
        operation: { type: 'playTrack', source: { serverId, trackId } },
    });
}

export async function playbackPreviewTrack(serverId: string, trackId: string): Promise<void> {
    const current = await playbackGetSession();
    await rpcCall('playback.previewTrack', {
        schemaVersion: 1, instanceId: current.instanceId, sessionId: current.sessionId,
        commandId: crypto.randomUUID(), expectedQueueRevision: current.queueRevision,
        expectedGenerationId: current.generationId, source: { serverId, trackId },
    });
}

export async function playbackPlayAlbum(serverId: string, albumId: string): Promise<void> {
    const current = await playbackGetSession();
    try {
        await rpcCall('playback.playAlbum', {
            schemaVersion: 1, instanceId: current.instanceId, sessionId: current.sessionId,
            commandId: crypto.randomUUID(), expectedQueueRevision: current.queueRevision,
            expectedGenerationId: current.generationId, source: { serverId, albumId },
        });
    } catch (error) {
        const data = error instanceof RpcError && error.data && typeof error.data === 'object'
            ? error.data as Record<string, unknown>
            : null;
        if (data?.code === 'ALBUM_SUPERSEDED') return;
        throw error;
    }
}

export async function playbackControl(action: 'back' | 'pause' | 'resume' | 'stop' | 'next' | 'retry' | 'returnToSession', observed?: PlaybackSessionSnapshot): Promise<void> {
    const current = observed ?? await playbackGetSession();
    if (!current.current) return;
    await rpcCall('playback.control', {
        schemaVersion: 1, instanceId: current.instanceId, sessionId: current.sessionId,
        commandId: crypto.randomUUID(), expectedGenerationId: current.generationId,
        occurrenceId: current.current.occurrenceId, action,
    });
}

export async function playbackSeek(positionMs: number, observed: PlaybackSessionSnapshot): Promise<PlaybackSessionSnapshot> {
    if (!observed.current || !Number.isSafeInteger(positionMs) || positionMs < 0) {
        throw new TypeError('positionMs must be a nonnegative safe integer');
    }
    return (await rpcCall('playback.seek', {
        schemaVersion: 1, instanceId: observed.instanceId, sessionId: observed.sessionId,
        commandId: crypto.randomUUID(), expectedGenerationId: observed.generationId,
        occurrenceId: observed.current.occurrenceId, positionMs,
    })).data;
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
): Promise<{ tracks: BrowseTrack[]; albums?: BrowseAlbum[]; possiblyTruncated?: boolean }> {
    return await rpcCall('browse.search', { query });
}

// --- Auto-Fill live preview (Story 12.7) ---

/** The minimal slice of the daemon `AutoFillItem` (camelCase serde) the preview needs: enough to
 * count items and sum their on-device size. The daemon returns richer fields (album/artist/etc.)
 * which we intentionally ignore here. */
export interface AutoFillPreviewItem {
    id: string;
    name: string;
    sizeBytes: number;
}

/** Computes a live, provider-routed preview of what the given (unsaved) pipeline would fill for
 * `serverId`, via the shared `basket.autoFill`+serverId sync-time seam (Story 12.7). Always routes
 * by portable `serverId` — never the legacy no-serverId Jellyfin path — so it previews correctly
 * for Subsonic/Navidrome servers too. Surfaced errors (unknown serverId → ERR_CONNECTION_FAILED,
 * malformed pipeline → ERR_INVALID_PARAMS, any RPC failure) propagate as thrown Errors for the
 * caller to display. */
export async function previewAutoFill(params: {
    serverId: string;
    pipeline: AutoFillPipeline;
    excludeItemIds?: string[];
    maxBytes?: number;
}): Promise<AutoFillPreviewItem[]> {
    const result = await rpcCall('basket.autoFill', {
        serverId: params.serverId,
        pipeline: params.pipeline,
        ...(params.excludeItemIds !== undefined && { excludeItemIds: params.excludeItemIds }),
        ...(params.maxBytes !== undefined && { maxBytes: params.maxBytes }),
    });
    return Array.isArray(result) ? (result as AutoFillPreviewItem[]) : [];
}
