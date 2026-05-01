use super::*;
use serde_json;
use std::fs;
use std::sync::Arc;
use tempfile::tempdir;

fn msc(dir: &std::path::Path) -> Arc<dyn crate::device_io::DeviceIO> {
    Arc::new(crate::device_io::MscBackend::new(dir.to_path_buf()))
}

// ===== Story 4.4 Tests =====

#[test]
fn test_dirty_flag_serde_default() {
    let json = r#"{"device_id": "dev-1", "name": "iPod", "version": "1.0"}"#;
    let manifest: DeviceManifest = serde_json::from_str(json).unwrap();
    assert!(!manifest.dirty, "dirty must default to false");
    assert!(
        manifest.pending_item_ids.is_empty(),
        "pending_item_ids must default to []"
    );
}

#[tokio::test]
async fn test_dirty_manifest_roundtrip() {
    let dir = tempdir().unwrap();
    let manifest = DeviceManifest {
        device_id: "dev-1".to_string(),
        name: None,
        icon: None,
        version: "1.0".to_string(),
        managed_paths: vec![],
        synced_items: vec![],
        dirty: true,
        pending_item_ids: vec!["id-1".to_string(), "id-2".to_string()],
        basket_items: vec![],
        auto_sync_on_connect: false,
        auto_fill: crate::device::AutoFillPrefs::default(),
        transcoding_profile_id: None,
        playlists: vec![],
    };
    write_manifest(msc(dir.path()), &manifest).await.unwrap();
    let content = tokio::fs::read_to_string(dir.path().join(".jellyfinsync.json"))
        .await
        .unwrap();
    let loaded: DeviceManifest = serde_json::from_str(&content).unwrap();
    assert!(loaded.dirty);
    assert_eq!(loaded.pending_item_ids, vec!["id-1", "id-2"]);
}

#[tokio::test]
async fn test_cleanup_tmp_files_no_music_dir() {
    let dir = tempdir().unwrap();
    let count = cleanup_tmp_files(msc(dir.path()), &["Music".to_string()])
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn test_cleanup_tmp_files_empty_music_dir() {
    let dir = tempdir().unwrap();
    tokio::fs::create_dir(dir.path().join("Music"))
        .await
        .unwrap();
    let count = cleanup_tmp_files(msc(dir.path()), &["Music".to_string()])
        .await
        .unwrap();
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

    let count = cleanup_tmp_files(msc(dir.path()), &["Music".to_string()])
        .await
        .unwrap();
    assert_eq!(count, 1);
    assert!(!tmp_file.exists(), ".tmp file must be deleted");
}

#[tokio::test]
async fn test_cleanup_tmp_files_nested_multiple() {
    let dir = tempdir().unwrap();
    let music = dir.path().join("Music");
    tokio::fs::create_dir_all(music.join("Artist1").join("Album1"))
        .await
        .unwrap();
    tokio::fs::create_dir_all(music.join("Artist2").join("Album2"))
        .await
        .unwrap();
    tokio::fs::create_dir_all(music.join("Artist3"))
        .await
        .unwrap();

    tokio::fs::write(
        music.join("Artist1").join("Album1").join("01.flac.tmp"),
        b"a",
    )
    .await
    .unwrap();
    tokio::fs::write(
        music.join("Artist2").join("Album2").join("02.flac.tmp"),
        b"b",
    )
    .await
    .unwrap();
    tokio::fs::write(music.join("Artist3").join("03.flac.tmp"), b"c")
        .await
        .unwrap();

    let count = cleanup_tmp_files(msc(dir.path()), &["Music".to_string()])
        .await
        .unwrap();
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

    let count = cleanup_tmp_files(msc(dir.path()), &["Music".to_string()])
        .await
        .unwrap();
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
    let manifest_path = dir.path().join(".jellyfinsync.json");
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
    let manifest_path = dir.path().join(".jellyfinsync.json");
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
    let manifest_path = dir.path().join(".jellyfinsync.json");
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
    let manifest_path = root.join(".jellyfinsync.json");
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
    let manifest_path = root.join(".jellyfinsync.json");
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
        icon: None,
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
        basket_items: vec![],
        auto_sync_on_connect: false,
        auto_fill: crate::device::AutoFillPrefs::default(),
        transcoding_profile_id: None,
        playlists: vec![],
    };

    write_manifest(msc(root), &manifest).await.unwrap();

    // Verify the manifest file exists (not the temp file)
    let manifest_path = root.join(".jellyfinsync.json");
    let tmp_path = root.join(".jellyfinsync.json.tmp");
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
    };
    write_manifest(msc(root), &manifest1).await.unwrap();

    // Overwrite with updated manifest
    let manifest2 = DeviceManifest {
        device_id: "dev-1".to_string(),
        name: Some("Updated".to_string()),
        icon: None,
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
        basket_items: vec![],
        auto_sync_on_connect: false,
        auto_fill: crate::device::AutoFillPrefs::default(),
        transcoding_profile_id: None,
        playlists: vec![],
    };
    write_manifest(msc(root), &manifest2).await.unwrap();

    let content = fs::read_to_string(root.join(".jellyfinsync.json")).unwrap();
    let loaded: DeviceManifest = serde_json::from_str(&content).unwrap();
    assert_eq!(loaded.name, Some("Updated".to_string()));
    assert_eq!(loaded.synced_items.len(), 1);
    assert_eq!(loaded.synced_items[0].jellyfin_id, "new-item");
}

