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
    pub track_ids: Vec<String>,  // ordered Jellyfin IDs — used for change detection
    pub last_modified: String,   // ISO 8601 timestamp
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
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct AutoFillPrefs {
    pub enabled: bool,
    pub max_bytes: Option<u64>,
}

/// Atomically writes a DeviceManifest to disk using Write-Temp-Rename pattern.
/// Writes to `.jellyfinsync.json.tmp`, calls `sync_all`, then renames to `.jellyfinsync.json`.
pub async fn write_manifest(device_root: &Path, manifest: &DeviceManifest) -> Result<()> {
    let manifest_path = device_root.join(".jellyfinsync.json");
    let tmp_path = device_root.join(".jellyfinsync.json.tmp");

    let json = serde_json::to_string_pretty(manifest)?;

    {
        use tokio::io::AsyncWriteExt;
        let mut file = tokio::fs::File::create(&tmp_path).await?;
        file.write_all(json.as_bytes()).await?;
        file.sync_all().await?;
    }

    tokio::fs::rename(&tmp_path, &manifest_path).await?;
    Ok(())
}

/// Scans the specified managed paths recursively for leftover `.tmp`
/// files from interrupted writes and deletes them. Returns the count of deleted files.
/// Non-fatal: individual deletion failures are silently skipped.
pub async fn cleanup_tmp_files(device_root: &Path, managed_paths: &[String]) -> Result<usize> {
    let mut count = 0;
    for path_str in managed_paths {
        let managed_path = device_root.join(path_str);

        // Ensure the path is a directory and not a symlink to prevent traversal
        if let Ok(meta) = tokio::fs::symlink_metadata(&managed_path).await {
            if !meta.is_dir() {
                continue;
            }
        } else {
            continue; // Doesn't exist or access error
        }

        let mut dirs_to_visit = vec![managed_path];
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
                    // Prevent symlink traversal out of managed zone
                    continue;
                } else if file_type.is_dir() {
                    dirs_to_visit.push(path);
                } else if file_type.is_file() {
                    let file_name = path.file_name().unwrap_or_default().to_string_lossy();
                    // Match files ending in .tmp
                    if file_name.ends_with(".tmp") {
                        if tokio::fs::remove_file(&path).await.is_ok() {
                            count += 1;
                        }
                    }
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
    },
    Removed(PathBuf),
    Unrecognized {
        path: PathBuf,
    },
}

pub struct DeviceProber;

impl DeviceProber {
    pub async fn probe(path: &Path) -> Result<Option<DeviceManifest>> {
        let manifest_path = path.join(".jellyfinsync.json");
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
    /// All currently connected managed devices, keyed by mount path.
    connected_devices: std::sync::Arc<tokio::sync::RwLock<std::collections::HashMap<PathBuf, DeviceManifest>>>,
    /// The device path targeted by all UI operations. None when no device is selected.
    selected_device_path: std::sync::Arc<tokio::sync::RwLock<Option<PathBuf>>>,
    unrecognized_device_path: std::sync::Arc<tokio::sync::RwLock<Option<PathBuf>>>,
}

impl DeviceManager {
    pub fn new(db: std::sync::Arc<crate::db::Database>) -> Self {
        Self {
            db,
            connected_devices: std::sync::Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            selected_device_path: std::sync::Arc::new(tokio::sync::RwLock::new(None)),
            unrecognized_device_path: std::sync::Arc::new(tokio::sync::RwLock::new(None)),
        }
    }

