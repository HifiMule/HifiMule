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
    AutoFillPipeline, Candidate, FilterStage, MemoryStage, OrderingKey, PipelineInput, QualityStage,
    SourceEntry, SourceKey, SourceKind, Unit, run_pipeline,
};
use super::{AutoFillItem, AutoFillParams};
use crate::domain::models::Song;
use crate::providers::{BrowseMode, MediaProvider, ProviderError};

/// One rotation-tier definition parsed from `MemoryStage::tiers` (Story 13.1, #25/#26). Conservative
/// shape: a playlist-backed tier (`{ "kind": "playlist", "ref": "<id>" }`) or the whole library
/// (`{ "kind": "library" }`). Malformed input parses to no tiers (rotation disabled).
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum TierDef {
    Playlist {
        #[serde(rename = "ref")]
        ref_id: String,
    },
    Library,
}

impl TierDef {
    fn source_key(&self) -> SourceKey {
        match self {
            TierDef::Playlist { ref_id } => {
                SourceKey::new(SourceKind::Playlist, Some(ref_id.clone()))
            }
            TierDef::Library => SourceKey::new(SourceKind::Library, None),
        }
    }

    fn source_entry(&self, share: Option<f32>) -> SourceEntry {
        match self {
            TierDef::Playlist { ref_id } => SourceEntry {
                kind: SourceKind::Playlist,
                ref_id: Some(ref_id.clone()),
                share,
            },
            TierDef::Library => SourceEntry {
                kind: SourceKind::Library,
                ref_id: None,
                share,
            },
        }
    }
}

/// Parse `MemoryStage::tiers` into an ordered list of [`TierDef`]. `None`/empty/malformed → no tiers
/// (a log on malformed), so rotation never aborts the slot and an unset value is today's behavior.
fn parse_tiers(tiers: &Option<serde_json::Value>) -> Vec<TierDef> {
    let Some(value) = tiers else {
        return Vec::new();
    };
    if value.is_null() {
        return Vec::new();
    }
    match serde_json::from_value::<Vec<TierDef>>(value.clone()) {
        Ok(defs) => {
            // Drop duplicate tiers (same kind+ref): they collapse to one materialized pool, so a
            // second copy would claim a budget share it can never fill (the pool is already `seen`).
            let mut seen: HashSet<SourceKey> = HashSet::new();
            defs.into_iter().filter(|d| seen.insert(d.source_key())).collect()
        }
        Err(e) => {
            crate::daemon_log!("[AutoFill] malformed memory.tiers ({}) — ignoring (no rotation)", e);
            Vec::new()
        }
    }
}

/// Whether a pipeline's Memory stage configures *usable* rotation tiers — i.e. `parse_tiers` yields
/// at least one tier. The sync-completion cursor-advance check must use this (not a bare
/// `tiers.is_some()`) so a malformed/empty `tiers` value that produced no rotation does not still
/// advance the rotation cursor.
pub fn pipeline_uses_tiers(p: &AutoFillPipeline) -> bool {
    !parse_tiers(&p.memory.tiers).is_empty()
}

