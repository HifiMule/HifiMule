use super::*;
use std::sync::Arc;

#[tokio::test]
async fn manifest_migration_regression_auto_completion_records_operation_profile_snapshot() {
    for (completed, current, expected_dirty) in [
        (Some("rockbox-mp3-320"), None, true),
        (Some("rockbox-mp3-320"), Some("other-profile"), true),
        (None, Some("rockbox-mp3-320"), true),
        (Some("passthrough"), None, false),
        (Some("rockbox-mp3-320"), Some("rockbox-mp3-320"), false),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let mut manifest = device::DeviceManifest {
            device_id: "auto-operation-profile".into(),
            version: "1.0".into(),
            managed_paths: vec!["Music".into()],
            transcoding_profile_id: completed.map(str::to_string),
            last_synced_transcoding_profile_id: Some("passthrough".into()),
            transcoding_profile_dirty: true,
            dirty: true,
            pending_item_ids: vec!["unfinished".into()],
            ..Default::default()
        };
        manifest.synced_items = serde_json::from_str(r#"[{"providerItemId":"track","name":"Track","localPath":"Music/Track.mp3","sizeBytes":1234,"syncedAt":"2026-10-11T00:00:00Z"}]"#).unwrap();
        let manager = device::DeviceManager::new(Arc::new(db::Database::memory().unwrap()));
        let io = Arc::new(device_io::MscBackend::new(dir.path().into()));
        device::write_manifest(io.clone(), &manifest).await.unwrap();
        manager
            .handle_device_detected(dir.path().into(), manifest.clone(), io)
            .await
            .unwrap();
        let target: sync::SyncTarget = manager
            .get_sync_target_for_device(&manifest.device_id)
            .await
            .unwrap()
            .into();
        let operations = sync::SyncOperationManager::new();
        manager
            .admit_sync_operation(&operations, "operation".into(), 1, &manifest.device_id)
            .await
            .unwrap();
        manager
            .update_manifest(|m| m.set_transcoding_profile(current.map(str::to_string)))
            .await
            .unwrap();
        let (status, _) = operations
            .finalize_operation("operation", vec![], || async {
                manager
                    .update_manifest_for_connection(
                        &manifest.device_id,
                        target.observation,
                        &target.io,
                        |m| {
                            finish_auto_sync_manifest(
                                m,
                                target.manifest.transcoding_profile_id.as_deref(),
                            )
                        },
                    )
                    .await
            })
            .await;
        assert_eq!(status, sync::SyncStatus::Complete);
        let durable = device::DeviceProber::probe(dir.path())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            durable.last_synced_transcoding_profile_id.as_deref(),
            completed,
            "record actual operation profile"
        );
        assert_eq!(durable.transcoding_profile_id.as_deref(), current);
        assert_eq!(
            durable.transcoding_profile_dirty, expected_dirty,
            "retain a selection changed during execution"
        );
        assert!(!durable.dirty && durable.pending_item_ids.is_empty());
    }
}