// ===== Story 5.4 Tests =====

#[tokio::test]
async fn test_get_discrepancies_missing_file() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // Create managed path but no files in it
    fs::create_dir_all(root.join("Music").join("Artist").join("Album")).unwrap();

    // Write manifest with a synced item that DOESN'T exist on disk
    let manifest = DeviceManifest {
        device_id: "dev-1".to_string(),
        name: Some("Test Device".to_string()),
        icon: None,
        version: "1.0".to_string(),
        managed_paths: vec!["Music".to_string()],
        synced_items: vec![SyncedItem {
            jellyfin_id: "item-1".to_string(),
            name: "Track One".to_string(),
            album: Some("Album".to_string()),
            artist: Some("Artist".to_string()),
            local_path: "Music/Artist/Album/01 - Track One.flac".to_string(),
            size_bytes: 1000,
            synced_at: "2026-02-28T10:00:00Z".to_string(),
            original_name: None,
            etag: None,
        }],
        dirty: true,
        pending_item_ids: vec![],
        basket_items: vec![],
        auto_sync_on_connect: false,
        auto_fill: crate::device::AutoFillPrefs::default(),
        transcoding_profile_id: None,
        playlists: vec![],
    };
    write_manifest(msc(root), &manifest).await.unwrap();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);
    manager
        .handle_device_detected(root.to_path_buf(), manifest)
        .await
        .unwrap();

    let discrepancies = manager.get_discrepancies().await.unwrap().unwrap();
    assert_eq!(
        discrepancies.missing.len(),
        1,
        "Should detect one missing file"
    );
    assert_eq!(discrepancies.missing[0].jellyfin_id, "item-1");
    assert_eq!(discrepancies.orphaned.len(), 0);
}

#[tokio::test]
async fn test_get_discrepancies_orphaned_file() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // Create managed path with a file
    let music_path = root.join("Music").join("Rogue").join("Album");
    fs::create_dir_all(&music_path).unwrap();
    fs::write(music_path.join("track.flac"), b"audio data").unwrap();

    // Write manifest WITHOUT that file
    let manifest = DeviceManifest {
        device_id: "dev-1".to_string(),
        name: Some("Test Device".to_string()),
        icon: None,
        version: "1.0".to_string(),
        managed_paths: vec!["Music".to_string()],
        synced_items: vec![],
        dirty: true,
        pending_item_ids: vec![],
        basket_items: vec![],
        auto_sync_on_connect: false,
        auto_fill: crate::device::AutoFillPrefs::default(),
        transcoding_profile_id: None,
        playlists: vec![],
    };
    write_manifest(msc(root), &manifest).await.unwrap();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);
    manager
        .handle_device_detected(root.to_path_buf(), manifest)
        .await
        .unwrap();

    let discrepancies = manager.get_discrepancies().await.unwrap().unwrap();
    assert_eq!(discrepancies.missing.len(), 0);
    assert_eq!(
        discrepancies.orphaned.len(),
        1,
        "Should detect one orphaned file"
    );
    assert_eq!(
        discrepancies.orphaned[0].local_path,
        "Music/Rogue/Album/track.flac"
    );
}

