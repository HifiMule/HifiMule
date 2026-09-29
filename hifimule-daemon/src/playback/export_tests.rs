use super::*;
use crate::playback::feedback::FeedbackTarget;
use crate::providers::feedback::Preference;

fn fixture(db: &Database, n: usize, current: usize) -> (SaveSnapshotParams, Vec<String>) {
    db.init_playback().unwrap();
    let session = Uuid::new_v4().to_string();
    let ids: Vec<_> = (0..n).map(|_| Uuid::new_v4().to_string()).collect();
    let mut conn = db.conn.lock().unwrap();
    let tx = conn.transaction().unwrap();
    tx.execute("INSERT INTO playback_sessions(singleton_id,session_id,queue_revision,checkpoint_sequence,transport_state,current_occurrence_id,position_ms) VALUES(1,?1,0,0,'paused',?2,0)", params![session,ids.get(current)]).unwrap();
    for (i, id) in ids.iter().enumerate() {
        tx.execute("INSERT INTO playback_occurrences(session_id,occurrence_id,ordinal,server_id,track_id) VALUES(?1,?2,?3,?4,?5)",params![session,id,i as i64,if i%2==0 {"source-a"} else {"source-b"},format!("track-{}",i%3)]).unwrap();
    }
    tx.commit().unwrap();
    (
        SaveSnapshotParams {
            schema_version: 1,
            operation_id: Uuid::new_v4().to_string(),
            instance_id: Uuid::new_v4().to_string(),
            session_id: session,
            expected_queue_revision: "0".into(),
            expected_main_occurrence_id: ids.get(current).cloned(),
            name: None,
        },
        ids,
    )
}
fn attempt(db: &Database, p: &SaveSnapshotParams, id: &str, disposition: &str) {
    db.conn.lock().unwrap().execute("INSERT INTO playback_attempts(attempt_id,session_id,occurrence_id,server_id,track_id,disposition) SELECT ?1,session_id,occurrence_id,server_id,track_id,?3 FROM playback_occurrences WHERE occurrence_id=?2 AND session_id=?4",params![Uuid::new_v4().to_string(),id,disposition,p.session_id]).unwrap();
}
fn capture(db: &Database, p: &SaveSnapshotParams) -> ListeningSnapshotSummary {
    match db.capture_listening_snapshot(p, None, None).unwrap() {
        SaveSnapshotResult::Saved { snapshot, .. } => *snapshot,
        other => panic!("{other:?}"),
    }
}
fn entries(db: &Database, id: &str, cursor: Option<String>, limit: usize) -> SnapshotEntryPage {
    db.list_listening_snapshot_entries(ListSnapshotEntriesParams {
        schema_version: 1,
        snapshot_id: id.into(),
        cursor,
        limit: Some(limit),
    })
    .unwrap()
}
fn saved_ids(db: &Database, id: &str) -> Vec<String> {
    entries(db, id, None, 200)
        .entries
        .into_iter()
        .map(|e| e.occurrence_id)
        .collect()
}
fn feedback(db: &Database, p: &SaveSnapshotParams, id: &str, value: Preference) {
    db.record_feedback_disposition(
        &FeedbackTarget {
            session_id: p.session_id.clone(),
            logical_session_id: p.session_id.clone(),
            occurrence_id: id.into(),
            source: TrackSource {
                server_id: "source-a".into(),
                track_id: "track-0".into(),
            },
        },
        value,
    )
    .unwrap();
}

