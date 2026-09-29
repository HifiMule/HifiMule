use super::*;
use crate::playback::basket_export::*;
use crate::playback::export::*;
use crate::playback::server_export::*;
use serde_json::json;

static SAVES: std::sync::LazyLock<Arc<tokio::sync::Semaphore>> =
    std::sync::LazyLock::new(|| Arc::new(tokio::sync::Semaphore::new(1)));
static READS: std::sync::LazyLock<Arc<tokio::sync::Semaphore>> =
    std::sync::LazyLock::new(|| Arc::new(tokio::sync::Semaphore::new(4)));
static EXPORT_PARTS: std::sync::LazyLock<Arc<tokio::sync::Semaphore>> =
    std::sync::LazyLock::new(|| Arc::new(tokio::sync::Semaphore::new(4)));
static BASKET_EXPORTS: std::sync::LazyLock<Arc<tokio::sync::Semaphore>> =
    std::sync::LazyLock::new(|| Arc::new(tokio::sync::Semaphore::new(1)));
fn parse<T: serde::de::DeserializeOwned>(params: Option<Value>) -> Result<T, JsonRpcError> {
    serde_json::from_value(params.unwrap_or(Value::Null))
        .map_err(|_| playback_error(error("INVALID_SNAPSHOT_REQUEST")))
}
pub(super) async fn save(
    state: &AppState,
    params: Option<Value>,
    guard: Option<crate::sync::MutationGuard>,
) -> Result<Value, JsonRpcError> {
    let p: SaveSnapshotParams = parse(params)?;
    p.validate().map_err(playback_error)?;
    let permit = SAVES
        .clone()
        .try_acquire_owned()
        .map_err(|_| playback_error(error("PLAYBACK_BUSY")))?;
    let owner = state.playback.clone();
    // Both permits and shutdown ownership stay with actual blocking work.
    let result = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        owner.save_snapshot(p, guard)
    })
    .await
    .map_err(playback_task_error)?
    .map_err(playback_error)?;
    Ok(json!({"data":result}))
}
pub(super) async fn read(
    state: &AppState,
    method: &str,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    enum Read {
        List(ListSnapshotsParams),
        Get(GetSnapshotParams),
        Entries(ListSnapshotEntriesParams),
    }
    let query = match method {
        "playback.listSnapshots" => Read::List(parse(params)?),
        "playback.getSnapshot" => Read::Get(parse(params)?),
        _ => Read::Entries(parse(params)?),
    };
    let permit = READS
        .clone()
        .try_acquire_owned()
        .map_err(|_| playback_error(error("PLAYBACK_BUSY")))?;
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        match query {
            Read::List(p) => db.list_listening_snapshots(p).map(|v| json!(v)),
            Read::Get(p) => db.get_listening_snapshot(p).map(|v| json!(v)),
            Read::Entries(p) => db.list_listening_snapshot_entries(p).map(|v| json!(v)),
        }
    })
    .await
    .map_err(playback_task_error)?
    .map_err(playback_error)?;
    Ok(json!({"data":result}))
}

fn export_parse<T: serde::de::DeserializeOwned>(params: Option<Value>) -> Result<T, JsonRpcError> {
    serde_json::from_value(params.unwrap_or(Value::Null))
        .map_err(|_| playback_error(crate::playback::export::error("INVALID_SNAPSHOT_REQUEST")))
}