#[tokio::test]
async fn test_get_discrepancies_no_issues() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // Create managed path with files matching the manifest
    let music_path = root.join("Music").join("Artist").join("Album");
    fs::create_dir_all(&music_path).unwrap();
    fs::write(music_path.join("01 - Track.flac"), b"audio data").unwrap();

    let manifest = DeviceManifest {
        device_id: "dev-1".to_string(),
        name: None,
        icon: None,
        version: "1.0".to_string(),
        managed_paths: vec!["Music".to_string()],
        synced_items: vec![SyncedItem {
            jellyfin_id: "item-1".to_string(),
            name: "Track".to_string(),
            album: None,
            artist: None,
            local_path: "Music/Artist/Album/01 - Track.flac".to_string(),
            size_bytes: 1000,
            synced_at: "2026-02-28T10:00:00Z".to_string(),
            original_name: None,
            etag: None,
        }],
        dirty: false,
        pending_item_ids: vec![],
        basket_items: vec![],
        auto_sync_on_connect: false,
        auto_fill: crate::device::AutoFillPrefs::default(),
        transcoding_profile_id: None,
        playlists: vec![],
    };
    write_manifest(msc(root), &manifest).await.unwrap();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);
    manager
        .handle_device_detected(root.to_path_buf(), manifest)
        .await
        .unwrap();

    let discrepancies = manager.get_discrepancies().await.unwrap().unwrap();
    assert_eq!(discrepancies.missing.len(), 0);
    assert_eq!(discrepancies.orphaned.len(), 0);
}

#[tokio::test]
async fn test_prune_items() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    fs::create_dir(root.join("Music")).unwrap();

    let manifest = DeviceManifest {
        device_id: "dev-1".to_string(),
        name: None,
        icon: None,
        version: "1.0".to_string(),
        managed_paths: vec!["Music".to_string()],
        synced_items: vec![
            SyncedItem {
                jellyfin_id: "item-1".to_string(),
                name: "Track 1".to_string(),
                album: None,
                artist: None,
                local_path: "Music/track1.flac".to_string(),
                size_bytes: 100,
                synced_at: "2026-02-28T10:00:00Z".to_string(),
                original_name: None,
                etag: None,
            },
            SyncedItem {
                jellyfin_id: "item-2".to_string(),
                name: "Track 2".to_string(),
                album: None,
                artist: None,
                local_path: "Music/track2.flac".to_string(),
                size_bytes: 200,
                synced_at: "2026-02-28T10:00:00Z".to_string(),
                original_name: None,
                etag: None,
            },
        ],
        dirty: true,
        pending_item_ids: vec![],
        basket_items: vec![],
        auto_sync_on_connect: false,
        auto_fill: crate::device::AutoFillPrefs::default(),
        transcoding_profile_id: None,
        playlists: vec![],
    };
    write_manifest(msc(root), &manifest).await.unwrap();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);
    manager
        .handle_device_detected(root.to_path_buf(), manifest)
        .await
        .unwrap();

    let removed = manager.prune_items(&["item-1".to_string()]).await.unwrap();
    assert_eq!(removed, 1);

    let device = manager.get_current_device().await.unwrap();
    assert_eq!(device.synced_items.len(), 1);
    assert_eq!(device.synced_items[0].jellyfin_id, "item-2");

    // Verify persisted to disk
    let content = fs::read_to_string(root.join(".jellyfinsync.json")).unwrap();
    let loaded: DeviceManifest = serde_json::from_str(&content).unwrap();
    assert_eq!(loaded.synced_items.len(), 1);
}

#[tokio::test]
async fn test_relink_item() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    fs::create_dir(root.join("Music")).unwrap();

    let manifest = DeviceManifest {
        device_id: "dev-1".to_string(),
        name: None,
        icon: None,
        version: "1.0".to_string(),
        managed_paths: vec!["Music".to_string()],
        synced_items: vec![SyncedItem {
            jellyfin_id: "item-1".to_string(),
            name: "Track".to_string(),
            album: None,
            artist: None,
            local_path: "Music/old_path.flac".to_string(),
            size_bytes: 100,
            synced_at: "2026-02-28T10:00:00Z".to_string(),
            original_name: None,
            etag: None,
        }],
        dirty: true,
        pending_item_ids: vec![],
        basket_items: vec![],
        auto_sync_on_connect: false,
        auto_fill: crate::device::AutoFillPrefs::default(),
        transcoding_profile_id: None,
        playlists: vec![],
    };
    write_manifest(msc(root), &manifest).await.unwrap();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);
    manager
        .handle_device_detected(root.to_path_buf(), manifest)
        .await
        .unwrap();

    fs::write(root.join("Music").join("new_path.flac"), b"audio data").unwrap();

    let found = manager
        .relink_item("item-1", "Music/new_path.flac")
        .await
        .unwrap();
    assert!(found);

    let device = manager.get_current_device().await.unwrap();
    assert_eq!(device.synced_items[0].local_path, "Music/new_path.flac");
    assert_eq!(
        device.synced_items[0].original_name,
        Some("Music/old_path.flac".to_string())
    );
}

