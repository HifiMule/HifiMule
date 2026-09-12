use super::model::*;
use crate::db::Database;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct PlaybackError {
    pub code: &'static str,
    pub message: &'static str,
    pub conflict: bool,
}
impl PlaybackError {
    fn invalid(code: &'static str, msg: &'static str) -> Self {
        Self {
            code,
            message: msg,
            conflict: false,
        }
    }
    fn conflict(code: &'static str, msg: &'static str) -> Self {
        Self {
            code,
            message: msg,
            conflict: true,
        }
    }
}
type PResult<T> = std::result::Result<T, PlaybackError>;

#[derive(Clone)]
pub struct PlaybackSession {
    inner: Arc<Mutex<Inner>>,
}

struct Inner {
    db: Arc<Database>,
    instance_id: String,
    session: PersistedSession,
    generation_id: String,
    state_sequence: u64,
    restoration: Status,
    persistence: Status,
    dedup: HashMap<String, DedupEntry>,
    dedup_order: VecDeque<String>,
    progress: Option<Progress>,
    dirty: bool,
    checkpointed_position_ms: u64,
}
#[derive(Clone)]
struct DedupEntry {
    at: Instant,
    payload: ApplySessionParams,
    result: PResult<ApplyResult>,
}
#[derive(Clone)]
struct Progress {
    sequence: u64,
}

