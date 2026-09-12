use std::sync::Mutex;
use tauri::{Manager, RunEvent};

struct PendingLaunch(Mutex<Option<String>>);

/// Stores the sidecar launch status so the frontend can query it.
/// Values: "starting", "startup" (connected to running daemon via health check),
/// "service" (started via sc start), "running (pid=N)",
/// "spawn_failed: ...", "command_failed: ...", "terminated (code=N)"
struct SidecarStatus(Mutex<String>);

#[tauri::command]
fn get_sidecar_status(state: tauri::State<'_, SidecarStatus>) -> String {
    state.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

fn resolve_daemon_binary_path() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        #[cfg(windows)]
        let matches = name.starts_with("hifimule-daemon") && name.ends_with(".exe");
        #[cfg(not(windows))]
        let matches = name == "hifimule-daemon" || name.starts_with("hifimule-daemon-");
        if matches && entry.path().is_file() {
            return Some(entry.path());
        }
    }
    None
}

/// Check if the daemon is already running by sending a health-check RPC call.
fn daemon_health_response_ok(
    data: &serde_json::Value,
    descriptor: &hifimule_lifecycle::OwnerDescriptor,
) -> bool {
    data.get("error").is_none_or(serde_json::Value::is_null)
        && data
            .pointer("/result/data/status")
            .and_then(serde_json::Value::as_str)
            == Some("ok")
        && data
            .pointer("/result/data/protocolVersion")
            .and_then(serde_json::Value::as_u64)
            == Some(u64::from(hifimule_lifecycle::PROTOCOL_VERSION))
        && data
            .pointer("/result/data/instanceId")
            .and_then(serde_json::Value::as_str)
            == Some(descriptor.instance_id.as_str())
}

fn check_daemon_health(descriptor: &hifimule_lifecycle::OwnerDescriptor) -> bool {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build();
    let client = match client {
        Ok(c) => c,
        Err(_) => return false,
    };
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "daemon.health",
        "params": {},
        "id": 1
    });
    match client
        .post(format!("http://127.0.0.1:{}", descriptor.port))
        .bearer_auth(&descriptor.token)
        .json(&body)
        .send()
    {
        Ok(resp) if resp.status().is_success() => resp
            .json::<serde_json::Value>()
            .is_ok_and(|data| daemon_health_response_ok(&data, descriptor)),
        Ok(_) => false,
        Err(_) => false,
    }
}

async fn validate_owner_async(
    client: &reqwest::Client,
    descriptor: &hifimule_lifecycle::OwnerDescriptor,
) -> Result<(), String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "daemon.health",
        "params": {},
        "id": 1
    });
    let response = client
        .post(format!("http://127.0.0.1:{}", descriptor.port))
        .bearer_auth(&descriptor.token)
        .json(&body)
        .timeout(hifimule_lifecycle::HEALTH_TIMEOUT)
        .send()
        .await
        .map_err(|_| "OWNER_CHANGED: local daemon health check failed".to_string())?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err("LOCAL_ACCESS_DENIED: local daemon rejected access".to_string());
    }
    let data = response
        .json::<serde_json::Value>()
        .await
        .map_err(|_| "OWNER_CHANGED: local daemon health response was malformed".to_string())?;
    if data
        .pointer("/result/data/status")
        .and_then(serde_json::Value::as_str)
        == Some("stopping")
    {
        return Err("DAEMON_STOPPED: local daemon is stopping".to_string());
    }
    if !daemon_health_response_ok(&data, descriptor) {
        return Err("OWNER_CHANGED: daemon identity or protocol changed".to_string());
    }
    Ok(())
}

fn spawn_detached_daemon(expected_generation: u64, attempt_id: &str) -> Result<(), String> {
    let path = resolve_daemon_binary_path()
        .ok_or_else(|| "SPAWN_FAILED: daemon binary was not found".to_string())?;
    let mut command = std::process::Command::new(path);
    let generation_arg = expected_generation.to_string();
    command
        .args([
            "--launch-generation",
            &generation_arg,
            "--launch-attempt",
            attempt_id,
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS | CREATE_NO_WINDOW);
    }
    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("SPAWN_FAILED: {error}"))
}

