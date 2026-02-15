use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::time::{sleep, Duration};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DeviceManifest {
    pub device_id: String,
    pub name: Option<String>,
    pub version: String,
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

    let wide_path: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
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
            mounts.push(entry.path());
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
                            mounts.push(sub_entry.path());
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
