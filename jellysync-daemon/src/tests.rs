use super::*;
use std::fs;

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
fn test_load_icon_empty_bytes() {
    // Verify that loading invalid bytes returns an error
    let bytes: [u8; 0] = [];
    let res = load_icon(&bytes, "test_icon");
    assert!(res.is_err());
}

#[test]
fn test_load_icon_garbage_bytes() {
    // Verify that loading garbage bytes returns an error
    let bytes = vec![0, 1, 2, 3];
    let res = load_icon(&bytes, "test_icon");
    assert!(res.is_err());
}

#[test]
fn test_file_storage() {
    use crate::api::FileCredentialManager;
    use std::path::PathBuf;

    let test_url = "http://localhost:8096";
    let test_token = "test-token-123";

    // Use a temporary file for testing
    let temp_creds_path = PathBuf::from("test_credentials.json");
    // Ensure clean state
    if temp_creds_path.exists() {
        fs::remove_file(&temp_creds_path).expect("Failed to clean up old test file");
    }

    FileCredentialManager::set_credentials_path(temp_creds_path.clone());

    // Test Save
    FileCredentialManager::save_credentials(test_url, test_token).expect("Failed to save");

    assert!(temp_creds_path.exists());

    // Test Get
    let (url, token) = FileCredentialManager::get_credentials().expect("Failed to retrieve");

    assert_eq!(url, test_url);
    assert_eq!(token, test_token);

    // Clean up
    fs::remove_file(temp_creds_path).expect("Failed to delete test file");
}
