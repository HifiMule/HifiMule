use super::*;
use crate::device_io::DeviceIO;
use serde_json;

#[cfg(target_os = "windows")]
fn dummy_mtp_device_info(id: &str) -> mtp::MtpDeviceInfo {
    mtp::MtpDeviceInfo {
        device_id: id.to_string(),
        friendly_name: "Test Device".to_string(),
        inner: mtp::MtpDeviceInner::Wpd {
            wpd_device_id: id.to_string(),
        },
    }
}

#[cfg(unix)]
fn dummy_mtp_device_info(id: &str) -> mtp::MtpDeviceInfo {
    mtp::MtpDeviceInfo {
        device_id: id.to_string(),
        friendly_name: "Test Device".to_string(),
        inner: mtp::MtpDeviceInner::Libmtp {
            bus_location: 0,
            dev_num: 0,
        },
    }
}
use std::fs;
use std::sync::Arc;
use tempfile::tempdir;
use tokio::time::{timeout, Duration};

fn msc(dir: &std::path::Path) -> Arc<dyn crate::device_io::DeviceIO> {
    Arc::new(crate::device_io::MscBackend::new(dir.to_path_buf()))
}

#[derive(Debug)]
struct StorageIdDeviceIo {
    inner: crate::device_io::MscBackend,
    storage_id: Option<String>,
}

#[derive(Debug)]
struct FailingReadDeviceIo {
    inner: crate::device_io::MscBackend,
}

impl FailingReadDeviceIo {
    fn new(root: &std::path::Path) -> Arc<dyn crate::device_io::DeviceIO> {
        Arc::new(Self {
            inner: crate::device_io::MscBackend::new(root.to_path_buf()),
        })
    }
}

#[async_trait::async_trait]
impl crate::device_io::DeviceIO for FailingReadDeviceIo {
    async fn read_file(&self, _path: &str) -> Result<Vec<u8>> {
        Err(anyhow::anyhow!("MTP transport temporarily unavailable"))
    }

    async fn write_file(&self, path: &str, data: &[u8]) -> Result<()> {
        self.inner.write_file(path, data).await
    }

    async fn write_with_verify(&self, path: &str, data: &[u8]) -> Result<()> {
        self.inner.write_with_verify(path, data).await
    }

    async fn delete_file(&self, path: &str) -> Result<()> {
        self.inner.delete_file(path).await
    }

    async fn list_files(&self, path: &str) -> Result<Vec<crate::device_io::FileEntry>> {
        self.inner.list_files(path).await
    }

    async fn free_space(&self) -> Result<u64> {
        self.inner.free_space().await
    }

    async fn ensure_dir(&self, path: &str) -> Result<()> {
        self.inner.ensure_dir(path).await
    }

    async fn cleanup_empty_subdirs(&self, path: &str) -> Result<()> {
        self.inner.cleanup_empty_subdirs(path).await
    }
}

impl StorageIdDeviceIo {
    fn new(
        root: &std::path::Path,
        storage_id: Option<String>,
    ) -> Arc<dyn crate::device_io::DeviceIO> {
        Arc::new(Self {
            inner: crate::device_io::MscBackend::new(root.to_path_buf()),
            storage_id,
        })
    }
}

