async fn sync_plan_test_state() -> (tempfile::TempDir, Arc<AppState>) {
    let state = make_test_state(Arc::new(crate::db::Database::memory().unwrap()));
    let provider = crate::providers::subsonic::SubsonicProvider::from_stored_config(
        ProviderCredentials {
            server_url: "http://127.0.0.1:9".into(),
            credential: CredentialKind::Password {
                username: "fixture".into(),
                password: "fixture".into(),
            },
        },
        true,
        Some("1.16.1".into()),
    )
    .unwrap();
    state
        .server_manager
        .write()
        .await
        .set_test_provider(Arc::new(provider));
    let dir = tempfile::tempdir().unwrap();
    let manifest = crate::device::DeviceManifest {
        device_id: "prepared-plan-device".into(),
        version: "1.1".into(),
        managed_paths: vec!["Music".into()],
        ..Default::default()
    };
    state
        .device_manager
        .handle_device_detected(
            dir.path().to_path_buf(),
            manifest,
            Arc::new(crate::device_io::MscBackend::new(dir.path().to_path_buf())),
        )
        .await
        .unwrap();
    (dir, state)
}

#[tokio::test]
async fn compact_daemon_state_preserves_metadata_and_capacity_without_track_records() {
    let _guard = credential_test_lock();
    let (_dir, state) = sync_plan_test_state().await;
    state.device_manager.update_manifest(|manifest| {
        manifest.synced_items=vec![serde_json::from_value(json!({"providerItemId":"manual","name":"Manual","localPath":"Music/manual.mp3","sizeBytes":123,"syncedAt":"fixture"})).unwrap(),serde_json::from_value(json!({"providerItemId":"auto","name":"Auto","localPath":"Music/auto.mp3","sizeBytes":456,"syncedAt":"fixture","isAutoFill":true})).unwrap()];
        manifest.pending_item_ids=vec!["pending".into()];manifest.dirty=true;manifest.name=Some("Settings preserved".into());
    }).await.unwrap();
    let full = handle_get_daemon_state(&state).await.unwrap();
    let compact = handle_get_daemon_state_params(&state, Some(json!({"includeManifest":false})))
        .await
        .unwrap();
    for field in [
        "deviceMapping",
        "connectedDevices",
        "selectedDevicePath",
        "destinations",
        "destinationRevision",
        "dirtyManifest",
        "autoSyncOnConnect",
        "autoFill",
    ] {
        assert_eq!(compact[field], full[field], "{field}");
    }
    for (key, value) in full["currentDevice"].as_object().unwrap() {
        if [
            "synced_items",
            "pending_item_ids",
            "basket_items",
            "playlists",
            "folder_ids",
        ]
        .contains(&key.as_str())
        {
            assert!(compact["currentDevice"].get(key).is_none());
        } else {
            assert_eq!(&compact["currentDevice"][key], value, "{key}");
        }
    }
    assert_eq!(compact["currentDevice"]["syncedItemCount"], 2);
    assert_eq!(compact["currentDevice"]["pendingItemCount"], 1);
    assert_eq!(
        compact["currentDevice"]["syncedByteTotals"],
        json!({"selectedBytes":123,"autofillBytes":456})
    );
    assert_eq!(
        full["currentDevice"]["synced_items"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        handle_get_daemon_state_params(&state, Some(json!({"includeManifest":true})))
            .await
            .unwrap()["currentDevice"],
        full["currentDevice"]
    );
}

async fn publish_test_plan(state: &AppState, delta: crate::sync::SyncDelta) -> Value {
    let manifest = state.device_manager.get_current_device().await.unwrap();
    finish_sync_delta(state, delta, &manifest, &json!({"prepared":true}))
        .await
        .unwrap()
}

#[tokio::test]
async fn prepared_sync_summary_pages_all_blocked_details_and_discard_releases_ownership() {
    let _guard = credential_test_lock();
    let (_dir, state) = sync_plan_test_state().await;
    let mut delta = crate::sync::calculate_delta(&[], &crate::device::DeviceManifest::default());
    delta.blocked = (0..1001)
        .map(|i| crate::sync::SyncBlockedItem {
            provider_item_id: format!("blocked-{i}"),
            name: format!("Book {i}"),
            server_id: None,
            media_role: crate::device::MediaRole::Audiobook,
            reason_code: "incompatible-direct-format".into(),
            reason: "Unsupported audio".into(),
        })
        .collect();
    let summary = publish_test_plan(&state, delta).await;
    assert_eq!(summary["blockedCount"], 1001);
    assert!(summary.get("adds").is_none());
    assert!(summary.get("blocked").is_none());
    let id = summary["planId"].as_str().unwrap();
    let first = handle_sync_plan_details(&state, Some(json!({"planId":id,"offset":0})))
        .await
        .unwrap();
    assert_eq!(first["items"].as_array().unwrap().len(), 500);
    assert_eq!(first["items"][0]["providerItemId"], "blocked-0");
    assert_eq!(first["nextOffset"], 500);
    let last = handle_sync_plan_details(&state, Some(json!({"planId":id,"offset":1000})))
        .await
        .unwrap();
    assert_eq!(last["items"].as_array().unwrap().len(), 1);
    assert_eq!(last["items"][0]["providerItemId"], "blocked-1000");
    assert!(last["nextOffset"].is_null());
    assert!(state.sync_operation_manager.try_start_pipeline().is_some());
    handle_sync_plan_discard(&state, Some(json!({"planId":id}))).unwrap();
    assert!(
        state
            .sync_operation_manager
            .pending_plan
            .lock()
            .unwrap()
            .is_none()
    );
    assert!(
        handle_sync_plan_details(&state, Some(json!({"planId":id})))
            .await
            .is_err()
    );
    assert!(
        !state
            .device_manager
            .get_current_device()
            .await
            .unwrap()
            .dirty
    );
}

#[tokio::test]
async fn prepared_sync_expiry_and_manifest_changes_are_rejected_before_admission() {
    let _guard = credential_test_lock();
    let (_dir, state) = sync_plan_test_state().await;
    let empty = || crate::sync::calculate_delta(&[], &crate::device::DeviceManifest::default());
    let summary = publish_test_plan(&state, empty()).await;
    state
        .sync_operation_manager
        .pending_plan
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .created_at = std::time::Instant::now() - SYNC_PLAN_TTL;
    let error = handle_sync_execute(&state, Some(json!({"planId":summary["planId"]})))
        .await
        .unwrap_err();
    assert_eq!(error.data.unwrap()["requiresSyncPreparation"], true);
    assert!(
        state
            .sync_operation_manager
            .get_all_operations()
            .await
            .is_empty()
    );
    let summary = publish_test_plan(&state, empty()).await;
    state
        .device_manager
        .update_manifest(|m| m.name = Some("Configuration changed".into()))
        .await
        .unwrap();
    let error = handle_sync_execute(&state, Some(json!({"planId":summary["planId"]})))
        .await
        .unwrap_err();
    assert_eq!(error.data.unwrap()["requiresSyncPreparation"], true);
    assert!(
        state
            .sync_operation_manager
            .get_all_operations()
            .await
            .is_empty()
    );
    assert!(
        !state
            .device_manager
            .get_current_device()
            .await
            .unwrap()
            .dirty
    );
    assert!(
        state
            .sync_operation_manager
            .pending_plan
            .lock()
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn prepared_sync_token_is_single_use_and_confirmation_does_not_keep_admission_fence() {
    let _guard = credential_test_lock();
    let (_dir, state) = sync_plan_test_state().await;
    let summary = handle_sync_calculate_delta(&state, Some(json!({"prepared":true,"itemIds":[]})))
        .await
        .unwrap();
    assert!(state.sync_operation_manager.try_start_pipeline().is_some());
    let result = handle_sync_execute(&state, Some(json!({"planId":summary["planId"]})))
        .await
        .unwrap();
    let operation = result["operationId"].as_str().unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while state.sync_operation_manager.has_active_operation().await {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        state
            .sync_operation_manager
            .get_operation(operation)
            .await
            .unwrap()
            .status,
        crate::sync::SyncStatus::Complete
    );
    assert!(
        handle_sync_execute(&state, Some(json!({"planId":summary["planId"]})))
            .await
            .is_err()
    );
    assert!(
        !state
            .device_manager
            .get_current_device()
            .await
            .unwrap()
            .dirty
    );
}

#[tokio::test]
async fn preparation_rejects_changed_server_configuration_without_publishing_a_plan() {
    let _guard = credential_test_lock();
    let (_dir, state) = sync_plan_test_state().await;
    let target: crate::sync::SyncTarget = state
        .device_manager
        .get_selected_sync_target()
        .await
        .unwrap()
        .into();
    let fingerprint = sync_target_fingerprint(&state, &target).await.unwrap();
    state.server_manager.write().await.servers[0].url = "http://changed.invalid".into();
    let delta = crate::sync::calculate_delta(&[], &target.manifest);
    let error = finish_sync_delta(
        &state,
        delta,
        &target.manifest,
        &json!({"prepared":true,"preparationFingerprint":fingerprint}),
    )
    .await
    .unwrap_err();
    assert_eq!(error.data.unwrap()["requiresSyncPreparation"], true);
    assert!(
        state
            .sync_operation_manager
            .pending_plan
            .lock()
            .unwrap()
            .is_none()
    );
    assert!(
        state
            .sync_operation_manager
            .get_all_operations()
            .await
            .is_empty()
    );
    assert!(
        !state
            .device_manager
            .get_current_device()
            .await
            .unwrap()
            .dirty
    );
}

#[tokio::test]
async fn replaced_sync_token_cannot_discard_the_current_plan() {
    let _guard = credential_test_lock();
    let (_dir, state) = sync_plan_test_state().await;
    let empty = || crate::sync::calculate_delta(&[], &crate::device::DeviceManifest::default());
    let old = publish_test_plan(&state, empty()).await;
    let current = publish_test_plan(&state, empty()).await;
    assert!(
        handle_sync_execute(&state, Some(json!({"planId":old["planId"]})))
            .await
            .is_err()
    );
    assert_eq!(
        state
            .sync_operation_manager
            .pending_plan
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .id,
        current["planId"].as_str().unwrap()
    );
    assert!(
        handle_sync_plan_details(&state, Some(json!({"planId":current["planId"]})))
            .await
            .is_ok()
    );
    handle_sync_plan_discard(&state, Some(json!({"planId":current["planId"]}))).unwrap();
}

#[test]
fn spooled_sync_history_preserves_resident_tiers_and_id_reassignment() {
    let db = crate::db::Database::memory().unwrap();
    db.upsert_autofill_history("device", "server", "resident", Some(10), Some("0"))
        .unwrap();
    db.upsert_autofill_history("device", "server", "old", Some(20), Some("2"))
        .unwrap();
    let mut manifest = crate::device::DeviceManifest {
        device_id: "device".into(),
        ..Default::default()
    };
    manifest.synced_items = serde_json::from_value(json!([
        {"providerItemId":"resident","name":"Resident","album":null,"artist":null,"localPath":"Music/resident.mp3","sizeBytes":4,"syncedAt":"before","serverId":"server"},
        {"providerItemId":"old","name":"Rekeyed","album":null,"artist":null,"localPath":"Music/old.mp3","sizeBytes":4,"syncedAt":"before","serverId":"server"}
    ])).unwrap();
    let delta:crate::sync::SyncDelta = serde_json::from_value(json!({"adds":[],"deletes":[],"unchanged":1,"idChanges":[{"oldJellyfinId":"old","newJellyfinId":"new","oldLocalPath":"Music/old.mp3","name":"Rekeyed","album":null,"artist":null,"sizeBytes":4,"etag":null,"sourceServerId":"server"}]})).unwrap();
    let plan = crate::sync_plan::SyncPlan::from_delta(delta, &manifest).unwrap();
    manifest.synced_items.clear();
    record_autofill_plan_history(&db, &manifest, &plan, &[], 100).unwrap();
    let rows = db.get_autofill_history("device", "server").unwrap();
    assert!(rows.contains(&("resident".into(), Some(100), Some("0".into()))));
    assert!(rows.contains(&("new".into(), Some(100), Some("2".into()))));
    assert!(rows.contains(&("old".into(), Some(20), Some("2".into()))));
}

#[tokio::test]
async fn reviewed_forced_blocked_rekey_preserves_audio_and_old_identity() {
    let _guard = credential_test_lock();
    let (dir, state) = sync_plan_test_state().await;
    std::fs::create_dir_all(dir.path().join("Podcasts")).unwrap();
    std::fs::write(dir.path().join("Podcasts/old.mp3"), b"original audio").unwrap();
    state.device_manager.update_manifest(|m| {
        m.podcast_path=Some("Podcasts".into());
        m.synced_items=serde_json::from_value(json!([{ "providerItemId":"old", "mediaRole":"podcast", "name":"Episode", "localPath":"Podcasts/old.mp3", "sizeBytes":14, "syncedAt":"before" }])).unwrap();
    }).await.unwrap();
    let delta = json!({"adds":[],"deletes":[],"unchanged":0,"idChanges":[{"oldJellyfinId":"old","newJellyfinId":"new","mediaRole":"podcast","oldLocalPath":"Podcasts/old.mp3","name":"Episode","album":null,"artist":null,"sizeBytes":14,"etag":null}]});
    let result = handle_sync_execute(&state, Some(json!({"force":true,"delta":delta})))
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while state.sync_operation_manager.has_active_operation().await {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(result.get("operationId").is_some());
    assert_eq!(
        std::fs::read(dir.path().join("Podcasts/old.mp3")).unwrap(),
        b"original audio"
    );
    assert_eq!(
        state
            .device_manager
            .get_current_device()
            .await
            .unwrap()
            .synced_items[0]
            .jellyfin_id,
        "old"
    );
}

#[test]
fn reviewed_force_does_not_reintroduce_already_blocked_resident() {
    let mut manifest = crate::device::DeviceManifest::default();
    manifest.synced_items=serde_json::from_value(json!([{ "providerItemId":"old", "mediaRole":"podcast", "name":"Episode", "localPath":"Podcasts/old.mp3", "sizeBytes":14, "syncedAt":"before" }])).unwrap();
    let mut delta = crate::sync::calculate_delta(&[], &Default::default());
    delta.blocked.push(crate::sync::SyncBlockedItem {
        provider_item_id: "old".into(),
        name: "Episode".into(),
        server_id: None,
        media_role: crate::device::MediaRole::Podcast,
        reason_code: "server-unavailable".into(),
        reason: "Unavailable".into(),
    });
    apply_force_sync(&mut delta, &manifest);
    assert!(delta.adds.is_empty());
    assert!(delta.deletes.is_empty());
    assert_eq!(delta.blocked.len(), 1);
}

#[tokio::test]
async fn reviewed_prepared_force_preserves_blocked_resident_and_exact_count() {
    let _guard = credential_test_lock();
    let (dir, state) = sync_plan_test_state().await;
    std::fs::create_dir_all(dir.path().join("Podcasts")).unwrap();
    std::fs::write(dir.path().join("Podcasts/old.mp3"), b"original audio").unwrap();
    state.device_manager.update_manifest(|m| {
        m.podcast_path=Some("Podcasts".into());
        m.synced_items=serde_json::from_value(json!([{ "providerItemId":"old", "mediaRole":"podcast", "name":"Episode", "localPath":"Podcasts/old.mp3", "sizeBytes":14, "syncedAt":"before" }])).unwrap();
    }).await.unwrap();
    let manifest = state.device_manager.get_current_device().await.unwrap();
    let mut delta = crate::sync::calculate_delta(&[], &Default::default());
    delta.blocked.push(crate::sync::SyncBlockedItem {
        provider_item_id: "old".into(),
        name: "Episode".into(),
        server_id: None,
        media_role: crate::device::MediaRole::Podcast,
        reason_code: "server-unavailable".into(),
        reason: "Unavailable".into(),
    });
    let summary = finish_sync_delta(
        &state,
        delta,
        &manifest,
        &json!({"prepared":true,"force":true}),
    )
    .await
    .unwrap();
    assert_eq!(summary["blockedCount"], 1);
    assert_eq!(summary["addsCount"], 0);
    assert_eq!(summary["deletesCount"], 0);
    handle_sync_execute(
        &state,
        Some(json!({"planId":summary["planId"],"force":true})),
    )
    .await
    .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while state.sync_operation_manager.has_active_operation().await {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        std::fs::read(dir.path().join("Podcasts/old.mp3")).unwrap(),
        b"original audio"
    );
    assert_eq!(
        state
            .device_manager
            .get_current_device()
            .await
            .unwrap()
            .synced_items[0]
            .jellyfin_id,
        "old"
    );
}

#[tokio::test]
async fn reviewed_invalid_or_busy_preparation_preserves_pending_plan() {
    let _guard = credential_test_lock();
    let (_dir, state) = sync_plan_test_state().await;
    let summary = publish_test_plan(
        &state,
        crate::sync::calculate_delta(&[], &Default::default()),
    )
    .await;
    for params in [json!([]), json!("bad"), Value::Null] {
        let response = handler(
            State(state.clone()),
            Json(JsonRpcRequest {
                jsonrpc: "2.0".into(),
                method: "sync_prepare".into(),
                params: Some(params),
                id: json!(1),
            }),
        )
        .await
        .0;
        assert_eq!(response.error.unwrap().code, ERR_INVALID_PARAMS);
        assert_eq!(
            state
                .sync_operation_manager
                .pending_plan
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .id,
            summary["planId"].as_str().unwrap()
        );
    }
    let _pipeline = state.sync_operation_manager.try_start_pipeline().unwrap();
    let response = handler(
        State(state.clone()),
        Json(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            method: "sync_prepare".into(),
            params: Some(json!({"itemIds":[]})),
            id: json!(2),
        }),
    )
    .await
    .0;
    assert_eq!(response.error.unwrap().code, ERR_SYNC_IN_PROGRESS);
    assert_eq!(
        state
            .sync_operation_manager
            .pending_plan
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .id,
        summary["planId"].as_str().unwrap()
    );
    drop(_pipeline);
    state
        .device_manager
        .update_manifest(|m| {
            m.transcoding_profile_id = Some("missing-profile-for-preparation-test".into())
        })
        .await
        .unwrap();
    let response = handler(
        State(state.clone()),
        Json(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            method: "sync_prepare".into(),
            params: Some(json!({"itemIds":[]})),
            id: json!(3),
        }),
    )
    .await
    .0;
    assert!(response.error.is_some());
    assert_eq!(
        state
            .sync_operation_manager
            .pending_plan
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .id,
        summary["planId"].as_str().unwrap()
    );
}

#[tokio::test]
async fn reviewed_cancel_or_shutdown_during_final_preflight_cannot_publish() {
    let _guard = credential_test_lock();
    for shutdown in [false, true] {
        let (_dir, state) = sync_plan_test_state().await;
        let manifest = state.device_manager.get_current_device().await.unwrap();
        let _pipeline = state.sync_operation_manager.try_start_pipeline().unwrap();
        let locked = state.server_manager.write().await;
        let mut delta = crate::sync::calculate_delta(&[], &Default::default());
        let mut add = add_item("episode", Some(&locked.servers[0].id));
        add.media_role = crate::device::MediaRole::Podcast;
        delta.adds.push(add);
        let taskstate = state.clone();
        let mut task = tokio::spawn(async move {
            finish_sync_delta(&taskstate, delta, &manifest, &json!({"prepared":true})).await
        });
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(30), &mut task)
                .await
                .is_err()
        );
        if shutdown {
            state.sync_operation_manager.commit_shutdown().await;
        } else {
            assert!(state.sync_operation_manager.request_pipeline_cancel());
        }
        drop(locked);
        assert!(task.await.unwrap().is_err());
        assert!(
            state
                .sync_operation_manager
                .pending_plan
                .lock()
                .unwrap()
                .is_none()
        );
    }
}

