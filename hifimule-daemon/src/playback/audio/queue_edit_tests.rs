use super::*;

#[test]
fn retiring_revoked_decoder_joins_and_drains_only_the_inactive_slot() {
    for active_slot in [0, 1] {
        let parent = Arc::new(AtomicBool::new(false));
        let child = Arc::new(AtomicBool::new(false));
        let retired = Arc::new(AtomicBool::new(false));
        let a = ArrayQueue::new(2);
        let b = ArrayQueue::new(2);
        a.push(1.0).unwrap();
        b.push(2.0).unwrap();
        let active = super::super::output::DecoderWorker::spawn(parent.clone(), || ());
        let child_cancel = child.clone();
        let done = retired.clone();
        let prepared = super::super::output::DecoderWorker::spawn(child.clone(), move || {
            while !child_cancel.load(Ordering::Acquire) {
                std::thread::yield_now();
            }
            done.store(true, Ordering::Release);
        });
        let (mut wa, mut wb) = if active_slot == 0 {
            (Some(active), Some(prepared))
        } else {
            (Some(prepared), Some(active))
        };
        retire_successor_slot(active_slot, &mut wa, &mut wb, &a, &b);
        assert!(
            retired.load(Ordering::Acquire),
            "replacement cannot overlap a retired worker"
        );
        assert!(child.load(Ordering::Acquire));
        assert!(!parent.load(Ordering::Acquire));
        if active_slot == 0 {
            assert!(wb.is_none());
            assert_eq!(a.pop(), Some(1.0));
            assert!(b.is_empty());
        } else {
            assert!(wa.is_none());
            assert_eq!(b.pop(), Some(2.0));
            assert!(a.is_empty());
        }
    }
}

#[tokio::test]
async fn provider_ticket_captured_before_resolution_is_invalid_after_edit_or_shutdown() {
    let fence = Arc::new(SuccessorFence::default());
    let parent = Arc::new(AtomicBool::new(false));
    let ticket = SuccessorTicket {
        epoch: fence.epoch(),
        output_epoch: 1,
        fence: fence.clone(),
        parent_cancel: parent.clone(),
        alive: Arc::new(AtomicBool::new(true)),
        slot_free: Arc::new(AtomicBool::new(true)),
    };
    assert!(ticket.is_current());
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let provider = tokio::spawn(async move {
        started_tx.send(()).unwrap();
        release_rx.await.unwrap();
        // This is the same ticket captured before provider resolution, not a
        // fresh epoch assigned to an obsolete candidate afterward.
        ticket.is_current()
    });
    started_rx.await.unwrap();
    assert!(fence.revoke());
    release_tx.send(()).unwrap();
    assert!(!provider.await.unwrap());
    let ticket = SuccessorTicket {
        epoch: fence.epoch(),
        output_epoch: 1,
        fence,
        parent_cancel: parent.clone(),
        alive: Arc::new(AtomicBool::new(true)),
        slot_free: Arc::new(AtomicBool::new(true)),
    };
    parent.store(true, Ordering::Release);
    ticket.cancelled().await;
    assert!(!ticket.is_current());
}

fn engine_fixture() -> (AudioEngine, mpsc::Receiver<PreparedSuccessor>) {
    let (tx, rx) = mpsc::sync_channel(1);
    let pipeline = Pipeline {
        cancel: Arc::new(AtomicBool::new(false)),
        gate: Arc::new(AtomicBool::new(true)),
        alive: Arc::new(AtomicBool::new(true)),
        generation: "generation".into(),
        event_epoch: Arc::new(AtomicU64::new(1)),
        worker: std::thread::spawn(|| {}),
        compressed_high_water: Arc::new(AtomicU64::new(0)),
        successor_compressed_high_water: Arc::new(AtomicU64::new(0)),
        pcm_high_water: Arc::new(AtomicU64::new(0)),
        endpoint: Arc::new(Mutex::new(None)),
        position_ms: Arc::new(AtomicU64::new(1234)),
        output_control: Arc::new(OutputControl::new()),
        successor_tx: tx,
        output_epoch: 7,
        boundary_pending: Arc::new(AtomicBool::new(false)),
        handoff: Arc::new(HandoffReceipt::new()),
        successor_fence: Arc::new(SuccessorFence::default()),
        successor_slot_free: Arc::new(AtomicBool::new(true)),
        successor_coordinator: AtomicBool::new(false),
    };
    (
        AudioEngine {
            current: Mutex::new(Some(pipeline)),
            starts: tokio::sync::Mutex::new(()),
        },
        rx,
    )
}

