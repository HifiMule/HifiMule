//! Async pool-materialization layer for the configurable auto-fill pipeline (Epic 12, Story 12.4).
//!
//! This is the impure half of the split documented in [`super::pipeline`]: it performs the
//! provider I/O (playlist fetch, genre membership, favorites/history/library pagination) needed
//! to turn a configured [`AutoFillPipeline`] into the already-materialized
//! [`PipelineInput`] pools that the pure `run_pipeline` engine consumes. The pure engine stays
//! pure — all network/`async`/`MediaProvider` work lives here, never in `pipeline.rs`.
//!
//! ## Default vs configurable — the fast-path discriminator
//!
//! [`needs_configurable_expansion`] decides whether a pipeline is *default-legacy-equivalent*
//! (favorites→playCount→dateCreated over the library, byte-budgeted). Default pipelines keep the
//! existing smart-incremental `run_auto_fill_provider` path (zero regression). Only a genuinely
//! non-default pipeline (playlist/genre/favorites/history sources, a filter, per-source shares, a
//! non-`Track` unit, a non-legacy ordering, memory modifiers, or a fallback chain) is routed
//! through [`expand_with_pipeline`], which materializes pools and runs the pure engine.
//!
//! ## Scope boundaries (Story 12.4)
//!
//! - **Tag filter** is config-only: no provider enumerates per-track tags yet, so tag constraints
//!   are dropped (pass-through) with a log. (A real tag source is Epic 13.)
//! - **Genre filter** is capability-gated: when the provider does not advertise
//!   [`BrowseMode::Genres`] (or `get_genre_tracks` returns `UnsupportedCapability`), genre
//!   constraints are dropped (pass-through) rather than silently emptying the fill.
//! - **History/memory** is inert: an empty [`HistorySnapshot`] is supplied; the `autofill_history`
//!   DB table is neither read nor written here (Epic 13 wires it). Cooldown/played-exclusion config
//!   therefore has no effect yet.

use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::pipeline::{
    AutoFillPipeline, Candidate, FilterStage, HistorySnapshot, MemoryStage, OrderingKey,
    PipelineInput, SourceKey, SourceKind, Unit, run_pipeline,
};
use super::{AutoFillItem, AutoFillParams};
use crate::domain::models::Song;
use crate::providers::{BrowseMode, MediaProvider, ProviderError};

/// Fetch bounds, mirrored verbatim from `run_auto_fill_provider` so the configurable path never
/// re-derives different constants (Story 12.4 task note).
const MAX_PER_LIST: u32 = 2000;
const PAGE_SIZE: u32 = 500;
const MAX_BULK_PAGES: u32 = 200;

/// Whether a pipeline must go through the configurable async materialization path.
///
/// Returns `false` (take the fast `run_auto_fill_provider`/`run_auto_fill` path) **only** when the
/// pipeline is default-legacy-equivalent:
/// - `sources` is empty or exactly one bare `Library` source (no `ref`, no `share`),
/// - `filter` is all-empty,
/// - `ordering` is empty or exactly `[Favorite, PlayCount, DateCreated]`,
/// - `unit == Track`,
/// - `memory == MemoryStage::default()`,
/// - `fallback` is empty.
///
/// Any deviation (a playlist/genre source, a filter, a share weight, an album/artist unit, a
/// quality ordering, a memory modifier, a fallback chain) returns `true`. `enabled` and `budget`
/// are intentionally NOT part of the discriminator: enabling is a caller concern and the budget is
/// already honored by the fast path's `max_fill_bytes`, so neither requires materialization.
pub fn needs_configurable_expansion(p: &AutoFillPipeline) -> bool {
    let sources_default = match p.sources.as_slice() {
        [] => true,
        [s] => s.kind == SourceKind::Library && s.ref_id.is_none() && s.share.is_none(),
        _ => false,
    };
    let filter_default = p.filter == FilterStage::default();
    let ordering_default = p.ordering.is_empty()
        || p.ordering
            == [
                OrderingKey::Favorite,
                OrderingKey::PlayCount,
                OrderingKey::DateCreated,
            ];
    let unit_default = p.unit == Unit::Track;
    let memory_default = p.memory == MemoryStage::default();
    let fallback_default = p.fallback.is_empty();

    !(sources_default
        && filter_default
        && ordering_default
        && unit_default
        && memory_default
        && fallback_default)
}