fn coordinate_daemon(
    pending: &PendingLaunch,
) -> Result<hifimule_lifecycle::OwnerDescriptor, String> {
    let app_data = hifimule_lifecycle::resolve_app_data_dir().map_err(|error| error.to_string())?;
    let deadline = std::time::Instant::now() + hifimule_lifecycle::STARTUP_DEADLINE;
    let mut launched = false;
    let mut attempt_id: Option<String> = None;
    loop {
        match hifimule_lifecycle::read_descriptor(&app_data) {
            Ok(descriptor) => {
                descriptor
                    .validate_compatible()
                    .map_err(|error| error.to_string())?;
                if check_daemon_health(&descriptor) {
                    if let Some(id) = attempt_id.as_deref() {
                        let _ = hifimule_lifecycle::cancel_launch_ticket(&app_data, id);
                    }
                    *pending.0.lock().unwrap_or_else(|error| error.into_inner()) = None;
                    return Ok(descriptor);
                }
                if !launched && let Some(id) = try_launch_candidate(&app_data, pending)? {
                    attempt_id = Some(id);
                    launched = true;
                }
            }
            Err(error) if !launched => {
                if let Some(id) = try_launch_candidate(&app_data, pending)? {
                    attempt_id = Some(id);
                    launched = true;
                } else if error.code() == hifimule_lifecycle::LifecycleErrorCode::ProtocolMismatch {
                    return Err(error.to_string());
                }
            }
            Err(_) => {}
        }
        if std::time::Instant::now() >= deadline {
            if let Some(id) = attempt_id.as_deref() {
                let _ = hifimule_lifecycle::cancel_launch_ticket(&app_data, id);
            }
            *pending.0.lock().unwrap_or_else(|error| error.into_inner()) = None;
            return Err("STARTUP_TIMEOUT: daemon readiness exceeded 15 seconds".to_string());
        }
        std::thread::sleep(hifimule_lifecycle::POLL_INTERVAL);
    }
}

