use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceManifest {
    pub id: String,
    pub name: Option<String>,
}

#[derive(Debug)]
pub enum DeviceEvent {
    Detected {
        path: PathBuf,
        manifest: DeviceManifest,
    },
    #[allow(dead_code)]
    Removed(PathBuf),
}

pub struct DeviceProber;

impl DeviceProber {
    pub async fn probe(path: &Path) -> Result<Option<DeviceManifest>> {
        let manifest_path = path.join(".jellysync.json");
        // FIX: Use async metadata check instead of blocking std::fs::exists
        if tokio::fs::metadata(&manifest_path).await.is_err() {
            return Ok(None);
        }

        let content = tokio::fs::read_to_string(&manifest_path).await?;
        let manifest: DeviceManifest = serde_json::from_str(&content)?;
        Ok(Some(manifest))
    }
}

pub async fn run_observer(tx: tokio::sync::mpsc::Sender<DeviceEvent>) {
    println!("[Device] Observer thread started");
    // FIX: Run initial scan in blocking thread
    let mut last_mounts = tokio::task::spawn_blocking(get_mounts)
        .await
        .unwrap_or_default();

    println!("[Device] Initial mounts detected: {}", last_mounts.len());
    for m in &last_mounts {
        println!("[Device] Existing mount: {:?}", m);
    }

    let mut loop_count = 0;
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        loop_count += 1;

        if loop_count % 30 == 0 {
            println!("[Device] Heartbeat: Observer loop still running...");
        }

        // FIX: Run polling in blocking thread to avoid stalling async runtime
        let current_mounts = match tokio::task::spawn_blocking(get_mounts).await {
            Ok(mounts) => mounts,
            Err(e) => {
                eprintln!("[Device] Task join error: {}", e);
                continue;
            }
        };

        for mount in &current_mounts {
            if !last_mounts.contains(mount) {
                // New device detected!
                println!("[Device] New mount detected: {:?}", mount);

                // Probe for manifest
                match DeviceProber::probe(mount).await {
                    Ok(Some(manifest)) => {
                        println!("[Device] Found valid manifest for device: {}", manifest.id);
                        let _ = tx
                            .send(DeviceEvent::Detected {
                                path: mount.clone(),
                                manifest,
                            })
                            .await;
                    }
                    Ok(None) => {
                        println!("[Device] No .jellysync.json found on {:?}", mount);
                    }
                    Err(e) => {
                        eprintln!("[Device] Error probing {:?}: {}", mount, e);
                    }
                }
            }
        }

        last_mounts = current_mounts;
    }
}

#[cfg(windows)]
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
    // Basic Linux implementation: check common mount points
    let mut mounts = Vec::new();
    let paths = ["/media", "/run/media"];
    for base in paths {
        if let Ok(entries) = std::fs::read_dir(base) {
            for entry in entries.flatten() {
                // Media folder usually contains user folders which contain mounts
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
