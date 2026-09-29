use super::super::export::{SaveSnapshotParams, SaveSnapshotResult};
use super::*;
#[cfg(test)]
pub(crate) static LAST_OWNER_HOLD_US: AtomicU64 = AtomicU64::new(0);

/// Admission belongs to the actual owner command, even if its caller vanishes.
pub(super) struct SavePermit(Arc<AtomicBool>);
impl Drop for SavePermit {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
impl PlaybackSession {
    pub fn save_snapshot(
        &self,
        p: SaveSnapshotParams,
        guard: Option<crate::sync::MutationGuard>,
    ) -> PResult<SaveSnapshotResult> {
        p.validate()?;
        if self.fenced.load(Ordering::Acquire) {
            return Err(owner_stopped());
        }
        self.snapshot_save_gate
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| super::super::export::error("PLAYBACK_BUSY"))?;
        let permit = SavePermit(self.snapshot_save_gate.clone());
        let (tx, rx) = mpsc::channel();
        self.command_tx
            .try_send(OwnerCommand::SaveSnapshot(p, guard, permit, tx))
            .map_err(admission_error)?;
        rx.recv().unwrap_or_else(|_| Err(owner_stopped()))
    }
}

pub(super) fn capture(i: &Inner, p: &SaveSnapshotParams) -> PResult<SaveSnapshotResult> {
    use super::super::export::error;
    // A committed operation is authoritative even after live identity changes.
    #[cfg(test)]
    let held_at = Instant::now();
    if let Some(snapshot) = i.db.recover_listening_snapshot(p)? {
        return Ok(SaveSnapshotResult::Saved {
            schema_version: 1,
            snapshot: Box::new(snapshot),
        });
    }
    if p.instance_id != i.instance_id {
        return Err(error("STALE_INSTANCE"));
    }
    if p.session_id != i.session.session_id {
        return Err(error("STALE_SESSION"));
    }
    if p.expected_queue_revision != i.session.queue_revision.to_string() {
        return Err(error("QUEUE_CONFLICT"));
    }
    if p.expected_main_occurrence_id != i.session.current_occurrence_id {
        return Err(error("STALE_MAIN_OCCURRENCE"));
    }
    if i.restoration.status == "error" {
        return Err(error("RESTORE_FAILED"));
    }
    if i.pending_terminal.is_some() {
        return Err(error("TERMINAL_PENDING"));
    }
    // The currently displayed metadata belongs to the audition in Preview.
    let metadata = if i.preview.is_none() {
        i.playback.metadata.as_ref()
    } else {
        None
    };
    let result =
        i.db.capture_listening_snapshot(p, metadata, i.playback.duration_ms);
    #[cfg(test)]
    LAST_OWNER_HOLD_US.store(held_at.elapsed().as_micros() as u64, Ordering::Release);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playback::export::ListSnapshotEntriesParams;
    fn request(s: &SessionSnapshot) -> SaveSnapshotParams {
        SaveSnapshotParams {
            schema_version: 1,
            operation_id: Uuid::new_v4().to_string(),
            instance_id: s.instance_id.clone(),
            session_id: s.session_id.clone(),
            expected_queue_revision: s.queue_revision.clone(),
            expected_main_occurrence_id: s.main_current.as_ref().map(|o| o.occurrence_id.clone()),
            name: Some("My session".into()),
        }
    }
    fn setup() -> (Arc<Database>, PlaybackSession, SessionSnapshot) {
        let db = Arc::new(Database::memory().unwrap());
        let owner = PlaybackSession::restore(db.clone(), Uuid::new_v4().to_string());
        let s = owner.snapshot().unwrap();
        owner
            .apply(ApplySessionParams {
                schema_version: 1,
                instance_id: s.instance_id,
                session_id: s.session_id,
                expected_queue_revision: s.queue_revision,
                command_id: Uuid::new_v4().to_string(),
                operation: SessionOperation::ReplaceQueue {
                    sources: (0..3)
                        .map(|n| TrackSource {
                            server_id: "offline".into(),
                            track_id: format!("track{n}"),
                        })
                        .collect(),
                },
            })
            .unwrap();
        let s = owner.snapshot().unwrap();
        (db, owner, s)
    }
    #[test]
    fn snapshot_owner_preserves_transport_and_recovers_after_replacement_and_restart() {
        let (db, owner, s) = setup();
        let p = request(&s);
        let first = owner.save_snapshot(p.clone(), None).unwrap();
        let after = owner.snapshot().unwrap();
        assert_eq!(
            serde_json::to_value(&s).unwrap(),
            serde_json::to_value(after).unwrap()
        );
        owner
            .apply(ApplySessionParams {
                schema_version: 1,
                instance_id: s.instance_id,
                session_id: s.session_id,
                expected_queue_revision: s.queue_revision,
                command_id: Uuid::new_v4().to_string(),
                operation: SessionOperation::Clear,
            })
            .unwrap();
        assert_eq!(owner.save_snapshot(p.clone(), None).unwrap(), first);
        let mut reused = p.clone();
        reused.name = Some("Different".into());
        assert_eq!(
            owner.save_snapshot(reused, None).unwrap_err().code,
            "SNAPSHOT_OPERATION_REUSED"
        );
        owner.stop_and_join().unwrap();
        let restored = PlaybackSession::restore(db, Uuid::new_v4().to_string());
        assert_eq!(restored.save_snapshot(p, None).unwrap(), first);
        restored.stop_and_join().unwrap();
    }
    #[test]
    fn snapshot_preview_captures_only_main_and_excludes_audition_metadata() {
        let (db, owner, s) = setup();
        let p = request(&s);
        let preview = owner
            .preview_with_guard(
                PreviewTrackParams {
                    schema_version: 1,
                    instance_id: s.instance_id.clone(),
                    session_id: s.session_id.clone(),
                    command_id: Uuid::new_v4().to_string(),
                    expected_queue_revision: s.queue_revision.clone(),
                    expected_generation_id: s.generation_id.clone(),
                    source: s.current.as_ref().unwrap().source.clone(),
                },
                None,
            )
            .unwrap();
        {
            let mut i = owner.inner.lock().unwrap();
            i.playback.metadata = Some(PlaybackTrackMetadata {
                source: preview.current.as_ref().unwrap().source.clone(),
                title: "Audition must not leak".into(),
                artist: None,
                album: None,
            });
        }
        let SaveSnapshotResult::Saved { snapshot, .. } = owner.save_snapshot(p, None).unwrap()
        else {
            panic!()
        };
        let page = db
            .list_listening_snapshot_entries(ListSnapshotEntriesParams {
                schema_version: 1,
                snapshot_id: snapshot.snapshot_id,
                cursor: None,
                limit: None,
            })
            .unwrap();
        assert_eq!(
            page.entries
                .iter()
                .map(|e| e.occurrence_id.clone())
                .collect::<Vec<_>>(),
            s.occurrences
                .iter()
                .map(|o| o.occurrence_id.clone())
                .collect::<Vec<_>>()
        );
        assert!(page.entries.iter().all(|e| e.title.is_none()));
        assert_eq!(owner.snapshot().unwrap().current, preview.current);
        owner.stop_and_join().unwrap();
    }
    #[test]
    fn snapshot_admission_busy_and_stale_requests_do_not_save() {
        let (db, owner, s) = setup();
        let mut p = request(&s);
        owner.snapshot_save_gate.store(true, Ordering::Release);
        assert_eq!(
            owner.save_snapshot(p.clone(), None).unwrap_err().code,
            "PLAYBACK_BUSY"
        );
        owner.snapshot_save_gate.store(false, Ordering::Release);
        p.expected_main_occurrence_id = Some(Uuid::new_v4().to_string());
        assert_eq!(
            owner.save_snapshot(p.clone(), None).unwrap_err().code,
            "STALE_MAIN_OCCURRENCE"
        );
        p = request(&s);
        p.expected_queue_revision = "99".into();
        assert_eq!(
            owner.save_snapshot(p, None).unwrap_err().code,
            "QUEUE_CONFLICT"
        );
        assert!(
            db.list_listening_snapshots(crate::playback::export::ListSnapshotsParams {
                schema_version: 1,
                cursor: None,
                limit: None
            })
            .unwrap()
            .snapshots
            .is_empty()
        );
        owner.stop_and_join().unwrap();
    }

    #[test]
    fn snapshot_preview_without_main_is_empty_and_does_not_stop_audition() {
        let db = Arc::new(Database::memory().unwrap());
        let owner = PlaybackSession::restore(db, Uuid::new_v4().to_string());
        let s = owner.snapshot().unwrap();
        let preview = owner
            .preview_with_guard(
                PreviewTrackParams {
                    schema_version: 1,
                    instance_id: s.instance_id,
                    session_id: s.session_id,
                    command_id: Uuid::new_v4().to_string(),
                    expected_queue_revision: s.queue_revision,
                    expected_generation_id: s.generation_id,
                    source: TrackSource {
                        server_id: "offline".into(),
                        track_id: "audition".into(),
                    },
                },
                None,
            )
            .unwrap();
        assert!(matches!(
            owner.save_snapshot(request(&preview), None).unwrap(),
            SaveSnapshotResult::Empty {
                reason: "noMainSelection",
                ..
            }
        ));
        assert_eq!(owner.snapshot().unwrap().current, preview.current);
        owner.stop_and_join().unwrap();
    }

    #[test]
    fn snapshot_owner_orders_feedback_and_queue_edits_around_capture() {
        use crate::playback::feedback::{FeedbackQuery, FeedbackWrite};
        use crate::providers::feedback::{FeedbackCapabilities, Preference, ProviderFeedback};
        let (db, owner, s) = setup();
        let target = owner
            .feedback_target(FeedbackQuery {
                schema_version: 1,
                expected_session_id: s.session_id.clone(),
                occurrence_id: s.current.as_ref().unwrap().occurrence_id.clone(),
            })
            .unwrap();
        let verified = ProviderFeedback {
            capabilities: FeedbackCapabilities::ratings(),
            value: Preference::Neutral,
            account_scope: "offline-account".into(),
        };
        let write = |value| FeedbackWrite {
            schema_version: 1,
            expected_session_id: s.session_id.clone(),
            occurrence_id: target.occurrence_id.clone(),
            operation_id: Uuid::new_v4().to_string(),
            value,
        };
        let saved_ids = |p: &SaveSnapshotParams| {
            let header = db.recover_listening_snapshot(p).unwrap().unwrap();
            db.list_listening_snapshot_entries(ListSnapshotEntriesParams {
                schema_version: 1,
                snapshot_id: header.snapshot_id,
                cursor: None,
                limit: None,
            })
            .unwrap()
            .entries
            .into_iter()
            .map(|entry| entry.occurrence_id)
            .collect::<Vec<_>>()
        };
        let expected = s
            .occurrences
            .iter()
            .map(|o| o.occurrence_id.clone())
            .collect::<Vec<_>>();
        // Deterministically enqueue both commands while the owner is blocked.
        let first = request(&s);
        let locked = owner.inner.lock().unwrap();
        let (save_reply, save_rx) = mpsc::channel();
        let (feedback_reply, feedback_rx) = mpsc::channel();
        owner.snapshot_save_gate.store(true, Ordering::Release);
        owner
            .command_tx
            .try_send(OwnerCommand::SaveSnapshot(
                first.clone(),
                None,
                SavePermit(owner.snapshot_save_gate.clone()),
                save_reply,
            ))
            .ok()
            .unwrap();
        owner
            .command_tx
            .try_send(OwnerCommand::AcceptFeedback(
                target.clone(),
                verified.clone(),
                write(Preference::Dislike),
                None,
                feedback_reply,
            ))
            .ok()
            .unwrap();
        drop(locked);
        save_rx.recv().unwrap().unwrap();
        feedback_rx.recv().unwrap().unwrap();
        assert_eq!(saved_ids(&first), expected);
        let rejected = request(&s);
        owner.save_snapshot(rejected.clone(), None).unwrap();
        assert_eq!(saved_ids(&rejected), expected[1..]);
        // Neutral leaves the local rejection intact; Like before capture clears it.
        owner
            .accept_feedback(
                target.clone(),
                verified.clone(),
                write(Preference::Neutral),
                None,
            )
            .unwrap();
        let neutral = request(&s);
        owner.save_snapshot(neutral.clone(), None).unwrap();
        assert_eq!(saved_ids(&neutral), expected[1..]);
        owner
            .accept_feedback(target.clone(), verified, write(Preference::Like), None)
            .unwrap();
        let accepted = request(&s);
        owner.save_snapshot(accepted.clone(), None).unwrap();
        assert_eq!(saved_ids(&accepted), expected);
        // Reordering later cannot mutate any committed sequence.
        owner
            .apply(ApplySessionParams {
                schema_version: 1,
                instance_id: s.instance_id,
                session_id: s.session_id,
                expected_queue_revision: s.queue_revision,
                command_id: Uuid::new_v4().to_string(),
                operation: SessionOperation::MoveUpcoming {
                    occurrence_id: expected[2].clone(),
                    before_occurrence_id: Some(expected[1].clone()),
                },
            })
            .unwrap();
        let changed = request(&owner.snapshot().unwrap());
        owner.save_snapshot(changed.clone(), None).unwrap();
        assert_eq!(
            saved_ids(&changed),
            vec![
                expected[0].clone(),
                expected[2].clone(),
                expected[1].clone()
            ]
        );
        assert_eq!(saved_ids(&accepted), expected);
        owner.stop_and_join().unwrap();
    }

    #[test]
    fn snapshot_album_and_radio_capture_preserve_scope_and_survive_replacement() {
        let (db, owner, mut s) = setup();
        let sources = s
            .occurrences
            .iter()
            .map(|o| o.source.clone())
            .collect::<Vec<_>>();
        for operation in [
            SessionOperation::PlayAlbum {
                sources: sources.clone(),
            },
            SessionOperation::StartRadio {
                source: sources[0].clone(),
                center: None,
                settings: None,
                recording: None,
            },
        ] {
            owner
                .apply(ApplySessionParams {
                    schema_version: 1,
                    instance_id: s.instance_id,
                    session_id: s.session_id,
                    expected_queue_revision: s.queue_revision,
                    command_id: Uuid::new_v4().to_string(),
                    operation,
                })
                .unwrap();
            s = owner.snapshot().unwrap();
            let p = request(&s);
            let SaveSnapshotResult::Saved { snapshot, .. } =
                owner.save_snapshot(p.clone(), None).unwrap()
            else {
                panic!()
            };
            assert_eq!(
                snapshot.logical_session_id,
                s.radio.as_ref().map(|r| r.logical_id.clone())
            );
            assert_eq!(snapshot.entry_count, s.occurrences.len().to_string());
            let page = db
                .list_listening_snapshot_entries(ListSnapshotEntriesParams {
                    schema_version: 1,
                    snapshot_id: snapshot.snapshot_id.clone(),
                    cursor: None,
                    limit: None,
                })
                .unwrap();
            assert_eq!(
                page.entries
                    .iter()
                    .map(|e| &e.occurrence_id)
                    .collect::<Vec<_>>(),
                s.occurrences
                    .iter()
                    .map(|o| &o.occurrence_id)
                    .collect::<Vec<_>>()
            );
            owner
                .apply(ApplySessionParams {
                    schema_version: 1,
                    instance_id: s.instance_id,
                    session_id: s.session_id,
                    expected_queue_revision: s.queue_revision,
                    command_id: Uuid::new_v4().to_string(),
                    operation: SessionOperation::Clear,
                })
                .unwrap();
            s = owner.snapshot().unwrap();
            assert_eq!(
                db.recover_listening_snapshot(&p).unwrap().unwrap(),
                *snapshot
            );
        }
        owner.stop_and_join().unwrap();
    }

    #[test]
    fn snapshot_pending_terminal_and_restore_errors_block_new_capture() {
        let (db, owner, s) = setup();
        let p = request(&s);
        {
            let mut i = owner.inner.lock().unwrap();
            i.pending_terminal = Some(PendingTerminal {
                instance_id: i.instance_id.clone(),
                generation_id: i.generation_id.clone(),
                occurrence_id: s.current.as_ref().unwrap().occurrence_id.clone(),
                departed_position_ms: 100,
                session: i.session.clone(),
                outcome: "naturalCompletion",
                status: PlaybackStatus::Completed,
                failure: None,
                resolve_successor: false,
                preserve_existing_outcome: false,
            });
            assert_eq!(capture(&i, &p).unwrap_err().code, "TERMINAL_PENDING");
            i.pending_terminal = None;
            i.restoration.status = "error".into();
            assert_eq!(capture(&i, &p).unwrap_err().code, "RESTORE_FAILED");
            i.restoration.status = "ok".into();
        }
        assert!(db.recover_listening_snapshot(&p).unwrap().is_none());
        owner.stop_and_join().unwrap();
    }

    #[tokio::test]
    async fn snapshot_dropped_reply_retains_shutdown_guard_and_orders_later_next() {
        let (db, owner, s) = setup();
        let p = request(&s);
        let operations = Arc::new(crate::sync::SyncOperationManager::new());
        let guard = operations.try_admit_mutation().unwrap();
        let locked = owner.inner.lock().unwrap();
        let (reply, rx) = mpsc::channel();
        owner.snapshot_save_gate.store(true, Ordering::Release);
        owner
            .command_tx
            .try_send(OwnerCommand::SaveSnapshot(
                p.clone(),
                Some(guard),
                SavePermit(owner.snapshot_save_gate.clone()),
                reply,
            ))
            .ok()
            .unwrap();
        let (next_reply, next) = mpsc::channel();
        owner
            .command_tx
            .try_send(OwnerCommand::Control(
                ControlParams {
                    schema_version: 1,
                    instance_id: s.instance_id.clone(),
                    session_id: s.session_id.clone(),
                    command_id: Uuid::new_v4().to_string(),
                    expected_generation_id: s.generation_id.clone(),
                    occurrence_id: s.current.as_ref().unwrap().occurrence_id.clone(),
                    action: ControlAction::Next,
                },
                None,
                next_reply,
            ))
            .ok()
            .unwrap();
        drop(rx);
        assert_eq!(operations.begin_shutdown_fence().pending_mutation_count, 1);
        drop(locked);
        let after = next.recv().unwrap().unwrap();
        let saved = db.recover_listening_snapshot(&p).unwrap().unwrap();
        let entries = db
            .list_listening_snapshot_entries(ListSnapshotEntriesParams {
                schema_version: 1,
                snapshot_id: saved.snapshot_id.clone(),
                cursor: None,
                limit: None,
            })
            .unwrap();
        assert_eq!(
            entries
                .entries
                .iter()
                .map(|e| &e.occurrence_id)
                .collect::<Vec<_>>(),
            s.occurrences
                .iter()
                .map(|o| &o.occurrence_id)
                .collect::<Vec<_>>()
        );
        assert_ne!(after.current, s.current);
        assert_eq!(
            operations
                .shutdown_snapshot()
                .await
                .unwrap()
                .pending_mutation_count,
            0
        );
        let SaveSnapshotResult::Saved { snapshot, .. } =
            owner.save_snapshot(request(&after), None).unwrap()
        else {
            panic!()
        };
        assert_eq!(snapshot.entry_count, "2");
        assert_eq!(saved.entry_count, "3");
        owner.stop_and_join().unwrap();
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "explicit resource measurement; run with SNAPSHOT_BENCH_ROWS and SNAPSHOT_BENCH_SAVE"]
    fn snapshot_resource_measurement() {
        use crossbeam_queue::ArrayQueue;
        use rusqlite::params;
        let count: i64 = std::env::var("SNAPSHOT_BENCH_ROWS")
            .unwrap_or_else(|_| "100000".into())
            .parse()
            .unwrap();
        let save = std::env::var("SNAPSHOT_BENCH_SAVE").as_deref() != Ok("0");
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(Database::new(dir.path().join("bench.db")).unwrap());
        db.init_playback().unwrap();
        let session = Uuid::new_v4().to_string();
        {
            let mut conn = db.conn.lock().unwrap();
            let tx = conn.transaction().unwrap();
            tx.execute("WITH RECURSIVE seq(x) AS (VALUES(0) UNION ALL SELECT x+1 FROM seq WHERE x+1<?1) INSERT INTO playback_occurrences(session_id,occurrence_id,ordinal,server_id,track_id) SELECT ?2,printf('00000000-0000-4000-8000-%012d',x),x,CASE x%2 WHEN 0 THEN 'offline-a' ELSE 'offline-b' END,printf('track-%d',x%20) FROM seq",params![count,session]).unwrap();
            tx.execute("INSERT INTO playback_sessions(singleton_id,session_id,queue_revision,checkpoint_sequence,transport_state,current_occurrence_id,position_ms) VALUES(1,?1,0,0,'paused',?2,0)",params![session,format!("00000000-0000-4000-8000-{:012}",count-1)]).unwrap();
            tx.execute("WITH RECURSIVE visits(v) AS (VALUES(0) UNION ALL SELECT v+1 FROM visits WHERE v<4) INSERT INTO playback_attempts(attempt_id,session_id,occurrence_id,server_id,track_id,disposition) SELECT printf('10000000-0000-4000-8000-%012d',ordinal*5+v),session_id,occurrence_id,server_id,track_id,'restarted' FROM playback_occurrences CROSS JOIN visits ORDER BY ordinal,v",[]).unwrap();
            tx.commit().unwrap();
        }
        let owner = PlaybackSession::restore(db.clone(), Uuid::new_v4().to_string());
        let s = owner.snapshot().unwrap();
        assert_eq!(s.restoration.status, "ok");
        let running = Arc::new(AtomicBool::new(true));
        let db_running = running.clone();
        let sync_db = db.clone();
        let sync_worker = std::thread::spawn(move || {
            let mut writes = 0;
            while db_running.load(Ordering::Acquire) {
                sync_db
                    .upsert_autofill_history("bench", "offline-a", "track", Some(writes), None)
                    .unwrap();
                writes += 1;
                std::thread::sleep(Duration::from_millis(1));
            }
            writes
        });
        let audio_running = running.clone();
        let audio_worker = std::thread::spawn(move || {
            let pcm = ArrayQueue::new(2048);
            let mut consumer = crate::playback::output::PcmConsumer::new(2, 480);
            let mut output = [0f32; 480];
            let mut underruns = 0;
            let mut callbacks = 0;
            while audio_running.load(Ordering::Acquire) {
                while pcm.push(0.1).is_ok() {}
                if consumer.render(&mut output, &pcm, true, false).samples != 480 {
                    underruns += 1;
                }
                callbacks += 1;
                std::thread::sleep(Duration::from_millis(5));
            }
            (callbacks, underruns)
        });
        let peak = || {
            let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
            assert_eq!(
                unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) },
                0
            );
            let value = unsafe { usage.assume_init() }.ru_maxrss as u64;
            if cfg!(target_os = "macos") {
                value
            } else {
                value * 1024
            }
        };
        std::thread::sleep(Duration::from_millis(100));
        let before = peak();
        let started = Instant::now();
        let pending = if save {
            let (tx, rx) = mpsc::channel();
            owner.snapshot_save_gate.store(true, Ordering::Release);
            owner
                .command_tx
                .try_send(OwnerCommand::SaveSnapshot(
                    request(&s),
                    None,
                    SavePermit(owner.snapshot_save_gate.clone()),
                    tx,
                ))
                .ok()
                .unwrap();
            Some(rx)
        } else {
            None
        };
        let control_start = Instant::now();
        owner
            .native_control(NativeControlIntent::Pause, None)
            .unwrap();
        let control_ms = control_start.elapsed().as_secs_f64() * 1000.;
        if let Some(reply) = pending {
            let SaveSnapshotResult::Saved { snapshot, .. } = reply.recv().unwrap().unwrap() else {
                panic!()
            };
            assert_eq!(snapshot.entry_count, count.to_string());
        }
        let elapsed_ms = started.elapsed().as_secs_f64() * 1000.;
        let after = peak();
        std::thread::sleep(Duration::from_millis(100));
        running.store(false, Ordering::Release);
        let sync_writes = sync_worker.join().unwrap();
        let (callbacks, underruns) = audio_worker.join().unwrap();
        owner.stop_and_join().unwrap();
        println!(
            "SNAPSHOT_RESOURCE rows={count} attempts={} save={save} elapsed_ms={elapsed_ms:.3} db_hold_ms={:.3} owner_hold_ms={:.3} queued_control_ms={control_ms:.3} peak_rss_bytes={after} peak_growth_bytes={} sync_writes={sync_writes} pcm_callbacks={callbacks} synthetic_underruns={underruns}",
            count * 5,
            crate::playback::export::LAST_DB_HOLD_US.load(Ordering::Acquire) as f64 / 1000.,
            LAST_OWNER_HOLD_US.load(Ordering::Acquire) as f64 / 1000.,
            after.saturating_sub(before)
        );
        assert!(
            control_ms < 2000.,
            "queued control exceeded local measurement budget"
        );
        assert!(
            after.saturating_sub(before) < 32 * 1024 * 1024,
            "capture exceeded incremental peak memory budget"
        );
        assert_eq!(underruns, 0);
    }
}
