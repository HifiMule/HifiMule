//! Immutable local selections. Only the session owner admits capture; reads are
//! independent of playback, provider availability and the command retry cache.
use super::model::{PlaybackTrackMetadata, TrackSource};
use super::session::PlaybackError;
use crate::db::Database;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

type Result<T> = std::result::Result<T, PlaybackError>;
#[cfg(test)]
pub(crate) static LAST_DB_HOLD_US: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);
pub(crate) fn error(code: &'static str) -> PlaybackError {
    PlaybackError {
        code,
        message: "Listening snapshot unavailable",
        conflict: matches!(
            code,
            "SNAPSHOT_OPERATION_REUSED"
                | "QUEUE_CONFLICT"
                | "STALE_INSTANCE"
                | "STALE_SESSION"
                | "STALE_MAIN_OCCURRENCE"
                | "PLAYBACK_BUSY"
                | "TERMINAL_PENDING"
                | "DAEMON_STOPPED"
        ),
        authoritative: None,
    }
}
fn storage(_: rusqlite::Error) -> PlaybackError {
    error("SNAPSHOT_STORAGE_FAILED")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveSnapshotParams {
    pub schema_version: u32,
    pub operation_id: String,
    pub instance_id: String,
    pub session_id: String,
    pub expected_queue_revision: String,
    #[serde(deserialize_with = "Option::<String>::deserialize")]
    pub expected_main_occurrence_id: Option<String>,
    pub name: Option<String>,
}
impl SaveSnapshotParams {
    pub fn validate(&self) -> Result<()> {
        version(self.schema_version)?;
        for id in [&self.operation_id, &self.instance_id, &self.session_id] {
            uuid(id)?;
        }
        if let Some(id) = &self.expected_main_occurrence_id {
            uuid(id)?;
        }
        decimal(&self.expected_queue_revision).map_err(|_| error("INVALID_SNAPSHOT_REQUEST"))?;
        if let Some(name) = &self.name
            && (name.len() > 480
                || name.chars().any(char::is_control)
                || name.trim().chars().count() > 120)
        {
            return Err(error("INVALID_SNAPSHOT_NAME"));
        }
        Ok(())
    }
    fn canonical(&self) -> Result<String> {
        self.validate()?;
        let mut request = self.clone();
        request.name = self
            .name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        serde_json::to_string(&request).map_err(|_| error("INVALID_SNAPSHOT_REQUEST"))
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListSnapshotsParams {
    pub schema_version: u32,
    pub cursor: Option<String>,
    pub limit: Option<usize>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GetSnapshotParams {
    pub schema_version: u32,
    pub snapshot_id: Option<String>,
    pub operation_id: Option<String>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListSnapshotEntriesParams {
    pub schema_version: u32,
    pub snapshot_id: String,
    pub cursor: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListeningSnapshotSummary {
    pub schema_version: u32,
    pub policy_version: u32,
    pub snapshot_id: String,
    pub operation_id: String,
    pub name: String,
    pub created_at: String,
    pub instance_id: String,
    pub session_id: String,
    pub logical_session_id: Option<String>,
    pub queue_revision: String,
    pub main_occurrence_id: Option<String>,
    pub entry_count: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListeningSnapshotEntry {
    pub ordinal: String,
    pub occurrence_id: String,
    pub source: TrackSource,
    pub origin: String,
    pub source_label: String,
    pub source_icon: Option<String>,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_ms: Option<i64>,
    pub source_available: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "status",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SaveSnapshotResult {
    Saved {
        schema_version: u32,
        snapshot: Box<ListeningSnapshotSummary>,
    },
    Empty {
        schema_version: u32,
        reason: &'static str,
    },
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotList {
    pub schema_version: u32,
    pub snapshots: Vec<ListeningSnapshotSummary>,
    pub next_cursor: Option<String>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotEntryPage {
    pub schema_version: u32,
    pub snapshot_id: String,
    pub entries: Vec<ListeningSnapshotEntry>,
    pub next_cursor: Option<String>,
    pub total_count: String,
}

pub(crate) fn migrate(tx: &Transaction<'_>) -> rusqlite::Result<()> {
    tx.execute_batch("CREATE TABLE IF NOT EXISTS playback_listening_snapshots (
        creation_seq INTEGER PRIMARY KEY AUTOINCREMENT,
        snapshot_id TEXT NOT NULL UNIQUE, operation_id TEXT NOT NULL UNIQUE,
        schema_version INTEGER NOT NULL CHECK(schema_version=1), policy_version INTEGER NOT NULL CHECK(policy_version=1),
        canonical_request TEXT NOT NULL, name TEXT NOT NULL, created_at TEXT NOT NULL,
        instance_id TEXT NOT NULL, session_id TEXT NOT NULL, logical_session_id TEXT,
        queue_revision INTEGER NOT NULL CHECK(queue_revision>=0), main_occurrence_id TEXT,
        entry_count INTEGER NOT NULL CHECK(entry_count>=0)
    );
    CREATE INDEX IF NOT EXISTS playback_listening_snapshots_list ON playback_listening_snapshots(creation_seq DESC);
    CREATE TABLE IF NOT EXISTS playback_listening_snapshot_entries (
        snapshot_id TEXT NOT NULL, ordinal INTEGER NOT NULL CHECK(ordinal>=0), occurrence_id TEXT NOT NULL,
        server_id TEXT NOT NULL, track_id TEXT NOT NULL, origin TEXT NOT NULL CHECK(origin IN ('history','current','upcoming')),
        source_label TEXT NOT NULL, source_icon TEXT, title TEXT, artist TEXT, album TEXT,
        duration_ms INTEGER CHECK(duration_ms>=0 AND duration_ms<=9007199254740991),
        PRIMARY KEY(snapshot_id,ordinal), UNIQUE(snapshot_id,occurrence_id)
    );")
}
fn version(value: u32) -> Result<()> {
    if value != 1 {
        return Err(error("INVALID_SNAPSHOT_REQUEST"));
    }
    Ok(())
}
fn uuid(value: &str) -> Result<()> {
    if Uuid::parse_str(value).is_err() || value.len() != 36 {
        return Err(error("INVALID_SNAPSHOT_REQUEST"));
    }
    Ok(())
}
fn decimal(value: &str) -> Result<i64> {
    if value.is_empty()
        || !value.bytes().all(|b| b.is_ascii_digit())
        || (value.len() > 1 && value.starts_with('0'))
    {
        return Err(error("INVALID_SNAPSHOT_CURSOR"));
    }
    value
        .parse::<i64>()
        .map_err(|_| error("INVALID_SNAPSHOT_CURSOR"))
}
fn paging(
    cursor: Option<&str>,
    limit: Option<usize>,
    prefix: &str,
) -> Result<(Option<i64>, usize)> {
    let limit = limit.unwrap_or(50);
    if !(1..=200).contains(&limit) {
        return Err(error("INVALID_SNAPSHOT_REQUEST"));
    }
    let after = cursor
        .map(|c| {
            c.strip_prefix(prefix)
                .ok_or_else(|| error("INVALID_SNAPSHOT_CURSOR"))
                .and_then(decimal)
        })
        .transpose()?;
    Ok((after, limit))
}
const HEADER: &str = "snapshot_id,operation_id,schema_version,policy_version,name,created_at,instance_id,session_id,logical_session_id,queue_revision,main_occurrence_id,entry_count";
fn summary(row: &rusqlite::Row<'_>) -> rusqlite::Result<ListeningSnapshotSummary> {
    Ok(ListeningSnapshotSummary {
        snapshot_id: row.get(0)?,
        operation_id: row.get(1)?,
        schema_version: row.get(2)?,
        policy_version: row.get(3)?,
        name: row.get(4)?,
        created_at: row.get(5)?,
        instance_id: row.get(6)?,
        session_id: row.get(7)?,
        logical_session_id: row.get(8)?,
        queue_revision: row.get::<_, i64>(9)?.to_string(),
        main_occurrence_id: row.get(10)?,
        entry_count: row.get::<_, i64>(11)?.to_string(),
    })
}
fn integrity(conn: &Connection, s: &ListeningSnapshotSummary) -> Result<()> {
    if s.schema_version != 1 || s.policy_version != 1 {
        return Err(error("UNSUPPORTED_SNAPSHOT_VERSION"));
    }
    let count = decimal(&s.entry_count).map_err(|_| error("SNAPSHOT_CORRUPT"))?;
    let (actual, first, last): (i64, Option<i64>, Option<i64>) = conn.query_row(
        "SELECT COUNT(*),MIN(ordinal),MAX(ordinal) FROM playback_listening_snapshot_entries WHERE snapshot_id=?1", [&s.snapshot_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map_err(storage)?;
    if count <= 0 || actual != count || first != Some(0) || last != Some(count - 1) {
        return Err(error("SNAPSHOT_CORRUPT"));
    }
    Ok(())
}
fn recover(
    conn: &Connection,
    p: &SaveSnapshotParams,
    canonical: &str,
) -> Result<Option<ListeningSnapshotSummary>> {
    let found = conn.query_row(&format!("SELECT {HEADER},canonical_request FROM playback_listening_snapshots WHERE operation_id=?1"), [&p.operation_id], |r| Ok((summary(r)?, r.get::<_, String>(12)?))).optional().map_err(storage)?;
    if let Some((saved, original)) = found {
        if original != canonical {
            return Err(error("SNAPSHOT_OPERATION_REUSED"));
        }
        integrity(conn, &saved)?;
        return Ok(Some(saved));
    }
    Ok(None)
}

impl Database {
    pub(crate) fn recover_listening_snapshot(
        &self,
        p: &SaveSnapshotParams,
    ) -> Result<Option<ListeningSnapshotSummary>> {
        let canonical = p.canonical()?;
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        recover(&conn, p, &canonical)
    }
    /// Called with the owner lock held. SQL performs materialization without a
    /// Rust history vector. Dropping the transaction rolls back every row.
    pub(crate) fn capture_listening_snapshot(
        &self,
        p: &SaveSnapshotParams,
        metadata: Option<&PlaybackTrackMetadata>,
        duration_ms: Option<u64>,
    ) -> Result<SaveSnapshotResult> {
        let canonical = p.canonical()?;
        let mut conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        #[cfg(test)]
        let db_held_at = std::time::Instant::now();
        let tx = conn.transaction().map_err(storage)?;
        if let Some(snapshot) = recover(&tx, p, &canonical)? {
            return Ok(SaveSnapshotResult::Saved {
                schema_version: 1,
                snapshot: Box::new(snapshot),
            });
        }
        let live: Option<(String, i64, Option<String>)> = tx.query_row("SELECT session_id,queue_revision,current_occurrence_id FROM playback_sessions WHERE singleton_id=1", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).optional().map_err(storage)?;
        let Some((session, revision, current)) = live else {
            return Err(error("SNAPSHOT_CORRUPT"));
        };
        if session != p.session_id {
            return Err(error("STALE_SESSION"));
        }
        if revision.to_string() != p.expected_queue_revision {
            return Err(error("QUEUE_CONFLICT"));
        }
        if current != p.expected_main_occurrence_id {
            return Err(error("STALE_MAIN_OCCURRENCE"));
        }
        let Some(current) = current else {
            let nonempty: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM playback_occurrences WHERE session_id=?1)",
                    [&session],
                    |r| r.get(0),
                )
                .map_err(storage)?;
            if nonempty {
                return Err(error("SNAPSHOT_CORRUPT"));
            }
            return Ok(SaveSnapshotResult::Empty {
                schema_version: 1,
                reason: "noMainSelection",
            });
        };
        let ordinal: i64 = tx
            .query_row(
                "SELECT ordinal FROM playback_occurrences WHERE session_id=?1 AND occurrence_id=?2",
                params![session, current],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage)?
            .ok_or_else(|| error("SNAPSHOT_CORRUPT"))?;
        let id = Uuid::new_v4().to_string();
        let created = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let name = p
            .name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("Listening snapshot {created}"));
        tx.execute("INSERT INTO playback_listening_snapshots(snapshot_id,operation_id,schema_version,policy_version,canonical_request,name,created_at,instance_id,session_id,logical_session_id,queue_revision,main_occurrence_id,entry_count) VALUES(?1,?2,1,1,?3,?4,?5,?6,?7,(SELECT logical_id FROM playback_radio WHERE session_id=?7),?8,?9,0)", params![id,p.operation_id,canonical,name,created,p.instance_id,session,revision,current]).map_err(storage)?;
        let metadata = metadata.filter(|m| m.source.validate().is_ok());
        // H uses first closed visit order. C/U membership takes precedence over
        // any earlier visits, and explicitSkip survives later retries/likes.
        tx.execute("WITH candidates AS (
            SELECT o.*, (SELECT MIN(a.attempt_seq) FROM playback_attempts a WHERE a.session_id=o.session_id AND a.occurrence_id=o.occurrence_id AND a.disposition IS NOT NULL) AS first_visit
            FROM playback_occurrences o WHERE o.session_id=?2
            AND NOT EXISTS(SELECT 1 FROM playback_attempts a WHERE a.session_id=o.session_id AND a.occurrence_id=o.occurrence_id AND a.disposition='explicitSkip')
            AND NOT EXISTS(SELECT 1 FROM playback_feedback_dispositions d WHERE d.session_id=o.session_id AND d.occurrence_id=o.occurrence_id AND d.rejected=1)
        ), accepted AS (
            SELECT *, CASE WHEN ordinal<?3 THEN 0 WHEN ordinal=?3 THEN 1 ELSE 2 END AS section
            FROM candidates WHERE ordinal>=?3 OR first_visit IS NOT NULL
        )
        INSERT INTO playback_listening_snapshot_entries(snapshot_id,ordinal,occurrence_id,server_id,track_id,origin,source_label,source_icon,title,artist,album,duration_ms)
        SELECT ?1,ROW_NUMBER() OVER(ORDER BY section,CASE WHEN section=0 THEN first_visit ELSE ordinal END)-1,
            occurrence_id,server_id,track_id,CASE section WHEN 0 THEN 'history' WHEN 1 THEN 'current' ELSE 'upcoming' END,
            COALESCE((SELECT NULLIF(trim(name),'') FROM server_config s WHERE s.server_id=accepted.server_id ORDER BY id LIMIT 1),server_id),
            (SELECT icon FROM server_config s WHERE s.server_id=accepted.server_id ORDER BY id LIMIT 1),
            CASE WHEN section=1 AND server_id=?4 AND track_id=?5 THEN ?6 END,
            CASE WHEN section=1 AND server_id=?4 AND track_id=?5 THEN ?7 END,
            CASE WHEN section=1 AND server_id=?4 AND track_id=?5 THEN ?8 END,
            CASE WHEN section=1 AND server_id=?4 AND track_id=?5 THEN ?9 END
        FROM accepted", params![id,session,ordinal,metadata.map(|m| &m.source.server_id),metadata.map(|m| &m.source.track_id),metadata.map(|m| &m.title),metadata.and_then(|m| m.artist.as_ref()),metadata.and_then(|m| m.album.as_ref()),duration_ms.filter(|v| *v<=9_007_199_254_740_991).map(|v|v as i64)]).map_err(storage)?;
        let count: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM playback_listening_snapshot_entries WHERE snapshot_id=?1",
                [&id],
                |r| r.get(0),
            )
            .map_err(storage)?;
        if count == 0 {
            return Ok(SaveSnapshotResult::Empty {
                schema_version: 1,
                reason: "noEligibleOccurrences",
            });
        }
        tx.execute(
            "UPDATE playback_listening_snapshots SET entry_count=?2 WHERE snapshot_id=?1",
            params![id, count],
        )
        .map_err(storage)?;
        let snapshot = recover(&tx, p, &canonical)?.ok_or_else(|| error("SNAPSHOT_CORRUPT"))?;
        #[cfg(test)]
        if let Ok(ready) = std::env::var("HIFIMULE_SNAPSHOT_CRASH_READY") {
            std::fs::write(ready, b"before commit").unwrap();
            std::thread::sleep(std::time::Duration::from_secs(30));
        }
        tx.commit().map_err(storage)?;
        #[cfg(test)]
        LAST_DB_HOLD_US.store(
            db_held_at.elapsed().as_micros() as u64,
            std::sync::atomic::Ordering::Release,
        );
        Ok(SaveSnapshotResult::Saved {
            schema_version: 1,
            snapshot: Box::new(snapshot),
        })
    }

    pub fn get_listening_snapshot(&self, p: GetSnapshotParams) -> Result<ListeningSnapshotSummary> {
        version(p.schema_version)?;
        let (column, id) = match (&p.snapshot_id, &p.operation_id) {
            (Some(id), None) => ("snapshot_id", id),
            (None, Some(id)) => ("operation_id", id),
            _ => return Err(error("INVALID_SNAPSHOT_REQUEST")),
        };
        uuid(id)?;
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let saved = conn
            .query_row(
                &format!("SELECT {HEADER} FROM playback_listening_snapshots WHERE {column}=?1"),
                [id],
                summary,
            )
            .optional()
            .map_err(storage)?
            .ok_or_else(|| error("SNAPSHOT_NOT_FOUND"))?;
        integrity(&conn, &saved)?;
        Ok(saved)
    }
    pub fn list_listening_snapshots(&self, p: ListSnapshotsParams) -> Result<SnapshotList> {
        version(p.schema_version)?;
        let (before, limit) = paging(p.cursor.as_deref(), p.limit, "1:list:")?;
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn.prepare(&format!("SELECT {HEADER},creation_seq FROM playback_listening_snapshots WHERE (?1 IS NULL OR creation_seq<?1) ORDER BY creation_seq DESC LIMIT ?2")).map_err(storage)?;
        let mut rows = stmt
            .query_map(params![before, (limit + 1) as i64], |r| {
                Ok((summary(r)?, r.get::<_, i64>(12)?))
            })
            .map_err(storage)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage)?;
        let more = rows.len() > limit;
        rows.truncate(limit);
        for (s, _) in &rows {
            integrity(&conn, s)?;
        }
        let next_cursor = if more {
            rows.last().map(|(_, seq)| format!("1:list:{seq}"))
        } else {
            None
        };
        Ok(SnapshotList {
            schema_version: 1,
            snapshots: rows.into_iter().map(|(s, _)| s).collect(),
            next_cursor,
        })
    }
    pub fn list_listening_snapshot_entries(
        &self,
        p: ListSnapshotEntriesParams,
    ) -> Result<SnapshotEntryPage> {
        version(p.schema_version)?;
        uuid(&p.snapshot_id)?;
        let prefix = format!("1:entries:{}:", p.snapshot_id);
        let (after, limit) = paging(p.cursor.as_deref(), p.limit, &prefix)?;
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let saved = conn
            .query_row(
                &format!("SELECT {HEADER} FROM playback_listening_snapshots WHERE snapshot_id=?1"),
                [&p.snapshot_id],
                summary,
            )
            .optional()
            .map_err(storage)?
            .ok_or_else(|| error("SNAPSHOT_NOT_FOUND"))?;
        integrity(&conn, &saved)?;
        if after.is_some_and(|n| n >= saved.entry_count.parse::<i64>().unwrap_or(0)) {
            return Err(error("INVALID_SNAPSHOT_CURSOR"));
        }
        let mut stmt = conn.prepare("SELECT ordinal,occurrence_id,server_id,track_id,origin,source_label,source_icon,title,artist,album,duration_ms,EXISTS(SELECT 1 FROM server_config s WHERE s.server_id=e.server_id) FROM playback_listening_snapshot_entries e WHERE snapshot_id=?1 AND ordinal>?2 ORDER BY ordinal LIMIT ?3").map_err(storage)?;
        let mut entries = stmt
            .query_map(
                params![p.snapshot_id, after.unwrap_or(-1), (limit + 1) as i64],
                |r| {
                    Ok(ListeningSnapshotEntry {
                        ordinal: r.get::<_, i64>(0)?.to_string(),
                        occurrence_id: r.get(1)?,
                        source: TrackSource {
                            server_id: r.get(2)?,
                            track_id: r.get(3)?,
                        },
                        origin: r.get(4)?,
                        source_label: r.get(5)?,
                        source_icon: r.get(6)?,
                        title: r.get(7)?,
                        artist: r.get(8)?,
                        album: r.get(9)?,
                        duration_ms: r.get(10)?,
                        source_available: r.get(11)?,
                    })
                },
            )
            .map_err(storage)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage)?;
        let more = entries.len() > limit;
        entries.truncate(limit);
        let next_cursor = if more {
            entries.last().map(|e| format!("{prefix}{}", e.ordinal))
        } else {
            None
        };
        Ok(SnapshotEntryPage {
            schema_version: 1,
            snapshot_id: p.snapshot_id,
            entries,
            next_cursor,
            total_count: saved.entry_count,
        })
    }
}

#[cfg(test)]
#[path = "export_tests.rs"]
mod tests;
