//! Durable, source-scoped feedback. No transport or listening-report commands.
use super::model::TrackSource;
use crate::db::Database;
use crate::providers::feedback::{FeedbackCapabilities, Preference};
use crate::providers::{MediaProvider, ProviderError};
use anyhow::{Result, anyhow};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

pub const MAX_PENDING_FEEDBACK: i64 = 128;
pub const MAX_FEEDBACK_ROWS: i64 = 2048;
pub const MAX_SESSION_REJECTIONS: i64 = 10_000;
const RETENTION_SECONDS: i64 = 90 * 24 * 60 * 60;
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(25);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FeedbackQuery {
    pub schema_version: u32,
    pub expected_session_id: String,
    pub occurrence_id: String,
}
impl FeedbackQuery {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1
            || uuid::Uuid::parse_str(&self.expected_session_id).is_err()
            || uuid::Uuid::parse_str(&self.occurrence_id).is_err()
        {
            return Err(anyhow!("INVALID_FEEDBACK_QUERY"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FeedbackWrite {
    pub schema_version: u32,
    pub expected_session_id: String,
    pub occurrence_id: String,
    pub operation_id: String,
    pub value: Preference,
}
impl FeedbackWrite {
    pub fn query(&self) -> FeedbackQuery {
        FeedbackQuery {
            schema_version: self.schema_version,
            expected_session_id: self.expected_session_id.clone(),
            occurrence_id: self.occurrence_id.clone(),
        }
    }
    pub fn validate(&self) -> Result<()> {
        self.query().validate()?;
        if uuid::Uuid::parse_str(&self.operation_id).is_err() {
            return Err(anyhow!("INVALID_FEEDBACK_OPERATION"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackTarget {
    pub session_id: String,
    pub logical_session_id: String,
    pub occurrence_id: String,
    pub source: TrackSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FeedbackOperationStatus {
    Pending,
    Sending,
    Confirmed,
    Failed,
    Ambiguous,
    Conflict,
    Reconciled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FeedbackDiagnostic {
    JournalUnavailable,
    QueueFull,
    RemoteRejected,
    TransportAmbiguous,
    AccountChanged,
    SourceUnavailable,
    PriorWriteUnresolved,
    ValueObserved,
    ProviderUnsupported,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackOperation {
    pub operation_id: String,
    pub sequence: String,
    pub target: FeedbackTarget,
    pub requested_value: Preference,
    pub status: FeedbackOperationStatus,
    pub diagnostic: Option<FeedbackDiagnostic>,
    pub observed_value: Option<Preference>,
    #[serde(skip)]
    pub account_scope: String,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FeedbackReadStatus {
    Known,
    Unknown,
    Unsupported,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackView {
    pub schema_version: u32,
    pub target: FeedbackTarget,
    pub capabilities: FeedbackCapabilities,
    pub preference: Option<Preference>,
    pub read_status: FeedbackReadStatus,
    pub operation: Option<FeedbackOperation>,
    pub rejected: bool,
    pub diagnostic: Option<FeedbackDiagnostic>,
}

fn enum_str<T: Serialize>(value: T) -> String {
    serde_json::to_value(value)
        .expect("enum")
        .as_str()
        .expect("enum string")
        .into()
}
fn from_sql<T: serde::de::DeserializeOwned>(value: String) -> rusqlite::Result<T> {
    serde_json::from_value(serde_json::Value::String(value)).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}
const COLUMNS: &str = "operation_id,sequence,session_id,occurrence_id,server_id,track_id,account_scope,requested_value,status,diagnostic,observed_value,logical_session_id";
fn operation_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<FeedbackOperation> {
    Ok(FeedbackOperation {
        operation_id: row.get(0)?,
        sequence: row.get::<_, i64>(1)?.to_string(),
        target: FeedbackTarget {
            session_id: row.get(2)?,
            logical_session_id: row.get(11)?,
            occurrence_id: row.get(3)?,
            source: TrackSource {
                server_id: row.get(4)?,
                track_id: row.get(5)?,
            },
        },
        account_scope: row.get(6)?,
        requested_value: from_sql(row.get(7)?)?,
        status: from_sql(row.get(8)?)?,
        diagnostic: row.get::<_, Option<String>>(9)?.map(from_sql).transpose()?,
        observed_value: row
            .get::<_, Option<String>>(10)?
            .map(from_sql)
            .transpose()?,
    })
}

impl Database {
    pub fn feedback_operation(&self, id: &str) -> Result<Option<FeedbackOperation>> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        Ok(conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM playback_feedback WHERE operation_id=?1"),
                [id],
                operation_row,
            )
            .optional()?)
    }

    /// Separate transaction: a journal failure must not erase an explicit session rejection.
    pub fn record_feedback_disposition(
        &self,
        target: &FeedbackTarget,
        value: Preference,
    ) -> Result<()> {
        if value == Preference::Neutral {
            return Ok(());
        }
        let mut conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let tx = conn.transaction()?;
        let current: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM playback_sessions WHERE session_id=?1)",
            [&target.session_id],
            |r| r.get(0),
        )?;
        if !current {
            return Err(anyhow!("STALE_FEEDBACK_SESSION"));
        }
        if value == Preference::Like {
            tx.execute("DELETE FROM playback_feedback_dispositions WHERE session_id=?1 AND occurrence_id=?2",params![target.session_id,target.occurrence_id])?;
        } else {
            let count: i64 = tx.query_row(
                "SELECT COUNT(*) FROM playback_feedback_dispositions",
                [],
                |r| r.get(0),
            )?;
            let exists:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM playback_feedback_dispositions WHERE session_id=?1 AND occurrence_id=?2)",params![target.session_id,target.occurrence_id],|r|r.get(0))?;
            if count >= MAX_SESSION_REJECTIONS && !exists {
                return Err(anyhow!("FEEDBACK_DISPOSITION_FULL"));
            }
            tx.execute("INSERT OR IGNORE INTO playback_feedback_dispositions(schema_version,session_id,logical_session_id,occurrence_id,rejected) VALUES(1,?1,?3,?2,1)",params![target.session_id,target.occurrence_id,target.logical_session_id])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn is_playback_occurrence_rejected(&self, session: &str, occurrence: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        Ok(conn.query_row("SELECT EXISTS(SELECT 1 FROM playback_feedback_dispositions WHERE session_id=?1 AND occurrence_id=?2)",params![session,occurrence],|r|r.get(0))?)
    }

    /// Story 16.9 reads exact occurrence identities, including departed previews.
    #[allow(
        dead_code,
        reason = "Public read path for the next listening-snapshot story"
    )]
    pub fn rejected_playback_occurrences(&self, session: &str) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut statement=conn.prepare("SELECT occurrence_id FROM playback_feedback_dispositions WHERE session_id=?1 ORDER BY occurrence_id LIMIT ?2")?;
        Ok(statement
            .query_map(params![session, MAX_SESSION_REJECTIONS], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn feedback_before_read(&self, source: &TrackSource) -> Result<Option<FeedbackOperation>> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        Ok(conn.query_row(&format!("SELECT {COLUMNS} FROM playback_feedback WHERE server_id=?1 AND track_id=?2 ORDER BY sequence DESC LIMIT 1"),params![source.server_id,source.track_id],operation_row).optional()?)
    }
    pub fn record_feedback(
        &self,
        target: &FeedbackTarget,
        account: &str,
        value: Preference,
        operation_id: &str,
    ) -> Result<FeedbackOperation> {
        let mut conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let tx = conn.transaction()?;
        if let Some(row) = tx
            .query_row(
                &format!("SELECT {COLUMNS} FROM playback_feedback WHERE operation_id=?1"),
                [operation_id],
                operation_row,
            )
            .optional()?
        {
            if row.target != *target || row.account_scope != account || row.requested_value != value
            {
                return Err(anyhow!("FEEDBACK_OPERATION_REUSED"));
            }
            return Ok(row);
        }
        // Unresolved rows are never silently discarded to make room.
        tx.execute("DELETE FROM playback_feedback WHERE status IN ('confirmed','failed','reconciled') AND updated_at<unixepoch()-?1",[RETENTION_SECONDS])?;
        let total: i64 =
            tx.query_row("SELECT COUNT(*) FROM playback_feedback", [], |r| r.get(0))?;
        if total >= MAX_FEEDBACK_ROWS {
            tx.execute("DELETE FROM playback_feedback WHERE sequence IN (SELECT sequence FROM playback_feedback WHERE status IN ('confirmed','failed','reconciled') ORDER BY sequence LIMIT ?1)",[total-MAX_FEEDBACK_ROWS+1])?;
        }
        let total: i64 =
            tx.query_row("SELECT COUNT(*) FROM playback_feedback", [], |r| r.get(0))?;
        let pending: i64 = tx.query_row(
            "SELECT COUNT(*) FROM playback_feedback WHERE status IN ('pending','sending')",
            [],
            |r| r.get(0),
        )?;
        if total >= MAX_FEEDBACK_ROWS || pending >= MAX_PENDING_FEEDBACK {
            return Err(anyhow!("FEEDBACK_QUEUE_FULL"));
        }
        let conflict:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM playback_feedback WHERE server_id=?1 AND track_id=?2 AND account_scope=?3 AND status IN ('ambiguous','conflict'))",params![target.source.server_id,target.source.track_id,account],|r|r.get(0))?;
        tx.execute("INSERT INTO playback_feedback(operation_id,schema_version,session_id,occurrence_id,server_id,track_id,account_scope,requested_value,status,diagnostic,logical_session_id) VALUES(?1,1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![operation_id,target.session_id,target.occurrence_id,target.source.server_id,target.source.track_id,account,enum_str(value),if conflict {"conflict"} else {"pending"},if conflict {Some("priorWriteUnresolved")} else {None},target.logical_session_id])?;
        let result = tx.query_row(
            &format!("SELECT {COLUMNS} FROM playback_feedback WHERE operation_id=?1"),
            [operation_id],
            operation_row,
        )?;
        tx.commit()?;
        Ok(result)
    }

    pub fn latest_feedback(
        &self,
        source: &TrackSource,
        account: &str,
    ) -> Result<Option<FeedbackOperation>> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        Ok(conn.query_row(&format!("SELECT {COLUMNS} FROM playback_feedback WHERE server_id=?1 AND track_id=?2 AND account_scope=?3 ORDER BY sequence DESC LIMIT 1"),params![source.server_id,source.track_id,account],operation_row).optional()?)
    }

    pub fn claim_feedback(&self) -> Result<Option<FeedbackOperation>> {
        let mut conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let tx = conn.transaction()?;
        // A single worker also bounds load across sources. Atomic claim protects against duplicate workers.
        let sending: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM playback_feedback WHERE status='sending')",
            [],
            |r| r.get(0),
        )?;
        if sending {
            return Ok(None);
        }
        let next:Option<String>=tx.query_row("SELECT operation_id FROM playback_feedback p WHERE status='pending' AND NOT EXISTS(SELECT 1 FROM playback_feedback old WHERE old.server_id=p.server_id AND old.track_id=p.track_id AND old.account_scope=p.account_scope AND old.sequence<p.sequence AND old.status IN ('pending','sending','ambiguous','conflict')) ORDER BY sequence LIMIT 1",[],|r|r.get(0)).optional()?;
        let Some(id) = next else {
            return Ok(None);
        };
        tx.execute("UPDATE playback_feedback SET status='sending',updated_at=unixepoch() WHERE operation_id=?1 AND status='pending'",[&id])?;
        let result = tx.query_row(
            &format!("SELECT {COLUMNS} FROM playback_feedback WHERE operation_id=?1"),
            [&id],
            operation_row,
        )?;
        tx.commit()?;
        Ok(Some(result))
    }

    pub fn settle_feedback(
        &self,
        operation_id: &str,
        status: FeedbackOperationStatus,
        diagnostic: Option<FeedbackDiagnostic>,
    ) -> Result<bool> {
        if !matches!(
            status,
            FeedbackOperationStatus::Confirmed
                | FeedbackOperationStatus::Failed
                | FeedbackOperationStatus::Ambiguous
        ) {
            return Err(anyhow!("INVALID_FEEDBACK_SETTLEMENT"));
        }
        let mut conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let tx = conn.transaction()?;
        let changed=tx.execute("UPDATE playback_feedback SET status=?2,diagnostic=?3,updated_at=unixepoch() WHERE operation_id=?1 AND status='sending'",params![operation_id,enum_str(status),diagnostic.map(enum_str)])?;
        if changed == 1 && status == FeedbackOperationStatus::Ambiguous {
            tx.execute("UPDATE playback_feedback SET status='conflict',diagnostic='priorWriteUnresolved',updated_at=unixepoch() WHERE status='pending' AND (server_id,track_id,account_scope)=(SELECT server_id,track_id,account_scope FROM playback_feedback WHERE operation_id=?1)",[operation_id])?;
        }
        tx.commit()?;
        Ok(changed == 1)
    }

    pub fn recover_feedback(&self) -> Result<()> {
        let mut conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let tx = conn.transaction()?;
        tx.execute("UPDATE playback_feedback SET status='ambiguous',diagnostic='transportAmbiguous',updated_at=unixepoch() WHERE status='sending'",[])?;
        tx.execute("UPDATE playback_feedback SET status='conflict',diagnostic='priorWriteUnresolved' WHERE status='pending' AND (server_id,track_id,account_scope) IN (SELECT server_id,track_id,account_scope FROM playback_feedback WHERE status='ambiguous')",[])?;
        tx.commit()?;
        Ok(())
    }

    /// Explicit refresh establishes current value, never claims that an uncertain operation caused it.
    /// Queued opposing operations are resolved as observations, never replayed automatically.
    pub fn reconcile_feedback(
        &self,
        source: &TrackSource,
        account: &str,
        before: &FeedbackOperation,
        value: Preference,
    ) -> Result<()> {
        if !matches!(
            before.status,
            FeedbackOperationStatus::Ambiguous | FeedbackOperationStatus::Conflict
        ) {
            return Ok(());
        }
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute("UPDATE playback_feedback SET status='reconciled',diagnostic='valueObserved',observed_value=?4,updated_at=unixepoch() WHERE server_id=?1 AND track_id=?2 AND account_scope=?3 AND status IN ('ambiguous','conflict') AND ?5=(SELECT MAX(sequence) FROM playback_feedback WHERE server_id=?1 AND track_id=?2 AND account_scope=?3) AND NOT EXISTS(SELECT 1 FROM playback_feedback WHERE server_id=?1 AND track_id=?2 AND account_scope=?3 AND status IN ('pending','sending'))",
            params![source.server_id,source.track_id,account,enum_str(value),before.sequence.parse::<i64>()?])?;
        Ok(())
    }
}

fn classify(error: &ProviderError) -> (FeedbackOperationStatus, FeedbackDiagnostic) {
    match error {
        ProviderError::UnsupportedCapability(_) => (
            FeedbackOperationStatus::Failed,
            FeedbackDiagnostic::ProviderUnsupported,
        ),
        ProviderError::Auth(_)
        | ProviderError::NotFound { .. }
        | ProviderError::Http {
            status: Some(400 | 401 | 403 | 404 | 405 | 422),
            ..
        } => (
            FeedbackOperationStatus::Failed,
            FeedbackDiagnostic::RemoteRejected,
        ),
        _ => (
            FeedbackOperationStatus::Ambiguous,
            FeedbackDiagnostic::TransportAmbiguous,
        ),
    }
}

pub async fn deliver_feedback(
    db: &Database,
    row: &FeedbackOperation,
    provider: &dyn MediaProvider,
    shutdown: &AtomicBool,
) -> Result<()> {
    let before = tokio::time::timeout(
        REQUEST_TIMEOUT,
        provider.read_feedback(&row.target.source.track_id),
    )
    .await;
    let verified = match before {
        Ok(Ok(value)) => value,
        _ => {
            db.settle_feedback(
                &row.operation_id,
                FeedbackOperationStatus::Failed,
                Some(FeedbackDiagnostic::SourceUnavailable),
            )?;
            return Ok(());
        }
    };
    if verified.account_scope != row.account_scope
        || !verified.capabilities.supports(row.requested_value)
    {
        db.settle_feedback(
            &row.operation_id,
            FeedbackOperationStatus::Failed,
            Some(FeedbackDiagnostic::AccountChanged),
        )?;
        return Ok(());
    }
    if shutdown.load(Ordering::Acquire) {
        db.settle_feedback(
            &row.operation_id,
            FeedbackOperationStatus::Failed,
            Some(FeedbackDiagnostic::SourceUnavailable),
        )?;
        return Ok(());
    }
    let result = tokio::time::timeout(
        REQUEST_TIMEOUT,
        provider.set_feedback(&row.target.source.track_id, row.requested_value),
    )
    .await;
    let (status, diagnostic) = match result {
        Ok(Ok(())) => {
            match tokio::time::timeout(
                REQUEST_TIMEOUT,
                provider.read_feedback(&row.target.source.track_id),
            )
            .await
            {
                Ok(Ok(after))
                    if after.account_scope == row.account_scope
                        && after.value == row.requested_value =>
                {
                    (FeedbackOperationStatus::Confirmed, None)
                }
                _ => (
                    FeedbackOperationStatus::Ambiguous,
                    Some(FeedbackDiagnostic::TransportAmbiguous),
                ),
            }
        }
        Ok(Err(error)) => {
            let (status, diagnostic) = classify(&error);
            (status, Some(diagnostic))
        }
        Err(_) => (
            FeedbackOperationStatus::Ambiguous,
            Some(FeedbackDiagnostic::TransportAmbiguous),
        ),
    };
    db.settle_feedback(&row.operation_id, status, diagnostic)?;
    Ok(())
}

pub async fn run_feedback(
    db: Arc<Database>,
    manager: Arc<tokio::sync::RwLock<crate::server_manager::ServerManager>>,
    shutdown: Arc<AtomicBool>,
) {
    if db.recover_feedback().is_err() {
        eprintln!("[Feedback] recovery unavailable");
        return;
    }
    while !shutdown.load(Ordering::Acquire) {
        match db.claim_feedback() {
            Ok(Some(row)) => {
                let provider = tokio::time::timeout(
                    REQUEST_TIMEOUT,
                    crate::server_manager::get_provider_by_server_id(
                        &manager,
                        &db,
                        &row.target.source.server_id,
                    ),
                )
                .await;
                match provider {
                    Ok(Ok(provider)) if !shutdown.load(Ordering::Acquire) => {
                        if deliver_feedback(&db, &row, provider.as_ref(), &shutdown)
                            .await
                            .is_err()
                        {
                            eprintln!("[Feedback] settlement unavailable");
                            // The delivery future has ended. Recover its unsettled row as uncertain;
                            // retry only this local persistence step, never the remote request.
                            while !shutdown.load(Ordering::Acquire)
                                && db.recover_feedback().is_err()
                            {
                                tokio::time::sleep(Duration::from_millis(500)).await;
                            }
                        }
                    }
                    _ => {
                        let _ = db.settle_feedback(
                            &row.operation_id,
                            FeedbackOperationStatus::Failed,
                            Some(FeedbackDiagnostic::SourceUnavailable),
                        );
                    }
                }
            }
            Ok(None) => {}
            Err(_) => eprintln!("[Feedback] journal unavailable"),
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn target(server: &str, occurrence: &str) -> FeedbackTarget {
        FeedbackTarget {
            session_id: "session".into(),
            logical_session_id: "session".into(),
            occurrence_id: occurrence.into(),
            source: super::super::model::TrackSource {
                server_id: server.into(),
                track_id: "shared-track".into(),
            },
        }
    }
    #[test]
    fn feedback_requests_are_versioned_and_bound_to_exact_occurrences() {
        let p: FeedbackQuery=serde_json::from_value(serde_json::json!({"schemaVersion":1,"expectedSessionId":uuid::Uuid::new_v4().to_string(),"occurrenceId":uuid::Uuid::new_v4().to_string()})).unwrap();
        assert!(p.validate().is_ok());
        assert!(
            FeedbackQuery {
                schema_version: 2,
                ..p.clone()
            }
            .validate()
            .is_err()
        );
        assert!(
            FeedbackQuery {
                occurrence_id: "x".repeat(1025),
                ..p
            }
            .validate()
            .is_err()
        );
    }
    #[test]
    fn feedback_journal_serializes_opposing_intents_and_fences_old_completions() {
        let db = Database::memory().unwrap();
        db.init_playback().unwrap();
        let a = target("server-a", "occurrence-a");
        let first = db
            .record_feedback(&a, "account", Preference::Dislike, "op1")
            .unwrap();
        let second = db
            .record_feedback(&a, "account", Preference::Like, "op2")
            .unwrap();
        assert!(second.sequence.parse::<u64>().unwrap() > first.sequence.parse::<u64>().unwrap());
        assert_eq!(db.claim_feedback().unwrap().unwrap().operation_id, "op1");
        assert!(db.claim_feedback().unwrap().is_none());
        db.settle_feedback("op1", FeedbackOperationStatus::Confirmed, None)
            .unwrap();
        assert_eq!(
            db.latest_feedback(&a.source, "account")
                .unwrap()
                .unwrap()
                .operation_id,
            "op2"
        );
        assert_eq!(db.claim_feedback().unwrap().unwrap().operation_id, "op2");
        assert!(
            !db.settle_feedback("op1", FeedbackOperationStatus::Failed, None)
                .unwrap()
        );
    }
    #[test]
    fn feedback_crash_ambiguity_blocks_opposed_replay_but_not_another_source() {
        let db = Database::memory().unwrap();
        db.init_playback().unwrap();
        let a = target("server-a", "occurrence-a");
        db.record_feedback(&a, "account", Preference::Dislike, "op1")
            .unwrap();
        db.claim_feedback().unwrap();
        db.record_feedback(&a, "account", Preference::Like, "op2")
            .unwrap();
        db.recover_feedback().unwrap();
        assert!(db.claim_feedback().unwrap().is_none());
        assert_eq!(
            db.latest_feedback(&a.source, "account")
                .unwrap()
                .unwrap()
                .status,
            FeedbackOperationStatus::Conflict
        );
        let b = target("server-b", "occurrence-b");
        db.record_feedback(&b, "account", Preference::Like, "op3")
            .unwrap();
        assert_eq!(db.claim_feedback().unwrap().unwrap().operation_id, "op3");
    }

    #[test]
    fn feedback_reconciliation_observes_remote_value_without_replaying_queued_intent() {
        let db = Database::memory().unwrap();
        db.init_playback().unwrap();
        let a = target("server-a", "occurrence-a");
        db.record_feedback(&a, "account", Preference::Dislike, "op1")
            .unwrap();
        db.claim_feedback().unwrap();
        db.record_feedback(&a, "account", Preference::Like, "op2")
            .unwrap();
        db.recover_feedback().unwrap();
        let before = db.latest_feedback(&a.source, "account").unwrap().unwrap();
        db.reconcile_feedback(&a.source, "account", &before, Preference::Dislike)
            .unwrap();
        let row = db.latest_feedback(&a.source, "account").unwrap().unwrap();
        assert_eq!(row.status, FeedbackOperationStatus::Reconciled);
        assert_eq!(row.requested_value, Preference::Like);
        assert_eq!(row.observed_value, Some(Preference::Dislike));
        assert!(db.claim_feedback().unwrap().is_none());
        // Only a fresh explicit action after observing the current value can send again.
        db.record_feedback(&a, "account", Preference::Like, "op3")
            .unwrap();
        assert_eq!(db.claim_feedback().unwrap().unwrap().operation_id, "op3");
    }

    #[test]
    fn feedback_journal_bounds_pending_work_and_deduplicates_operation_identity() {
        let db = Database::memory().unwrap();
        db.init_playback().unwrap();
        let a = target("server-a", "occurrence-a");
        let first = db
            .record_feedback(&a, "account", Preference::Like, "same")
            .unwrap();
        assert_eq!(
            first.sequence,
            db.record_feedback(&a, "account", Preference::Like, "same")
                .unwrap()
                .sequence
        );
        assert!(
            db.record_feedback(&a, "account", Preference::Dislike, "same")
                .is_err()
        );
        for n in 1..MAX_PENDING_FEEDBACK {
            db.record_feedback(&a, "account", Preference::Like, &format!("op{n}"))
                .unwrap();
        }
        assert!(
            db.record_feedback(&a, "account", Preference::Like, "overflow")
                .is_err()
        );
    }

    #[test]
    fn feedback_file_restart_preserves_unsent_work_and_only_reconciles_the_read_watermark() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("feedback.db");
        let a = target("server-a", "a");
        let b = target("server-b", "b");
        {
            let db = Database::new(path.clone()).unwrap();
            db.init_playback().unwrap();
            db.record_feedback(&a, "account", Preference::Dislike, "sending")
                .unwrap();
            db.claim_feedback().unwrap();
            db.record_feedback(&a, "account", Preference::Like, "opposed")
                .unwrap();
            db.record_feedback(&b, "account", Preference::Like, "unsent")
                .unwrap();
        }
        let db = Database::new(path).unwrap();
        db.init_playback().unwrap();
        db.recover_feedback().unwrap();
        let before = db.latest_feedback(&a.source, "account").unwrap().unwrap();
        assert_eq!(db.claim_feedback().unwrap().unwrap().operation_id, "unsent");
        db.record_feedback(&a, "account", Preference::Neutral, "newer")
            .unwrap();
        db.reconcile_feedback(&a.source, "account", &before, Preference::Dislike)
            .unwrap();
        assert_eq!(
            db.latest_feedback(&a.source, "account")
                .unwrap()
                .unwrap()
                .status,
            FeedbackOperationStatus::Conflict
        );
        assert_eq!(
            db.feedback_operation("sending").unwrap().unwrap().status,
            FeedbackOperationStatus::Ambiguous
        );
    }

    #[test]
    fn feedback_retention_prunes_terminal_rows_but_preserves_uncertainty_and_account_scope() {
        let db = Database::memory().unwrap();
        db.init_playback().unwrap();
        let a = target("a", "a");
        db.record_feedback(&a, "account", Preference::Like, "old-confirmed")
            .unwrap();
        db.claim_feedback().unwrap();
        db.settle_feedback("old-confirmed", FeedbackOperationStatus::Confirmed, None)
            .unwrap();
        db.record_feedback(&a, "account", Preference::Dislike, "uncertain")
            .unwrap();
        db.claim_feedback().unwrap();
        db.recover_feedback().unwrap();
        db.conn
            .lock()
            .unwrap()
            .execute("UPDATE playback_feedback SET updated_at=0", [])
            .unwrap();
        db.record_feedback(&a, "other-account", Preference::Like, "new-account")
            .unwrap();
        assert!(db.feedback_operation("old-confirmed").unwrap().is_none());
        assert_eq!(
            db.feedback_operation("uncertain").unwrap().unwrap().status,
            FeedbackOperationStatus::Ambiguous
        );
        assert_eq!(
            db.claim_feedback().unwrap().unwrap().operation_id,
            "new-account"
        );
        // Fill only unresolved rows. Capacity must fail, never erase these rows.
        let conn = db.conn.lock().unwrap();
        conn.execute("WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<?1) INSERT INTO playback_feedback(operation_id,schema_version,session_id,logical_session_id,occurrence_id,server_id,track_id,account_scope,requested_value,status) SELECT 'filler-'||x,1,'s','s','o','a','t','account','like','conflict' FROM n",[MAX_FEEDBACK_ROWS-2]).unwrap();
        drop(conn);
        assert!(
            db.record_feedback(&a, "account", Preference::Like, "overflow")
                .is_err()
        );
        assert_eq!(
            db.feedback_operation("uncertain").unwrap().unwrap().status,
            FeedbackOperationStatus::Ambiguous
        );
    }
}