#[async_trait::async_trait]
impl crate::device_io::DeviceIO for StorageIdDeviceIo {
    async fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        self.inner.read_file(path).await
    }

    async fn write_file(&self, path: &str, data: &[u8]) -> Result<()> {
        self.inner.write_file(path, data).await
    }

    async fn write_with_verify(&self, path: &str, data: &[u8]) -> Result<()> {
        self.inner.write_with_verify(path, data).await
    }

    async fn delete_file(&self, path: &str) -> Result<()> {
        self.inner.delete_file(path).await
    }

    async fn list_files(&self, path: &str) -> Result<Vec<crate::device_io::FileEntry>> {
        self.inner.list_files(path).await
    }

    async fn free_space(&self) -> Result<u64> {
        self.inner.free_space().await
    }

    async fn storage_id(&self) -> Result<Option<String>> {
        Ok(self.storage_id.clone())
    }

    async fn ensure_dir(&self, path: &str) -> Result<()> {
        self.inner.ensure_dir(path).await
    }

    async fn cleanup_empty_subdirs(&self, path: &str) -> Result<()> {
        self.inner.cleanup_empty_subdirs(path).await
    }
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
        storage_id: None,
    };
    write_manifest(msc(dir.path()), &manifest).await.unwrap();
    let content = tokio::fs::read_to_string(dir.path().join(".hifimule.json"))
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
    let manifest_path = dir.path().join(".hifimule.json");
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
    let manifest_path = dir.path().join(".hifimule.json");
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
    let manifest_path = dir.path().join(".hifimule.json");
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
    let manifest_path = root.join(".hifimule.json");
    let manifest_json = r#"{"device_id": "test-id", "name": "Test Device", "version": "1.0", "managed_paths": ["Music"]}"#;
    fs::write(manifest_path, manifest_json).unwrap();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    // Simulate detection
    let manifest: DeviceManifest = serde_json::from_str(manifest_json).unwrap();
    manager
        .handle_device_detected(root.to_path_buf(), manifest, msc(root))
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
    let manifest_path = root.join(".hifimule.json");
    let manifest_json = r#"{"device_id": "empty-id", "name": "Empty Device", "version": "1.0"}"#;
    fs::write(manifest_path, manifest_json).unwrap();

    // Add a hidden folder and a file (should be skipped)
    fs::create_dir(root.join(".hidden")).unwrap();
    fs::write(root.join("readme.txt"), "file").unwrap();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    let manifest: DeviceManifest = serde_json::from_str(manifest_json).unwrap();
    manager
        .handle_device_detected(root.to_path_buf(), manifest, msc(root))
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
                "providerItemId": "item-1",
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
            provider_album_id: None,
            provider_content_type: None,
            provider_suffix: None,
        }],
        dirty: false,
        pending_item_ids: vec![],
        basket_items: vec![],
        auto_sync_on_connect: false,
        auto_fill: crate::device::AutoFillPrefs::default(),
        transcoding_profile_id: None,
        playlists: vec![],
        storage_id: None,
    };

    write_manifest(msc(root), &manifest).await.unwrap();

    // Verify the manifest file exists (not the temp file)
    let manifest_path = root.join(".hifimule.json");
    let tmp_path = root.join(".hifimule.json.tmp");
    assert!(manifest_path.exists());
    assert!(!tmp_path.exists());

    // Verify content can be read back
    let content = fs::read_to_string(&manifest_path).unwrap();
    assert!(
        content.contains("\"providerItemId\""),
        "new manifests must use provider-neutral item IDs"
    );
    assert!(
        !content.contains("\"jellyfinId\""),
        "new manifests must not write the Jellyfin-specific synced item key"
    );
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
        storage_id: None,
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
            provider_album_id: None,
            provider_content_type: None,
            provider_suffix: None,
        }],
        dirty: false,
        pending_item_ids: vec![],
        basket_items: vec![],
        auto_sync_on_connect: false,
        auto_fill: crate::device::AutoFillPrefs::default(),
        transcoding_profile_id: None,
        playlists: vec![],
        storage_id: None,
    };
    write_manifest(msc(root), &manifest2).await.unwrap();

    let content = fs::read_to_string(root.join(".hifimule.json")).unwrap();
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
            provider_album_id: None,
            provider_content_type: None,
            provider_suffix: None,
        }],
        dirty: true,
        pending_item_ids: vec![],
        basket_items: vec![],
        auto_sync_on_connect: false,
        auto_fill: crate::device::AutoFillPrefs::default(),
        transcoding_profile_id: None,
        playlists: vec![],
        storage_id: None,
    };
    write_manifest(msc(root), &manifest).await.unwrap();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);
    manager
        .handle_device_detected(root.to_path_buf(), manifest, msc(root))
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
        storage_id: None,
    };
    write_manifest(msc(root), &manifest).await.unwrap();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);
    manager
        .handle_device_detected(root.to_path_buf(), manifest, msc(root))
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
            provider_album_id: None,
            provider_content_type: None,
            provider_suffix: None,
        }],
        dirty: false,
        pending_item_ids: vec![],
        basket_items: vec![],
        auto_sync_on_connect: false,
        auto_fill: crate::device::AutoFillPrefs::default(),
        transcoding_profile_id: None,
        playlists: vec![],
        storage_id: None,
    };
    write_manifest(msc(root), &manifest).await.unwrap();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);
    manager
        .handle_device_detected(root.to_path_buf(), manifest, msc(root))
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
                provider_album_id: None,
                provider_content_type: None,
                provider_suffix: None,
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
                provider_album_id: None,
                provider_content_type: None,
                provider_suffix: None,
            },
        ],
        dirty: true,
        pending_item_ids: vec![],
        basket_items: vec![],
        auto_sync_on_connect: false,
        auto_fill: crate::device::AutoFillPrefs::default(),
        transcoding_profile_id: None,
        playlists: vec![],
        storage_id: None,
    };
    write_manifest(msc(root), &manifest).await.unwrap();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);
    manager
        .handle_device_detected(root.to_path_buf(), manifest, msc(root))
        .await
        .unwrap();

    let removed = manager.prune_items(&["item-1".to_string()]).await.unwrap();
    assert_eq!(removed, 1);

    let device = manager.get_current_device().await.unwrap();
    assert_eq!(device.synced_items.len(), 1);
    assert_eq!(device.synced_items[0].jellyfin_id, "item-2");

    // Verify persisted to disk
    let content = fs::read_to_string(root.join(".hifimule.json")).unwrap();
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
            provider_album_id: None,
            provider_content_type: None,
            provider_suffix: None,
        }],
        dirty: true,
        pending_item_ids: vec![],
        basket_items: vec![],
        auto_sync_on_connect: false,
        auto_fill: crate::device::AutoFillPrefs::default(),
        transcoding_profile_id: None,
        playlists: vec![],
        storage_id: None,
    };
    write_manifest(msc(root), &manifest).await.unwrap();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);
    manager
        .handle_device_detected(root.to_path_buf(), manifest, msc(root))
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
        storage_id: None,
    };
    write_manifest(msc(root), &manifest).await.unwrap();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);
    manager
        .handle_device_detected(root.to_path_buf(), manifest, msc(root))
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
        storage_id: None,
    };
    write_manifest(msc(root), &manifest).await.unwrap();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);
    manager
        .handle_device_detected(root.to_path_buf(), manifest, msc(root))
        .await
        .unwrap();

    manager.clear_dirty_flag().await.unwrap();

    let device = manager.get_current_device().await.unwrap();
    assert!(!device.dirty);
    assert!(device.pending_item_ids.is_empty());

    // Verify persisted to disk
    let content = fs::read_to_string(root.join(".hifimule.json")).unwrap();
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
        .handle_device_unrecognized(dir.path().to_path_buf(), msc(dir.path()), None)
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
        .handle_device_unrecognized(path.clone(), msc(dir.path()), None)
        .await;
    assert!(manager.get_unrecognized_device_path().await.is_some());

    manager.handle_device_removed(&path).await;

    assert!(manager.get_unrecognized_device_path().await.is_none());
    assert!(manager.get_current_device().await.is_none());
}

