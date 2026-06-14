use anyhow::{Context, Result};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::RwLock;

use crate::device::{DeviceManifest, SyncedItem};
use crate::providers::{MediaProvider, TranscodeProfile};

pub const DESTRUCTIVE_CLEANUP_THRESHOLD: usize = 25;

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

#[derive(Debug, Clone, Copy)]
struct TransferTiming {
    elapsed_ms: f64,
    speed_mb_s: f64,
}

fn transfer_timing(size_bytes: u64, elapsed: Duration) -> TransferTiming {
    let elapsed_secs = elapsed.as_secs_f64();
    let speed_mb_s = if elapsed_secs > 0.0 {
        size_bytes as f64 / elapsed_secs / 1_000_000.0
    } else {
        0.0
    };

    TransferTiming {
        elapsed_ms: elapsed_secs * 1000.0,
        speed_mb_s,
    }
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
    pub provider_album_id: Option<String>,
    pub provider_content_type: Option<String>,
    pub provider_suffix: Option<String>,
    /// Current server-side bitrate in bps. Used to detect quality upgrades since last sync.
    pub original_bitrate: Option<u32>,
    pub track_number: Option<u32>,
    /// Originating server UUID (Story 2.11), set during per-server delta calc.
    pub server_id: Option<String>,
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
    #[serde(default)]
    pub provider_album_id: Option<String>,
    #[serde(default)]
    pub provider_content_type: Option<String>,
    #[serde(default)]
    pub provider_suffix: Option<String>,
    #[serde(default)]
    pub original_bitrate: Option<u32>,
    #[serde(default)]
    pub track_number: Option<u32>,
    #[serde(default)]
    pub reason_code: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    /// Originating server UUID (Story 2.11). Drives multi-provider download
    /// routing in execute; `None` for single-server / legacy items.
    #[serde(default)]
    pub server_id: Option<String>,
    /// Story 13.1: auto-fill rotation-tier index (string) when this add came from a Memory-tiered
    /// slot. Survives the delta round-trip to sync-execute, where it is recorded into
    /// `autofill_history.tier`. `None` for manual items and non-tiered fills.
    #[serde(default)]
    pub tier: Option<String>,
}

/// An item to be deleted from the device.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SyncDeleteItem {
    pub jellyfin_id: String,
    pub local_path: String,
    pub name: String,
    #[serde(default)]
    pub reason_code: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
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
    #[serde(default)]
    pub provider_album_id: Option<String>,
    #[serde(default)]
    pub provider_content_type: Option<String>,
    #[serde(default)]
    pub provider_suffix: Option<String>,
    /// Preserved from the old manifest entry — set if the filename was previously truncated.
    #[serde(default)]
    pub original_name: Option<String>,
    #[serde(default)]
    pub reason_code: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    /// Originating server UUID (Story 2.11), preserved across id changes.
    #[serde(default)]
    pub source_server_id: Option<String>,
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

fn change_reason(code: &str) -> String {
    match code {
        "new-selection" => "new selection".to_string(),
        "removed-selection" => "removed from sync selection".to_string(),
        "transcoding-profile-change" => "transcoding profile changed".to_string(),
        "music-folder-change" => "music folder changed".to_string(),
        "bitrate-increase" => "source bitrate increased".to_string(),
        "bitrate-missing" => "previous sync did not record source bitrate".to_string(),
        "device-file-missing" => "device file is missing".to_string(),
        "server-id-change" => "server item ID changed".to_string(),
        "force-sync" => "force sync requested".to_string(),
        other => other.replace('-', " "),
    }
}

fn bitrate_stale_reason(server: Option<u32>, local: Option<u32>) -> Option<&'static str> {
    match (server, local) {
        (Some(server), Some(local)) if server > local => Some("bitrate-increase"),
        _ => None,
    }
}

fn annotate_add(mut item: SyncAddItem, code: &str) -> SyncAddItem {
    item.reason_code = Some(code.to_string());
    item.reason = Some(change_reason(code));
    item
}

fn annotate_delete(mut item: SyncDeleteItem, code: &str) -> SyncDeleteItem {
    item.reason_code = Some(code.to_string());
    item.reason = Some(change_reason(code));
    item
}

fn annotate_id_change(mut item: SyncIdChangeItem, code: &str) -> SyncIdChangeItem {
    item.reason_code = Some(code.to_string());
    item.reason = Some(change_reason(code));
    item
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SyncReasonSummary {
    pub reason_code: String,
    pub reason: String,
    pub count: usize,
}

pub fn change_reason_summary(delta: &SyncDelta) -> Vec<SyncReasonSummary> {
    let mut counts: HashMap<String, usize> = HashMap::new();

    let delete_reasons_by_id: HashMap<&str, &str> = delta
        .deletes
        .iter()
        .filter_map(|item| {
            item.reason_code
                .as_deref()
                .map(|code| (item.jellyfin_id.as_str(), code))
        })
        .collect();
    let mut paired_delete_ids: HashSet<&str> = HashSet::new();

    for add in &delta.adds {
        let Some(code) = add.reason_code.as_deref() else {
            continue;
        };
        if delete_reasons_by_id.contains_key(add.jellyfin_id.as_str()) {
            paired_delete_ids.insert(add.jellyfin_id.as_str());
        }
        *counts.entry(code.to_string()).or_insert(0) += 1;
    }
    for delete in &delta.deletes {
        if paired_delete_ids.contains(delete.jellyfin_id.as_str()) {
            continue;
        }
        if let Some(code) = delete.reason_code.as_deref() {
            *counts.entry(code.to_string()).or_insert(0) += 1;
        }
    }
    for id_change in &delta.id_changes {
        if let Some(code) = id_change.reason_code.as_deref() {
            *counts.entry(code.to_string()).or_insert(0) += 1;
        }
    }

    let mut summary: Vec<SyncReasonSummary> = counts
        .into_iter()
        .map(|(reason_code, count)| SyncReasonSummary {
            reason: change_reason(&reason_code),
            reason_code,
            count,
        })
        .collect();
    summary.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.reason.cmp(&right.reason))
    });
    summary
}

pub fn format_change_reason_summary(delta: &SyncDelta) -> String {
    let summary = change_reason_summary(delta);
    if summary.is_empty() {
        return "none".to_string();
    }
    summary
        .iter()
        .map(|entry| format!("{} {}", entry.count, entry.reason))
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn format_id_change_diagnostics(delta: &SyncDelta, limit: usize) -> String {
    if delta.id_changes.is_empty() || limit == 0 {
        return "none".to_string();
    }

    let shown = delta
        .id_changes
        .iter()
        .take(limit)
        .map(|change| {
            format!(
                "{} -> {} name={:?} album={:?} artist={:?} provider_album_id={:?} format={:?}/{:?} source_size={} old_path={:?}",
                change.old_jellyfin_id,
                change.new_jellyfin_id,
                change.name,
                change.album,
                change.artist,
                change.provider_album_id,
                change.provider_content_type,
                change.provider_suffix,
                change.size_bytes,
                change.old_local_path
            )
        })
        .collect::<Vec<_>>()
        .join("; ");

    let omitted = delta.id_changes.len().saturating_sub(limit);
    if omitted > 0 {
        format!("{shown}; ... {omitted} more")
    } else {
        shown
    }
}

fn compatible_optional_eq<T: PartialEq>(left: &Option<T>, right: &Option<T>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left == right,
        _ => true,
    }
}

fn id_change_candidate_matches(add: &SyncAddItem, old: &SyncedItem) -> bool {
    compatible_optional_eq(&add.provider_album_id, &old.provider_album_id)
        && compatible_optional_eq(&add.provider_content_type, &old.provider_content_type)
        && compatible_optional_eq(&add.provider_suffix, &old.provider_suffix)
        && compatible_optional_eq(&add.track_number, &old.track_number)
}

async fn cleanup_replaced_file_after_write(
    delete_item: &SyncDeleteItem,
    new_local_path: &str,
    device_path: &Path,
    managed_path: &Path,
    managed_subfolder: Option<&str>,
    is_mtp: bool,
    owned_manifest_paths: &HashSet<String>,
    device_io: &Arc<dyn crate::device_io::DeviceIO>,
    operation_manager: &Arc<SyncOperationManager>,
    operation_id: &str,
) -> Option<SyncFileError> {
    if delete_item.local_path == new_local_path {
        if let Some(mut operation) = operation_manager.get_operation(operation_id).await {
            operation.files_completed += 1;
            operation_manager
                .update_operation(operation_id, operation)
                .await;
        }
        return None;
    }

    if let Err(error_message) = validate_delete_path_for_managed_zone(
        device_path,
        managed_path,
        managed_subfolder,
        &delete_item.local_path,
        is_mtp,
        owned_manifest_paths,
    ) {
        return Some(SyncFileError {
            jellyfin_id: delete_item.jellyfin_id.clone(),
            filename: delete_item.name.clone(),
            error_message,
        });
    }

    match device_io.delete_file(&delete_item.local_path).await {
        Ok(_) => {
            if let Some(mut operation) = operation_manager.get_operation(operation_id).await {
                operation.files_completed += 1;
                operation_manager
                    .update_operation(operation_id, operation)
                    .await;
            }
            None
        }
        Err(e) if is_missing_delete_error(&e) => {
            if let Some(mut operation) = operation_manager.get_operation(operation_id).await {
                operation.files_completed += 1;
                operation_manager
                    .update_operation(operation_id, operation)
                    .await;
            }
            None
        }
        Err(e) => Some(SyncFileError {
            jellyfin_id: delete_item.jellyfin_id.clone(),
            filename: delete_item.name.clone(),
            error_message: format!("Failed to remove replaced file: {}", e),
        }),
    }
}

/// Status of a sync operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum SyncStatus {
    Running,
    Complete,
    Failed,
    #[serde(rename = "cancelled")]
    Cancelled,
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

/// RAII guard that releases the pipeline lock when dropped.
/// Obtained via [`SyncOperationManager::try_start_pipeline`].
pub struct PipelineGuard(Arc<AtomicBool>);
impl Drop for PipelineGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// Manager for tracking active sync operations in memory.
pub struct SyncOperationManager {
    operations: Arc<RwLock<HashMap<String, SyncOperation>>>,
    /// True while a sync pipeline (delta calculation or execution) is active.
    /// Covers the window between pipeline start and the first `create_operation` call,
    /// where `has_active_operation` would otherwise return false.
    pipeline_active: Arc<AtomicBool>,
    /// Per-operation cancellation flags. Set to `true` by `request_cancel`; polled by
    /// the sync loop between files via `is_cancelled`. Never removed — old entries for
    /// completed operations are harmless and naturally sized (one AtomicBool per UUID).
    cancel_tokens: Arc<RwLock<HashMap<String, Arc<AtomicBool>>>>,
}

impl SyncOperationManager {
    pub fn new() -> Self {
        Self {
            operations: Arc::new(RwLock::new(HashMap::new())),
            pipeline_active: Arc::new(AtomicBool::new(false)),
            cancel_tokens: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Atomically claim the sync pipeline. Returns a [`PipelineGuard`] that releases
    /// the lock on drop. Returns `None` if a pipeline is already active — the caller
    /// must treat this as a concurrency conflict and abort.
    pub fn try_start_pipeline(&self) -> Option<PipelineGuard> {
        let flag = Arc::clone(&self.pipeline_active);
        flag.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| PipelineGuard(flag))
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
        ops.insert(operation_id.clone(), operation.clone());
        drop(ops);

        let mut tokens = self.cancel_tokens.write().await;
        tokens.insert(operation_id, Arc::new(AtomicBool::new(false)));
        operation
    }

    /// Signals the sync loop for the given operation to stop after the current file.
    /// Returns `true` if the operation was found (whether running or already terminal),
    /// `false` if the operation ID is unknown.
    pub async fn request_cancel(&self, id: &str) -> bool {
        let tokens = self.cancel_tokens.read().await;
        if let Some(token) = tokens.get(id) {
            token.store(true, Ordering::Release);
            true
        } else {
            false
        }
    }

    /// Returns `true` if cancellation has been requested for the given operation.
    pub async fn is_cancelled(&self, id: &str) -> bool {
        let tokens = self.cancel_tokens.read().await;
        tokens
            .get(id)
            .map(|t| t.load(Ordering::Acquire))
            .unwrap_or(false)
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
        // Check pipeline flag first (covers the delta-calculation phase before any
        // operation is registered) then fall back to the operations map.
        self.pipeline_active.load(Ordering::Acquire)
            || self.get_active_operation_id().await.is_some()
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
                return Err(anyhow::anyhow!(
                    "Resulting path is too long for Windows MAX_PATH ({}), even after minimal truncation: {}",
                    WINDOWS_MAX_PATH,
                    path.display()
                ));
            }
        }

        return Ok(PathConstructionResult {
            path,
            original_name,
        });
    }
}

