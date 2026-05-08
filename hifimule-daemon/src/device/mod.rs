pub mod mtp;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::time::{sleep, Duration};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SyncedItem {
    pub jellyfin_id: String,
    pub name: String,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub artist: Option<String>,
    pub local_path: String,
    pub size_bytes: u64,
    pub synced_at: String,
    #[serde(default)]
    pub original_name: Option<String>,
    #[serde(default)]
    pub etag: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BasketItem {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub item_type: String,
    #[serde(default)]
    pub artist: Option<String>,
    pub child_count: u32,
    pub size_ticks: i64,
    pub size_bytes: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistManifestEntry {
    pub jellyfin_id: String,
    pub filename: String,
    pub track_count: u32,
    pub track_ids: Vec<String>, // ordered Jellyfin IDs — used for change detection
    pub last_modified: String,  // ISO 8601 timestamp
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DeviceManifest {
    pub device_id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    pub version: String,
    #[serde(default)]
    pub managed_paths: Vec<String>,
    #[serde(default)]
    pub synced_items: Vec<SyncedItem>,
    #[serde(default)]
    pub dirty: bool,
    #[serde(default)]
    pub pending_item_ids: Vec<String>,
    #[serde(default)]
    pub basket_items: Vec<BasketItem>,
    #[serde(default)]
    pub auto_sync_on_connect: bool,
    /// Auto-fill preferences persisted per device.
    #[serde(default)]
    pub auto_fill: AutoFillPrefs,
    /// ID referencing an entry in device-profiles.json. None = no transcoding (passthrough).
    #[serde(default)]
    pub transcoding_profile_id: Option<String>,
    #[serde(default)]
    pub playlists: Vec<PlaylistManifestEntry>,
    /// WPD storage object ID for MTP devices. When set, WPD calls skip first-child
    /// enumeration under DEVICE and use this ID directly. Backward-compatible via serde(default).
    #[serde(default)]
    pub storage_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct AutoFillPrefs {
    pub enabled: bool,
    pub max_bytes: Option<u64>,
}

/// Atomically writes a DeviceManifest via the device's IO backend.
pub async fn write_manifest(
    device_io: std::sync::Arc<dyn crate::device_io::DeviceIO>,
    manifest: &DeviceManifest,
) -> Result<()> {
    let json = serde_json::to_string_pretty(manifest)?;
    device_io
        .write_with_verify(".hifimule.json", json.as_bytes())
        .await
}

#[derive(Debug, Clone, PartialEq)]
pub enum DeviceClass {
    Msc,
    Mtp,
}

fn device_class_from_path(path: &Path) -> DeviceClass {
    if path.to_string_lossy().starts_with("mtp://") {
        DeviceClass::Mtp
    } else {
        DeviceClass::Msc
    }
}

/// Bundles the in-memory manifest with its IO backend for a connected device.
pub struct ConnectedDevice {
    pub manifest: DeviceManifest,
    pub device_io: std::sync::Arc<dyn crate::device_io::DeviceIO>,
    pub device_class: DeviceClass,
}

pub struct UnrecognizedDeviceState {
    pub path: PathBuf,
    pub io: std::sync::Arc<dyn crate::device_io::DeviceIO>,
    pub friendly_name: Option<String>,
}

struct DeviceManagerState {
    connected_devices: std::collections::HashMap<PathBuf, ConnectedDevice>,
    selected_device_path: Option<PathBuf>,
}

/// Scans the specified managed paths recursively for leftover `.tmp`
/// files from interrupted writes and deletes them. Returns the count of deleted files.
/// Non-fatal: individual deletion failures are silently skipped.
pub async fn cleanup_tmp_files(
    device_io: std::sync::Arc<dyn crate::device_io::DeviceIO>,
    managed_paths: &[String],
) -> Result<usize> {
    let mut count = 0;
    // Sweep device root ("") plus all managed paths. managed_paths never contains "" because
    // initialize_device validates folder_path is non-empty before adding it.
    let paths: Vec<&str> = std::iter::once("")
        .chain(managed_paths.iter().map(|s| s.as_str()))
        .collect();
    for path_str in paths {
        let entries = match device_io.list_files(path_str).await {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries {
            if entry.name.ends_with(".tmp") {
                if device_io.delete_file(&entry.path).await.is_ok() {
                    count += 1;
                }
            }
        }
    }
    Ok(count)
}

#[derive(Debug, Clone)]
pub enum DeviceEvent {
    Detected {
        path: PathBuf,
        manifest: DeviceManifest,
        device_io: std::sync::Arc<dyn crate::device_io::DeviceIO>,
    },
    Removed(PathBuf),
    Unrecognized {
        path: PathBuf,
        device_io: std::sync::Arc<dyn crate::device_io::DeviceIO>,
        friendly_name: Option<String>,
    },
}

pub struct DeviceProber;

impl DeviceProber {
    pub async fn probe(path: &Path) -> Result<Option<DeviceManifest>> {
        let manifest_path = path.join(".hifimule.json");
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
    /// Connected-device map and selection are guarded together to avoid torn reads and lock inversion.
    state: std::sync::Arc<tokio::sync::RwLock<DeviceManagerState>>,
    /// Pending initialization state is stored as one coherent snapshot.
    unrecognized_device: std::sync::Arc<tokio::sync::RwLock<Option<UnrecognizedDeviceState>>>,
}

impl DeviceManager {
    pub fn new(db: std::sync::Arc<crate::db::Database>) -> Self {
        Self {
            db,
            state: std::sync::Arc::new(tokio::sync::RwLock::new(DeviceManagerState {
                connected_devices: std::collections::HashMap::new(),
                selected_device_path: None,
            })),
            unrecognized_device: std::sync::Arc::new(tokio::sync::RwLock::new(None)),
        }
    }

    pub async fn handle_device_detected(
        &self,
        path: PathBuf,
        manifest: DeviceManifest,
        device_io: std::sync::Arc<dyn crate::device_io::DeviceIO>,
    ) -> Result<crate::DaemonState> {
        {
            let state = self.state.read().await;
            if state.connected_devices.contains_key(&path) {
                return Ok(crate::DaemonState::Idle);
            }
        }
        let device_class = device_class_from_path(&path);

        // T5.9: Scan for MTP-style dirty markers on reconnect.
        // For MSC devices, leftover `.dirty` markers indicate an interrupted MtpBackend write
        // from a previous session (e.g., the device was used with MTP before).
        if let Ok(files) = device_io.list_files("").await {
            if files.iter().any(|f| f.name.ends_with(".dirty")) {
                daemon_log!(
                    "[Device] Dirty marker detected on reconnect at {:?} — firing on_device_dirty",
                    path
                );
                // The dirty flag in the manifest is the on_device_dirty signal path (same as MSC).
                // We surface it via the manifest.dirty field; the RPC dirty-resume handler handles cleanup.
                let mut dirty_manifest = manifest.clone();
                dirty_manifest.dirty = true;
                let connected = ConnectedDevice {
                    manifest: dirty_manifest.clone(),
                    device_io: std::sync::Arc::clone(&device_io),
                    device_class: device_class.clone(),
                };
                {
                    let mut state = self.state.write().await;
                    state.connected_devices.insert(path.clone(), connected);
                    if state.selected_device_path.is_none() {
                        state.selected_device_path = Some(path.clone());
                    }
                }
                {
                    *self.unrecognized_device.write().await = None;
                }
                let name = dirty_manifest
                    .name
                    .clone()
                    .unwrap_or_else(|| dirty_manifest.device_id.clone());
                let mapping = self
                    .db
                    .get_device_mapping(&dirty_manifest.device_id)
                    .unwrap_or(None);
                return if let Some(m) = mapping {
                    if let Some(profile_id) = m.jellyfin_user_id {
                        Ok(crate::DaemonState::DeviceRecognized { name, profile_id })
                    } else {
                        Ok(crate::DaemonState::DeviceFound(name))
                    }
                } else {
                    Ok(crate::DaemonState::DeviceFound(name))
                };
            }
        }

        let connected = ConnectedDevice {
            manifest: manifest.clone(),
            device_io,
            device_class,
        };
        {
            let mut state = self.state.write().await;
            state.connected_devices.insert(path.clone(), connected);
            if state.selected_device_path.is_none() {
                // Auto-select first/only device
                state.selected_device_path = Some(path.clone());
            }
            // If another device is already selected, don't change selection
        }
        {
            *self.unrecognized_device.write().await = None;
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

    pub async fn handle_device_unrecognized(
        &self,
        path: PathBuf,
        device_io: std::sync::Arc<dyn crate::device_io::DeviceIO>,
        friendly_name: Option<String>,
    ) -> crate::DaemonState {
        let path_str = path.to_string_lossy().to_string();
        // Unrecognized devices have no manifest — ensure they are not in connected_devices.
        // Do NOT change selected_device_path since other recognized devices may still be connected.
        // Both operations are performed under state.write() so no concurrent handle_device_detected
        // can re-insert the path between the removal and the pending-state set (which would leave
        // two live IO backends for the same physical device).
        {
            let mut state = self.state.write().await;
            state.connected_devices.remove(&path);
            let mut unrecognized = self.unrecognized_device.write().await;
            if unrecognized.is_some() {
                daemon_log!(
                    "[Device] Warning: overwriting pending unrecognized device {:?} with {:?}",
                    unrecognized.as_ref().map(|state| &state.path),
                    path
                );
            }
            *unrecognized = Some(UnrecognizedDeviceState {
                path,
                io: device_io,
                friendly_name,
            });
        }
        crate::DaemonState::DeviceFound(path_str)
    }

    pub async fn get_unrecognized_device_snapshot(&self) -> Option<UnrecognizedDeviceState> {
        self.unrecognized_device
            .read()
            .await
            .as_ref()
            .map(|state| UnrecognizedDeviceState {
                path: state.path.clone(),
                io: std::sync::Arc::clone(&state.io),
                friendly_name: state.friendly_name.clone(),
            })
    }

    #[cfg(test)]
    pub async fn get_unrecognized_device_path(&self) -> Option<PathBuf> {
        self.get_unrecognized_device_snapshot()
            .await
            .map(|state| state.path)
    }

    pub async fn get_unrecognized_device_io(
        &self,
    ) -> Option<std::sync::Arc<dyn crate::device_io::DeviceIO>> {
        self.get_unrecognized_device_snapshot()
            .await
            .map(|state| state.io)
    }

    pub async fn handle_device_removed(&self, removed_path: &PathBuf) {
        {
            let mut state = self.state.write().await;
            state.connected_devices.remove(removed_path);
            if state.selected_device_path.as_ref() == Some(removed_path) {
                state.selected_device_path = state.connected_devices.keys().next().cloned();
            }
        }
        {
            let mut unrecognized = self.unrecognized_device.write().await;
            // Only clear if the removed path matches; an unrelated managed device disconnecting
            // must not erase a pending initialization for a different unrecognized device.
            if unrecognized
                .as_ref()
                .is_some_and(|state| &state.path == removed_path)
            {
                *unrecognized = None;
            }
        }
    }

    pub async fn get_current_device(&self) -> Option<DeviceManifest> {
        let state = self.state.read().await;
        let path = state.selected_device_path.as_ref()?;
        state
            .connected_devices
            .get(path)
            .map(|d| d.manifest.clone())
    }

    /// Returns the IO backend for the currently selected device.
    pub async fn get_device_io(&self) -> Option<std::sync::Arc<dyn crate::device_io::DeviceIO>> {
        let state = self.state.read().await;
        let path = state.selected_device_path.as_ref()?;
        state
            .connected_devices
            .get(path)
            .map(|d| std::sync::Arc::clone(&d.device_io))
    }

    /// Atomically returns the manifest and IO backend for the currently selected device.
    /// Prefer this over separate `get_current_device` + `get_device_io` calls to avoid
    /// TOCTOU races when the device disconnects between the two reads.
    pub async fn get_manifest_and_io(
        &self,
    ) -> Option<(
        DeviceManifest,
        std::sync::Arc<dyn crate::device_io::DeviceIO>,
    )> {
        let state = self.state.read().await;
        let path = state.selected_device_path.as_ref()?;
        state
            .connected_devices
            .get(path)
            .map(|d| (d.manifest.clone(), std::sync::Arc::clone(&d.device_io)))
    }

    pub async fn get_current_device_path(&self) -> Option<PathBuf> {
        self.state.read().await.selected_device_path.clone()
    }

    /// Returns a snapshot of all currently connected managed devices.
    pub async fn get_connected_devices(&self) -> Vec<(PathBuf, DeviceManifest, DeviceClass)> {
        self.state
            .read()
            .await
            .connected_devices
            .iter()
            .map(|(p, d)| (p.clone(), d.manifest.clone(), d.device_class.clone()))
            .collect()
    }

    /// Returns a consistent snapshot of connected devices and selected path in a single
    /// atomic read, preventing torn reads in get_daemon_state.
    pub async fn get_multi_device_snapshot(
        &self,
    ) -> (Vec<(PathBuf, DeviceManifest, DeviceClass)>, Option<PathBuf>) {
        let state = self.state.read().await;
        let device_list = state
            .connected_devices
            .iter()
            .map(|(p, d)| (p.clone(), d.manifest.clone(), d.device_class.clone()))
            .collect();
        (device_list, state.selected_device_path.clone())
    }

    /// Sets the selected device path. Returns false if path is not in connected_devices.
    pub async fn select_device(&self, path: PathBuf) -> bool {
        let mut state = self.state.write().await;
        if !state.connected_devices.contains_key(&path) {
            return false;
        }
        state.selected_device_path = Some(path);
        true
    }

    /// Atomically updates both the in-memory manifest and the on-disk file.
    /// Used during sync operations to prevent read-modify-write race conditions.
    /// Returns Err("No device connected") when no device is present — callers must
    /// handle this case rather than silently discarding writes.
    pub async fn update_manifest<F>(&self, mutation: F) -> Result<()>
    where
        F: FnOnce(&mut DeviceManifest),
    {
        let mut state = self.state.write().await;
        let path = state
            .selected_device_path
            .clone()
            .ok_or_else(|| anyhow::anyhow!("No device connected"))?;
        let connected = state
            .connected_devices
            .get_mut(&path)
            .ok_or_else(|| anyhow::anyhow!("Selected device not in connected map"))?;
        mutation(&mut connected.manifest);
        // Clone Arc and manifest so we can drop the write guard before the async I/O.
        // Serialization is still guaranteed: the in-memory state is mutated under the lock;
        // the snapshot written to disk is always consistent with the in-memory state.
        let device_io = std::sync::Arc::clone(&connected.device_io);
        let manifest_snapshot = connected.manifest.clone();
        drop(state);
        crate::device::write_manifest(device_io, &manifest_snapshot).await?;
        Ok(())
    }

    pub async fn get_device_storage(&self) -> Option<StorageInfo> {
        let (path, device_io) = {
            let state = self.state.read().await;
            let path = state.selected_device_path.clone()?;
            let device_io = state
                .connected_devices
                .get(&path)
                .map(|d| std::sync::Arc::clone(&d.device_io))?;
            (path, device_io)
        };
        if let Some(info) = get_storage_info(&path) {
            return Some(info);
        }

        let free_bytes = device_io.free_space().await.ok()?;
        Some(StorageInfo {
            total_bytes: free_bytes,
            free_bytes,
            used_bytes: 0,
            device_path: path.to_string_lossy().to_string(),
        })
    }

    /// Initializes a new device by generating a UUID, writing the initial manifest,
    /// and transitioning the device from unrecognized to recognized state.
    pub async fn initialize_device(
        &self,
        folder_path: &str,
        transcoding_profile_id: Option<String>,
        name: String,
        icon: Option<String>,
        _device_io: std::sync::Arc<dyn crate::device_io::DeviceIO>,
    ) -> Result<DeviceManifest> {
        // Validate folder_path: no traversal, no absolute paths; multi-level paths (e.g.
        // "Music/HifiMule") are allowed — both MscBackend (create_dir_all) and MtpBackend
        // (auto-creates parent objects) handle them transparently.
        if !folder_path.is_empty() {
            let components: Vec<&str> = folder_path.split(&['/', '\\']).collect();
            if components.iter().any(|c| *c == "..") {
                return Err(anyhow::anyhow!(
                    "Invalid folder path: path traversal ('..') not allowed"
                ));
            }
            if folder_path.starts_with('/') || folder_path.starts_with('\\') {
                return Err(anyhow::anyhow!(
                    "Invalid folder path: absolute paths not allowed"
                ));
            }
        }

        let pending = self
            .get_unrecognized_device_snapshot()
            .await
            .ok_or_else(|| anyhow::anyhow!("No unrecognized device connected"))?;
        let device_root = pending.path;
        let device_io = pending.io;

        // Liveness probe: detect stale IO from a device that disconnected and reconnected
        // between the Unrecognized event and the user completing initialization.
        if device_io.list_files("").await.is_err() {
            return Err(anyhow::anyhow!(
                "Device no longer accessible — reconnect the device and try again"
            ));
        }

        let device_id = uuid::Uuid::new_v4().to_string();

        let managed_paths = if folder_path.is_empty() {
            vec![]
        } else {
            device_io.ensure_dir(folder_path).await?;
            vec![folder_path.to_string()]
        };

        let storage_id = device_io.storage_id().await.unwrap_or(None);

        let manifest = DeviceManifest {
            device_id,
            name: Some(name).filter(|s| !s.is_empty()),
            icon,
            version: "1.0".to_string(),
            managed_paths,
            synced_items: vec![],
            dirty: false,
            pending_item_ids: vec![],
            basket_items: vec![],
            auto_sync_on_connect: false,
            auto_fill: AutoFillPrefs::default(),
            transcoding_profile_id, // stored in .hifimule.json; read back on sync to apply transcoding
            playlists: vec![],
            storage_id,
        };
        let manifest_bytes = serde_json::to_string_pretty(&manifest)?;
        device_io
            .write_with_verify(".hifimule.json", manifest_bytes.as_bytes())
            .await?;

        {
            let mut state = self.state.write().await;
            state.connected_devices.insert(
                device_root.clone(),
                ConnectedDevice {
                    manifest: manifest.clone(),
                    device_class: device_class_from_path(&device_root),
                    device_io,
                },
            );
            // Only auto-select if no device is currently selected; don't steal selection
            // from a device the user has already chosen in a multi-device session.
            if state.selected_device_path.is_none() {
                state.selected_device_path = Some(device_root.clone());
            }
        }
        {
            // Only clear the pending slot if it still holds this device's path.
            // A concurrent remove + new unrecognized probe may have replaced it with a
            // different device between the snapshot and here; clearing unconditionally
            // would silently lose that device's pending state.
            let mut unrecognized = self.unrecognized_device.write().await;
            if unrecognized.as_ref().is_some_and(|s| s.path == device_root) {
                *unrecognized = None;
            }
        }

        Ok(manifest)
    }

    pub async fn list_root_folders(&self) -> Result<Option<DeviceRootFoldersResponse>> {
        let selected_snapshot = {
            let state = self.state.read().await;
            state.selected_device_path.clone().map(|path| {
                let manifest = state
                    .connected_devices
                    .get(&path)
                    .map(|device| device.manifest.clone());
                (path, manifest)
            })
        };
        let (device_path, manifest, pending_friendly_name) =
            if let Some((path, manifest)) = selected_snapshot {
                (path, manifest, None)
            } else if let Some(pending) = self.get_unrecognized_device_snapshot().await {
                (pending.path, None, pending.friendly_name)
            } else {
                return Ok(None);
            };
        let has_manifest = manifest.is_some();
        // If manifest doesn't exist, we treat no folders as managed (empty vec)
        let managed_paths = manifest
            .as_ref()
            .map(|m| m.managed_paths.clone())
            .unwrap_or_default();

        // MTP devices use a synthetic path that cannot be traversed via std::fs.
        // Return a response built from the manifest (or empty for unrecognized) so the UI
        // can show the managed folder list or the "Initialize" banner without filesystem access.
        // unmanaged_count is always 0: MTP cannot enumerate real device folders.
        if device_path
            .to_string_lossy()
            .to_lowercase()
            .starts_with("mtp://")
        {
            // Read the stored friendly name before composing device_name.
            // Only use it for unrecognized devices; recognized devices have their name in the manifest.
            let device_name = manifest
                .as_ref()
                .and_then(|m| m.name.clone())
                .or(pending_friendly_name)
                .unwrap_or_else(|| "MTP Device".to_string());
            // For MTP, name == relative_path because managed_paths stores relative folder names
            // directly (no filesystem enumeration is possible to derive a display name separately).
            let folders: Vec<DeviceFolderInfo> = managed_paths
                .iter()
                .map(|p| DeviceFolderInfo {
                    name: p.clone(),
                    relative_path: p.clone(),
                    is_managed: true,
                })
                .collect();
            let managed_count = folders.len();
            return Ok(Some(DeviceRootFoldersResponse {
                device_name,
                device_path: device_path.to_string_lossy().to_string(),
                has_manifest,
                folders,
                managed_count,
                unmanaged_count: 0,
            }));
        }

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

        let device_name = manifest.and_then(|m| m.name).unwrap_or_else(|| {
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

    /// Scans managed paths on device and compares against manifest to find discrepancies.
    /// Returns lists of missing files (in manifest but not on disk) and orphaned files
    /// (on disk but not tracked in manifest).
    pub async fn get_discrepancies(&self) -> Result<Option<ManifestDiscrepancies>> {
        let device_path = match self.get_current_device_path().await {
            Some(p) => p,
            None => return Ok(None),
        };

        let manifest = match self.get_current_device().await {
            Some(m) => m,
            None => return Ok(None),
        };

        // Collect all actual files on disk within managed paths
        let mut on_disk_files: std::collections::HashSet<String> = std::collections::HashSet::new();
        for managed_path in &manifest.managed_paths {
            let full_path = device_path.join(managed_path);
            if let Ok(meta) = tokio::fs::symlink_metadata(&full_path).await {
                if !meta.is_dir() {
                    continue;
                }
            } else {
                continue;
            }

            let mut dirs_to_visit = vec![full_path];
            while let Some(dir) = dirs_to_visit.pop() {
                let mut entries = match tokio::fs::read_dir(&dir).await {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                while let Some(entry) = entries.next_entry().await.unwrap_or(None) {
                    let path = entry.path();
                    let file_type = match entry.file_type().await {
                        Ok(ft) => ft,
                        Err(_) => continue,
                    };

                    if file_type.is_symlink() {
                        continue;
                    } else if file_type.is_dir() {
                        dirs_to_visit.push(path);
                    } else if file_type.is_file() {
                        let file_name = path.file_name().unwrap_or_default().to_string_lossy();
                        // Skip hidden files and temp files
                        if file_name.starts_with('.') || file_name.ends_with(".tmp") {
                            continue;
                        }
                        // Store as relative path from device root using forward slashes
                        if let Ok(rel) = path.strip_prefix(&device_path) {
                            let rel_str = rel.to_string_lossy().replace('\\', "/");
                            on_disk_files.insert(rel_str);
                        }
                    }
                }
            }
        }

        // Build set of manifest paths
        let manifest_paths: std::collections::HashSet<&str> = manifest
            .synced_items
            .iter()
            .map(|item| item.local_path.as_str())
            .collect();

        // Missing: in manifest but not on disk
        let missing: Vec<DiscrepancyItem> = manifest
            .synced_items
            .iter()
            .filter(|item| !on_disk_files.contains(&item.local_path))
            .map(|item| DiscrepancyItem {
                jellyfin_id: item.jellyfin_id.clone(),
                name: item.name.clone(),
                local_path: item.local_path.clone(),
                album: item.album.clone(),
                artist: item.artist.clone(),
            })
            .collect();

        // Orphaned: on disk but not in manifest
        let orphaned: Vec<DiscrepancyItem> = on_disk_files
            .iter()
            .filter(|path| !manifest_paths.contains(path.as_str()))
            .map(|path| {
                let file_name = Path::new(path)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                DiscrepancyItem {
                    jellyfin_id: String::new(),
                    name: file_name,
                    local_path: path.clone(),
                    album: None,
                    artist: None,
                }
            })
            .collect();

        Ok(Some(ManifestDiscrepancies { missing, orphaned }))
    }

    /// Removes items from the manifest by their Jellyfin IDs using atomic write.
    pub async fn prune_items(&self, item_ids: &[String]) -> Result<usize> {
        let id_set: std::collections::HashSet<&str> = item_ids.iter().map(|s| s.as_str()).collect();
        let mut removed = 0usize;
        self.update_manifest(|manifest| {
            let before = manifest.synced_items.len();
            manifest
                .synced_items
                .retain(|item| !id_set.contains(item.jellyfin_id.as_str()));
            removed = before - manifest.synced_items.len();
        })
        .await?;
        Ok(removed)
    }

    /// Re-links an orphaned file on disk to a missing manifest entry by updating
    /// the manifest item's local_path (and optionally original_name) to match the
    /// actual file on disk.
    pub async fn relink_item(&self, jellyfin_id: &str, new_local_path: &str) -> Result<bool> {
        if new_local_path.contains("..")
            || new_local_path.starts_with('/')
            || new_local_path.starts_with('\\')
        {
            return Err(anyhow::anyhow!("Invalid path: path traversal detected"));
        }

        let device_path = self
            .get_current_device_path()
            .await
            .ok_or_else(|| anyhow::anyhow!("No device connected"))?;
        let full_path = device_path.join(new_local_path);
        if !tokio::fs::try_exists(&full_path).await.unwrap_or(false) {
            return Err(anyhow::anyhow!("File does not exist: {}", new_local_path));
        }

        let mut found = false;
        self.update_manifest(|manifest| {
            if let Some(item) = manifest
                .synced_items
                .iter_mut()
                .find(|i| i.jellyfin_id == jellyfin_id)
            {
                // Store old path as original_name if not already set
                if item.original_name.is_none() {
                    item.original_name = Some(item.local_path.clone());
                }
                item.local_path = new_local_path.to_string();
                found = true;
            }
        })
        .await?;
        Ok(found)
    }

    /// Clears the dirty flag on the manifest if no discrepancies remain.
    pub async fn clear_dirty_flag(&self) -> Result<()> {
        let discrepancies = self
            .get_discrepancies()
            .await?
            .ok_or_else(|| anyhow::anyhow!("No device connected"))?;
        if !discrepancies.missing.is_empty() || !discrepancies.orphaned.is_empty() {
            return Err(anyhow::anyhow!(
                "Cannot clear dirty flag: discrepancies still exist"
            ));
        }

        self.update_manifest(|manifest| {
            manifest.dirty = false;
            manifest.pending_item_ids.clear();
        })
        .await
    }

    /// Saves the current basket selection to the device manifest
    pub async fn save_basket(&self, items: Vec<BasketItem>) -> Result<()> {
        self.update_manifest(|manifest| {
            manifest.basket_items = items;
        })
        .await
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

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ManifestDiscrepancies {
    pub missing: Vec<DiscrepancyItem>,
    pub orphaned: Vec<DiscrepancyItem>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DiscrepancyItem {
    pub jellyfin_id: String,
    pub name: String,
    pub local_path: String,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub artist: Option<String>,
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
    // (e.g., "Music"). Manifest might have "Music/HifiMule", but T2.1 says "enumerate top-level directories".
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

/// Public helper used by `MscBackend::free_space()`.
pub fn get_storage_info_free_bytes(path: &std::path::Path) -> anyhow::Result<u64> {
    get_storage_info(path)
        .map(|s| s.free_bytes)
        .ok_or_else(|| anyhow::anyhow!("Failed to query storage info for {}", path.display()))
}

#[cfg(target_os = "windows")]
fn is_removable_drive(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDriveTypeW;
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe { GetDriveTypeW(wide.as_ptr()) == 2 } // DRIVE_REMOVABLE
}

#[cfg(not(target_os = "windows"))]
fn is_removable_drive(_path: &Path) -> bool {
    true // macOS/Linux already filtered by mount detection
}

/// Returns true if a removable drive exists that corresponds to the same physical USB device
/// as the MTP device identified by `wpd_device_id`. Matching uses hardware instance IDs
/// (`SetupDiGetDeviceInstanceIdW`) to avoid false positives from two drives sharing a label.
/// Falls back to volume-label comparison if hardware-ID lookup fails for a given drive.
#[cfg(target_os = "windows")]
fn has_msc_drive_for_device(friendly_name: &str, wpd_device_id: &str) -> bool {
    use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
        SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiGetClassDevsW,
        SetupDiGetDeviceInstanceIdW, DIGCF_PRESENT, SP_DEVINFO_DATA,
    };
    use windows_sys::Win32::Storage::FileSystem::GetLogicalDrives;

    // Parse the USB instance-ID fragment from the WPD device path.
    // WPD ID format: "\\?\usb#vid_XXXX&pid_YYYY#SERIAL#{guid}"
    // We extract "vid_XXXX&pid_YYYY#SERIAL" and normalise to "USB\VID_XXXX&PID_YYYY\SERIAL".
    let wpd_usb_fragment: Option<String> = (|| {
        let lower = wpd_device_id.to_ascii_lowercase();
        // Find the "usb#" or "usb\\" prefix
        let start = lower.find("usb#").or_else(|| lower.find("usb\\"))?;
        let rest = &wpd_device_id[start + 4..]; // skip "usb#"
                                                // rest = "vid_XXXX&pid_YYYY#SERIAL#{guid}" — take up to the third '#' separator
        let parts: Vec<&str> = rest.splitn(3, '#').collect();
        if parts.len() < 2 {
            return None;
        }
        // Normalise: "USB\VID_XXXX&PID_YYYY\SERIAL" (case-insensitive comparison later)
        Some(format!(
            "USB\\{}\\{}",
            parts[0].to_ascii_uppercase(),
            parts[1].to_ascii_uppercase()
        ))
    })();

    // Enumerate all present disk-drive devices and collect their instance IDs.
    // GUID_DEVCLASS_DISKDRIVE = {4D36E967-E325-11CE-BFC1-08002BE10318}
    let disk_drive_class = windows_sys::core::GUID {
        data1: 0x4D36E967,
        data2: 0xE325,
        data3: 0x11CE,
        data4: [0xBF, 0xC1, 0x08, 0x00, 0x2B, 0xE1, 0x03, 0x18],
    };

    let disk_instance_ids: Vec<String> = unsafe {
        let devs = SetupDiGetClassDevsW(
            &disk_drive_class,
            std::ptr::null(),
            std::ptr::null_mut(),
            DIGCF_PRESENT,
        );
        if devs as isize == -1 {
            vec![]
        } else {
            let mut ids = Vec::new();
            let mut index = 0u32;
            loop {
                let mut devinfo: SP_DEVINFO_DATA = std::mem::zeroed();
                devinfo.cbSize = std::mem::size_of::<SP_DEVINFO_DATA>() as u32;
                if SetupDiEnumDeviceInfo(devs, index, &mut devinfo) == 0 {
                    break;
                }
                index += 1;
                let mut id_buf = vec![0u16; 512];
                let mut required = 0u32;
                if SetupDiGetDeviceInstanceIdW(
                    devs,
                    &devinfo,
                    id_buf.as_mut_ptr(),
                    id_buf.len() as u32,
                    &mut required,
                ) != 0
                {
                    let len = id_buf.iter().position(|&c| c == 0).unwrap_or(id_buf.len());
                    ids.push(String::from_utf16_lossy(&id_buf[..len]).to_string());
                }
            }
            SetupDiDestroyDeviceInfoList(devs);
            ids
        }
    };

    let drives = unsafe { GetLogicalDrives() };
    for i in 0..26u32 {
        if (drives >> i) & 1 == 0 {
            continue;
        }
        let letter = (b'A' + i as u8) as char;
        let path_str = format!("{}:\\", letter);
        let path = PathBuf::from(&path_str);
        if !is_removable_drive(&path) {
            continue;
        }

        // Try hardware-ID match first.
        if let Some(ref usb_frag) = wpd_usb_fragment {
            // Check if any disk-drive instance ID matches this USB fragment.
            let hw_matched = disk_instance_ids
                .iter()
                .any(|id| id.to_ascii_uppercase().contains(usb_frag.as_str()));
            if hw_matched {
                // Confirm this drive letter actually belongs to the matched disk.
                // We use the volume label as a secondary check only to bind the drive letter
                // to the already-matched hardware instance — not as the primary discriminator.
                // A mismatch here means the drive letters may be swapped; keep looking.
                let label = get_volume_label(&path_str);
                if label
                    .as_deref()
                    .map_or(false, |l| l.eq_ignore_ascii_case(friendly_name))
                {
                    return true;
                }
                // Hardware ID matched but label doesn't — still return true to avoid re-registering
                // the already-matched USB device (label may differ from friendly name).
                return true;
            }
            // Hardware lookup succeeded with no match — skip label fallback for this drive.
            if !disk_instance_ids.is_empty() {
                continue;
            }
        }

        // Fallback: label comparison when hardware ID lookup produced no results.
        if let Some(label) = get_volume_label(&path_str) {
            if label.eq_ignore_ascii_case(friendly_name) {
                return true;
            }
        }
    }
    false
}

#[cfg(target_os = "windows")]
fn get_volume_label(path_str: &str) -> Option<String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetVolumeInformationW;
    let wide: Vec<u16> = std::ffi::OsStr::new(path_str)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut label_buf = [0u16; 256];
    let ok = unsafe {
        GetVolumeInformationW(
            wide.as_ptr(),
            label_buf.as_mut_ptr(),
            label_buf.len() as u32,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
        ) != 0
    };
    if !ok {
        return None;
    }
    let len = label_buf
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(label_buf.len());
    Some(String::from_utf16_lossy(&label_buf[..len]).to_string())
}

#[cfg(not(target_os = "windows"))]
fn has_msc_drive_for_device(_friendly_name: &str, _wpd_device_id: &str) -> bool {
    false
}

pub async fn run_observer(tx: tokio::sync::mpsc::Sender<DeviceEvent>) {
    println!("[Device] Observer thread started");
    let mut known_mounts = std::collections::HashSet::new();

    loop {
        let current_mounts = get_mounts();

        // Filter stale observer state through the current safe mount list each cycle.
        // This evicts boot/system volumes captured by older binaries before new detection.
        known_mounts.retain(|mount| {
            if !current_mounts.contains(mount) {
                let _ = tx.try_send(DeviceEvent::Removed(mount.clone()));
                false
            } else {
                true
            }
        });

        // Detect new mounts
        for mount in &current_mounts {
            if !known_mounts.contains(mount) {
                known_mounts.insert(mount.clone());
                match DeviceProber::probe(mount).await {
                    Ok(Some(manifest)) => {
                        let device_io: std::sync::Arc<dyn crate::device_io::DeviceIO> =
                            std::sync::Arc::new(crate::device_io::MscBackend::new(mount.clone()));
                        let _ = tx
                            .send(DeviceEvent::Detected {
                                path: mount.clone(),
                                manifest,
                                device_io,
                            })
                            .await;
                    }
                    Ok(None) => {
                        if is_removable_drive(mount) {
                            // Detection layer creates the IO backend so the type (MSC/MTP)
                            // is determined here once, not re-derived downstream.
                            let device_io: std::sync::Arc<dyn crate::device_io::DeviceIO> =
                                std::sync::Arc::new(crate::device_io::MscBackend::new(
                                    mount.clone(),
                                ));
                            let _ = tx
                                .send(DeviceEvent::Unrecognized {
                                    path: mount.clone(),
                                    device_io,
                                    friendly_name: None,
                                })
                                .await;
                        }
                    }
                    Err(_) => {} // Probe failed (e.g., permission error) — ignore
                }
            }
        }

        sleep(Duration::from_secs(2)).await;
    }
}

pub async fn run_mtp_observer(tx: tokio::sync::mpsc::Sender<DeviceEvent>) {
    let mut known_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

    loop {
        let devices = tokio::task::spawn_blocking(mtp::enumerate_mtp_devices)
            .await
            .unwrap_or_default();

        for dev in &devices {
            if !known_ids.contains(&dev.device_id) {
                // Prefer MSC over MTP: if the device is also mounted as a drive
                // letter, skip it here and let run_observer handle it.
                if has_msc_drive_for_device(&dev.friendly_name, &dev.device_id) {
                    continue;
                }
                let synthetic_path = PathBuf::from(format!("mtp://{}", dev.device_id));
                let dev_clone = dev.clone();
                let dev_for_probe = dev.clone();
                let dev_id = dev.device_id.clone();
                let friendly_name = dev.friendly_name.clone();

                let backend =
                    tokio::task::spawn_blocking(move || mtp::create_mtp_backend(&dev_clone, None))
                        .await
                        .unwrap_or_else(|e| Err(anyhow::anyhow!("spawn_blocking panicked: {}", e)));

                match backend {
                    Ok(backend) => {
                        let backend_arc: std::sync::Arc<dyn crate::device_io::DeviceIO> =
                            std::sync::Arc::new(backend);
                        if emit_mtp_probe_event(
                            &tx,
                            synthetic_path,
                            &dev_id,
                            dev_for_probe,
                            friendly_name,
                            backend_arc,
                        )
                        .await
                        {
                            known_ids.insert(dev_id.clone());
                        }
                    }
                    Err(e) => {
                        daemon_log!("[MTP] Failed to open device {}: {}", dev_id, e);
                    }
                }
            }
        }

        let disconnected: Vec<String> = known_ids
            .iter()
            .filter(|id| !devices.iter().any(|d| &d.device_id == *id))
            .cloned()
            .collect();
        for id in &disconnected {
            let synthetic_path = PathBuf::from(format!("mtp://{}", id));
            let _ = tx.send(DeviceEvent::Removed(synthetic_path)).await;
            known_ids.remove(id);
        }

        sleep(Duration::from_secs(2)).await;
    }
}

fn is_missing_manifest_error(error: &anyhow::Error) -> bool {
    if error
        .downcast_ref::<std::io::Error>()
        .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound)
    {
        return true;
    }

    let message = error.to_string().to_ascii_lowercase();
    message.contains(".hifimule.json")
        && (message.contains("not found") || message.contains("no such file"))
}

async fn emit_mtp_probe_event(
    tx: &tokio::sync::mpsc::Sender<DeviceEvent>,
    synthetic_path: PathBuf,
    dev_id: &str,
    dev_info: mtp::MtpDeviceInfo,
    friendly_name: String,
    backend_arc: std::sync::Arc<dyn crate::device_io::DeviceIO>,
) -> bool {
    match backend_arc.read_file(".hifimule.json").await {
        Ok(data) => match serde_json::from_slice::<DeviceManifest>(&data) {
            Ok(manifest) => {
                // If the manifest carries a cached storage_id, open a second backend that
                // uses it directly — this makes free_space() and path lookups skip the
                // first-child DEVICE enumeration on every call.
                let final_io: std::sync::Arc<dyn crate::device_io::DeviceIO> =
                    if let Some(storage_id) = manifest.storage_id.clone() {
                        match tokio::task::spawn_blocking(move || {
                            mtp::create_mtp_backend(&dev_info, Some(storage_id))
                        })
                        .await
                        {
                            Ok(Ok(storage_backend)) => std::sync::Arc::new(storage_backend),
                            _ => backend_arc,
                        }
                    } else {
                        backend_arc
                    };
                tx.send(DeviceEvent::Detected {
                    path: synthetic_path,
                    manifest,
                    device_io: final_io,
                })
                .await
                .is_ok()
            }
            Err(e) => {
                daemon_log!("[MTP] Manifest parse error on {}: {}", dev_id, e);
                tx.send(DeviceEvent::Unrecognized {
                    path: synthetic_path,
                    device_io: backend_arc,
                    friendly_name: Some(friendly_name),
                })
                .await
                .is_ok()
            }
        },
        Err(e) => {
            daemon_log!("[MTP] Manifest read failed on {}: {}", dev_id, e);
            if is_missing_manifest_error(&e) {
                tx.send(DeviceEvent::Unrecognized {
                    path: synthetic_path,
                    device_io: backend_arc,
                    friendly_name: Some(friendly_name),
                })
                .await
                .is_ok()
            } else {
                false
            }
        }
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

#[cfg(any(target_os = "macos", test))]
fn is_boot_volume_device(candidate_dev: Option<u64>, root_dev: u64) -> bool {
    candidate_dev.map(|dev| dev == root_dev).unwrap_or(true)
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
    use std::os::unix::fs::MetadataExt;
    let mut mounts = Vec::new();
    // If we cannot stat / we cannot safely filter the boot volume; return empty.
    let Ok(root_meta) = std::fs::metadata("/") else {
        return mounts;
    };
    let root_dev = root_meta.dev();
    if let Ok(entries) = std::fs::read_dir("/Volumes") {
        for entry in entries.flatten() {
            let path = entry.path();
            // Skip the system boot volume. On macOS (including Apple Silicon with
            // APFS firmlinks) the boot volume in /Volumes shares the same device ID
            // as the root filesystem. Device-ID comparison is firmlink-safe;
            // canonicalize/realpath does not reliably follow APFS firmlinks.
            // On metadata error, skip the entry (fail-safe).
            let candidate_dev = std::fs::metadata(&path).ok().map(|m| m.dev());
            if is_boot_volume_device(candidate_dev, root_dev) {
                continue;
            }
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