#[tokio::test]
async fn test_handle_device_detected_clears_unrecognized_path() {
    let dir = tempdir().unwrap();
    let manifest_path = dir.path().join(".hifimule.json");
    let manifest_json = r#"{"device_id": "dev-1", "name": "My Device", "version": "1.0"}"#;
    fs::write(&manifest_path, manifest_json).unwrap();
    let manifest: DeviceManifest = serde_json::from_str(manifest_json).unwrap();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    // Set unrecognized path first
    manager
        .handle_device_unrecognized(dir.path().to_path_buf(), msc(dir.path()), None)
        .await;
    assert!(manager.get_unrecognized_device_path().await.is_some());

    // Detect device (recognized)
    manager
        .handle_device_detected(dir.path().to_path_buf(), manifest, msc(dir.path()))
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

    manager
        .handle_device_unrecognized(root.to_path_buf(), msc(root), None)
        .await;

    let res = manager.list_root_folders().await.unwrap().unwrap();

    assert_eq!(res.folders.len(), 2);
    assert!(!res.has_manifest, "No manifest should be present");
    assert_eq!(res.managed_count, 0, "No managed paths without manifest");
}

#[tokio::test]
async fn test_list_root_folders_unrecognized_mtp_device() {
    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    let mtp_path = PathBuf::from("mtp://fake-device-id");
    manager
        .handle_device_unrecognized(
            mtp_path,
            msc(std::path::Path::new(".")),
            Some("Garmin Watch".to_string()),
        )
        .await;

    let res = manager.list_root_folders().await.unwrap().unwrap();

    assert!(!res.has_manifest, "Unrecognized MTP device has no manifest");
    assert!(
        res.folders.is_empty(),
        "Unrecognized MTP device has no known folders"
    );
    assert_eq!(res.managed_count, 0);
    assert_eq!(res.unmanaged_count, 0);
    assert_eq!(
        res.device_name, "Garmin Watch",
        "friendly_name should be used as device_name"
    );
}

