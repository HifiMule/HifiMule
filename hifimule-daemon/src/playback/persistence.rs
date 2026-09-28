use super::book_progress::{BookMap, BookOccurrenceRecord, BookPart};
use super::model::{
    AuditionOutcome, Occurrence, PersistedAudition, PersistedSession, PlaybackAttempt,
    SourceAvailability, TrackSource, TransportState,
};
use crate::db::Database;
use crate::domain::models::ProviderIdentity;
use crate::providers::BookTiming;
use anyhow::{Result, anyhow};
use rusqlite::{OptionalExtension, params};

pub const PERSISTENCE_VERSION: i64 = 11;
type RadioRow = (
    String,
    Option<String>,
    Option<String>,
    String,
    Option<String>,
    Option<String>,
    i64,
    Option<String>,
);

fn planned_book_successor(
    tx: &rusqlite::Transaction<'_>,
    session: &PersistedSession,
    departed_occurrence_id: &str,
    outcome: &str,
) -> Result<Option<BookOccurrenceRecord>> {
    if outcome != "naturalCompletion" {
        return Ok(None);
    }
    let Some(next_id) = session
        .current_occurrence_id
        .as_deref()
        .filter(|id| *id != departed_occurrence_id)
    else {
        return Ok(None);
    };
    let previous: Option<(String, String, String, String, String)> = tx.query_row(
        "SELECT c.server_id,c.library_id,c.item_id,c.media_id,m.timing_json FROM playback_book_continuity c JOIN playback_book_mapping m ON m.session_id=c.session_id WHERE c.session_id=?1 AND c.occurrence_id=?2 AND c.mapping_valid=1 AND m.queue_revision=?3",
        params![session.session_id, departed_occurrence_id, session.queue_revision as i64],
        |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?)),
    ).optional()?;
    let Some((server_id, library_id, item_id, media_id, timing_json)) = previous else {
        return Ok(None);
    };
    let Ok(timing) = serde_json::from_str::<BookTiming>(&timing_json) else {
        return Ok(None);
    };
    if timing.identity.library_id != library_id
        || timing.identity.library_item_id != item_id
        || timing.identity.media_id != media_id
    {
        return Ok(None);
    }
    let next: Option<(String, String)> = tx.query_row(
        "SELECT server_id,track_id FROM playback_occurrences WHERE session_id=?1 AND occurrence_id=?2",
        params![session.session_id, next_id], |row| Ok((row.get(0)?,row.get(1)?)),
    ).optional()?;
    let Some((next_server, track_id)) = next else {
        return Ok(None);
    };
    if next_server != server_id {
        return Ok(None);
    }
    let Some(map) = BookMap::new(
        timing
            .parts
            .iter()
            .map(|part| BookPart::new(&part.track_id, &part.audio_file_id, part.duration_ms))
            .collect(),
    ) else {
        return Ok(None);
    };
    let Some((part, offset)) = map.part(&track_id) else {
        return Ok(None);
    };
    if map.duration_ms() > i64::MAX as u64 {
        return Ok(None);
    }
    Ok(Some(BookOccurrenceRecord {
        session_id: session.session_id.clone(),
        occurrence_id: next_id.into(),
        server_id,
        track_id,
        identity: timing.identity,
        audio_file_id: part.file_id.clone(),
        part_offset_ms: offset,
        duration_ms: map.duration_ms(),
        whole_ms: offset,
        mapping_valid: true,
    }))
}

