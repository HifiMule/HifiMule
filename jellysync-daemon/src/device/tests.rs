use super::*;
use std::fs;
use std::sync::Arc;
use tempfile::tempdir;
use serde_json;

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

#[test]
fn test_backward_compat_manifest_without_synced_items() {
    // Old-format manifest without synced_items field
    let json = r#"{"device_id": "old-device", "name": "Old iPod", "version": "1.0", "managed_paths": ["Music"]}"#;
    let manifest: DeviceManifest = serde_json::from_str(json).unwrap();

    assert_eq!(manifest.device_id, "old-device");
    assert_eq!(manifest.name, Some("Old iPod".to_string()));
    assert!(manifest.synced_items.is_empty()); // Default empty vec
}

#[test]
fn test_manifest_with_synced_items_deserialization() {
    let json = r#"{
        "device_id": "dev-1",
        "name": "My iPod",
        "version": "1.1",
        "managed_paths": ["Music"],
        "synced_items": [
            {
                "jellyfinId": "item-1",
                "name": "Track One",
                "album": "Album A",
                "artist": "Artist X",
                "localPath": "Music/Artist X/Album A/01 - Track One.flac",
                "sizeBytes": 34521088,
                "syncedAt": "2026-02-15T10:30:00Z"
            }
        ]
    }"#;
    let manifest: DeviceManifest = serde_json::from_str(json).unwrap();

    assert_eq!(manifest.synced_items.len(), 1);
    assert_eq!(manifest.synced_items[0].jellyfin_id, "item-1");
    assert_eq!(manifest.synced_items[0].name, "Track One");
    assert_eq!(manifest.synced_items[0].album, Some("Album A".to_string()));
    assert_eq!(manifest.synced_items[0].artist, Some("Artist X".to_string()));
    assert_eq!(manifest.synced_items[0].local_path, "Music/Artist X/Album A/01 - Track One.flac");
    assert_eq!(manifest.synced_items[0].size_bytes, 34521088);
}

#[test]
fn test_write_manifest_atomic() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    let manifest = DeviceManifest {
        device_id: "test-device".to_string(),
        name: Some("Test".to_string()),
        version: "1.1".to_string(),
        managed_paths: vec!["Music".to_string()],
        synced_items: vec![
            SyncedItem {
                jellyfin_id: "item-1".to_string(),
                name: "Track".to_string(),
                album: Some("Album".to_string()),
                artist: Some("Artist".to_string()),
                local_path: "Music/Artist/Album/Track.flac".to_string(),
                size_bytes: 1000,
                synced_at: "2026-02-15T10:00:00Z".to_string(),
            },
        ],
    };

    write_manifest(root, &manifest).unwrap();

    // Verify the manifest file exists (not the temp file)
    let manifest_path = root.join(".jellysync.json");
    let tmp_path = root.join(".jellysync.json.tmp");
    assert!(manifest_path.exists());
    assert!(!tmp_path.exists());

    // Verify content can be read back
    let content = fs::read_to_string(&manifest_path).unwrap();
    let loaded: DeviceManifest = serde_json::from_str(&content).unwrap();
    assert_eq!(loaded.device_id, "test-device");
    assert_eq!(loaded.synced_items.len(), 1);
    assert_eq!(loaded.synced_items[0].jellyfin_id, "item-1");
}

#[test]
fn test_write_manifest_overwrites_existing() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // Write initial manifest
    let manifest1 = DeviceManifest {
        device_id: "dev-1".to_string(),
        name: Some("First".to_string()),
        version: "1.0".to_string(),
        managed_paths: vec![],
        synced_items: vec![],
    };
    write_manifest(root, &manifest1).unwrap();

    // Overwrite with updated manifest
    let manifest2 = DeviceManifest {
        device_id: "dev-1".to_string(),
        name: Some("Updated".to_string()),
        version: "1.1".to_string(),
        managed_paths: vec!["Music".to_string()],
        synced_items: vec![
            SyncedItem {
                jellyfin_id: "new-item".to_string(),
                name: "New Track".to_string(),
                album: None,
                artist: None,
                local_path: "Music/track.flac".to_string(),
                size_bytes: 500,
                synced_at: "2026-02-15T12:00:00Z".to_string(),
            },
        ],
    };
    write_manifest(root, &manifest2).unwrap();

    let content = fs::read_to_string(root.join(".jellysync.json")).unwrap();
    let loaded: DeviceManifest = serde_json::from_str(&content).unwrap();
    assert_eq!(loaded.name, Some("Updated".to_string()));
    assert_eq!(loaded.synced_items.len(), 1);
    assert_eq!(loaded.synced_items[0].jellyfin_id, "new-item");
}