/// Materialize a configured pipeline's source pools from the provider, then run the pure engine.
///
/// `async` only for the fetch; the selection core stays the synchronous, fixture-tested
/// `run_pipeline`. Returns the budget-bounded, dedup'd selection. Best-effort throughout: a source
/// the provider can't satisfy (`UnsupportedCapability`/`NotFound`/missing playlist `ref`)
/// contributes an empty pool and a log rather than aborting the slot.
pub async fn expand_with_pipeline(
    provider: Arc<dyn MediaProvider>,
    pipeline: &AutoFillPipeline,
    params: AutoFillParams,
) -> Result<Vec<AutoFillItem>> {
    // Work on a normalized clone: drop tag constraints (no data), capability-gate genre
    // constraints, and cap the budget by the slot's shared-remaining ceiling.
    let mut normalized = pipeline.clone();

    // --- Tag constraints (AC 4): always dropped in 12.4 — no provider enumerates per-track tags.
    if !normalized.filter.include_tags.is_empty() || !normalized.filter.exclude_tags.is_empty() {
        crate::daemon_log!(
            "[AutoFill] tag filter configured but no provider tag data in 12.4 — dropping tag constraints (pass-through)"
        );
        normalized.filter.include_tags.clear();
        normalized.filter.exclude_tags.clear();
    }

    // --- Genre membership (AC 3): capability-gated, drop-to-pass-through on unsupported.
    let genre_map = resolve_genre_membership(provider.as_ref(), &mut normalized).await;

    // --- Budget cap (AC 6): honor a smaller user budget but never exceed the slot's ceiling.
    let max_fill_bytes = params.max_fill_bytes;
    let capped = normalized
        .budget
        .max_bytes
        .unwrap_or(max_fill_bytes)
        .min(max_fill_bytes);
    normalized.budget.max_bytes = Some(capped);
    // Story 12.4 only honors max_bytes capped by the slot budget. Headroom and duration-target
    // refinements belong to Story 12.5, so keep them inert in the materialized path for now.
    normalized.budget.target_duration_secs = None;
    normalized.budget.headroom_bytes = None;

    // --- Materialize one pool per distinct (kind, ref) across sources ∪ fallback.
    let mut keys: Vec<SourceKey> = Vec::new();
    let mut seen_keys: HashSet<SourceKey> = HashSet::new();
    if normalized.sources.is_empty() {
        let key = SourceKey::new(SourceKind::Library, None);
        seen_keys.insert(key.clone());
        keys.push(key);
    }
    for entry in normalized.sources.iter().chain(normalized.fallback.iter()) {
        let key = SourceKey::new(entry.kind, entry.ref_id.clone());
        if seen_keys.insert(key.clone()) {
            keys.push(key);
        }
    }

    let mut pools: HashMap<SourceKey, Vec<Candidate>> = HashMap::new();
    for key in keys {
        let songs = materialize_pool(provider.as_ref(), &key).await;
        let candidates = songs
            .into_iter()
            .map(|song| {
                let genres = genre_map.get(&song.id).cloned().unwrap_or_default();
                Candidate {
                    song,
                    genres,
                    tags: Vec::new(),
                }
            })
            .collect();
        pools.insert(key, candidates);
    }

    // --- History (AC 7): empty snapshot — cooldown/played-exclusion inert until Epic 13. No DB read.
    let input = PipelineInput {
        pools,
        history: HistorySnapshot::default(),
        exclude_item_ids: params.exclude_item_ids,
    };

    Ok(run_pipeline(&input, &normalized))
}

/// Resolve `track_id -> [genre]` membership for the filter's referenced genres, mutating
/// `pipeline.filter` to drop genre constraints (pass-through) when the provider cannot enumerate
/// genres. Returns the membership map (empty when no genre filter applies or it was dropped).
async fn resolve_genre_membership(
    provider: &dyn MediaProvider,
    pipeline: &mut AutoFillPipeline,
) -> HashMap<String, Vec<String>> {
    let wants_genre =
        !pipeline.filter.include_genres.is_empty() || !pipeline.filter.exclude_genres.is_empty();
    if !wants_genre {
        return HashMap::new();
    }

    let supports_genres = provider
        .capabilities()
        .browse
        .list_modes
        .contains(&BrowseMode::Genres);
    if !supports_genres {
        crate::daemon_log!(
            "[AutoFill] genre filter configured but provider does not advertise BrowseMode::Genres — dropping genre constraints (pass-through)"
        );
        pipeline.filter.include_genres.clear();
        pipeline.filter.exclude_genres.clear();
        return HashMap::new();
    }

    // Distinct genres referenced by include ∪ exclude.
    let mut genres: Vec<String> = pipeline
        .filter
        .include_genres
        .iter()
        .chain(pipeline.filter.exclude_genres.iter())
        .cloned()
        .collect();
    genres.sort();
    genres.dedup();

    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    for genre in &genres {
        let mut offset = 0u32;
        let mut pages = 0u32;
        loop {
            match provider.get_genre_tracks(genre, offset, MAX_PER_LIST).await {
                Ok((songs, total)) => {
                    let count = songs.len() as u32;
                    if count == 0 {
                        break;
                    }

                    for song in songs {
                        map.entry(song.id).or_default().push(genre.clone());
                    }

                    pages += 1;
                    let next_offset = offset.saturating_add(count);
                    if count < MAX_PER_LIST || next_offset >= total || pages >= MAX_BULK_PAGES {
                        break;
                    }
                    offset = next_offset;
                }
                Err(ProviderError::UnsupportedCapability(_)) => {
                    // Belt-and-suspenders: a runtime UnsupportedCapability is treated as the same
                    // graceful drop as a missing BrowseMode::Genres. Clear constraints and bail.
                    crate::daemon_log!(
                        "[AutoFill] get_genre_tracks returned UnsupportedCapability — dropping genre constraints (pass-through)"
                    );
                    pipeline.filter.include_genres.clear();
                    pipeline.filter.exclude_genres.clear();
                    return HashMap::new();
                }
                Err(e) => {
                    // Enforcing include/exclude with incomplete membership would silently empty or
                    // pollute fills. Treat provider lookup failures as pass-through for this slot.
                    crate::daemon_log!(
                        "[AutoFill] get_genre_tracks({}) failed: {} — dropping genre constraints (pass-through)",
                        genre,
                        e
                    );
                    pipeline.filter.include_genres.clear();
                    pipeline.filter.exclude_genres.clear();
                    return HashMap::new();
                }
            }
        }
    }
    map
}

