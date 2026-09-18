use super::model::{Occurrence, PersistedSession, SourceAvailability, TrackSource, TransportState};
use crate::db::Database;
use anyhow::{Result, anyhow};
use rusqlite::{OptionalExtension, params};

pub const PERSISTENCE_VERSION: i64 = 3;

impl Database {
    pub fn has_portable_server(&self, server_id: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        Ok(conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM server_config WHERE server_id=?1)",
            [server_id],
            |row| row.get(0),
        )?)
    }

    pub fn init_playback(&self) -> Result<()> {
        self.init_playback_inner(false)
    }

    fn init_playback_inner(&self, fail_before_version_commit: bool) -> Result<()> {
        let mut conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.busy_timeout(std::time::Duration::from_millis(1000))?;
        let tx = conn.transaction()?;
        let version: Option<i64> = tx
            .query_row(
                "SELECT version FROM playback_schema WHERE singleton_id=1",
                [],
                |r| r.get(0),
            )
            .optional()
            .or_else(|error| {
                if error.to_string().contains("no such table") {
                    Ok(None)
                } else {
                    Err(error)
                }
            })?;
        if let Some(version) = version
            && !(1..=PERSISTENCE_VERSION).contains(&version)
        {
            return Err(anyhow!("UNSUPPORTED_PLAYBACK_VERSION"));
        }
        tx.execute_batch("CREATE TABLE IF NOT EXISTS playback_schema (singleton_id INTEGER PRIMARY KEY CHECK(singleton_id=1), version INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS playback_sessions (singleton_id INTEGER PRIMARY KEY CHECK(singleton_id=1), session_id TEXT NOT NULL, queue_revision INTEGER NOT NULL CHECK(queue_revision>=0), checkpoint_sequence INTEGER NOT NULL CHECK(checkpoint_sequence>=0), transport_state TEXT NOT NULL, current_occurrence_id TEXT, position_ms INTEGER NOT NULL CHECK(position_ms>=0), album_context_json TEXT);
            CREATE TABLE IF NOT EXISTS playback_occurrences (session_id TEXT NOT NULL, occurrence_id TEXT NOT NULL UNIQUE, ordinal INTEGER NOT NULL CHECK(ordinal>=0), server_id TEXT NOT NULL, track_id TEXT NOT NULL, PRIMARY KEY(session_id, ordinal));")?;
        if version != Some(PERSISTENCE_VERSION) {
            let has_outcome = {
                let mut statement = tx.prepare("PRAGMA table_info(playback_occurrences)")?;
                statement
                    .query_map([], |row| row.get::<_, String>(1))?
                    .collect::<rusqlite::Result<Vec<_>>>()?
                    .iter()
                    .any(|name| name == "outcome")
            };
            if !has_outcome {
                tx.execute("ALTER TABLE playback_occurrences ADD COLUMN outcome TEXT CHECK(outcome IN ('naturalCompletion','explicitSkip','technicalFailure') OR outcome IS NULL)", [])?;
            }
        }
        if fail_before_version_commit {
            return Err(anyhow!("injected playback migration failure"));
        }
        let has_failure_code = {
            let mut statement = tx.prepare("PRAGMA table_info(playback_occurrences)")?;
            statement
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<rusqlite::Result<Vec<_>>>()?
                .iter()
                .any(|name| name == "failure_code")
        };
        if !has_failure_code {
            tx.execute(
                "ALTER TABLE playback_occurrences ADD COLUMN failure_code TEXT",
                [],
            )?;
        }
        let has_album_context = {
            let mut statement = tx.prepare("PRAGMA table_info(playback_sessions)")?;
            statement
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<rusqlite::Result<Vec<_>>>()?
                .iter()
                .any(|name| name == "album_context_json")
        };
        if !has_album_context {
            tx.execute(
                "ALTER TABLE playback_sessions ADD COLUMN album_context_json TEXT",
                [],
            )?;
        }
        tx.execute(
            "INSERT OR IGNORE INTO playback_schema(singleton_id,version) VALUES(1,?1)",
            [PERSISTENCE_VERSION],
        )?;
        tx.execute(
            "UPDATE playback_schema SET version=?1 WHERE singleton_id=1 AND version<>?1",
            [PERSISTENCE_VERSION],
        )?;
        tx.commit()?;
        Ok(())
    }

    #[cfg(test)]
    pub fn init_playback_with_injected_failure(&self) -> Result<()> {
        self.init_playback_inner(true)
    }

    pub fn load_playback_session(&self) -> Result<Option<PersistedSession>> {
        self.init_playback()?;
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let loaded = conn.query_row("SELECT session_id,queue_revision,checkpoint_sequence,transport_state,current_occurrence_id,position_ms,album_context_json FROM playback_sessions WHERE singleton_id=1", [], |r| {
            let state: String = r.get(3)?;
            let state = match state.as_str() { "idle"=>TransportState::Idle, "paused"=>TransportState::Paused, "buffering"=>TransportState::Buffering, "playing"=>TransportState::Playing, "stopping"=>TransportState::Stopping, _=>return Err(rusqlite::Error::InvalidQuery) };
            let context_json: Option<String> = r.get(6)?;
            let album_context = context_json.map(|json| serde_json::from_str(&json).map_err(|error| rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(error)))).transpose()?;
            Ok(PersistedSession { session_id:r.get(0)?, queue_revision:nonnegative(r,1)?, checkpoint_sequence:nonnegative(r,2)?, state, current_occurrence_id:r.get(4)?, position_ms:nonnegative(r,5)?, album_context })
        }).optional()?;
        if loaded.is_none() {
            let count: i64 =
                conn.query_row("SELECT COUNT(*) FROM playback_occurrences", [], |r| {
                    r.get(0)
                })?;
            if count != 0 {
                return Err(anyhow!("INVALID_SESSION"));
            }
        }
        Ok(loaded)
    }

    pub fn validate_playback_session(&self, session: &PersistedSession) -> Result<()> {
        if uuid::Uuid::parse_str(&session.session_id).is_err()
            || session.position_ms > 9_007_199_254_740_991
            || session.queue_revision > i64::MAX as u64
            || session.checkpoint_sequence > i64::MAX as u64
        {
            return Err(anyhow!("INVALID_SESSION"));
        }
        if session
            .album_context
            .as_ref()
            .is_some_and(|context| context.validate().is_err())
        {
            return Err(anyhow!("INVALID_SESSION"));
        }
        let mut conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let tx = conn.transaction()?;
        let count: i64 = tx.query_row("SELECT COUNT(*) FROM playback_occurrences", [], |r| {
            r.get(0)
        })?;
        if (count == 0) != session.current_occurrence_id.is_none()
            || (count == 0) != (session.state == TransportState::Idle)
            || (count == 0 && session.position_ms != 0)
        {
            return Err(anyhow!("INVALID_SESSION"));
        }
        let duplicates: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM playback_occurrences GROUP BY occurrence_id HAVING COUNT(*) > 1)", [], |r|r.get(0))?;
        if duplicates {
            return Err(anyhow!("INVALID_SESSION"));
        }
        let invalid_outcomes: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM playback_occurrences WHERE outcome NOT IN ('naturalCompletion','explicitSkip','technicalFailure') OR (failure_code IS NOT NULL AND (length(failure_code)=0 OR length(failure_code)>128 OR failure_code GLOB '*[^A-Z0-9_]*')))", [], |r| r.get(0))?;
        if invalid_outcomes {
            return Err(anyhow!("INVALID_SESSION"));
        }
        let mut after = -1i64;
        let mut found = false;
        let mut scanned = 0i64;
        let mut album_membership = super::model::album_membership_hasher();
        loop {
            let mut stmt = tx.prepare("SELECT occurrence_id,ordinal,server_id,track_id FROM playback_occurrences WHERE session_id=?1 AND ordinal>?2 ORDER BY ordinal LIMIT 200")?;
            let rows = stmt
                .query_map(params![session.session_id, after], occurrence_from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            if rows.is_empty() {
                break;
            }
            for row in rows {
                if uuid::Uuid::parse_str(&row.occurrence_id).is_err()
                    || row.source.validate().is_err()
                    || row.ordinal > i64::MAX as u64
                    || row.ordinal as i64 <= after
                {
                    return Err(anyhow!("INVALID_SESSION"));
                }
                found |= Some(&row.occurrence_id) == session.current_occurrence_id.as_ref();
                if session.album_context.as_ref().is_some_and(|context| {
                    row.ordinal < context.member_count
                        && row.source.server_id != context.source.server_id
                }) {
                    return Err(anyhow!("INVALID_SESSION"));
                }
                if session
                    .album_context
                    .as_ref()
                    .is_some_and(|context| row.ordinal < context.member_count)
                {
                    super::model::hash_album_member(&mut album_membership, &row.source);
                }
                after = row.ordinal as i64;
                scanned += 1;
            }
        }
        // Also detects orphaned/foreign-session rows and negative ordinals hidden
        // by the keyset predicate; no whole-history collection is retained.
        if scanned != count
            || (count > 0 && !found)
            || session
                .album_context
                .as_ref()
                .is_some_and(|context| context.member_count > count as u64)
        {
            return Err(anyhow!("INVALID_SESSION"));
        }
        if session.album_context.as_ref().is_some_and(|context| {
            album_membership.finalize().to_hex().as_str() != context.membership_digest
        }) {
            return Err(anyhow!("INVALID_SESSION"));
        }
        tx.commit()?;
        Ok(())
    }

    pub fn playback_count(&self, session_id: &str) -> Result<u64> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM playback_occurrences WHERE session_id=?1",
            [session_id],
            |r| r.get::<_, i64>(0),
        )? as u64)
    }

    pub fn playback_page(
        &self,
        session_id: &str,
        after: Option<u64>,
        limit: usize,
    ) -> Result<Vec<Occurrence>> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn.prepare("SELECT occurrence_id,ordinal,server_id,track_id FROM playback_occurrences WHERE session_id=?1 AND ordinal>?2 ORDER BY ordinal LIMIT ?3")?;
        let rows = stmt.query_map(
            params![
                session_id,
                after.map(i64::try_from).transpose()?.unwrap_or(-1),
                limit as i64
            ],
            |r| {
                Ok(Occurrence {
                    occurrence_id: r.get(0)?,
                    ordinal: nonnegative(r, 1)?,
                    source: TrackSource {
                        server_id: r.get(2)?,
                        track_id: r.get(3)?,
                    },
                    availability: SourceAvailability::Unknown,
                })
            },
        )?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn playback_occurrence(
        &self,
        session_id: &str,
        occurrence_id: &str,
    ) -> Result<Option<Occurrence>> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.query_row(
            "SELECT occurrence_id,ordinal,server_id,track_id FROM playback_occurrences WHERE session_id=?1 AND occurrence_id=?2",
            params![session_id, occurrence_id],
            occurrence_from_row,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn playback_successor(&self, session_id: &str, ordinal: u64) -> Result<Option<Occurrence>> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.query_row(
            "SELECT occurrence_id,ordinal,server_id,track_id FROM playback_occurrences WHERE session_id=?1 AND ordinal>?2 ORDER BY ordinal LIMIT 1",
            params![session_id, i64::try_from(ordinal)?], occurrence_from_row,
        ).optional().map_err(Into::into)
    }

    pub fn playback_next_ordinal(&self, session_id: &str) -> Result<u64> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let last: Option<i64> = conn.query_row(
            "SELECT MAX(ordinal) FROM playback_occurrences WHERE session_id=?1",
            [session_id],
            |row| row.get(0),
        )?;
        match last {
            Some(value) => u64::try_from(value)?
                .checked_add(1)
                .filter(|n| *n <= i64::MAX as u64)
                .ok_or_else(|| anyhow!("playback ordinal overflow")),
            None => Ok(0),
        }
    }

    pub fn persist_playback_structure(
        &self,
        session: &PersistedSession,
        occurrences: &[Occurrence],
    ) -> Result<()> {
        self.persist_playback_structure_inner(session, occurrences, false)
    }

    fn persist_playback_structure_inner(
        &self,
        session: &PersistedSession,
        occurrences: &[Occurrence],
        fail_after_delete: bool,
    ) -> Result<()> {
        let mut conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let tx = conn.transaction()?;
        let album_context_json = album_context_json(session)?;
        tx.execute("INSERT INTO playback_sessions(singleton_id,session_id,queue_revision,checkpoint_sequence,transport_state,current_occurrence_id,position_ms,album_context_json) VALUES(1,?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(singleton_id) DO UPDATE SET session_id=excluded.session_id,queue_revision=excluded.queue_revision,checkpoint_sequence=excluded.checkpoint_sequence,transport_state=excluded.transport_state,current_occurrence_id=excluded.current_occurrence_id,position_ms=excluded.position_ms,album_context_json=excluded.album_context_json", params![session.session_id,session.queue_revision as i64,session.checkpoint_sequence as i64,state_name(session.state),session.current_occurrence_id,session.position_ms as i64,album_context_json])?;
        tx.execute(
            "DELETE FROM playback_occurrences WHERE session_id=?1",
            [&session.session_id],
        )?;
        if fail_after_delete {
            return Err(anyhow!("injected interrupted playback transaction"));
        }
        {
            let mut stmt=tx.prepare("INSERT INTO playback_occurrences(session_id,occurrence_id,ordinal,server_id,track_id) VALUES(?1,?2,?3,?4,?5)")?;
            for o in occurrences {
                stmt.execute(params![
                    session.session_id,
                    o.occurrence_id,
                    o.ordinal as i64,
                    o.source.server_id,
                    o.source.track_id
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    #[cfg(test)]
    pub fn persist_playback_structure_with_injected_failure(
        &self,
        session: &PersistedSession,
        occurrences: &[Occurrence],
    ) -> Result<()> {
        self.persist_playback_structure_inner(session, occurrences, true)
    }

    pub fn append_playback_occurrences(
        &self,
        session: &PersistedSession,
        occurrences: &[Occurrence],
    ) -> Result<()> {
        let mut conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let tx = conn.transaction()?;
        update_session(&tx, session)?;
        insert_occurrences(&tx, &session.session_id, occurrences)?;
        tx.commit()?;
        Ok(())
    }

    pub fn select_playback_current(&self, session: &PersistedSession) -> Result<()> {
        let mut conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let tx = conn.transaction()?;
        let current = session
            .current_occurrence_id
            .as_deref()
            .ok_or_else(|| anyhow!("current occurrence is required"))?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM playback_occurrences WHERE session_id=?1 AND occurrence_id=?2)",
            params![session.session_id, current],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(anyhow!("selected playback occurrence is absent"));
        }
        update_session(&tx, session)?;
        tx.commit()?;
        Ok(())
    }

    pub fn persist_playback_terminal(
        &self,
        session: &PersistedSession,
        departed_occurrence_id: &str,
        outcome: &str,
        failure_code: Option<&str>,
    ) -> Result<()> {
        if failure_code.is_some_and(|code| {
            code.is_empty()
                || code.len() > 128
                || !code
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        }) {
            return Err(anyhow!("invalid playback failure code"));
        }
        let mut conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let tx = conn.transaction()?;
        let changed = tx.execute(
            "UPDATE playback_occurrences SET outcome=?1,failure_code=?4 WHERE session_id=?2 AND occurrence_id=?3 AND (outcome IS NULL OR (?1='explicitSkip' AND outcome='technicalFailure'))",
            params![outcome, session.session_id, departed_occurrence_id, failure_code],
        )?;
        if changed != 1 {
            return Err(anyhow!("terminal playback outcome was already consumed"));
        }
        update_session(&tx, session)?;
        tx.commit()?;
        Ok(())
    }

    /// A new attempt keeps occurrence identity and the last diagnostic, but
    /// releases the terminal disposition so that recovery can finish normally.
    pub fn reset_playback_attempt(&self, session: &PersistedSession) -> Result<()> {
        let mut conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let tx = conn.transaction()?;
        let changed = tx.execute(
            "UPDATE playback_occurrences SET outcome=NULL WHERE session_id=?1 AND occurrence_id=?2",
            params![session.session_id, session.current_occurrence_id],
        )?;
        if changed != 1 {
            return Err(anyhow!("current playback occurrence is absent"));
        }
        update_session(&tx, session)?;
        tx.commit()?;
        Ok(())
    }

    pub fn playback_outcome(&self, occurrence_id: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.query_row(
            "SELECT outcome FROM playback_occurrences WHERE occurrence_id=?1",
            [occurrence_id],
            |row| row.get(0),
        )
        .optional()
        .map(|value| value.flatten())
        .map_err(Into::into)
    }

    pub fn clear_playback_failure(&self, session_id: &str, occurrence_id: &str) -> Result<()> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner()).execute(
            "UPDATE playback_occurrences SET failure_code=NULL WHERE session_id=?1 AND occurrence_id=?2 AND outcome IS NULL AND failure_code IS NOT NULL",
            params![session_id, occurrence_id],
        )?;
        Ok(())
    }

    pub fn clear_playback_session(&self, session: &PersistedSession) -> Result<()> {
        let mut conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM playback_occurrences WHERE session_id=?1",
            [&session.session_id],
        )?;
        update_session(&tx, session)?;
        tx.commit()?;
        Ok(())
    }

    pub fn checkpoint_playback_position(
        &self,
        session_id: &str,
        sequence: u64,
        position_ms: u64,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let changed=conn.execute("UPDATE playback_sessions SET checkpoint_sequence=?1,position_ms=?2 WHERE singleton_id=1 AND session_id=?3 AND checkpoint_sequence<=?1",params![sequence as i64,position_ms as i64,session_id])?;
        if changed != 1 {
            return Err(anyhow!("PERSISTENCE_FAILED"));
        }
        Ok(())
    }
}

