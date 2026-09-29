use super::*;
use crate::playback::feedback::*;
use crate::providers::feedback::FeedbackCapabilities;

static PERMITS: std::sync::LazyLock<Arc<tokio::sync::Semaphore>> =
    std::sync::LazyLock::new(|| Arc::new(tokio::sync::Semaphore::new(4)));
fn error(code: &'static str) -> JsonRpcError {
    JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Playback feedback unavailable".into(),
        data: Some(serde_json::json!({"code":code})),
    }
}

async fn target(state: &AppState, query: FeedbackQuery) -> Result<FeedbackTarget, JsonRpcError> {
    let playback = state.playback.clone();
    tokio::task::spawn_blocking(move || playback.feedback_target(query))
        .await
        .map_err(playback_task_error)?
        .map_err(playback_error)
}

fn read_is_current(
    before: Option<&FeedbackOperation>,
    after: Option<&FeedbackOperation>,
    account: &str,
) -> bool {
    match (before.filter(|row| row.account_scope == account), after) {
        (None, None) => true,
        (Some(before), Some(after)) => {
            before.sequence == after.sequence
                && (before.status == after.status
                    || (matches!(
                        before.status,
                        FeedbackOperationStatus::Ambiguous | FeedbackOperationStatus::Conflict
                    ) && after.status == FeedbackOperationStatus::Reconciled))
        }
        _ => false,
    }
}

pub(super) async fn read(state: &AppState, params: Option<Value>) -> Result<Value, JsonRpcError> {
    let query: FeedbackQuery = serde_json::from_value(params.unwrap_or(Value::Null))
        .map_err(|_| error("INVALID_FEEDBACK_QUERY"))?;
    query
        .validate()
        .map_err(|_| error("INVALID_FEEDBACK_QUERY"))?;
    let _permit = PERMITS
        .clone()
        .try_acquire_owned()
        .map_err(|_| error("FEEDBACK_BUSY"))?;
    let frozen = target(state, query.clone()).await?;
    let before = state
        .db
        .feedback_before_read(&frozen.source)
        .map_err(|_| error("FEEDBACK_JOURNAL_UNAVAILABLE"))?;
    let mut view = FeedbackView {
        schema_version: 1,
        target: frozen.clone(),
        capabilities: FeedbackCapabilities::default(),
        preference: None,
        read_status: FeedbackReadStatus::Unknown,
        operation: None,
        rejected: false,
        diagnostic: Some(FeedbackDiagnostic::SourceUnavailable),
    };
    let result = tokio::time::timeout(REQUEST_TIMEOUT, async {
        let provider = get_provider_by_server_id_for(state, &frozen.source.server_id)
            .await
            .map_err(|_| ())?;
        Ok::<_, ()>(provider.read_feedback(&frozen.source.track_id).await)
    })
    .await;
    match result {
        Ok(Ok(Ok(remote))) => {
            if let Some(before) = before
                .as_ref()
                .filter(|before| before.account_scope == remote.account_scope)
            {
                state
                    .db
                    .reconcile_feedback(&frozen.source, &remote.account_scope, before, remote.value)
                    .map_err(|_| error("FEEDBACK_JOURNAL_UNAVAILABLE"))?;
            }
            view.operation = state
                .db
                .latest_feedback(&frozen.source, &remote.account_scope)
                .map_err(|_| error("FEEDBACK_JOURNAL_UNAVAILABLE"))?;
            view.capabilities = remote.capabilities;
            view.preference = Some(remote.value);
            view.read_status = FeedbackReadStatus::Known;
            view.diagnostic = None;
            if !read_is_current(
                before.as_ref(),
                view.operation.as_ref(),
                &remote.account_scope,
            ) {
                view.preference = None;
                view.read_status = FeedbackReadStatus::Unknown;
            }
            state
                .playback
                .remember_feedback_verification(frozen.clone(), remote);
        }
        Ok(Ok(Err(ProviderError::UnsupportedCapability(_)))) => {
            state.playback.forget_feedback_verification(&frozen);
            view.read_status = FeedbackReadStatus::Unsupported;
            view.diagnostic = Some(FeedbackDiagnostic::ProviderUnsupported);
        }
        _ => {
            view.operation = before;
        }
    }
    if target(state, query).await? != frozen {
        return Err(error("STALE_FEEDBACK_OCCURRENCE"));
    }
    view.rejected = state
        .db
        .is_playback_occurrence_rejected(&frozen.session_id, &frozen.occurrence_id)
        .map_err(|_| error("FEEDBACK_DISPOSITION_UNAVAILABLE"))?;
    Ok(serde_json::json!({"data":view}))
}

