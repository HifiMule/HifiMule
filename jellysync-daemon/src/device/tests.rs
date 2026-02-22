use super::*;
use serde_json;
use std::fs;
use std::sync::Arc;
use tempfile::tempdir;

// ===== Story 4.4 Tests =====

#[test]
fn test_dirty_flag_serde_default() {
    let json = r#"{"device_id": "dev-1", "name": "iPod", "version": "1.0"}"#;
    let manifest: DeviceManifest = serde_json::from_str(json).unwrap();
    assert!(!manifest.dirty, "dirty must default to false");
    assert!(manifest.pending_item_ids.is_empty(), "pending_item_ids must default to []");
}

#[tokio::test]
async fn test_dirty_manifest_roundtrip() {
    let dir = tempdir().unwrap();
    let manifest = DeviceManifest {
        device_id: "dev-1".to_string(),
        name: None,
        version: "1.0".to_string(),
        managed_paths: vec![],
        synced_items: vec![],
        dirty: true,
        pending_item_ids: vec!["id-1".to_string(), "id-2".to_string()],
    };
    write_manifest(dir.path(), &manifest).await.unwrap();
    let content = tokio::fs::read_to_string(dir.path().join(".jellysync.json")).await.unwrap();
    let loaded: DeviceManifest = serde_json::from_str(&content).unwrap();
    assert!(loaded.dirty);
    assert_eq!(loaded.pending_item_ids, vec!["id-1", "id-2"]);
}

#[tokio::test]
async fn test_cleanup_tmp_files_no_music_dir() {
    let dir = tempdir().unwrap();
    let count = cleanup_tmp_files(dir.path()).await.unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn test_cleanup_tmp_files_empty_music_dir() {
    let dir = tempdir().unwrap();
    tokio::fs::create_dir(dir.path().join("Music")).await.unwrap();
    let count = cleanup_tmp_files(dir.path()).await.unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn test_cleanup_tmp_files_finds_and_deletes() {
    let dir = tempdir().unwrap();
    let tmp_path = dir.path().join("Music").join("Artist").join("Album");
    tokio::fs::create_dir_all(&tmp_path).await.unwrap();
    let tmp_file = tmp_path.join("01 - Track.flac.tmp");
    tokio::fs::write(&tmp_file, b"partial").await.unwrap();
    assert!(tmp_file.exists());

    let count = cleanup_tmp_files(dir.path()).await.unwrap();
    assert_eq!(count, 1);
    assert!(!tmp_file.exists(), ".tmp file must be deleted");
}

#[tokio::test]
async fn test_cleanup_tmp_files_nested_multiple() {
    let dir = tempdir().unwrap();
    let music = dir.path().join("Music");
    tokio::fs::create_dir_all(music.join("Artist1").join("Album1")).await.unwrap();
    tokio::fs::create_dir_all(music.join("Artist2").join("Album2")).await.unwrap();
    tokio::fs::create_dir_all(music.join("Artist3")).await.unwrap();

    tokio::fs::write(music.join("Artist1").join("Album1").join("01.flac.tmp"), b"a").await.unwrap();
    tokio::fs::write(music.join("Artist2").join("Album2").join("02.flac.tmp"), b"b").await.unwrap();
    tokio::fs::write(music.join("Artist3").join("03.flac.tmp"), b"c").await.unwrap();

    let count = cleanup_tmp_files(dir.path()).await.unwrap();
    assert_eq!(count, 3);
}

#[tokio::test]
async fn test_cleanup_tmp_files_non_tmp_preserved() {
    let dir = tempdir().unwrap();
    let music_dir = dir.path().join("Music").join("Artist");
    tokio::fs::create_dir_all(&music_dir).await.unwrap();

    let flac_file = music_dir.join("track.flac");
    let mp3_file = music_dir.join("track.mp3");
    let tmp_file = music_dir.join("track.flac.tmp");

    tokio::fs::write(&flac_file, b"real flac").await.unwrap();
    tokio::fs::write(&mp3_file, b"real mp3").await.unwrap();
    tokio::fs::write(&tmp_file, b"partial").await.unwrap();

    let count = cleanup_tmp_files(dir.path()).await.unwrap();
    assert_eq!(count, 1, "Only .tmp file should be deleted");
    assert!(flac_file.exists(), ".flac file must be preserved");
    assert!(mp3_file.exists(), ".mp3 file must be preserved");
    assert!(!tmp_file.exists(), ".tmp file must be deleted");
}

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
    let manifest_json = r#"{"device_id": "empty-id", "name": "Empty Device", "version": "1.0"}"#;
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
    assert_eq!(
        manifest.synced_items[0].artist,
        Some("Artist X".to_string())
    );
    assert_eq!(
        manifest.synced_items[0].local_path,
        "Music/Artist X/Album A/01 - Track One.flac"
    );
    assert_eq!(manifest.synced_items[0].size_bytes, 34521088);
}

#[tokio::test]
async fn test_write_manifest_creates_files() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    let manifest = DeviceManifest {
        device_id: "test-device".to_string(),
        name: Some("Test".to_string()),
        version: "1.1".to_string(),
        managed_paths: vec!["Music".to_string()],
        synced_items: vec![SyncedItem {
            jellyfin_id: "item-1".to_string(),
            name: "Track".to_string(),
            album: Some("Album".to_string()),
            artist: Some("Artist".to_string()),
            local_path: "Music/Artist/Album/Track.flac".to_string(),
            size_bytes: 1000,
            synced_at: "2026-02-15T10:00:00Z".to_string(),
            original_name: None,
            etag: None,
        }],
        dirty: false,
        pending_item_ids: vec![],
    };

    write_manifest(root, &manifest).await.unwrap();

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

#[tokio::test]
async fn test_write_manifest_overwrites_existing() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // Write initial manifest
    let manifest1 = DeviceManifest {
        device_id: "dev-1".to_string(),
        name: Some("First".to_string()),
        version: "1.0".to_string(),
        managed_paths: vec![],
        synced_items: vec![],
        dirty: false,
        pending_item_ids: vec![],
    };
    write_manifest(root, &manifest1).await.unwrap();

    // Overwrite with updated manifest
    let manifest2 = DeviceManifest {
        device_id: "dev-1".to_string(),
        name: Some("Updated".to_string()),
        version: "1.1".to_string(),
        managed_paths: vec!["Music".to_string()],
        synced_items: vec![SyncedItem {
            jellyfin_id: "new-item".to_string(),
            name: "New Track".to_string(),
            album: None,
            artist: None,
            local_path: "Music/track2.flac".to_string(),
            size_bytes: 200,
            synced_at: "2026-02-15T13:00:00Z".to_string(),
            original_name: None,
            etag: None,
        }],
        dirty: false,
        pending_item_ids: vec![],
    };
    write_manifest(root, &manifest2).await.unwrap();

    let content = fs::read_to_string(root.join(".jellysync.json")).unwrap();
    let loaded: DeviceManifest = serde_json::from_str(&content).unwrap();
    assert_eq!(loaded.name, Some("Updated".to_string()));
    assert_eq!(loaded.synced_items.len(), 1);
    assert_eq!(loaded.synced_items[0].jellyfin_id, "new-item");
}
