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
    pub original_settings: Option<super::selection::PlaybackSelectionConfig>,
    pub cycle: u64,
    pub transition: Option<RadioTransition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RadioTransition {
    pub center: ArtistIdentity,
    pub kind: RadioTransitionKind,
    /// A fixed locale key, never provider text or a URL.
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RadioTransitionKind {
    SharedTrackCredit,
    SimilarArtist,
    NewStartingPoint,
}

impl RadioTransition {
    pub fn new(center: ArtistIdentity, kind: RadioTransitionKind) -> Self {
        let reason = match kind {
            RadioTransitionKind::SharedTrackCredit => "radio.sharedTrackCredit",
            RadioTransitionKind::SimilarArtist => "radio.similarArtist",
            RadioTransitionKind::NewStartingPoint => "radio.newStartingPoint",
        }
        .to_string();
        Self {
            center,
            kind,
            reason,
        }
    }
}

/// Rank only verified source-local evidence. Stable ties cannot depend on
/// provider arrival timing or display names.
pub fn rank_relations(
    mut relations: Vec<(ArtistIdentity, RadioTransitionKind, usize, usize)>,
) -> Vec<RadioTransition> {
    relations.sort_by(|a, b| {
        let tier = |kind| match kind {
            RadioTransitionKind::SharedTrackCredit => 0,
            RadioTransitionKind::SimilarArtist => 1,
            RadioTransitionKind::NewStartingPoint => 2,
        };
        (tier(a.1), a.2, a.3, &a.0.server_id, &a.0.artist_id).cmp(&(
            tier(b.1),
            b.2,
            b.3,
            &b.0.server_id,
            &b.0.artist_id,
        ))
    });
    let mut seen = std::collections::HashSet::new();
    relations
        .into_iter()
        .filter(|(artist, _, _, _)| {
            seen.insert((artist.server_id.clone(), artist.artist_id.clone()))
        })
        .map(|(artist, kind, _, _)| RadioTransition::new(artist, kind))
        .collect()
}

pub fn choose_transition(
    settings: &super::selection::PlaybackSelectionConfig,
    pools: &[super::selection::SelectionPool],
    relations: &[RadioTransition],
    allow_fresh: bool,
) -> Option<(RadioTransition, Vec<TrackSource>)> {
    let mut stable = std::collections::HashMap::<(String, String), Option<String>>::new();
    for pool in pools {
        for song in &pool.tracks {
            let candidate = (!song.provider_metadata.ambiguous_music_artist)
                .then_some(song.artist_id.as_deref())
                .flatten()
                .filter(|id| !id.is_empty() && id.len() <= super::model::MAX_ID_BYTES)
                .map(str::to_owned);
            let key = (pool.source.server_id.clone(), song.id.clone());
            match stable.entry(key) {
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(candidate);
                }
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    if entry.get() != &candidate {
                        entry.insert(None);
                    }
                }
            }
        }
    }
    let valid_pools: Vec<_> = pools
        .iter()
        .cloned()
        .map(|mut pool| {
            pool.tracks.retain(|song| {
                stable
                    .get(&(pool.source.server_id.clone(), song.id.clone()))
                    .is_some_and(Option::is_some)
            });
            pool
        })
        .collect();
    crate::daemon_log!(
        "[Radio] transition_search relations={} allow_fresh={} fetched={} stable_artist_tracks={}",
        relations.len(),
        allow_fresh,
        pools.iter().map(|pool| pool.tracks.len()).sum::<usize>(),
        valid_pools
            .iter()
            .map(|pool| pool.tracks.len())
            .sum::<usize>(),
    );
    for relation in relations {
        crate::daemon_log!(
            "[Radio] transition_attempt kind={:?} server={:?} artist_id={:?}",
            relation.kind,
            relation.center.server_id,
            relation.center.artist_id,
        );
        let scoped = valid_pools
            .iter()
            .cloned()
            .map(|mut pool| {
                pool.tracks.retain(|song| {
                    stable
                        .get(&(pool.source.server_id.clone(), song.id.clone()))
                        .and_then(Option::as_deref)
                        == Some(relation.center.artist_id.as_str())
                        && relation.center.server_id == pool.source.server_id
                });
                pool
            })
            .collect();
        let scoped = include_recording_copies(&valid_pools, scoped);
        if let Ok(ordered) = super::selection::select_radio_order(settings, scoped)
            && !ordered.is_empty()
        {
            crate::daemon_log!(
                "[Radio] transition_proposed kind={:?} server={:?} artist_id={:?} candidates={}",
                relation.kind,
                relation.center.server_id,
                relation.center.artist_id,
                ordered.len()
            );
            return Some((relation.clone(), ordered));
        }
    }
    if !allow_fresh {
        crate::daemon_log!(
            "[Radio] transition_search no_related_candidate; continuing related scan"
        );
        return None;
    }
    let ordered = super::selection::select_radio_order(settings, valid_pools.clone()).ok()?;
    for source in ordered {
        let Some(artist_id) = stable
            .get(&(source.server_id.clone(), source.track_id.clone()))
            .and_then(Option::as_ref)
        else {
            continue;
        };
        let center = ArtistIdentity {
            server_id: source.server_id,
            artist_id: artist_id.clone(),
        };
        let scoped = valid_pools
            .iter()
            .cloned()
            .map(|mut pool| {
                pool.tracks.retain(|song| {
                    stable
                        .get(&(pool.source.server_id.clone(), song.id.clone()))
                        .and_then(Option::as_deref)
                        == Some(center.artist_id.as_str())
                        && center.server_id == pool.source.server_id
                });
                pool
            })
            .collect();
        let scoped = include_recording_copies(&valid_pools, scoped);
        if let Ok(ordered) = super::selection::select_radio_order(settings, scoped)
            && !ordered.is_empty()
        {
            crate::daemon_log!(
                "[Radio] transition_proposed kind=NewStartingPoint server={:?} artist_id={:?} candidates={}",
                center.server_id,
                center.artist_id,
                ordered.len()
            );
            return Some((
                RadioTransition::new(center, RadioTransitionKind::NewStartingPoint),
                ordered,
            ));
        }
    }
    None
}

