//! Daemon-owned evidence and delivery for source-scoped live listening reports.

use crate::db::Database;
use anyhow::{Result, anyhow};
use rusqlite::{OptionalExtension, params};
use serde::Serialize;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use uuid::Uuid;

const MAX_PENDING_REPORTS: i64 = 256;
const MAX_REPORT_ROWS: i64 = 2048;
const REPORT_RETENTION_SECONDS: i64 = 90 * 24 * 60 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LiveReportStatus {
    Ineligible,
    Pending,
    Sending,
    Confirmed,
    Failed,
    Ambiguous,
    Unsupported,
}

impl LiveReportStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ineligible => "ineligible",
            Self::Pending => "pending",
            Self::Sending => "sending",
            Self::Confirmed => "confirmed",
            Self::Failed => "failed",
            Self::Ambiguous => "ambiguous",
            Self::Unsupported => "unsupported",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        Ok(match value {
            "ineligible" => Self::Ineligible,
            "pending" => Self::Pending,
            "sending" => Self::Sending,
            "confirmed" => Self::Confirmed,
            "failed" => Self::Failed,
            "ambiguous" => Self::Ambiguous,
            "unsupported" => Self::Unsupported,
            _ => return Err(anyhow!("INVALID_REPORT_STATUS")),
        })
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveReportRow {
    pub operation_id: String,
    pub session_id: String,
    pub occurrence_id: String,
    pub attempt_id: String,
    pub server_id: String,
    pub track_id: String,
    pub heard_ms: u64,
    pub duration_ms: Option<u64>,
    pub status: LiveReportStatus,
    pub attempt_count: u32,
    pub diagnostic: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveStatusSample {
    pub session_id: String,
    pub occurrence_id: String,
    pub server_id: String,
    pub track_id: String,
    pub generation_id: String,
    pub position_ms: u64,
    pub state: crate::providers::LivePlaybackState,
}

impl TerminalReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::NaturalEnd => "naturalCompletion",
            Self::Skip => "explicitSkip",
            Self::Stop => "stopped",
            Self::Replacement => "replaced",
            Self::Return => "returned",
            Self::TechnicalFailure => "technicalFailure",
            Self::OutputLoss => "outputLoss",
            Self::Shutdown => "shutdown",
        }
    }
}

