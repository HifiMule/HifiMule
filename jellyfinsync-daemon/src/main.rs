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

/// Simple file-based logger for release mode where stdout/stderr are unavailable.
/// Writes to `%APPDATA%/JellyfinSync/daemon.log`.
pub fn log_to_file(msg: &str) {
    if let Ok(dir) = paths::get_app_data_dir() {
        let log_path = dir.join("daemon.log");
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

            // Start RPC server
            daemon_log!("Starting RPC server on port 19140");
            let db_clone = Arc::clone(&db);
            let dm_clone = Arc::clone(&device_manager);
            let scrobbler_result_rpc = Arc::clone(&last_scrobbler_result);
            let state_tx_rpc = state_tx.clone();
            tokio::spawn(async move {
                rpc::run_server(19140, db_clone, dm_clone, scrobbler_result_rpc, state_tx_rpc).await;
            });

            // Handle Device Events
            let state_tx_clone = state_tx.clone();
            let jellyfin_client = Arc::new(api::JellyfinClient::new());
            tokio::spawn(async move {
                while let Some(event) = device_rx.recv().await {
                    match event {
                        device::DeviceEvent::Detected { path, manifest } => {
                            println!("Device detected at {:?}: {:?}", path, manifest);
                            match device_manager.handle_device_detected(path.clone(), manifest).await {
                                Ok(new_state) => {
                                    let _ = state_tx_clone.send(new_state);
                                }
                                Err(e) => {
                                    eprintln!("Error handling device detection: {}", e);
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
                                    println!("[Scrobbler] Result: {:?}", result);
                                    let mut guard = scrobbler_result_clone.write().await;
                                    *guard = Some(result);
                                });
                            }
                        }
                        device::DeviceEvent::Unrecognized { path } => {
                            println!("Unrecognized device at {:?}", path);
                            let new_state = device_manager.handle_device_unrecognized(path).await;
                            let _ = state_tx_clone.send(new_state);
                        }
                        device::DeviceEvent::Removed(path) => {
                            println!("Device removed at {:?}", path);
                            device_manager.handle_device_removed().await;
                            let _ = state_tx_clone.send(DaemonState::Idle);
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
