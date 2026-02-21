use anyhow::{Context, Result};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::sync::RwLock;

use crate::device::DeviceManifest;

/// An item desired for sync (from the UI basket / Jellyfin API).
#[derive(Debug, Clone)]
pub struct DesiredItem {
    pub jellyfin_id: String,
    pub name: String,
    pub album: Option<String>,
    pub artist: Option<String>,
    pub size_bytes: u64,
}

/// An item to be added to the device.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SyncAddItem {
    pub jellyfin_id: String,
    pub name: String,
    pub album: Option<String>,
    pub artist: Option<String>,
    pub size_bytes: u64,
}

/// An item to be deleted from the device.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SyncDeleteItem {
    pub jellyfin_id: String,
    pub local_path: String,
    pub name: String,
}

/// The result of a delta calculation between desired items and current manifest.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SyncDelta {
    pub adds: Vec<SyncAddItem>,
    pub deletes: Vec<SyncDeleteItem>,
    pub unchanged: Vec<String>,
}

/// Status of a sync operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum SyncStatus {
    Running,
    Complete,
    Failed,
}

/// Error details for a failed file operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncFileError {
    pub jellyfin_id: String,
    pub filename: String,
    pub error_message: String,
}

/// Tracks the state of an active sync operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncOperation {
    pub id: String,
    pub status: SyncStatus,
    pub started_at: String,
    pub current_file: Option<String>,
    pub bytes_current: u64,
    pub bytes_total: u64,
    pub files_completed: usize,
    pub files_total: usize,
    pub errors: Vec<SyncFileError>,
}

// Note: Push-based SyncProgress events deferred to future story.
// Progress is available via polling sync_get_operation_status RPC method.

/// Progress callback function signature for streaming file writes.
pub type ProgressCallback = Arc<dyn Fn(u64, u64) + Send + Sync>;

/// Manager for tracking active sync operations in memory.
pub struct SyncOperationManager {
    operations: Arc<RwLock<HashMap<String, SyncOperation>>>,
}

