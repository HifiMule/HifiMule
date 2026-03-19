#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::Result;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::thread;
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem},
    Icon, TrayIconBuilder,
};

#[cfg(windows)]
mod service;

const LOG_MAX_BYTES: u64 = 1_048_576; // 1 MB

/// Simple file-based logger for release mode where stdout/stderr are unavailable.
/// Writes to `%APPDATA%/JellyfinSync/daemon.log`. Truncates at 1 MB.
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
mod paths;
mod rpc;
mod scrobbler;
mod sync;

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
            .enable_all()
            .build()
            .expect("Failed to build tokio runtime");

        rt.block_on(async {
            daemon_log!("JellyfinSync Daemon tokio runtime started");

            // Initialize database
            let db_path = match paths::get_app_data_dir() {
                Ok(p) => p.join("jellyfinsync.db"),
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

            // Initial state
            if let Err(e) = state_tx.send(DaemonState::Idle) {
                daemon_log!("Failed to send initial state: {}", e);
                return;
            }

            // Start Device Observer
            let (device_tx, mut device_rx) = tokio::sync::mpsc::channel(10);
            tokio::spawn(async move {
                device::run_observer(device_tx).await;
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
                        device::DeviceEvent::Detected { path, manifest } => {
                            daemon_log!("Device detected at {:?}: {:?}", path, manifest);
                            let auto_sync_enabled = manifest.auto_sync_on_connect;
                            let has_basket = !manifest.basket_items.is_empty();
                            let auto_fill_enabled = manifest.auto_fill.enabled;
                            match device_manager.handle_device_detected(path.clone(), manifest).await {
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
                                let db_scrobble = Arc::clone(&db);
                                let client_scrobble = Arc::clone(&jellyfin_client);
                                let device_path_clone = path.clone();
                                let scrobbler_result_clone = Arc::clone(&last_scrobbler_result);
                                tokio::spawn(async move {
                                    let result = scrobbler::process_device_scrobbles(
                                        &device_path_clone,
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

                            // Auto-sync trigger: check if device has auto_sync_on_connect enabled
                            if auto_sync_enabled && (has_basket || auto_fill_enabled) {
                                // Check DB mapping also has auto_sync enabled
                                let device = device_manager.get_current_device().await;
                                let device_id = device.as_ref().map(|d| d.device_id.clone());
                                let db_enabled = device_id
                                    .as_ref()
                                    .and_then(|id| db.get_device_mapping(id).ok().flatten())
                                    .map(|m| m.auto_sync_on_connect)
                                    .unwrap_or(false);

                                if db_enabled {
                                    let has_active_sync = som_events.has_active_operation().await;

                                    if !has_active_sync {
                                        if let Ok((url, token, user_id)) =
                                            api::CredentialManager::get_credentials()
                                        {
                                            let user_id = user_id.unwrap_or_else(|| "Me".to_string());
                                            let client = Arc::clone(&jellyfin_client);
                                            let dm = Arc::clone(&device_manager);
                                            let som = Arc::clone(&som_events);
                                            let state_tx_sync = state_tx_clone.clone();
                                            let device_path = path.clone();

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
                                }
                            } else if auto_sync_enabled && !has_basket && !auto_fill_enabled {
                                daemon_log!("[AutoSync] Skipped: auto-sync enabled but no basket items configured");
                            }
                        }
                        device::DeviceEvent::Unrecognized { path } => {
                            println!("Unrecognized device at {:?}", path);
                            let new_state = device_manager.handle_device_unrecognized(path).await;
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
                                            error_message: "Device removed during sync".to_string(),
                                        });
                                        som_events.update_operation(&op.id.clone(), op).await;
                                    }
                                }
                                let _ = tokio::task::spawn_blocking(|| {
                                    if let Err(e) = notify_rust::Notification::new()
                                        .summary("JellyfinSync")
                                        .body("Sync interrupted: device was removed.")
                                        .show()
                                    {
                                        daemon_log!("[AutoSync] Notification failed: {}", e);
                                    }
                                });
                                let _ = state_tx_clone.send(DaemonState::Error);
                            } else {
                                let _ = state_tx_clone.send(DaemonState::Idle);
                            }
                            device_manager.handle_device_removed().await;
                        }
                    }
                }
            });

            // Daemon work loop - check for shutdown signal
            while !shutdown_clone.load(Ordering::Relaxed) {
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }

            daemon_log!("JellyfinSync Daemon shutting down gracefully");
        });
    });

    Ok((shutdown, state_rx))
}