#[tokio::test]
async fn test_list_root_folders_unrecognized_mtp_device_no_friendly_name() {
    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    let mtp_path = PathBuf::from("mtp://fake-device-id");
    manager
        .handle_device_unrecognized(mtp_path, msc(std::path::Path::new(".")), None)
        .await;

    let res = manager.list_root_folders().await.unwrap().unwrap();

    assert!(!res.has_manifest);
    assert_eq!(
        res.device_name, "MTP Device",
        "Should fall back to 'MTP Device' when no friendly_name"
    );
}

#[tokio::test]
async fn test_initialize_device_root() {
    let dir = tempdir().unwrap();
    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    manager
        .handle_device_unrecognized(dir.path().to_path_buf(), msc(dir.path()), None)
        .await;

    // Initialize with root (empty folder_path)
    let manifest = manager
        .initialize_device("", None, "My Device".to_string(), None, msc(dir.path()))
        .await
        .unwrap();

    assert!(manifest.managed_paths.is_empty());
    assert_eq!(manifest.version, "1.0");
    assert!(!manifest.dirty);
    assert!(manifest.synced_items.is_empty());

    // Manifest file should exist on disk
    let manifest_path = dir.path().join(".hifimule.json");
    assert!(manifest_path.exists(), ".hifimule.json must be created");

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
        .handle_device_unrecognized(dir.path().to_path_buf(), msc(dir.path()), None)
        .await;

    // Initialize with a subfolder
    let manifest = manager
        .initialize_device(
            "Music",
            None,
            "My Device".to_string(),
            None,
            msc(dir.path()),
        )
        .await
        .unwrap();

    assert_eq!(manifest.managed_paths, vec!["Music".to_string()]);

    // Music folder should have been created
    let music_folder = dir.path().join("Music");
    assert!(music_folder.exists(), "Music subfolder should be created");

    // Manifest on disk should have managed_paths = ["Music"]
    let content = tokio::fs::read_to_string(dir.path().join(".hifimule.json"))
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
    let res = manager
        .initialize_device("", None, "My Device".to_string(), None, msc(dir.path()))
        .await;
    assert!(res.is_err());
    assert!(res
        .unwrap_err()
        .to_string()
        .contains("No unrecognized device"));
}

#[tokio::test]
async fn test_initialize_device_persists_storage_id_from_device_io() {
    let dir = tempdir().unwrap();
    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);
    let device_io = StorageIdDeviceIo::new(dir.path(), Some("storage-123".to_string()));

    manager
        .handle_device_unrecognized(dir.path().to_path_buf(), Arc::clone(&device_io), None)
        .await;

    let manifest = manager
        .initialize_device(
            "Music",
            None,
            "My Device".to_string(),
            None,
            Arc::clone(&device_io),
        )
        .await
        .unwrap();

    assert_eq!(manifest.storage_id.as_deref(), Some("storage-123"));
    let content = tokio::fs::read_to_string(dir.path().join(".hifimule.json"))
        .await
        .unwrap();
    let loaded: DeviceManifest = serde_json::from_str(&content).unwrap();
    assert_eq!(loaded.storage_id.as_deref(), Some("storage-123"));
}

#[tokio::test]
async fn test_initialize_device_rejects_path_traversal() {
    let dir = tempdir().unwrap();
    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    manager
        .handle_device_unrecognized(dir.path().to_path_buf(), msc(dir.path()), None)
        .await;

    // Path traversal with ".."
    let res = manager
        .initialize_device(
            "../escape",
            None,
            "My Device".to_string(),
            None,
            msc(dir.path()),
        )
        .await;
    assert!(res.is_err());
    assert!(res.unwrap_err().to_string().contains("Invalid folder path"));

    // Absolute path
    let res = manager
        .initialize_device(
            "/etc/hacked",
            None,
            "My Device".to_string(),
            None,
            msc(dir.path()),
        )
        .await;
    assert!(res.is_err());

    // Multi-level paths with slash separators are now allowed (T9)
    let res = manager
        .initialize_device(
            "Music/SubFolder",
            None,
            "My Device".to_string(),
            None,
            msc(dir.path()),
        )
        .await;
    assert!(res.is_ok(), "multi-level paths must be accepted: {:?}", res);
}

