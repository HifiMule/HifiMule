#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::Result;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::thread;
use std::time::{Duration, Instant};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tray_icon::{
    Icon, TrayIconBuilder,
    menu::{Menu, MenuEvent, MenuItem},
};

#[cfg(windows)]
mod service;

const LOG_MAX_BYTES: u64 = 1_048_576; // 1 MB
const MAX_TOKIO_WORKER_THREADS: usize = 4; // Limit worker threads to prevent resource contention on low-end systems

/// Simple file-based logger for release mode where stdout/stderr are unavailable.
/// Writes to `%APPDATA%/HifiMule/daemon.log`. Truncates at 1 MB.
pub fn log_to_file(msg: &str) {
    if let Ok(dir) = paths::get_app_data_dir() {
        let log_path = dir.join("daemon.log");
        // Truncate if over 1 MB
        if let Ok(meta) = std::fs::metadata(&log_path) {
            if meta.len() > LOG_MAX_BYTES {
                let _ = std::fs::write(&log_path, "--- log truncated ---\n");
            }
        }
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
        {
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let _ = writeln!(f, "[{}] {}", timestamp, msg);
        }
    }
}

#[macro_export]
macro_rules! daemon_log {
    ($($arg:tt)*) => {{
        let msg = format!($($arg)*);
        println!("{}", msg);
        $crate::log_to_file(&msg);
    }};
}

mod api;
mod auto_fill;
mod db;
mod device;
pub mod device_io;
// Provider catalog models are part of the multi-provider contract, even while
// the current daemon binary only uses the sync-facing subset.
#[allow(dead_code)]
mod domain;
mod paths;
#[allow(dead_code)]
mod providers;
mod rpc;
mod scrobbler;
mod sync;
mod transcoding;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum DaemonState {
    Idle,
    Syncing,
    Scanning,
    DeviceFound(String),
    DeviceRecognized { name: String, profile_id: String },
    Error,
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let service_mode = args.iter().any(|arg| arg == "--service");
    let install_service = args.iter().any(|arg| arg == "--install-service");
    let uninstall_service = args.iter().any(|arg| arg == "--uninstall-service");

    daemon_log!(
        "Daemon process starting (release={}, service={})",
        !cfg!(debug_assertions),
        service_mode
    );

    #[cfg(windows)]
    {
        if install_service {
            return service::install().map_err(|e| e.into());
        }
        if uninstall_service {
            return service::uninstall().map_err(|e| e.into());
        }
        if service_mode {
            return service::run().map_err(|e| e.into());
        }
    }

    #[cfg(not(windows))]
    {
        if service_mode || install_service || uninstall_service {
            anyhow::bail!("Service flags are only supported on Windows");
        }
    }

    run_interactive()
}