async fn basket_plan(
    state: &AppState,
    p: PlanBasketExportParams,
) -> Result<BasketExportPlan, JsonRpcError> {
    validate_plan(&p).map_err(playback_error)?;
    let manifest = state
        .device_manager
        .get_manifest_for_device(&p.target_device_id)
        .await
        .ok_or_else(|| {
            playback_error(crate::playback::export::error(
                "BASKET_EXPORT_TARGET_UNAVAILABLE",
            ))
        })?;
    let mut entries = Vec::new();
    let mut cursor = None;
    loop {
        let page = state
            .db
            .list_listening_snapshot_entries(ListSnapshotEntriesParams {
                schema_version: 1,
                snapshot_id: p.snapshot_id.clone(),
                cursor,
                limit: Some(200),
            })
            .map_err(playback_error)?;
        entries.extend(page.entries);
        if entries.len() > MAX_ENTRIES {
            return Err(playback_error(crate::playback::export::error(
                "BASKET_EXPORT_TOO_LARGE",
            )));
        }
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    let mut limitations = Vec::new();
    let mut items = Vec::new();
    if entries.is_empty() {
        limitations.push(BasketExportLimitation {
            ordinal: "0".into(),
            code: "emptySnapshot".into(),
            server_id: "".into(),
            track_id: "".into(),
        });
    }
    let mut identities = std::collections::HashSet::new();
    let mut raw_ids = std::collections::HashMap::<String, String>::new();
    for entry in &entries {
        let ordinal = entry.ordinal.clone();
        let server_id = entry.source.server_id.clone();
        let track_id = entry.source.track_id.clone();
        let identity = (server_id.clone(), track_id.clone());
        if !identities.insert(identity) {
            limitations.push(BasketExportLimitation {
                ordinal,
                code: "repeatUnsupported".into(),
                server_id,
                track_id,
            });
            continue;
        }
        if raw_ids
            .insert(track_id.clone(), server_id.clone())
            .is_some_and(|prior| prior != server_id)
        {
            limitations.push(BasketExportLimitation {
                ordinal,
                code: "rawIdCollisionUnsupported".into(),
                server_id,
                track_id,
            });
            continue;
        }
        let provider = match crate::server_manager::get_provider_by_server_id(
            &state.server_manager,
            &state.db,
            &server_id,
        )
        .await
        {
            Ok(provider) => provider,
            Err(_) => {
                limitations.push(BasketExportLimitation {
                    ordinal,
                    code: "sourceUnavailable".into(),
                    server_id,
                    track_id,
                });
                continue;
            }
        };
        let song = match provider.get_song(&track_id).await {
            Ok(song) => song,
            Err(_) => {
                limitations.push(BasketExportLimitation {
                    ordinal,
                    code: "trackUnavailable".into(),
                    server_id,
                    track_id,
                });
                continue;
            }
        };
        if song.id != track_id {
            limitations.push(BasketExportLimitation {
                ordinal,
                code: "identityChanged".into(),
                server_id,
                track_id,
            });
            continue;
        }
        let Some(size_bytes) = song.size_bytes.filter(|size| *size > 0) else {
            limitations.push(BasketExportLimitation {
                ordinal,
                code: "sizeUnavailable".into(),
                server_id,
                track_id,
            });
            continue;
        };
        let Some(size_ticks) = i64::from(song.duration_seconds).checked_mul(10_000_000) else {
            limitations.push(BasketExportLimitation {
                ordinal,
                code: "metadataOverflow".into(),
                server_id,
                track_id,
            });
            continue;
        };
        items.push(crate::device::BasketItem {
            id: song.id,
            name: song.title,
            item_type: "Audio".into(),
            server_id: Some(server_id),
            artist: song.artist_name,
            child_count: 1,
            size_ticks,
            size_bytes,
        });
    }
    let source_count = items
        .iter()
        .filter_map(|item| item.server_id.as_deref())
        .collect::<std::collections::HashSet<_>>()
        .len();
    if manifest.auto_sync_on_connect && source_count > 1 {
        limitations.push(BasketExportLimitation {
            ordinal: "0".into(),
            code: "autoSyncMixedSourceUnsupported".into(),
            server_id: "".into(),
            track_id: "".into(),
        });
    }
    let projected_items = projected(p.action, &manifest, &items);
    Ok(BasketExportPlan {
        schema_version: 1,
        snapshot_id: p.snapshot_id,
        target_device_id: p.target_device_id,
        target_name: manifest
            .name
            .clone()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| manifest.device_id.clone()),
        target_icon: manifest.icon.clone(),
        action: p.action,
        observed_basket_hash: crate::device::canonical_basket_hash(&manifest.basket_items),
        snapshot_entry_count: entries.len().to_string(),
        projected_entry_count: projected_items.len().to_string(),
        faithful: limitations.is_empty(),
        limitations,
        items,
    })
}