#[tokio::test]
async fn test_handle_device_unrecognized_preserves_recognized_device() {
    // Story 2.7: When an unrecognized device arrives, recognized devices must NOT be cleared.
    // Other recognized devices may still be connected; only unrecognized_device_path is updated.
    let dir = tempdir().unwrap();
    let manifest_json = r#"{"device_id": "dev-1", "name": "My Device", "version": "1.0"}"#;
    fs::write(dir.path().join(".hifimule.json"), manifest_json).unwrap();
    let manifest: DeviceManifest = serde_json::from_str(manifest_json).unwrap();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    // First detect a recognized device
    manager
        .handle_device_detected(dir.path().to_path_buf(), manifest, msc(dir.path()))
        .await
        .unwrap();
    assert!(manager.get_current_device().await.is_some());
    assert!(manager.get_current_device_path().await.is_some());

    // Now handle an unrecognized device at a DIFFERENT path — recognized device must remain
    let dir2 = tempdir().unwrap();
    manager
        .handle_device_unrecognized(dir2.path().to_path_buf(), msc(dir2.path()), None)
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
        devices.iter().all(|(p, _, _)| p != dir2.path()),
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
        .handle_device_unrecognized(dir.path().to_path_buf(), msc(dir.path()), None)
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
        .handle_device_unrecognized(dir.path().to_path_buf(), msc(dir.path()), None)
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
        dir.path().join(".hifimule.json").exists(),
        "Manifest must be written to the device root"
    );
}

#[tokio::test]
async fn test_initialize_device_uses_coherent_pending_snapshot_not_stale_caller_io() {
    let pending_dir = tempdir().unwrap();
    let stale_dir = tempdir().unwrap();
    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    manager
        .handle_device_unrecognized(
            pending_dir.path().to_path_buf(),
            msc(pending_dir.path()),
            None,
        )
        .await;

    manager
        .initialize_device(
            "",
            None,
            "My Device".to_string(),
            None,
            msc(stale_dir.path()),
        )
        .await
        .unwrap();

    assert!(
        pending_dir.path().join(".hifimule.json").exists(),
        "manifest must be written through the pending snapshot IO"
    );
    assert!(
        !stale_dir.path().join(".hifimule.json").exists(),
        "stale caller IO must not be paired with the pending path"
    );
}

#[tokio::test]
async fn test_handle_device_removed_clears_unrecognized_io() {
    // Verify that removing the unrecognized device also clears the stored IO backend.
    let dir = tempdir().unwrap();
    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    let path = dir.path().to_path_buf();
    manager
        .handle_device_unrecognized(path.clone(), msc(dir.path()), None)
        .await;

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
        storage_id: None,
    };
    write_manifest(msc(dir.path()), &manifest).await.unwrap();

    manager
        .handle_device_detected(dir.path().to_path_buf(), manifest, msc(dir.path()))
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
    let content = fs::read_to_string(dir.path().join(".hifimule.json")).unwrap();
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
        storage_id: None,
    };
    write_manifest(msc(dir.path()), &manifest).await.unwrap();

    let content = tokio::fs::read_to_string(dir.path().join(".hifimule.json"))
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
        storage_id: None,
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

    manager
        .handle_device_detected(path1.clone(), manifest1, msc(&path1))
        .await
        .unwrap();
    manager
        .handle_device_detected(path2.clone(), manifest2, msc(&path2))
        .await
        .unwrap();

    let devices = manager.get_connected_devices().await;
    assert_eq!(
        devices.len(),
        2,
        "Both devices must be in connected_devices"
    );

    // First device should be auto-selected (second doesn't override)
    let selected = manager.get_current_device_path().await;
    assert_eq!(
        selected,
        Some(path1.clone()),
        "First device must remain selected"
    );

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

    manager
        .handle_device_detected(path1.clone(), manifest1, msc(&path1))
        .await
        .unwrap();
    manager
        .handle_device_detected(path2.clone(), manifest2, msc(&path2))
        .await
        .unwrap();

    // Selected is path1 — remove it
    manager.handle_device_removed(&path1).await;

    let devices = manager.get_connected_devices().await;
    assert_eq!(devices.len(), 1, "Only one device should remain");

    // Remaining device (path2) must be auto-selected
    let selected = manager.get_current_device_path().await;
    assert_eq!(
        selected,
        Some(path2.clone()),
        "Remaining device must be auto-selected"
    );
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

    manager
        .handle_device_detected(path1.clone(), manifest1, msc(&path1))
        .await
        .unwrap();
    manager
        .handle_device_detected(path2.clone(), manifest2, msc(&path2))
        .await
        .unwrap();

    // Selected is path1 — remove path2 (non-selected)
    manager.handle_device_removed(&path2).await;

    let devices = manager.get_connected_devices().await;
    assert_eq!(devices.len(), 1, "Only one device should remain");

    // path1 must still be selected
    let selected = manager.get_current_device_path().await;
    assert_eq!(
        selected,
        Some(path1.clone()),
        "Selection must remain unchanged"
    );
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

    manager
        .handle_device_detected(path1.clone(), manifest1, msc(&path1))
        .await
        .unwrap();
    manager
        .handle_device_detected(path2.clone(), manifest2, msc(&path2))
        .await
        .unwrap();

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
    assert!(
        !ok,
        "select_device must return false for an unconnected path"
    );

    assert!(manager.get_current_device_path().await.is_none());
}

