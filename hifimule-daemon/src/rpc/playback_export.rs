use super::*;
use crate::playback::export::*;
use crate::playback::server_export::{self, *};
use serde_json::json;

static SAVES: std::sync::LazyLock<Arc<tokio::sync::Semaphore>> =
    std::sync::LazyLock::new(|| Arc::new(tokio::sync::Semaphore::new(1)));
static READS: std::sync::LazyLock<Arc<tokio::sync::Semaphore>> =
    std::sync::LazyLock::new(|| Arc::new(tokio::sync::Semaphore::new(4)));
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

pub(super) async fn export_read(
    state: &AppState,
    method: &str,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let db = state.db.clone();
    let value = match method {
        "playback.planSnapshotPlaylistExport" => {
            let p: PlanExportParams = export_parse(params)?;
            json!(
                tokio::task::spawn_blocking(move || db.plan_server_export(p))
                    .await
                    .map_err(playback_task_error)?
                    .map_err(playback_error)?
            )
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

async fn execute_part(state: &AppState, operation_id: &str, part: ExportPart, name: &str) {
    if part.track_ids.len() > server_export::MAX_PROVIDER_IDS {
        let _ = state.db.transition_export_part(
            operation_id,
            &part.server_id,
            PartState::Pending,
            PartState::Unsupported,
            None,
            0,
            Some("providerRequestLimit"),
        );
        return;
    }
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
                operation_id,
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
                operation_id,
                &part.server_id,
                PartState::Pending,
                PartState::Failed,
                None,
                0,
                Some("sourceUnavailable"),
            );
            return;
        }
    };
    if !provider.capabilities().supports_playlist_write {
        let _ = state.db.transition_export_part(
            operation_id,
            &part.server_id,
            PartState::Pending,
            PartState::Unsupported,
            None,
            0,
            Some("playlistWriteUnsupported"),
        );
        return;
    }
    match provider.list_playlists().await {
        Ok(existing) if existing.iter().any(|p| p.name == name) => {
            let _ = state.db.transition_export_part(
                operation_id,
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
                operation_id,
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
                operation_id,
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
            operation_id,
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
    match provider.create_playlist(name, &part.track_ids).await {
        Ok(playlist_id) => match provider.get_playlist(&playlist_id).await {
            Ok(saved) => {
                let actual = saved
                    .tracks
                    .iter()
                    .map(|v| v.id.as_str())
                    .collect::<Vec<_>>();
                let expected = part
                    .track_ids
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>();
                let (next_state, confirmed, reason) = if actual == expected {
                    (PartState::Succeeded, expected.len(), None)
                } else {
                    (
                        PartState::Partial,
                        common_prefix(&actual, &expected),
                        Some("contentMismatch"),
                    )
                };
                let _ = state.db.transition_export_part(
                    operation_id,
                    &part.server_id,
                    PartState::Creating,
                    next_state,
                    Some(&playlist_id),
                    confirmed,
                    reason,
                );
            }
            Err(_) => {
                let _ = state.db.transition_export_part(
                    operation_id,
                    &part.server_id,
                    PartState::Creating,
                    PartState::Partial,
                    Some(&playlist_id),
                    0,
                    Some("readBackUnavailable"),
                );
            }
        },
        Err(crate::providers::ProviderError::Forbidden)
        | Err(crate::providers::ProviderError::Auth(_)) => {
            let _ = state.db.transition_export_part(
                operation_id,
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
                operation_id,
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
fn common_prefix(actual: &[&str], expected: &[&str]) -> usize {
    actual
        .iter()
        .zip(expected)
        .take_while(|(a, b)| a == b)
        .count()
}

pub(super) async fn export_write(
    state: &AppState,
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
                .transition_export_part(
                    &p.operation_id,
                    &p.server_id,
                    PartState::Failed,
                    PartState::Pending,
                    None,
                    part.confirmed_count.parse().unwrap_or(0),
                    None,
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
            if part.state == PartState::Ambiguous {
                state
                    .db
                    .transition_export_part(
                        &p.operation_id,
                        &p.server_id,
                        PartState::Ambiguous,
                        PartState::Unresolved,
                        part.playlist_id.as_deref(),
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
        let id = operation.operation_id.clone().unwrap();
        for part in operation
            .parts
            .clone()
            .into_iter()
            .filter(|v| v.state == PartState::Pending)
        {
            execute_part(state, &id, part, &operation.name).await;
        }
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
