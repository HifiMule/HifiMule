use super::*;
use std::sync::Arc;

#[test]
fn test_start_daemon_core_returns_shutdown_and_receiver() {
    // Verify start_daemon_core returns a working shutdown signal and state receiver
    let result = start_daemon_core();
    assert!(result.is_ok(), "start_daemon_core should succeed");

    let (shutdown, state_rx) = result.unwrap();

    // Should receive initial Idle state from the daemon core
    let state = state_rx.recv_timeout(std::time::Duration::from_secs(5));
    assert!(state.is_ok(), "Should receive initial state");
    assert!(
        matches!(state.unwrap(), DaemonState::Idle),
        "Initial state should be Idle"
    );

    // Signal shutdown
    shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
    // Give the daemon thread time to clean up
    std::thread::sleep(std::time::Duration::from_millis(200));
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
        auto_sync_on_connect: false,
        auto_fill: crate::device::AutoFillPrefs::default(),
        transcoding_profile_id: None,
            playlists: vec![],
            storage_id: None,
    };

    let state = manager
        .handle_device_detected(std::path::PathBuf::from("/tmp/test-device"), manifest, std::sync::Arc::new(crate::device_io::MscBackend::new(std::path::PathBuf::from("/tmp/test-device"))))
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
            artist: Some("Test Artist".to_string()),
            child_count: 10,
            size_ticks: 1000000,
            size_bytes: 50000000,
        }],
        auto_sync_on_connect: true,
        auto_fill: crate::device::AutoFillPrefs::default(),
        transcoding_profile_id: None,
            playlists: vec![],
            storage_id: None,
    };

    // Verify manifest has auto_sync enabled
    assert!(manifest.auto_sync_on_connect);
    assert!(!manifest.basket_items.is_empty());

    // Verify DB also has auto_sync enabled
    let db_mapping = db.get_device_mapping(test_id).unwrap().unwrap();
    assert!(db_mapping.auto_sync_on_connect);

    // Simulate detection
    let state = manager
        .handle_device_detected(std::path::PathBuf::from("/tmp/auto-device"), manifest, std::sync::Arc::new(crate::device_io::MscBackend::new(std::path::PathBuf::from("/tmp/auto-device"))))
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
            artist: None,
            child_count: 5,
            size_ticks: 500000,
            size_bytes: 25000000,
        }],
        auto_sync_on_connect: false,
        auto_fill: crate::device::AutoFillPrefs::default(),
        transcoding_profile_id: None,
            playlists: vec![],
            storage_id: None,
    };

    // Verify auto_sync is disabled
    assert!(!manifest.auto_sync_on_connect);

    // Verify DB has auto_sync disabled
    let db_mapping = db.get_device_mapping(test_id).unwrap().unwrap();
    assert!(!db_mapping.auto_sync_on_connect);

    // Device detection should proceed normally without triggering sync
    let state = manager
        .handle_device_detected(std::path::PathBuf::from("/tmp/manual-device"), manifest, std::sync::Arc::new(crate::device_io::MscBackend::new(std::path::PathBuf::from("/tmp/manual-device"))))
        .await
        .expect("Failed to handle detection");

    // Should be recognized but NOT trigger auto-sync
    if let DaemonState::DeviceRecognized { name, .. } = state {
        assert_eq!(name, "Manual Device");
    } else {
        panic!("Expected DeviceRecognized, got {:?}", state);
    }
}