fn candidate() -> crate::playback::continuity::SuccessorCandidate {
    crate::playback::continuity::SuccessorCandidate {
        instance_id: "instance".into(),
        session_id: "session".into(),
        predecessor_occurrence_id: "current".into(),
        successor: crate::playback::model::Occurrence {
            occurrence_id: "successor".into(),
            ordinal: 1,
            source: crate::playback::model::TrackSource {
                server_id: "server".into(),
                track_id: "track".into(),
            },
            availability: crate::playback::model::SourceAvailability::Unknown,
        },
        queue_revision: 1,
        control_epoch: 1,
        preparation_generation: 1,
        gain_bits: 1.0f32.to_bits(),
        qualified_suffix: None,
    }
}

fn description(url: &str) -> PlaybackDescription {
    PlaybackDescription {
        song: serde_json::from_value(
            serde_json::json!({ "id":"track", "title":"Track", "duration":1 }),
        )
        .unwrap(),
        representations: vec![crate::providers::PlaybackRepresentation {
            codec: Some("pcm_s16le".into()),
            container: Some("wav".into()),
            bitrate_kbps: None,
            sample_rate: Some(48000),
            bit_depth: Some(16),
            provenance: crate::providers::PlaybackProvenance::Original,
            seek_mechanism: None,
            request: PlaybackRequest {
                url: reqwest::Url::parse(url).unwrap(),
                headers: reqwest::header::HeaderMap::new(),
                range_supported: false,
            },
        }],
    }
}

#[tokio::test]
async fn delayed_successor_http_is_cancelled_without_cancelling_current_or_retaining_a_slot() {
    use tokio::io::AsyncReadExt;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/audio", listener.local_addr().unwrap());
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0u8; 2048];
        socket.read(&mut request).await.unwrap();
        entered_tx.send(()).unwrap();
        let _ = release_rx.await;
    });
    let (engine, rx) = engine_fixture();
    let ticket = engine.successor_ticket("generation").unwrap();
    let preparing = engine.prepare_successor(
        candidate(),
        &ticket,
        description(&url),
        "generation".into(),
        std::time::Instant::now() + std::time::Duration::from_secs(60),
    );
    tokio::pin!(preparing);
    tokio::select! {
        biased;
        result = &mut preparing => panic!("preparation finished before HTTP barrier: {result:?}"),
        _ = entered_rx => {},
    }
    assert!(
        !engine.revoke_successor("generation"),
        "edit should win before submission"
    );
    let failure = tokio::time::timeout(std::time::Duration::from_secs(1), &mut preparing)
        .await
        .unwrap()
        .unwrap_err();
    assert!(!failure.is_publishable());
    assert!(rx.try_recv().is_err());
    assert!(engine.successor_ticket("generation").is_some());
    let current = engine.current.lock().unwrap();
    let pipeline = current.as_ref().unwrap();
    assert!(!pipeline.cancel.load(Ordering::Acquire));
    assert!(pipeline.gate.load(Ordering::Acquire));
    assert_eq!(pipeline.position_ms.load(Ordering::Acquire), 1234);
    drop(current);
    release_tx.send(()).unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn revoked_provider_result_is_rejected_before_source_selection_and_new_ticket_can_prepare() {
    let (engine, rx) = engine_fixture();
    let stale = engine.successor_ticket("generation").unwrap();
    assert!(!engine.revoke_successor("generation"));
    let failure = engine
        .prepare_successor(
            candidate(),
            &stale,
            PlaybackDescription {
                song: description("http://127.0.0.1/unused").song,
                representations: vec![],
            },
            "generation".into(),
            std::time::Instant::now(),
        )
        .await
        .unwrap_err();
    assert!(
        !failure.is_publishable(),
        "reject old provider work before validating representations"
    );
    assert!(rx.try_recv().is_err());
    let next = engine.successor_ticket("generation").unwrap();
    assert_ne!(next.epoch, stale.epoch);
    assert!(
        next.is_current(),
        "unchanged authoritative successor can be retried after rollback"
    );
}
