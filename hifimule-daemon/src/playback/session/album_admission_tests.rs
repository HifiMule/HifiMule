use super::*;

struct Owner(PlaybackSession);
impl Owner {
    fn new() -> Self {
        Self(PlaybackSession::restore(
            Arc::new(Database::memory().unwrap()),
            Uuid::new_v4().to_string(),
        ))
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        self.0.stop_and_join().unwrap();
    }
}

fn request(snapshot: &SessionSnapshot) -> PlayAlbumParams {
    PlayAlbumParams {
        schema_version: SCHEMA_VERSION,
        instance_id: snapshot.instance_id.clone(),
        session_id: snapshot.session_id.clone(),
        command_id: Uuid::new_v4().to_string(),
        expected_queue_revision: snapshot.queue_revision.clone(),
        expected_generation_id: snapshot.generation_id.clone(),
        source: AlbumSource {
            server_id: "server".into(),
            album_id: "album".into(),
        },
    }
}

fn sources(count: usize) -> Vec<TrackSource> {
    (0..count)
        .map(|n| TrackSource {
            server_id: "server".into(),
            track_id: format!("track-{n}"),
        })
        .collect()
}

fn resolve(owner: &PlaybackSession, params: PlayAlbumParams) -> AlbumReservation {
    match owner.reserve_album(params, None).unwrap() {
        AlbumAdmission::Resolve(reservation) => reservation,
        AlbumAdmission::Replay(_) => panic!("expected fresh reservation"),
    }
}

fn error_code<T>(result: PResult<T>) -> &'static str {
    match result {
        Ok(_) => panic!("expected rejected admission"),
        Err(error) => error.code,
    }
}

fn replace(owner: &PlaybackSession, command_id: String) -> PResult<ApplyResult> {
    let snapshot = owner.snapshot().unwrap();
    owner.apply(ApplySessionParams {
        schema_version: SCHEMA_VERSION,
        instance_id: snapshot.instance_id,
        session_id: snapshot.session_id,
        command_id,
        expected_queue_revision: snapshot.queue_revision,
        operation: SessionOperation::ReplaceQueue {
            sources: sources(2),
        },
    })
}

#[test]
fn frozen_album_gain_follows_members_but_not_appended_occurrences() {
    let owner = Owner::new();
    let initial = owner.0.snapshot().unwrap();
    let reservation = resolve(&owner.0, request(&initial));
    let policy = crate::playback::loudness::AlbumLoudnessPolicy {
        version: crate::playback::loudness::ALBUM_LOUDNESS_POLICY_VERSION,
        scalar_bits: 0.75f32.to_bits(),
        gain_db_bits: Some(0.0f64.to_bits()),
        peak_bits: Some((crate::playback::loudness::SAMPLE_PEAK_CEILING / 0.75).to_bits()),
        reason: crate::playback::loudness::AlbumLoudnessReason::OpenSubsonicReplayGain,
    };
    let committed = owner
        .0
        .commit_album_with_policy(reservation, sources(2), policy)
        .unwrap();
    assert_eq!(f32::from_bits(committed.gain_bits), 0.75);
    let originals = committed.occurrences.clone();

    let appended = owner
        .0
        .apply(ApplySessionParams {
            schema_version: SCHEMA_VERSION,
            instance_id: committed.instance_id.clone(),
            session_id: committed.session_id.clone(),
            command_id: Uuid::new_v4().to_string(),
            expected_queue_revision: committed.queue_revision.clone(),
            operation: SessionOperation::AppendQueue {
                sources: vec![TrackSource {
                    server_id: "other".into(),
                    track_id: "appended".into(),
                }],
            },
        })
        .unwrap()
        .assigned_occurrences
        .pop()
        .unwrap();
    let after_append = owner.0.snapshot().unwrap();
    owner
        .0
        .apply(ApplySessionParams {
            schema_version: SCHEMA_VERSION,
            instance_id: after_append.instance_id,
            session_id: after_append.session_id,
            command_id: Uuid::new_v4().to_string(),
            expected_queue_revision: after_append.queue_revision,
            operation: SessionOperation::SelectCurrent {
                occurrence_id: appended.occurrence_id,
            },
        })
        .unwrap();
    assert_eq!(f32::from_bits(owner.0.snapshot().unwrap().gain_bits), 1.0);

    let selected = owner.0.snapshot().unwrap();
    owner
        .0
        .apply(ApplySessionParams {
            schema_version: SCHEMA_VERSION,
            instance_id: selected.instance_id,
            session_id: selected.session_id,
            command_id: Uuid::new_v4().to_string(),
            expected_queue_revision: selected.queue_revision,
            operation: SessionOperation::SelectCurrent {
                occurrence_id: originals[1].occurrence_id.clone(),
            },
        })
        .unwrap();
    assert_eq!(f32::from_bits(owner.0.snapshot().unwrap().gain_bits), 0.75);
}

