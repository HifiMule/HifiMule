pub mod mtp;

use crate::auto_fill::AutoFillPipeline;
use crate::providers::{ProviderChangeContext, ProviderSyncedSong};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use tokio::time::{Duration, sleep};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SyncedItem {
    #[serde(rename = "providerItemId")]
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
    #[serde(default)]
    pub provider_album_id: Option<String>,
    #[serde(default)]
    pub provider_content_type: Option<String>,
    #[serde(default)]
    pub provider_suffix: Option<String>,
    /// Source bitrate at sync time in bps — used to detect quality upgrades on next sync.
    #[serde(default)]
    pub original_bitrate: Option<u32>,
    /// Source container/codec at sync time (e.g. "flac", "mp3").
    #[serde(default)]
    pub original_container: Option<String>,
    #[serde(default)]
    pub track_number: Option<u32>,
    /// Originating server UUID (Story 2.11). Lets force-sync re-route each item to
    /// its source provider. `None` for items synced before multi-server support.
    #[serde(default)]
    pub server_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BasketItem {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub item_type: String,
    #[serde(default)]
    pub server_id: Option<String>,
    #[serde(default)]
    pub artist: Option<String>,
    pub child_count: u32,
    pub size_ticks: i64,
    pub size_bytes: u64,
}

pub fn is_auto_fill_slot_id(id: &str) -> bool {
    id == "__auto_fill_slot__" || id.starts_with("__auto_fill_slot__:")
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

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct DeviceManifest {
    pub device_id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    pub version: String,
    #[serde(default)]
    pub managed_paths: Vec<String>,
    #[serde(
        default,
        rename = "playlistPath",
        alias = "playlist_path",
        skip_serializing_if = "Option::is_none"
    )]
    pub playlist_path: Option<String>,
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
    /// Auto-fill config persisted per device, keyed by portable `server_id` (Story 12.2).
    /// Custom serde reads either the legacy `{ enabled, maxBytes }` block or the new
    /// per-server `{ "<serverId>": { …pipeline… } }` map; see [`AutoFillConfig`].
    #[serde(default)]
    pub auto_fill: AutoFillConfig,
    /// ID referencing an entry in device-profiles.json. None = no transcoding (passthrough).
    #[serde(default)]
    pub transcoding_profile_id: Option<String>,
    /// Last profile that produced the synced files currently recorded in the manifest.
    #[serde(default)]
    pub last_synced_transcoding_profile_id: Option<String>,
    /// True when the active profile changed and matching tracks must be rewritten.
    #[serde(default)]
    pub transcoding_profile_dirty: bool,
    #[serde(default)]
    pub playlists: Vec<PlaylistManifestEntry>,
    /// WPD storage object ID for MTP devices. When set, WPD calls skip first-child
    /// enumeration under DEVICE and use this ID directly. Backward-compatible via serde(default).
    #[serde(default)]
    pub storage_id: Option<String>,
    /// MTP folder object IDs cached across syncs for devices (e.g. Garmin smartwatches) that
    /// omit sub-folder objects from MTP enumeration results. Maps device-relative path to
    /// LIBMTP folder object ID. Backward-compatible via serde(default).
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub folder_ids: std::collections::HashMap<String, u32>,
}

impl DeviceManifest {
    pub fn resolved_playlist_path(&self) -> Option<&str> {
        self.playlist_path
            .as_deref()
            .filter(|path| !path.trim().is_empty())
            .or_else(|| self.managed_paths.first().map(String::as_str))
    }

    pub fn provider_change_context(&self) -> ProviderChangeContext {
        let synced_album_ids: BTreeSet<String> = self
            .basket_items
            .iter()
            .filter(|item| item.item_type == "MusicAlbum")
            .map(|item| item.id.clone())
            .collect();

        ProviderChangeContext {
            synced_songs: self
                .synced_items
                .iter()
                .map(|item| ProviderSyncedSong {
                    song_id: item.jellyfin_id.clone(),
                    album_id: item.provider_album_id.clone(),
                    size: item
                        .provider_content_type
                        .as_ref()
                        .zip(item.provider_suffix.as_ref())
                        .map(|_| item.size_bytes),
                    content_type: item.provider_content_type.clone(),
                    suffix: item.provider_suffix.clone(),
                    version: item.etag.clone(),
                })
                .collect(),
            synced_album_ids: synced_album_ids.into_iter().collect(),
        }
    }
}

/// Legacy single-block auto-fill shape (`{ "enabled", "maxBytes" }`). Retained as both the
/// pre-12.2 on-disk shape and the transient migration carrier inside [`AutoFillConfig`].
///
/// `default` tolerates a partial legacy block (e.g. a hand-edited `{ "maxBytes": 123 }` missing
/// `enabled`): without it the missing scalar fails *both* untagged `AutoFillConfig` variants and
/// the entire manifest fails to load. Missing fields fall back to the `Default` (`false`/`None`).
#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct AutoFillPrefs {
    pub enabled: bool,
    pub max_bytes: Option<u64>,
}

/// Per-device auto-fill configuration (Story 12.2).
///
/// The on-disk shape is one of:
///   - the new per-server map `{ "<serverId>": { …AutoFillPipeline… }, … }` (keys are the
///     portable `server_id` shared with `SyncedItem.server_id` / `BasketItem.server_id`), or
///   - the legacy block `{ "enabled": <bool>, "maxBytes": <u64|null> }` from pre-12.2 manifests.
///
/// Deserialization reads either shape (see the custom `Deserialize`); a meaningful legacy block
/// is parked in `legacy` until [`AutoFillConfig::migrate_legacy_to`] maps it onto the selected
/// server's portable id at device-detect time. An empty/default legacy block deserializes to
/// `legacy: None` so it is never mistaken for real config.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AutoFillConfig {
    /// Per-portable-serverId pipeline configs (the new model). Empty = none configured.
    pub pipelines: HashMap<String, AutoFillPipeline>,
    /// Legacy single-block read from an old manifest, pending migration onto the selected
    /// server's portable id (see [`AutoFillConfig::migrate_legacy_to`]). `None` once migrated,
    /// or when the legacy block was the empty default.
    pub legacy: Option<AutoFillPrefs>,
}

/// Builds a behavior-preserving [`AutoFillPipeline`] from a legacy `{ enabled, maxBytes }` block:
/// today's favorites→playCount→dateCreated single-Ordering pipeline, with `enabled` carried over.
fn pipeline_from_legacy(prefs: &AutoFillPrefs) -> AutoFillPipeline {
    let mut pipeline = AutoFillPipeline::default_legacy(prefs.max_bytes);
    pipeline.enabled = prefs.enabled;
    pipeline
}

impl Serialize for AutoFillConfig {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // Non-empty map → emit the per-server map verbatim. Otherwise emit the legacy block
        // (or, with nothing configured, the empty default `{ "enabled": false, "maxBytes": null }`
        // — byte-for-byte identical to the pre-12.2 `AutoFillPrefs::default()` output so
        // get_daemon_state and freshly-written manifests are unchanged).
        if !self.pipelines.is_empty() {
            self.pipelines.serialize(serializer)
        } else if let Some(legacy) = &self.legacy {
            legacy.serialize(serializer)
        } else {
            AutoFillPrefs::default().serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for AutoFillConfig {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // Both shapes are JSON objects, so we discriminate by value type. The per-server map's
        // values are pipeline *objects*; the legacy block's `enabled`/`maxBytes` values are
        // scalars/null, so a `HashMap<String, AutoFillPipeline>` deserialize fails on them and
        // falls through to the legacy variant. `PerServer` is tried first; an empty `{}` parses
        // as an empty map (no config).
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            PerServer(HashMap<String, AutoFillPipeline>),
            Legacy(AutoFillPrefs),
        }

        Ok(match Raw::deserialize(deserializer)? {
            Raw::PerServer(pipelines) => AutoFillConfig {
                pipelines,
                legacy: None,
            },
            Raw::Legacy(prefs) => {
                // A fresh/default `{ enabled: false, maxBytes: null }` is "no config" — never a
                // migration trigger. Anything meaningful is parked for migration.
                let legacy = if !prefs.enabled && prefs.max_bytes.is_none() {
                    None
                } else {
                    Some(prefs)
                };
                AutoFillConfig {
                    pipelines: HashMap::new(),
                    legacy,
                }
            }
        })
    }
}