    pub async fn handle_device_detected(
        &self,
        path: PathBuf,
        manifest: DeviceManifest,
    ) -> Result<crate::DaemonState> {
        {
            let mut devices = self.connected_devices.write().await;
            devices.insert(path.clone(), manifest.clone());
        }
        {
            let mut sel = self.selected_device_path.write().await;
            if sel.is_none() {
                // Auto-select first/only device
                *sel = Some(path.clone());
            }
            // If another device is already selected, don't change selection
        }
        {
            let mut unrecognized_path = self.unrecognized_device_path.write().await;
            *unrecognized_path = None;
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

    pub async fn handle_device_unrecognized(&self, path: PathBuf) -> crate::DaemonState {
        let path_str = path.to_string_lossy().to_string();
        // Unrecognized devices have no manifest — ensure they are not in connected_devices.
        // Do NOT change selected_device_path since other recognized devices may still be connected.
        {
            let mut devices = self.connected_devices.write().await;
            devices.remove(&path);
        }
        {
            let mut unrecognized_path = self.unrecognized_device_path.write().await;
            *unrecognized_path = Some(path);
        }
        crate::DaemonState::DeviceFound(path_str)
    }

    pub async fn get_unrecognized_device_path(&self) -> Option<PathBuf> {
        self.unrecognized_device_path.read().await.clone()
    }

    pub async fn handle_device_removed(&self, removed_path: &PathBuf) {
        // Capture remaining keys inside the write block to avoid holding connected_devices
        // read lock while acquiring selected_device_path write lock (would violate locking order).
        let remaining_keys: Vec<PathBuf> = {
            let mut devices = self.connected_devices.write().await;
            devices.remove(removed_path);
            devices.keys().cloned().collect()
        };
        {
            let mut sel = self.selected_device_path.write().await;
            if sel.as_ref() == Some(removed_path) {
                *sel = if remaining_keys.len() == 1 {
                    Some(remaining_keys[0].clone())
                } else {
                    None
                };
            }
        }
        {
            let mut unrecognized_path = self.unrecognized_device_path.write().await;
            // Only clear unrecognized_device_path if it matches the removed path; an
            // unrelated managed device removing should not erase a pending initialization.
            if unrecognized_path.as_ref() == Some(removed_path) {
                *unrecognized_path = None;
            }
        }
    }

    pub async fn get_current_device(&self) -> Option<DeviceManifest> {
        let sel = self.selected_device_path.read().await.clone();
        let path = sel?;
        let devices = self.connected_devices.read().await;
        devices.get(&path).cloned()
    }

    pub async fn get_current_device_path(&self) -> Option<PathBuf> {
        self.selected_device_path.read().await.clone()
    }

    /// Returns a snapshot of all currently connected managed devices.
    pub async fn get_connected_devices(&self) -> Vec<(PathBuf, DeviceManifest)> {
        self.connected_devices
            .read()
            .await
            .iter()
            .map(|(p, m)| (p.clone(), m.clone()))
            .collect()
    }

    /// Returns a consistent snapshot of connected devices and selected path in a single
    /// atomic read, preventing torn reads in get_daemon_state.
    pub async fn get_multi_device_snapshot(
        &self,
    ) -> (Vec<(PathBuf, DeviceManifest)>, Option<PathBuf>) {
        let devices = self.connected_devices.read().await;
        let sel = self.selected_device_path.read().await.clone();
        let device_list = devices.iter().map(|(p, m)| (p.clone(), m.clone())).collect();
        (device_list, sel)
    }

    /// Sets the selected device path. Returns false if path is not in connected_devices.
    pub async fn select_device(&self, path: PathBuf) -> bool {
        // Hold the write lock through both the existence check and the selection write so
        // a concurrent handle_device_removed cannot remove the device in between (TOCTOU).
        // Locking order: connected_devices (1) → selected_device_path (2).
        let devices = self.connected_devices.write().await;
        if !devices.contains_key(&path) {
            return false;
        }
        let mut sel = self.selected_device_path.write().await;
        *sel = Some(path);
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
        let selected_path = self.selected_device_path.read().await.clone();
        let path = selected_path.ok_or_else(|| anyhow::anyhow!("No device connected"))?;
        let mut devices = self.connected_devices.write().await;
        let manifest = devices
            .get_mut(&path)
            .ok_or_else(|| anyhow::anyhow!("Selected device not in connected map"))?;
        mutation(manifest);
        crate::device::write_manifest(&path, manifest).await?;
        Ok(())
    }

    pub async fn get_device_storage(&self) -> Option<StorageInfo> {
        let path = self.get_current_device_path().await?;
        get_storage_info(&path)
    }

    /// Initializes a new device by generating a UUID, writing the initial manifest,
    /// and transitioning the device from unrecognized to recognized state.
    pub async fn initialize_device(
        &self,
        folder_path: &str,
        transcoding_profile_id: Option<String>,
        name: String,
        icon: Option<String>,
    ) -> Result<DeviceManifest> {
        // Validate folder_path: no traversal, no absolute paths, single-level only
        if !folder_path.is_empty() {
            if folder_path.contains("..")
                || folder_path.starts_with('/')
                || folder_path.starts_with('\\')
                || folder_path.contains('/')
                || folder_path.contains('\\')
            {
                return Err(anyhow::anyhow!(
                    "Invalid folder path: must be a single folder name without path separators"
                ));
            }
        }

        let device_root = self
            .get_unrecognized_device_path()
            .await
            .ok_or_else(|| anyhow::anyhow!("No unrecognized device connected"))?;

        let device_id = uuid::Uuid::new_v4().to_string();

        let managed_paths = if folder_path.is_empty() {
            vec![]
        } else {
            // Create the subfolder on the device if it doesn't exist
            let target_folder = device_root.join(folder_path);
            tokio::fs::create_dir(&target_folder).await.or_else(|e| {
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    Ok(())
                } else {
                    Err(e)
                }
            })?;
            vec![folder_path.to_string()]
        };

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
            transcoding_profile_id,   // NEW — stored in .jellyfinsync.json on the device
            playlists: vec![],
        };

        write_manifest(&device_root, &manifest).await?;

        {
            let mut devices = self.connected_devices.write().await;
            devices.insert(device_root.clone(), manifest.clone());
        }
        {
            let mut sel = self.selected_device_path.write().await;
            // Only auto-select if no device is currently selected; don't steal selection
            // from a device the user has already chosen in a multi-device session.
            if sel.is_none() {
                *sel = Some(device_root);
            }
        }
        {
            let mut unrecognized_path = self.unrecognized_device_path.write().await;
            *unrecognized_path = None;
        }

        Ok(manifest)
    }