#[test]
fn transport_and_replacement_supersede_album_resolution_atomically() {
    for action in [
        Some(ControlAction::Pause),
        Some(ControlAction::Stop),
        Some(ControlAction::Next),
        None,
    ] {
        let owner = Owner::new();
        replace(&owner.0, Uuid::new_v4().to_string()).unwrap();
        let before = owner.0.snapshot().unwrap();
        let reservation = resolve(&owner.0, request(&before));
        if let Some(action) = action {
            owner
                .0
                .control_with_guard(
                    ControlParams {
                        schema_version: SCHEMA_VERSION,
                        instance_id: before.instance_id,
                        session_id: before.session_id,
                        command_id: Uuid::new_v4().to_string(),
                        expected_generation_id: before.generation_id,
                        occurrence_id: before.current.unwrap().occurrence_id,
                        action,
                    },
                    None,
                )
                .unwrap();
        } else {
            replace(&owner.0, Uuid::new_v4().to_string()).unwrap();
        }
        let superseding = owner.0.snapshot().unwrap();
        assert_eq!(
            error_code(owner.0.commit_album(reservation, sources(3))),
            "GENERATION_CONFLICT"
        );
        let after = owner.0.snapshot().unwrap();
        assert_eq!(after.queue_revision, superseding.queue_revision);
        assert_eq!(after.generation_id, superseding.generation_id);
        assert_eq!(
            after.current.unwrap().occurrence_id,
            superseding.current.unwrap().occurrence_id
        );
        assert_eq!(after.total_occurrence_count, 2);
    }
}

#[test]
fn successful_receipt_replays_without_replacement_and_rejects_changed_payload() {
    let owner = Owner::new();
    let params = request(&owner.0.snapshot().unwrap());
    let reservation = resolve(&owner.0, params.clone());
    let committed = owner.0.commit_album(reservation, sources(2)).unwrap();
    match owner.0.reserve_album(params.clone(), None).unwrap() {
        AlbumAdmission::Replay(replay) => {
            assert_eq!(replay.queue_revision, committed.queue_revision);
            assert_eq!(replay.generation_id, committed.generation_id);
            assert!(!replay.resume_audio);
        }
        AlbumAdmission::Resolve(_) => panic!("receipt must replay"),
    }
    let mut changed = params.clone();
    changed.source.album_id = "another-album".into();
    assert_eq!(
        error_code(owner.0.reserve_album(changed, None)),
        "COMMAND_ID_REUSED"
    );
    assert_eq!(
        error_code(replace(&owner.0, params.command_id)),
        "COMMAND_ID_REUSED"
    );
}

#[test]
fn pending_and_other_operation_command_ids_cannot_be_reused() {
    let owner = Owner::new();
    let reused = Uuid::new_v4().to_string();
    replace(&owner.0, reused.clone()).unwrap();
    let mut params = request(&owner.0.snapshot().unwrap());
    params.command_id = reused;
    assert_eq!(
        error_code(owner.0.reserve_album(params, None)),
        "COMMAND_ID_REUSED"
    );
    let params = request(&owner.0.snapshot().unwrap());
    let reservation = resolve(&owner.0, params.clone());
    assert_eq!(
        error_code(replace(&owner.0, params.command_id.clone())),
        "COMMAND_ID_REUSED"
    );
    let mut changed = params.clone();
    changed.source.album_id = "different".into();
    assert_eq!(
        error_code(owner.0.reserve_album(changed, None)),
        "COMMAND_ID_REUSED"
    );
    assert_eq!(
        error_code(owner.0.reserve_album(params, None)),
        "PLAYBACK_BUSY"
    );
    drop(reservation);
}