impl SyncOperationManager {
    pub fn new() -> Self {
        Self {
            operations: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn create_operation(&self, operation_id: String, files_total: usize) -> SyncOperation {
        // Generate ISO8601/RFC3339 timestamp
        // Using unix timestamp as placeholder - in production would use proper datetime crate
        let unix_timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let timestamp = unix_timestamp.to_string();

        let operation = SyncOperation {
            id: operation_id.clone(),
            status: SyncStatus::Running,
            started_at: timestamp,
            current_file: None,
            bytes_current: 0,
            bytes_total: 0,
            files_completed: 0,
            files_total,
            errors: vec![],
        };

        let mut ops = self.operations.write().await;
        ops.insert(operation_id, operation.clone());
        operation
    }

    pub async fn update_operation(&self, operation_id: &str, operation: SyncOperation) {
        let mut ops = self.operations.write().await;
        ops.insert(operation_id.to_string(), operation);
    }

    pub async fn get_operation(&self, operation_id: &str) -> Option<SyncOperation> {
        let ops = self.operations.read().await;
        ops.get(operation_id).cloned()
    }
}

/// Constructs a file path from Jellyfin item metadata.
///
/// Pattern: `{managed_path}/{AlbumArtist}/{Album}/{TrackNumber} - {Name}.{extension}`
///
/// Sanitizes path components to remove invalid filesystem characters.
pub fn construct_file_path(
    managed_path: &Path,
    item: &crate::api::JellyfinItem,
) -> Result<std::path::PathBuf> {
    // Extract and sanitize components
    let artist = item
        .album_artist
        .as_deref()
        .unwrap_or("Unknown Artist");
    let album = item.album.as_deref().unwrap_or("Unknown Album");
    let track_name = &item.name;

    // Format track number with zero padding if available
    let track_number = item
        .index_number
        .map(|n| format!("{:02}", n))
        .unwrap_or_else(|| String::from("00"));

    // Determine file extension from Container field
    let extension = item
        .container
        .as_deref()
        .unwrap_or("mp3");

    // Sanitize path components
    let artist_clean = sanitize_path_component(artist);
    let album_clean = sanitize_path_component(album);
    let track_name_clean = sanitize_path_component(track_name);

    // Build path: managed_path/Artist/Album/## - Track.ext
    let filename = format!("{} - {}.{}", track_number, track_name_clean, extension);
    let path = managed_path
        .join(artist_clean)
        .join(album_clean)
        .join(filename);

    // TODO: Add path length validation for legacy hardware (Story 4.3)

    Ok(path)
}

/// Sanitizes a path component by removing/replacing invalid filesystem characters.
fn sanitize_path_component(component: &str) -> String {
    component
        .chars()
        .map(|c| match c {
            // Invalid characters for Windows/FAT32
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            // Control characters
            c if c.is_control() => '_',
            // Valid character
            c => c,
        })
        .collect::<String>()
        .trim()
        .to_string()
}

/// Executes a sync operation by downloading adds and deleting removals.
///
/// This function handles individual file failures gracefully - if one file fails,
/// it continues with the remaining files and collects errors for reporting.
///
/// Returns a tuple of (successfully_synced_items, errors).
pub async fn execute_sync(
    delta: &SyncDelta,
    device_path: &Path,
    jellyfin_client: &crate::api::JellyfinClient,
    jellyfin_url: &str,
    jellyfin_token: &str,
    jellyfin_user_id: &str,
    operation_manager: Arc<SyncOperationManager>,
    operation_id: String,
) -> Result<(Vec<crate::device::SyncedItem>, Vec<SyncFileError>)> {
    let mut synced_items = Vec::new();
    let mut errors = Vec::new();

    // Determine managed path (assume first managed path from device manifest)
    // In a real implementation, this would be passed in or determined from manifest
    let managed_path = device_path.join("Music");

    // Process adds (downloads)
    for add_item in delta.adds.iter() {
        // Fetch item details to get metadata for path construction
        let item_result = jellyfin_client
            .get_item_details(jellyfin_url, jellyfin_token, jellyfin_user_id, &add_item.jellyfin_id)
            .await;

        let item = match item_result {
            Ok(item) => item,
            Err(e) => {
                errors.push(SyncFileError {
                    jellyfin_id: add_item.jellyfin_id.clone(),
                    filename: add_item.name.clone(),
                    error_message: format!("Failed to fetch item details: {}", e),
                });
                continue;
            }
        };

        // Construct target path
        let target_path = match construct_file_path(&managed_path, &item) {
            Ok(path) => path,
            Err(e) => {
                errors.push(SyncFileError {
                    jellyfin_id: add_item.jellyfin_id.clone(),
                    filename: add_item.name.clone(),
                    error_message: format!("Failed to construct file path: {}", e),
                });
                continue;
            }
        };

        // Get download stream
        let stream_result = jellyfin_client
            .download_item_stream(jellyfin_url, jellyfin_token, &add_item.jellyfin_id)
            .await;

        let stream = match stream_result {
            Ok(stream) => stream,
            Err(e) => {
                errors.push(SyncFileError {
                    jellyfin_id: add_item.jellyfin_id.clone(),
                    filename: add_item.name.clone(),
                    error_message: format!("Failed to get download stream: {}", e),
                });
                continue;
            }
        };

        // Create progress callback for this file
        let op_manager = operation_manager.clone();
        let op_id = operation_id.clone();
        let file_name = add_item.name.clone();
        let total_size = add_item.size_bytes;

        // Throttle progress updates to avoid spawning a task per chunk.
        // Only updates every 256KB or on the final chunk.
        let last_reported = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let progress_callback = Arc::new(move |bytes_written: u64, total: u64| {
            let last = last_reported.load(std::sync::atomic::Ordering::Relaxed);
            if bytes_written.saturating_sub(last) < 256 * 1024 && bytes_written < total {
                return;
            }
            last_reported.store(bytes_written, std::sync::atomic::Ordering::Relaxed);

            let op_manager_inner = op_manager.clone();
            let op_id_inner = op_id.clone();
            let file_name_inner = file_name.clone();

            tokio::spawn(async move {
                if let Some(mut operation) = op_manager_inner.get_operation(&op_id_inner).await {
                    operation.current_file = Some(file_name_inner);
                    operation.bytes_current = bytes_written;
                    operation.bytes_total = total;
                    op_manager_inner.update_operation(&op_id_inner, operation).await;
                }
            });
        }) as ProgressCallback;

        // Write file to disk using atomic pattern
        let write_result = write_file_streamed(stream, &target_path, total_size, progress_callback).await;

        match write_result {
            Ok(_) => {
                // Successfully synced - add to synced items
                let unix_timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();

                synced_items.push(crate::device::SyncedItem {
                    jellyfin_id: add_item.jellyfin_id.clone(),
                    name: add_item.name.clone(),
                    album: add_item.album.clone(),
                    artist: add_item.artist.clone(),
                    local_path: target_path
                        .strip_prefix(device_path)
                        .unwrap_or(&target_path)
                        .to_string_lossy()
                        .to_string(),
                    size_bytes: add_item.size_bytes,
                    synced_at: unix_timestamp.to_string(),
                });

                // Update operation progress
                if let Some(mut operation) = operation_manager.get_operation(&operation_id).await {
                    operation.files_completed += 1;
                    operation_manager.update_operation(&operation_id, operation).await;
                }
            }
            Err(e) => {
                errors.push(SyncFileError {
                    jellyfin_id: add_item.jellyfin_id.clone(),
                    filename: add_item.name.clone(),
                    error_message: format!("Failed to write file: {}", e),
                });
            }
        }
    }

    // Process deletes
    for delete_item in &delta.deletes {
        let file_path = device_path.join(&delete_item.local_path);

        // Verify file is in managed zone (security check)
        if !file_path.starts_with(&managed_path) {
            errors.push(SyncFileError {
                jellyfin_id: delete_item.jellyfin_id.clone(),
                filename: delete_item.name.clone(),
                error_message: "File is not in managed zone - refusing to delete".to_string(),
            });
            continue;
        }

        // Delete file
        match tokio::fs::remove_file(&file_path).await {
            Ok(_) => {
                // Successfully deleted
                if let Some(mut operation) = operation_manager.get_operation(&operation_id).await {
                    operation.files_completed += 1;
                    operation_manager.update_operation(&operation_id, operation).await;
                }
            }
            Err(e) => {
                errors.push(SyncFileError {
                    jellyfin_id: delete_item.jellyfin_id.clone(),
                    filename: delete_item.name.clone(),
                    error_message: format!("Failed to delete file: {}", e),
                });
            }
        }
    }

    Ok((synced_items, errors))
}

/// Streams a file from a byte stream to disk using the atomic Write-Temp-Rename pattern.
///
/// This function:
/// 1. Creates parent directories if needed
/// 2. Writes to a `.tmp` file first
/// 3. Calls progress callback after each chunk
/// 4. Calls `sync_all()` to flush to disk
/// 5. Atomically renames to final path
/// 6. Deletes `.tmp` on error
///
/// This pattern prevents corruption on unexpected disconnection.
pub async fn write_file_streamed<S>(
    mut stream: S,
    target_path: &Path,
    total_size: u64,
    on_progress: ProgressCallback,
) -> Result<()>
where
    S: futures::Stream<Item = std::result::Result<bytes::Bytes, reqwest::Error>> + Unpin,
{
    // Create parent directories if they don't exist
    if let Some(parent) = target_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .context("Failed to create parent directories")?;
    }

    // Determine temp file path — append .tmp to preserve original extension
    // e.g., "track.flac" → "track.flac.tmp" (NOT "track.tmp")
    let file_name = target_path
        .file_name()
        .context("Invalid target path: no filename")?;
    let tmp_path = target_path.with_file_name(format!("{}.tmp", file_name.to_string_lossy()));

    // Write to temp file with error cleanup
    let write_result: Result<()> = async {
        let mut file = File::create(&tmp_path)
            .await
            .context("Failed to create temp file")?;

        let mut bytes_written = 0u64;

        // Stream chunks to file
        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result.context("Failed to read chunk from stream")?;
            file.write_all(&chunk)
                .await
                .context("Failed to write chunk to file")?;

            bytes_written += chunk.len() as u64;
            on_progress(bytes_written, total_size);
        }

        // Flush all data to disk before rename (CRITICAL for atomicity)
        file.sync_all()
            .await
            .context("Failed to sync file to disk")?;

        Ok(())
    }
    .await;