    pub async fn list_root_folders(&self) -> Result<Option<DeviceRootFoldersResponse>> {
        let device_path = match self.get_current_device_path().await {
            Some(p) => p,
            None => match self.get_unrecognized_device_path().await {
                Some(p) => p,
                None => return Ok(None),
            },
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

    /// Persists auto-fill preferences to the device manifest
    pub async fn save_auto_fill_prefs(&self, prefs: AutoFillPrefs) -> Result<()> {
        self.update_manifest(|manifest| {
            manifest.auto_fill = prefs;
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

pub async fn run_observer(tx: tokio::sync::mpsc::Sender<DeviceEvent>) {
    println!("[Device] Observer thread started");
    let mut known_mounts = std::collections::HashSet::new();

    loop {
        let current_mounts = get_mounts();

        // Detect new mounts
        for mount in &current_mounts {
            if !known_mounts.contains(mount) {
                known_mounts.insert(mount.clone());
                match DeviceProber::probe(mount).await {
                    Ok(Some(manifest)) => {
                        let _ = tx
                            .send(DeviceEvent::Detected {
                                path: mount.clone(),
                                manifest,
                            })
                            .await;
                    }
                    Ok(None) => {
                        if is_removable_drive(mount) {
                            let _ = tx
                                .send(DeviceEvent::Unrecognized {
                                    path: mount.clone(),
                                })
                                .await;
                        }
                    }
                    Err(_) => {} // Probe failed (e.g., permission error) — ignore
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
            let is_root_device = std::fs::metadata(&path)
                .map(|m| m.dev() == root_dev)
                .unwrap_or(true);
            if is_root_device {
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