#[tokio::test]
async fn test_relink_item_path_traversal() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    fs::create_dir(root.join("Music")).unwrap();

    let manifest = DeviceManifest {
        device_id: "dev-1".to_string(),
        name: None,
        icon: None,
        version: "1.0".to_string(),
        managed_paths: vec!["Music".to_string()],
        synced_items: vec![],
        dirty: false,
        pending_item_ids: vec![],
        basket_items: vec![],
        auto_sync_on_connect: false,
        auto_fill: crate::device::AutoFillPrefs::default(),
        transcoding_profile_id: None,
        playlists: vec![],
    };
    write_manifest(msc(root), &manifest).await.unwrap();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);
    manager
        .handle_device_detected(root.to_path_buf(), manifest)
        .await
        .unwrap();

    let res = manager.relink_item("item-1", "../secret.txt").await;
    assert!(res.is_err());
    assert!(res.unwrap_err().to_string().contains("path traversal"));
}

#[tokio::test]
async fn test_clear_dirty_flag() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    let manifest = DeviceManifest {
        device_id: "dev-1".to_string(),
        name: None,
        icon: None,
        version: "1.0".to_string(),
        managed_paths: vec![],
        synced_items: vec![],
        dirty: true,
        pending_item_ids: vec!["pending-1".to_string()],
        basket_items: vec![],
        auto_sync_on_connect: false,
        auto_fill: crate::device::AutoFillPrefs::default(),
        transcoding_profile_id: None,
        playlists: vec![],
    };
    write_manifest(msc(root), &manifest).await.unwrap();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);
    manager
        .handle_device_detected(root.to_path_buf(), manifest)
        .await
        .unwrap();

    manager.clear_dirty_flag().await.unwrap();

    let device = manager.get_current_device().await.unwrap();
    assert!(!device.dirty);
    assert!(device.pending_item_ids.is_empty());

    // Verify persisted to disk
    let content = fs::read_to_string(root.join(".jellyfinsync.json")).unwrap();
    let loaded: DeviceManifest = serde_json::from_str(&content).unwrap();
    assert!(!loaded.dirty);
    assert!(loaded.pending_item_ids.is_empty());
}

// ===== Story 2.6 Tests =====

#[tokio::test]
async fn test_handle_device_unrecognized_stores_path() {
    let dir = tempdir().unwrap();
    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    // Initially no unrecognized path
    assert!(manager.get_unrecognized_device_path().await.is_none());
    assert!(manager.get_current_device().await.is_none());

    let state = manager
        .handle_device_unrecognized(dir.path().to_path_buf(), msc(dir.path()))
        .await;

    // Path should now be set
    let path = manager.get_unrecognized_device_path().await;
    assert!(path.is_some());
    assert_eq!(path.unwrap(), dir.path());

    // State should be DeviceFound with the path string
    match state {
        crate::DaemonState::DeviceFound(s) => {
            assert!(s.contains(&*dir.path().to_string_lossy()));
        }
        _ => panic!("Expected DeviceFound state"),
    }
}

#[tokio::test]
async fn test_handle_device_removed_clears_unrecognized_path() {
    let dir = tempdir().unwrap();
    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    let path = dir.path().to_path_buf();
    manager
        .handle_device_unrecognized(path.clone(), msc(dir.path()))
        .await;
    assert!(manager.get_unrecognized_device_path().await.is_some());

    manager.handle_device_removed(&path).await;

    assert!(manager.get_unrecognized_device_path().await.is_none());
    assert!(manager.get_current_device().await.is_none());
}