fn construct_desired_file_path(
    managed_path: &Path,
    item: &SyncAddItem,
    extension_override: Option<&str>,
) -> Result<PathConstructionResult> {
    let artist = item.artist.as_deref().unwrap_or("Unknown Artist");
    let album = item.album.as_deref().unwrap_or("Unknown Album");
    let track_name = &item.name;
    let extension = extension_override
        .or(item.provider_suffix.as_deref())
        .unwrap_or("mp3");

    let artist_clean = sanitize_path_component(artist);
    let album_clean = sanitize_path_component(album);
    let track_name_clean = sanitize_path_component(track_name);
    let mut current_max_component = MAX_PATH_COMPONENT_LEN;

    loop {
        let artist_final = truncate_component(&artist_clean, current_max_component);
        let album_final = truncate_component(&album_clean, current_max_component);
        let track_num = item
            .track_number
            .map(|n| format!("{:02}", n))
            .unwrap_or_else(|| "00".to_string());
        let filename_base = format!("{} - {}", track_num, track_name_clean);
        let filename_candidate = format!("{}.{}", filename_base, extension);
        let (filename, original_name) =
            if filename_candidate.chars().count() > current_max_component {
                let truncated = truncate_filename(&filename_base, extension, current_max_component);
                (truncated, Some(item.name.clone()))
            } else {
                (filename_candidate, None)
            };
        let path = managed_path
            .join(&artist_final)
            .join(&album_final)
            .join(&filename);
        let approx_abs_len = match path.canonicalize() {
            Ok(p) => p.to_string_lossy().chars().count(),
            Err(_) => match std::env::current_dir() {
                Ok(cwd) => cwd.join(&path).to_string_lossy().chars().count(),
                Err(_) => path.to_string_lossy().chars().count() + 30,
            },
        };
        if approx_abs_len > WINDOWS_MAX_PATH {
            if current_max_component > 30 {
                current_max_component = (current_max_component * 3) / 4;
                continue;
            }
            return Err(anyhow::anyhow!(
                "Resulting path is too long for Windows MAX_PATH ({}), even after minimal truncation: {}",
                WINDOWS_MAX_PATH,
                path.display()
            ));
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
        "Name": format!("HifiMule-Forced-{}", container),
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

#[derive(Debug, Clone, Default)]
struct AudioCompatibilityProfile {
    direct_formats: Vec<AudioFormatRequirement>,
    output_formats: Vec<AudioFormatRequirement>,
    transcode_profile: Option<TranscodeProfile>,
}

#[derive(Debug, Clone, Default)]
struct AudioFormat {
    containers: HashSet<String>,
    codecs: HashSet<String>,
    extension: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct AudioFormatRequirement {
    containers: HashSet<String>,
    codecs: HashSet<String>,
}

impl AudioFormat {
    fn is_empty(&self) -> bool {
        self.containers.is_empty() && self.codecs.is_empty()
    }
}

impl AudioFormatRequirement {
    fn is_empty(&self) -> bool {
        self.containers.is_empty() && self.codecs.is_empty()
    }

    fn matches(&self, format: &AudioFormat) -> bool {
        let container_matches =
            self.containers.is_empty() || !format.containers.is_disjoint(&self.containers);
        let codec_matches = self.codecs.is_empty() || !format.codecs.is_disjoint(&self.codecs);
        container_matches && (codec_matches || self.matches_ambiguous_mp4_audio(format))
    }

    fn matches_ambiguous_mp4_audio(&self, format: &AudioFormat) -> bool {
        if !format.codecs.is_empty() || self.codecs.is_empty() {
            return false;
        }
        let source_is_mp4_audio =
            format.containers.contains("m4a") || format.containers.contains("mp4");
        let requirement_is_mp4_audio =
            self.containers.contains("m4a") || self.containers.contains("mp4");
        let requirement_accepts_common_mp4_audio = self
            .codecs
            .iter()
            .all(|codec| matches!(codec.as_str(), "aac" | "alac"));

        source_is_mp4_audio && requirement_is_mp4_audio && requirement_accepts_common_mp4_audio
    }
}

impl AudioCompatibilityProfile {
    fn is_constrained(&self) -> bool {
        !self.output_formats.is_empty() || self.transcode_profile.is_some()
    }

    fn source_is_direct_compatible(&self, source: &AudioFormat) -> bool {
        if !self.is_constrained() {
            return true;
        }
        !source.is_empty()
            && self
                .direct_formats
                .iter()
                .any(|requirement| requirement.matches(source))
    }

    fn output_is_compatible(&self, output: &AudioFormat) -> bool {
        if !self.is_constrained() {
            return true;
        }
        !output.is_empty()
            && self
                .output_formats
                .iter()
                .any(|requirement| requirement.matches(output))
    }

    fn transcode_target_label(&self) -> String {
        self.transcode_profile
            .as_ref()
            .and_then(|profile| profile.container.as_deref())
            .unwrap_or("requested profile")
            .to_string()
    }
}

fn audio_compatibility_profile(
    device_profile: Option<&serde_json::Value>,
    preferred_audio_container: Option<&str>,
) -> AudioCompatibilityProfile {
    if let Some(profile) = device_profile {
        let mut direct_formats = Vec::new();
        if let Some(profiles) = profile["DirectPlayProfiles"].as_array() {
            for direct in profiles {
                if direct["Type"]
                    .as_str()
                    .is_some_and(|kind| !kind.eq_ignore_ascii_case("Audio"))
                {
                    continue;
                }
                if let Some(requirement) =
                    audio_requirement(direct["Container"].as_str(), direct["AudioCodec"].as_str())
                {
                    direct_formats.push(requirement);
                }
            }
        }

        let transcode_profile = transcode_profile_from_device_profile(profile);
        let mut output_formats = direct_formats.clone();
        if let Some(profile) = &transcode_profile
            && let Some(requirement) =
                audio_requirement(profile.container.as_deref(), profile.audio_codec.as_deref())
        {
            output_formats.push(requirement);
        }

        return AudioCompatibilityProfile {
            direct_formats,
            output_formats,
            transcode_profile,
        };
    }

    if let Some(container) = preferred_audio_container {
        let direct_requirement: Vec<AudioFormatRequirement> =
            audio_requirement(Some(container), Some(container))
                .into_iter()
                .collect();
        let output_formats = direct_requirement.clone();
        return AudioCompatibilityProfile {
            direct_formats: direct_requirement,
            output_formats,
            transcode_profile: Some(TranscodeProfile {
                container: Some(container.to_string()),
                audio_codec: Some(container.to_string()),
                max_bitrate_kbps: Some(if container.eq_ignore_ascii_case("mp3") {
                    320
                } else {
                    256
                }),
            }),
        };
    }

    AudioCompatibilityProfile::default()
}

fn transcode_profile_from_device_profile(profile: &serde_json::Value) -> Option<TranscodeProfile> {
    let transcode = profile["TranscodingProfiles"]
        .as_array()?
        .iter()
        .find(|candidate| {
            candidate["Type"]
                .as_str()
                .is_none_or(|kind| kind.eq_ignore_ascii_case("Audio"))
        })?;
    let container = transcode["Container"]
        .as_str()
        .map(|container| container.to_string());
    let audio_codec = transcode["AudioCodec"]
        .as_str()
        .map(|codec| codec.to_string());

    Some(TranscodeProfile {
        container,
        audio_codec,
        max_bitrate_kbps: profile_bitrate_kbps(profile),
    })
}

fn profile_bitrate_kbps(profile: &serde_json::Value) -> Option<u32> {
    let bitrate = profile["MusicStreamingTranscodingBitrate"]
        .as_u64()
        .or_else(|| profile["MaxStreamingBitrate"].as_u64())?;
    if bitrate >= 1000 {
        Some((bitrate / 1000) as u32)
    } else {
        Some(bitrate as u32)
    }
}

fn provider_audio_format(suffix: Option<&str>, content_type: Option<&str>) -> AudioFormat {
    let mut format = AudioFormat::default();
    if let Some(suffix) = suffix {
        add_source_suffix_format(&mut format, suffix);
        format.extension = clean_audio_extension(suffix);
    }
    if let Some(content_type) = content_type {
        add_content_type_format(&mut format, content_type);
        if format.extension.is_none() {
            format.extension = extension_from_content_type(content_type).map(str::to_string);
        }
    }
    format
}

fn audio_requirement(
    container: Option<&str>,
    codec: Option<&str>,
) -> Option<AudioFormatRequirement> {
    let mut requirement = AudioFormatRequirement::default();
    if let Some(container) = container {
        add_audio_container_keys(&mut requirement.containers, container);
    }
    if let Some(codec) = codec {
        add_audio_codec_keys(&mut requirement.codecs, codec);
    }
    (!requirement.is_empty()).then_some(requirement)
}

fn add_source_suffix_format(format: &mut AudioFormat, value: &str) {
    for part in value.split(',') {
        let Some(key) = normalized_audio_key(part) else {
            continue;
        };
        add_audio_container_key(&mut format.containers, &key);
        if is_self_describing_audio_key(&key) {
            add_audio_codec_key(&mut format.codecs, &key);
        }
    }
}

fn add_content_type_format(format: &mut AudioFormat, value: &str) {
    for part in value.split(',') {
        let Some(key) = normalized_audio_key(part) else {
            continue;
        };
        match key.as_str() {
            "mp3" | "flac" | "aac" | "opus" | "wav" => {
                add_audio_container_key(&mut format.containers, &key);
                add_audio_codec_key(&mut format.codecs, &key);
            }
            "m4a" | "mp4" | "ogg" | "oga" => {
                add_audio_container_key(&mut format.containers, &key);
            }
            "vorbis" => {
                add_audio_codec_key(&mut format.codecs, &key);
            }
            _ => {}
        }
    }
}

fn add_audio_container_keys(keys: &mut HashSet<String>, value: &str) {
    for part in value.split(',') {
        if let Some(key) = normalized_audio_key(part) {
            add_audio_container_key(keys, &key);
        }
    }
}

fn add_audio_codec_keys(keys: &mut HashSet<String>, value: &str) {
    for part in value.split(',') {
        if let Some(key) = normalized_audio_key(part) {
            add_audio_codec_key(keys, &key);
        }
    }
}

fn add_audio_container_key(keys: &mut HashSet<String>, key: &str) {
    match key {
        "mp3" | "mpeg" => {
            keys.insert("mp3".to_string());
        }
        "flac" | "x-flac" => {
            keys.insert("flac".to_string());
        }
        "mp4" | "m4a" => {
            keys.insert("mp4".to_string());
            keys.insert("m4a".to_string());
        }
        "ogg" | "oga" => {
            keys.insert("ogg".to_string());
            keys.insert("oga".to_string());
        }
        "aac" => {
            keys.insert("aac".to_string());
        }
        "opus" => {
            keys.insert("opus".to_string());
        }
        "wav" | "wave" | "audio/wav" | "audio/x-wav" => {
            keys.insert("wav".to_string());
        }
        other => {
            keys.insert(other.to_string());
        }
    }
}

fn add_audio_codec_key(keys: &mut HashSet<String>, key: &str) {
    match key {
        "mp3" | "mpeg" => {
            keys.insert("mp3".to_string());
        }
        "flac" | "x-flac" => {
            keys.insert("flac".to_string());
        }
        "aac" => {
            keys.insert("aac".to_string());
        }
        "vorbis" => {
            keys.insert("vorbis".to_string());
        }
        "opus" => {
            keys.insert("opus".to_string());
        }
        "wav" | "wave" | "audio/wav" | "audio/x-wav" | "pcm_s16le" | "pcm" => {
            keys.insert("pcm_s16le".to_string());
            keys.insert("pcm".to_string());
        }
        other => {
            keys.insert(other.to_string());
        }
    }
}

fn is_self_describing_audio_key(key: &str) -> bool {
    matches!(key, "mp3" | "flac" | "aac" | "opus" | "wav" | "wave")
}

fn normalized_audio_key(value: &str) -> Option<String> {
    let normalized = value
        .split(';')
        .next()
        .unwrap_or(value)
        .trim()
        .trim_start_matches('.')
        .to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }

    Some(
        match normalized.as_str() {
            "audio/mpeg" | "audio/mp3" | "mpeg" | "mpga" => "mp3",
            "audio/flac" | "audio/x-flac" | "x-flac" => "flac",
            "audio/mp4" | "audio/x-m4a" => "m4a",
            "audio/aac" | "audio/aacp" => "aac",
            "audio/ogg" | "application/ogg" => "ogg",
            "audio/opus" => "opus",
            "audio/wav" | "audio/x-wav" | "audio/wave" | "audio/vnd.wave" => "wav",
            "application/octet-stream" | "binary/octet-stream" => return None,
            other => other,
        }
        .to_string(),
    )
}

fn clean_audio_extension(value: &str) -> Option<String> {
    let extension = value.trim().trim_start_matches('.').to_ascii_lowercase();
    if extension.is_empty() || extension.contains('/') {
        None
    } else {
        Some(extension)
    }
}

fn extension_from_content_type(content_type: &str) -> Option<&'static str> {
    match normalized_audio_key(content_type)?.as_str() {
        "mp3" | "mpeg" => Some("mp3"),
        "flac" | "x-flac" => Some("flac"),
        "ogg" | "oga" | "vorbis" => Some("ogg"),
        "opus" => Some("opus"),
        "wav" | "wave" => Some("wav"),
        "mp4" | "m4a" | "aac" => Some("m4a"),
        _ => None,
    }
}

fn is_generic_binary_content_type(content_type: &str) -> bool {
    let content_type = content_type
        .split(';')
        .next()
        .unwrap_or(content_type)
        .trim()
        .to_ascii_lowercase();
    matches!(
        content_type.as_str(),
        "application/octet-stream" | "binary/octet-stream"
    )
}

async fn mark_operation_item_handled(
    operation_manager: &Arc<SyncOperationManager>,
    operation_id: &str,
    skipped_bytes: u64,
) {
    if let Some(mut operation) = operation_manager.get_operation(operation_id).await {
        operation.files_completed += 1;
        let adjusted_total = operation.total_bytes.saturating_sub(skipped_bytes);
        operation.total_bytes = adjusted_total.max(operation.bytes_transferred);
        operation_manager
            .update_operation(operation_id, operation)
            .await;
    }
}

async fn mark_operation_preparing_file(
    operation_manager: &Arc<SyncOperationManager>,
    operation_id: &str,
    file_name: &str,
    bytes_total: u64,
) {
    if let Some(mut operation) = operation_manager.get_operation(operation_id).await {
        operation.current_file = Some(file_name.to_string());
        operation.bytes_current = 0;
        operation.bytes_total = bytes_total;
        operation_manager
            .update_operation(operation_id, operation)
            .await;
    }
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
    let cleaned = source.trim_end_matches([' ', '.']);
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
        let clean_ext = truncated_ext.trim_end_matches([' ', '.']);
        return format!(".{}", clean_ext);
    }
    let max_base_len = max_len - ext_len;
    let truncated_base: String = base.chars().take(max_base_len).collect();
    let clean_base = truncated_base.trim_end_matches([' ', '.']);
    format!("{}.{}", clean_base, extension)
}

fn is_missing_delete_error(error: &anyhow::Error) -> bool {
    if error
        .downcast_ref::<std::io::Error>()
        .map(|io| io.kind() == std::io::ErrorKind::NotFound)
        .unwrap_or(false)
    {
        return true;
    }

    let message = error.to_string().to_ascii_lowercase();
    message.contains("os error 2")
        || message.contains("mtp path component not found:")
        || message.contains("libmtp: path component") && message.contains("not found")
}

fn component_looks_like_windows_drive(component: &str) -> bool {
    component.len() == 2
        && component.as_bytes()[1] == b':'
        && component.as_bytes()[0].is_ascii_alphabetic()
}

fn normalized_relative_components_are_safe(path: &str) -> bool {
    path.split('/').enumerate().all(|(index, component)| {
        !component.is_empty()
            && component != "."
            && component != ".."
            && !(index == 0 && component_looks_like_windows_drive(component))
    })
}

fn normalize_delete_relative_path(path: &str) -> Option<String> {
    let candidate = Path::new(path);
    if candidate.is_absolute()
        || candidate.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::Prefix(_)
                    | std::path::Component::RootDir
            )
        })
    {
        return None;
    }

    let normalized_path = path.replace('\\', "/");
    if normalized_path.is_empty()
        || normalized_path.starts_with('/')
        || normalized_path.ends_with('/')
        || !normalized_relative_components_are_safe(&normalized_path)
    {
        return None;
    }

    Some(normalized_path)
}

fn normalize_managed_subfolder(managed_subfolder: &str) -> Option<String> {
    let normalized = managed_subfolder
        .replace('\\', "/")
        .trim_matches('/')
        .to_string();
    if normalized.is_empty() {
        return Some(String::new());
    }
    normalized_relative_components_are_safe(&normalized).then_some(normalized)
}

fn relative_path_is_in_managed_subfolder(path: &str, managed_subfolder: &str) -> bool {
    let Some(normalized_path) = normalize_delete_relative_path(path) else {
        return false;
    };
    let Some(normalized_managed) = normalize_managed_subfolder(managed_subfolder) else {
        return false;
    };

    normalized_managed.is_empty()
        || normalized_path
            .strip_prefix(&format!("{}/", normalized_managed))
            .is_some()
}

