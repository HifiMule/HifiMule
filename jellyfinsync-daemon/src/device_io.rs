use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub path: String,
    pub name: String,
    pub size: u64,
}

#[async_trait]
pub trait DeviceIO: Send + Sync + std::fmt::Debug {
    async fn read_file(&self, path: &str) -> Result<Vec<u8>>;
    async fn write_file(&self, path: &str, data: &[u8]) -> Result<()>;
    async fn write_with_verify(&self, path: &str, data: &[u8]) -> Result<()>;
    async fn delete_file(&self, path: &str) -> Result<()>;
    async fn list_files(&self, path: &str) -> Result<Vec<FileEntry>>;
    async fn free_space(&self) -> Result<u64>;
    async fn ensure_dir(&self, path: &str) -> Result<()>;
    async fn cleanup_empty_subdirs(&self, path: &str) -> Result<()>;
    fn preferred_audio_container(&self) -> Option<&'static str> {
        None
    }
}

// ─── MSC Backend ────────────────────────────────────────────────────────────

/// Validates that a DeviceIO path is relative and contains no parent-directory traversal.
fn check_relative(path: &str) -> Result<()> {
    let p = std::path::Path::new(path);
    if p.is_absolute() {
        return Err(anyhow::anyhow!("DeviceIO path must be relative: {}", path));
    }
    for component in p.components() {
        if matches!(component, std::path::Component::ParentDir) {
            return Err(anyhow::anyhow!("DeviceIO path must not traverse parent directories: {}", path));
        }
    }
    Ok(())
}

/// Recursively removes empty subdirectories under `path`. The `path` root itself is not removed.
async fn msc_cleanup_empty_dirs(path: &std::path::Path) -> Result<()> {
    if !path.is_dir() {
        return Ok(());
    }
    let mut entries = tokio::fs::read_dir(path).await?;
    while let Some(entry) = entries.next_entry().await? {
        let entry_path = entry.path();
        if entry.file_type().await?.is_dir() {
            Box::pin(msc_cleanup_empty_dirs(&entry_path)).await?;
            let mut sub_entries = tokio::fs::read_dir(&entry_path).await?;
            if sub_entries.next_entry().await?.is_none() {
                if let Err(e) = tokio::fs::remove_dir(&entry_path).await {
                    eprintln!("[Sync] Warning: failed to remove empty directory {}: {}", entry_path.display(), e);
                }
            }
        }
    }
    Ok(())
}

#[derive(Debug)]
pub struct MscBackend {
    pub root: PathBuf,
}

impl MscBackend {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

#[async_trait]
impl DeviceIO for MscBackend {
    async fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        check_relative(path)?;
        let full = self.root.join(path);
        Ok(tokio::fs::read(&full).await?)
    }

    async fn write_file(&self, path: &str, data: &[u8]) -> Result<()> {
        check_relative(path)?;
        let full = self.root.join(path);
        if let Some(parent) = full.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(&full, data).await?;
        Ok(())
    }

    async fn write_with_verify(&self, path: &str, data: &[u8]) -> Result<()> {
        use tokio::io::AsyncWriteExt;

        check_relative(path)?;
        let full = self.root.join(path);
        if let Some(parent) = full.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let tmp_path = full.with_file_name(format!(
            "{}.tmp",
            full.file_name().unwrap_or_default().to_string_lossy()
        ));

        let write_result: Result<()> = async {
            let mut file = tokio::fs::File::create(&tmp_path).await?;
            file.write_all(data).await?;
            file.sync_all().await?;
            Ok(())
        }
        .await;

        if write_result.is_err() {
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return write_result;
        }

        tokio::fs::rename(&tmp_path, &full).await?;
        Ok(())
    }

    async fn delete_file(&self, path: &str) -> Result<()> {
        check_relative(path)?;
        let full = self.root.join(path);
        tokio::fs::remove_file(&full).await?;
        Ok(())
    }