impl Database {
    /// Creates one operation for an occurrence. No network send may precede this commit.
    pub fn record_live_report(&self, evidence: &HeardEvidence) -> Result<()> {
        let Some((reason, duration_ms, qualified)) = evidence.terminal() else {
            return Err(anyhow!("UNFROZEN_REPORT_EVIDENCE"));
        };
        let mut conn = self.conn.lock().unwrap_or_else(|error| error.into_inner());
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM playback_live_reports WHERE status IN ('ineligible','confirmed','failed','unsupported','ambiguous') AND updated_at<unixepoch()-?1",
            [REPORT_RETENTION_SECONDS],
        )?;
        let existing: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM playback_live_reports WHERE session_id=?1 AND occurrence_id=?2 AND kind='completed')", params![evidence.session_id, evidence.occurrence_id], |r| r.get(0))?;
        if existing {
            return Ok(());
        }
        let total: i64 = tx.query_row("SELECT COUNT(*) FROM playback_live_reports", [], |r| {
            r.get(0)
        })?;
        if total >= MAX_REPORT_ROWS {
            tx.execute(
                "DELETE FROM playback_live_reports WHERE operation_id IN (SELECT operation_id FROM playback_live_reports WHERE status IN ('ineligible','confirmed','failed','unsupported','ambiguous') ORDER BY updated_at LIMIT ?1)",
                [total - MAX_REPORT_ROWS + 1],
            )?;
        }
        let total: i64 = tx.query_row("SELECT COUNT(*) FROM playback_live_reports", [], |r| {
            r.get(0)
        })?;
        if total >= MAX_REPORT_ROWS {
            return Err(anyhow!("REPORT_JOURNAL_FULL"));
        }
        let pending: i64 = tx.query_row(
            "SELECT COUNT(*) FROM playback_live_reports WHERE status IN ('pending','sending')",
            [],
            |r| r.get(0),
        )?;
        let status = if !qualified {
            LiveReportStatus::Ineligible
        } else if pending >= MAX_PENDING_REPORTS {
            LiveReportStatus::Failed
        } else {
            LiveReportStatus::Pending
        };
        tx.execute(
            "INSERT OR IGNORE INTO playback_live_reports(operation_id,schema_version,session_id,occurrence_id,attempt_id,server_id,track_id,kind,heard_ms,duration_ms,terminal_reason,status,diagnostic) VALUES(?1,1,?2,?3,?4,?5,?6,'completed',?7,?8,?9,?10,?11)",
            params![Uuid::new_v4().to_string(), evidence.session_id, evidence.occurrence_id, evidence.attempt_id, evidence.server_id, evidence.track_id, i64::try_from(evidence.heard_ms())?, duration_ms.filter(|value| *value > 0 && *value <= i64::MAX as u64).map(|value| value as i64), reason.as_str(), status.as_str(), if status == LiveReportStatus::Failed { Some("queueFull") } else { None }],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Atomic claim. A crash or transport uncertainty after this point is ambiguous.
    pub fn claim_live_report(&self) -> Result<Option<LiveReportRow>> {
        let mut conn = self.conn.lock().unwrap_or_else(|error| error.into_inner());
        let tx = conn.transaction()?;
        let operation_id: Option<String> = tx.query_row(
            "SELECT operation_id FROM playback_live_reports WHERE status='pending' AND next_attempt_at<=unixepoch() ORDER BY created_at, rowid LIMIT 1",
            [], |r| r.get(0),
        ).optional()?;
        let Some(operation_id) = operation_id else {
            return Ok(None);
        };
        tx.execute(
            "UPDATE playback_live_reports SET status='sending',attempt_count=attempt_count+1,updated_at=unixepoch() WHERE operation_id=?1 AND status='pending'",
            [&operation_id],
        )?;
        let row = tx.query_row(
            "SELECT operation_id,session_id,occurrence_id,attempt_id,server_id,track_id,heard_ms,duration_ms,status,attempt_count,diagnostic FROM playback_live_reports WHERE operation_id=?1",
            [&operation_id], live_report_from_row,
        )?;
        tx.commit()?;
        Ok(Some(row))
    }

    pub fn settle_live_report(
        &self,
        operation_id: &str,
        status: LiveReportStatus,
        diagnostic: Option<&'static str>,
    ) -> Result<bool> {
        if !matches!(
            status,
            LiveReportStatus::Confirmed
                | LiveReportStatus::Failed
                | LiveReportStatus::Ambiguous
                | LiveReportStatus::Unsupported
        ) {
            return Err(anyhow!("INVALID_REPORT_SETTLEMENT"));
        }
        let diagnostic = match diagnostic {
            None => None,
            Some(
                value @ ("providerUnsupported"
                | "definiteRejection"
                | "transportAmbiguous"
                | "serverUnavailable"
                | "credentialUnavailable"),
            ) => Some(value),
            Some(_) => return Err(anyhow!("INVALID_REPORT_DIAGNOSTIC")),
        };
        let conn = self.conn.lock().unwrap_or_else(|error| error.into_inner());
        Ok(conn.execute(
            "UPDATE playback_live_reports SET status=?2,diagnostic=?3,updated_at=unixepoch() WHERE operation_id=?1 AND status='sending'",
            params![operation_id, status.as_str(), diagnostic],
        )? == 1)
    }

    /// A provider could not be resolved, so no completion request was issued.
    /// Only this pre-send case may return to the pending queue.
    pub fn defer_live_report(&self, operation_id: &str, attempt_count: u32) -> Result<bool> {
        let conn = self.conn.lock().unwrap_or_else(|error| error.into_inner());
        let exhausted = attempt_count >= 5;
        let delay = 15_i64.saturating_mul(1_i64 << attempt_count.min(6));
        Ok(conn.execute(
            "UPDATE playback_live_reports SET status=?2,diagnostic='serverUnavailable',next_attempt_at=unixepoch()+?3,updated_at=unixepoch() WHERE operation_id=?1 AND status='sending'",
            params![operation_id, if exhausted { "failed" } else { "pending" }, if exhausted { 0 } else { delay }],
        )? == 1)
    }

    pub fn recover_live_reports(&self) -> Result<usize> {
        let conn = self.conn.lock().unwrap_or_else(|error| error.into_inner());
        Ok(conn.execute(
            "UPDATE playback_live_reports SET status='ambiguous',diagnostic='transportAmbiguous',updated_at=unixepoch() WHERE status='sending'",
            [],
        )?)
    }

    pub fn list_live_reports(&self, session_id: &str, limit: usize) -> Result<Vec<LiveReportRow>> {
        let conn = self.conn.lock().unwrap_or_else(|error| error.into_inner());
        let mut statement = conn.prepare(
            "SELECT operation_id,session_id,occurrence_id,attempt_id,server_id,track_id,heard_ms,duration_ms,status,attempt_count,diagnostic FROM playback_live_reports WHERE session_id=?1 ORDER BY created_at DESC,rowid DESC LIMIT ?2",
        )?;
        Ok(statement
            .query_map(
                params![session_id, limit.min(200) as i64],
                live_report_from_row,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

/// One completion request at a time. A claimed write is never retried after
/// an uncertain response; recovery classifies an interrupted claim as ambiguous.
pub async fn run_reporter(
    db: Arc<Database>,
    manager: Arc<tokio::sync::RwLock<crate::server_manager::ServerManager>>,
    playback: crate::playback::PlaybackSession,
    mut status_events: tokio::sync::mpsc::Receiver<Option<LiveStatusSample>>,
    shutdown: Arc<AtomicBool>,
) {
    if let Err(error) = db.recover_live_reports() {
        eprintln!("[LiveReport] recovery failed: {error:#}");
        return;
    }
    let mut previous_status: Option<LiveStatusSample> = None;
    let mut last_status_sent = std::time::Instant::now() - Duration::from_secs(30);
    while !shutdown.load(Ordering::Acquire) {
        while let Ok(event) = status_events.try_recv() {
            apply_live_status(
                &db,
                &manager,
                &mut previous_status,
                &mut last_status_sent,
                event,
            )
            .await;
        }
        apply_live_status(
            &db,
            &manager,
            &mut previous_status,
            &mut last_status_sent,
            playback.live_status_sample(),
        )
        .await;
        match db.claim_live_report() {
            Ok(Some(row)) => {
                let provider =
                    crate::server_manager::get_provider_by_server_id(&manager, &db, &row.server_id)
                        .await;
                match provider {
                    Ok(provider) => {
                        let result = provider.report_live_completed(&row.track_id).await;
                        let (status, diagnostic) = classify_completion(result);
                        if let Err(error) =
                            db.settle_live_report(&row.operation_id, status, diagnostic)
                        {
                            eprintln!("[LiveReport] settlement failed: {error:#}");
                        }
                    }
                    Err(_) => {
                        if let Err(error) =
                            db.defer_live_report(&row.operation_id, row.attempt_count)
                        {
                            eprintln!("[LiveReport] deferral failed: {error:#}");
                        }
                    }
                }
            }
            Ok(None) => tokio::time::sleep(Duration::from_millis(500)).await,
            Err(error) => {
                eprintln!("[LiveReport] claim failed: {error:#}");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

async fn apply_live_status(
    db: &Database,
    manager: &Arc<tokio::sync::RwLock<crate::server_manager::ServerManager>>,
    previous: &mut Option<LiveStatusSample>,
    last_sent: &mut std::time::Instant,
    current: Option<LiveStatusSample>,
) {
    let same_identity = |left: &LiveStatusSample, right: &LiveStatusSample| {
        left.session_id == right.session_id
            && left.occurrence_id == right.occurrence_id
            && left.generation_id == right.generation_id
    };
    if let Some(old) = previous.as_ref()
        && current.as_ref().is_none_or(|new| !same_identity(old, new))
    {
        send_live_status(
            db,
            manager,
            old,
            crate::providers::LivePlaybackState::Stopped,
        )
        .await;
    }
    if let Some(new) = current.as_ref()
        && (previous
            .as_ref()
            .is_none_or(|old| !same_identity(old, new) || old.state != new.state)
            || last_sent.elapsed() >= Duration::from_secs(15))
    {
        send_live_status(db, manager, new, new.state).await;
        *last_sent = std::time::Instant::now();
    }
    *previous = current;
}

async fn send_live_status(
    db: &Database,
    manager: &Arc<tokio::sync::RwLock<crate::server_manager::ServerManager>>,
    sample: &LiveStatusSample,
    state: crate::providers::LivePlaybackState,
) {
    if let Ok(provider) =
        crate::server_manager::get_provider_by_server_id(manager, db, &sample.server_id).await
    {
        let _ = provider
            .report_live_status(crate::providers::LiveStatusRequest {
                song_id: sample.track_id.clone(),
                position_ms: sample.position_ms,
                state,
            })
            .await;
    }
}

fn classify_completion(
    result: Result<(), crate::providers::ProviderError>,
) -> (LiveReportStatus, Option<&'static str>) {
    use crate::providers::ProviderError;
    match result {
        Ok(()) => (LiveReportStatus::Confirmed, None),
        Err(ProviderError::UnsupportedCapability(_)) => {
            (LiveReportStatus::Unsupported, Some("providerUnsupported"))
        }
        Err(
            ProviderError::Auth(_)
            | ProviderError::Forbidden
            | ProviderError::NotFound { .. }
            | ProviderError::StaleConfiguration(_),
        ) => (LiveReportStatus::Failed, Some("definiteRejection")),
        Err(ProviderError::Http {
            status: Some(400..=499),
            ..
        }) => (LiveReportStatus::Failed, Some("definiteRejection")),
        Err(_) => (LiveReportStatus::Ambiguous, Some("transportAmbiguous")),
    }
}

fn live_report_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<LiveReportRow> {
    let status: String = row.get(8)?;
    let status = LiveReportStatus::parse(&status).map_err(|_| {
        rusqlite::Error::InvalidColumnType(8, "status".into(), rusqlite::types::Type::Text)
    })?;
    Ok(LiveReportRow {
        operation_id: row.get(0)?,
        session_id: row.get(1)?,
        occurrence_id: row.get(2)?,
        attempt_id: row.get(3)?,
        server_id: row.get(4)?,
        track_id: row.get(5)?,
        heard_ms: row.get::<_, i64>(6)? as u64,
        duration_ms: row.get::<_, Option<i64>>(7)?.map(|value| value as u64),
        status,
        attempt_count: row.get::<_, i64>(9)? as u32,
        diagnostic: row.get(10)?,
    })
}

/// Frozen identity and consumed-media evidence for one playback attempt. Only the
/// owner may sample it, using the output's generation-fenced progress ingress.
#[derive(Debug, Clone)]
pub struct HeardEvidence {
    pub session_id: String,
    pub occurrence_id: String,
    pub attempt_id: String,
    pub server_id: String,
    pub track_id: String,
    generation_id: String,
    last_position_ms: u64,
    heard_ms: u64,
    frozen: bool,
    terminal: Option<(TerminalReason, Option<u64>, bool)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalReason {
    NaturalEnd,
    Skip,
    Stop,
    Replacement,
    Return,
    TechnicalFailure,
    OutputLoss,
    Shutdown,
}

impl HeardEvidence {
    pub fn new(
        session_id: &str,
        occurrence_id: &str,
        attempt_id: &str,
        server_id: &str,
        track_id: &str,
        generation_id: &str,
        position_ms: u64,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            occurrence_id: occurrence_id.into(),
            attempt_id: attempt_id.into(),
            server_id: server_id.into(),
            track_id: track_id.into(),
            generation_id: generation_id.into(),
            last_position_ms: position_ms,
            heard_ms: 0,
            frozen: false,
            terminal: None,
        }
    }

    pub fn sample(&mut self, generation_id: &str, position_ms: u64, audible: bool) {
        if self.frozen || generation_id != self.generation_id {
            return;
        }
        if position_ms < self.last_position_ms {
            // A backwards jump without a generation change is not trusted.
            self.last_position_ms = position_ms;
            return;
        }
        if audible {
            self.heard_ms = self
                .heard_ms
                .saturating_add(position_ms - self.last_position_ms)
                .min(24 * 60 * 60 * 1000);
        }
        self.last_position_ms = position_ms;
    }

    pub fn discontinuity(&mut self, generation_id: &str, position_ms: u64) {
        if self.frozen {
            return;
        }
        self.generation_id = generation_id.into();
        self.last_position_ms = position_ms;
    }

    pub fn heard_ms(&self) -> u64 {
        self.heard_ms
    }

    pub fn generation_id(&self) -> &str {
        &self.generation_id
    }

    pub fn qualifies(&self, duration_ms: Option<u64>, reason: TerminalReason) -> bool {
        if matches!(
            reason,
            TerminalReason::TechnicalFailure
                | TerminalReason::OutputLoss
                | TerminalReason::Shutdown
        ) {
            return false;
        }
        let Some(duration_ms) =
            duration_ms.filter(|duration| *duration > 0 && *duration <= i64::MAX as u64)
        else {
            return false;
        };
        let threshold = duration_ms.div_ceil(2).min(240_000);
        self.heard_ms >= threshold
    }

    pub fn freeze(&mut self, duration_ms: Option<u64>, reason: TerminalReason) -> bool {
        if self.frozen {
            return false;
        }
        let qualified = self.qualifies(duration_ms, reason);
        self.frozen = true;
        self.terminal = Some((reason, duration_ms, qualified));
        qualified
    }

    pub fn terminal(&self) -> Option<(TerminalReason, Option<u64>, bool)> {
        self.terminal
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    fn evidence() -> HeardEvidence {
        HeardEvidence::new(
            "session",
            "occurrence",
            "attempt",
            "source",
            "track",
            "g1",
            0,
        )
    }

    #[test]
    fn duplicate_and_stale_samples_do_not_add_heard_time() {
        let mut e = evidence();
        e.sample("g1", 10_000, true);
        e.sample("g1", 10_000, true);
        e.sample("stale", 40_000, true);
        assert_eq!(e.heard_ms(), 10_000);
        assert!(!e.qualifies(Some(40_000), TerminalReason::Skip));
    }

    #[test]
    fn seek_forward_does_not_count_skipped_duration_and_replay_counts_consumed_audio() {
        let mut e = evidence();
        e.sample("g1", 5_000, true);
        e.discontinuity("g2", 30_000);
        e.sample("g2", 35_000, true);
        assert_eq!(e.heard_ms(), 10_000);
        e.discontinuity("g3", 20_000);
        e.sample("g3", 30_000, true);
        assert_eq!(e.heard_ms(), 20_000);
        assert!(e.qualifies(Some(40_000), TerminalReason::Skip));
    }

    #[test]
    fn pause_buffer_and_restored_cursor_add_no_heard_time() {
        let mut e = HeardEvidence::new("s", "o", "a", "server", "track", "g1", 20_000);
        e.sample("g1", 20_000, false);
        e.sample("g1", 30_000, false);
        assert_eq!(e.heard_ms(), 0);
        e.discontinuity("g2", 30_000);
        e.sample("g2", 40_000, true);
        assert_eq!(e.heard_ms(), 10_000);
    }

    #[test]
    fn unknown_duration_and_technical_failure_do_not_qualify() {
        let mut e = evidence();
        e.sample("g1", 300_000, true);
        assert!(!e.qualifies(None, TerminalReason::NaturalEnd));
        assert!(!e.qualifies(Some(300_000), TerminalReason::TechnicalFailure));
        assert!(!e.qualifies(Some(300_000), TerminalReason::OutputLoss));
        assert!(!e.qualifies(Some(300_000), TerminalReason::Shutdown));
    }

    #[test]
    fn frozen_output_loss_never_enters_delivery_queue() {
        let db = Database::memory().unwrap();
        db.init_playback().unwrap();
        let mut e = evidence();
        e.sample("g1", 300_000, true);
        assert!(!e.freeze(Some(300_000), TerminalReason::OutputLoss));
        db.record_live_report(&e).unwrap();
        assert!(db.claim_live_report().unwrap().is_none());
        assert_eq!(
            db.list_live_reports("session", 10).unwrap()[0].status,
            LiveReportStatus::Ineligible
        );
    }

    #[test]
    fn early_skip_is_inspectable_but_not_delivered() {
        let db = Database::memory().unwrap();
        db.init_playback().unwrap();
        let mut e = evidence();
        e.sample("g1", 9_000, true);
        assert!(!e.freeze(Some(40_000), TerminalReason::Skip));
        db.record_live_report(&e).unwrap();
        assert!(db.claim_live_report().unwrap().is_none());
        let row = &db.list_live_reports("session", 10).unwrap()[0];
        assert_eq!(row.heard_ms, 9_000);
        assert_eq!(row.status, LiveReportStatus::Ineligible);
    }

    #[test]
    fn zero_duration_is_recorded_as_unknown_and_never_qualifies() {
        let db = Database::memory().unwrap();
        db.init_playback().unwrap();
        let mut e = evidence();
        e.sample("g1", 30_000, true);
        assert!(!e.freeze(Some(0), TerminalReason::NaturalEnd));
        db.record_live_report(&e).unwrap();
        let row = &db.list_live_reports("session", 10).unwrap()[0];
        assert_eq!(row.duration_ms, None);
        assert_eq!(row.status, LiveReportStatus::Ineligible);
    }

    #[test]
    fn full_audition_qualifies_and_frozen_outcome_cannot_repeat() {
        let mut e = evidence();
        e.sample("g1", 38_000, true);
        assert!(e.freeze(Some(38_000), TerminalReason::NaturalEnd));
        assert!(!e.freeze(Some(38_000), TerminalReason::NaturalEnd));
    }

    #[test]
    fn durable_intent_deduplicates_one_occurrence_but_keeps_repeats_distinct() {
        let db = Database::memory().unwrap();
        db.init_playback().unwrap();
        let mut first = evidence();
        first.sample("g1", 30_000, true);
        assert!(first.freeze(Some(40_000), TerminalReason::NaturalEnd));
        db.record_live_report(&first).unwrap();
        db.record_live_report(&first).unwrap();
        let mut repeat =
            HeardEvidence::new("session", "repeat", "attempt2", "source", "track", "g2", 0);
        repeat.sample("g2", 30_000, true);
        assert!(repeat.freeze(Some(40_000), TerminalReason::NaturalEnd));
        db.record_live_report(&repeat).unwrap();
        let reports = db.list_live_reports("session", 10).unwrap();
        assert_eq!(reports.len(), 2);
        assert_ne!(reports[0].operation_id, reports[1].operation_id);
        assert_eq!(reports[0].status, LiveReportStatus::Pending);
    }

    #[test]
    fn crash_after_claim_becomes_ambiguous_and_is_never_reclaimed() {
        let db = Database::memory().unwrap();
        db.init_playback().unwrap();
        let mut e = evidence();
        e.sample("g1", 30_000, true);
        e.freeze(Some(40_000), TerminalReason::NaturalEnd);
        db.record_live_report(&e).unwrap();
        let claimed = db.claim_live_report().unwrap().unwrap();
        assert_eq!(claimed.status, LiveReportStatus::Sending);
        db.recover_live_reports().unwrap();
        assert!(db.claim_live_report().unwrap().is_none());
        let rows = db.list_live_reports("session", 10).unwrap();
        assert_eq!(rows[0].status, LiveReportStatus::Ambiguous);
    }

    #[test]
    fn pre_send_unavailability_defers_without_duplicate_operation() {
        let db = Database::memory().unwrap();
        db.init_playback().unwrap();
        let mut e = evidence();
        e.sample("g1", 30_000, true);
        e.freeze(Some(40_000), TerminalReason::NaturalEnd);
        db.record_live_report(&e).unwrap();
        let claimed = db.claim_live_report().unwrap().unwrap();
        assert!(
            db.defer_live_report(&claimed.operation_id, claimed.attempt_count)
                .unwrap()
        );
        assert!(db.claim_live_report().unwrap().is_none());
        let rows = db.list_live_reports("session", 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, LiveReportStatus::Pending);
        assert_eq!(rows[0].attempt_count, 1);
    }

    #[test]
    fn definite_rejections_and_uncertain_responses_have_distinct_outcomes() {
        use crate::providers::ProviderError;
        assert_eq!(
            classify_completion(Ok(())),
            (LiveReportStatus::Confirmed, None)
        );
        assert_eq!(
            classify_completion(Err(ProviderError::Auth("denied".into()))).0,
            LiveReportStatus::Failed
        );
        assert_eq!(
            classify_completion(Err(ProviderError::Http {
                status: Some(404),
                message: "missing".into()
            }))
            .0,
            LiveReportStatus::Failed
        );
        assert_eq!(
            classify_completion(Err(ProviderError::Http {
                status: Some(500),
                message: "uncertain".into()
            }))
            .0,
            LiveReportStatus::Ambiguous
        );
        assert_eq!(
            classify_completion(Err(ProviderError::UnsupportedCapability(
                "unsupported".into()
            )))
            .0,
            LiveReportStatus::Unsupported
        );
    }

    #[test]
    fn two_sources_with_same_track_id_are_distinct_operations() {
        let db = Database::memory().unwrap();
        db.init_playback().unwrap();
        for (source, occurrence) in [("server-a", "occurrence-a"), ("server-b", "occurrence-b")] {
            let mut e = HeardEvidence::new(
                "session",
                occurrence,
                occurrence,
                source,
                "same-track",
                "g1",
                0,
            );
            e.sample("g1", 30_000, true);
            e.freeze(Some(40_000), TerminalReason::NaturalEnd);
            db.record_live_report(&e).unwrap();
        }
        let rows = db.list_live_reports("session", 10).unwrap();
        assert_eq!(rows.len(), 2);
        assert_ne!(rows[0].server_id, rows[1].server_id);
        assert_eq!(rows[0].track_id, rows[1].track_id);
    }
}