pub(super) async fn basket_export_read(
    state: &AppState,
    method: &str,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let value = match method {
        "playback.planSnapshotBasketExport" => {
            let p: PlanBasketExportParams = export_parse(params)?;
            json!(basket_plan(state, p).await?)
        }
        "playback.getSnapshotBasketExport" => {
            let p: BasketExportLocator = export_parse(params)?;
            if p.schema_version != 1 {
                return Err(playback_error(crate::playback::export::error(
                    "INVALID_BASKET_EXPORT_REQUEST",
                )));
            }
            json!(
                state
                    .db
                    .get_basket_export(&p.operation_id)
                    .map_err(playback_error)?
            )
        }
        "playback.listSnapshotBasketExports" => {
            let p: ListBasketExportsParams = export_parse(params)?;
            json!(state.db.list_basket_exports(p).map_err(playback_error)?)
        }
        _ => {
            let p: BasketExportLocator = export_parse(params)?;
            if p.schema_version != 1 {
                return Err(playback_error(crate::playback::export::error(
                    "INVALID_BASKET_EXPORT_REQUEST",
                )));
            }
            let operation = state
                .db
                .get_basket_export(&p.operation_id)
                .map_err(playback_error)?;
            if operation.state != BasketExportState::CommitUncertain {
                json!(operation)
            } else {
                let manifest = state
                    .device_manager
                    .get_manifest_for_device(&operation.target_device_id)
                    .await;
                let current = manifest
                    .as_ref()
                    .map(|m| crate::device::canonical_basket_hash(&m.basket_items));
                let recovered = if current.as_deref() == operation.post_basket_hash.as_deref() {
                    state.db.finish_basket_export(
                        &operation.operation_id,
                        BasketExportState::CommitConfirmed,
                        current.as_deref(),
                        manifest.as_ref().map(|m| m.basket_items.as_slice()),
                        Some("reconciledPostHash"),
                    )
                } else if current.as_deref() == Some(operation.pre_basket_hash.as_str()) {
                    state.db.finish_basket_export(
                        &operation.operation_id,
                        BasketExportState::Failed,
                        current.as_deref(),
                        manifest.as_ref().map(|m| m.basket_items.as_slice()),
                        Some("confirmedRollback"),
                    )
                } else {
                    Ok(operation)
                }
                .map_err(playback_error)?;
                json!(recovered)
            }
        }
    };
    Ok(json!({"data":value}))
}

pub(super) async fn basket_export_write(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let p: CommitBasketExportParams = export_parse(params)?;
    let _permit = BASKET_EXPORTS
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| playback_error(crate::playback::export::error("PLAYBACK_BUSY")))?;
    let plan = basket_plan(
        state,
        PlanBasketExportParams {
            schema_version: p.schema_version,
            snapshot_id: p.snapshot_id.clone(),
            target_device_id: p.target_device_id.clone(),
            action: p.action,
        },
    )
    .await?;
    let current = state
        .device_manager
        .get_manifest_for_device(&p.target_device_id)
        .await
        .ok_or_else(|| {
            playback_error(crate::playback::export::error(
                "BASKET_EXPORT_TARGET_UNAVAILABLE",
            ))
        })?;
    let projected_items = projected(p.action, &current, &plan.items);
    let post_hash = crate::device::canonical_basket_hash(&projected_items);
    let prior = state
        .db
        .record_basket_export_intent(&p, &plan.observed_basket_hash, &post_hash)
        .map_err(playback_error)?;
    if prior.state != BasketExportState::IntentRecorded {
        return Ok(json!({"data":prior}));
    }
    if !plan.faithful {
        let operation = state
            .db
            .finish_basket_export(
                &p.operation_id,
                BasketExportState::Failed,
                None,
                None,
                Some("preflightLimitations"),
            )
            .map_err(playback_error)?;
        return Ok(json!({"data":operation}));
    }
    if p.action == BasketExportAction::Replace
        && plan.observed_basket_hash != p.expected_basket_hash
    {
        let operation = state
            .db
            .finish_basket_export(
                &p.operation_id,
                BasketExportState::Conflict,
                Some(&plan.observed_basket_hash),
                None,
                Some("basketChanged"),
            )
            .map_err(playback_error)?;
        return Ok(json!({"data":operation}));
    }
    let action = p.action;
    let planned = plan.items;
    match state
        .device_manager
        .update_basket_checked(
            &p.target_device_id,
            (p.action == BasketExportAction::Replace).then_some(p.expected_basket_hash.as_str()),
            move |existing| {
                Ok(match action {
                    BasketExportAction::Add => merge_add(existing, &planned),
                    BasketExportAction::Replace => planned,
                })
            },
        )
        .await
    {
        Ok((manifest, hash)) => {
            let operation = state
                .db
                .finish_basket_export(
                    &p.operation_id,
                    BasketExportState::CommitConfirmed,
                    Some(&hash),
                    Some(&manifest.basket_items),
                    None,
                )
                .map_err(playback_error)?;
            Ok(json!({"data":operation}))
        }
        Err(crate::device::CheckedManifestUpdateError::Conflict { authoritative_hash }) => {
            let operation = state
                .db
                .finish_basket_export(
                    &p.operation_id,
                    BasketExportState::Conflict,
                    Some(&authoritative_hash),
                    None,
                    Some("basketChanged"),
                )
                .map_err(playback_error)?;
            Ok(json!({"data":operation}))
        }
        Err(_) => {
            let operation = state
                .db
                .finish_basket_export(
                    &p.operation_id,
                    BasketExportState::CommitUncertain,
                    None,
                    None,
                    Some("manifestPersistenceUncertain"),
                )
                .map_err(playback_error)?;
            Ok(json!({"data":operation}))
        }
    }
}

