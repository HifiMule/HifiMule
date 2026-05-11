use std::sync::Mutex;
use tauri::{Manager, RunEvent};
use tauri_plugin_shell::process::CommandChild;
use tauri_plugin_shell::ShellExt;

struct DaemonProcess(Mutex<Option<CommandChild>>);

/// Stores the sidecar launch status so the frontend can query it.
/// Values: "starting", "startup" (connected to running daemon via health check),
/// "service" (started via sc start), "running (pid=N)",
/// "spawn_failed: ...", "command_failed: ...", "terminated (code=N)"
struct SidecarStatus(Mutex<String>);

const RPC_PORT: u16 = 19140;

#[tauri::command]
fn get_sidecar_status(state: tauri::State<'_, SidecarStatus>) -> String {
    state.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

#[cfg(target_os = "macos")]
fn resolve_daemon_binary_path() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        if entry.file_name().to_string_lossy().starts_with("hifimule-daemon") {
            return Some(entry.path());
        }
    }
    None
}

/// Check if the daemon is already running by sending a health-check RPC call.
fn check_daemon_health() -> bool {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build();
    let client = match client {
        Ok(c) => c,
        Err(_) => return false,
    };
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "get_daemon_state",
        "params": {},
        "id": 1
    });
    match client
        .post(format!("http://127.0.0.1:{}", RPC_PORT))
        .json(&body)
        .send()
    {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    }
}

#[cfg(target_os = "macos")]
const LAUNCHD_PLIST_TEMPLATE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.hifimule.daemon</string>
    <key>ProgramArguments</key>
    <array>
        <string>{DAEMON_PATH}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <false/>
    <key>StandardOutPath</key>
    <string>/tmp/hifimule-daemon-stdout.log</string>
    <key>StandardErrorPath</key>
    <string>/tmp/hifimule-daemon-stderr.log</string>
</dict>
</plist>"#;

#[cfg(target_os = "macos")]
fn launchd_plist_path() -> Option<std::path::PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(std::path::Path::new(&home).join("Library/LaunchAgents/com.hifimule.daemon.plist"))
}

#[cfg(target_os = "macos")]
fn install_launchd_plist() -> Result<(), String> {
    let daemon_path = resolve_daemon_binary_path()
        .ok_or_else(|| "Cannot resolve daemon binary path for plist".to_string())?;
    let daemon_path_str = daemon_path
        .to_str()
        .ok_or_else(|| "Daemon path is not valid UTF-8".to_string())?;
    let plist_content = LAUNCHD_PLIST_TEMPLATE.replace("{DAEMON_PATH}", daemon_path_str);
    let plist_path = launchd_plist_path()
        .ok_or_else(|| "Cannot resolve LaunchAgents path (HOME not set?)".to_string())?;
    let launch_agents = plist_path
        .parent()
        .ok_or_else(|| "Cannot get LaunchAgents parent dir".to_string())?;
    std::fs::create_dir_all(launch_agents)
        .map_err(|e| format!("Cannot create LaunchAgents dir: {}", e))?;
    std::fs::write(&plist_path, plist_content)
        .map_err(|e| format!("Cannot write plist: {}", e))?;
    let output = std::process::Command::new("launchctl")
        .args(["load", plist_path.to_str().unwrap_or("")])
        .output()
        .map_err(|e| format!("launchctl load failed to execute: {}", e))?;
    if !output.status.success() {
        return Err(format!(
            "launchctl load exited {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn unload_and_remove_launchd_plist() -> Result<(), String> {
    let plist_path = launchd_plist_path()
        .ok_or_else(|| "Cannot resolve LaunchAgents path".to_string())?;
    if plist_path.exists() {
        let output = std::process::Command::new("launchctl")
            .args(["unload", plist_path.to_str().unwrap_or("")])
            .output()
            .map_err(|e| format!("launchctl unload failed to execute: {}", e))?;
        if !output.status.success() {
            ui_log(&format!(
                "launchctl unload warning (may already be unloaded): {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        std::fs::remove_file(&plist_path)
            .map_err(|e| format!("Cannot remove plist: {}", e))?;
    }
    Ok(())
}

/// Attempt to start the daemon Windows Service via `sc start`.
#[cfg(windows)]
fn try_start_service() -> bool {
    use std::process::Command;
    let result = Command::new("sc")
        .args(["start", "hifimule-daemon"])
        .output();
    match result {
        Ok(output) => {
            if !output.status.success() {
                ui_log(&format!(
                    "sc start failed (exit={}): {}",
                    output.status,
                    String::from_utf8_lossy(&output.stderr)
                ));
            }
            output.status.success()
        }
        Err(e) => {
            ui_log(&format!("sc start command failed: {}", e));
            false
        }
    }
}

/// Proxies a Jellyfin image from the daemon, returning it as a base64 data URL.
/// Images loaded via CSS `background-image: url(...)` can't use invoke, so the frontend
/// must call this and set the result as inline style.
#[tauri::command]
async fn image_proxy(
    id: String,
    max_height: Option<u32>,
    quality: Option<u32>,
) -> Result<String, String> {
    let client = reqwest::Client::new();
    let mut url = format!("http://127.0.0.1:{}/jellyfin/image/{}", RPC_PORT, id);
    let mut query_parts = Vec::new();
    if let Some(h) = max_height {
        query_parts.push(format!("maxHeight={}", h));
    }
    if let Some(q) = quality {
        query_parts.push(format!("quality={}", q));
    }
    if !query_parts.is_empty() {
        url = format!("{}?{}", url, query_parts.join("&"));
    }

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Image fetch failed: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Image fetch returned {}", response.status()));
    }

    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("image/jpeg")
        .to_string();

    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Image read failed: {}", e))?;

    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(format!("data:{};base64,{}", content_type, b64))
}

/// Proxies JSON-RPC calls from the frontend to the daemon.
/// This bypasses browser security restrictions (mixed content, CORS) that block
/// fetch() from https://tauri.localhost to http://localhost:19140 in release mode.
#[tauri::command]
async fn rpc_proxy(method: String, params: serde_json::Value) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
        "id": 1
    });

    let response = client
        .post(format!("http://127.0.0.1:{}", RPC_PORT))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("RPC connection failed: {}", e))?;

    let data: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("RPC response parse failed: {}", e))?;

    if let Some(error) = data.get("error").filter(|e| !e.is_null()) {
        return Err(error["message"]
            .as_str()
            .unwrap_or("Unknown RPC error")
            .to_string());
    }

    Ok(data
        .get("result")
        .cloned()
        .unwrap_or(serde_json::Value::Null))
}