#[test]
fn exact_policy_preserves_repeats_failed_listens_and_local_disposition() {
    let db = Database::memory().unwrap();
    let (mut p, ids) = fixture(&db, 8, 4);
    attempt(&db, &p, &ids[0], "naturalCompletion");
    attempt(&db, &p, &ids[1], "naturalCompletion");
    attempt(&db, &p, &ids[1], "explicitSkip"); // later replay skip overrides first completion
    attempt(&db, &p, &ids[2], "technicalFailure");
    // ids[3] was jumped over without an attempt.
    attempt(&db, &p, &ids[4], "naturalCompletion"); // final/current included only once
    feedback(&db, &p, &ids[5], Preference::Dislike);
    feedback(&db, &p, &ids[5], Preference::Neutral);
    feedback(&db, &p, &ids[1], Preference::Like); // Like cannot clear a skip
    let first = capture(&db, &p);
    assert_eq!(
        saved_ids(&db, &first.snapshot_id),
        [0, 2, 4, 6, 7].map(|i| ids[i].clone())
    );
    feedback(&db, &p, &ids[5], Preference::Like);
    p.operation_id = Uuid::new_v4().to_string();
    let second = capture(&db, &p);
    assert_eq!(
        saved_ids(&db, &second.snapshot_id),
        [0, 2, 4, 5, 6, 7].map(|i| ids[i].clone())
    );
    assert_eq!(
        saved_ids(&db, &first.snapshot_id),
        [0, 2, 4, 6, 7].map(|i| ids[i].clone())
    );
    let page = entries(&db, &second.snapshot_id, None, 200);
    assert_eq!(page.entries[0].source, page.entries[4].source); // deliberate identical source/track
    assert!(page.entries.iter().all(|e| !e.source_available));
}

#[test]
fn first_visit_history_and_back_reordering_use_distinct_boundaries() {
    let db = Database::memory().unwrap();
    let (mut p, ids) = fixture(&db, 4, 3);
    for i in [1, 0, 1, 2] {
        attempt(&db, &p, &ids[i], "backNavigation");
    }
    let s = capture(&db, &p);
    assert_eq!(
        saved_ids(&db, &s.snapshot_id),
        [1, 0, 2, 3].map(|i| ids[i].clone())
    );
    let conn = db.conn.lock().unwrap();
    conn.execute(
        "UPDATE playback_sessions SET current_occurrence_id=?1",
        [&ids[0]],
    )
    .unwrap();
    conn.execute("UPDATE playback_occurrences SET ordinal=ordinal+10", [])
        .unwrap();
    for (ord, i) in [0, 2, 1, 3].iter().enumerate() {
        conn.execute(
            "UPDATE playback_occurrences SET ordinal=?1 WHERE occurrence_id=?2",
            params![ord as i64, ids[*i]],
        )
        .unwrap();
    }
    drop(conn);
    p.expected_main_occurrence_id = Some(ids[0].clone());
    p.operation_id = Uuid::new_v4().to_string();
    assert_eq!(
        saved_ids(&db, &capture(&db, &p).snapshot_id),
        [0, 2, 1, 3].map(|i| ids[i].clone())
    );
}

#[test]
fn rollback_is_atomic_and_success_identity_survives_replacement_restart_and_source_removal() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("saved.db");
    let db = Database::new(path.clone()).unwrap();
    let (mut p, ids) = fixture(&db, 3, 0);
    let first = capture(&db, &p);
    assert_eq!(capture(&db, &p), first);
    let mut changed = p.clone();
    changed.name = Some("changed".into());
    assert_eq!(
        db.capture_listening_snapshot(&changed, None, None)
            .unwrap_err()
            .code,
        "SNAPSHOT_OPERATION_REUSED"
    );
    for table in [
        "playback_listening_snapshots",
        "playback_listening_snapshot_entries",
    ] {
        db.conn.lock().unwrap().execute_batch(&format!("CREATE TRIGGER fail_save BEFORE INSERT ON {table} BEGIN SELECT RAISE(ABORT,'injected'); END;")).unwrap();
        p.operation_id = Uuid::new_v4().to_string();
        assert_eq!(
            db.capture_listening_snapshot(&p, None, None)
                .unwrap_err()
                .code,
            "SNAPSHOT_STORAGE_FAILED"
        );
        assert!(db.recover_listening_snapshot(&p).unwrap().is_none());
        assert_eq!(saved_ids(&db, &first.snapshot_id), ids);
        db.conn
            .lock()
            .unwrap()
            .execute_batch("DROP TRIGGER fail_save;")
            .unwrap();
    }
    db.conn.lock().unwrap().execute_batch("DELETE FROM playback_occurrences; DELETE FROM playback_sessions; DELETE FROM server_config;").unwrap();
    drop(db);
    let reopened = Database::new(path).unwrap();
    reopened.init_playback().unwrap();
    assert_eq!(saved_ids(&reopened, &first.snapshot_id), ids);
    assert_eq!(
        reopened
            .get_listening_snapshot(GetSnapshotParams {
                schema_version: 1,
                snapshot_id: None,
                operation_id: Some(first.operation_id.clone())
            })
            .unwrap(),
        first
    );
}