#[tokio::test]
async fn test_handle_device_removed_selected_with_multiple_remaining_autoselects() {
    let dir1 = tempdir().unwrap();
    let dir2 = tempdir().unwrap();
    let dir3 = tempdir().unwrap();
    let path1 = dir1.path().to_path_buf();
    let path2 = dir2.path().to_path_buf();
    let path3 = dir3.path().to_path_buf();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    let manifest1 = make_manifest("device-1", "iPod");
    let manifest2 = make_manifest("device-2", "Walkman");
    let manifest3 = make_manifest("device-3", "Zune");

    write_manifest(msc(&path1), &manifest1).await.unwrap();
    write_manifest(msc(&path2), &manifest2).await.unwrap();
    write_manifest(msc(&path3), &manifest3).await.unwrap();

    manager
        .handle_device_detected(path1.clone(), manifest1, msc(&path1))
        .await
        .unwrap();
    manager
        .handle_device_detected(path2.clone(), manifest2, msc(&path2))
        .await
        .unwrap();
    manager
        .handle_device_detected(path3.clone(), manifest3, msc(&path3))
        .await
        .unwrap();

    manager.handle_device_removed(&path1).await;

    let selected = manager.get_current_device_path().await;
    assert!(
        selected.as_ref() == Some(&path2) || selected.as_ref() == Some(&path3),
        "one remaining device must be selected after selected removal"
    );
}

#[tokio::test]
async fn test_select_device_and_update_manifest_do_not_deadlock() {
    let dir1 = tempdir().unwrap();
    let dir2 = tempdir().unwrap();
    let path1 = dir1.path().to_path_buf();
    let path2 = dir2.path().to_path_buf();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = Arc::new(DeviceManager::new(db));

    let manifest1 = make_manifest("device-1", "iPod");
    let manifest2 = make_manifest("device-2", "Walkman");
    write_manifest(msc(&path1), &manifest1).await.unwrap();
    write_manifest(msc(&path2), &manifest2).await.unwrap();

    manager
        .handle_device_detected(path1.clone(), manifest1, msc(&path1))
        .await
        .unwrap();
    manager
        .handle_device_detected(path2.clone(), manifest2, msc(&path2))
        .await
        .unwrap();

    let selecting = {
        let manager = Arc::clone(&manager);
        let path2 = path2.clone();
        tokio::spawn(async move { manager.select_device(path2).await })
    };
    let updating = {
        let manager = Arc::clone(&manager);
        tokio::spawn(async move {
            manager
                .update_manifest(|manifest| manifest.name = Some("Updated".to_string()))
                .await
        })
    };

    timeout(Duration::from_secs(2), async {
        let _ = tokio::join!(selecting, updating);
    })
    .await
    .expect("select_device and update_manifest must not deadlock");
}

#[tokio::test]
async fn test_unrecognized_state_concurrent_set_and_remove_is_coherent() {
    let dir = tempdir().unwrap();
    let path = dir.path().to_path_buf();
    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = Arc::new(DeviceManager::new(db));

    let setter = {
        let manager = Arc::clone(&manager);
        let path = path.clone();
        tokio::spawn(async move {
            for _ in 0..100 {
                manager
                    .handle_device_unrecognized(path.clone(), msc(&path), Some("Pending".into()))
                    .await;
            }
        })
    };
    let remover = {
        let manager = Arc::clone(&manager);
        let path = path.clone();
        tokio::spawn(async move {
            for _ in 0..100 {
                manager.handle_device_removed(&path).await;
            }
        })
    };

    timeout(Duration::from_secs(2), async {
        let _ = tokio::join!(setter, remover);
    })
    .await
    .expect("concurrent unrecognized set/remove must complete");

    if let Some(snapshot) = manager.get_unrecognized_device_snapshot().await {
        assert_eq!(snapshot.path, path);
        assert!(snapshot.io.free_space().await.is_ok());
        assert_eq!(snapshot.friendly_name.as_deref(), Some("Pending"));
    }
}