/// Starts the core daemon logic (RPC server, device observer, event handling)
/// in a background thread. Returns the shutdown signal and state receiver.
/// The caller is responsible for the main thread's event loop (tray icon or service wait).
pub fn start_daemon_core() -> Result<(Arc<AtomicBool>, mpsc::Receiver<DaemonState>)> {
    let (state_tx, state_rx) = mpsc::channel::<DaemonState>();
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = Arc::clone(&shutdown);

    // Start Tokio runtime in a background thread
    // REQUIRED for macOS: main thread MUST handle the event loop
    thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(daemon_worker_threads())
            .enable_all()
            .build()
            .expect("Failed to build tokio runtime");

        rt.block_on(async {
            daemon_log!("HifiMule Daemon tokio runtime started");

            // Initialize database
            let db_path = match paths::get_app_data_dir() {
                Ok(p) => p.join("hifimule.db"),
                Err(e) => {
                    daemon_log!("Failed to get app data directory: {}", e);
                    let _ = state_tx.send(DaemonState::Error);
                    return;
                }
            };
            let db = match db::Database::new(db_path) {
                Ok(db) => Arc::new(db),
                Err(e) => {
                    daemon_log!("Failed to initialize database: {}", e);
                    let _ = state_tx.send(DaemonState::Error);
                    return;
                }
            };

            // Seed default device-profiles.json if not present
            let profiles_default = include_bytes!("../assets/device-profiles.json");
            if let Ok(profiles_path) = crate::paths::get_device_profiles_path() {
                if let Err(e) = crate::transcoding::ensure_profiles_file_exists(&profiles_path, profiles_default) {
                    daemon_log!("Warning: Failed to seed device-profiles.json: {}", e);
                    // Non-fatal — transcoding will be unavailable until the file exists
                }
            }

            // Initial state
            if let Err(e) = state_tx.send(DaemonState::Idle) {
                daemon_log!("Failed to send initial state: {}", e);
                return;
            }

            // Start Device Observer
            let (device_tx, mut device_rx) = tokio::sync::mpsc::channel(10);
            let device_tx_msc = device_tx.clone();
            tokio::spawn(async move {
                device::run_observer(device_tx_msc).await;
            });

            // Start MTP Observer
            let device_tx_mtp = device_tx.clone();
            tokio::spawn(async move {
                device::run_mtp_observer(device_tx_mtp).await;
            });

            // Initialize Device Manager
            let device_manager = Arc::new(device::DeviceManager::new(Arc::clone(&db)));

            // Shared scrobbler result state
            let last_scrobbler_result: Arc<
                tokio::sync::RwLock<Option<scrobbler::ScrobblerResult>>,
            > = Arc::new(tokio::sync::RwLock::new(None));

            // Initialize shared sync operation manager
            let sync_operation_manager = Arc::new(sync::SyncOperationManager::new());

            // Start RPC server
            daemon_log!("Starting RPC server on port 19140");
            let db_clone = Arc::clone(&db);
            let dm_clone = Arc::clone(&device_manager);
            let scrobbler_result_rpc = Arc::clone(&last_scrobbler_result);
            let state_tx_rpc = state_tx.clone();
            let som_rpc = Arc::clone(&sync_operation_manager);
            tokio::spawn(async move {
                rpc::run_server(19140, db_clone, dm_clone, scrobbler_result_rpc, state_tx_rpc, som_rpc).await;
            });

            // Handle Device Events
            let state_tx_clone = state_tx.clone();
            let jellyfin_client = Arc::new(api::JellyfinClient::new());
            let som_events = Arc::clone(&sync_operation_manager);
            tokio::spawn(async move {
                while let Some(event) = device_rx.recv().await {
                    match event {
                        device::DeviceEvent::Detected { path, manifest, device_io } => {
                            daemon_log!("Device detected at {:?}: {:?}", path, manifest);
                            let auto_sync_enabled = manifest.auto_sync_on_connect;
                            let has_basket = !manifest.basket_items.is_empty();
                            let auto_fill_enabled = manifest.auto_fill.enabled;
                            let has_synced_items = !manifest.synced_items.is_empty();
                            let manifest_device_id = manifest.device_id.clone();
                            let scrobble_manifest = Arc::new(manifest.clone());
                            match device_manager.handle_device_detected(path.clone(), manifest, device_io).await {
                                Ok(new_state) => {
                                    let _ = state_tx_clone.send(new_state);
                                }
                                Err(e) => {
                                    daemon_log!("Error handling device detection: {}", e);
                                    let _ = state_tx_clone.send(DaemonState::Error);
                                }
                            }

                            // Spawn background scrobbler task
                            if let Ok((url, token, Some(user_id))) =
                                api::CredentialManager::get_credentials()
                            {
                                match device_manager.get_device_io().await {
                                    Some(scrobble_device_io) => {
                                        let db_scrobble = Arc::clone(&db);
                                        let client_scrobble = Arc::clone(&jellyfin_client);
                                        let scrobbler_result_clone = Arc::clone(&last_scrobbler_result);
                                        let scrobble_device_id = manifest_device_id.clone();
                                        let scrobble_manifest = Arc::clone(&scrobble_manifest);
                                        tokio::spawn(async move {
                                            let result = scrobbler::process_device_scrobbles(
                                                scrobble_device_io,
                                                scrobble_device_id,
                                                Some(scrobble_manifest),
                                                db_scrobble,
                                                client_scrobble,
                                                &url,
                                                &token,
                                                &user_id,
                                            )
                                            .await;
                                            daemon_log!("[Scrobbler] Result: {:?}", result);
                                            let mut guard = scrobbler_result_clone.write().await;
                                            *guard = Some(result);
                                        });
                                    }
                                    None => {
                                        daemon_log!("[Scrobbler] Skipped — device IO unavailable (device may have disconnected)");
                                    }
                                }
                            }

                            // Auto-sync trigger: the connected manifest is the source of truth.
                            // SQLite may be stale or absent when the UI has not opened yet.
                            if auto_sync_enabled && (has_basket || auto_fill_enabled || has_synced_items) {
                                let has_active_sync = som_events.has_active_operation().await;

                                if !has_active_sync {
                                    let dm = Arc::clone(&device_manager);
                                    let som = Arc::clone(&som_events);
                                    let state_tx_sync = state_tx_clone.clone();
                                    let device_path = path.clone();

                                    let subsonic_in_db = db.get_server_config()
                                        .ok().flatten()
                                        .map(|c| matches!(c.server_type.as_str(), "subsonic" | "openSubsonic"))
                                        .unwrap_or(false);

                                    if subsonic_in_db {
                                        if let Some(provider) = get_non_jellyfin_provider(&db).await {
                                            tokio::spawn(async move {
                                                daemon_log!("[AutoSync] Starting auto-sync via provider");
                                                if let Err(e) = run_auto_sync_via_provider(
                                                    provider, dm, som, state_tx_sync, device_path,
                                                ).await {
                                                    daemon_log!("[AutoSync] Provider auto-sync failed: {}", e);
                                                }
                                            });
                                        } else {
                                            daemon_log!("[AutoSync] Skipped: could not connect to Subsonic/Navidrome server");
                                        }
                                    } else if let Ok((url, token, user_id)) =
                                        api::CredentialManager::get_credentials()
                                    {
                                        let user_id = user_id.unwrap_or_else(|| "Me".to_string());
                                        let client = Arc::clone(&jellyfin_client);
                                        tokio::spawn(async move {
                                            daemon_log!("[AutoSync] Starting auto-sync for device");
                                            if let Err(e) = run_auto_sync(
                                                client, dm, som, state_tx_sync,
                                                device_path, url, token, user_id,
                                            ).await {
                                                daemon_log!("[AutoSync] Failed: {}", e);
                                            }
                                        });
                                    } else {
                                        daemon_log!("[AutoSync] Skipped: no credentials available");
                                    }
                                }
                            } else if auto_sync_enabled && !has_basket && !auto_fill_enabled {
                                daemon_log!("[AutoSync] Skipped: auto-sync enabled but no basket items configured");
                            }
                        }
                        device::DeviceEvent::Unrecognized { path, device_io, friendly_name } => {
                            println!("Unrecognized device at {:?}", path);
                            let new_state = device_manager.handle_device_unrecognized(path, device_io, friendly_name).await;
                            let _ = state_tx_clone.send(new_state);
                        }
                        device::DeviceEvent::Removed(path) => {
                            daemon_log!("Device removed at {:?}", path);
                            // If a sync is running, mark it as failed before clearing device state
                            if som_events.has_active_operation().await {
                                daemon_log!("[AutoSync] Device removed during active sync — marking failed");
                                let ops_snapshot = som_events.get_all_operations().await;
                                for mut op in ops_snapshot {
                                    if op.status == sync::SyncStatus::Running {
                                        op.status = sync::SyncStatus::Failed;
                                        op.errors.push(sync::SyncFileError {
                                            jellyfin_id: String::new(),
                                            filename: String::new(),
                                            error_message: hifimule_i18n::t(
                                                "error.device_removed_during_sync",
                                            ),
                                        });
                                        som_events.update_operation(&op.id.clone(), op).await;
                                    }
                                }
                                let _ = tokio::task::spawn_blocking(|| {
                                    if let Err(e) = notify_rust::Notification::new()
                                        .summary(&hifimule_i18n::t("app.name"))
                                        .body(&hifimule_i18n::t(
                                            "notification.sync_interrupted_removed",
                                        ))
                                        .show()
                                    {
                                        daemon_log!("[AutoSync] Notification failed: {}", e);
                                    }
                                });
                                let _ = state_tx_clone.send(DaemonState::Error);
                            } else {
                                let _ = state_tx_clone.send(DaemonState::Idle);
                            }
                            device_manager.handle_device_removed(&path).await;
                        }
                    }
                }
            });

            // Daemon work loop - check for shutdown signal
            while !shutdown_clone.load(Ordering::Relaxed) {
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }

            daemon_log!("HifiMule Daemon shutting down gracefully");
        });
    });

    Ok((shutdown, state_rx))
}

fn daemon_worker_threads() -> usize {
    std::thread::available_parallelism()
        .map(|cores| cores.get().min(MAX_TOKIO_WORKER_THREADS))
        .unwrap_or(1)
}

