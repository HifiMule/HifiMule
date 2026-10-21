#[cfg(test)]
tokio::task_local! { pub(crate) static TEST_DEVICE_PROFILES_PATH: PathBuf; }

use anyhow::{Result, anyhow};
use std::path::PathBuf;

#[cfg(test)]
static TEST_APP_DATA_DIR: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

#[cfg(test)]
pub fn set_test_app_data_dir(path: PathBuf) {
    *TEST_APP_DATA_DIR.lock().unwrap() = Some(path);
}

#[cfg(test)]
pub fn clear_test_app_data_dir() {
    *TEST_APP_DATA_DIR.lock().unwrap() = None;
}

pub fn get_app_data_dir() -> Result<PathBuf> {
    #[cfg(test)]
    if let Some(path) = TEST_APP_DATA_DIR.lock().unwrap().clone() {
        if !path.exists() {
            std::fs::create_dir_all(&path)
                .map_err(|e| anyhow!("Failed to create application data directory: {}", e))?;
        }
        return Ok(path);
    }

    let path = hifimule_lifecycle::resolve_app_data_dir()?;

    if !path.exists() {
        std::fs::create_dir_all(&path)
            .map_err(|e| anyhow!("Failed to create application data directory: {}", e))?;
    }

    Ok(path)
}

pub fn get_device_profiles_path() -> Result<PathBuf> {
    #[cfg(test)]
    if let Ok(path) = TEST_DEVICE_PROFILES_PATH.try_with(Clone::clone) { return Ok(path); }
    Ok(get_app_data_dir()?.join("device-profiles.json"))
}

/// Returns the local filesystem path for the cached MTP device manifest.
/// MTP writes are unreliable on some devices (e.g. Garmin); the local cache
/// is the authoritative manifest store and is preferred over the on-device copy.
pub fn get_local_mtp_manifest_path(device_id: &str) -> Result<PathBuf> {
    let dir = get_app_data_dir()?.join("manifests");
    std::fs::create_dir_all(&dir)
        .map_err(|e| anyhow!("Failed to create manifests directory: {}", e))?;
    Ok(dir.join(format!("{}.json.zst", device_id)))
}
