use super::*;
use std::fs;
use std::sync::Arc;

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

#[test]
fn test_file_storage() {
    use crate::api::CredentialManager;
    use std::path::PathBuf;

    let test_url = "http://localhost:8096";
    let test_token = "test-token-1234567890";

    // Use a temporary file for testing
    let temp_config_path = PathBuf::from("test_config.json");
    // Ensure clean state
    if temp_config_path.exists() {
        let _ = fs::remove_file(&temp_config_path);
    }

    CredentialManager::set_config_path(temp_config_path.clone());

    // Test Save
    CredentialManager::save_credentials(test_url, test_token).expect("Failed to save");

    assert!(temp_config_path.exists());

    // Test Get
    let (url, token) = CredentialManager::get_credentials().expect("Failed to retrieve");

    assert_eq!(url, test_url);
    assert_eq!(token, test_token);

    // Clean up
    let _ = fs::remove_file(temp_config_path);
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
    };

    let state = manager
        .handle_device_detected(manifest)
        .await
        .expect("Failed to handle detection");

    // 5. Verify Recognized state
    if let DaemonState::DeviceRecognized { name, profile_id } = state {
        assert_eq!(name, "Test Phone");
        assert_eq!(profile_id, test_profile);
    } else {
        panic!("Expected DeviceRecognized state, got {:?}", state);
    }

    // 6. Simulate removal
    manager.handle_device_removed().await;
    let device = manager.get_current_device().await;
    assert!(device.is_none());
}
