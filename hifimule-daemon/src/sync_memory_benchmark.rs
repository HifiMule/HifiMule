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
    let operations = Arc::new(SyncOperationManager::new());
    let id = unique_operation_id("memory-benchmark");
    operations.create_operation(id.clone(), count).await;
    println!("SYNC_MEMORY_PHASE=idle count={count}");
    tokio::time::sleep(Duration::from_secs(2)).await;
    println!("SYNC_MEMORY_PHASE=preparation count={count}");
    manager
        .update_manifest(|m| {
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
    println!("SYNC_MEMORY_PHASE=transfer count={count}");
    tokio::time::sleep(Duration::from_secs(hold)).await;
    println!("SYNC_MEMORY_PHASE=cleanup count={count}");
    operations.request_cancel(&id).await;
    blocking.release_first_write();
    task.await.unwrap().unwrap();
    println!("SYNC_MEMORY_PHASE=done count={count}");
}