pub(super) async fn export_read(
    state: &AppState,
    method: &str,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let db = state.db.clone();
    let value = match method {
        "playback.planSnapshotPlaylistExport" => {
            let p: PlanExportParams = export_parse(params)?;
            let mut plan = tokio::task::spawn_blocking(move || db.plan_server_export(p))
                .await
                .map_err(playback_task_error)?
                .map_err(playback_error)?;
            let requested_name = plan.name.clone();
            for part in &mut plan.parts {
                match crate::server_manager::get_provider_by_server_id(
                    &state.server_manager,
                    &state.db,
                    &part.server_id,
                )
                .await
                {
                    Ok(provider)
                        if provider.playlist_export_support().is_some_and(|support| {
                            support.preserves_order_and_repeats
                                && support.reliable_ordered_read_back
                        }) =>
                    {
                        match provider.list_playlists().await {
                            Ok(existing)
                                if existing
                                    .iter()
                                    .any(|playlist| playlist.name == requested_name) =>
                            {
                                part.state = PartState::Failed;
                                part.reason = Some("nameCollision".into());
                            }
                            Ok(_) => {}
                            Err(crate::providers::ProviderError::Auth(_))
                            | Err(crate::providers::ProviderError::Forbidden) => {
                                part.state = PartState::Denied;
                                part.reason = Some("permissionDenied".into());
                            }
                            Err(_) => {
                                part.state = PartState::Failed;
                                part.reason = Some("sourceUnavailable".into());
                            }
                        }
                    }
                    Ok(_) => {
                        part.state = PartState::Unsupported;
                        part.reason = Some("playlistExportUnverified".into());
                    }
                    Err(crate::providers::ProviderError::Auth(_))
                    | Err(crate::providers::ProviderError::Forbidden) => {
                        part.state = PartState::Denied;
                        part.reason = Some("permissionDenied".into());
                    }
                    Err(_) => {
                        part.state = PartState::Failed;
                        part.reason = Some("sourceUnavailable".into());
                    }
                }
            }
            plan.status = aggregate(&plan.parts.iter().map(|part| part.state).collect::<Vec<_>>());
            json!(plan)
        }
        "playback.getSnapshotPlaylistExport" => {
            let p: ExportLocator = export_parse(params)?;
            json!(
                tokio::task::spawn_blocking(move || db.get_server_export(p))
                    .await
                    .map_err(playback_task_error)?
                    .map_err(playback_error)?
            )
        }
        _ => {
            let p: crate::playback::export::ListSnapshotEntriesParams = export_parse(params)?;
            let snapshot_id = p.snapshot_id;
            json!(
                tokio::task::spawn_blocking(move || db.list_server_exports(&snapshot_id))
                    .await
                    .map_err(playback_task_error)?
                    .map_err(playback_error)?
            )
        }
    };
    Ok(json!({"data":value}))
}