#[tokio::test]
async fn manifest_migration_regression_auto_finalization_preserves_profile_history() {
    for scenario in ["failed", "cancelled", "complete"] {
        let dir = tempfile::tempdir().unwrap();
        let manifest = device::DeviceManifest {
            device_id: "auto-profile-history".into(),
            version: "1.0".into(),
            managed_paths: vec!["Music".into()],
            transcoding_profile_id: Some("rockbox-mp3-320".into()),
            last_synced_transcoding_profile_id: Some("passthrough".into()),
            transcoding_profile_dirty: true,
            dirty: true,
            pending_item_ids: vec!["unfinished".into()],
            synced_items: serde_json::from_str(r#"[{"providerItemId":"track","name":"Track",
                "localPath":"Music/Track.mp3","sizeBytes":1234,"syncedAt":"2026-10-11T00:00:00Z"}]"#).unwrap(),
            ..Default::default()
        };
        let manager = device::DeviceManager::new(Arc::new(db::Database::memory().unwrap()));
        let io = Arc::new(device_io::MscBackend::new(dir.path().into()));
        device::write_manifest(io.clone(), &manifest).await.unwrap();
        manager
            .handle_device_detected(dir.path().into(), manifest.clone(), io)
            .await
            .unwrap();
        let target: sync::SyncTarget = manager
            .get_sync_target_for_device(&manifest.device_id)
            .await
            .unwrap()
            .into();
        let operations = sync::SyncOperationManager::new();
        manager
            .admit_sync_operation(&operations, scenario.into(), 1, &manifest.device_id)
            .await
            .unwrap();
        let errors = if scenario == "failed" {
            vec![sync::SyncFileError {
                jellyfin_id: "unfinished".into(),
                filename: "unfinished.flac".into(),
                error_message: "transfer failed".into(),
            }]
        } else {
            vec![]
        };
        if scenario == "cancelled" {
            operations.request_cancel(scenario).await;
        }
        let (status, _) = operations
            .finalize_operation(scenario, errors, || async {
                manager
                    .update_manifest_for_connection(
                        &manifest.device_id,
                        target.observation,
                        &target.io,
                        |m| {
                            finish_auto_sync_manifest(
                                m,
                                target.manifest.transcoding_profile_id.as_deref(),
                            )
                        },
                    )
                    .await
            })
            .await;
        let mut durable = device::DeviceProber::probe(dir.path())
            .await
            .unwrap()
            .unwrap();
        if scenario == "complete" {
            assert_eq!(status, sync::SyncStatus::Complete);
            assert!(!durable.dirty && durable.pending_item_ids.is_empty());
            assert!(!durable.transcoding_profile_dirty);
            assert!(durable.last_sync_at.is_some());
            durable.set_transcoding_profile(None);
            let item = &durable.synced_items[0];
            let desired = sync::DesiredItem {source_audio: Default::default(), disc_number: None,
                duration_seconds: 0,
                media_role: item.media_role.clone(),
                jellyfin_id: item.jellyfin_id.clone(),
                name: item.name.clone(),
                album: item.album.clone(),
                artist: item.artist.clone(),
                size_bytes: item.size_bytes,
                etag: item.etag.clone(),
                provider_album_id: item.provider_album_id.clone(),
                provider_content_type: item.provider_content_type.clone(),
                provider_suffix: item.provider_suffix.clone(),
                original_bitrate: item.original_bitrate,
                track_number: item.track_number,
                server_id: item.server_id.clone(),
            };
            let delta = sync::calculate_delta(&[desired], &durable);
            assert_eq!(
                delta.adds.len(),
                1,
                "completed real profile must be replaced when returning to passthrough"
            );
            assert_eq!(delta.deletes.len(), 1);
            assert_eq!(
                delta.adds[0].reason_code.as_deref(),
                Some("transcoding-profile-change")
            );
            assert_eq!(
                durable.last_synced_transcoding_profile_id.as_deref(),
                Some("rockbox-mp3-320")
            );
        } else {
            assert_eq!(
                status,
                if scenario == "failed" {
                    sync::SyncStatus::Failed
                } else {
                    sync::SyncStatus::Cancelled
                }
            );
            assert_eq!(
                serde_json::to_value(durable).unwrap(),
                serde_json::to_value(manifest).unwrap(),
                "{scenario} retains all recovery metadata"
            );
        }
    }
}

#[test]
fn manifest_migration_regression_auto_profile_loading_rejects_missing_selection() {
    assert!(
        load_auto_sync_transcoding_profile(Some("missing-profile-migration-regression-6c55b076"))
            .is_err()
    );
}

