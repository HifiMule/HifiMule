//! Playback-owned finite selection. Provider work ends before entering the pure selector.
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
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
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionOption {
    pub id: String,
    pub name: String,
}

/// Options are resolved by portable server identity and never by the UI's browsed server.
/// The bounded catalog is also the reference validation surface for settings writes.
pub async fn options(
    provider: &dyn MediaProvider,
    kind: SelectionKind,
) -> Result<Vec<SelectionOption>, SelectionError> {
    let mode = match kind {
        SelectionKind::Playlist => BrowseMode::Playlists,
        SelectionKind::Artist => BrowseMode::Artists,
        SelectionKind::Genre => BrowseMode::Genres,
    };
    if !provider.capabilities().browse.list_modes.contains(&mode) {
        return Err(SelectionError::SourceUnavailable);
    }
    let values = match kind {
        SelectionKind::Playlist => provider
            .list_playlists()
            .await
            .map_err(|_| SelectionError::SourceUnavailable)?
            .into_iter()
            .take(MAX_CANDIDATES_PER_SOURCE)
            .map(|p| SelectionOption {
                id: p.id,
                name: p.name,
            })
            .collect(),
        SelectionKind::Artist => provider
            .list_artists(None, None, 0, MAX_CANDIDATES_PER_SOURCE as u32)
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
            .list_genres(None, 0, MAX_CANDIDATES_PER_SOURCE as u32)
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
    Ok(values)
}

pub async fn fetch_source(
    provider: &dyn MediaProvider,
    source: &SelectionSource,
) -> Result<SelectionPool, SelectionError> {
    if matches!(source.kind, SelectionKind::Artist | SelectionKind::Genre)
        && !options(provider, source.kind)
            .await?
            .iter()
            .any(|choice| choice.id == source.ref_id)
    {
        return Err(SelectionError::SourceUnavailable);
    }
    let mut tracks = match source.kind {
        SelectionKind::Playlist => {
            provider
                .get_playlist(&source.ref_id)
                .await
                .map_err(|_| SelectionError::SourceUnavailable)?
                .tracks
        }
        SelectionKind::Artist => {
            let artist = provider
                .get_artist(&source.ref_id)
                .await
                .map_err(|_| SelectionError::SourceUnavailable)?;
            let mut all = Vec::new();
            for album in artist.albums.into_iter().take(MAX_ARTIST_ALBUMS) {
                let result = provider
                    .get_album(&album.id)
                    .await
                    .map_err(|_| SelectionError::SourceUnavailable)?;
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

/// Materialize the shared pure selector. A single-server pool uses its original IDs, so
/// equivalent sync and Playback fixtures yield the same order for an explicit seed. With
/// multiple servers the internal ID is a reversible, collision-free namespace; the output
/// is mapped back to the typed portable identity before it leaves this function.
pub fn select(
    config: &PlaybackSelectionConfig,
    pools: Vec<SelectionPool>,
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
    let mut pipeline = AutoFillPipeline::default();
    pipeline.ordering = config.ordering.clone();
    pipeline.sources = vec![SourceEntry::new(SourceKind::Library)];
    let mut input = PipelineInput {
        seed: config.seed,
        ..Default::default()
    };
    let mut identities = HashMap::<String, TrackSource>::new();
    let mut candidates = Vec::new();
    for source in &config.sources {
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
            };
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
            candidates.push(Candidate::new(normalized));
        }
    }
    input
        .pools
        .insert(SourceKey::new(SourceKind::Library, None), candidates);
    let mut seen = HashSet::new();
    let result: Vec<_> = run_pipeline(&input, &pipeline)
        .into_iter()
        .filter_map(|item| identities.get(&item.id).cloned())
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