impl PlaybackSession {
    pub fn restore(db: Arc<Database>, instance_id: String) -> Self {
        let loaded = db.load_playback_session();
        let (session, restoration) = match loaded {
            Ok(Some(mut s)) => {
                let valid =
                    Uuid::parse_str(&s.session_id).is_ok() && validate_stored(&db, &s).is_ok();
                if valid {
                    if s.state != TransportState::Idle {
                        s.state = TransportState::Paused;
                    }
                    (
                        s,
                        Status {
                            status: "ok".into(),
                            code: None,
                        },
                    )
                } else {
                    (
                        fresh_session(),
                        Status {
                            status: "error".into(),
                            code: Some("INVALID_SESSION".into()),
                        },
                    )
                }
            }
            Ok(None) => {
                let s = fresh_session();
                let status = match db.persist_playback_structure(&s, &[]) {
                    Ok(()) => Status {
                        status: "ok".into(),
                        code: None,
                    },
                    Err(_) => Status {
                        status: "error".into(),
                        code: Some("PERSISTENCE_FAILED".into()),
                    },
                };
                (s, status)
            }
            Err(e) => {
                let code = if e.to_string().contains("UNSUPPORTED_PLAYBACK_VERSION") {
                    "UNSUPPORTED_PLAYBACK_VERSION"
                } else {
                    "RESTORE_FAILED"
                };
                (
                    fresh_session(),
                    Status {
                        status: "error".into(),
                        code: Some(code.into()),
                    },
                )
            }
        };
        let checkpointed_position_ms = session.position_ms;
        let result = Self {
            inner: Arc::new(Mutex::new(Inner {
                db,
                instance_id,
                session,
                generation_id: Uuid::new_v4().to_string(),
                state_sequence: 0,
                persistence: Status {
                    status: "ok".into(),
                    code: None,
                },
                restoration,
                dedup: HashMap::new(),
                dedup_order: VecDeque::new(),
                progress: None,
                dirty: false,
                checkpointed_position_ms,
            })),
        };
        let weak = Arc::downgrade(&result.inner);
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(Duration::from_secs(5));
                let Some(inner) = weak.upgrade() else { return };
                let session = PlaybackSession { inner };
                let _ = session.final_checkpoint();
            }
        });
        result
    }

    pub fn snapshot(&self) -> PResult<SessionSnapshot> {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        snapshot(&inner)
    }

    pub fn list(&self, p: ListOccurrencesParams) -> PResult<OccurrencePage> {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        require_schema(p.schema_version)?;
        if p.session_id != inner.session.session_id {
            return Err(PlaybackError::conflict(
                "SESSION_MISMATCH",
                "session identity is stale",
            ));
        }
        if parse_revision(&p.expected_queue_revision)? != inner.session.queue_revision {
            return Err(PlaybackError::conflict(
                "QUEUE_REVISION_CONFLICT",
                "queue revision is stale",
            ));
        }
        let limit = p.limit.unwrap_or(DEFAULT_PAGE_SIZE);
        if !(1..=MAX_PAGE_SIZE).contains(&limit) {
            return Err(PlaybackError::invalid(
                "INVALID_CURSOR",
                "limit must be between 1 and 200",
            ));
        }
        let after = match p.cursor {
            Some(c) => Some(decode_cursor(&c, &inner.session)?),
            None => None,
        };
        let mut rows = inner
            .db
            .playback_page(&inner.session.session_id, after, limit + 1)
            .map_err(storage)?;
        let next = if rows.len() > limit {
            rows.truncate(limit);
            rows.last()
                .map(|o| encode_cursor(&inner.session, o.ordinal))
        } else {
            None
        };
        decorate_availability(&inner.db, &mut rows)?;
        Ok(OccurrencePage {
            occurrences: rows,
            next_cursor: next,
            total_occurrence_count: inner
                .db
                .playback_count(&inner.session.session_id)
                .map_err(storage)?,
        })
    }

    pub fn apply(&self, p: ApplySessionParams) -> PResult<ApplyResult> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        prune_dedup(&mut inner);
        if let Some(old) = inner.dedup.get(&p.command_id) {
            return if old.payload == p {
                old.result.clone()
            } else {
                Err(PlaybackError::conflict(
                    "COMMAND_ID_REUSED",
                    "command identity was reused with another payload",
                ))
            };
        }
        let result = apply_inner(&mut inner, &p);
        retain_dedup(&mut inner, p, result.clone());
        result
    }

    pub fn retry_restore(&self) -> PResult<SessionSnapshot> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if inner.restoration.status != "error" || inner.dirty {
            return Err(PlaybackError::conflict(
                "PLAYBACK_BUSY",
                "restore retry is unavailable",
            ));
        }
        let mut loaded = inner
            .db
            .load_playback_session()
            .map_err(storage)?
            .ok_or_else(|| PlaybackError::invalid("INVALID_SESSION", "stored session is absent"))?;
        validate_stored(&inner.db, &loaded)?;
        if loaded.state != TransportState::Idle {
            loaded.state = TransportState::Paused;
        }
        inner.checkpointed_position_ms = loaded.position_ms;
        inner.session = loaded;
        inner.generation_id = Uuid::new_v4().to_string();
        inner.state_sequence += 1;
        inner.restoration = Status {
            status: "ok".into(),
            code: None,
        };
        snapshot(&inner)
    }

    #[allow(dead_code)]
    pub fn report_progress(
        &self,
        generation_id: &str,
        occurrence_id: &str,
        sequence: u64,
        position_ms: u64,
    ) -> PResult<()> {
        const JS_SAFE: u64 = 9_007_199_254_740_991;
        if position_ms > JS_SAFE {
            return Err(PlaybackError::invalid(
                "STALE_PROGRESS",
                "position is out of range",
            ));
        }
        let mut i = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let current = i.session.current_occurrence_id.as_deref();
        let last = i.progress.as_ref().map(|p| p.sequence);
        if generation_id != i.generation_id
            || current != Some(occurrence_id)
            || last.is_some_and(|v| sequence <= v)
        {
            return Err(PlaybackError::conflict(
                "STALE_PROGRESS",
                "progress sample is stale",
            ));
        }
        i.session.position_ms = position_ms;
        i.progress = Some(Progress { sequence });
        i.dirty = true;
        Ok(())
    }

    pub fn final_checkpoint(&self) -> PResult<()> {
        let mut i = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if i.restoration.status == "error" {
            return Ok(());
        }
        if !i.dirty {
            return Ok(());
        }
        let seq = i
            .session
            .checkpoint_sequence
            .checked_add(1)
            .ok_or_else(|| {
                PlaybackError::invalid("PERSISTENCE_FAILED", "checkpoint sequence overflow")
            })?;
        match i
            .db
            .checkpoint_playback_position(&i.session.session_id, seq, i.session.position_ms)
        {
            Ok(()) => {
                i.session.checkpoint_sequence = seq;
                i.checkpointed_position_ms = i.session.position_ms;
                i.dirty = false;
                i.persistence = Status {
                    status: "ok".into(),
                    code: None,
                };
                Ok(())
            }
            Err(_) => {
                i.persistence = Status {
                    status: "error".into(),
                    code: Some("PERSISTENCE_FAILED".into()),
                };
                Err(storage(anyhow::anyhow!("checkpoint failed")))
            }
        }
    }
}