pub(super) async fn write(
    state: &AppState,
    params: Option<Value>,
    guard: Option<crate::sync::MutationGuard>,
) -> Result<Value, JsonRpcError> {
    let write: FeedbackWrite = serde_json::from_value(params.unwrap_or(Value::Null))
        .map_err(|_| error("INVALID_FEEDBACK_OPERATION"))?;
    write
        .validate()
        .map_err(|_| error("INVALID_FEEDBACK_OPERATION"))?;
    let _permit = PERMITS
        .clone()
        .try_acquire_owned()
        .map_err(|_| error("FEEDBACK_BUSY"))?;
    let frozen = target(state, write.query()).await?;
    let remote = if let Some(verified) = state.playback.feedback_verification(&frozen) {
        verified
    } else {
        tokio::time::timeout(REQUEST_TIMEOUT, async {
            let provider = get_provider_by_server_id_for(state, &frozen.source.server_id)
                .await
                .map_err(|_| error("FEEDBACK_SOURCE_UNAVAILABLE"))?;
            provider
                .read_feedback(&frozen.source.track_id)
                .await
                .map_err(|error_value| match error_value {
                    ProviderError::UnsupportedCapability(_) => error("FEEDBACK_UNSUPPORTED"),
                    _ => error("FEEDBACK_SOURCE_UNAVAILABLE"),
                })
        })
        .await
        .map_err(|_| error("FEEDBACK_SOURCE_UNAVAILABLE"))??
    };
    if !remote.capabilities.supports(write.value) {
        return Err(error("FEEDBACK_UNSUPPORTED"));
    }
    let playback = state.playback.clone();
    let row =
        tokio::task::spawn_blocking(move || playback.accept_feedback(frozen, remote, write, guard))
            .await
            .map_err(playback_task_error)?
            .map_err(playback_error)?;
    Ok(serde_json::json!({"data":row}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::feedback::{Preference, ProviderFeedback};
    #[test]
    fn feedback_rpc_read_cannot_publish_a_value_read_before_new_admission_or_settlement() {
        let db = crate::db::Database::memory().unwrap();
        db.init_playback().unwrap();
        let target = FeedbackTarget {
            session_id: "session".into(),
            logical_session_id: "session".into(),
            occurrence_id: "occ".into(),
            source: crate::playback::model::TrackSource {
                server_id: "server".into(),
                track_id: "track".into(),
            },
        };
        let pending = db
            .record_feedback(&target, "account", Preference::Like, "op1")
            .unwrap();
        assert!(!read_is_current(None, Some(&pending), "account"));
        let sending = db.claim_feedback().unwrap().unwrap();
        assert!(!read_is_current(Some(&pending), Some(&sending), "account"));
        db.recover_feedback().unwrap();
        let ambiguous = db.feedback_operation("op1").unwrap().unwrap();
        db.reconcile_feedback(&target.source, "account", &ambiguous, Preference::Neutral)
            .unwrap();
        let reconciled = db.feedback_operation("op1").unwrap().unwrap();
        assert!(read_is_current(
            Some(&ambiguous),
            Some(&reconciled),
            "account"
        ));
        let newer = db
            .record_feedback(&target, "account", Preference::Dislike, "op2")
            .unwrap();
        assert!(!read_is_current(Some(&ambiguous), Some(&newer), "account"));
        assert!(read_is_current(Some(&ambiguous), None, "different-account"));
    }
    #[tokio::test]
    async fn feedback_rpc_rejects_unsupported_negative_and_accepts_verified_offline_intent() {
        use crate::playback::model::{ApplySessionParams, SessionOperation, TrackSource};
        let state =
            super::super::tests::make_test_state(Arc::new(crate::db::Database::memory().unwrap()));
        let initial = state.playback.snapshot().unwrap();
        state
            .playback
            .apply(ApplySessionParams {
                schema_version: 1,
                session_id: initial.session_id,
                instance_id: initial.instance_id,
                expected_queue_revision: initial.queue_revision,
                command_id: uuid::Uuid::new_v4().to_string(),
                operation: SessionOperation::ReplaceQueue {
                    sources: vec![TrackSource {
                        server_id: "offline".into(),
                        track_id: "same-track".into(),
                    }],
                },
            })
            .unwrap();
        let current = state.playback.snapshot().unwrap();
        let occurrence = current.current.as_ref().unwrap().occurrence_id.clone();
        let frozen = state
            .playback
            .feedback_target(FeedbackQuery {
                schema_version: 1,
                expected_session_id: current.session_id.clone(),
                occurrence_id: occurrence.clone(),
            })
            .unwrap();
        let request = serde_json::json!({"schemaVersion":1,"expectedSessionId":current.session_id,"occurrenceId":occurrence,"operationId":uuid::Uuid::new_v4().to_string(),"value":"dislike"});
        let mut remote = ProviderFeedback {
            capabilities: FeedbackCapabilities::favorites(),
            value: Preference::Neutral,
            account_scope: "account".into(),
        };
        state
            .playback
            .remember_feedback_verification(frozen.clone(), remote.clone());
        assert!(
            write(&state, Some(request.clone()), None)
                .await
                .unwrap_err()
                .data
                .unwrap()
                .to_string()
                .contains("FEEDBACK_UNSUPPORTED")
        );
        assert!(
            !state
                .db
                .is_playback_occurrence_rejected(&current.session_id, &occurrence)
                .unwrap()
        );
        remote.capabilities = FeedbackCapabilities::ratings();
        state
            .playback
            .remember_feedback_verification(frozen, remote);
        let result = write(&state, Some(request), None).await.unwrap();
        assert_eq!(result["data"]["status"], "pending");
        assert!(
            state
                .db
                .is_playback_occurrence_rejected(&current.session_id, &occurrence)
                .unwrap()
        );
        assert_eq!(
            state.playback.snapshot().unwrap().generation_id,
            current.generation_id
        );
        assert!(
            state
                .db
                .list_live_reports(&current.session_id, 50)
                .unwrap()
                .is_empty()
        );
        state.playback.stop_and_join().unwrap();
    }
    #[tokio::test]
    async fn feedback_rpc_rejects_invalid_schema_and_stale_occurrence_before_provider_lookup() {
        let state =
            super::super::tests::make_test_state(Arc::new(crate::db::Database::memory().unwrap()));
        let session = state.playback.snapshot().unwrap();
        let query = serde_json::json!({"schemaVersion":1,"expectedSessionId":session.session_id,"occurrenceId":uuid::Uuid::new_v4().to_string()});
        assert!(
            read(&state, Some(query.clone()))
                .await
                .unwrap_err()
                .data
                .unwrap()
                .to_string()
                .contains("STALE_FEEDBACK_OCCURRENCE")
        );
        let mut mutation = query.clone();
        mutation["value"] = serde_json::json!("dislike");
        mutation["operationId"] = serde_json::json!(uuid::Uuid::new_v4().to_string());
        assert!(
            write(&state, Some(mutation.clone()), None)
                .await
                .unwrap_err()
                .data
                .unwrap()
                .to_string()
                .contains("STALE_FEEDBACK_OCCURRENCE")
        );
        mutation["schemaVersion"] = serde_json::json!(2);
        assert!(
            write(&state, Some(mutation), None)
                .await
                .unwrap_err()
                .data
                .unwrap()
                .to_string()
                .contains("INVALID_FEEDBACK_OPERATION")
        );
        assert!(state.db.claim_feedback().unwrap().is_none());
        state.playback.stop_and_join().unwrap();
    }
}
