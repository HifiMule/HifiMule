//! Playback-owned finite selection. Provider work ends before entering the pure selector.
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    future::Future,
    io::{Read, Write},
    path::Path,
};

use serde::{Deserialize, Serialize};

use crate::{
    auto_fill::pipeline::{
        AutoFillPipeline, Candidate, OrderingKey, PipelineInput, SourceEntry, SourceKey,
        SourceKind, run_pipeline,
    },
    domain::models::Song,
    playback::model::TrackSource,
    providers::{BrowseMode, MediaProvider},
};

pub const MAX_SOURCES: usize = 8;
pub const PAGE_SIZE: u32 = 100;
pub const MAX_PAGES: u32 = 4;
pub const MAX_CANDIDATES_PER_SOURCE: usize = (PAGE_SIZE * MAX_PAGES) as usize;
pub const MAX_ARTIST_ALBUMS: usize = 32;
const MAX_FILE_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SelectionKind {
    Playlist,
    Artist,
    Genre,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SelectionSource {
    pub server_id: String,
    pub kind: SelectionKind,
    #[serde(rename = "ref")]
    pub ref_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlaybackSelectionConfig {
    pub schema_version: u32,
    pub sources: Vec<SelectionSource>,
    pub ordering: Vec<OrderingKey>,
    pub seed: u64,
    pub max_tracks: u16,
}

impl Default for PlaybackSelectionConfig {
    fn default() -> Self {
        Self {
            schema_version: 1,
            sources: Vec::new(),
            ordering: vec![OrderingKey::Random],
            seed: 0,
            max_tracks: 16,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SelectionError {
    #[error("PLAYBACK_SELECTION_SETUP")]
    Setup,
    #[error("PLAYBACK_SELECTION_SAVE_FAILED")]
    Save,
    #[error("PLAYBACK_SELECTION_SOURCE_UNAVAILABLE")]
    SourceUnavailable,
    #[error("PLAYBACK_SELECTION_PREPARATION_FAILED")]
    PreparationFailed,
    #[error("PLAYBACK_SELECTION_EMPTY")]
    Empty,
    #[error("PLAYBACK_SELECTION_CANCELLED")]
    Cancelled,
}

impl PlaybackSelectionConfig {
    pub fn validate(&self) -> Result<(), SelectionError> {
        if self.schema_version != 1
            || self.sources.len() > MAX_SOURCES
            || self.seed > u32::MAX as u64
            || self.max_tracks == 0
            || self.max_tracks > 100
            || self.ordering.len() > 8
            || self.ordering.is_empty()
            || self.sources.iter().any(|source| {
                source.server_id.is_empty()
                    || source.server_id.len() > 1024
                    || source.ref_id.is_empty()
                    || source.ref_id.len() > 1024
                    || source.server_id.contains('\0')
                    || source.ref_id.contains('\0')
            })
        {
            return Err(SelectionError::Setup);
        }
        let mut identities = HashSet::new();
        if self.sources.iter().any(|source| {
            !identities.insert((
                source.server_id.as_str(),
                source.kind,
                source.ref_id.as_str(),
            ))
        }) {
            return Err(SelectionError::Setup);
        }
        Ok(())
    }
}

pub fn load(path: &Path) -> Result<PlaybackSelectionConfig, SelectionError> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(PlaybackSelectionConfig::default());
        }
        Err(_) => return Err(SelectionError::Setup),
    };
    let mut bytes = Vec::new();
    file.take((MAX_FILE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| SelectionError::Setup)?;
    if bytes.len() > MAX_FILE_BYTES {
        return Err(SelectionError::Setup);
    }
    let config: PlaybackSelectionConfig =
        serde_json::from_slice(&bytes).map_err(|_| SelectionError::Setup)?;
    config.validate()?;
    Ok(config)
}

pub fn save(path: &Path, config: &PlaybackSelectionConfig) -> Result<(), SelectionError> {
    config.validate()?;
    if path.exists() {
        load(path)?; // Preserve an unknown future version or damaged file for recovery.
    }
    let bytes = serde_json::to_vec(config).map_err(|_| SelectionError::Setup)?;
    if bytes.len() > MAX_FILE_BYTES {
        return Err(SelectionError::Setup);
    }
    let parent = path.parent().ok_or(SelectionError::Save)?;
    fs::create_dir_all(parent).map_err(|_| SelectionError::Save)?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|_| SelectionError::Save)?;
    temporary
        .write_all(&bytes)
        .and_then(|_| temporary.as_file().sync_all())
        .map_err(|_| SelectionError::Save)?;
    temporary.persist(path).map_err(|_| SelectionError::Save)?;
    #[cfg(unix)]
    File::open(parent)
        .and_then(|file| file.sync_all())
        .map_err(|_| SelectionError::Save)?;
    Ok(())
}

/// A source pool has an explicit portable owner; provider-local ids are never compared alone.
#[derive(Debug, Clone)]
pub struct SelectionPool {
    pub source: SelectionSource,
    pub tracks: Vec<Song>,
}

/// Playback's typed candidate boundary before the shared selector receives a
/// collision-safe internal song ID. The typed source is restored on output.
#[derive(Debug, Clone)]
pub struct SelectionCandidate {
    pub source: TrackSource,
    pub song: Song,
    /// Source-qualified artist anchor when a better copy comes from another server.
    pub center_origin: Option<super::radio::ArtistIdentity>,
}

fn stable_recording_keys(pools: &[SelectionPool]) -> HashMap<(String, String), Option<String>> {
    let mut observed = HashMap::<(String, String), Option<String>>::new();
    let mut conflicts = HashSet::new();
    for pool in pools {
        for song in &pool.tracks {
            let source = (pool.source.server_id.clone(), song.id.clone());
            let key = song
                .provider_metadata
                .recording
                .as_ref()
                .and_then(|e| e.key())
                .map(|k| k.as_str().to_owned());
            if conflicts.contains(&source) {
                continue;
            }
            match observed.entry(source.clone()) {
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(key);
                }
                std::collections::hash_map::Entry::Occupied(entry) if entry.get() != &key => {
                    entry.into_mut().take();
                    conflicts.insert(source);
                }
                _ => {}
            }
        }
    }
    observed
}

/// A source-local track seen through more than one configured source must not
/// inherit one arbitrary metadata observation when the observations disagree.
pub fn clear_conflicting_recordings(pools: &mut [SelectionPool]) {
    let observed = stable_recording_keys(pools);
    for pool in pools {
        for song in &mut pool.tracks {
            if observed
                .get(&(pool.source.server_id.clone(), song.id.clone()))
                .is_some_and(Option::is_none)
            {
                song.provider_metadata.recording = None;
            }
        }
    }
}

pub fn recording_keys_after_conflict_clear(pools: &mut [SelectionPool]) -> HashSet<String> {
    clear_conflicting_recordings(pools);
    pools
        .iter()
        .flat_map(|pool| &pool.tracks)
        .filter_map(|song| {
            song.provider_metadata
                .recording
                .as_ref()
                .and_then(|e| e.key())
                .map(|key| key.as_str().to_owned())
        })
        .collect()
}

#[derive(Clone)]
struct RankedCopy {
    source: TrackSource,
    codec: Option<String>,
    bitrate: Option<u32>,
    source_order: usize,
}

impl RankedCopy {
    fn new(source: TrackSource, song: &Song, source_order: usize) -> Self {
        Self {
            source,
            codec: song.suffix.as_ref().map(|value| value.to_ascii_lowercase()),
            bitrate: song.bitrate_kbps.filter(|value| *value > 0),
            source_order,
        }
    }
}

/// Pick the earliest configured copy as the codec comparison anchor. Quality
/// is ordered only within that codec; incomparable copies retain configured
/// order and portable IDs. This is a total, arrival-order-independent ranking.
fn rank_copies(mut copies: Vec<RankedCopy>) -> Vec<TrackSource> {
    let Some(anchor) = copies.iter().min_by_key(|copy| {
        (
            copy.source_order,
            &copy.source.server_id,
            &copy.source.track_id,
        )
    }) else {
        return Vec::new();
    };
    let comparable_codec = anchor.bitrate.and(anchor.codec.clone());
    copies.sort_by(|a, b| {
        let comparable = |copy: &RankedCopy| {
            comparable_codec.as_ref() == copy.codec.as_ref() && copy.bitrate.is_some()
        };
        comparable(b)
            .cmp(&comparable(a))
            .then_with(|| {
                if comparable(a) && comparable(b) {
                    b.bitrate.cmp(&a.bitrate)
                } else {
                    std::cmp::Ordering::Equal
                }
            })
            .then_with(|| {
                (a.source_order, &a.source.server_id, &a.source.track_id).cmp(&(
                    b.source_order,
                    &b.source.server_id,
                    &b.source.track_id,
                ))
            })
    });
    let mut seen = HashSet::new();
    copies
        .into_iter()
        .filter(|copy| seen.insert((copy.source.server_id.clone(), copy.source.track_id.clone())))
        .map(|copy| copy.source)
        .collect()
}

/// Return a single stable provider artist ID for a selected source. Multiple
/// configured pools may contain the same track; conflicting or missing evidence
/// must not silently choose a center.
pub fn artist_for_source(pools: &[SelectionPool], source: &TrackSource) -> Option<String> {
    let mut observed: Option<&str> = None;
    let mut found = false;
    for pool in pools
        .iter()
        .filter(|pool| pool.source.server_id == source.server_id)
    {
        for song in pool.tracks.iter().filter(|song| song.id == source.track_id) {
            found = true;
            if song.provider_metadata.ambiguous_music_artist {
                return None;
            }
            let artist_id = song.artist_id.as_deref().filter(|id| !id.is_empty())?;
            if observed.is_some_and(|previous| previous != artist_id) {
                return None;
            }
            observed = Some(artist_id);
        }
    }
    found.then(|| observed.map(str::to_owned)).flatten()
}

/// A preferred recording copy may lack an artist ID. Retain a source-qualified
/// center from another confident copy instead of starting a centerless Radio.
pub fn radio_center_for_source(
    config: &PlaybackSelectionConfig,
    pools: &[SelectionPool],
    selected: &TrackSource,
) -> Option<super::radio::ArtistIdentity> {
    if let Some(artist_id) = artist_for_source(pools, selected) {
        return super::radio::center_for(selected, Some(&artist_id));
    }
    let stable = stable_recording_keys(pools);
    let key = stable
        .get(&(selected.server_id.clone(), selected.track_id.clone()))
        .and_then(Option::as_ref)?;
    let mut copies = Vec::new();
    for (order, configured) in config.sources.iter().enumerate() {
        for pool in pools.iter().filter(|pool| pool.source == *configured) {
            for song in &pool.tracks {
                let source = TrackSource {
                    server_id: pool.source.server_id.clone(),
                    track_id: song.id.clone(),
                };
                if stable
                    .get(&(source.server_id.clone(), source.track_id.clone()))
                    .and_then(Option::as_ref)
                    == Some(key)
                    && let Some(artist_id) = artist_for_source(pools, &source)
                {
                    copies.push((order, source, artist_id));
                }
            }
        }
    }
    copies.sort_by(|a, b| {
        (a.0, &a.1.server_id, &a.1.track_id).cmp(&(b.0, &b.1.server_id, &b.1.track_id))
    });
    copies
        .into_iter()
        .next()
        .and_then(|(_, source, artist_id)| super::radio::center_for(&source, Some(&artist_id)))
}

/// Radio needs enough ranked candidates for five admissions plus its bounded
/// preparation-failure budget, regardless of the first-track setting.
pub fn select_radio_order(
    config: &PlaybackSelectionConfig,
    pools: Vec<SelectionPool>,
) -> Result<Vec<TrackSource>, SelectionError> {
    let mut request = config.clone();
    request.max_tracks =
        (super::radio::AUTO_UPCOMING_TARGET + super::radio::MAX_PREPARATION_FAILURES) as u16;
    select_with_recordings(&request, pools)
}

/// Radio groups only provider-verified recording keys. Source choice is stable:
/// a fetched source is available, then bitrate is compared only for the same
/// codec, followed by configured order and portable source identity.
pub fn select_with_recordings(
    config: &PlaybackSelectionConfig,
    pools: Vec<SelectionPool>,
) -> Result<Vec<TrackSource>, SelectionError> {
    select_inner(config, pools, true)
}

/// Other fetched copies are tried only after the selected source fails to
/// prepare. No extra provider request is made to rank them.
pub fn radio_copy_alternates(
    config: &PlaybackSelectionConfig,
    pools: &[SelectionPool],
    primary: &TrackSource,
) -> Vec<TrackSource> {
    let stable = stable_recording_keys(pools);
    let key = stable
        .get(&(primary.server_id.clone(), primary.track_id.clone()))
        .and_then(Option::as_ref);
    let Some(key) = key else {
        return Vec::new();
    };
    let mut alternatives = Vec::new();
    for (order, source) in config.sources.iter().enumerate() {
        for pool in pools.iter().filter(|pool| pool.source == *source) {
            for song in &pool.tracks {
                if stable
                    .get(&(pool.source.server_id.clone(), song.id.clone()))
                    .and_then(Option::as_ref)
                    == Some(key)
                    && (pool.source.server_id != primary.server_id || song.id != primary.track_id)
                {
                    alternatives.push(RankedCopy::new(
                        TrackSource {
                            server_id: pool.source.server_id.clone(),
                            track_id: song.id.clone(),
                        },
                        song,
                        order,
                    ));
                }
            }
        }
    }
    rank_copies(alternatives)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionOption {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionOptionPage {
    pub options: Vec<SelectionOption>,
    pub has_more: bool,
}

fn browse_mode(kind: SelectionKind) -> BrowseMode {
    match kind {
        SelectionKind::Playlist => BrowseMode::Playlists,
        SelectionKind::Artist => BrowseMode::Artists,
        SelectionKind::Genre => BrowseMode::Genres,
    }
}

/// Options are resolved by portable server identity and paged for the UI.
pub async fn options_page(
    provider: &dyn MediaProvider,
    kind: SelectionKind,
    offset: u32,
) -> Result<SelectionOptionPage, SelectionError> {
    if !provider
        .capabilities()
        .browse
        .list_modes
        .contains(&browse_mode(kind))
    {
        return Err(SelectionError::SourceUnavailable);
    }
    let limit = MAX_CANDIDATES_PER_SOURCE as u32;
    let values = match kind {
        SelectionKind::Playlist => provider
            .list_playlists()
            .await
            .map_err(|_| SelectionError::SourceUnavailable)?
            .into_iter()
            .skip(offset as usize)
            .take(MAX_CANDIDATES_PER_SOURCE + 1)
            .map(|p| SelectionOption {
                id: p.id,
                name: p.name,
            })
            .collect(),
        SelectionKind::Artist => provider
            .list_artists(None, None, offset, limit + 1)
            .await
            .map_err(|_| SelectionError::SourceUnavailable)?
            .0
            .into_iter()
            .map(|a| SelectionOption {
                id: a.id,
                name: a.name,
            })
            .collect(),
        SelectionKind::Genre => provider
            .list_genres(None, offset, limit + 1)
            .await
            .map_err(|_| SelectionError::SourceUnavailable)?
            .0
            .into_iter()
            .map(|g| SelectionOption {
                id: g.id,
                name: g.name,
            })
            .collect(),
    };
    let mut options: Vec<SelectionOption> = values;
    let has_more = options.len() > MAX_CANDIDATES_PER_SOURCE;
    options.truncate(MAX_CANDIDATES_PER_SOURCE);
    Ok(SelectionOptionPage { options, has_more })
}

/// Validate an exact reference independently of the UI's current page.
pub async fn reference_exists(
    provider: &dyn MediaProvider,
    source: &SelectionSource,
) -> Result<bool, SelectionError> {
    if !provider
        .capabilities()
        .browse
        .list_modes
        .contains(&browse_mode(source.kind))
    {
        return Err(SelectionError::SourceUnavailable);
    }
    match source.kind {
        SelectionKind::Playlist => Ok(provider
            .list_playlists()
            .await
            .map_err(|_| SelectionError::SourceUnavailable)?
            .iter()
            .any(|item| item.id == source.ref_id)),
        SelectionKind::Artist => Ok(provider.get_artist(&source.ref_id).await.is_ok()),
        SelectionKind::Genre => {
            let mut offset = 0;
            loop {
                let page = options_page(provider, source.kind, offset).await?;
                if page.options.iter().any(|item| item.id == source.ref_id) {
                    return Ok(true);
                }
                if !page.has_more {
                    return Ok(false);
                }
                let next = offset.saturating_add(MAX_CANDIDATES_PER_SOURCE as u32);
                if next == offset {
                    return Ok(false);
                }
                offset = next;
            }
        }
    }
}

pub async fn fetch_source(
    provider: &dyn MediaProvider,
    source: &SelectionSource,
) -> Result<SelectionPool, SelectionError> {
    if matches!(source.kind, SelectionKind::Genre) && !reference_exists(provider, source).await? {
        return Err(SelectionError::SourceUnavailable);
    }
    let mut tracks = match source.kind {
        SelectionKind::Playlist => provider
            .get_playlist_tracks_bounded(&source.ref_id, MAX_CANDIDATES_PER_SOURCE as u32)
            .await
            .map_err(|_| SelectionError::SourceUnavailable)?,
        SelectionKind::Artist => {
            let artist = provider
                .get_artist(&source.ref_id)
                .await
                .map_err(|_| SelectionError::SourceUnavailable)?;
            let mut all = Vec::new();
            let had_albums = !artist.albums.is_empty();
            let mut loaded_album = false;
            for album in artist.albums.into_iter().take(MAX_ARTIST_ALBUMS) {
                let Ok(result) = provider.get_album(&album.id).await else {
                    continue;
                };
                loaded_album = true;
                all.extend(
                    result
                        .tracks
                        .into_iter()
                        .take(MAX_CANDIDATES_PER_SOURCE - all.len()),
                );
                if all.len() >= MAX_CANDIDATES_PER_SOURCE {
                    break;
                }
            }
            if !loaded_album && had_albums {
                return Err(SelectionError::SourceUnavailable);
            }
            all
        }
        SelectionKind::Genre => {
            let mut all = Vec::new();
            for page in 0..MAX_PAGES {
                let result = provider
                    .get_genre_tracks_bounded(&source.ref_id, page * PAGE_SIZE, PAGE_SIZE)
                    .await
                    .map_err(|_| SelectionError::SourceUnavailable)?;
                let count = result.len();
                all.extend(result.into_iter().take(PAGE_SIZE as usize));
                if count < PAGE_SIZE as usize {
                    break;
                }
            }
            all
        }
    };
    tracks.truncate(MAX_CANDIDATES_PER_SOURCE);
    Ok(SelectionPool {
        source: source.clone(),
        tracks,
    })
}

/// Cursor for one bounded Radio source window. For an artist, `index` is the
/// album index and `intra` is the track index within that album. Other source
/// kinds use `index` as the track offset and keep `intra` at zero.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RadioSourceCursor {
    pub index: u32,
    pub intra: u32,
}

async fn fetch_offset_window<F, Fut, E>(
    cursor: RadioSourceCursor,
    mut fetch: F,
) -> Result<(Vec<Song>, Option<RadioSourceCursor>), SelectionError>
where
    F: FnMut(u32, u32) -> Fut,
    Fut: Future<Output = Result<Vec<Song>, E>>,
{
    let mut tracks = fetch(cursor.index, MAX_CANDIDATES_PER_SOURCE as u32)
        .await
        .map_err(|_| SelectionError::SourceUnavailable)?;
    if tracks.len() > MAX_CANDIDATES_PER_SOURCE {
        return Err(SelectionError::SourceUnavailable);
    }
    let offset = cursor
        .index
        .checked_add(tracks.len() as u32)
        .ok_or(SelectionError::SourceUnavailable)?;
    if tracks.len() == MAX_CANDIDATES_PER_SOURCE {
        return Ok((
            tracks,
            Some(RadioSourceCursor {
                index: offset,
                intra: 0,
            }),
        ));
    }
    // Some servers underfill a bounded page. Probe the exact next offset before
    // treating a short page as exhaustion; a failed probe means unknown.
    let mut probe = fetch(offset, 1)
        .await
        .map_err(|_| SelectionError::SourceUnavailable)?;
    if probe.len() > 1 {
        return Err(SelectionError::SourceUnavailable);
    }
    if probe.is_empty() {
        return Ok((tracks, None));
    }
    if tracks.is_empty() {
        tracks.push(probe.remove(0));
        let index = offset
            .checked_add(1)
            .ok_or(SelectionError::SourceUnavailable)?;
        Ok((tracks, Some(RadioSourceCursor { index, intra: 0 })))
    } else {
        Ok((
            tracks,
            Some(RadioSourceCursor {
                index: offset,
                intra: 0,
            }),
        ))
    }
}

pub async fn fetch_radio_window(
    provider: &dyn MediaProvider,
    source: &SelectionSource,
    cursor: RadioSourceCursor,
) -> Result<(SelectionPool, Option<RadioSourceCursor>), SelectionError> {
    let mut next = None;
    let tracks = match source.kind {
        SelectionKind::Playlist => {
            let (tracks, following) = fetch_offset_window(cursor, |offset, limit| {
                provider.get_playlist_tracks_window(&source.ref_id, offset, limit)
            })
            .await?;
            next = following;
            tracks
        }
        SelectionKind::Genre => {
            let (tracks, following) = fetch_offset_window(cursor, |offset, limit| {
                provider.get_genre_tracks_bounded(&source.ref_id, offset, limit)
            })
            .await?;
            next = following;
            tracks
        }
        SelectionKind::Artist => {
            let artist = provider
                .get_artist(&source.ref_id)
                .await
                .map_err(|_| SelectionError::SourceUnavailable)?;
            let mut all = Vec::new();
            let mut album_index = cursor.index as usize;
            let mut track_index = cursor.intra as usize;
            let mut visited = 0;
            while album_index < artist.albums.len()
                && visited < MAX_ARTIST_ALBUMS
                && all.len() < MAX_CANDIDATES_PER_SOURCE
            {
                let album = &artist.albums[album_index];
                let result = provider
                    .get_album(&album.id)
                    .await
                    .map_err(|_| SelectionError::SourceUnavailable)?;
                visited += 1;
                let remaining = MAX_CANDIDATES_PER_SOURCE - all.len();
                let track_count = result.tracks.len();
                let available = track_count.saturating_sub(track_index);
                let take = remaining.min(available);
                all.extend(result.tracks.into_iter().skip(track_index).take(take));
                track_index += take;
                if track_index < track_count {
                    break;
                }
                album_index += 1;
                track_index = 0;
            }
            if album_index < artist.albums.len() {
                next = Some(RadioSourceCursor {
                    index: album_index as u32,
                    intra: track_index as u32,
                });
            }
            all
        }
    };
    Ok((
        SelectionPool {
            source: source.clone(),
            tracks,
        },
        next,
    ))
}

/// Materialize the shared pure selector. A single-server pool uses its original IDs, so
/// equivalent sync and Playback fixtures yield the same order for an explicit seed. With
/// multiple servers the internal ID is a reversible, collision-free namespace; the output
/// is mapped back to the typed portable identity before it leaves this function.
pub fn select(
    config: &PlaybackSelectionConfig,
    pools: Vec<SelectionPool>,
) -> Result<Vec<TrackSource>, SelectionError> {
    select_inner(config, pools, false)
}

fn select_inner(
    config: &PlaybackSelectionConfig,
    pools: Vec<SelectionPool>,
    group_recordings: bool,
) -> Result<Vec<TrackSource>, SelectionError> {
    config.validate()?;
    if config.sources.is_empty() {
        return Err(SelectionError::Setup);
    }
    let multiple_servers = config
        .sources
        .iter()
        .map(|s| s.server_id.as_str())
        .collect::<HashSet<_>>()
        .len()
        > 1;
    let pipeline = AutoFillPipeline {
        ordering: config.ordering.clone(),
        sources: vec![SourceEntry::new(SourceKind::Library)],
        ..Default::default()
    };
    let mut input = PipelineInput {
        seed: config.seed,
        ..Default::default()
    };
    let mut identities = HashMap::<String, TrackSource>::new();
    let stable_recording = if group_recordings {
        stable_recording_keys(&pools)
    } else {
        HashMap::new()
    };
    let mut recording_for_source = HashMap::new();
    let mut copies_for_recording = HashMap::<String, Vec<RankedCopy>>::new();
    let mut candidates = Vec::new();
    for (source_order, source) in config.sources.iter().enumerate() {
        let tracks = pools
            .iter()
            .find(|pool| pool.source == *source)
            .map(|pool| pool.tracks.as_slice())
            .unwrap_or(&[]);
        for song in tracks.iter().take(MAX_CANDIDATES_PER_SOURCE) {
            if song.id.is_empty() {
                continue;
            }
            let candidate = SelectionCandidate {
                source: TrackSource {
                    server_id: source.server_id.clone(),
                    track_id: song.id.clone(),
                },
                song: song.clone(),
                center_origin: None,
            };
            if group_recordings
                && let Some(key) = stable_recording
                    .get(&(
                        candidate.source.server_id.clone(),
                        candidate.source.track_id.clone(),
                    ))
                    .and_then(Option::as_ref)
            {
                let key = key.clone();
                recording_for_source.insert(
                    (
                        candidate.source.server_id.clone(),
                        candidate.source.track_id.clone(),
                    ),
                    key.clone(),
                );
                copies_for_recording
                    .entry(key)
                    .or_default()
                    .push(RankedCopy::new(
                        candidate.source.clone(),
                        &candidate.song,
                        source_order,
                    ));
            }
            let id = if multiple_servers {
                format!(
                    "{}:{}:{}",
                    candidate.source.server_id.len(),
                    candidate.source.server_id,
                    candidate.source.track_id
                )
            } else {
                candidate.source.track_id.clone()
            };
            identities.insert(id.clone(), candidate.source);
            let mut normalized = candidate.song;
            normalized.id = id;
            // Playback has no byte budget. The shared sync selector still needs a
            // positive estimate to admit an otherwise playable track.
            if normalized.size_bytes == Some(0)
                || (normalized.size_bytes.is_none()
                    && (normalized.bitrate_kbps.unwrap_or(0) == 0
                        || normalized.duration_seconds == 0))
            {
                normalized.size_bytes = Some(1);
            }
            candidates.push(Candidate::new(normalized));
        }
    }
    input
        .pools
        .insert(SourceKey::new(SourceKind::Library, None), candidates);
    let best_copy: HashMap<_, _> = copies_for_recording
        .into_iter()
        .filter_map(|(key, copies)| {
            rank_copies(copies)
                .into_iter()
                .next()
                .map(|copy| (key, copy))
        })
        .collect();
    let mut seen = HashSet::new();
    let mut seen_recordings = HashSet::new();
    let result: Vec<_> = run_pipeline(&input, &pipeline)
        .into_iter()
        .filter_map(|item| identities.get(&item.id).cloned())
        .filter(|source| {
            recording_for_source
                .get(&(source.server_id.clone(), source.track_id.clone()))
                .is_none_or(|key| seen_recordings.insert(key.clone()))
        })
        .map(|source| {
            recording_for_source
                .get(&(source.server_id.clone(), source.track_id.clone()))
                .and_then(|key| best_copy.get(key))
                .cloned()
                .unwrap_or(source)
        })
        .filter(|source| seen.insert((source.server_id.clone(), source.track_id.clone())))
        .take(usize::from(config.max_tracks))
        .collect();
    if result.is_empty() {
        Err(SelectionError::Empty)
    } else {
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn short_radio_page_requires_a_successful_next_offset_probe() {
        let fetch = |offset, _limit| {
            std::future::ready(Ok::<Vec<Song>, ()>(match offset {
                0 => vec![song("first", "First")],
                1 => vec![song("second", "Second")],
                _ => Vec::new(),
            }))
        };
        let (first, next) = fetch_offset_window(RadioSourceCursor::default(), fetch)
            .await
            .unwrap();
        assert_eq!(first[0].id, "first");
        assert_eq!(next.unwrap().index, 1);
        let (second, end) = fetch_offset_window(next.unwrap(), fetch).await.unwrap();
        assert_eq!(second[0].id, "second");
        assert!(end.is_none());
        let failed = fetch_offset_window(RadioSourceCursor::default(), |offset, _| {
            std::future::ready(if offset == 0 {
                Ok(vec![song("first", "First")])
            } else {
                Err(())
            })
        })
        .await;
        assert!(matches!(failed, Err(SelectionError::SourceUnavailable)));
    }

    fn song(id: &str, title: &str) -> Song {
        Song {
            id: id.into(),
            title: title.into(),
            artist_id: None,
            artist_name: None,
            album_id: None,
            album_title: None,
            duration_seconds: 180,
            bitrate_kbps: Some(256),
            track_number: None,
            disc_number: None,
            cover_art_id: None,
            date_added: None,
            last_played_at: None,
            play_count: None,
            is_favorite: None,
            content_type: None,
            suffix: None,
            size_bytes: Some(1_000),
            album_loudness: Default::default(),
            track_loudness: Default::default(),
            provider_metadata: Default::default(),
        }
    }

    fn source(server_id: &str) -> SelectionSource {
        SelectionSource {
            server_id: server_id.into(),
            kind: SelectionKind::Playlist,
            ref_id: "playlist".into(),
        }
    }

    fn recorded_song(id: &str, recording_id: &str, bitrate: u32, codec: &str) -> Song {
        use crate::playback::recording::{RecordingEvidence, RecordingProvenance};
        let mut item = song(id, "Take");
        item.bitrate_kbps = Some(bitrate);
        item.suffix = Some(codec.into());
        item.provider_metadata.recording = Some(RecordingEvidence::from_recording_field(
            RecordingProvenance::OpenSubsonicSong,
            Some(recording_id),
            &item.title,
        ));
        item
    }

    #[test]
    fn recording_conflict_stays_uncertain_after_a_third_observation() {
        let first_id = "189002e7-3285-4e2e-92a3-7f6c30d407a2";
        let second_id = "aaaaaaaa-aaaa-4aaa-aaaa-aaaaaaaaaaaa";
        let mut pools = (0..3)
            .map(|index| {
                let mut scope = source("one");
                scope.ref_id = format!("scope-{index}");
                SelectionPool {
                    source: scope,
                    tracks: vec![recorded_song(
                        "local",
                        if index == 0 { first_id } else { second_id },
                        256,
                        "mp3",
                    )],
                }
            })
            .collect::<Vec<_>>();
        assert!(recording_keys_after_conflict_clear(&mut pools).is_empty());
        assert!(
            pools
                .iter()
                .all(|pool| pool.tracks[0].provider_metadata.recording.is_none())
        );
    }

    #[test]
    fn copy_ranking_is_stable_and_fallback_uses_comparable_quality() {
        let id = "189002e7-3285-4e2e-92a3-7f6c30d407a2";
        let config = PlaybackSelectionConfig {
            sources: vec![source("one"), source("two"), source("three")],
            ..Default::default()
        };
        let first = recorded_song("a", id, 700, "flac");
        let best = recorded_song("b", id, 1000, "flac");
        let incompatible = recorded_song("c", id, 320, "mp3");
        let pools = vec![
            SelectionPool {
                source: source("one"),
                tracks: vec![first, incompatible],
            },
            SelectionPool {
                source: source("two"),
                tracks: vec![best],
            },
        ];
        let expected = TrackSource {
            server_id: "two".into(),
            track_id: "b".into(),
        };
        assert_eq!(
            select_with_recordings(&config, pools.clone()).unwrap(),
            vec![expected.clone()]
        );
        let mut reversed = pools.clone();
        reversed[0].tracks.reverse();
        assert_eq!(
            select_with_recordings(&config, reversed).unwrap(),
            vec![expected]
        );

        let pools = vec![
            SelectionPool {
                source: source("one"),
                tracks: vec![recorded_song("primary", id, 320, "mp3")],
            },
            SelectionPool {
                source: source("two"),
                tracks: vec![recorded_song("low", id, 128, "mp3")],
            },
            SelectionPool {
                source: source("three"),
                tracks: vec![recorded_song("high", id, 256, "mp3")],
            },
        ];
        let primary = TrackSource {
            server_id: "one".into(),
            track_id: "primary".into(),
        };
        assert_eq!(
            radio_copy_alternates(&config, &pools, &primary)
                .iter()
                .map(|source| source.track_id.as_str())
                .collect::<Vec<_>>(),
            vec!["high", "low"]
        );
    }

    #[test]
    fn radio_start_uses_artist_anchor_from_another_confident_copy() {
        let id = "189002e7-3285-4e2e-92a3-7f6c30d407a2";
        let config = PlaybackSelectionConfig {
            sources: vec![source("one"), source("two")],
            ..Default::default()
        };
        let mut anchor = recorded_song("anchor", id, 128, "mp3");
        anchor.artist_id = Some("artist-one".into());
        let pools = vec![
            SelectionPool {
                source: source("one"),
                tracks: vec![anchor],
            },
            SelectionPool {
                source: source("two"),
                tracks: vec![recorded_song("preferred", id, 320, "mp3")],
            },
        ];
        let preferred = TrackSource {
            server_id: "two".into(),
            track_id: "preferred".into(),
        };
        assert_eq!(
            radio_center_for_source(&config, &pools, &preferred),
            Some(super::super::radio::ArtistIdentity {
                server_id: "one".into(),
                artist_id: "artist-one".into()
            })
        );
    }

    #[test]
    fn radio_groups_confident_copies_and_keeps_uncertain_performances() {
        use crate::playback::recording::{RecordingEvidence, RecordingProvenance};
        let id = "189002e7-3285-4e2e-92a3-7f6c30d407a2";
        let mut first = song("colliding", "Take");
        first.suffix = Some("mp3".into());
        first.bitrate_kbps = Some(128);
        first.provider_metadata.recording = Some(RecordingEvidence::from_recording_field(
            RecordingProvenance::JellyfinRecording,
            Some(id),
            &first.title,
        ));
        let mut better = first.clone();
        better.bitrate_kbps = Some(320);
        better.provider_metadata.recording = Some(RecordingEvidence::from_recording_field(
            RecordingProvenance::OpenSubsonicSong,
            Some(id),
            &better.title,
        ));
        let mut live = song("live", "Take (Live)");
        live.provider_metadata.recording = Some(RecordingEvidence::from_recording_field(
            RecordingProvenance::OpenSubsonicSong,
            Some(id),
            &live.title,
        ));
        let uncertain = song("uncertain", "Take");
        let config = PlaybackSelectionConfig {
            sources: vec![source("one"), source("two")],
            max_tracks: 10,
            ..Default::default()
        };
        let result = select_with_recordings(
            &config,
            vec![
                SelectionPool {
                    source: source("one"),
                    tracks: vec![first, live],
                },
                SelectionPool {
                    source: source("two"),
                    tracks: vec![better, uncertain],
                },
            ],
        )
        .unwrap();
        assert_eq!(result.len(), 3);
        assert!(result.contains(&TrackSource {
            server_id: "two".into(),
            track_id: "colliding".into()
        }));
        assert!(result.contains(&TrackSource {
            server_id: "one".into(),
            track_id: "live".into()
        }));
        assert!(result.contains(&TrackSource {
            server_id: "two".into(),
            track_id: "uncertain".into()
        }));
        let mut original = song("first", "Take");
        original.provider_metadata.recording = Some(RecordingEvidence::from_recording_field(
            RecordingProvenance::JellyfinRecording,
            Some(id),
            "Take",
        ));
        let mut copy = song("second", "Take");
        copy.provider_metadata.recording = Some(RecordingEvidence::from_recording_field(
            RecordingProvenance::OpenSubsonicSong,
            Some(id),
            "Take",
        ));
        let fallbacks = radio_copy_alternates(
            &config,
            &[
                SelectionPool {
                    source: source("one"),
                    tracks: vec![original],
                },
                SelectionPool {
                    source: source("two"),
                    tracks: vec![copy],
                },
            ],
            &TrackSource {
                server_id: "one".into(),
                track_id: "first".into(),
            },
        );
        assert_eq!(
            fallbacks,
            vec![TrackSource {
                server_id: "two".into(),
                track_id: "second".into()
            }]
        );
    }

    #[test]
    fn incompatible_or_unknown_quality_uses_configured_source_order() {
        use crate::playback::recording::{RecordingEvidence, RecordingProvenance};
        let id = "189002e7-3285-4e2e-92a3-7f6c30d407a2";
        let mut first = song("a", "Take");
        first.suffix = Some("flac".into());
        first.bitrate_kbps = Some(700);
        first.provider_metadata.recording = Some(RecordingEvidence::from_recording_field(
            RecordingProvenance::JellyfinRecording,
            Some(id),
            "Take",
        ));
        let mut second = song("b", "Take");
        second.suffix = Some("mp3".into());
        second.bitrate_kbps = Some(320);
        second.provider_metadata.recording = Some(RecordingEvidence::from_recording_field(
            RecordingProvenance::OpenSubsonicSong,
            Some(id),
            "Take",
        ));
        let config = PlaybackSelectionConfig {
            sources: vec![source("one"), source("two")],
            ..Default::default()
        };
        let pools = vec![
            SelectionPool {
                source: source("one"),
                tracks: vec![first.clone()],
            },
            SelectionPool {
                source: source("two"),
                tracks: vec![second.clone()],
            },
        ];
        assert_eq!(
            select_with_recordings(&config, pools).unwrap(),
            vec![TrackSource {
                server_id: "one".into(),
                track_id: "a".into()
            }]
        );
        first.suffix = Some("mp3".into());
        first.bitrate_kbps = None;
        let pools = vec![
            SelectionPool {
                source: source("one"),
                tracks: vec![first],
            },
            SelectionPool {
                source: source("two"),
                tracks: vec![second],
            },
        ];
        assert_eq!(
            select_with_recordings(&config, pools).unwrap(),
            vec![TrackSource {
                server_id: "one".into(),
                track_id: "a".into()
            }]
        );
    }

    #[test]
    fn conflicting_observations_of_one_source_copy_remove_its_identity() {
        use crate::playback::recording::{RecordingEvidence, RecordingProvenance};
        let mut first = song("local", "Take");
        first.provider_metadata.recording = Some(RecordingEvidence::from_recording_field(
            RecordingProvenance::OpenSubsonicSong,
            Some("189002e7-3285-4e2e-92a3-7f6c30d407a2"),
            "Take",
        ));
        let mut changed = first.clone();
        changed.provider_metadata.recording = Some(RecordingEvidence::from_recording_field(
            RecordingProvenance::OpenSubsonicSong,
            Some("aaaaaaaa-aaaa-4aaa-aaaa-aaaaaaaaaaaa"),
            "Take",
        ));
        let mut other_scope = source("one");
        other_scope.ref_id = "second-playlist".into();
        let mut pools = vec![
            SelectionPool {
                source: source("one"),
                tracks: vec![first],
            },
            SelectionPool {
                source: other_scope,
                tracks: vec![changed],
            },
        ];
        clear_conflicting_recordings(&mut pools);
        assert!(
            pools
                .iter()
                .flat_map(|pool| &pool.tracks)
                .all(|song| song.provider_metadata.recording.is_none())
        );
    }

    #[test]
    fn radio_order_overrides_first_track_cap_without_changing_saved_settings() {
        let mut config = PlaybackSelectionConfig {
            sources: vec![source("server")],
            max_tracks: 1,
            ..Default::default()
        };
        config.seed = 11;
        let pool = SelectionPool {
            source: source("server"),
            tracks: (0..12)
                .map(|n| song(&format!("track-{n}"), "track"))
                .collect(),
        };
        assert_eq!(select(&config, vec![pool.clone()]).unwrap().len(), 1);
        assert_eq!(select_radio_order(&config, vec![pool]).unwrap().len(), 10);
        assert_eq!(config.max_tracks, 1);
    }

    #[test]
    fn artist_evidence_rejects_missing_or_conflicting_metadata() {
        let mut known = song("track", "Track");
        known.artist_id = Some("artist".into());
        let source_id = TrackSource {
            server_id: "server".into(),
            track_id: "track".into(),
        };
        let pool = SelectionPool {
            source: source("server"),
            tracks: vec![known.clone()],
        };
        assert_eq!(
            artist_for_source(&[pool.clone()], &source_id).as_deref(),
            Some("artist")
        );
        let mut missing = known.clone();
        missing.artist_id = None;
        assert_eq!(
            artist_for_source(
                &[
                    pool.clone(),
                    SelectionPool {
                        source: source("server"),
                        tracks: vec![missing]
                    }
                ],
                &source_id
            ),
            None
        );
        let mut different = known;
        different.artist_id = Some("other".into());
        assert_eq!(
            artist_for_source(
                &[
                    pool,
                    SelectionPool {
                        source: source("server"),
                        tracks: vec![different]
                    }
                ],
                &source_id
            ),
            None
        );
        let mut ambiguous = song("track", "Track");
        ambiguous.artist_id = Some("artist".into());
        ambiguous.provider_metadata.ambiguous_music_artist = true;
        assert_eq!(
            artist_for_source(
                &[SelectionPool {
                    source: source("server"),
                    tracks: vec![ambiguous]
                }],
                &source_id
            ),
            None
        );
    }

    #[test]
    fn same_server_uses_shared_selector_order_for_explicit_seed() {
        let config = PlaybackSelectionConfig {
            sources: vec![source("server")],
            ordering: vec![OrderingKey::Random],
            seed: 42,
            max_tracks: 3,
            ..Default::default()
        };
        let tracks = vec![song("b", "Two"), song("a", "One"), song("c", "Three")];
        let selected = select(
            &config,
            vec![SelectionPool {
                source: source("server"),
                tracks: tracks.clone(),
            }],
        )
        .unwrap();
        let mut pipeline = AutoFillPipeline::default();
        pipeline.ordering = config.ordering.clone();
        pipeline.sources = vec![SourceEntry::new(SourceKind::Library)];
        let input = PipelineInput {
            seed: 42,
            ..PipelineInput::default()
        }
        .with_pool(
            SourceKind::Library,
            None,
            tracks.into_iter().map(Candidate::new).collect(),
        );
        let expected: Vec<_> = run_pipeline(&input, &pipeline)
            .into_iter()
            .map(|item| item.id)
            .collect();
        assert_eq!(
            selected.into_iter().map(|s| s.track_id).collect::<Vec<_>>(),
            expected
        );
    }

    #[test]
    fn equal_local_ids_and_titles_from_different_servers_stay_distinct() {
        let config = PlaybackSelectionConfig {
            sources: vec![source("server-a"), source("server-b")],
            ordering: vec![OrderingKey::Random],
            max_tracks: 2,
            ..Default::default()
        };
        let selected = select(
            &config,
            vec![
                SelectionPool {
                    source: source("server-a"),
                    tracks: vec![song("same", "Same title")],
                },
                SelectionPool {
                    source: source("server-b"),
                    tracks: vec![song("same", "Same title")],
                },
            ],
        )
        .unwrap();
        assert_eq!(selected.len(), 2);
        assert_ne!(selected[0].server_id, selected[1].server_id);
        assert!(selected.iter().all(|s| s.track_id == "same"));
    }

    #[test]
    fn missing_configuration_and_empty_pool_have_distinct_errors() {
        assert!(matches!(
            select(&PlaybackSelectionConfig::default(), vec![]),
            Err(SelectionError::Setup)
        ));
        let config = PlaybackSelectionConfig {
            sources: vec![source("server")],
            ..Default::default()
        };
        assert!(matches!(
            select(
                &config,
                vec![SelectionPool {
                    source: source("server"),
                    tracks: vec![]
                }]
            ),
            Err(SelectionError::Empty)
        ));
    }

    #[test]
    fn playable_track_without_sync_size_metadata_is_selected() {
        let config = PlaybackSelectionConfig {
            sources: vec![source("server")],
            ..Default::default()
        };
        let mut track = song("stream-only", "Playable");
        track.size_bytes = None;
        track.bitrate_kbps = None;
        assert_eq!(
            select(
                &config,
                vec![SelectionPool {
                    source: source("server"),
                    tracks: vec![track],
                }],
            )
            .unwrap()[0]
                .track_id,
            "stream-only"
        );
    }

    #[test]
    fn concurrent_pure_selection_does_not_share_memory_or_random_state() {
        let config = PlaybackSelectionConfig {
            sources: vec![source("server")],
            seed: 1234,
            max_tracks: 3,
            ..Default::default()
        };
        let pool = SelectionPool {
            source: source("server"),
            tracks: vec![song("one", "A"), song("two", "B"), song("three", "C")],
        };
        let expected = select(&config, vec![pool.clone()]).unwrap();
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let config = config.clone();
                let pool = pool.clone();
                std::thread::spawn(move || select(&config, vec![pool]).unwrap())
            })
            .collect();
        for handle in handles {
            assert_eq!(handle.join().unwrap(), expected);
        }
    }

    #[test]
    fn simultaneous_sync_and_playback_selection_keep_independent_inputs() {
        let songs = vec![song("one", "A"), song("two", "B"), song("three", "C")];
        let mut sync_pipeline = AutoFillPipeline::default();
        sync_pipeline.sources = vec![SourceEntry::new(SourceKind::Library)];
        sync_pipeline.ordering = vec![OrderingKey::Random];
        let sync_input = PipelineInput {
            seed: 77,
            ..Default::default()
        }
        .with_pool(
            SourceKind::Library,
            None,
            songs.iter().cloned().map(Candidate::new).collect(),
        );
        let expected_sync: Vec<_> = run_pipeline(&sync_input, &sync_pipeline)
            .into_iter()
            .map(|item| item.id)
            .collect();
        let playback_config = PlaybackSelectionConfig {
            sources: vec![source("portable")],
            seed: 77,
            max_tracks: 3,
            ..Default::default()
        };
        let sync = std::thread::spawn(move || {
            run_pipeline(&sync_input, &sync_pipeline)
                .into_iter()
                .map(|item| item.id)
                .collect::<Vec<_>>()
        });
        let playback = std::thread::spawn(move || {
            select(
                &playback_config,
                vec![SelectionPool {
                    source: source("portable"),
                    tracks: songs,
                }],
            )
            .unwrap()
        });
        assert_eq!(sync.join().unwrap(), expected_sync);
        assert_eq!(
            playback
                .join()
                .unwrap()
                .into_iter()
                .map(|s| s.track_id)
                .collect::<Vec<_>>(),
            expected_sync
        );
    }

    #[test]
    fn config_round_trip_is_independent_of_output_preference() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("playback.json");
        fs::write(&output, br#"{"schemaVersion":1,"output":null}"#).unwrap();
        let path = dir.path().join("playback-selection.json");
        let config = PlaybackSelectionConfig {
            sources: vec![SelectionSource {
                server_id: "portable-a".into(),
                kind: SelectionKind::Playlist,
                ref_id: "playlist".into(),
            }],
            ..Default::default()
        };
        save(&path, &config).unwrap();
        assert_eq!(load(&path).unwrap(), config);
        assert_eq!(
            fs::read(&output).unwrap(),
            br#"{"schemaVersion":1,"output":null}"#
        );
    }

    #[test]
    fn invalid_schema_and_unbounded_sources_are_rejected() {
        let mut config = PlaybackSelectionConfig::default();
        config.schema_version = 2;
        assert!(config.validate().is_err());
        config.schema_version = 1;
        config.sources = (0..=MAX_SOURCES)
            .map(|n| SelectionSource {
                server_id: "s".into(),
                kind: SelectionKind::Playlist,
                ref_id: n.to_string(),
            })
            .collect();
        assert!(config.validate().is_err());
    }

    #[test]
    fn future_schema_file_is_preserved_on_save_attempt() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("playback-selection.json");
        let future =
            br#"{"schemaVersion":2,"sources":[],"ordering":["random"],"seed":0,"maxTracks":1}"#;
        fs::write(&path, future).unwrap();
        assert!(matches!(
            save(&path, &PlaybackSelectionConfig::default()),
            Err(SelectionError::Setup)
        ));
        assert_eq!(fs::read(&path).unwrap(), future);
    }
}