impl AutoFillConfig {
    /// Maps a parked legacy block onto `server_id`'s portable pipeline slot. Returns `true` when
    /// it changed the config (so the caller persists). Idempotent: returns `false` when there is
    /// no legacy block, or when a pipeline already exists for `server_id` (never overwritten).
    pub fn migrate_legacy_to(&mut self, server_id: &str) -> bool {
        if self.legacy.is_some() && !self.pipelines.contains_key(server_id) {
            let prefs = self.legacy.take().expect("legacy is Some");
            self.pipelines
                .insert(server_id.to_string(), pipeline_from_legacy(&prefs));
            true
        } else {
            false
        }
    }

    /// Resolves the effective pipeline for a (possibly absent) server id: the keyed pipeline when
    /// present, else the sole pipeline of a single-server install, else `None` (legacy fallback).
    fn resolve_pipeline(&self, server_id: Option<&str>) -> Option<&AutoFillPipeline> {
        if let Some(id) = server_id
            && let Some(pipeline) = self.pipelines.get(id)
        {
            return Some(pipeline);
        }
        if self.pipelines.len() == 1 {
            return self.pipelines.values().next();
        }
        None
    }

    /// Whether auto-fill is enabled for `server_id` (single-entry/legacy fallback when unkeyed).
    pub fn enabled_for(&self, server_id: Option<&str>) -> bool {
        match self.resolve_pipeline(server_id) {
            Some(pipeline) => pipeline.enabled,
            None => self.legacy.as_ref().is_some_and(|l| l.enabled),
        }
    }

    /// The byte budget for `server_id` (single-entry/legacy fallback when unkeyed).
    pub fn max_bytes_for(&self, server_id: Option<&str>) -> Option<u64> {
        match self.resolve_pipeline(server_id) {
            Some(pipeline) => pipeline.budget.max_bytes,
            None => self.legacy.as_ref().and_then(|l| l.max_bytes),
        }
    }

    /// Server-agnostic enabled read for callers without server context (resolves the single
    /// migrated pipeline of a single-server install, else the legacy block).
    pub fn legacy_enabled(&self) -> bool {
        self.enabled_for(None)
    }

    /// Server-agnostic max-bytes read for callers without server context.
    pub fn legacy_max_bytes(&self) -> Option<u64> {
        self.max_bytes_for(None)
    }

    /// Upserts the pipeline for `server_id` from a legacy-equivalent `{ enabled, maxBytes }` and
    /// clears any parked legacy block. Used by `setAutoFill` (which can resolve the selected id).
    pub fn set_for(&mut self, server_id: &str, enabled: bool, max_bytes: Option<u64>) {
        self.pipelines.insert(
            server_id.to_string(),
            pipeline_from_legacy(&AutoFillPrefs { enabled, max_bytes }),
        );
        self.legacy = None;
    }

    /// Write path when no selected portable id is available (e.g. the server is momentarily
    /// unselected). When a single pipeline already exists (single-server install), update it in
    /// place — parking a `legacy` block here would be silently dropped by `serialize`, which emits
    /// the non-empty `pipelines` map (mirrors `resolve_pipeline`'s single-entry fallback). Only an
    /// empty (or genuinely multi-server, Story 12.3) config falls back to parking the legacy block.
    pub fn set_legacy(&mut self, enabled: bool, max_bytes: Option<u64>) {
        if self.pipelines.len() == 1 {
            let key = self.pipelines.keys().next().expect("len == 1").clone();
            self.pipelines.insert(
                key,
                pipeline_from_legacy(&AutoFillPrefs { enabled, max_bytes }),
            );
            self.legacy = None;
        } else {
            self.legacy = Some(AutoFillPrefs { enabled, max_bytes });
        }
    }

    /// The pipeline for `server_id`, if one is configured. Read at each sync-time auto-fill
    /// expansion site (Story 12.4) to route the slot through the configurable engine when a
    /// non-default pipeline is configured for that portable serverId.
    pub fn pipeline_for(&self, server_id: &str) -> Option<&AutoFillPipeline> {
        self.pipelines.get(server_id)
    }

    /// Upserts a full [`AutoFillPipeline`] for `server_id` and clears any parked legacy block
    /// (Story 12.6). This is the write path behind `autoFill.setPipeline`: it inserts or replaces
    /// **only** that server's entry — every other server's pipeline is left byte-for-byte
    /// unchanged — so reconfiguring one server can never disturb another's slot. Unlike
    /// [`AutoFillConfig::set_for`] (legacy `{ enabled, maxBytes }` upsert), this persists the
    /// caller-supplied pipeline verbatim, including reserved Epic 13 fields.
    pub fn set_pipeline(&mut self, server_id: &str, pipeline: AutoFillPipeline) {
        self.pipelines.insert(server_id.to_string(), pipeline);
        self.legacy = None;
    }
}

/// Rewrites a single optional `server_id` tag from a legacy machine-local UUID or
/// pre-2.11 composite to the deterministic portable id (Story 2.13), using `remap`
/// (`{ legacy → portable }`). Returns true when the tag changed. Idempotent: a tag
/// that is already portable is not a key in `remap`, so it is left untouched.
fn remap_server_id_tag(
    tag: &mut Option<String>,
    remap: &std::collections::HashMap<String, String>,
) -> bool {
    if let Some(current) = tag.as_deref()
        && let Some(portable) = remap.get(current)
        && portable.as_str() != current
    {
        *tag = Some(portable.clone());
        return true;
    }
    false
}

/// Reconciles a device manifest's `synced_items` and `basket_items` `server_id`
/// tags onto the portable identity (Story 2.13 AC6), using `remap`
/// (`{ machine-local UUID → portable, legacy composite → portable }`). Returns true
/// when anything changed. Idempotent and order-independent.
pub fn reconcile_manifest_server_ids(
    manifest: &mut DeviceManifest,
    remap: &std::collections::HashMap<String, String>,
) -> bool {
    if remap.is_empty() {
        return false;
    }
    let mut changed = false;
    for item in &mut manifest.synced_items {
        changed |= remap_server_id_tag(&mut item.server_id, remap);
    }
    for item in &mut manifest.basket_items {
        changed |= remap_server_id_tag(&mut item.server_id, remap);
    }
    changed
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

async fn persist_local_manifest_atomic(path: &Path, manifest: &DeviceManifest) -> Result<()> {
    persist_local_manifest_atomic_with_sync(path, manifest, sync_manifest_directory).await
}

#[derive(Debug)]
struct ManifestCacheCommitUncertain(std::io::Error);

impl std::fmt::Display for ManifestCacheCommitUncertain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Manifest cache rename completed but directory sync failed: {}",
            self.0
        )
    }
}

impl std::error::Error for ManifestCacheCommitUncertain {}

