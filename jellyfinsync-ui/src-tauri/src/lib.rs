use std::sync::Mutex;
use tauri::{Manager, RunEvent};
use tauri_plugin_shell::process::CommandChild;
use tauri_plugin_shell::ShellExt;

struct DaemonProcess(Mutex<Option<CommandChild>>);

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(debug_assertions)]
    println!("JellyfinSync UI starting in DEBUG mode");
    #[cfg(not(debug_assertions))]
    println!("JellyfinSync UI starting in RELEASE mode");

    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![greet])
        .setup(|app| {
            app.manage(DaemonProcess(Mutex::new(None)));

            // Launch the daemon sidecar on startup gracefully without panicking
            match app.shell().sidecar("jellyfinsync-daemon") {
                Ok(sidecar) => {
                    match sidecar.spawn() {
                        Ok((_rx, child)) => {
                            println!("jellyfinsync-daemon sidecar launched successfully");
                            if let Some(state) = app.try_state::<DaemonProcess>() {
                                if let Ok(mut daemon_proc) = state.0.lock() {
                                    *daemon_proc = Some(child);
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("Failed to spawn jellyfinsync-daemon sidecar: {}", e);
                            // UI will continue to load, it can show connection errors appropriately.
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Failed to create sidecar command: {}", e);
                }
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    builder.run(|app_handle, event| {
        if let RunEvent::Exit = event {
            // Explicitly kill the daemon sidecar process to prevent zombie processes
            if let Some(state) = app_handle.try_state::<DaemonProcess>() {
                if let Ok(mut daemon_proc) = state.0.lock() {
                    if let Some(child) = daemon_proc.take() {
                        println!("Killing jellyfinsync-daemon sidecar before exit");
                        let _ = child.kill();
                    }
                }
            }
        }
    });
}

// final check