fn try_launch_candidate(
    app_data: &std::path::Path,
    pending: &PendingLaunch,
) -> Result<Option<String>, String> {
    match hifimule_lifecycle::OwnerGuard::acquire(app_data) {
        Ok(owner) => drop(owner),
        Err(error) if error.code() == hifimule_lifecycle::LifecycleErrorCode::OwnerChanged => {
            return Ok(None);
        }
        Err(error) => return Err(error.to_string()),
    }
    let generation =
        hifimule_lifecycle::read_generation(app_data).map_err(|error| error.to_string())?;
    let id = hifimule_lifecycle::create_launch_ticket(app_data, generation)
        .map_err(|error| error.to_string())?;
    *pending.0.lock().unwrap_or_else(|error| error.into_inner()) = Some(id.clone());
    if let Err(error) = spawn_detached_daemon(generation, &id) {
        let _ = hifimule_lifecycle::cancel_launch_ticket(app_data, &id);
        *pending.0.lock().unwrap_or_else(|error| error.into_inner()) = None;
        return Err(error);
    }
    Ok(Some(id))
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
    let daemon_path_escaped = daemon_path_str
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    let plist_content = LAUNCHD_PLIST_TEMPLATE.replace("{DAEMON_PATH}", &daemon_path_escaped);
    let plist_path = launchd_plist_path()
        .ok_or_else(|| "Cannot resolve LaunchAgents path (HOME not set?)".to_string())?;
    let launch_agents = plist_path
        .parent()
        .ok_or_else(|| "Cannot get LaunchAgents parent dir".to_string())?;
    std::fs::create_dir_all(launch_agents)
        .map_err(|e| format!("Cannot create LaunchAgents dir: {}", e))?;
    std::fs::write(&plist_path, plist_content).map_err(|e| format!("Cannot write plist: {}", e))?;
    let plist_str = plist_path
        .to_str()
        .ok_or_else(|| "Plist path is not valid UTF-8".to_string())?;
    let output = std::process::Command::new("launchctl")
        .args(["load", plist_str])
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
    let plist_path =
        launchd_plist_path().ok_or_else(|| "Cannot resolve LaunchAgents path".to_string())?;
    if plist_path.exists() {
        let plist_str = plist_path
            .to_str()
            .ok_or_else(|| "Plist path is not valid UTF-8".to_string())?;
        match std::process::Command::new("launchctl")
            .args(["unload", plist_str])
            .output()
        {
            Err(e) => ui_log(&format!(
                "launchctl unload warning (failed to execute): {}",
                e
            )),
            Ok(output) if !output.status.success() => ui_log(&format!(
                "launchctl unload warning (may already be unloaded): {}",
                String::from_utf8_lossy(&output.stderr)
            )),
            Ok(_) => {}
        }
        std::fs::remove_file(&plist_path).map_err(|e| format!("Cannot remove plist: {}", e))?;
    }
    Ok(())
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
    let app_data = hifimule_lifecycle::resolve_app_data_dir().map_err(|error| error.to_string())?;
    let descriptor =
        hifimule_lifecycle::read_descriptor(&app_data).map_err(|error| error.to_string())?;
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| format!("LOCAL_ACCESS_DENIED: {error}"))?;
    validate_owner_async(&client, &descriptor).await?;
    let mut url = format!("http://127.0.0.1:{}/jellyfin/image/{}", descriptor.port, id);
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
        .bearer_auth(&descriptor.token)
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
async fn rpc_proxy(
    method: String,
    params: serde_json::Value,
) -> Result<serde_json::Value, serde_json::Value> {
    let app_data = hifimule_lifecycle::resolve_app_data_dir().map_err(
        |error| serde_json::json!({ "code": "LOCAL_ACCESS_DENIED", "message": error.to_string() }),
    )?;
    let descriptor = hifimule_lifecycle::read_descriptor(&app_data).map_err(
        |error| serde_json::json!({ "code": error.code().as_str(), "message": error.to_string() }),
    )?;
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| serde_json::json!({ "code": "LOCAL_ACCESS_DENIED", "message": error.to_string() }))?;
    if method != "daemon.health" {
        validate_owner_async(&client, &descriptor)
            .await
            .map_err(|message| {
                let code = message
                    .split_once(':')
                    .map(|(code, _)| code)
                    .unwrap_or("OWNER_CHANGED");
                serde_json::json!({ "code": code, "message": message })
            })?;
    }
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
        "id": 1
    });

    let mut request = client
        .post(format!("http://127.0.0.1:{}", descriptor.port))
        .bearer_auth(&descriptor.token)
        .json(&body);
    if method == "get_daemon_state" || method == "daemon.health" {
        request = request.timeout(std::time::Duration::from_secs(15));
    }
    let response = request
        .send()
        .await
        .map_err(|e| serde_json::json!({ "code": "OWNER_CHANGED", "message": format!("RPC connection failed: {}", e) }))?;

    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(serde_json::json!({
            "code": "LOCAL_ACCESS_DENIED",
            "message": "Local daemon access was rejected"
        }));
    }
    let data: serde_json::Value = response.json().await.map_err(
        |e| serde_json::json!({ "message": format!("RPC response parse failed: {}", e) }),
    )?;

    if let Some(error) = data.get("error").filter(|e| !e.is_null()) {
        // Forward the full JSON-RPC error envelope (code + message + data) so the
        // UI can react to specific codes (e.g. ERR_UNAUTHORIZED → scoped re-auth).
        return Err(error.clone());
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

#[cfg(any(target_os = "windows", target_os = "macos"))]
const LOG_MAX_BYTES: u64 = 1_048_576; // 1 MB

fn log_timestamp() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Simple file-based log for release mode where stdout/stderr are unavailable.
/// Truncates at 1 MB.
fn ui_log(msg: &str) {
    // Always try println (works in debug mode)
    println!("{}", msg);

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    let timestamp = log_timestamp();

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
        let log_dir = std::path::Path::new(&home).join("Library/Application Support/HifiMule");
        let _ = std::fs::create_dir_all(&log_dir);
        let log_path = log_dir.join("ui.log");
        if let Ok(meta) = std::fs::metadata(&log_path)
            && meta.len() > LOG_MAX_BYTES
        {
            let _ = std::fs::write(&log_path, "--- log truncated ---\n");
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
        .invoke_handler(tauri::generate_handler![
            get_sidecar_status,
            rpc_proxy,
            image_proxy,
            settings_set_launch_on_startup
        ])
        .setup(|app| {
            app.manage(SidecarStatus(Mutex::new("starting".to_string())));
            app.manage(PendingLaunch(Mutex::new(None)));
            let app_handle = app.handle().clone();
            std::thread::spawn(move || {
                let result = app_handle
                    .try_state::<PendingLaunch>()
                    .ok_or_else(|| "Lifecycle coordinator state is unavailable".to_string())
                    .and_then(|pending| coordinate_daemon(&pending));
                if let Some(state) = app_handle.try_state::<SidecarStatus>()
                    && let Ok(mut status) = state.0.lock()
                {
                    *status = match result {
                        Ok(descriptor) => format!(
                            "ready (pid={}, instance={})",
                            descriptor.pid, descriptor.instance_id
                        ),
                        Err(error) => format!("failed: {error}"),
                    };
                }
            });

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    builder.run(|app_handle, event| {
        if let RunEvent::Exit = event {
            // Cancels only an unaccepted launch ticket. An established owner is detached
            // and intentionally survives UI closure.
            if let Some(state) = app_handle.try_state::<PendingLaunch>()
                && let Ok(mut pending) = state.0.lock()
                && let Some(attempt_id) = pending.take()
                && let Ok(app_data) = hifimule_lifecycle::resolve_app_data_dir()
            {
                let _ = hifimule_lifecycle::cancel_launch_ticket(&app_data, &attempt_id);
            }
        }
    });
}

#[cfg(test)]
mod log_timestamp_tests {
    use super::{daemon_health_response_ok, log_timestamp};

    #[test]
    fn daemon_health_response_requires_ok_result() {
        let descriptor = hifimule_lifecycle::OwnerDescriptor {
            schema_version: 1,
            protocol_version: 1,
            instance_id: "instance-1".to_string(),
            pid: 123,
            port: 12345,
            token: "00".repeat(32),
            launch_generation: "0".to_string(),
        };
        assert!(daemon_health_response_ok(
            &serde_json::json!({
                "jsonrpc": "2.0",
                "result": { "data": { "status": "ok", "protocolVersion": 1, "instanceId": "instance-1" } },
                "error": null,
                "id": 1
            }),
            &descriptor
        ));
        assert!(!daemon_health_response_ok(
            &serde_json::json!({
                "jsonrpc": "2.0",
                "result": null,
                "error": { "code": -32601, "message": "Method not found" },
                "id": 1
            }),
            &descriptor
        ));
        assert!(!daemon_health_response_ok(
            &serde_json::json!({
                "jsonrpc": "2.0",
                "result": { "data": { "status": "starting" } },
                "error": null,
                "id": 1
            }),
            &descriptor
        ));
    }

    #[test]
    fn log_timestamp_is_readable_and_sortable() {
        let timestamp = log_timestamp();
        assert_eq!(timestamp.len(), 19);
        assert_eq!(&timestamp[4..5], "-");
        assert_eq!(&timestamp[7..8], "-");
        assert_eq!(&timestamp[10..11], " ");
        assert_eq!(&timestamp[13..14], ":");
        assert_eq!(&timestamp[16..17], ":");
    }
}
