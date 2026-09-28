//! Policy for a single logical, same-artist Radio session.
//!
//! Radio never derives an artist from a display name. A source qualifies only
//! when the provider supplies a stable artist ID. This model has one artist ID
//! per song; a provider reporting no stable single ID leaves Radio waiting.

use serde::{Deserialize, Serialize};

use super::model::TrackSource;

pub const AUTO_UPCOMING_TARGET: usize = 5;
pub const AUTO_UPCOMING_TRIGGER: usize = 2;
pub const MAX_PREPARATION_FAILURES: usize = 5;
pub const RETRIEVAL_DEADLINE_SECS: u64 = 15;
pub const PREPARATION_DEADLINE_SECS: u64 = 15;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtistIdentity {
    pub server_id: String,
    pub artist_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RadioStatus {
    Filling,
    Ready,
    Waiting,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RadioState {
    pub logical_id: String,
    pub center: Option<ArtistIdentity>,
    pub status: RadioStatus,
    pub reason: Option<String>,
}

impl RadioState {
    pub fn validate(&self) -> Result<(), &'static str> {
        if uuid::Uuid::parse_str(&self.logical_id).is_err() {
            return Err("INVALID_RADIO_STATE");
        }
        if let Some(center) = &self.center {
            if center.server_id.is_empty()
                || center.artist_id.is_empty()
                || center.server_id.len() > super::model::MAX_ID_BYTES
                || center.artist_id.len() > super::model::MAX_ID_BYTES
            {
                return Err("INVALID_RADIO_STATE");
            }
        } else if matches!(self.status, RadioStatus::Filling | RadioStatus::Ready) {
            return Err("INVALID_RADIO_STATE");
        }
        if self.reason.as_deref().is_some_and(|reason| {
            !matches!(
                reason,
                "radio.exhausted"
                    | "radio.artistUnavailable"
                    | "radio.sourceFailure"
                    | "radio.settingsUnavailable"
                    | "radio.persistenceFailure"
            )
        }) {
            return Err("INVALID_RADIO_STATE");
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct RefillLease {
    pub session_id: String,
    pub generation_id: String,
    pub queue_revision: u64,
    pub refill_id: String,
    pub center: ArtistIdentity,
    pub capacity: usize,
}

impl ArtistIdentity {
    pub fn matches(&self, server_id: &str, artist_id: Option<&str>) -> bool {
        self.server_id == server_id && artist_id == Some(self.artist_id.as_str())
    }
}

pub fn center_for(source: &TrackSource, artist_id: Option<&str>) -> Option<ArtistIdentity> {
    let artist_id =
        artist_id.filter(|id| !id.is_empty() && id.len() <= super::model::MAX_ID_BYTES)?;
    Some(ArtistIdentity {
        server_id: source.server_id.clone(),
        artist_id: artist_id.to_owned(),
    })
}

pub fn refill_capacity(auto_upcoming: usize) -> Option<usize> {
    (auto_upcoming < AUTO_UPCOMING_TRIGGER)
        .then_some(AUTO_UPCOMING_TARGET.saturating_sub(auto_upcoming))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        db::Database,
        domain::models::Song,
        playback::{
            model::{
                ApplySessionParams, ControlAction, ControlParams, PlaybackEvent, PlaybackStatus,
                PreviewTrackParams, SessionOperation, TrackSource, TransportState,
            },
            selection::SelectionCandidate,
            session::PlaybackSession,
        },
    };
    use std::sync::Arc;
    use uuid::Uuid;

    fn source(server: &str, track: &str) -> TrackSource {
        TrackSource {
            server_id: server.into(),
            track_id: track.into(),
        }
    }

    fn candidate(server: &str, track: &str, artist: &str) -> SelectionCandidate {
        SelectionCandidate {
            source: source(server, track),
            song: Song {
                id: track.into(),
                title: track.into(),
                artist_id: Some(artist.into()),
                artist_name: None,
                album_id: None,
                album_title: None,
                duration_seconds: 180,
                bitrate_kbps: Some(256),
                track_number: None,
                disc_number: None,
                cover_art_id: None,
                date_added: None,
                last_played_at: None,
                play_count: None,
                is_favorite: None,
                content_type: None,
                suffix: None,
                size_bytes: Some(1000),
                album_loudness: Default::default(),
                provider_metadata: Default::default(),
            },
        }
    }

    fn apply(session: &PlaybackSession, operation: SessionOperation) {
        let snapshot = session.snapshot().unwrap();
        session
            .apply(ApplySessionParams {
                schema_version: 1,
                instance_id: snapshot.instance_id,
                session_id: snapshot.session_id,
                command_id: Uuid::new_v4().to_string(),
                expected_queue_revision: snapshot.queue_revision,
                operation,
            })
            .unwrap();
    }

    fn start(session: &PlaybackSession) {
        apply(
            session,
            SessionOperation::StartRadio {
                source: source("one", "first"),
                center: Some(ArtistIdentity {
                    server_id: "one".into(),
                    artist_id: "artist".into(),
                }),
            },
        );
    }

    #[test]
    fn center_needs_stable_source_qualified_artist() {
        let source = TrackSource {
            server_id: "one".into(),
            track_id: "track".into(),
        };
        assert_eq!(
            center_for(&source, Some("artist")),
            Some(ArtistIdentity {
                server_id: "one".into(),
                artist_id: "artist".into()
            })
        );
        assert_eq!(center_for(&source, None), None);
        assert_eq!(center_for(&source, Some("")), None);
    }

    #[test]
    fn threshold_and_capacity_are_independent_of_manual_occurrences() {
        assert_eq!(refill_capacity(0), Some(5));
        assert_eq!(refill_capacity(1), Some(4));
        assert_eq!(refill_capacity(2), None);
        assert_eq!(refill_capacity(5), None);
    }

    #[test]
    fn only_matching_server_and_artist_are_eligible() {
        let center = ArtistIdentity {
            server_id: "one".into(),
            artist_id: "same".into(),
        };
        assert!(center.matches("one", Some("same")));
        assert!(!center.matches("two", Some("same")));
        assert!(!center.matches("one", Some("other")));
        assert!(!center.matches("one", None));
    }

    #[test]
    fn owner_admits_one_bounded_batch_and_preserves_manual_edits() {
        let db = Arc::new(Database::memory().unwrap());
        let session = PlaybackSession::restore(db.clone(), "radio-test".into());
        start(&session);
        let initial = session.snapshot().unwrap();
        assert_eq!(initial.queue_kind, crate::playback::model::QueueKind::Radio);
        let lease = session.reserve_radio_refill().unwrap().unwrap();
        assert_eq!(lease.capacity, 5);
        let candidates = (0..5)
            .map(|n| candidate("one", &format!("auto-{n}"), "artist"))
            .collect();
        assert!(
            session
                .admit_radio_refill(lease.clone(), candidates, None)
                .unwrap()
        );
        assert!(!session.admit_radio_refill(lease, Vec::new(), None).unwrap());
        let filled = session.snapshot().unwrap();
        assert_eq!(
            filled.queue_revision.parse::<u64>().unwrap(),
            initial.queue_revision.parse::<u64>().unwrap() + 1
        );
        assert_eq!(filled.total_occurrence_count, 6);
        apply(
            &session,
            SessionOperation::AppendQueue {
                sources: vec![source("one", "manual")],
            },
        );
        let edited = session.snapshot().unwrap();
        assert_eq!(edited.queue_kind, crate::playback::model::QueueKind::Radio);
        assert_eq!(edited.total_occurrence_count, 7);
        let auto_id = db
            .playback_page(&edited.session_id, None, 100)
            .unwrap()
            .into_iter()
            .find(|occurrence| occurrence.source.track_id == "auto-0")
            .unwrap()
            .occurrence_id;
        apply(
            &session,
            SessionOperation::MoveUpcoming {
                occurrence_id: auto_id.clone(),
                before_occurrence_id: None,
            },
        );
        assert_eq!(
            db.radio_auto_upcoming_count(&edited.session_id, 0).unwrap(),
            5
        );
        apply(
            &session,
            SessionOperation::RemoveUpcoming {
                occurrence_ids: vec![auto_id],
            },
        );
        assert!(
            db.radio_has_membership(&edited.session_id, &source("one", "auto-0"))
                .unwrap()
        );
        assert_eq!(
            db.radio_auto_upcoming_count(&edited.session_id, 0).unwrap(),
            4
        );
        let more_auto_ids: Vec<_> = db
            .playback_page(&edited.session_id, None, 100)
            .unwrap()
            .into_iter()
            .filter(|o| matches!(o.source.track_id.as_str(), "auto-1" | "auto-2" | "auto-3"))
            .map(|o| o.occurrence_id)
            .collect();
        apply(
            &session,
            SessionOperation::RemoveUpcoming {
                occurrence_ids: more_auto_ids,
            },
        );
        assert_eq!(
            db.radio_auto_upcoming_count(&edited.session_id, 0).unwrap(),
            1
        );
        assert!(session.reserve_radio_refill().unwrap().is_some());
        let manual_id = db
            .playback_page(&edited.session_id, None, 100)
            .unwrap()
            .into_iter()
            .find(|o| o.source.track_id == "manual")
            .unwrap()
            .occurrence_id;
        apply(
            &session,
            SessionOperation::RemoveUpcoming {
                occurrence_ids: vec![manual_id],
            },
        );
        assert!(
            !db.radio_has_membership(&edited.session_id, &source("one", "manual"))
                .unwrap()
        );
    }

    #[test]
    fn stale_refill_cannot_overwrite_a_manual_edit() {
        let db = Arc::new(Database::memory().unwrap());
        let session = PlaybackSession::restore(db, "radio-test".into());
        start(&session);
        let lease = session.reserve_radio_refill().unwrap().unwrap();
        apply(
            &session,
            SessionOperation::AppendQueue {
                sources: vec![source("one", "manual")],
            },
        );
        let before = session.snapshot().unwrap();
        assert!(
            !session
                .admit_radio_refill(lease, vec![candidate("one", "stale", "artist")], None)
                .unwrap()
        );
        let after = session.snapshot().unwrap();
        assert_eq!(after.queue_revision, before.queue_revision);
        assert_eq!(after.total_occurrence_count, before.total_occurrence_count);
    }

    #[test]
    fn empty_bounded_window_advances_without_claiming_exhaustion() {
        let db = Arc::new(Database::memory().unwrap());
        let session = PlaybackSession::restore(db, "radio-test".into());
        start(&session);
        let lease = session.reserve_radio_refill().unwrap().unwrap();
        assert!(
            !session
                .admit_radio_refill_with_more(lease, Vec::new(), None, true)
                .unwrap()
        );
        let snapshot = session.snapshot().unwrap();
        assert_eq!(snapshot.radio.unwrap().status, RadioStatus::Ready);
        assert!(session.reserve_radio_refill().unwrap().is_some());
    }

    #[test]
    fn next_excludes_but_technical_failure_does_not() {
        let db = Arc::new(Database::memory().unwrap());
        let session = PlaybackSession::restore(db.clone(), "radio-test".into());
        start(&session);
        let lease = session.reserve_radio_refill().unwrap().unwrap();
        session
            .admit_radio_refill(lease, vec![candidate("one", "second", "artist")], None)
            .unwrap();
        let before = session.snapshot().unwrap();
        session
            .control_with_guard(
                ControlParams {
                    schema_version: 1,
                    instance_id: before.instance_id,
                    session_id: before.session_id.clone(),
                    command_id: Uuid::new_v4().to_string(),
                    expected_generation_id: before.generation_id,
                    occurrence_id: before.current.unwrap().occurrence_id,
                    action: ControlAction::Next,
                },
                None,
            )
            .unwrap();
        assert!(
            db.radio_has_membership(&before.session_id, &source("one", "first"))
                .unwrap()
        );
        assert!(
            !db.radio_has_membership(&before.session_id, &source("two", "first"))
                .unwrap()
        );
        let stored = db.load_playback_session().unwrap().unwrap();
        let current = stored.current_occurrence_id.clone().unwrap();
        db.persist_playback_terminal(
            &stored,
            &current,
            "technicalFailure",
            Some("SOURCE_FAILED"),
            0,
        )
        .unwrap();
        assert!(
            !db.radio_has_membership(&before.session_id, &source("one", "second"))
                .unwrap()
        );
    }

    #[test]
    fn explicit_replay_can_select_an_excluded_source() {
        let db = Arc::new(Database::memory().unwrap());
        let session = PlaybackSession::restore(db.clone(), "radio-test".into());
        start(&session);
        let lease = session.reserve_radio_refill().unwrap().unwrap();
        session
            .admit_radio_refill(lease, vec![candidate("one", "second", "artist")], None)
            .unwrap();
        let before = session.snapshot().unwrap();
        let first_id = before.current.as_ref().unwrap().occurrence_id.clone();
        session
            .control_with_guard(
                ControlParams {
                    schema_version: 1,
                    instance_id: before.instance_id,
                    session_id: before.session_id.clone(),
                    command_id: Uuid::new_v4().to_string(),
                    expected_generation_id: before.generation_id,
                    occurrence_id: first_id.clone(),
                    action: ControlAction::Next,
                },
                None,
            )
            .unwrap();
        assert!(
            db.radio_has_membership(&before.session_id, &source("one", "first"))
                .unwrap()
        );
        apply(
            &session,
            SessionOperation::SelectCurrent {
                occurrence_id: first_id.clone(),
            },
        );
        let replayed = session.snapshot().unwrap();
        assert_eq!(replayed.current.unwrap().occurrence_id, first_id);
        assert_eq!(
            replayed.queue_kind,
            crate::playback::model::QueueKind::Radio
        );
    }

    #[test]
    fn restart_restores_logical_id_and_exclusions_then_new_radio_resets_them() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("radio.db");
        let db = Arc::new(Database::new(path.clone()).unwrap());
        let session = PlaybackSession::restore(db.clone(), "radio-test".into());
        start(&session);
        let lease = session.reserve_radio_refill().unwrap().unwrap();
        let source_key = "artist-source";
        assert!(
            db.advance_radio_scan(
                &lease.session_id,
                lease.queue_revision,
                source_key,
                crate::playback::selection::RadioSourceCursor::default(),
                Some(crate::playback::selection::RadioSourceCursor {
                    index: 32,
                    intra: 7
                }),
            )
            .unwrap()
        );
        session
            .admit_radio_refill(lease, vec![candidate("one", "second", "artist")], None)
            .unwrap();
        let snapshot = session.snapshot().unwrap();
        let logical_id = snapshot.radio.unwrap().logical_id;
        let automatic = db
            .playback_successor(&snapshot.session_id, 0)
            .unwrap()
            .unwrap();
        apply(
            &session,
            SessionOperation::RemoveUpcoming {
                occurrence_ids: vec![automatic.occurrence_id],
            },
        );
        assert!(
            db.radio_has_membership(&snapshot.session_id, &source("one", "second"))
                .unwrap()
        );
        session.shutdown_checkpoint().unwrap();
        session.stop_and_join().unwrap();
        drop(session);
        drop(db);

        let reopened = Arc::new(Database::new(path).unwrap());
        let restored = PlaybackSession::restore(reopened.clone(), "radio-restart".into());
        let after = restored.snapshot().unwrap();
        assert_eq!(after.state, TransportState::Paused);
        assert_eq!(after.radio.as_ref().unwrap().logical_id, logical_id);
        assert_eq!(
            reopened
                .radio_scan_cursor(&after.session_id, source_key)
                .unwrap(),
            Some(crate::playback::selection::RadioSourceCursor {
                index: 32,
                intra: 7
            })
        );
        assert!(
            reopened
                .radio_has_membership(&after.session_id, &source("one", "second"))
                .unwrap()
        );
        start(&restored);
        let fresh = restored.snapshot().unwrap();
        assert_ne!(fresh.radio.unwrap().logical_id, logical_id);
        assert_eq!(
            reopened
                .radio_scan_cursor(&fresh.session_id, source_key)
                .unwrap(),
            Some(crate::playback::selection::RadioSourceCursor::default())
        );
        assert!(
            !reopened
                .radio_has_membership(&fresh.session_id, &source("one", "second"))
                .unwrap()
        );
    }

    #[test]
    fn artist_without_stable_id_waits_without_refill() {
        let db = Arc::new(Database::memory().unwrap());
        let session = PlaybackSession::restore(db, "radio-test".into());
        apply(
            &session,
            SessionOperation::StartRadio {
                source: source("one", "first"),
                center: None,
            },
        );
        let snapshot = session.snapshot().unwrap();
        assert_eq!(
            snapshot.radio.unwrap().reason.as_deref(),
            Some("radio.artistUnavailable")
        );
        assert!(session.reserve_radio_refill().unwrap().is_none());
    }

    #[test]
    fn missing_membership_table_fails_restore_without_resetting_radio() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("corrupt-radio.db");
        let db = Arc::new(Database::new(path.clone()).unwrap());
        let session = PlaybackSession::restore(db.clone(), "radio-test".into());
        start(&session);
        session.shutdown_checkpoint().unwrap();
        session.stop_and_join().unwrap();
        drop(session);
        drop(db);
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .execute("DROP TABLE playback_radio_membership", [])
            .unwrap();
        drop(connection);
        let reopened = Arc::new(Database::new(path).unwrap());
        let restored = PlaybackSession::restore(reopened, "radio-restart".into());
        let snapshot = restored.snapshot().unwrap();
        assert_eq!(snapshot.restoration.status, "error");
        assert!(snapshot.radio.is_none());
    }

    #[test]
    fn completion_before_refill_admission_can_continue_from_new_successor() {
        let db = Arc::new(Database::memory().unwrap());
        let session = PlaybackSession::restore(db.clone(), "radio-test".into());
        start(&session);
        let lease = session.reserve_radio_refill().unwrap().unwrap();
        session.publish_event(lease.generation_id.clone(), PlaybackEvent::Active);
        session.publish_event(
            lease.generation_id.clone(),
            PlaybackEvent::Completed {
                position_ms: 180_000,
            },
        );
        std::thread::sleep(std::time::Duration::from_millis(40));
        assert_eq!(
            session.snapshot().unwrap().playback.status,
            PlaybackStatus::Completed
        );
        assert!(
            session
                .admit_radio_refill(lease, vec![candidate("one", "second", "artist")], None)
                .unwrap()
        );
        let after = session.snapshot().unwrap();
        assert_eq!(after.current.unwrap().source.track_id, "second");
        assert_eq!(after.playback.status, PlaybackStatus::Loading);
        assert!(
            db.radio_has_membership(&after.session_id, &source("one", "first"))
                .unwrap()
        );
    }

    #[test]
    fn pause_admits_queue_metadata_without_resuming_audio_and_stop_rejects_late_refill() {
        let db = Arc::new(Database::memory().unwrap());
        let session = PlaybackSession::restore(db, "radio-test".into());
        start(&session);
        let lease = session.reserve_radio_refill().unwrap().unwrap();
        let before = session.snapshot().unwrap();
        session
            .control_with_guard(
                ControlParams {
                    schema_version: 1,
                    instance_id: before.instance_id,
                    session_id: before.session_id,
                    command_id: Uuid::new_v4().to_string(),
                    expected_generation_id: before.generation_id,
                    occurrence_id: before.current.unwrap().occurrence_id,
                    action: ControlAction::Pause,
                },
                None,
            )
            .unwrap();
        assert!(
            session
                .admit_radio_refill(lease, vec![candidate("one", "second", "artist")], None)
                .unwrap()
        );
        let paused = session.snapshot().unwrap();
        assert_eq!(paused.state, TransportState::Paused);
        assert_eq!(paused.playback.status, PlaybackStatus::Paused);
        let again = session.reserve_radio_refill().unwrap();
        assert!(again.is_none()); // A short pass waits for a meaningful trigger.
        session
            .control_with_guard(
                ControlParams {
                    schema_version: 1,
                    instance_id: paused.instance_id,
                    session_id: paused.session_id,
                    command_id: Uuid::new_v4().to_string(),
                    expected_generation_id: paused.generation_id,
                    occurrence_id: paused.current.unwrap().occurrence_id,
                    action: ControlAction::Retry,
                },
                None,
            )
            .unwrap();
        let pending = session.reserve_radio_refill().unwrap().unwrap();
        let ready = session.snapshot().unwrap();
        session
            .control_with_guard(
                ControlParams {
                    schema_version: 1,
                    instance_id: ready.instance_id,
                    session_id: ready.session_id,
                    command_id: Uuid::new_v4().to_string(),
                    expected_generation_id: ready.generation_id,
                    occurrence_id: ready.current.unwrap().occurrence_id,
                    action: ControlAction::Stop,
                },
                None,
            )
            .unwrap();
        assert!(
            !session
                .admit_radio_refill(pending, vec![candidate("one", "third", "artist")], None)
                .unwrap()
        );
        assert_eq!(
            session.snapshot().unwrap().radio.unwrap().status,
            RadioStatus::Stopped
        );
    }

    #[test]
    fn preview_discards_late_refill_and_preserves_radio_identity() {
        let db = Arc::new(Database::memory().unwrap());
        let session = PlaybackSession::restore(db, "radio-test".into());
        start(&session);
        let lease = session.reserve_radio_refill().unwrap().unwrap();
        let before = session.snapshot().unwrap();
        let logical_id = before.radio.unwrap().logical_id;
        let preview = session
            .preview_with_guard(
                PreviewTrackParams {
                    schema_version: 1,
                    instance_id: before.instance_id,
                    session_id: before.session_id,
                    command_id: Uuid::new_v4().to_string(),
                    expected_queue_revision: before.queue_revision,
                    expected_generation_id: before.generation_id,
                    source: source("one", "preview"),
                },
                None,
            )
            .unwrap();
        assert!(
            !session
                .admit_radio_refill(lease, vec![candidate("one", "stale", "artist")], None)
                .unwrap()
        );
        assert_eq!(preview.radio.unwrap().logical_id, logical_id);
        let current = session.snapshot().unwrap();
        let returned = session
            .control_with_guard(
                ControlParams {
                    schema_version: 1,
                    instance_id: current.instance_id,
                    session_id: current.session_id,
                    command_id: Uuid::new_v4().to_string(),
                    expected_generation_id: current.generation_id,
                    occurrence_id: current.current.unwrap().occurrence_id,
                    action: ControlAction::ReturnToSession,
                },
                None,
            )
            .unwrap();
        assert_eq!(returned.radio.unwrap().logical_id, logical_id);
        assert!(session.reserve_radio_refill().unwrap().is_some());
    }

    #[test]
    fn replacement_invalidates_old_refill_identity() {
        let db = Arc::new(Database::memory().unwrap());
        let session = PlaybackSession::restore(db, "radio-test".into());
        start(&session);
        let old_id = session.snapshot().unwrap().radio.unwrap().logical_id;
        let lease = session.reserve_radio_refill().unwrap().unwrap();
        start(&session);
        let after = session.snapshot().unwrap();
        assert_ne!(after.radio.unwrap().logical_id, old_id);
        assert!(
            !session
                .admit_radio_refill(lease, vec![candidate("one", "stale", "artist")], None)
                .unwrap()
        );
        assert_eq!(session.snapshot().unwrap().total_occurrence_count, 1);
        assert!(session.reserve_radio_refill().unwrap().is_some());
    }

    #[test]
    fn failed_or_exhausted_pass_waits_until_explicit_retry() {
        for (failure, expected) in [
            (
                Some("radio.sourceFailure".to_string()),
                "radio.sourceFailure",
            ),
            (None, "radio.exhausted"),
        ] {
            let db = Arc::new(Database::memory().unwrap());
            let session = PlaybackSession::restore(db, "radio-test".into());
            start(&session);
            let lease = session.reserve_radio_refill().unwrap().unwrap();
            assert!(
                !session
                    .admit_radio_refill(lease, Vec::new(), failure)
                    .unwrap()
            );
            let waiting = session.snapshot().unwrap();
            assert_eq!(waiting.radio.unwrap().reason.as_deref(), Some(expected));
            assert!(session.reserve_radio_refill().unwrap().is_none());
            session
                .control_with_guard(
                    ControlParams {
                        schema_version: 1,
                        instance_id: waiting.instance_id,
                        session_id: waiting.session_id,
                        command_id: Uuid::new_v4().to_string(),
                        expected_generation_id: waiting.generation_id,
                        occurrence_id: waiting.current.unwrap().occurrence_id,
                        action: ControlAction::Retry,
                    },
                    None,
                )
                .unwrap();
            assert!(session.reserve_radio_refill().unwrap().is_some());
        }
    }

    #[test]
    fn retry_rechecks_exhausted_sources_without_clearing_membership() {
        let db = Arc::new(Database::memory().unwrap());
        let session = PlaybackSession::restore(db.clone(), "radio-test".into());
        start(&session);
        let lease = session.reserve_radio_refill().unwrap().unwrap();
        assert!(
            db.advance_radio_scan(
                &lease.session_id,
                lease.queue_revision,
                "source",
                crate::playback::selection::RadioSourceCursor::default(),
                None,
            )
            .unwrap()
        );
        session.admit_radio_refill(lease, Vec::new(), None).unwrap();
        let waiting = session.snapshot().unwrap();
        assert!(
            db.radio_scan_cursor(&waiting.session_id, "source")
                .unwrap()
                .is_none()
        );
        session
            .control_with_guard(
                ControlParams {
                    schema_version: 1,
                    instance_id: waiting.instance_id,
                    session_id: waiting.session_id.clone(),
                    command_id: Uuid::new_v4().to_string(),
                    expected_generation_id: waiting.generation_id,
                    occurrence_id: waiting.current.unwrap().occurrence_id,
                    action: ControlAction::Retry,
                },
                None,
            )
            .unwrap();
        assert_eq!(
            db.radio_scan_cursor(&waiting.session_id, "source").unwrap(),
            Some(crate::playback::selection::RadioSourceCursor::default())
        );
    }
}