#[tokio::test]
async fn manifest_migration_regression_auto_missing_profile_rejected_before_admission() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = device::DeviceManifest {
        device_id: "auto-missing-profile".into(),
        version: "1.0".into(),
        transcoding_profile_id: Some("missing-profile-migration-regression-6c55b076".into()),
        last_synced_transcoding_profile_id: Some("passthrough".into()),
        transcoding_profile_dirty: true,
        dirty: true,
        pending_item_ids: vec!["unfinished".into()],
        ..Default::default()
    };
    let manager = Arc::new(device::DeviceManager::new(Arc::new(
        db::Database::memory().unwrap(),
    )));
    let io = Arc::new(device_io::MscBackend::new(dir.path().into()));
    device::write_manifest(io.clone(), &manifest).await.unwrap();
    manager
        .handle_device_detected(dir.path().into(), manifest.clone(), io)
        .await
        .unwrap();
    let operations = Arc::new(sync::SyncOperationManager::new());
    let provider = Arc::new(providers::jellyfin::JellyfinProvider::new(
        api::JellyfinClient::new(),
        "http://127.0.0.1:1",
        "unused",
        "unused",
    ));
    let (tx, rx) = std::sync::mpsc::channel();
    let protection = sync::protection::Publisher::default();
    let result = run_auto_sync_via_provider(
        provider,
        None,
        Default::default(),
        manager.clone(),
        operations.clone(),
        tx,
        manifest.device_id.clone(),
        protection.observer(),
    )
    .await;
    assert!(
        result.is_err(),
        "missing selected profile must not fall back to passthrough"
    );
    assert!(operations.get_all_operations().await.is_empty());
    assert!(matches!(rx.try_iter().last(), Some(DaemonState::Error)));
    let durable = device::DeviceProber::probe(dir.path())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::to_value(durable).unwrap(),
        serde_json::to_value(manifest).unwrap()
    );
}

#[test]
fn test_start_daemon_core_returns_shutdown_and_receiver() {
    let temp_dir = tempfile::tempdir().unwrap();
    crate::paths::set_test_app_data_dir(temp_dir.path().to_path_buf());

    let mut owner = hifimule_lifecycle::OwnerGuard::acquire(temp_dir.path()).unwrap();
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    let descriptor = owner
        .prepare_descriptor(listener.local_addr().unwrap().port())
        .unwrap();
    // Verify start_daemon_core returns lifecycle readiness and state.
    let result = start_daemon_core(listener, descriptor.clone());
    assert!(result.is_ok(), "start_daemon_core should succeed");

    let core = result.unwrap();
    assert!(
        core.ready_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap()
            .is_ok()
    );
    owner.publish_descriptor(&descriptor).unwrap();

    // Should receive initial Idle state from the daemon core
    let state = core
        .state_rx
        .recv_timeout(std::time::Duration::from_secs(5));
    assert!(state.is_ok(), "Should receive initial state");
    assert!(
        matches!(state.unwrap(), DaemonState::Idle),
        "Initial state should be Idle"
    );

    // Signal shutdown
    core.shutdown
        .store(true, std::sync::atomic::Ordering::Relaxed);
    // Give the daemon thread time to clean up
    std::thread::sleep(std::time::Duration::from_millis(200));
    crate::paths::clear_test_app_data_dir();
}

#[test]
fn test_daemon_state_variants() {
    // Verify all variants can be constructed and debugged
    let idle = DaemonState::Idle;
    let syncing = DaemonState::Syncing;
    let error = DaemonState::Error;

    assert_eq!(format!("{:?}", idle), "Idle");
    assert_eq!(format!("{:?}", syncing), "Syncing");
    assert_eq!(format!("{:?}", error), "Error");
}