#[test]
fn dropping_reservation_releases_busy_slot() {
    let owner = Owner::new();
    let params = request(&owner.0.snapshot().unwrap());
    let reservation = resolve(&owner.0, params.clone());
    assert_eq!(
        error_code(owner.0.reserve_album(params.clone(), None)),
        "PLAYBACK_BUSY"
    );
    drop(reservation);
    let replacement = resolve(&owner.0, params);
    owner.0.commit_album(replacement, sources(1)).unwrap();
}

#[test]
fn fresh_album_reservation_supersedes_pending_same_or_different_album() {
    for replacement_album in ["album", "different-album"] {
        let owner = Owner::new();
        let snapshot = owner.0.snapshot().unwrap();
        let first_params = request(&snapshot);
        let first = resolve(&owner.0, first_params);
        let mut replacement_params = request(&snapshot);
        replacement_params.source.album_id = replacement_album.into();

        let replacement = resolve(&owner.0, replacement_params);

        assert!(first.is_cancelled());
        assert!(first.is_superseded());
        assert_eq!(
            error_code(owner.0.commit_album(first, sources(1))),
            "ALBUM_SUPERSEDED"
        );
        let committed = owner.0.commit_album(replacement, sources(2)).unwrap();
        assert_eq!(committed.total_occurrence_count, 2);
    }
}

#[test]
fn stale_replacement_does_not_cancel_valid_pending_reservation() {
    let owner = Owner::new();
    let snapshot = owner.0.snapshot().unwrap();
    let first = resolve(&owner.0, request(&snapshot));
    let mut stale = request(&snapshot);
    stale.expected_queue_revision = "999".into();

    assert_eq!(
        error_code(owner.0.reserve_album(stale, None)),
        "GENERATION_CONFLICT"
    );
    assert!(!first.is_cancelled());
    owner.0.commit_album(first, sources(1)).unwrap();
}

#[test]
fn expired_deadline_cancels_commit_without_waiting() {
    let owner = Owner::new();
    let before = owner.0.snapshot().unwrap();
    let reservation = resolve(&owner.0, request(&before));
    {
        let mut inner = owner.0.inner.lock().unwrap();
        inner.album.pending.as_mut().unwrap().deadline = Instant::now() - Duration::from_secs(1);
    }
    assert_eq!(
        error_code(owner.0.commit_album(reservation, sources(1))),
        "GENERATION_CONFLICT"
    );
    assert_eq!(
        owner.0.snapshot().unwrap().queue_revision,
        before.queue_revision
    );
    drop(resolve(&owner.0, request(&owner.0.snapshot().unwrap())));
}

#[test]
fn expired_receipt_is_pruned_before_admission_without_waiting() {
    let owner = Owner::new();
    let params = request(&owner.0.snapshot().unwrap());
    owner
        .0
        .commit_album(resolve(&owner.0, params.clone()), sources(1))
        .unwrap();
    {
        let mut inner = owner.0.inner.lock().unwrap();
        inner.album.receipts.get_mut(&params.command_id).unwrap().0 =
            Instant::now() - RECEIPT_TTL - Duration::from_secs(1);
    }
    assert_eq!(
        error_code(owner.0.reserve_album(params, None)),
        "GENERATION_CONFLICT"
    );
}

#[test]
fn full_album_commit_keeps_rows_beyond_snapshot_page() {
    let owner = Owner::new();
    let reservation = resolve(&owner.0, request(&owner.0.snapshot().unwrap()));
    let committed = owner.0.commit_album(reservation, sources(201)).unwrap();
    assert_eq!(committed.total_occurrence_count, 201);
    assert_eq!(committed.occurrences.len(), DEFAULT_PAGE_SIZE);
    assert!(committed.playback.can_go_next);
    let mut rows = committed.occurrences;
    let mut cursor = committed.next_cursor;
    while cursor.is_some() {
        let page = owner
            .0
            .list(ListOccurrencesParams {
                schema_version: SCHEMA_VERSION,
                session_id: committed.session_id.clone(),
                expected_queue_revision: committed.queue_revision.clone(),
                cursor,
                limit: None,
            })
            .unwrap();
        rows.extend(page.occurrences);
        cursor = page.next_cursor;
    }
    assert_eq!(rows.len(), 201);
    for (ordinal, row) in rows.iter().enumerate() {
        assert_eq!(row.ordinal, ordinal as u64);
        assert_eq!(row.source.track_id, format!("track-{ordinal}"));
    }
}

