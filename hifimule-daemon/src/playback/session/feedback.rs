use super::super::feedback::{
    FeedbackDiagnostic, FeedbackOperation, FeedbackOperationStatus, FeedbackQuery, FeedbackTarget,
    FeedbackWrite,
};
use super::*;
use crate::providers::feedback::ProviderFeedback;

pub(super) struct Verification {
    target: FeedbackTarget,
    remote: ProviderFeedback,
    at: std::time::Instant,
}

impl PlaybackSession {
    /// One short-lived capability grant, never used as displayed preference evidence.
    /// A click can be journaled even if the source goes offline after the control appeared.
    pub fn remember_feedback_verification(&self, target: FeedbackTarget, remote: ProviderFeedback) {
        *self
            .feedback_verification
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(Verification {
            target,
            remote,
            at: std::time::Instant::now(),
        });
    }
    pub fn feedback_verification(&self, target: &FeedbackTarget) -> Option<ProviderFeedback> {
        self.feedback_verification
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .filter(|grant| {
                grant.target == *target && grant.at.elapsed() < std::time::Duration::from_secs(60)
            })
            .map(|grant| grant.remote.clone())
    }
    pub fn forget_feedback_verification(&self, target: &FeedbackTarget) {
        let mut grant = self
            .feedback_verification
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if grant.as_ref().is_some_and(|grant| grant.target == *target) {
            *grant = None;
        }
    }
    pub fn feedback_target(&self, query: FeedbackQuery) -> PResult<FeedbackTarget> {
        let (tx, rx) = mpsc::channel();
        self.command_tx
            .try_send(OwnerCommand::FeedbackTarget(query, tx))
            .map_err(admission_error)?;
        rx.recv().unwrap_or_else(|_| Err(owner_stopped()))
    }
    pub fn accept_feedback(
        &self,
        target: FeedbackTarget,
        verified: ProviderFeedback,
        write: FeedbackWrite,
        guard: Option<crate::sync::MutationGuard>,
    ) -> PResult<FeedbackOperation> {
        let (tx, rx) = mpsc::channel();
        self.command_tx
            .try_send(OwnerCommand::AcceptFeedback(
                target, verified, write, guard, tx,
            ))
            .map_err(admission_error)?;
        rx.recv().unwrap_or_else(|_| Err(owner_stopped()))
    }
}

pub(super) fn target(inner: &Inner, query: &FeedbackQuery) -> PResult<FeedbackTarget> {
    query
        .validate()
        .map_err(|_| PlaybackError::invalid("INVALID_FEEDBACK_QUERY", "Invalid feedback query"))?;
    let state = snapshot(inner)?;
    let occurrence = state
        .current
        .filter(|occurrence| {
            state.session_id == query.expected_session_id
                && occurrence.occurrence_id == query.occurrence_id
        })
        .ok_or_else(|| {
            PlaybackError::conflict("STALE_FEEDBACK_OCCURRENCE", "Feedback occurrence changed")
        })?;
    Ok(FeedbackTarget {
        logical_session_id: state
            .radio
            .as_ref()
            .map(|radio| radio.logical_id.clone())
            .unwrap_or_else(|| state.session_id.clone()),
        session_id: state.session_id,
        occurrence_id: occurrence.occurrence_id,
        source: occurrence.source,
    })
}

pub(super) fn accept(
    inner: &Inner,
    frozen: FeedbackTarget,
    verified: ProviderFeedback,
    write: FeedbackWrite,
) -> PResult<FeedbackOperation> {
    write.validate().map_err(|_| {
        PlaybackError::invalid("INVALID_FEEDBACK_OPERATION", "Invalid feedback operation")
    })?;
    if target(inner, &write.query())? != frozen {
        return Err(PlaybackError::conflict(
            "STALE_FEEDBACK_OCCURRENCE",
            "Feedback source changed",
        ));
    }
    if !verified.capabilities.supports(write.value) {
        return Err(PlaybackError::invalid(
            "FEEDBACK_UNSUPPORTED",
            "Preference unavailable on source",
        ));
    }
    if let Ok(Some(existing)) = inner.db.feedback_operation(&write.operation_id) {
        if existing.target != frozen
            || existing.requested_value != write.value
            || existing.account_scope != verified.account_scope
        {
            return Err(PlaybackError::invalid(
                "FEEDBACK_OPERATION_REUSED",
                "Operation identity already used",
            ));
        }
        return Ok(existing);
    }
    inner
        .db
        .record_feedback_disposition(&frozen, write.value)
        .map_err(|_| {
            PlaybackError::invalid(
                "FEEDBACK_DISPOSITION_UNAVAILABLE",
                "Session preference could not be saved",
            )
        })?;
    match inner.db.record_feedback(
        &frozen,
        &verified.account_scope,
        write.value,
        &write.operation_id,
    ) {
        Ok(row) => Ok(row),
        Err(error) if error.to_string() == "FEEDBACK_OPERATION_REUSED" => {
            Err(PlaybackError::invalid(
                "FEEDBACK_OPERATION_REUSED",
                "Operation identity already used",
            ))
        }
        Err(error) => Ok(FeedbackOperation {
            operation_id: write.operation_id,
            sequence: "0".into(),
            target: frozen,
            requested_value: write.value,
            status: FeedbackOperationStatus::Failed,
            diagnostic: Some(if error.to_string() == "FEEDBACK_QUEUE_FULL" {
                FeedbackDiagnostic::QueueFull
            } else {
                FeedbackDiagnostic::JournalUnavailable
            }),
            observed_value: None,
            account_scope: verified.account_scope,
        }),
    }
}