#[tokio::test]
async fn test_device_recognition_integration() {
    use crate::db::Database;
    use crate::device::{DeviceManager, DeviceManifest};

    // 1. Setup in-memory DB
    let db = Arc::new(Database::memory().unwrap());

    // 2. Add a mapping for a test device
    let test_id = "test-device-uuid-123";
    let test_profile = "jellyfin-user-abc";
    db.upsert_device_mapping(test_id, Some("Test Phone"), Some(test_profile), None)
        .unwrap();

    // 3. Initialize DeviceManager
    let manager = DeviceManager::new(db.clone());

    // 4. Simulate device detection
    let manifest = DeviceManifest {
        device_id: test_id.to_string(),
        name: Some("Test Phone".to_string()),
        icon: None,
        version: "1.0".to_string(),
        managed_paths: vec![],
        synced_items: vec![],
        dirty: false,
        pending_item_ids: vec![],
        basket_items: vec![],
        reconcile_before_sync: false,
        auto_sync_on_connect: false,
        auto_fill: crate::device::AutoFillConfig::default(),
        transcoding_profile_id: None,
        playlists: vec![],
        storage_id: None,
        ..Default::default()
    };

    let state = manager
        .handle_device_detected(
            std::path::PathBuf::from("/tmp/test-device"),
            manifest,
            std::sync::Arc::new(crate::device_io::MscBackend::new(std::path::PathBuf::from(
                "/tmp/test-device",
            ))),
        )
        .await
        .expect("Failed to handle detection");

    // 5. Verify Recognized state
    if let DaemonState::DeviceRecognized { name, profile_id } = state {
        assert_eq!(name, "Test Phone");
        assert_eq!(profile_id, test_profile);
    } else {
        panic!("Expected DeviceRecognized state, got {:?}", state);
    }

    // 6. Verify path is stored
    let path = manager.get_current_device_path().await;
    assert_eq!(path, Some(std::path::PathBuf::from("/tmp/test-device")));

    // 7. Verify storage info returns Some (on a real path, it should return info)
    // Note: /tmp/test-device probably doesn't exist, so get_device_storage may return None
    // This is expected behavior per T2.3

    // 8. Simulate removal
    let removed_path = std::path::PathBuf::from("/tmp/test-device");
    manager.handle_device_removed(&removed_path).await;
    let device = manager.get_current_device().await;
    assert!(device.is_none());
    let path = manager.get_current_device_path().await;
    assert!(path.is_none());
}

/// Integration test: device detection with auto_sync_on_connect enabled
/// verifies the manifest flag is correctly read and the device is recognized.
#[tokio::test]
async fn test_device_detection_auto_sync_enabled() {
    use crate::db::Database;
    use crate::device::{BasketItem, DeviceManager, DeviceManifest};

    let db = Arc::new(Database::memory().unwrap());
    let test_id = "auto-sync-device-001";
    db.upsert_device_mapping(test_id, Some("Auto Device"), Some("user-1"), None)
        .unwrap();
    db.set_auto_sync_on_connect(test_id, true).unwrap();

    let manager = DeviceManager::new(db.clone());

    let manifest = DeviceManifest {
        device_id: test_id.to_string(),
        name: Some("Auto Device".to_string()),
        icon: None,
        version: "1.0".to_string(),
        managed_paths: vec!["Music".to_string()],
        synced_items: vec![],
        dirty: false,
        pending_item_ids: vec![],
        basket_items: vec![BasketItem {
            id: "album-1".to_string(),
            name: "Test Album".to_string(),
            item_type: "MusicAlbum".to_string(),
            server_id: None,
            artist: Some("Test Artist".to_string()),
            child_count: 10,
            size_ticks: 1000000,
            size_bytes: 50000000,
        }],
        reconcile_before_sync: false,
        auto_sync_on_connect: true,
        auto_fill: crate::device::AutoFillConfig::default(),
        transcoding_profile_id: None,
        playlists: vec![],
        storage_id: None,
        ..Default::default()
    };

    // Verify manifest has auto_sync enabled
    assert!(manifest.auto_sync_on_connect);
    assert!(!manifest.basket_items.is_empty());

    // Verify DB also has auto_sync enabled
    let db_mapping = db.get_device_mapping(test_id).unwrap().unwrap();
    assert!(db_mapping.auto_sync_on_connect);

    // Simulate detection
    let state = manager
        .handle_device_detected(
            std::path::PathBuf::from("/tmp/auto-device"),
            manifest,
            std::sync::Arc::new(crate::device_io::MscBackend::new(std::path::PathBuf::from(
                "/tmp/auto-device",
            ))),
        )
        .await
        .expect("Failed to handle detection");

    if let DaemonState::DeviceRecognized { name, .. } = state {
        assert_eq!(name, "Auto Device");
    } else {
        panic!("Expected DeviceRecognized, got {:?}", state);
    }
}