#[test]
fn public_apply_wire_rejects_internal_play_album_operation() {
    let owner = Owner::new();
    let snapshot = owner.0.snapshot().unwrap();
    let wire = serde_json::json!({
        "schemaVersion": SCHEMA_VERSION, "instanceId": snapshot.instance_id,
        "sessionId": snapshot.session_id, "commandId": Uuid::new_v4().to_string(),
        "expectedQueueRevision": snapshot.queue_revision,
        "operation": { "type": "playAlbum", "sources": [{ "serverId": "server", "trackId": "track" }] }
    });
    assert!(serde_json::from_value::<ApplySessionParams>(wire).is_err());
}

#[tokio::test]
async fn abandoned_reserve_reply_releases_its_mutation_guard() {
    let owner = Owner::new();
    let params = request(&owner.0.snapshot().unwrap());
    let operations = crate::sync::SyncOperationManager::new();
    let guard = operations.try_admit_mutation().unwrap();
    let (reply, receiver) = mpsc::channel();
    drop(receiver);
    owner
        .0
        .command_tx
        .try_send(OwnerCommand::ReserveAlbum(params, Some(guard), reply))
        .unwrap();
    operations.begin_shutdown_fence();
    // Commands are serialized: the snapshot follows the abandoned reserve reply
    // and the owner's next maintenance pass, without relying on a sleep.
    owner.0.snapshot().unwrap();
    assert_eq!(
        operations
            .shutdown_snapshot()
            .await
            .unwrap()
            .pending_mutation_count,
        0
    );
    assert!(owner.0.inner.lock().unwrap().album.pending.is_none());
}

#[tokio::test]
async fn reservation_expiry_releases_mutation_guard() {
    let owner = Owner::new();
    let operations = crate::sync::SyncOperationManager::new();
    let guard = operations.try_admit_mutation().unwrap();
    let reservation = match owner
        .0
        .reserve_album(request(&owner.0.snapshot().unwrap()), Some(guard))
        .unwrap()
    {
        AlbumAdmission::Resolve(reservation) => reservation,
        AlbumAdmission::Replay(_) => panic!("expected fresh reservation"),
    };
    assert_eq!(operations.begin_shutdown_fence().pending_mutation_count, 1);
    {
        let mut inner = owner.0.inner.lock().unwrap();
        inner.album.pending.as_mut().unwrap().deadline = Instant::now() - Duration::from_secs(1);
    }
    assert_eq!(
        error_code(owner.0.commit_album(reservation, sources(1))),
        "GENERATION_CONFLICT"
    );
    assert_eq!(
        operations
            .shutdown_snapshot()
            .await
            .unwrap()
            .pending_mutation_count,
        0
    );
}

#[tokio::test]
async fn shutdown_cancels_pending_reservation_and_releases_mutation_guard() {
    let owner = Owner::new();
    let operations = crate::sync::SyncOperationManager::new();
    let guard = operations.try_admit_mutation().unwrap();
    let reservation = match owner
        .0
        .reserve_album(request(&owner.0.snapshot().unwrap()), Some(guard))
        .unwrap()
    {
        AlbumAdmission::Resolve(reservation) => reservation,
        AlbumAdmission::Replay(_) => panic!("expected fresh reservation"),
    };
    assert_eq!(operations.begin_shutdown_fence().pending_mutation_count, 1);
    owner.0.stop_and_join().unwrap();
    assert!(reservation.is_cancelled());
    assert_eq!(
        operations
            .shutdown_snapshot()
            .await
            .unwrap()
            .pending_mutation_count,
        0
    );
    assert!(owner.0.inner.lock().unwrap().album.pending.is_none());
}
