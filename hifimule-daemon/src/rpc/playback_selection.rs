//! Playback selection RPCs. This state is local and independent of device auto-fill.
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    AppState, ERR_INVALID_PARAMS, ERR_STORAGE_ERROR, JsonRpcError,
    handle_playback_apply_session_prepared,
};
use crate::playback::{
    model::{ApplySessionParams, SessionOperation},
    selection::{self, PlaybackSelectionConfig, SelectionError, SelectionKind, SelectionPool},
};

static START_GATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static START_EPOCH: AtomicU64 = AtomicU64::new(0);
const DEADLINE: Duration = Duration::from_secs(15);

#[cfg(test)]
pub(super) async fn test_ticket() -> u64 {
    let _guard = START_GATE.lock().await;
    START_EPOCH.fetch_add(1, Ordering::AcqRel) + 1
}

fn error(kind: SelectionError) -> JsonRpcError {
    let code = match kind {
        SelectionError::Setup => ERR_INVALID_PARAMS,
        SelectionError::Save => ERR_STORAGE_ERROR,
        SelectionError::SourceUnavailable => super::ERR_CONNECTION_FAILED,
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
}

pub async fn options(state: &AppState, params: Option<Value>) -> Result<Value, JsonRpcError> {
    let args: OptionsParams = serde_json::from_value(params.unwrap_or(Value::Null))
        .map_err(|_| error(SelectionError::Setup))?;
    if args.schema_version != 1 || args.server_id.is_empty() {
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
    match tokio::time::timeout(DEADLINE, selection::options(provider.as_ref(), args.kind)).await {
        Ok(Ok(values)) => Ok(json!({"data": {"supported": true, "options": values}})),
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
            let choices = selection::options(provider.as_ref(), source.kind)
                .await
                .map_err(error)?;
            if !choices.iter().any(|choice| choice.id == source.ref_id) {
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
    let _guard = START_GATE.lock().await;
    START_EPOCH.fetch_add(1, Ordering::AcqRel);
    Ok(json!({"data": {"cancelled": true}}))
}

pub async fn start(
    state: &AppState,
    params: Option<Value>,
    mutation_guard: Option<crate::sync::MutationGuard>,
) -> Result<Value, JsonRpcError> {
    version(params)?;
    let ticket = {
        let _guard = START_GATE.lock().await;
        START_EPOCH.fetch_add(1, Ordering::AcqRel) + 1
    };
    let path = path()?;
    let config = tokio::task::spawn_blocking(move || selection::load(&path))
        .await
        .map_err(|_| error(SelectionError::Save))?
        .map_err(error)?;
    start_with_config(state, config, ticket, mutation_guard).await
}

pub(super) async fn start_with_config(
    state: &AppState,
    config: PlaybackSelectionConfig,
    ticket: u64,
    mutation_guard: Option<crate::sync::MutationGuard>,
) -> Result<Value, JsonRpcError> {
    if config.sources.is_empty() {
        return Err(error(SelectionError::Setup));
    }
    let fetch = async {
        let mut pools: Vec<SelectionPool> = Vec::with_capacity(config.sources.len());
        for source in &config.sources {
            if START_EPOCH.load(Ordering::Acquire) != ticket {
                return Err(error(SelectionError::Cancelled));
            }
            let provider = provider(state, &source.server_id).await?;
            pools.push(
                selection::fetch_source(provider.as_ref(), source)
                    .await
                    .map_err(error)?,
            );
        }
        Ok(pools)
    };
    let pools = tokio::time::timeout(DEADLINE, fetch)
        .await
        .map_err(|_| error(SelectionError::SourceUnavailable))??;
    if START_EPOCH.load(Ordering::Acquire) != ticket {
        return Err(error(SelectionError::Cancelled));
    }
    let selected = selection::select(&config, pools).map_err(error)?;
    // Resolve before touching the owner. A missing or expired source leaves the current
    // listening session intact and generates no skip/taste signal. Try the next
    // bounded selected result if a track vanished after candidate retrieval.
    let preflight = async {
        for source in selected {
            if START_EPOCH.load(Ordering::Acquire) != ticket {
                return Err(error(SelectionError::Cancelled));
            }
            let provider = provider(state, &source.server_id).await?;
            if let Ok(description) = provider.resolve_playback(&source.track_id).await
                && !description.representations.is_empty()
            {
                return Ok((source, description));
            }
        }
        Err(error(SelectionError::SourceUnavailable))
    };
    let (first, description) = tokio::time::timeout(DEADLINE, preflight)
        .await
        .map_err(|_| error(SelectionError::SourceUnavailable))??;
    let _guard = START_GATE.lock().await;
    if START_EPOCH.load(Ordering::Acquire) != ticket {
        return Err(error(SelectionError::Cancelled));
    }
    let snapshot = state.playback.snapshot().map_err(super::playback_error)?;
    let command = ApplySessionParams {
        schema_version: 1,
        instance_id: snapshot.instance_id,
        session_id: snapshot.session_id,
        command_id: uuid::Uuid::new_v4().to_string(),
        expected_queue_revision: snapshot.queue_revision,
        operation: SessionOperation::PlayTrack { source: first },
    };
    handle_playback_apply_session_prepared(state, command, mutation_guard, false, Some(description))
        .await
}
