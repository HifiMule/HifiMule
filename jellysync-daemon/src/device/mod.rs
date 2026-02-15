use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::time::{sleep, Duration};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DeviceManifest {
    pub device_id: String,
    pub name: Option<String>,
    pub version: String,
    #[serde(default)]
    pub managed_paths: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum DeviceEvent {
    Detected {
        path: PathBuf,
        manifest: DeviceManifest,
    },
    Removed(PathBuf),
}

pub struct DeviceProber;

impl DeviceProber {
    pub async fn probe(path: &Path) -> Result<Option<DeviceManifest>> {
        let manifest_path = path.join(".jellysync.json");
        if tokio::fs::metadata(&manifest_path).await.is_err() {
            return Ok(None);
        }

        let content = tokio::fs::read_to_string(&manifest_path).await?;
        let manifest: DeviceManifest = serde_json::from_str(&content)?;
        Ok(Some(manifest))
    }
}

pub struct DeviceManager {
    db: std::sync::Arc<crate::db::Database>,
    current_device: std::sync::Arc<tokio::sync::RwLock<Option<DeviceManifest>>>,
    current_device_path: std::sync::Arc<tokio::sync::RwLock<Option<PathBuf>>>,
}

impl DeviceManager {
    pub fn new(db: std::sync::Arc<crate::db::Database>) -> Self {
        Self {
            db,
            current_device: std::sync::Arc::new(tokio::sync::RwLock::new(None)),
            current_device_path: std::sync::Arc::new(tokio::sync::RwLock::new(None)),
        }
    }

    pub async fn handle_device_detected(
        &self,
        path: PathBuf,
        manifest: DeviceManifest,
    ) -> Result<crate::DaemonState> {
        {
            let mut current = self.current_device.write().await;
            *current = Some(manifest.clone());
        }
        {
            let mut current_path = self.current_device_path.write().await;
            *current_path = Some(path);
        }

        let name = manifest
            .name
            .clone()
            .unwrap_or_else(|| manifest.device_id.clone());
        let mapping = self
            .db
            .get_device_mapping(&manifest.device_id)
            .unwrap_or(None);

        if let Some(m) = mapping {
            if let Some(profile_id) = m.jellyfin_user_id {
                Ok(crate::DaemonState::DeviceRecognized { name, profile_id })
            } else {
                Ok(crate::DaemonState::DeviceFound(name))
            }
        } else {
            Ok(crate::DaemonState::DeviceFound(name))
        }
    }

    pub async fn handle_device_removed(&self) {
        let mut current = self.current_device.write().await;
        *current = None;
        let mut current_path = self.current_device_path.write().await;
        *current_path = None;
    }

    pub async fn get_current_device(&self) -> Option<DeviceManifest> {
        self.current_device.read().await.clone()
    }

    pub async fn get_current_device_path(&self) -> Option<PathBuf> {
        self.current_device_path.read().await.clone()
    }

    pub async fn get_device_storage(&self) -> Option<StorageInfo> {
        let path = self.get_current_device_path().await?;
        get_storage_info(&path)
    }

    pub async fn list_root_folders(&self) -> Result<Option<DeviceRootFoldersResponse>> {
        let device_path = match self.get_current_device_path().await {
            Some(p) => p,
            None => return Ok(None),
        };

        let manifest = self.get_current_device().await;
        let has_manifest = manifest.is_some();
        // If manifest doesn't exist, we treat no folders as managed (empty vec)
        let managed_paths = manifest
            .as_ref()
            .map(|m| m.managed_paths.clone())
            .unwrap_or_default();

        let mut folders = Vec::new();
        let mut managed_count = 0;
        let mut unmanaged_count = 0;

        let mut entries = tokio::fs::read_dir(&device_path).await?;
        while let Some(entry) = entries.next_entry().await? {
            let file_type = entry.file_type().await?;
            if !file_type.is_dir() {
                continue;
            }

            let name = entry.file_name().to_string_lossy().to_string();

            // Skip hidden folders
            if name.starts_with('.') {
                continue;
            }

            // Skip system folders
            if is_system_folder(&name) {
                continue;
            }

            let is_managed = managed_paths.iter().any(|p| is_path_match(&name, p));

            if is_managed {
                managed_count += 1;
            } else {
                unmanaged_count += 1;
            }

            folders.push(DeviceFolderInfo {
                name: name.clone(),
                relative_path: name,
                is_managed,
            });
        }

        // Sort alphabetically
        folders.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

        let device_name = manifest
            .and_then(|m| m.name)
            .unwrap_or_else(|| {
                device_path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "Unknown Device".to_string())
            });

        Ok(Some(DeviceRootFoldersResponse {
            device_name,
            device_path: device_path.to_string_lossy().to_string(),
            has_manifest,
            folders,
            managed_count,
            unmanaged_count,
        }))
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DeviceRootFoldersResponse {
    pub device_name: String,
    pub device_path: String,
    pub has_manifest: bool,
    pub folders: Vec<DeviceFolderInfo>,
    pub managed_count: usize,
    pub unmanaged_count: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DeviceFolderInfo {
    pub name: String,
    pub relative_path: String,
    pub is_managed: bool,
}

fn is_system_folder(name: &str) -> bool {
    let system_folders = [
        "System Volume Information",
        "$RECYCLE.BIN",
        "RECYCLER",
        ".Spotlight-V100",
        ".fseventsd",
        ".Trashes",
        "lost+found",
    ];
    system_folders.iter().any(|&f| f.eq_ignore_ascii_case(name))
}

fn is_path_match(name: &str, managed_path: &str) -> bool {
    // For now, we only support top-level managed paths as specified in the story
    // (e.g., "Music"). Manifest might have "Music/JellyfinSync", but T2.1 says "enumerate top-level directories".
    // If a top-level directory is a parent of a managed path, should we mark it as managed?
    // Story 3.4 AC #3 says "When folders on the device match those paths".
    // Let's keep it simple: exact match of top-level name.
    #[cfg(target_os = "windows")]
    {
        name.eq_ignore_ascii_case(managed_path)
    }
    #[cfg(not(target_os = "windows"))]
    {
        name == managed_path
    }
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StorageInfo {
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub used_bytes: u64,
    pub device_path: String,
}

#[cfg(target_os = "windows")]
fn get_storage_info(path: &Path) -> Option<StorageInfo> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    let wide_path: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut free_bytes_available: u64 = 0;
    let mut total_bytes: u64 = 0;
    let mut total_free_bytes: u64 = 0;

    let success = unsafe {
        GetDiskFreeSpaceExW(
            wide_path.as_ptr(),
            &mut free_bytes_available as *mut u64,
            &mut total_bytes as *mut u64,
            &mut total_free_bytes as *mut u64,
        )
    };

    if success != 0 {
        Some(StorageInfo {
            total_bytes,
            free_bytes: total_free_bytes,
            used_bytes: total_bytes.saturating_sub(total_free_bytes),
            device_path: path.to_string_lossy().to_string(),
        })
    } else {
        None
    }
}

#[cfg(target_os = "macos")]
fn get_storage_info(path: &Path) -> Option<StorageInfo> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    let result = unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) };

    if result == 0 {
        let total = stat.f_blocks as u64 * stat.f_frsize as u64;
        let free = stat.f_bfree as u64 * stat.f_frsize as u64;
        Some(StorageInfo {
            total_bytes: total,
            free_bytes: free,
            used_bytes: total.saturating_sub(free),
            device_path: path.to_string_lossy().to_string(),
        })
    } else {
        None
    }
}