#[test]
fn bounded_pages_validate_cursors_and_list_insertion_keeps_older_boundary() {
    let db = Database::memory().unwrap();
    let (mut p, ids) = fixture(&db, 10_001, 0);
    let first = capture(&db, &p);
    let mut cursor = None;
    let mut offset = 0;
    loop {
        let page = entries(&db, &first.snapshot_id, cursor, 200);
        assert!(page.entries.len() <= 200);
        assert_eq!(page.total_count, "10001");
        for e in &page.entries {
            assert_eq!(e.occurrence_id, ids[offset]);
            assert_eq!(e.ordinal, offset.to_string());
            offset += 1;
        }
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(offset, ids.len());
    p.operation_id = Uuid::new_v4().to_string();
    let second = capture(&db, &p);
    let list = db
        .list_listening_snapshots(ListSnapshotsParams {
            schema_version: 1,
            cursor: None,
            limit: Some(1),
        })
        .unwrap();
    assert_eq!(list.snapshots[0], second);
    p.operation_id = Uuid::new_v4().to_string();
    capture(&db, &p);
    let next = db
        .list_listening_snapshots(ListSnapshotsParams {
            schema_version: 1,
            cursor: list.next_cursor,
            limit: Some(1),
        })
        .unwrap();
    assert_eq!(next.snapshots[0], first);
    assert!(next.next_cursor.is_none());
    for cursor in [
        "bogus".into(),
        format!("2:entries:{}:1", first.snapshot_id),
        format!("1:entries:{}:1", second.snapshot_id),
        format!("1:entries:{}:9223372036854775808", first.snapshot_id),
    ] {
        assert_eq!(
            db.list_listening_snapshot_entries(ListSnapshotEntriesParams {
                schema_version: 1,
                snapshot_id: first.snapshot_id.clone(),
                cursor: Some(cursor),
                limit: Some(50)
            })
            .unwrap_err()
            .code,
            "INVALID_SNAPSHOT_CURSOR"
        );
    }
}

#[test]
fn empty_integrity_versions_names_and_missing_locators_are_explicit() {
    let db = Database::memory().unwrap();
    let (mut p, ids) = fixture(&db, 1, 0);
    feedback(&db, &p, &ids[0], Preference::Dislike);
    assert!(matches!(
        db.capture_listening_snapshot(&p, None, None).unwrap(),
        SaveSnapshotResult::Empty {
            reason: "noEligibleOccurrences",
            ..
        }
    ));
    assert!(db.recover_listening_snapshot(&p).unwrap().is_none());
    feedback(&db, &p, &ids[0], Preference::Like);
    p.name = Some("  café 🎵  ".into());
    let s = capture(&db, &p);
    assert_eq!(s.name, "café 🎵");
    for name in ["bad\nname".into(), "🎵".repeat(121)] {
        p.name = Some(name);
        assert_eq!(p.validate().unwrap_err().code, "INVALID_SNAPSHOT_NAME");
    }
    db.conn
        .lock()
        .unwrap()
        .execute(
            "UPDATE playback_listening_snapshot_entries SET ordinal=4",
            [],
        )
        .unwrap();
    let get = || {
        db.get_listening_snapshot(GetSnapshotParams {
            schema_version: 1,
            snapshot_id: Some(s.snapshot_id.clone()),
            operation_id: None,
        })
    };
    assert_eq!(get().unwrap_err().code, "SNAPSHOT_CORRUPT");
    db.conn.lock().unwrap().execute_batch("UPDATE playback_listening_snapshot_entries SET ordinal=0; PRAGMA ignore_check_constraints=ON; UPDATE playback_listening_snapshots SET policy_version=2;").unwrap();
    assert_eq!(get().unwrap_err().code, "UNSUPPORTED_SNAPSHOT_VERSION");
    db.conn.lock().unwrap().execute_batch("DELETE FROM playback_occurrences; UPDATE playback_sessions SET current_occurrence_id=NULL;").unwrap();
    p.name = None;
    p.operation_id = Uuid::new_v4().to_string();
    p.expected_main_occurrence_id = None;
    assert!(matches!(
        db.capture_listening_snapshot(&p, None, None).unwrap(),
        SaveSnapshotResult::Empty {
            reason: "noMainSelection",
            ..
        }
    ));
}

#[test]
fn snapshot_display_freezes_safe_metadata_and_survives_source_deletion() {
    let db = Database::memory().unwrap();
    let (p, ids) = fixture(&db, 2, 0);
    db.conn.lock().unwrap().execute("INSERT INTO server_config(id,url,server_type,username,name,icon,updated_at,selected,server_id) VALUES('local','https://secret.example/token','jellyfin','private','Music room','music-note',0,1,'source-a')",[]).unwrap();
    let meta = PlaybackTrackMetadata {
        source: TrackSource {
            server_id: "source-a".into(),
            track_id: "track-0".into(),
        },
        title: "Saved title".into(),
        artist: Some("Artist".into()),
        album: Some("Album".into()),
    };
    let SaveSnapshotResult::Saved { snapshot, .. } = db
        .capture_listening_snapshot(&p, Some(&meta), Some(12345))
        .unwrap()
    else {
        panic!()
    };
    let before = entries(&db, &snapshot.snapshot_id, None, 50);
    assert_eq!(before.entries[0].title.as_deref(), Some("Saved title"));
    assert!(before.entries[0].source_available);
    db.conn.lock().unwrap().execute_batch("DELETE FROM server_config; DELETE FROM playback_occurrences; DELETE FROM playback_sessions;").unwrap();
    let after = entries(&db, &snapshot.snapshot_id, None, 50);
    assert_eq!(after.entries[0].source_label, "Music room");
    assert_eq!(after.entries[0].duration_ms, Some(12345));
    assert!(!after.entries[0].source_available);
    assert_eq!(saved_ids(&db, &snapshot.snapshot_id), ids);
    let json = serde_json::to_string(&after).unwrap();
    assert!(!json.contains("secret.example"));
    assert!(!json.contains("private"));
}

#[test]
fn snapshot_process_interruption_rolls_back_actual_capture_and_retry_is_complete() {
    use std::process::{Command, Stdio};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("snapshot.db");
    let ready = dir.path().join("ready");
    let db = Database::new(path.clone()).unwrap();
    let (mut p, ids) = fixture(&db, 3, 0);
    let earlier = capture(&db, &p);
    p.operation_id = Uuid::new_v4().to_string();
    drop(db);
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "playback::export::tests::snapshot_open_transaction_child",
            "--nocapture",
        ])
        .env("HIFIMULE_SNAPSHOT_CRASH_DB", &path)
        .env(
            "HIFIMULE_SNAPSHOT_CRASH_REQUEST",
            serde_json::to_string(&p).unwrap(),
        )
        .env("HIFIMULE_SNAPSHOT_CRASH_READY", &ready)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !ready.exists() {
        if std::time::Instant::now() > deadline {
            let _ = child.kill();
            panic!("capture did not reach commit barrier");
        }
        assert!(child.try_wait().unwrap().is_none());
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    child.kill().unwrap();
    child.wait().unwrap();
    let reopened = Database::new(path).unwrap();
    reopened.init_playback().unwrap();
    assert!(reopened.recover_listening_snapshot(&p).unwrap().is_none());
    assert_eq!(saved_ids(&reopened, &earlier.snapshot_id), ids);
    assert_eq!(
        saved_ids(&reopened, &capture(&reopened, &p).snapshot_id),
        ids
    );
}

#[test]
fn snapshot_open_transaction_child() {
    let Ok(path) = std::env::var("HIFIMULE_SNAPSHOT_CRASH_DB") else {
        return;
    };
    let p: SaveSnapshotParams =
        serde_json::from_str(&std::env::var("HIFIMULE_SNAPSHOT_CRASH_REQUEST").unwrap()).unwrap();
    let db = Database::new(path.into()).unwrap();
    db.init_playback().unwrap();
    capture(&db, &p);
}