#[tokio::test]
async fn test_handle_device_unrecognized_removes_connected_entry_for_same_path() {
    let dir = tempdir().unwrap();
    let path = dir.path().to_path_buf();
    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);
    let manifest = make_manifest("device-1", "iPod");

    write_manifest(msc(&path), &manifest).await.unwrap();
    manager
        .handle_device_detected(path.clone(), manifest, msc(&path))
        .await
        .unwrap();
    assert_eq!(manager.get_connected_devices().await.len(), 1);

    manager
        .handle_device_unrecognized(path.clone(), msc(&path), None)
        .await;

    assert!(
        manager.get_connected_devices().await.is_empty(),
        "same-path unrecognized probe must remove the connected entry"
    );
    assert_eq!(manager.get_unrecognized_device_path().await, Some(path));
}

#[tokio::test]
async fn test_mtp_probe_missing_manifest_emits_unrecognized_and_marks_known() {
    let dir = tempdir().unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    let backend = msc(dir.path());

    let inserted = emit_mtp_probe_event(
        &tx,
        PathBuf::from("mtp://missing-manifest"),
        "missing-manifest",
        dummy_mtp_device_info("missing-manifest"),
        "Fresh Device".to_string(),
        backend,
    )
    .await;

    assert!(
        inserted,
        "missing manifest emits Unrecognized and can be marked known"
    );
    match rx.recv().await.unwrap() {
        DeviceEvent::Unrecognized {
            path,
            friendly_name,
            ..
        } => {
            assert_eq!(path, PathBuf::from("mtp://missing-manifest"));
            assert_eq!(friendly_name.as_deref(), Some("Fresh Device"));
        }
        _ => panic!("expected unrecognized event"),
    }
}

#[tokio::test]
async fn test_mtp_probe_read_failure_does_not_mark_known() {
    let dir = tempdir().unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    let backend = FailingReadDeviceIo::new(dir.path());

    let inserted = emit_mtp_probe_event(
        &tx,
        PathBuf::from("mtp://retry-me"),
        "retry-me",
        dummy_mtp_device_info("retry-me"),
        "Retry Device".to_string(),
        backend,
    )
    .await;

    assert!(
        !inserted,
        "manifest read failure must leave device retryable"
    );
    assert!(
        rx.try_recv().is_err(),
        "no event should be emitted on read failure"
    );
}

#[tokio::test]
async fn test_mtp_probe_parse_failure_emits_unrecognized_and_marks_known() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".hifimule.json"), b"not-json").unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    let backend = msc(dir.path());

    let inserted = emit_mtp_probe_event(
        &tx,
        PathBuf::from("mtp://unrecognized"),
        "unrecognized",
        dummy_mtp_device_info("unrecognized"),
        "Unrecognized Device".to_string(),
        backend,
    )
    .await;

    assert!(
        inserted,
        "parse failure emits Unrecognized and can be marked known"
    );
    match rx.recv().await.unwrap() {
        DeviceEvent::Unrecognized { friendly_name, .. } => {
            assert_eq!(friendly_name.as_deref(), Some("Unrecognized Device"));
        }
        _ => panic!("expected unrecognized event"),
    }
}

// ===== Story 7.3 Tests =====

#[test]
fn test_empty_device_name_falls_back_to_device_id() {
    let device_id = "abc-123-def".to_string();
    // Simulates the name resolution logic in handle_get_daemon_state connected_devices_json
    let name_some_empty: Option<String> = Some("".to_string());
    let resolved = name_some_empty
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| device_id.clone());
    assert_eq!(
        resolved, device_id,
        "empty name must fall back to device_id"
    );

    let name_none: Option<String> = None;
    let resolved_none = name_none
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| device_id.clone());
    assert_eq!(
        resolved_none, device_id,
        "None name must fall back to device_id"
    );

    let name_real: Option<String> = Some("My Garmin".to_string());
    let resolved_real = name_real
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| device_id.clone());
    assert_eq!(resolved_real, "My Garmin", "real name must be preserved");
}

// ===== Story 7.4 Tests =====

#[test]
fn test_boot_volume_device_is_skipped_when_device_id_matches_root() {
    assert!(
        is_boot_volume_device(Some(42), 42),
        "candidate with the root device ID must be skipped"
    );
}

#[test]
fn test_boot_volume_device_allows_different_device_id() {
    assert!(
        !is_boot_volume_device(Some(43), 42),
        "candidate with a different device ID can be considered a removable mount"
    );
}

#[test]
fn test_boot_volume_device_metadata_error_is_fail_safe_skip() {
    assert!(
        is_boot_volume_device(None, 42),
        "metadata failures must skip the candidate rather than risk selecting the boot volume"
    );
}

