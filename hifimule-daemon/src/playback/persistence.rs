use super::model::{Occurrence, PersistedSession, SourceAvailability, TrackSource, TransportState};
use crate::db::Database;
use anyhow::{Result, anyhow};
use rusqlite::{OptionalExtension, params};

pub const PERSISTENCE_VERSION: i64 = 1;

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
            && version != PERSISTENCE_VERSION
        {
            return Err(anyhow!("UNSUPPORTED_PLAYBACK_VERSION"));
        }
        tx.execute_batch("CREATE TABLE IF NOT EXISTS playback_schema (singleton_id INTEGER PRIMARY KEY CHECK(singleton_id=1), version INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS playback_sessions (singleton_id INTEGER PRIMARY KEY CHECK(singleton_id=1), session_id TEXT NOT NULL, queue_revision INTEGER NOT NULL CHECK(queue_revision>=0), checkpoint_sequence INTEGER NOT NULL CHECK(checkpoint_sequence>=0), transport_state TEXT NOT NULL, current_occurrence_id TEXT, position_ms INTEGER NOT NULL CHECK(position_ms>=0));
            CREATE TABLE IF NOT EXISTS playback_occurrences (session_id TEXT NOT NULL, occurrence_id TEXT NOT NULL UNIQUE, ordinal INTEGER NOT NULL CHECK(ordinal>=0), server_id TEXT NOT NULL, track_id TEXT NOT NULL, PRIMARY KEY(session_id, ordinal));")?;
        tx.execute(
            "INSERT OR IGNORE INTO playback_schema(singleton_id,version) VALUES(1,1)",
            [],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn load_playback_session(&self) -> Result<Option<PersistedSession>> {
        self.init_playback()?;
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.query_row("SELECT session_id,queue_revision,checkpoint_sequence,transport_state,current_occurrence_id,position_ms FROM playback_sessions WHERE singleton_id=1", [], |r| {
            let state: String = r.get(3)?;
            let state = match state.as_str() { "idle"=>TransportState::Idle, "paused"=>TransportState::Paused, "buffering"=>TransportState::Buffering, "playing"=>TransportState::Playing, "stopping"=>TransportState::Stopping, _=>return Err(rusqlite::Error::InvalidQuery) };
            Ok(PersistedSession { session_id:r.get(0)?, queue_revision:r.get::<_,i64>(1)? as u64, checkpoint_sequence:r.get::<_,i64>(2)? as u64, state, current_occurrence_id:r.get(4)?, position_ms:r.get::<_,i64>(5)? as u64 })
        }).optional().map_err(Into::into)
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
                after.map(|v| v as i64).unwrap_or(-1),
                limit as i64
            ],
            |r| {
                Ok(Occurrence {
                    occurrence_id: r.get(0)?,
                    ordinal: r.get::<_, i64>(1)? as u64,
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

    pub fn playback_next_ordinal(&self, session_id: &str) -> Result<u64> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let last: Option<i64> = conn.query_row(
            "SELECT MAX(ordinal) FROM playback_occurrences WHERE session_id=?1",
            [session_id],
            |row| row.get(0),
        )?;
        match last {
            Some(value) => (value as u64)
                .checked_add(1)
                .ok_or_else(|| anyhow!("playback ordinal overflow")),
            None => Ok(0),
        }
    }

    pub fn persist_playback_structure(
        &self,
        session: &PersistedSession,
        occurrences: &[Occurrence],
    ) -> Result<()> {
        let mut conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let tx = conn.transaction()?;
        tx.execute("INSERT INTO playback_sessions(singleton_id,session_id,queue_revision,checkpoint_sequence,transport_state,current_occurrence_id,position_ms) VALUES(1,?1,?2,?3,?4,?5,?6) ON CONFLICT(singleton_id) DO UPDATE SET session_id=excluded.session_id,queue_revision=excluded.queue_revision,checkpoint_sequence=excluded.checkpoint_sequence,transport_state=excluded.transport_state,current_occurrence_id=excluded.current_occurrence_id,position_ms=excluded.position_ms", params![session.session_id,session.queue_revision as i64,session.checkpoint_sequence as i64,state_name(session.state),session.current_occurrence_id,session.position_ms as i64])?;
        tx.execute(
            "DELETE FROM playback_occurrences WHERE session_id=?1",
            [&session.session_id],
        )?;
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
        ordinal: row.get::<_, i64>(1)? as u64,
        source: TrackSource {
            server_id: row.get(2)?,
            track_id: row.get(3)?,
        },
        availability: SourceAvailability::Unknown,
    })
}

fn update_session(tx: &rusqlite::Transaction<'_>, session: &PersistedSession) -> Result<()> {
    let changed = tx.execute(
        "UPDATE playback_sessions SET session_id=?1,queue_revision=?2,checkpoint_sequence=?3,transport_state=?4,current_occurrence_id=?5,position_ms=?6 WHERE singleton_id=1",
        params![session.session_id, session.queue_revision as i64, session.checkpoint_sequence as i64, state_name(session.state), session.current_occurrence_id, session.position_ms as i64],
    )?;
    if changed != 1 {
        return Err(anyhow!("playback session is absent"));
    }
    Ok(())
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
