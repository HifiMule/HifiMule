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

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
enum DaemonState {
    Idle,
    Syncing,
    Error,
}

fn main() -> Result<()> {
    // 1. Setup communication channels
    // State updates from tokio thread to main thread
    let (state_tx, state_rx) = mpsc::channel::<DaemonState>();
    // Shutdown signal from main thread to tokio thread
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = Arc::clone(&shutdown);

    // 2. Start Tokio runtime in a background thread
    // REQUIRED for macOS: main thread MUST handle the event loop
    // Note: This thread will be terminated when the process exits
    let _daemon_thread = thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed to build tokio runtime");

        rt.block_on(async {
            println!("JellyfinSync Daemon started");

            // Initial state
            if state_tx.send(DaemonState::Idle).is_err() {
                eprintln!("Failed to send initial state");
                return;
            }

            // Daemon work loop - check for shutdown signal
            while !shutdown_clone.load(Ordering::Relaxed) {
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                // TODO: Actual daemon work will go here in future stories
            }

            println!("JellyfinSync Daemon shutting down gracefully");
        });
    });

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
            let (tooltip, icon) = match state {
                DaemonState::Idle => ("JellyfinSync: Idle", &icon_idle),
                DaemonState::Syncing => ("JellyfinSync: Syncing...", &icon_syncing),
                DaemonState::Error => ("JellyfinSync: Error!", &icon_error),
            };

            if let Some(ref mut tray) = tray_icon {
                if let Err(e) = tray.set_tooltip(Some(tooltip)) {
                    eprintln!("Failed to set tooltip: {}", e);
                }
                if let Err(e) = tray.set_icon(Some((**icon).clone())) {
                    eprintln!("Failed to set icon: {}", e);
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
                        .map(|p| p.join("jellysync-ui"))
                        .unwrap_or_else(|| std::path::PathBuf::from("../jellysync-ui"));

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
                        "jellysync-ui.exe"
                    } else {
                        "jellysync-ui"
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