    // Handle errors: delete temp file if write failed
    if let Err(e) = write_result {
        let _ = tokio::fs::remove_file(&tmp_path).await; // Best effort cleanup
        return Err(e);
    }

    // Atomically rename temp to final path
    tokio::fs::rename(&tmp_path, target_path)
        .await
        .context("Failed to rename temp file to final path")?;

    Ok(())
}

/// Calculates the delta between desired items (from basket) and the current manifest.
///
/// Performs server ID change detection: if an item in adds matches a delete by
/// (name, album, artist) metadata, it's treated as an ID reassignment rather than
/// a separate add+delete.
pub fn calculate_delta(desired_items: &[DesiredItem], manifest: &DeviceManifest) -> SyncDelta {
    let current_ids: HashSet<&str> = manifest
        .synced_items
        .iter()
        .map(|i| i.jellyfin_id.as_str())
        .collect();

    let desired_ids: HashSet<&str> = desired_items
        .iter()
        .map(|i| i.jellyfin_id.as_str())
        .collect();

    // Initial adds: desired items not in current manifest
    let adds: Vec<SyncAddItem> = desired_items
        .iter()
        .filter(|i| !current_ids.contains(i.jellyfin_id.as_str()))
        .map(|i| SyncAddItem {
            jellyfin_id: i.jellyfin_id.clone(),
            name: i.name.clone(),
            album: i.album.clone(),
            artist: i.artist.clone(),
            size_bytes: i.size_bytes,
        })
        .collect();

    // Initial deletes: manifest items not in desired set
    // AND build the metadata map in the same pass
    let mut deletes: Vec<SyncDeleteItem> = Vec::new();
    let mut delete_by_metadata: HashMap<(String, Option<String>, Option<String>), usize> =
        HashMap::new();

    for item in &manifest.synced_items {
        if !desired_ids.contains(item.jellyfin_id.as_str()) {
            let idx = deletes.len();
            deletes.push(SyncDeleteItem {
                jellyfin_id: item.jellyfin_id.clone(),
                local_path: item.local_path.clone(),
                name: item.name.clone(),
            });

            let key = (
                item.name.to_lowercase(),
                item.album.as_ref().map(|a| a.to_lowercase()),
                item.artist.as_ref().map(|a| a.to_lowercase()),
            );
            delete_by_metadata.insert(key, idx);
        }
    }

    // Find adds that match a delete by metadata (ID change detection)
    let mut matched_add_indices: HashSet<usize> = HashSet::new();
    let mut matched_delete_indices: HashSet<usize> = HashSet::new();

    for (add_idx, add) in adds.iter().enumerate() {
        let key = (
            add.name.to_lowercase(),
            add.album.as_ref().map(|a| a.to_lowercase()),
            add.artist.as_ref().map(|a| a.to_lowercase()),
        );

        if let Some(&del_idx) = delete_by_metadata.get(&key) {
            if !matched_delete_indices.contains(&del_idx) {
                matched_add_indices.insert(add_idx);
                matched_delete_indices.insert(del_idx);
            }
        }
    }

    // Remove matched pairs — these are ID reassignments, not real adds/deletes
    // The add still stays (new ID needs to be recorded), but the delete is removed
    // since the file content is equivalent.
    // Actually, for ID changes: we keep the add (new ID) and remove the delete
    // so the file stays on disk but gets updated in manifest with new ID.
    let deletes: Vec<SyncDeleteItem> = deletes
        .into_iter()
        .enumerate()
        .filter(|(idx, _)| !matched_delete_indices.contains(idx))
        .map(|(_, d)| d)
        .collect();

    // Keep all adds — even ID-changed ones need to be re-recorded with new ID
    // But actually, matched adds should also stay since the manifest needs updating.
    // The adds remain as-is; only the deletes are suppressed for matched pairs.

    // Unchanged: items in both sets
    let unchanged: Vec<String> = desired_items
        .iter()
        .filter(|i| current_ids.contains(i.jellyfin_id.as_str()))
        .map(|i| i.jellyfin_id.clone())
        .collect();

    SyncDelta {
        adds,
        deletes,
        unchanged,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::{DeviceManifest, SyncedItem};

    fn empty_manifest() -> DeviceManifest {
        DeviceManifest {
            device_id: "test-device".to_string(),
            name: Some("Test".to_string()),
            version: "1.0".to_string(),
            managed_paths: vec!["Music".to_string()],
            synced_items: vec![],
        }
    }

    fn make_synced_item(
        id: &str,
        name: &str,
        album: Option<&str>,
        artist: Option<&str>,
    ) -> SyncedItem {
        SyncedItem {
            jellyfin_id: id.to_string(),
            name: name.to_string(),
            album: album.map(|s| s.to_string()),
            artist: artist.map(|s| s.to_string()),
            local_path: format!("Music/{}/{}.flac", artist.unwrap_or("Unknown"), name),
            size_bytes: 10_000_000,
            synced_at: "2026-02-15T10:00:00Z".to_string(),
        }
    }

    fn make_desired(
        id: &str,
        name: &str,
        album: Option<&str>,
        artist: Option<&str>,
    ) -> DesiredItem {
        DesiredItem {
            jellyfin_id: id.to_string(),
            name: name.to_string(),
            album: album.map(|s| s.to_string()),
            artist: artist.map(|s| s.to_string()),
            size_bytes: 10_000_000,
        }
    }

    #[test]
    fn test_delta_empty_manifest() {
        let manifest = empty_manifest();
        let desired = vec![
            make_desired("a", "Track A", Some("Album"), Some("Artist")),
            make_desired("b", "Track B", Some("Album"), Some("Artist")),
        ];

        let delta = calculate_delta(&desired, &manifest);
        assert_eq!(delta.adds.len(), 2);
        assert_eq!(delta.deletes.len(), 0);
        assert_eq!(delta.unchanged.len(), 0);
    }

    #[test]
    fn test_delta_full_overlap() {
        let mut manifest = empty_manifest();
        manifest.synced_items = vec![
            make_synced_item("a", "Track A", Some("Album"), Some("Artist")),
            make_synced_item("b", "Track B", Some("Album"), Some("Artist")),
        ];

        let desired = vec![
            make_desired("a", "Track A", Some("Album"), Some("Artist")),
            make_desired("b", "Track B", Some("Album"), Some("Artist")),
        ];

        let delta = calculate_delta(&desired, &manifest);
        assert_eq!(delta.adds.len(), 0);
        assert_eq!(delta.deletes.len(), 0);
        assert_eq!(delta.unchanged.len(), 2);
    }

    #[test]
    fn test_delta_partial_overlap() {
        let mut manifest = empty_manifest();
        manifest.synced_items = vec![
            make_synced_item("a", "Track A", Some("Album"), Some("Artist")),
            make_synced_item("b", "Track B", Some("Album"), Some("Artist")),
        ];

        let desired = vec![
            make_desired("a", "Track A", Some("Album"), Some("Artist")),
            make_desired("c", "Track C", Some("Album"), Some("Artist")),
        ];

        let delta = calculate_delta(&desired, &manifest);
        assert_eq!(delta.adds.len(), 1);
        assert_eq!(delta.adds[0].jellyfin_id, "c");
        assert_eq!(delta.deletes.len(), 1);
        assert_eq!(delta.deletes[0].jellyfin_id, "b");
        assert_eq!(delta.unchanged.len(), 1);
        assert_eq!(delta.unchanged[0], "a");
    }

    #[test]
    fn test_delta_complete_replacement() {
        let mut manifest = empty_manifest();
        manifest.synced_items = vec![
            make_synced_item("a", "Track A", Some("Album"), Some("Artist")),
            make_synced_item("b", "Track B", Some("Album"), Some("Artist")),
        ];

        let desired = vec![
            make_desired("c", "Track C", Some("Album2"), Some("Artist2")),
            make_desired("d", "Track D", Some("Album2"), Some("Artist2")),
        ];

        let delta = calculate_delta(&desired, &manifest);
        assert_eq!(delta.adds.len(), 2);
        assert_eq!(delta.deletes.len(), 2);
        assert_eq!(delta.unchanged.len(), 0);
    }

    #[test]
    fn test_delta_server_id_change_detection() {
        let mut manifest = empty_manifest();
        manifest.synced_items = vec![make_synced_item(
            "old-id-1",
            "My Song",
            Some("My Album"),
            Some("My Artist"),
        )];

        // Same metadata but different Jellyfin ID (server re-scanned)
        let desired = vec![make_desired(
            "new-id-1",
            "My Song",
            Some("My Album"),
            Some("My Artist"),
        )];

        let delta = calculate_delta(&desired, &manifest);
        // The delete should be suppressed (ID change detected via metadata match)
        assert_eq!(delta.deletes.len(), 0);
        // The add remains so the manifest gets updated with the new ID
        assert_eq!(delta.adds.len(), 1);
        assert_eq!(delta.adds[0].jellyfin_id, "new-id-1");
        assert_eq!(delta.unchanged.len(), 0);
    }

    // ===== Story 4.2 Tests =====

    #[test]
    fn test_construct_file_path_basic() {
        let managed = std::path::PathBuf::from("Music");
        let item = crate::api::JellyfinItem {
            id: "item1".to_string(),
            name: "Speak to Me".to_string(),
            item_type: "Audio".to_string(),
            album: Some("The Dark Side of the Moon".to_string()),
            album_artist: Some("Pink Floyd".to_string()),
            index_number: Some(1),
            container: Some("flac".to_string()),
            production_year: None,
            recursive_item_count: None,
            cumulative_run_time_ticks: None,
            media_sources: None,
        };

        let path = construct_file_path(&managed, &item).unwrap();
        let expected = managed
            .join("Pink Floyd")
            .join("The Dark Side of the Moon")
            .join("01 - Speak to Me.flac");
        assert_eq!(path, expected);
    }

    #[test]
    fn test_construct_file_path_missing_fields_uses_defaults() {
        let managed = std::path::PathBuf::from("Music");
        let item = crate::api::JellyfinItem {
            id: "item2".to_string(),
            name: "Unknown Track".to_string(),
            item_type: "Audio".to_string(),
            album: None,
            album_artist: None,
            index_number: None,
            container: None,
            production_year: None,
            recursive_item_count: None,
            cumulative_run_time_ticks: None,
            media_sources: None,
        };

        let path = construct_file_path(&managed, &item).unwrap();
        let expected = managed
            .join("Unknown Artist")
            .join("Unknown Album")
            .join("00 - Unknown Track.mp3");
        assert_eq!(path, expected);
    }

    #[test]
    fn test_sanitize_path_component_replaces_invalid_chars() {
        assert_eq!(sanitize_path_component("Hello: World"), "Hello_ World");
        assert_eq!(sanitize_path_component("A<B>C"), "A_B_C");
        assert_eq!(sanitize_path_component("file/name\\test"), "file_name_test");
        assert_eq!(sanitize_path_component("pipe|question?star*"), "pipe_question_star_");
        assert_eq!(sanitize_path_component("ok chars 123"), "ok chars 123");
    }

    #[test]
    fn test_sanitize_path_component_trims_whitespace() {
        assert_eq!(sanitize_path_component("  trimmed  "), "trimmed");
    }

    #[tokio::test]
    async fn test_write_file_streamed_success() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let file_path = tmp_dir.path().join("artist").join("album").join("01 - track.flac");

        let data: Vec<std::result::Result<bytes::Bytes, reqwest::Error>> = vec![
            Ok(bytes::Bytes::from("chunk1")),
            Ok(bytes::Bytes::from("chunk2")),
            Ok(bytes::Bytes::from("chunk3")),
        ];
        let stream = futures::stream::iter(data);

        let progress_count = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let pc = progress_count.clone();
        let callback: ProgressCallback = Arc::new(move |_bytes, _total| {
            pc.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        });

        let result = write_file_streamed(stream, &file_path, 18, callback).await;
        assert!(result.is_ok(), "write_file_streamed failed: {:?}", result);
        assert!(file_path.exists(), "Final file should exist");

        let content = tokio::fs::read_to_string(&file_path).await.unwrap();
        assert_eq!(content, "chunk1chunk2chunk3");

        // Verify .tmp file was cleaned up (renamed to final)
        let tmp_path = file_path.with_file_name("01 - track.flac.tmp");
        assert!(!tmp_path.exists(), ".tmp file should not remain after success");

        // Verify progress was called for each chunk
        assert_eq!(
            progress_count.load(std::sync::atomic::Ordering::Relaxed),
            3
        );
    }

    #[tokio::test]
    async fn test_write_file_streamed_creates_parent_dirs() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let deep_path = tmp_dir
            .path()
            .join("a")
            .join("b")
            .join("c")
            .join("file.mp3");

        let data: Vec<std::result::Result<bytes::Bytes, reqwest::Error>> =
            vec![Ok(bytes::Bytes::from("data"))];
        let stream = futures::stream::iter(data);
        let callback: ProgressCallback = Arc::new(|_, _| {});

        let result = write_file_streamed(stream, &deep_path, 4, callback).await;
        assert!(result.is_ok());
        assert!(deep_path.exists());
    }

    #[tokio::test]
    async fn test_sync_operation_manager_lifecycle() {
        let manager = SyncOperationManager::new();

        // Create operation
        let op = manager.create_operation("op-1".to_string(), 10).await;
        assert_eq!(op.status, SyncStatus::Running);
        assert_eq!(op.files_total, 10);
        assert_eq!(op.files_completed, 0);

        // Get operation
        let fetched = manager.get_operation("op-1").await;
        assert!(fetched.is_some());
        assert_eq!(fetched.unwrap().id, "op-1");

        // Update operation
        let mut updated = manager.get_operation("op-1").await.unwrap();
        updated.files_completed = 5;
        updated.status = SyncStatus::Complete;
        manager.update_operation("op-1", updated).await;

        let final_op = manager.get_operation("op-1").await.unwrap();
        assert_eq!(final_op.files_completed, 5);
        assert_eq!(final_op.status, SyncStatus::Complete);

        // Non-existent operation
        assert!(manager.get_operation("non-existent").await.is_none());
    }

    #[test]
    fn test_delta_id_change_case_insensitive() {
        let mut manifest = empty_manifest();
        manifest.synced_items = vec![make_synced_item(
            "old-id",
            "my song",
            Some("my album"),
            Some("my artist"),
        )];

        let desired = vec![make_desired(
            "new-id",
            "My Song",
            Some("My Album"),
            Some("My Artist"),
        )];

        let delta = calculate_delta(&desired, &manifest);
        assert_eq!(delta.deletes.len(), 0);
        assert_eq!(delta.adds.len(), 1);
    }
}
