use tauri_plugin_shell::ShellExt;

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

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![greet])
        .setup(|app| {
            // Launch the daemon sidecar on startup
            let sidecar = app.shell().sidecar("jellyfinsync-daemon")
                .expect("failed to create sidecar command");
            let (mut _rx, _child) = sidecar.spawn()
                .expect("failed to spawn jellyfinsync-daemon sidecar");
            println!("jellyfinsync-daemon sidecar launched");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

// final check