fn fresh_session() -> PersistedSession {
    PersistedSession {
        session_id: Uuid::new_v4().to_string(),
        queue_revision: 0,
        checkpoint_sequence: 0,
        state: TransportState::Idle,
        current_occurrence_id: None,
        position_ms: 0,
    }
}
fn require_schema(v: u32) -> PResult<()> {
    if v == SCHEMA_VERSION {
        Ok(())
    } else {
        Err(PlaybackError::invalid(
            "UNSUPPORTED_PLAYBACK_VERSION",
            "unsupported playback schema version",
        ))
    }
}
fn parse_revision(v: &str) -> PResult<u64> {
    if v.is_empty() || (v.len() > 1 && v.starts_with('0')) {
        return Err(PlaybackError::invalid(
            "INVALID_SESSION",
            "revision is not canonical",
        ));
    }
    v.parse()
        .map_err(|_| PlaybackError::invalid("INVALID_SESSION", "revision is invalid"))
}
fn storage(_: anyhow::Error) -> PlaybackError {
    PlaybackError::invalid("PERSISTENCE_FAILED", "playback storage operation failed")
}
fn validate_stored(db: &Database, s: &PersistedSession) -> PResult<()> {
    let count = db.playback_count(&s.session_id).map_err(storage)?;
    if (count == 0) != (s.current_occurrence_id.is_none()) || (count == 0 && s.position_ms != 0) {
        return Err(PlaybackError::invalid(
            "INVALID_SESSION",
            "stored session invariants failed",
        ));
    }
    if let Some(id) = &s.current_occurrence_id {
        let mut after = None;
        let mut found = false;
        loop {
            let page = db
                .playback_page(&s.session_id, after, MAX_PAGE_SIZE)
                .map_err(storage)?;
            if page.is_empty() {
                break;
            }
            for o in &page {
                if Uuid::parse_str(&o.occurrence_id).is_err() || o.source.validate().is_err() {
                    return Err(PlaybackError::invalid(
                        "INVALID_SESSION",
                        "stored occurrence is invalid",
                    ));
                }
                found |= &o.occurrence_id == id;
                after = Some(o.ordinal);
            }
        }
        if !found {
            return Err(PlaybackError::invalid(
                "INVALID_SESSION",
                "current occurrence is missing",
            ));
        }
    }
    Ok(())
}
fn apply_inner(i: &mut Inner, p: &ApplySessionParams) -> PResult<ApplyResult> {
    require_schema(p.schema_version)?;
    if Uuid::parse_str(&p.command_id).is_err() {
        return Err(PlaybackError::invalid(
            "INVALID_SESSION",
            "commandId must be a UUID",
        ));
    }
    if p.instance_id != i.instance_id {
        return Err(PlaybackError::conflict(
            "INSTANCE_MISMATCH",
            "daemon instance changed",
        ));
    }
    if p.session_id != i.session.session_id {
        return Err(PlaybackError::conflict(
            "SESSION_MISMATCH",
            "session identity is stale",
        ));
    }
    if parse_revision(&p.expected_queue_revision)? != i.session.queue_revision {
        return Err(PlaybackError::conflict(
            "QUEUE_REVISION_CONFLICT",
            "queue revision is stale",
        ));
    }
    if i.restoration.status == "error" {
        return Err(PlaybackError::conflict(
            "RESTORE_FAILED",
            "stored playback state must be recovered first",
        ));
    }
    let mut next_session = i.session.clone();
    let mut all =
        i.db.playback_page(&i.session.session_id, None, usize::MAX)
            .map_err(storage)?;
    let mut assigned = Vec::new();
    let mut structural = false;
    match &p.operation {
        SessionOperation::ReplaceQueue { sources } => {
            validate_sources(sources)?;
            all.clear();
            assigned = make_occurrences(sources, 0);
            all.extend(assigned.clone());
            next_session.current_occurrence_id = all.first().map(|o| o.occurrence_id.clone());
            next_session.position_ms = 0;
            structural = true;
        }
        SessionOperation::AppendQueue { sources } => {
            validate_sources(sources)?;
            assigned = make_occurrences(sources, all.len() as u64);
            let was_empty = all.is_empty();
            all.extend(assigned.clone());
            if was_empty {
                next_session.current_occurrence_id = all.first().map(|o| o.occurrence_id.clone());
                next_session.position_ms = 0;
            }
            structural = true;
        }
        SessionOperation::SelectCurrent { occurrence_id } => {
            if !all.iter().any(|o| &o.occurrence_id == occurrence_id) {
                return Err(PlaybackError::invalid(
                    "INVALID_SESSION",
                    "selected occurrence is absent",
                ));
            }
            next_session.current_occurrence_id = Some(occurrence_id.clone());
            next_session.position_ms = 0;
        }
        SessionOperation::Clear => {
            all.clear();
            next_session.current_occurrence_id = None;
            next_session.position_ms = 0;
            structural = true;
        }
    }
    if structural {
        next_session.queue_revision = next_session
            .queue_revision
            .checked_add(1)
            .ok_or_else(|| PlaybackError::invalid("INVALID_SESSION", "queue revision overflow"))?;
    }
    next_session.state = if all.is_empty() {
        TransportState::Idle
    } else {
        TransportState::Paused
    };
    let next_sequence = i
        .state_sequence
        .checked_add(1)
        .ok_or_else(|| PlaybackError::invalid("INVALID_SESSION", "state sequence overflow"))?;
    i.db.persist_playback_structure(&next_session, &all)
        .map_err(storage)?;
    i.session = next_session;
    i.state_sequence = next_sequence;
    i.generation_id = Uuid::new_v4().to_string();
    i.progress = None;
    i.dirty = false;
    i.checkpointed_position_ms = i.session.position_ms;
    decorate_availability(&i.db, &mut assigned)?;
    Ok(ApplyResult {
        session_id: i.session.session_id.clone(),
        queue_revision: i.session.queue_revision.to_string(),
        state_sequence: i.state_sequence.to_string(),
        generation_id: i.generation_id.clone(),
        assigned_occurrences: assigned,
    })
}
fn validate_sources(s: &[TrackSource]) -> PResult<()> {
    if s.len() > MAX_INSERT_BATCH {
        return Err(PlaybackError::invalid(
            "INVALID_SESSION",
            "insert batch exceeds 200 sources",
        ));
    }
    for v in s {
        v.validate()
            .map_err(|m| PlaybackError::invalid("INVALID_SESSION", m))?;
    }
    Ok(())
}
fn make_occurrences(s: &[TrackSource], start: u64) -> Vec<Occurrence> {
    s.iter()
        .enumerate()
        .map(|(n, source)| Occurrence {
            occurrence_id: Uuid::new_v4().to_string(),
            ordinal: start + n as u64,
            source: source.clone(),
            availability: SourceAvailability::Unknown,
        })
        .collect()
}
fn snapshot(i: &Inner) -> PResult<SessionSnapshot> {
    let mut rows =
        i.db.playback_page(&i.session.session_id, None, DEFAULT_PAGE_SIZE + 1)
            .map_err(storage)?;
    let next = if rows.len() > DEFAULT_PAGE_SIZE {
        rows.truncate(DEFAULT_PAGE_SIZE);
        rows.last().map(|o| encode_cursor(&i.session, o.ordinal))
    } else {
        None
    };
    decorate_availability(&i.db, &mut rows)?;
    let mut current = if let Some(id) = &i.session.current_occurrence_id {
        i.db.playback_page(&i.session.session_id, None, usize::MAX)
            .map_err(storage)?
            .into_iter()
            .find(|o| &o.occurrence_id == id)
    } else {
        None
    };
    if let Some(current) = current.as_mut() {
        current.availability = availability(&i.db, &current.source.server_id)?;
    }
    Ok(SessionSnapshot {
        schema_version: SCHEMA_VERSION,
        instance_id: i.instance_id.clone(),
        session_id: i.session.session_id.clone(),
        queue_revision: i.session.queue_revision.to_string(),
        state_sequence: i.state_sequence.to_string(),
        generation_id: i.generation_id.clone(),
        state: i.session.state,
        current,
        position_ms: i.session.position_ms,
        checkpointed_position_ms: i.checkpointed_position_ms,
        persistence: i.persistence.clone(),
        restoration: i.restoration.clone(),
        total_occurrence_count: i
            .db
            .playback_count(&i.session.session_id)
            .map_err(storage)?,
        occurrences: rows,
        next_cursor: next,
    })
}
fn availability(db: &Database, server_id: &str) -> PResult<SourceAvailability> {
    db.has_portable_server(server_id)
        .map(|configured| {
            if configured {
                SourceAvailability::Unknown
            } else {
                SourceAvailability::NotConfigured
            }
        })
        .map_err(storage)
}
fn decorate_availability(db: &Database, rows: &mut [Occurrence]) -> PResult<()> {
    for occurrence in rows {
        occurrence.availability = availability(db, &occurrence.source.server_id)?;
    }
    Ok(())
}
fn encode_cursor(s: &PersistedSession, ordinal: u64) -> String {
    format!("{}:{}:{}", s.session_id, s.queue_revision, ordinal)
}
fn decode_cursor(v: &str, s: &PersistedSession) -> PResult<u64> {
    let mut x = v.rsplitn(3, ':');
    let ord = x.next().and_then(|v| v.parse().ok());
    let rev = x.next().and_then(|v| v.parse().ok());
    let sid = x.next();
    if sid != Some(s.session_id.as_str()) || rev != Some(s.queue_revision) || ord.is_none() {
        return Err(PlaybackError::conflict(
            "INVALID_CURSOR",
            "cursor does not belong to this snapshot",
        ));
    }
    Ok(ord.unwrap())
}
fn prune_dedup(i: &mut Inner) {
    while let Some(k) = i.dedup_order.front() {
        if i.dedup
            .get(k)
            .is_some_and(|v| v.at.elapsed() > Duration::from_secs(600))
        {
            let k = i.dedup_order.pop_front().unwrap();
            i.dedup.remove(&k);
        } else {
            break;
        }
    }
}
fn retain_dedup(i: &mut Inner, p: ApplySessionParams, r: PResult<ApplyResult>) {
    let k = p.command_id.clone();
    i.dedup.insert(
        k.clone(),
        DedupEntry {
            at: Instant::now(),
            payload: p,
            result: r,
        },
    );
    i.dedup_order.push_back(k);
    while i.dedup_order.len() > 1024 {
        if let Some(k) = i.dedup_order.pop_front() {
            i.dedup.remove(&k);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn params(s: &SessionSnapshot, op: SessionOperation) -> ApplySessionParams {
        ApplySessionParams {
            schema_version: 1,
            instance_id: s.instance_id.clone(),
            session_id: s.session_id.clone(),
            command_id: Uuid::new_v4().to_string(),
            expected_queue_revision: s.queue_revision.clone(),
            operation: op,
        }
    }
    #[test]
    fn paused_round_trip_preserves_repeats_and_identity() {
        let db = Arc::new(Database::memory().unwrap());
        let p = PlaybackSession::restore(db.clone(), "owner-a".into());
        let s = p.snapshot().unwrap();
        let source = TrackSource {
            server_id: "portable".into(),
            track_id: "track".into(),
        };
        let r = p
            .apply(params(
                &s,
                SessionOperation::ReplaceQueue {
                    sources: vec![source.clone(), source],
                },
            ))
            .unwrap();
        assert_ne!(
            r.assigned_occurrences[0].occurrence_id,
            r.assigned_occurrences[1].occurrence_id
        );
        let restored = PlaybackSession::restore(db, "owner-b".into())
            .snapshot()
            .unwrap();
        assert_eq!(restored.state, TransportState::Paused);
        assert_eq!(restored.occurrences, r.assigned_occurrences);
    }
    #[test]
    fn stale_revision_and_command_reuse_are_rejected() {
        let db = Arc::new(Database::memory().unwrap());
        let p = PlaybackSession::restore(db, "owner".into());
        let s = p.snapshot().unwrap();
        let mut cmd = params(&s, SessionOperation::Clear);
        p.apply(cmd.clone()).unwrap();
        assert_eq!(p.apply(cmd.clone()).unwrap().queue_revision, "1");
        cmd.operation = SessionOperation::AppendQueue { sources: vec![] };
        assert_eq!(p.apply(cmd).unwrap_err().code, "COMMAND_ID_REUSED");
        let stale = params(&s, SessionOperation::Clear);
        assert_eq!(p.apply(stale).unwrap_err().code, "QUEUE_REVISION_CONFLICT");
    }
    #[test]
    fn explicit_clear_is_durable_idle() {
        let db = Arc::new(Database::memory().unwrap());
        let p = PlaybackSession::restore(db.clone(), "a".into());
        let s = p.snapshot().unwrap();
        let source = TrackSource {
            server_id: "s".into(),
            track_id: "t".into(),
        };
        p.apply(params(
            &s,
            SessionOperation::ReplaceQueue {
                sources: vec![source],
            },
        ))
        .unwrap();
        let s = p.snapshot().unwrap();
        p.apply(params(&s, SessionOperation::Clear)).unwrap();
        let s = PlaybackSession::restore(db, "b".into()).snapshot().unwrap();
        assert_eq!(s.state, TransportState::Idle);
        assert!(s.current.is_none());
        assert!(s.occurrences.is_empty());
        assert_eq!(s.position_ms, 0);
    }
    #[test]
    fn progress_checkpoint_restores_paused_position() {
        let db = Arc::new(Database::memory().unwrap());
        let p = PlaybackSession::restore(db.clone(), "a".into());
        let s = p.snapshot().unwrap();
        let source = TrackSource {
            server_id: "offline-server".into(),
            track_id: "opaque".into(),
        };
        p.apply(params(
            &s,
            SessionOperation::ReplaceQueue {
                sources: vec![source],
            },
        ))
        .unwrap();
        let s = p.snapshot().unwrap();
        let id = s.current.as_ref().unwrap().occurrence_id.clone();
        p.report_progress(&s.generation_id, &id, 1, 4242).unwrap();
        assert_eq!(p.snapshot().unwrap().checkpointed_position_ms, 0);
        p.final_checkpoint().unwrap();
        let restored = PlaybackSession::restore(db, "b".into()).snapshot().unwrap();
        assert_eq!(restored.position_ms, 4242);
        assert_eq!(restored.state, TransportState::Paused);
    }
    #[test]
    fn pages_are_revision_bound_and_bounded() {
        let db = Arc::new(Database::memory().unwrap());
        let p = PlaybackSession::restore(db, "owner".into());
        let s = p.snapshot().unwrap();
        let sources = (0..200)
            .map(|n| TrackSource {
                server_id: "s".into(),
                track_id: n.to_string(),
            })
            .collect();
        p.apply(params(&s, SessionOperation::ReplaceQueue { sources }))
            .unwrap();
        let s = p.snapshot().unwrap();
        assert_eq!(s.occurrences.len(), 100);
        let page = p
            .list(ListOccurrencesParams {
                schema_version: 1,
                session_id: s.session_id.clone(),
                expected_queue_revision: s.queue_revision.clone(),
                cursor: s.next_cursor.clone(),
                limit: Some(200),
            })
            .unwrap();
        assert_eq!(page.occurrences.len(), 100);
        assert!(page.next_cursor.is_none());
        let bad = p
            .list(ListOccurrencesParams {
                schema_version: 1,
                session_id: s.session_id,
                expected_queue_revision: s.queue_revision,
                cursor: Some("foreign:0:0".into()),
                limit: None,
            })
            .unwrap_err();
        assert_eq!(bad.code, "INVALID_CURSOR");
    }
    #[test]
    fn contract_json_is_exact_camel_case() {
        let value = serde_json::to_value(TrackSource {
            server_id: "s".into(),
            track_id: "t".into(),
        })
        .unwrap();
        assert_eq!(value, serde_json::json!({"serverId":"s","trackId":"t"}));
        assert!(
            serde_json::from_value::<TrackSource>(
                serde_json::json!({"serverId":"s","trackId":"t","url":"secret"})
            )
            .is_err()
        );
    }

    #[test]
    fn real_file_restart_and_ten_thousand_occurrence_paging() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("playback.db");
        let db = Arc::new(Database::new(path.clone()).unwrap());
        db.init_playback().unwrap();
        let session = PersistedSession {
            session_id: Uuid::new_v4().to_string(),
            queue_revision: 7,
            checkpoint_sequence: 3,
            state: TransportState::Playing,
            current_occurrence_id: None,
            position_ms: 0,
        };
        let mut occurrences: Vec<_> = (0..10_000)
            .map(|ordinal| Occurrence {
                occurrence_id: Uuid::new_v4().to_string(),
                ordinal,
                source: TrackSource {
                    server_id: "offline".into(),
                    track_id: (ordinal % 17).to_string(),
                },
                availability: SourceAvailability::NotConfigured,
            })
            .collect();
        let mut session = session;
        session.current_occurrence_id = Some(occurrences[4321].occurrence_id.clone());
        session.position_ms = 9876;
        db.persist_playback_structure(&session, &occurrences)
            .unwrap();
        drop(db);

        let restored = PlaybackSession::restore(
            Arc::new(Database::new(path).unwrap()),
            "new-instance".into(),
        );
        let mut snapshot = restored.snapshot().unwrap();
        assert_eq!(snapshot.state, TransportState::Paused);
        assert_eq!(snapshot.position_ms, 9876);
        assert_eq!(snapshot.total_occurrence_count, 10_000);
        let mut seen = snapshot.occurrences.len();
        while let Some(cursor) = snapshot.next_cursor.take() {
            let page = restored
                .list(ListOccurrencesParams {
                    schema_version: 1,
                    session_id: snapshot.session_id.clone(),
                    expected_queue_revision: snapshot.queue_revision.clone(),
                    cursor: Some(cursor),
                    limit: Some(200),
                })
                .unwrap();
            seen += page.occurrences.len();
            snapshot.next_cursor = page.next_cursor;
        }
        assert_eq!(seen, 10_000);
        occurrences.clear();
    }

    #[test]
    fn unsupported_version_preserves_evidence_and_blocks_mutation() {
        let db = Arc::new(Database::memory().unwrap());
        db.init_playback().unwrap();
        {
            let conn = db.conn.lock().unwrap();
            conn.execute("UPDATE playback_schema SET version=99", [])
                .unwrap();
        }
        let playback = PlaybackSession::restore(db.clone(), "owner".into());
        let snapshot = playback.snapshot().unwrap();
        assert_eq!(snapshot.restoration.status, "error");
        assert_eq!(
            snapshot.restoration.code.as_deref(),
            Some("UNSUPPORTED_PLAYBACK_VERSION")
        );
        assert_eq!(
            db.conn
                .lock()
                .unwrap()
                .query_row("SELECT version FROM playback_schema", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            99
        );
        assert_eq!(
            playback
                .apply(params(&snapshot, SessionOperation::Clear))
                .unwrap_err()
                .code,
            "RESTORE_FAILED"
        );
        playback.final_checkpoint().unwrap();
        assert_eq!(
            db.conn
                .lock()
                .unwrap()
                .query_row("SELECT version FROM playback_schema", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            99
        );
    }
}
