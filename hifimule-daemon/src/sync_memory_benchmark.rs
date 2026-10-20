#[tokio::test]
#[ignore = "external process memory benchmark"]
async fn benchmark_large_sync_retention() {
    let count: usize = std::env::var("HIFIMULE_SYNC_BENCH_COUNT")
        .unwrap_or_else(|_| "40000".into())
        .parse()
        .unwrap();
    let hold: u64 = std::env::var("HIFIMULE_SYNC_BENCH_HOLD_SECS")
        .unwrap_or_else(|_| "20".into())
        .parse()
        .unwrap();
    let committed: usize = std::env::var("HIFIMULE_SYNC_BENCH_COMMITTED")
        .unwrap_or_else(|_| "0".into())
        .parse()
        .unwrap();
    let mut server = mockito::Server::new_async().await;
    let _download = server
        .mock("GET", "/rest/download.view")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_header("content-type", "audio/mpeg")
        .with_body(vec![1_u8, 2, 3, 4])
        .create_async()
        .await;
    let dir = tempfile::tempdir().unwrap();
    let (manager, io) = setup_provider_sync_device(dir.path()).await;
    let blocking = BlockingFirstWriteDeviceIo::new(io);
    blocking.block_at_write.store(committed, Ordering::SeqCst);
    let sync_io: Arc<dyn crate::device_io::DeviceIO> = blocking.clone();
    manager
        .set_test_sync_io(dir.path(), Arc::clone(&sync_io))
        .await;
    let operations = Arc::new(SyncOperationManager::new());
    let id = unique_operation_id("memory-benchmark");
    operations.create_operation(id.clone(), count).await;
    let poll_state =
        std::env::var("HIFIMULE_SYNC_BENCH_POLL_STATE").is_ok_and(|value| value == "1");
    let compact =
        std::env::var("HIFIMULE_DAEMON_STATE_BENCH_COMPACT").is_ok_and(|value| value == "1");
    let polling_state = if poll_state {
        Some(
            crate::rpc::benchmark_polling_state(Arc::clone(&manager), Arc::clone(&operations))
                .await,
        )
    } else {
        None
    };
    println!(
        "SYNC_MEMORY_PHASE=idle count={count} poll_state={poll_state} compact={compact} committed={committed}"
    );
    tokio::time::sleep(Duration::from_secs(2)).await;
    println!("SYNC_MEMORY_PHASE=preparation count={count}");
    manager
        .update_manifest(|m| {
            m.dirty = true;
            m.pending_item_ids = (0..count).map(|i| format!("new-{i:08}")).collect();
            m.synced_items = (0..count)
                .map(|i| {
                    let mut item = make_playlist_synced_item(
                        &format!("old-{i:08}"),
                        &format!(
                            "Music/Artist {}/Album {}/Old track {i:08}.mp3",
                            i / 100,
                            i / 10
                        ),
                    );
                    item.name = format!("Existing song {i:08} with representative music metadata");
                    item.album = Some(format!(
                        "Album {:08} original edition and bonus tracks",
                        i / 10
                    ));
                    item.artist = Some(format!("Artist {:08} and collaborators", i / 100));
                    item.etag = Some(format!("etag-{i:08}-{}", "e".repeat(48)));
                    item
                })
                .collect();
        })
        .await
        .unwrap();
    let delta = SyncDelta {
        provenance_updates: HashMap::new(),
        blocked: vec![],
        adds: (0..count)
            .map(|i| {
                let mut add =
                    add_item_with_provider_format(&format!("new-{i:08}"), "mp3", "audio/mpeg", 4);
                add.name = format!("New song {i:08} with representative music metadata");
                add.album = Some(format!(
                    "Album {:08} original edition and bonus tracks",
                    i / 10
                ));
                add.artist = Some(format!("Artist {:08} and collaborators", i / 100));
                add.provider_album_id =
                    Some(format!("provider-album-{:08}-original-edition", i / 10));
                add.etag = Some(format!("etag-{i:08}-{}", "e".repeat(48)));
                add
            })
            .collect(),
        deletes: vec![],
        id_changes: vec![],
        unchanged: count,
        playlists: vec![],
        pity_fired_servers: vec![],
    };
    let mut target = SyncTarget {
        path: dir.path().to_path_buf(),
        manifest: manager.get_current_device().await.unwrap(),
        observation: manager.get_selected_sync_target().await.unwrap().3,
        io: sync_io,
    };
    let plan = crate::sync_plan::SyncPlan::from_delta(delta, &target.manifest).unwrap();
    crate::sync_plan::compact_target(&mut target);
    let op_for_task = operations.clone();
    let id_for_task = id.clone();
    let task = tokio::spawn(async move {
        execute_plan_sync_with_protection(
            &plan,
            &target,
            ProviderSyncSource {
                local_encoding_servers: Default::default(),
                provider: subsonic_provider(server.url()),
                transcoding_profile: Some(rockbox_direct_profile()),
                providers_by_server: HashMap::new(),
            },
            op_for_task,
            id_for_task,
            manager,
            None,
        )
        .await
    });
    assert!(
        tokio::time::timeout(
            Duration::from_secs(120),
            blocking.first_write_started.notified()
        )
        .await
        .is_ok()
    );
    assert_eq!(
        blocking
            .trace
            .lock()
            .unwrap()
            .iter()
            .filter(|event| **event == "write")
            .count(),
        committed
    );
    println!(
        "SYNC_MEMORY_CHECKPOINTS manifest_writes={} committed={committed}",
        blocking.manifest_writes.load(Ordering::SeqCst)
    );
    if committed == 100 {
        assert_eq!(
            blocking.manifest_writes.load(Ordering::SeqCst),
            2,
            "initial dirty marker plus100trackcheckpoint before101st audio write"
        );
    }
    println!("SYNC_MEMORY_PHASE=transfer count={count}");
    let started = tokio::time::Instant::now();
    while started.elapsed() < Duration::from_secs(hold) {
        if let Some(state) = &polling_state {
            let bytes = crate::rpc::benchmark_encode_state(state, compact).await;
            println!("SYNC_MEMORY_POLL phase=transfer compact={compact} encoded_bytes={bytes}");
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    println!("SYNC_MEMORY_PHASE=cleanup count={count}");
    operations.request_cancel(&id).await;
    blocking.release_first_write();
    task.await.unwrap().unwrap();
    drop(polling_state);
    println!("SYNC_MEMORY_PHASE=done count={count}");
}