fn occurrence_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Occurrence> {
    Ok(Occurrence {
        occurrence_id: row.get(0)?,
        ordinal: nonnegative(row, 1)?,
        source: TrackSource {
            server_id: row.get(2)?,
            track_id: row.get(3)?,
        },
        availability: SourceAvailability::Unknown,
    })
}

fn nonnegative(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(index)?;
    u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, value))
}

fn update_session(tx: &rusqlite::Transaction<'_>, session: &PersistedSession) -> Result<()> {
    let album_context_json = album_context_json(session)?;
    let changed = tx.execute(
        "UPDATE playback_sessions SET session_id=?1,queue_revision=?2,checkpoint_sequence=?3,transport_state=?4,current_occurrence_id=?5,position_ms=?6,album_context_json=?7 WHERE singleton_id=1",
        params![session.session_id, session.queue_revision as i64, session.checkpoint_sequence as i64, state_name(session.state), session.current_occurrence_id, session.position_ms as i64, album_context_json],
    )?;
    if changed != 1 {
        return Err(anyhow!("playback session is absent"));
    }
    Ok(())
}

fn album_context_json(session: &PersistedSession) -> Result<Option<String>> {
    session
        .album_context
        .as_ref()
        .map(|context| {
            context.validate().map_err(|message| anyhow!(message))?;
            serde_json::to_string(context).map_err(Into::into)
        })
        .transpose()
}