#[tokio::test]
async fn test_handle_device_detected_clears_unrecognized_path() {
    let dir = tempdir().unwrap();
    let manifest_path = dir.path().join(".jellyfinsync.json");
    let manifest_json = r#"{"device_id": "dev-1", "name": "My Device", "version": "1.0"}"#;
    fs::write(&manifest_path, manifest_json).unwrap();
    let manifest: DeviceManifest = serde_json::from_str(manifest_json).unwrap();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    // Set unrecognized path first
    manager
        .handle_device_unrecognized(dir.path().to_path_buf(), msc(dir.path()))
        .await;
    assert!(manager.get_unrecognized_device_path().await.is_some());

    // Detect device (recognized)
    manager
        .handle_device_detected(dir.path().to_path_buf(), manifest)
        .await
        .unwrap();

    // Unrecognized path should be cleared
    assert!(manager.get_unrecognized_device_path().await.is_none());
    assert!(manager.get_current_device().await.is_some());
}

#[tokio::test]
async fn test_list_root_folders_unrecognized_device() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // No manifest — folders should still be listed via unrecognized path fallback
    fs::create_dir(root.join("Music")).unwrap();
    fs::create_dir(root.join("Podcasts")).unwrap();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    manager.handle_device_unrecognized(root.to_path_buf(), msc(root)).await;

    let res = manager.list_root_folders().await.unwrap().unwrap();

    assert_eq!(res.folders.len(), 2);
    assert!(!res.has_manifest, "No manifest should be present");
    assert_eq!(res.managed_count, 0, "No managed paths without manifest");
}

#[tokio::test]
async fn test_initialize_device_root() {
    let dir = tempdir().unwrap();
    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    manager
        .handle_device_unrecognized(dir.path().to_path_buf(), msc(dir.path()))
        .await;

    // Initialize with root (empty folder_path)
    let manifest = manager.initialize_device("", None, "My Device".to_string(), None, msc(dir.path())).await.unwrap();

    assert!(manifest.managed_paths.is_empty());
    assert_eq!(manifest.version, "1.0");
    assert!(!manifest.dirty);
    assert!(manifest.synced_items.is_empty());

    // Manifest file should exist on disk
    let manifest_path = dir.path().join(".jellyfinsync.json");
    assert!(manifest_path.exists(), ".jellyfinsync.json must be created");

    // Device should now be current (recognized)
    assert!(manager.get_current_device().await.is_some());
    assert!(manager.get_unrecognized_device_path().await.is_none());
    assert!(manager.get_current_device_path().await.is_some());
}

#[tokio::test]
async fn test_initialize_device_subfolder() {
    let dir = tempdir().unwrap();
    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    manager
        .handle_device_unrecognized(dir.path().to_path_buf(), msc(dir.path()))
        .await;

    // Initialize with a subfolder
    let manifest = manager.initialize_device("Music", None, "My Device".to_string(), None, msc(dir.path())).await.unwrap();

    assert_eq!(manifest.managed_paths, vec!["Music".to_string()]);

    // Music folder should have been created
    let music_folder = dir.path().join("Music");
    assert!(music_folder.exists(), "Music subfolder should be created");

    // Manifest on disk should have managed_paths = ["Music"]
    let content = tokio::fs::read_to_string(dir.path().join(".jellyfinsync.json"))
        .await
        .unwrap();
    let loaded: DeviceManifest = serde_json::from_str(&content).unwrap();
    assert_eq!(loaded.managed_paths, vec!["Music".to_string()]);
}

#[tokio::test]
async fn test_initialize_device_requires_unrecognized_path() {
    let dir = tempdir().unwrap();
    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    // No unrecognized path set → should fail even when a backend is provided
    let res = manager.initialize_device("", None, "My Device".to_string(), None, msc(dir.path())).await;
    assert!(res.is_err());
    assert!(res
        .unwrap_err()
        .to_string()
        .contains("No unrecognized device"));
}

#[tokio::test]
async fn test_initialize_device_rejects_path_traversal() {
    let dir = tempdir().unwrap();
    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    manager
        .handle_device_unrecognized(dir.path().to_path_buf(), msc(dir.path()))
        .await;

    // Path traversal with ".."
    let res = manager.initialize_device("../escape", None, "My Device".to_string(), None, msc(dir.path())).await;
    assert!(res.is_err());
    assert!(res.unwrap_err().to_string().contains("Invalid folder path"));

    // Absolute path
    let res = manager.initialize_device("/etc/hacked", None, "My Device".to_string(), None, msc(dir.path())).await;
    assert!(res.is_err());

    // Nested path with separator
    let res = manager.initialize_device("Music/SubFolder", None, "My Device".to_string(), None, msc(dir.path())).await;
    assert!(res.is_err());
    assert!(res.unwrap_err().to_string().contains("single folder name"));

    // Backslash separator
    let res = manager.initialize_device("Music\\SubFolder", None, "My Device".to_string(), None, msc(dir.path())).await;
    assert!(res.is_err());
}