/// Derive a present-or-absent `last_played_at` for a candidate song (AC 5). A track counts as
/// played when the **media server** reports `play_count > 0` or any `last_played_at`. The pure
/// engine's played-exclusion only checks `.is_some()`, so the concrete value is never compared — we
/// use `now` as a present-but-unknown sentinel rather than parsing the provider ISO timestamp.
fn derive_last_played(song: &Song, now: i64) -> Option<i64> {
    let played = song.play_count.unwrap_or(0) > 0 || song.last_played_at.is_some();
    played.then_some(now)
}

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
/// quality ordering, a memory modifier, a fallback chain, a best-version/version-preference quality
/// setting) returns `true`. `enabled` is a caller
/// concern and not part of the discriminator. For the budget (Story 12.5): a headroom reserve or a
/// duration target forces the configurable path, because the fast `run_auto_fill_provider` path
/// only knows `max_fill_bytes` — it can neither reserve device headroom nor count playtime. A bare
/// `maxBytes` budget keeps the fast path (it honors `max_fill_bytes` directly, so no materialization
/// is needed).
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
    // Story 13.2: a quality-only pipeline (default ordering, but `best_version`/`version_preference`
    // set) must still route to the engine path. The `OrderingKey::Quality` case is already caught by
    // `ordering_default` (it makes the ordering non-legacy). `QualityStage::default()` is today's
    // behavior, so it keeps the fast path.
    let quality_default = p.quality == QualityStage::default();
    // A headroom reserve or duration target can only be honored by the materialized engine path;
    // `max_bytes` alone is honored by the fast path's `max_fill_bytes` and stays default-legacy.
    let budget_default = !(p.budget.headroom_bytes.is_some_and(|h| h > 0)
        || p.budget.target_duration_secs.is_some_and(|t| t > 0));

    !(sources_default
        && filter_default
        && ordering_default
        && unit_default
        && memory_default
        && fallback_default
        && budget_default
        && quality_default)
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
    crate::daemon_log!(
        "[AutoFill] expanding slot device={} server={} (history entries={}, cursor={})",
        params.device_id,
        params.server_id,
        params.history.entries.len(),
        params.rotation_cursor
    );
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

    // --- Budget reconciliation (Story 12.5, AC 1/2/5/6). Two distinct byte numbers meet here:
    //   * `capacity` = the slot's available device bytes (`params.max_fill_bytes`, manual items
    //     already subtracted in rpc.rs);
    //   * `budget.max_bytes` = the user's optional configured cap ("never fill past 8 GB").
    // FR52's headroom reserve subtracts from *device capacity*, not from the configured cap, so the
    // effective ceiling is `min(config.max_bytes.unwrap_or(capacity), capacity - headroom)`. Bake
    // that ceiling into `max_bytes` and zero `headroom_bytes` so the pure engine's `budget_ceiling`
    // (which subtracts headroom from max_bytes) does not double-subtract. `target_duration_secs`
    // stays live for the engine to enforce via real accumulated playtime.
    let capacity = params.max_fill_bytes;
    let headroom = normalized.budget.headroom_bytes.unwrap_or(0);
    let cap_after_reserve = capacity.saturating_sub(headroom);
    let ceiling = normalized
        .budget
        .max_bytes
        .map(|m| m.min(cap_after_reserve))
        .unwrap_or(cap_after_reserve);
    normalized.budget.max_bytes = Some(ceiling);
    normalized.budget.headroom_bytes = None;
    // A zero duration target is inert (matching `needs_configurable_expansion`'s `> 0` guard). If a
    // pipeline reaches this seam for some other reason (a non-default source/filter/fallback) while
    // carrying `target_duration_secs = Some(0)`, leaving it live would make the engine break on the
    // first unit (`cum_secs 0 >= target 0`) and silently empty the fill. Normalize it to `None`.
    if normalized.budget.target_duration_secs == Some(0) {
        normalized.budget.target_duration_secs = None;
    }

    // --- Rotation tiers (#25/#26, AC 8): when `memory.tiers` is configured, the (rotated) tier list
    // defines this run's sources. The cursor (caller-supplied, machine-local) rotates the list so the
    // lead tier shifts each sync; the lead tier gets the dominant budget share (50%), the rest split
    // the remaining 50% equally. `tier_defs`/`lead` are kept to map each emitted track to its
    // *original* tier index after the engine runs. Unset/empty/malformed tiers → today's behavior.
    let tier_defs = parse_tiers(&normalized.memory.tiers);
    let tier_lead = if tier_defs.is_empty() {
        0
    } else {
        let n = tier_defs.len();
        let lead = params.rotation_cursor.rem_euclid(n as i64) as usize;
        let rest_share = if n > 1 { 0.5 / (n as f32 - 1.0) } else { 0.0 };
        let mut sources = Vec::with_capacity(n);
        let mut fallback = Vec::with_capacity(n);
        for rot in 0..n {
            let orig = (lead + rot) % n;
            let share = if n == 1 {
                None
            } else if rot == 0 {
                Some(0.5)
            } else {
                Some(rest_share)
            };
            sources.push(tier_defs[orig].source_entry(share));
            // Spillover: when a tier's pool is smaller than its budget share (e.g. a short lead
            // playlist that can't fill its 50%), the leftover budget would otherwise go unused. Mirror
            // the tiers (uncapped, same rotation order) into the terminal fallback chain so the engine
            // back-fills the remainder from the other tiers after the share-capped primary passes.
            fallback.push(tier_defs[orig].source_entry(None));
        }
        normalized.sources = sources;
        normalized.fallback = fallback;
        lead
    };

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

    // --- Map each candidate to its original tier index (AC 8). First tier (in rotation order) that
    // contains a song wins, so the recorded index is stable regardless of which tier currently leads.
    let mut tier_of: HashMap<String, usize> = HashMap::new();
    if !tier_defs.is_empty() {
        let n = tier_defs.len();
        for rot in 0..n {
            let orig = (tier_lead + rot) % n;
            if let Some(cands) = pools.get(&tier_defs[orig].source_key()) {
                for c in cands {
                    tier_of.entry(c.song.id.clone()).or_insert(orig);
                }
            }
        }
    }

    // --- History (AC 3/4/5): start from the DB-sourced snapshot (last_synced_at/tier per track),
    // override `now` with the caller's value, then merge per-candidate `last_played_at` derived from
    // the materialized provider songs. The DB read already happened in the RPC layer (best-effort:
    // an empty snapshot means memory is inert), keeping this fetch layer DB-free.
    let mut history = params.history;
    history.now = params.now_unix;
    for cands in pools.values() {
        for c in cands {
            if let Some(played) = derive_last_played(&c.song, history.now) {
                history
                    .entries
                    .entry(c.song.id.clone())
                    .or_default()
                    .last_played_at = Some(played);
            }
        }
    }

    let input = PipelineInput {
        pools,
        history,
        exclude_item_ids: params.exclude_item_ids,
    };

    let mut items = run_pipeline(&input, &normalized);
    // Tag each emitted track with its source tier index (string) for sync-completion recording.
    if !tier_of.is_empty() {
        for item in &mut items {
            item.tier = tier_of.get(&item.id).map(|idx| idx.to_string());
        }
    }
    Ok(items)
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
    use crate::auto_fill::pipeline::{BudgetStage, HistorySnapshot, SourceEntry, TrackHistory};
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
            device_id: String::new(),
            server_id: String::new(),
            now_unix: 0,
            history: HistorySnapshot::default(),
            rotation_cursor: 0,
        }
    }

    /// `params` with a pre-built DB history snapshot + rotation cursor (Story 13.1 tests).
    fn params_with(
        max_fill_bytes: u64,
        now_unix: i64,
        history: HistorySnapshot,
        rotation_cursor: i64,
    ) -> AutoFillParams {
        AutoFillParams {
            exclude_item_ids: Vec::new(),
            max_fill_bytes,
            device_id: "dev".to_string(),
            server_id: "srv".to_string(),
            now_unix,
            history,
            rotation_cursor,
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

    #[test]
    fn discriminator_new_memory_fields_force_configurable() {
        // Story 13.1 (AC 9): a pipeline whose only deviation is a new Memory field must route through
        // the materialized engine path. This already holds via the `memory == MemoryStage::default()`
        // check; this test locks it in for stableCorePct/repeatTolerance/tiers.
        let mut p = AutoFillPipeline::default_legacy(Some(1));
        p.memory.stable_core_pct = Some(0.5);
        assert!(needs_configurable_expansion(&p), "stableCorePct forces configurable");

        let mut p = AutoFillPipeline::default_legacy(Some(1));
        p.memory.repeat_tolerance = Some(0.5);
        assert!(needs_configurable_expansion(&p), "repeatTolerance forces configurable");

        let mut p = AutoFillPipeline::default_legacy(Some(1));
        p.memory.tiers = Some(serde_json::json!([{ "kind": "library" }]));
        assert!(needs_configurable_expansion(&p), "tiers forces configurable");
    }

    #[test]
    fn discriminator_new_quality_fields_force_configurable() {
        use crate::auto_fill::pipeline::VersionTrait;

        // Story 13.2 (AC 8): a quality-only pipeline must route to the engine path.
        // (1) best_version-only (default ordering otherwise).
        let mut p = AutoFillPipeline::default_legacy(Some(1));
        p.quality.best_version = true;
        assert!(needs_configurable_expansion(&p), "best_version forces configurable");

        // (2) version_preference-only.
        let mut p = AutoFillPipeline::default_legacy(Some(1));
        p.quality.version_preference = vec![VersionTrait::Live];
        assert!(needs_configurable_expansion(&p), "version_preference forces configurable");

        // (3) Quality-ordering-only is already caught by the ordering discriminator.
        let mut p = AutoFillPipeline::default_legacy(Some(1));
        p.ordering = vec![OrderingKey::Quality];
        assert!(needs_configurable_expansion(&p), "quality ordering key forces configurable");

        // A default QualityStage keeps the fast path (legacy pipeline stays default-equivalent).
        let p = AutoFillPipeline::default_legacy(Some(1));
        assert!(!needs_configurable_expansion(&p), "default quality stage stays on the fast path");
    }

    #[test]
    fn discriminator_new_ordering_keys_force_configurable() {
        // Story 13.3 (AC 7): a pipeline whose only non-default aspect is the new Excavation or
        // Rediscovery ordering key must route to the materialized engine path. This already holds
        // via the `ordering_default` check (any non-legacy ordering trips it); locked in here.
        let mut p = AutoFillPipeline::default_legacy(Some(1));
        p.ordering = vec![OrderingKey::Excavation];
        assert!(needs_configurable_expansion(&p), "excavation ordering key forces configurable");

        let mut p = AutoFillPipeline::default_legacy(Some(1));
        p.ordering = vec![OrderingKey::Rediscovery];
        assert!(needs_configurable_expansion(&p), "rediscovery ordering key forces configurable");

        // The legacy default ordering still takes the fast path (no behavior change).
        let p = AutoFillPipeline::default_legacy(Some(1));
        assert!(!needs_configurable_expansion(&p), "legacy default ordering stays on the fast path");
    }

    #[test]
    fn discriminator_budget_headroom_and_duration_force_configurable() {
        // Story 12.5: a headroom reserve or duration target forces the configurable path; a bare
        // maxBytes (or all-None) budget stays on the fast path.
        let mut p = AutoFillPipeline::default_legacy(None);
        p.budget.headroom_bytes = Some(1_000_000);
        assert!(
            needs_configurable_expansion(&p),
            "headroom reserve forces materialization"
        );

        let mut p = AutoFillPipeline::default_legacy(None);
        p.budget.target_duration_secs = Some(3600);
        assert!(
            needs_configurable_expansion(&p),
            "duration target forces materialization"
        );

        // maxBytes-only stays on the fast path (honored by max_fill_bytes).
        let mut p = AutoFillPipeline::default_legacy(None);
        p.budget.max_bytes = Some(8_000_000_000);
        assert!(
            !needs_configurable_expansion(&p),
            "a bare maxBytes budget keeps the fast path"
        );

        // A zero headroom/duration is treated as no refinement.
        let mut p = AutoFillPipeline::default_legacy(None);
        p.budget.headroom_bytes = Some(0);
        p.budget.target_duration_secs = Some(0);
        assert!(
            !needs_configurable_expansion(&p),
            "zero headroom/duration is inert and stays on the fast path"
        );

        // All-None budget on an otherwise-default pipeline stays on the fast path.
        assert!(!needs_configurable_expansion(&AutoFillPipeline::default()));
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

    // -------------------------------------------------------------------
    // Story 12.5: headroom reserve, live duration target, fallback-reaches-target through the
    // async materialization seam. These replace the 12.4 inert guard test.
    // -------------------------------------------------------------------

    #[tokio::test]
    async fn headroom_reserve_subtracts_from_device_capacity() {
        // Capacity C = 10 units, reserve R = 1 unit, no configured max_bytes.
        // Effective ceiling = C − R = 9 units, so the fill never exceeds 9 of the 10 songs.
        let provider = arc(MockProvider {
            library: (0..10).map(|i| song(&format!("l{i}"), 1_000_000)).collect(),
            ..Default::default()
        });
        let pipeline = AutoFillPipeline {
            sources: vec![SourceEntry::new(SourceKind::Library)],
            budget: BudgetStage {
                headroom_bytes: Some(1_000_000),
                ..Default::default()
            },
            ..Default::default()
        };
        let result = expand_with_pipeline(provider, &pipeline, params(10_000_000))
            .await
            .unwrap();
        assert_eq!(
            result.len(),
            9,
            "fill ≤ capacity − reserve (10 − 1 = 9 units)"
        );
        assert_eq!(
            ids(&result),
            (0..9).map(|i| format!("l{i}")).collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn config_max_bytes_and_headroom_reconcile() {
        // The reserve subtracts from capacity, NOT from the configured max_bytes.
        // Case A: max_bytes = 8, C = 10, R = 1 → min(8, 10 − 1) = 8 (not 7).
        let provider = arc(MockProvider {
            library: (0..10).map(|i| song(&format!("l{i}"), 1_000_000)).collect(),
            ..Default::default()
        });
        let pipeline = AutoFillPipeline {
            sources: vec![SourceEntry::new(SourceKind::Library)],
            budget: BudgetStage {
                max_bytes: Some(8_000_000),
                headroom_bytes: Some(1_000_000),
                ..Default::default()
            },
            ..Default::default()
        };
        let result = expand_with_pipeline(provider, &pipeline, params(10_000_000))
            .await
            .unwrap();
        assert_eq!(result.len(), 8, "ceiling = min(8, 10 − 1) = 8 units");

        // Case B: max_bytes = 8, C = 5, R = 1 → min(8, 5 − 1) = 4.
        let provider = arc(MockProvider {
            library: (0..10).map(|i| song(&format!("l{i}"), 1_000_000)).collect(),
            ..Default::default()
        });
        let result = expand_with_pipeline(provider, &pipeline, params(5_000_000))
            .await
            .unwrap();
        assert_eq!(result.len(), 4, "ceiling = min(8, 5 − 1) = 4 units");
    }

    #[tokio::test]
    async fn duration_target_live_through_async_path() {
        // Each fixture track is 180s. With a 400s target, two tracks (360s) fit; a third (540s)
        // would overshoot, so the fill stops at two. Byte ceiling is generous so duration binds.
        let provider = arc(MockProvider {
            library: (0..5).map(|i| song(&format!("l{i}"), 1_000_000)).collect(),
            ..Default::default()
        });
        let pipeline = AutoFillPipeline {
            sources: vec![SourceEntry::new(SourceKind::Library)],
            budget: BudgetStage {
                target_duration_secs: Some(400),
                ..Default::default()
            },
            ..Default::default()
        };
        let result = expand_with_pipeline(provider, &pipeline, params(1_000_000_000))
            .await
            .unwrap();
        assert_eq!(
            result.len(),
            2,
            "real accumulated playtime stops the fill before overshooting the duration target"
        );
    }

    #[tokio::test]
    async fn zero_duration_target_is_inert_not_empty_fill() {
        // A `target_duration_secs = Some(0)` is inert (the discriminator treats it as no refinement).
        // When a pipeline reaches this seam for another reason — here a non-default Playlist source —
        // the zero target must be normalized away, NOT left live (which would make the engine break
        // on the first unit at `cum_secs 0 >= target 0` and silently empty the fill).
        let mut playlists = HashMap::new();
        playlists.insert(
            "p1".to_string(),
            (0..3).map(|i| song(&format!("p{i}"), 1_000_000)).collect(),
        );
        let provider = arc(MockProvider {
            playlists,
            ..Default::default()
        });
        let pipeline = AutoFillPipeline {
            sources: vec![SourceEntry {
                kind: SourceKind::Playlist,
                ref_id: Some("p1".into()),
                share: None,
            }],
            budget: BudgetStage {
                target_duration_secs: Some(0),
                ..Default::default()
            },
            ..Default::default()
        };
        let result = expand_with_pipeline(provider, &pipeline, params(10_000_000))
            .await
            .unwrap();
        assert_eq!(
            result.len(),
            3,
            "zero duration target is inert — the playlist fills normally, not empty"
        );
    }

    #[tokio::test]
    async fn fallback_reaches_target_through_async_path() {
        // Primary playlist holds a single small track — too little to fill the budget. A Library
        // fallback pool must be materialized (provider stub serves it) and drawn to reach the
        // byte ceiling. Capacity 5 units → 5 songs total = 1 playlist + 4 fallback library.
        let mut playlists = HashMap::new();
        playlists.insert("p1".to_string(), vec![song("p0", 1_000_000)]);
        let provider = arc(MockProvider {
            library: (0..6).map(|i| song(&format!("l{i}"), 1_000_000)).collect(),
            playlists,
            ..Default::default()
        });
        let pipeline = AutoFillPipeline {
            sources: vec![SourceEntry {
                kind: SourceKind::Playlist,
                ref_id: Some("p1".into()),
                share: None,
            }],
            fallback: vec![SourceEntry::new(SourceKind::Library)],
            ..Default::default()
        };
        let result = expand_with_pipeline(provider, &pipeline, params(5_000_000))
            .await
            .unwrap();
        assert_eq!(result.len(), 5, "fallback fills the budget after the primary runs dry");
        assert!(
            result.iter().any(|i| i.id == "p0"),
            "the primary playlist track is included"
        );
        assert_eq!(
            result.iter().filter(|i| i.id.starts_with('l')).count(),
            4,
            "the fallback Library pool was materialized and drawn to reach the ceiling"
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

    // -------------------------------------------------------------------
    // Story 13.1: cooldown/played-exclusion with populated history, and rotation tiers.
    // -------------------------------------------------------------------

    fn played_song(id: &str, size_bytes: u64) -> Song {
        Song {
            play_count: Some(5),
            ..song(id, size_bytes)
        }
    }

    #[tokio::test]
    async fn cooldown_excludes_recently_synced_with_populated_history() {
        let now = 1_000_000_000i64;
        let week = 7 * 86_400i64;
        let provider = arc(MockProvider {
            library: vec![song("recent", 1_000_000), song("old", 1_000_000)],
            ..Default::default()
        });
        let mut history = HistorySnapshot {
            now,
            ..Default::default()
        };
        history.entries.insert(
            "recent".to_string(),
            TrackHistory {
                last_synced_at: Some(now - week), // 1 week ago — inside the 2-week cooldown
                ..Default::default()
            },
        );
        history.entries.insert(
            "old".to_string(),
            TrackHistory {
                last_synced_at: Some(now - 100 * week), // long ago — eligible
                ..Default::default()
            },
        );
        let pipeline = AutoFillPipeline {
            sources: vec![SourceEntry::new(SourceKind::Library)],
            memory: MemoryStage {
                cooldown_weeks: Some(2),
                ..Default::default()
            },
            budget: BudgetStage {
                max_bytes: Some(100_000_000),
                ..Default::default()
            },
            ..Default::default()
        };
        let result = expand_with_pipeline(provider, &pipeline, params_with(100_000_000, now, history, 0))
            .await
            .unwrap();
        assert_eq!(
            ids(&result),
            vec!["old"],
            "recently-synced track excluded; old one survives"
        );
    }

    #[tokio::test]
    async fn played_exclusion_drops_server_played_tracks() {
        // play_count > 0 on the provider Song → excluded; no autofill_history row needed.
        let provider = arc(MockProvider {
            library: vec![played_song("played", 1_000_000), song("fresh", 1_000_000)],
            ..Default::default()
        });
        let pipeline = AutoFillPipeline {
            sources: vec![SourceEntry::new(SourceKind::Library)],
            memory: MemoryStage {
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
        assert_eq!(ids(&result), vec!["fresh"], "server-played track excluded");
    }

    #[tokio::test]
    async fn rotation_tiers_shift_lead_and_tag_tier_index() {
        // 3 playlist tiers (A/B/C), 4 × 1 MB tracks each. Lead tier gets 50% of the 4 MB ceiling
        // (2 tracks); the other two split the rest (1 track each). Advancing the cursor shifts which
        // tier dominates. Emitted tracks carry their ORIGINAL tier index (stable across rotation).
        let mk = |p: &str| (0..4).map(|i| song(&format!("{p}{i}"), 1_000_000)).collect::<Vec<_>>();
        let playlists = HashMap::from([
            ("A".to_string(), mk("a")),
            ("B".to_string(), mk("b")),
            ("C".to_string(), mk("c")),
        ]);
        let tiers = serde_json::json!([
            { "kind": "playlist", "ref": "A" },
            { "kind": "playlist", "ref": "B" },
            { "kind": "playlist", "ref": "C" },
        ]);
        let pipeline = AutoFillPipeline {
            memory: MemoryStage {
                tiers: Some(tiers),
                ..Default::default()
            },
            budget: BudgetStage {
                max_bytes: Some(4_000_000),
                ..Default::default()
            },
            ..Default::default()
        };

        // cursor 0 → tier A leads (2 tracks); B, C contribute 1 each.
        let provider = arc(MockProvider {
            playlists: playlists.clone(),
            ..Default::default()
        });
        let r0 = expand_with_pipeline(provider, &pipeline, params_with(4_000_000, 0, HistorySnapshot::default(), 0))
            .await
            .unwrap();
        let a0 = r0.iter().filter(|i| i.id.starts_with('a')).count();
        assert_eq!(a0, 2, "cursor 0 → tier A dominates");
        // Each emitted track is tagged with its original tier index (a→0, b→1, c→2).
        for item in &r0 {
            let expected = match item.id.chars().next().unwrap() {
                'a' => "0",
                'b' => "1",
                _ => "2",
            };
            assert_eq!(item.tier.as_deref(), Some(expected), "tier index tagged");
        }

        // cursor 1 → tier B leads (2 tracks).
        let provider = arc(MockProvider {
            playlists,
            ..Default::default()
        });
        let r1 = expand_with_pipeline(provider, &pipeline, params_with(4_000_000, 0, HistorySnapshot::default(), 1))
            .await
            .unwrap();
        let b1 = r1.iter().filter(|i| i.id.starts_with('b')).count();
        assert_eq!(b1, 2, "cursor 1 → lead shifts to tier B");
    }

    #[tokio::test]
    async fn malformed_tiers_disable_rotation_without_aborting() {
        // A malformed tiers value must not abort the slot — the configured library source still fills.
        let provider = arc(MockProvider {
            library: vec![song("l0", 1_000_000), song("l1", 1_000_000)],
            ..Default::default()
        });
        let pipeline = AutoFillPipeline {
            sources: vec![SourceEntry::new(SourceKind::Library)],
            memory: MemoryStage {
                tiers: Some(serde_json::json!({ "not": "an array" })),
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
        assert_eq!(ids(&result), vec!["l0", "l1"], "malformed tiers ignored, normal fill");
        assert!(result.iter().all(|i| i.tier.is_none()), "no tier tags without rotation");
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