#[tokio::test]
async fn test_cleanup_tmp_files_at_device_root() {
    // T8: root-level .tmp files must be swept even with empty managed_paths.
    let dir = tempdir().unwrap();
    let tmp_file = dir.path().join("partial.flac.tmp");
    tokio::fs::write(&tmp_file, b"partial").await.unwrap();
    assert!(tmp_file.exists());

    let count = cleanup_tmp_files(msc(dir.path()), &[]).await.unwrap();
    assert_eq!(count, 1, "root-level .tmp file must be deleted");
    assert!(!tmp_file.exists());
}

#[tokio::test]
async fn test_cleanup_tmp_files_root_and_managed() {
    // T8: both root and managed-path .tmp files are swept.
    let dir = tempdir().unwrap();
    let music_dir = dir.path().join("Music");
    tokio::fs::create_dir_all(&music_dir).await.unwrap();
    let root_tmp = dir.path().join("partial.tmp");
    let music_tmp = music_dir.join("track.flac.tmp");
    tokio::fs::write(&root_tmp, b"a").await.unwrap();
    tokio::fs::write(&music_tmp, b"b").await.unwrap();

    let count = cleanup_tmp_files(msc(dir.path()), &["Music".to_string()])
        .await
        .unwrap();
    assert_eq!(
        count, 2,
        "both root and managed-path .tmp files must be deleted"
    );
    assert!(!root_tmp.exists());
    assert!(!music_tmp.exists());
}

// ===== Story 8.6 Tests =====

#[test]
fn test_synced_item_provider_metadata_defaults_for_old_manifests() {
    let json = r#"{
        "providerItemId": "song1",
        "name": "Track",
        "album": "Album",
        "artist": "Artist",
        "localPath": "Music/Artist/Album/Track.mp3",
        "sizeBytes": 1234,
        "syncedAt": "2026-05-09T10:00:00Z",
        "etag": "v1"
    }"#;

    let item: SyncedItem =
        serde_json::from_str(json).expect("synced item with provider-neutral item id");

    assert_eq!(item.jellyfin_id, "song1");
    assert_eq!(item.provider_album_id, None);
    assert_eq!(item.provider_content_type, None);
    assert_eq!(item.provider_suffix, None);
}

#[test]
fn test_synced_item_provider_metadata_serializes_camel_case_and_builds_context() {
    let item = SyncedItem {
        jellyfin_id: "song1".to_string(),
        name: "Track".to_string(),
        album: Some("Album".to_string()),
        artist: Some("Artist".to_string()),
        local_path: "Music/Artist/Album/Track.mp3".to_string(),
        size_bytes: 1234,
        synced_at: "2026-05-09T10:00:00Z".to_string(),
        original_name: None,
        etag: Some("v1".to_string()),
        provider_album_id: Some("album1".to_string()),
        provider_content_type: Some("audio/mpeg".to_string()),
        provider_suffix: Some("mp3".to_string()),
    };
    let value = serde_json::to_value(&item).expect("synced item json");

    assert_eq!(value["providerItemId"].as_str(), Some("song1"));
    assert!(value.get("jellyfinId").is_none());
    assert_eq!(value["providerAlbumId"].as_str(), Some("album1"));
    assert_eq!(value["providerContentType"].as_str(), Some("audio/mpeg"));
    assert_eq!(value["providerSuffix"].as_str(), Some("mp3"));
    assert!(value.get("provider_album_id").is_none());

    let mut manifest = DeviceManifest {
        device_id: "dev1".to_string(),
        name: None,
        icon: None,
        version: "1.1".to_string(),
        managed_paths: vec!["Music".to_string()],
        synced_items: vec![item],
        dirty: false,
        pending_item_ids: vec![],
        basket_items: vec![],
        auto_sync_on_connect: false,
        auto_fill: crate::device::AutoFillPrefs::default(),
        transcoding_profile_id: None,
        playlists: vec![],
        storage_id: None,
    };
    let context = manifest.provider_change_context();

    assert_eq!(context.synced_songs.len(), 1);
    assert_eq!(context.synced_songs[0].song_id, "song1");
    assert_eq!(context.synced_songs[0].album_id.as_deref(), Some("album1"));
    assert_eq!(context.synced_songs[0].size, Some(1234));
    assert_eq!(
        context.synced_songs[0].content_type.as_deref(),
        Some("audio/mpeg")
    );
    assert_eq!(context.synced_songs[0].suffix.as_deref(), Some("mp3"));

    manifest.synced_items.clear();
    assert!(manifest.provider_change_context().synced_songs.is_empty());
}