fn include_recording_copies(
    available: &[super::selection::SelectionPool],
    anchors: Vec<super::selection::SelectionPool>,
) -> Vec<super::selection::SelectionPool> {
    let keys: std::collections::HashSet<_> = anchors
        .iter()
        .flat_map(|pool| &pool.tracks)
        .filter_map(|song| {
            song.provider_metadata
                .recording
                .as_ref()
                .and_then(|e| e.key())
        })
        .map(|key| key.as_str().to_owned())
        .collect();
    available
        .iter()
        .cloned()
        .zip(anchors)
        .map(|(mut pool, anchor)| {
            let anchor_ids: std::collections::HashSet<_> =
                anchor.tracks.iter().map(|song| song.id.as_str()).collect();
            pool.tracks.retain(|song| {
                anchor_ids.contains(song.id.as_str())
                    || song
                        .provider_metadata
                        .recording
                        .as_ref()
                        .and_then(|e| e.key())
                        .is_some_and(|key| keys.contains(key.as_str()))
            });
            pool
        })
        .collect()
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
        } else if matches!(self.status, RadioStatus::Filling | RadioStatus::Ready)
            && self.original_settings.is_none()
        {
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
                    | "radio.snapshotUnavailable"
                    | "radio.sharedTrackCredit"
                    | "radio.similarArtist"
                    | "radio.newStartingPoint"
                    | "radio.newCycle"
                    | "radio.cyclePending"
            )
        }) {
            return Err("INVALID_RADIO_STATE");
        }
        if self.cycle == 0
            || self
                .original_settings
                .as_ref()
                .is_some_and(|settings| settings.validate().is_err())
        {
            return Err("INVALID_RADIO_STATE");
        }
        if self.transition.as_ref().is_some_and(|transition| {
            self.center.as_ref() != Some(&transition.center)
                || RadioTransition::new(transition.center.clone(), transition.kind).reason
                    != transition.reason
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
    pub center: Option<ArtistIdentity>,
    pub cycle: u64,
    pub original_settings: super::selection::PlaybackSelectionConfig,
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

    #[test]
    fn radio_gain_freezes_for_current_source_and_restores_paused() {
        use crate::domain::models::TrackLoudnessEvidence;
        let db = Arc::new(Database::memory().unwrap());
        let session = PlaybackSession::restore(db.clone(), "radio-policy".into());
        start(&session);
        let snapshot = session.snapshot().unwrap();
        let occurrence = snapshot.current.clone().unwrap();
        let mut song = candidate("one", "first", "artist").song;
        song.suffix = Some("flac".into());
        song.content_type = Some("audio/flac".into());
        song.track_loudness = TrackLoudnessEvidence::open_subsonic(-6.0, 0.8);
        let epoch = session.control_epoch();
        let revision = snapshot.queue_revision.parse().unwrap();
        assert!(
            session
                .freeze_radio_policy("stale", epoch, revision, &occurrence, &song)
                .unwrap()
                .is_none()
        );
        let first = session
            .freeze_radio_policy(&snapshot.generation_id, epoch, revision, &occurrence, &song)
            .unwrap()
            .unwrap();
        assert!(first.0 < 1.0);
        song.track_loudness = TrackLoudnessEvidence::open_subsonic(6.0, 0.8);
        assert_eq!(
            session
                .freeze_radio_policy(&snapshot.generation_id, epoch, revision, &occurrence, &song)
                .unwrap(),
            Some(first.clone())
        );
        let saved = session.snapshot().unwrap();
        assert_eq!(f32::from_bits(saved.gain_bits), first.0);
        assert_eq!(saved.qualified_suffix.as_deref(), Some("flac"));
        let restored = PlaybackSession::restore(db, "radio-policy-restored".into())
            .snapshot()
            .unwrap();
        assert_eq!(restored.state, TransportState::Paused);
        assert_eq!(
            restored.current.as_ref().unwrap().occurrence_id,
            occurrence.occurrence_id
        );
        assert_eq!(f32::from_bits(restored.gain_bits), first.0);
    }

    #[test]
    fn legacy_radio_without_policy_restores_paused_and_freezes_on_resolution() {
        let db = Arc::new(Database::memory().unwrap());
        let initial = PlaybackSession::restore(db.clone(), "legacy-radio".into());
        start(&initial);
        let original = initial.snapshot().unwrap().current.unwrap();
        let restored = PlaybackSession::restore(db, "legacy-radio-restored".into());
        let before = restored.snapshot().unwrap();
        assert_eq!(before.state, TransportState::Paused);
        assert_eq!(before.gain_bits, 1.0f32.to_bits());
        assert_eq!(
            before.current.as_ref().unwrap().occurrence_id,
            original.occurrence_id
        );
        let mut song = candidate("one", "first", "artist").song;
        song.suffix = Some("flac".into());
        song.content_type = Some("audio/flac".into());
        song.track_loudness =
            crate::domain::models::TrackLoudnessEvidence::open_subsonic(-3.0, 0.8);
        // Output recreation and ordinary commands share this resolution path.
        let decision = crate::playback::commands::resolved_gain(
            &restored,
            before.queue_kind,
            before.queue_revision.parse().unwrap(),
            &before.generation_id,
            restored.control_epoch(),
            before.current.as_ref().unwrap(),
            &song,
            f32::from_bits(before.gain_bits),
            before.qualified_suffix,
        )
        .unwrap();
        assert!(decision.0 < 1.0);
        assert_eq!(
            f32::from_bits(restored.snapshot().unwrap().gain_bits),
            decision.0
        );
    }

    fn source(server: &str, track: &str) -> TrackSource {
        TrackSource {
            server_id: server.into(),
            track_id: track.into(),
        }
    }

    fn candidate(server: &str, track: &str, artist: &str) -> SelectionCandidate {
        SelectionCandidate {
            source: source(server, track),
            center_origin: None,
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
                track_loudness: Default::default(),
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
                settings: Some(crate::playback::selection::PlaybackSelectionConfig::default()),
                recording: None,
            },
        );
    }

    fn evidence(title: &str) -> crate::playback::recording::RecordingEvidence {
        crate::playback::recording::RecordingEvidence::from_recording_field(
            crate::playback::recording::RecordingProvenance::OpenSubsonicSong,
            Some("189002e7-3285-4e2e-92a3-7f6c30d407a2"),
            title,
        )
    }

    #[test]
    fn initial_radio_recording_blocks_another_server_then_skip_survives_restart() {
        let temp = tempfile::tempdir().unwrap();
        let db = Arc::new(Database::new(temp.path().join("recording.db")).unwrap());
        let session = PlaybackSession::restore(db.clone(), "recording".into());
        apply(
            &session,
            SessionOperation::StartRadio {
                source: source("one", "first"),
                center: Some(ArtistIdentity {
                    server_id: "one".into(),
                    artist_id: "artist".into(),
                }),
                settings: Some(crate::playback::selection::PlaybackSelectionConfig::default()),
                recording: Some(evidence("Take")),
            },
        );
        let key = evidence("Take").key().unwrap().as_str().to_owned();
        let initial = session.snapshot().unwrap();
        assert!(db.radio_recording_used(&initial.session_id, &key).unwrap());
        assert_eq!(
            db.radio_occurrence_recording_key(
                &initial.session_id,
                &initial.current.as_ref().unwrap().occurrence_id
            )
            .unwrap(),
            Some(key.clone())
        );
        assert_eq!(
            db.radio_occurrence_recording_key(
                "another-session",
                &initial.current.as_ref().unwrap().occurrence_id
            )
            .unwrap(),
            None
        );
        let lease = session.reserve_radio_refill().unwrap().unwrap();
        let mut copy = candidate("two", "another-local-id", "other-artist");
        copy.song.title = "Take".into();
        copy.song.provider_metadata.recording = Some(evidence("Take"));
        let transition = RadioTransition::new(
            ArtistIdentity {
                server_id: "two".into(),
                artist_id: "other-artist".into(),
            },
            RadioTransitionKind::NewStartingPoint,
        );
        assert!(
            !session
                .admit_radio_refill_plan(lease, vec![copy], None, false, Some(transition))
                .unwrap()
        );
        assert_eq!(
            db.playback_page(&initial.session_id, None, 20)
                .unwrap()
                .len(),
            1
        );
        let stored = db.load_playback_session().unwrap().unwrap();
        db.persist_playback_terminal(
            &stored,
            stored.current_occurrence_id.as_deref().unwrap(),
            "explicitSkip",
            None,
            1000,
        )
        .unwrap();
        assert!(
            db.radio_recording_has_kind(&initial.session_id, &key, "excluded")
                .unwrap()
        );
        session.shutdown_checkpoint().unwrap();
        session.stop_and_join().unwrap();
        drop(session);
        let restored = PlaybackSession::restore(db.clone(), "restored".into());
        assert_eq!(restored.snapshot().unwrap().total_occurrence_count, 1);
        assert!(
            db.radio_recording_has_kind(&initial.session_id, &key, "excluded")
                .unwrap()
        );
    }

    #[test]
    fn automatic_copies_share_one_slot_and_removal_excludes_the_recording() {
        let db = Arc::new(Database::memory().unwrap());
        let session = PlaybackSession::restore(db.clone(), "copies".into());
        start(&session);
        let lease = session.reserve_radio_refill().unwrap().unwrap();
        let mut first = candidate("one", "copy-a", "artist");
        first.song.title = "Take".into();
        first.song.provider_metadata.recording = Some(evidence("Take"));
        let mut second = candidate("one", "copy-b", "artist");
        second.song.title = "Take".into();
        second.song.provider_metadata.recording = Some(evidence("Take"));
        assert!(
            session
                .admit_radio_refill_plan(lease, vec![first, second], None, true, None)
                .unwrap()
        );
        let snapshot = session.snapshot().unwrap();
        let old_logical_id = snapshot.radio.as_ref().unwrap().logical_id.clone();
        let page = db.playback_page(&snapshot.session_id, None, 20).unwrap();
        assert_eq!(page.len(), 2);
        assert_eq!(page[1].source, source("one", "copy-a"));
        apply(
            &session,
            SessionOperation::RemoveUpcoming {
                occurrence_ids: vec![page[1].occurrence_id.clone()],
            },
        );
        let key = evidence("Take").key().unwrap().as_str().to_owned();
        assert!(
            db.radio_recording_has_kind(&snapshot.session_id, &key, "excluded")
                .unwrap()
        );
        assert!(db.radio_recording_used(&snapshot.session_id, &key).unwrap());
        start(&session);
        let fresh = session.snapshot().unwrap();
        assert_ne!(fresh.radio.as_ref().unwrap().logical_id, old_logical_id);
        assert!(!db.radio_recording_used(&fresh.session_id, &key).unwrap());
    }

    #[test]
    fn technical_failure_does_not_exclude_a_recording_copy() {
        let db = Arc::new(Database::memory().unwrap());
        let session = PlaybackSession::restore(db.clone(), "failure".into());
        apply(
            &session,
            SessionOperation::StartRadio {
                source: source("one", "first"),
                center: Some(ArtistIdentity {
                    server_id: "one".into(),
                    artist_id: "artist".into(),
                }),
                settings: Some(crate::playback::selection::PlaybackSelectionConfig::default()),
                recording: Some(evidence("Take")),
            },
        );
        let stored = db.load_playback_session().unwrap().unwrap();
        let key = evidence("Take").key().unwrap().as_str().to_owned();
        db.persist_playback_terminal(
            &stored,
            stored.current_occurrence_id.as_deref().unwrap(),
            "technicalFailure",
            Some("SOURCE_UNAVAILABLE"),
            0,
        )
        .unwrap();
        assert!(
            !db.radio_recording_has_kind(&stored.session_id, &key, "excluded")
                .unwrap()
        );
        assert!(
            !db.radio_recording_has_kind(&stored.session_id, &key, "heard")
                .unwrap()
        );
        assert!(!db.radio_recording_used(&stored.session_id, &key).unwrap());
    }

    #[test]
    fn cycle_renewal_clears_recording_heard_but_keeps_exclusion() {
        let db = Arc::new(Database::memory().unwrap());
        let session = PlaybackSession::restore(db.clone(), "cycle-recording".into());
        apply(
            &session,
            SessionOperation::StartRadio {
                source: source("one", "first"),
                center: Some(ArtistIdentity {
                    server_id: "one".into(),
                    artist_id: "artist".into(),
                }),
                settings: Some(crate::playback::selection::PlaybackSelectionConfig::default()),
                recording: Some(evidence("Take")),
            },
        );
        let mut stored = db.load_playback_session().unwrap().unwrap();
        let key = evidence("Take").key().unwrap().as_str().to_owned();
        db.persist_playback_terminal(
            &stored,
            stored.current_occurrence_id.as_deref().unwrap(),
            "naturalCompletion",
            None,
            1000,
        )
        .unwrap();
        assert!(
            db.radio_recording_has_kind(&stored.session_id, &key, "heard")
                .unwrap()
        );
        let excluded = "mbrec:1:aaaaaaaa-aaaa-4aaa-aaaa-aaaaaaaaaaaa:plain";
        db.conn.lock().unwrap().execute("INSERT INTO playback_radio_recording_membership(session_id,recording_key,kind) VALUES(?1,?2,'excluded')", rusqlite::params![stored.session_id,excluded]).unwrap();
        stored.radio.as_mut().unwrap().cycle = 2;
        db.renew_radio_cycle(&stored, 1).unwrap();
        assert!(
            !db.radio_recording_has_kind(&stored.session_id, &key, "heard")
                .unwrap()
        );
        assert!(
            db.radio_recording_has_kind(&stored.session_id, excluded, "excluded")
                .unwrap()
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
    fn centerless_first_track_can_reserve_a_fallback_refill() {
        let db = Arc::new(Database::memory().unwrap());
        let session = PlaybackSession::restore(db, "centerless-radio".into());
        apply(
            &session,
            SessionOperation::StartRadio {
                source: source("one", "first"),
                center: None,
                settings: Some(crate::playback::selection::PlaybackSelectionConfig::default()),
                recording: None,
            },
        );
        let lease = session.reserve_radio_refill().unwrap();
        assert!(lease.is_some(), "centerless Radio needs a worker lease");
        assert!(lease.unwrap().center.is_none());
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
    fn direct_credit_precedes_server_suggestion_and_deduplicates_identity() {
        let artist = |server: &str, id: &str| ArtistIdentity {
            server_id: server.into(),
            artist_id: id.into(),
        };
        let ranked = rank_relations(vec![
            (
                artist("one", "suggested"),
                RadioTransitionKind::SimilarArtist,
                0,
                0,
            ),
            (
                artist("one", "credit"),
                RadioTransitionKind::SharedTrackCredit,
                1,
                0,
            ),
            (
                artist("one", "credit"),
                RadioTransitionKind::SimilarArtist,
                0,
                0,
            ),
            (
                artist("two", "credit"),
                RadioTransitionKind::SharedTrackCredit,
                0,
                1,
            ),
        ]);
        assert_eq!(
            ranked
                .iter()
                .map(|item| item.center.artist_id.as_str())
                .collect::<Vec<_>>(),
            vec!["credit", "credit", "suggested"]
        );
        assert_eq!(ranked[0].kind, RadioTransitionKind::SharedTrackCredit);
        assert_ne!(ranked[0].center.server_id, ranked[1].center.server_id);
    }

    #[test]
    fn related_artist_needs_a_track_in_original_scope_and_genre_alone_falls_back() {
        use crate::playback::selection::{
            PlaybackSelectionConfig, SelectionKind, SelectionPool, SelectionSource,
        };
        let mut settings = PlaybackSelectionConfig::default();
        settings.sources = vec![
            SelectionSource {
                server_id: "one".into(),
                kind: SelectionKind::Genre,
                ref_id: "rock".into(),
            },
            SelectionSource {
                server_id: "two".into(),
                kind: SelectionKind::Genre,
                ref_id: "rock".into(),
            },
        ];
        let pools = vec![
            SelectionPool {
                source: settings.sources[0].clone(),
                tracks: vec![candidate("one", "linked-song", "linked").song],
            },
            SelectionPool {
                source: settings.sources[1].clone(),
                tracks: vec![candidate("two", "other-song", "other").song],
            },
        ];
        let relation = RadioTransition::new(
            ArtistIdentity {
                server_id: "one".into(),
                artist_id: "linked".into(),
            },
            RadioTransitionKind::SharedTrackCredit,
        );
        let (chosen, ordered) = choose_transition(&settings, &pools, &[relation], false).unwrap();
        assert_eq!(chosen.kind, RadioTransitionKind::SharedTrackCredit);
        assert!(ordered.iter().all(|source| source.server_id == "one"));
        let unavailable = RadioTransition::new(
            ArtistIdentity {
                server_id: "two".into(),
                artist_id: "linked".into(),
            },
            RadioTransitionKind::SimilarArtist,
        );
        assert!(choose_transition(&settings, &pools, &[unavailable], false).is_none());
        let (fresh, _) = choose_transition(&settings, &pools, &[], true).unwrap();
        assert_eq!(fresh.kind, RadioTransitionKind::NewStartingPoint);
    }

    #[test]
    fn transition_can_choose_better_copy_without_changing_its_artist_anchor() {
        use crate::playback::selection::{
            PlaybackSelectionConfig, SelectionKind, SelectionPool, SelectionSource,
        };
        let settings = PlaybackSelectionConfig {
            sources: vec![
                SelectionSource {
                    server_id: "one".into(),
                    kind: SelectionKind::Genre,
                    ref_id: "rock".into(),
                },
                SelectionSource {
                    server_id: "two".into(),
                    kind: SelectionKind::Genre,
                    ref_id: "rock".into(),
                },
            ],
            ..Default::default()
        };
        let mut anchor = candidate("one", "first", "artist").song;
        anchor.title = "Take".into();
        anchor.suffix = Some("mp3".into());
        anchor.bitrate_kbps = Some(128);
        anchor.provider_metadata.recording = Some(evidence("Take"));
        let mut better = candidate("two", "second", "other").song;
        better.title = "Take".into();
        better.suffix = Some("mp3".into());
        better.bitrate_kbps = Some(320);
        better.provider_metadata.recording = Some(evidence("Take"));
        let relation = RadioTransition::new(
            ArtistIdentity {
                server_id: "one".into(),
                artist_id: "artist".into(),
            },
            RadioTransitionKind::SimilarArtist,
        );
        let (chosen, ordered) = choose_transition(
            &settings,
            &[
                SelectionPool {
                    source: settings.sources[0].clone(),
                    tracks: vec![anchor],
                },
                SelectionPool {
                    source: settings.sources[1].clone(),
                    tracks: vec![better],
                },
            ],
            &[relation],
            false,
        )
        .unwrap();
        assert_eq!(chosen.center.server_id, "one");
        assert_eq!(ordered[0], source("two", "second"));
    }

    #[test]
    fn owner_preserves_cross_server_copy_source_and_artist_anchor() {
        let db = Arc::new(Database::memory().unwrap());
        let session = PlaybackSession::restore(db.clone(), "copy-source".into());
        start(&session);
        let lease = session.reserve_radio_refill().unwrap().unwrap();
        let mut copy = candidate("two", "better", "other-artist");
        copy.song.title = "Take".into();
        copy.song.provider_metadata.recording = Some(evidence("Take"));
        copy.center_origin = Some(ArtistIdentity {
            server_id: "one".into(),
            artist_id: "artist".into(),
        });
        assert!(
            session
                .admit_radio_refill_plan(lease, vec![copy], None, true, None)
                .unwrap()
        );
        let snapshot = session.snapshot().unwrap();
        let page = db.playback_page(&snapshot.session_id, None, 20).unwrap();
        assert_eq!(page[1].source, source("two", "better"));
        assert_eq!(snapshot.radio.unwrap().center.unwrap().server_id, "one");
    }

    #[test]
    fn deliberate_manual_repeats_keep_independent_occurrences() {
        let db = Arc::new(Database::memory().unwrap());
        let session = PlaybackSession::restore(db.clone(), "manual-repeats".into());
        apply(
            &session,
            SessionOperation::StartRadio {
                source: source("one", "first"),
                center: Some(ArtistIdentity {
                    server_id: "one".into(),
                    artist_id: "artist".into(),
                }),
                settings: Some(crate::playback::selection::PlaybackSelectionConfig::default()),
                recording: Some(evidence("Take")),
            },
        );
        apply(
            &session,
            SessionOperation::AppendQueue {
                sources: vec![source("one", "first"), source("one", "first")],
            },
        );
        let snapshot = session.snapshot().unwrap();
        let page = db.playback_page(&snapshot.session_id, None, 20).unwrap();
        assert_eq!(page.len(), 3);
        assert_ne!(page[1].occurrence_id, page[2].occurrence_id);
        assert_eq!(page[1].source, page[2].source);
        apply(
            &session,
            SessionOperation::RemoveUpcoming {
                occurrence_ids: vec![page[1].occurrence_id.clone()],
            },
        );
        assert!(
            !db.radio_recording_has_kind(
                &snapshot.session_id,
                evidence("Take").key().unwrap().as_str(),
                "excluded"
            )
            .unwrap()
        );
    }

    #[test]
    fn contradictory_artist_ids_for_one_source_track_cannot_become_a_center() {
        use crate::playback::selection::{
            PlaybackSelectionConfig, SelectionKind, SelectionPool, SelectionSource,
        };
        let mut settings = PlaybackSelectionConfig::default();
        settings.sources = vec![
            SelectionSource {
                server_id: "one".into(),
                kind: SelectionKind::Playlist,
                ref_id: "list".into(),
            },
            SelectionSource {
                server_id: "one".into(),
                kind: SelectionKind::Genre,
                ref_id: "rock".into(),
            },
        ];
        let pools = vec![
            SelectionPool {
                source: settings.sources[0].clone(),
                tracks: vec![candidate("one", "same-track", "first").song],
            },
            SelectionPool {
                source: settings.sources[1].clone(),
                tracks: vec![candidate("one", "same-track", "second").song],
            },
        ];
        let relation = RadioTransition::new(
            ArtistIdentity {
                server_id: "one".into(),
                artist_id: "first".into(),
            },
            RadioTransitionKind::SimilarArtist,
        );
        assert!(choose_transition(&settings, &pools, &[relation], true).is_none());
    }

    #[test]
    fn transition_and_append_commit_once_under_the_original_lease() {
        let db = Arc::new(Database::memory().unwrap());
        let session = PlaybackSession::restore(db.clone(), "transition".into());
        start(&session);
        let lease = session.reserve_radio_refill().unwrap().unwrap();
        assert!(
            db.advance_radio_scan(
                &lease.session_id,
                lease.queue_revision,
                "fresh:scope",
                Default::default(),
                None
            )
            .unwrap()
        );
        let transition = RadioTransition::new(
            ArtistIdentity {
                server_id: "two".into(),
                artist_id: "linked".into(),
            },
            RadioTransitionKind::SimilarArtist,
        );
        assert!(
            session
                .admit_radio_refill_plan(
                    lease.clone(),
                    vec![candidate("two", "second", "linked")],
                    None,
                    true,
                    Some(transition.clone())
                )
                .unwrap()
        );
        assert!(
            !session
                .admit_radio_refill_plan(
                    lease,
                    vec![candidate("two", "third", "linked")],
                    None,
                    false,
                    Some(transition.clone())
                )
                .unwrap()
        );
        let radio = session.snapshot().unwrap().radio.unwrap();
        assert_eq!(radio.center, Some(transition.center));
        assert_eq!(radio.transition.unwrap().reason, "radio.similarArtist");
        assert_eq!(
            db.playback_page(&session.snapshot().unwrap().session_id, None, 20)
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            db.radio_scan_cursor(&session.snapshot().unwrap().session_id, "fresh:scope")
                .unwrap(),
            Some(Default::default())
        );
    }

    #[test]
    fn original_settings_and_transition_restore_without_adopting_later_edits() {
        use crate::playback::selection::{PlaybackSelectionConfig, SelectionKind, SelectionSource};
        let temp = tempfile::tempdir().unwrap();
        let db = Arc::new(Database::new(temp.path().join("radio-settings.db")).unwrap());
        let session = PlaybackSession::restore(db.clone(), "settings".into());
        let mut settings = PlaybackSelectionConfig::default();
        settings.sources.push(SelectionSource {
            server_id: "one".into(),
            kind: SelectionKind::Artist,
            ref_id: "artist".into(),
        });
        settings.seed = 17;
        apply(
            &session,
            SessionOperation::StartRadio {
                source: source("one", "first"),
                center: None,
                settings: Some(settings.clone()),
                recording: None,
            },
        );
        settings.seed = 99;
        let lease = session.reserve_radio_refill().unwrap().unwrap();
        assert_eq!(lease.original_settings.seed, 17);
        let transition = RadioTransition::new(
            ArtistIdentity {
                server_id: "one".into(),
                artist_id: "new".into(),
            },
            RadioTransitionKind::NewStartingPoint,
        );
        assert!(
            session
                .admit_radio_refill_plan(
                    lease,
                    vec![candidate("one", "new-track", "new")],
                    None,
                    true,
                    Some(transition)
                )
                .unwrap()
        );
        session.shutdown_checkpoint().unwrap();
        session.stop_and_join().unwrap();
        let restored = PlaybackSession::restore(db, "settings-restored".into());
        let radio = restored.snapshot().unwrap().radio.unwrap();
        assert_eq!(radio.original_settings.unwrap().seed, 17);
        assert_eq!(radio.transition.unwrap().reason, "radio.newStartingPoint");
    }

    #[test]
    fn legacy_radio_without_provable_snapshot_keeps_queue_but_waits_for_new_start() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("legacy-radio.db");
        let db = Arc::new(Database::new(path.clone()).unwrap());
        let session = PlaybackSession::restore(db.clone(), "legacy".into());
        start(&session);
        let before = session.snapshot().unwrap();
        session.shutdown_checkpoint().unwrap();
        session.stop_and_join().unwrap();
        drop(session);
        drop(db);
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .execute(
                "UPDATE playback_radio SET settings_json=NULL,status='ready',reason=NULL",
                [],
            )
            .unwrap();
        connection
            .execute("UPDATE playback_schema SET version=8", [])
            .unwrap();
        drop(connection);
        let reopened = Arc::new(Database::new(path).unwrap());
        let restored = PlaybackSession::restore(reopened, "legacy-restored".into());
        let after = restored.snapshot().unwrap();
        assert_eq!(after.total_occurrence_count, before.total_occurrence_count);
        assert_eq!(
            after.radio.as_ref().unwrap().logical_id,
            before.radio.unwrap().logical_id
        );
        assert_eq!(
            after.radio.unwrap().reason.as_deref(),
            Some("radio.snapshotUnavailable")
        );
        assert!(restored.reserve_radio_refill().unwrap().is_none());
    }

    #[test]
    fn verified_fresh_scope_exhaustion_renews_only_heard_membership() {
        let db = Arc::new(Database::memory().unwrap());
        let session = PlaybackSession::restore(db.clone(), "cycle".into());
        start(&session);
        let lease = session.reserve_radio_refill().unwrap().unwrap();
        let stored = db.load_playback_session().unwrap().unwrap();
        let current_id = stored.current_occurrence_id.clone().unwrap();
        db.persist_playback_terminal(&stored, &current_id, "naturalCompletion", None, 1000)
            .unwrap();
        db.conn.lock().unwrap().execute(
            "INSERT INTO playback_radio_membership(session_id,server_id,track_id,kind) VALUES(?1,'one','skipped','excluded')",
            [&stored.session_id],
        ).unwrap();
        assert!(
            db.radio_is_heard(&stored.session_id, &source("one", "first"))
                .unwrap()
        );
        assert!(
            db.advance_radio_scan_with_heard(
                &stored.session_id,
                lease.queue_revision,
                "fresh:one",
                Default::default(),
                None,
                true
            )
            .unwrap()
        );
        assert!(
            !session
                .admit_radio_refill_plan(
                    lease,
                    Vec::new(),
                    Some("radio.exhausted".into()),
                    false,
                    None
                )
                .unwrap()
        );
        let radio = session.snapshot().unwrap().radio.unwrap();
        assert_eq!(radio.cycle, 2);
        assert_eq!(radio.reason.as_deref(), Some("radio.newCycle"));
        assert!(
            !db.radio_has_membership(&stored.session_id, &source("one", "first"))
                .unwrap()
        );
        assert!(
            db.radio_has_membership(&stored.session_id, &source("one", "skipped"))
                .unwrap()
        );
        assert_eq!(
            db.radio_scan_cursor(&stored.session_id, "fresh:one")
                .unwrap(),
            Some(Default::default())
        );
        session.shutdown_checkpoint().unwrap();
        session.stop_and_join().unwrap();
        let restored = PlaybackSession::restore(db, "cycle-restored".into());
        let after_restart = restored.snapshot().unwrap();
        assert_eq!(after_restart.radio.unwrap().cycle, 2);
        assert_eq!(after_restart.state, TransportState::Paused);
    }

    #[test]
    fn completing_the_only_track_reopens_an_exhausted_scan_for_cycle_proof() {
        let db = Arc::new(Database::memory().unwrap());
        let session = PlaybackSession::restore(db.clone(), "single-track-cycle".into());
        start(&session);
        let lease = session.reserve_radio_refill().unwrap().unwrap();
        db.advance_radio_scan(
            &lease.session_id,
            lease.queue_revision,
            "fresh:only",
            Default::default(),
            None,
        )
        .unwrap();
        session
            .admit_radio_refill(lease, Vec::new(), Some("radio.exhausted".into()))
            .unwrap();
        let mut stored = db.load_playback_session().unwrap().unwrap();
        assert_eq!(
            stored.radio.as_ref().unwrap().reason.as_deref(),
            Some("radio.exhausted")
        );
        stored.radio.as_mut().unwrap().status = RadioStatus::Ready;
        stored.radio.as_mut().unwrap().reason = None;
        let current_id = stored.current_occurrence_id.clone().unwrap();
        db.persist_playback_terminal(&stored, &current_id, "naturalCompletion", None, 1000)
            .unwrap();
        assert_eq!(
            db.radio_scan_cursor(&stored.session_id, "fresh:only")
                .unwrap(),
            Some(Default::default())
        );
        assert!(
            db.radio_is_heard(&stored.session_id, &source("one", "first"))
                .unwrap()
        );
    }

    #[test]
    fn public_session_operation_rejects_forged_radio_snapshot() {
        let forged = serde_json::json!({
            "type": "startRadio",
            "source": {"serverId": "one", "trackId": "first"},
            "center": {"serverId": "one", "artistId": "artist"},
            "settings": {"schemaVersion": 1, "sources": [], "ordering": ["random"], "seed": 0, "maxTracks": 16}
        });
        assert!(serde_json::from_value::<SessionOperation>(forged).is_err());
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
                settings: None,
                recording: None,
            },
        );
        let snapshot = session.snapshot().unwrap();
        assert_eq!(
            snapshot.radio.unwrap().reason.as_deref(),
            Some("radio.snapshotUnavailable")
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
    fn corrupt_transition_metadata_preserves_queue_with_recoverable_waiting_reason() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("corrupt-transition.db");
        let db = Arc::new(Database::new(path.clone()).unwrap());
        let session = PlaybackSession::restore(db.clone(), "radio-test".into());
        start(&session);
        let before = session.snapshot().unwrap();
        session.shutdown_checkpoint().unwrap();
        session.stop_and_join().unwrap();
        drop(session);
        drop(db);
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .execute("UPDATE playback_radio SET transition_json='{malformed'", [])
            .unwrap();
        drop(connection);
        let reopened = Arc::new(Database::new(path).unwrap());
        let restored = PlaybackSession::restore(reopened, "radio-restart".into());
        let after = restored.snapshot().unwrap();
        assert_eq!(after.session_id, before.session_id);
        assert_eq!(after.current, before.current);
        let radio = after.radio.unwrap();
        assert_eq!(radio.status, RadioStatus::Waiting);
        assert_eq!(radio.reason.as_deref(), Some("radio.snapshotUnavailable"));
        assert!(restored.reserve_radio_refill().unwrap().is_none());
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
        let mut song = candidate("one", "first", "artist").song;
        song.suffix = Some("flac".into());
        song.content_type = Some("audio/flac".into());
        song.track_loudness =
            crate::domain::models::TrackLoudnessEvidence::open_subsonic(-6.0, 0.8);
        let frozen = session
            .freeze_radio_policy(
                &before.generation_id,
                session.control_epoch(),
                before.queue_revision.parse().unwrap(),
                before.current.as_ref().unwrap(),
                &song,
            )
            .unwrap()
            .unwrap();
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
        assert_eq!(preview.gain_bits, 1.0f32.to_bits());
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
        assert_eq!(f32::from_bits(returned.gain_bits), frozen.0);
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