#[tokio::test]
async fn reviewed_confirmation_error_keeps_matching_token_for_retry() {
    let _guard = credential_test_lock();
    let (_dir, state) = sync_plan_test_state().await;
    let mut delta = crate::sync::calculate_delta(&[], &Default::default());
    delta.deletes = (0..=crate::sync::DESTRUCTIVE_CLEANUP_THRESHOLD)
        .map(|i| crate::sync::SyncDeleteItem {
            jellyfin_id: format!("old-{i}"),
            local_path: format!("Music/{i}.mp3"),
            name: format!("Old {i}"),
            reason_code: None,
            reason: None,
        })
        .collect();
    let summary = publish_test_plan(&state, delta).await;
    let error = handle_sync_execute(&state, Some(json!({"planId":summary["planId"]})))
        .await
        .unwrap_err();
    assert_eq!(
        error.data.unwrap()["requiresDestructiveCleanupConfirmation"],
        true
    );
    assert_eq!(
        state
            .sync_operation_manager
            .pending_plan
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .id,
        summary["planId"].as_str().unwrap()
    );
    assert!(
        handle_sync_execute(
            &state,
            Some(json!({"planId":summary["planId"],"confirmDestructiveCleanup":true}))
        )
        .await
        .is_ok()
    );
    assert!(
        state
            .sync_operation_manager
            .pending_plan
            .lock()
            .unwrap()
            .is_none()
    );
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while state.sync_operation_manager.has_active_operation().await {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn reviewed_replacement_aborts_superseded_expiry_task() {
    let _guard = credential_test_lock();
    let (_dir, state) = sync_plan_test_state().await;
    publish_test_plan(
        &state,
        crate::sync::calculate_delta(&[], &Default::default()),
    )
    .await;
    publish_test_plan(
        &state,
        crate::sync::calculate_delta(&[], &Default::default()),
    )
    .await;
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    assert_eq!(Arc::weak_count(&state.sync_operation_manager), 1);
    state.sync_operation_manager.commit_shutdown().await;
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    assert_eq!(Arc::weak_count(&state.sync_operation_manager), 0);
}

#[tokio::test]
async fn reviewed_cancel_after_publication_releases_plan_and_expiry() {
    let _guard = credential_test_lock();
    let (_dir, state) = sync_plan_test_state().await;
    let _pipeline = state.sync_operation_manager.try_start_pipeline().unwrap();
    let summary = publish_test_plan(
        &state,
        crate::sync::calculate_delta(&[], &Default::default()),
    )
    .await;
    // Hold the actual publication mutex after its cancellation check, so the
    // cancel flag can change while the publisher still owns the completed plan.
    let publication = state.sync_operation_manager.pending_plan.lock().unwrap();
    assert_eq!(
        publication.as_ref().unwrap().id,
        summary["planId"].as_str().unwrap()
    );
    assert!(!state.sync_operation_manager.is_pipeline_cancelled());
    let manager = state.sync_operation_manager.clone();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let cancellation =
        std::thread::spawn(move || done_tx.send(manager.request_pipeline_cancel()).unwrap());
    let started = std::time::Instant::now();
    while !state.sync_operation_manager.is_pipeline_cancelled() {
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        std::thread::yield_now();
    }
    let completed_while_publication_owned = done_rx
        .recv_timeout(std::time::Duration::from_millis(30))
        .ok();
    drop(publication);
    let cancelled = completed_while_publication_owned.unwrap_or_else(|| {
        done_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap()
    });
    cancellation.join().unwrap();
    assert!(cancelled);
    assert!(
        state
            .sync_operation_manager
            .pending_plan
            .lock()
            .unwrap()
            .is_none()
    );
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    assert_eq!(Arc::weak_count(&state.sync_operation_manager), 0);
    assert!(
        handle_sync_plan_details(&state, Some(json!({"planId":summary["planId"]})))
            .await
            .is_err()
    );
    assert!(
        !state
            .device_manager
            .get_current_device()
            .await
            .unwrap()
            .dirty
    );
}

#[tokio::test]
#[ignore = "external process memory diagnostic for real daemon-state polling"]
async fn benchmark_daemon_state_retention() {
    let _guard = credential_test_lock();
    let count: usize = std::env::var("HIFIMULE_SYNC_BENCH_COUNT")
        .unwrap_or_else(|_| "40000".into())
        .parse()
        .unwrap();
    let hold: u64 = std::env::var("HIFIMULE_SYNC_BENCH_HOLD_SECS")
        .unwrap_or_else(|_| "20".into())
        .parse()
        .unwrap();
    let prep_polls: usize = std::env::var("HIFIMULE_DAEMON_STATE_BENCH_POLLS")
        .unwrap_or_else(|_| "10".into())
        .parse()
        .unwrap();
    let (_dir, state) = sync_plan_test_state().await;
    let compact =
        std::env::var("HIFIMULE_DAEMON_STATE_BENCH_COMPACT").is_ok_and(|value| value == "1");
    let params = compact.then(|| json!({"includeManifest":false}));
    println!(
        "SYNC_MEMORY_PHASE=idle count={count} preparation_polls={prep_polls} compact={compact}"
    );
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    println!("SYNC_MEMORY_PHASE=preparation count={count}");
    state
        .device_manager
        .update_manifest(|manifest| {
            manifest.dirty = true;
            manifest.synced_items = (0..count)
                .map(|index| crate::device::SyncedItem {
                    is_auto_fill: index % 2 == 0,
                    media_role: crate::device::MediaRole::Music,
                    jellyfin_id: format!("old-{index:08}"),
                    name: format!("Existing song {index:08} with representative music metadata"),
                    album: Some(format!(
                        "Album {:08} original edition and bonus tracks",
                        index / 10
                    )),
                    artist: Some(format!("Artist {:08} and collaborators", index / 100)),
                    local_path: format!(
                        "Music/Artist {}/Album {}/Old track {index:08}.mp3",
                        index / 100,
                        index / 10
                    ),
                    size_bytes: 4_000_000 + index as u64,
                    synced_at: "2026-10-10T10:00:00Z".into(),
                    original_name: None,
                    etag: Some(format!("etag-{index:08}-{}", "e".repeat(48))),
                    provider_album_id: Some(format!("provider-album-{:08}", index / 10)),
                    provider_content_type: Some("audio/mpeg".into()),
                    provider_suffix: Some("mp3".into()),
                    original_bitrate: Some(320_000),
                    original_container: Some("mp3".into()),
                    track_number: Some((index % 10 + 1) as u32),
                    server_id: Some("controlled-fixture-server".into()),
                })
                .collect();
            manifest.pending_item_ids = (0..count + 17)
                .map(|index| format!("pending-{index:08}"))
                .collect();
            manifest.playlists = (0..56)
                .map(|index| {
                    let track_ids: Vec<_> = (index * count / 56..(index + 1) * count / 56)
                        .map(|track| format!("old-{track:08}"))
                        .collect();
                    crate::device::PlaylistManifestEntry {
                        jellyfin_id: format!("fixture-playlist-{index:02}"),
                        filename: format!("Controlled playlist {index:02}.m3u"),
                        track_count: track_ids.len() as u32,
                        track_ids,
                        last_modified: "2026-10-10T10:00:00Z".into(),
                    }
                })
                .collect();
        })
        .await
        .unwrap();

    // Exercise the actual full legacy response and the HTTP envelope encoding.
    // Each response/body is dropped before the next poll; only the authoritative
    // DeviceManager manifest remains. No synthetic response collection is kept.
    for index in 0..prep_polls {
        let value = handle_get_daemon_state_params(&state, params.clone())
            .await
            .unwrap();
        assert_eq!(
            if compact {
                value["currentDevice"]["syncedItemCount"].as_u64().unwrap() as usize
            } else {
                value["currentDevice"]["synced_items"]
                    .as_array()
                    .unwrap()
                    .len()
            },
            count
        );
        assert_eq!(
            if compact {
                value["currentDevice"]["pendingItemCount"].as_u64().unwrap() as usize
            } else {
                value["currentDevice"]["pending_item_ids"]
                    .as_array()
                    .unwrap()
                    .len()
            },
            count + 17
        );
        let response = JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: json!(index),
            result: Some(value),
            error: None,
        };
        let encoded = serde_json::to_vec(&response).unwrap();
        println!(
            "SYNC_MEMORY_POLL phase=preparation index={index} encoded_bytes={}",
            encoded.len()
        );
        drop(encoded);
        drop(response);
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }

    println!("SYNC_MEMORY_PHASE=transfer count={count}");
    let started = std::time::Instant::now();
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut polls = 0;
    while started.elapsed() < std::time::Duration::from_secs(hold) {
        interval.tick().await;
        if started.elapsed() >= std::time::Duration::from_secs(hold) {
            break;
        }
        let response = JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: json!(polls),
            result: Some(
                handle_get_daemon_state_params(&state, params.clone())
                    .await
                    .unwrap(),
            ),
            error: None,
        };
        let encoded = serde_json::to_vec(&response).unwrap();
        println!(
            "SYNC_MEMORY_POLL phase=transfer index={polls} encoded_bytes={}",
            encoded.len()
        );
        drop(encoded);
        drop(response);
        polls += 1;
    }
    println!("SYNC_MEMORY_PHASE=cleanup count={count} transfer_polls={polls}");
    state
        .device_manager
        .update_manifest(|manifest| {
            manifest.synced_items.clear();
            manifest.pending_item_ids.clear();
            manifest.playlists.clear();
        })
        .await
        .unwrap();
    drop(state);
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    println!("SYNC_MEMORY_PHASE=done count={count}");
}