/// Interactive mode: tray icon + event loop on the main thread
fn run_interactive() -> Result<()> {
    let (shutdown, state_rx) = start_daemon_core()?;

    // 3. Setup Tray Icon and Event Loop on the main thread
    #[cfg(target_os = "macos")]
    let mut event_loop = EventLoopBuilder::new().build();
    #[cfg(not(target_os = "macos"))]
    let event_loop = EventLoopBuilder::new().build();
    #[cfg(target_os = "macos")]
    {
        use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
        event_loop.set_activation_policy(ActivationPolicy::Accessory);

        // Pre-set the notification bundle ID so mac-notification-sys doesn't
        // run an AppleScript lookup for "use_default", which causes macOS to
        // show a "Choose Application" dialog at the end of a sync.
        let _ = mac_notification_sys::set_application("hifimule.github.io");
    }

    // Load icons from assets (embedded using include_bytes!)
    // Use Arc to avoid cloning large icon data in the event loop
    let icon_idle = Arc::new(load_icon(include_bytes!("../assets/icon.png"), "idle")?);
    let icon_syncing = Arc::new(load_icon(
        include_bytes!("../assets/icon_syncing.png"),
        "syncing",
    )?);
    let icon_error = Arc::new(load_icon(
        include_bytes!("../assets/icon_error.png"),
        "error",
    )?);

    // Setup Menu
    let tray_menu = Menu::new();
    let quit_item = MenuItem::new(hifimule_i18n::t("tray.quit"), true, None);
    let open_ui_item = MenuItem::new(hifimule_i18n::t("tray.open_ui"), true, None);
    tray_menu
        .append_items(&[&open_ui_item, &quit_item])
        .map_err(|e| anyhow::anyhow!("Failed to create tray menu: {}", e))?;

    let mut tray_icon = Some(
        TrayIconBuilder::new()
            .with_menu(Box::new(tray_menu))
            .with_tooltip(hifimule_i18n::t("tray.tooltip.idle"))
            .with_icon((*icon_idle).clone())
            .build()?,
    );

    let menu_channel = MenuEvent::receiver();

    // 4. Run the event loop
    // This will block the main thread
    event_loop.run(move |_event, _, control_flow| {
        // WaitUntil lets the OS sleep this thread until a native event arrives or the
        // deadline expires. ControlFlow::Poll would spin at 100% CPU when idle.
        *control_flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(250));

        // Handle state updates from tokio thread
        if let Ok(state) = state_rx.try_recv() {
            if let Some(ref mut tray) = tray_icon {
                match state {
                    DaemonState::Idle => {
                        let _ = tray.set_tooltip(Some(&hifimule_i18n::t("tray.tooltip.idle")));
                        let _ = tray.set_icon(Some((*icon_idle).clone()));
                    }
                    DaemonState::Syncing => {
                        let _ = tray.set_tooltip(Some(&hifimule_i18n::t("tray.tooltip.syncing")));
                        let _ = tray.set_icon(Some((*icon_syncing).clone()));
                    }
                    DaemonState::Scanning => {
                        let _ = tray.set_tooltip(Some(&hifimule_i18n::t("tray.tooltip.scanning")));
                        let _ = tray.set_icon(Some((*icon_syncing).clone()));
                    }
                    DaemonState::DeviceFound(name) => {
                        let _ = tray.set_tooltip(Some(&hifimule_i18n::tf(
                            "tray.tooltip.found",
                            &[("name", &name)],
                        )));
                        let _ = tray.set_icon(Some((*icon_syncing).clone()));
                    }
                    DaemonState::DeviceRecognized { name, profile_id } => {
                        let _ = tray.set_tooltip(Some(&hifimule_i18n::tf(
                            "tray.tooltip.recognized",
                            &[("name", &name), ("profile", &profile_id)],
                        )));
                        let _ = tray.set_icon(Some((*icon_syncing).clone()));
                    }
                    DaemonState::Error => {
                        let _ = tray.set_tooltip(Some(&hifimule_i18n::t("tray.tooltip.error")));
                        let _ = tray.set_icon(Some((*icon_error).clone()));
                    }
                }
            }
        }

        // Handle menu events (Quit, Open UI)
        if let Ok(event) = menu_channel.try_recv() {
            if event.id == quit_item.id() {
                println!("Quit requested - shutting down gracefully");

                // Signal tokio thread to shutdown
                shutdown.store(true, Ordering::Relaxed);

                // Clean up tray icon
                tray_icon.take();

                // Exit event loop
                *control_flow = ControlFlow::Exit;
            } else if event.id == open_ui_item.id() {
                println!("'Open UI' clicked - Launching Tauri UI...");

                let status = if cfg!(debug_assertions) {
                    // Use Cargo's compile-time manifest path so Windows debug
                    // launches do not depend on process env vars or cwd.
                    let ui_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                        .parent()
                        .map(|p| p.join("hifimule-ui"))
                        .unwrap_or_else(|| std::path::PathBuf::from("../hifimule-ui"));

                    #[cfg(windows)]
                    {
                        std::process::Command::new("cmd")
                            .args(["/C", "npm", "run", "tauri", "dev"])
                            .current_dir(ui_dir)
                            .spawn()
                    }
                    #[cfg(not(windows))]
                    {
                        std::process::Command::new("npm")
                            .args(["run", "tauri", "dev"])
                            .current_dir(ui_dir)
                            .spawn()
                    }
                } else {
                    // In release, we assume the UI executable is in the same folder
                    let mut ui_path = std::env::current_exe().unwrap_or_default();
                    let ui_name = if cfg!(windows) {
                        "hifimule-ui.exe"
                    } else {
                        "hifimule-ui"
                    };
                    ui_path.set_file_name(ui_name);

                    if ui_path.exists() {
                        std::process::Command::new(ui_path).spawn()
                    } else {
                        Err(std::io::Error::new(
                            std::io::ErrorKind::NotFound,
                            format!("UI executable not found at {:?}", ui_path),
                        ))
                    }
                };

                if let Err(e) = status {
                    eprintln!("Failed to launch UI: {}", e);
                }
            }
        }
    });
}