impl Database {
    /// Read a frozen Radio decision bound to this exact accepted source copy.
    /// Missing decisions are v10/legacy occurrences and may freeze on their
    /// first successful resolution. Malformed or future decisions are errors.
    pub(crate) fn radio_track_policy(
        &self,
        session_id: &str,
        occurrence: &Occurrence,
    ) -> Result<Option<super::loudness::FrozenTrackPolicy>> {
        let conn = self.conn.lock().unwrap_or_else(|error| error.into_inner());
        let row: Option<(String, String, Option<String>)> = conn
            .query_row(
                "SELECT server_id,track_id,radio_policy_json FROM playback_occurrences WHERE session_id=?1 AND occurrence_id=?2",
                params![session_id, occurrence.occurrence_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let Some((server_id, track_id, json)) = row else {
            return Err(anyhow!("Radio occurrence is absent"));
        };
        if server_id != occurrence.source.server_id || track_id != occurrence.source.track_id {
            return Err(anyhow!("Radio policy source differs from occurrence"));
        }
        json.map(|json| {
            let policy: super::loudness::FrozenTrackPolicy = serde_json::from_str(&json)?;
            policy.validate().map_err(|reason| anyhow!(reason))?;
            Ok(policy)
        })
        .transpose()
    }

    pub(crate) fn freeze_radio_track_policy(
        &self,
        session_id: &str,
        occurrence: &Occurrence,
        candidate: &super::loudness::FrozenTrackPolicy,
        is_current: bool,
    ) -> Result<super::loudness::FrozenTrackPolicy> {
        candidate.validate().map_err(|reason| anyhow!(reason))?;
        let mut conn = self.conn.lock().unwrap_or_else(|error| error.into_inner());
        let tx = conn.transaction()?;
        let row: Option<(String, String, Option<String>)> = tx
            .query_row(
                "SELECT server_id,track_id,radio_policy_json FROM playback_occurrences WHERE session_id=?1 AND occurrence_id=?2",
                params![session_id, occurrence.occurrence_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let Some((server_id, track_id, json)) = row else {
            return Err(anyhow!("Radio occurrence is absent"));
        };
        if server_id != occurrence.source.server_id || track_id != occurrence.source.track_id {
            return Err(anyhow!("Radio policy source differs from occurrence"));
        }
        let policy = if let Some(json) = json {
            let existing: super::loudness::FrozenTrackPolicy = serde_json::from_str(&json)?;
            existing.validate().map_err(|reason| anyhow!(reason))?;
            existing
        } else {
            tx.execute(
                "UPDATE playback_occurrences SET radio_policy_json=?1 WHERE session_id=?2 AND occurrence_id=?3 AND radio_policy_json IS NULL",
                params![serde_json::to_string(candidate)?, session_id, occurrence.occurrence_id],
            )?;
            candidate.clone()
        };
        if is_current {
            let changed = tx.execute(
                "UPDATE playback_sessions SET current_gain_bits=?1,current_qualified_suffix=?2 WHERE session_id=?3 AND current_occurrence_id=?4 AND EXISTS(SELECT 1 FROM playback_radio WHERE session_id=?3)",
                params![i64::from(policy.policy.scalar_bits), policy.qualified_suffix, session_id, occurrence.occurrence_id],
            )?;
            if changed != 1 {
                return Err(anyhow!("Radio current occurrence changed"));
            }
        }
        tx.commit()?;
        Ok(policy)
    }

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
        if version == Some(PERSISTENCE_VERSION) {
            let radio_tables: i64 = tx.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('playback_radio','playback_radio_origin','playback_radio_membership','playback_radio_recording','playback_radio_recording_membership')",
                [], |row| row.get(0))?;
            if radio_tables != 5 {
                return Err(anyhow!("INVALID_RADIO_STATE"));
            }
            let unknown: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM playback_radio_recording WHERE resolver_version<>?1)",
                [super::recording::RESOLVER_VERSION],
                |row| row.get(0),
            )?;
            if unknown {
                return Err(anyhow!("UNSUPPORTED_RECORDING_RESOLVER_VERSION"));
            }
        }
        tx.execute_batch("CREATE TABLE IF NOT EXISTS playback_schema (singleton_id INTEGER PRIMARY KEY CHECK(singleton_id=1), version INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS playback_sessions (singleton_id INTEGER PRIMARY KEY CHECK(singleton_id=1), session_id TEXT NOT NULL, queue_revision INTEGER NOT NULL CHECK(queue_revision>=0), checkpoint_sequence INTEGER NOT NULL CHECK(checkpoint_sequence>=0), transport_state TEXT NOT NULL, current_occurrence_id TEXT, position_ms INTEGER NOT NULL CHECK(position_ms>=0), album_context_json TEXT, queue_kind TEXT NOT NULL DEFAULT 'manual' CHECK(queue_kind IN ('album','manual')), current_gain_bits INTEGER NOT NULL DEFAULT 1065353216 CHECK(current_gain_bits>=0 AND current_gain_bits<=4294967295), current_qualified_suffix TEXT, active_attempt_id TEXT);
            CREATE TABLE IF NOT EXISTS playback_occurrences (session_id TEXT NOT NULL, occurrence_id TEXT NOT NULL UNIQUE, ordinal INTEGER NOT NULL CHECK(ordinal>=0), server_id TEXT NOT NULL, track_id TEXT NOT NULL, radio_policy_json TEXT, PRIMARY KEY(session_id, ordinal));
            CREATE INDEX IF NOT EXISTS playback_occurrences_source ON playback_occurrences(session_id,server_id,track_id);
            CREATE TABLE IF NOT EXISTS playback_attempts (attempt_seq INTEGER PRIMARY KEY AUTOINCREMENT, attempt_id TEXT NOT NULL UNIQUE, session_id TEXT NOT NULL, occurrence_id TEXT NOT NULL, server_id TEXT NOT NULL, track_id TEXT NOT NULL, disposition TEXT CHECK(disposition IN ('naturalCompletion','explicitSkip','technicalFailure','restarted','backNavigation','superseded','interrupted') OR disposition IS NULL), failure_code TEXT, terminal_position_ms INTEGER CHECK(terminal_position_ms>=0), legacy INTEGER NOT NULL DEFAULT 0 CHECK(legacy IN (0,1)));
            CREATE INDEX IF NOT EXISTS playback_attempts_page ON playback_attempts(session_id,attempt_seq);
            CREATE INDEX IF NOT EXISTS playback_attempts_occurrence ON playback_attempts(session_id,occurrence_id,attempt_seq);
            CREATE TABLE IF NOT EXISTS playback_radio (session_id TEXT PRIMARY KEY, logical_id TEXT NOT NULL, center_server_id TEXT, center_artist_id TEXT, status TEXT NOT NULL CHECK(status IN ('filling','ready','waiting','stopped')), reason TEXT, CHECK((center_server_id IS NULL) = (center_artist_id IS NULL)));
            CREATE TABLE IF NOT EXISTS playback_radio_origin (occurrence_id TEXT PRIMARY KEY, session_id TEXT NOT NULL, ordinal INTEGER NOT NULL CHECK(ordinal>=0));
            CREATE INDEX IF NOT EXISTS playback_radio_origin_session ON playback_radio_origin(session_id,ordinal);
            CREATE TABLE IF NOT EXISTS playback_radio_membership (session_id TEXT NOT NULL, server_id TEXT NOT NULL, track_id TEXT NOT NULL, kind TEXT NOT NULL CHECK(kind IN ('heard','excluded')), PRIMARY KEY(session_id,server_id,track_id,kind));
            CREATE INDEX IF NOT EXISTS playback_radio_membership_lookup ON playback_radio_membership(session_id,server_id,track_id);
            CREATE TABLE IF NOT EXISTS playback_radio_recording (occurrence_id TEXT PRIMARY KEY, session_id TEXT NOT NULL, recording_key TEXT NOT NULL, resolver_version INTEGER NOT NULL, provenance TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS playback_radio_recording_key ON playback_radio_recording(session_id,recording_key);
            CREATE TABLE IF NOT EXISTS playback_radio_recording_membership (session_id TEXT NOT NULL, recording_key TEXT NOT NULL, kind TEXT NOT NULL CHECK(kind IN ('heard','excluded')), PRIMARY KEY(session_id,recording_key,kind));
            CREATE INDEX IF NOT EXISTS playback_radio_recording_membership_lookup ON playback_radio_recording_membership(session_id,recording_key);
            CREATE TABLE IF NOT EXISTS playback_radio_scan (session_id TEXT NOT NULL, source_key TEXT NOT NULL, cursor_index INTEGER NOT NULL CHECK(cursor_index>=0), cursor_intra INTEGER NOT NULL CHECK(cursor_intra>=0), exhausted INTEGER NOT NULL CHECK(exhausted IN (0,1)), PRIMARY KEY(session_id,source_key));
            CREATE TABLE IF NOT EXISTS playback_audition (singleton_id INTEGER PRIMARY KEY CHECK(singleton_id=1), audition_id TEXT NOT NULL UNIQUE, parent_session_id TEXT NOT NULL, server_id TEXT NOT NULL, track_id TEXT NOT NULL, position_ms INTEGER NOT NULL CHECK(position_ms>=0), transport_state TEXT NOT NULL, saved_main_occurrence_id TEXT, saved_main_position_ms INTEGER NOT NULL CHECK(saved_main_position_ms>=0), saved_main_intent TEXT NOT NULL, resume_inhibited INTEGER NOT NULL CHECK(resume_inhibited IN (0,1)), contiguous_heard_ms INTEGER NOT NULL CHECK(contiguous_heard_ms>=0), coverage_unknown INTEGER NOT NULL CHECK(coverage_unknown IN (0,1)), seek_discontinuous INTEGER NOT NULL CHECK(seek_discontinuous IN (0,1)));
            CREATE TABLE IF NOT EXISTS playback_audition_outcomes (outcome_id INTEGER PRIMARY KEY AUTOINCREMENT, audition_id TEXT NOT NULL UNIQUE, parent_session_id TEXT NOT NULL, server_id TEXT NOT NULL, track_id TEXT NOT NULL, disposition TEXT NOT NULL CHECK(disposition IN ('naturalCompletion','stopped','returned','replaced','superseded','technicalFailure','interrupted')), terminal_position_ms INTEGER NOT NULL CHECK(terminal_position_ms>=0), duration_ms INTEGER CHECK(duration_ms>=0), failure_code TEXT, contiguous_heard_ms INTEGER NOT NULL CHECK(contiguous_heard_ms>=0), coverage_unknown INTEGER NOT NULL CHECK(coverage_unknown IN (0,1)), seek_discontinuous INTEGER NOT NULL CHECK(seek_discontinuous IN (0,1)), fully_heard INTEGER NOT NULL CHECK(fully_heard IN (0,1)));
            CREATE INDEX IF NOT EXISTS playback_audition_outcomes_page ON playback_audition_outcomes(outcome_id);
            CREATE TABLE IF NOT EXISTS playback_book_continuity (session_id TEXT NOT NULL, occurrence_id TEXT PRIMARY KEY, server_id TEXT NOT NULL, track_id TEXT NOT NULL, library_id TEXT NOT NULL, item_id TEXT NOT NULL, media_id TEXT NOT NULL, audio_file_id TEXT NOT NULL, part_offset_ms INTEGER NOT NULL CHECK(part_offset_ms>=0), duration_ms INTEGER NOT NULL CHECK(duration_ms>0), whole_ms INTEGER NOT NULL CHECK(whole_ms>=0), mapping_valid INTEGER NOT NULL CHECK(mapping_valid IN (0,1)), stop_flush INTEGER NOT NULL DEFAULT 0, report_failed INTEGER NOT NULL DEFAULT 0);
            CREATE TABLE IF NOT EXISTS playback_book_mapping (session_id TEXT PRIMARY KEY, queue_revision INTEGER NOT NULL, timing_json TEXT NOT NULL);
            CREATE TRIGGER IF NOT EXISTS playback_book_mapping_queue AFTER UPDATE OF session_id,queue_revision ON playback_sessions BEGIN DELETE FROM playback_book_mapping WHERE session_id<>NEW.session_id OR queue_revision<>NEW.queue_revision; END;
            CREATE TRIGGER IF NOT EXISTS playback_book_continuity_current AFTER UPDATE OF current_occurrence_id,session_id ON playback_sessions BEGIN DELETE FROM playback_book_continuity WHERE session_id<>NEW.session_id OR occurrence_id<>COALESCE(NEW.current_occurrence_id,''); END;
            CREATE TRIGGER IF NOT EXISTS playback_book_continuity_occurrence AFTER DELETE ON playback_occurrences BEGIN DELETE FROM playback_book_continuity WHERE occurrence_id=OLD.occurrence_id; END;")?;
        let key_pattern = format!("mbrec:{}:%", super::recording::RESOLVER_VERSION);
        let unknown: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM playback_radio_recording WHERE resolver_version<>?1 OR recording_key NOT LIKE ?2 UNION SELECT 1 FROM playback_radio_recording_membership WHERE recording_key NOT LIKE ?2)", params![super::recording::RESOLVER_VERSION,key_pattern], |row|row.get(0))?;
        if unknown {
            return Err(anyhow!("UNSUPPORTED_RECORDING_RESOLVER_VERSION"));
        }
        let radio_columns = {
            let mut statement = tx.prepare("PRAGMA table_info(playback_radio)")?;
            statement
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        if !radio_columns.iter().any(|name| name == "settings_json") {
            tx.execute(
                "ALTER TABLE playback_radio ADD COLUMN settings_json TEXT",
                [],
            )?;
        }
        if !radio_columns.iter().any(|name| name == "cycle") {
            tx.execute("ALTER TABLE playback_radio ADD COLUMN cycle INTEGER NOT NULL DEFAULT 1 CHECK(cycle>0)", [])?;
            tx.execute(
                "UPDATE playback_radio SET status='waiting',reason='radio.snapshotUnavailable'",
                [],
            )?;
        }
        if !radio_columns.iter().any(|name| name == "transition_json") {
            tx.execute(
                "ALTER TABLE playback_radio ADD COLUMN transition_json TEXT",
                [],
            )?;
        }
        let scan_columns = {
            let mut statement = tx.prepare("PRAGMA table_info(playback_radio_scan)")?;
            statement
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        if !scan_columns.iter().any(|name| name == "seen_heard") {
            tx.execute("ALTER TABLE playback_radio_scan ADD COLUMN seen_heard INTEGER NOT NULL DEFAULT 0 CHECK(seen_heard IN (0,1))", [])?;
        }
        let book_columns = {
            let mut statement = tx.prepare("PRAGMA table_info(playback_book_continuity)")?;
            statement
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        if !book_columns.iter().any(|name| name == "stop_flush") {
            tx.execute("ALTER TABLE playback_book_continuity ADD COLUMN stop_flush INTEGER NOT NULL DEFAULT 0", [])?;
        }
        if !book_columns.iter().any(|name| name == "report_failed") {
            tx.execute("ALTER TABLE playback_book_continuity ADD COLUMN report_failed INTEGER NOT NULL DEFAULT 0", [])?;
        }
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
        let has_radio_policy = {
            let mut statement = tx.prepare("PRAGMA table_info(playback_occurrences)")?;
            statement
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<rusqlite::Result<Vec<_>>>()?
                .iter()
                .any(|name| name == "radio_policy_json")
        };
        if !has_radio_policy {
            tx.execute(
                "ALTER TABLE playback_occurrences ADD COLUMN radio_policy_json TEXT",
                [],
            )?;
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
        let has_active_attempt = {
            let mut statement = tx.prepare("PRAGMA table_info(playback_sessions)")?;
            statement
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<rusqlite::Result<Vec<_>>>()?
                .iter()
                .any(|name| name == "active_attempt_id")
        };
        if !has_active_attempt {
            tx.execute(
                "ALTER TABLE playback_sessions ADD COLUMN active_attempt_id TEXT",
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
        if version.unwrap_or(0) < 6 {
            migrate_current_policy(&tx)?;
            migrate_playback_attempts(&tx)?;
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
        let mut loaded = conn.query_row("SELECT session_id,queue_revision,checkpoint_sequence,transport_state,current_occurrence_id,position_ms,album_context_json,queue_kind,current_gain_bits,current_qualified_suffix FROM playback_sessions WHERE singleton_id=1", [], |r| {
            let state: String = r.get(3)?;
            let state = match state.as_str() { "idle"=>TransportState::Idle, "paused"=>TransportState::Paused, "buffering"=>TransportState::Buffering, "playing"=>TransportState::Playing, "stopping"=>TransportState::Stopping, _=>return Err(rusqlite::Error::InvalidQuery) };
            let context_json: Option<String> = r.get(6)?;
            let album_context = context_json.map(|json| serde_json::from_str(&json).map_err(|error| rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(error)))).transpose()?;
            let queue_kind = match r.get::<_, String>(7)?.as_str() { "album" => super::model::QueueKind::Album, "manual" => super::model::QueueKind::Manual, _ => return Err(rusqlite::Error::InvalidQuery) };
            let current_gain_bits = u32::try_from(r.get::<_, i64>(8)?).map_err(|_| rusqlite::Error::InvalidQuery)?;
            Ok(PersistedSession { session_id:r.get(0)?, queue_revision:nonnegative(r,1)?, checkpoint_sequence:nonnegative(r,2)?, state, current_occurrence_id:r.get(4)?, position_ms:nonnegative(r,5)?, queue_kind, radio: None, current_gain_bits, current_qualified_suffix:r.get(9)?, album_context })
        }).optional()?;
        if let Some(session) = loaded.as_mut() {
            let radio: Option<RadioRow> = conn
                .query_row(
                    "SELECT logical_id,center_server_id,center_artist_id,status,reason,settings_json,cycle,transition_json FROM playback_radio WHERE session_id=?1",
                    [&session.session_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?)),
                )
                .optional()?;
            if let Some((
                logical_id,
                server_id,
                artist_id,
                status,
                reason,
                settings_json,
                cycle,
                transition_json,
            )) = radio
            {
                if uuid::Uuid::parse_str(&logical_id).is_err() {
                    return Err(anyhow!("INVALID_RADIO_STATE"));
                }
                use super::radio::{ArtistIdentity, RadioState, RadioStatus};
                let center = match (server_id, artist_id) {
                    (Some(server_id), Some(artist_id))
                        if !server_id.is_empty() && !artist_id.is_empty() =>
                    {
                        Some(ArtistIdentity {
                            server_id,
                            artist_id,
                        })
                    }
                    (None, None) => None,
                    _ => return Err(anyhow!("INVALID_RADIO_STATE")),
                };
                let mut status = match status.as_str() {
                    "filling" => RadioStatus::Filling,
                    "ready" => RadioStatus::Ready,
                    "waiting" => RadioStatus::Waiting,
                    "stopped" => RadioStatus::Stopped,
                    _ => return Err(anyhow!("INVALID_RADIO_STATE")),
                };
                let original_settings = settings_json
                    .and_then(|json| {
                        serde_json::from_str::<super::selection::PlaybackSelectionConfig>(&json)
                            .ok()
                    })
                    .filter(|settings| settings.validate().is_ok());
                let transition = transition_json
                    .as_deref()
                    .map(serde_json::from_str::<super::radio::RadioTransition>)
                    .transpose();
                let transition_invalid = match &transition {
                    Ok(Some(value)) => {
                        center.as_ref() != Some(&value.center)
                            || super::radio::RadioTransition::new(value.center.clone(), value.kind)
                                .reason
                                != value.reason
                    }
                    Err(_) => true,
                    Ok(None) => false,
                };
                let reason = if original_settings.is_none() || transition_invalid {
                    status = RadioStatus::Waiting;
                    Some("radio.snapshotUnavailable".into())
                } else {
                    reason
                };
                session.queue_kind = super::model::QueueKind::Radio;
                let radio = RadioState {
                    logical_id,
                    center,
                    status,
                    reason,
                    original_settings,
                    cycle: u64::try_from(cycle)?,
                    transition: (!transition_invalid)
                        .then(|| transition.ok().flatten())
                        .flatten(),
                };
                radio
                    .validate()
                    .map_err(|_| anyhow!("INVALID_RADIO_STATE"))?;
                session.radio = Some(radio);
            }
        }
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
        let active_attempt_id: Option<String> = tx.query_row(
            "SELECT active_attempt_id FROM playback_sessions WHERE singleton_id=1",
            [],
            |row| row.get(0),
        )?;
        if let Some(active_attempt_id) = active_attempt_id.as_deref() {
            let active_matches: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM playback_attempts WHERE attempt_id=?1 AND session_id=?2 AND occurrence_id=?3 AND disposition IS NULL)",
                params![active_attempt_id, session.session_id, session.current_occurrence_id],
                |row| row.get(0),
            )?;
            if !active_matches {
                return Err(anyhow!("INVALID_SESSION"));
            }
        }
        {
            let mut statement = tx.prepare(
                "SELECT attempt_id,occurrence_id,server_id,track_id,failure_code,terminal_position_ms FROM playback_attempts WHERE session_id=?1 ORDER BY attempt_seq",
            )?;
            let mut rows = statement.query([&session.session_id])?;
            while let Some(row) = rows.next()? {
                let attempt_id: String = row.get(0)?;
                let occurrence_id: String = row.get(1)?;
                let source = TrackSource {
                    server_id: row.get(2)?,
                    track_id: row.get(3)?,
                };
                let failure_code: Option<String> = row.get(4)?;
                let terminal_position: Option<i64> = row.get(5)?;
                if uuid::Uuid::parse_str(&attempt_id).is_err()
                    || uuid::Uuid::parse_str(&occurrence_id).is_err()
                    || source.validate().is_err()
                    || failure_code.is_some_and(|code| {
                        code.is_empty()
                            || code.len() > 128
                            || !code.bytes().all(|byte| {
                                byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_'
                            })
                    })
                    || terminal_position.is_some_and(|position| {
                        position < 0 || position as u64 > 9_007_199_254_740_991
                    })
                {
                    return Err(anyhow!("INVALID_SESSION"));
                }
            }
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

    pub fn radio_has_membership(&self, session_id: &str, source: &TrackSource) -> Result<bool> {
        let conn = self.conn.lock().unwrap_or_else(|error| error.into_inner());
        Ok(conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM playback_radio_membership WHERE session_id=?1 AND server_id=?2 AND track_id=?3)",
            params![session_id, source.server_id, source.track_id],
            |row| row.get(0),
        )?)
    }

    pub fn radio_recording_used(&self, session_id: &str, recording_key: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap_or_else(|error| error.into_inner());
        Ok(conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM playback_radio_recording_membership WHERE session_id=?1 AND recording_key=?2 UNION SELECT 1 FROM playback_radio_recording r JOIN playback_occurrences o ON o.occurrence_id=r.occurrence_id AND o.session_id=r.session_id WHERE r.session_id=?1 AND r.recording_key=?2 AND o.outcome IS NULL)",
            params![session_id, recording_key], |row| row.get(0),
        )?)
    }

    pub fn radio_recording_has_kind(
        &self,
        session_id: &str,
        recording_key: &str,
        kind: &str,
    ) -> Result<bool> {
        let conn = self.conn.lock().unwrap_or_else(|error| error.into_inner());
        Ok(conn.query_row("SELECT EXISTS(SELECT 1 FROM playback_radio_recording_membership WHERE session_id=?1 AND recording_key=?2 AND kind=?3)", params![session_id,recording_key,kind], |row|row.get(0))?)
    }

    pub fn radio_is_heard(&self, session_id: &str, source: &TrackSource) -> Result<bool> {
        let conn = self.conn.lock().unwrap_or_else(|error| error.into_inner());
        Ok(conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM playback_radio_membership WHERE session_id=?1 AND server_id=?2 AND track_id=?3 AND kind='heard')",
            params![session_id, source.server_id, source.track_id], |row| row.get(0),
        )?)
    }

