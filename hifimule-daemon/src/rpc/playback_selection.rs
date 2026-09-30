//! Playback selection RPCs. This state is local and independent of device auto-fill.
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    AppState, ERR_INVALID_PARAMS, ERR_STORAGE_ERROR, JsonRpcError,
    handle_playback_apply_session_prepared_fenced,
};
use crate::playback::{
    model::{ApplySessionParams, SessionOperation},
    selection::{self, PlaybackSelectionConfig, SelectionError, SelectionKind, SelectionPool},
};

pub(crate) static START_GATE: std::sync::Mutex<()> = std::sync::Mutex::new(());
pub(crate) static START_EPOCH: AtomicU64 = AtomicU64::new(0);
const DEADLINE: Duration = Duration::from_secs(15);

/// A later accepted session command wins over any selection still fetching or
/// preparing. The same gate orders invalidation against the owner's final
/// StartRadio commit. Repeated starts supersede instead of coalescing.
pub(crate) fn supersede_pending_start() {
    let _guard = START_GATE.lock().unwrap_or_else(|error| error.into_inner());
    START_EPOCH.fetch_add(1, Ordering::AcqRel);
}

pub(crate) fn begin_start() -> u64 {
    let _guard = START_GATE.lock().unwrap_or_else(|error| error.into_inner());
    START_EPOCH.fetch_add(1, Ordering::AcqRel) + 1
}

#[cfg(test)]
mod ordering_tests {
    use super::*;

    #[test]
    fn later_session_command_invalidates_pending_start() {
        let ticket = START_EPOCH.fetch_add(1, Ordering::AcqRel) + 1;
        supersede_pending_start();
        assert_ne!(START_EPOCH.load(Ordering::Acquire), ticket);
    }

    #[test]
    fn radio_start_requires_the_selected_available_output() {
        let mut output = crate::playback::model::OutputState::default();
        assert!(!selected_output_ready(&output));
        let descriptor = crate::playback::devices::OutputDescriptor {
            output_id: "chosen".into(),
            display_name: "Chosen".into(),
            detail: String::new(),
            backend: "test".into(),
            available: false,
            is_default: false,
            identity_confidence: "exact".into(),
            is_virtual: false,
            preference: None,
        };
        output.selected = Some(descriptor.clone());
        assert!(!selected_output_ready(&output));
        output.selected.as_mut().unwrap().available = true;
        assert!(selected_output_ready(&output));
        output.pending = Some(descriptor);
        assert!(!selected_output_ready(&output));
    }
}

fn selected_output_ready(output: &crate::playback::model::OutputState) -> bool {
    output
        .selected
        .as_ref()
        .is_some_and(|selected| selected.available)
        && output.pending.is_none()
}

#[cfg(test)]
pub(super) async fn test_ticket() -> u64 {
    let _guard = START_GATE.lock().unwrap_or_else(|error| error.into_inner());
    START_EPOCH.fetch_add(1, Ordering::AcqRel) + 1
}

fn error(kind: SelectionError) -> JsonRpcError {
    let code = match kind {
        SelectionError::Setup => ERR_INVALID_PARAMS,
        SelectionError::Save => ERR_STORAGE_ERROR,
        SelectionError::SourceUnavailable | SelectionError::PreparationFailed => {
            super::ERR_CONNECTION_FAILED
        }
        SelectionError::Empty => super::ERR_NOT_FOUND,
        SelectionError::Cancelled => super::ERR_SYNC_CANCELLED,
    };
    JsonRpcError {
        code,
        message: kind.to_string(),
        data: Some(json!({"code": kind.to_string()})),
    }
}

