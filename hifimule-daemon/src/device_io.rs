use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
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
    async fn begin_sync_job(&self) -> Result<()> {
        Ok(())
    }
    async fn read_file(&self, path: &str) -> Result<Vec<u8>>;
    async fn write_file(&self, path: &str, data: &[u8]) -> Result<()>;
    async fn write_with_verify(&self, path: &str, data: &[u8]) -> Result<()>;
    /// Write a staged file without loading its contents into memory. Test doubles may
    /// use this default; production backends override it with streaming transfers.
    async fn write_with_verify_from_path(&self, path: &str, source: &Path) -> Result<()> {
        let data = tokio::fs::read(source).await?;
        self.write_with_verify(path, &data).await
    }
    async fn delete_file(&self, path: &str) -> Result<()>;
    async fn list_files(&self, path: &str) -> Result<Vec<FileEntry>>;
    async fn free_space(&self) -> Result<u64>;
    async fn storage_id(&self) -> Result<Option<String>> {
        Ok(None)
    }
    async fn ensure_dir(&self, path: &str) -> Result<()>;
    async fn cleanup_empty_subdirs(&self, path: &str) -> Result<()>;
    async fn take_warnings(&self) -> Vec<String> {
        Vec::new()
    }
    async fn end_sync_job(&self) -> Result<()> {
        Ok(())
    }
    fn preferred_audio_container(&self) -> Option<&'static str> {
        None
    }
    /// Returns true if write_with_verify already confirms the file exists on the device
    /// (e.g. via a direct object-ID lookup), so the caller can skip a redundant list_files
    /// existence check. Devices that hide files from enumeration (e.g. some Garmin watches)
    /// should return true here and do internal verification instead.
    fn write_verifies_internally(&self) -> bool {
        false
    }
    /// Returns true if the file at `path` (relative to device root) exists on the device.
    /// The default implementation uses `list_files`, which may have Unicode normalization
    /// issues on macOS. MSC backends override this with a direct `metadata` lookup that
    /// is normalization-insensitive on all macOS filesystems (FAT32, HFS+, APFS).
    async fn file_exists(&self, path: &str) -> bool {
        let parent = path.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
        self.list_files(parent)
            .await
            .map(|files| files.iter().any(|f| f.path == path))
            .unwrap_or(false)
    }
    async fn load_folder_hints(&self, _hints: std::collections::HashMap<String, u32>) {}
    async fn drain_folder_hints(&self) -> std::collections::HashMap<String, u32> {
        std::collections::HashMap::new()
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
            return Err(anyhow::anyhow!(
                "DeviceIO path must not traverse parent directories: {}",
                path
            ));
        }
    }
    let normalized = path.replace('\\', "/");
    if normalized.starts_with('/') {
        return Err(anyhow::anyhow!("DeviceIO path must be relative: {}", path));
    }
    for (index, component) in normalized.split('/').enumerate() {
        if component == "." || component == ".." {
            return Err(anyhow::anyhow!(
                "DeviceIO path must not traverse parent directories: {}",
                path
            ));
        }
        if index == 0
            && component.len() == 2
            && component.as_bytes()[1] == b':'
            && component.as_bytes()[0].is_ascii_alphabetic()
        {
            return Err(anyhow::anyhow!("DeviceIO path must be relative: {}", path));
        }
    }
    Ok(())
}