async fn execute_part(state: Arc<AppState>, operation_id: String, part: ExportPart, name: String) {
    let _permit = match EXPORT_PARTS.clone().acquire_owned().await {
        Ok(permit) => permit,
        Err(_) => return,
    };
    let provider = match crate::server_manager::get_provider_by_server_id(
        &state.server_manager,
        &state.db,
        &part.server_id,
    )
    .await
    {
        Ok(provider) => provider,
        Err(crate::providers::ProviderError::Auth(_))
        | Err(crate::providers::ProviderError::Forbidden) => {
            let _ = state.db.transition_export_part(
                &operation_id,
                &part.server_id,
                part.state,
                PartState::Denied,
                None,
                0,
                Some("permissionDenied"),
            );
            return;
        }
        Err(_) => {
            let _ = state.db.transition_export_part(
                &operation_id,
                &part.server_id,
                part.state,
                PartState::Failed,
                None,
                0,
                Some("sourceUnavailable"),
            );
            return;
        }
    };
    let Some(support) = provider.playlist_export_support().filter(|support| {
        support.preserves_order_and_repeats
            && support.reliable_ordered_read_back
            && support.max_create_ids > 0
            && support.max_append_ids > 0
    }) else {
        let _ = state.db.transition_export_part(
            &operation_id,
            &part.server_id,
            part.state,
            PartState::Unsupported,
            None,
            0,
            Some("playlistWriteUnsupported"),
        );
        return;
    };
    if part.state == PartState::Populating {
        let Some(playlist_id) = part.playlist_id.as_deref() else {
            let _ = state.db.transition_export_part(
                &operation_id,
                &part.server_id,
                PartState::Populating,
                PartState::Unresolved,
                None,
                part.confirmed_count.parse().unwrap_or(0),
                Some("missingPlaylistIdentity"),
            );
            return;
        };
        match provider.get_playlist(playlist_id).await {
            Ok(saved) => {
                let actual = saved
                    .tracks
                    .iter()
                    .map(|track| track.id.as_str())
                    .collect::<Vec<_>>();
                let expected = part
                    .track_ids
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>();
                let confirmed_before = part.confirmed_count.parse().unwrap_or(0);
                if actual.len() < confirmed_before
                    || actual.len() > expected.len()
                    || actual != expected[..actual.len()]
                {
                    let _ = state.db.transition_export_part(
                        &operation_id,
                        &part.server_id,
                        PartState::Populating,
                        PartState::Unresolved,
                        Some(playlist_id),
                        common_prefix(&actual, &expected),
                        Some("contentMismatch"),
                    );
                    return;
                }
                let _ =
                    state
                        .db
                        .checkpoint_export_part(&operation_id, &part.server_id, actual.len());
                return populate_batches(
                    &state,
                    &operation_id,
                    &part,
                    &name,
                    provider,
                    support,
                    playlist_id.to_owned(),
                    actual.len(),
                )
                .await;
            }
            Err(_) => {
                let _ = state.db.transition_export_part(
                    &operation_id,
                    &part.server_id,
                    PartState::Populating,
                    PartState::Ambiguous,
                    Some(playlist_id),
                    part.confirmed_count.parse().unwrap_or(0),
                    Some("readBackUnavailable"),
                );
                return;
            }
        }
    }
    match provider.list_playlists().await {
        Ok(existing) if existing.iter().any(|p| p.name == name) => {
            let _ = state.db.transition_export_part(
                &operation_id,
                &part.server_id,
                PartState::Pending,
                PartState::Failed,
                None,
                0,
                Some("nameCollision"),
            );
            return;
        }
        Err(crate::providers::ProviderError::Forbidden)
        | Err(crate::providers::ProviderError::Auth(_)) => {
            let _ = state.db.transition_export_part(
                &operation_id,
                &part.server_id,
                PartState::Pending,
                PartState::Denied,
                None,
                0,
                Some("permissionDenied"),
            );
            return;
        }
        Err(_) => {
            let _ = state.db.transition_export_part(
                &operation_id,
                &part.server_id,
                PartState::Pending,
                PartState::Failed,
                None,
                0,
                Some("collisionCheckFailed"),
            );
            return;
        }
        _ => {}
    }
    if state
        .db
        .transition_export_part(
            &operation_id,
            &part.server_id,
            PartState::Pending,
            PartState::Creating,
            None,
            0,
            None,
        )
        .is_err()
    {
        return;
    }
    let first_len = part.track_ids.len().min(support.max_create_ids);
    match provider
        .create_playlist(&name, &part.track_ids[..first_len])
        .await
    {
        Ok(playlist_id) => match provider.get_playlist(&playlist_id).await {
            Ok(saved) => {
                let actual = saved
                    .tracks
                    .iter()
                    .map(|v| v.id.as_str())
                    .collect::<Vec<_>>();
                let expected = part.track_ids[..first_len]
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>();
                let (next_state, confirmed, reason) =
                    if actual == expected && first_len == part.track_ids.len() {
                        (PartState::Succeeded, expected.len(), None)
                    } else if actual == expected {
                        (PartState::Populating, expected.len(), None)
                    } else {
                        (
                            PartState::Partial,
                            common_prefix(&actual, &expected),
                            Some("contentMismatch"),
                        )
                    };
                let transitioned = state.db.transition_export_part(
                    &operation_id,
                    &part.server_id,
                    PartState::Creating,
                    next_state,
                    Some(&playlist_id),
                    confirmed,
                    reason,
                );
                if next_state == PartState::Populating && transitioned.is_ok() {
                    populate_batches(
                        &state,
                        &operation_id,
                        &part,
                        &name,
                        provider,
                        support,
                        playlist_id,
                        confirmed,
                    )
                    .await;
                }
            }
            Err(_) => {
                let _ = state.db.transition_export_part(
                    &operation_id,
                    &part.server_id,
                    PartState::Creating,
                    PartState::Ambiguous,
                    Some(&playlist_id),
                    0,
                    Some("readBackUnavailable"),
                );
            }
        },
        Err(crate::providers::ProviderError::Forbidden)
        | Err(crate::providers::ProviderError::Auth(_)) => {
            let _ = state.db.transition_export_part(
                &operation_id,
                &part.server_id,
                PartState::Creating,
                PartState::Denied,
                None,
                0,
                Some("permissionDenied"),
            );
        }
        Err(_) => {
            let _ = state.db.transition_export_part(
                &operation_id,
                &part.server_id,
                PartState::Creating,
                PartState::Ambiguous,
                None,
                0,
                Some("createReplyAmbiguous"),
            );
        }
    }
}

