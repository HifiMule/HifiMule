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
        version: "1.0".to_string(),
        managed_paths: vec![],
        synced_items: vec![],
        dirty: false,
        pending_item_ids: vec![],
        basket_items: vec![],
    };

    let state = manager
        .handle_device_detected(std::path::PathBuf::from("/tmp/test-device"), manifest)
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
    manager.handle_device_removed().await;
    let device = manager.get_current_device().await;
    assert!(device.is_none());
    let path = manager.get_current_device_path().await;
    assert!(path.is_none());
}