/// Runs auto-sync for a device that has `auto_sync_on_connect` enabled.
/// Resolves basket items into a sync delta, then executes the sync operation.
async fn run_auto_sync(
    jellyfin_client: Arc<api::JellyfinClient>,
    device_manager: Arc<device::DeviceManager>,
    sync_op_manager: Arc<sync::SyncOperationManager>,
    state_tx: std::sync::mpsc::Sender<DaemonState>,
    device_path: std::path::PathBuf,
    url: String,
    token: String,
    user_id: String,
) -> anyhow::Result<()> {
    // Signal syncing state immediately so tray icon updates before any network activity
    let _ = state_tx.send(DaemonState::Syncing);

    let manifest = device_manager
        .get_current_device()
        .await
        .ok_or_else(|| anyhow::anyhow!("No device connected"))?;

    let mut desired_items = Vec::new();
    let mut playlist_sync_items: Vec<sync::PlaylistSyncItem> = Vec::new();

    if manifest.basket_items.is_empty() {
        if manifest.auto_fill.enabled {
            // No manual basket — run auto-fill algorithm to derive desired items
            daemon_log!("[AutoSync] Basket empty, running auto-fill algorithm");
            let max_fill_bytes = if let Some(mb) = manifest.auto_fill.max_bytes {
                mb
            } else {
                match device_manager.get_device_storage().await {
                    Some(info) => info.free_bytes,
                    None => {
                        daemon_log!("[AutoSync] Cannot determine device capacity for auto-fill");
                        let _ = state_tx.send(DaemonState::Idle);
                        return Ok(());
                    }
                }
            };
            // Pass any already-synced basket item IDs as exclusions so auto-fill
            // doesn't re-select tracks the device has from a previous manual sync.
            let exclude_item_ids: Vec<String> =
                manifest.basket_items.iter().map(|b| b.id.clone()).collect();
            let fill_params = crate::auto_fill::AutoFillParams {
                exclude_item_ids,
                max_fill_bytes,
            };
            match crate::auto_fill::run_auto_fill(&jellyfin_client, fill_params).await {
                Ok(items) if items.is_empty() => {
                    daemon_log!("[AutoSync] Auto-fill returned no items, skipping");
                    let _ = state_tx.send(DaemonState::Idle);
                    return Ok(());
                }
                Ok(items) => {
                    daemon_log!("[AutoSync] Auto-fill resolved {} items", items.len());
                    for item in items {
                        desired_items.push(sync::DesiredItem {
                            jellyfin_id: item.id,
                            name: item.name,
                            album: item.album,
                            artist: item.artist,
                            size_bytes: item.size_bytes,
                            etag: None,
                            provider_album_id: item.provider_album_id,
                            provider_content_type: None,
                            provider_suffix: item.provider_suffix,
                            original_bitrate: None,
                            track_number: None,
                        });
                    }
                }
                Err(e) => {
                    daemon_log!("[AutoSync] Auto-fill failed: {}", e);
                    let _ = state_tx.send(DaemonState::Error);
                    return Ok(());
                }
            }
        } else if manifest.synced_items.is_empty() {
            daemon_log!("[AutoSync] No basket items and no synced items, skipping");
            let _ = state_tx.send(DaemonState::Idle);
            return Ok(());
        } else {
            daemon_log!("[AutoSync] Basket empty but device has {} synced item(s) — running cleanup sync", manifest.synced_items.len());
            // desired_items stays empty; calculate_delta will mark all synced items for deletion.
        }
    } else {
        // Manual basket: resolve basket items to desired items via Jellyfin API
        let item_ids: Vec<String> = manifest.basket_items.iter().map(|b| b.id.clone()).collect();
        let favorite_basket_by_id: std::collections::HashMap<String, device::BasketItem> = manifest
            .basket_items
            .iter()
            .filter(|item| matches!(item.item_type.as_str(), "FavoriteArtist" | "FavoriteAlbum"))
            .map(|item| (item.id.clone(), item.clone()))
            .collect();
        let normal_item_ids: Vec<String> = item_ids
            .iter()
            .filter(|id| !favorite_basket_by_id.contains_key(*id))
            .cloned()
            .collect();
        let is_downloadable = |t: &str| matches!(t, "Audio" | "MusicVideo");

        const API_TIMEOUT: tokio::time::Duration = tokio::time::Duration::from_secs(30);

        for favorite_item in favorite_basket_by_id.values() {
            match resolve_jellyfin_favorite_basket_item(
                &jellyfin_client,
                &url,
                &token,
                &user_id,
                favorite_item,
            )
            .await
            {
                Ok(items) => desired_items.extend(items),
                Err(e) => daemon_log!(
                    "[AutoSync] Failed to expand favorite item {}: {}",
                    favorite_item.id,
                    e
                ),
            }
        }

        for chunk in normal_item_ids.chunks(100) {
            let chunk_strs: Vec<&str> = chunk.iter().map(|s| s.as_str()).collect();
            let fetch_result = tokio::time::timeout(
                API_TIMEOUT,
                jellyfin_client.get_items_by_ids(&url, &token, &user_id, &chunk_strs),
            )
            .await
            .map_err(|_| anyhow::anyhow!("Timeout fetching items from Jellyfin"))?;

            match fetch_result {
                Ok(items) => {
                    for item in items {
                        if is_downloadable(&item.item_type) {
                            desired_items.push(to_desired_item(item));
                        } else {
                            // Expand container item (album/playlist)
                            let is_playlist = item.item_type == "Playlist";
                            let playlist_id = item.id.clone();
                            let playlist_name = item.name.clone();
                            let expand_result = tokio::time::timeout(
                                API_TIMEOUT,
                                jellyfin_client
                                    .get_child_items_with_sizes(&url, &token, &user_id, &item.id),
                            )
                            .await;

                            match expand_result {
                                Ok(Ok(children)) => {
                                    if is_playlist {
                                        let tracks: Vec<sync::PlaylistTrackInfo> = children
                                            .iter()
                                            .filter(|c| is_downloadable(&c.item_type))
                                            .map(|c| sync::PlaylistTrackInfo {
                                                jellyfin_id: c.id.clone(),
                                                artist: c.album_artist.clone(),
                                                run_time_seconds: c
                                                    .run_time_ticks
                                                    .map(|t| (t / 10_000_000) as i64)
                                                    .unwrap_or(-1),
                                            })
                                            .collect();
                                        if !tracks.is_empty() {
                                            playlist_sync_items.push(sync::PlaylistSyncItem {
                                                jellyfin_id: playlist_id,
                                                name: playlist_name,
                                                tracks,
                                            });
                                        }
                                    }
                                    for child in children {
                                        if is_downloadable(&child.item_type) {
                                            desired_items.push(to_desired_item(child));
                                        }
                                    }
                                }
                                Ok(Err(e)) => {
                                    daemon_log!(
                                        "[AutoSync] Failed to expand item {}: {}",
                                        item.id,
                                        e
                                    );
                                }
                                Err(_) => {
                                    daemon_log!("[AutoSync] Timeout expanding item {}", item.id);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    daemon_log!("[AutoSync] Failed to fetch items from Jellyfin: {}", e);
                    return Err(e.into());
                }
            }
        }
    } // end else (basket_items non-empty)

    let mut seen_desired_ids = std::collections::HashSet::new();
    desired_items.retain(|item| seen_desired_ids.insert(item.jellyfin_id.clone()));

    if desired_items.is_empty() && !manifest.basket_items.is_empty() {
        daemon_log!("[AutoSync] No downloadable items resolved from basket, skipping");
        return Ok(());
    }

    let mut delta = sync::calculate_delta(&desired_items, &manifest);
    delta.playlists = playlist_sync_items;
    let total_files = delta.adds.len() + delta.deletes.len();

    if total_files == 0 && delta.id_changes.is_empty() {
        daemon_log!("[AutoSync] Device already in sync, nothing to do");
        return Ok(());
    }
    let destructive_cleanup_count = sync::destructive_cleanup_count(&delta, &manifest);
    if destructive_cleanup_count > sync::DESTRUCTIVE_CLEANUP_THRESHOLD {
        daemon_log!(
            "[AutoSync] Skipped: sync would delete {} managed files, exceeding the confirmation threshold of {}",
            destructive_cleanup_count,
            sync::DESTRUCTIVE_CLEANUP_THRESHOLD
        );
        return Ok(());
    }

    daemon_log!(
        "[AutoSync] Delta: {} adds, {} deletes, {} id-changes",
        delta.adds.len(),
        delta.deletes.len(),
        delta.id_changes.len()
    );

    let operation_id = uuid::Uuid::new_v4().to_string();
    sync_op_manager
        .create_operation(operation_id.clone(), total_files)
        .await;

    // Mark manifest dirty before sync
    let pending_ids: Vec<String> = delta
        .adds
        .iter()
        .map(|a| a.jellyfin_id.clone())
        .chain(delta.id_changes.iter().map(|c| c.new_jellyfin_id.clone()))
        .collect();

    device_manager
        .update_manifest(|m| {
            m.dirty = true;
            m.pending_item_ids = pending_ids;
        })
        .await?;

    // Atomically fetch manifest + IO backend after marking dirty to avoid TOCTOU
    // if the device disconnects between the dirty write and acquiring the IO handle.
    let (current_manifest, device_io) = match device_manager.get_manifest_and_io().await {
        Some(pair) => pair,
        None => {
            daemon_log!("[AutoSync] Device disconnected before sync started — aborting");
            let _ = state_tx.send(DaemonState::Error);
            return Ok(());
        }
    };
    // Refresh transcoding profile from the atomically fetched manifest
    let transcoding_profile = if let Some(ref profile_id) = current_manifest.transcoding_profile_id
    {
        match crate::paths::get_device_profiles_path()
            .and_then(|p| crate::transcoding::find_device_profile(&p, profile_id))
        {
            Ok(profile) => profile,
            Err(e) => {
                daemon_log!(
                    "[AutoSync] Failed to load transcoding profile '{}': {}",
                    profile_id,
                    e
                );
                None
            }
        }
    } else {
        None
    };

    let result = sync::execute_sync(
        &delta,
        &device_path,
        &jellyfin_client,
        &url,
        &token,
        &user_id,
        sync_op_manager.clone(),
        operation_id.clone(),
        device_manager.clone(),
        transcoding_profile,
        device_io,
    )
    .await;

    match result {
        Ok((_synced_items, errors)) => {
            // Clear dirty flag
            if let Err(e) = device_manager
                .update_manifest(|m| {
                    m.dirty = false;
                    m.pending_item_ids = vec![];
                })
                .await
            {
                daemon_log!("[AutoSync] Failed to clear dirty flag: {}", e);
            }

            if let Some(mut operation) = sync_op_manager.get_operation(&operation_id).await {
                operation.status = if errors.is_empty() {
                    sync::SyncStatus::Complete
                } else {
                    sync::SyncStatus::Failed
                };
                operation.errors = errors.clone();
                sync_op_manager
                    .update_operation(&operation_id, operation)
                    .await;
            }

            if errors.is_empty() {
                daemon_log!("[AutoSync] Sync completed successfully");
                let _ = tokio::task::spawn_blocking(|| {
                    if let Err(e) = notify_rust::Notification::new()
                        .summary(&hifimule_i18n::t("app.name"))
                        .body(&hifimule_i18n::t("notification.sync_complete_safe"))
                        .show()
                    {
                        daemon_log!("[AutoSync] Notification failed: {}", e);
                    }
                });
                let _ = state_tx.send(DaemonState::Idle);
            } else {
                daemon_log!("[AutoSync] Sync completed with {} errors", errors.len());
                let error_msg = format!("Sync completed with {} error(s)", errors.len());
                let _ = tokio::task::spawn_blocking(move || {
                    if let Err(e) = notify_rust::Notification::new()
                        .summary("HifiMule")
                        .body(&error_msg)
                        .show()
                    {
                        daemon_log!("[AutoSync] Notification failed: {}", e);
                    }
                });
                let _ = state_tx.send(DaemonState::Error);
            }
        }
        Err(e) => {
            daemon_log!("[AutoSync] Sync failed: {}", e);

            if let Some(mut operation) = sync_op_manager.get_operation(&operation_id).await {
                operation.status = sync::SyncStatus::Failed;
                operation.errors.push(sync::SyncFileError {
                    jellyfin_id: String::new(),
                    filename: "auto_sync".to_string(),
                    error_message: e.to_string(),
                });
                sync_op_manager
                    .update_operation(&operation_id, operation)
                    .await;
            }

            let error_msg = format!("Sync failed: {}", e);
            let _ = tokio::task::spawn_blocking(move || {
                if let Err(e) = notify_rust::Notification::new()
                    .summary("HifiMule")
                    .body(&error_msg)
                    .show()
                {
                    daemon_log!("[AutoSync] Notification failed: {}", e);
                }
            });
            let _ = state_tx.send(DaemonState::Error);
        }
    }

    Ok(())
}

fn to_desired_item(item: api::JellyfinItem) -> sync::DesiredItem {
    let size_bytes = item
        .media_sources
        .as_ref()
        .and_then(|sources| sources.first())
        .and_then(|s| s.size)
        .unwrap_or(0)
        .max(0) as u64;
    let provider_suffix = item
        .media_sources
        .as_ref()
        .and_then(|sources| sources.first())
        .and_then(|source| source.container.clone())
        .or_else(|| item.container.clone());
    let original_bitrate = item
        .media_sources
        .as_ref()
        .and_then(|sources| sources.first())
        .and_then(|s| {
            s.bitrate.or_else(|| {
                s.media_streams
                    .as_ref()?
                    .iter()
                    .find(|ms| ms.stream_type == "Audio")
                    .and_then(|ms| ms.bit_rate)
            })
        })
        .or(item.bitrate);
    sync::DesiredItem {
        jellyfin_id: item.id,
        name: item.name,
        album: item.album,
        artist: item.album_artist,
        size_bytes,
        etag: item.etag,
        provider_album_id: item.album_id,
        provider_content_type: None,
        provider_suffix,
        original_bitrate,
        track_number: item.index_number,
    }
}

fn scoped_favorite_target_id<'a>(basket_item: &'a device::BasketItem, prefix: &str) -> &'a str {
    basket_item
        .id
        .strip_prefix(prefix)
        .unwrap_or(&basket_item.id)
}

async fn resolve_jellyfin_favorite_basket_item(
    jellyfin_client: &api::JellyfinClient,
    url: &str,
    token: &str,
    user_id: &str,
    basket_item: &device::BasketItem,
) -> anyhow::Result<Vec<sync::DesiredItem>> {
    let favorites = jellyfin_client
        .get_favorite_music_items(url, token, user_id, None)
        .await?;

    match basket_item.item_type.as_str() {
        "FavoriteAlbum" => {
            let album_id = scoped_favorite_target_id(basket_item, "favorites:album:");
            Ok(favorites
                .items
                .into_iter()
                .filter(|item| {
                    matches!(item.item_type.as_str(), "Audio" | "MusicVideo")
                        && item.album_id.as_deref() == Some(album_id)
                })
                .map(to_desired_item)
                .collect())
        }
        "FavoriteArtist" => {
            let artist_id = scoped_favorite_target_id(basket_item, "favorites:artist:");
            let mut desired_items = Vec::new();
            let mut favorite_album_ids = Vec::new();
            for item in favorites.items {
                match item.item_type.as_str() {
                    "MusicAlbum" => {
                        let item_artist_id = item
                            .artist_items
                            .as_ref()
                            .and_then(|items| items.first())
                            .map(|artist| artist.id.as_str());
                        if item_artist_id == Some(artist_id) {
                            favorite_album_ids.push(item.id);
                        }
                    }
                    "Audio" | "MusicVideo" => {
                        let item_artist_id = item
                            .artist_items
                            .as_ref()
                            .and_then(|items| items.first())
                            .map(|artist| artist.id.as_str());
                        if item_artist_id == Some(artist_id) {
                            desired_items.push(to_desired_item(item));
                        }
                    }
                    _ => {}
                }
            }

            for album_id in favorite_album_ids {
                let children = jellyfin_client
                    .get_child_items_with_sizes(url, token, user_id, &album_id)
                    .await?;
                desired_items.extend(
                    children
                        .into_iter()
                        .filter(|child| matches!(child.item_type.as_str(), "Audio" | "MusicVideo"))
                        .map(to_desired_item),
                );
            }
            Ok(desired_items)
        }
        _ => Ok(Vec::new()),
    }
}

/// Returns an active Subsonic/OpenSubsonic provider if the DB config indicates a non-Jellyfin
/// server. Returns `None` for Jellyfin, unknown types, or on credential/connection failure.
async fn get_non_jellyfin_provider(
    db: &Arc<db::Database>,
) -> Option<Arc<dyn providers::MediaProvider>> {
    let config = db.get_server_config().ok()??;
    if !matches!(config.server_type.as_str(), "subsonic" | "openSubsonic") {
        return None;
    }
    // Try stored password aliases in preference order
    let candidates: Vec<String> = {
        let mut out = Vec::new();
        for alias in [config.server_type.as_str(), "openSubsonic", "subsonic"] {
            if let Ok(secret) = api::CredentialManager::get_server_secret(alias) {
                if !out.contains(&secret) {
                    out.push(secret);
                }
            }
        }
        out
    };
    for password in candidates {
        let credentials = providers::ProviderCredentials {
            server_url: config.url.clone(),
            credential: providers::CredentialKind::Password {
                username: config.username.clone(),
                password,
            },
        };
        if let Ok(provider) =
            providers::connect(&config.url, &credentials, providers::ServerTypeHint::Subsonic).await
        {
            return Some(provider);
        }
    }
    daemon_log!("[AutoSync] Could not connect to Subsonic provider — skipping provider path");
    None
}

/// Provider-based auto-sync: resolves basket items via MediaProvider, runs auto-fill if needed,
/// then executes sync via execute_provider_sync. Used for Subsonic/Navidrome devices.
async fn run_auto_sync_via_provider(
    provider: Arc<dyn providers::MediaProvider>,
    device_manager: Arc<device::DeviceManager>,
    sync_op_manager: Arc<sync::SyncOperationManager>,
    state_tx: std::sync::mpsc::Sender<DaemonState>,
    device_path: std::path::PathBuf,
) -> anyhow::Result<()> {
    let _ = state_tx.send(DaemonState::Syncing);

    let manifest = device_manager
        .get_current_device()
        .await
        .ok_or_else(|| anyhow::anyhow!("No device connected"))?;

    let mut desired_items: Vec<sync::DesiredItem> = Vec::new();
    let mut playlist_sync_items: Vec<sync::PlaylistSyncItem> = Vec::new();

    if manifest.basket_items.is_empty() && !manifest.auto_fill.enabled {
        if manifest.synced_items.is_empty() {
            daemon_log!("[AutoSync] No basket items and no synced items, skipping");
            let _ = state_tx.send(DaemonState::Idle);
            return Ok(());
        } else {
            daemon_log!(
                "[AutoSync] Basket empty but device has {} synced item(s) — running cleanup sync",
                manifest.synced_items.len()
            );
        }
    }

    // Resolve basket items (always, even when auto-fill is also enabled).
    if !manifest.basket_items.is_empty() {
        let (items, playlists) =
            resolve_provider_basket_items(provider.clone(), &manifest.basket_items).await;
        desired_items = items;
        playlist_sync_items = playlists;
    }

    // Auto-fill: fill remaining space after basket items (or fill entirely when basket is empty).
    if manifest.auto_fill.enabled {
        let synced_bytes: u64 = manifest.synced_items.iter().map(|s| s.size_bytes).sum();
        let total_budget = if let Some(mb) = manifest.auto_fill.max_bytes {
            mb
        } else {
            match device_manager.get_device_storage().await {
                Some(info) => info.free_bytes.saturating_add(synced_bytes),
                None => {
                    daemon_log!("[AutoSync] Cannot determine device capacity for auto-fill");
                    let _ = state_tx.send(DaemonState::Idle);
                    return Ok(());
                }
            }
        };
        let basket_size: u64 = desired_items.iter().map(|i| i.size_bytes).sum();
        let auto_fill_budget = total_budget.saturating_sub(basket_size);
        if auto_fill_budget > 0 {
            let exclude_item_ids: Vec<String> =
                desired_items.iter().map(|i| i.jellyfin_id.clone()).collect();
            let fill_params = auto_fill::AutoFillParams { exclude_item_ids, max_fill_bytes: auto_fill_budget };
            match auto_fill::run_auto_fill_provider(provider.clone(), fill_params).await {
                Ok(items) if items.is_empty() && desired_items.is_empty() => {
                    daemon_log!("[AutoSync] Provider auto-fill returned no items, skipping");
                    let _ = state_tx.send(DaemonState::Idle);
                    return Ok(());
                }
                Ok(items) => {
                    daemon_log!("[AutoSync] Provider auto-fill resolved {} items", items.len());
                    for item in items {
                        desired_items.push(sync::DesiredItem {
                            jellyfin_id: item.id,
                            name: item.name,
                            album: item.album,
                            artist: item.artist,
                            size_bytes: item.size_bytes,
                            etag: None,
                            provider_album_id: item.provider_album_id,
                            provider_content_type: None,
                            provider_suffix: item.provider_suffix,
                            original_bitrate: None,
                            track_number: None,
                        });
                    }
                }
                Err(e) => {
                    daemon_log!("[AutoSync] Provider auto-fill failed: {}", e);
                    let _ = state_tx.send(DaemonState::Error);
                    return Ok(());
                }
            }
        }
    }

    let mut seen_ids = std::collections::HashSet::new();
    desired_items.retain(|item| seen_ids.insert(item.jellyfin_id.clone()));

    if desired_items.is_empty() && !manifest.basket_items.is_empty() {
        daemon_log!("[AutoSync] No downloadable items resolved from basket, skipping");
        return Ok(());
    }

    let mut delta = sync::calculate_delta(&desired_items, &manifest);
    delta.playlists = playlist_sync_items;
    let total_files = delta.adds.len() + delta.deletes.len();

    if total_files == 0 && delta.id_changes.is_empty() {
        daemon_log!("[AutoSync] Device already in sync, nothing to do");
        let _ = state_tx.send(DaemonState::Idle);
        return Ok(());
    }
    let destructive_cleanup_count = sync::destructive_cleanup_count(&delta, &manifest);
    if destructive_cleanup_count > sync::DESTRUCTIVE_CLEANUP_THRESHOLD {
        daemon_log!(
            "[AutoSync] Skipped: sync would delete {} managed files, exceeding threshold of {}",
            destructive_cleanup_count,
            sync::DESTRUCTIVE_CLEANUP_THRESHOLD
        );
        let _ = state_tx.send(DaemonState::Idle);
        return Ok(());
    }

    daemon_log!(
        "[AutoSync] Delta: {} adds, {} deletes, {} id-changes",
        delta.adds.len(),
        delta.deletes.len(),
        delta.id_changes.len()
    );

    let operation_id = uuid::Uuid::new_v4().to_string();
    sync_op_manager
        .create_operation(operation_id.clone(), total_files)
        .await;

    let pending_ids: Vec<String> = delta
        .adds
        .iter()
        .map(|a| a.jellyfin_id.clone())
        .chain(delta.id_changes.iter().map(|c| c.new_jellyfin_id.clone()))
        .collect();

    device_manager
        .update_manifest(|m| {
            m.dirty = true;
            m.pending_item_ids = pending_ids;
        })
        .await?;

    let (current_manifest, device_io) = match device_manager.get_manifest_and_io().await {
        Some(pair) => pair,
        None => {
            daemon_log!("[AutoSync] Device disconnected before sync started — aborting");
            let _ = state_tx.send(DaemonState::Error);
            return Ok(());
        }
    };

    let transcoding_profile = if let Some(ref profile_id) = current_manifest.transcoding_profile_id
    {
        match crate::paths::get_device_profiles_path()
            .and_then(|p| crate::transcoding::find_device_profile(&p, profile_id))
        {
            Ok(profile) => profile,
            Err(e) => {
                daemon_log!(
                    "[AutoSync] Failed to load transcoding profile '{}': {}",
                    profile_id,
                    e
                );
                None
            }
        }
    } else {
        None
    };

    let result = sync::execute_provider_sync(
        &delta,
        &device_path,
        sync::ProviderSyncSource { provider, transcoding_profile },
        sync_op_manager.clone(),
        operation_id.clone(),
        device_manager.clone(),
        device_io,
    )
    .await;

    match result {
        Ok((_synced_items, errors)) => {
            if let Err(e) = device_manager
                .update_manifest(|m| {
                    m.dirty = false;
                    m.pending_item_ids = vec![];
                })
                .await
            {
                daemon_log!("[AutoSync] Failed to clear dirty flag: {}", e);
            }

            if let Some(mut operation) = sync_op_manager.get_operation(&operation_id).await {
                operation.status = if errors.is_empty() {
                    sync::SyncStatus::Complete
                } else {
                    sync::SyncStatus::Failed
                };
                operation.errors = errors.clone();
                sync_op_manager.update_operation(&operation_id, operation).await;
            }

            if errors.is_empty() {
                daemon_log!("[AutoSync] Sync completed successfully");
                let _ = tokio::task::spawn_blocking(|| {
                    if let Err(e) = notify_rust::Notification::new()
                        .summary(&hifimule_i18n::t("app.name"))
                        .body(&hifimule_i18n::t("notification.sync_complete_safe"))
                        .show()
                    {
                        daemon_log!("[AutoSync] Notification failed: {}", e);
                    }
                });
                let _ = state_tx.send(DaemonState::Idle);
            } else {
                daemon_log!("[AutoSync] Sync completed with {} errors", errors.len());
                let error_msg = format!("Sync completed with {} error(s)", errors.len());
                let _ = tokio::task::spawn_blocking(move || {
                    if let Err(e) = notify_rust::Notification::new()
                        .summary("HifiMule")
                        .body(&error_msg)
                        .show()
                    {
                        daemon_log!("[AutoSync] Notification failed: {}", e);
                    }
                });
                let _ = state_tx.send(DaemonState::Error);
            }
        }
        Err(e) => {
            daemon_log!("[AutoSync] Sync failed: {}", e);
            if let Some(mut operation) = sync_op_manager.get_operation(&operation_id).await {
                operation.status = sync::SyncStatus::Failed;
                operation.errors.push(sync::SyncFileError {
                    jellyfin_id: String::new(),
                    filename: "auto_sync_provider".to_string(),
                    error_message: e.to_string(),
                });
                sync_op_manager.update_operation(&operation_id, operation).await;
            }
            let error_msg = format!("Sync failed: {}", e);
            let _ = tokio::task::spawn_blocking(move || {
                if let Err(e) = notify_rust::Notification::new()
                    .summary("HifiMule")
                    .body(&error_msg)
                    .show()
                {
                    daemon_log!("[AutoSync] Notification failed: {}", e);
                }
            });
            let _ = state_tx.send(DaemonState::Error);
        }
    }

    Ok(())
}

/// Resolves a list of BasketItems to DesiredItems + PlaylistSyncItems using the MediaProvider.
async fn resolve_provider_basket_items(
    provider: Arc<dyn providers::MediaProvider>,
    basket_items: &[device::BasketItem],
) -> (Vec<sync::DesiredItem>, Vec<sync::PlaylistSyncItem>) {
    let mut desired_items: Vec<sync::DesiredItem> = Vec::new();
    let mut playlist_sync_items: Vec<sync::PlaylistSyncItem> = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();

    let favorite_items: Vec<&device::BasketItem> = basket_items
        .iter()
        .filter(|b| matches!(b.item_type.as_str(), "FavoriteArtist" | "FavoriteAlbum"))
        .collect();
    let normal_items: Vec<&device::BasketItem> = basket_items
        .iter()
        .filter(|b| !matches!(b.item_type.as_str(), "FavoriteArtist" | "FavoriteAlbum"))
        .collect();

    for basket_item in favorite_items {
        match resolve_provider_favorite_item(provider.clone(), basket_item).await {
            Ok(items) => {
                for item in items {
                    if seen_ids.insert(item.jellyfin_id.clone()) {
                        desired_items.push(item);
                    }
                }
            }
            Err(e) => {
                daemon_log!("[AutoSync] Failed to resolve favorite item {}: {}", basket_item.id, e);
            }
        }
    }

    for basket_item in normal_items {
        match resolve_provider_item(provider.clone(), &basket_item.id).await {
            Ok((tracks, playlist)) => {
                if let Some(p) = playlist {
                    playlist_sync_items.push(p);
                }
                for item in tracks {
                    if seen_ids.insert(item.jellyfin_id.clone()) {
                        desired_items.push(item);
                    }
                }
            }
            Err(e) => {
                daemon_log!("[AutoSync] Failed to resolve item {}: {}", basket_item.id, e);
            }
        }
    }

    (desired_items, playlist_sync_items)
}

async fn resolve_provider_favorite_item(
    provider: Arc<dyn providers::MediaProvider>,
    basket_item: &device::BasketItem,
) -> anyhow::Result<Vec<sync::DesiredItem>> {
    let favorites = provider
        .list_favorite_items(None)
        .await
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    match basket_item.item_type.as_str() {
        "FavoriteAlbum" => {
            let album_id = scoped_favorite_target_id(basket_item, "favorites:album:");
            Ok(favorites
                .songs
                .iter()
                .filter(|song| song.album_id.as_deref() == Some(album_id))
                .map(provider_song_to_desired)
                .collect())
        }
        "FavoriteArtist" => {
            let artist_id = scoped_favorite_target_id(basket_item, "favorites:artist:");
            let mut desired_items = Vec::new();
            for album in favorites
                .albums
                .iter()
                .filter(|album| album.artist_id.as_deref() == Some(artist_id))
            {
                match provider.get_album(&album.id).await {
                    Ok(album_with_tracks) => {
                        desired_items
                            .extend(album_with_tracks.tracks.iter().map(provider_song_to_desired));
                    }
                    Err(e) => {
                        daemon_log!("[AutoSync] Failed to expand favorite album {}: {}", album.id, e);
                    }
                }
            }
            desired_items.extend(
                favorites
                    .songs
                    .iter()
                    .filter(|song| song.artist_id.as_deref() == Some(artist_id))
                    .map(provider_song_to_desired),
            );
            Ok(desired_items)
        }
        _ => Ok(Vec::new()),
    }
}

async fn resolve_provider_item(
    provider: Arc<dyn providers::MediaProvider>,
    item_id: &str,
) -> anyhow::Result<(Vec<sync::DesiredItem>, Option<sync::PlaylistSyncItem>)> {
    if let Ok(album) = provider.get_album(item_id).await {
        return Ok((album.tracks.iter().map(provider_song_to_desired).collect(), None));
    }

    if let Ok(playlist) = provider.get_playlist(item_id).await {
        let tracks = playlist.tracks.iter().map(provider_song_to_desired).collect::<Vec<_>>();
        let playlist_item = sync::PlaylistSyncItem {
            jellyfin_id: playlist.playlist.id.clone(),
            name: playlist.playlist.name.clone(),
            tracks: playlist
                .tracks
                .iter()
                .map(|t| sync::PlaylistTrackInfo {
                    jellyfin_id: t.id.clone(),
                    artist: t.artist_name.clone(),
                    run_time_seconds: i64::from(t.duration_seconds),
                })
                .collect(),
        };
        return Ok((tracks, Some(playlist_item)));
    }

    if let Ok(artist) = provider.get_artist(item_id).await {
        let mut tracks = Vec::new();
        for album in artist.albums {
            match provider.get_album(&album.id).await {
                Ok(album_with_tracks) => {
                    tracks.extend(album_with_tracks.tracks.iter().map(provider_song_to_desired));
                }
                Err(e) => {
                    daemon_log!("[AutoSync] Failed to expand artist album {}: {}", album.id, e);
                }
            }
        }
        return Ok((tracks, None));
    }

    match provider.get_song(item_id).await {
        Ok(song) => return Ok((vec![provider_song_to_desired(&song)], None)),
        Err(providers::ProviderError::UnsupportedCapability(_))
        | Err(providers::ProviderError::NotFound { .. }) => {}
        Err(e) => return Err(anyhow::anyhow!("{}", e)),
    }

    Err(anyhow::anyhow!("Item {} not found via provider", item_id))
}

fn provider_song_to_desired(song: &crate::domain::models::Song) -> sync::DesiredItem {
    let size_bytes = song
        .bitrate_kbps
        .map(|kbps| (u64::from(kbps) * 1_000 / 8) * u64::from(song.duration_seconds))
        .unwrap_or(0);
    sync::DesiredItem {
        jellyfin_id: song.id.clone(),
        name: song.title.clone(),
        album: song.album_title.clone(),
        artist: song.artist_name.clone(),
        size_bytes,
        etag: None,
        provider_album_id: song.album_id.clone(),
        provider_content_type: song.content_type.clone(),
        provider_suffix: song.suffix.clone(),
        original_bitrate: song.bitrate_kbps.map(|kbps| kbps * 1000),
        track_number: song.track_number,
    }
}

fn load_icon(bytes: &[u8], name: &str) -> anyhow::Result<Icon> {
    // Resize to 32x32 with Lanczos3 before handing off to the OS.
    // Windows tray slots are 16–32 px; letting the OS scale from 1024 px produces blurry results.
    let image = image::load_from_memory(bytes)
        .map_err(|e| anyhow::anyhow!("Failed to load {} icon: {}", name, e))?
        .resize_exact(32, 32, image::imageops::FilterType::Lanczos3)
        .to_rgba8();
    let (width, height) = image.dimensions();
    Icon::from_rgba(image.into_raw(), width, height)
        .map_err(|e| anyhow::anyhow!("Failed to create {} tray icon: {}", name, e))
}