    async fn list_files(&self, path: &str) -> Result<Vec<FileEntry>> {
        if !path.is_empty() {
            check_relative(path)?;
        }
        let base = if path.is_empty() {
            self.root.clone()
        } else {
            self.root.join(path)
        };

        let mut entries = Vec::new();

        if let Ok(meta) = tokio::fs::symlink_metadata(&base).await {
            if !meta.is_dir() {
                return Ok(entries);
            }
        } else {
            return Ok(entries);
        }

        let mut dirs_to_visit = vec![base.clone()];
        while let Some(dir) = dirs_to_visit.pop() {
            let mut read_dir = match tokio::fs::read_dir(&dir).await {
                Ok(r) => r,
                Err(_) => continue,
            };
            while let Some(entry) = read_dir.next_entry().await.unwrap_or(None) {
                let entry_path = entry.path();
                let file_type = match entry.file_type().await {
                    Ok(ft) => ft,
                    Err(_) => continue,
                };

                if file_type.is_symlink() {
                    continue;
                } else if file_type.is_dir() {
                    dirs_to_visit.push(entry_path);
                } else if file_type.is_file() {
                    let size = entry.metadata().await.map(|m| m.len()).unwrap_or(0);
                    let name = entry.file_name().to_string_lossy().to_string();
                    let rel = entry_path
                        .strip_prefix(&self.root)
                        .unwrap_or(&entry_path)
                        .to_string_lossy()
                        .replace('\\', "/");
                    entries.push(FileEntry { path: rel, name, size });
                }
            }
        }

        Ok(entries)
    }

    async fn free_space(&self) -> Result<u64> {
        crate::device::get_storage_info_free_bytes(&self.root)
    }

    async fn ensure_dir(&self, path: &str) -> Result<()> {
        let full = self.root.join(path);
        tokio::fs::create_dir_all(&full).await?;
        Ok(())
    }

    async fn cleanup_empty_subdirs(&self, path: &str) -> Result<()> {
        if !path.is_empty() {
            check_relative(path)?;
        }
        let base = if path.is_empty() { self.root.clone() } else { self.root.join(path) };
        msc_cleanup_empty_dirs(&base).await
    }
}

// ─── MTP Backend ─────────────────────────────────────────────────────────────
//
// MtpBackend is fully implemented and unit-testable via MockMtpHandle.
// DeviceManager always instantiates MscBackend today; MtpBackend activates
// when Story 2.10 (MTP device detection) lands.

pub struct MtpBackend {
    pub handle: Arc<dyn MtpHandle>,
}

impl std::fmt::Debug for MtpBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MtpBackend").finish_non_exhaustive()
    }
}

/// Platform-independent MTP handle trait, enabling mock injection for tests.
pub trait MtpHandle: Send + Sync {
    fn read_file(&self, path: &str) -> Result<Vec<u8>>;
    fn write_file(&self, path: &str, data: &[u8]) -> Result<()>;
    fn delete_file(&self, path: &str) -> Result<()>;
    fn list_files(&self, path: &str) -> Result<Vec<FileEntry>>;
    fn free_space(&self) -> Result<u64>;
    fn preferred_audio_container(&self) -> Option<&'static str> {
        None
    }
}

