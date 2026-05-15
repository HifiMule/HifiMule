use anyhow::{anyhow, Result};
use std::path::PathBuf;

pub fn get_app_data_dir() -> Result<PathBuf> {
    if let Ok(override_path) = std::env::var("HIFIMULE_APP_DATA_DIR") {
        let path = PathBuf::from(override_path);
        if !path.exists() {
            std::fs::create_dir_all(&path)
                .map_err(|e| anyhow!("Failed to create application data directory: {}", e))?;
        }
        return Ok(path);
    }

    let mut path = PathBuf::new();

    #[cfg(target_os = "windows")]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            path.push(appdata);
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = std::env::var("HOME") {
            path.push(home);
            path.push("Library");
            path.push("Application Support");
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Ok(xdg_data) = std::env::var("XDG_DATA_HOME") {
            path.push(xdg_data);
        } else if let Ok(home) = std::env::var("HOME") {
            path.push(home);
            path.push(".local");
            path.push("share");
        }
    }

    // Fallback to current directory if no standard path is found
    if path.as_os_str().is_empty() {
        path = PathBuf::from(".");
    }

    path.push("HifiMule");

    if !path.exists() {
        std::fs::create_dir_all(&path)
            .map_err(|e| anyhow!("Failed to create application data directory: {}", e))?;
    }

    Ok(path)
}

pub fn get_device_profiles_path() -> Result<PathBuf> {
    Ok(get_app_data_dir()?.join("device-profiles.json"))
}

/// Returns the local filesystem path for the cached MTP device manifest.
/// MTP writes are unreliable on some devices (e.g. Garmin); the local cache
/// is the authoritative manifest store and is preferred over the on-device copy.
pub fn get_local_mtp_manifest_path(device_id: &str) -> Result<PathBuf> {
    let dir = get_app_data_dir()?.join("manifests");
    std::fs::create_dir_all(&dir)
        .map_err(|e| anyhow!("Failed to create manifests directory: {}", e))?;
    Ok(dir.join(format!("{}.json", device_id)))
}