async fn populate_batches(
    state: &AppState,
    operation_id: &str,
    part: &ExportPart,
    _name: &str,
    provider: Arc<dyn crate::providers::MediaProvider>,
    support: crate::providers::PlaylistExportSupport,
    playlist_id: String,
    mut confirmed: usize,
) {
    while confirmed < part.track_ids.len() {
        let end = (confirmed + support.max_append_ids).min(part.track_ids.len());
        if provider
            .add_to_playlist(&playlist_id, &part.track_ids[confirmed..end])
            .await
            .is_err()
        {
            let _ = state.db.transition_export_part(
                operation_id,
                &part.server_id,
                PartState::Populating,
                PartState::Ambiguous,
                Some(&playlist_id),
                confirmed,
                Some("appendReplyAmbiguous"),
            );
            return;
        }
        let saved = match provider.get_playlist(&playlist_id).await {
            Ok(saved) => saved,
            Err(_) => {
                let _ = state.db.transition_export_part(
                    operation_id,
                    &part.server_id,
                    PartState::Populating,
                    PartState::Ambiguous,
                    Some(&playlist_id),
                    confirmed,
                    Some("readBackUnavailable"),
                );
                return;
            }
        };
        let actual = saved
            .tracks
            .iter()
            .map(|track| track.id.as_str())
            .collect::<Vec<_>>();
        let expected = part.track_ids[..end]
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        if actual != expected {
            let _ = state.db.transition_export_part(
                operation_id,
                &part.server_id,
                PartState::Populating,
                PartState::Unresolved,
                Some(&playlist_id),
                common_prefix(&actual, &expected),
                Some("contentMismatch"),
            );
            return;
        }
        confirmed = end;
        if state
            .db
            .checkpoint_export_part(operation_id, &part.server_id, confirmed)
            .is_err()
        {
            return;
        }
    }
    let _ = state.db.transition_export_part(
        operation_id,
        &part.server_id,
        PartState::Populating,
        PartState::Succeeded,
        Some(&playlist_id),
        confirmed,
        None,
    );
}
fn common_prefix(actual: &[&str], expected: &[&str]) -> usize {
    actual
        .iter()
        .zip(expected)
        .take_while(|(a, b)| a == b)
        .count()
}