#[async_trait]
impl DeviceIO for MtpBackend {
    async fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        let handle = Arc::clone(&self.handle);
        let path = path.to_string();
        tokio::task::spawn_blocking(move || handle.read_file(&path))
            .await
            .map_err(|e| anyhow::anyhow!("MTP read_file task panicked: {}", e))?
    }

    async fn write_file(&self, path: &str, data: &[u8]) -> Result<()> {
        let handle = Arc::clone(&self.handle);
        let path = path.to_string();
        let data = data.to_vec();
        tokio::task::spawn_blocking(move || handle.write_file(&path, &data))
            .await
            .map_err(|e| anyhow::anyhow!("MTP write_file task panicked: {}", e))?
    }

    async fn write_with_verify(&self, path: &str, data: &[u8]) -> Result<()> {
        // MTP providers can reject or silently drop synthetic marker files.
        // The manifest-level dirty flag already tracks interrupted syncs; keep
        // the device write path focused on the real destination object.
        self.write_file(path, data).await
    }

    async fn delete_file(&self, path: &str) -> Result<()> {
        let handle = Arc::clone(&self.handle);
        let path = path.to_string();
        tokio::task::spawn_blocking(move || handle.delete_file(&path))
            .await
            .map_err(|e| anyhow::anyhow!("MTP delete_file task panicked: {}", e))?
    }

    async fn list_files(&self, path: &str) -> Result<Vec<FileEntry>> {
        let handle = Arc::clone(&self.handle);
        let path = path.to_string();
        tokio::task::spawn_blocking(move || handle.list_files(&path))
            .await
            .map_err(|e| anyhow::anyhow!("MTP list_files task panicked: {}", e))?
    }

    async fn free_space(&self) -> Result<u64> {
        let handle = Arc::clone(&self.handle);
        tokio::task::spawn_blocking(move || handle.free_space())
            .await
            .map_err(|e| anyhow::anyhow!("MTP free_space task panicked: {}", e))?
    }

    // MTP creates parent directories automatically when objects are created.
    async fn ensure_dir(&self, _path: &str) -> Result<()> {
        Ok(())
    }

    // MTP manages directory objects automatically; empty directory pruning is not needed.
    async fn cleanup_empty_subdirs(&self, _path: &str) -> Result<()> {
        Ok(())
    }

    fn preferred_audio_container(&self) -> Option<&'static str> {
        self.handle.preferred_audio_container()
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use tempfile::tempdir;

    // ── MscBackend tests ───────────────────────────────────────────────────

    #[tokio::test]
    async fn msc_write_read_roundtrip() {
        let dir = tempdir().unwrap();
        let backend = MscBackend::new(dir.path().to_path_buf());
        backend.write_file("test.txt", b"hello").await.unwrap();
        let data = backend.read_file("test.txt").await.unwrap();
        assert_eq!(data, b"hello");
    }

    #[tokio::test]
    async fn msc_write_file_creates_parent_dirs() {
        let dir = tempdir().unwrap();
        let backend = MscBackend::new(dir.path().to_path_buf());
        backend
            .write_file("Music/Artist/Album/track.flac", b"data")
            .await
            .unwrap();
        assert!(dir.path().join("Music/Artist/Album/track.flac").exists());
    }

    #[tokio::test]
    async fn msc_write_with_verify_no_tmp_on_success() {
        let dir = tempdir().unwrap();
        let backend = MscBackend::new(dir.path().to_path_buf());
        backend
            .write_with_verify(".jellyfinsync.json", b"{}")
            .await
            .unwrap();
        assert!(dir.path().join(".jellyfinsync.json").exists());
        assert!(!dir.path().join(".jellyfinsync.json.tmp").exists());
    }

    #[tokio::test]
    async fn msc_delete_file() {
        let dir = tempdir().unwrap();
        let backend = MscBackend::new(dir.path().to_path_buf());
        backend.write_file("a.txt", b"x").await.unwrap();
        backend.delete_file("a.txt").await.unwrap();
        assert!(!dir.path().join("a.txt").exists());
    }

    #[tokio::test]
    async fn msc_ensure_dir_creates_path() {
        let dir = tempdir().unwrap();
        let backend = MscBackend::new(dir.path().to_path_buf());
        backend.ensure_dir("Music/JellyfinSync").await.unwrap();
        assert!(dir.path().join("Music/JellyfinSync").is_dir());
    }

    #[tokio::test]
    async fn msc_ensure_dir_idempotent() {
        let dir = tempdir().unwrap();
        let backend = MscBackend::new(dir.path().to_path_buf());
        backend.ensure_dir("Music").await.unwrap();
        backend.ensure_dir("Music").await.unwrap(); // already exists — should not error
    }

    #[tokio::test]
    async fn msc_list_files_recursive() {
        let dir = tempdir().unwrap();
        let backend = MscBackend::new(dir.path().to_path_buf());
        backend.write_file("Music/a.mp3", b"").await.unwrap();
        backend.write_file("Music/Sub/b.flac", b"").await.unwrap();
        let mut files = backend.list_files("").await.unwrap();
        files.sort_by(|a, b| a.path.cmp(&b.path));
        let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        assert!(paths.contains(&"Music/a.mp3"));
        assert!(paths.contains(&"Music/Sub/b.flac"));
    }

    // ── MockMtpHandle ──────────────────────────────────────────────────────

    pub struct MockMtpHandle {
        pub files: Mutex<HashMap<String, Vec<u8>>>,
        pub call_log: Mutex<Vec<String>>,
    }

    impl MockMtpHandle {
        pub fn new() -> Self {
            Self {
                files: Mutex::new(HashMap::new()),
                call_log: Mutex::new(Vec::new()),
            }
        }
    }

    impl MtpHandle for MockMtpHandle {
        fn read_file(&self, path: &str) -> Result<Vec<u8>> {
            self.call_log.lock().unwrap().push(format!("read:{}", path));
            self.files
                .lock()
                .unwrap()
                .get(path)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("file not found: {}", path))
        }

        fn write_file(&self, path: &str, data: &[u8]) -> Result<()> {
            self.call_log
                .lock()
                .unwrap()
                .push(format!("write:{}", path));
            self.files
                .lock()
                .unwrap()
                .insert(path.to_string(), data.to_vec());
            Ok(())
        }

        fn delete_file(&self, path: &str) -> Result<()> {
            self.call_log
                .lock()
                .unwrap()
                .push(format!("delete:{}", path));
            self.files.lock().unwrap().remove(path);
            Ok(())
        }

        fn list_files(&self, _path: &str) -> Result<Vec<FileEntry>> {
            let files = self.files.lock().unwrap();
            Ok(files
                .keys()
                .map(|k| FileEntry {
                    path: k.clone(),
                    name: k.split('/').last().unwrap_or(k).to_string(),
                    size: 0,
                })
                .collect())
        }

        fn free_space(&self) -> Result<u64> {
            Ok(1_000_000_000)
        }
    }

    // ── MtpBackend tests ───────────────────────────────────────────────────

    #[tokio::test]
    async fn mtp_write_with_verify_writes_target_only() {
        let mock = Arc::new(MockMtpHandle::new());
        let backend = MtpBackend {
            handle: Arc::clone(&mock) as Arc<dyn MtpHandle>,
        };

        backend
            .write_with_verify("Music/track.mp3", b"audio")
            .await
            .unwrap();

        let log = mock.call_log.lock().unwrap().clone();
        assert_eq!(log, vec!["write:Music/track.mp3"]);

        assert!(mock.files.lock().unwrap().contains_key("Music/track.mp3"));
        assert!(!mock
            .files
            .lock()
            .unwrap()
            .contains_key("Music/track.mp3.dirty"));
    }

    #[tokio::test]
    async fn mtp_backend_manifest_probe() {
        let mock = Arc::new(MockMtpHandle::new());
        mock.files.lock().unwrap().insert(
            ".jellyfinsync.json".to_string(),
            br#"{"device_id":"test-id","version":"1.0","managedPaths":[],"syncedItems":[]}"#.to_vec(),
        );
        let backend = MtpBackend {
            handle: Arc::clone(&mock) as Arc<dyn MtpHandle>,
        };
        let data = backend.read_file(".jellyfinsync.json").await.unwrap();
        let manifest: serde_json::Value = serde_json::from_slice(&data).unwrap();
        assert_eq!(manifest["device_id"], "test-id");
    }

    #[tokio::test]
    async fn mtp_dirty_marker_detected_on_reconnect() {
        let mock = Arc::new(MockMtpHandle::new());
        // Pre-populate: target file + dirty marker (simulates interrupted write)
        mock.files
            .lock()
            .unwrap()
            .insert("Music/track.mp3".to_string(), b"partial".to_vec());
        mock.files
            .lock()
            .unwrap()
            .insert("Music/track.mp3.dirty".to_string(), vec![]);

        let backend = MtpBackend {
            handle: Arc::clone(&mock) as Arc<dyn MtpHandle>,
        };

        let files = backend.list_files("").await.unwrap();
        let has_dirty = files.iter().any(|f| f.path.ends_with(".dirty"));
        assert!(has_dirty, "dirty marker must be visible in listing");
    }
}