    pub fn radio_is_excluded(&self, session_id: &str, source: &TrackSource) -> Result<bool> {
        let conn = self.conn.lock().unwrap_or_else(|error| error.into_inner());
        Ok(conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM playback_radio_membership WHERE session_id=?1 AND server_id=?2 AND track_id=?3 AND kind='excluded')",
            params![session_id, source.server_id, source.track_id], |row| row.get(0),
        )?)
    }

    pub fn radio_scan_cursor(
        &self,
        session_id: &str,
        source_key: &str,
    ) -> Result<Option<super::selection::RadioSourceCursor>> {
        let conn = self.conn.lock().unwrap_or_else(|error| error.into_inner());
        let row: Option<(i64, i64, i64)> = conn.query_row(
            "SELECT cursor_index,cursor_intra,exhausted FROM playback_radio_scan WHERE session_id=?1 AND source_key=?2",
            params![session_id, source_key],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).optional()?;
        match row {
            Some((_, _, 1)) => Ok(None),
            Some((index, intra, 0)) => Ok(Some(super::selection::RadioSourceCursor {
                index: u32::try_from(index)?,
                intra: u32::try_from(intra)?,
            })),
            None => Ok(Some(super::selection::RadioSourceCursor::default())),
            _ => Err(anyhow!("INVALID_RADIO_STATE")),
        }
    }

    pub fn advance_radio_scan(
        &self,
        session_id: &str,
        expected_revision: u64,
        source_key: &str,
        cursor: super::selection::RadioSourceCursor,
        next: Option<super::selection::RadioSourceCursor>,
    ) -> Result<bool> {
        self.advance_radio_scan_with_heard(
            session_id,
            expected_revision,
            source_key,
            cursor,
            next,
            false,
        )
    }

    pub fn advance_radio_scan_with_heard(
        &self,
        session_id: &str,
        expected_revision: u64,
        source_key: &str,
        cursor: super::selection::RadioSourceCursor,
        next: Option<super::selection::RadioSourceCursor>,
        seen_heard: bool,
    ) -> Result<bool> {
        let mut conn = self.conn.lock().unwrap_or_else(|error| error.into_inner());
        let tx = conn.transaction()?;
        let active: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM playback_radio r JOIN playback_sessions s ON s.session_id=r.session_id WHERE r.session_id=?1 AND s.queue_revision=?2 AND r.status='filling' AND NOT EXISTS(SELECT 1 FROM playback_audition a WHERE a.parent_session_id=r.session_id))",
            params![session_id, i64::try_from(expected_revision)?],
            |row| row.get(0),
        )?;
        if !active {
            return Ok(false);
        }
        let stored: Option<(i64, i64, i64)> = tx.query_row(
            "SELECT cursor_index,cursor_intra,exhausted FROM playback_radio_scan WHERE session_id=?1 AND source_key=?2",
            params![session_id, source_key],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).optional()?;
        if stored.is_some_and(|(index, intra, exhausted)| {
            index != i64::from(cursor.index) || intra != i64::from(cursor.intra) || exhausted != 0
        }) {
            return Ok(false);
        }
        let (index, intra, exhausted) = next.map_or((0, 0, 1), |next| {
            (i64::from(next.index), i64::from(next.intra), 0)
        });
        tx.execute(
            "INSERT INTO playback_radio_scan(session_id,source_key,cursor_index,cursor_intra,exhausted,seen_heard) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(session_id,source_key) DO UPDATE SET cursor_index=excluded.cursor_index,cursor_intra=excluded.cursor_intra,exhausted=excluded.exhausted,seen_heard=MAX(playback_radio_scan.seen_heard,excluded.seen_heard)",
            params![session_id, source_key, index, intra, exhausted, seen_heard as i64],
        )?;
        tx.commit()?;
        Ok(true)
    }

    pub fn radio_fresh_scan_saw_heard(&self, session_id: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap_or_else(|error| error.into_inner());
        Ok(conn.query_row("SELECT EXISTS(SELECT 1 FROM playback_radio_scan WHERE session_id=?1 AND source_key LIKE 'fresh:%' AND seen_heard=1)", [session_id], |row| row.get(0))?)
    }

    pub fn renew_radio_cycle(&self, session: &PersistedSession, expected_cycle: u64) -> Result<()> {
        let mut conn = self.conn.lock().unwrap_or_else(|error| error.into_inner());
        let tx = conn.transaction()?;
        let active: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM playback_radio r JOIN playback_sessions s ON s.session_id=r.session_id WHERE r.session_id=?1 AND s.queue_revision=?2 AND r.cycle=?3 AND r.status='filling')",
            params![session.session_id, i64::try_from(session.queue_revision)?, i64::try_from(expected_cycle)?], |row| row.get(0),
        )?;
        if !active
            || session
                .radio
                .as_ref()
                .is_none_or(|radio| radio.cycle != expected_cycle + 1)
        {
            return Err(anyhow!("RADIO_CYCLE_CHANGED"));
        }
        tx.execute(
            "DELETE FROM playback_radio_membership WHERE session_id=?1 AND kind='heard'",
            [&session.session_id],
        )?;
        tx.execute(
            "DELETE FROM playback_radio_recording_membership WHERE session_id=?1 AND kind='heard'",
            [&session.session_id],
        )?;
        tx.execute(
            "DELETE FROM playback_radio_scan WHERE session_id=?1",
            [&session.session_id],
        )?;
        store_radio_state(&tx, session)?;
        tx.commit()?;
        Ok(())
    }

    pub fn radio_has_occurrence(&self, session_id: &str, source: &TrackSource) -> Result<bool> {
        let conn = self.conn.lock().unwrap_or_else(|error| error.into_inner());
        Ok(conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM playback_occurrences WHERE session_id=?1 AND server_id=?2 AND track_id=?3 AND outcome IS NULL)",
            params![session_id,source.server_id,source.track_id], |row| row.get(0))?)
    }

    pub fn checkpoint_radio_state(&self, session: &PersistedSession) -> Result<()> {
        let mut conn = self.conn.lock().unwrap_or_else(|error| error.into_inner());
        let tx = conn.transaction()?;
        let revision: i64 = tx.query_row(
            "SELECT queue_revision FROM playback_sessions WHERE singleton_id=1 AND session_id=?1",
            [&session.session_id],
            |row| row.get(0),
        )?;
        if revision != i64::try_from(session.queue_revision)? {
            return Err(anyhow!("RADIO_REVISION_CHANGED"));
        }
        let previous_reason: Option<String> = tx
            .query_row(
                "SELECT reason FROM playback_radio WHERE session_id=?1",
                [&session.session_id],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        if previous_reason.as_deref() == Some("radio.exhausted")
            && session
                .radio
                .as_ref()
                .is_some_and(|radio| radio.status == super::radio::RadioStatus::Ready)
        {
            // Explicit Retry rechecks an exhausted source after its catalog changes.
            // Heard and excluded membership remains intact.
            tx.execute(
                "DELETE FROM playback_radio_scan WHERE session_id=?1",
                [&session.session_id],
            )?;
        }
        store_radio_state(&tx, session)?;
        tx.commit()?;
        Ok(())
    }

    pub fn radio_auto_upcoming_count(&self, session_id: &str, current_ordinal: u64) -> Result<u64> {
        let conn = self.conn.lock().unwrap_or_else(|error| error.into_inner());
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM playback_radio_origin INDEXED BY playback_radio_origin_session WHERE session_id=?1 AND ordinal>?2",
            params![session_id, i64::try_from(current_ordinal)?],
            |row| row.get::<_, i64>(0),
        )? as u64)
    }

    pub fn append_radio_occurrences(
        &self,
        session: &PersistedSession,
        prior_current_id: &str,
        occurrences: &[Occurrence],
        candidates: &[super::selection::SelectionCandidate],
    ) -> Result<()> {
        if session.queue_kind != super::model::QueueKind::Radio
            || occurrences.len() > super::radio::AUTO_UPCOMING_TARGET
            || occurrences.len() != candidates.len()
        {
            return Err(anyhow!("INVALID_RADIO_STATE"));
        }
        let mut conn = self.conn.lock().unwrap_or_else(|error| error.into_inner());
        let tx = conn.transaction()?;
        let (stored_revision,stored_current): (i64,Option<String>) = tx.query_row(
            "SELECT queue_revision,current_occurrence_id FROM playback_sessions WHERE singleton_id=1 AND session_id=?1",
            [&session.session_id], |row|Ok((row.get(0)?,row.get(1)?)))?;
        if stored_revision
            != i64::try_from(
                session
                    .queue_revision
                    .checked_sub(1)
                    .ok_or_else(|| anyhow!("RADIO_REVISION_CHANGED"))?,
            )?
            || stored_current.as_deref() != Some(prior_current_id)
        {
            return Err(anyhow!("RADIO_REVISION_CHANGED"));
        }
        let current_ordinal: i64 = tx.query_row(
            "SELECT ordinal FROM playback_occurrences WHERE session_id=?1 AND occurrence_id=?2",
            params![session.session_id, prior_current_id],
            |row| row.get(0),
        )?;
        if session.current_occurrence_id.as_deref() != Some(prior_current_id) {
            let completed: bool = tx.query_row(
                "SELECT outcome='naturalCompletion' FROM playback_occurrences WHERE session_id=?1 AND occurrence_id=?2",
                params![session.session_id,prior_current_id], |row|row.get(0))?;
            if !completed
                || session.current_occurrence_id.as_deref()
                    != occurrences.first().map(|item| item.occurrence_id.as_str())
            {
                return Err(anyhow!("RADIO_CURRENT_CHANGED"));
            }
        }
        let count: i64 = tx.query_row(
            "SELECT COUNT(*) FROM playback_radio_origin INDEXED BY playback_radio_origin_session WHERE session_id=?1 AND ordinal>?2",
            params![session.session_id,current_ordinal], |row| row.get(0))?;
        if count >= super::radio::AUTO_UPCOMING_TRIGGER as i64
            || count + occurrences.len() as i64 > super::radio::AUTO_UPCOMING_TARGET as i64
        {
            return Err(anyhow!("RADIO_CAPACITY_CHANGED"));
        }
        let mut seen = std::collections::HashSet::new();
        let mut seen_recordings = std::collections::HashSet::new();
        for (occurrence, candidate) in occurrences.iter().zip(candidates) {
            if occurrence.source != candidate.source {
                return Err(anyhow!("RADIO_SOURCE_CHANGED"));
            }
            occurrence
                .source
                .validate()
                .map_err(|_| anyhow!("INVALID_RADIO_SOURCE"))?;
            if !seen.insert((&occurrence.source.server_id, &occurrence.source.track_id)) {
                return Err(anyhow!("DUPLICATE_RADIO_SOURCE"));
            }
            let used: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM playback_radio_membership WHERE session_id=?1 AND server_id=?2 AND track_id=?3 UNION SELECT 1 FROM playback_occurrences WHERE session_id=?1 AND server_id=?2 AND track_id=?3 AND outcome IS NULL)", params![session.session_id,occurrence.source.server_id,occurrence.source.track_id], |row| row.get(0))?;
            if used {
                return Err(anyhow!("RADIO_SOURCE_ALREADY_USED"));
            }
            if let Some(key) = candidate
                .song
                .provider_metadata
                .recording
                .as_ref()
                .and_then(|e| e.key())
            {
                if !seen_recordings.insert(key.as_str()) {
                    return Err(anyhow!("DUPLICATE_RADIO_RECORDING"));
                }
                let used: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM playback_radio_recording_membership WHERE session_id=?1 AND recording_key=?2 UNION SELECT 1 FROM playback_radio_recording r JOIN playback_occurrences o ON o.occurrence_id=r.occurrence_id AND o.session_id=r.session_id WHERE r.session_id=?1 AND r.recording_key=?2 AND o.outcome IS NULL)", params![session.session_id,key.as_str()], |row|row.get(0))?;
                if used {
                    return Err(anyhow!("RADIO_RECORDING_ALREADY_USED"));
                }
            }
        }
        let previous_center: Option<(Option<String>, Option<String>, i64)> = tx.query_row(
            "SELECT center_server_id,center_artist_id,cycle FROM playback_radio WHERE session_id=?1",
            [&session.session_id], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?)),
        ).optional()?;
        if previous_center
            .as_ref()
            .is_some_and(|(server, artist, cycle)| {
                server.as_deref()
                    != session
                        .radio
                        .as_ref()
                        .and_then(|radio| radio.center.as_ref())
                        .map(|center| center.server_id.as_str())
                    || artist.as_deref()
                        != session
                            .radio
                            .as_ref()
                            .and_then(|radio| radio.center.as_ref())
                            .map(|center| center.artist_id.as_str())
                    || *cycle
                        != session
                            .radio
                            .as_ref()
                            .map(|radio| radio.cycle as i64)
                            .unwrap_or_default()
            })
        {
            tx.execute(
                "DELETE FROM playback_radio_scan WHERE session_id=?1",
                [&session.session_id],
            )?;
        }
        update_session(&tx, session)?;
        insert_occurrences(&tx, &session.session_id, occurrences)?;
        for (occurrence, candidate) in occurrences.iter().zip(candidates) {
            tx.execute(
                "INSERT INTO playback_radio_origin(occurrence_id,session_id,ordinal) VALUES(?1,?2,?3)",
                params![occurrence.occurrence_id, session.session_id, i64::try_from(occurrence.ordinal)?],
            )?;
            insert_radio_recording(
                &tx,
                &session.session_id,
                &occurrence.occurrence_id,
                candidate.song.provider_metadata.recording.as_ref(),
            )?;
        }
        ensure_active_attempt(&tx, session, "superseded")?;
        tx.commit()?;
        Ok(())
    }

    pub fn persist_playback_structure(
        &self,
        session: &PersistedSession,
        occurrences: &[Occurrence],
    ) -> Result<()> {
        self.persist_playback_structure_inner(session, occurrences, false, None)
    }

    pub fn persist_radio_start_structure(
        &self,
        session: &PersistedSession,
        occurrences: &[Occurrence],
        evidence: &super::recording::RecordingEvidence,
    ) -> Result<()> {
        self.persist_playback_structure_inner(session, occurrences, false, Some(evidence))
    }

    pub fn persist_playback_structure_superseding_audition(
        &self,
        session: &PersistedSession,
        occurrences: &[Occurrence],
        audition: &PersistedAudition,
        recording: Option<&super::recording::RecordingEvidence>,
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
        close_active_attempt(&tx, &session.session_id, "superseded")?;
        let album_context_json = album_context_json(session)?;
        tx.execute("INSERT INTO playback_sessions(singleton_id,session_id,queue_revision,checkpoint_sequence,transport_state,current_occurrence_id,position_ms,album_context_json,queue_kind,current_gain_bits,current_qualified_suffix) VALUES(1,?1,?2,?3,?4,?5,?6,?7,?8,?9,?10) ON CONFLICT(singleton_id) DO UPDATE SET session_id=excluded.session_id,queue_revision=excluded.queue_revision,checkpoint_sequence=excluded.checkpoint_sequence,transport_state=excluded.transport_state,current_occurrence_id=excluded.current_occurrence_id,position_ms=excluded.position_ms,album_context_json=excluded.album_context_json,queue_kind=excluded.queue_kind,current_gain_bits=excluded.current_gain_bits,current_qualified_suffix=excluded.current_qualified_suffix", params![session.session_id,session.queue_revision as i64,session.checkpoint_sequence as i64,state_name(session.state),session.current_occurrence_id,session.position_ms as i64,album_context_json,queue_kind_name(session.queue_kind),i64::from(session.current_gain_bits),session.current_qualified_suffix])?;
        replace_radio_state(&tx, session)?;
        tx.execute(
            "DELETE FROM playback_occurrences WHERE session_id=?1",
            [&session.session_id],
        )?;
        insert_occurrences(&tx, &session.session_id, occurrences)?;
        if let Some(evidence) = recording {
            let first = occurrences
                .first()
                .ok_or_else(|| anyhow!("RADIO_CURRENT_CHANGED"))?;
            insert_radio_recording(
                &tx,
                &session.session_id,
                &first.occurrence_id,
                Some(evidence),
            )?;
        }
        ensure_active_attempt(&tx, session, "superseded")?;
        tx.commit()?;
        Ok(())
    }

    fn persist_playback_structure_inner(
        &self,
        session: &PersistedSession,
        occurrences: &[Occurrence],
        fail_after_delete: bool,
        recording: Option<&super::recording::RecordingEvidence>,
    ) -> Result<()> {
        let mut conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let tx = conn.transaction()?;
        close_active_attempt(&tx, &session.session_id, "superseded")?;
        let album_context_json = album_context_json(session)?;
        tx.execute("INSERT INTO playback_sessions(singleton_id,session_id,queue_revision,checkpoint_sequence,transport_state,current_occurrence_id,position_ms,album_context_json,queue_kind,current_gain_bits,current_qualified_suffix) VALUES(1,?1,?2,?3,?4,?5,?6,?7,?8,?9,?10) ON CONFLICT(singleton_id) DO UPDATE SET session_id=excluded.session_id,queue_revision=excluded.queue_revision,checkpoint_sequence=excluded.checkpoint_sequence,transport_state=excluded.transport_state,current_occurrence_id=excluded.current_occurrence_id,position_ms=excluded.position_ms,album_context_json=excluded.album_context_json,queue_kind=excluded.queue_kind,current_gain_bits=excluded.current_gain_bits,current_qualified_suffix=excluded.current_qualified_suffix", params![session.session_id,session.queue_revision as i64,session.checkpoint_sequence as i64,state_name(session.state),session.current_occurrence_id,session.position_ms as i64,album_context_json,queue_kind_name(session.queue_kind),i64::from(session.current_gain_bits),session.current_qualified_suffix])?;
        replace_radio_state(&tx, session)?;
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
        if let Some(evidence) = recording {
            let first = occurrences
                .first()
                .ok_or_else(|| anyhow!("RADIO_CURRENT_CHANGED"))?;
            insert_radio_recording(
                &tx,
                &session.session_id,
                &first.occurrence_id,
                Some(evidence),
            )?;
        }
        ensure_active_attempt(&tx, session, "superseded")?;
        tx.commit()?;
        Ok(())
    }

    #[cfg(test)]
    pub fn persist_playback_structure_with_injected_failure(
        &self,
        session: &PersistedSession,
        occurrences: &[Occurrence],
    ) -> Result<()> {
        self.persist_playback_structure_inner(session, occurrences, true, None)
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
        ensure_active_attempt(&tx, session, "superseded")?;
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
        ensure_active_attempt(&tx, session, "superseded")?;
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
            tx.execute(
                "DELETE FROM playback_radio_origin WHERE occurrence_id=?1 AND session_id=?2",
                params![occurrence_id, session.session_id],
            )?;
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

    pub fn remove_playback_upcoming(
        &self,
        session: &PersistedSession,
        current_ordinal: u64,
        occurrence_ids: &[String],
    ) -> Result<()> {
        let mut conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let tx = conn.transaction()?;
        validate_edit_baseline(&tx, session)?;
        for occurrence_id in occurrence_ids {
            if session.queue_kind == super::model::QueueKind::Radio {
                tx.execute(
                    "INSERT OR IGNORE INTO playback_radio_membership(session_id,server_id,track_id,kind) SELECT o.session_id,o.server_id,o.track_id,'excluded' FROM playback_occurrences o JOIN playback_radio_origin r ON r.occurrence_id=o.occurrence_id WHERE o.session_id=?1 AND o.occurrence_id=?2 AND o.ordinal>?3",
                    params![session.session_id,occurrence_id,i64::try_from(current_ordinal)?],
                )?;
                tx.execute("INSERT OR IGNORE INTO playback_radio_recording_membership(session_id,recording_key,kind) SELECT r.session_id,r.recording_key,'excluded' FROM playback_radio_recording r JOIN playback_radio_origin o ON o.occurrence_id=r.occurrence_id WHERE r.session_id=?1 AND r.occurrence_id=?2 AND o.ordinal>?3", params![session.session_id,occurrence_id,i64::try_from(current_ordinal)?])?;
            }
            let changed = tx.execute(
                "DELETE FROM playback_occurrences WHERE session_id=?1 AND occurrence_id=?2 AND ordinal>?3",
                params![session.session_id, occurrence_id, i64::try_from(current_ordinal)?],
            )?;
            if changed != 1 {
                return Err(anyhow!("removed occurrence is no longer upcoming"));
            }
            tx.execute(
                "DELETE FROM playback_radio_origin WHERE session_id=?1 AND occurrence_id=?2",
                params![session.session_id, occurrence_id],
            )?;
        }
        update_session(&tx, session)?;
        tx.commit()?;
        Ok(())
    }

    pub fn move_playback_upcoming(
        &self,
        session: &PersistedSession,
        current_ordinal: u64,
        occurrence_id: &str,
        before_occurrence_id: Option<&str>,
    ) -> Result<()> {
        let mut conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let tx = conn.transaction()?;
        validate_edit_baseline(&tx, session)?;
        let current_ordinal = i64::try_from(current_ordinal)?;
        let moved_exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM playback_occurrences WHERE session_id=?1 AND occurrence_id=?2 AND ordinal>?3)",
            params![session.session_id, occurrence_id, current_ordinal],
            |row| row.get(0),
        )?;
        if !moved_exists {
            return Err(anyhow!("moved occurrence is no longer upcoming"));
        }
        let anchor_ordinal: Option<i64> = before_occurrence_id
            .map(|anchor| {
                tx.query_row(
                    "SELECT ordinal FROM playback_occurrences WHERE session_id=?1 AND occurrence_id=?2 AND ordinal>?3",
                    params![session.session_id, anchor, current_ordinal],
                    |row| row.get(0),
                )
                .optional()
                .map(|value| value.ok_or_else(|| rusqlite::Error::QueryReturnedNoRows))?
            })
            .transpose()?;
        let move_key = anchor_ordinal.unwrap_or(i64::MAX);
        tx.execute_batch("DROP TABLE IF EXISTS temp.playback_move_rows")?;
        tx.execute(
            "CREATE TEMP TABLE playback_move_rows AS SELECT occurrence_id,server_id,track_id,outcome,failure_code, CASE WHEN occurrence_id=?1 THEN ?4 ELSE ordinal END AS move_key, CASE WHEN occurrence_id=?1 THEN 0 ELSE 1 END AS tie_key, ordinal AS old_ordinal FROM playback_occurrences WHERE session_id=?2 AND ordinal>?3",
            params![occurrence_id, session.session_id, current_ordinal, move_key],
        )?;
        tx.execute(
            "DELETE FROM playback_occurrences WHERE session_id=?1 AND ordinal>?2",
            params![session.session_id, current_ordinal],
        )?;
        tx.execute(
            "INSERT INTO playback_occurrences(session_id,occurrence_id,ordinal,server_id,track_id,outcome,failure_code) SELECT ?1,occurrence_id,?2 + ROW_NUMBER() OVER (ORDER BY move_key,tie_key,old_ordinal),server_id,track_id,outcome,failure_code FROM playback_move_rows",
            params![session.session_id, current_ordinal],
        )?;
        tx.execute_batch("DROP TABLE temp.playback_move_rows")?;
        tx.execute(
            "UPDATE playback_radio_origin SET ordinal=(SELECT ordinal FROM playback_occurrences WHERE occurrence_id=playback_radio_origin.occurrence_id) WHERE session_id=?1 AND ordinal>?2",
            params![session.session_id,current_ordinal],
        )?;
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
        close_active_attempt(&tx, &session.session_id, "superseded")?;
        update_session(&tx, session)?;
        let (server_id, track_id): (String, String) = tx.query_row(
            "SELECT server_id,track_id FROM playback_occurrences WHERE session_id=?1 AND occurrence_id=?2",
            params![session.session_id, current],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let attempt_id = uuid::Uuid::new_v4().to_string();
        tx.execute(
            "INSERT INTO playback_attempts(attempt_id,session_id,occurrence_id,server_id,track_id,legacy) VALUES(?1,?2,?3,?4,?5,0)",
            params![attempt_id, session.session_id, current, server_id, track_id],
        )?;
        tx.execute(
            "UPDATE playback_sessions SET active_attempt_id=?1 WHERE singleton_id=1 AND session_id=?2",
            params![attempt_id, session.session_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn persist_playback_terminal(
        &self,
        session: &PersistedSession,
        departed_occurrence_id: &str,
        outcome: &str,
        failure_code: Option<&str>,
        terminal_position_ms: u64,
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
        let book_successor = planned_book_successor(&tx, session, departed_occurrence_id, outcome)?;
        let terminal_position = i64::try_from(terminal_position_ms)?;
        let changed = tx.execute(
            "UPDATE playback_attempts SET disposition=?1,failure_code=?4,terminal_position_ms=?5 WHERE attempt_id=(SELECT active_attempt_id FROM playback_sessions WHERE singleton_id=1 AND session_id=?2) AND session_id=?2 AND occurrence_id=?3 AND disposition IS NULL",
            params![outcome, session.session_id, departed_occurrence_id, failure_code, terminal_position],
        )?;
        if changed != 1 {
            return Err(anyhow!("terminal playback outcome was already consumed"));
        }
        tx.execute(
            "UPDATE playback_occurrences SET outcome=?1,failure_code=?4 WHERE session_id=?2 AND occurrence_id=?3 AND outcome IS NULL",
            params![outcome, session.session_id, departed_occurrence_id, failure_code],
        )?;
        if session.queue_kind == super::model::QueueKind::Radio
            && matches!(outcome, "naturalCompletion" | "explicitSkip")
        {
            let kind = if outcome == "naturalCompletion" {
                "heard"
            } else {
                "excluded"
            };
            tx.execute(
                "INSERT OR IGNORE INTO playback_radio_membership(session_id,server_id,track_id,kind) SELECT session_id,server_id,track_id,?3 FROM playback_occurrences WHERE session_id=?1 AND occurrence_id=?2",
                params![session.session_id,departed_occurrence_id,kind],
            )?;
            tx.execute("INSERT OR IGNORE INTO playback_radio_recording_membership(session_id,recording_key,kind) SELECT session_id,recording_key,?3 FROM playback_radio_recording WHERE session_id=?1 AND occurrence_id=?2", params![session.session_id,departed_occurrence_id,kind])?;
        }
        if session.queue_kind == super::model::QueueKind::Radio
            && outcome == "naturalCompletion"
            && session.radio.as_ref().is_some_and(|radio| {
                radio.status == super::radio::RadioStatus::Ready && radio.reason.is_none()
            })
        {
            let prior_reason: Option<String> = tx
                .query_row(
                    "SELECT reason FROM playback_radio WHERE session_id=?1",
                    [&session.session_id],
                    |row| row.get(0),
                )
                .optional()?
                .flatten();
            if prior_reason.as_deref() == Some("radio.exhausted") {
                tx.execute(
                    "DELETE FROM playback_radio_scan WHERE session_id=?1",
                    [&session.session_id],
                )?;
            }
        }
        update_session(&tx, session)?;
        if let Some(record) = book_successor {
            tx.execute(
                "INSERT INTO playback_book_continuity(session_id,occurrence_id,server_id,track_id,library_id,item_id,media_id,audio_file_id,part_offset_ms,duration_ms,whole_ms,mapping_valid) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,1)",
                params![record.session_id,record.occurrence_id,record.server_id,record.track_id,record.identity.library_id,record.identity.library_item_id,record.identity.media_id,record.audio_file_id,record.part_offset_ms as i64,record.duration_ms as i64,record.whole_ms as i64],
            )?;
        }
        tx.execute(
            "UPDATE playback_sessions SET active_attempt_id=NULL WHERE singleton_id=1 AND session_id=?1",
            [&session.session_id],
        )?;
        open_current_attempt_after_departure(&tx, session, departed_occurrence_id)?;
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
        let terminal_exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM playback_attempts WHERE session_id=?1 AND occurrence_id=?2 AND disposition IS NOT NULL)",
            params![session.session_id, departed_occurrence_id],
            |row| row.get(0),
        )?;
        if !terminal_exists {
            return Err(anyhow!("terminal playback outcome is absent"));
        }
        update_session(&tx, session)?;
        open_current_attempt_after_departure(&tx, session, departed_occurrence_id)?;
        tx.commit()?;
        Ok(())
    }

    /// Start a fresh visit while keeping all earlier terminal evidence immutable.
    pub fn reset_playback_attempt(&self, session: &PersistedSession) -> Result<()> {
        let occurrence_id = session
            .current_occurrence_id
            .as_deref()
            .ok_or_else(|| anyhow!("current playback occurrence is absent"))?;
        let occurrence = self
            .playback_occurrence(&session.session_id, occurrence_id)?
            .ok_or_else(|| anyhow!("current playback occurrence is absent"))?;
        self.start_playback_attempt(session, &occurrence, None, None)
    }

    pub fn start_playback_attempt(
        &self,
        session: &PersistedSession,
        occurrence: &Occurrence,
        close_disposition: Option<&str>,
        close_position_ms: Option<u64>,
    ) -> Result<()> {
        if session.current_occurrence_id.as_deref() != Some(&occurrence.occurrence_id) {
            return Err(anyhow!("attempt target is not current"));
        }
        if close_disposition.is_some_and(|value| {
            !matches!(
                value,
                "restarted" | "backNavigation" | "superseded" | "interrupted"
            )
        }) {
            return Err(anyhow!("invalid playback attempt disposition"));
        }
        let mut conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let tx = conn.transaction()?;
        let active: Option<(String, i64)> = tx
            .query_row(
                "SELECT active_attempt_id,position_ms FROM playback_sessions WHERE singleton_id=1 AND session_id=?1 AND active_attempt_id IS NOT NULL",
                [&session.session_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((attempt_id, position_ms)) = active {
            let disposition = close_disposition
                .ok_or_else(|| anyhow!("active playback attempt is still open"))?;
            let terminal_position_ms = close_position_ms
                .map(i64::try_from)
                .transpose()?
                .unwrap_or(position_ms);
            let changed = tx.execute(
                "UPDATE playback_attempts SET disposition=?1,terminal_position_ms=?2 WHERE attempt_id=?3 AND disposition IS NULL",
                params![disposition, terminal_position_ms, attempt_id],
            )?;
            if changed != 1 {
                return Err(anyhow!("active playback attempt changed"));
            }
        }
        let attempt_id = uuid::Uuid::new_v4().to_string();
        tx.execute(
            "INSERT INTO playback_attempts(attempt_id,session_id,occurrence_id,server_id,track_id,legacy) VALUES(?1,?2,?3,?4,?5,0)",
            params![attempt_id, session.session_id, occurrence.occurrence_id, occurrence.source.server_id, occurrence.source.track_id],
        )?;
        update_session(&tx, session)?;
        tx.execute(
            "UPDATE playback_sessions SET active_attempt_id=?1 WHERE singleton_id=1 AND session_id=?2",
            params![attempt_id, session.session_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn active_playback_attempt(&self) -> Result<Option<String>> {
        self.conn
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .query_row(
                "SELECT active_attempt_id FROM playback_sessions WHERE singleton_id=1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map(|value| value.flatten())
            .map_err(Into::into)
    }

    pub fn playback_attempts(
        &self,
        session_id: &str,
        after_attempt_seq: Option<u64>,
        limit: usize,
    ) -> Result<Vec<PlaybackAttempt>> {
        if !(1..=200).contains(&limit) {
            return Err(anyhow!("invalid playback attempt page size"));
        }
        let after = i64::try_from(after_attempt_seq.unwrap_or(0))?;
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut statement = conn.prepare("SELECT attempt_seq,attempt_id,session_id,occurrence_id,server_id,track_id,disposition,failure_code,terminal_position_ms,legacy FROM playback_attempts WHERE session_id=?1 AND attempt_seq>?2 ORDER BY attempt_seq LIMIT ?3")?;
        statement
            .query_map(params![session_id, after, limit as i64], |row| {
                Ok(PlaybackAttempt {
                    attempt_seq: u64::try_from(row.get::<_, i64>(0)?)
                        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, -1))?,
                    attempt_id: row.get(1)?,
                    session_id: row.get(2)?,
                    occurrence_id: row.get(3)?,
                    source: TrackSource {
                        server_id: row.get(4)?,
                        track_id: row.get(5)?,
                    },
                    disposition: row.get(6)?,
                    failure_code: row.get(7)?,
                    terminal_position_ms: row
                        .get::<_, Option<i64>>(8)?
                        .map(u64::try_from)
                        .transpose()
                        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(8, -1))?,
                    legacy: row.get(9)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
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
            "DELETE FROM playback_radio_scan WHERE session_id=?1",
            [&session.session_id],
        )?;
        tx.execute(
            "DELETE FROM playback_radio_membership WHERE session_id=?1",
            [&session.session_id],
        )?;
        tx.execute(
            "DELETE FROM playback_radio_recording_membership WHERE session_id=?1",
            [&session.session_id],
        )?;
        tx.execute(
            "DELETE FROM playback_radio_recording WHERE session_id=?1",
            [&session.session_id],
        )?;
        tx.execute(
            "DELETE FROM playback_radio_origin WHERE session_id=?1",
            [&session.session_id],
        )?;
        tx.execute(
            "DELETE FROM playback_occurrences WHERE session_id=?1",
            [&session.session_id],
        )?;
        close_active_attempt(&tx, &session.session_id, "interrupted")?;
        update_session(&tx, session)?;
        tx.execute(
            "UPDATE playback_sessions SET active_attempt_id=NULL WHERE singleton_id=1 AND session_id=?1",
            [&session.session_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn checkpoint_playback_position(
        &self,
        session_id: &str,
        sequence: u64,
        position_ms: u64,
    ) -> Result<()> {
        let mut conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let tx = conn.transaction()?;
        let changed=tx.execute("UPDATE playback_sessions SET checkpoint_sequence=?1,position_ms=?2 WHERE singleton_id=1 AND session_id=?3 AND checkpoint_sequence<=?1",params![sequence as i64,position_ms as i64,session_id])?;
        if changed != 1 {
            return Err(anyhow!("PERSISTENCE_FAILED"));
        }
        tx.execute("UPDATE playback_book_continuity SET whole_ms=CASE WHEN part_offset_ms+?1<=duration_ms THEN part_offset_ms+?1 ELSE whole_ms END, mapping_valid=CASE WHEN part_offset_ms+?1<=duration_ms THEN mapping_valid ELSE 0 END WHERE session_id=?2 AND occurrence_id=(SELECT current_occurrence_id FROM playback_sessions WHERE singleton_id=1) AND stop_flush=0", params![position_ms as i64, session_id])?;
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn save_book_continuity(&self, record: &BookOccurrenceRecord) -> Result<()> {
        self.save_book_continuity_with_timing(record, None)
    }

    pub(crate) fn save_book_continuity_with_timing(
        &self,
        record: &BookOccurrenceRecord,
        timing: Option<&BookTiming>,
    ) -> Result<()> {
        if record.duration_ms == 0
            || record.whole_ms > record.duration_ms
            || record.part_offset_ms > record.whole_ms
            || [record.duration_ms, record.whole_ms, record.part_offset_ms]
                .iter()
                .any(|v| *v > i64::MAX as u64)
            || record.audio_file_id.is_empty()
        {
            return Err(anyhow!("invalid book continuity"));
        }
        let mut conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let tx = conn.transaction()?;
        let changed = tx.execute("INSERT INTO playback_book_continuity(session_id,occurrence_id,server_id,track_id,library_id,item_id,media_id,audio_file_id,part_offset_ms,duration_ms,whole_ms,mapping_valid) SELECT ?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12 WHERE EXISTS(SELECT 1 FROM playback_sessions s JOIN playback_occurrences o ON o.session_id=s.session_id AND o.occurrence_id=s.current_occurrence_id WHERE s.singleton_id=1 AND s.session_id=?1 AND o.occurrence_id=?2 AND o.server_id=?3 AND o.track_id=?4) ON CONFLICT(occurrence_id) DO NOTHING", params![record.session_id, record.occurrence_id, record.server_id, record.track_id, record.identity.library_id, record.identity.library_item_id, record.identity.media_id, record.audio_file_id, record.part_offset_ms as i64, record.duration_ms as i64, record.whole_ms as i64, i64::from(record.mapping_valid)])?;
        if changed != 1 {
            return Err(anyhow!("stale book occurrence"));
        }
        if let Some(timing) = timing {
            let map = super::book_progress::BookMap::new(
                timing
                    .parts
                    .iter()
                    .map(|part| {
                        super::book_progress::BookPart::new(
                            &part.track_id,
                            &part.audio_file_id,
                            part.duration_ms,
                        )
                    })
                    .collect(),
            )
            .ok_or_else(|| anyhow!("invalid book mapping"))?;
            let (part, offset) = map
                .part(&record.track_id)
                .ok_or_else(|| anyhow!("invalid book part"))?;
            if timing.identity != record.identity
                || part.file_id != record.audio_file_id
                || offset != record.part_offset_ms
                || map.duration_ms() != record.duration_ms
            {
                return Err(anyhow!("invalid book mapping"));
            }
            let timing_json = serde_json::to_string(timing)?;
            tx.execute("INSERT INTO playback_book_mapping(session_id,queue_revision,timing_json) SELECT session_id,queue_revision,?2 FROM playback_sessions WHERE singleton_id=1 AND session_id=?1 ON CONFLICT(session_id) DO NOTHING", params![record.session_id, timing_json])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn load_book_continuity(
        &self,
        session_id: &str,
        occurrence_id: &str,
    ) -> Result<Option<BookOccurrenceRecord>> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.query_row("SELECT c.session_id,c.occurrence_id,c.server_id,c.track_id,c.library_id,c.item_id,c.media_id,c.audio_file_id,c.part_offset_ms,c.duration_ms,c.whole_ms,c.mapping_valid FROM playback_book_continuity c JOIN playback_sessions s ON s.session_id=c.session_id AND s.current_occurrence_id=c.occurrence_id WHERE s.singleton_id=1 AND c.session_id=?1 AND c.occurrence_id=?2", params![session_id, occurrence_id], |r| Ok(BookOccurrenceRecord {
            session_id: r.get(0)?, occurrence_id: r.get(1)?, server_id: r.get(2)?, track_id: r.get(3)?,
            identity: ProviderIdentity { library_id: r.get(4)?, library_item_id: r.get(5)?, media_id: r.get(6)? },
            audio_file_id: r.get(7)?, part_offset_ms: r.get::<_, i64>(8)? as u64, duration_ms: r.get::<_, i64>(9)? as u64,
            whole_ms: r.get::<_, i64>(10)? as u64, mapping_valid: r.get::<_, i64>(11)? == 1,
        })).optional().map_err(Into::into)
    }

    pub(crate) fn book_attempt_outcome(
        &self,
        session_id: &str,
        occurrence_id: &str,
    ) -> Result<Option<(String, u64)>> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.query_row(
            "SELECT disposition,terminal_position_ms FROM playback_attempts WHERE session_id=?1 AND occurrence_id=?2 AND disposition IS NOT NULL AND terminal_position_ms IS NOT NULL ORDER BY attempt_seq DESC LIMIT 1",
            params![session_id, occurrence_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as u64)),
        ).optional().map_err(Into::into)
    }

    pub(crate) fn mark_book_stop_flush(
        &self,
        session_id: &str,
        occurrence_id: &str,
        position_ms: u64,
    ) -> Result<()> {
        let position = i64::try_from(position_ms)?;
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "UPDATE playback_book_continuity SET whole_ms=part_offset_ms+?3,stop_flush=1 WHERE session_id=?1 AND occurrence_id=?2 AND mapping_valid=1 AND part_offset_ms+?3<=duration_ms",
            params![session_id, occurrence_id, position],
        )?;
        Ok(())
    }

    pub(crate) fn book_report_state(&self, record: &BookOccurrenceRecord) -> Result<(bool, bool)> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.query_row(
            "SELECT stop_flush,report_failed FROM playback_book_continuity WHERE session_id=?1 AND occurrence_id=?2",
            params![record.session_id, record.occurrence_id],
            |row| Ok((row.get::<_, i64>(0)? == 1, row.get::<_, i64>(1)? == 1)),
        ).map_err(Into::into)
    }

    pub(crate) fn clear_book_stop_flush(
        &self,
        session_id: &str,
        occurrence_id: &str,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "UPDATE playback_book_continuity SET stop_flush=0 WHERE session_id=?1 AND occurrence_id=?2",
            params![session_id, occurrence_id],
        )?;
        Ok(())
    }

    pub(crate) fn set_book_report_failed(
        &self,
        record: &BookOccurrenceRecord,
        failed: bool,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "UPDATE playback_book_continuity SET report_failed=?3 WHERE session_id=?1 AND occurrence_id=?2 AND mapping_valid=1",
            params![record.session_id, record.occurrence_id, i64::from(failed)],
        )?;
        Ok(())
    }

    pub(crate) fn invalidate_book_continuity(&self, record: &BookOccurrenceRecord) -> Result<()> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute("UPDATE playback_book_continuity SET mapping_valid=0 WHERE session_id=?1 AND occurrence_id=?2 AND server_id=?3 AND track_id=?4 AND library_id=?5 AND item_id=?6 AND media_id=?7 AND audio_file_id=?8", params![record.session_id,record.occurrence_id,record.server_id,record.track_id,record.identity.library_id,record.identity.library_item_id,record.identity.media_id,record.audio_file_id])?;
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

fn migrate_playback_attempts(tx: &rusqlite::Transaction<'_>) -> Result<()> {
    // Versions before 6 have no attempt-ledger authority. Rebuild from their
    // occurrence evidence so a rolled-back/fixture downgrade cannot retain a
    // stale active reference created by newer code.
    tx.execute("DELETE FROM playback_attempts", [])?;
    tx.execute(
        "UPDATE playback_sessions SET active_attempt_id=NULL WHERE singleton_id=1",
        [],
    )?;
    let session: Option<(String, Option<String>)> = tx
        .query_row(
            "SELECT session_id,current_occurrence_id FROM playback_sessions WHERE singleton_id=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((session_id, current_id)) = session else {
        return Ok(());
    };
    let rows = {
        let mut statement = tx.prepare(
            "SELECT occurrence_id,server_id,track_id,outcome,failure_code FROM playback_occurrences WHERE session_id=?1 ORDER BY ordinal",
        )?;
        statement
            .query_map([&session_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    let mut active_attempt_id = None;
    for (occurrence_id, server_id, track_id, disposition, failure_code) in rows {
        if disposition.is_none() && current_id.as_deref() != Some(&occurrence_id) {
            continue;
        }
        let attempt_id = uuid::Uuid::new_v4().to_string();
        let legacy = disposition.is_some();
        tx.execute(
            "INSERT INTO playback_attempts(attempt_id,session_id,occurrence_id,server_id,track_id,disposition,failure_code,legacy) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![attempt_id, session_id, occurrence_id, server_id, track_id, disposition, failure_code, legacy],
        )?;
        if !legacy {
            active_attempt_id = Some(attempt_id);
        }
    }
    tx.execute(
        "UPDATE playback_sessions SET active_attempt_id=?1 WHERE singleton_id=1",
        [active_attempt_id],
    )?;
    Ok(())
}

fn close_active_attempt(
    tx: &rusqlite::Transaction<'_>,
    session_id: &str,
    disposition: &str,
) -> Result<()> {
    let active: Option<(String, i64)> = tx
        .query_row(
            "SELECT active_attempt_id,position_ms FROM playback_sessions WHERE singleton_id=1 AND session_id=?1 AND active_attempt_id IS NOT NULL",
            [session_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    if let Some((attempt_id, position_ms)) = active {
        let changed = tx.execute(
            "UPDATE playback_attempts SET disposition=?1,terminal_position_ms=?2 WHERE attempt_id=?3 AND disposition IS NULL",
            params![disposition, position_ms, attempt_id],
        )?;
        if changed != 1 {
            return Err(anyhow!("active playback attempt changed"));
        }
        tx.execute(
            "UPDATE playback_sessions SET active_attempt_id=NULL WHERE singleton_id=1 AND session_id=?1",
            [session_id],
        )?;
    }
    Ok(())
}

fn ensure_active_attempt(
    tx: &rusqlite::Transaction<'_>,
    session: &PersistedSession,
    close_disposition: &str,
) -> Result<()> {
    let active: Option<(String, String)> = tx
        .query_row(
            "SELECT a.attempt_id,a.occurrence_id FROM playback_sessions s JOIN playback_attempts a ON a.attempt_id=s.active_attempt_id WHERE s.singleton_id=1 AND s.session_id=?1 AND a.disposition IS NULL",
            [&session.session_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    if (session.current_occurrence_id.is_none() && active.is_none())
        || active
            .as_ref()
            .map(|(_, occurrence_id)| occurrence_id.as_str())
            == session.current_occurrence_id.as_deref()
    {
        return Ok(());
    }
    close_active_attempt(tx, &session.session_id, close_disposition)?;
    let Some(current_id) = session.current_occurrence_id.as_deref() else {
        tx.execute(
            "UPDATE playback_sessions SET active_attempt_id=NULL WHERE singleton_id=1 AND session_id=?1",
            [&session.session_id],
        )?;
        return Ok(());
    };
    let (server_id, track_id): (String, String) = tx.query_row(
        "SELECT server_id,track_id FROM playback_occurrences WHERE session_id=?1 AND occurrence_id=?2",
        params![session.session_id, current_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let attempt_id = uuid::Uuid::new_v4().to_string();
    tx.execute(
        "INSERT INTO playback_attempts(attempt_id,session_id,occurrence_id,server_id,track_id,legacy) VALUES(?1,?2,?3,?4,?5,0)",
        params![attempt_id, session.session_id, current_id, server_id, track_id],
    )?;
    tx.execute(
        "UPDATE playback_sessions SET active_attempt_id=?1 WHERE singleton_id=1 AND session_id=?2",
        params![attempt_id, session.session_id],
    )?;
    Ok(())
}

fn open_current_attempt_after_departure(
    tx: &rusqlite::Transaction<'_>,
    session: &PersistedSession,
    departed_occurrence_id: &str,
) -> Result<()> {
    let Some(current_id) = session
        .current_occurrence_id
        .as_deref()
        .filter(|current_id| *current_id != departed_occurrence_id)
    else {
        return Ok(());
    };
    let (server_id, track_id): (String, String) = tx.query_row(
        "SELECT server_id,track_id FROM playback_occurrences WHERE session_id=?1 AND occurrence_id=?2",
        params![session.session_id, current_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let attempt_id = uuid::Uuid::new_v4().to_string();
    tx.execute(
        "INSERT INTO playback_attempts(attempt_id,session_id,occurrence_id,server_id,track_id,legacy) VALUES(?1,?2,?3,?4,?5,0)",
        params![attempt_id, session.session_id, current_id, server_id, track_id],
    )?;
    tx.execute(
        "UPDATE playback_sessions SET active_attempt_id=?1 WHERE singleton_id=1 AND session_id=?2",
        params![attempt_id, session.session_id],
    )?;
    Ok(())
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
    store_radio_state(tx, session)?;
    Ok(())
}

fn replace_radio_state(tx: &rusqlite::Transaction<'_>, session: &PersistedSession) -> Result<()> {
    tx.execute("DELETE FROM playback_radio_scan", [])?;
    tx.execute("DELETE FROM playback_radio_membership", [])?;
    tx.execute("DELETE FROM playback_radio_recording_membership", [])?;
    tx.execute("DELETE FROM playback_radio_recording", [])?;
    tx.execute("DELETE FROM playback_radio_origin", [])?;
    tx.execute("DELETE FROM playback_radio", [])?;
    store_radio_state(tx, session)
}

fn insert_radio_recording(
    tx: &rusqlite::Transaction<'_>,
    session_id: &str,
    occurrence_id: &str,
    evidence: Option<&super::recording::RecordingEvidence>,
) -> Result<()> {
    if let Some(evidence) = evidence
        && let Some(key) = evidence.key()
    {
        tx.execute("INSERT INTO playback_radio_recording(occurrence_id,session_id,recording_key,resolver_version,provenance) VALUES(?1,?2,?3,?4,?5)", params![occurrence_id,session_id,key.as_str(),super::recording::RESOLVER_VERSION,evidence.provenance.as_str()])?;
    }
    Ok(())
}

fn store_radio_state(tx: &rusqlite::Transaction<'_>, session: &PersistedSession) -> Result<()> {
    use super::radio::RadioStatus;
    let Some(radio) = session.radio.as_ref() else {
        tx.execute(
            "DELETE FROM playback_radio WHERE session_id=?1",
            [&session.session_id],
        )?;
        return Ok(());
    };
    if session.queue_kind != super::model::QueueKind::Radio {
        return Err(anyhow!("INVALID_RADIO_STATE"));
    }
    radio
        .validate()
        .map_err(|_| anyhow!("INVALID_RADIO_STATE"))?;
    let status = match radio.status {
        RadioStatus::Filling => "filling",
        RadioStatus::Ready => "ready",
        RadioStatus::Waiting => "waiting",
        RadioStatus::Stopped => "stopped",
    };
    let settings_json = radio
        .original_settings
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    let transition_json = radio
        .transition
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    tx.execute(
        "INSERT INTO playback_radio(session_id,logical_id,center_server_id,center_artist_id,status,reason,settings_json,cycle,transition_json) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT(session_id) DO UPDATE SET logical_id=excluded.logical_id,center_server_id=excluded.center_server_id,center_artist_id=excluded.center_artist_id,status=excluded.status,reason=excluded.reason,settings_json=excluded.settings_json,cycle=excluded.cycle,transition_json=excluded.transition_json",
        params![session.session_id, radio.logical_id, radio.center.as_ref().map(|center| &center.server_id), radio.center.as_ref().map(|center| &center.artist_id), status, radio.reason, settings_json, i64::try_from(radio.cycle)?, transition_json],
    )?;
    Ok(())
}

fn validate_edit_baseline(
    tx: &rusqlite::Transaction<'_>,
    session: &PersistedSession,
) -> Result<()> {
    let prior_revision = session
        .queue_revision
        .checked_sub(1)
        .ok_or_else(|| anyhow!("queue revision underflow"))?;
    let (stored_revision, stored_current): (i64, Option<String>) = tx.query_row(
        "SELECT queue_revision,current_occurrence_id FROM playback_sessions WHERE singleton_id=1 AND session_id=?1",
        [&session.session_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if stored_revision != i64::try_from(prior_revision)?
        || stored_current != session.current_occurrence_id
    {
        return Err(anyhow!("authoritative queue changed"));
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
        // Older SQLite rows constrain this column to album/manual; the versioned
        // playback_radio row is the durable authority for the Radio kind.
        super::model::QueueKind::Radio => "manual",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn radio_policy_is_source_bound_immutable_and_rejects_corruption() {
        use crate::domain::models::TrackLoudnessEvidence;
        let db = Database::memory().unwrap();
        db.init_playback().unwrap();
        let session_id = uuid::Uuid::new_v4().to_string();
        let first = occurrence(&session_id, 0, "same-track");
        let mut second = occurrence(&session_id, 1, "same-track");
        second.source.server_id = "other-server".into();
        {
            let conn = db.conn.lock().unwrap();
            conn.execute("INSERT INTO playback_sessions(singleton_id,session_id,queue_revision,checkpoint_sequence,transport_state,current_occurrence_id,position_ms) VALUES(1,?1,0,0,'paused',?2,0)",
                params![session_id, first.occurrence_id]).unwrap();
            conn.execute(
                "INSERT INTO playback_radio(session_id,logical_id,status) VALUES(?1,?2,'waiting')",
                params![session_id, uuid::Uuid::new_v4().to_string()],
            )
            .unwrap();
            for row in [&first, &second] {
                conn.execute("INSERT INTO playback_occurrences(session_id,occurrence_id,ordinal,server_id,track_id) VALUES(?1,?2,?3,?4,?5)",
                    params![session_id, row.occurrence_id, row.ordinal as i64, row.source.server_id, row.source.track_id]).unwrap();
            }
        }
        let mut song: crate::domain::models::Song = serde_json::from_value(serde_json::json!({
            "id":"same-track","title":"Track","duration":1,"suffix":"flac","contentType":"audio/flac"
        })).unwrap();
        song.track_loudness = TrackLoudnessEvidence::open_subsonic(-6.0, 0.8);
        let quiet = super::super::loudness::FrozenTrackPolicy::from_song(&song);
        song.track_loudness = TrackLoudnessEvidence::open_subsonic(6.0, 0.8);
        let loud = super::super::loudness::FrozenTrackPolicy::from_song(&song);
        assert_ne!(quiet.policy.scalar_bits, loud.policy.scalar_bits);
        db.freeze_radio_track_policy(&session_id, &first, &quiet, true)
            .unwrap();
        db.freeze_radio_track_policy(&session_id, &second, &loud, false)
            .unwrap();
        assert_eq!(
            db.radio_track_policy(&session_id, &first).unwrap(),
            Some(quiet.clone())
        );
        assert_eq!(
            db.radio_track_policy(&session_id, &second).unwrap(),
            Some(loud.clone())
        );
        assert_eq!(
            db.freeze_radio_track_policy(&session_id, &first, &loud, true)
                .unwrap(),
            quiet
        );
        let mut wrong_copy = first.clone();
        wrong_copy.source.server_id = "other-server".into();
        assert!(db.radio_track_policy(&session_id, &wrong_copy).is_err());
        db.conn.lock().unwrap().execute(
            r#"UPDATE playback_occurrences SET radio_policy_json='{"version":99}' WHERE occurrence_id=?1"#,
            [&second.occurrence_id],
        ).unwrap();
        assert!(db.radio_track_policy(&session_id, &second).is_err());
    }

    #[test]
    fn schema_ten_radio_occurrences_migrate_without_losing_source_rows() {
        let db = Database::memory().unwrap();
        db.init_playback().unwrap();
        let session_id = uuid::Uuid::new_v4().to_string();
        let occurrence_id = uuid::Uuid::new_v4().to_string();
        {
            let conn = db.conn.lock().unwrap();
            conn.execute("INSERT INTO playback_sessions(singleton_id,session_id,queue_revision,checkpoint_sequence,transport_state,current_occurrence_id,position_ms) VALUES(1,?1,7,3,'paused',?2,1234)",
                params![session_id, occurrence_id]).unwrap();
            conn.execute("INSERT INTO playback_occurrences(session_id,occurrence_id,ordinal,server_id,track_id) VALUES(?1,?2,0,'one','track')",
                params![session_id, occurrence_id]).unwrap();
            conn.execute(
                "INSERT INTO playback_radio(session_id,logical_id,status) VALUES(?1,?2,'waiting')",
                params![session_id, uuid::Uuid::new_v4().to_string()],
            )
            .unwrap();
            conn.execute_batch("ALTER TABLE playback_occurrences DROP COLUMN radio_policy_json; UPDATE playback_schema SET version=10;").unwrap();
        }
        assert!(db.init_playback_with_injected_failure().is_err());
        {
            let conn = db.conn.lock().unwrap();
            let version: i64 = conn
                .query_row("SELECT version FROM playback_schema", [], |row| row.get(0))
                .unwrap();
            assert_eq!(version, 10);
            let columns: Vec<String> = conn
                .prepare("PRAGMA table_info(playback_occurrences)")
                .unwrap()
                .query_map([], |row| row.get(1))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap();
            assert!(!columns.iter().any(|name| name == "radio_policy_json"));
        }
        db.init_playback().unwrap();
        let row: (i64, String, String, Option<String>) = db.conn.lock().unwrap()
            .query_row("SELECT ordinal,server_id,track_id,radio_policy_json FROM playback_occurrences WHERE occurrence_id=?1",
                [&occurrence_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))).unwrap();
        assert_eq!(row, (0, "one".into(), "track".into(), None));
        let state = db.load_playback_session().unwrap().unwrap();
        assert_eq!(state.queue_revision, 7);
        assert_eq!(state.position_ms, 1234);
        assert_eq!(state.queue_kind, super::super::model::QueueKind::Radio);
    }

    #[test]
    fn v9_radio_membership_migrates_without_guessing_recording_identity() {
        let db = Database::memory().unwrap();
        db.init_playback().unwrap();
        {
            let conn = db.conn.lock().unwrap();
            conn.execute("INSERT INTO playback_radio_membership(session_id,server_id,track_id,kind) VALUES('old','one','track','excluded')", []).unwrap();
            conn.execute_batch("DROP TABLE playback_radio_recording; DROP TABLE playback_radio_recording_membership; UPDATE playback_schema SET version=9;").unwrap();
        }
        db.init_playback().unwrap();
        assert!(
            db.radio_has_membership(
                "old",
                &TrackSource {
                    server_id: "one".into(),
                    track_id: "track".into()
                }
            )
            .unwrap()
        );
        assert!(
            !db.radio_recording_used("old", "mbrec:1:189002e7-3285-4e2e-92a3-7f6c30d407a2:plain")
                .unwrap()
        );
        let version: i64 = db
            .conn
            .lock()
            .unwrap()
            .query_row("SELECT version FROM playback_schema", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 11);
    }

    #[test]
    fn unknown_recording_resolver_version_is_recoverable_error() {
        let db = Database::memory().unwrap();
        db.init_playback().unwrap();
        db.conn.lock().unwrap().execute("INSERT INTO playback_radio_recording(occurrence_id,session_id,recording_key,resolver_version,provenance) VALUES('old','old','mbrec:2:uuid:plain',2,'future')", []).unwrap();
        assert!(
            db.init_playback()
                .unwrap_err()
                .to_string()
                .contains("UNSUPPORTED_RECORDING_RESOLVER_VERSION")
        );
    }
    use uuid::Uuid;

    #[test]
    fn radio_membership_uses_indexed_source_qualified_lookup_with_long_history() {
        let db = Database::memory().unwrap();
        db.init_playback().unwrap();
        let session_id = Uuid::new_v4().to_string();
        {
            let mut conn = db.conn.lock().unwrap();
            let tx = conn.transaction().unwrap();
            for index in 0..10_000 {
                tx.execute("INSERT INTO playback_radio_membership(session_id,server_id,track_id,kind) VALUES(?1,'one',?2,'heard')",params![session_id,format!("track-{index}")]).unwrap();
            }
            tx.commit().unwrap();
            let detail: String = conn.query_row(
                "EXPLAIN QUERY PLAN SELECT 1 FROM playback_radio_membership WHERE session_id=?1 AND server_id='one' AND track_id='track-9999'",
                [&session_id], |row|row.get(3)).unwrap();
            assert!(detail.contains("INDEX"), "{detail}");
        }
        assert!(
            db.radio_has_membership(
                &session_id,
                &TrackSource {
                    server_id: "one".into(),
                    track_id: "track-9999".into()
                }
            )
            .unwrap()
        );
        assert!(
            !db.radio_has_membership(
                &session_id,
                &TrackSource {
                    server_id: "two".into(),
                    track_id: "track-9999".into()
                }
            )
            .unwrap()
        );
    }

    #[test]
    fn book_continuity_is_checkpointed_and_invalidated_with_occurrence_transition() {
        let db = Database::memory().unwrap();
        db.init_playback().unwrap();
        let session = Uuid::new_v4().to_string();
        let occurrence = Uuid::new_v4().to_string();
        {
            let conn = db.conn.lock().unwrap();
            conn.execute("INSERT INTO playback_sessions(singleton_id,session_id,queue_revision,checkpoint_sequence,transport_state,current_occurrence_id,position_ms) VALUES(1,?1,0,0,'playing',?2,0)", params![session,occurrence]).unwrap();
            conn.execute("INSERT INTO playback_occurrences(session_id,occurrence_id,ordinal,server_id,track_id) VALUES(?1,?2,0,'server','track')", params![session,occurrence]).unwrap();
        }
        let record = crate::playback::book_progress::BookOccurrenceRecord {
            session_id: session.clone(),
            occurrence_id: occurrence.clone(),
            server_id: "server".into(),
            track_id: "track".into(),
            identity: crate::domain::models::ProviderIdentity {
                library_id: "library".into(),
                library_item_id: "item".into(),
                media_id: "media".into(),
            },
            audio_file_id: "file".into(),
            part_offset_ms: 10_000,
            duration_ms: 50_000,
            whole_ms: 10_000,
            mapping_valid: true,
        };
        db.save_book_continuity(&record).unwrap();
        db.checkpoint_playback_position(&session, 1, 2_000).unwrap();
        assert_eq!(
            db.load_book_continuity(&session, &occurrence)
                .unwrap()
                .unwrap()
                .whole_ms,
            12_000
        );
        db.mark_book_stop_flush(&session, &occurrence, 2_500)
            .unwrap();
        assert!(db.book_report_state(&record).unwrap().0);
        db.checkpoint_playback_position(&session, 2, 0).unwrap();
        assert_eq!(
            db.load_book_continuity(&session, &occurrence)
                .unwrap()
                .unwrap()
                .whole_ms,
            12_500
        );
        db.set_book_report_failed(&record, true).unwrap();
        assert!(db.book_report_state(&record).unwrap().1);
        db.clear_book_stop_flush(&session, &occurrence).unwrap();
        assert!(!db.book_report_state(&record).unwrap().0);
        db.conn
            .lock()
            .unwrap()
            .execute(
                "UPDATE playback_sessions SET current_occurrence_id=NULL WHERE singleton_id=1",
                [],
            )
            .unwrap();
        assert!(
            db.load_book_continuity(&session, &occurrence)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn v5_migration_preserves_legacy_outcomes_and_opens_only_the_current_attempt() {
        let db = Database::memory().unwrap();
        let session_id = Uuid::new_v4().to_string();
        let completed_id = Uuid::new_v4().to_string();
        let current_id = Uuid::new_v4().to_string();
        db.conn.lock().unwrap().execute_batch(&format!(
            "CREATE TABLE playback_schema (singleton_id INTEGER PRIMARY KEY, version INTEGER NOT NULL);\n\
             INSERT INTO playback_schema VALUES(1,5);\n\
             CREATE TABLE playback_sessions (singleton_id INTEGER PRIMARY KEY, session_id TEXT NOT NULL, queue_revision INTEGER NOT NULL, checkpoint_sequence INTEGER NOT NULL, transport_state TEXT NOT NULL, current_occurrence_id TEXT, position_ms INTEGER NOT NULL, album_context_json TEXT, queue_kind TEXT NOT NULL DEFAULT 'manual', current_gain_bits INTEGER NOT NULL DEFAULT 1065353216, current_qualified_suffix TEXT);\n\
             CREATE TABLE playback_occurrences (session_id TEXT NOT NULL, occurrence_id TEXT NOT NULL UNIQUE, ordinal INTEGER NOT NULL, server_id TEXT NOT NULL, track_id TEXT NOT NULL, outcome TEXT, failure_code TEXT, PRIMARY KEY(session_id, ordinal));\n\
             INSERT INTO playback_sessions VALUES(1,'{session_id}',0,0,'paused','{current_id}',1234,NULL,'manual',1065353216,NULL);\n\
             INSERT INTO playback_occurrences VALUES('{session_id}','{completed_id}',0,'server','first','naturalCompletion',NULL);\n\
             INSERT INTO playback_occurrences VALUES('{session_id}','{current_id}',1,'server','second',NULL,NULL);"
        )).unwrap();

        db.init_playback().unwrap();

        assert_eq!(PERSISTENCE_VERSION, 11);
        let attempts = db.playback_attempts(&session_id, None, 20).unwrap();
        assert_eq!(attempts.len(), 2);
        assert_eq!(attempts[0].occurrence_id, completed_id);
        assert_eq!(
            attempts[0].disposition.as_deref(),
            Some("naturalCompletion")
        );
        assert!(attempts[0].legacy);
        assert_eq!(attempts[1].occurrence_id, current_id);
        assert_eq!(attempts[1].disposition, None);
        assert!(!attempts[1].legacy);
        assert_eq!(
            db.active_playback_attempt().unwrap().as_deref(),
            Some(attempts[1].attempt_id.as_str())
        );
    }

    #[test]
    fn replay_attempts_preserve_legacy_outcome_and_terminal_evidence() {
        let db = Database::memory().unwrap();
        db.init_playback().unwrap();
        let current = occurrence("ignored", 0, "track");
        let mut committed = session(1, Some(current.occurrence_id.clone()), 4_200);
        db.persist_playback_structure(&committed, std::slice::from_ref(&current))
            .unwrap();
        committed.position_ms = 8_500;
        db.persist_playback_terminal(
            &committed,
            &current.occurrence_id,
            "technicalFailure",
            Some("DECODE_FAILED"),
            committed.position_ms,
        )
        .unwrap();
        let first = db
            .playback_attempts(&committed.session_id, None, 20)
            .unwrap();
        let failed_id = first.last().unwrap().attempt_id.clone();

        committed.position_ms = 0;
        db.start_playback_attempt(&committed, &current, None, None)
            .unwrap();

        let attempts = db
            .playback_attempts(&committed.session_id, None, 20)
            .unwrap();
        assert_eq!(attempts.len(), 2);
        assert_eq!(attempts[0].attempt_id, failed_id);
        assert_eq!(attempts[0].terminal_position_ms, Some(8_500));
        assert_eq!(attempts[0].disposition.as_deref(), Some("technicalFailure"));
        assert_eq!(attempts[0].failure_code.as_deref(), Some("DECODE_FAILED"));
        assert_eq!(attempts[1].disposition, None);
        assert!(attempts[1].attempt_seq > attempts[0].attempt_seq);
        let second_page = db
            .playback_attempts(&committed.session_id, Some(attempts[0].attempt_seq), 1)
            .unwrap();
        assert_eq!(second_page, vec![attempts[1].clone()]);
        assert_eq!(
            db.playback_outcome(&current.occurrence_id)
                .unwrap()
                .as_deref(),
            Some("technicalFailure")
        );
    }

    #[test]
    fn back_attempt_transition_is_atomic_and_retains_removed_row_evidence() {
        let db = Database::memory().unwrap();
        db.init_playback().unwrap();
        let first = occurrence("ignored", 0, "first");
        let second = occurrence("ignored", 1, "second");
        let mut committed = session(1, Some(second.occurrence_id.clone()), 2_000);
        db.persist_playback_structure(&committed, &[first.clone(), second.clone()])
            .unwrap();
        committed.current_occurrence_id = Some(first.occurrence_id.clone());
        committed.position_ms = 0;
        db.start_playback_attempt(&committed, &first, Some("backNavigation"), Some(2_999))
            .unwrap();
        assert_eq!(
            db.load_playback_session()
                .unwrap()
                .unwrap()
                .current_occurrence_id,
            Some(first.occurrence_id.clone())
        );

        let mut cleared = committed.clone();
        cleared.current_occurrence_id = None;
        cleared.position_ms = 0;
        cleared.state = TransportState::Idle;
        db.clear_playback_session(&cleared).unwrap();
        let attempts = db
            .playback_attempts(&committed.session_id, None, 20)
            .unwrap();
        assert_eq!(attempts.len(), 2);
        assert_eq!(attempts[0].disposition.as_deref(), Some("backNavigation"));
        assert_eq!(attempts[0].terminal_position_ms, Some(2_999));
        assert_eq!(attempts[0].source.track_id, "second");
        assert_eq!(attempts[1].source.track_id, "first");
    }

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
            radio: None,
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
    fn v4_album_migration_freezes_current_policy_and_rolls_back_on_failure() {
        let db = Database::memory().unwrap();
        let first = occurrence("ignored", 0, "repeated");
        let current = occurrence("ignored", 1, "repeated");
        let context = super::super::model::FrozenAlbumContext {
            source: super::super::model::AlbumSource {
                server_id: "portable-server".into(),
                album_id: "album".into(),
            },
            member_count: 2,
            membership_digest: super::super::model::album_membership_digest([
                &first.source,
                &current.source,
            ]),
            representations: vec!["mp3".into(), "flac".into()],
            policy: super::super::loudness::AlbumLoudnessPolicy {
                version: super::super::loudness::ALBUM_LOUDNESS_POLICY_VERSION,
                scalar_bits: 0.75f32.to_bits(),
                gain_db_bits: Some(0.0f64.to_bits()),
                peak_bits: Some((super::super::loudness::SAMPLE_PEAK_CEILING / 0.75).to_bits()),
                reason: super::super::loudness::AlbumLoudnessReason::OpenSubsonicReplayGain,
            },
        };
        let committed = session(7, Some(current.occurrence_id.clone()), 4_200);
        {
            let conn = db.conn.lock().unwrap();
            conn.execute_batch(
                "CREATE TABLE playback_schema (singleton_id INTEGER PRIMARY KEY, version INTEGER NOT NULL);
                 INSERT INTO playback_schema VALUES(1,4);
                 CREATE TABLE playback_sessions (singleton_id INTEGER PRIMARY KEY, session_id TEXT NOT NULL, queue_revision INTEGER NOT NULL, checkpoint_sequence INTEGER NOT NULL, transport_state TEXT NOT NULL, current_occurrence_id TEXT, position_ms INTEGER NOT NULL, album_context_json TEXT);
                 CREATE TABLE playback_occurrences (session_id TEXT NOT NULL, occurrence_id TEXT NOT NULL UNIQUE, ordinal INTEGER NOT NULL, server_id TEXT NOT NULL, track_id TEXT NOT NULL, outcome TEXT, failure_code TEXT, PRIMARY KEY(session_id, ordinal));",
            ).unwrap();
            conn.execute(
                "INSERT INTO playback_sessions VALUES(1,?1,7,3,'playing',?2,4200,?3)",
                params![
                    committed.session_id,
                    current.occurrence_id,
                    serde_json::to_string(&context).unwrap()
                ],
            )
            .unwrap();
            for row in [&first, &current] {
                conn.execute(
                    "INSERT INTO playback_occurrences VALUES(?1,?2,?3,?4,?5,?6,NULL)",
                    params![
                        committed.session_id,
                        row.occurrence_id,
                        row.ordinal as i64,
                        row.source.server_id,
                        row.source.track_id,
                        if row.ordinal == 0 {
                            Some("naturalCompletion")
                        } else {
                            None
                        }
                    ],
                )
                .unwrap();
            }
        }
        assert!(db.init_playback_with_injected_failure().is_err());
        {
            let conn = db.conn.lock().unwrap();
            let version: i64 = conn
                .query_row("SELECT version FROM playback_schema", [], |row| row.get(0))
                .unwrap();
            assert_eq!(version, 4);
            let columns = conn
                .prepare("PRAGMA table_info(playback_sessions)")
                .unwrap()
                .query_map([], |row| row.get::<_, String>(1))
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap();
            assert!(
                !columns
                    .iter()
                    .any(|name| name == "queue_kind" || name == "current_gain_bits")
            );
        }
        let migrated = db.load_playback_session().unwrap().unwrap();
        db.validate_playback_session(&migrated).unwrap();
        assert_eq!(migrated.queue_kind, super::super::model::QueueKind::Album);
        assert_eq!(migrated.current_gain_bits, 0.75f32.to_bits());
        assert_eq!(migrated.current_qualified_suffix.as_deref(), Some("flac"));
        assert_eq!(migrated.album_context.as_ref(), Some(&context));
        assert_eq!(migrated.current_occurrence_id, Some(current.occurrence_id));
        assert_eq!(migrated.queue_revision, 7);
        assert_eq!(migrated.checkpoint_sequence, 3);
        assert_eq!(migrated.position_ms, 4_200);
        assert_eq!(
            db.playback_outcome(&first.occurrence_id)
                .unwrap()
                .as_deref(),
            Some("naturalCompletion")
        );
        let again = db.load_playback_session().unwrap().unwrap();
        assert_eq!(again.current_gain_bits, migrated.current_gain_bits);
        assert_eq!(
            again.current_qualified_suffix,
            migrated.current_qualified_suffix
        );
    }

    #[test]
    fn v4_manual_migration_uses_unity_for_existing_and_empty_current() {
        for has_current in [false, true] {
            let db = Database::memory().unwrap();
            let current = occurrence("ignored", 0, "manual-current");
            let current_id = has_current.then(|| current.occurrence_id.clone());
            let committed = session(6, current_id.clone(), if has_current { 1_234 } else { 0 });
            {
                let conn = db.conn.lock().unwrap();
                conn.execute_batch(
                    "CREATE TABLE playback_schema (singleton_id INTEGER PRIMARY KEY, version INTEGER NOT NULL);
                     INSERT INTO playback_schema VALUES(1,4);
                     CREATE TABLE playback_sessions (singleton_id INTEGER PRIMARY KEY, session_id TEXT NOT NULL, queue_revision INTEGER NOT NULL, checkpoint_sequence INTEGER NOT NULL, transport_state TEXT NOT NULL, current_occurrence_id TEXT, position_ms INTEGER NOT NULL, album_context_json TEXT);
                     CREATE TABLE playback_occurrences (session_id TEXT NOT NULL, occurrence_id TEXT NOT NULL UNIQUE, ordinal INTEGER NOT NULL, server_id TEXT NOT NULL, track_id TEXT NOT NULL, outcome TEXT, failure_code TEXT, PRIMARY KEY(session_id, ordinal));",
                ).unwrap();
                conn.execute(
                    "INSERT INTO playback_sessions VALUES(1,?1,6,2,?2,?3,?4,NULL)",
                    params![
                        committed.session_id,
                        state_name(committed.state),
                        current_id,
                        committed.position_ms as i64
                    ],
                )
                .unwrap();
                if has_current {
                    conn.execute(
                        "INSERT INTO playback_occurrences VALUES(?1,?2,0,?3,?4,NULL,NULL)",
                        params![
                            committed.session_id,
                            current.occurrence_id,
                            current.source.server_id,
                            current.source.track_id
                        ],
                    )
                    .unwrap();
                }
            }
            // Load twice to check that the migration is stable once the version is updated.
            for _ in 0..2 {
                let migrated = db.load_playback_session().unwrap().unwrap();
                db.validate_playback_session(&migrated).unwrap();
                assert_eq!(migrated.queue_kind, super::super::model::QueueKind::Manual);
                assert_eq!(migrated.current_gain_bits, 1.0f32.to_bits());
                assert_eq!(migrated.current_qualified_suffix, None);
                assert_eq!(migrated.album_context, None);
                assert_eq!(migrated.current_occurrence_id, current_id);
                assert_eq!(migrated.position_ms, committed.position_ms);
                assert_eq!(migrated.state, committed.state);
                assert_eq!(migrated.queue_revision, 6);
                assert_eq!(migrated.checkpoint_sequence, 2);
                assert_eq!(
                    db.playback_count(&committed.session_id).unwrap(),
                    u64::from(has_current)
                );
            }
            let version: i64 = db
                .conn
                .lock()
                .unwrap()
                .query_row("SELECT version FROM playback_schema", [], |row| row.get(0))
                .unwrap();
            assert_eq!(version, PERSISTENCE_VERSION);
        }
    }

    #[test]
    fn queue_edit_abort_restores_deleted_rows_ordinals_revision_and_current_policy() {
        let db = Database::memory().unwrap();
        db.init_playback().unwrap();
        let rows: Vec<_> = (0..5)
            .map(|ordinal| occurrence("ignored", ordinal, "repeated"))
            .collect();
        let mut committed = session(4, Some(rows[1].occurrence_id.clone()), 1_234);
        committed.current_gain_bits = 0.75f32.to_bits();
        committed.current_qualified_suffix = Some("flac".into());
        db.persist_playback_structure(&committed, &rows).unwrap();
        db.conn.lock().unwrap().execute(
            "UPDATE playback_occurrences SET outcome='naturalCompletion' WHERE occurrence_id=?1",
            [&rows[0].occurrence_id],
        ).unwrap();
        let mut edited = committed.clone();
        edited.queue_revision += 1;
        let ordered = [rows[4].clone(), rows[2].clone()];
        let removed = [rows[3].occurrence_id.clone()];
        // Fail after DELETE and all ordinal rewrites, at the final session update.
        db.conn
            .lock()
            .unwrap()
            .execute_batch(
                "CREATE TEMP TRIGGER abort_queue_edit BEFORE UPDATE ON playback_sessions
             BEGIN SELECT RAISE(ABORT, 'injected queue edit failure'); END;",
            )
            .unwrap();
        assert!(
            db.edit_playback_upcoming(&edited, &ordered, &removed)
                .is_err()
        );
        let unchanged = db.playback_page(&committed.session_id, None, 200).unwrap();
        assert_eq!(
            unchanged
                .iter()
                .map(|row| (&row.occurrence_id, row.ordinal, &row.source))
                .collect::<Vec<_>>(),
            rows.iter()
                .map(|row| (&row.occurrence_id, row.ordinal, &row.source))
                .collect::<Vec<_>>()
        );
        let restored = db.load_playback_session().unwrap().unwrap();
        assert_eq!(restored.queue_revision, committed.queue_revision);
        assert_eq!(
            restored.current_occurrence_id,
            committed.current_occurrence_id
        );
        assert_eq!(restored.position_ms, committed.position_ms);
        assert_eq!(restored.current_gain_bits, committed.current_gain_bits);
        assert_eq!(
            restored.current_qualified_suffix,
            committed.current_qualified_suffix
        );
        assert_eq!(
            db.playback_outcome(&rows[0].occurrence_id)
                .unwrap()
                .as_deref(),
            Some("naturalCompletion")
        );
        db.conn
            .lock()
            .unwrap()
            .execute_batch("DROP TRIGGER abort_queue_edit")
            .unwrap();
        db.edit_playback_upcoming(&edited, &ordered, &removed)
            .unwrap();
        let after = db.playback_page(&committed.session_id, None, 200).unwrap();
        assert_eq!(
            after
                .iter()
                .map(|row| &row.occurrence_id)
                .collect::<Vec<_>>(),
            [
                &rows[0].occurrence_id,
                &rows[1].occurrence_id,
                &rows[4].occurrence_id,
                &rows[2].occurrence_id
            ]
        );
        assert_eq!(
            after.iter().map(|row| row.ordinal).collect::<Vec<_>>(),
            [0, 1, 2, 3]
        );
        assert_eq!(
            db.load_playback_session().unwrap().unwrap().queue_revision,
            5
        );
    }

    #[test]
    fn empty_main_preview_append_abort_rolls_back_queue_and_saved_baseline_together() {
        let db = Database::memory().unwrap();
        db.init_playback().unwrap();
        let committed = session(0, None, 0);
        db.persist_playback_structure(&committed, &[]).unwrap();
        let audition = PersistedAudition {
            audition_id: Uuid::new_v4().to_string(),
            parent_session_id: committed.session_id.clone(),
            source: TrackSource {
                server_id: "portable-server".into(),
                track_id: "audition".into(),
            },
            position_ms: 2_345,
            state: TransportState::Playing,
            saved_main_occurrence_id: None,
            saved_main_position_ms: 0,
            saved_main_intent: TransportState::Idle,
            resume_inhibited: false,
            contiguous_heard_ms: 1_200,
            coverage_unknown: false,
            seek_discontinuous: true,
        };
        db.persist_audition_admission(&committed, &audition)
            .unwrap();
        let first = occurrence("ignored", 0, "main");
        let mut appended = committed.clone();
        appended.queue_revision = 1;
        appended.current_occurrence_id = Some(first.occurrence_id.clone());
        appended.state = TransportState::Paused;
        db.conn
            .lock()
            .unwrap()
            .execute_batch(
                "CREATE TEMP TRIGGER abort_preview_baseline BEFORE UPDATE ON playback_audition
             BEGIN SELECT RAISE(ABORT, 'injected preview baseline failure'); END;",
            )
            .unwrap();
        assert!(
            db.append_playback_occurrences_with_audition_baseline(
                &appended,
                std::slice::from_ref(&first)
            )
            .is_err()
        );
        let restored = db.load_playback_session().unwrap().unwrap();
        assert_eq!(restored.queue_revision, 0);
        assert_eq!(restored.current_occurrence_id, None);
        assert_eq!(restored.state, TransportState::Idle);
        assert_eq!(db.playback_count(&committed.session_id).unwrap(), 0);
        assert_eq!(db.load_playback_audition().unwrap(), Some(audition.clone()));
        db.conn
            .lock()
            .unwrap()
            .execute_batch("DROP TRIGGER abort_preview_baseline")
            .unwrap();
        db.append_playback_occurrences_with_audition_baseline(&appended, &[first])
            .unwrap();
        let expected = PersistedAudition {
            saved_main_occurrence_id: appended.current_occurrence_id.clone(),
            saved_main_intent: TransportState::Paused,
            ..audition
        };
        assert_eq!(db.load_playback_audition().unwrap(), Some(expected));
        assert_eq!(db.playback_count(&committed.session_id).unwrap(), 1);
        let restored = db.load_playback_session().unwrap().unwrap();
        assert_eq!(
            restored.current_occurrence_id,
            appended.current_occurrence_id
        );
        assert_eq!(restored.queue_revision, 1);
        assert_eq!(restored.state, TransportState::Paused);
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
