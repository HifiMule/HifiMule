use anyhow::{Context, Result};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::device::DeviceManifest;

/// Returns the current UTC time as an ISO 8601 / RFC 3339 string.
///
/// Format: `YYYY-MM-DDTHH:MM:SSZ`
///
/// Uses pure `std` arithmetic — no `chrono` dependency required.
fn now_iso8601() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Days since epoch
    let days = secs / 86400;
    let day_secs = secs % 86400;
    let hours = day_secs / 3600;
    let minutes = (day_secs % 3600) / 60;
    let seconds = day_secs % 60;

    // Civil date from days since 1970-01-01 (algorithm from Howard Hinnant)
    let z = days as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y, m, d, hours, minutes, seconds
    )
}

/// An item desired for sync (from the UI basket / Jellyfin API).
#[derive(Debug, Clone)]
pub struct DesiredItem {
    pub jellyfin_id: String,
    pub name: String,
    pub album: Option<String>,
    pub artist: Option<String>,
    pub size_bytes: u64,
    pub etag: Option<String>,
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
    pub etag: Option<String>,
}

/// An item to be deleted from the device.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SyncDeleteItem {
    pub jellyfin_id: String,
    pub local_path: String,
    pub name: String,
}

/// An item whose Jellyfin ID changed but file remains identical.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SyncIdChangeItem {
    pub old_jellyfin_id: String,
    pub new_jellyfin_id: String,
    pub old_local_path: String,
    pub name: String,
    pub album: Option<String>,
    pub artist: Option<String>,
    pub size_bytes: u64,
    pub etag: Option<String>,
    /// Preserved from the old manifest entry — set if the filename was previously truncated.
    #[serde(default)]
    pub original_name: Option<String>,
}

/// Metadata for a single track within a playlist, for M3U generation.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistTrackInfo {
    pub jellyfin_id: String,
    pub artist: Option<String>,
    pub run_time_seconds: i64, // RunTimeTicks / 10_000_000; -1 if unknown
}

/// A Jellyfin playlist from the basket, with its ordered track list for M3U generation.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistSyncItem {
    pub jellyfin_id: String,
    pub name: String,
    pub tracks: Vec<PlaylistTrackInfo>,
}

/// The result of a delta calculation between desired items and current manifest.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SyncDelta {
    pub adds: Vec<SyncAddItem>,
    pub deletes: Vec<SyncDeleteItem>,
    pub id_changes: Vec<SyncIdChangeItem>,
    pub unchanged: usize,
    #[serde(default)]
    pub playlists: Vec<PlaylistSyncItem>, // playlist basket items with ordered tracks
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
    pub bytes_transferred: u64,
    pub total_bytes: u64,
    pub files_completed: usize,
    pub files_total: usize,
    pub errors: Vec<SyncFileError>,
    #[serde(default)]
    pub warnings: Vec<String>,
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

    pub async fn create_operation(
        &self,
        operation_id: String,
        files_total: usize,
    ) -> SyncOperation {
        let timestamp = now_iso8601();

        let operation = SyncOperation {
            id: operation_id.clone(),
            status: SyncStatus::Running,
            started_at: timestamp,
            current_file: None,
            bytes_current: 0,
            bytes_total: 0,
            bytes_transferred: 0,
            total_bytes: 0,
            files_completed: 0,
            files_total,
            errors: vec![],
            warnings: vec![],
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

    pub async fn has_active_operation(&self) -> bool {
        self.get_active_operation_id().await.is_some()
    }

    pub async fn get_active_operation_id(&self) -> Option<String> {
        let ops = self.operations.read().await;
        ops.values()
            .find(|op| op.status == SyncStatus::Running)
            .map(|op| op.id.clone())
    }

    pub async fn get_all_operations(&self) -> Vec<SyncOperation> {
        let ops = self.operations.read().await;
        ops.values().cloned().collect()
    }
}

/// Maximum characters per path component enforced for FAT32/Rockbox legacy hardware.
pub const MAX_PATH_COMPONENT_LEN: usize = 255;

/// Windows MAX_PATH limit (260). We use a conservative 250 to allow for drive letters and slight overhead.
pub const WINDOWS_MAX_PATH: usize = 250;

/// The result of constructing a file path from Jellyfin metadata.
///
/// Contains the resolved filesystem path and an optional mapping of the
/// original Jellyfin track name if truncation was applied.
#[derive(Debug)]
pub struct PathConstructionResult {
    /// The final path where the file will be written (truncated as necessary).
    pub path: std::path::PathBuf,
    /// The original Jellyfin track name, set only if the filename component
    /// was truncated due to legacy hardware path length constraints.
    pub original_name: Option<String>,
}

/// Constructs a file path from Jellyfin item metadata.
///
/// Pattern: `{managed_path}/{AlbumArtist}/{Album}/{TrackNumber} - {Name}.{extension}`
///
/// Sanitizes path components to remove invalid filesystem characters and enforces
/// legacy hardware path length limits (255 characters per component).
#[allow(dead_code)]
pub fn construct_file_path(
    managed_path: &Path,
    item: &crate::api::JellyfinItem,
) -> Result<PathConstructionResult> {
    construct_file_path_with_extension(managed_path, item, None)
}

fn construct_file_path_with_extension(
    managed_path: &Path,
    item: &crate::api::JellyfinItem,
    extension_override: Option<&str>,
) -> Result<PathConstructionResult> {
    // Extract and sanitize components
    let artist = item.album_artist.as_deref().unwrap_or("Unknown Artist");
    let album = item.album.as_deref().unwrap_or("Unknown Album");
    let track_name = &item.name;

    // Format track number with zero padding if available
    let track_number = item
        .index_number
        .map(|n| format!("{:02}", n))
        .unwrap_or_else(|| String::from("00"));

    // Determine file extension from Container field
    let extension = extension_override.unwrap_or_else(|| source_container(item).unwrap_or("mp3"));

    // Step 1: Sanitize path components (remove invalid chars)
    let artist_clean = sanitize_path_component(artist);
    let album_clean = sanitize_path_component(album);
    let track_name_clean = sanitize_path_component(track_name);

    // Step 2: Enforce per-component length limit for legacy hardware (FAT32/Rockbox)
    // We initially use the max allowed, but we may need to shrink it if the total path exceeds MAX_PATH.
    let mut current_max_component = MAX_PATH_COMPONENT_LEN;

    // We will loop to iteratively shrink the component size if we hit MAX_PATH
    loop {
        let artist_final = truncate_component(&artist_clean, current_max_component);
        let album_final = truncate_component(&album_clean, current_max_component);

        // Build filename and check component length
        let filename_base = format!("{} - {}", track_number, track_name_clean);
        let filename_candidate = format!("{}.{}", filename_base, extension);

        let (filename, original_name) =
            if filename_candidate.chars().count() > current_max_component {
                let truncated = truncate_filename(&filename_base, extension, current_max_component);
                (truncated, Some(item.name.clone()))
            } else {
                (filename_candidate, None)
            };

        // Build final path
        let path = managed_path
            .join(&artist_final)
            .join(&album_final)
            .join(&filename);

        // Check total path length against Windows MAX_PATH
        // Provide a reasonable absolute path approximation if managed_path is relative
        let approx_abs_len = match path.canonicalize() {
            Ok(p) => p.to_string_lossy().chars().count(),
            // If it doesn't exist yet, we approximate by converting it to an absolute path first
            Err(_) => match std::env::current_dir() {
                Ok(cwd) => cwd.join(&path).to_string_lossy().chars().count(),
                Err(_) => path.to_string_lossy().chars().count() + 30, // rough guess for C:\Workspaces\...
            },
        };

        if approx_abs_len > WINDOWS_MAX_PATH {
            // Path is too long. We need to shrink the components.
            // Shrinking aggressively to reach a safe length faster.
            if current_max_component > 30 {
                current_max_component = (current_max_component * 3) / 4; // Reduce by 25%
                continue;
            } else {
                // Even with minimal components, it's too long. The managed_path itself is probably too long.
                // We have to return what we have and let the OS error out, or return our own error.
                return Err(anyhow::anyhow!("Resulting path is too long for Windows MAX_PATH ({}), even after minimal truncation: {}", WINDOWS_MAX_PATH, path.display()));
            }
        }

        return Ok(PathConstructionResult {
            path,
            original_name,
        });
    }
}

fn source_container(item: &crate::api::JellyfinItem) -> Option<&str> {
    item.media_sources
        .as_ref()
        .and_then(|sources| sources.first())
        .and_then(|s| s.container.as_deref())
        .or(item.container.as_deref())
}

fn forced_audio_profile(container: &str) -> serde_json::Value {
    let bitrate = if container.eq_ignore_ascii_case("mp3") {
        320000
    } else {
        256000
    };

    serde_json::json!({
        "Name": format!("JellyfinSync-Forced-{}", container),
        "MaxStreamingBitrate": bitrate,
        "MusicStreamingTranscodingBitrate": bitrate,
        "DirectPlayProfiles": [],
        "TranscodingProfiles": [
            {
                "Container": container,
                "Type": "Audio",
                "AudioCodec": container,
                "Protocol": "http",
                "EstimateContentLength": true,
                "EnableMpegtsM2TsMode": false
            }
        ],
        "CodecProfiles": []
    })
}

/// Sanitizes a path component by removing/replacing invalid filesystem characters.
///
/// Also strips trailing dots and spaces — forbidden by FAT32/Windows for both
/// files and folders (e.g. `"Once upon a..."` → `"Once upon a"`).
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
        .trim_end_matches('.')
        .to_string()
}