#[tokio::test]
async fn test_handle_device_unrecognized_preserves_recognized_device() {
    // Story 2.7: When an unrecognized device arrives, recognized devices must NOT be cleared.
    // Other recognized devices may still be connected; only unrecognized_device_path is updated.
    let dir = tempdir().unwrap();
    let manifest_json = r#"{"device_id": "dev-1", "name": "My Device", "version": "1.0"}"#;
    fs::write(dir.path().join(".jellyfinsync.json"), manifest_json).unwrap();
    let manifest: DeviceManifest = serde_json::from_str(manifest_json).unwrap();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    // First detect a recognized device
    manager
        .handle_device_detected(dir.path().to_path_buf(), manifest)
        .await
        .unwrap();
    assert!(manager.get_current_device().await.is_some());
    assert!(manager.get_current_device_path().await.is_some());

    // Now handle an unrecognized device at a DIFFERENT path — recognized device must remain
    let dir2 = tempdir().unwrap();
    manager
        .handle_device_unrecognized(dir2.path().to_path_buf(), msc(dir2.path()))
        .await;

    assert!(
        manager.get_current_device().await.is_some(),
        "Recognized device must remain selected when an unrecognized device arrives"
    );
    assert!(
        manager.get_current_device_path().await.is_some(),
        "selected_device_path must remain set"
    );
    assert!(
        manager.get_unrecognized_device_path().await.is_some(),
        "unrecognized_device_path must be set"
    );
    // The unrecognized path must NOT appear in connected_devices
    let devices = manager.get_connected_devices().await;
    assert!(
        devices.iter().all(|(p, _)| p != dir2.path()),
        "Unrecognized device path must not be in connected_devices"
    );
}

// ===== Story 2.6 MTP Task Tests =====

#[tokio::test]
async fn test_handle_device_unrecognized_stores_device_io() {
    // Verify that handle_device_unrecognized stores a DeviceIO backend alongside the path.
    let dir = tempdir().unwrap();
    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    assert!(manager.get_unrecognized_device_io().await.is_none());

    manager
        .handle_device_unrecognized(dir.path().to_path_buf(), msc(dir.path()))
        .await;

    assert!(
        manager.get_unrecognized_device_io().await.is_some(),
        "IO backend must be stored alongside unrecognized device path"
    );
}

#[tokio::test]
async fn test_initialize_device_uses_stored_io_and_clears_it() {
    // Verify that initialize_device (spec form: device_io as parameter) clears stored IO on success.
    let dir = tempdir().unwrap();
    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    manager
        .handle_device_unrecognized(dir.path().to_path_buf(), msc(dir.path()))
        .await;

    assert!(manager.get_unrecognized_device_io().await.is_some());

    let device_io = manager.get_unrecognized_device_io().await.unwrap();
    manager
        .initialize_device("", None, "My Device".to_string(), None, device_io)
        .await
        .unwrap();

    assert!(
        manager.get_unrecognized_device_io().await.is_none(),
        "IO backend must be cleared after successful initialization"
    );
    assert!(
        manager.get_unrecognized_device_path().await.is_none(),
        "Unrecognized path must be cleared after successful initialization"
    );
    // Manifest must have been written via the stored IO backend
    assert!(
        dir.path().join(".jellyfinsync.json").exists(),
        "Manifest must be written to the device root"
    );
}

#[tokio::test]
async fn test_handle_device_removed_clears_unrecognized_io() {
    // Verify that removing the unrecognized device also clears the stored IO backend.
    let dir = tempdir().unwrap();
    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    let path = dir.path().to_path_buf();
    manager.handle_device_unrecognized(path.clone(), msc(dir.path())).await;

    assert!(manager.get_unrecognized_device_path().await.is_some());
    assert!(manager.get_unrecognized_device_io().await.is_some());

    manager.handle_device_removed(&path).await;

    assert!(
        manager.get_unrecognized_device_path().await.is_none(),
        "Unrecognized path must be cleared on device removal"
    );
    assert!(
        manager.get_unrecognized_device_io().await.is_none(),
        "IO backend must be cleared when unrecognized device is removed"
    );
}

// ===== Basket Selection Tests =====