fn check_delete_target(path: &str) -> Result<()> {
    check_relative(path)?;
    let normalized = path.replace('\\', "/");
    if normalized.is_empty() || normalized.ends_with('/') {
        return Err(anyhow::anyhow!(
            "DeviceIO delete path must name a file: {}",
            path
        ));
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
            if sub_entries.next_entry().await?.is_none()
                && let Err(e) = tokio::fs::remove_dir(&entry_path).await
            {
                eprintln!(
                    "[Sync] Warning: failed to remove empty directory {}: {}",
                    entry_path.display(),
                    e
                );
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

/// Removes an incomplete MSC sibling file if a transfer is cancelled or fails.
struct PendingMscWrite(PathBuf);

impl Drop for PendingMscWrite {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn is_missing_object_error(error: &anyhow::Error) -> bool {
    if error
        .downcast_ref::<std::io::Error>()
        .map(|io| io.kind() == std::io::ErrorKind::NotFound)
        .unwrap_or(false)
    {
        return true;
    }

    let message = error.to_string().to_ascii_lowercase();
    message.contains("mtp path component not found")
        || message.contains("libmtp: path component") && message.contains("not found")
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

    async fn write_with_verify_from_path(&self, path: &str, source: &Path) -> Result<()> {
        check_relative(path)?;
        let full = self.root.join(path);
        if let Some(parent) = full.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let tmp_path = full.with_file_name(format!(
            "{}.tmp",
            full.file_name().unwrap_or_default().to_string_lossy()
        ));
        let _cleanup = PendingMscWrite(tmp_path.clone());
        let mut input = tokio::fs::File::open(source).await?;
        let source_size = input.metadata().await?.len();
        let mut output = tokio::fs::File::create(&tmp_path).await?;
        let copied = tokio::io::copy(&mut input, &mut output).await?;
        output.sync_all().await?;
        if copied != source_size || input.metadata().await?.len() != source_size {
            anyhow::bail!("MSC source changed size during transfer");
        }
        drop(output);
        tokio::fs::rename(&tmp_path, &full).await?;
        Ok(())
    }

    async fn delete_file(&self, path: &str) -> Result<()> {
        check_delete_target(path)?;
        let full = self.root.join(path);
        match tokio::fs::remove_file(&full).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
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
                    entries.push(FileEntry {
                        path: rel,
                        name,
                        size,
                    });
                }
            }
        }

        Ok(entries)
    }

    async fn file_exists(&self, path: &str) -> bool {
        // Direct metadata check: normalization-insensitive on macOS for all filesystem
        // types (FAT32, HFS+, APFS), avoiding NFC/NFD comparison failures that occur
        // when the OS returns a different Unicode normalization form than what was written.
        if check_relative(path).is_err() {
            return false;
        }
        let full = self.root.join(path);
        tokio::fs::metadata(&full).await.is_ok()
    }

    async fn free_space(&self) -> Result<u64> {
        crate::device::get_storage_info_free_bytes(&self.root)
    }

    async fn ensure_dir(&self, path: &str) -> Result<()> {
        if !path.is_empty() {
            check_relative(path)?;
        }
        let full = self.root.join(path);
        tokio::fs::create_dir_all(&full).await?;
        Ok(())
    }

    async fn cleanup_empty_subdirs(&self, path: &str) -> Result<()> {
        if !path.is_empty() {
            check_relative(path)?;
        }
        let base = if path.is_empty() {
            self.root.clone()
        } else {
            self.root.join(path)
        };
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
    operation_lock: Arc<tokio::sync::Mutex<()>>,
}

impl MtpBackend {
    pub fn new(handle: Arc<dyn MtpHandle>) -> Self {
        Self {
            handle,
            operation_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }
}

impl std::fmt::Debug for MtpBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MtpBackend").finish_non_exhaustive()
    }
}

/// Platform-independent MTP handle trait, enabling mock injection for tests.
pub trait MtpHandle: Send + Sync {
    fn begin_sync_job(&self) -> Result<()> {
        Ok(())
    }
    fn read_file(&self, path: &str) -> Result<Vec<u8>>;
    fn write_file(&self, path: &str, data: &[u8]) -> Result<()>;
    fn write_file_from_path(&self, path: &str, source: &Path) -> Result<()> {
        let data = std::fs::read(source)?;
        self.write_file(path, &data)
    }
    fn delete_file(&self, path: &str) -> Result<()>;
    fn list_files(&self, path: &str) -> Result<Vec<FileEntry>>;
    fn free_space(&self) -> Result<u64>;
    fn storage_id(&self) -> Result<Option<String>> {
        Ok(None)
    }
    fn take_warnings(&self) -> Vec<String> {
        Vec::new()
    }
    fn end_sync_job(&self) -> Result<()> {
        Ok(())
    }
    fn preferred_audio_container(&self) -> Option<&'static str> {
        None
    }
    /// Load previously cached MTP folder IDs into the handle for the upcoming sync job.
    /// Only meaningful on libmtp; other backends ignore this via the default no-op.
    fn load_folder_hints(&self, _hints: std::collections::HashMap<String, u32>) {}
    /// Drain all folder IDs discovered or used during the sync job, clearing the cache.
    /// Returns a map of device-relative path → LIBMTP folder object ID.
    fn drain_folder_hints(&self) -> std::collections::HashMap<String, u32> {
        std::collections::HashMap::new()
    }
}