#[tauri::command]
async fn settings_set_launch_on_startup(enabled: bool) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        if enabled {
            install_launchd_plist()
        } else {
            unload_and_remove_launchd_plist()
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = enabled;
        Ok(())
    }
}

const LOG_MAX_BYTES: u64 = 1_048_576; // 1 MB

/// Simple file-based log for release mode where stdout/stderr are unavailable.
/// Truncates at 1 MB.
fn ui_log(msg: &str) {
    // Always try println (works in debug mode)
    println!("{}", msg);

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    #[cfg(target_os = "windows")]
    if let Ok(appdata) = std::env::var("APPDATA") {
        let log_dir = std::path::Path::new(&appdata).join("HifiMule");
        let _ = std::fs::create_dir_all(&log_dir);
        let log_path = log_dir.join("ui.log");
        if let Ok(meta) = std::fs::metadata(&log_path) {
            if meta.len() > LOG_MAX_BYTES {
                let _ = std::fs::write(&log_path, "--- log truncated ---\n");
            }
        }
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
        {
            use std::io::Write;
            let _ = writeln!(f, "[{}] {}", timestamp, msg);
        }
    }

    #[cfg(target_os = "macos")]
    if let Ok(home) = std::env::var("HOME") {
        let log_dir = std::path::Path::new(&home)
            .join("Library/Application Support/HifiMule");
        let _ = std::fs::create_dir_all(&log_dir);
        let log_path = log_dir.join("ui.log");
        if let Ok(meta) = std::fs::metadata(&log_path) {
            if meta.len() > LOG_MAX_BYTES {
                let _ = std::fs::write(&log_path, "--- log truncated ---\n");
            }
        }
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
        {
            use std::io::Write;
            let _ = writeln!(f, "[{}] {}", timestamp, msg);
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    ui_log(&format!(
        "HifiMule UI starting (release={})",
        !cfg!(debug_assertions)
    ));

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![get_sidecar_status, rpc_proxy, image_proxy, settings_set_launch_on_startup])
        .setup(|app| {
            app.manage(DaemonProcess(Mutex::new(None)));
            app.manage(SidecarStatus(Mutex::new("starting".to_string())));

            // macOS: install launchd user agent on first launch (or after upgrade removes plist)
            #[cfg(target_os = "macos")]
            {
                let plist_missing = launchd_plist_path().is_some_and(|p| !p.exists());
                if plist_missing {
                    match install_launchd_plist() {
                        Ok(()) => ui_log("launchd plist installed and loaded"),
                        Err(e) => ui_log(&format!("launchd plist install failed: {}", e)),
                    }
                }
            }

            // Perform daemon detection off the main thread to avoid blocking UI startup
            let app_handle = app.handle().clone();
            std::thread::spawn(move || {
                // Step 1: Check if daemon is already running (e.g., as startup app or Windows Service)
                if check_daemon_health() {
                    ui_log("Daemon already running (startup app or existing instance), skipping sidecar spawn");
                    if let Some(state) = app_handle.try_state::<SidecarStatus>() {
                        if let Ok(mut s) = state.0.lock() {
                            *s = "startup".to_string();
                        }
                    }
                    return;
                }

                // Step 2: Try to start the Windows Service
                #[cfg(windows)]
                {
                    ui_log("Daemon not running, attempting to start Windows Service...");
                    if try_start_service() {
                        // Give the service a moment to start and verify
                        std::thread::sleep(std::time::Duration::from_secs(2));
                        if check_daemon_health() {
                            ui_log("Windows Service started successfully");
                            if let Some(state) = app_handle.try_state::<SidecarStatus>() {
                                if let Ok(mut s) = state.0.lock() {
                                    *s = "service".to_string();
                                }
                            }
                            return;
                        }
                        ui_log("Service started but health check failed, falling back to sidecar");
                    } else {
                        ui_log("Windows Service not available, falling back to sidecar");
                    }
                }

                // Step 3: On non-Windows (or Windows fallback), spawn sidecar
                // Note: spawn_sidecar needs the App, but we're on a background thread.
                // Use the app_handle to get state and spawn via shell.
                ui_log("Spawning sidecar from background thread");

                // On macOS, Gatekeeper bypass only clears com.apple.quarantine on the
                // top-level bundle directory, not recursively. Strip it from the sidecar
                // binary explicitly so macOS allows programmatic spawning.
                // Tauri bundles sidecars with a target-triple suffix (e.g.
                // hifimule-daemon-universal-apple-darwin), so scan the directory rather
                // than joining a plain name that does not exist.
                #[cfg(target_os = "macos")]
                if let Some(sp) = resolve_daemon_binary_path() {
                    ui_log(&format!("Resolving macOS sidecar at {:?}", sp));
                    let _ = std::process::Command::new("xattr")
                        .args(["-d", "com.apple.quarantine"])
                        .arg(&sp)
                        .output();
                }

                match app_handle.shell().sidecar("hifimule-daemon") {
                    Ok(sidecar) => {
                        ui_log("Sidecar command created, spawning...");
                        match sidecar.spawn() {
                            Ok((mut rx, child)) => {
                                ui_log(&format!(
                                    "Sidecar spawned successfully (pid={})",
                                    child.pid()
                                ));
                                if let Some(state) = app_handle.try_state::<SidecarStatus>() {
                                    if let Ok(mut s) = state.0.lock() {
                                        *s = format!("running (pid={})", child.pid());
                                    }
                                }
                                if let Some(state) = app_handle.try_state::<DaemonProcess>() {
                                    if let Ok(mut daemon_proc) = state.0.lock() {
                                        *daemon_proc = Some(child);
                                    }
                                }
                                let handle_clone = app_handle.clone();
                                tauri::async_runtime::spawn(async move {
                                    use tauri_plugin_shell::process::CommandEvent;
                                    while let Some(event) = rx.recv().await {
                                        match event {
                                            CommandEvent::Stdout(line) => {
                                                ui_log(&format!(
                                                    "Daemon stdout: {}",
                                                    String::from_utf8_lossy(&line)
                                                ));
                                            }
                                            CommandEvent::Stderr(line) => {
                                                ui_log(&format!(
                                                    "Daemon stderr: {}",
                                                    String::from_utf8_lossy(&line)
                                                ));
                                            }
                                            CommandEvent::Terminated(payload) => {
                                                let msg = format!(
                                                    "Daemon process terminated (code={:?}, signal={:?})",
                                                    payload.code, payload.signal
                                                );
                                                ui_log(&msg);
                                                if let Some(state) =
                                                    handle_clone.try_state::<SidecarStatus>()
                                                {
                                                    if let Ok(mut s) = state.0.lock() {
                                                        *s = format!(
                                                            "terminated (code={:?})",
                                                            payload.code
                                                        );
                                                    }
                                                }
                                                break;
                                            }
                                            CommandEvent::Error(err) => {
                                                ui_log(&format!("Daemon event error: {}", err));
                                            }
                                            _ => {}
                                        }
                                    }
                                });
                            }
                            Err(e) => {
                                let msg = format!("Failed to spawn sidecar: {}", e);
                                ui_log(&msg);
                                if let Some(state) = app_handle.try_state::<SidecarStatus>() {
                                    if let Ok(mut s) = state.0.lock() {
                                        *s = format!("spawn_failed: {}", e);
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        let msg = format!("Failed to create sidecar command: {}", e);
                        ui_log(&msg);
                        if let Some(state) = app_handle.try_state::<SidecarStatus>() {
                            if let Ok(mut s) = state.0.lock() {
                                *s = format!("command_failed: {}", e);
                            }
                        }
                    }
                }
            });

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
                        ui_log("Killing hifimule-daemon sidecar before exit");
                        let _ = child.kill();
                    }
                }
            }
        }
    });
}