#[test]
fn test_basket_items_serde_default() {
    let json = r#"{"device_id": "old-device", "name": "Old iPod", "version": "1.0", "managed_paths": ["Music"]}"#;
    let manifest: DeviceManifest = serde_json::from_str(json).unwrap();
    assert!(
        manifest.basket_items.is_empty(),
        "basket_items should default to empty vec"
    );
}

#[tokio::test]
async fn test_save_basket_roundtrip() {
    let dir = tempdir().unwrap();
    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    let manifest = DeviceManifest {
        device_id: "dev-basket".to_string(),
        name: Some("Basket Test".to_string()),
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
    };
    write_manifest(msc(dir.path()), &manifest).await.unwrap();

    manager
        .handle_device_detected(dir.path().to_path_buf(), manifest)
        .await
        .unwrap();

    let items = vec![BasketItem {
        id: "basket-1".to_string(),
        name: "Basket Playlist".to_string(),
        item_type: "Playlist".to_string(),
        artist: None,
        child_count: 5,
        size_ticks: 1000,
        size_bytes: 500000,
    }];

    manager.save_basket(items.clone()).await.unwrap();

    // Verify in-memory state
    let memory_device = manager.get_current_device().await.unwrap();
    assert_eq!(memory_device.basket_items.len(), 1);
    assert_eq!(memory_device.basket_items[0].id, "basket-1");

    // Verify disk state
    let content = fs::read_to_string(dir.path().join(".jellyfinsync.json")).unwrap();
    let loaded: DeviceManifest = serde_json::from_str(&content).unwrap();
    assert_eq!(loaded.basket_items.len(), 1);
    assert_eq!(loaded.basket_items[0].name, "Basket Playlist");
}

// ===== Story 2.3b Tests: Auto-Sync on Connect =====

#[test]
fn test_auto_sync_on_connect_serde_default() {
    let json = r#"{"device_id": "dev-1", "name": "iPod", "version": "1.0"}"#;
    let manifest: DeviceManifest = serde_json::from_str(json).unwrap();
    assert!(
        !manifest.auto_sync_on_connect,
        "auto_sync_on_connect must default to false"
    );
}

#[tokio::test]
async fn test_auto_sync_on_connect_roundtrip() {
    let dir = tempdir().unwrap();
    let manifest = DeviceManifest {
        device_id: "dev-auto".to_string(),
        name: Some("Auto Device".to_string()),
        icon: None,
        version: "1.0".to_string(),
        managed_paths: vec![],
        synced_items: vec![],
        dirty: false,
        pending_item_ids: vec![],
        basket_items: vec![],
        auto_sync_on_connect: true,
        auto_fill: crate::device::AutoFillPrefs::default(),
        transcoding_profile_id: None,
        playlists: vec![],
    };
    write_manifest(msc(dir.path()), &manifest).await.unwrap();

    let content = tokio::fs::read_to_string(dir.path().join(".jellyfinsync.json"))
        .await
        .unwrap();
    let loaded: DeviceManifest = serde_json::from_str(&content).unwrap();
    assert!(loaded.auto_sync_on_connect);

    // Verify snake_case serialization (DeviceManifest doesn't use rename_all)
    let raw: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(raw["auto_sync_on_connect"], true);
}

#[test]
fn test_auto_sync_on_connect_explicit_false() {
    let json = r#"{"device_id": "dev-1", "version": "1.0", "auto_sync_on_connect": false}"#;
    let manifest: DeviceManifest = serde_json::from_str(json).unwrap();
    assert!(!manifest.auto_sync_on_connect);
}

#[test]
fn test_auto_sync_on_connect_explicit_true() {
    let json = r#"{"device_id": "dev-1", "version": "1.0", "auto_sync_on_connect": true}"#;
    let manifest: DeviceManifest = serde_json::from_str(json).unwrap();
    assert!(manifest.auto_sync_on_connect);
}

// ===== Story 2.7 Tests =====

fn make_manifest(device_id: &str, name: &str) -> DeviceManifest {
    DeviceManifest {
        device_id: device_id.to_string(),
        name: Some(name.to_string()),
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
    }
}