/// Integration test: auto-sync disabled → no sync triggered
#[tokio::test]
async fn test_device_detection_auto_sync_disabled() {
    use crate::db::Database;
    use crate::device::{BasketItem, DeviceManager, DeviceManifest};

    let db = Arc::new(Database::memory().unwrap());
    let test_id = "no-auto-sync-device";
    db.upsert_device_mapping(test_id, Some("Manual Device"), Some("user-2"), None)
        .unwrap();
    // auto_sync_on_connect defaults to false in DB

    let manager = DeviceManager::new(db.clone());

    let manifest = DeviceManifest {
        device_id: test_id.to_string(),
        name: Some("Manual Device".to_string()),
        icon: None,
        version: "1.0".to_string(),
        managed_paths: vec!["Music".to_string()],
        synced_items: vec![],
        dirty: false,
        pending_item_ids: vec![],
        basket_items: vec![BasketItem {
            id: "album-2".to_string(),
            name: "Another Album".to_string(),
            item_type: "MusicAlbum".to_string(),
            server_id: None,
            artist: None,
            child_count: 5,
            size_ticks: 500000,
            size_bytes: 25000000,
        }],
        reconcile_before_sync: false,
        auto_sync_on_connect: false,
        auto_fill: crate::device::AutoFillConfig::default(),
        transcoding_profile_id: None,
        playlists: vec![],
        storage_id: None,
        ..Default::default()
    };

    // Verify auto_sync is disabled
    assert!(!manifest.auto_sync_on_connect);

    // Verify DB has auto_sync disabled
    let db_mapping = db.get_device_mapping(test_id).unwrap().unwrap();
    assert!(!db_mapping.auto_sync_on_connect);

    // Device detection should proceed normally without triggering sync
    let state = manager
        .handle_device_detected(
            std::path::PathBuf::from("/tmp/manual-device"),
            manifest,
            std::sync::Arc::new(crate::device_io::MscBackend::new(std::path::PathBuf::from(
                "/tmp/manual-device",
            ))),
        )
        .await
        .expect("Failed to handle detection");

    // Should be recognized but NOT trigger auto-sync
    if let DaemonState::DeviceRecognized { name, .. } = state {
        assert_eq!(name, "Manual Device");
    } else {
        panic!("Expected DeviceRecognized, got {:?}", state);
    }
}


#[tokio::test]
async fn auto_sync_capacity_io_failure_exits_syncing_before_admission() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = device::DeviceManifest {
        device_id: "auto-capacity-error".into(), version: "1.1".into(),
        auto_fill: device::AutoFillConfig { legacy: Some(device::AutoFillPrefs { enabled: true, max_bytes: None }),
            ..Default::default() },
        ..Default::default()
    };
    let manager = Arc::new(device::DeviceManager::new(Arc::new(db::Database::memory().unwrap())));
    manager.handle_device_detected(dir.path().into(), manifest.clone(),
        Arc::new(device_io::MscBackend::new(dir.path().into()))).await.unwrap();
    tokio::fs::remove_dir_all(dir.path()).await.unwrap();
    let provider = Arc::new(providers::jellyfin::JellyfinProvider::new(api::JellyfinClient::new(),
        "http://127.0.0.1:1", "unused", "unused"));
    let operations = Arc::new(sync::SyncOperationManager::new());
    let (tx, rx) = std::sync::mpsc::channel();
    let protection = sync::protection::Publisher::default();
    let result = run_auto_sync_via_provider(provider, None, Default::default(), manager, operations.clone(), tx,
        manifest.device_id, protection.observer()).await;
    assert!(result.is_err());
    let states: Vec<_> = rx.try_iter().collect();
    assert!(matches!(states.first(), Some(DaemonState::Syncing)));
    assert!(matches!(states.last(), Some(DaemonState::Error)));
    assert!(operations.get_all_operations().await.is_empty());
}
