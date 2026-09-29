//! Durable, source-frozen export of immutable listening snapshots.
use super::session::PlaybackError;
use crate::db::Database;
use rusqlite::{OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

type Result<T> = std::result::Result<T, PlaybackError>;
pub const MAX_EXPORT_ENTRIES: usize = 10_000;
pub const MAX_EXPORT_PARTS: usize = 32;
pub const MAX_PROVIDER_IDS: usize = 1_000;
pub const RETENTION_SECONDS: i64 = 30 * 24 * 60 * 60;

fn err(code: &'static str) -> PlaybackError {
    PlaybackError {
        code,
        message: "Snapshot playlist export unavailable",
        conflict: matches!(
            code,
            "EXPORT_OPERATION_REUSED" | "EXPORT_COLLISION" | "EXPORT_STALE_TRANSITION"
        ),
        authoritative: None,
    }
}
fn storage(_: rusqlite::Error) -> PlaybackError {
    err("EXPORT_STORAGE_FAILED")
}
pub fn validate_export_name(value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 480
        || value.chars().count() > 120
        || value.chars().any(char::is_control)
    {
        Err(err("INVALID_EXPORT_NAME"))
    } else {
        Ok(value.to_owned())
    }
}
fn valid_uuid(value: &str) -> bool {
    value.len() == 36 && Uuid::parse_str(value).is_ok()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PartState {
    Planned,
    Unsupported,
    Denied,
    Pending,
    Creating,
    Populating,
    Partial,
    Succeeded,
    Failed,
    Ambiguous,
    Unresolved,
    Canceled,
}
impl PartState {
    pub fn can_transition(self, next: Self) -> bool {
        matches!(
            (self, next),
            (
                Self::Planned,
                Self::Pending | Self::Unsupported | Self::Denied | Self::Canceled
            ) | (
                Self::Pending,
                Self::Creating | Self::Unsupported | Self::Denied | Self::Failed | Self::Canceled
            ) | (
                Self::Creating,
                Self::Succeeded | Self::Failed | Self::Ambiguous | Self::Partial | Self::Denied
            ) | (
                Self::Populating,
                Self::Succeeded | Self::Partial | Self::Ambiguous | Self::Failed
            ) | (
                Self::Partial,
                Self::Populating | Self::Unresolved | Self::Canceled
            ) | (Self::Failed, Self::Pending | Self::Canceled)
                | (
                    Self::Ambiguous,
                    Self::Succeeded | Self::Partial | Self::Unresolved
                )
                | (Self::Unresolved, Self::Canceled)
        )
    }
    fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Unsupported => "unsupported",
            Self::Denied => "denied",
            Self::Pending => "pending",
            Self::Creating => "creating",
            Self::Populating => "populating",
            Self::Partial => "partial",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Ambiguous => "ambiguous",
            Self::Unresolved => "unresolved",
            Self::Canceled => "canceled",
        }
    }
    fn parse(v: &str) -> Result<Self> {
        Ok(match v {
            "planned" => Self::Planned,
            "unsupported" => Self::Unsupported,
            "denied" => Self::Denied,
            "pending" => Self::Pending,
            "creating" => Self::Creating,
            "populating" => Self::Populating,
            "partial" => Self::Partial,
            "succeeded" => Self::Succeeded,
            "failed" => Self::Failed,
            "ambiguous" => Self::Ambiguous,
            "unresolved" => Self::Unresolved,
            "canceled" => Self::Canceled,
            _ => return Err(err("EXPORT_CORRUPT")),
        })
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AggregateState {
    Planned,
    Running,
    Partial,
    Succeeded,
    Failed,
    Unresolved,
    Canceled,
}
pub fn aggregate(parts: &[PartState]) -> AggregateState {
    if parts.is_empty() {
        return AggregateState::Failed;
    }
    if parts.iter().all(|v| *v == PartState::Succeeded) {
        return AggregateState::Succeeded;
    }
    if parts.iter().all(|v| *v == PartState::Canceled) {
        return AggregateState::Canceled;
    }
    if parts
        .iter()
        .any(|v| matches!(v, PartState::Ambiguous | PartState::Unresolved))
    {
        return AggregateState::Unresolved;
    }
    if parts.contains(&PartState::Succeeded) {
        return AggregateState::Partial;
    }
    if parts.iter().any(|v| {
        matches!(
            v,
            PartState::Pending | PartState::Creating | PartState::Populating
        )
    }) {
        return AggregateState::Running;
    }
    if parts.iter().all(|v| *v == PartState::Planned) {
        AggregateState::Planned
    } else {
        AggregateState::Failed
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanExportParams {
    pub schema_version: u32,
    pub snapshot_id: String,
    pub name: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartExportParams {
    pub schema_version: u32,
    pub operation_id: String,
    pub snapshot_id: String,
    pub name: String,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportLocator {
    pub schema_version: u32,
    pub operation_id: String,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportPartLocator {
    pub schema_version: u32,
    pub operation_id: String,
    pub server_id: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportPart {
    pub server_id: String,
    pub source_label: String,
    pub state: PartState,
    pub expected_count: String,
    pub confirmed_count: String,
    pub playlist_id: Option<String>,
    pub reason: Option<String>,
    pub attempt: u32,
    pub safe_to_retry: bool,
    #[serde(skip_serializing)]
    pub track_ids: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportOperation {
    pub schema_version: u32,
    pub operation_id: Option<String>,
    pub snapshot_id: String,
    pub name: String,
    pub status: AggregateState,
    pub created_at: Option<String>,
    pub parts: Vec<ExportPart>,
}

pub(crate) fn migrate(tx: &Transaction<'_>) -> rusqlite::Result<()> {
    tx.execute_batch("CREATE TABLE IF NOT EXISTS playback_server_exports(operation_id TEXT PRIMARY KEY,schema_version INTEGER NOT NULL CHECK(schema_version=1),snapshot_id TEXT NOT NULL,name TEXT NOT NULL,canonical_request TEXT NOT NULL,created_at INTEGER NOT NULL DEFAULT(unixepoch()),updated_at INTEGER NOT NULL DEFAULT(unixepoch()),UNIQUE(snapshot_id,name));CREATE TABLE IF NOT EXISTS playback_server_export_parts(operation_id TEXT NOT NULL,ordinal INTEGER NOT NULL,server_id TEXT NOT NULL,source_label TEXT NOT NULL,state TEXT NOT NULL CHECK(state IN ('planned','unsupported','denied','pending','creating','populating','partial','succeeded','failed','ambiguous','unresolved','canceled')),track_ids_json TEXT NOT NULL,expected_count INTEGER NOT NULL CHECK(expected_count>0),confirmed_count INTEGER NOT NULL DEFAULT 0 CHECK(confirmed_count>=0),playlist_id TEXT,reason TEXT,attempt INTEGER NOT NULL DEFAULT 0 CHECK(attempt>=0),generation INTEGER NOT NULL DEFAULT 0 CHECK(generation>=0),next_attempt_at INTEGER NOT NULL DEFAULT 0,updated_at INTEGER NOT NULL DEFAULT(unixepoch()),PRIMARY KEY(operation_id,server_id),UNIQUE(operation_id,ordinal));CREATE INDEX IF NOT EXISTS playback_server_exports_retention ON playback_server_exports(updated_at);UPDATE playback_server_export_parts SET state='ambiguous',reason='daemonRestartedAfterSubmit',updated_at=unixepoch() WHERE state IN ('creating','populating');")
}

impl Database {
    pub fn list_server_exports(&self, snapshot_id: &str) -> Result<Vec<ExportOperation>> {
        if !valid_uuid(snapshot_id) {
            return Err(err("INVALID_EXPORT_REQUEST"));
        }
        let ids = {
            let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
            let mut stmt=conn.prepare("SELECT operation_id FROM playback_server_exports WHERE snapshot_id=?1 ORDER BY created_at DESC LIMIT 20").map_err(storage)?;
            stmt.query_map([snapshot_id], |r| r.get::<_, String>(0))
                .map_err(storage)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(storage)?
        };
        ids.into_iter()
            .map(|operation_id| {
                self.get_server_export(ExportLocator {
                    schema_version: 1,
                    operation_id,
                })
            })
            .collect()
    }
    pub fn plan_server_export(&self, p: PlanExportParams) -> Result<ExportOperation> {
        validate_request(p.schema_version, &p.snapshot_id)?;
        let name = validate_export_name(&p.name)?;
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM playback_listening_snapshots WHERE snapshot_id=?1)",
                [&p.snapshot_id],
                |r| r.get(0),
            )
            .map_err(storage)?;
        if !exists {
            return Err(err("SNAPSHOT_NOT_FOUND"));
        }
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM playback_listening_snapshot_entries WHERE snapshot_id=?1",
                [&p.snapshot_id],
                |r| r.get(0),
            )
            .map_err(storage)?;
        if count <= 0 || count as usize > MAX_EXPORT_ENTRIES {
            return Err(err("EXPORT_SIZE_LIMIT"));
        }
        let mut stmt=conn.prepare("SELECT server_id,source_label,track_id FROM playback_listening_snapshot_entries WHERE snapshot_id=?1 ORDER BY ordinal").map_err(storage)?;
        let rows = stmt
            .query_map([&p.snapshot_id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .map_err(storage)?;
        let mut order = Vec::<String>::new();
        let mut grouped = BTreeMap::<String, (String, Vec<String>)>::new();
        for row in rows {
            let (server, label, track) = row.map_err(storage)?;
            if !grouped.contains_key(&server) {
                order.push(server.clone())
            }
            grouped
                .entry(server)
                .or_insert_with(|| (label, Vec::new()))
                .1
                .push(track)
        }
        if order.len() > MAX_EXPORT_PARTS {
            return Err(err("EXPORT_SOURCE_LIMIT"));
        }
        let parts = order
            .into_iter()
            .map(|server_id| {
                let (source_label, track_ids) = grouped.remove(&server_id).unwrap();
                ExportPart {
                    server_id,
                    source_label,
                    state: PartState::Planned,
                    expected_count: track_ids.len().to_string(),
                    confirmed_count: "0".into(),
                    playlist_id: None,
                    reason: None,
                    attempt: 0,
                    safe_to_retry: false,
                    track_ids,
                }
            })
            .collect::<Vec<_>>();
        Ok(ExportOperation {
            schema_version: 1,
            operation_id: None,
            snapshot_id: p.snapshot_id,
            name,
            status: aggregate(&parts.iter().map(|v| v.state).collect::<Vec<_>>()),
            created_at: None,
            parts,
        })
    }
    pub fn create_server_export(&self, p: StartExportParams) -> Result<ExportOperation> {
        validate_request(p.schema_version, &p.snapshot_id)?;
        if !valid_uuid(&p.operation_id) {
            return Err(err("INVALID_EXPORT_REQUEST"));
        }
        let plan = self.plan_server_export(PlanExportParams {
            schema_version: 1,
            snapshot_id: p.snapshot_id.clone(),
            name: p.name.clone(),
        })?;
        let canonical = serde_json::to_string(&StartExportParams {
            name: plan.name.clone(),
            ..p.clone()
        })
        .map_err(|_| err("INVALID_EXPORT_REQUEST"))?;
        let mut conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let tx = conn.transaction().map_err(storage)?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT canonical_request FROM playback_server_exports WHERE operation_id=?1",
                [&p.operation_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage)?;
        if let Some(old) = existing {
            if old != canonical {
                return Err(err("EXPORT_OPERATION_REUSED"));
            }
            drop(tx);
            return self.get_server_export(ExportLocator {
                schema_version: 1,
                operation_id: p.operation_id,
            });
        }
        let reserved:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM playback_server_exports WHERE snapshot_id=?1 AND name=?2)",params![p.snapshot_id,plan.name],|r|r.get(0)).map_err(storage)?;
        if reserved {
            return Err(err("EXPORT_COLLISION"));
        }
        tx.execute(
            "DELETE FROM playback_server_exports WHERE updated_at < unixepoch()-?1",
            [RETENTION_SECONDS],
        )
        .map_err(storage)?;
        tx.execute("INSERT INTO playback_server_exports(operation_id,schema_version,snapshot_id,name,canonical_request)VALUES(?1,1,?2,?3,?4)",params![p.operation_id,p.snapshot_id,plan.name,canonical]).map_err(storage)?;
        for (ordinal, part) in plan.parts.iter().enumerate() {
            tx.execute("INSERT INTO playback_server_export_parts(operation_id,ordinal,server_id,source_label,state,track_ids_json,expected_count)VALUES(?1,?2,?3,?4,'pending',?5,?6)",params![p.operation_id,ordinal as i64,part.server_id,part.source_label,serde_json::to_string(&part.track_ids).unwrap(),part.track_ids.len() as i64]).map_err(storage)?;
        }
        tx.commit().map_err(storage)?;
        self.get_server_export(ExportLocator {
            schema_version: 1,
            operation_id: p.operation_id,
        })
    }
    pub fn get_server_export(&self, p: ExportLocator) -> Result<ExportOperation> {
        if p.schema_version != 1 || !valid_uuid(&p.operation_id) {
            return Err(err("INVALID_EXPORT_REQUEST"));
        }
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let head:Option<(String,String,i64)>=conn.query_row("SELECT snapshot_id,name,created_at FROM playback_server_exports WHERE operation_id=?1",[&p.operation_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(storage)?;
        let Some((snapshot_id, name, created)) = head else {
            return Err(err("EXPORT_NOT_FOUND"));
        };
        let mut stmt=conn.prepare("SELECT server_id,source_label,state,expected_count,confirmed_count,playlist_id,reason,attempt,track_ids_json FROM playback_server_export_parts WHERE operation_id=?1 ORDER BY ordinal").map_err(storage)?;
        let parts = stmt
            .query_map([&p.operation_id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, Option<String>>(6)?,
                    r.get::<_, i64>(7)?,
                    r.get::<_, String>(8)?,
                ))
            })
            .map_err(storage)?
            .map(|row| {
                let (
                    server_id,
                    source_label,
                    state,
                    expected,
                    confirmed,
                    playlist_id,
                    reason,
                    attempt,
                    tracks,
                ) = row.map_err(storage)?;
                let state = PartState::parse(&state)?;
                let track_ids = serde_json::from_str::<Vec<String>>(&tracks)
                    .map_err(|_| err("EXPORT_CORRUPT"))?;
                Ok(ExportPart {
                    server_id,
                    source_label,
                    state,
                    expected_count: expected.to_string(),
                    confirmed_count: confirmed.to_string(),
                    playlist_id,
                    reason,
                    attempt: attempt as u32,
                    safe_to_retry: state == PartState::Failed,
                    track_ids,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(ExportOperation {
            schema_version: 1,
            operation_id: Some(p.operation_id),
            snapshot_id,
            name,
            status: aggregate(&parts.iter().map(|v| v.state).collect::<Vec<_>>()),
            created_at: chrono::DateTime::from_timestamp(created, 0).map(|v| v.to_rfc3339()),
            parts,
        })
    }
    #[allow(clippy::too_many_arguments)]
    pub fn transition_export_part(
        &self,
        operation_id: &str,
        server_id: &str,
        expected: PartState,
        next: PartState,
        playlist_id: Option<&str>,
        confirmed: usize,
        reason: Option<&str>,
    ) -> Result<ExportPart> {
        if !expected.can_transition(next) {
            return Err(err("EXPORT_STALE_TRANSITION"));
        }
        let mut conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let tx = conn.transaction().map_err(storage)?;
        let changed=tx.execute("UPDATE playback_server_export_parts SET state=?1,playlist_id=COALESCE(?2,playlist_id),confirmed_count=?3,reason=?4,attempt=attempt+CASE WHEN ?1='creating' THEN 1 ELSE 0 END,generation=generation+1,updated_at=unixepoch() WHERE operation_id=?5 AND server_id=?6 AND state=?7",params![next.as_str(),playlist_id,confirmed as i64,reason,operation_id,server_id,expected.as_str()]).map_err(storage)?;
        if changed != 1 {
            return Err(err("EXPORT_STALE_TRANSITION"));
        }
        tx.execute(
            "UPDATE playback_server_exports SET updated_at=unixepoch() WHERE operation_id=?1",
            [operation_id],
        )
        .map_err(storage)?;
        tx.commit().map_err(storage)?;
        self.get_server_export(ExportLocator {
            schema_version: 1,
            operation_id: operation_id.into(),
        })?
        .parts
        .into_iter()
        .find(|v| v.server_id == server_id)
        .ok_or_else(|| err("EXPORT_CORRUPT"))
    }
}
fn validate_request(version: u32, snapshot_id: &str) -> Result<()> {
    if version != 1 || !valid_uuid(snapshot_id) {
        Err(err("INVALID_EXPORT_REQUEST"))
    } else {
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn export_name_validation_is_bounded_and_strict() {
        assert!(validate_export_name("Discoveries").is_ok());
        assert!(validate_export_name("  ").is_err());
        assert!(validate_export_name(&"x".repeat(481)).is_err());
        assert!(validate_export_name("bad\nname").is_err())
    }
    #[test]
    fn aggregate_status_never_flattens_mixed_results() {
        assert_eq!(
            aggregate(&[PartState::Succeeded]),
            AggregateState::Succeeded
        );
        assert_eq!(
            aggregate(&[PartState::Succeeded, PartState::Unsupported]),
            AggregateState::Partial
        );
        assert_eq!(
            aggregate(&[PartState::Ambiguous]),
            AggregateState::Unresolved
        )
    }
    #[test]
    fn transitions_reject_blind_replay_boundaries() {
        assert!(PartState::Planned.can_transition(PartState::Pending));
        assert!(PartState::Pending.can_transition(PartState::Creating));
        assert!(PartState::Creating.can_transition(PartState::Ambiguous));
        assert!(!PartState::Ambiguous.can_transition(PartState::Creating));
        assert!(!PartState::Succeeded.can_transition(PartState::Pending))
    }
    #[test]
    fn plan_partitions_interleaved_sources_without_deduplicating_repeats() {
        let db = Database::memory().unwrap();
        db.init_playback().unwrap();
        let snapshot = Uuid::new_v4().to_string();
        {
            let conn = db.conn.lock().unwrap();
            conn.execute("INSERT INTO playback_listening_snapshots(snapshot_id,operation_id,schema_version,policy_version,canonical_request,name,created_at,instance_id,session_id,queue_revision,entry_count)VALUES(?1,?2,1,1,'{}','n','2026-09-29T00:00:00Z',?3,?4,0,4)",params![snapshot,Uuid::new_v4().to_string(),Uuid::new_v4().to_string(),Uuid::new_v4().to_string()]).unwrap();
            for (ordinal, server, track) in [
                (0, "server-a", "same"),
                (1, "server-b", "same"),
                (2, "server-a", "same"),
                (3, "server-b", "last"),
            ] {
                conn.execute("INSERT INTO playback_listening_snapshot_entries(snapshot_id,ordinal,occurrence_id,server_id,track_id,origin,source_label)VALUES(?1,?2,?3,?4,?5,'upcoming',?4)",params![snapshot,ordinal,Uuid::new_v4().to_string(),server,track]).unwrap();
            }
        }
        let plan = db
            .plan_server_export(PlanExportParams {
                schema_version: 1,
                snapshot_id: snapshot,
                name: "Discoveries".into(),
            })
            .unwrap();
        assert_eq!(plan.parts.len(), 2);
        assert_eq!(plan.parts[0].track_ids, vec!["same", "same"]);
        assert_eq!(plan.parts[1].track_ids, vec!["same", "last"]);
    }
}