#[async_trait]
impl DeviceIO for MtpBackend {
    async fn begin_sync_job(&self) -> Result<()> {
        let _guard = self.operation_lock.lock().await;
        let handle = Arc::clone(&self.handle);
        tokio::task::spawn_blocking(move || handle.begin_sync_job())
            .await
            .map_err(|e| anyhow::anyhow!("MTP begin_sync_job task panicked: {}", e))?
    }

    async fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        let _guard = self.operation_lock.lock().await;
        let handle = Arc::clone(&self.handle);
        let path = path.to_string();
        tokio::task::spawn_blocking(move || handle.read_file(&path))
            .await
            .map_err(|e| anyhow::anyhow!("MTP read_file task panicked: {}", e))?
    }

    async fn write_file(&self, path: &str, data: &[u8]) -> Result<()> {
        let _guard = self.operation_lock.lock().await;
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

    async fn write_with_verify_from_path(&self, path: &str, source: &Path) -> Result<()> {
        check_relative(path)?;
        // Keep both the device lock and a hard link to the staged bytes inside
        // the blocking task. Dropping the async caller must not let staging
        // cleanup remove its source or let another MTP operation start early.
        let guard = Arc::clone(&self.operation_lock).lock_owned().await;
        let source_dir = tempfile::tempdir()?;
        let preserved_source = source_dir.path().join("source");
        std::fs::hard_link(source, &preserved_source).map_err(|error| {
            anyhow::anyhow!(
                "Cannot preserve staged file for MTP transfer with a hard link: {error}"
            )
        })?;
        let handle = Arc::clone(&self.handle);
        let path = path.to_string();
        tokio::task::spawn_blocking(move || {
            let _guard = guard;
            let _source_dir = source_dir;
            handle.write_file_from_path(&path, &preserved_source)
        })
        .await
        .map_err(|e| anyhow::anyhow!("MTP write_file_from_path task panicked: {}", e))?
    }

    async fn delete_file(&self, path: &str) -> Result<()> {
        check_delete_target(path)?;
        let _guard = self.operation_lock.lock().await;
        let handle = Arc::clone(&self.handle);
        let path = path.to_string();
        let result = tokio::task::spawn_blocking(move || handle.delete_file(&path))
            .await
            .map_err(|e| anyhow::anyhow!("MTP delete_file task panicked: {}", e))?;
        match result {
            Ok(()) => Ok(()),
            Err(e) if is_missing_object_error(&e) => Ok(()),
            Err(e) => Err(e),
        }
    }

    async fn list_files(&self, path: &str) -> Result<Vec<FileEntry>> {
        let _guard = self.operation_lock.lock().await;
        let handle = Arc::clone(&self.handle);
        let path = path.to_string();
        tokio::task::spawn_blocking(move || handle.list_files(&path))
            .await
            .map_err(|e| anyhow::anyhow!("MTP list_files task panicked: {}", e))?
    }

    async fn free_space(&self) -> Result<u64> {
        let _guard = self.operation_lock.lock().await;
        let handle = Arc::clone(&self.handle);
        tokio::task::spawn_blocking(move || handle.free_space())
            .await
            .map_err(|e| anyhow::anyhow!("MTP free_space task panicked: {}", e))?
    }

    async fn storage_id(&self) -> Result<Option<String>> {
        let _guard = self.operation_lock.lock().await;
        let handle = Arc::clone(&self.handle);
        tokio::task::spawn_blocking(move || handle.storage_id())
            .await
            .map_err(|e| anyhow::anyhow!("MTP storage_id task panicked: {}", e))?
    }

    // MTP creates parent directories automatically when objects are created.
    async fn ensure_dir(&self, _path: &str) -> Result<()> {
        Ok(())
    }

    // MTP manages directory objects automatically; empty directory pruning is not needed.
    async fn cleanup_empty_subdirs(&self, _path: &str) -> Result<()> {
        Ok(())
    }

    async fn take_warnings(&self) -> Vec<String> {
        let _guard = self.operation_lock.lock().await;
        let handle = Arc::clone(&self.handle);
        tokio::task::spawn_blocking(move || handle.take_warnings())
            .await
            .unwrap_or_default()
    }

    async fn end_sync_job(&self) -> Result<()> {
        let _guard = self.operation_lock.lock().await;
        let handle = Arc::clone(&self.handle);
        tokio::task::spawn_blocking(move || handle.end_sync_job())
            .await
            .map_err(|e| anyhow::anyhow!("MTP end_sync_job task panicked: {}", e))?
    }

    fn preferred_audio_container(&self) -> Option<&'static str> {
        self.handle.preferred_audio_container()
    }

    fn write_verifies_internally(&self) -> bool {
        true
    }

    // operation_lock is intentionally NOT acquired for load_folder_hints /
    // drain_folder_hints. These methods only touch an in-memory Mutex<HashMap>
    // inside LibmtpHandle, not the device pointer. The inner Mutex provides the
    // necessary synchronization. Acquiring operation_lock would risk deadlock if
    // either method is ever called while a device operation is in flight.
    async fn load_folder_hints(&self, hints: std::collections::HashMap<String, u32>) {
        let handle = Arc::clone(&self.handle);
        tokio::task::spawn_blocking(move || handle.load_folder_hints(hints))
            .await
            .ok();
    }

    async fn drain_folder_hints(&self) -> std::collections::HashMap<String, u32> {
        let handle = Arc::clone(&self.handle);
        tokio::task::spawn_blocking(move || handle.drain_folder_hints())
            .await
            .unwrap_or_default()
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;
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
    async fn msc_streamed_write_replaces_file_and_cleans_failed_temp() {
        let dir = tempdir().unwrap();
        let backend = MscBackend::new(dir.path().join("device"));
        let source = dir.path().join("staged.audio");
        let bytes: Vec<u8> = (0..2_500_000).map(|index| (index % 251) as u8).collect();
        tokio::fs::write(&source, &bytes).await.unwrap();
        backend.write_file("Music/book.mp3", b"old").await.unwrap();

        backend
            .write_with_verify_from_path("Music/book.mp3", &source)
            .await
            .unwrap();
        assert_eq!(
            tokio::fs::read(dir.path().join("device/Music/book.mp3"))
                .await
                .unwrap(),
            bytes
        );
        assert!(!dir.path().join("device/Music/book.mp3.tmp").exists());

        let error = backend
            .write_with_verify_from_path("Music/book.mp3", &dir.path().join("missing"))
            .await
            .unwrap_err();
        assert_eq!(
            error.downcast_ref::<std::io::Error>().unwrap().kind(),
            std::io::ErrorKind::NotFound
        );
        assert!(!dir.path().join("device/Music/book.mp3.tmp").exists());
        assert_eq!(
            tokio::fs::read(dir.path().join("device/Music/book.mp3"))
                .await
                .unwrap(),
            bytes
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn msc_cancelled_partial_copy_removes_temp_and_preserves_target() {
        use std::os::unix::ffi::OsStrExt;

        let dir = tempdir().unwrap();
        let source = dir.path().join("source.pipe");
        let source_name = std::ffi::CString::new(source.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(source_name.as_ptr(), 0o600) }, 0);
        let backend = Arc::new(MscBackend::new(dir.path().join("device")));
        backend.write_file("Music/book.mp3", b"old").await.unwrap();
        let tmp_path = dir.path().join("device/Music/book.mp3.tmp");
        let target_path = dir.path().join("device/Music/book.mp3");

        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let source_for_writer = source.clone();
        let writer = tokio::task::spawn_blocking(move || {
            use std::io::Write;
            let mut pipe = std::fs::OpenOptions::new()
                .write(true)
                .open(source_for_writer)
                .unwrap();
            pipe.write_all(&[42; 8192]).unwrap();
            release_rx.recv().unwrap();
        });
        let transfer = tokio::spawn(async move {
            backend
                .write_with_verify_from_path("Music/book.mp3", &source)
                .await
        });
        let partial_copy = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if tokio::fs::metadata(&tmp_path)
                    .await
                    .is_ok_and(|meta| meta.len() > 0)
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await;
        transfer.abort();
        let _ = transfer.await;
        release_tx.send(()).unwrap();
        writer.await.unwrap();
        partial_copy.unwrap();
        assert!(!tmp_path.exists());
        assert_eq!(tokio::fs::read(target_path).await.unwrap(), b"old");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn msc_rejects_source_size_change_before_replacement() {
        use std::os::unix::ffi::OsStrExt;

        let dir = tempdir().unwrap();
        let source = dir.path().join("source.pipe");
        let source_name = std::ffi::CString::new(source.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(source_name.as_ptr(), 0o600) }, 0);
        let backend = MscBackend::new(dir.path().join("device"));
        backend.write_file("Music/book.mp3", b"old").await.unwrap();
        let source_for_writer = source.clone();
        let writer = tokio::task::spawn_blocking(move || {
            use std::io::Write;
            let mut pipe = std::fs::OpenOptions::new()
                .write(true)
                .open(source_for_writer)
                .unwrap();
            pipe.write_all(b"new bytes").unwrap();
        });

        let error = backend
            .write_with_verify_from_path("Music/book.mp3", &source)
            .await
            .unwrap_err();
        writer.await.unwrap();
        assert!(error.to_string().contains("source changed size"));
        assert_eq!(
            tokio::fs::read(dir.path().join("device/Music/book.mp3"))
                .await
                .unwrap(),
            b"old"
        );
        assert!(!dir.path().join("device/Music/book.mp3.tmp").exists());
    }

    #[tokio::test]
    async fn msc_write_with_verify_no_tmp_on_success() {
        let dir = tempdir().unwrap();
        let backend = MscBackend::new(dir.path().to_path_buf());
        backend
            .write_with_verify(".hifimule.json", b"{}")
            .await
            .unwrap();
        assert!(dir.path().join(".hifimule.json").exists());
        assert!(!dir.path().join(".hifimule.json.tmp").exists());
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
    async fn msc_delete_missing_file_is_idempotent() {
        let dir = tempdir().unwrap();
        let backend = MscBackend::new(dir.path().to_path_buf());

        backend.delete_file("missing.txt").await.unwrap();
    }

    #[tokio::test]
    async fn msc_delete_directory_reports_real_io_error() {
        let dir = tempdir().unwrap();
        let backend = MscBackend::new(dir.path().to_path_buf());
        backend.ensure_dir("Music").await.unwrap();

        let err = backend.delete_file("Music").await.unwrap_err();

        assert!(
            err.downcast_ref::<std::io::Error>()
                .map(|io| io.kind() != std::io::ErrorKind::NotFound)
                .unwrap_or(true),
            "directory deletion failure must not be treated as missing-file success"
        );
    }

    #[tokio::test]
    async fn msc_delete_backslash_parent_traversal_is_rejected() {
        let dir = tempdir().unwrap();
        let backend = MscBackend::new(dir.path().to_path_buf());

        let err = backend
            .delete_file("Music\\..\\outside.txt")
            .await
            .unwrap_err();

        assert!(err.to_string().contains("must not traverse"));
    }

    #[tokio::test]
    async fn msc_ensure_dir_creates_path() {
        let dir = tempdir().unwrap();
        let backend = MscBackend::new(dir.path().to_path_buf());
        backend.ensure_dir("Music/HifiMule").await.unwrap();
        assert!(dir.path().join("Music/HifiMule").is_dir());
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

        fn write_file_from_path(&self, path: &str, source: &Path) -> Result<()> {
            self.call_log
                .lock()
                .unwrap()
                .push(format!("write_from_path:{}", path));
            self.files
                .lock()
                .unwrap()
                .insert(path.to_string(), std::fs::read(source)?);
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

    pub struct FailingDeleteMtpHandle {
        message: &'static str,
    }

    impl FailingDeleteMtpHandle {
        pub fn new(message: &'static str) -> Self {
            Self { message }
        }
    }

    impl MtpHandle for FailingDeleteMtpHandle {
        fn read_file(&self, _path: &str) -> Result<Vec<u8>> {
            Ok(Vec::new())
        }

        fn write_file(&self, _path: &str, _data: &[u8]) -> Result<()> {
            Ok(())
        }

        fn delete_file(&self, _path: &str) -> Result<()> {
            Err(anyhow::anyhow!(self.message))
        }

        fn list_files(&self, _path: &str) -> Result<Vec<FileEntry>> {
            Ok(Vec::new())
        }

        fn free_space(&self) -> Result<u64> {
            Ok(1)
        }
    }

    pub struct ConcurrentMtpHandle {
        in_flight: AtomicUsize,
        max_in_flight: AtomicUsize,
    }

    impl ConcurrentMtpHandle {
        pub fn new() -> Self {
            Self {
                in_flight: AtomicUsize::new(0),
                max_in_flight: AtomicUsize::new(0),
            }
        }

        fn enter(&self) {
            let current = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_in_flight.fetch_max(current, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(25));
            self.in_flight.fetch_sub(1, Ordering::SeqCst);
        }
    }

    impl MtpHandle for ConcurrentMtpHandle {
        fn read_file(&self, _path: &str) -> Result<Vec<u8>> {
            self.enter();
            Ok(Vec::new())
        }

        fn write_file(&self, _path: &str, _data: &[u8]) -> Result<()> {
            self.enter();
            Ok(())
        }

        fn delete_file(&self, _path: &str) -> Result<()> {
            self.enter();
            Ok(())
        }

        fn list_files(&self, _path: &str) -> Result<Vec<FileEntry>> {
            self.enter();
            Ok(Vec::new())
        }

        fn free_space(&self) -> Result<u64> {
            self.enter();
            Ok(1)
        }
    }

    // ── MtpBackend tests ───────────────────────────────────────────────────

    #[tokio::test]
    async fn mtp_write_with_verify_writes_target_only() {
        let mock = Arc::new(MockMtpHandle::new());
        let backend = MtpBackend::new(Arc::clone(&mock) as Arc<dyn MtpHandle>);

        backend
            .write_with_verify("Music/track.mp3", b"audio")
            .await
            .unwrap();

        let log = mock.call_log.lock().unwrap().clone();
        assert_eq!(log, vec!["write:Music/track.mp3"]);

        assert!(mock.files.lock().unwrap().contains_key("Music/track.mp3"));
        assert!(
            !mock
                .files
                .lock()
                .unwrap()
                .contains_key("Music/track.mp3.dirty")
        );
    }

    #[tokio::test]
    async fn mtp_staged_write_forwards_source_path_to_handle() {
        let dir = tempdir().unwrap();
        let source = dir.path().join("staged.audio");
        let bytes: Vec<u8> = (0..1_500_000).map(|index| (index % 251) as u8).collect();
        tokio::fs::write(&source, &bytes).await.unwrap();
        let mock = Arc::new(MockMtpHandle::new());
        let backend = MtpBackend::new(Arc::clone(&mock) as Arc<dyn MtpHandle>);

        backend
            .write_with_verify_from_path("Music/book.mp3", &source)
            .await
            .unwrap();

        assert_eq!(
            *mock.call_log.lock().unwrap(),
            vec!["write_from_path:Music/book.mp3"]
        );
        assert_eq!(mock.files.lock().unwrap()["Music/book.mp3"], bytes);
    }

    struct BlockingPathMtpHandle {
        started: Mutex<Option<tokio::sync::oneshot::Sender<PathBuf>>>,
        release: Mutex<std::sync::mpsc::Receiver<()>>,
        received: Mutex<Vec<u8>>,
        list_calls: AtomicUsize,
    }

    impl MtpHandle for BlockingPathMtpHandle {
        fn read_file(&self, _path: &str) -> Result<Vec<u8>> {
            Ok(Vec::new())
        }
        fn write_file(&self, _path: &str, _data: &[u8]) -> Result<()> {
            Ok(())
        }
        fn write_file_from_path(&self, _path: &str, source: &Path) -> Result<()> {
            self.started
                .lock()
                .unwrap()
                .take()
                .unwrap()
                .send(source.to_path_buf())
                .unwrap();
            self.release.lock().unwrap().recv().unwrap();
            *self.received.lock().unwrap() = std::fs::read(source)?;
            Ok(())
        }
        fn delete_file(&self, _path: &str) -> Result<()> {
            Ok(())
        }
        fn list_files(&self, _path: &str) -> Result<Vec<FileEntry>> {
            self.list_calls.fetch_add(1, Ordering::SeqCst);
            Ok(Vec::new())
        }
        fn free_space(&self) -> Result<u64> {
            Ok(1)
        }
    }

    #[tokio::test]
    async fn mtp_cancelled_caller_keeps_source_and_device_lock_until_transfer_ends() {
        let dir = tempdir().unwrap();
        let source = dir.path().join("staged.audio");
        let bytes = b"staged audio".to_vec();
        tokio::fs::write(&source, &bytes).await.unwrap();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let mock = Arc::new(BlockingPathMtpHandle {
            started: Mutex::new(Some(started_tx)),
            release: Mutex::new(release_rx),
            received: Mutex::new(Vec::new()),
            list_calls: AtomicUsize::new(0),
        });
        let backend = Arc::new(MtpBackend::new(Arc::clone(&mock) as Arc<dyn MtpHandle>));
        let write = {
            let backend = Arc::clone(&backend);
            let source = source.clone();
            tokio::spawn(async move {
                backend
                    .write_with_verify_from_path("Music/book.mp3", &source)
                    .await
            })
        };
        let preserved_source = started_rx.await.unwrap();
        write.abort();
        let _ = write.await;
        tokio::fs::remove_file(&source).await.unwrap();
        assert!(preserved_source.exists());

        let list = {
            let backend = Arc::clone(&backend);
            tokio::spawn(async move { backend.list_files("").await })
        };
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(mock.list_calls.load(Ordering::SeqCst), 0);
        release_tx.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(2), list)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(*mock.received.lock().unwrap(), bytes);
        assert_eq!(mock.list_calls.load(Ordering::SeqCst), 1);
        assert!(!preserved_source.exists());
    }

    #[tokio::test]
    async fn mtp_backend_manifest_probe() {
        let mock = Arc::new(MockMtpHandle::new());
        mock.files.lock().unwrap().insert(
            ".hifimule.json".to_string(),
            br#"{"device_id":"test-id","version":"1.0","managedPaths":[],"syncedItems":[]}"#
                .to_vec(),
        );
        let backend = MtpBackend::new(Arc::clone(&mock) as Arc<dyn MtpHandle>);
        let data = backend.read_file(".hifimule.json").await.unwrap();
        let manifest: serde_json::Value = serde_json::from_slice(&data).unwrap();
        assert_eq!(manifest["device_id"], "test-id");
    }

    #[tokio::test]
    async fn mtp_dirty_marker_detected_on_reconnect() {
        let mock = Arc::new(MockMtpHandle::new());
        // Pre-populate: target file + dirty marker with sentinel content b"\x00"
        mock.files
            .lock()
            .unwrap()
            .insert("Music/track.mp3".to_string(), b"partial".to_vec());
        mock.files
            .lock()
            .unwrap()
            .insert("Music/track.mp3.dirty".to_string(), b"\x00".to_vec());

        let backend = MtpBackend::new(Arc::clone(&mock) as Arc<dyn MtpHandle>);

        let files = backend.list_files("").await.unwrap();
        let has_dirty = files.iter().any(|f| f.path.ends_with(".dirty"));
        assert!(has_dirty, "dirty marker must be visible in listing");

        // T8: assert sentinel content is exactly b"\x00", not merely present.
        let marker_content = backend.read_file("Music/track.mp3.dirty").await.unwrap();
        assert_eq!(
            marker_content, b"\x00",
            "dirty marker must contain sentinel byte \\x00"
        );
    }

    #[tokio::test]
    async fn mtp_backend_serializes_operations_per_backend() {
        let mock = Arc::new(ConcurrentMtpHandle::new());
        let backend = Arc::new(MtpBackend::new(Arc::clone(&mock) as Arc<dyn MtpHandle>));

        let read = {
            let backend = Arc::clone(&backend);
            tokio::spawn(async move { backend.read_file("a").await })
        };
        let write = {
            let backend = Arc::clone(&backend);
            tokio::spawn(async move { backend.write_file("b", b"data").await })
        };
        let list = {
            let backend = Arc::clone(&backend);
            tokio::spawn(async move { backend.list_files("").await })
        };
        let free = {
            let backend = Arc::clone(&backend);
            tokio::spawn(async move { backend.free_space().await })
        };

        let _ = tokio::join!(read, write, list, free);

        assert_eq!(
            mock.max_in_flight.load(Ordering::SeqCst),
            1,
            "operations for one MtpBackend must not overlap"
        );
    }

    #[tokio::test]
    async fn mtp_delete_missing_object_is_idempotent_when_distinguishable() {
        let mock = Arc::new(FailingDeleteMtpHandle::new(
            "libmtp: path component 'missing.mp3' not found",
        ));
        let backend = MtpBackend::new(Arc::clone(&mock) as Arc<dyn MtpHandle>);

        backend.delete_file("Music/missing.mp3").await.unwrap();
    }

    #[tokio::test]
    async fn mtp_delete_generic_failure_remains_visible() {
        let mock = Arc::new(FailingDeleteMtpHandle::new(
            "libmtp delete_file failed: rc=-1",
        ));
        let backend = MtpBackend::new(Arc::clone(&mock) as Arc<dyn MtpHandle>);

        let err = backend.delete_file("Music/track.mp3").await.unwrap_err();

        assert!(err.to_string().contains("rc=-1"));
    }

    #[tokio::test]
    async fn mtp_delete_empty_path_is_rejected_before_backend_call() {
        let mock = Arc::new(MockMtpHandle::new());
        let backend = MtpBackend::new(Arc::clone(&mock) as Arc<dyn MtpHandle>);

        let err = backend.delete_file("").await.unwrap_err();

        assert!(err.to_string().contains("must name a file"));
        assert!(mock.call_log.lock().unwrap().is_empty());
    }
}