pub(super) async fn export_write(
    state: &Arc<AppState>,
    method: &str,
    params: Option<Value>,
    _guard: Option<crate::sync::MutationGuard>,
) -> Result<Value, JsonRpcError> {
    let operation = match method {
        "playback.startSnapshotPlaylistExport" => {
            let p: StartExportParams = export_parse(params)?;
            let db = state.db.clone();
            tokio::task::spawn_blocking(move || db.create_server_export(p))
                .await
                .map_err(playback_task_error)?
                .map_err(playback_error)?
        }
        "playback.retrySnapshotPlaylistExport" => {
            let p: ExportPartLocator = export_parse(params)?;
            let current = state
                .db
                .get_server_export(ExportLocator {
                    schema_version: p.schema_version,
                    operation_id: p.operation_id.clone(),
                })
                .map_err(playback_error)?;
            let Some(part) = current.parts.iter().find(|v| v.server_id == p.server_id) else {
                return Err(playback_error(crate::playback::export::error(
                    "SNAPSHOT_NOT_FOUND",
                )));
            };
            if part.state != PartState::Failed || !part.safe_to_retry {
                return Err(playback_error(crate::playback::export::error(
                    "INVALID_SNAPSHOT_REQUEST",
                )));
            }
            state
                .db
                .retry_export_part(&p.operation_id, &p.server_id)
                .map_err(playback_error)?;
            state
                .db
                .get_server_export(ExportLocator {
                    schema_version: 1,
                    operation_id: p.operation_id,
                })
                .map_err(playback_error)?
        }
        "playback.reconcileSnapshotPlaylistExport" => {
            let p: ExportPartLocator = export_parse(params)?;
            let current = state
                .db
                .get_server_export(ExportLocator {
                    schema_version: p.schema_version,
                    operation_id: p.operation_id.clone(),
                })
                .map_err(playback_error)?;
            let Some(part) = current.parts.iter().find(|v| v.server_id == p.server_id) else {
                return Err(playback_error(crate::playback::export::error(
                    "SNAPSHOT_NOT_FOUND",
                )));
            };
            if matches!(part.state, PartState::Ambiguous | PartState::Partial)
                && part.playlist_id.is_some()
            {
                state
                    .db
                    .transition_export_part(
                        &p.operation_id,
                        &p.server_id,
                        part.state,
                        PartState::Populating,
                        part.playlist_id.as_deref(),
                        part.confirmed_count.parse().unwrap_or(0),
                        Some("reconciliationRequested"),
                    )
                    .map_err(playback_error)?;
            } else if part.state == PartState::Ambiguous {
                state
                    .db
                    .transition_export_part(
                        &p.operation_id,
                        &p.server_id,
                        PartState::Ambiguous,
                        PartState::Unresolved,
                        None,
                        part.confirmed_count.parse().unwrap_or(0),
                        Some("manualRecoveryRequired"),
                    )
                    .map_err(playback_error)?;
            }
            state
                .db
                .get_server_export(ExportLocator {
                    schema_version: 1,
                    operation_id: p.operation_id,
                })
                .map_err(playback_error)?
        }
        _ => {
            let p: ExportPartLocator = export_parse(params)?;
            let current = state
                .db
                .get_server_export(ExportLocator {
                    schema_version: p.schema_version,
                    operation_id: p.operation_id.clone(),
                })
                .map_err(playback_error)?;
            let Some(part) = current.parts.iter().find(|v| v.server_id == p.server_id) else {
                return Err(playback_error(crate::playback::export::error(
                    "SNAPSHOT_NOT_FOUND",
                )));
            };
            state
                .db
                .transition_export_part(
                    &p.operation_id,
                    &p.server_id,
                    part.state,
                    PartState::Canceled,
                    part.playlist_id.as_deref(),
                    part.confirmed_count.parse().unwrap_or(0),
                    Some("userCanceled"),
                )
                .map_err(playback_error)?;
            state
                .db
                .get_server_export(ExportLocator {
                    schema_version: 1,
                    operation_id: p.operation_id,
                })
                .map_err(playback_error)?
        }
    };
    if matches!(
        method,
        "playback.startSnapshotPlaylistExport" | "playback.retrySnapshotPlaylistExport"
    ) {
        schedule_operation(state.clone(), operation.clone());
    } else if method == "playback.reconcileSnapshotPlaylistExport" {
        schedule_operation(state.clone(), operation.clone());
    }
    let result = state
        .db
        .get_server_export(ExportLocator {
            schema_version: 1,
            operation_id: operation.operation_id.unwrap(),
        })
        .map_err(playback_error)?;
    Ok(json!({"data":result}))
}