fn sync_manifest_directory(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    std::fs::File::open(path)?.sync_all()?;
    // Windows does not support opening directories with std::fs::File for fsync.
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

async fn persist_local_manifest_atomic_with_sync(
    path: &Path,
    manifest: &DeviceManifest,
    sync_directory: fn(&Path) -> std::io::Result<()>,
) -> Result<()> {
    use tokio::io::AsyncWriteExt;

    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Manifest cache path has no parent"))?;
    tokio::fs::create_dir_all(parent).await?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow::anyhow!("Manifest cache path is invalid"))?;
    let temporary = parent.join(format!(".{file_name}.{}.tmp", uuid::Uuid::new_v4()));
    let bytes = serde_json::to_vec_pretty(manifest)?;
    let mut file = tokio::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .await?;
    if let Err(error) = async {
        file.write_all(&bytes).await?;
        file.flush().await?;
        file.sync_all().await?;
        drop(file);
        tokio::fs::rename(&temporary, path).await?;
        Ok::<(), std::io::Error>(())
    }
    .await
    {
        let _ = tokio::fs::remove_file(&temporary).await;
        return Err(error.into());
    }
    // A rename is atomic but its directory entry is not durable until the parent
    // is synced. Distinguish this failure: the new bytes are already visible.
    let parent = parent.to_path_buf();
    tokio::task::spawn_blocking(move || sync_directory(&parent))
        .await
        .map_err(|error| ManifestCacheCommitUncertain(std::io::Error::other(error)))?
        .map_err(ManifestCacheCommitUncertain)?;
    Ok(())
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
    observation_token: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Destination {
    Playback {
        id: String,
        selected: bool,
    },
    Device {
        path: String,
        device_id: String,
        name: String,
        icon: Option<String>,
        selected: bool,
    },
    PendingDevice {
        pending_id: String,
        name: String,
        selected: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DestinationSnapshot {
    pub revision: u64,
    pub destinations: Vec<Destination>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceDiscoveryIssue {
    pub discovery_id: String,
    pub code: String,
    pub display_name: Option<String>,
    pub retryable: bool,
    pub revision: String,
}

struct StoredDiscoveryIssue {
    path: PathBuf,
    wire: DeviceDiscoveryIssue,
}

pub struct UnrecognizedDeviceState {
    pub pending_id: String,
    pub path: PathBuf,
    pub io: std::sync::Arc<dyn crate::device_io::DeviceIO>,
    pub friendly_name: Option<String>,
    pub observation_token: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SelectedDestination {
    Playback,
    Device(PathBuf),
    Pending(String),
}

struct DeviceManagerState {
    connected_devices: std::collections::HashMap<PathBuf, ConnectedDevice>,
    selected_device_path: Option<PathBuf>,
    selected_destination: SelectedDestination,
    pending_devices: std::collections::BTreeMap<String, UnrecognizedDeviceState>,
    destination_revision: u64,
    latest_selection_token: u64,
    discovery_issues: std::collections::BTreeMap<String, StoredDiscoveryIssue>,
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
            if entry.name.ends_with(".tmp") && device_io.delete_file(&entry.path).await.is_ok() {
                count += 1;
            }
        }
    }
    Ok(count)
}

#[derive(Debug, Clone)]
pub enum DeviceEvent {
    Detected {
        observation_token: u64,
        path: PathBuf,
        manifest: DeviceManifest,
        device_io: std::sync::Arc<dyn crate::device_io::DeviceIO>,
    },
    Removed(PathBuf),
    Unrecognized {
        observation_token: u64,
        path: PathBuf,
        device_io: std::sync::Arc<dyn crate::device_io::DeviceIO>,
        friendly_name: Option<String>,
    },
    DiscoveryFailed {
        path: PathBuf,
        code: &'static str,
        display_name: Option<String>,
    },
}

static DESTINATION_MUTATION_CLOCK: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

fn next_destination_mutation() -> u64 {
    use std::sync::atomic::Ordering;
    DESTINATION_MUTATION_CLOCK.fetch_add(1, Ordering::SeqCst) + 1
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
    mtp_manifest_cache_path: std::sync::Arc<dyn Fn(&str) -> Result<PathBuf> + Send + Sync>,
    /// Connected-device map and selection are guarded together to avoid torn reads and lock inversion.
    state: std::sync::Arc<tokio::sync::RwLock<DeviceManagerState>>,
    /// Serializes authoritative manifest commits per portable device identity without
    /// holding the global connected-device lock over device or filesystem I/O.
    manifest_commit_locks:
        std::sync::Mutex<std::collections::HashMap<String, std::sync::Arc<tokio::sync::Mutex<()>>>>,
}

impl DeviceManager {
    pub fn begin_destination_mutation(&self) -> u64 {
        next_destination_mutation()
    }

    pub fn new(db: std::sync::Arc<crate::db::Database>) -> Self {
        Self {
            db,
            mtp_manifest_cache_path: std::sync::Arc::new(crate::paths::get_local_mtp_manifest_path),
            state: std::sync::Arc::new(tokio::sync::RwLock::new(DeviceManagerState {
                connected_devices: std::collections::HashMap::new(),
                selected_device_path: None,
                selected_destination: SelectedDestination::Playback,
                pending_devices: std::collections::BTreeMap::new(),
                destination_revision: 0,
                latest_selection_token: 0,
                discovery_issues: std::collections::BTreeMap::new(),
            })),
            manifest_commit_locks: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    #[cfg(test)]
    fn new_with_mtp_manifest_cache_path(
        db: std::sync::Arc<crate::db::Database>,
        mtp_manifest_cache_path: std::sync::Arc<dyn Fn(&str) -> Result<PathBuf> + Send + Sync>,
    ) -> Self {
        Self {
            db,
            mtp_manifest_cache_path,
            state: std::sync::Arc::new(tokio::sync::RwLock::new(DeviceManagerState {
                connected_devices: std::collections::HashMap::new(),
                selected_device_path: None,
                selected_destination: SelectedDestination::Playback,
                pending_devices: std::collections::BTreeMap::new(),
                destination_revision: 0,
                latest_selection_token: 0,
                discovery_issues: std::collections::BTreeMap::new(),
            })),
            manifest_commit_locks: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    pub async fn handle_device_detected(
        &self,
        path: PathBuf,
        manifest: DeviceManifest,
        device_io: std::sync::Arc<dyn crate::device_io::DeviceIO>,
    ) -> Result<crate::DaemonState> {
        let observation_token = self.begin_destination_mutation();
        self.handle_device_detected_at(observation_token, path, manifest, device_io)
            .await
    }

    pub async fn handle_device_detected_at(
        &self,
        observation_token: u64,
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

        // Story 2.13: reconcile manifest server_id tags (2.11 machine-local UUID or
        // pre-2.11 composite) onto the deterministic portable id, so a device synced
        // on another machine — or before this change — is recognized without a
        // spurious full resync. Idempotent; best-effort persistence (never blocks
        // device load on a write failure).
        let mut manifest = manifest;
        let remap = self.db.server_id_remap();
        if reconcile_manifest_server_ids(&mut manifest, &remap)
            && let Err(e) = write_manifest(std::sync::Arc::clone(&device_io), &manifest).await
        {
            daemon_log!(
                "[Device] Manifest server_id reconciliation persist failed (continuing): {}",
                e
            );
        }

        // Story 12.2: actively migrate a legacy `{ enabled, maxBytes }` auto-fill block onto the
        // currently selected server's portable id (the only faithful target — the legacy block
        // carried no serverId). Best-effort persistence, mirroring the reconcile path above; if
        // no server is selected yet the legacy block stays parked and migrates on a later detect.
        if let Ok(Some(sel)) = self.db.get_server_config()
            && let Some(portable) = sel.server_id
            && manifest.auto_fill.migrate_legacy_to(&portable)
            && let Err(e) = write_manifest(std::sync::Arc::clone(&device_io), &manifest).await
        {
            daemon_log!(
                "[Device] Auto-fill legacy migration persist failed (continuing): {}",
                e
            );
        }

        // T5.9: Scan for MTP-style dirty markers on reconnect.
        // For MSC devices, leftover `.dirty` markers indicate an interrupted MtpBackend write
        // from a previous session (e.g., the device was used with MTP before).
        if let Ok(files) = device_io.list_files("").await
            && files.iter().any(|f| f.name.ends_with(".dirty"))
        {
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
                observation_token,
            };
            {
                let mut state = self.state.write().await;
                state.connected_devices.insert(path.clone(), connected);
                state
                    .pending_devices
                    .retain(|_, pending| pending.path != path);
                state.discovery_issues.retain(|_, issue| issue.path != path);
                if observation_token > state.latest_selection_token {
                    state.selected_device_path = Some(path.clone());
                    state.selected_destination = SelectedDestination::Device(path.clone());
                    state.latest_selection_token = observation_token;
                }
                state.destination_revision += 1;
            }
            let name = dirty_manifest
                .name
                .clone()
                .unwrap_or_else(|| dirty_manifest.device_id.clone());
            let mapping = self
                .db
                .get_device_mapping(&dirty_manifest.device_id)
                .unwrap_or(None);
            if mapping.is_some()
                && let Err(e) = self.db.set_auto_sync_on_connect(
                    &dirty_manifest.device_id,
                    dirty_manifest.auto_sync_on_connect,
                )
            {
                daemon_log!(
                    "[Device] Failed to sync auto_sync_on_connect from manifest: {}",
                    e
                );
            }
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

        let connected = ConnectedDevice {
            manifest: manifest.clone(),
            device_io,
            device_class,
            observation_token,
        };
        {
            let mut state = self.state.write().await;
            state.connected_devices.insert(path.clone(), connected);
            state
                .pending_devices
                .retain(|_, pending| pending.path != path);
            state.discovery_issues.retain(|_, issue| issue.path != path);
            if observation_token > state.latest_selection_token {
                state.selected_device_path = Some(path.clone());
                state.selected_destination = SelectedDestination::Device(path.clone());
                state.latest_selection_token = observation_token;
            }
            state.destination_revision += 1;
        }

        let name = manifest
            .name
            .clone()
            .unwrap_or_else(|| manifest.device_id.clone());
        let mapping = self
            .db
            .get_device_mapping(&manifest.device_id)
            .unwrap_or(None);
        if mapping.is_some()
            && let Err(e) = self
                .db
                .set_auto_sync_on_connect(&manifest.device_id, manifest.auto_sync_on_connect)
        {
            daemon_log!(
                "[Device] Failed to sync auto_sync_on_connect from manifest: {}",
                e
            );
        }

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
        let observation_token = self.begin_destination_mutation();
        self.handle_device_unrecognized_at(observation_token, path, device_io, friendly_name)
            .await
    }

    pub async fn handle_device_unrecognized_at(
        &self,
        observation_token: u64,
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
            state
                .pending_devices
                .retain(|_, pending| pending.path != path);
            state.discovery_issues.retain(|_, issue| issue.path != path);
            let pending_id = uuid::Uuid::new_v4().to_string();
            state.pending_devices.insert(
                pending_id.clone(),
                UnrecognizedDeviceState {
                    pending_id: pending_id.clone(),
                    path: path.clone(),
                    io: device_io,
                    friendly_name,
                    observation_token,
                },
            );
            if observation_token > state.latest_selection_token {
                state.selected_device_path = None;
                state.selected_destination = SelectedDestination::Pending(pending_id);
                state.latest_selection_token = observation_token;
            }
            state.destination_revision += 1;
        }
        crate::DaemonState::DeviceFound(path_str)
    }

    pub async fn get_unrecognized_device_snapshot(&self) -> Option<UnrecognizedDeviceState> {
        let state = self.state.read().await;
        let pending = match &state.selected_destination {
            SelectedDestination::Pending(id) => state.pending_devices.get(id),
            _ => state
                .pending_devices
                .values()
                .max_by_key(|pending| pending.observation_token),
        }?;
        Some(UnrecognizedDeviceState {
            pending_id: pending.pending_id.clone(),
            path: pending.path.clone(),
            io: std::sync::Arc::clone(&pending.io),
            friendly_name: pending.friendly_name.clone(),
            observation_token: pending.observation_token,
        })
    }

    pub async fn get_pending_devices_snapshot(&self) -> Vec<UnrecognizedDeviceState> {
        let state = self.state.read().await;
        let mut pending: Vec<_> = state
            .pending_devices
            .values()
            .map(|entry| UnrecognizedDeviceState {
                pending_id: entry.pending_id.clone(),
                path: entry.path.clone(),
                io: std::sync::Arc::clone(&entry.io),
                friendly_name: entry.friendly_name.clone(),
                observation_token: entry.observation_token,
            })
            .collect();
        pending.sort_by_key(|entry| entry.observation_token);
        pending
    }

    pub async fn report_discovery_failure(
        &self,
        path: PathBuf,
        code: &str,
        display_name: Option<String>,
    ) {
        let display_name = display_name.map(|name| name.chars().take(120).collect::<String>());
        let mut state = self.state.write().await;
        let existing = state
            .discovery_issues
            .iter()
            .find_map(|(id, issue)| (issue.path == path).then(|| (id.clone(), issue.wire.clone())));
        if existing
            .as_ref()
            .is_some_and(|(_, issue)| issue.code == code && issue.display_name == display_name)
        {
            return;
        }
        if let Some((id, _)) = existing {
            state.discovery_issues.remove(&id);
        }
        state.destination_revision += 1;
        let revision = state.destination_revision.to_string();
        let discovery_id = uuid::Uuid::new_v4().to_string();
        state.discovery_issues.insert(
            discovery_id.clone(),
            StoredDiscoveryIssue {
                path,
                wire: DeviceDiscoveryIssue {
                    discovery_id,
                    code: if code == "DEVICE_OPEN_FAILED" {
                        "DEVICE_OPEN_FAILED"
                    } else {
                        "DEVICE_READ_FAILED"
                    }
                    .to_string(),
                    display_name,
                    retryable: true,
                    revision,
                },
            },
        );
    }

    pub async fn get_discovery_issues(&self) -> Vec<DeviceDiscoveryIssue> {
        self.state
            .read()
            .await
            .discovery_issues
            .values()
            .map(|issue| issue.wire.clone())
            .collect()
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
        let token = self.begin_destination_mutation();
        let mut state = self.state.write().await;
        state.connected_devices.remove(removed_path);
        state
            .discovery_issues
            .retain(|_, issue| &issue.path != removed_path);
        let removed_pending: Vec<String> = state
            .pending_devices
            .iter()
            .filter(|(_, pending)| &pending.path == removed_path)
            .map(|(id, _)| id.clone())
            .collect();
        for id in &removed_pending {
            state.pending_devices.remove(id);
        }
        let selected_removed = state.selected_device_path.as_ref() == Some(removed_path)
            || matches!(&state.selected_destination, SelectedDestination::Pending(id) if removed_pending.contains(id));
        if selected_removed {
            state.selected_device_path = None;
            state.selected_destination = SelectedDestination::Playback;
            state.latest_selection_token = token;
        }
        state.destination_revision += 1;
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

    pub async fn get_current_device_path(&self) -> Option<PathBuf> {
        self.state.read().await.selected_device_path.clone()
    }

    pub async fn get_device_id_for_path(&self, path: &Path) -> Option<String> {
        self.state
            .read()
            .await
            .connected_devices
            .get(path)
            .map(|device| device.manifest.device_id.clone())
    }

    /// Capture the selected sync destination under one state read.
    pub async fn get_selected_sync_target(
        &self,
    ) -> Option<(
        PathBuf,
        DeviceManifest,
        std::sync::Arc<dyn crate::device_io::DeviceIO>,
    )> {
        let state = self.state.read().await;
        let path = state.selected_device_path.as_ref()?;
        let connected = state.connected_devices.get(path)?;
        Some((
            path.clone(),
            connected.manifest.clone(),
            std::sync::Arc::clone(&connected.device_io),
        ))
    }

    pub async fn get_sync_target_for_device(
        &self,
        device_id: &str,
    ) -> Option<(
        PathBuf,
        DeviceManifest,
        std::sync::Arc<dyn crate::device_io::DeviceIO>,
    )> {
        let state = self.state.read().await;
        state
            .connected_devices
            .iter()
            .find_map(|(path, connected)| {
                (connected.manifest.device_id == device_id).then(|| {
                    (
                        path.clone(),
                        connected.manifest.clone(),
                        std::sync::Arc::clone(&connected.device_io),
                    )
                })
            })
    }

    pub async fn get_manifest_for_device(&self, device_id: &str) -> Option<DeviceManifest> {
        let state = self.state.read().await;
        state
            .connected_devices
            .values()
            .find(|connected| connected.manifest.device_id == device_id)
            .map(|connected| connected.manifest.clone())
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

    pub async fn get_destination_snapshot(&self) -> DestinationSnapshot {
        let state = self.state.read().await;
        let mut destinations = vec![Destination::Playback {
            id: "playback".to_string(),
            selected: matches!(state.selected_destination, SelectedDestination::Playback),
        }];
        let mut devices: Vec<_> = state.connected_devices.iter().collect();
        devices.sort_by_key(|(_, device)| device.observation_token);
        destinations.extend(devices.into_iter().map(|(path, device)| Destination::Device {
            path: path.to_string_lossy().to_string(),
            device_id: device.manifest.device_id.clone(),
            name: device.manifest.name.clone().filter(|name| !name.is_empty())
                .unwrap_or_else(|| device.manifest.device_id.clone()),
            icon: device.manifest.icon.clone(),
            selected: matches!(&state.selected_destination, SelectedDestination::Device(selected) if selected == path),
        }));
        let mut pending: Vec<_> = state.pending_devices.values().collect();
        pending.sort_by_key(|entry| entry.observation_token);
        destinations.extend(pending.into_iter().map(|entry| Destination::PendingDevice {
            pending_id: entry.pending_id.clone(),
            name: entry.friendly_name.clone().unwrap_or_else(|| "Unconfigured device".to_string()),
            selected: matches!(&state.selected_destination, SelectedDestination::Pending(selected) if selected == &entry.pending_id),
        }));
        DestinationSnapshot {
            revision: state.destination_revision,
            destinations,
        }
    }

    pub async fn select_playback(&self) {
        let token = self.begin_destination_mutation();
        let mut state = self.state.write().await;
        state.selected_device_path = None;
        state.selected_destination = SelectedDestination::Playback;
        state.latest_selection_token = token;
        state.destination_revision += 1;
    }

    pub async fn select_pending_device(&self, pending_id: &str) -> bool {
        let token = self.begin_destination_mutation();
        let mut state = self.state.write().await;
        if !state.pending_devices.contains_key(pending_id) {
            return false;
        }
        state.selected_device_path = None;
        state.selected_destination = SelectedDestination::Pending(pending_id.to_string());
        state.latest_selection_token = token;
        state.destination_revision += 1;
        true
    }

    /// Sets the selected device path. Returns false if path is not in connected_devices.
    pub async fn select_device(&self, path: PathBuf) -> bool {
        let token = self.begin_destination_mutation();
        let mut state = self.state.write().await;
        if !state.connected_devices.contains_key(&path) {
            return false;
        }
        state.selected_device_path = Some(path.clone());
        state.selected_destination = SelectedDestination::Device(path);
        state.latest_selection_token = token;
        state.destination_revision += 1;
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
        let device_id = self
            .get_current_device()
            .await
            .ok_or_else(|| anyhow::anyhow!("No device connected"))?
            .device_id;
        self.update_manifest_for_device(&device_id, mutation).await
    }

    /// Persist a manifest update against the operation's stable device identity, not
    /// whichever device the UI happens to select while the operation is running.
    pub async fn update_manifest_for_device<F>(&self, device_id: &str, mutation: F) -> Result<()>
    where
        F: FnOnce(&mut DeviceManifest),
    {
        self.update_manifest_for_device_with_cache_path(
            device_id,
            mutation,
            crate::paths::get_local_mtp_manifest_path,
            sync_manifest_directory,
        )
        .await
    }

    async fn update_manifest_for_device_with_cache_path<F, P>(
        &self,
        device_id: &str,
        mutation: F,
        cache_path: P,
        sync_directory: fn(&Path) -> std::io::Result<()>,
    ) -> Result<()>
    where
        F: FnOnce(&mut DeviceManifest),
        P: FnOnce(&str) -> Result<PathBuf>,
    {
        let commit_lock = {
            let mut locks = self
                .manifest_commit_locks
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            std::sync::Arc::clone(
                locks
                    .entry(device_id.to_string())
                    .or_insert_with(|| std::sync::Arc::new(tokio::sync::Mutex::new(()))),
            )
        };
        let _commit = commit_lock.lock().await;
        let mut state = self.state.write().await;
        let path = state
            .connected_devices
            .iter()
            .find_map(|(path, connected)| {
                (connected.manifest.device_id == device_id).then(|| path.clone())
            })
            .ok_or_else(|| anyhow::anyhow!("Device is no longer connected: {device_id}"))?;
        let connected = state
            .connected_devices
            .get_mut(&path)
            .ok_or_else(|| anyhow::anyhow!("Device not in connected map"))?;
        let mut previous_manifest = connected.manifest.clone();
        mutation(&mut connected.manifest);
        // Clone Arc and manifest so we can drop the write guard before the async I/O.
        // Serialization is still guaranteed: the in-memory state is mutated under the lock;
        // the snapshot written to disk is always consistent with the in-memory state.
        let device_io = std::sync::Arc::clone(&connected.device_io);
        let manifest_snapshot = connected.manifest.clone();
        let is_mtp = connected.device_class == DeviceClass::Mtp;
        drop(state);
        let persist_result = if is_mtp {
            // For MTP devices the local cache is authoritative: on-device writes are
            // best-effort only. Do not publish a clean mirror before the recovery
            // cache has committed: a failed cache path must leave the old mirror intact.
            async {
                let local_path = cache_path(&manifest_snapshot.device_id)?;
                let result = persist_local_manifest_atomic_with_sync(
                    &local_path,
                    &manifest_snapshot,
                    sync_directory,
                )
                .await;
                if result
                    .as_ref()
                    .is_err_and(|error| error.is::<ManifestCacheCommitUncertain>())
                {
                    // The replacement is visible but durability is unknown. Restore
                    // recovery evidence both in memory and, best-effort, on disk.
                    previous_manifest.dirty = true;
                    for item in &manifest_snapshot.pending_item_ids {
                        if !previous_manifest.pending_item_ids.contains(item) {
                            previous_manifest.pending_item_ids.push(item.clone());
                        }
                    }
                    let _ = persist_local_manifest_atomic(&local_path, &previous_manifest).await;
                }
                result?;
                let _ = crate::device::write_manifest(device_io, &manifest_snapshot).await;
                Ok(())
            }
            .await
        } else {
            crate::device::write_manifest(device_io, &manifest_snapshot).await
        };
        if let Err(error) = persist_result {
            let mut state = self.state.write().await;
            if let Some(connected) = state.connected_devices.get_mut(&path)
                && connected.manifest.device_id == manifest_snapshot.device_id
            {
                connected.manifest = previous_manifest;
            }
            return Err(error);
        }
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
        playlist_folder_path: Option<&str>,
        transcoding_profile_id: Option<String>,
        name: String,
        icon: Option<String>,
        _device_io: std::sync::Arc<dyn crate::device_io::DeviceIO>,
    ) -> Result<DeviceManifest> {
        let pending = self
            .get_unrecognized_device_snapshot()
            .await
            .ok_or_else(|| anyhow::anyhow!("No unrecognized device connected"))?;
        let revision = self.state.read().await.destination_revision;
        self.initialize_pending_device(
            &pending.pending_id,
            revision,
            folder_path,
            playlist_folder_path,
            transcoding_profile_id,
            name,
            icon,
        )
        .await
    }

    pub async fn initialize_pending_device(
        &self,
        pending_id: &str,
        observed_destination_revision: u64,
        folder_path: &str,
        playlist_folder_path: Option<&str>,
        transcoding_profile_id: Option<String>,
        name: String,
        icon: Option<String>,
    ) -> Result<DeviceManifest> {
        // Validate folder_path: no traversal, no absolute paths; multi-level paths (e.g.
        // "Music/HifiMule") are allowed — both MscBackend (create_dir_all) and MtpBackend
        // (auto-creates parent objects) handle them transparently.
        if !folder_path.is_empty() {
            let components: Vec<&str> = folder_path.split(&['/', '\\']).collect();
            if components.contains(&"..") {
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

        let pending = {
            let state = self.state.read().await;
            if state.destination_revision != observed_destination_revision {
                return Err(anyhow::anyhow!("Pending destination revision is stale"));
            }
            let pending = state
                .pending_devices
                .get(pending_id)
                .ok_or_else(|| anyhow::anyhow!("Pending destination is no longer available"))?;
            UnrecognizedDeviceState {
                pending_id: pending.pending_id.clone(),
                path: pending.path.clone(),
                io: std::sync::Arc::clone(&pending.io),
                friendly_name: pending.friendly_name.clone(),
                observation_token: pending.observation_token,
            }
        };
        let pending_id = pending.pending_id.clone();
        let observation_token = pending.observation_token;
        let device_root = pending.path;
        let device_io = pending.io;
        let device_class = device_class_from_path(&device_root);
        let device_path = device_root.to_string_lossy().to_string();

        // Liveness probe: detect stale IO from a device that disconnected and reconnected
        // between the Unrecognized event and the user completing initialization. For MTP,
        // avoid list_files("") because WPD recursively walks the whole phone storage.
        let probed_storage_id = match device_class {
            DeviceClass::Mtp => match device_io.storage_id().await {
                Ok(id) => id,
                Err(e) => {
                    daemon_log!(
                        "[DeviceInit] MTP storage-root probe failed device_path={} class={:?} error={:#}",
                        device_path,
                        device_class,
                        e
                    );
                    return Err(anyhow::anyhow!(
                        "Device no longer accessible — reconnect the device and try again"
                    ));
                }
            },
            DeviceClass::Msc => {
                if let Err(e) = device_io.list_files("").await {
                    daemon_log!(
                        "[DeviceInit] Root liveness probe failed device_path={} class={:?} error={:#}",
                        device_path,
                        device_class,
                        e
                    );
                    return Err(anyhow::anyhow!(
                        "Device no longer accessible — reconnect the device and try again"
                    ));
                }
                None
            }
        };

        let device_id = uuid::Uuid::new_v4().to_string();

        let managed_paths = if folder_path.is_empty() {
            vec![]
        } else {
            device_io.ensure_dir(folder_path).await.with_context(|| {
                format!(
                    "Failed to create managed folder '{}' on device {}",
                    folder_path, device_path
                )
            })?;
            vec![folder_path.to_string()]
        };

        let storage_id = match device_class {
            DeviceClass::Mtp => probed_storage_id,
            DeviceClass::Msc => match device_io.storage_id().await {
                Ok(id) => id,
                Err(e) => {
                    daemon_log!(
                        "[DeviceInit] storage_id lookup failed device_path={} class={:?} error={:#}",
                        device_path,
                        device_class,
                        e
                    );
                    None
                }
            },
        };

        let playlist_path = playlist_folder_path
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(str::to_string)
            .or_else(|| managed_paths.first().cloned());
        if let Some(path) = playlist_path.as_deref()
            && !path.is_empty()
            && managed_paths.first().map(String::as_str) != Some(path)
        {
            device_io.ensure_dir(path).await.with_context(|| {
                format!(
                    "Failed to create playlist folder '{}' on device {}",
                    path, device_path
                )
            })?;
        }

        let manifest = DeviceManifest {
            device_id,
            name: Some(name).filter(|s| !s.is_empty()),
            icon,
            version: "1.0".to_string(),
            managed_paths,
            playlist_path,
            synced_items: vec![],
            dirty: false,
            pending_item_ids: vec![],
            basket_items: vec![],
            auto_sync_on_connect: false,
            auto_fill: AutoFillConfig::default(),
            transcoding_profile_id, // stored in .hifimule.json; read back on sync to apply transcoding
            last_synced_transcoding_profile_id: None,
            transcoding_profile_dirty: false,
            playlists: vec![],
            storage_id,
            folder_ids: std::collections::HashMap::new(),
        };
        let manifest_bytes = serde_json::to_string_pretty(&manifest)?;
        daemon_log!(
            "[DeviceInit] Writing manifest path=.hifimule.json device_path={} class={:?} managed_paths={:?} storage_id={:?} bytes={}",
            device_path,
            device_class,
            manifest.managed_paths,
            manifest.storage_id,
            manifest_bytes.len()
        );
        if device_class == DeviceClass::Mtp {
            let cache_path = (self.mtp_manifest_cache_path)(&manifest.device_id)?;
            persist_local_manifest_atomic(&cache_path, &manifest).await?;
            if let Err(e) = device_io
                .write_with_verify(".hifimule.json", manifest_bytes.as_bytes())
                .await
            {
                daemon_log!(
                    "[DeviceInit] Manifest mirror failed after authoritative local cache commit path=.hifimule.json device_path={} class={:?} storage_id={:?} error={:#}",
                    device_path,
                    device_class,
                    manifest.storage_id,
                    e
                );
            }
        } else if let Err(e) = device_io
            .write_with_verify(".hifimule.json", manifest_bytes.as_bytes())
            .await
        {
            daemon_log!(
                "[DeviceInit] Manifest write failed path=.hifimule.json device_path={} class={:?} managed_paths={:?} storage_id={:?} error={:#}",
                device_path,
                device_class,
                manifest.managed_paths,
                manifest.storage_id,
                e
            );
            return Err(e).with_context(|| {
                format!(
                    "Failed to write .hifimule.json on device {} via {:?}",
                    device_path, device_class
                )
            });
        }

        {
            let mut state = self.state.write().await;
            state.connected_devices.insert(
                device_root.clone(),
                ConnectedDevice {
                    manifest: manifest.clone(),
                    device_class,
                    device_io,
                    observation_token,
                },
            );
            state.pending_devices.remove(&pending_id);
            if matches!(&state.selected_destination, SelectedDestination::Pending(id) if id == &pending_id)
            {
                state.selected_device_path = Some(device_root.clone());
                state.selected_destination = SelectedDestination::Device(device_root.clone());
            }
            state.destination_revision += 1;
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
        folders.sort_by_key(|a| a.name.to_lowercase());

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

    /// Clears the dirty flag on the manifest once all missing files are resolved.
    /// Orphaned files (on disk but not in manifest) are not required to be cleared —
    /// they will be handled by the next sync.
    pub async fn clear_dirty_flag(&self) -> Result<()> {
        let discrepancies = self
            .get_discrepancies()
            .await?
            .ok_or_else(|| anyhow::anyhow!("No device connected"))?;
        if !discrepancies.missing.is_empty() {
            return Err(anyhow::anyhow!(
                "Cannot clear dirty flag: {} missing file(s) still need to be resolved",
                discrepancies.missing.len()
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
fn is_usable_removable_drive(drive_type: u32, media_present: bool) -> bool {
    drive_type == 2 && media_present // DRIVE_REMOVABLE
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
    is_usable_removable_drive(
        unsafe { GetDriveTypeW(wide.as_ptr()) },
        get_volume_label(&path.to_string_lossy()).is_some(),
    )
}

#[cfg(not(target_os = "windows"))]
fn is_removable_drive(_path: &Path) -> bool {
    true // macOS/Linux already filtered by mount detection
}

#[cfg(target_os = "windows")]
fn friendly_name_drive_root(friendly_name: &str) -> Option<PathBuf> {
    let s = friendly_name.trim();
    let bytes = s.as_bytes();
    if bytes.len() == 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/')
    {
        Some(PathBuf::from(format!(
            "{}:\\",
            (bytes[0] as char).to_ascii_uppercase()
        )))
    } else {
        None
    }
}

/// Returns true if a removable drive exists that corresponds to the same physical USB device
/// as the MTP device identified by `wpd_device_id`. Matching uses hardware instance IDs
/// (`SetupDiGetDeviceInstanceIdW`) to avoid false positives from two drives sharing a label.
/// Falls back to volume-label comparison if hardware-ID lookup fails for a given drive.
#[cfg(target_os = "windows")]
fn has_msc_drive_for_device(friendly_name: &str, wpd_device_id: &str) -> bool {
    use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
        DIGCF_PRESENT, SP_DEVINFO_DATA, SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo,
        SetupDiGetClassDevsW, SetupDiGetDeviceInstanceIdW,
    };
    use windows_sys::Win32::Storage::FileSystem::GetLogicalDrives;

    if let Some(path) = friendly_name_drive_root(friendly_name) {
        return is_removable_drive(&path);
    }

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
    let mut retry_state: std::collections::HashMap<PathBuf, (u32, std::time::Instant)> =
        std::collections::HashMap::new();

    loop {
        let current_mounts = get_mounts();

        // Filter stale observer state through the current safe mount list each cycle.
        // This evicts boot/system volumes captured by older binaries before new detection.
        known_mounts.retain(|mount| {
            if !current_mounts.contains(mount) {
                let _ = tx.try_send(DeviceEvent::Removed(mount.clone()));
                retry_state.remove(mount);
                false
            } else {
                true
            }
        });

        // Detect new mounts
        for mount in &current_mounts {
            if !known_mounts.contains(mount) {
                if retry_state
                    .get(mount)
                    .is_some_and(|(_, next)| std::time::Instant::now() < *next)
                {
                    continue;
                }
                known_mounts.insert(mount.clone());
                // Stamp the observation before any potentially slow probe so a late
                // completion cannot override a newer explicit destination choice.
                let observation_token = next_destination_mutation();
                match DeviceProber::probe(mount).await {
                    Ok(Some(manifest)) => {
                        retry_state.remove(mount);
                        let device_io: std::sync::Arc<dyn crate::device_io::DeviceIO> =
                            std::sync::Arc::new(crate::device_io::MscBackend::new(mount.clone()));
                        let _ = tx
                            .send(DeviceEvent::Detected {
                                observation_token,
                                path: mount.clone(),
                                manifest,
                                device_io,
                            })
                            .await;
                    }
                    Ok(None) => {
                        retry_state.remove(mount);
                        if is_removable_drive(mount) {
                            // Detection layer creates the IO backend so the type (MSC/MTP)
                            // is determined here once, not re-derived downstream.
                            let device_io: std::sync::Arc<dyn crate::device_io::DeviceIO> =
                                std::sync::Arc::new(crate::device_io::MscBackend::new(
                                    mount.clone(),
                                ));
                            let _ = tx
                                .send(DeviceEvent::Unrecognized {
                                    observation_token,
                                    path: mount.clone(),
                                    device_io,
                                    friendly_name: None,
                                })
                                .await;
                        }
                    }
                    Err(_) => {
                        known_mounts.remove(mount);
                        let attempt = retry_state
                            .get(mount)
                            .map_or(1, |(attempt, _)| attempt.saturating_add(1));
                        let seconds = match attempt {
                            1 => 2,
                            2 => 4,
                            3 => 8,
                            4 => 16,
                            _ => 30,
                        };
                        retry_state.insert(
                            mount.clone(),
                            (
                                attempt,
                                std::time::Instant::now() + Duration::from_secs(seconds),
                            ),
                        );
                        let _ = tx
                            .send(DeviceEvent::DiscoveryFailed {
                                path: mount.clone(),
                                code: "DEVICE_READ_FAILED",
                                display_name: mount
                                    .file_name()
                                    .map(|name| name.to_string_lossy().chars().take(120).collect()),
                            })
                            .await;
                    }
                }
            }
        }

        sleep(Duration::from_secs(2)).await;
    }
}

/// Starts one passive MTP enumeration outside Tokio's blocking pool.
///
/// `LIBMTP_Detect_Raw_Devices` is synchronous and cannot be cancelled safely.  In
/// particular, dropping the daemon's Tokio runtime waits for its blocking pool,
/// so scheduling discovery there makes an otherwise idle quit wait on a USB
/// library call.  This deliberately detached thread is only for observational
/// discovery; active MTP I/O continues to use the core runtime's blocking pool
/// and therefore retains its orderly shutdown drain.
pub(crate) fn spawn_passive_mtp_enumeration<F>(
    enumerate: F,
) -> std::io::Result<tokio::sync::oneshot::Receiver<Result<Vec<mtp::MtpDeviceInfo>>>>
where
    F: FnOnce() -> Result<Vec<mtp::MtpDeviceInfo>> + Send + 'static,
{
    let (result_tx, result_rx) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("hifimule-mtp-discovery".into())
        .spawn(move || {
            // The receiver can disappear when the observer is aborted for shutdown.
            // In that case the late discovery result must not re-enter the core.
            let _ = result_tx.send(enumerate());
        })?;
    Ok(result_rx)
}

type MtpEnumerator = std::sync::Arc<dyn Fn() -> Result<Vec<mtp::MtpDeviceInfo>> + Send + Sync>;

pub async fn run_mtp_observer(tx: tokio::sync::mpsc::Sender<DeviceEvent>) {
    run_mtp_observer_with_enumerator(
        tx,
        std::sync::Arc::new(|| Ok(mtp::enumerate_mtp_devices())),
        Duration::from_secs(2),
    )
    .await;
}

pub(crate) async fn run_mtp_observer_with_enumerator(
    tx: tokio::sync::mpsc::Sender<DeviceEvent>,
    enumerate: MtpEnumerator,
    poll_interval: Duration,
) {
    let mut known_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut failed_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut retry_state: std::collections::HashMap<String, (u32, std::time::Instant)> =
        std::collections::HashMap::new();
    let mut liveness_tick: u32 = 0;

    loop {
        let enumerator = std::sync::Arc::clone(&enumerate);
        let devices = match spawn_passive_mtp_enumeration(move || enumerator()) {
            Ok(result_rx) => match result_rx.await {
                Ok(Ok(devices)) => devices,
                Ok(Err(error)) => {
                    daemon_log!("[MTP] Passive device enumeration failed: {}", error);
                    Vec::new()
                }
                Err(_) => {
                    daemon_log!("[MTP] Passive device enumeration task ended unexpectedly");
                    Vec::new()
                }
            },
            Err(error) => {
                daemon_log!(
                    "[MTP] Could not start passive device enumeration: {}",
                    error
                );
                Vec::new()
            }
        };

        for dev in &devices {
            if !known_ids.contains(&dev.device_id) {
                if retry_state
                    .get(&dev.device_id)
                    .is_some_and(|(_, next)| std::time::Instant::now() < *next)
                {
                    continue;
                }
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
                // Capture ordering before opening or reading the device. MTP can block
                // long enough for the user to make a newer selection meanwhile.
                let observation_token = next_destination_mutation();

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
                            observation_token,
                            synthetic_path,
                            &dev_id,
                            dev_for_probe,
                            friendly_name,
                            backend_arc,
                        )
                        .await
                        {
                            known_ids.insert(dev_id.clone());
                            failed_ids.remove(&dev_id);
                            retry_state.remove(&dev_id);
                        } else {
                            let attempt = retry_state
                                .get(&dev_id)
                                .map_or(1, |(attempt, _)| attempt.saturating_add(1));
                            let seconds = match attempt {
                                1 => 2,
                                2 => 4,
                                3 => 8,
                                4 => 16,
                                _ => 30,
                            };
                            retry_state.insert(
                                dev_id.clone(),
                                (
                                    attempt,
                                    std::time::Instant::now() + Duration::from_secs(seconds),
                                ),
                            );
                            failed_ids.insert(dev_id.clone());
                        }
                    }
                    Err(e) => {
                        daemon_log!("[MTP] Failed to open device {}: {}", dev_id, e);
                        let attempt = retry_state
                            .get(&dev_id)
                            .map_or(1, |(attempt, _)| attempt.saturating_add(1));
                        let seconds = match attempt {
                            1 => 2,
                            2 => 4,
                            3 => 8,
                            4 => 16,
                            _ => 30,
                        };
                        retry_state.insert(
                            dev_id.clone(),
                            (
                                attempt,
                                std::time::Instant::now() + Duration::from_secs(seconds),
                            ),
                        );
                        failed_ids.insert(dev_id.clone());
                        let _ = tx
                            .send(DeviceEvent::DiscoveryFailed {
                                path: synthetic_path,
                                code: "DEVICE_OPEN_FAILED",
                                display_name: Some(friendly_name.chars().take(120).collect()),
                            })
                            .await;
                    }
                }
            }
        }

        let disconnected: Vec<String> = known_ids
            .iter()
            .chain(failed_ids.iter())
            .filter(|id| !devices.iter().any(|d| &d.device_id == *id))
            .cloned()
            .collect();
        for id in &disconnected {
            let synthetic_path = PathBuf::from(format!("mtp://{}", id));
            let _ = tx.send(DeviceEvent::Removed(synthetic_path)).await;
            known_ids.remove(id);
            failed_ids.remove(id);
            retry_state.remove(id);
        }

        // Liveness probe for known devices (~every 16 s). Handles the case where Android
        // disables MTP without triggering a USB re-enumerate, so the device stays in the
        // enumeration list but storage is no longer accessible.
        liveness_tick = liveness_tick.wrapping_add(1);
        if liveness_tick.is_multiple_of(8) {
            for dev in &devices {
                let dev_id = dev.device_id.clone();
                if !known_ids.contains(&dev_id) {
                    continue;
                }
                let dev_clone = dev.clone();
                let probe =
                    tokio::task::spawn_blocking(move || mtp::create_mtp_backend(&dev_clone, None))
                        .await
                        .unwrap_or_else(|e| Err(anyhow::anyhow!("probe panicked: {}", e)));
                if let Err(e) = probe {
                    // Only "MTP storage not accessible" means the phone disabled MTP.
                    // Other errors (e.g. device busy during an active sync) are ignored.
                    if e.to_string().contains("MTP storage not accessible") {
                        daemon_log!(
                            "[MTP] Device {} lost storage access — treating as disconnected",
                            dev_id
                        );
                        let synthetic_path = PathBuf::from(format!("mtp://{}", dev_id));
                        let _ = tx.send(DeviceEvent::Removed(synthetic_path)).await;
                        known_ids.remove(&dev_id);
                    }
                }
                // Ok(_) → storage still accessible; drop the handle immediately.
            }
        }

        sleep(poll_interval).await;
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
    if message.contains(".hifimule.json")
        && (message.contains("not found") || message.contains("no such file"))
    {
        return true;
    }

    false
}

async fn emit_mtp_probe_event(
    tx: &tokio::sync::mpsc::Sender<DeviceEvent>,
    observation_token: u64,
    synthetic_path: PathBuf,
    dev_id: &str,
    dev_info: mtp::MtpDeviceInfo,
    friendly_name: String,
    backend_arc: std::sync::Arc<dyn crate::device_io::DeviceIO>,
) -> bool {
    // Prefer the local manifest cache for MTP devices — it is the authoritative store
    // (updated by update_manifest on every successful sync), so it survives device
    // reconnects and daemon restarts without depending on an MTP write succeeding.
    let local_manifest: Option<DeviceManifest> = crate::paths::get_local_mtp_manifest_path(dev_id)
        .ok()
        .and_then(|p| std::fs::read_to_string(&p).ok())
        .and_then(|s| serde_json::from_str(&s).ok());

    if let Some(manifest) = local_manifest {
        daemon_log!(
            "[MTP] Using local manifest cache for device_id={} friendly_name={}",
            dev_id,
            friendly_name
        );
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
        return tx
            .send(DeviceEvent::Detected {
                observation_token,
                path: synthetic_path,
                manifest,
                device_io: final_io,
            })
            .await
            .is_ok();
    }

    match backend_arc.read_file(".hifimule.json").await {
        Ok(data) => match serde_json::from_slice::<DeviceManifest>(&data) {
            Ok(device_manifest) => {
                // The backend probe identity can differ from the portable manifest
                // identity. Once the on-device copy establishes that mapping, prefer
                // the matching authoritative local cache so older clean mirror bytes
                // cannot hide interrupted/dirty recovery evidence.
                let manifest =
                    crate::paths::get_local_mtp_manifest_path(&device_manifest.device_id)
                        .ok()
                        .and_then(|path| std::fs::read_to_string(path).ok())
                        .and_then(|json| serde_json::from_str::<DeviceManifest>(&json).ok())
                        .unwrap_or(device_manifest);
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
                    observation_token,
                    path: synthetic_path,
                    manifest,
                    device_io: final_io,
                })
                .await
                .is_ok()
            }
            Err(e) => {
                daemon_log!(
                    "[MTP] Manifest parse error during probe path=.hifimule.json device_id={} friendly_name={} synthetic_path={} error={:#}; publishing sanitized read failure",
                    dev_id,
                    friendly_name,
                    synthetic_path.display(),
                    e
                );
                let _ = tx
                    .send(DeviceEvent::DiscoveryFailed {
                        path: synthetic_path,
                        code: "DEVICE_READ_FAILED",
                        display_name: Some(friendly_name),
                    })
                    .await;
                false
            }
        },
        Err(e) => {
            if is_missing_manifest_error(&e) {
                daemon_log!(
                    "[MTP] Manifest missing during probe path=.hifimule.json device_id={} friendly_name={} synthetic_path={} error={:#}; treating device as unrecognized",
                    dev_id,
                    friendly_name,
                    synthetic_path.display(),
                    e
                );
                tx.send(DeviceEvent::Unrecognized {
                    observation_token,
                    path: synthetic_path,
                    device_io: backend_arc,
                    friendly_name: Some(friendly_name),
                })
                .await
                .is_ok()
            } else {
                daemon_log!(
                    "[MTP] Manifest read failed during probe path=.hifimule.json device_id={} friendly_name={} synthetic_path={} error={:#}; suppressing device event until next poll",
                    dev_id,
                    friendly_name,
                    synthetic_path.display(),
                    e
                );
                let _ = tx
                    .send(DeviceEvent::DiscoveryFailed {
                        path: synthetic_path,
                        code: "DEVICE_READ_FAILED",
                        display_name: Some(friendly_name),
                    })
                    .await;
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
    if let (Some(parent), Ok(path_meta)) = (path.parent(), std::fs::metadata(path))
        && let Ok(parent_meta) = std::fs::metadata(parent)
    {
        return parent_meta.dev() != path_meta.dev();
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
fn mount_flags(path: &Path) -> Option<u32> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stat = std::mem::MaybeUninit::<libc::statfs>::uninit();
    // SAFETY: c_path is NUL-terminated and stat points to writable statfs storage.
    if unsafe { libc::statfs(c_path.as_ptr(), stat.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: a successful statfs call initialized stat.
    Some(unsafe { stat.assume_init() }.f_flags)
}

#[cfg(target_os = "macos")]
fn mount_flags_allow_discovery(flags: u32) -> bool {
    flags & (libc::MNT_RDONLY | libc::MNT_DONTBROWSE) as u32 == 0
}

#[cfg(target_os = "macos")]
fn is_discoverable_mount(path: &Path) -> bool {
    // Unavailable metadata skips this candidate until a subsequent scan.
    mount_flags(path).is_some_and(mount_flags_allow_discovery)
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
                // Skip read-only and hidden volumes (including macOS Recovery)
                // before manifest probing or an unrecognized-device prompt.
                if !is_discoverable_mount(&path) {
                    continue;
                }
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