/// Fetch the songs backing one source pool. Every unsupported/failed fetch yields an empty pool
/// plus a log — never an error that aborts the slot.
async fn materialize_pool(provider: &dyn MediaProvider, key: &SourceKey) -> Vec<Song> {
    match key.kind {
        SourceKind::Library => fetch_library(provider).await,
        SourceKind::Favorites => fetch_priority_list(
            provider.list_favorites(None, 0, MAX_PER_LIST).await,
            "list_favorites",
        ),
        SourceKind::History => fetch_priority_list(
            provider.list_recently_played(None, 0, MAX_PER_LIST).await,
            "list_recently_played",
        ),
        SourceKind::Playlist => fetch_playlist(provider, key.ref_id.as_deref()).await,
    }
}

/// Unwrap a `(songs, total)` priority-list result into a pool, mapping unsupported/error to empty.
fn fetch_priority_list(
    result: std::result::Result<(Vec<Song>, u32), ProviderError>,
    method: &str,
) -> Vec<Song> {
    match result {
        Ok((songs, _)) => songs,
        Err(ProviderError::UnsupportedCapability(_)) => {
            crate::daemon_log!("[AutoFill] {}: UnsupportedCapability, empty pool", method);
            Vec::new()
        }
        Err(e) => {
            crate::daemon_log!("[AutoFill] {} failed (non-fatal): {}", method, e);
            Vec::new()
        }
    }
}

/// Paginate the full library into a single pool, bounded by the shared `PAGE_SIZE`/`MAX_BULK_PAGES`
/// constants. Mirrors `run_auto_fill_provider`'s bulk-fill bounds.
async fn fetch_library(provider: &dyn MediaProvider) -> Vec<Song> {
    let mut all = Vec::new();
    let mut offset = 0u32;
    let mut pages = 0u32;
    loop {
        match provider.list_all_songs_page(None, offset, PAGE_SIZE).await {
            Ok((songs, _)) => {
                let count = songs.len() as u32;
                all.extend(songs);
                pages += 1;
                if count < PAGE_SIZE || pages >= MAX_BULK_PAGES {
                    break;
                }
                offset += PAGE_SIZE;
            }
            Err(ProviderError::UnsupportedCapability(_)) => {
                crate::daemon_log!(
                    "[AutoFill] list_all_songs_page: UnsupportedCapability, empty pool"
                );
                break;
            }
            Err(e) => {
                crate::daemon_log!("[AutoFill] list_all_songs_page failed (non-fatal): {}", e);
                break;
            }
        }
    }
    all
}