fn schedule_operation(state: Arc<AppState>, operation: ExportOperation) {
    let Some(operation_id) = operation.operation_id.clone() else {
        return;
    };
    for part in operation
        .parts
        .into_iter()
        .filter(|part| matches!(part.state, PartState::Pending | PartState::Populating))
    {
        let state = state.clone();
        let operation_id = operation_id.clone();
        let name = operation.name.clone();
        tokio::spawn(async move {
            execute_part(state, operation_id, part, name).await;
        });
    }
}

pub(super) fn resume_pending_exports(state: Arc<AppState>) {
    tokio::spawn(async move {
        let db = state.db.clone();
        if let Ok(Ok(operations)) =
            tokio::task::spawn_blocking(move || db.pending_server_exports()).await
        {
            for operation in operations {
                schedule_operation(state.clone(), operation);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::playback::model::*;
    #[tokio::test]
    async fn snapshot_rpc_round_trip_is_offline_bounded_and_strict() {
        let mut state = super::super::tests::make_test_state(Arc::new(Database::memory().unwrap()));
        state.playback.stop_and_join().unwrap();
        Arc::get_mut(&mut state).unwrap().playback = crate::playback::PlaybackSession::restore(
            state.db.clone(),
            uuid::Uuid::new_v4().to_string(),
        );
        let first = state.playback.snapshot().unwrap();
        state
            .playback
            .apply(ApplySessionParams {
                schema_version: 1,
                instance_id: first.instance_id,
                session_id: first.session_id,
                command_id: uuid::Uuid::new_v4().to_string(),
                expected_queue_revision: first.queue_revision,
                operation: SessionOperation::ReplaceQueue {
                    sources: vec![
                        TrackSource {
                            server_id: "removed-source".into(),
                            track_id: "opaque:book/part".into()
                        };
                        3
                    ],
                },
            })
            .unwrap();
        let observed = state.playback.snapshot().unwrap();
        let p = json!({"schemaVersion":1,"operationId":uuid::Uuid::new_v4().to_string(),"instanceId":observed.instance_id,"sessionId":observed.session_id,"expectedQueueRevision":observed.queue_revision,"expectedMainOccurrenceId":observed.main_current.as_ref().unwrap().occurrence_id});
        let result = save(
            &state,
            Some(p.clone()),
            state.sync_operation_manager.try_admit_mutation(),
        )
        .await
        .unwrap();
        assert_eq!(result["data"]["status"], "saved");
        let header = &result["data"]["snapshot"];
        assert_eq!(header["entryCount"], "3");
        assert!(header.get("canonicalRequest").is_none());
        assert_eq!(save(&state, Some(p.clone()), None).await.unwrap(), result);
        let mut missing_main = p.clone();
        missing_main
            .as_object_mut()
            .unwrap()
            .remove("expectedMainOccurrenceId");
        assert!(save(&state, Some(missing_main), None).await.is_err());
        let recovered = read(
            &state,
            "playback.getSnapshot",
            Some(json!({"schemaVersion":1,"operationId":p["operationId"]})),
        )
        .await
        .unwrap();
        assert_eq!(&recovered["data"], header);
        let page = read(
            &state,
            "playback.listSnapshotEntries",
            Some(json!({"schemaVersion":1,"snapshotId":header["snapshotId"],"limit":2})),
        )
        .await
        .unwrap();
        assert_eq!(page["data"]["entries"].as_array().unwrap().len(), 2);
        assert_eq!(
            page["data"]["entries"][0]["source"]["trackId"],
            "opaque:book/part"
        );
        assert_eq!(page["data"]["entries"][0]["sourceAvailable"], false);
        for bad in [
            json!({"schemaVersion":2}),
            json!({"schemaVersion":1,"limit":201}),
            json!({"schemaVersion":1,"cursor":"2:list:0"}),
            json!({"schemaVersion":1,"unexpected":true}),
        ] {
            assert!(
                read(&state, "playback.listSnapshots", Some(bad))
                    .await
                    .is_err()
            );
        }
        assert_eq!(
            serde_json::to_value(state.playback.snapshot().unwrap()).unwrap(),
            serde_json::to_value(observed).unwrap()
        );
        assert!(state.server_manager.read().await.servers.is_empty());
        assert!(state.db.claim_feedback().unwrap().is_none());
        state.playback.stop_and_join().unwrap();
    }
}