/// Truncates a path component (artist or album folder) to `max_len` characters.
///
/// Uses `chars().count()` for character-aware length (not byte length), safe for Unicode.
/// Always strips trailing spaces and dots — forbidden by FAT32 regardless of truncation.
/// Falls back to `"_"` if the result would be empty (all chars stripped), preventing
/// invalid empty path components like `Music//Album/track.flac`.
fn truncate_component(component: &str, max_len: usize) -> String {
    let source: String = if component.chars().count() <= max_len {
        component.to_string()
    } else {
        component.chars().take(max_len).collect()
    };
    let cleaned = source.trim_end_matches(|c| c == ' ' || c == '.');
    if cleaned.is_empty() {
        "_".to_string()
    } else {
        cleaned.to_string()
    }
}

/// Truncates a filename (base + extension) to `max_len` characters, preserving the extension.
///
/// The extension is always preserved — only the base name is truncated.
/// Strips trailing spaces and dots from the base after truncation (FAT32 requirement).
/// In the pathological case where the extension itself is ≥ max_len characters, the extension
/// is truncated to fit (dot + first N-1 chars) rather than dropping it — preserving extension
/// is more important than strict length compliance for device compatibility.
fn truncate_filename(base: &str, extension: &str, max_len: usize) -> String {
    let ext_len = extension.chars().count() + 1; // +1 for the '.' separator
    if ext_len >= max_len {
        // Pathological: extension itself fills the limit.
        // Return a truncated extension rather than dropping it entirely.
        let truncated_ext: String = extension.chars().take(max_len.saturating_sub(1)).collect();
        let clean_ext = truncated_ext.trim_end_matches(|c| c == ' ' || c == '.');
        return format!(".{}", clean_ext);
    }
    let max_base_len = max_len - ext_len;
    let truncated_base: String = base.chars().take(max_base_len).collect();
    let clean_base = truncated_base.trim_end_matches(|c| c == ' ' || c == '.');
    format!("{}.{}", clean_base, extension)
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
    device_manager: Arc<crate::device::DeviceManager>,
    transcoding_profile: Option<serde_json::Value>,
    device_io: Arc<dyn crate::device_io::DeviceIO>,
) -> Result<(Vec<crate::device::SyncedItem>, Vec<SyncFileError>)> {
    let mut synced_items = Vec::new();
    let mut errors = Vec::new();
    if let Err(e) = device_io.begin_sync_job().await {
        errors.push(SyncFileError {
            jellyfin_id: String::new(),
            filename: String::new(),
            error_message: format!("Failed to begin device sync job: {}", e),
        });
    }

    if delta.adds.is_empty() && delta.deletes.is_empty() && delta.id_changes.is_empty() {
        println!("[Sync] Executing empty sync to clear device managed paths");
    }

    // Compute total bytes for ETA (adds + id_changes both contribute bytes)
    let total_job_bytes: u64 = delta.adds.iter().map(|a| a.size_bytes).sum::<u64>()
        + delta.id_changes.iter().map(|c| c.size_bytes).sum::<u64>();
    if let Some(mut operation) = operation_manager.get_operation(&operation_id).await {
        operation.total_bytes = total_job_bytes;
        operation_manager.update_operation(&operation_id, operation).await;
    }

    // Shared counter for cumulative bytes written across all files (for ETA)
    let completed_bytes_arc = Arc::new(std::sync::atomic::AtomicU64::new(0));

    // Determine managed path from the device manifest's first managed_paths entry.
    let managed_path = {
        let snapshot = device_manager.get_current_device().await;
        let subfolder = snapshot
            .as_ref()
            .and_then(|m| m.managed_paths.first())
            .map(|s| s.as_str())
            .unwrap_or("Music");
        device_path.join(subfolder)
    };

    // Pre-fetch all item details for adds to avoid N+1 queries
    let mut fetched_items = std::collections::HashMap::new();
    let add_ids: Vec<&str> = delta.adds.iter().map(|a| a.jellyfin_id.as_str()).collect();
    for chunk in add_ids.chunks(100) {
        match jellyfin_client
            .get_items_by_ids(jellyfin_url, jellyfin_token, jellyfin_user_id, chunk)
            .await
        {
            Ok(items) => {
                for item in items {
                    fetched_items.insert(item.id.clone(), item);
                }
            }
            Err(e) => {
                for id in chunk {
                    let filename = delta
                        .adds
                        .iter()
                        .find(|a| a.jellyfin_id == **id)
                        .map(|a| a.name.clone())
                        .unwrap_or_else(|| "Unknown".to_string());
                    errors.push(SyncFileError {
                        jellyfin_id: id.to_string(),
                        filename,
                        error_message: format!("Failed to fetch chunk involving item: {}", e),
                    });
                }
            }
        }
    }

    // Process adds (downloads)
    for add_item in delta.adds.iter() {
        // Find prefetched item
        let item = match fetched_items.get(&add_item.jellyfin_id) {
            Some(i) => i,
            None => {
                // Not found (either didn't exist or fell into a failed chunk)
                // If it wasn't a chunk failure, it might just be missing
                if !errors.iter().any(|e| e.jellyfin_id == add_item.jellyfin_id) {
                    errors.push(SyncFileError {
                        jellyfin_id: add_item.jellyfin_id.clone(),
                        filename: add_item.name.clone(),
                        error_message: "Failed to fetch item details: Not found or API error."
                            .to_string(),
                    });
                }
                continue;
            }
        };

        let preferred_audio_container = device_io.preferred_audio_container();
        let effective_transcoding_profile;
        let stream_profile = if let Some(container) = preferred_audio_container {
            effective_transcoding_profile = forced_audio_profile(container);
            Some(&effective_transcoding_profile)
        } else {
            transcoding_profile.as_ref()
        };

        // Construct target path (includes legacy hardware path length validation)
        let construction = match construct_file_path_with_extension(
            &managed_path,
            &item,
            preferred_audio_container,
        ) {
            Ok(result) => result,
            Err(e) => {
                errors.push(SyncFileError {
                    jellyfin_id: add_item.jellyfin_id.clone(),
                    filename: add_item.name.clone(),
                    error_message: format!("Failed to construct file path: {}", e),
                });
                continue;
            }
        };
        let target_path = construction.path;

        // Resolve stream via PlaybackInfo if a profile is set, else direct /Download
        let stream_result = jellyfin_client
            .get_item_stream(
                jellyfin_url,
                jellyfin_token,
                jellyfin_user_id,
                &add_item.jellyfin_id,
                stream_profile,
            )
            .await;

        let stream = match stream_result {
            Ok(stream) => stream,
            Err(e) => {
                errors.push(SyncFileError {
                    jellyfin_id: add_item.jellyfin_id.clone(),
                    filename: add_item.name.clone(),
                    error_message: format!("Failed to get stream: {}", e),
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
        // Note: bytes_transferred is only updated at file completion (not mid-file) to avoid
        // a race where a stale spawned task could overwrite a higher post-completion value.
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
                    op_manager_inner
                        .update_operation(&op_id_inner, operation)
                        .await;
                }
            });
        }) as ProgressCallback;

        // Compute relative path for DeviceIO (relative to device root)
        let rel_path = target_path
            .strip_prefix(device_path)
            .unwrap_or(&target_path)
            .to_string_lossy()
            .replace('\\', "/");

        // Buffer the stream into memory, reporting progress during download
        let buffer_result = buffer_stream(stream, total_size, progress_callback).await;
        let buffer = match buffer_result {
            Ok(b) => b,
            Err(e) => {
                errors.push(SyncFileError {
                    jellyfin_id: add_item.jellyfin_id.clone(),
                    filename: add_item.name.clone(),
                    error_message: format!("Failed to buffer stream: {}", e),
                });
                continue;
            }
        };

        // Write file via device IO abstraction
        let write_result = device_io.write_with_verify(&rel_path, &buffer).await;

        match write_result {
            Ok(_) => {
                // Successfully synced - add to synced items
                let synced_at = now_iso8601();

                synced_items.push(crate::device::SyncedItem {
                    jellyfin_id: add_item.jellyfin_id.clone(),
                    name: add_item.name.clone(),
                    album: add_item.album.clone(),
                    artist: add_item.artist.clone(),
                    local_path: rel_path.clone(),
                    size_bytes: add_item.size_bytes,
                    synced_at,
                    original_name: construction.original_name,
                    etag: add_item.etag.clone(),
                });

                // Update operation progress and cumulative bytes
                completed_bytes_arc
                    .fetch_add(add_item.size_bytes, std::sync::atomic::Ordering::Relaxed);
                let cumulative = completed_bytes_arc.load(std::sync::atomic::Ordering::Relaxed);
                if let Some(mut operation) = operation_manager.get_operation(&operation_id).await {
                    operation.files_completed += 1;
                    operation.bytes_transferred = cumulative;
                    operation_manager
                        .update_operation(&operation_id, operation)
                        .await;
                }

                // Per-file manifest update for dirty-resume support (Story 4.4)
                // Per-file writes ensure manifest always reflects completed work for true delta resume.
                let synced_item = synced_items.last().unwrap().clone();
                if let Err(e) = device_manager
                    .update_manifest(|m| {
                        m.synced_items.push(synced_item);
                    })
                    .await
                {
                    eprintln!("[Sync] Warning: per-file manifest write failed: {}", e);
                    // Non-fatal: sync continues even if per-file write fails
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
        // Resolve absolute paths to prevent directory traversal attacks (e.g. local_path = "../../../etc/passwd")
        let absolute_file_path = match file_path.canonicalize() {
            Ok(path) => path,
            Err(_) => {
                // If it doesn't exist, we can't canonicalize it, but that also means there's nothing to delete.
                // Just skip it.
                continue;
            }
        };

        // We also need to canonicalize the managed_path for a robust prefix check
        let absolute_managed_path = match managed_path.canonicalize() {
            Ok(path) => path,
            Err(e) => {
                errors.push(SyncFileError {
                    jellyfin_id: delete_item.jellyfin_id.clone(),
                    filename: delete_item.name.clone(),
                    error_message: format!("Failed to resolve managed path: {}", e),
                });
                continue;
            }
        };

        if !absolute_file_path.starts_with(&absolute_managed_path) {
            errors.push(SyncFileError {
                jellyfin_id: delete_item.jellyfin_id.clone(),
                filename: delete_item.name.clone(),
                error_message: "File is not in managed zone - refusing to delete".to_string(),
            });
            continue;
        }

        // Delete file via device IO abstraction (relative path, backend handles resolution)
        match device_io.delete_file(&delete_item.local_path).await {
            Ok(_) => {
                // Successfully deleted
                if let Some(mut operation) = operation_manager.get_operation(&operation_id).await {
                    operation.files_completed += 1;
                    operation_manager
                        .update_operation(&operation_id, operation)
                        .await;
                }

                // Per-delete manifest update for dirty-resume support (Story 4.4)
                let id_to_remove = delete_item.jellyfin_id.clone();
                if let Err(e) = device_manager
                    .update_manifest(|m| {
                        m.synced_items.retain(|i| i.jellyfin_id != id_to_remove);
                    })
                    .await
                {
                    eprintln!("[Sync] Warning: per-delete manifest write failed: {}", e);
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

    // After all deletions (files), prune any resulting empty directories via DeviceIO
    let managed_subfolder = managed_path
        .strip_prefix(device_path)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();
    if let Err(e) = device_io.cleanup_empty_subdirs(&managed_subfolder).await {
        eprintln!("[Sync] Warning: directory cleanup failed: {}", e);
    }

    // Process ID changes (virtual adds: we don't download, just update manifest records)
    for id_change in &delta.id_changes {
        let synced_at = now_iso8601(); // Or we could try to preserve original synced_at if we wanted

        synced_items.push(crate::device::SyncedItem {
            jellyfin_id: id_change.new_jellyfin_id.clone(),
            name: id_change.name.clone(),
            album: id_change.album.clone(),
            artist: id_change.artist.clone(),
            local_path: id_change.old_local_path.clone(), // Keep existing path!
            size_bytes: id_change.size_bytes,
            synced_at,
            original_name: id_change.original_name.clone(), // Preserved from old manifest (AC #4)
            etag: id_change.etag.clone(),
        });

        // Update operation progress and cumulative bytes (an ID change is instantly completed)
        completed_bytes_arc
            .fetch_add(id_change.size_bytes, std::sync::atomic::Ordering::Relaxed);
        let cumulative = completed_bytes_arc.load(std::sync::atomic::Ordering::Relaxed);
        if let Some(mut operation) = operation_manager.get_operation(&operation_id).await {
            operation.files_completed += 1;
            operation.bytes_transferred = cumulative;
            operation_manager
                .update_operation(&operation_id, operation)
                .await;
        }

        // Per-ID-change manifest update for dirty-resume support (Story 4.4)
        // Remove old ID entry, add new ID entry atomically.
        let synced_item = synced_items.last().unwrap().clone();
        let id_to_remove = id_change.old_jellyfin_id.clone();
        if let Err(e) = device_manager
            .update_manifest(|m| {
                m.synced_items.retain(|i| i.jellyfin_id != id_to_remove);
                m.synced_items.push(synced_item);
            })
            .await
        {
            eprintln!("[Sync] Warning: per-ID-change manifest write failed: {}", e);
        }
    }

    // --- M3U Playlist Generation ---
    // Runs when there are playlist basket items OR manifest entries that need cleanup.
    // The auto-sync path calls execute_sync with delta.playlists = []; the inner guard
    // skips work when both sides are empty, avoiding unnecessary manifest reads.
    if let Some(mut manifest_snapshot) = device_manager.get_current_device().await {
        if !delta.playlists.is_empty() || !manifest_snapshot.playlists.is_empty() {
            let warnings = generate_m3u_files(
                &delta.playlists,
                device_path,
                &managed_path,
                &manifest_snapshot.synced_items.clone(),
                &mut manifest_snapshot,
                Arc::clone(&device_io),
            )
            .await;

            for w in &warnings {
                eprintln!("{}", w);
            }

            // Persist the updated playlists array back through the device manager.
            let updated_playlists = manifest_snapshot.playlists;
            if let Err(e) = device_manager
                .update_manifest(|m| {
                    m.playlists = updated_playlists;
                })
                .await
            {
                eprintln!("[M3U] Failed to persist manifest after M3U update: {}", e);
            }
        }
    }

    let mut device_warnings = device_io.take_warnings().await;
    if let Err(e) = device_io.end_sync_job().await {
        device_warnings.push(format!("[DeviceIO] Failed to end device sync job cleanly: {}", e));
    }
    if !device_warnings.is_empty() {
        if let Some(mut operation) = operation_manager.get_operation(&operation_id).await {
            operation.warnings.append(&mut device_warnings);
            operation_manager.update_operation(&operation_id, operation).await;
        }
    }

    Ok((synced_items, errors))
}

/// Buffers a byte stream into memory while reporting progress.
/// Used by `execute_sync` to download a file before writing via `DeviceIO`.
async fn buffer_stream<S>(
    mut stream: S,
    total_size: u64,
    on_progress: ProgressCallback,
) -> Result<Vec<u8>>
where
    S: futures::Stream<Item = std::result::Result<bytes::Bytes, reqwest::Error>> + Unpin,
{
    const MAX_FILE_BUFFER_BYTES: u64 = 2 * 1024 * 1024 * 1024; // 2 GB hard cap
    if total_size > MAX_FILE_BUFFER_BYTES {
        return Err(anyhow::anyhow!(
            "File too large to buffer ({} bytes > {} byte limit)",
            total_size,
            MAX_FILE_BUFFER_BYTES
        ));
    }
    let capacity = total_size.min(MAX_FILE_BUFFER_BYTES) as usize;
    let mut buffer = Vec::with_capacity(capacity);
    let mut bytes_written = 0u64;
    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.context("Failed to read chunk from stream")?;
        buffer.extend_from_slice(&chunk);
        bytes_written += chunk.len() as u64;
        if buffer.len() as u64 > MAX_FILE_BUFFER_BYTES {
            return Err(anyhow::anyhow!(
                "File stream exceeded {} byte buffer limit",
                MAX_FILE_BUFFER_BYTES
            ));
        }
        on_progress(bytes_written, total_size);
    }
    Ok(buffer)
}


/// Extracts the filename stem from a relative path for use as an EXTINF display label.
///
/// Example: `"Music/Artist/Album/01 - Track Name.flac"` → `"01 - Track Name"`
fn extract_display_name(rel_path: &str) -> &str {
    let path = std::path::Path::new(rel_path);
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(rel_path)
}

/// Generates, regenerates, or cleans up .m3u files for playlists in the sync basket.
///
/// Called once per sync run, after all file transfers complete.
/// Uses Write-Temp-Rename (atomic write) for all .m3u writes.
///
/// `device_path` is the device root (local_path in SyncedItem is relative to this).
/// `managed_path` is the music folder (e.g. `device_path/Music`) — .m3u files are written here,
/// and track paths in the .m3u are made relative to this folder.
async fn generate_m3u_files(
    playlist_items: &[PlaylistSyncItem],
    device_path: &Path,
    managed_path: &Path,
    all_synced_items: &[crate::device::SyncedItem],
    manifest: &mut crate::device::DeviceManifest,
    device_io: Arc<dyn crate::device_io::DeviceIO>,
) -> Vec<String> {
    // Subfolder prefix for computing device-relative paths (e.g. "Music")
    let managed_subfolder = managed_path
        .strip_prefix(device_path)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();
    let mut warnings: Vec<String> = Vec::new();

    // Build a lookup: jellyfin_id → local_path (relative to device_path)
    let path_lookup: HashMap<&str, &str> = all_synced_items
        .iter()
        .map(|i| (i.jellyfin_id.as_str(), i.local_path.as_str()))
        .collect();

    // Track which playlist jellyfin IDs are still active (for cleanup)
    let active_ids: HashSet<&str> = playlist_items
        .iter()
        .map(|p| p.jellyfin_id.as_str())
        .collect();

    // CLEANUP: remove .m3u for playlists no longer in basket
    let to_remove: Vec<crate::device::PlaylistManifestEntry> = manifest
        .playlists
        .iter()
        .filter(|e| !active_ids.contains(e.jellyfin_id.as_str()))
        .cloned()
        .collect();
    for entry in &to_remove {
        let rel_path = if managed_subfolder.is_empty() {
            entry.filename.clone()
        } else {
            format!("{}/{}", managed_subfolder, entry.filename)
        };
        match device_io.delete_file(&rel_path).await {
            Ok(()) => {
                println!("[M3U] Deleted removed playlist: {}", entry.filename);
            }
            Err(e) if e.downcast_ref::<std::io::Error>().map(|io| io.kind() == std::io::ErrorKind::NotFound).unwrap_or(false) => {
                // Already gone — still remove manifest entry
            }
            Err(e) => {
                warnings.push(format!("[M3U] Failed to delete {}: {}", entry.filename, e));
                continue; // Don't remove from manifest if file deletion failed
            }
        }
        manifest
            .playlists
            .retain(|e2| e2.jellyfin_id != entry.jellyfin_id);
    }

    // Track filenames committed this run to detect collisions across playlists
    let mut used_filenames: HashSet<String> = HashSet::new();

    // GENERATE / REGENERATE for each playlist in basket
    for playlist in playlist_items {
        // Build .m3u filename — fall back to jellyfin_id if name sanitizes to empty
        let sanitized_name = sanitize_path_component(&playlist.name);
        let base_name = if sanitized_name.is_empty() {
            playlist.jellyfin_id[..playlist.jellyfin_id.len().min(32)].to_string()
        } else {
            sanitized_name
        };
        let m3u_filename = {
            let candidate = truncate_filename(&base_name, "m3u", 255);
            if used_filenames.contains(&candidate) {
                // Two playlists produced the same sanitized name — disambiguate with a short ID tag
                let id_tag = &playlist.jellyfin_id[..8.min(playlist.jellyfin_id.len())];
                let tagged = format!("{} ({})", base_name, id_tag);
                let deduped = truncate_filename(&tagged, "m3u", 255);
                warnings.push(format!(
                    "[M3U] Filename collision for '{}', using '{}'",
                    playlist.name, deduped
                ));
                deduped
            } else {
                candidate
            }
        };
        used_filenames.insert(m3u_filename.clone());

        // Resolve which tracks are available; emit warnings for missing ones.
        // Only resolved tracks are written to the M3U and stored in track_ids — this ensures
        // the manifest accurately reflects file content and re-triggers a write if a previously
        // missing track becomes available on the next sync.
        let mut resolved_tracks: Vec<(&PlaylistTrackInfo, &str)> = Vec::new();
        for track in &playlist.tracks {
            match path_lookup.get(track.jellyfin_id.as_str()) {
                None => {
                    warnings.push(format!(
                        "[M3U] Track {} not in manifest — omitted from {}",
                        track.jellyfin_id, m3u_filename
                    ));
                }
                Some(rel_path) => {
                    resolved_tracks.push((track, rel_path));
                }
            }
        }

        if resolved_tracks.is_empty() {
            warnings.push(format!(
                "[M3U] No tracks resolved for playlist {} — skipping write",
                playlist.name
            ));
            continue;
        }

        let resolved_track_ids: Vec<String> =
            resolved_tracks.iter().map(|(t, _)| t.jellyfin_id.clone()).collect();

        // Determine if regeneration is needed (filename or resolved track list changed)
        let (needs_write, old_filename_opt) =
            match manifest.playlists.iter().find(|e| e.jellyfin_id == playlist.jellyfin_id) {
                None => (true, None),
                Some(e) => {
                    let changed = e.filename != m3u_filename || e.track_ids != resolved_track_ids;
                    (changed, Some(e.filename.clone()))
                }
            };

        if !needs_write {
            println!("[M3U] Playlist unchanged, skipping: {}", m3u_filename);
            continue;
        }

        // Build M3U content
        let mut lines: Vec<String> = vec!["#EXTM3U".to_string()];
        for (track, rel_path) in &resolved_tracks {
            let label = match &track.artist {
                Some(a) => format!("{} - {}", a, extract_display_name(rel_path)),
                None => extract_display_name(rel_path).to_string(),
            };
            lines.push(format!("#EXTINF:{},{}", track.run_time_seconds, label));
            // Make path relative to managed_path (where the .m3u lives).
            // local_path is relative to device_path (e.g. "Music/Artist/Album/track.flac").
            // Strip the managed subfolder prefix so the .m3u entry is just
            // "Artist/Album/track.flac" for a Rockbox/DAP player in the Music folder.
            let track_entry = device_path
                .join(rel_path)
                .strip_prefix(managed_path)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|_| rel_path.replace('\\', "/"));
            lines.push(track_entry);
        }

        let content = lines.join("\n") + "\n";

        // Write via device IO abstraction (handles Write-Temp-Rename internally)
        let rel_m3u = if managed_subfolder.is_empty() {
            m3u_filename.clone()
        } else {
            format!("{}/{}", managed_subfolder, m3u_filename)
        };
        match device_io.write_with_verify(&rel_m3u, content.as_bytes()).await {
            Ok(()) => {
                println!("[M3U] Wrote {}: {} tracks", m3u_filename, resolved_tracks.len());

                // Delete old file if the playlist was renamed
                if let Some(old_fn) = &old_filename_opt {
                    if *old_fn != m3u_filename {
                        let rel_old = if managed_subfolder.is_empty() {
                            old_fn.clone()
                        } else {
                            format!("{}/{}", managed_subfolder, old_fn)
                        };
                        if let Err(e) = device_io.delete_file(&rel_old).await {
                            warnings.push(format!(
                                "[M3U] Failed to delete old file {}: {}",
                                old_fn, e
                            ));
                        }
                    }
                }

                let now = now_iso8601();
                manifest
                    .playlists
                    .retain(|e| e.jellyfin_id != playlist.jellyfin_id);
                manifest.playlists.push(crate::device::PlaylistManifestEntry {
                    jellyfin_id: playlist.jellyfin_id.clone(),
                    filename: m3u_filename,
                    track_count: resolved_tracks.len() as u32,
                    track_ids: resolved_track_ids,
                    last_modified: now,
                });
            }
            Err(e) => {
                warnings.push(format!("[M3U] Failed to write {}: {}", m3u_filename, e));
            }
        }
    }

    warnings
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
            etag: i.etag.clone(),
        })
        .collect();

    // Initial deletes: manifest items not in desired set
    // AND build the metadata map in the same pass
    let mut deletes: Vec<SyncDeleteItem> = Vec::new();
    let mut delete_by_metadata: HashMap<(String, Option<String>, Option<String>), Vec<usize>> =
        HashMap::new();
    // Index original_name by jellyfin_id for ID-change preservation (AC #4 requirement)
    let original_name_by_id: HashMap<&str, Option<&str>> = manifest
        .synced_items
        .iter()
        .map(|i| (i.jellyfin_id.as_str(), i.original_name.as_deref()))
        .collect();

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
            delete_by_metadata
                .entry(key)
                .or_insert_with(Vec::new)
                .push(idx);
        }
    }

    // Find adds that match a delete by metadata (ID change detection)
    let mut matched_add_indices: HashSet<usize> = HashSet::new();
    let mut matched_delete_indices: HashSet<usize> = HashSet::new();
    let mut id_changes: Vec<SyncIdChangeItem> = Vec::new();

    for (add_idx, add) in adds.iter().enumerate() {
        let key = (
            add.name.to_lowercase(),
            add.album.as_ref().map(|a| a.to_lowercase()),
            add.artist.as_ref().map(|a| a.to_lowercase()),
        );

        if let Some(del_indices) = delete_by_metadata.get(&key) {
            // Find the first unmatched delete for this metadata
            if let Some(&del_idx) = del_indices
                .iter()
                .find(|&&idx| !matched_delete_indices.contains(&idx))
            {
                matched_add_indices.insert(add_idx);
                matched_delete_indices.insert(del_idx);

                let del = &deletes[del_idx];
                // Preserve original_name from the old manifest entry (AC #4: must not lose mapping)
                let preserved_original_name = original_name_by_id
                    .get(del.jellyfin_id.as_str())
                    .and_then(|&v| v)
                    .map(|s| s.to_string());
                id_changes.push(SyncIdChangeItem {
                    old_jellyfin_id: del.jellyfin_id.clone(),
                    new_jellyfin_id: add.jellyfin_id.clone(),
                    old_local_path: del.local_path.clone(),
                    name: add.name.clone(),
                    album: add.album.clone(),
                    artist: add.artist.clone(),
                    size_bytes: add.size_bytes,
                    etag: add.etag.clone(),
                    original_name: preserved_original_name,
                });
            }
        }
    }

    let unchanged: usize = desired_items
        .iter()
        .filter(|i| current_ids.contains(i.jellyfin_id.as_str()))
        .count();

    // Remove matched pairs — these are ID reassignments, not real adds/deletes
    let deletes: Vec<SyncDeleteItem> = deletes
        .into_iter()
        .enumerate()
        .filter(|(idx, _)| !matched_delete_indices.contains(idx))
        .map(|(_, d)| d)
        .collect();

    let adds: Vec<SyncAddItem> = adds
        .into_iter()
        .enumerate()
        .filter(|(idx, _)| !matched_add_indices.contains(idx))
        .map(|(_, a)| a)
        .collect();

    SyncDelta {
        adds,
        deletes,
        id_changes,
        unchanged,
        playlists: vec![],
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
            icon: None,
            version: "1.0".to_string(),
            managed_paths: vec!["Music".to_string()],
            synced_items: vec![],
            dirty: false,
            pending_item_ids: vec![],
            basket_items: vec![],
            auto_sync_on_connect: false,
            auto_fill: crate::device::AutoFillPrefs::default(),
            transcoding_profile_id: None,
            playlists: vec![],
            storage_id: None,
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
            original_name: None,
            etag: Some("test-etag".to_string()),
        }
    }

    fn make_test_item(
        name: &str,
        album_artist: Option<&str>,
        album: Option<&str>,
        index: Option<u32>,
        container: Option<&str>,
    ) -> crate::api::JellyfinItem {
        crate::api::JellyfinItem {
            id: "test-id".to_string(),
            name: name.to_string(),
            item_type: "Audio".to_string(),
            album: album.map(|s| s.to_string()),
            album_artist: album_artist.map(|s| s.to_string()),
            artists: None,
            index_number: index,
            container: container.map(|s| s.to_string()),
            production_year: None,
            recursive_item_count: None,
            cumulative_run_time_ticks: None,
            run_time_ticks: None,
            media_sources: None,
            etag: None,
            user_data: None,
            date_created: None,
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
            etag: Some("test-etag".to_string()),
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
        assert_eq!(delta.id_changes.len(), 0);
        assert_eq!(delta.unchanged, 0);
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
        assert_eq!(delta.id_changes.len(), 0);
        assert_eq!(delta.unchanged, 2);
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
        assert_eq!(delta.id_changes.len(), 0);
        assert_eq!(delta.unchanged, 1);
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
        assert_eq!(delta.id_changes.len(), 0);
        assert_eq!(delta.unchanged, 0);
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
        // The delete and add should be suppressed, moved to id_changes
        assert_eq!(delta.deletes.len(), 0);
        assert_eq!(delta.adds.len(), 0);
        assert_eq!(delta.id_changes.len(), 1);
        assert_eq!(delta.id_changes[0].new_jellyfin_id, "new-id-1");
        assert_eq!(delta.id_changes[0].old_jellyfin_id, "old-id-1");
        assert_eq!(delta.unchanged, 0);
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
            artists: None,
            index_number: Some(1),
            container: Some("flac".to_string()),
            production_year: None,
            recursive_item_count: None,
            cumulative_run_time_ticks: None,
            run_time_ticks: None,
            media_sources: None,
            etag: None,
            user_data: None,
            date_created: None,
        };

        let path = construct_file_path(&managed, &item).unwrap().path;
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
            artists: None,
            index_number: None,
            container: None,
            production_year: None,
            recursive_item_count: None,
            cumulative_run_time_ticks: None,
            run_time_ticks: None,
            media_sources: None,
            etag: None,
            user_data: None,
            date_created: None,
        };

        let path = construct_file_path(&managed, &item).unwrap().path;
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
        assert_eq!(
            sanitize_path_component("pipe|question?star*"),
            "pipe_question_star_"
        );
        assert_eq!(sanitize_path_component("ok chars 123"), "ok chars 123");
    }

    #[test]
    fn test_sanitize_path_component_trims_whitespace() {
        assert_eq!(sanitize_path_component("  trimmed  "), "trimmed");
    }

    #[test]
    fn test_sanitize_path_component_strips_trailing_dots() {
        // FAT32/Windows forbids folder/file names ending with dots
        assert_eq!(sanitize_path_component("Once upon a..."), "Once upon a");
        assert_eq!(sanitize_path_component("Album..."), "Album");
        assert_eq!(sanitize_path_component("no dots"), "no dots");
        assert_eq!(sanitize_path_component("mid.dot.ok"), "mid.dot.ok");
    }

    #[test]
    fn test_truncate_component_strips_trailing_dots_without_truncation() {
        // Short component (no truncation needed) must still have trailing dots stripped
        assert_eq!(truncate_component("Once upon a...", 255), "Once upon a");
        assert_eq!(truncate_component("Album...", 255), "Album");
        assert_eq!(truncate_component("no dots", 255), "no dots");
    }

    #[tokio::test]
    async fn test_sync_operation_manager_lifecycle() {
        let manager = SyncOperationManager::new();

        // Create operation
        let op = manager.create_operation("op-1".to_string(), 10).await;
        assert_eq!(op.status, SyncStatus::Running);
        assert_eq!(op.files_total, 10);
        assert_eq!(op.files_completed, 0);
        assert_eq!(op.bytes_transferred, 0);
        assert_eq!(op.total_bytes, 0);

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
        assert_eq!(delta.adds.len(), 0);
        assert_eq!(delta.id_changes.len(), 1);
    }

    // ===== Story 4.3 Tests =====

    #[test]
    fn test_truncate_component_short_name_unchanged() {
        let name = "A".repeat(255);
        let result = truncate_component(&name, 255);
        assert_eq!(result.chars().count(), 255);
        assert_eq!(result, name);
    }

    #[test]
    fn test_truncate_component_300_char_name() {
        let name = "A".repeat(300);
        let result = truncate_component(&name, 255);
        assert_eq!(result.chars().count(), 255);
    }

    #[test]
    fn test_truncate_component_trailing_dots_stripped() {
        // Build a string that is exactly 255 chars with trailing dots
        let base = "A".repeat(250);
        let name = format!("{}.....X", base); // 257 chars; after take(255): 250 A's + 5 dots
        let result = truncate_component(&name, 255);
        assert!(!result.ends_with('.'), "Trailing dots must be stripped");
        assert!(result.chars().count() <= 255);
    }

    #[test]
    fn test_truncate_component_trailing_spaces_stripped() {
        // Build a string that truncates to trailing spaces
        let base = "A".repeat(250);
        let name = format!("{}     X", base); // 257 chars; after take(255): 250 A's + 5 spaces
        let result = truncate_component(&name, 255);
        assert!(!result.ends_with(' '), "Trailing spaces must be stripped");
        assert!(result.chars().count() <= 255);
    }

    #[test]
    fn test_construct_file_path_short_name_no_original_name() {
        let managed = std::path::PathBuf::from("Music");
        let item = make_test_item(
            "Short Track",
            Some("Artist"),
            Some("Album"),
            Some(1),
            Some("flac"),
        );
        let result = construct_file_path(&managed, &item).unwrap();
        assert!(
            result.original_name.is_none(),
            "original_name must be None for short names"
        );
    }

    #[test]
    fn test_construct_file_path_long_filename_extension_preserved() {
        let long_track_name: String = "A".repeat(300);
        let managed = std::path::PathBuf::from("Music");
        let item = make_test_item(
            &long_track_name,
            Some("Artist"),
            Some("Album"),
            Some(1),
            Some("flac"),
        );
        let result = construct_file_path(&managed, &item).unwrap();

        let filename = result.path.file_name().unwrap().to_string_lossy();
        assert!(
            filename.ends_with(".flac"),
            "Extension must be .flac, got: {}",
            filename
        );
        assert!(
            filename.chars().count() <= 255,
            "Filename too long: {} chars",
            filename.chars().count()
        );
        assert!(
            result.original_name.is_some(),
            "original_name must be set when truncated"
        );
        assert_eq!(result.original_name.unwrap(), long_track_name);
    }

    #[test]
    fn test_construct_file_path_extension_override() {
        let managed = std::path::PathBuf::from("Music");
        let item = make_test_item(
            "K.",
            Some("Cigarettes After Sex"),
            Some("Cigarettes After Sex"),
            Some(1),
            Some("flac"),
        );

        let result = construct_file_path_with_extension(&managed, &item, Some("mp3")).unwrap();
        let filename = result.path.file_name().unwrap().to_string_lossy();

        assert_eq!(filename, "01 - K.mp3");
    }

    #[test]
    fn test_construct_file_path_long_album_artist_truncated() {
        let long_artist: String = "B".repeat(300);
        let long_album: String = "C".repeat(300);
        let managed = std::path::PathBuf::from("Music");
        let item = make_test_item(
            "Track",
            Some(&long_artist),
            Some(&long_album),
            Some(1),
            Some("mp3"),
        );
        let result = construct_file_path(&managed, &item).unwrap();

        let components: Vec<_> = result.path.components().collect();
        // path = Music / artist / album / filename
        // components[1] = artist, components[2] = album
        let artist_comp = components[1].as_os_str().to_string_lossy();
        let album_comp = components[2].as_os_str().to_string_lossy();
        assert!(
            artist_comp.chars().count() <= 255,
            "Artist component too long: {} chars",
            artist_comp.chars().count()
        );
        assert!(
            album_comp.chars().count() <= 255,
            "Album component too long: {} chars",
            album_comp.chars().count()
        );
    }

    // ===== Code Review Fix Tests =====

    #[test]
    fn test_truncate_component_all_dots_returns_fallback() {
        // All-dots string truncates and strips to empty → fallback "_"
        let dots = ".".repeat(300);
        let result = truncate_component(&dots, 255);
        assert_eq!(result, "_", "All-dots component must fall back to '_'");
    }

    #[test]
    fn test_truncate_component_all_spaces_returns_fallback() {
        // All-spaces string truncates and strips to empty → fallback "_"
        let spaces = " ".repeat(300);
        let result = truncate_component(&spaces, 255);
        assert_eq!(result, "_", "All-spaces component must fall back to '_'");
    }

    #[test]
    fn test_truncate_filename_pathological_extension_preserves_dot() {
        // Extension longer than max_len — must still return something with a dot
        let long_ext = "x".repeat(300);
        let result = truncate_filename("base", &long_ext, 255);
        assert!(
            result.starts_with('.'),
            "Result must start with '.' to preserve extension: {}",
            result
        );
        assert!(
            result.chars().count() <= 256,
            "Result should be close to limit: {} chars",
            result.chars().count()
        );
    }

    #[test]
    fn test_truncate_filename_pathological_does_not_drop_extension_entirely() {
        // Verify old bug is fixed: no extensionless filename returned
        let long_ext = "flac".repeat(70); // ~280 chars
        let result = truncate_filename("01 - Track", &long_ext, 255);
        assert!(
            result.contains('.'),
            "Extension dot must be present: {}",
            result
        );
    }

    #[test]
    fn test_calculate_delta_id_change_preserves_original_name() {
        let mut manifest = empty_manifest();
        manifest.synced_items = vec![{
            let mut item =
                make_synced_item("old-id", "My Song", Some("My Album"), Some("My Artist"));
            item.original_name = Some("My Very Long Song Name That Was Truncated".to_string());
            item
        }];

        let desired = vec![make_desired(
            "new-id",
            "My Song",
            Some("My Album"),
            Some("My Artist"),
        )];
        let delta = calculate_delta(&desired, &manifest);

        assert_eq!(delta.id_changes.len(), 1);
        assert_eq!(
            delta.id_changes[0].original_name,
            Some("My Very Long Song Name That Was Truncated".to_string()),
            "original_name must be preserved through ID changes"
        );
    }

    #[test]
    fn test_calculate_delta_id_change_no_original_name_stays_none() {
        let mut manifest = empty_manifest();
        manifest.synced_items = vec![make_synced_item(
            "old-id",
            "Short Song",
            Some("Album"),
            Some("Artist"),
        )];

        let desired = vec![make_desired(
            "new-id",
            "Short Song",
            Some("Album"),
            Some("Artist"),
        )];
        let delta = calculate_delta(&desired, &manifest);

        assert_eq!(delta.id_changes.len(), 1);
        assert!(
            delta.id_changes[0].original_name.is_none(),
            "original_name must stay None when no truncation occurred"
        );
    }

    #[test]
    fn test_synced_item_original_name_serializes_as_camel_case() {
        let item = crate::device::SyncedItem {
            jellyfin_id: "id1".to_string(),
            name: "Truncated Track".to_string(),
            album: None,
            artist: None,
            local_path: "Music/Track.flac".to_string(),
            size_bytes: 1000,
            synced_at: "2026-01-01".to_string(),
            original_name: Some("Very Long Original Track Name".to_string()),
            etag: None,
        };

        let value = serde_json::to_value(&item).unwrap();
        assert!(
            value.get("originalName").is_some(),
            "Field must serialize as 'originalName' (camelCase)"
        );
        assert_eq!(
            value["originalName"].as_str().unwrap(),
            "Very Long Original Track Name"
        );
        assert!(
            value.get("original_name").is_none(),
            "snake_case key must not appear"
        );
    }

    // ===== Story 4.7 Tests =====

    fn make_playlist_synced_item(id: &str, local_path: &str) -> crate::device::SyncedItem {
        crate::device::SyncedItem {
            jellyfin_id: id.to_string(),
            name: local_path.to_string(),
            album: None,
            artist: None,
            local_path: local_path.to_string(),
            size_bytes: 1_000_000,
            synced_at: "2026-04-01T00:00:00Z".to_string(),
            original_name: None,
            etag: None,
        }
    }

    #[tokio::test]
    async fn test_generate_m3u_basic() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let device_path = tmp_dir.path();
        let managed_path = device_path.join("Music");
        tokio::fs::create_dir_all(&managed_path).await.unwrap();

        let playlist_items = vec![
            PlaylistSyncItem {
                jellyfin_id: "pl1".to_string(),
                name: "My Playlist".to_string(),
                tracks: vec![
                    PlaylistTrackInfo {
                        jellyfin_id: "t1".to_string(),
                        artist: Some("Pink Floyd".to_string()),
                        run_time_seconds: 210,
                    },
                    PlaylistTrackInfo {
                        jellyfin_id: "t2".to_string(),
                        artist: Some("Pink Floyd".to_string()),
                        run_time_seconds: 180,
                    },
                    PlaylistTrackInfo {
                        jellyfin_id: "t3".to_string(),
                        artist: None,
                        run_time_seconds: -1,
                    },
                ],
            },
            PlaylistSyncItem {
                jellyfin_id: "pl2".to_string(),
                name: "Second Playlist".to_string(),
                tracks: vec![PlaylistTrackInfo {
                    jellyfin_id: "t4".to_string(),
                    artist: Some("Artist".to_string()),
                    run_time_seconds: 300,
                }],
            },
        ];

        let all_synced = vec![
            make_playlist_synced_item("t1", "Music/Pink Floyd/The Wall/01 - In the Flesh.flac"),
            make_playlist_synced_item("t2", "Music/Pink Floyd/The Wall/02 - The Thin Ice.flac"),
            make_playlist_synced_item("t3", "Music/Various/Album/03 - Unknown.mp3"),
            make_playlist_synced_item("t4", "Music/Artist/Album/01 - Track.flac"),
        ];

        let mut manifest = empty_manifest();
        let device_io = std::sync::Arc::new(crate::device_io::MscBackend::new(device_path.to_path_buf()));
        let warnings = generate_m3u_files(
            &playlist_items,
            device_path,
            &managed_path,
            &all_synced,
            &mut manifest,
            device_io,
        )
        .await;

        // No warnings expected (all tracks resolved)
        assert!(warnings.is_empty(), "Expected no warnings, got: {:?}", warnings);

        // .m3u files should be in the Music folder, not the device root
        let m3u1 = managed_path.join("My Playlist.m3u");
        let m3u2 = managed_path.join("Second Playlist.m3u");
        assert!(m3u1.exists(), "My Playlist.m3u should exist in Music/");
        assert!(m3u2.exists(), "Second Playlist.m3u should exist in Music/");
        assert!(!device_path.join("My Playlist.m3u").exists(), ".m3u must NOT be at device root");

        // Check content — paths are relative to Music/, so no "Music/" prefix
        let content1 = tokio::fs::read_to_string(&m3u1).await.unwrap();
        assert!(content1.starts_with("#EXTM3U\n"), "Must start with #EXTM3U");
        assert!(content1.contains("#EXTINF:210,Pink Floyd - 01 - In the Flesh"));
        assert!(content1.contains("Pink Floyd/The Wall/01 - In the Flesh.flac"));
        assert!(!content1.contains("Music/Pink Floyd"), "Path must NOT include Music/ prefix");
        assert!(content1.contains("#EXTINF:-1,03 - Unknown"), "No-artist track uses filename only");

        // manifest.playlists should have two entries
        assert_eq!(manifest.playlists.len(), 2);
        let entry1 = manifest.playlists.iter().find(|e| e.jellyfin_id == "pl1").unwrap();
        assert_eq!(entry1.track_count, 3);
        assert_eq!(entry1.track_ids, vec!["t1", "t2", "t3"]);
    }

    #[tokio::test]
    async fn test_generate_m3u_no_rewrite_if_unchanged() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let device_path = tmp_dir.path();
        let managed_path = device_path.join("Music");
        tokio::fs::create_dir_all(&managed_path).await.unwrap();

        let playlist_items = vec![PlaylistSyncItem {
            jellyfin_id: "pl1".to_string(),
            name: "Stable Playlist".to_string(),
            tracks: vec![PlaylistTrackInfo {
                jellyfin_id: "t1".to_string(),
                artist: None,
                run_time_seconds: 120,
            }],
        }];
        let all_synced = vec![make_playlist_synced_item("t1", "Music/A/B/01 - Song.flac")];

        let mut manifest = empty_manifest();
        let device_io: std::sync::Arc<dyn crate::device_io::DeviceIO> = std::sync::Arc::new(crate::device_io::MscBackend::new(device_path.to_path_buf()));

        // First call — writes file
        generate_m3u_files(
            &playlist_items,
            device_path,
            &managed_path,
            &all_synced,
            &mut manifest,
            std::sync::Arc::clone(&device_io),
        )
        .await;
        let m3u_path = managed_path.join("Stable Playlist.m3u");
        assert!(m3u_path.exists());

        let mtime1 = tokio::fs::metadata(&m3u_path)
            .await
            .unwrap()
            .modified()
            .unwrap();

        // Wait a moment to ensure mtime would differ if rewritten
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Second call with same track_ids — should NOT rewrite
        generate_m3u_files(
            &playlist_items,
            device_path,
            &managed_path,
            &all_synced,
            &mut manifest,
            std::sync::Arc::clone(&device_io),
        )
        .await;

        let mtime2 = tokio::fs::metadata(&m3u_path)
            .await
            .unwrap()
            .modified()
            .unwrap();

        assert_eq!(mtime1, mtime2, "File must not be rewritten if track list unchanged");
    }

    #[tokio::test]
    async fn test_generate_m3u_cleanup() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let device_path = tmp_dir.path();
        let managed_path = device_path.join("Music");
        tokio::fs::create_dir_all(&managed_path).await.unwrap();

        // Pre-populate manifest with an entry and a corresponding .m3u file in Music/
        let m3u_path = managed_path.join("Old Playlist.m3u");
        tokio::fs::write(&m3u_path, b"#EXTM3U\n").await.unwrap();

        let mut manifest = empty_manifest();
        manifest.playlists.push(crate::device::PlaylistManifestEntry {
            jellyfin_id: "old-pl".to_string(),
            filename: "Old Playlist.m3u".to_string(),
            track_count: 1,
            track_ids: vec!["t1".to_string()],
            last_modified: "2026-01-01T00:00:00Z".to_string(),
        });

        // Call with empty playlist_items (playlist removed from basket)
        let device_io = std::sync::Arc::new(crate::device_io::MscBackend::new(device_path.to_path_buf()));
        let warnings =
            generate_m3u_files(&[], device_path, &managed_path, &[], &mut manifest, device_io).await;

        assert!(warnings.is_empty(), "No warnings expected: {:?}", warnings);
        assert!(!m3u_path.exists(), "Old .m3u file should have been deleted from Music/");
        assert!(
            manifest.playlists.is_empty(),
            "Manifest playlists entry should have been removed"
        );
    }

    #[tokio::test]
    async fn test_generate_m3u_missing_track_omitted() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let device_path = tmp_dir.path();
        let managed_path = device_path.join("Music");
        tokio::fs::create_dir_all(&managed_path).await.unwrap();

        let playlist_items = vec![PlaylistSyncItem {
            jellyfin_id: "pl1".to_string(),
            name: "Partial Playlist".to_string(),
            tracks: vec![
                PlaylistTrackInfo {
                    jellyfin_id: "t1".to_string(),
                    artist: Some("Artist A".to_string()),
                    run_time_seconds: 200,
                },
                PlaylistTrackInfo {
                    jellyfin_id: "t2-missing".to_string(), // not in synced items
                    artist: Some("Artist B".to_string()),
                    run_time_seconds: 150,
                },
                PlaylistTrackInfo {
                    jellyfin_id: "t3".to_string(),
                    artist: None,
                    run_time_seconds: 90,
                },
            ],
        }];

        // Only t1 and t3 are in synced items — t2 is missing
        let all_synced = vec![
            make_playlist_synced_item("t1", "Music/A/01 - Track1.flac"),
            make_playlist_synced_item("t3", "Music/C/03 - Track3.flac"),
        ];

        let mut manifest = empty_manifest();
        let device_io = std::sync::Arc::new(crate::device_io::MscBackend::new(device_path.to_path_buf()));
        let warnings = generate_m3u_files(
            &playlist_items,
            device_path,
            &managed_path,
            &all_synced,
            &mut manifest,
            device_io,
        )
        .await;

        // One warning for the missing track
        assert_eq!(warnings.len(), 1, "Expected 1 warning for missing track");
        assert!(warnings[0].contains("t2-missing"), "Warning should name the missing track");

        // .m3u should exist in Music/ with 2 tracks (t1 and t3)
        let m3u_path = managed_path.join("Partial Playlist.m3u");
        assert!(m3u_path.exists());

        let content = tokio::fs::read_to_string(&m3u_path).await.unwrap();
        let extinf_count = content.lines().filter(|l| l.starts_with("#EXTINF")).count();
        assert_eq!(extinf_count, 2, "M3U should contain exactly 2 tracks");

        let manifest_entry = manifest.playlists.iter().find(|e| e.jellyfin_id == "pl1").unwrap();
        assert_eq!(manifest_entry.track_count, 2, "track_count should be 2 (only resolved tracks)");
    }
}