fn insert_occurrences(
    tx: &rusqlite::Transaction<'_>,
    session_id: &str,
    occurrences: &[Occurrence],
) -> Result<()> {
    let mut stmt = tx.prepare("INSERT INTO playback_occurrences(session_id,occurrence_id,ordinal,server_id,track_id) VALUES(?1,?2,?3,?4,?5)")?;
    for occurrence in occurrences {
        stmt.execute(params![
            session_id,
            occurrence.occurrence_id,
            occurrence.ordinal as i64,
            occurrence.source.server_id,
            occurrence.source.track_id
        ])?;
    }
    Ok(())
}

fn state_name(state: TransportState) -> &'static str {
    match state {
        TransportState::Idle => "idle",
        TransportState::Paused => "paused",
        TransportState::Buffering => "buffering",
        TransportState::Playing => "playing",
        TransportState::Stopping => "stopping",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn session(revision: u64, current: Option<String>, position_ms: u64) -> PersistedSession {
        PersistedSession {
            session_id: Uuid::new_v4().to_string(),
            queue_revision: revision,
            checkpoint_sequence: 0,
            state: if current.is_some() {
                TransportState::Paused
            } else {
                TransportState::Idle
            },
            current_occurrence_id: current,
            position_ms,
            album_context: None,
        }
    }

    fn occurrence(session_id: &str, ordinal: u64, track_id: &str) -> Occurrence {
        let _ = session_id;
        Occurrence {
            occurrence_id: Uuid::new_v4().to_string(),
            ordinal,
            source: TrackSource {
                server_id: "portable-server".into(),
                track_id: track_id.into(),
            },
            availability: SourceAvailability::NotConfigured,
        }
    }

    #[test]
    fn album_context_migrates_and_round_trips_atomically() {
        let db = Database::memory().unwrap();
        db.init_playback().unwrap();
        let first = occurrence("ignored", 0, "first");
        let second = occurrence("ignored", 1, "second");
        let membership_digest =
            super::super::model::album_membership_digest([&first.source, &second.source]);
        let mut committed = session(1, Some(first.occurrence_id.clone()), 0);
        committed.album_context = Some(super::super::model::FrozenAlbumContext {
            source: super::super::model::AlbumSource {
                server_id: "portable-server".into(),
                album_id: "album".into(),
            },
            member_count: 2,
            membership_digest,
            policy: super::super::loudness::AlbumLoudnessPolicy {
                version: super::super::loudness::ALBUM_LOUDNESS_POLICY_VERSION,
                scalar_bits: 0.75f32.to_bits(),
                gain_db_bits: Some(0.0f64.to_bits()),
                peak_bits: Some((super::super::loudness::SAMPLE_PEAK_CEILING / 0.75).to_bits()),
                reason: super::super::loudness::AlbumLoudnessReason::OpenSubsonicReplayGain,
            },
        });
        db.persist_playback_structure(&committed, &[first.clone(), second.clone()])
            .unwrap();

        let restored = db.load_playback_session().unwrap().unwrap();
        assert_eq!(restored.album_context, committed.album_context);
        let context = restored.album_context.unwrap();
        assert_eq!(context.scalar_for(&first), 0.75);
        let appended = occurrence("ignored", 2, "appended");
        assert_eq!(context.scalar_for(&appended), 1.0);
        db.conn
            .lock()
            .unwrap()
            .execute(
                "UPDATE playback_occurrences SET track_id='replacement' WHERE ordinal=1",
                [],
            )
            .unwrap();
        assert!(db.validate_playback_session(&committed).is_err());
        let version: i64 = db
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT version FROM playback_schema WHERE singleton_id=1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version, 3);
    }

    #[test]
    fn v2_session_migrates_to_unity_without_inventing_album_membership() {
        let db = Database::memory().unwrap();
        db.conn
            .lock()
            .unwrap()
            .execute_batch(
                "CREATE TABLE playback_schema (singleton_id INTEGER PRIMARY KEY, version INTEGER NOT NULL);
                 INSERT INTO playback_schema VALUES(1,2);
                 CREATE TABLE playback_sessions (singleton_id INTEGER PRIMARY KEY, session_id TEXT NOT NULL, queue_revision INTEGER NOT NULL, checkpoint_sequence INTEGER NOT NULL, transport_state TEXT NOT NULL, current_occurrence_id TEXT, position_ms INTEGER NOT NULL);
                 CREATE TABLE playback_occurrences (session_id TEXT NOT NULL, occurrence_id TEXT NOT NULL UNIQUE, ordinal INTEGER NOT NULL, server_id TEXT NOT NULL, track_id TEXT NOT NULL, outcome TEXT, failure_code TEXT, PRIMARY KEY(session_id, ordinal));",
            )
            .unwrap();
        db.init_playback().unwrap();
        let columns: Vec<String> = db
            .conn
            .lock()
            .unwrap()
            .prepare("PRAGMA table_info(playback_sessions)")
            .unwrap()
            .query_map([], |row| row.get(1))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert!(columns.iter().any(|column| column == "album_context_json"));
        let version: i64 = db
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT version FROM playback_schema WHERE singleton_id=1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version, PERSISTENCE_VERSION);
    }

    #[test]
    fn corrupt_or_future_album_policy_is_rejected_without_reinterpretation() {
        let db = Database::memory().unwrap();
        db.init_playback().unwrap();
        let current = occurrence("ignored", 0, "track");
        let committed = session(1, Some(current.occurrence_id.clone()), 0);
        db.persist_playback_structure(&committed, &[current])
            .unwrap();
        db.conn
            .lock()
            .unwrap()
            .execute(
                "UPDATE playback_sessions SET album_context_json=?1 WHERE singleton_id=1",
                ["{malformed"],
            )
            .unwrap();
        assert!(db.load_playback_session().is_err());

        let future = serde_json::json!({
            "source":{"serverId":"portable-server","albumId":"album"},
            "memberCount":1,
            "membershipDigest":"0000000000000000000000000000000000000000000000000000000000000000",
            "policy":{"version":99,"scalarBits":1.0f32.to_bits(),"gainDbBits":null,
                "peakBits":null,"reason":"metadataAbsent"}
        });
        db.conn
            .lock()
            .unwrap()
            .execute(
                "UPDATE playback_sessions SET album_context_json=?1 WHERE singleton_id=1",
                [future.to_string()],
            )
            .unwrap();
        let loaded = db.load_playback_session().unwrap().unwrap();
        assert!(db.validate_playback_session(&loaded).is_err());
    }

    #[test]
    fn migration_failure_rolls_back_ddl_and_version_atomically() {
        let db = Database::memory().unwrap();
        assert!(db.init_playback_with_injected_failure().is_err());
        let conn = db.conn.lock().unwrap();
        for table in [
            "playback_schema",
            "playback_sessions",
            "playback_occurrences",
        ] {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 0, "{table} escaped the failed migration");
        }
        drop(conn);
        db.init_playback().unwrap();
        assert!(db.load_playback_session().unwrap().is_none());
    }

    #[test]
    fn interrupted_structural_transaction_retains_previous_coherent_commit() {
        let db = Database::memory().unwrap();
        db.init_playback().unwrap();
        let first = occurrence("ignored", 0, "old-track");
        let mut committed = session(4, Some(first.occurrence_id.clone()), 1234);
        db.persist_playback_structure(&committed, std::slice::from_ref(&first))
            .unwrap();

        let replacement = occurrence("ignored", 0, "replacement-track");
        committed.queue_revision = 5;
        committed.current_occurrence_id = Some(replacement.occurrence_id.clone());
        committed.position_ms = 0;
        assert!(
            db.persist_playback_structure_with_injected_failure(
                &committed,
                std::slice::from_ref(&replacement),
            )
            .is_err()
        );

        let restored = db.load_playback_session().unwrap().unwrap();
        assert_eq!(restored.queue_revision, 4);
        assert_eq!(
            restored.current_occurrence_id.as_deref(),
            Some(first.occurrence_id.as_str())
        );
        assert_eq!(restored.position_ms, 1234);
        let rows = db.playback_page(&restored.session_id, None, 200).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].source.track_id, "old-track");
    }

    #[test]
    fn failed_checkpoint_preserves_committed_position_and_can_retry() {
        let db = Database::memory().unwrap();
        db.init_playback().unwrap();
        let current = occurrence("ignored", 0, "track");
        let committed = session(1, Some(current.occurrence_id.clone()), 1000);
        db.persist_playback_structure(&committed, &[current])
            .unwrap();
        {
            let conn = db.conn.lock().unwrap();
            conn.execute_batch(
                "CREATE TEMP TRIGGER fail_playback_checkpoint
                 BEFORE UPDATE OF position_ms ON playback_sessions
                 BEGIN SELECT RAISE(ABORT, 'injected checkpoint failure'); END;",
            )
            .unwrap();
        }
        assert!(
            db.checkpoint_playback_position(&committed.session_id, 1, 2000)
                .is_err()
        );
        let unchanged = db.load_playback_session().unwrap().unwrap();
        assert_eq!(unchanged.position_ms, 1000);
        assert_eq!(unchanged.checkpoint_sequence, 0);
        db.conn
            .lock()
            .unwrap()
            .execute_batch("DROP TRIGGER fail_playback_checkpoint")
            .unwrap();
        db.checkpoint_playback_position(&committed.session_id, 1, 2000)
            .unwrap();
        let retried = db.load_playback_session().unwrap().unwrap();
        assert_eq!(retried.position_ms, 2000);
        assert_eq!(retried.checkpoint_sequence, 1);
    }

    #[test]
    fn killed_process_mid_transaction_restores_previous_commit() {
        let temp = tempfile::tempdir().unwrap();
        let database_path = temp.path().join("interrupted.db");
        let ready_path = temp.path().join("transaction-open");
        let db = Database::new(database_path.clone()).unwrap();
        db.init_playback().unwrap();
        let current = occurrence("ignored", 0, "committed-track");
        let committed = session(8, Some(current.occurrence_id.clone()), 3333);
        db.persist_playback_structure(&committed, std::slice::from_ref(&current))
            .unwrap();
        drop(db);

        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "playback::persistence::tests::open_transaction_crash_fixture",
                "--exact",
                "--nocapture",
            ])
            .env("HIFIMULE_PLAYBACK_CRASH_DB", &database_path)
            .env("HIFIMULE_PLAYBACK_CRASH_READY", &ready_path)
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !ready_path.exists() {
            assert!(
                std::time::Instant::now() < deadline,
                "child did not open its transaction"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        child.kill().unwrap();
        child.wait().unwrap();

        let reopened = Database::new(database_path).unwrap();
        let restored = reopened.load_playback_session().unwrap().unwrap();
        assert_eq!(restored.queue_revision, 8);
        assert_eq!(restored.position_ms, 3333);
        assert_eq!(
            restored.current_occurrence_id.as_deref(),
            Some(current.occurrence_id.as_str())
        );
        let rows = reopened
            .playback_page(&restored.session_id, None, 200)
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].source.track_id, "committed-track");
    }

    #[test]
    fn open_transaction_crash_fixture() {
        let (Ok(database_path), Ok(ready_path)) = (
            std::env::var("HIFIMULE_PLAYBACK_CRASH_DB"),
            std::env::var("HIFIMULE_PLAYBACK_CRASH_READY"),
        ) else {
            return;
        };
        let db = Database::new(database_path.into()).unwrap();
        let mut conn = db.conn.lock().unwrap();
        let tx = conn.transaction().unwrap();
        tx.execute("DELETE FROM playback_occurrences", []).unwrap();
        tx.execute(
            "UPDATE playback_sessions SET queue_revision=999, current_occurrence_id=NULL, position_ms=0 WHERE singleton_id=1",
            [],
        )
        .unwrap();
        std::fs::write(ready_path, b"open").unwrap();
        std::thread::sleep(std::time::Duration::from_secs(30));
        tx.commit().unwrap();
    }
}