/// Fetch a playlist's tracks. A missing/blank `ref` or an unsupported/not-found playlist yields an
/// empty pool plus a log — never a full-library leak and never a slot abort.
async fn fetch_playlist(provider: &dyn MediaProvider, ref_id: Option<&str>) -> Vec<Song> {
    let Some(playlist_id) = ref_id.map(str::trim).filter(|s| !s.is_empty()) else {
        crate::daemon_log!("[AutoFill] playlist source has no ref — skipping (empty pool)");
        return Vec::new();
    };
    match provider.get_playlist(playlist_id).await {
        Ok(playlist) => playlist.tracks,
        Err(ProviderError::UnsupportedCapability(_)) => {
            crate::daemon_log!(
                "[AutoFill] get_playlist({}): UnsupportedCapability, empty pool",
                playlist_id
            );
            Vec::new()
        }
        Err(ProviderError::NotFound { .. }) => {
            crate::daemon_log!(
                "[AutoFill] get_playlist({}): not found, empty pool",
                playlist_id
            );
            Vec::new()
        }
        Err(e) => {
            crate::daemon_log!(
                "[AutoFill] get_playlist({}) failed (non-fatal): {}",
                playlist_id,
                e
            );
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auto_fill::pipeline::{BudgetStage, SourceEntry};
    use crate::domain::models::{Playlist, PlaylistWithTracks, SearchResult, Song};
    use crate::providers::{
        BrowseCapabilities, Capabilities, ProviderChangeContext, ScrobbleRequest, ServerType,
        TranscodeProfile,
    };
    use async_trait::async_trait;

    // -------------------------------------------------------------------
    // A hand-rolled MediaProvider returning canned pools/genres. Only the methods the fetch layer
    // calls are meaningful; the rest return UnsupportedCapability via trait defaults.
    // -------------------------------------------------------------------
    #[derive(Default)]
    struct MockProvider {
        library: Vec<Song>,
        favorites: Vec<Song>,
        recently_played: Vec<Song>,
        playlists: HashMap<String, Vec<Song>>,
        /// genre name -> song objects (so get_genre_tracks can return Songs).
        genre_songs: HashMap<String, Vec<Song>>,
        supports_genres: bool,
        genre_unsupported_at_runtime: bool,
        genre_error_at_runtime: bool,
        unsupported_recently_played: bool,
    }

    #[async_trait]
    impl MediaProvider for MockProvider {
        fn server_type(&self) -> ServerType {
            ServerType::Unknown
        }

        fn capabilities(&self) -> Capabilities {
            let mut list_modes = Vec::new();
            if self.supports_genres {
                list_modes.push(BrowseMode::Genres);
            }
            Capabilities {
                open_subsonic: false,
                supports_changes_since: false,
                supports_server_transcoding: false,
                supports_playlist_write: false,
                browse: BrowseCapabilities { list_modes },
            }
        }

        async fn list_libraries(
            &self,
        ) -> Result<Vec<crate::domain::models::Library>, ProviderError> {
            Ok(Vec::new())
        }

        async fn list_artists(
            &self,
            _library_id: Option<&str>,
            _letter: Option<&str>,
            _offset: u32,
            _limit: u32,
        ) -> Result<(Vec<crate::domain::models::Artist>, u32), ProviderError> {
            Ok((Vec::new(), 0))
        }

        async fn get_artist(
            &self,
            _artist_id: &str,
        ) -> Result<crate::domain::models::ArtistWithAlbums, ProviderError> {
            Err(ProviderError::UnsupportedCapability("get_artist".into()))
        }

        async fn list_albums(
            &self,
            _library_id: Option<&str>,
            _letter: Option<&str>,
            _offset: u32,
            _limit: u32,
        ) -> Result<(Vec<crate::domain::models::Album>, u32), ProviderError> {
            Ok((Vec::new(), 0))
        }

        async fn get_album(
            &self,
            _album_id: &str,
        ) -> Result<crate::domain::models::AlbumWithTracks, ProviderError> {
            Err(ProviderError::UnsupportedCapability("get_album".into()))
        }

        async fn list_playlists(
            &self,
        ) -> Result<Vec<crate::domain::models::Playlist>, ProviderError> {
            Ok(Vec::new())
        }

        async fn get_playlist(
            &self,
            playlist_id: &str,
        ) -> Result<PlaylistWithTracks, ProviderError> {
            match self.playlists.get(playlist_id) {
                Some(tracks) => Ok(PlaylistWithTracks {
                    playlist: Playlist {
                        id: playlist_id.to_string(),
                        name: format!("Playlist {playlist_id}"),
                        song_count: Some(tracks.len() as u32),
                        duration_seconds: None,
                        cover_art_id: None,
                    },
                    tracks: tracks.clone(),
                }),
                None => Err(ProviderError::NotFound {
                    item_type: "playlist".into(),
                    id: playlist_id.to_string(),
                }),
            }
        }

        async fn search(&self, _query: &str) -> Result<SearchResult, ProviderError> {
            Ok(SearchResult::default())
        }

        async fn download_url(
            &self,
            _song_id: &str,
            _profile: Option<&TranscodeProfile>,
        ) -> Result<String, ProviderError> {
            Err(ProviderError::UnsupportedCapability("download_url".into()))
        }

        async fn cover_art_url(&self, _cover_art_id: &str) -> Result<String, ProviderError> {
            Err(ProviderError::UnsupportedCapability("cover_art_url".into()))
        }

        async fn changes_since_with_context(
            &self,
            _token: Option<&str>,
            _context: &ProviderChangeContext,
        ) -> Result<Vec<crate::domain::models::ChangeEvent>, ProviderError> {
            Ok(Vec::new())
        }

        async fn scrobble(&self, _request: ScrobbleRequest) -> Result<(), ProviderError> {
            Ok(())
        }

        async fn list_favorites(
            &self,
            _library_id: Option<&str>,
            _offset: u32,
            _limit: u32,
        ) -> Result<(Vec<Song>, u32), ProviderError> {
            Ok((self.favorites.clone(), self.favorites.len() as u32))
        }

        async fn list_recently_played(
            &self,
            _library_id: Option<&str>,
            _offset: u32,
            _limit: u32,
        ) -> Result<(Vec<Song>, u32), ProviderError> {
            if self.unsupported_recently_played {
                return Err(ProviderError::UnsupportedCapability(
                    "list_recently_played".into(),
                ));
            }
            Ok((
                self.recently_played.clone(),
                self.recently_played.len() as u32,
            ))
        }

        async fn list_all_songs_page(
            &self,
            _library_id: Option<&str>,
            offset: u32,
            limit: u32,
        ) -> Result<(Vec<Song>, u32), ProviderError> {
            let start = offset as usize;
            let end = (start + limit as usize).min(self.library.len());
            let page = if start >= self.library.len() {
                Vec::new()
            } else {
                self.library[start..end].to_vec()
            };
            Ok((page, self.library.len() as u32))
        }

        async fn get_genre_tracks(
            &self,
            genre_id_or_name: &str,
            offset: u32,
            limit: u32,
        ) -> Result<(Vec<Song>, u32), ProviderError> {
            if !self.supports_genres || self.genre_unsupported_at_runtime {
                return Err(ProviderError::UnsupportedCapability(
                    "get_genre_tracks".into(),
                ));
            }
            if self.genre_error_at_runtime {
                return Err(ProviderError::Other(anyhow::anyhow!("genre fetch failed")));
            }
            let all = self
                .genre_songs
                .get(genre_id_or_name)
                .cloned()
                .unwrap_or_default();
            let total = all.len() as u32;
            let start = offset as usize;
            let end = (start + limit as usize).min(all.len());
            let page = if start >= all.len() {
                Vec::new()
            } else {
                all[start..end].to_vec()
            };
            Ok((page, total))
        }
    }

    fn song(id: &str, size_bytes: u64) -> Song {
        Song {
            id: id.to_string(),
            title: format!("Track {id}"),
            artist_id: Some(format!("artist-{id}")),
            artist_name: Some(format!("Artist {id}")),
            album_id: Some(format!("album-{id}")),
            album_title: Some(format!("Album {id}")),
            duration_seconds: 180,
            bitrate_kbps: Some(256),
            track_number: Some(1),
            disc_number: None,
            cover_art_id: None,
            date_added: Some("2024-01-01".to_string()),
            last_played_at: None,
            play_count: Some(0),
            is_favorite: Some(false),
            content_type: Some("audio/mpeg".to_string()),
            suffix: Some("mp3".to_string()),
            size_bytes: Some(size_bytes),
        }
    }

    fn params(max_fill_bytes: u64) -> AutoFillParams {
        AutoFillParams {
            exclude_item_ids: Vec::new(),
            max_fill_bytes,
        }
    }

    fn ids(items: &[AutoFillItem]) -> Vec<String> {
        items.iter().map(|i| i.id.clone()).collect()
    }

    fn arc(p: MockProvider) -> Arc<dyn MediaProvider> {
        Arc::new(p)
    }

    // ===================================================================
    // needs_configurable_expansion — both branches (AC 1, 8).
    // ===================================================================

    #[test]
    fn discriminator_default_pipelines_take_fast_path() {
        assert!(!needs_configurable_expansion(&AutoFillPipeline::default()));
        assert!(!needs_configurable_expansion(
            &AutoFillPipeline::default_legacy(Some(8_000_000_000))
        ));
        assert!(!needs_configurable_expansion(
            &AutoFillPipeline::default_legacy(None)
        ));
    }

    #[test]
    fn discriminator_non_default_pipelines_materialize() {
        // Playlist source.
        assert!(needs_configurable_expansion(&AutoFillPipeline {
            sources: vec![SourceEntry {
                kind: SourceKind::Playlist,
                ref_id: Some("energy".into()),
                share: None,
            }],
            ..Default::default()
        }));

        // Non-empty filter.
        let mut p = AutoFillPipeline::default_legacy(Some(1));
        p.filter.include_genres = vec!["kids".into()];
        assert!(needs_configurable_expansion(&p));

        // Per-source share.
        assert!(needs_configurable_expansion(&AutoFillPipeline {
            sources: vec![SourceEntry {
                kind: SourceKind::Library,
                ref_id: None,
                share: Some(0.5),
            }],
            ..Default::default()
        }));

        // Album unit.
        let mut p = AutoFillPipeline::default_legacy(Some(1));
        p.unit = Unit::Album;
        assert!(needs_configurable_expansion(&p));

        // Quality ordering.
        assert!(needs_configurable_expansion(&AutoFillPipeline {
            ordering: vec![OrderingKey::Quality],
            ..Default::default()
        }));

        // Memory modifier.
        let mut p = AutoFillPipeline::default_legacy(Some(1));
        p.memory.cooldown_weeks = Some(2);
        assert!(needs_configurable_expansion(&p));

        // Fallback chain.
        let mut p = AutoFillPipeline::default_legacy(Some(1));
        p.fallback = vec![SourceEntry::new(SourceKind::Library)];
        assert!(needs_configurable_expansion(&p));
    }

    // ===================================================================
    // expand_with_pipeline (AC 2, 3, 4, 5, 7).
    // ===================================================================

    #[tokio::test]
    async fn playlist_source_materializes_only_that_playlist() {
        let provider = arc(MockProvider {
            library: vec![song("lib-x", 1_000_000)],
            playlists: HashMap::from([(
                "energy".to_string(),
                vec![
                    song("e1", 3_000_000),
                    song("e2", 3_000_000),
                    song("e3", 3_000_000),
                ],
            )]),
            ..Default::default()
        });
        let pipeline = AutoFillPipeline {
            sources: vec![SourceEntry {
                kind: SourceKind::Playlist,
                ref_id: Some("energy".into()),
                share: None,
            }],
            budget: BudgetStage {
                max_bytes: Some(7_000_000),
                ..Default::default()
            },
            ..Default::default()
        };
        let result = expand_with_pipeline(provider, &pipeline, params(7_000_000))
            .await
            .unwrap();
        assert_eq!(
            ids(&result),
            vec!["e1", "e2"],
            "only playlist tracks, budget-truncated"
        );
        assert!(
            !ids(&result).contains(&"lib-x".to_string()),
            "library must not leak in"
        );
    }

    #[tokio::test]
    async fn playlist_source_with_missing_ref_is_skipped() {
        let provider = arc(MockProvider {
            playlists: HashMap::from([("energy".to_string(), vec![song("e1", 1_000_000)])]),
            ..Default::default()
        });
        let pipeline = AutoFillPipeline {
            sources: vec![SourceEntry {
                kind: SourceKind::Playlist,
                ref_id: None, // no ref → skipped, no leak, no panic
                share: None,
            }],
            budget: BudgetStage {
                max_bytes: Some(10_000_000),
                ..Default::default()
            },
            ..Default::default()
        };
        let result = expand_with_pipeline(provider, &pipeline, params(10_000_000))
            .await
            .unwrap();
        assert!(
            result.is_empty(),
            "missing playlist ref yields an empty pool"
        );
    }

    #[tokio::test]
    async fn genre_filter_includes_and_excludes_when_supported() {
        // Library has three tracks; "kids" includes t1/t2, "explicit" excludes t2.
        let provider = arc(MockProvider {
            library: vec![
                song("t1", 1_000_000),
                song("t2", 1_000_000),
                song("t3", 1_000_000),
            ],
            supports_genres: true,
            genre_songs: HashMap::from([
                (
                    "kids".to_string(),
                    vec![song("t1", 1_000_000), song("t2", 1_000_000)],
                ),
                ("explicit".to_string(), vec![song("t2", 1_000_000)]),
            ]),
            ..Default::default()
        });
        let pipeline = AutoFillPipeline {
            filter: FilterStage {
                include_genres: vec!["kids".into()],
                exclude_genres: vec!["explicit".into()],
                ..Default::default()
            },
            sources: vec![SourceEntry::new(SourceKind::Library)],
            budget: BudgetStage {
                max_bytes: Some(100_000_000),
                ..Default::default()
            },
            ..Default::default()
        };
        let result = expand_with_pipeline(provider, &pipeline, params(100_000_000))
            .await
            .unwrap();
        // t1 is "kids" and not "explicit" → kept. t2 is excluded. t3 not in "kids" → dropped.
        assert_eq!(ids(&result), vec!["t1"]);
    }

    #[tokio::test]
    async fn implicit_library_source_is_materialized_for_filter_only_pipeline() {
        // Empty `sources` defaults to Library inside run_pipeline; the fetch layer must materialize
        // the same implicit Library pool when any other stage makes the pipeline configurable.
        let provider = arc(MockProvider {
            library: vec![song("t1", 1_000_000), song("t2", 1_000_000)],
            supports_genres: true,
            genre_songs: HashMap::from([("kids".to_string(), vec![song("t1", 1_000_000)])]),
            ..Default::default()
        });
        let pipeline = AutoFillPipeline {
            filter: FilterStage {
                include_genres: vec!["kids".into()],
                ..Default::default()
            },
            budget: BudgetStage {
                max_bytes: Some(100_000_000),
                ..Default::default()
            },
            ..Default::default()
        };
        let result = expand_with_pipeline(provider, &pipeline, params(100_000_000))
            .await
            .unwrap();
        assert_eq!(ids(&result), vec!["t1"]);
    }

    #[tokio::test]
    async fn genre_filter_dropped_when_provider_lacks_capability() {
        // Provider does NOT advertise Genres; the genre filter must be dropped (pass-through),
        // never silently emptying the library fill.
        let provider = arc(MockProvider {
            library: vec![song("t1", 1_000_000), song("t2", 1_000_000)],
            supports_genres: false,
            ..Default::default()
        });
        let pipeline = AutoFillPipeline {
            filter: FilterStage {
                include_genres: vec!["kids".into()],
                ..Default::default()
            },
            sources: vec![SourceEntry::new(SourceKind::Library)],
            budget: BudgetStage {
                max_bytes: Some(100_000_000),
                ..Default::default()
            },
            ..Default::default()
        };
        let result = expand_with_pipeline(provider, &pipeline, params(100_000_000))
            .await
            .unwrap();
        assert_eq!(
            ids(&result),
            vec!["t1", "t2"],
            "genre filter dropped → all library tracks"
        );
    }

    #[tokio::test]
    async fn genre_filter_dropped_when_runtime_lookup_is_unsupported() {
        // Provider advertises Genres but the method returns UnsupportedCapability at runtime; this
        // must use the same pass-through fallback as a missing advertised capability.
        let provider = arc(MockProvider {
            library: vec![song("t1", 1_000_000), song("t2", 1_000_000)],
            supports_genres: true,
            genre_unsupported_at_runtime: true,
            ..Default::default()
        });
        let pipeline = AutoFillPipeline {
            filter: FilterStage {
                include_genres: vec!["kids".into()],
                ..Default::default()
            },
            sources: vec![SourceEntry::new(SourceKind::Library)],
            budget: BudgetStage {
                max_bytes: Some(100_000_000),
                ..Default::default()
            },
            ..Default::default()
        };
        let result = expand_with_pipeline(provider, &pipeline, params(100_000_000))
            .await
            .unwrap();
        assert_eq!(
            ids(&result),
            vec!["t1", "t2"],
            "runtime unsupported → pass-through"
        );
    }

    #[tokio::test]
    async fn genre_filter_dropped_when_membership_lookup_errors() {
        // Partial/incomplete genre membership would make include filters silently empty the fill.
        // Treat lookup failures as pass-through for the slot.
        let provider = arc(MockProvider {
            library: vec![song("t1", 1_000_000), song("t2", 1_000_000)],
            supports_genres: true,
            genre_error_at_runtime: true,
            ..Default::default()
        });
        let pipeline = AutoFillPipeline {
            filter: FilterStage {
                include_genres: vec!["kids".into()],
                ..Default::default()
            },
            sources: vec![SourceEntry::new(SourceKind::Library)],
            budget: BudgetStage {
                max_bytes: Some(100_000_000),
                ..Default::default()
            },
            ..Default::default()
        };
        let result = expand_with_pipeline(provider, &pipeline, params(100_000_000))
            .await
            .unwrap();
        assert_eq!(
            ids(&result),
            vec!["t1", "t2"],
            "genre lookup error → pass-through"
        );
    }

    #[tokio::test]
    async fn genre_membership_is_paginated_beyond_first_page() {
        let library: Vec<Song> = (0..2001).map(|i| song(&format!("t{i}"), 1)).collect();
        let provider = arc(MockProvider {
            library: library.clone(),
            supports_genres: true,
            genre_songs: HashMap::from([("deep".to_string(), library)]),
            ..Default::default()
        });
        let pipeline = AutoFillPipeline {
            filter: FilterStage {
                include_genres: vec!["deep".into()],
                ..Default::default()
            },
            sources: vec![SourceEntry::new(SourceKind::Library)],
            budget: BudgetStage {
                max_bytes: Some(10_000),
                ..Default::default()
            },
            ..Default::default()
        };
        let result = expand_with_pipeline(provider, &pipeline, params(10_000))
            .await
            .unwrap();
        assert_eq!(result.len(), 2001);
        assert_eq!(result.last().map(|i| i.id.as_str()), Some("t2000"));
    }

    #[tokio::test]
    async fn tag_constraints_are_dropped() {
        let provider = arc(MockProvider {
            library: vec![song("t1", 1_000_000), song("t2", 1_000_000)],
            ..Default::default()
        });
        let pipeline = AutoFillPipeline {
            filter: FilterStage {
                include_tags: vec!["chill".into()],
                exclude_tags: vec!["explicit".into()],
                ..Default::default()
            },
            sources: vec![SourceEntry::new(SourceKind::Library)],
            budget: BudgetStage {
                max_bytes: Some(100_000_000),
                ..Default::default()
            },
            ..Default::default()
        };
        let result = expand_with_pipeline(provider, &pipeline, params(100_000_000))
            .await
            .unwrap();
        // No candidate carries tags; an enforced include_tags would empty the fill. Dropped → kept.
        assert_eq!(
            ids(&result),
            vec!["t1", "t2"],
            "tag constraints dropped → all tracks kept"
        );
    }

    #[tokio::test]
    async fn two_shared_sources_blend_by_share() {
        let favorites: Vec<Song> = (0..8).map(|i| song(&format!("f{i}"), 1_000_000)).collect();
        let library: Vec<Song> = (0..8).map(|i| song(&format!("l{i}"), 1_000_000)).collect();
        let provider = arc(MockProvider {
            library,
            favorites,
            ..Default::default()
        });
        let pipeline = AutoFillPipeline {
            sources: vec![
                SourceEntry {
                    kind: SourceKind::Favorites,
                    ref_id: None,
                    share: Some(0.5),
                },
                SourceEntry {
                    kind: SourceKind::Library,
                    ref_id: None,
                    share: Some(0.5),
                },
            ],
            budget: BudgetStage {
                max_bytes: Some(10_000_000),
                ..Default::default()
            },
            ..Default::default()
        };
        let result = expand_with_pipeline(provider, &pipeline, params(10_000_000))
            .await
            .unwrap();
        let fav = result.iter().filter(|i| i.id.starts_with('f')).count();
        let lib = result.iter().filter(|i| i.id.starts_with('l')).count();
        assert_eq!(fav, 5, "favorites capped at its 50% share");
        assert_eq!(lib, 5, "library capped at its 50% share");
    }

    #[tokio::test]
    async fn budget_capped_by_slot_remaining() {
        // Pipeline configures a huge budget, but the slot's max_fill_bytes is the real ceiling.
        let provider = arc(MockProvider {
            library: (0..10).map(|i| song(&format!("l{i}"), 1_000_000)).collect(),
            ..Default::default()
        });
        let pipeline = AutoFillPipeline {
            sources: vec![SourceEntry {
                kind: SourceKind::Library,
                ref_id: None,
                share: Some(1.0),
            }],
            budget: BudgetStage {
                max_bytes: Some(1_000_000_000),
                ..Default::default()
            },
            ..Default::default()
        };
        // Slot only allows 3 MB → at most 3 tracks.
        let result = expand_with_pipeline(provider, &pipeline, params(3_000_000))
            .await
            .unwrap();
        assert_eq!(result.len(), 3, "slot ceiling caps the configured budget");
    }

    #[tokio::test]
    async fn headroom_and_duration_budget_fields_are_inert_until_12_5() {
        let provider = arc(MockProvider {
            library: vec![
                song("t1", 1_000_000),
                song("t2", 1_000_000),
                song("t3", 1_000_000),
            ],
            ..Default::default()
        });
        let pipeline = AutoFillPipeline {
            sources: vec![SourceEntry::new(SourceKind::Library)],
            budget: BudgetStage {
                max_bytes: Some(5_000_000),
                target_duration_secs: Some(100),
                headroom_bytes: Some(4_000_000),
            },
            ..Default::default()
        };
        let result = expand_with_pipeline(provider, &pipeline, params(5_000_000))
            .await
            .unwrap();
        assert_eq!(
            ids(&result),
            vec!["t1", "t2", "t3"],
            "12.4 honors max_bytes only; duration/headroom are 12.5 concerns"
        );
    }

    #[tokio::test]
    async fn empty_history_makes_cooldown_inert() {
        // Memory cooldown/played-exclusion are configured, but the empty HistorySnapshot means
        // nothing is excluded — all library tracks survive.
        let provider = arc(MockProvider {
            library: vec![song("t1", 1_000_000), song("t2", 1_000_000)],
            ..Default::default()
        });
        let pipeline = AutoFillPipeline {
            sources: vec![SourceEntry::new(SourceKind::Library)],
            memory: MemoryStage {
                cooldown_weeks: Some(52),
                played_exclusion: true,
                ..Default::default()
            },
            budget: BudgetStage {
                max_bytes: Some(100_000_000),
                ..Default::default()
            },
            ..Default::default()
        };
        let result = expand_with_pipeline(provider, &pipeline, params(100_000_000))
            .await
            .unwrap();
        assert_eq!(
            ids(&result),
            vec!["t1", "t2"],
            "empty history → cooldown/played config inert"
        );
    }

    #[tokio::test]
    async fn unsupported_source_contributes_empty_pool_without_aborting() {
        // History source is unsupported by the provider → empty pool, but the playlist source
        // still fills the slot (best-effort, no abort).
        let provider = arc(MockProvider {
            playlists: HashMap::from([("energy".to_string(), vec![song("e1", 1_000_000)])]),
            unsupported_recently_played: true,
            ..Default::default()
        });
        let pipeline = AutoFillPipeline {
            sources: vec![
                SourceEntry::new(SourceKind::History), // unsupported → empty pool
                SourceEntry {
                    kind: SourceKind::Playlist,
                    ref_id: Some("energy".into()),
                    share: None,
                },
            ],
            budget: BudgetStage {
                max_bytes: Some(100_000_000),
                ..Default::default()
            },
            ..Default::default()
        };
        let result = expand_with_pipeline(provider, &pipeline, params(100_000_000))
            .await
            .unwrap();
        assert_eq!(
            ids(&result),
            vec!["e1"],
            "playlist still fills despite unsupported history"
        );
    }
}
