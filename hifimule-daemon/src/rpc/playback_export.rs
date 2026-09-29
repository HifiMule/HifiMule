use super::*;
use crate::playback::export::*;
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