#[cfg(target_os = "linux")]
fn get_storage_info(path: &Path) -> Option<StorageInfo> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    let result = unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) };

    if result == 0 {
        let total = stat.f_blocks as u64 * stat.f_frsize as u64;
        let free = stat.f_bfree as u64 * stat.f_frsize as u64;
        Some(StorageInfo {
            total_bytes: total,
            free_bytes: free,
            used_bytes: total.saturating_sub(free),
            device_path: path.to_string_lossy().to_string(),
        })
    } else {
        None
    }
}

pub async fn run_observer(tx: tokio::sync::mpsc::Sender<DeviceEvent>) {
    println!("[Device] Observer thread started");
    let mut known_mounts = std::collections::HashSet::new();

    loop {
        let current_mounts = get_mounts();

        // Detect new mounts
        for mount in &current_mounts {
            if !known_mounts.contains(mount) {
                known_mounts.insert(mount.clone());
                if let Ok(Some(manifest)) = DeviceProber::probe(mount).await {
                    let _ = tx
                        .send(DeviceEvent::Detected {
                            path: mount.clone(),
                            manifest,
                        })
                        .await;
                }
            }
        }

        // Detect removed mounts
        known_mounts.retain(|mount| {
            if !current_mounts.contains(mount) {
                let _ = tx.try_send(DeviceEvent::Removed(mount.clone()));
                false
            } else {
                true
            }
        });

        sleep(Duration::from_secs(2)).await;
    }
}

/// Checks if a path is an actual mount point by comparing its filesystem
/// device ID with its parent's. A mounted filesystem will have a different
/// device ID than the directory it's mounted on.
#[cfg(unix)]
fn is_mount_point(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    if let (Some(parent), Ok(path_meta)) = (path.parent(), std::fs::metadata(path)) {
        if let Ok(parent_meta) = std::fs::metadata(parent) {
            return parent_meta.dev() != path_meta.dev();
        }
    }
    false
}

#[cfg(target_os = "windows")]
fn get_mounts() -> Vec<PathBuf> {
    use windows_sys::Win32::Storage::FileSystem::GetLogicalDrives;
    let mut mounts = Vec::new();
    let drives = unsafe { GetLogicalDrives() };
    for i in 0..26 {
        if (drives >> i) & 1 == 1 {
            let drive_letter = (b'A' + i) as char;
            mounts.push(PathBuf::from(format!("{}:\\", drive_letter)));
        }
    }
    mounts
}

#[cfg(target_os = "macos")]
fn get_mounts() -> Vec<PathBuf> {
    let mut mounts = Vec::new();
    if let Ok(entries) = std::fs::read_dir("/Volumes") {
        for entry in entries.flatten() {
            let path = entry.path();
            if is_mount_point(&path) {
                mounts.push(path);
            }
        }
    }
    mounts
}

#[cfg(target_os = "linux")]
fn get_mounts() -> Vec<PathBuf> {
    let mut mounts = Vec::new();
    let paths = ["/media", "/run/media"];
    for base in paths {
        if let Ok(entries) = std::fs::read_dir(base) {
            for entry in entries.flatten() {
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    if let Ok(sub_entries) = std::fs::read_dir(entry.path()) {
                        for sub_entry in sub_entries.flatten() {
                            let path = sub_entry.path();
                            if is_mount_point(&path) {
                                mounts.push(path);
                            }
                        }
                    }
                }
            }
        }
    }
    mounts
}

#[cfg(test)]
mod tests;
