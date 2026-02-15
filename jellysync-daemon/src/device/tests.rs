use super::*;
use std::fs;
use std::sync::Arc;
use tempfile::tempdir;

#[tokio::test]
async fn test_probe_no_manifest() {
    let dir = tempdir().unwrap();
    let res = DeviceProber::probe(dir.path()).await.unwrap();
    assert!(res.is_none());
}

#[tokio::test]
async fn test_probe_with_manifest() {
    let dir = tempdir().unwrap();
    let manifest_path = dir.path().join(".jellysync.json");
    let manifest_json = r#"{"device_id": "test-device-123", "name": "My iPod", "version": "1.0"}"#;
    fs::write(manifest_path, manifest_json).unwrap();

    let res = DeviceProber::probe(dir.path()).await.unwrap();
    assert!(res.is_some());
    let manifest = res.unwrap();
    assert_eq!(manifest.device_id, "test-device-123");
    assert_eq!(manifest.name, Some("My iPod".to_string()));
    assert_eq!(manifest.version, "1.0".to_string());
    assert!(manifest.managed_paths.is_empty());
}

#[tokio::test]
async fn test_probe_with_managed_paths() {
    let dir = tempdir().unwrap();
    let manifest_path = dir.path().join(".jellysync.json");
    let manifest_json = r#"{"device_id": "test-device-123", "name": "My iPod", "version": "1.0", "managed_paths": ["Music", "Podcasts"]}"#;
    fs::write(manifest_path, manifest_json).unwrap();

    let res = DeviceProber::probe(dir.path()).await.unwrap();
    assert!(res.is_some());
    let manifest = res.unwrap();
    assert_eq!(
        manifest.managed_paths,
        vec!["Music".to_string(), "Podcasts".to_string()]
    );
}

#[tokio::test]
async fn test_probe_invalid_manifest() {
    let dir = tempdir().unwrap();
    let manifest_path = dir.path().join(".jellysync.json");
    let manifest_json = r#"{"id": "test-device-123", "invalid": true"#; // Missing closing brace
    fs::write(manifest_path, manifest_json).unwrap();

    let res = DeviceProber::probe(dir.path()).await;
    assert!(res.is_err());
}

#[tokio::test]
async fn test_list_root_folders_mixed() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // Create directories
    fs::create_dir(root.join("Music")).unwrap();
    fs::create_dir(root.join("Notes")).unwrap();
    fs::create_dir(root.join("Podcasts")).unwrap();
    fs::create_dir(root.join(".hidden")).unwrap();
    fs::create_dir(root.join("System Volume Information")).unwrap();
    fs::write(root.join("file.txt"), "not a dir").unwrap();

    // Create manifest
    let manifest_path = root.join(".jellysync.json");
    let manifest_json = r#"{"device_id": "test-id", "name": "Test Device", "version": "1.0", "managed_paths": ["Music"]}"#;
    fs::write(manifest_path, manifest_json).unwrap();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    // Simulate detection
    let manifest: DeviceManifest = serde_json::from_str(manifest_json).unwrap();
    manager
        .handle_device_detected(root.to_path_buf(), manifest)
        .await
        .unwrap();

    let res = manager.list_root_folders().await.unwrap().unwrap();

    assert_eq!(res.folders.len(), 3);
    assert_eq!(res.managed_count, 1);
    assert_eq!(res.unmanaged_count, 2);

    // Sorted alphabetically
    assert_eq!(res.folders[0].name, "Music");
    assert!(res.folders[0].is_managed);

    assert_eq!(res.folders[1].name, "Notes");
    assert!(!res.folders[1].is_managed);

    assert_eq!(res.folders[2].name, "Podcasts");
    assert!(!res.folders[2].is_managed);
}

#[tokio::test]
async fn test_list_root_folders_empty_device() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // Create manifest but no folders (only hidden/system entries)
    let manifest_path = root.join(".jellysync.json");
    let manifest_json =
        r#"{"device_id": "empty-id", "name": "Empty Device", "version": "1.0"}"#;
    fs::write(manifest_path, manifest_json).unwrap();

    // Add a hidden folder and a file (should be skipped)
    fs::create_dir(root.join(".hidden")).unwrap();
    fs::write(root.join("readme.txt"), "file").unwrap();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    let manifest: DeviceManifest = serde_json::from_str(manifest_json).unwrap();
    manager
        .handle_device_detected(root.to_path_buf(), manifest)
        .await
        .unwrap();

    let res = manager.list_root_folders().await.unwrap().unwrap();

    assert_eq!(res.folders.len(), 0);
    assert_eq!(res.managed_count, 0);
    assert_eq!(res.unmanaged_count, 0);
    assert!(res.has_manifest);
    assert_eq!(res.device_name, "Empty Device");
}

#[tokio::test]
async fn test_list_root_folders_no_device() {
    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    // No device detected — should return None
    let res = manager.list_root_folders().await.unwrap();
    assert!(res.is_none());
}