/// Interactive mode: tray icon + event loop on the main thread
fn run_interactive() -> Result<()> {
    let (shutdown, state_rx) = start_daemon_core()?;

    // 3. Setup Tray Icon and Event Loop on the main thread
    let event_loop = EventLoopBuilder::new().build();

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
    let quit_item = MenuItem::new("Quit", true, None);
    let open_ui_item = MenuItem::new("Open UI", true, None);
    tray_menu
        .append_items(&[&open_ui_item, &quit_item])
        .map_err(|e| anyhow::anyhow!("Failed to create tray menu: {}", e))?;

    let mut tray_icon = Some(
        TrayIconBuilder::new()
            .with_menu(Box::new(tray_menu))
            .with_tooltip("JellyfinSync: Idle")
            .with_icon((*icon_idle).clone())
            .build()?,
    );

    let menu_channel = MenuEvent::receiver();

    // 4. Run the event loop
    // This will block the main thread
    event_loop.run(move |_event, _, control_flow| {
        *control_flow = ControlFlow::Poll;

        // Handle state updates from tokio thread
        if let Ok(state) = state_rx.try_recv() {
            if let Some(ref mut tray) = tray_icon {
                match state {
                    DaemonState::Idle => {
                        let _ = tray.set_tooltip(Some("JellyfinSync: Idle"));
                        let _ = tray.set_icon(Some((*icon_idle).clone()));
                    }
                    DaemonState::Syncing => {
                        let _ = tray.set_tooltip(Some("JellyfinSync: Syncing..."));
                        let _ = tray.set_icon(Some((*icon_syncing).clone()));
                    }
                    DaemonState::Scanning => {
                        let _ = tray.set_tooltip(Some("JellyfinSync: Scanning..."));
                        let _ = tray.set_icon(Some((*icon_syncing).clone()));
                    }
                    DaemonState::DeviceFound(name) => {
                        let _ = tray.set_tooltip(Some(&format!("JellyfinSync: Found {}", name)));
                        let _ = tray.set_icon(Some((*icon_syncing).clone()));
                    }
                    DaemonState::DeviceRecognized { name, profile_id } => {
                        let _ = tray.set_tooltip(Some(&format!(
                            "JellyfinSync: Recognized {} (Profile: {})",
                            name, profile_id
                        )));
                        let _ = tray.set_icon(Some((*icon_syncing).clone()));
                    }
                    DaemonState::Error => {
                        let _ = tray.set_tooltip(Some("JellyfinSync: Error!"));
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
                    // Use CARGO_MANIFEST_DIR to find the workspace root reliably in development
                    let manifest_dir =
                        std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
                    let ui_dir = std::path::Path::new(&manifest_dir)
                        .parent()
                        .map(|p| p.join("jellyfinsync-ui"))
                        .unwrap_or_else(|| std::path::PathBuf::from("../jellyfinsync-ui"));

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
                        "jellyfinsync-ui.exe"
                    } else {
                        "jellyfinsync-ui"
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
                        });
                    }
                }
                Err(e) => {
                    daemon_log!("[AutoSync] Auto-fill failed: {}", e);
                    let _ = state_tx.send(DaemonState::Error);
                    return Ok(());
                }
            }
        } else {
            daemon_log!("[AutoSync] No basket items configured, skipping");
            let _ = state_tx.send(DaemonState::Idle);
            return Ok(());
        }
    } else {
    // Manual basket: resolve basket items to desired items via Jellyfin API
    let item_ids: Vec<String> = manifest.basket_items.iter().map(|b| b.id.clone()).collect();
    let is_downloadable = |t: &str| matches!(t, "Audio" | "MusicVideo");

    const API_TIMEOUT: tokio::time::Duration = tokio::time::Duration::from_secs(30);

    for chunk in item_ids.chunks(100) {
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
                        let expand_result = tokio::time::timeout(
                            API_TIMEOUT,
                            jellyfin_client.get_child_items_with_sizes(&url, &token, &user_id, &item.id),
                        )
                        .await;

                        match expand_result {
                            Ok(Ok(children)) => {
                                for child in children {
                                    if is_downloadable(&child.item_type) {
                                        desired_items.push(to_desired_item(child));
                                    }
                                }
                            }
                            Ok(Err(e)) => {
                                daemon_log!("[AutoSync] Failed to expand item {}: {}", item.id, e);
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

    if desired_items.is_empty() {
        daemon_log!("[AutoSync] No downloadable items resolved, skipping");
        return Ok(());
    }

    let delta = sync::calculate_delta(&desired_items, &manifest);
    let total_files = delta.adds.len() + delta.deletes.len();

    if total_files == 0 && delta.id_changes.is_empty() {
        daemon_log!("[AutoSync] Device already in sync, nothing to do");
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
                        .summary("JellyfinSync")
                        .body("Sync Complete. Safe to eject.")
                        .show()
                    {
                        daemon_log!("[AutoSync] Notification failed: {}", e);
                    }
                });
                let _ = state_tx.send(DaemonState::Idle);
            } else {
                daemon_log!("[AutoSync] Sync completed with {} errors", errors.len());
                let error_msg = format!(
                    "Sync completed with {} error(s)",
                    errors.len()
                );
                let _ = tokio::task::spawn_blocking(move || {
                    if let Err(e) = notify_rust::Notification::new()
                        .summary("JellyfinSync")
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
                    .summary("JellyfinSync")
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
    sync::DesiredItem {
        jellyfin_id: item.id,
        name: item.name,
        album: item.album,
        artist: item.album_artist,
        size_bytes,
        etag: item.etag,
    }
}

// Helper to load icon with proper error handling
// Extracted from main for testability
fn load_icon(bytes: &[u8], name: &str) -> anyhow::Result<Icon> {
    let image = image::load_from_memory(bytes)
        .map_err(|e| anyhow::anyhow!("Failed to load {} icon: {}", name, e))?
        .to_rgba8();
    let (width, height) = image.dimensions();
    Icon::from_rgba(image.into_raw(), width, height)
        .map_err(|e| anyhow::anyhow!("Failed to create {} tray icon: {}", name, e))
}