fn validate_delete_path_for_managed_zone(
    device_path: &Path,
    managed_path: &Path,
    managed_subfolder: Option<&str>,
    local_path: &str,
    is_mtp: bool,
    owned_manifest_paths: &HashSet<String>,
) -> std::result::Result<(), String> {
    if owned_manifest_paths.contains(&normalized_device_folder(local_path)) {
        validate_owned_manifest_delete_path(device_path, local_path, is_mtp)?;
        return Ok(());
    }

    let Some(managed_subfolder) = managed_subfolder else {
        return Err("Failed to resolve managed subfolder - refusing to delete".to_string());
    };

    if !relative_path_is_in_managed_subfolder(local_path, managed_subfolder) {
        return Err("File is not in managed zone - refusing to delete".to_string());
    }

    if is_mtp {
        return Ok(());
    }

    let file_path = device_path.join(local_path);
    match file_path.canonicalize() {
        Ok(absolute_file_path) => {
            let absolute_managed_path = managed_path
                .canonicalize()
                .map_err(|e| format!("Failed to resolve managed path: {}", e))?;
            if !absolute_file_path.starts_with(&absolute_managed_path) {
                return Err("File is not in managed zone - refusing to delete".to_string());
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("Failed to resolve file path: {}", e)),
    }

    Ok(())
}

fn validate_owned_manifest_delete_path(
    device_path: &Path,
    local_path: &str,
    is_mtp: bool,
) -> std::result::Result<(), String> {
    validate_device_relative_path(local_path)?;
    if is_mtp {
        return Ok(());
    }

    let file_path = device_path.join(local_path);
    match file_path.canonicalize() {
        Ok(absolute_file_path) => {
            let absolute_device_path = device_path
                .canonicalize()
                .map_err(|e| format!("Failed to resolve device path: {}", e))?;
            if !absolute_file_path.starts_with(&absolute_device_path) {
                return Err("File is not on device - refusing to delete".to_string());
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("Failed to resolve file path: {}", e)),
    }

    Ok(())
}

fn normalized_device_folder(path: &str) -> String {
    path.replace('\\', "/").trim_matches('/').to_string()
}

fn validate_device_relative_path(path: &str) -> std::result::Result<(), String> {
    let normalized = path.replace('\\', "/");
    if normalized.starts_with('/') || Path::new(&normalized).is_absolute() {
        return Err("Device path must be relative".to_string());
    }
    if normalized.split('/').any(|component| {
        component.is_empty() || component == "." || component == ".." || component.contains(':')
    }) {
        return Err(
            "Device path must not contain empty, current, parent, or drive-prefix components"
                .to_string(),
        );
    }
    Ok(())
}

fn validate_device_relative_folder(path: &str) -> std::result::Result<String, String> {
    let normalized = normalized_device_folder(path);
    if normalized.is_empty() {
        return Ok(normalized);
    }
    validate_device_relative_path(&normalized)?;
    Ok(normalized)
}

fn device_path_in_or_equal(path: &str, folder: &str) -> bool {
    let path = normalized_device_folder(path);
    let folder = normalized_device_folder(folder);
    if folder.is_empty() {
        return true;
    }
    path == folder || path.starts_with(&format!("{folder}/"))
}

fn prefixed_device_path(folder: &str, filename: &str) -> String {
    let folder = normalized_device_folder(folder);
    let filename = filename.replace('\\', "/").trim_matches('/').to_string();
    if folder.is_empty() {
        filename
    } else {
        format!("{folder}/{filename}")
    }
}

fn relative_device_path_from_folder(from_folder: &str, target_path: &str) -> String {
    let normalized_from = normalized_device_folder(from_folder);
    let normalized_target = normalized_device_folder(target_path);
    let from_parts: Vec<&str> = normalized_from
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    let target_parts: Vec<&str> = normalized_target
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    let mut common = 0;
    while common < from_parts.len()
        && common < target_parts.len()
        && from_parts[common] == target_parts[common]
    {
        common += 1;
    }

    let mut rel_parts: Vec<String> = Vec::new();
    rel_parts.extend((common..from_parts.len()).map(|_| "..".to_string()));
    rel_parts.extend(
        target_parts[common..]
            .iter()
            .map(|part| (*part).to_string()),
    );
    if rel_parts.is_empty() {
        ".".to_string()
    } else {
        rel_parts.join("/")
    }
}

fn playlist_manifest_rel_path(filename: &str, playlist_subfolder: &str) -> String {
    let normalized_filename = filename.replace('\\', "/").trim_matches('/').to_string();
    if normalized_filename.contains('/') {
        normalized_filename
    } else {
        prefixed_device_path(playlist_subfolder, &normalized_filename)
    }
}

fn planned_playlist_filenames(playlist_items: &[PlaylistSyncItem]) -> HashMap<&str, String> {
    let mut used_filenames: HashSet<String> = HashSet::new();
    let mut filenames = HashMap::new();

    for playlist in playlist_items {
        let sanitized_name = sanitize_path_component(&playlist.name);
        let base_name = if sanitized_name.is_empty() {
            playlist.jellyfin_id[..playlist.jellyfin_id.len().min(32)].to_string()
        } else {
            sanitized_name
        };
        let candidate = truncate_filename(&base_name, "m3u", 255);
        let filename = if used_filenames.contains(&candidate) {
            let id_tag = &playlist.jellyfin_id[..8.min(playlist.jellyfin_id.len())];
            let tagged = format!("{} ({})", base_name, id_tag);
            truncate_filename(&tagged, "m3u", 255)
        } else {
            candidate
        };
        used_filenames.insert(filename.clone());
        filenames.insert(playlist.jellyfin_id.as_str(), filename);
    }

    filenames
}

pub fn destructive_cleanup_count(delta: &SyncDelta, manifest: &DeviceManifest) -> usize {
    let playlist_subfolder = manifest
        .resolved_playlist_path()
        .map(normalized_device_folder)
        .or_else(|| {
            manifest
                .managed_paths
                .first()
                .map(|path| normalized_device_folder(path))
        })
        .unwrap_or_default();
    let planned_filenames = planned_playlist_filenames(&delta.playlists);
    let active_ids: HashSet<&str> = delta
        .playlists
        .iter()
        .map(|playlist| playlist.jellyfin_id.as_str())
        .collect();
    let mut playlist_cleanup_paths = HashSet::new();

    for entry in &manifest.playlists {
        let old_path = playlist_manifest_rel_path(&entry.filename, &playlist_subfolder);
        if !active_ids.contains(entry.jellyfin_id.as_str()) {
            playlist_cleanup_paths.insert(old_path);
            continue;
        }
        if let Some(planned_filename) = planned_filenames.get(entry.jellyfin_id.as_str()) {
            let new_path = prefixed_device_path(&playlist_subfolder, planned_filename);
            if old_path != new_path {
                playlist_cleanup_paths.insert(old_path);
            }
        }
    }

    // Readd pairs (same jellyfin_id in both adds and deletes) are file replacements, not removals.
    let readd_ids: HashSet<&str> = delta.adds.iter().map(|a| a.jellyfin_id.as_str()).collect();
    let net_deletes = delta
        .deletes
        .iter()
        .filter(|d| !readd_ids.contains(d.jellyfin_id.as_str()))
        .count();
    net_deletes + playlist_cleanup_paths.len()
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

    crate::daemon_log!(
        "[Sync] execute_sync preparing: adds={} deletes={} id_changes={} playlists={}",
        delta.adds.len(),
        delta.deletes.len(),
        delta.id_changes.len(),
        delta.playlists.len()
    );

    // Compute total bytes for ETA (adds + id_changes both contribute bytes)
    let total_job_bytes: u64 = delta.adds.iter().map(|a| a.size_bytes).sum::<u64>()
        + delta.id_changes.iter().map(|c| c.size_bytes).sum::<u64>();
    if let Some(mut operation) = operation_manager.get_operation(&operation_id).await {
        operation.total_bytes = total_job_bytes;
        operation_manager
            .update_operation(&operation_id, operation)
            .await;
    }

    // Shared counter for cumulative bytes written across all files (for ETA)
    let completed_bytes_arc = Arc::new(std::sync::atomic::AtomicU64::new(0));

    let manifest_snapshot = device_manager.get_current_device().await;
    let owned_manifest_paths: HashSet<String> = manifest_snapshot
        .as_ref()
        .map(|manifest| {
            manifest
                .synced_items
                .iter()
                .map(|item| normalized_device_folder(&item.local_path))
                .collect()
        })
        .unwrap_or_default();

    // Determine managed path from the device manifest's first managed_paths entry.
    let managed_path = {
        let subfolder = manifest_snapshot
            .as_ref()
            .and_then(|m| m.managed_paths.first())
            .map(|s| s.as_str())
            .unwrap_or("Music");
        device_path.join(subfolder)
    };
    let is_mtp = device_path.to_string_lossy().starts_with("mtp://");
    let managed_subfolder_for_delete: Option<String> =
        managed_path.strip_prefix(device_path).ok().map(|p| {
            p.to_string_lossy()
                .replace('\\', "/")
                .trim_end_matches('/')
                .to_string()
        });
    let readd_ids: HashSet<&str> = delta
        .adds
        .iter()
        .map(|add| add.jellyfin_id.as_str())
        .collect();
    let readd_delete_by_id: HashMap<&str, &SyncDeleteItem> = delta
        .deletes
        .iter()
        .filter(|delete| readd_ids.contains(delete.jellyfin_id.as_str()))
        .map(|delete| (delete.jellyfin_id.as_str(), delete))
        .collect();

    // Pre-fetch all item details for adds to avoid N+1 queries
    let mut fetched_items = std::collections::HashMap::new();
    let add_ids: Vec<&str> = delta.adds.iter().map(|a| a.jellyfin_id.as_str()).collect();
    let total_chunks = add_ids.len().div_ceil(100);
    for (chunk_index, chunk) in add_ids.chunks(100).enumerate() {
        crate::daemon_log!(
            "[Sync] Preparing: fetching Jellyfin metadata chunk {}/{} ({} item(s))",
            chunk_index + 1,
            total_chunks,
            chunk.len()
        );
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
    for (index, add_item) in delta.adds.iter().enumerate() {
        if operation_manager.is_cancelled(&operation_id).await {
            break;
        }
        crate::daemon_log!(
            "[Sync] Preparing file {}/{}: '{}' ({}, {} bytes)",
            index + 1,
            delta.adds.len(),
            add_item.name,
            add_item.jellyfin_id,
            add_item.size_bytes
        );
        mark_operation_preparing_file(
            &operation_manager,
            &operation_id,
            &add_item.name,
            add_item.size_bytes,
        )
        .await;

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

        let device_preferred_audio_container = device_io.preferred_audio_container();
        let preferred_audio_container = if transcoding_profile.is_some() {
            None
        } else {
            device_preferred_audio_container
        };
        let effective_transcoding_profile;
        let compatibility =
            audio_compatibility_profile(transcoding_profile.as_ref(), preferred_audio_container);
        let source_format = provider_audio_format(source_container(item), None);
        let source_direct_compatible = compatibility.source_is_direct_compatible(&source_format);
        let stream_profile = if let Some(container) = preferred_audio_container {
            effective_transcoding_profile = forced_audio_profile(container);
            Some(&effective_transcoding_profile)
        } else if compatibility.is_constrained() && source_direct_compatible {
            None
        } else {
            transcoding_profile.as_ref()
        };

        // Resolve stream via PlaybackInfo if a profile is set, else direct /Download.
        // is_transcoded tells us whether the server actually transcodes the content,
        // which determines whether the profile's target container applies as the file extension.
        crate::daemon_log!(
            "[Sync] Preparing '{}': resolving stream (profile={})",
            add_item.name,
            stream_profile.is_some()
        );
        let stream_result = jellyfin_client
            .get_item_stream(
                jellyfin_url,
                jellyfin_token,
                jellyfin_user_id,
                &add_item.jellyfin_id,
                stream_profile,
            )
            .await;

        let (stream, is_transcoded) = match stream_result {
            Ok(result) => result,
            Err(e) => {
                errors.push(SyncFileError {
                    jellyfin_id: add_item.jellyfin_id.clone(),
                    filename: add_item.name.clone(),
                    error_message: format!("Failed to get stream: {}", e),
                });
                continue;
            }
        };

        // Determine the output extension. The transcoding profile's target container only
        // applies when the server is actually transcoding — for direct-play downloads the
        // original file format is served unchanged, so the source extension must be used.
        let profile_container = if is_transcoded {
            stream_profile.and_then(|p| {
                p["TranscodingProfiles"]
                    .as_array()
                    .and_then(|a| a.first())
                    .and_then(|tp| tp["Container"].as_str())
            })
        } else {
            None
        };
        let extension_override = preferred_audio_container.or(profile_container);
        eprintln!(
            "[Sync] item={} extension_override={:?} (preferred_audio_container={:?}, device_preferred_audio_container={:?}, profile_container={:?}, is_transcoded={})",
            add_item.jellyfin_id,
            extension_override,
            preferred_audio_container,
            device_preferred_audio_container,
            profile_container,
            is_transcoded
        );

        // Construct target path (includes legacy hardware path length validation)
        crate::daemon_log!(
            "[Sync] Preparing '{}': constructing target path",
            add_item.name
        );
        let construction =
            match construct_file_path_with_extension(&managed_path, item, extension_override) {
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
        crate::daemon_log!("[Sync] Downloading '{}'", add_item.name);
        let t_download = std::time::Instant::now();
        let buffer_result = buffer_stream(stream, total_size, progress_callback).await;
        let download_timing = transfer_timing(add_item.size_bytes, t_download.elapsed());
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
        crate::daemon_log!("[Sync] Writing '{}'", add_item.name);
        let t_write = std::time::Instant::now();
        let write_result = device_io.write_with_verify(&rel_path, &buffer).await;
        let write_timing = transfer_timing(add_item.size_bytes, t_write.elapsed());

        match write_result {
            Ok(_) => {
                crate::daemon_log!(
                    "[Sync] '{}' size={}B download={:.2}ms({:.1}MB/s) write={:.2}ms({:.1}MB/s)",
                    add_item.name,
                    add_item.size_bytes,
                    download_timing.elapsed_ms,
                    download_timing.speed_mb_s,
                    write_timing.elapsed_ms,
                    write_timing.speed_mb_s
                );
                // For backends that do not verify internally (MSC), confirm the file
                // actually landed before marking it synced. Backends like MTP already
                // verify via LIBMTP_Get_Filemetadata (direct object-ID lookup) inside
                // write_with_verify, so the list_files enumeration check is skipped —
                // some devices (e.g. Garmin) hide files from enumeration even after a
                // successful write.
                if !device_io.write_verifies_internally()
                    && !device_file_exists(device_io.as_ref(), &rel_path).await
                {
                    errors.push(SyncFileError {
                        jellyfin_id: add_item.jellyfin_id.clone(),
                        filename: add_item.name.clone(),
                        error_message:
                            "File reported as written but not found on device after transfer"
                                .to_string(),
                    });
                    continue;
                }

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
                    provider_album_id: add_item.provider_album_id.clone(),
                    provider_content_type: add_item.provider_content_type.clone(),
                    provider_suffix: add_item.provider_suffix.clone(),
                    original_bitrate: add_item.original_bitrate,
                    original_container: add_item.provider_suffix.clone(),
                    track_number: add_item.track_number,
                    server_id: add_item.server_id.clone(),
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
                if let Some(delete_item) = readd_delete_by_id.get(add_item.jellyfin_id.as_str())
                    && let Some(error) = cleanup_replaced_file_after_write(
                        delete_item,
                        &rel_path,
                        device_path,
                        &managed_path,
                        managed_subfolder_for_delete.as_deref(),
                        is_mtp,
                        &owned_manifest_paths,
                        &device_io,
                        &operation_manager,
                        &operation_id,
                    )
                    .await
                {
                    errors.push(error);
                }
                let synced_item = synced_items.last().unwrap().clone();
                let id_to_replace = add_item.jellyfin_id.clone();
                if let Err(e) = device_manager
                    .update_manifest(|m| {
                        m.synced_items
                            .retain(|item| item.jellyfin_id != id_to_replace);
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
    //
    // Managed zone check strategy:
    // - MSC: canonicalize() both paths to real absolute filesystem paths, then prefix-check.
    // - MTP: device_path is a synthetic "mtp://…" URI that doesn't exist on the local
    //   filesystem — canonicalize() always fails and would silently skip every delete.
    //   Use a string-prefix check on the relative local_path instead.
    let _is_mtp_after_add_unused = device_path.to_string_lossy().starts_with("mtp://");
    // Option<String>: None = strip_prefix failed (malformed managed_path) → fail-safe reject all.
    // Some("") = whole device root is managed → all paths are valid.
    // Some("Music") = only paths under "Music/" are valid.
    let _managed_subfolder_after_add_unused: Option<String> =
        managed_path.strip_prefix(device_path).ok().map(|p| {
            p.to_string_lossy()
                .replace('\\', "/")
                .trim_end_matches('/')
                .to_string()
        });

    for delete_item in delta
        .deletes
        .iter()
        .filter(|delete| !readd_ids.contains(delete.jellyfin_id.as_str()))
    {
        if operation_manager.is_cancelled(&operation_id).await {
            break;
        }
        // Verify file is in managed zone (security check)
        if let Err(error_message) = validate_delete_path_for_managed_zone(
            device_path,
            &managed_path,
            managed_subfolder_for_delete.as_deref(),
            &delete_item.local_path,
            is_mtp,
            &owned_manifest_paths,
        ) {
            errors.push(SyncFileError {
                jellyfin_id: delete_item.jellyfin_id.clone(),
                filename: delete_item.name.clone(),
                error_message,
            });
            continue;
        }

        // Delete file via device IO abstraction (relative path, backend handles resolution)
        let delete_result = device_io.delete_file(&delete_item.local_path).await;
        // Treat "not found" as idempotent success: the file is already absent, which is
        // the goal of deletion. This handles duplicate manifest entries (e.g. same path
        // added via basket and playlist) and re-runs after a failed manifest update.
        let already_absent = matches!(&delete_result, Err(e) if is_missing_delete_error(e));
        match delete_result {
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
            Err(_) if already_absent => {
                // File was not on device (already deleted or never written).
                // Remove the manifest entry so this item is not retried on the next sync.
                if let Some(mut operation) = operation_manager.get_operation(&operation_id).await {
                    operation.files_completed += 1;
                    operation_manager
                        .update_operation(&operation_id, operation)
                        .await;
                }

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
        if operation_manager.is_cancelled(&operation_id).await {
            break;
        }
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
            provider_album_id: id_change.provider_album_id.clone(),
            provider_content_type: id_change.provider_content_type.clone(),
            provider_suffix: id_change.provider_suffix.clone(),
            original_bitrate: None,
            original_container: None,
            track_number: None,
            server_id: id_change.source_server_id.clone(),
        });

        // Update operation progress and cumulative bytes (an ID change is instantly completed)
        completed_bytes_arc.fetch_add(id_change.size_bytes, std::sync::atomic::Ordering::Relaxed);
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
    if let Some(mut manifest_snapshot) = device_manager.get_current_device().await
        && (!delta.playlists.is_empty() || !manifest_snapshot.playlists.is_empty())
    {
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

    let mut device_warnings = device_io.take_warnings().await;
    if let Err(e) = device_io.end_sync_job().await {
        device_warnings.push(format!(
            "[DeviceIO] Failed to end device sync job cleanly: {}",
            e
        ));
    }
    if !device_warnings.is_empty()
        && let Some(mut operation) = operation_manager.get_operation(&operation_id).await
    {
        operation.warnings.append(&mut device_warnings);
        operation_manager
            .update_operation(&operation_id, operation)
            .await;
    }

    Ok((synced_items, errors))
}

pub struct ProviderSyncSource {
    pub provider: Arc<dyn MediaProvider>,
    pub transcoding_profile: Option<serde_json::Value>,
}

pub async fn execute_provider_sync(
    delta: &SyncDelta,
    device_path: &Path,
    source: ProviderSyncSource,
    operation_manager: Arc<SyncOperationManager>,
    operation_id: String,
    device_manager: Arc<crate::device::DeviceManager>,
    device_io: Arc<dyn crate::device_io::DeviceIO>,
) -> Result<(Vec<crate::device::SyncedItem>, Vec<SyncFileError>)> {
    let ProviderSyncSource {
        provider,
        transcoding_profile,
    } = source;
    let mut synced_items = Vec::new();
    let mut errors = Vec::new();
    let mut sync_warnings = Vec::new();
    if let Err(e) = device_io.begin_sync_job().await {
        errors.push(SyncFileError {
            jellyfin_id: String::new(),
            filename: String::new(),
            error_message: format!("Failed to begin device sync job: {}", e),
        });
    }

    crate::daemon_log!(
        "[Sync] execute_provider_sync preparing: adds={} deletes={} id_changes={} playlists={}",
        delta.adds.len(),
        delta.deletes.len(),
        delta.id_changes.len(),
        delta.playlists.len()
    );

    let total_job_bytes: u64 = delta.adds.iter().map(|a| a.size_bytes).sum::<u64>()
        + delta.id_changes.iter().map(|c| c.size_bytes).sum::<u64>();
    if let Some(mut operation) = operation_manager.get_operation(&operation_id).await {
        operation.total_bytes = total_job_bytes;
        operation_manager
            .update_operation(&operation_id, operation)
            .await;
    }

    let completed_bytes_arc = Arc::new(std::sync::atomic::AtomicU64::new(0));

    let manifest_snapshot = device_manager.get_current_device().await;
    let owned_manifest_paths: HashSet<String> = manifest_snapshot
        .as_ref()
        .map(|manifest| {
            manifest
                .synced_items
                .iter()
                .map(|item| normalized_device_folder(&item.local_path))
                .collect()
        })
        .unwrap_or_default();

    let managed_path = {
        let subfolder = manifest_snapshot
            .as_ref()
            .and_then(|m| m.managed_paths.first())
            .map(|s| s.as_str())
            .unwrap_or("Music");
        device_path.join(subfolder)
    };
    let is_mtp = device_path.to_string_lossy().starts_with("mtp://");
    let managed_subfolder_for_delete: Option<String> =
        managed_path.strip_prefix(device_path).ok().map(|p| {
            p.to_string_lossy()
                .replace('\\', "/")
                .trim_end_matches('/')
                .to_string()
        });

    let device_preferred_audio_container = device_io.preferred_audio_container();
    let preferred_audio_container = if transcoding_profile.is_some() {
        None
    } else {
        device_preferred_audio_container
    };
    let compatibility =
        audio_compatibility_profile(transcoding_profile.as_ref(), preferred_audio_container);
    let readd_ids: HashSet<&str> = delta
        .adds
        .iter()
        .map(|add| add.jellyfin_id.as_str())
        .collect();
    let readd_delete_by_id: HashMap<&str, &SyncDeleteItem> = delta
        .deletes
        .iter()
        .filter(|delete| readd_ids.contains(delete.jellyfin_id.as_str()))
        .map(|delete| (delete.jellyfin_id.as_str(), delete))
        .collect();

    for (index, add_item) in delta.adds.iter().enumerate() {
        if operation_manager.is_cancelled(&operation_id).await {
            break;
        }
        crate::daemon_log!(
            "[Sync] Preparing file {}/{}: '{}' ({}, {} bytes)",
            index + 1,
            delta.adds.len(),
            add_item.name,
            add_item.jellyfin_id,
            add_item.size_bytes
        );
        mark_operation_preparing_file(
            &operation_manager,
            &operation_id,
            &add_item.name,
            add_item.size_bytes,
        )
        .await;

        let source_format = provider_audio_format(
            add_item.provider_suffix.as_deref(),
            add_item.provider_content_type.as_deref(),
        );
        let source_direct_compatible = compatibility.source_is_direct_compatible(&source_format);
        let profile = if compatibility.is_constrained() && !source_direct_compatible {
            match compatibility.transcode_profile.clone() {
                Some(profile) => Some(profile),
                None => {
                    sync_warnings.push(format!(
                        "[Sync] Skipped '{}' ({}) because the source format is incompatible and no compatible transcode profile is available",
                        add_item.name, add_item.jellyfin_id
                    ));
                    mark_operation_item_handled(
                        &operation_manager,
                        &operation_id,
                        add_item.size_bytes,
                    )
                    .await;
                    continue;
                }
            }
        } else {
            None
        };
        crate::daemon_log!(
            "[Sync] Preparing '{}': resolving provider download URL (transcode={}, source_suffix={:?}, source_content_type={:?}, direct_compatible={}, preferred_audio_container={:?}, device_preferred_audio_container={:?})",
            add_item.name,
            profile.is_some(),
            add_item.provider_suffix,
            add_item.provider_content_type,
            source_direct_compatible,
            preferred_audio_container,
            device_preferred_audio_container
        );
        let url = match provider
            .download_url(&add_item.jellyfin_id, profile.as_ref())
            .await
        {
            Ok(url) => url,
            Err(e) => {
                if profile.is_some() {
                    sync_warnings.push(format!(
                        "[Sync] Skipped '{}' ({}) because required transcoding could not be negotiated: {}",
                        add_item.name, add_item.jellyfin_id, e
                    ));
                    mark_operation_item_handled(
                        &operation_manager,
                        &operation_id,
                        add_item.size_bytes,
                    )
                    .await;
                } else {
                    errors.push(SyncFileError {
                        jellyfin_id: add_item.jellyfin_id.clone(),
                        filename: add_item.name.clone(),
                        error_message: format!("Failed to get stream: {}", e),
                    });
                }
                continue;
            }
        };
        crate::daemon_log!("[Sync] Preparing '{}': opening HTTP stream", add_item.name);
        let response = match reqwest::Client::new().get(url).send().await {
            Ok(response) => response,
            Err(e) => {
                errors.push(SyncFileError {
                    jellyfin_id: add_item.jellyfin_id.clone(),
                    filename: add_item.name.clone(),
                    error_message: format!("Failed to open stream: {}", e),
                });
                continue;
            }
        };
        if !response.status().is_success() {
            if profile.is_some() {
                sync_warnings.push(format!(
                    "[Sync] Skipped '{}' ({}) because required transcoding returned status {}",
                    add_item.name,
                    add_item.jellyfin_id,
                    response.status()
                ));
                mark_operation_item_handled(&operation_manager, &operation_id, add_item.size_bytes)
                    .await;
            } else {
                errors.push(SyncFileError {
                    jellyfin_id: add_item.jellyfin_id.clone(),
                    filename: add_item.name.clone(),
                    error_message: format!("Stream returned status {}", response.status()),
                });
            }
            continue;
        }

        let response_content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok());
        let response_format = provider_audio_format(None, response_content_type);
        let extension_override = if !compatibility.is_constrained() {
            source_format
                .extension
                .clone()
                .or_else(|| response_format.extension.clone())
        } else if profile.is_some() {
            if !response_format.is_empty() && compatibility.output_is_compatible(&response_format) {
                response_format.extension.clone().or_else(|| {
                    profile
                        .as_ref()
                        .and_then(|profile| profile.container.as_deref())
                        .and_then(clean_audio_extension)
                })
            } else {
                let reason = if response_format.is_empty() {
                    format!(
                        "transcoding to {} was requested but the provider output was unconfirmed",
                        compatibility.transcode_target_label()
                    )
                } else {
                    format!(
                        "the provider returned incompatible content type {:?}",
                        response_content_type.unwrap_or("unknown")
                    )
                };
                sync_warnings.push(format!(
                    "[Sync] Skipped '{}' ({}) because {}",
                    add_item.name, add_item.jellyfin_id, reason
                ));
                mark_operation_item_handled(&operation_manager, &operation_id, add_item.size_bytes)
                    .await;
                continue;
            }
        } else {
            let has_unrecognized_specific_content_type =
                response_content_type.is_some_and(|content_type| {
                    response_format.is_empty() && !is_generic_binary_content_type(content_type)
                });
            if (!response_format.is_empty()
                && !compatibility.output_is_compatible(&response_format))
                || has_unrecognized_specific_content_type
            {
                sync_warnings.push(format!(
                    "[Sync] Skipped '{}' ({}) because the provider returned incompatible content type {:?}",
                    add_item.name,
                    add_item.jellyfin_id,
                    response_content_type.unwrap_or("unknown")
                ));
                mark_operation_item_handled(&operation_manager, &operation_id, add_item.size_bytes)
                    .await;
                continue;
            }
            source_format
                .extension
                .clone()
                .or_else(|| response_format.extension.clone())
        };

        crate::daemon_log!(
            "[Sync] Preparing '{}': constructing target path",
            add_item.name
        );
        let construction = match construct_desired_file_path(
            &managed_path,
            add_item,
            extension_override.as_deref(),
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

        let file_name = add_item.name.clone();
        let total_size = add_item.size_bytes;
        let op_manager = operation_manager.clone();
        let op_id = operation_id.clone();
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

        crate::daemon_log!("[Sync] Downloading '{}'", add_item.name);
        let t_download = std::time::Instant::now();
        let buffer =
            match buffer_stream(response.bytes_stream(), total_size, progress_callback).await {
                Ok(buffer) => buffer,
                Err(e) => {
                    errors.push(SyncFileError {
                        jellyfin_id: add_item.jellyfin_id.clone(),
                        filename: add_item.name.clone(),
                        error_message: format!("Failed to buffer stream: {}", e),
                    });
                    continue;
                }
            };
        let download_timing = transfer_timing(add_item.size_bytes, t_download.elapsed());
        let rel_path = construction
            .path
            .strip_prefix(device_path)
            .unwrap_or(&construction.path)
            .to_string_lossy()
            .replace('\\', "/");

        crate::daemon_log!("[Sync] Writing '{}'", add_item.name);
        let t_write = std::time::Instant::now();
        match device_io.write_with_verify(&rel_path, &buffer).await {
            Ok(_) => {
                let write_timing = transfer_timing(add_item.size_bytes, t_write.elapsed());
                crate::daemon_log!(
                    "[Sync] '{}' size={}B download={:.2}ms({:.1}MB/s) write={:.2}ms({:.1}MB/s)",
                    add_item.name,
                    add_item.size_bytes,
                    download_timing.elapsed_ms,
                    download_timing.speed_mb_s,
                    write_timing.elapsed_ms,
                    write_timing.speed_mb_s
                );
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
                    provider_album_id: add_item.provider_album_id.clone(),
                    provider_content_type: add_item.provider_content_type.clone(),
                    provider_suffix: add_item.provider_suffix.clone(),
                    original_bitrate: add_item.original_bitrate,
                    original_container: add_item.provider_suffix.clone(),
                    track_number: add_item.track_number,
                    server_id: add_item.server_id.clone(),
                });
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
                let synced_item = synced_items.last().unwrap().clone();
                if let Some(delete_item) = readd_delete_by_id.get(add_item.jellyfin_id.as_str())
                    && let Some(error) = cleanup_replaced_file_after_write(
                        delete_item,
                        &rel_path,
                        device_path,
                        &managed_path,
                        managed_subfolder_for_delete.as_deref(),
                        is_mtp,
                        &owned_manifest_paths,
                        &device_io,
                        &operation_manager,
                        &operation_id,
                    )
                    .await
                {
                    errors.push(error);
                }
                let id_to_replace = add_item.jellyfin_id.clone();
                if let Err(e) = device_manager
                    .update_manifest(|m| {
                        m.synced_items
                            .retain(|item| item.jellyfin_id != id_to_replace);
                        m.synced_items.push(synced_item);
                    })
                    .await
                {
                    eprintln!("[Sync] Warning: per-file manifest write failed: {}", e);
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

    for delete_item in delta
        .deletes
        .iter()
        .filter(|delete| !readd_ids.contains(delete.jellyfin_id.as_str()))
    {
        if operation_manager.is_cancelled(&operation_id).await {
            break;
        }
        if let Err(error_message) = validate_delete_path_for_managed_zone(
            device_path,
            &managed_path,
            managed_subfolder_for_delete.as_deref(),
            &delete_item.local_path,
            is_mtp,
            &owned_manifest_paths,
        ) {
            errors.push(SyncFileError {
                jellyfin_id: delete_item.jellyfin_id.clone(),
                filename: delete_item.name.clone(),
                error_message,
            });
            continue;
        }

        let delete_result = device_io.delete_file(&delete_item.local_path).await;
        let already_absent = matches!(&delete_result, Err(e) if is_missing_delete_error(e));
        match delete_result {
            Ok(_) => {
                if let Some(mut operation) = operation_manager.get_operation(&operation_id).await {
                    operation.files_completed += 1;
                    operation_manager
                        .update_operation(&operation_id, operation)
                        .await;
                }
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
            Err(_) if already_absent => {
                if let Some(mut operation) = operation_manager.get_operation(&operation_id).await {
                    operation.files_completed += 1;
                    operation_manager
                        .update_operation(&operation_id, operation)
                        .await;
                }

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
            Err(e) => errors.push(SyncFileError {
                jellyfin_id: delete_item.jellyfin_id.clone(),
                filename: delete_item.name.clone(),
                error_message: format!("Failed to delete file: {}", e),
            }),
        }
    }

    let managed_subfolder = managed_path
        .strip_prefix(device_path)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();
    if let Err(e) = device_io.cleanup_empty_subdirs(&managed_subfolder).await {
        eprintln!("[Sync] Warning: directory cleanup failed: {}", e);
    }

    for id_change in &delta.id_changes {
        if operation_manager.is_cancelled(&operation_id).await {
            break;
        }
        let synced_at = now_iso8601();
        synced_items.push(crate::device::SyncedItem {
            jellyfin_id: id_change.new_jellyfin_id.clone(),
            name: id_change.name.clone(),
            album: id_change.album.clone(),
            artist: id_change.artist.clone(),
            local_path: id_change.old_local_path.clone(),
            size_bytes: id_change.size_bytes,
            synced_at,
            original_name: id_change.original_name.clone(),
            etag: id_change.etag.clone(),
            provider_album_id: id_change.provider_album_id.clone(),
            provider_content_type: id_change.provider_content_type.clone(),
            provider_suffix: id_change.provider_suffix.clone(),
            original_bitrate: None,
            original_container: None,
            track_number: None,
            server_id: id_change.source_server_id.clone(),
        });
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

    if let Some(mut manifest_snapshot) = device_manager.get_current_device().await
        && (!delta.playlists.is_empty() || !manifest_snapshot.playlists.is_empty())
    {
        let warnings = generate_m3u_files(
            &delta.playlists,
            device_path,
            &managed_path,
            &manifest_snapshot.synced_items.clone(),
            &mut manifest_snapshot,
            Arc::clone(&device_io),
        )
        .await;
        for warning in &warnings {
            eprintln!("{}", warning);
        }
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

    let mut device_warnings = sync_warnings;
    device_warnings.extend(device_io.take_warnings().await);
    if let Err(e) = device_io.end_sync_job().await {
        device_warnings.push(format!(
            "[DeviceIO] Failed to end device sync job cleanly: {}",
            e
        ));
    }
    if !device_warnings.is_empty()
        && let Some(mut operation) = operation_manager.get_operation(&operation_id).await
    {
        operation.warnings.append(&mut device_warnings);
        operation_manager
            .update_operation(&operation_id, operation)
            .await;
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

async fn device_file_exists(device_io: &dyn crate::device_io::DeviceIO, rel_path: &str) -> bool {
    device_io.file_exists(rel_path).await
}

/// Promotes "unchanged" manifest items that are missing on the device into `delta.adds`.
///
/// Called after `calculate_delta` so the UI preview correctly reflects files that were
/// manually deleted from the device. Does NOT run during auto-sync (UI-preview concern only).
pub async fn augment_delta_with_existence_check(
    delta: &mut SyncDelta,
    desired_items: &[DesiredItem],
    manifest: &crate::device::DeviceManifest,
    device_io: &dyn crate::device_io::DeviceIO,
) {
    let already_in_delta: HashSet<String> = delta
        .adds
        .iter()
        .map(|a| a.jellyfin_id.clone())
        .chain(delta.deletes.iter().map(|d| d.jellyfin_id.clone()))
        .chain(delta.id_changes.iter().map(|c| c.new_jellyfin_id.clone()))
        .collect();

    let desired_by_id: std::collections::HashMap<&str, &DesiredItem> = desired_items
        .iter()
        .map(|d| (d.jellyfin_id.as_str(), d))
        .collect();

    let mut to_add: Vec<SyncAddItem> = Vec::new();
    for item in &manifest.synced_items {
        if already_in_delta.contains(&item.jellyfin_id) {
            continue;
        }
        let Some(desired) = desired_by_id.get(item.jellyfin_id.as_str()).copied() else {
            continue;
        };
        if !device_file_exists(device_io, &item.local_path).await {
            to_add.push(annotate_add(
                SyncAddItem {
                    jellyfin_id: desired.jellyfin_id.clone(),
                    name: desired.name.clone(),
                    album: desired.album.clone(),
                    artist: desired.artist.clone(),
                    size_bytes: desired.size_bytes,
                    etag: desired.etag.clone(),
                    provider_album_id: desired.provider_album_id.clone(),
                    provider_content_type: desired.provider_content_type.clone(),
                    provider_suffix: desired.provider_suffix.clone(),
                    original_bitrate: desired.original_bitrate,
                    track_number: desired.track_number,
                    reason_code: None,
                    reason: None,
                    server_id: desired.server_id.clone(),
                    tier: None,
                },
                "device-file-missing",
            ));
        }
    }
    let recovered = to_add.len();
    delta.adds.extend(to_add);
    delta.unchanged = delta.unchanged.saturating_sub(recovered);
}

/// Generates, regenerates, or cleans up .m3u files for playlists in the sync basket.
///
/// Called once per sync run, after all file transfers complete.
/// Uses Write-Temp-Rename (atomic write) for all .m3u writes.
///
/// `device_path` is the device root (local_path in SyncedItem is relative to this).
/// `managed_path` is the music folder (e.g. `device_path/Music`).
/// Playlist files are written to manifest.playlist_path, falling back to managed_path.
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
    let raw_playlist_subfolder = manifest
        .resolved_playlist_path()
        .map(normalized_device_folder)
        .unwrap_or_else(|| normalized_device_folder(&managed_subfolder));
    let playlist_subfolder = match validate_device_relative_folder(&raw_playlist_subfolder) {
        Ok(path) => path,
        Err(e) => {
            warnings.push(format!("[M3U] Invalid playlist folder: {}", e));
            return warnings;
        }
    };

    if let Err(e) = device_io.ensure_dir(&playlist_subfolder).await {
        warnings.push(format!(
            "[M3U] Failed to create playlist folder {}: {}",
            playlist_subfolder, e
        ));
        return warnings;
    }

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
        let rel_path = playlist_manifest_rel_path(&entry.filename, &playlist_subfolder);
        match device_io.delete_file(&rel_path).await {
            Ok(()) => {
                println!("[M3U] Deleted removed playlist: {}", rel_path);
            }
            Err(e) if is_missing_delete_error(&e) => {}
            Err(e) => {
                warnings.push(format!("[M3U] Failed to delete {}: {}", rel_path, e));
                continue;
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

        let resolved_track_ids: Vec<String> = resolved_tracks
            .iter()
            .map(|(t, _)| t.jellyfin_id.clone())
            .collect();

        let rel_m3u = prefixed_device_path(&playlist_subfolder, &m3u_filename);

        // Determine if regeneration is needed (filename or resolved track list changed)
        let (needs_write, old_filename_opt) = match manifest
            .playlists
            .iter()
            .find(|e| e.jellyfin_id == playlist.jellyfin_id)
        {
            None => (true, None),
            Some(e) => {
                let old_rel_m3u = playlist_manifest_rel_path(&e.filename, &playlist_subfolder);
                let changed = old_rel_m3u != rel_m3u || e.track_ids != resolved_track_ids;
                (changed, Some(e.filename.clone()))
            }
        };

        if !needs_write {
            if device_file_exists(device_io.as_ref(), &rel_m3u).await {
                println!("[M3U] Playlist unchanged, skipping: {}", m3u_filename);
                continue;
            }
            println!(
                "[M3U] Playlist manifest unchanged but file missing, rewriting: {}",
                m3u_filename
            );
        }

        // Build M3U content
        let mut lines: Vec<String> = vec!["#EXTM3U".to_string()];
        for (track, rel_path) in &resolved_tracks {
            let label = match &track.artist {
                Some(a) => format!("{} - {}", a, extract_display_name(rel_path)),
                None => extract_display_name(rel_path).to_string(),
            };
            lines.push(format!("#EXTINF:{},{}", track.run_time_seconds, label));
            // local_path is relative to device_path; M3U entries are relative to the
            // playlist folder and always use forward slashes.
            let track_entry = relative_device_path_from_folder(&playlist_subfolder, rel_path);
            lines.push(track_entry);
        }

        let content = lines.join("\n") + "\n";

        // Write via device IO abstraction (handles Write-Temp-Rename internally)
        match device_io
            .write_with_verify(&rel_m3u, content.as_bytes())
            .await
        {
            Ok(()) => {
                println!(
                    "[M3U] Wrote {}: {} tracks",
                    m3u_filename,
                    resolved_tracks.len()
                );

                // Delete old file if the playlist was renamed
                if let Some(old_fn) = &old_filename_opt
                    && *old_fn != m3u_filename
                {
                    let rel_old = playlist_manifest_rel_path(old_fn, &playlist_subfolder);
                    if rel_old != rel_m3u
                        && let Err(e) = device_io.delete_file(&rel_old).await
                        && !is_missing_delete_error(&e)
                    {
                        warnings.push(format!(
                            "[M3U] Failed to delete old file {}: {}",
                            rel_old, e
                        ));
                    }
                }

                let now = now_iso8601();
                manifest
                    .playlists
                    .retain(|e| e.jellyfin_id != playlist.jellyfin_id);
                manifest
                    .playlists
                    .push(crate::device::PlaylistManifestEntry {
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
    let profile_dirty = manifest.transcoding_profile_dirty;
    let music_folder = manifest
        .managed_paths
        .first()
        .map(|path| normalized_device_folder(path))
        .unwrap_or_default();
    // Pre-index desired items for O(1) lookup — avoids O(N×M) scans in both passes below.
    let desired_by_id: std::collections::HashMap<&str, &DesiredItem> = desired_items
        .iter()
        .map(|d| (d.jellyfin_id.as_str(), d))
        .collect();
    let current_ids: HashSet<&str> = manifest
        .synced_items
        .iter()
        .filter(|i| {
            let desired = desired_by_id.get(i.jellyfin_id.as_str()).copied();
            let outside_music_folder = !device_path_in_or_equal(&i.local_path, &music_folder);
            if outside_music_folder {
                return false;
            }
            if profile_dirty && desired.is_some() {
                return false;
            }
            // Quality-upgrade check: re-sync when server reports higher bitrate than recorded,
            // or when the manifest entry has no bitrate recorded (old manifest, populate on next sync).
            if let Some(desired) = desired
                && bitrate_stale_reason(desired.original_bitrate, i.original_bitrate).is_some()
            {
                return false;
            }
            true
        })
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
        .map(|i| {
            let reason_code = manifest
                .synced_items
                .iter()
                .find(|item| item.jellyfin_id == i.jellyfin_id)
                .and_then(|item| {
                    if profile_dirty {
                        return Some("transcoding-profile-change");
                    }
                    if !device_path_in_or_equal(&item.local_path, &music_folder) {
                        return Some("music-folder-change");
                    }
                    bitrate_stale_reason(i.original_bitrate, item.original_bitrate)
                })
                .unwrap_or("new-selection");
            annotate_add(
                SyncAddItem {
                    jellyfin_id: i.jellyfin_id.clone(),
                    name: i.name.clone(),
                    album: i.album.clone(),
                    artist: i.artist.clone(),
                    size_bytes: i.size_bytes,
                    etag: i.etag.clone(),
                    provider_album_id: i.provider_album_id.clone(),
                    provider_content_type: i.provider_content_type.clone(),
                    provider_suffix: i.provider_suffix.clone(),
                    original_bitrate: i.original_bitrate,
                    track_number: i.track_number,
                    reason_code: None,
                    reason: None,
                    server_id: i.server_id.clone(),
                    // Story 13.1: tier is patched onto delta.adds post-calculation (patch_delta_tiers)
                    // from the auto-fill results, since DesiredItem does not carry it.
                    tier: None,
                },
                reason_code,
            )
        })
        .collect();

    // Initial deletes: manifest items not in desired set
    // AND build the metadata map in the same pass
    let mut deletes: Vec<SyncDeleteItem> = Vec::new();
    let mut delete_by_metadata: HashMap<(String, Option<String>, Option<String>), Vec<usize>> =
        HashMap::new();
    let mut relocation_delete_indices: HashSet<usize> = HashSet::new();
    let synced_by_id: HashMap<&str, &SyncedItem> = manifest
        .synced_items
        .iter()
        .map(|i| (i.jellyfin_id.as_str(), i))
        .collect();
    // Index original_name by jellyfin_id for ID-change preservation (AC #4 requirement)
    let original_name_by_id: HashMap<&str, Option<&str>> = manifest
        .synced_items
        .iter()
        .map(|i| (i.jellyfin_id.as_str(), i.original_name.as_deref()))
        .collect();

    for item in &manifest.synced_items {
        let stale_for_profile = profile_dirty && desired_ids.contains(item.jellyfin_id.as_str());
        let stale_for_relocation = !device_path_in_or_equal(&item.local_path, &music_folder);
        let stale_for_quality = desired_by_id
            .get(item.jellyfin_id.as_str())
            .copied()
            .and_then(|desired| {
                bitrate_stale_reason(desired.original_bitrate, item.original_bitrate)
            })
            .is_some();
        let reason_code = if stale_for_profile {
            "transcoding-profile-change"
        } else if stale_for_relocation {
            "music-folder-change"
        } else {
            desired_by_id
                .get(item.jellyfin_id.as_str())
                .copied()
                .and_then(|desired| {
                    bitrate_stale_reason(desired.original_bitrate, item.original_bitrate)
                })
                .unwrap_or("removed-selection")
        };
        if stale_for_profile
            || stale_for_relocation
            || stale_for_quality
            || !desired_ids.contains(item.jellyfin_id.as_str())
        {
            let idx = deletes.len();
            deletes.push(annotate_delete(
                SyncDeleteItem {
                    jellyfin_id: item.jellyfin_id.clone(),
                    local_path: item.local_path.clone(),
                    name: item.name.clone(),
                    reason_code: None,
                    reason: None,
                },
                reason_code,
            ));
            if stale_for_relocation {
                relocation_delete_indices.insert(idx);
            }

            let key = (
                item.name.to_lowercase(),
                item.album.as_ref().map(|a| a.to_lowercase()),
                item.artist.as_ref().map(|a| a.to_lowercase()),
            );
            delete_by_metadata.entry(key).or_default().push(idx);
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
            if let Some(&del_idx) = del_indices.iter().find(|&&idx| {
                if matched_delete_indices.contains(&idx) || relocation_delete_indices.contains(&idx)
                {
                    return false;
                }
                let del = &deletes[idx];
                synced_by_id
                    .get(del.jellyfin_id.as_str())
                    .map(|old| id_change_candidate_matches(add, old))
                    .unwrap_or(true)
            }) {
                matched_add_indices.insert(add_idx);
                matched_delete_indices.insert(del_idx);

                let del = &deletes[del_idx];
                if del.jellyfin_id == add.jellyfin_id {
                    matched_add_indices.remove(&add_idx);
                    matched_delete_indices.remove(&del_idx);
                    continue;
                }
                // Preserve original_name from the old manifest entry (AC #4: must not lose mapping)
                let preserved_original_name = original_name_by_id
                    .get(del.jellyfin_id.as_str())
                    .and_then(|&v| v)
                    .map(|s| s.to_string());
                id_changes.push(annotate_id_change(
                    SyncIdChangeItem {
                        old_jellyfin_id: del.jellyfin_id.clone(),
                        new_jellyfin_id: add.jellyfin_id.clone(),
                        old_local_path: del.local_path.clone(),
                        name: add.name.clone(),
                        album: add.album.clone(),
                        artist: add.artist.clone(),
                        size_bytes: add.size_bytes,
                        etag: add.etag.clone(),
                        provider_album_id: add.provider_album_id.clone(),
                        provider_content_type: add.provider_content_type.clone(),
                        provider_suffix: add.provider_suffix.clone(),
                        original_name: preserved_original_name,
                        reason_code: None,
                        reason: None,
                        source_server_id: add.server_id.clone(),
                    },
                    "server-id-change",
                ));
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
            auto_fill: crate::device::AutoFillConfig::default(),
            transcoding_profile_id: None,
            playlists: vec![],
            storage_id: None,
            ..Default::default()
        }
    }

    #[test]
    fn transfer_timing_keeps_sub_millisecond_speed() {
        let timing = transfer_timing(1_000_000, std::time::Duration::from_micros(500));

        assert_eq!(timing.elapsed_ms, 0.5);
        assert_eq!(timing.speed_mb_s, 2000.0);
    }

    #[test]
    fn transfer_timing_handles_zero_duration() {
        let timing = transfer_timing(1_000_000, std::time::Duration::ZERO);

        assert_eq!(timing.elapsed_ms, 0.0);
        assert_eq!(timing.speed_mb_s, 0.0);
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
            provider_album_id: None,
            provider_content_type: None,
            provider_suffix: None,
            original_bitrate: None,
            original_container: None,
            track_number: None,
            server_id: None,
        }
    }

    #[derive(Debug)]
    struct MissingDeleteDeviceIo;

    #[async_trait::async_trait]
    impl crate::device_io::DeviceIO for MissingDeleteDeviceIo {
        async fn read_file(&self, _path: &str) -> anyhow::Result<Vec<u8>> {
            Ok(Vec::new())
        }

        async fn write_file(&self, _path: &str, _data: &[u8]) -> anyhow::Result<()> {
            Ok(())
        }

        async fn write_with_verify(&self, _path: &str, _data: &[u8]) -> anyhow::Result<()> {
            Ok(())
        }

        async fn delete_file(&self, _path: &str) -> anyhow::Result<()> {
            Err(std::io::Error::new(std::io::ErrorKind::NotFound, "missing").into())
        }

        async fn list_files(
            &self,
            _path: &str,
        ) -> anyhow::Result<Vec<crate::device_io::FileEntry>> {
            Ok(Vec::new())
        }

        async fn free_space(&self) -> anyhow::Result<u64> {
            Ok(1)
        }

        async fn ensure_dir(&self, _path: &str) -> anyhow::Result<()> {
            Ok(())
        }

        async fn cleanup_empty_subdirs(&self, _path: &str) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_execute_sync_removes_manifest_entry_when_managed_file_missing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        tokio::fs::create_dir_all(root.join("Music/Artist"))
            .await
            .unwrap();

        let mut manifest = empty_manifest();
        manifest.synced_items = vec![make_synced_item(
            "stale-id",
            "Missing Track",
            Some("Album"),
            Some("Artist"),
        )];
        crate::device::write_manifest(
            Arc::new(crate::device_io::MscBackend::new(root.clone())),
            &manifest,
        )
        .await
        .unwrap();

        let db = Arc::new(crate::db::Database::memory().unwrap());
        let manager = Arc::new(crate::device::DeviceManager::new(db));
        let device_io: Arc<dyn crate::device_io::DeviceIO> =
            Arc::new(crate::device_io::MscBackend::new(root.clone()));
        manager
            .handle_device_detected(root.clone(), manifest, Arc::clone(&device_io))
            .await
            .unwrap();

        let delta = SyncDelta {
            adds: vec![],
            deletes: vec![SyncDeleteItem {
                jellyfin_id: "stale-id".to_string(),
                local_path: "Music/Artist/Missing Track.flac".to_string(),
                name: "Missing Track".to_string(),
                reason_code: Some("removed-selection".to_string()),
                reason: Some("removed from sync selection".to_string()),
            }],
            id_changes: vec![],
            unchanged: 0,
            playlists: vec![],
        };

        let (_synced, errors) = execute_sync(
            &delta,
            &root,
            &crate::api::JellyfinClient::new(),
            "",
            "",
            "",
            Arc::new(SyncOperationManager::new()),
            "op-missing-delete".to_string(),
            Arc::clone(&manager),
            None,
            device_io,
        )
        .await
        .unwrap();

        assert!(errors.is_empty(), "missing managed file is already gone");
        let updated = manager.get_current_device().await.unwrap();
        assert!(
            updated.synced_items.is_empty(),
            "stale manifest entry must be removed after idempotent cleanup"
        );

        let manifest_json = tokio::fs::read_to_string(root.join(".hifimule.json"))
            .await
            .unwrap();
        let persisted: crate::device::DeviceManifest =
            serde_json::from_str(&manifest_json).unwrap();
        assert!(
            persisted.synced_items.is_empty(),
            "manifest cleanup must be persisted"
        );
    }

    #[tokio::test]
    async fn test_execute_sync_counts_already_absent_delete_as_completed() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();

        let mut manifest = empty_manifest();
        manifest.synced_items = vec![make_synced_item(
            "stale-id",
            "Missing Track",
            Some("Album"),
            Some("Artist"),
        )];
        crate::device::write_manifest(
            Arc::new(crate::device_io::MscBackend::new(root.clone())),
            &manifest,
        )
        .await
        .unwrap();

        let db = Arc::new(crate::db::Database::memory().unwrap());
        let manager = Arc::new(crate::device::DeviceManager::new(db));
        let manifest_io: Arc<dyn crate::device_io::DeviceIO> =
            Arc::new(crate::device_io::MscBackend::new(root.clone()));
        manager
            .handle_device_detected(root.clone(), manifest, Arc::clone(&manifest_io))
            .await
            .unwrap();

        let delta = SyncDelta {
            adds: vec![],
            deletes: vec![SyncDeleteItem {
                jellyfin_id: "stale-id".to_string(),
                local_path: "Music/Artist/Missing Track.flac".to_string(),
                name: "Missing Track".to_string(),
                reason_code: Some("removed-selection".to_string()),
                reason: Some("removed from sync selection".to_string()),
            }],
            id_changes: vec![],
            unchanged: 0,
            playlists: vec![],
        };
        let operation_manager = Arc::new(SyncOperationManager::new());
        let operation_id = "op-missing-delete-progress".to_string();
        operation_manager
            .create_operation(operation_id.clone(), 1)
            .await;

        let (_synced, errors) = execute_sync(
            &delta,
            &root,
            &crate::api::JellyfinClient::new(),
            "",
            "",
            "",
            Arc::clone(&operation_manager),
            operation_id.clone(),
            Arc::clone(&manager),
            None,
            Arc::new(MissingDeleteDeviceIo),
        )
        .await
        .unwrap();

        assert!(errors.is_empty());
        let operation = operation_manager
            .get_operation(&operation_id)
            .await
            .unwrap();
        assert_eq!(operation.files_completed, 1);
    }

    #[test]
    fn test_delete_validation_rejects_unmanaged_relative_path() {
        assert!(relative_path_is_in_managed_subfolder(
            "Music/Artist/Track.flac",
            "Music"
        ));
        assert!(!relative_path_is_in_managed_subfolder(
            "Podcasts/Track.flac",
            "Music"
        ));
        assert!(!relative_path_is_in_managed_subfolder(
            "Music/../Podcasts/Track.flac",
            "Music"
        ));
        assert!(!relative_path_is_in_managed_subfolder(
            "Music\\..\\Podcasts\\Track.flac",
            "Music"
        ));
        assert!(!relative_path_is_in_managed_subfolder(
            "\\Music\\Artist\\Track.flac",
            "Music"
        ));
        assert!(!relative_path_is_in_managed_subfolder("Music", "Music"));
        assert!(!relative_path_is_in_managed_subfolder("", ""));
        assert!(relative_path_is_in_managed_subfolder("Track.flac", ""));
        assert!(!relative_path_is_in_managed_subfolder(
            "Music2/Track.flac",
            "Music"
        ));
        assert!(!relative_path_is_in_managed_subfolder(
            "Music../Track.flac",
            "Music"
        ));
    }

    #[test]
    fn test_missing_delete_error_classification_is_narrow() {
        let io_missing: anyhow::Error =
            std::io::Error::new(std::io::ErrorKind::NotFound, "missing").into();
        assert!(is_missing_delete_error(&io_missing));
        assert!(is_missing_delete_error(&anyhow::anyhow!(
            "Le fichier specifie est introuvable. (os error 2)"
        )));
        assert!(is_missing_delete_error(&anyhow::anyhow!(
            "libmtp: path component 'missing.mp3' not found"
        )));
        assert!(is_missing_delete_error(&anyhow::anyhow!(
            "MTP path component not found: missing.mp3"
        )));
        assert!(!is_missing_delete_error(&anyhow::anyhow!(
            "MTP device 1:2 not found"
        )));
        assert!(!is_missing_delete_error(&anyhow::anyhow!(
            "WPD: device 'Phone' not found in Shell namespace under This PC"
        )));
        assert!(!is_missing_delete_error(&anyhow::anyhow!(
            "file not found on device after transfer"
        )));
    }

    #[test]
    fn test_msc_delete_validation_allows_missing_managed_directory() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let managed_path = root.join("Music");

        assert!(
            validate_delete_path_for_managed_zone(
                &root,
                &managed_path,
                Some("Music"),
                "Music/Artist/Missing Track.flac",
                false,
                &HashSet::new(),
            )
            .is_ok()
        );
    }

    #[test]
    fn test_msc_delete_validation_allows_manifest_owned_relocation_path() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let managed_path = root.join("Audio");
        let mut owned_paths = HashSet::new();
        owned_paths.insert("Music/Artist/Old.flac".to_string());

        assert!(
            validate_delete_path_for_managed_zone(
                &root,
                &managed_path,
                Some("Audio"),
                "Music/Artist/Old.flac",
                false,
                &owned_paths,
            )
            .is_ok()
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_msc_delete_validation_rejects_symlink_escape() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let managed_path = root.join("Music");
        std::fs::create_dir_all(&managed_path).unwrap();
        std::fs::write(outside.path().join("Track.flac"), b"outside").unwrap();
        std::os::unix::fs::symlink(outside.path(), managed_path.join("link")).unwrap();

        let err = validate_delete_path_for_managed_zone(
            &root,
            &managed_path,
            Some("Music"),
            "Music/link/Track.flac",
            false,
            &HashSet::new(),
        )
        .unwrap_err();

        assert_eq!(err, "File is not in managed zone - refusing to delete");
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
            parent_index_number: None,
            parent_id: None,
            album_id: None,
            artist_items: None,
            container: container.map(|s| s.to_string()),
            production_year: None,
            recursive_item_count: None,
            song_count: None,
            cumulative_run_time_ticks: None,
            run_time_ticks: None,
            bitrate: None,
            media_sources: None,
            image_tags: None,
            etag: None,
            user_data: None,
            date_created: None,
            playlist_item_id: None,
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
            provider_album_id: None,
            provider_content_type: None,
            provider_suffix: None,
            original_bitrate: None,
            track_number: None,
            server_id: None,
        }
    }

    // AC27: calculate_delta carries each desired item's server_id onto its add, so
    // execute can route the download to the correct provider.
    #[test]
    fn test_calculate_delta_propagates_server_id() {
        let mut a = make_desired("track-a", "A", Some("Alb"), Some("Art"));
        a.server_id = Some("server-jelly".to_string());
        let mut b = make_desired("track-b", "B", Some("Alb"), Some("Art"));
        b.server_id = Some("server-navi".to_string());

        let manifest = DeviceManifest {
            device_id: "dev".to_string(),
            name: None,
            icon: None,
            version: "1.0".to_string(),
            managed_paths: vec!["Music".to_string()],
            synced_items: vec![],
            ..Default::default()
        };
        let delta = calculate_delta(&[a, b], &manifest);
        assert_eq!(delta.adds.len(), 2);
        let by_id: std::collections::HashMap<_, _> = delta
            .adds
            .iter()
            .map(|add| (add.jellyfin_id.as_str(), add.server_id.as_deref()))
            .collect();
        assert_eq!(by_id["track-a"], Some("server-jelly"));
        assert_eq!(by_id["track-b"], Some("server-navi"));
    }

    fn generic_mp3_profile() -> serde_json::Value {
        serde_json::json!({
            "Name": "Test Generic MP3",
            "MaxStreamingBitrate": 320000,
            "MusicStreamingTranscodingBitrate": 320000,
            "DirectPlayProfiles": [
                { "Container": "mp3", "Type": "Audio", "AudioCodec": "mp3" }
            ],
            "TranscodingProfiles": [
                {
                    "Container": "mp3",
                    "Type": "Audio",
                    "AudioCodec": "mp3",
                    "Protocol": "http",
                    "EstimateContentLength": true,
                    "EnableMpegtsM2TsMode": false
                }
            ],
            "CodecProfiles": []
        })
    }

    fn rockbox_direct_profile() -> serde_json::Value {
        serde_json::json!({
            "Name": "Test Rockbox",
            "MaxStreamingBitrate": 320000,
            "MusicStreamingTranscodingBitrate": 320000,
            "DirectPlayProfiles": [
                { "Container": "mp3", "Type": "Audio", "AudioCodec": "mp3" },
                { "Container": "flac", "Type": "Audio", "AudioCodec": "flac" }
            ],
            "TranscodingProfiles": [
                {
                    "Container": "mp3",
                    "Type": "Audio",
                    "AudioCodec": "mp3",
                    "Protocol": "http",
                    "EstimateContentLength": true,
                    "EnableMpegtsM2TsMode": false
                }
            ],
            "CodecProfiles": []
        })
    }

    fn m4a_aac_direct_profile() -> serde_json::Value {
        serde_json::json!({
            "Name": "Test M4A AAC",
            "MaxStreamingBitrate": 256000,
            "MusicStreamingTranscodingBitrate": 256000,
            "DirectPlayProfiles": [
                { "Container": "m4a", "Type": "Audio", "AudioCodec": "aac" }
            ],
            "TranscodingProfiles": [
                {
                    "Container": "mp3",
                    "Type": "Audio",
                    "AudioCodec": "mp3",
                    "Protocol": "http",
                    "EstimateContentLength": true,
                    "EnableMpegtsM2TsMode": false
                }
            ],
            "CodecProfiles": []
        })
    }

    fn modern_dap_lossless_profile() -> serde_json::Value {
        serde_json::json!({
            "Name": "Test Modern DAP Lossless",
            "MaxStreamingBitrate": 9216000,
            "MusicStreamingTranscodingBitrate": 9216000,
            "DirectPlayProfiles": [
                { "Container": "mp3", "Type": "Audio", "AudioCodec": "mp3" },
                { "Container": "mp4", "Type": "Audio", "AudioCodec": "aac" },
                { "Container": "m4a", "Type": "Audio", "AudioCodec": "aac" },
                { "Container": "m4a", "Type": "Audio", "AudioCodec": "alac" },
                { "Container": "flac", "Type": "Audio", "AudioCodec": "flac" },
                { "Container": "ogg", "Type": "Audio", "AudioCodec": "vorbis" },
                { "Container": "opus", "Type": "Audio", "AudioCodec": "opus" },
                { "Container": "wav", "Type": "Audio", "AudioCodec": "pcm_s16le" }
            ],
            "TranscodingProfiles": [
                {
                    "Container": "flac",
                    "Type": "Audio",
                    "AudioCodec": "flac",
                    "Protocol": "http",
                    "EstimateContentLength": true,
                    "EnableMpegtsM2TsMode": false
                }
            ],
            "CodecProfiles": []
        })
    }

    fn provider_credentials(server_url: String) -> crate::providers::ProviderCredentials {
        crate::providers::ProviderCredentials {
            server_url,
            credential: crate::providers::CredentialKind::Password {
                username: "tester".to_string(),
                password: "secret".to_string(),
            },
        }
    }

    fn subsonic_provider(server_url: String) -> Arc<dyn crate::providers::MediaProvider> {
        Arc::new(
            crate::providers::subsonic::SubsonicProvider::from_stored_config(
                provider_credentials(server_url),
                true,
                Some("1.16.1".to_string()),
            )
            .expect("subsonic provider"),
        )
    }

    #[test]
    fn test_audio_compatibility_accepts_mp4_container_when_profile_supports_common_mp4_audio() {
        let compatibility = audio_compatibility_profile(Some(&m4a_aac_direct_profile()), None);
        let m4a_container_only = provider_audio_format(Some("m4a"), Some("audio/mp4"));
        let confirmed_aac = provider_audio_format(Some("m4a"), Some("audio/aac"));

        assert!(
            compatibility.source_is_direct_compatible(&m4a_container_only),
            "m4a/mp4 container metadata should direct-download when the profile supports AAC/M4A"
        );
        assert!(
            compatibility.source_is_direct_compatible(&confirmed_aac),
            "explicit AAC metadata with an M4A suffix should satisfy the profile"
        );
    }

    #[test]
    fn test_modern_dap_direct_formats_are_detected_from_expanded_metadata() {
        let compatibility = audio_compatibility_profile(Some(&modern_dap_lossless_profile()), None);

        assert!(
            compatibility.source_is_direct_compatible(&provider_audio_format(
                Some("flac"),
                Some("audio/flac")
            )),
            "FLAC metadata should direct-download for Modern DAP"
        );
        assert!(
            compatibility.source_is_direct_compatible(&provider_audio_format(
                Some("mp3"),
                Some("audio/mpeg")
            )),
            "MP3 metadata should direct-download for Modern DAP"
        );
        assert!(
            compatibility.source_is_direct_compatible(&provider_audio_format(
                Some("m4a"),
                Some("audio/mp4")
            )),
            "M4A/MP4 metadata should direct-download for Modern DAP"
        );
        assert!(
            compatibility.source_is_direct_compatible(&provider_audio_format(
                Some("wav"),
                Some("audio/wav")
            )),
            "WAV metadata should direct-download for Modern DAP"
        );
    }

    #[test]
    fn test_explicit_device_profile_takes_precedence_over_mtp_preferred_container() {
        let compatibility =
            audio_compatibility_profile(Some(&modern_dap_lossless_profile()), Some("mp3"));

        assert!(
            compatibility.source_is_direct_compatible(&provider_audio_format(
                Some("flac"),
                Some("audio/flac")
            )),
            "Modern DAP FLAC direct-play support should not be overridden by an MTP mp3 preference"
        );
        assert_eq!(compatibility.transcode_target_label(), "flac");
    }

    #[test]
    fn test_mtp_preferred_container_still_applies_without_device_profile() {
        let compatibility = audio_compatibility_profile(None, Some("mp3"));

        assert!(
            compatibility.source_is_direct_compatible(&provider_audio_format(
                Some("mp3"),
                Some("audio/mpeg")
            )),
            "MP3 should remain direct-compatible for the MTP fallback"
        );
        assert!(
            !compatibility.source_is_direct_compatible(&provider_audio_format(
                Some("flac"),
                Some("audio/flac")
            )),
            "The MTP fallback should still force non-MP3 sources to transcode when no profile is selected"
        );
    }

    fn add_item_with_provider_format(
        id: &str,
        suffix: &str,
        content_type: &str,
        size_bytes: u64,
    ) -> SyncAddItem {
        SyncAddItem {
            jellyfin_id: id.to_string(),
            name: format!("Track {id}"),
            album: Some("Album".to_string()),
            artist: Some("Artist".to_string()),
            size_bytes,
            etag: None,
            provider_album_id: Some("album1".to_string()),
            provider_content_type: Some(content_type.to_string()),
            provider_suffix: Some(suffix.to_string()),
            original_bitrate: None,
            track_number: None,
            reason_code: Some("new-selection".to_string()),
            reason: Some("new selection".to_string()),
            server_id: None,
            tier: None,
        }
    }

    async fn setup_provider_sync_device(
        root: &Path,
    ) -> (
        Arc<crate::device::DeviceManager>,
        Arc<dyn crate::device_io::DeviceIO>,
    ) {
        let manifest = empty_manifest();
        crate::device::write_manifest(
            Arc::new(crate::device_io::MscBackend::new(root.to_path_buf())),
            &manifest,
        )
        .await
        .unwrap();
        let manager = Arc::new(crate::device::DeviceManager::new(Arc::new(
            crate::db::Database::memory().unwrap(),
        )));
        let device_io: Arc<dyn crate::device_io::DeviceIO> =
            Arc::new(crate::device_io::MscBackend::new(root.to_path_buf()));
        manager
            .handle_device_detected(root.to_path_buf(), manifest, Arc::clone(&device_io))
            .await
            .unwrap();
        (manager, device_io)
    }

    #[tokio::test]
    async fn test_execute_provider_sync_transcodes_subsonic_flac_to_mp3_with_kbps() {
        let mut server = mockito::Server::new_async().await;
        let _stream = server
            .mock("GET", "/rest/stream.view")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("id".into(), "song-flac".into()),
                mockito::Matcher::UrlEncoded("format".into(), "mp3".into()),
                mockito::Matcher::UrlEncoded("maxBitRate".into(), "320".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "audio/mpeg")
            .with_body(vec![1_u8, 2, 3, 4])
            .expect(1)
            .create_async()
            .await;

        let dir = tempfile::tempdir().unwrap();
        let (manager, device_io) = setup_provider_sync_device(dir.path()).await;
        let operation_manager = Arc::new(SyncOperationManager::new());
        let operation_id = "op-transcode-mp3".to_string();
        operation_manager
            .create_operation(operation_id.clone(), 1)
            .await;
        let delta = SyncDelta {
            adds: vec![add_item_with_provider_format(
                "song-flac",
                "flac",
                "audio/flac",
                4,
            )],
            deletes: vec![],
            id_changes: vec![],
            unchanged: 0,
            playlists: vec![],
        };

        let (synced, errors) = execute_provider_sync(
            &delta,
            dir.path(),
            ProviderSyncSource {
                provider: subsonic_provider(server.url()),
                transcoding_profile: Some(generic_mp3_profile()),
            },
            Arc::clone(&operation_manager),
            operation_id.clone(),
            Arc::clone(&manager),
            device_io,
        )
        .await
        .unwrap();

        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(synced.len(), 1);
        assert!(
            synced[0].local_path.ends_with(".mp3"),
            "transcoded output must use confirmed mp3 extension: {}",
            synced[0].local_path
        );
        assert!(
            dir.path().join(&synced[0].local_path).exists(),
            "transcoded file should be written"
        );
        let manifest = manager.get_current_device().await.unwrap();
        assert_eq!(manifest.synced_items.len(), 1);
        let operation = operation_manager
            .get_operation(&operation_id)
            .await
            .unwrap();
        assert!(operation.warnings.is_empty(), "{:?}", operation.warnings);
    }

    #[tokio::test]
    async fn test_execute_provider_sync_preserves_compatible_direct_suffix() {
        let mut server = mockito::Server::new_async().await;
        let _download = server
            .mock("GET", "/rest/download.view")
            .match_query(mockito::Matcher::AllOf(vec![mockito::Matcher::UrlEncoded(
                "id".into(),
                "song-flac".into(),
            )]))
            .with_status(200)
            .with_header("content-type", "application/octet-stream")
            .with_body(vec![1_u8, 2, 3, 4])
            .expect(1)
            .create_async()
            .await;
        let _stream = server
            .mock("GET", "/rest/stream.view")
            .expect(0)
            .create_async()
            .await;

        let dir = tempfile::tempdir().unwrap();
        let (manager, device_io) = setup_provider_sync_device(dir.path()).await;
        let operation_manager = Arc::new(SyncOperationManager::new());
        let operation_id = "op-direct-flac".to_string();
        operation_manager
            .create_operation(operation_id.clone(), 1)
            .await;
        let delta = SyncDelta {
            adds: vec![add_item_with_provider_format(
                "song-flac",
                "flac",
                "audio/flac",
                4,
            )],
            deletes: vec![],
            id_changes: vec![],
            unchanged: 0,
            playlists: vec![],
        };

        let (synced, errors) = execute_provider_sync(
            &delta,
            dir.path(),
            ProviderSyncSource {
                provider: subsonic_provider(server.url()),
                transcoding_profile: Some(rockbox_direct_profile()),
            },
            Arc::clone(&operation_manager),
            operation_id.clone(),
            Arc::clone(&manager),
            device_io,
        )
        .await
        .unwrap();

        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(synced.len(), 1);
        assert!(
            synced[0].local_path.ends_with(".flac"),
            "compatible passthrough should keep source suffix: {}",
            synced[0].local_path
        );
    }

    #[tokio::test]
    async fn test_execute_provider_sync_skips_incompatible_direct_response() {
        let mut server = mockito::Server::new_async().await;
        let _stream = server
            .mock("GET", "/rest/stream.view")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("id".into(), "song-flac".into()),
                mockito::Matcher::UrlEncoded("format".into(), "mp3".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "audio/flac")
            .with_body(vec![1_u8, 2, 3, 4])
            .expect(1)
            .create_async()
            .await;

        let dir = tempfile::tempdir().unwrap();
        let (manager, device_io) = setup_provider_sync_device(dir.path()).await;
        let operation_manager = Arc::new(SyncOperationManager::new());
        let operation_id = "op-skip-incompatible".to_string();
        operation_manager
            .create_operation(operation_id.clone(), 1)
            .await;
        let delta = SyncDelta {
            adds: vec![add_item_with_provider_format(
                "song-flac",
                "flac",
                "audio/flac",
                4,
            )],
            deletes: vec![],
            id_changes: vec![],
            unchanged: 0,
            playlists: vec![],
        };

        let (synced, errors) = execute_provider_sync(
            &delta,
            dir.path(),
            ProviderSyncSource {
                provider: subsonic_provider(server.url()),
                transcoding_profile: Some(generic_mp3_profile()),
            },
            Arc::clone(&operation_manager),
            operation_id.clone(),
            Arc::clone(&manager),
            device_io,
        )
        .await
        .unwrap();

        assert!(errors.is_empty(), "{errors:?}");
        assert!(
            synced.is_empty(),
            "incompatible passthrough must be skipped"
        );
        assert!(
            manager
                .get_current_device()
                .await
                .unwrap()
                .synced_items
                .is_empty(),
            "skipped items must stay out of the manifest"
        );
        let operation = operation_manager
            .get_operation(&operation_id)
            .await
            .unwrap();
        assert_eq!(operation.files_completed, 1);
        assert_eq!(operation.bytes_transferred, 0);
        assert_eq!(operation.total_bytes, 0);
        assert_eq!(operation.warnings.len(), 1);
        assert!(
            operation.warnings[0].contains("incompatible"),
            "warning should explain incompatible output: {:?}",
            operation.warnings
        );
    }

    #[tokio::test]
    async fn test_execute_provider_sync_skips_unconfirmed_transcode_output() {
        let mut server = mockito::Server::new_async().await;
        let _stream = server
            .mock("GET", "/rest/stream.view")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("id".into(), "song-flac".into()),
                mockito::Matcher::UrlEncoded("format".into(), "mp3".into()),
            ]))
            .with_status(200)
            .with_body(vec![1_u8, 2, 3, 4])
            .expect(1)
            .create_async()
            .await;

        let dir = tempfile::tempdir().unwrap();
        let (manager, device_io) = setup_provider_sync_device(dir.path()).await;
        let operation_manager = Arc::new(SyncOperationManager::new());
        let operation_id = "op-skip-unconfirmed".to_string();
        operation_manager
            .create_operation(operation_id.clone(), 1)
            .await;
        let delta = SyncDelta {
            adds: vec![add_item_with_provider_format(
                "song-flac",
                "flac",
                "audio/flac",
                4,
            )],
            deletes: vec![],
            id_changes: vec![],
            unchanged: 0,
            playlists: vec![],
        };

        let (synced, errors) = execute_provider_sync(
            &delta,
            dir.path(),
            ProviderSyncSource {
                provider: subsonic_provider(server.url()),
                transcoding_profile: Some(generic_mp3_profile()),
            },
            Arc::clone(&operation_manager),
            operation_id.clone(),
            Arc::clone(&manager),
            device_io,
        )
        .await
        .unwrap();

        assert!(errors.is_empty(), "{errors:?}");
        assert!(synced.is_empty(), "unconfirmed transcode must be skipped");
        assert!(
            manager
                .get_current_device()
                .await
                .unwrap()
                .synced_items
                .is_empty(),
            "unconfirmed output must stay out of the manifest"
        );
        let operation = operation_manager
            .get_operation(&operation_id)
            .await
            .unwrap();
        assert_eq!(operation.warnings.len(), 1);
        assert!(
            operation.warnings[0].contains("unconfirmed"),
            "warning should explain unconfirmed output: {:?}",
            operation.warnings
        );
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
    fn test_delta_profile_dirty_rewrites_matching_tracks() {
        let mut manifest = empty_manifest();
        manifest.transcoding_profile_id = Some("rockbox-mp3-320".to_string());
        manifest.last_synced_transcoding_profile_id = Some("passthrough".to_string());
        manifest.transcoding_profile_dirty = true;
        manifest.synced_items = vec![make_synced_item(
            "a",
            "Track A",
            Some("Album"),
            Some("Artist"),
        )];

        let desired = vec![make_desired("a", "Track A", Some("Album"), Some("Artist"))];

        let delta = calculate_delta(&desired, &manifest);
        assert_eq!(delta.adds.len(), 1);
        assert_eq!(delta.adds[0].jellyfin_id, "a");
        assert_eq!(
            delta.adds[0].reason_code.as_deref(),
            Some("transcoding-profile-change")
        );
        assert_eq!(delta.deletes.len(), 1);
        assert_eq!(delta.deletes[0].jellyfin_id, "a");
        assert_eq!(
            delta.deletes[0].reason_code.as_deref(),
            Some("transcoding-profile-change")
        );
        assert_eq!(delta.unchanged, 0);
    }

    #[test]
    fn test_delta_missing_local_bitrate_does_not_force_rewrite() {
        let mut manifest = empty_manifest();
        manifest.synced_items = vec![make_synced_item(
            "a",
            "Track A",
            Some("Album"),
            Some("Artist"),
        )];
        let mut desired = make_desired("a", "Track A", Some("Album"), Some("Artist"));
        desired.original_bitrate = Some(320_000);

        let delta = calculate_delta(&[desired], &manifest);

        assert_eq!(delta.adds.len(), 0);
        assert_eq!(delta.deletes.len(), 0);
        assert_eq!(delta.id_changes.len(), 0);
        assert_eq!(delta.unchanged, 1);
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
        assert_eq!(
            delta.id_changes[0].reason_code.as_deref(),
            Some("server-id-change")
        );
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
            parent_index_number: None,
            parent_id: None,
            album_id: None,
            artist_items: None,
            container: Some("flac".to_string()),
            production_year: None,
            recursive_item_count: None,
            song_count: None,
            cumulative_run_time_ticks: None,
            run_time_ticks: None,
            bitrate: None,
            media_sources: None,
            image_tags: None,
            etag: None,
            user_data: None,
            date_created: None,
            playlist_item_id: None,
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
            parent_index_number: None,
            parent_id: None,
            album_id: None,
            artist_items: None,
            container: None,
            production_year: None,
            recursive_item_count: None,
            song_count: None,
            cumulative_run_time_ticks: None,
            run_time_ticks: None,
            bitrate: None,
            media_sources: None,
            image_tags: None,
            etag: None,
            user_data: None,
            date_created: None,
            playlist_item_id: None,
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
    fn test_calculate_delta_does_not_infer_id_change_when_provider_album_differs() {
        let mut synced = make_synced_item("old-id", "Same Song", Some("Album"), Some("Artist"));
        synced.provider_album_id = Some("album-old".to_string());

        let mut desired = make_desired("new-id", "Same Song", Some("Album"), Some("Artist"));
        desired.provider_album_id = Some("album-new".to_string());

        let mut manifest = empty_manifest();
        manifest.synced_items = vec![synced];

        let delta = calculate_delta(&[desired], &manifest);

        assert_eq!(delta.id_changes.len(), 0);
        assert_eq!(delta.adds.len(), 1);
        assert_eq!(delta.deletes.len(), 1);
    }

    #[test]
    fn test_calculate_delta_does_not_infer_id_change_when_track_number_differs() {
        let mut synced = make_synced_item("old-id", "Same Song", Some("Album"), Some("Artist"));
        synced.track_number = Some(1);

        let mut desired = make_desired("new-id", "Same Song", Some("Album"), Some("Artist"));
        desired.track_number = Some(2);

        let mut manifest = empty_manifest();
        manifest.synced_items = vec![synced];

        let delta = calculate_delta(&[desired], &manifest);

        assert_eq!(delta.id_changes.len(), 0);
        assert_eq!(delta.adds.len(), 1);
        assert_eq!(delta.deletes.len(), 1);
    }

    #[test]
    fn test_format_id_change_diagnostics_includes_sample_and_omitted_count() {
        let delta = SyncDelta {
            adds: vec![],
            deletes: vec![],
            id_changes: vec![
                annotate_id_change(
                    SyncIdChangeItem {
                        old_jellyfin_id: "old-1".to_string(),
                        new_jellyfin_id: "new-1".to_string(),
                        old_local_path: "Music/A/Track.flac".to_string(),
                        name: "Track".to_string(),
                        album: Some("Album".to_string()),
                        artist: Some("Artist".to_string()),
                        size_bytes: 123,
                        etag: None,
                        provider_album_id: Some("album-1".to_string()),
                        provider_content_type: Some("audio/flac".to_string()),
                        provider_suffix: Some("flac".to_string()),
                        original_name: None,
                        reason_code: None,
                        reason: None,
                        source_server_id: None,
                    },
                    "server-id-change",
                ),
                annotate_id_change(
                    SyncIdChangeItem {
                        old_jellyfin_id: "old-2".to_string(),
                        new_jellyfin_id: "new-2".to_string(),
                        old_local_path: "Music/A/Other.flac".to_string(),
                        name: "Other".to_string(),
                        album: None,
                        artist: None,
                        size_bytes: 456,
                        etag: None,
                        provider_album_id: None,
                        provider_content_type: None,
                        provider_suffix: None,
                        original_name: None,
                        reason_code: None,
                        reason: None,
                        source_server_id: None,
                    },
                    "server-id-change",
                ),
            ],
            unchanged: 0,
            playlists: vec![],
        };

        let diagnostics = format_id_change_diagnostics(&delta, 1);

        assert!(diagnostics.contains("old-1 -> new-1"));
        assert!(diagnostics.contains("provider_album_id=Some(\"album-1\")"));
        assert!(diagnostics.contains("... 1 more"));
        assert!(!diagnostics.contains("old-2 -> new-2"));
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
            provider_album_id: None,
            provider_content_type: None,
            provider_suffix: None,
            original_bitrate: None,
            original_container: None,
            track_number: None,
            server_id: None,
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
            provider_album_id: None,
            provider_content_type: None,
            provider_suffix: None,
            original_bitrate: None,
            original_container: None,
            track_number: None,
            server_id: None,
        }
    }

    #[test]
    fn test_calculate_delta_cleans_up_tracks_outside_current_music_folder() {
        let desired = vec![DesiredItem {
            jellyfin_id: "t1".to_string(),
            name: "Song".to_string(),
            album: Some("Album".to_string()),
            artist: Some("Artist".to_string()),
            size_bytes: 10,
            etag: None,
            provider_album_id: None,
            provider_content_type: None,
            provider_suffix: Some("flac".to_string()),
            original_bitrate: None,
            track_number: None,
            server_id: None,
        }];
        let mut manifest = empty_manifest();
        manifest.managed_paths = vec!["Audio".to_string()];
        manifest.synced_items = vec![make_playlist_synced_item(
            "t1",
            "Music/Artist/Album/01 - Song.flac",
        )];

        let delta = calculate_delta(&desired, &manifest);

        assert_eq!(
            delta.adds.len(),
            1,
            "track should be rewritten under Audio/"
        );
        assert_eq!(
            delta.deletes.len(),
            1,
            "old Music/ file should be cleaned up"
        );
        assert_eq!(
            delta.deletes[0].local_path,
            "Music/Artist/Album/01 - Song.flac"
        );
        assert_eq!(delta.unchanged, 0);
    }

    #[test]
    fn test_relative_device_path_from_sibling_playlist_folder() {
        assert_eq!(
            relative_device_path_from_folder("Playlists", "Music/A/B/01 - Song.flac"),
            "../Music/A/B/01 - Song.flac"
        );
        assert_eq!(
            relative_device_path_from_folder("Music", "Music/A/B/01 - Song.flac"),
            "A/B/01 - Song.flac"
        );
    }

    #[test]
    fn test_relative_device_path_preserves_case_distinct_folders() {
        assert_eq!(
            relative_device_path_from_folder("music", "Music/A/B/01 - Song.flac"),
            "../Music/A/B/01 - Song.flac"
        );
    }

    #[test]
    fn test_calculate_delta_does_not_convert_relocation_to_id_change() {
        let desired = vec![DesiredItem {
            jellyfin_id: "new-id".to_string(),
            name: "Song".to_string(),
            album: Some("Album".to_string()),
            artist: Some("Artist".to_string()),
            size_bytes: 10,
            etag: None,
            provider_album_id: None,
            provider_content_type: None,
            provider_suffix: Some("flac".to_string()),
            original_bitrate: None,
            track_number: None,
            server_id: None,
        }];
        let mut manifest = empty_manifest();
        manifest.managed_paths = vec!["Audio".to_string()];
        manifest.synced_items = vec![crate::device::SyncedItem {
            jellyfin_id: "old-id".to_string(),
            name: "Song".to_string(),
            album: Some("Album".to_string()),
            artist: Some("Artist".to_string()),
            local_path: "Music/Artist/Album/01 - Song.flac".to_string(),
            size_bytes: 10,
            synced_at: "2026-01-01T00:00:00Z".to_string(),
            original_name: None,
            etag: None,
            provider_album_id: None,
            provider_content_type: None,
            provider_suffix: Some("flac".to_string()),
            original_bitrate: None,
            original_container: None,
            track_number: None,
            server_id: None,
        }];

        let delta = calculate_delta(&desired, &manifest);

        assert_eq!(delta.adds.len(), 1);
        assert_eq!(delta.deletes.len(), 1);
        assert_eq!(delta.id_changes.len(), 0);
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
        let device_io =
            std::sync::Arc::new(crate::device_io::MscBackend::new(device_path.to_path_buf()));
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
        assert!(
            warnings.is_empty(),
            "Expected no warnings, got: {:?}",
            warnings
        );

        // .m3u files should be in the Music folder, not the device root
        let m3u1 = managed_path.join("My Playlist.m3u");
        let m3u2 = managed_path.join("Second Playlist.m3u");
        assert!(m3u1.exists(), "My Playlist.m3u should exist in Music/");
        assert!(m3u2.exists(), "Second Playlist.m3u should exist in Music/");
        assert!(
            !device_path.join("My Playlist.m3u").exists(),
            ".m3u must NOT be at device root"
        );

        // Check content — paths are relative to Music/, so no "Music/" prefix
        let content1 = tokio::fs::read_to_string(&m3u1).await.unwrap();
        assert!(content1.starts_with("#EXTM3U\n"), "Must start with #EXTM3U");
        assert!(content1.contains("#EXTINF:210,Pink Floyd - 01 - In the Flesh"));
        assert!(content1.contains("Pink Floyd/The Wall/01 - In the Flesh.flac"));
        assert!(
            !content1.contains("Music/Pink Floyd"),
            "Path must NOT include Music/ prefix"
        );
        assert!(
            content1.contains("#EXTINF:-1,03 - Unknown"),
            "No-artist track uses filename only"
        );

        // manifest.playlists should have two entries
        assert_eq!(manifest.playlists.len(), 2);
        let entry1 = manifest
            .playlists
            .iter()
            .find(|e| e.jellyfin_id == "pl1")
            .unwrap();
        assert_eq!(entry1.track_count, 3);
        assert_eq!(entry1.track_ids, vec!["t1", "t2", "t3"]);
    }

    #[tokio::test]
    async fn test_generate_m3u_uses_custom_playlist_folder_and_relative_track_paths() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let device_path = tmp_dir.path();
        let managed_path = device_path.join("Music");
        tokio::fs::create_dir_all(&managed_path).await.unwrap();

        let playlist_items = vec![PlaylistSyncItem {
            jellyfin_id: "pl1".to_string(),
            name: "Road".to_string(),
            tracks: vec![PlaylistTrackInfo {
                jellyfin_id: "t1".to_string(),
                artist: Some("Artist".to_string()),
                run_time_seconds: 120,
            }],
        }];
        let all_synced = vec![make_playlist_synced_item(
            "t1",
            "Music/Artist/Album/01 - Song.flac",
        )];
        let mut manifest = empty_manifest();
        manifest.playlist_path = Some("Playlists".to_string());
        let device_io =
            std::sync::Arc::new(crate::device_io::MscBackend::new(device_path.to_path_buf()));

        let warnings = generate_m3u_files(
            &playlist_items,
            device_path,
            &managed_path,
            &all_synced,
            &mut manifest,
            device_io,
        )
        .await;

        assert!(warnings.is_empty(), "No warnings expected: {:?}", warnings);
        let m3u_path = device_path.join("Playlists").join("Road.m3u");
        assert!(
            m3u_path.exists(),
            "playlist should be written to Playlists/"
        );
        assert!(
            !managed_path.join("Road.m3u").exists(),
            "playlist should not be written to Music/"
        );
        let content = tokio::fs::read_to_string(&m3u_path).await.unwrap();
        assert!(
            content.contains("../Music/Artist/Album/01 - Song.flac"),
            "track path should be relative from playlist folder: {content}"
        );
    }

    #[tokio::test]
    async fn test_generate_m3u_removes_manifest_owned_legacy_playlist_from_music_folder() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let device_path = tmp_dir.path();
        let managed_path = device_path.join("Music");
        tokio::fs::create_dir_all(&managed_path).await.unwrap();
        tokio::fs::write(managed_path.join("Road.m3u"), b"#EXTM3U\n")
            .await
            .unwrap();

        let playlist_items = vec![PlaylistSyncItem {
            jellyfin_id: "pl1".to_string(),
            name: "Road".to_string(),
            tracks: vec![PlaylistTrackInfo {
                jellyfin_id: "t1".to_string(),
                artist: None,
                run_time_seconds: 120,
            }],
        }];
        let all_synced = vec![make_playlist_synced_item(
            "t1",
            "Music/Artist/Album/01 - Song.flac",
        )];
        let mut manifest = empty_manifest();
        manifest.playlist_path = Some("Playlists".to_string());
        manifest
            .playlists
            .push(crate::device::PlaylistManifestEntry {
                jellyfin_id: "pl1".to_string(),
                filename: "Music/Road.m3u".to_string(),
                track_count: 1,
                track_ids: vec!["t1".to_string()],
                last_modified: "2026-01-01T00:00:00Z".to_string(),
            });
        let device_io =
            std::sync::Arc::new(crate::device_io::MscBackend::new(device_path.to_path_buf()));

        let warnings = generate_m3u_files(
            &playlist_items,
            device_path,
            &managed_path,
            &all_synced,
            &mut manifest,
            device_io,
        )
        .await;

        assert!(warnings.is_empty(), "No warnings expected: {:?}", warnings);
        assert!(
            !managed_path.join("Road.m3u").exists(),
            "old Music/ playlist should be removed"
        );
        assert!(device_path.join("Playlists").join("Road.m3u").exists());
    }

    #[tokio::test]
    async fn test_generate_m3u_does_not_delete_unowned_same_name_legacy_playlist() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let device_path = tmp_dir.path();
        let managed_path = device_path.join("Music");
        tokio::fs::create_dir_all(&managed_path).await.unwrap();
        tokio::fs::write(managed_path.join("Road.m3u"), b"#EXTM3U\n")
            .await
            .unwrap();

        let playlist_items = vec![PlaylistSyncItem {
            jellyfin_id: "pl1".to_string(),
            name: "Road".to_string(),
            tracks: vec![PlaylistTrackInfo {
                jellyfin_id: "t1".to_string(),
                artist: None,
                run_time_seconds: 120,
            }],
        }];
        let all_synced = vec![make_playlist_synced_item(
            "t1",
            "Music/Artist/Album/01 - Song.flac",
        )];
        let mut manifest = empty_manifest();
        manifest.playlist_path = Some("Playlists".to_string());
        manifest
            .playlists
            .push(crate::device::PlaylistManifestEntry {
                jellyfin_id: "pl1".to_string(),
                filename: "Road.m3u".to_string(),
                track_count: 1,
                track_ids: vec!["t1".to_string()],
                last_modified: "2026-01-01T00:00:00Z".to_string(),
            });
        let device_io =
            std::sync::Arc::new(crate::device_io::MscBackend::new(device_path.to_path_buf()));

        let warnings = generate_m3u_files(
            &playlist_items,
            device_path,
            &managed_path,
            &all_synced,
            &mut manifest,
            device_io,
        )
        .await;

        assert!(warnings.is_empty(), "No warnings expected: {:?}", warnings);
        assert!(
            managed_path.join("Road.m3u").exists(),
            "unowned Music/ playlist should be left alone"
        );
        assert!(device_path.join("Playlists").join("Road.m3u").exists());
    }

    #[tokio::test]
    async fn test_generate_m3u_rejects_invalid_stored_playlist_path_before_creating_dirs() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let device_path = tmp_dir.path();
        let managed_path = device_path.join("Music");
        tokio::fs::create_dir_all(&managed_path).await.unwrap();
        let playlist_items = vec![PlaylistSyncItem {
            jellyfin_id: "pl1".to_string(),
            name: "Road".to_string(),
            tracks: vec![PlaylistTrackInfo {
                jellyfin_id: "t1".to_string(),
                artist: None,
                run_time_seconds: 120,
            }],
        }];
        let all_synced = vec![make_playlist_synced_item(
            "t1",
            "Music/Artist/Album/01 - Song.flac",
        )];
        let mut manifest = empty_manifest();
        manifest.playlist_path = Some("../Outside".to_string());
        let device_io =
            std::sync::Arc::new(crate::device_io::MscBackend::new(device_path.to_path_buf()));

        let warnings = generate_m3u_files(
            &playlist_items,
            device_path,
            &managed_path,
            &all_synced,
            &mut manifest,
            device_io,
        )
        .await;

        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("Invalid playlist folder"));
        assert!(!device_path.join("Outside").exists());
    }

    #[test]
    fn test_destructive_cleanup_count_includes_playlist_relocation() {
        let mut manifest = empty_manifest();
        manifest.playlist_path = Some("Playlists".to_string());
        manifest
            .playlists
            .push(crate::device::PlaylistManifestEntry {
                jellyfin_id: "pl1".to_string(),
                filename: "Music/Road.m3u".to_string(),
                track_count: 1,
                track_ids: vec!["t1".to_string()],
                last_modified: "2026-01-01T00:00:00Z".to_string(),
            });
        let delta = SyncDelta {
            adds: vec![],
            deletes: vec![],
            id_changes: vec![],
            unchanged: 0,
            playlists: vec![PlaylistSyncItem {
                jellyfin_id: "pl1".to_string(),
                name: "Road".to_string(),
                tracks: vec![],
            }],
        };

        assert_eq!(destructive_cleanup_count(&delta, &manifest), 1);
    }

    #[test]
    fn test_change_reason_summary_counts_replacement_pair_once() {
        let delta = SyncDelta {
            adds: vec![annotate_add(
                SyncAddItem {
                    jellyfin_id: "track-1".to_string(),
                    name: "Track".to_string(),
                    album: None,
                    artist: None,
                    size_bytes: 1,
                    etag: None,
                    provider_album_id: None,
                    provider_content_type: None,
                    provider_suffix: None,
                    original_bitrate: None,
                    track_number: None,
                    reason_code: None,
                    reason: None,
                    server_id: None,
                    tier: None,
                },
                "bitrate-increase",
            )],
            deletes: vec![annotate_delete(
                SyncDeleteItem {
                    jellyfin_id: "track-1".to_string(),
                    local_path: "Music/Track.flac".to_string(),
                    name: "Track".to_string(),
                    reason_code: None,
                    reason: None,
                },
                "bitrate-increase",
            )],
            id_changes: vec![annotate_id_change(
                SyncIdChangeItem {
                    old_jellyfin_id: "old".to_string(),
                    new_jellyfin_id: "new".to_string(),
                    old_local_path: "Music/Other.flac".to_string(),
                    name: "Other".to_string(),
                    album: None,
                    artist: None,
                    size_bytes: 1,
                    etag: None,
                    provider_album_id: None,
                    provider_content_type: None,
                    provider_suffix: None,
                    original_name: None,
                    reason_code: None,
                    reason: None,
                    source_server_id: None,
                },
                "server-id-change",
            )],
            unchanged: 0,
            playlists: vec![],
        };

        let summary = change_reason_summary(&delta);

        assert_eq!(
            summary
                .iter()
                .find(|entry| entry.reason_code == "bitrate-increase")
                .map(|entry| entry.count),
            Some(1)
        );
        assert_eq!(
            summary
                .iter()
                .find(|entry| entry.reason_code == "server-id-change")
                .map(|entry| entry.count),
            Some(1)
        );
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
        let device_io: std::sync::Arc<dyn crate::device_io::DeviceIO> =
            std::sync::Arc::new(crate::device_io::MscBackend::new(device_path.to_path_buf()));

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

        assert_eq!(
            mtime1, mtime2,
            "File must not be rewritten if track list unchanged"
        );
    }

    #[tokio::test]
    async fn test_generate_m3u_rewrites_when_manifest_unchanged_but_file_missing() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let device_path = tmp_dir.path();
        let managed_path = device_path.join("Music");
        tokio::fs::create_dir_all(&managed_path).await.unwrap();

        let playlist_items = vec![PlaylistSyncItem {
            jellyfin_id: "pl1".to_string(),
            name: "Stable Playlist".to_string(),
            tracks: vec![PlaylistTrackInfo {
                jellyfin_id: "t1".to_string(),
                artist: Some("Artist".to_string()),
                run_time_seconds: 120,
            }],
        }];
        let all_synced = vec![make_playlist_synced_item("t1", "Music/A/B/01 - Song.flac")];

        let mut manifest = empty_manifest();
        manifest
            .playlists
            .push(crate::device::PlaylistManifestEntry {
                jellyfin_id: "pl1".to_string(),
                filename: "Stable Playlist.m3u".to_string(),
                track_count: 1,
                track_ids: vec!["t1".to_string()],
                last_modified: "2026-01-01T00:00:00Z".to_string(),
            });
        let device_io: std::sync::Arc<dyn crate::device_io::DeviceIO> =
            std::sync::Arc::new(crate::device_io::MscBackend::new(device_path.to_path_buf()));

        let m3u_path = managed_path.join("Stable Playlist.m3u");
        assert!(
            !m3u_path.exists(),
            "test setup should start with a missing M3U file"
        );

        let warnings = generate_m3u_files(
            &playlist_items,
            device_path,
            &managed_path,
            &all_synced,
            &mut manifest,
            device_io,
        )
        .await;

        assert!(warnings.is_empty(), "No warnings expected: {:?}", warnings);
        assert!(m3u_path.exists(), "Missing M3U file should be rewritten");
        let content = tokio::fs::read_to_string(&m3u_path).await.unwrap();
        assert!(content.contains("#EXTINF:120,Artist - 01 - Song"));
        assert!(content.contains("A/B/01 - Song.flac"));
        assert_eq!(manifest.playlists.len(), 1);
        assert_eq!(manifest.playlists[0].track_ids, vec!["t1"]);
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
        manifest
            .playlists
            .push(crate::device::PlaylistManifestEntry {
                jellyfin_id: "old-pl".to_string(),
                filename: "Old Playlist.m3u".to_string(),
                track_count: 1,
                track_ids: vec!["t1".to_string()],
                last_modified: "2026-01-01T00:00:00Z".to_string(),
            });

        // Call with empty playlist_items (playlist removed from basket)
        let device_io =
            std::sync::Arc::new(crate::device_io::MscBackend::new(device_path.to_path_buf()));
        let warnings = generate_m3u_files(
            &[],
            device_path,
            &managed_path,
            &[],
            &mut manifest,
            device_io,
        )
        .await;

        assert!(warnings.is_empty(), "No warnings expected: {:?}", warnings);
        assert!(
            !m3u_path.exists(),
            "Old .m3u file should have been deleted from Music/"
        );
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
        let device_io =
            std::sync::Arc::new(crate::device_io::MscBackend::new(device_path.to_path_buf()));
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
        assert!(
            warnings[0].contains("t2-missing"),
            "Warning should name the missing track"
        );

        // .m3u should exist in Music/ with 2 tracks (t1 and t3)
        let m3u_path = managed_path.join("Partial Playlist.m3u");
        assert!(m3u_path.exists());

        let content = tokio::fs::read_to_string(&m3u_path).await.unwrap();
        let extinf_count = content.lines().filter(|l| l.starts_with("#EXTINF")).count();
        assert_eq!(extinf_count, 2, "M3U should contain exactly 2 tracks");

        let manifest_entry = manifest
            .playlists
            .iter()
            .find(|e| e.jellyfin_id == "pl1")
            .unwrap();
        assert_eq!(
            manifest_entry.track_count, 2,
            "track_count should be 2 (only resolved tracks)"
        );
    }
}
