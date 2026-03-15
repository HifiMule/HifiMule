use std::sync::Mutex;
use tauri::{Manager, RunEvent};
use tauri_plugin_shell::process::CommandChild;
use tauri_plugin_shell::ShellExt;

struct DaemonProcess(Mutex<Option<CommandChild>>);

/// Stores the sidecar launch status so the frontend can query it.
/// Values: "starting", "service" (connected to Windows Service), "running (pid=N)",
/// "spawn_failed: ...", "command_failed: ...", "terminated (code=N)"
struct SidecarStatus(Mutex<String>);

const RPC_PORT: u16 = 19140;

#[tauri::command]
fn get_sidecar_status(state: tauri::State<'_, SidecarStatus>) -> String {
    state.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
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
    client
        .post(format!("http://127.0.0.1:{}", RPC_PORT))
        .json(&body)
        .send()
        .is_ok()
}

/// Attempt to start the daemon Windows Service via `sc start`.
#[cfg(windows)]
fn try_start_service() -> bool {
    use std::process::Command;
    let result = Command::new("sc")
        .args(["start", "jellyfinsync-daemon"])
        .output();
    match result {
        Ok(output) => output.status.success(),
        Err(_) => false,
    }
}

/// Proxies a Jellyfin image from the daemon, returning it as a base64 data URL.
/// Images loaded via CSS `background-image: url(...)` can't use invoke, so the frontend
/// must call this and set the result as inline style.
#[tauri::command]
async fn image_proxy(id: String, max_height: Option<u32>, quality: Option<u32>) -> Result<String, String> {
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
        return Err(error["message"].as_str().unwrap_or("Unknown RPC error").to_string());
    }

    Ok(data.get("result").cloned().unwrap_or(serde_json::Value::Null))
}

/// Spawn the daemon as a sidecar process (fallback when service is not available).
fn spawn_sidecar(app: &tauri::App) {
    match app.shell().sidecar("jellyfinsync-daemon") {
        Ok(sidecar) => {
            ui_log("Sidecar command created, spawning...");
            match sidecar.spawn() {
                Ok((mut rx, child)) => {
                    ui_log(&format!(
                        "Sidecar spawned successfully (pid={})",
                        child.pid()
                    ));
                    if let Some(state) = app.try_state::<SidecarStatus>() {
                        if let Ok(mut s) = state.0.lock() {
                            *s = format!("running (pid={})", child.pid());
                        }
                    }
                    if let Some(state) = app.try_state::<DaemonProcess>() {
                        if let Ok(mut daemon_proc) = state.0.lock() {
                            *daemon_proc = Some(child);
                        }
                    }

                    // Listen for sidecar events in background
                    let app_handle = app.handle().clone();
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
                                        app_handle.try_state::<SidecarStatus>()
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
                    if let Some(state) = app.try_state::<SidecarStatus>() {
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
            if let Some(state) = app.try_state::<SidecarStatus>() {
                if let Ok(mut s) = state.0.lock() {
                    *s = format!("command_failed: {}", e);
                }
            }
        }
    }
}

/// Simple file-based log for release mode where stdout/stderr are unavailable.
fn ui_log(msg: &str) {
    // Always try println (works in debug mode)
    println!("{}", msg);

    // Also write to file for release mode
    if let Ok(appdata) = std::env::var("APPDATA") {
        let log_dir = std::path::Path::new(&appdata).join("JellyfinSync");
        let _ = std::fs::create_dir_all(&log_dir);
        let log_path = log_dir.join("ui.log");
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
        {
            use std::io::Write;
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let _ = writeln!(f, "[{}] {}", timestamp, msg);
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    ui_log(&format!(
        "JellyfinSync UI starting (release={})",
        !cfg!(debug_assertions)
    ));

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![get_sidecar_status, rpc_proxy, image_proxy])
        .setup(|app| {
            app.manage(DaemonProcess(Mutex::new(None)));
            app.manage(SidecarStatus(Mutex::new("starting".to_string())));

            // Step 1: Check if daemon is already running (e.g., as a Windows Service)
            if check_daemon_health() {
                ui_log("Daemon already running (service or existing instance), skipping sidecar spawn");
                if let Some(state) = app.try_state::<SidecarStatus>() {
                    if let Ok(mut s) = state.0.lock() {
                        *s = "service".to_string();
                    }
                }
            } else {
                // Step 2: Try to start the Windows Service
                #[cfg(windows)]
                {
                    ui_log("Daemon not running, attempting to start Windows Service...");
                    if try_start_service() {
                        // Give the service a moment to start and verify
                        std::thread::sleep(std::time::Duration::from_secs(2));
                        if check_daemon_health() {
                            ui_log("Windows Service started successfully");
                            if let Some(state) = app.try_state::<SidecarStatus>() {
                                if let Ok(mut s) = state.0.lock() {
                                    *s = "service".to_string();
                                }
                            }
                        } else {
                            ui_log("Service started but health check failed, falling back to sidecar");
                            spawn_sidecar(app);
                        }
                    } else {
                        ui_log("Windows Service not available, falling back to sidecar");
                        spawn_sidecar(app);
                    }
                }

                // Step 3: On non-Windows, go straight to sidecar
                #[cfg(not(windows))]
                {
                    ui_log("Daemon not running, spawning sidecar");
                    spawn_sidecar(app);
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
                        ui_log("Killing jellyfinsync-daemon sidecar before exit");
                        let _ = child.kill();
                    }
                }
            }
        }
    });
}
