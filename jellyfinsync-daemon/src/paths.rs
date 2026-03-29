use anyhow::{anyhow, Result};
use std::path::PathBuf;

pub fn get_app_data_dir() -> Result<PathBuf> {
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

    path.push("JellyfinSync");

    if !path.exists() {
        std::fs::create_dir_all(&path)
            .map_err(|e| anyhow!("Failed to create application data directory: {}", e))?;
    }

    Ok(path)
}

pub fn get_device_profiles_path() -> Result<PathBuf> {
    Ok(get_app_data_dir()?.join("device-profiles.json"))
}