fn path() -> Result<std::path::PathBuf, JsonRpcError> {
    Ok(crate::paths::get_app_data_dir()
        .map_err(|_| error(SelectionError::Save))?
        .join("playback-selection.json"))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Version {
    schema_version: u32,
}

fn version(params: Option<Value>) -> Result<(), JsonRpcError> {
    match serde_json::from_value::<Version>(params.unwrap_or(Value::Null)) {
        Ok(Version { schema_version: 1 }) => Ok(()),
        _ => Err(error(SelectionError::Setup)),
    }
}

pub async fn get_config(params: Option<Value>) -> Result<Value, JsonRpcError> {
    version(params)?;
    let path = path()?;
    let config = tokio::task::spawn_blocking(move || selection::load(&path))
        .await
        .map_err(|_| error(SelectionError::Save))?
        .map_err(error)?;
    Ok(json!({"data": config}))
}

async fn provider(
    state: &AppState,
    server_id: &str,
) -> Result<std::sync::Arc<dyn crate::providers::MediaProvider>, JsonRpcError> {
    let exists = state
        .db
        .list_servers()
        .map_err(|_| error(SelectionError::Save))?
        .iter()
        .any(|server| server.server_id.as_deref() == Some(server_id));
    if !exists {
        return Err(error(SelectionError::SourceUnavailable));
    }
    let provider = crate::server_manager::get_provider_by_server_id(
        &state.server_manager,
        &state.db,
        server_id,
    )
    .await
    .map_err(|_| error(SelectionError::SourceUnavailable))?;
    if provider.library_role().is_some() {
        return Err(error(SelectionError::Setup));
    }
    Ok(provider)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OptionsParams {
    schema_version: u32,
    server_id: String,
    kind: SelectionKind,
    #[serde(default)]
    offset: u32,
    #[serde(default)]
    query: String,
    #[serde(default, rename = "ref")]
    ref_id: Option<String>,
}

pub async fn options(state: &AppState, params: Option<Value>) -> Result<Value, JsonRpcError> {
    let args: OptionsParams = serde_json::from_value(params.unwrap_or(Value::Null))
        .map_err(|_| error(SelectionError::Setup))?;
    if args.schema_version != 1
        || args.server_id.is_empty()
        || args.query.len() > 1024
        || args.query.contains('\0')
        || args
            .ref_id
            .as_ref()
            .is_some_and(|id| id.len() > 1024 || id.contains('\0'))
    {
        return Err(error(SelectionError::Setup));
    }
    let provider = provider(state, &args.server_id).await?;
    let required_mode = match args.kind {
        SelectionKind::Playlist => crate::providers::BrowseMode::Playlists,
        SelectionKind::Artist => crate::providers::BrowseMode::Artists,
        SelectionKind::Genre => crate::providers::BrowseMode::Genres,
    };
    if !provider
        .capabilities()
        .browse
        .list_modes
        .contains(&required_mode)
    {
        return Ok(
            json!({"data": {"supported": false, "options": [], "reason": "UNSUPPORTED_CAPABILITY"}}),
        );
    }
    if args.offset % selection::MAX_CANDIDATES_PER_SOURCE as u32 != 0 {
        return Err(error(SelectionError::Setup));
    }
    match tokio::time::timeout(
        DEADLINE,
        selection::search_options_page(provider.as_ref(), args.kind, args.offset, &args.query),
    )
    .await
    {
        Ok(Ok(page)) => {
            let selected = if let Some(ref_id) = args.ref_id.filter(|id| !id.is_empty()) {
                if let Some(option) = page.options.iter().find(|option| option.id == ref_id) {
                    Some(option.clone())
                } else {
                    let source = selection::SelectionSource {
                        server_id: args.server_id,
                        kind: args.kind,
                        ref_id,
                    };
                    tokio::time::timeout(
                        DEADLINE,
                        selection::resolve_option(provider.as_ref(), &source),
                    )
                    .await
                    .ok()
                    .and_then(Result::ok)
                    .flatten()
                }
            } else {
                None
            };
            Ok(
                json!({"data": {"supported": true, "options": page.options, "hasMore": page.has_more, "selected": selected}}),
            )
        }
        _ => Ok(
            json!({"data": {"supported": false, "options": [], "reason": "PLAYBACK_SELECTION_SOURCE_UNAVAILABLE"}}),
        ),
    }
}

pub async fn save_config(state: &AppState, params: Option<Value>) -> Result<Value, JsonRpcError> {
    let config: PlaybackSelectionConfig = serde_json::from_value(params.unwrap_or(Value::Null))
        .map_err(|_| error(SelectionError::Setup))?;
    config.validate().map_err(error)?;
    let validate_sources = async {
        for source in &config.sources {
            let provider = provider(state, &source.server_id).await?;
            if !selection::reference_exists(provider.as_ref(), source)
                .await
                .map_err(error)?
            {
                return Err(error(SelectionError::Setup));
            }
        }
        Ok(())
    };
    tokio::time::timeout(DEADLINE, validate_sources)
        .await
        .map_err(|_| error(SelectionError::SourceUnavailable))??;
    let path = path()?;
    tokio::task::spawn_blocking(move || selection::save(&path, &config))
        .await
        .map_err(|_| error(SelectionError::Save))?
        .map_err(error)?;
    Ok(json!({"data": {"saved": true}}))
}

pub async fn cancel(params: Option<Value>) -> Result<Value, JsonRpcError> {
    version(params)?;
    supersede_pending_start();
    Ok(json!({"data": {"cancelled": true}}))
}

pub async fn start(
    state: &AppState,
    params: Option<Value>,
    mutation_guard: Option<crate::sync::MutationGuard>,
) -> Result<Value, JsonRpcError> {
    version(params)?;
    let ticket = begin_start();
    start_with_ticket(state, ticket, mutation_guard).await
}

pub(crate) async fn start_with_ticket(
    state: &AppState,
    ticket: u64,
    mutation_guard: Option<crate::sync::MutationGuard>,
) -> Result<Value, JsonRpcError> {
    let path = path()?;
    let loaded = tokio::task::spawn_blocking(move || selection::load(&path))
        .await
        .map_err(|_| error(SelectionError::Save))?;
    if START_EPOCH.load(Ordering::Acquire) != ticket {
        return Err(error(SelectionError::Cancelled));
    }
    let config = loaded.map_err(error)?;
    start_with_config(state, config, ticket, mutation_guard).await
}

pub(super) async fn start_with_config(
    state: &AppState,
    config: PlaybackSelectionConfig,
    ticket: u64,
    mutation_guard: Option<crate::sync::MutationGuard>,
) -> Result<Value, JsonRpcError> {
    if START_EPOCH.load(Ordering::Acquire) != ticket {
        return Err(error(SelectionError::Cancelled));
    }
    if config.sources.is_empty() {
        return Err(error(SelectionError::Setup));
    }
    let fetch = async {
        let mut pools: Vec<SelectionPool> = Vec::with_capacity(config.sources.len());
        for source in &config.sources {
            if START_EPOCH.load(Ordering::Acquire) != ticket {
                return Err(error(SelectionError::Cancelled));
            }
            let Ok(provider) = provider(state, &source.server_id).await else {
                continue;
            };
            if let Ok(pool) = selection::fetch_source(provider.as_ref(), source).await {
                pools.push(pool);
            }
        }
        if pools.is_empty() {
            return Err(error(SelectionError::SourceUnavailable));
        }
        Ok(pools)
    };
    let mut pools = tokio::time::timeout(DEADLINE, fetch)
        .await
        .map_err(|_| error(SelectionError::SourceUnavailable))??;
    selection::clear_conflicting_recordings(&mut pools);
    if START_EPOCH.load(Ordering::Acquire) != ticket {
        return Err(error(SelectionError::Cancelled));
    }
    let selected = selection::select_with_recordings(&config, pools.clone()).map_err(error)?;
    // Resolve before touching the owner. A missing or expired source leaves the current
    // listening session intact and generates no skip/taste signal. Try the next
    // bounded selected result if a track vanished after candidate retrieval.
    let preflight = async {
        let mut failures = 0;
        for primary in selected {
            let attempts = std::iter::once(primary.clone())
                .chain(selection::radio_copy_alternates(&config, &pools, &primary));
            for source in attempts {
                if START_EPOCH.load(Ordering::Acquire) != ticket {
                    return Err(error(SelectionError::Cancelled));
                }
                if failures >= crate::playback::radio::MAX_PREPARATION_FAILURES {
                    break;
                }
                if let Ok(provider) = provider(state, &source.server_id).await
                    && let Ok(description) = provider.resolve_playback(&source.track_id).await
                    && let Ok(prepared) =
                        crate::playback::audio::prepare_selection_source(description).await
                {
                    let center = selection::radio_center_for_source(&config, &pools, &source);
                    return Ok((source, center, prepared));
                }
                failures += 1;
            }
        }
        Err(error(SelectionError::PreparationFailed))
    };
    let (first, center, prepared) = tokio::time::timeout(DEADLINE, preflight)
        .await
        .map_err(|_| error(SelectionError::PreparationFailed))??;
    if START_EPOCH.load(Ordering::Acquire) != ticket {
        return Err(error(SelectionError::Cancelled));
    }
    let snapshot = state.playback.snapshot().map_err(super::playback_error)?;
    if !selected_output_ready(&snapshot.output) {
        return Err(JsonRpcError {
            code: super::ERR_INVALID_PARAMS,
            message: "Choose an available audio output before starting Radio".into(),
            data: Some(json!({"code": "OUTPUT_UNAVAILABLE"})),
        });
    }
    let command = ApplySessionParams {
        schema_version: 1,
        instance_id: snapshot.instance_id,
        session_id: snapshot.session_id,
        command_id: uuid::Uuid::new_v4().to_string(),
        expected_queue_revision: snapshot.queue_revision,
        operation: SessionOperation::StartRadio {
            center,
            recording: pools
                .iter()
                .filter(|pool| pool.source.server_id == first.server_id)
                .flat_map(|pool| &pool.tracks)
                .find(|song| song.id == first.track_id)
                .and_then(|song| song.provider_metadata.recording.clone()),
            source: first,
            settings: Some(config),
        },
    };
    handle_playback_apply_session_prepared_fenced(
        state,
        command,
        mutation_guard,
        false,
        Some(prepared),
        Some(crate::playback::session::ApplyFence {
            gate: &START_GATE,
            epoch: &START_EPOCH,
            expected: ticket,
        }),
    )
    .await
}

/// A single listener serializes bounded Radio fetches. Owner wakeups are
/// coalesced; no polling timer or full-library cache is kept.
pub(super) async fn run_radio_worker(
    state: std::sync::Arc<AppState>,
    mut wake: tokio::sync::mpsc::Receiver<()>,
) {
    while wake.recv().await.is_some() {
        let owner = state.playback.clone();
        let lease = match tokio::task::spawn_blocking(move || owner.reserve_radio_refill()).await {
            Ok(Ok(Some(lease))) => lease,
            _ => continue,
        };
        let (candidates, reason, more_windows, transition) =
            gather_radio_candidates(&state, &lease).await;
        let owner = state.playback.clone();
        let _ = tokio::task::spawn_blocking(move || {
            owner.admit_radio_refill_plan(lease, candidates, reason, more_windows, transition)
        })
        .await;
    }
}

async fn prepare_radio_copy(
    state: &AppState,
    lease: &crate::playback::radio::RefillLease,
    source: &crate::playback::model::TrackSource,
) -> bool {
    if !state.playback.radio_lease_current(lease) {
        return false;
    }
    let Ok(source_provider) = provider(state, &source.server_id).await else {
        return false;
    };
    if !state.playback.radio_lease_current(lease) {
        return false;
    }
    let Ok(description) = source_provider.resolve_playback(&source.track_id).await else {
        return false;
    };
    if !state.playback.radio_lease_current(lease) {
        return false;
    }
    let prepared = crate::playback::audio::prepare_selection_source(description)
        .await
        .is_ok();
    prepared && state.playback.radio_lease_current(lease)
}

async fn gather_radio_candidates(
    state: &AppState,
    lease: &crate::playback::radio::RefillLease,
) -> (
    Vec<selection::SelectionCandidate>,
    Option<String>,
    bool,
    Option<crate::playback::radio::RadioTransition>,
) {
    if lease.center.is_none() {
        return gather_transition_candidates(state, lease).await;
    }
    let config = &lease.original_settings;
    let mut failed_source = false;
    let mut more_windows = false;
    let center = lease.center.as_ref().expect("center was checked above");
    let mut anchor_keys = HashSet::new();
    let retrieve = async {
        let mut pools = Vec::new();
        let mut center_pools_cleared = false;
        for source in config
            .sources
            .iter()
            .filter(|source| source.server_id == center.server_id)
            .chain(
                config
                    .sources
                    .iter()
                    .filter(|source| source.server_id != center.server_id),
            )
            .take(selection::MAX_SOURCES)
        {
            if !state.playback.radio_lease_current(lease) {
                break;
            }
            if source.server_id != center.server_id && !center_pools_cleared {
                anchor_keys = selection::recording_keys_after_conflict_clear(&mut pools);
                center_pools_cleared = true;
            }
            let Ok(provider) = provider(state, &source.server_id).await else {
                failed_source = true;
                continue;
            };
            if !state.playback.radio_lease_current(lease) {
                break;
            }
            let source_key = match serde_json::to_string(source) {
                Ok(key) => key,
                Err(_) => {
                    failed_source = true;
                    continue;
                }
            };
            let cursor = match state.db.radio_scan_cursor(&lease.session_id, &source_key) {
                Ok(Some(cursor)) => cursor,
                Ok(None) => continue,
                Err(_) => {
                    failed_source = true;
                    continue;
                }
            };
            match selection::fetch_radio_window(provider.as_ref(), source, cursor).await {
                Ok((mut pool, next)) => {
                    if !state.playback.radio_lease_current(lease) {
                        break;
                    }
                    let mut eligible = Vec::new();
                    for song in pool.tracks.drain(..) {
                        if song.provider_metadata.ambiguous_music_artist {
                            continue;
                        }
                        let same_artist =
                            center.matches(&source.server_id, song.artist_id.as_deref());
                        let key = song
                            .provider_metadata
                            .recording
                            .as_ref()
                            .and_then(|e| e.key())
                            .map(|key| key.as_str().to_owned());
                        if !same_artist && key.as_ref().is_none_or(|key| !anchor_keys.contains(key))
                        {
                            continue;
                        }
                        let identity = crate::playback::model::TrackSource {
                            server_id: source.server_id.clone(),
                            track_id: song.id.clone(),
                        };
                        if state
                            .db
                            .radio_has_membership(&lease.session_id, &identity)
                            .unwrap_or(true)
                            || state
                                .db
                                .radio_has_occurrence(&lease.session_id, &identity)
                                .unwrap_or(true)
                            || song
                                .provider_metadata
                                .recording
                                .as_ref()
                                .and_then(|e| e.key())
                                .is_some_and(|key| {
                                    state
                                        .db
                                        .radio_recording_used(&lease.session_id, key.as_str())
                                        .unwrap_or(true)
                                })
                        {
                            continue;
                        }
                        eligible.push(song);
                    }
                    if eligible.is_empty() {
                        if state.playback.radio_lease_current(lease) {
                            if state
                                .db
                                .advance_radio_scan(
                                    &lease.session_id,
                                    lease.queue_revision,
                                    &source_key,
                                    cursor,
                                    next,
                                )
                                .unwrap_or(false)
                            {
                                more_windows |= next.is_some();
                            } else {
                                failed_source = true;
                            }
                        }
                    } else {
                        more_windows |= next.is_some();
                    }
                    pool.tracks = eligible;
                    pools.push(pool);
                }
                Err(_) => failed_source = true,
            }
        }
        pools
    };
    let mut pools = match tokio::time::timeout(
        Duration::from_secs(crate::playback::radio::RETRIEVAL_DEADLINE_SECS),
        retrieve,
    )
    .await
    {
        Ok(pools) => pools,
        Err(_) => return (Vec::new(), Some("radio.sourceFailure".into()), false, None),
    };
    selection::clear_conflicting_recordings(&mut pools);
    let ordered = match selection::select_radio_order(config, pools.clone()) {
        Ok(ordered) => ordered,
        Err(_) => {
            return if failed_source {
                (Vec::new(), Some("radio.sourceFailure".into()), false, None)
            } else if more_windows {
                (Vec::new(), None, true, None)
            } else {
                gather_transition_candidates(state, lease).await
            };
        }
    };
    let alternates: HashMap<_, _> = ordered
        .iter()
        .map(|source| {
            (
                (source.server_id.clone(), source.track_id.clone()),
                selection::radio_copy_alternates(config, &pools, source),
            )
        })
        .collect();
    let mut evidence = HashMap::new();
    for pool in &pools {
        for song in &pool.tracks {
            evidence
                .entry((pool.source.server_id.clone(), song.id.clone()))
                .or_insert_with(|| song.clone());
        }
    }
    let prepare = async {
        let mut selected = Vec::new();
        let mut failures = 0;
        for primary in ordered {
            if !state.playback.radio_lease_current(lease) {
                break;
            }
            if selected.len() >= lease.capacity
                || failures >= crate::playback::radio::MAX_PREPARATION_FAILURES
            {
                break;
            }
            let attempts = std::iter::once(primary.clone()).chain(
                alternates
                    .get(&(primary.server_id.clone(), primary.track_id.clone()))
                    .into_iter()
                    .flatten()
                    .cloned(),
            );
            for source in attempts {
                if failures >= crate::playback::radio::MAX_PREPARATION_FAILURES
                    || !state.playback.radio_lease_current(lease)
                {
                    break;
                }
                if prepare_radio_copy(state, lease, &source).await {
                    if let Some(song) =
                        evidence.get(&(source.server_id.clone(), source.track_id.clone()))
                    {
                        let center_origin = (!center
                            .matches(&source.server_id, song.artist_id.as_deref()))
                        .then(|| center.clone());
                        selected.push(selection::SelectionCandidate {
                            source,
                            song: song.clone(),
                            center_origin,
                        });
                    }
                    break;
                }
                failures += 1;
            }
        }
        (selected, failures)
    };
    match tokio::time::timeout(
        Duration::from_secs(crate::playback::radio::PREPARATION_DEADLINE_SECS),
        prepare,
    )
    .await
    {
        Ok((selected, failures)) => {
            let continue_scan =
                more_windows || (!selected.is_empty() && selected.len() < lease.capacity);
            let reason = if failed_source || failures > 0 {
                Some("radio.sourceFailure".into())
            } else if selected.len() < lease.capacity && !continue_scan {
                Some("radio.exhausted".into())
            } else {
                None
            };
            (
                selected,
                reason,
                continue_scan && failures == 0 && !failed_source,
                None,
            )
        }
        Err(_) => (Vec::new(), Some("radio.sourceFailure".into()), false, None),
    }
}

/// One bounded pass through the original scope. Related search and fresh-start
/// search have independent durable cursors, so a short page never proves that
/// no eligible artist exists later in the configured scope.
async fn gather_transition_candidates(
    state: &AppState,
    lease: &crate::playback::radio::RefillLease,
) -> (
    Vec<selection::SelectionCandidate>,
    Option<String>,
    bool,
    Option<crate::playback::radio::RadioTransition>,
) {
    use crate::playback::radio::{ArtistIdentity, RadioTransitionKind};
    use crate::providers::ArtistRelationKind;
    let mut ranked = Vec::new();
    let mut relation_unknown = false;
    if let Some(center) = &lease.center {
        if let Ok(center_provider) = provider(state, &center.server_id).await {
            if !state.playback.radio_lease_current(lease) {
                return (Vec::new(), None, false, None);
            }
            match tokio::time::timeout(
                DEADLINE,
                center_provider.related_artists(&center.artist_id, 8),
            )
            .await
            {
                Ok(Ok(relations)) => {
                    for (index, relation) in relations.into_iter().enumerate() {
                        if relation.artist_id.is_empty()
                            || relation.artist_id == center.artist_id
                            || relation.artist_id.len() > crate::playback::model::MAX_ID_BYTES
                        {
                            continue;
                        }
                        let kind = match relation.kind {
                            ArtistRelationKind::SharedTrackCredit => {
                                RadioTransitionKind::SharedTrackCredit
                            }
                            ArtistRelationKind::SimilarArtist => RadioTransitionKind::SimilarArtist,
                        };
                        let source_order = lease
                            .original_settings
                            .sources
                            .iter()
                            .position(|source| source.server_id == center.server_id)
                            .unwrap_or(usize::MAX);
                        ranked.push((
                            ArtistIdentity {
                                server_id: center.server_id.clone(),
                                artist_id: relation.artist_id,
                            },
                            kind,
                            index,
                            source_order,
                        ));
                    }
                }
                Ok(Err(crate::providers::ProviderError::UnsupportedCapability(_))) => {}
                // Unsupported or unverified metadata cannot establish a link. The
                // fresh-center search still evaluates the original scope.
                _ => relation_unknown = true,
            }
        } else {
            relation_unknown = true;
        }
    }
    let ranked = crate::playback::radio::rank_relations(ranked);
    let related_remaining = lease.original_settings.sources.iter().any(|source| {
        serde_json::to_string(source)
            .ok()
            .and_then(|key| {
                state
                    .db
                    .radio_scan_cursor(&lease.session_id, &format!("related:{key}"))
                    .ok()
            })
            .flatten()
            .is_some()
    });
    let related_phase = lease.center.is_some() && !ranked.is_empty() && related_remaining;
    let phase = if related_phase { "related" } else { "fresh" };
    let (mut pools, cursors, failed) = match tokio::time::timeout(DEADLINE, async {
        let mut pools = Vec::new();
        let mut cursors = Vec::new();
        let mut failed = false;
        for source in &lease.original_settings.sources {
            if !state.playback.radio_lease_current(lease) {
                break;
            }
            let key = match serde_json::to_string(source) {
                Ok(key) => format!("{phase}:{key}"),
                Err(_) => {
                    failed = true;
                    continue;
                }
            };
            let cursor = match state.db.radio_scan_cursor(&lease.session_id, &key) {
                Ok(Some(cursor)) => cursor,
                Ok(None) => continue,
                Err(_) => {
                    failed = true;
                    continue;
                }
            };
            let Ok(source_provider) = provider(state, &source.server_id).await else {
                failed = true;
                continue;
            };
            if !state.playback.radio_lease_current(lease) {
                break;
            }
            match selection::fetch_radio_window(source_provider.as_ref(), source, cursor).await {
                Ok((mut pool, next)) => {
                    if !state.playback.radio_lease_current(lease) {
                        break;
                    }
                    let mut seen_heard = false;
                    pool.tracks.retain(|song| {
                        if song.provider_metadata.ambiguous_music_artist
                            || song.artist_id.as_deref().is_none_or(str::is_empty)
                        {
                            return false;
                        }
                        let identity = crate::playback::model::TrackSource {
                            server_id: source.server_id.clone(),
                            track_id: song.id.clone(),
                        };
                        match (
                            state.db.radio_is_heard(&lease.session_id, &identity),
                            state.db.radio_is_excluded(&lease.session_id, &identity),
                        ) {
                            (Ok(true), Ok(false)) => seen_heard = true,
                            (Ok(_), Ok(_)) => {}
                            _ => failed = true,
                        }
                        if let Some(key) = song
                            .provider_metadata
                            .recording
                            .as_ref()
                            .and_then(|e| e.key())
                        {
                            match (
                                state.db.radio_recording_has_kind(
                                    &lease.session_id,
                                    key.as_str(),
                                    "heard",
                                ),
                                state.db.radio_recording_has_kind(
                                    &lease.session_id,
                                    key.as_str(),
                                    "excluded",
                                ),
                            ) {
                                (Ok(true), Ok(false)) => seen_heard = true,
                                (Ok(_), Ok(_)) => {}
                                _ => failed = true,
                            }
                        }
                        !state
                            .db
                            .radio_has_membership(&lease.session_id, &identity)
                            .unwrap_or(true)
                            && !state
                                .db
                                .radio_has_occurrence(&lease.session_id, &identity)
                                .unwrap_or(true)
                            && song
                                .provider_metadata
                                .recording
                                .as_ref()
                                .and_then(|e| e.key())
                                .is_none_or(|key| {
                                    !state
                                        .db
                                        .radio_recording_used(&lease.session_id, key.as_str())
                                        .unwrap_or(true)
                                })
                    });
                    pools.push(pool);
                    cursors.push((key, cursor, next, seen_heard));
                }
                Err(_) => failed = true,
            }
        }
        (pools, cursors, failed)
    })
    .await
    {
        Ok(result) => result,
        Err(_) => return (Vec::new(), Some("radio.sourceFailure".into()), false, None),
    };
    selection::clear_conflicting_recordings(&mut pools);
    if !state.playback.radio_lease_current(lease) {
        return (Vec::new(), None, false, None);
    }
    let chosen = crate::playback::radio::choose_transition(
        &lease.original_settings,
        &pools,
        &ranked,
        !related_phase,
    );
    if let Some((transition, ordered)) = chosen {
        let alternates: HashMap<_, _> = ordered
            .iter()
            .map(|source| {
                (
                    (source.server_id.clone(), source.track_id.clone()),
                    selection::radio_copy_alternates(&lease.original_settings, &pools, source),
                )
            })
            .collect();
        let mut evidence = HashMap::new();
        for pool in &pools {
            for song in &pool.tracks {
                evidence.insert(
                    (pool.source.server_id.clone(), song.id.clone()),
                    song.clone(),
                );
            }
        }
        let prepared = tokio::time::timeout(DEADLINE, async {
            let mut selected = Vec::new();
            let mut failures = 0;
            for primary in ordered {
                if selected.len() >= lease.capacity
                    || failures >= crate::playback::radio::MAX_PREPARATION_FAILURES
                    || !state.playback.radio_lease_current(lease)
                {
                    break;
                }
                let attempts = std::iter::once(primary.clone()).chain(
                    alternates
                        .get(&(primary.server_id.clone(), primary.track_id.clone()))
                        .into_iter()
                        .flatten()
                        .cloned(),
                );
                for source in attempts {
                    if failures >= crate::playback::radio::MAX_PREPARATION_FAILURES
                        || !state.playback.radio_lease_current(lease)
                    {
                        break;
                    }
                    if prepare_radio_copy(state, lease, &source).await {
                        if let Some(song) =
                            evidence.get(&(source.server_id.clone(), source.track_id.clone()))
                        {
                            let center_origin = (!transition
                                .center
                                .matches(&source.server_id, song.artist_id.as_deref()))
                            .then(|| transition.center.clone());
                            selected.push(selection::SelectionCandidate {
                                source,
                                song: song.clone(),
                                center_origin,
                            });
                        }
                        break;
                    }
                    failures += 1;
                }
            }
            (selected, failures)
        })
        .await;
        let proposal = (lease.center.as_ref() != Some(&transition.center)).then_some(transition);
        return match prepared {
            Ok((selected, failures)) if !selected.is_empty() => (
                selected,
                (failures > 0 || failed).then(|| "radio.sourceFailure".into()),
                failures == 0 && !failed,
                proposal,
            ),
            _ => (Vec::new(), Some("radio.sourceFailure".into()), false, None),
        };
    }
    let mut more = false;
    for (key, cursor, next, seen_heard) in cursors {
        if !state.playback.radio_lease_current(lease) {
            return (Vec::new(), None, false, None);
        }
        if !state
            .db
            .advance_radio_scan_with_heard(
                &lease.session_id,
                lease.queue_revision,
                &key,
                cursor,
                next,
                seen_heard,
            )
            .unwrap_or(false)
        {
            return (Vec::new(), Some("radio.sourceFailure".into()), false, None);
        }
        more |= next.is_some();
    }
    if related_phase {
        // The next coalesced wake starts the separately namespaced fresh pass.
        return (Vec::new(), None, true, None);
    }
    if more {
        (Vec::new(), None, true, None)
    } else if relation_unknown || failed {
        (Vec::new(), Some("radio.sourceFailure".into()), false, None)
    } else {
        (Vec::new(), Some("radio.exhausted".into()), false, None)
    }
}
