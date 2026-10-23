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
    publish_sync_delta(state, delta, &manifest, &json!({"prepared":true}))
        .await
        .unwrap()
}

#[tokio::test]
async fn finish_sync_delta_preserves_force_recovery_and_autofill_metadata() {
    let _guard = credential_test_lock();
    let (dir, state) = sync_plan_test_state().await;
    tokio::fs::create_dir_all(dir.path().join("Music")).await.unwrap();
    tokio::fs::write(dir.path().join("Music/retained.mp3"), b"audio").await.unwrap();
    let mut manifest = state.device_manager.get_current_device().await.unwrap();
    manifest.synced_items = serde_json::from_value(json!([
        {"providerItemId":"retained","name":"retained","localPath":"Music/retained.mp3",
            "sizeBytes":1000,"syncedAt":"before","serverId":"source"},
        {"providerItemId":"missing","name":"missing","localPath":"Music/missing.mp3",
            "sizeBytes":1000,"syncedAt":"before","serverId":"source"}
    ])).unwrap();
    let desired: Vec<_> = ["retained", "missing", "new"].into_iter().map(|id| {
        let mut song = make_fake_song(id, id);
        song.duration_seconds = 240;
        song.size_bytes = Some(1000);
        let mut item = provider_song_to_desired_item(&song);
        item.server_id = Some("source".into());
        item
    }).collect();
    for force in [false, true] {
        let delta = finish_sync_delta(&state, &desired, &manifest, &json!({"force":force}), SyncDeltaPreparation {
            playlists: vec![crate::sync::PlaylistSyncItem {
                jellyfin_id: "playlist".into(), name: "Playlist".into(), tracks: vec![],
            }],
            auto_fill_ids: HashSet::from(["retained".into(), "missing".into()]),
            tiers: HashMap::from([("new".into(), "discovery".into())]),
            bitrate_overrides: HashMap::from([(("missing".into(), Some("source".into())), 96)]),
            pity_fired_servers: vec!["source".into()],
        }).await.unwrap();
        assert_eq!(delta.adds.len(), if force { 3 } else { 2 });
        assert_eq!(delta.unchanged, if force { 0 } else { 1 });
        let missing = delta.adds.iter().find(|item| item.jellyfin_id == "missing").unwrap();
        assert_eq!(missing.reason_code.as_deref(), Some(if force { "force-sync" } else { "device-file-missing" }));
        assert_eq!(missing.max_bitrate_override_kbps, Some(96));
        assert!(missing.is_auto_fill);
        assert_eq!(missing.duration_seconds, 240);
        let new = delta.adds.iter().find(|item| item.jellyfin_id == "new").unwrap();
        assert_eq!(new.tier.as_deref(), Some("discovery"));
        assert!(!new.is_auto_fill);
        if force {
            let retained = delta.adds.iter().find(|item| item.jellyfin_id == "retained").unwrap();
            assert!(retained.is_auto_fill);
            assert_eq!(retained.duration_seconds, 240);
        } else {
            assert_eq!(delta.provenance_updates.get("retained"), Some(&true));
        }
        assert_eq!(delta.playlists[0].jellyfin_id, "playlist");
        assert_eq!(delta.pity_fired_servers, ["source"]);
    }
    let mut conflicting = desired[0].clone();
    conflicting.server_id = Some("other-source".into());
    let error = finish_sync_delta(&state, &[conflicting], &manifest, &json!({}), Default::default()).await.unwrap_err();
    assert_eq!(error.data.unwrap()["code"], "AMBIGUOUS_PROVIDER_ITEM_ID");
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
        .update_manifest(|m| m.reconcile_before_sync = true)
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
    let error = publish_sync_delta(
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
    let summary = publish_sync_delta(
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
            publish_sync_delta(&taskstate, delta, &manifest, &json!({"prepared":true})).await
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

async fn setup_music_scan_fixture() -> (tempfile::TempDir, Arc<AppState>, String) {
    let (root,state)=sync_plan_test_state().await;
    let (tagged,_io,file)=crate::device_music_scan::tests::fixture().await;
    tokio::fs::create_dir_all(root.path().join("Music")).await.unwrap();
    tokio::fs::copy(tagged.path().join(file.path),root.path().join("Music/existing.mp3")).await.unwrap();
    let (manifest,observation)=state.device_manager.get_basket_target("prepared-plan-device").await.unwrap();
    let (_,_,io,_)=state.device_manager.get_sync_target_for_device(&manifest.device_id).await.unwrap();
    let mut song=crate::device_music_scan::tests::song();song.duration_seconds=2;
    let provider=Arc::new(crate::device_music_scan::tests::FixtureProvider{songs:vec![song],truncated:false});
    let id=uuid::Uuid::new_v4().to_string();
    music_setup_scans().lock().await.insert(id.clone(),MusicSetupScan{manifest,observation:observation.parse().unwrap(),io,provider,server:"captured-server".into(),progress:Arc::new(Default::default()),running:false,expires_at:std::time::Instant::now()+std::time::Duration::from_secs(300)});
    (root,state,id)
}

#[tokio::test]
async fn setup_music_scan_publishes_manifest_and_basket_atomically_for_captured_server() {
    let _guard=credential_test_lock();
    let (_root,state,id)=setup_music_scan_fixture().await;
    assert!(!state.device_manager.get_current_device().await.unwrap().reconcile_before_sync);
    let result=handle_music_setup_scan(&state,"device.music_scan_start",Some(json!({"scanId":id}))).await.unwrap();
    assert!(!state.device_manager.get_current_device().await.unwrap().reconcile_before_sync);
    assert_eq!(result["imported"],1);
    let manifest=state.device_manager.get_current_device().await.unwrap();
    assert_eq!(manifest.synced_items.len(),1);assert_eq!(manifest.basket_items.len(),1);
    assert_eq!(manifest.synced_items[0].server_id.as_deref(),Some("captured-server"));
    assert_eq!(manifest.basket_items[0].item_type,"MusicArtist");
    assert!(manifest.synced_items[0].size_bytes<50_000_000);
    assert!(!music_setup_pending(&manifest.device_id).await);
}

#[tokio::test]
async fn setup_music_scan_cancel_or_changed_destination_never_publishes_partial_import() {
    let _guard=credential_test_lock();
    let (_root,state,id)=setup_music_scan_fixture().await;
    handle_music_setup_scan(&state,"device.music_scan_cancel",Some(json!({"scanId":id}))).await.unwrap();
    assert!(handle_music_setup_scan(&state,"device.music_scan_start",Some(json!({"scanId":id}))).await.is_err());
    assert!(state.device_manager.get_current_device().await.unwrap().synced_items.is_empty());
    let (_root,state,id)=setup_music_scan_fixture().await;
    state.device_manager.update_manifest(|m|m.name=Some("changed while dialog was open".into())).await.unwrap();
    assert!(handle_music_setup_scan(&state,"device.music_scan_start",Some(json!({"scanId":id}))).await.is_err());
    let manifest=state.device_manager.get_current_device().await.unwrap();assert!(manifest.synced_items.is_empty());assert!(manifest.basket_items.is_empty());
    handle_music_setup_scan(&state,"device.music_scan_cancel",Some(json!({"scanId":id}))).await.unwrap();
}

#[tokio::test]
async fn normal_existing_audio_is_reused_but_force_schedules_replacement() {
    let _guard = credential_test_lock();
    let (root, state, id) = setup_music_scan_fixture().await;
    handle_music_setup_scan(&state, "device.music_scan_cancel", Some(json!({"scanId":id}))).await.unwrap();
    let before = tokio::fs::read(root.path().join("Music/existing.mp3")).await.unwrap();
    let manifest = state.device_manager.get_current_device().await.unwrap();
    let delta: crate::sync::SyncDelta = serde_json::from_value(json!({"adds":[{"jellyfinId":"track-1","name":"Track","album":"Album","artist":"Artist","sizeBytes":50000000,"providerSuffix":"flac"}],"deletes":[],"idChanges":[],"unchanged":0,"playlists":[]})).unwrap();
    assert!(!manifest.reconcile_before_sync);
    let disabled = publish_sync_delta(&state, delta.clone(), &manifest, &json!({})).await.unwrap();
    assert_eq!(disabled["adds"].as_array().unwrap().len(), 1);
    assert!(disabled["idChanges"].as_array().unwrap().is_empty());
    state.device_manager.update_manifest(|m| m.reconcile_before_sync = true).await.unwrap();
    let manifest = state.device_manager.get_current_device().await.unwrap();
    let preview = publish_sync_delta(&state, delta.clone(), &manifest, &json!({})).await.unwrap();
    assert!(preview["adds"].as_array().unwrap().is_empty());
    assert_eq!(preview["idChanges"][0]["sizeBytes"], before.len() as u64);
    assert!(state.device_manager.get_current_device().await.unwrap().synced_items.is_empty());
    let forced = publish_sync_delta(&state, delta.clone(), &manifest, &json!({"force":true})).await.unwrap();
    assert_eq!(forced["adds"].as_array().unwrap().len(), 1);
    assert!(forced["idChanges"].as_array().unwrap().is_empty());
    assert_eq!(tokio::fs::read(root.path().join("Music/existing.mp3")).await.unwrap(), before);
    let summary = publish_sync_delta(&state, delta, &manifest, &json!({"prepared":true})).await.unwrap();
    assert_eq!(summary["selectedBytes"], before.len() as u64);
    assert_eq!(summary["selectedExpectedBytes"], before.len() as u64);
    assert_eq!(summary["selectedReserveBytes"], 0);
    let started = handle_sync_execute(&state, Some(json!({"planId":summary["planId"]}))).await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while state.sync_operation_manager.has_active_operation().await {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.unwrap();
    let operation = state.sync_operation_manager.get_operation(started["operationId"].as_str().unwrap()).await.unwrap();
    assert_eq!(operation.status, crate::sync::SyncStatus::Complete);
    assert_eq!(tokio::fs::read(root.path().join("Music/existing.mp3")).await.unwrap(), before);
    let adopted = state.device_manager.get_current_device().await.unwrap();
    assert_eq!(adopted.synced_items.len(), 1);
    assert_eq!(adopted.synced_items[0].local_path, "Music/existing.mp3");
    let mut replacement: crate::sync::SyncDelta = serde_json::from_value(json!({"adds":[],"deletes":[],"idChanges":[],"unchanged":1,"playlists":[]})).unwrap();
    apply_force_sync(&mut replacement, &adopted);
    assert_eq!(replacement.adds.len(), 1);
    assert_eq!(replacement.deletes.len(), 1);
    assert_eq!(replacement.deletes[0].local_path, "Music/existing.mp3");
}

#[tokio::test]
async fn abandoned_setup_proposal_expires_and_does_not_block_sync_after_reload() {
    let _guard=credential_test_lock();
    let (_root,state,id)=setup_music_scan_fixture().await;
    assert!(music_setup_pending("prepared-plan-device").await);
    music_setup_scans().lock().await.get_mut(&id).unwrap().expires_at=std::time::Instant::now();
    assert!(!music_setup_pending("prepared-plan-device").await);
    assert!(handle_music_setup_scan(&state,"device.music_scan_start",Some(json!({"scanId":id}))).await.is_err());
}
#[tokio::test]
async fn cancellation_after_commit_decision_reports_committed() {
    let _guard=credential_test_lock();
    let (_root,state,id)=setup_music_scan_fixture().await;
    let progress={let mut scans=music_setup_scans().lock().await;let scan=scans.get_mut(&id).unwrap();scan.running=true;scan.progress.clone()};
    let gate=progress.commit_gate.lock().await;
    let request=handle_music_setup_scan(&state,"device.music_scan_cancel",Some(json!({"scanId":id})));
    tokio::pin!(request);
    assert!(tokio::time::timeout(std::time::Duration::from_millis(10),&mut request).await.is_err());
    progress.committed.store(true,std::sync::atomic::Ordering::Relaxed);
    drop(gate);
    let result=request.await.unwrap();
    assert_eq!(result["committed"],true);assert_eq!(result["cancelled"],false);
    assert!(!progress.cancelled.load(std::sync::atomic::Ordering::Relaxed));
    music_setup_scans().lock().await.remove(&id);
}
#[test]
fn disc_metadata_survives_provider_jellyfin_and_delta_mapping() {
    let mut song=crate::device_music_scan::tests::song();song.disc_number=Some(2);
    let desired=provider_song_to_desired_item(&song);
    assert_eq!(desired.disc_number,Some(2));
    let delta=crate::sync::calculate_delta(&[desired],&crate::device::DeviceManifest::default());
    assert_eq!(delta.adds[0].disc_number,Some(2));
    let restored: crate::sync::SyncDelta = serde_json::from_value(serde_json::to_value(delta).unwrap()).unwrap();
    assert_eq!(restored.adds[0].disc_number, Some(2));
    let fill: crate::auto_fill::AutoFillItem = serde_json::from_value(json!({
        "id":"one", "name":"Track", "discNumber":2, "trackNumber":3,
        "sizeBytes":1000, "priorityReason":"favorite"
    })).unwrap();
    assert_eq!(auto_fill_to_desired_item(fill, None, crate::device::MediaRole::Music, None).disc_number, Some(2));
    let item:crate::api::JellyfinItem=serde_json::from_value(json!({"Id":"one","Name":"Track","Type":"Audio","ParentIndexNumber":2})).unwrap();
    assert_eq!(jellyfin_item_to_desired_item(item).disc_number,Some(2));
}

#[tokio::test]
async fn prepared_sync_replaces_untracked_music_and_continues_in_normal_and_force_modes() {
    let _guard = credential_test_lock();
    for (folder, force) in [("Music", false), ("My Audio/Library", false), ("My Audio/Library", true)] {
        let (root, state) = sync_plan_test_state().await;
        state.device_manager.update_manifest(|m| m.managed_paths = vec![folder.into()]).await.unwrap();
        let mut server = mockito::Server::new_async().await;
        let audio = include_bytes!("../../tests/fixtures/generated-seek-mp3.mp3");
        let download = server.mock("GET", "/rest/download.view")
            .match_query(mockito::Matcher::Any).with_status(200)
            .with_header("content-type", "audio/mpeg").with_body(audio).expect(2)
            .create_async().await;
        let provider = crate::providers::subsonic::SubsonicProvider::from_stored_config(
            ProviderCredentials { server_url: server.url(), credential: CredentialKind::Password {
                username: "fixture".into(), password: "fixture".into(),
            }}, true, Some("1.16.1".into()),
        ).unwrap();
        state.server_manager.write().await.set_test_provider(Arc::new(provider));
        let relative = format!("{folder}/Ace of Base/The Sign/1-01 The Sign (radio edit).mp3");
        let destination = root.path().join(&relative);
        tokio::fs::create_dir_all(destination.parent().unwrap()).await.unwrap();
        tokio::fs::write(&destination, b"unrecognized old audio").await.unwrap();
        let unrelated = root.path().join(folder).join("unrelated.mp3");
        tokio::fs::write(&unrelated, b"unrelated original").await.unwrap();
        let manifest = state.device_manager.get_current_device().await.unwrap();
        assert!(manifest.synced_items.is_empty());
        let delta: crate::sync::SyncDelta = serde_json::from_value(json!({
            "adds":[
                {"jellyfinId":"track-1","name":"The Sign (radio edit)","album":"The Sign","artist":"Ace of Base","trackNumber":1,"sizeBytes":100,"providerSuffix":"mp3","providerContentType":"audio/mpeg"},
                {"jellyfinId":"track-2","name":"Next Track","album":"The Sign","artist":"Ace of Base","trackNumber":2,"sizeBytes":100,"providerSuffix":"mp3","providerContentType":"audio/mpeg"}
            ], "deletes":[],"idChanges":[],"unchanged":0,"playlists":[]
        })).unwrap();
        let summary = publish_sync_delta(&state, delta, &manifest, &json!({"prepared":true,"force":force})).await.unwrap();
        assert_eq!(tokio::fs::read(&destination).await.unwrap(), b"unrecognized old audio");
        assert!(state.device_manager.get_current_device().await.unwrap().synced_items.is_empty());
        let started = handle_sync_execute(&state, Some(json!({"planId":summary["planId"],"force":force}))).await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            while state.sync_operation_manager.has_active_operation().await {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }).await.unwrap();
        let operation = state.sync_operation_manager.get_operation(started["operationId"].as_str().unwrap()).await.unwrap();
        assert_eq!(operation.status, crate::sync::SyncStatus::Complete, "{:?}", operation.errors);
        assert_eq!(tokio::fs::read(&destination).await.unwrap(), audio);
        assert_eq!(tokio::fs::read(&unrelated).await.unwrap(), b"unrelated original");
        let manifest = state.device_manager.get_current_device().await.unwrap();
        assert_eq!(manifest.synced_items.len(), 2);
        assert!(manifest.synced_items.iter().all(|i| i.size_bytes == audio.len() as u64));
        assert!(manifest.synced_items.iter().any(|i| i.local_path == relative));
        assert!(manifest.synced_items.iter().all(|i| i.local_path != format!("{folder}/unrelated.mp3")));
        let durable: crate::device::DeviceManifest = crate::device::manifest_codec::decode_test_manifest(&tokio::fs::read(root.path().join(".hifimule.json.zst")).await.unwrap()).unwrap();
        assert_eq!(durable.synced_items.len(), 2);
        download.assert_async().await;
    }
}

#[tokio::test]
async fn prepared_sync_coalesces_selected_music_in_normal_and_force_modes() {
    let _guard = credential_test_lock();
    for (folder, force) in [("Music", false), ("My Audio/Library", false), ("My Audio/Library", true)] {
        let (root, state) = sync_plan_test_state().await;
        state.device_manager.update_manifest(|m| m.managed_paths = vec![folder.into()]).await.unwrap();
        let mut server = mockito::Server::new_async().await;
        let audio = include_bytes!("../../tests/fixtures/generated-seek-mp3.mp3");
        let download = server.mock("GET", "/rest/download.view")
            .match_query(mockito::Matcher::Any).with_status(200)
            .with_header("content-type", "audio/mpeg").with_body(audio).expect(1)
            .create_async().await;
        let provider = crate::providers::subsonic::SubsonicProvider::from_stored_config(
            ProviderCredentials { server_url: server.url(), credential: CredentialKind::Password {
                username: "fixture".into(), password: "fixture".into(),
            }}, true, Some("1.16.1".into()),
        ).unwrap();
        state.server_manager.write().await.set_test_provider(Arc::new(provider));
        let relative = format!("{folder}/Ace of Base/The Sign/1-01 The Sign (radio edit).mp3");
        let destination = root.path().join(&relative);
        tokio::fs::create_dir_all(destination.parent().unwrap()).await.unwrap();
        tokio::fs::write(&destination, b"unrecognized old audio").await.unwrap();
        let unrelated = root.path().join(folder).join("unrelated.mp3");
        tokio::fs::write(&unrelated, b"unrelated original").await.unwrap();
        let manifest = state.device_manager.get_current_device().await.unwrap();
        assert!(manifest.synced_items.is_empty());
        let delta: crate::sync::SyncDelta = serde_json::from_value(json!({
            "adds":[
                {"jellyfinId":"track-1","name":"The Sign (radio edit)","album":"The Sign","artist":"Ace of Base","trackNumber":1,"sizeBytes":100,"providerSuffix":"mp3","providerContentType":"audio/mpeg"},
                {"jellyfinId":"track-2","name":"The Sign (radio edit)","album":"The Sign","artist":"Ace of Base","trackNumber":1,"sizeBytes":100,"providerSuffix":"mp3","providerContentType":"audio/mpeg"}
            ], "deletes":[],"idChanges":[],"unchanged":0,"playlists":[]
        })).unwrap();
        let repeat_add = delta.adds[1].clone();
        let summary = publish_sync_delta(&state, delta, &manifest, &json!({"prepared":true,"force":force})).await.unwrap();
        assert_eq!(tokio::fs::read(&destination).await.unwrap(), b"unrecognized old audio");
        assert!(state.device_manager.get_current_device().await.unwrap().synced_items.is_empty());
        let started = handle_sync_execute(&state, Some(json!({"planId":summary["planId"],"force":force}))).await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            while state.sync_operation_manager.has_active_operation().await {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }).await.unwrap();
        let operation = state.sync_operation_manager.get_operation(started["operationId"].as_str().unwrap()).await.unwrap();
        assert_eq!(operation.status, crate::sync::SyncStatus::Complete, "{:?}", operation.errors);
        assert_eq!(tokio::fs::read(&destination).await.unwrap(), audio);
        assert_eq!(tokio::fs::read(&unrelated).await.unwrap(), b"unrelated original");
        let manifest = state.device_manager.get_current_device().await.unwrap();
        assert_eq!(manifest.synced_items.len(), 1);
        assert!(manifest.synced_items.iter().all(|i| i.size_bytes == audio.len() as u64));
        assert!(manifest.synced_items.iter().any(|i| i.local_path == relative));
        assert!(manifest.synced_items.iter().all(|i| i.local_path != format!("{folder}/unrelated.mp3")));
        let durable: crate::device::DeviceManifest = crate::device::manifest_codec::decode_test_manifest(&tokio::fs::read(root.path().join(".hifimule.json.zst")).await.unwrap()).unwrap();
        assert_eq!(durable.synced_items.len(), 1);
        // The alternate source ID remains an addition on the next normal plan,
        // but its resident destination must be reused without another request.
        let repeat_delta = crate::sync::SyncDelta {
            adds: vec![repeat_add], deletes: vec![], id_changes: vec![], unchanged: 1,
            playlists: vec![], provenance_updates: Default::default(), blocked: vec![],
            pity_fired_servers: vec![],
        };
        let repeat_summary = publish_sync_delta(&state, repeat_delta, &manifest, &json!({"prepared":true})).await.unwrap();
        let repeat_started = handle_sync_execute(&state, Some(json!({"planId":repeat_summary["planId"]}))).await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            while state.sync_operation_manager.has_active_operation().await {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }).await.unwrap();
        let repeat_operation = state.sync_operation_manager.get_operation(repeat_started["operationId"].as_str().unwrap()).await.unwrap();
        assert_eq!(repeat_operation.status, crate::sync::SyncStatus::Complete, "{:?}", repeat_operation.errors);
        assert_eq!(tokio::fs::read(&destination).await.unwrap(), audio);
        let repeat_manifest = state.device_manager.get_current_device().await.unwrap();
        assert_eq!(repeat_manifest.synced_items.len(), 1);
        assert_eq!(repeat_manifest.synced_items[0].jellyfin_id, manifest.synced_items[0].jellyfin_id);
        assert_eq!(repeat_operation.bytes_transferred, 0);
        download.assert_async().await;
    }
}


#[test]
fn basket_estimate_uses_expanded_tracks_instead_of_inconsistent_parent_duration() {
    let desired: Vec<_> = (0..91).map(|index| {
        let mut song = make_fake_song(&format!("track-{index}"), "Track");
        song.duration_seconds = if index == 90 { 385 } else { 250 };
        song.bitrate_kbps = Some(1600);
        song.size_bytes = Some(50_000_000);
        song.suffix = Some("flac".into());
        let mut item = provider_song_to_desired_item(&song);
        item.server_id = Some("source".into());
        item
    }).collect();
    let profile = json!({"MusicStreamingTranscodingBitrate":192000,"DirectPlayProfiles":[],
        "TranscodingProfiles":[{"Type":"Audio","Container":"mp3","AudioCodec":"mp3"}]});
    let estimate = basket_item_estimate("playlist", "source", &desired, &Default::default(), Some(&profile), None);
    assert_eq!(estimate["childCount"], 91);
    assert_eq!(estimate["sizeTicks"], 228850000000_u64);
    assert_eq!(estimate["expectedBytes"], 549240000_u64);
    assert_eq!(estimate["reserveBytes"], 11456176_u64);
    assert_eq!(estimate["plannedBytes"], 560696176_u64);
    assert_eq!((estimate["plannedBytes"].as_u64().unwrap() as f64 / 1048576.0).round(), 535.0);
}

#[test]
fn alchemy_99_track_breakdown_matches_one_percent_plus_per_track_overhead() {
    let tracks: Vec<_> = (0..99).map(|index| {
        let mut song = make_fake_song(&format!("track-{index}"), "Track");
        song.duration_seconds = if index == 98 { 242 } else { 280 };
        song.bitrate_kbps = Some(1600);
        song.size_bytes = Some(50_000_000);
        song.suffix = Some("flac".into());
        provider_song_to_desired_item(&song)
    }).collect();
    let profile = json!({"MusicStreamingTranscodingBitrate":192000,"DirectPlayProfiles":[],
        "TranscodingProfiles":[{"Type":"Audio","Container":"mp3","AudioCodec":"mp3"}]});
    let estimate = basket_item_estimate("alchemy", "source", &tracks, &Default::default(), Some(&profile), None);
    assert_eq!(estimate["expectedBytes"], 664368000_u64);
    assert_eq!(estimate["reserveBytes"], 13131744_u64);
    assert_eq!(estimate["plannedBytes"], 677499744_u64);
}

#[test]
fn prepared_manual_breakdown_uses_same_add_rekey_and_resident_filters_as_reserved_total() {
    let profile = json!({"MusicStreamingTranscodingBitrate":192000,"DirectPlayProfiles":[],
        "TranscodingProfiles":[{"Type":"Audio","Container":"mp3","AudioCodec":"mp3"}]});
    let mut manifest = crate::device::DeviceManifest::default();
    manifest.synced_items = serde_json::from_value(json!([
        {"providerItemId":"resident","name":"Resident","localPath":"Music/resident.mp3","sizeBytes":100,"syncedAt":"before"},
        {"providerItemId":"old","name":"Rekey","localPath":"Music/old.mp3","sizeBytes":200,"syncedAt":"before"},
        {"providerItemId":"deleted","name":"Deleted","localPath":"Music/deleted.mp3","sizeBytes":999,"syncedAt":"before"},
        {"providerItemId":"autofill","name":"Auto","localPath":"Music/auto.mp3","sizeBytes":999,"syncedAt":"before","isAutoFill":true}
    ])).unwrap();
    let mut delta: crate::sync::SyncDelta = serde_json::from_value(json!({
        "adds":[
            {"jellyfinId":"converted","name":"Converted","durationSeconds":240,"providerSuffix":"flac","sizeBytes":50000000},
            {"jellyfinId":"direct","name":"Direct","mediaRole":"podcast","sizeBytes":300},
            {"jellyfinId":"auto-new","name":"Auto","durationSeconds":240,"providerSuffix":"flac","sizeBytes":50000000,"isAutoFill":true}
        ],
        "idChanges":[{"oldJellyfinId":"old","newJellyfinId":"rekey","oldLocalPath":"Music/old.mp3","name":"Rekey","sizeBytes":200,"etag":null}],
        "deletes":[{"jellyfinId":"deleted","localPath":"Music/deleted.mp3","name":"Deleted"}],"unchanged":1,"playlists":[]
    })).unwrap();
    crate::sync::estimate_delta_outputs(&mut delta, Some(&profile), None);
    let size = prepared_manual_selection_size(&delta, &manifest, Some(&profile), None);
    assert_eq!(size.expected_bytes, 5_760_000 + 100 + 200 + 300);
    assert_eq!(size.reserve_bytes(), 57_600 + 65_536);
    assert_eq!(size.reserved_bytes, 5_883_136 + 600);
    // Force replaces the rekey: its measured bytes must no longer be counted.
    let mut replacement = delta.adds[0].clone();
    replacement.jellyfin_id = "rekey".into();
    delta.adds.push(replacement);
    let forced = prepared_manual_selection_size(&delta, &manifest, Some(&profile), None);
    assert_eq!(forced.expected_bytes, 2 * 5_760_000 + 100 + 300);
    assert_eq!(forced.reserve_bytes(), 2 * (57_600 + 65_536));
    assert_eq!(forced.reserved_bytes, 2 * 5_883_136 + 400);
}

#[tokio::test]
async fn basket_estimate_rejects_wrong_destination_and_changed_profile_without_fetching_sources() {
    let _guard = credential_test_lock();
    let (_dir, state) = sync_plan_test_state().await;
    let error = handle_basket_estimate(&state, Some(json!({"targetDeviceId":"disconnected","basketItems":[]})))
        .await.unwrap_err();
    assert!(error.message.contains("disconnected"));
    let error = handle_basket_estimate(&state, Some(json!({"targetDeviceId":"prepared-plan-device",
        "expectedProfileId":"changed-profile","basketItems":[]}))).await.unwrap_err();
    assert!(error.message.contains("profile changed"));
    let empty = handle_basket_estimate(&state, Some(json!({"targetDeviceId":"prepared-plan-device",
        "expectedProfileId":null,"basketItems":[]}))).await.unwrap();
    assert_eq!(empty["selectedBytes"], 0);
    assert_eq!(empty["expectedBytes"], 0);
    assert_eq!(empty["reserveBytes"], 0);
    assert_eq!(empty["trackCount"], 0);
    assert_eq!(empty["items"], json!([]));
}

#[test]
fn force_estimation_retains_desired_duration_source_and_is_idempotent() {
    let mut song = make_fake_song("song", "Track");
    song.disc_number = Some(2);
    song.track_number = Some(3);
    song.duration_seconds = 240;
    song.bitrate_kbps = Some(1600);
    song.size_bytes = Some(50_000_000);
    song.suffix = Some("flac".into());
    let item = provider_song_to_desired_item(&song);
    let mut manifest = crate::device::DeviceManifest::default();
    manifest.managed_paths = vec!["Music".into()];
    manifest.synced_items = serde_json::from_value(json!([{"providerItemId":"song","name":"Track",
        "localPath":"Music/Track.mp3","sizeBytes":5500000,"syncedAt":"before","providerSuffix":"flac",
        "originalBitrate":1600000}])).unwrap();
    let mut delta = crate::sync::calculate_delta(&[item.clone()], &manifest);
    assert_eq!(delta.unchanged, 1, "exercise retained-file force expansion, not an existing addition");
    assert!(delta.adds.is_empty());
    apply_force_sync_with_desired(&mut delta, &manifest, &[item]);
    let profile = json!({"MusicStreamingTranscodingBitrate":192000,"DirectPlayProfiles":[],
        "TranscodingProfiles":[{"Type":"Audio","Container":"mp3","AudioCodec":"mp3"}]});
    crate::sync::estimate_delta_outputs(&mut delta, Some(&profile), None);
    assert_eq!(delta.adds.len(), 1);
    assert_eq!(delta.adds[0].duration_seconds, 240);
    assert_eq!(delta.adds[0].disc_number, Some(2));
    assert_eq!(delta.adds[0].track_number, Some(3));
    assert_eq!(delta.adds[0].source_audio.original_size_bytes, Some(50_000_000));
    assert_eq!(delta.adds[0].size_bytes, 5_883_136);
    apply_force_sync(&mut delta, &manifest);
    crate::sync::estimate_delta_outputs(&mut delta, Some(&profile), None);
    assert_eq!(delta.adds.len(), 1);
    assert_eq!(delta.adds[0].size_bytes, 5_883_136);
}


#[test]
fn force_id_reassignment_counts_replacement_output_once_and_preserves_source_metadata() {
    let mut song = make_fake_song("new", "Track");
    song.duration_seconds = 240;
    song.size_bytes = Some(50_000_000);
    song.bitrate_kbps = Some(1600);
    song.suffix = Some("flac".into());
    let mut desired = provider_song_to_desired_item(&song);
    desired.server_id = Some("source".into());
    let mut manifest = crate::device::DeviceManifest::default();
    manifest.synced_items = serde_json::from_value(json!([{"providerItemId":"old","name":"Track",
        "localPath":"Music/old.mp3","sizeBytes":5500000,"syncedAt":"before"}])).unwrap();
    let mut delta = crate::sync::calculate_delta(&[], &Default::default());
    delta.id_changes = serde_json::from_value(json!([{"oldJellyfinId":"old","newJellyfinId":"new",
        "oldLocalPath":"Music/old.mp3","name":"Track","sizeBytes":5500000,"etag":null}])).unwrap();
    apply_force_sync_with_desired(&mut delta, &manifest, &[desired]);
    apply_force_sync(&mut delta, &manifest);
    let profile = json!({"MusicStreamingTranscodingBitrate":192000,"DirectPlayProfiles":[],
        "TranscodingProfiles":[{"Type":"Audio","Container":"mp3","AudioCodec":"mp3"}]});
    crate::sync::estimate_delta_outputs(&mut delta, Some(&profile), None);
    assert_eq!(delta.adds.len(), 1);
    assert_eq!(delta.adds[0].duration_seconds, 240);
    assert_eq!(delta.adds[0].server_id.as_deref(), Some("source"));
    assert_eq!(delta.id_changes.len(), 1, "retain old/new linkage for safe blocked-file handling");
    let plan = crate::sync_plan::SyncPlan::from_delta(delta, &manifest).unwrap();
    assert_eq!(plan.total_bytes, 5_883_136, "metadata rekey is not a second audio output");
}


#[test]
fn backend_manual_budget_distinguishes_retained_bytes_from_force_replacements() {
    let mut song = make_fake_song("song", "Track");
    song.duration_seconds = 240;
    song.bitrate_kbps = Some(1600);
    song.size_bytes = Some(50_000_000);
    song.suffix = Some("flac".into());
    let mut desired = provider_song_to_desired_item(&song);
    desired.server_id = Some("source".into());
    let mut manifest = crate::device::DeviceManifest::default();
    manifest.managed_paths = vec!["Music".into()];
    manifest.synced_items = serde_json::from_value(json!([{"providerItemId":"song","name":"Track",
        "localPath":"Music/Track.mp3","sizeBytes":5000000,"syncedAt":"before","originalBitrate":1600000,
        "serverId":"source"}])).unwrap();
    let profile = json!({"MusicStreamingTranscodingBitrate":192000,"DirectPlayProfiles":[],
        "TranscodingProfiles":[{"Type":"Audio","Container":"mp3","AudioCodec":"mp3"}]});
    assert_eq!(planned_manual_selection_bytes(&[desired.clone()], &manifest, Some(&profile), None, false), 5_000_000);
    assert_eq!(planned_manual_selection_bytes(&[desired], &manifest, Some(&profile), None, true), 5_883_136);
    assert_eq!(manifest.synced_items.len(), 1, "budgeting must preserve the real cleanup manifest");
}


#[tokio::test]
async fn missing_resident_files_are_not_credited_as_reusable_or_reclaimable_capacity() {
    let _guard = credential_test_lock();
    let (dir, state) = sync_plan_test_state().await;
    tokio::fs::create_dir_all(dir.path().join("Music")).await.unwrap();
    tokio::fs::write(dir.path().join("Music/present.mp3"), b"present").await.unwrap();
    let mut manifest = state.device_manager.get_current_device().await.unwrap();
    manifest.synced_items = serde_json::from_value(json!([
        {"providerItemId":"song","name":"Track","localPath":"Music/missing.mp3","sizeBytes":5000000,
            "syncedAt":"before","originalBitrate":1600000,"serverId":"source"},
        {"providerItemId":"present","name":"Present","localPath":"Music/present.mp3","sizeBytes":1000000,"syncedAt":"before"}
    ])).unwrap();
    let verified = verified_capacity_manifest(&state, &manifest).await.unwrap();
    assert_eq!(verified.synced_items.len(), 1);
    assert_eq!(verified.synced_items[0].jellyfin_id, "present");
    let mut song = make_fake_song("song", "Track");
    song.duration_seconds = 240;
    song.bitrate_kbps = Some(1600);
    song.size_bytes = Some(50_000_000);
    song.suffix = Some("flac".into());
    let mut desired = provider_song_to_desired_item(&song);
    desired.server_id = Some("source".into());
    let profile = json!({"MusicStreamingTranscodingBitrate":192000,"DirectPlayProfiles":[],
        "TranscodingProfiles":[{"Type":"Audio","Container":"mp3","AudioCodec":"mp3"}]});
    assert_eq!(planned_manual_selection_bytes(&[desired], &verified, Some(&profile), None, false), 5_883_136);
    assert_eq!(verified.synced_items.iter().map(|item| item.size_bytes).sum::<u64>(), 1_000_000);
    assert_eq!(manifest.synced_items.len(), 2, "verification must not alter cleanup provenance");
}

#[test]
fn exact_audio_bitrate_survives_the_song_adapter_at_transcode_boundaries() {
    let item: crate::api::JellyfinItem = serde_json::from_value(json!({"Id":"track","Name":"Track","Type":"Audio",
        "RunTimeTicks":2400000000_u64,"MediaSources":[{"Container":"mp3","Size":12000000,
        "Bitrate":200000,"MediaStreams":[{"Type":"Audio","BitRate":192500}]}]})).unwrap();
    let raw = jellyfin_item_to_desired_item(item.clone());
    let mut song = make_fake_song("track", "Track");
    song.duration_seconds = 240;
    song.bitrate_kbps = Some(192);
    song.size_bytes = Some(12_000_000);
    song.suffix = Some("mp3".into());
    song.provider_metadata.source_audio = item.source_audio_properties();
    let adapted = provider_song_to_desired_item(&song);
    assert_eq!(adapted.original_bitrate, Some(192500));
    assert_eq!(adapted.original_bitrate, raw.original_bitrate);
    let profile = json!({"MusicStreamingTranscodingBitrate":192000,
        "DirectPlayProfiles":[{"Type":"Audio","Container":"mp3","AudioCodec":"mp3"}],
        "TranscodingProfiles":[{"Type":"Audio","Container":"mp3","AudioCodec":"mp3"}]});
    let expected = crate::sync::planned_selection_bytes(&[raw], &Default::default(), Some(&profile), None);
    assert_eq!(crate::sync::planned_selection_bytes(&[adapted], &Default::default(), Some(&profile), None), expected);
    assert_eq!(crate::sync::planned_song_bytes(&song, Some(&profile), None), expected);
    assert_eq!(expected, 5_883_136);
}

#[test]
fn album_artist_survives_manual_and_auto_fill_adapters() {
    let item: crate::api::JellyfinItem = serde_json::from_value(json!({
        "Id":"guest", "Name":"Duet", "Type":"Audio", "Album":"Album",
        "AlbumArtist":"Main", "Artists":["Main feat. Guest"]
    }))
    .unwrap();
    let legacy = jellyfin_item_to_desired_item(item.clone());
    assert_eq!(legacy.artist.as_deref(), Some("Main"));
    assert_eq!(legacy.album_artist.as_deref(), Some("Main"));
    let song = crate::providers::jellyfin::song_from_item(item);
    let manual = provider_song_to_desired_item(&song);
    assert_eq!(manual.artist.as_deref(), Some("Main feat. Guest"));
    assert_eq!(manual.album_artist.as_deref(), Some("Main"));
    let fill: crate::auto_fill::AutoFillItem = serde_json::from_value(json!({
        "id":"guest", "name":"Duet", "album":"Album", "artist":"Main feat. Guest",
        "albumArtist":"Main", "sizeBytes":1000, "priorityReason":"favorite"
    }))
    .unwrap();
    let automatic = auto_fill_to_desired_item(fill, None, crate::device::MediaRole::Music, None);
    assert_eq!(automatic.artist, manual.artist);
    assert_eq!(automatic.album_artist, manual.album_artist);
    let mut legacy_song = serde_json::to_value(&song).unwrap();
    legacy_song.as_object_mut().unwrap().remove("albumArtist");
    let legacy_song: Song = serde_json::from_value(legacy_song).unwrap();
    assert_eq!(legacy_song.album_artist, None);
    assert_eq!(legacy_song.artist_name, song.artist_name);
}
