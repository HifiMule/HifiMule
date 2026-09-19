use super::model::{
    AuditionOutcome, Occurrence, PersistedAudition, PersistedSession, SourceAvailability,
    TrackSource, TransportState,
};
use crate::db::Database;
use anyhow::{Result, anyhow};
use rusqlite::{OptionalExtension, params};

pub const PERSISTENCE_VERSION: i64 = 5;

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
            CREATE TABLE IF NOT EXISTS playback_sessions (singleton_id INTEGER PRIMARY KEY CHECK(singleton_id=1), session_id TEXT NOT NULL, queue_revision INTEGER NOT NULL CHECK(queue_revision>=0), checkpoint_sequence INTEGER NOT NULL CHECK(checkpoint_sequence>=0), transport_state TEXT NOT NULL, current_occurrence_id TEXT, position_ms INTEGER NOT NULL CHECK(position_ms>=0), album_context_json TEXT, queue_kind TEXT NOT NULL DEFAULT 'manual' CHECK(queue_kind IN ('album','manual')), current_gain_bits INTEGER NOT NULL DEFAULT 1065353216 CHECK(current_gain_bits>=0 AND current_gain_bits<=4294967295), current_qualified_suffix TEXT);
            CREATE TABLE IF NOT EXISTS playback_occurrences (session_id TEXT NOT NULL, occurrence_id TEXT NOT NULL UNIQUE, ordinal INTEGER NOT NULL CHECK(ordinal>=0), server_id TEXT NOT NULL, track_id TEXT NOT NULL, PRIMARY KEY(session_id, ordinal));
            CREATE TABLE IF NOT EXISTS playback_audition (singleton_id INTEGER PRIMARY KEY CHECK(singleton_id=1), audition_id TEXT NOT NULL UNIQUE, parent_session_id TEXT NOT NULL, server_id TEXT NOT NULL, track_id TEXT NOT NULL, position_ms INTEGER NOT NULL CHECK(position_ms>=0), transport_state TEXT NOT NULL, saved_main_occurrence_id TEXT, saved_main_position_ms INTEGER NOT NULL CHECK(saved_main_position_ms>=0), saved_main_intent TEXT NOT NULL, resume_inhibited INTEGER NOT NULL CHECK(resume_inhibited IN (0,1)), contiguous_heard_ms INTEGER NOT NULL CHECK(contiguous_heard_ms>=0), coverage_unknown INTEGER NOT NULL CHECK(coverage_unknown IN (0,1)), seek_discontinuous INTEGER NOT NULL CHECK(seek_discontinuous IN (0,1)));
            CREATE TABLE IF NOT EXISTS playback_audition_outcomes (outcome_id INTEGER PRIMARY KEY AUTOINCREMENT, audition_id TEXT NOT NULL UNIQUE, parent_session_id TEXT NOT NULL, server_id TEXT NOT NULL, track_id TEXT NOT NULL, disposition TEXT NOT NULL CHECK(disposition IN ('naturalCompletion','stopped','returned','replaced','superseded','technicalFailure','interrupted')), terminal_position_ms INTEGER NOT NULL CHECK(terminal_position_ms>=0), duration_ms INTEGER CHECK(duration_ms>=0), failure_code TEXT, contiguous_heard_ms INTEGER NOT NULL CHECK(contiguous_heard_ms>=0), coverage_unknown INTEGER NOT NULL CHECK(coverage_unknown IN (0,1)), seek_discontinuous INTEGER NOT NULL CHECK(seek_discontinuous IN (0,1)), fully_heard INTEGER NOT NULL CHECK(fully_heard IN (0,1)));
            CREATE INDEX IF NOT EXISTS playback_audition_outcomes_page ON playback_audition_outcomes(outcome_id);")?;
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
        let has_queue_kind = {
            let mut statement = tx.prepare("PRAGMA table_info(playback_sessions)")?;
            statement
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<rusqlite::Result<Vec<_>>>()?
                .iter()
                .any(|name| name == "queue_kind")
        };
        if !has_queue_kind {
            tx.execute(
                "ALTER TABLE playback_sessions ADD COLUMN queue_kind TEXT NOT NULL DEFAULT 'manual' CHECK(queue_kind IN ('album','manual'))",
                [],
            )?;
            tx.execute(
                "UPDATE playback_sessions SET queue_kind=CASE WHEN album_context_json IS NULL THEN 'manual' ELSE 'album' END",
                [],
            )?;
        }
        let has_current_policy = {
            let mut statement = tx.prepare("PRAGMA table_info(playback_sessions)")?;
            statement
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<rusqlite::Result<Vec<_>>>()?
                .iter()
                .any(|name| name == "current_gain_bits")
        };
        if !has_current_policy {
            tx.execute(
                "ALTER TABLE playback_sessions ADD COLUMN current_gain_bits INTEGER NOT NULL DEFAULT 1065353216 CHECK(current_gain_bits>=0 AND current_gain_bits<=4294967295)",
                [],
            )?;
            tx.execute(
                "ALTER TABLE playback_sessions ADD COLUMN current_qualified_suffix TEXT",
                [],
            )?;
        }
        if version == Some(3) {
            migrate_v3_album_context(&tx)?;
            tx.execute(
                "UPDATE playback_sessions SET queue_kind=CASE WHEN album_context_json IS NULL THEN 'manual' ELSE 'album' END",
                [],
            )?;
        }
        if version != Some(PERSISTENCE_VERSION) {
            migrate_current_policy(&tx)?;
        }
        if fail_before_version_commit {
            return Err(anyhow!("injected playback migration failure"));
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
        let loaded = conn.query_row("SELECT session_id,queue_revision,checkpoint_sequence,transport_state,current_occurrence_id,position_ms,album_context_json,queue_kind,current_gain_bits,current_qualified_suffix FROM playback_sessions WHERE singleton_id=1", [], |r| {
            let state: String = r.get(3)?;
            let state = match state.as_str() { "idle"=>TransportState::Idle, "paused"=>TransportState::Paused, "buffering"=>TransportState::Buffering, "playing"=>TransportState::Playing, "stopping"=>TransportState::Stopping, _=>return Err(rusqlite::Error::InvalidQuery) };
            let context_json: Option<String> = r.get(6)?;
            let album_context = context_json.map(|json| serde_json::from_str(&json).map_err(|error| rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(error)))).transpose()?;
            let queue_kind = match r.get::<_, String>(7)?.as_str() { "album" => super::model::QueueKind::Album, "manual" => super::model::QueueKind::Manual, _ => return Err(rusqlite::Error::InvalidQuery) };
            let current_gain_bits = u32::try_from(r.get::<_, i64>(8)?).map_err(|_| rusqlite::Error::InvalidQuery)?;
            Ok(PersistedSession { session_id:r.get(0)?, queue_revision:nonnegative(r,1)?, checkpoint_sequence:nonnegative(r,2)?, state, current_occurrence_id:r.get(4)?, position_ms:nonnegative(r,5)?, queue_kind, current_gain_bits, current_qualified_suffix:r.get(9)?, album_context })
        }).optional()?;
        if loaded.is_none() {
            let occurrence_count: i64 =
                conn.query_row("SELECT COUNT(*) FROM playback_occurrences", [], |r| {
                    r.get(0)
                })?;
            let audition_count: i64 =
                conn.query_row("SELECT COUNT(*) FROM playback_audition", [], |r| r.get(0))?;
            if occurrence_count != 0 || audition_count != 0 {
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
            || !f32::from_bits(session.current_gain_bits).is_finite()
            || f32::from_bits(session.current_gain_bits) <= 0.0
            || session
                .current_qualified_suffix
                .as_ref()
                .is_some_and(|value| value.is_empty() || value.len() > 128)
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
        if session.queue_kind == super::model::QueueKind::Album && session.album_context.is_none() {
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
        let mut expected_album_current_policy = None;
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
                let is_current = Some(&row.occurrence_id) == session.current_occurrence_id.as_ref();
                found |= is_current;
                if is_current && session.queue_kind == super::model::QueueKind::Album {
                    expected_album_current_policy = session.album_context.as_ref().map(|context| {
                        (context.scalar_for(&row).to_bits(), context.suffix_for(&row))
                    });
                }
                if session.queue_kind == super::model::QueueKind::Album
                    && session.album_context.as_ref().is_some_and(|context| {
                        row.ordinal < context.member_count
                            && row.source.server_id != context.source.server_id
                    })
                {
                    return Err(anyhow!("INVALID_SESSION"));
                }
                if session.queue_kind == super::model::QueueKind::Album
                    && session
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
            || (session.queue_kind == super::model::QueueKind::Album
                && session
                    .album_context
                    .as_ref()
                    .is_some_and(|context| context.member_count > count as u64))
        {
            return Err(anyhow!("INVALID_SESSION"));
        }
        if session.queue_kind == super::model::QueueKind::Album
            && session.album_context.as_ref().is_some_and(|context| {
                album_membership.finalize().to_hex().as_str() != context.membership_digest
            })
        {
            return Err(anyhow!("INVALID_SESSION"));
        }
        if session.queue_kind == super::model::QueueKind::Album
            && expected_album_current_policy.as_ref()
                != Some(&(
                    session.current_gain_bits,
                    session.current_qualified_suffix.clone(),
                ))
        {
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

    pub fn playback_range_count(
        &self,
        session_id: &str,
        lower_exclusive: Option<u64>,
        upper_exclusive: Option<u64>,
    ) -> Result<u64> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let lower = lower_exclusive
            .map(i64::try_from)
            .transpose()?
            .unwrap_or(-1);
        let upper = upper_exclusive
            .map(i64::try_from)
            .transpose()?
            .unwrap_or(i64::MAX);
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM playback_occurrences WHERE session_id=?1 AND ordinal>?2 AND ordinal<?3",
            params![session_id, lower, upper],
            |row| row.get::<_, i64>(0),
        )? as u64)
    }

    pub fn playback_range_page(
        &self,
        session_id: &str,
        lower_exclusive: Option<u64>,
        upper_exclusive: Option<u64>,
        after: Option<u64>,
        limit: usize,
    ) -> Result<Vec<Occurrence>> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let lower = lower_exclusive
            .map(i64::try_from)
            .transpose()?
            .unwrap_or(-1);
        let after = after
            .map(i64::try_from)
            .transpose()?
            .unwrap_or(lower)
            .max(lower);
        let upper = upper_exclusive
            .map(i64::try_from)
            .transpose()?
            .unwrap_or(i64::MAX);
        let mut stmt = conn.prepare("SELECT occurrence_id,ordinal,server_id,track_id FROM playback_occurrences WHERE session_id=?1 AND ordinal>?2 AND ordinal<?3 ORDER BY ordinal LIMIT ?4")?;
        stmt.query_map(
            params![session_id, after, upper, limit as i64],
            occurrence_from_row,
        )?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
    }

    pub fn playback_predecessor_in_range(
        &self,
        session_id: &str,
        ordinal: u64,
        lower_exclusive: Option<u64>,
    ) -> Result<Option<Occurrence>> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.query_row(
            "SELECT occurrence_id,ordinal,server_id,track_id FROM playback_occurrences WHERE session_id=?1 AND ordinal<?2 AND ordinal>?3 ORDER BY ordinal DESC LIMIT 1",
            params![session_id, i64::try_from(ordinal)?, lower_exclusive.map(i64::try_from).transpose()?.unwrap_or(-1)],
            occurrence_from_row,
        ).optional().map_err(Into::into)
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

    pub fn persist_playback_structure_superseding_audition(
        &self,
        session: &PersistedSession,
        occurrences: &[Occurrence],
        audition: &PersistedAudition,
    ) -> Result<()> {
        let mut conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let tx = conn.transaction()?;
        if !audition_outcome_exists(&tx, &audition.audition_id)? {
            insert_audition_outcome(&tx, audition, "superseded", None, None)?;
        }
        let deleted = tx.execute(
            "DELETE FROM playback_audition WHERE singleton_id=1 AND audition_id=?1",
            [&audition.audition_id],
        )?;
        if deleted != 1 {
            return Err(anyhow!("active audition changed"));
        }
        let album_context_json = album_context_json(session)?;
        tx.execute("INSERT INTO playback_sessions(singleton_id,session_id,queue_revision,checkpoint_sequence,transport_state,current_occurrence_id,position_ms,album_context_json,queue_kind,current_gain_bits,current_qualified_suffix) VALUES(1,?1,?2,?3,?4,?5,?6,?7,?8,?9,?10) ON CONFLICT(singleton_id) DO UPDATE SET session_id=excluded.session_id,queue_revision=excluded.queue_revision,checkpoint_sequence=excluded.checkpoint_sequence,transport_state=excluded.transport_state,current_occurrence_id=excluded.current_occurrence_id,position_ms=excluded.position_ms,album_context_json=excluded.album_context_json,queue_kind=excluded.queue_kind,current_gain_bits=excluded.current_gain_bits,current_qualified_suffix=excluded.current_qualified_suffix", params![session.session_id,session.queue_revision as i64,session.checkpoint_sequence as i64,state_name(session.state),session.current_occurrence_id,session.position_ms as i64,album_context_json,queue_kind_name(session.queue_kind),i64::from(session.current_gain_bits),session.current_qualified_suffix])?;
        tx.execute(
            "DELETE FROM playback_occurrences WHERE session_id=?1",
            [&session.session_id],
        )?;
        insert_occurrences(&tx, &session.session_id, occurrences)?;
        tx.commit()?;
        Ok(())
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
        tx.execute("INSERT INTO playback_sessions(singleton_id,session_id,queue_revision,checkpoint_sequence,transport_state,current_occurrence_id,position_ms,album_context_json,queue_kind,current_gain_bits,current_qualified_suffix) VALUES(1,?1,?2,?3,?4,?5,?6,?7,?8,?9,?10) ON CONFLICT(singleton_id) DO UPDATE SET session_id=excluded.session_id,queue_revision=excluded.queue_revision,checkpoint_sequence=excluded.checkpoint_sequence,transport_state=excluded.transport_state,current_occurrence_id=excluded.current_occurrence_id,position_ms=excluded.position_ms,album_context_json=excluded.album_context_json,queue_kind=excluded.queue_kind,current_gain_bits=excluded.current_gain_bits,current_qualified_suffix=excluded.current_qualified_suffix", params![session.session_id,session.queue_revision as i64,session.checkpoint_sequence as i64,state_name(session.state),session.current_occurrence_id,session.position_ms as i64,album_context_json,queue_kind_name(session.queue_kind),i64::from(session.current_gain_bits),session.current_qualified_suffix])?;
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

    pub fn append_playback_occurrences_with_audition_baseline(
        &self,
        session: &PersistedSession,
        occurrences: &[Occurrence],
    ) -> Result<()> {
        let mut conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let tx = conn.transaction()?;
        update_session(&tx, session)?;
        insert_occurrences(&tx, &session.session_id, occurrences)?;
        let changed = tx.execute(
            "UPDATE playback_audition SET saved_main_occurrence_id=?1,saved_main_position_ms=0,saved_main_intent='paused' WHERE singleton_id=1 AND parent_session_id=?2 AND saved_main_occurrence_id IS NULL",
            params![session.current_occurrence_id, session.session_id],
        )?;
        if changed != 1 {
            return Err(anyhow!("active audition baseline changed"));
        }
        tx.commit()?;
        Ok(())
    }

    pub fn edit_playback_upcoming(
        &self,
        session: &PersistedSession,
        ordered_upcoming: &[Occurrence],
        removed_occurrence_ids: &[String],
    ) -> Result<()> {
        let mut conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let tx = conn.transaction()?;
        let (stored_revision, stored_current): (i64, Option<String>) = tx.query_row(
            "SELECT queue_revision,current_occurrence_id FROM playback_sessions WHERE singleton_id=1 AND session_id=?1",
            [&session.session_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let prior_revision = session
            .queue_revision
            .checked_sub(1)
            .ok_or_else(|| anyhow!("queue revision underflow"))?;
        if stored_revision != i64::try_from(prior_revision)?
            || stored_current != session.current_occurrence_id
        {
            return Err(anyhow!("authoritative queue changed"));
        }
        let current_id = stored_current.ok_or_else(|| anyhow!("current occurrence is required"))?;
        let current_ordinal: i64 = tx.query_row(
            "SELECT ordinal FROM playback_occurrences WHERE session_id=?1 AND occurrence_id=?2",
            params![session.session_id, current_id],
            |row| row.get(0),
        )?;
        let existing: Vec<String> = {
            let mut statement = tx.prepare(
                "SELECT occurrence_id FROM playback_occurrences WHERE session_id=?1 AND ordinal>?2 ORDER BY ordinal",
            )?;
            statement
                .query_map(params![session.session_id, current_ordinal], |row| {
                    row.get(0)
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        let mut expected = existing.clone();
        expected.retain(|id| !removed_occurrence_ids.contains(id));
        let proposed: Vec<&str> = ordered_upcoming
            .iter()
            .map(|row| row.occurrence_id.as_str())
            .collect();
        if expected.len() != proposed.len()
            || expected.iter().any(|id| !proposed.contains(&id.as_str()))
        {
            return Err(anyhow!("edited queue membership changed"));
        }
        for occurrence_id in removed_occurrence_ids {
            let changed = tx.execute(
                "DELETE FROM playback_occurrences WHERE session_id=?1 AND occurrence_id=?2 AND ordinal>?3",
                params![session.session_id, occurrence_id, current_ordinal],
            )?;
            if changed != 1 {
                return Err(anyhow!("removed occurrence is no longer upcoming"));
            }
        }
        if !ordered_upcoming.is_empty() {
            let max_ordinal: i64 = tx.query_row(
                "SELECT COALESCE(MAX(ordinal),0) FROM playback_occurrences WHERE session_id=?1",
                [&session.session_id],
                |row| row.get(0),
            )?;
            let offset = max_ordinal
                .checked_add(i64::try_from(ordered_upcoming.len())?)
                .and_then(|value| value.checked_add(1))
                .ok_or_else(|| anyhow!("playback ordinal overflow"))?;
            tx.execute(
                "UPDATE playback_occurrences SET ordinal=ordinal+?1 WHERE session_id=?2 AND ordinal>?3",
                params![offset, session.session_id, current_ordinal],
            )?;
            for (index, occurrence) in ordered_upcoming.iter().enumerate() {
                let ordinal = current_ordinal
                    .checked_add(i64::try_from(index)? + 1)
                    .ok_or_else(|| anyhow!("playback ordinal overflow"))?;
                let changed = tx.execute(
                    "UPDATE playback_occurrences SET ordinal=?1 WHERE session_id=?2 AND occurrence_id=?3",
                    params![ordinal, session.session_id, occurrence.occurrence_id],
                )?;
                if changed != 1 {
                    return Err(anyhow!("moved occurrence is absent"));
                }
            }
        }
        update_session(&tx, session)?;
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

    /// Move the durable cursor away from an occurrence whose terminal outcome
    /// was already committed. This is used when a successor is appended after
    /// the previous final current completed; the original outcome is immutable.
    pub fn persist_playback_departure(
        &self,
        session: &PersistedSession,
        departed_occurrence_id: &str,
    ) -> Result<()> {
        let mut conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let tx = conn.transaction()?;
        let changed = tx.execute(
            "UPDATE playback_occurrences SET outcome=outcome WHERE session_id=?1 AND occurrence_id=?2 AND outcome IS NOT NULL",
            params![session.session_id, departed_occurrence_id],
        )?;
        if changed != 1 {
            return Err(anyhow!("terminal playback outcome is absent"));
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

    pub fn persist_playback_state(&self, session: &PersistedSession) -> Result<()> {
        let mut conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let tx = conn.transaction()?;
        update_session(&tx, session)?;
        tx.commit()?;
        Ok(())
    }

    pub fn load_playback_audition(&self) -> Result<Option<PersistedAudition>> {
        self.init_playback()?;
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.query_row(
            "SELECT audition_id,parent_session_id,server_id,track_id,position_ms,transport_state,saved_main_occurrence_id,saved_main_position_ms,saved_main_intent,resume_inhibited,contiguous_heard_ms,coverage_unknown,seek_discontinuous FROM playback_audition WHERE singleton_id=1",
            [],
            audition_from_row,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn persist_audition_admission(
        &self,
        session: &PersistedSession,
        audition: &PersistedAudition,
    ) -> Result<()> {
        validate_audition(session, audition)?;
        let mut conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let tx = conn.transaction()?;
        update_session(&tx, session)?;
        insert_audition(&tx, audition)?;
        tx.commit()?;
        Ok(())
    }

    pub fn replace_audition(
        &self,
        session: &PersistedSession,
        previous: &PersistedAudition,
        replacement: &PersistedAudition,
    ) -> Result<()> {
        self.replace_audition_with_outcome(session, previous, replacement, "replaced", None)
    }

    pub fn retry_audition(
        &self,
        session: &PersistedSession,
        previous: &PersistedAudition,
        replacement: &PersistedAudition,
        failure_code: &str,
    ) -> Result<()> {
        self.replace_audition_with_outcome(
            session,
            previous,
            replacement,
            "technicalFailure",
            Some(failure_code),
        )
    }

    fn replace_audition_with_outcome(
        &self,
        session: &PersistedSession,
        previous: &PersistedAudition,
        replacement: &PersistedAudition,
        disposition: &str,
        failure_code: Option<&str>,
    ) -> Result<()> {
        validate_audition(session, previous)?;
        validate_audition(session, replacement)?;
        if previous.saved_main_occurrence_id != replacement.saved_main_occurrence_id
            || previous.saved_main_position_ms != replacement.saved_main_position_ms
            || previous.saved_main_intent != replacement.saved_main_intent
        {
            return Err(anyhow!("INVALID_SESSION"));
        }
        let mut conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let tx = conn.transaction()?;
        if !audition_outcome_exists(&tx, &previous.audition_id)? {
            insert_audition_outcome(&tx, previous, disposition, failure_code, None)?;
        }
        let deleted = tx.execute(
            "DELETE FROM playback_audition WHERE singleton_id=1 AND audition_id=?1",
            [&previous.audition_id],
        )?;
        if deleted != 1 {
            return Err(anyhow!("active audition changed"));
        }
        update_session(&tx, session)?;
        insert_audition(&tx, replacement)?;
        tx.commit()?;
        Ok(())
    }

    pub fn finish_audition(
        &self,
        session: &PersistedSession,
        audition: &PersistedAudition,
        disposition: &str,
        failure_code: Option<&str>,
        duration_ms: Option<u64>,
    ) -> Result<()> {
        validate_audition(session, audition)?;
        let mut conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let tx = conn.transaction()?;
        if !audition_outcome_exists(&tx, &audition.audition_id)? {
            insert_audition_outcome(&tx, audition, disposition, failure_code, duration_ms)?;
        }
        let deleted = tx.execute(
            "DELETE FROM playback_audition WHERE singleton_id=1 AND audition_id=?1",
            [&audition.audition_id],
        )?;
        if deleted != 1 {
            return Err(anyhow!("active audition changed"));
        }
        update_session(&tx, session)?;
        tx.commit()?;
        Ok(())
    }

    pub fn audition_outcomes(
        &self,
        after_outcome_id: Option<u64>,
        limit: usize,
    ) -> Result<Vec<AuditionOutcome>> {
        if !(1..=200).contains(&limit) {
            return Err(anyhow!("invalid audition outcome page size"));
        }
        let after = after_outcome_id.unwrap_or(0);
        let after = i64::try_from(after).map_err(|_| anyhow!("invalid audition outcome cursor"))?;
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut statement = conn.prepare("SELECT outcome_id,audition_id,parent_session_id,server_id,track_id,disposition,terminal_position_ms,duration_ms,failure_code,contiguous_heard_ms,coverage_unknown,seek_discontinuous,fully_heard FROM playback_audition_outcomes WHERE outcome_id>?1 ORDER BY outcome_id LIMIT ?2")?;
        statement
            .query_map(params![after, limit as i64], audition_outcome_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn checkpoint_audition(&self, audition: &PersistedAudition) -> Result<()> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let changed = conn.execute(
            "UPDATE playback_audition SET position_ms=?1,transport_state=?2,resume_inhibited=?3,contiguous_heard_ms=?4,coverage_unknown=?5,seek_discontinuous=?6 WHERE singleton_id=1 AND audition_id=?7",
            params![audition.position_ms as i64,state_name(audition.state),audition.resume_inhibited,audition.contiguous_heard_ms as i64,audition.coverage_unknown,audition.seek_discontinuous,audition.audition_id],
        )?;
        if changed != 1 {
            return Err(anyhow!("active audition changed"));
        }
        Ok(())
    }

    pub fn record_audition_failure(
        &self,
        session: &PersistedSession,
        audition: &PersistedAudition,
        failure_code: &str,
        duration_ms: Option<u64>,
    ) -> Result<()> {
        validate_audition(session, audition)?;
        let mut conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let tx = conn.transaction()?;
        update_audition(&tx, audition)?;
        if !audition_outcome_exists(&tx, &audition.audition_id)? {
            insert_audition_outcome(
                &tx,
                audition,
                "technicalFailure",
                Some(failure_code),
                duration_ms,
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn dismiss_interrupted_audition(&self, session: &mut PersistedSession) -> Result<bool> {
        let Some(audition) = self.load_playback_audition()? else {
            return Ok(false);
        };
        validate_audition(session, &audition)?;
        session.current_occurrence_id = audition.saved_main_occurrence_id.clone();
        session.position_ms = audition.saved_main_position_ms;
        session.state = if session.current_occurrence_id.is_some() {
            TransportState::Paused
        } else {
            TransportState::Idle
        };
        self.finish_audition(session, &audition, "interrupted", None, None)?;
        Ok(true)
    }
}

fn audition_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PersistedAudition> {
    let state: String = row.get(5)?;
    let saved_intent: String = row.get(8)?;
    Ok(PersistedAudition {
        audition_id: row.get(0)?,
        parent_session_id: row.get(1)?,
        source: TrackSource {
            server_id: row.get(2)?,
            track_id: row.get(3)?,
        },
        position_ms: nonnegative(row, 4)?,
        state: parse_state(&state).map_err(|_| rusqlite::Error::InvalidQuery)?,
        saved_main_occurrence_id: row.get(6)?,
        saved_main_position_ms: nonnegative(row, 7)?,
        saved_main_intent: parse_state(&saved_intent).map_err(|_| rusqlite::Error::InvalidQuery)?,
        resume_inhibited: row.get(9)?,
        contiguous_heard_ms: nonnegative(row, 10)?,
        coverage_unknown: row.get(11)?,
        seek_discontinuous: row.get(12)?,
    })
}

fn audition_outcome_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AuditionOutcome> {
    Ok(AuditionOutcome {
        outcome_id: nonnegative(row, 0)?,
        audition_id: row.get(1)?,
        parent_session_id: row.get(2)?,
        source: TrackSource {
            server_id: row.get(3)?,
            track_id: row.get(4)?,
        },
        disposition: row.get(5)?,
        terminal_position_ms: nonnegative(row, 6)?,
        duration_ms: row
            .get::<_, Option<i64>>(7)?
            .map(u64::try_from)
            .transpose()
            .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(7, -1))?,
        failure_code: row.get(8)?,
        contiguous_heard_ms: nonnegative(row, 9)?,
        coverage_unknown: row.get(10)?,
        seek_discontinuous: row.get(11)?,
        fully_heard: row.get(12)?,
    })
}

fn parse_state(value: &str) -> Result<TransportState> {
    match value {
        "idle" => Ok(TransportState::Idle),
        "paused" => Ok(TransportState::Paused),
        "buffering" => Ok(TransportState::Buffering),
        "playing" => Ok(TransportState::Playing),
        "stopping" => Ok(TransportState::Stopping),
        _ => Err(anyhow!("INVALID_SESSION")),
    }
}

fn validate_audition(session: &PersistedSession, audition: &PersistedAudition) -> Result<()> {
    if uuid::Uuid::parse_str(&audition.audition_id).is_err()
        || audition.parent_session_id != session.session_id
        || audition.source.validate().is_err()
        || audition.position_ms > 9_007_199_254_740_991
        || audition.saved_main_position_ms > 9_007_199_254_740_991
        || audition.contiguous_heard_ms > 9_007_199_254_740_991
        || audition.saved_main_occurrence_id != session.current_occurrence_id
        || !matches!(
            audition.state,
            TransportState::Paused | TransportState::Buffering | TransportState::Playing
        )
        || matches!(audition.saved_main_intent, TransportState::Stopping)
        || (audition.saved_main_occurrence_id.is_none()
            && (audition.saved_main_position_ms != 0
                || audition.saved_main_intent != TransportState::Idle))
        || (audition.saved_main_occurrence_id.is_some()
            && audition.saved_main_intent == TransportState::Idle)
    {
        return Err(anyhow!("INVALID_SESSION"));
    }
    Ok(())
}

fn insert_audition(tx: &rusqlite::Transaction<'_>, audition: &PersistedAudition) -> Result<()> {
    tx.execute(
        "INSERT INTO playback_audition(singleton_id,audition_id,parent_session_id,server_id,track_id,position_ms,transport_state,saved_main_occurrence_id,saved_main_position_ms,saved_main_intent,resume_inhibited,contiguous_heard_ms,coverage_unknown,seek_discontinuous) VALUES(1,?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
        params![audition.audition_id,audition.parent_session_id,audition.source.server_id,audition.source.track_id,audition.position_ms as i64,state_name(audition.state),audition.saved_main_occurrence_id,audition.saved_main_position_ms as i64,state_name(audition.saved_main_intent),audition.resume_inhibited,audition.contiguous_heard_ms as i64,audition.coverage_unknown,audition.seek_discontinuous],
    )?;
    Ok(())
}

fn insert_audition_outcome(
    tx: &rusqlite::Transaction<'_>,
    audition: &PersistedAudition,
    disposition: &str,
    failure_code: Option<&str>,
    duration_ms: Option<u64>,
) -> Result<()> {
    if duration_ms.is_some_and(|duration| duration > 9_007_199_254_740_991)
        || !matches!(
            disposition,
            "naturalCompletion"
                | "stopped"
                | "returned"
                | "replaced"
                | "superseded"
                | "technicalFailure"
                | "interrupted"
        )
        || failure_code.is_some_and(|code| {
            code.is_empty()
                || code.len() > 128
                || !code
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
        })
    {
        return Err(anyhow!("invalid audition outcome"));
    }
    let fully_heard = disposition == "naturalCompletion"
        && !audition.coverage_unknown
        && !audition.seek_discontinuous
        && duration_ms.is_some_and(|duration| audition.contiguous_heard_ms >= duration);
    tx.execute(
        "INSERT INTO playback_audition_outcomes(audition_id,parent_session_id,server_id,track_id,disposition,terminal_position_ms,duration_ms,failure_code,contiguous_heard_ms,coverage_unknown,seek_discontinuous,fully_heard) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
        params![audition.audition_id,audition.parent_session_id,audition.source.server_id,audition.source.track_id,disposition,audition.position_ms as i64,duration_ms.map(i64::try_from).transpose()?,failure_code,audition.contiguous_heard_ms as i64,audition.coverage_unknown,audition.seek_discontinuous,fully_heard],
    )?;
    Ok(())
}

fn audition_outcome_exists(tx: &rusqlite::Transaction<'_>, audition_id: &str) -> Result<bool> {
    tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM playback_audition_outcomes WHERE audition_id=?1)",
        [audition_id],
        |row| row.get(0),
    )
    .map_err(Into::into)
}

fn update_audition(tx: &rusqlite::Transaction<'_>, audition: &PersistedAudition) -> Result<()> {
    let changed = tx.execute(
        "UPDATE playback_audition SET position_ms=?1,transport_state=?2,resume_inhibited=?3,contiguous_heard_ms=?4,coverage_unknown=?5,seek_discontinuous=?6 WHERE singleton_id=1 AND audition_id=?7",
        params![audition.position_ms as i64,state_name(audition.state),audition.resume_inhibited,audition.contiguous_heard_ms as i64,audition.coverage_unknown,audition.seek_discontinuous,audition.audition_id],
    )?;
    if changed != 1 {
        return Err(anyhow!("active audition changed"));
    }
    Ok(())
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

/// Early v3 album records predate the digest and per-member format evidence.
/// Migrate only that schema, preserving queue, cursor, outcomes and frozen gain.
/// Unknown formats block adjusted preparation, not restoration or a new album.
fn migrate_v3_album_context(tx: &rusqlite::Transaction<'_>) -> Result<()> {
    use super::model::{AlbumSource, FrozenAlbumContext, UNVERIFIED_ALBUM_FORMAT};
    let row: Option<(String, String)> = tx.query_row(
        "SELECT session_id,album_context_json FROM playback_sessions WHERE singleton_id=1 AND album_context_json IS NOT NULL",
        [], |row| Ok((row.get(0)?, row.get(1)?)),
    ).optional()?;
    let Some((session_id, json)) = row else {
        return Ok(());
    };
    let mut value: serde_json::Value = serde_json::from_str(&json)?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| anyhow!("INVALID_SESSION"))?;
    let source: AlbumSource = serde_json::from_value(
        object
            .get("source")
            .cloned()
            .ok_or_else(|| anyhow!("INVALID_SESSION"))?,
    )?;
    source.validate().map_err(|error| anyhow!(error))?;
    let count = object
        .get("memberCount")
        .and_then(serde_json::Value::as_u64)
        .filter(|count| (1..=super::album::MAX_ALBUM_OCCURRENCES as u64).contains(count))
        .ok_or_else(|| anyhow!("INVALID_SESSION"))?;
    let policy: super::loudness::AlbumLoudnessPolicy = serde_json::from_value(
        object
            .get("policy")
            .cloned()
            .ok_or_else(|| anyhow!("INVALID_SESSION"))?,
    )?;
    policy.validate().map_err(|error| anyhow!(error))?;
    let mut hasher = super::model::album_membership_hasher();
    let mut statement = tx.prepare("SELECT occurrence_id,ordinal,server_id,track_id FROM playback_occurrences WHERE session_id=?1 AND ordinal<?2 ORDER BY ordinal")?;
    let rows = statement.query_map(params![session_id, count as i64], occurrence_from_row)?;
    let mut seen = 0;
    for row in rows {
        let row = row?;
        if row.ordinal != seen
            || row.source.server_id != source.server_id
            || row.source.validate().is_err()
            || uuid::Uuid::parse_str(&row.occurrence_id).is_err()
        {
            return Err(anyhow!("INVALID_SESSION"));
        }
        super::model::hash_album_member(&mut hasher, &row.source);
        seen += 1;
    }
    if seen != count {
        return Err(anyhow!("INVALID_SESSION"));
    }
    let digest = hasher.finalize().to_hex().to_string();
    if !object.contains_key("membershipDigest") {
        object.insert("membershipDigest".into(), digest.clone().into());
    }
    if !object.contains_key("representations") {
        let formats = if policy.scalar() == 1.0 {
            Vec::new()
        } else {
            vec![UNVERIFIED_ALBUM_FORMAT; count as usize]
        };
        object.insert("representations".into(), serde_json::to_value(formats)?);
    }
    let context: FrozenAlbumContext = serde_json::from_value(value)?;
    context.validate().map_err(|error| anyhow!(error))?;
    if context.membership_digest != digest {
        return Err(anyhow!("INVALID_SESSION"));
    }
    tx.execute(
        "UPDATE playback_sessions SET album_context_json=?1 WHERE singleton_id=1",
        [serde_json::to_string(&context)?],
    )?;
    Ok(())
}

fn migrate_current_policy(tx: &rusqlite::Transaction<'_>) -> Result<()> {
    let row: Option<(String, Option<String>, Option<String>, String)> = tx
        .query_row(
            "SELECT session_id,current_occurrence_id,album_context_json,queue_kind FROM playback_sessions WHERE singleton_id=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;
    let Some((session_id, current_id, context_json, queue_kind)) = row else {
        return Ok(());
    };
    let policy = if queue_kind == "album" {
        current_id
            .zip(context_json)
            .map(|(current_id, json)| -> Result<(u32, Option<String>)> {
                let context: super::model::FrozenAlbumContext = serde_json::from_str(&json)?;
                context.validate().map_err(|error| anyhow!(error))?;
                let current = tx.query_row(
                    "SELECT occurrence_id,ordinal,server_id,track_id FROM playback_occurrences WHERE session_id=?1 AND occurrence_id=?2",
                    params![session_id, current_id],
                    occurrence_from_row,
                )?;
                Ok((
                    context.scalar_for(&current).to_bits(),
                    context.suffix_for(&current),
                ))
            })
            .transpose()?
    } else if queue_kind == "manual" {
        None
    } else {
        return Err(anyhow!("INVALID_SESSION"));
    };
    let (gain_bits, suffix) = policy.unwrap_or((1.0f32.to_bits(), None));
    tx.execute(
        "UPDATE playback_sessions SET current_gain_bits=?1,current_qualified_suffix=?2 WHERE singleton_id=1",
        params![i64::from(gain_bits), suffix],
    )?;
    Ok(())
}

fn nonnegative(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(index)?;
    u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, value))
}

fn update_session(tx: &rusqlite::Transaction<'_>, session: &PersistedSession) -> Result<()> {
    let album_context_json = album_context_json(session)?;
    let changed = tx.execute(
        "UPDATE playback_sessions SET session_id=?1,queue_revision=?2,checkpoint_sequence=?3,transport_state=?4,current_occurrence_id=?5,position_ms=?6,album_context_json=?7,queue_kind=?8,current_gain_bits=?9,current_qualified_suffix=?10 WHERE singleton_id=1",
        params![session.session_id, session.queue_revision as i64, session.checkpoint_sequence as i64, state_name(session.state), session.current_occurrence_id, session.position_ms as i64, album_context_json, queue_kind_name(session.queue_kind), i64::from(session.current_gain_bits), session.current_qualified_suffix],
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

fn queue_kind_name(kind: super::model::QueueKind) -> &'static str {
    match kind {
        super::model::QueueKind::Album => "album",
        super::model::QueueKind::Manual => "manual",
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
            queue_kind: super::super::model::QueueKind::Manual,
            current_gain_bits: 1.0f32.to_bits(),
            current_qualified_suffix: None,
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
        committed.queue_kind = super::super::model::QueueKind::Album;
        committed.album_context = Some(super::super::model::FrozenAlbumContext {
            source: super::super::model::AlbumSource {
                server_id: "portable-server".into(),
                album_id: "album".into(),
            },
            member_count: 2,
            membership_digest,
            representations: vec!["flac".into(); 2],
            policy: super::super::loudness::AlbumLoudnessPolicy {
                version: super::super::loudness::ALBUM_LOUDNESS_POLICY_VERSION,
                scalar_bits: 0.75f32.to_bits(),
                gain_db_bits: Some(0.0f64.to_bits()),
                peak_bits: Some((super::super::loudness::SAMPLE_PEAK_CEILING / 0.75).to_bits()),
                reason: super::super::loudness::AlbumLoudnessReason::OpenSubsonicReplayGain,
            },
        });
        committed.current_gain_bits = 0.75f32.to_bits();
        committed.current_qualified_suffix = Some("flac".into());
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
        assert_eq!(version, PERSISTENCE_VERSION);
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
    fn v4_audition_admission_replacement_and_terminal_outcomes_are_atomic_and_paged() {
        let db = Database::memory().unwrap();
        db.init_playback().unwrap();
        let main = occurrence("ignored", 0, "main");
        let committed = session(1, Some(main.occurrence_id.clone()), 4_200);
        db.persist_playback_structure(&committed, &[main]).unwrap();

        let first = PersistedAudition {
            audition_id: Uuid::new_v4().to_string(),
            parent_session_id: committed.session_id.clone(),
            source: TrackSource {
                server_id: "portable-server".into(),
                track_id: "a".into(),
            },
            position_ms: 0,
            state: TransportState::Buffering,
            saved_main_occurrence_id: committed.current_occurrence_id.clone(),
            saved_main_position_ms: 4_200,
            saved_main_intent: TransportState::Playing,
            resume_inhibited: false,
            contiguous_heard_ms: 0,
            coverage_unknown: false,
            seek_discontinuous: false,
        };
        db.persist_audition_admission(&committed, &first).unwrap();
        assert_eq!(db.load_playback_audition().unwrap(), Some(first.clone()));

        let second = PersistedAudition {
            audition_id: Uuid::new_v4().to_string(),
            source: TrackSource {
                server_id: "portable-server".into(),
                track_id: "b".into(),
            },
            ..first.clone()
        };
        db.replace_audition(&committed, &first, &second).unwrap();
        assert_eq!(db.load_playback_audition().unwrap(), Some(second.clone()));
        db.finish_audition(&committed, &second, "returned", None, Some(8_000))
            .unwrap();
        assert!(db.load_playback_audition().unwrap().is_none());
        let outcomes = db.audition_outcomes(None, 100).unwrap();
        assert_eq!(outcomes.len(), 2);
        assert_eq!(outcomes[0].disposition, "replaced");
        assert_eq!(outcomes[1].disposition, "returned");
        assert!(!outcomes[1].fully_heard);
        assert!(db.audition_outcomes(None, 201).is_err());
        assert!(
            db.audition_outcomes(Some(i64::MAX as u64 + 1), 100)
                .is_err()
        );
    }

    #[test]
    fn orphan_or_invalid_audition_state_blocks_restoration() {
        let db = Database::memory().unwrap();
        db.init_playback().unwrap();
        let main = occurrence("ignored", 0, "main");
        let committed = session(1, Some(main.occurrence_id.clone()), 4_200);
        db.persist_playback_structure(&committed, &[main]).unwrap();
        let audition = PersistedAudition {
            audition_id: Uuid::new_v4().to_string(),
            parent_session_id: committed.session_id.clone(),
            source: TrackSource {
                server_id: "portable-server".into(),
                track_id: "preview".into(),
            },
            position_ms: 0,
            state: TransportState::Buffering,
            saved_main_occurrence_id: committed.current_occurrence_id.clone(),
            saved_main_position_ms: 4_200,
            saved_main_intent: TransportState::Playing,
            resume_inhibited: false,
            contiguous_heard_ms: 0,
            coverage_unknown: false,
            seek_discontinuous: false,
        };
        db.persist_audition_admission(&committed, &audition)
            .unwrap();
        db.conn
            .lock()
            .unwrap()
            .execute(
                "UPDATE playback_audition SET transport_state='stopping'",
                [],
            )
            .unwrap();
        let invalid = db.load_playback_audition().unwrap().unwrap();
        assert!(validate_audition(&committed, &invalid).is_err());

        db.conn
            .lock()
            .unwrap()
            .execute("DELETE FROM playback_sessions", [])
            .unwrap();
        assert!(db.load_playback_session().is_err());
    }

    #[test]
    fn audition_outcome_rejects_duration_outside_the_wire_safe_integer_range() {
        let db = Database::memory().unwrap();
        db.init_playback().unwrap();
        let main = occurrence("ignored", 0, "main");
        let committed = session(1, Some(main.occurrence_id.clone()), 0);
        db.persist_playback_structure(&committed, &[main]).unwrap();
        let audition = PersistedAudition {
            audition_id: Uuid::new_v4().to_string(),
            parent_session_id: committed.session_id.clone(),
            source: TrackSource {
                server_id: "portable-server".into(),
                track_id: "preview".into(),
            },
            position_ms: 0,
            state: TransportState::Paused,
            saved_main_occurrence_id: committed.current_occurrence_id.clone(),
            saved_main_position_ms: 0,
            saved_main_intent: TransportState::Paused,
            resume_inhibited: false,
            contiguous_heard_ms: 0,
            coverage_unknown: false,
            seek_discontinuous: false,
        };
        db.persist_audition_admission(&committed, &audition)
            .unwrap();
        assert!(
            db.finish_audition(
                &committed,
                &audition,
                "returned",
                None,
                Some(9_007_199_254_740_992),
            )
            .is_err()
        );
        assert_eq!(db.load_playback_audition().unwrap(), Some(audition));
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