#[tokio::test]
async fn test_handle_device_detected_two_sequential_devices() {
    let dir1 = tempdir().unwrap();
    let dir2 = tempdir().unwrap();
    let path1 = dir1.path().to_path_buf();
    let path2 = dir2.path().to_path_buf();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    let manifest1 = make_manifest("device-1", "iPod");
    let manifest2 = make_manifest("device-2", "Walkman");

    // Write manifests so handle_device_detected can store them
    write_manifest(msc(&path1), &manifest1).await.unwrap();
    write_manifest(msc(&path2), &manifest2).await.unwrap();

    manager.handle_device_detected(path1.clone(), manifest1).await.unwrap();
    manager.handle_device_detected(path2.clone(), manifest2).await.unwrap();

    let devices = manager.get_connected_devices().await;
    assert_eq!(devices.len(), 2, "Both devices must be in connected_devices");

    // First device should be auto-selected (second doesn't override)
    let selected = manager.get_current_device_path().await;
    assert_eq!(selected, Some(path1.clone()), "First device must remain selected");

    // get_current_device returns the manifest for the selected path
    let current = manager.get_current_device().await;
    assert!(current.is_some());
    assert_eq!(current.unwrap().device_id, "device-1");
}

#[tokio::test]
async fn test_handle_device_removed_selected_with_remaining_autoselects() {
    let dir1 = tempdir().unwrap();
    let dir2 = tempdir().unwrap();
    let path1 = dir1.path().to_path_buf();
    let path2 = dir2.path().to_path_buf();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    let manifest1 = make_manifest("device-1", "iPod");
    let manifest2 = make_manifest("device-2", "Walkman");

    write_manifest(msc(&path1), &manifest1).await.unwrap();
    write_manifest(msc(&path2), &manifest2).await.unwrap();

    manager.handle_device_detected(path1.clone(), manifest1).await.unwrap();
    manager.handle_device_detected(path2.clone(), manifest2).await.unwrap();

    // Selected is path1 — remove it
    manager.handle_device_removed(&path1).await;

    let devices = manager.get_connected_devices().await;
    assert_eq!(devices.len(), 1, "Only one device should remain");

    // Remaining device (path2) must be auto-selected
    let selected = manager.get_current_device_path().await;
    assert_eq!(selected, Some(path2.clone()), "Remaining device must be auto-selected");
}

#[tokio::test]
async fn test_handle_device_removed_non_selected_selection_unchanged() {
    let dir1 = tempdir().unwrap();
    let dir2 = tempdir().unwrap();
    let path1 = dir1.path().to_path_buf();
    let path2 = dir2.path().to_path_buf();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    let manifest1 = make_manifest("device-1", "iPod");
    let manifest2 = make_manifest("device-2", "Walkman");

    write_manifest(msc(&path1), &manifest1).await.unwrap();
    write_manifest(msc(&path2), &manifest2).await.unwrap();

    manager.handle_device_detected(path1.clone(), manifest1).await.unwrap();
    manager.handle_device_detected(path2.clone(), manifest2).await.unwrap();

    // Selected is path1 — remove path2 (non-selected)
    manager.handle_device_removed(&path2).await;

    let devices = manager.get_connected_devices().await;
    assert_eq!(devices.len(), 1, "Only one device should remain");

    // path1 must still be selected
    let selected = manager.get_current_device_path().await;
    assert_eq!(selected, Some(path1.clone()), "Selection must remain unchanged");
}

#[tokio::test]
async fn test_select_device_valid_path() {
    let dir1 = tempdir().unwrap();
    let dir2 = tempdir().unwrap();
    let path1 = dir1.path().to_path_buf();
    let path2 = dir2.path().to_path_buf();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    let manifest1 = make_manifest("device-1", "iPod");
    let manifest2 = make_manifest("device-2", "Walkman");

    write_manifest(msc(&path1), &manifest1).await.unwrap();
    write_manifest(msc(&path2), &manifest2).await.unwrap();

    manager.handle_device_detected(path1.clone(), manifest1).await.unwrap();
    manager.handle_device_detected(path2.clone(), manifest2).await.unwrap();

    // Switch to path2
    let ok = manager.select_device(path2.clone()).await;
    assert!(ok, "select_device must return true for a connected device");

    let selected = manager.get_current_device_path().await;
    assert_eq!(selected, Some(path2.clone()));

    let current = manager.get_current_device().await;
    assert_eq!(current.unwrap().device_id, "device-2");
}

#[tokio::test]
async fn test_select_device_unknown_path_returns_false() {
    let dir = tempdir().unwrap();
    let path = dir.path().to_path_buf();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    // No devices connected — selecting an unknown path must fail
    let ok = manager.select_device(path).await;
    assert!(!ok, "select_device must return false for an unconnected path");

    assert!(manager.get_current_device_path().await.is_none());
}
