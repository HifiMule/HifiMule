use std::sync::Mutex;
use tauri::{Manager, RunEvent};

use hifimule_lifecycle::{LifecycleErrorCode as Code, LifecycleState, LifecycleStatus};
use std::sync::Arc;
use std::time::Instant;

struct StartupState {
    epoch: u64,
    closed: bool,
    ticket: Option<String>,
    status: LifecycleStatus,
    hydrated: Option<(u64, hifimule_lifecycle::OwnerDescriptor)>,
}

#[derive(Clone)]
struct StartupCoordinator(Arc<Mutex<StartupState>>);

fn status(state: LifecycleState, error_code: Option<Code>) -> LifecycleStatus {
    LifecycleStatus {
        state,
        error_code,
        instance_id: None,
        pid: None,
    }
}

#[tauri::command]
fn get_sidecar_status(coordinator: tauri::State<'_, StartupCoordinator>) -> LifecycleStatus {
    coordinator
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .status
        .clone()
}

impl StartupCoordinator {
    fn current(&self, epoch: u64) -> bool {
        let state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        !state.closed && state.epoch == epoch
    }

    fn begin_attempt(&self) -> Option<(u64, Option<String>)> {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if state.closed {
            return None;
        }
        state.epoch += 1;
        state.hydrated = None;
        state.status = status(LifecycleState::Starting, None);
        Some((state.epoch, state.ticket.take()))
    }

    fn restart(&self) {
        let Some((epoch, old_ticket)) = self.begin_attempt() else {
            return;
        };
        let coordinator = self.clone();
        std::thread::spawn(move || {
            if let (Some(ticket), Ok(path)) =
                (old_ticket, hifimule_lifecycle::resolve_app_data_dir())
            {
                let _ = hifimule_lifecycle::cancel_launch_ticket(&path, &ticket);
            }
            let outcome = coordinate_daemon(&coordinator, epoch);
            let mut state = coordinator.0.lock().unwrap_or_else(|e| e.into_inner());
            if state.epoch == epoch && !state.closed {
                state.status = match outcome {
                    Ok(owner) => LifecycleStatus {
                        state: LifecycleState::Ready,
                        instance_id: Some(owner.instance_id),
                        pid: Some(owner.pid),
                        error_code: None,
                    },
                    Err(code) => status(
                        if code == Code::DaemonStopped {
                            LifecycleState::Stopped
                        } else {
                            LifecycleState::Failed
                        },
                        Some(code),
                    ),
                };
                state.ticket = None;
            }
        });
    }

    fn close(&self) {
        let ticket = {
            let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
            state.closed = true;
            state.epoch += 1;
            state.ticket.take()
        };
        if let (Some(ticket), Ok(path)) = (ticket, hifimule_lifecycle::resolve_app_data_dir()) {
            let _ = hifimule_lifecycle::cancel_launch_ticket(&path, &ticket);
        }
    }
}

#[tauri::command]
fn retry_daemon_startup(coordinator: tauri::State<'_, StartupCoordinator>) {
    coordinator.restart();
}

#[tauri::command]
fn reload_main_window(app: tauri::AppHandle) -> Result<(), String> {
    app.get_webview_window("main")
        .ok_or("Main window is unavailable")?
        .eval("window.location.reload()")
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn close_ui(
    app: tauri::AppHandle,
    coordinator: tauri::State<'_, StartupCoordinator>,
) -> Result<(), String> {
    let coordinator = coordinator.inner().clone();
    tauri::async_runtime::spawn_blocking(move || coordinator.close())
        .await
        .map_err(|e| e.to_string())?;
    app.exit(0);
    Ok(())
}

/// An installed smoke run may request a non-secret acknowledgment after the actual
/// main webview has hydrated through rpc_proxy and finished rendering its route.
#[tauri::command]
fn report_ui_ready(coordinator: tauri::State<'_, StartupCoordinator>) -> Result<(), String> {
    let state = coordinator.0.lock().unwrap_or_else(|e| e.into_inner());
    let (epoch, owner) = state.hydrated.as_ref().ok_or("UI has not hydrated")?;
    if *epoch != state.epoch || state.closed || state.status.state != LifecycleState::Ready {
        return Err("UI startup attempt was abandoned".into());
    }
    let args: Vec<_> = std::env::args().collect();
    if let Some(pair) = args.windows(2).find(|pair| pair[0] == "--smoke-id") {
        let path = hifimule_lifecycle::resolve_app_data_dir().map_err(|e| e.to_string())?;
        hifimule_lifecycle::publish_ui_ready(&path, &pair[1], owner).map_err(|e| e.to_string())?;
    }
    Ok(())
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
    match hifimule_lifecycle::validate_health_response(&data, descriptor) {
        Ok(LifecycleState::Ready) => Ok(()),
        Ok(_) => Err("DAEMON_STOPPED: local daemon is stopping".into()),
        Err(error) => Err(format!(
            "{}: local health validation failed",
            error.code().as_str()
        )),
    }
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
    coordinator: &StartupCoordinator,
    epoch: u64,
) -> Result<hifimule_lifecycle::OwnerDescriptor, Code> {
    let deadline = Instant::now() + hifimule_lifecycle::STARTUP_DEADLINE;
    let app_data = hifimule_lifecycle::resolve_app_data_dir().map_err(|e| e.code())?;
    coordinate_daemon_at(
        &app_data,
        coordinator,
        epoch,
        deadline,
        spawn_detached_daemon,
    )
}

fn coordinate_daemon_at(
    app_data: &std::path::Path,
    coordinator: &StartupCoordinator,
    epoch: u64,
    deadline: Instant,
    mut spawn: impl FnMut(u64, &str) -> Result<(), String>,
) -> Result<hifimule_lifecycle::OwnerDescriptor, Code> {
    let generation = hifimule_lifecycle::read_generation(app_data).map_err(|e| e.code())?;
    let mut observed_owner = false;
    let mut ticket: Option<String> = None;
    let result = (|| {
        loop {
            if !coordinator.current(epoch) {
                return Err(Code::DaemonStopped);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(Code::StartupTimeout);
            }
            if hifimule_lifecycle::read_generation(app_data).map_err(|e| e.code())? != generation {
                return Err(Code::DaemonStopped);
            }
            if let Some(id) = ticket.as_deref()
                && let Some(code) =
                    hifimule_lifecycle::read_launch_failure(app_data, id).map_err(|e| e.code())?
            {
                return Err(code);
            }
            let mut incompatible_discovery = false;
            match hifimule_lifecycle::read_descriptor(app_data) {
                Ok(descriptor) => {
                    match hifimule_lifecycle::check_owner_health(&descriptor, remaining) {
                        Ok(LifecycleState::Ready) => {
                            if !coordinator.current(epoch) {
                                return Err(Code::DaemonStopped);
                            }
                            return Ok(descriptor);
                        }
                        Ok(_) => return Err(Code::DaemonStopped),
                        Err(error) if !error.is_retryable() => {
                            return Err(error.code());
                        }
                        Err(_) => {}
                    }
                }
                Err(error) if error.code() == Code::UnsafeRuntimePath => return Err(error.code()),
                Err(error) if error.code() == Code::ProtocolMismatch => {
                    if observed_owner {
                        return Err(Code::ProtocolMismatch);
                    }
                    incompatible_discovery = true;
                }
                Err(_) => {}
            }
            if ticket.is_none() && !observed_owner {
                match hifimule_lifecycle::OwnerGuard::acquire(app_data) {
                    Ok(owner) => {
                        owner
                            .verify_expected_generation(generation)
                            .map_err(|e| e.code())?;
                        drop(owner);
                        let mut state = coordinator.0.lock().unwrap_or_else(|e| e.into_inner());
                        if state.closed || state.epoch != epoch {
                            return Err(Code::DaemonStopped);
                        }
                        if Instant::now() >= deadline {
                            return Err(Code::StartupTimeout);
                        }
                        let id = hifimule_lifecycle::create_launch_ticket_until(
                            app_data, generation, deadline,
                        )
                        .map_err(|e| e.code())?;
                        state.ticket = Some(id.clone());
                        ticket = Some(id.clone());
                        spawn(generation, &id).map_err(|_| Code::SpawnFailed)?;
                    }
                    Err(error) if error.code() == Code::OwnerChanged => {
                        if incompatible_discovery {
                            return Err(Code::ProtocolMismatch);
                        }
                        observed_owner = true;
                    }
                    Err(error) => return Err(error.code()),
                }
            }
            std::thread::sleep(
                deadline
                    .saturating_duration_since(Instant::now())
                    .min(hifimule_lifecycle::POLL_INTERVAL),
            );
        }
    })();
    if let Some(id) = ticket {
        let _ = hifimule_lifecycle::cancel_launch_ticket_until(app_data, &id, deadline);
        hifimule_lifecycle::clear_launch_failure(app_data, &id);
    }
    result
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
    coordinator: tauri::State<'_, StartupCoordinator>,
) -> Result<serde_json::Value, serde_json::Value> {
    let epoch = coordinator
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .epoch;
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

    if method == "get_daemon_state" {
        let mut state = coordinator.0.lock().unwrap_or_else(|e| e.into_inner());
        if state.epoch == epoch && !state.closed {
            state.hydrated = Some((epoch, descriptor));
        }
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
            retry_daemon_startup,
            reload_main_window,
            close_ui,
            report_ui_ready,
            rpc_proxy,
            image_proxy,
            settings_set_launch_on_startup
        ])
        .setup(|app| {
            let coordinator = StartupCoordinator(Arc::new(Mutex::new(StartupState {
                epoch: 0,
                closed: false,
                ticket: None,
                hydrated: None,
                status: status(LifecycleState::Starting, None),
            })));
            app.manage(coordinator.clone());
            coordinator.restart();

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    builder.run(|app_handle, event| {
        if let RunEvent::Exit = event
            && let Some(coordinator) = app_handle.try_state::<StartupCoordinator>()
        {
            coordinator.close();
        }
    });
}

#[cfg(test)]
mod log_timestamp_tests {
    use super::log_timestamp;

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

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use std::time::Duration;

    fn coordinator() -> StartupCoordinator {
        StartupCoordinator(Arc::new(Mutex::new(StartupState {
            epoch: 1,
            closed: false,
            ticket: None,
            hydrated: None,
            status: status(LifecycleState::Starting, None),
        })))
    }

    #[test]
    fn pre_quit_waiter_never_adopts_new_generation() {
        let temp = tempfile::tempdir().unwrap();
        let owner = hifimule_lifecycle::OwnerGuard::acquire(temp.path()).unwrap();
        let worker = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            owner.advance_generation().unwrap();
            drop(owner);
        });
        let mut spawned = false;
        let result = coordinate_daemon_at(
            temp.path(),
            &coordinator(),
            1,
            Instant::now() + Duration::from_secs(1),
            |_, _| {
                spawned = true;
                Ok(())
            },
        );
        worker.join().unwrap();
        assert_eq!(result.err(), Some(Code::DaemonStopped));
        assert!(!spawned);
    }

    #[test]
    fn observed_slow_owner_never_authorizes_later_election() {
        let temp = tempfile::tempdir().unwrap();
        let owner = hifimule_lifecycle::OwnerGuard::acquire(temp.path()).unwrap();
        let worker = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            drop(owner);
        });
        let mut spawned = false;
        let result = coordinate_daemon_at(
            temp.path(),
            &coordinator(),
            1,
            Instant::now() + Duration::from_millis(450),
            |_, _| {
                spawned = true;
                Ok(())
            },
        );
        worker.join().unwrap();
        assert_eq!(result.err(), Some(Code::StartupTimeout));
        assert!(!spawned);
    }

    #[test]
    fn candidate_failure_is_reported_without_waiting_for_timeout() {
        let temp = tempfile::tempdir().unwrap();
        let started = Instant::now();
        let result = coordinate_daemon_at(
            temp.path(),
            &coordinator(),
            1,
            started + Duration::from_secs(2),
            |_, id| {
                hifimule_lifecycle::publish_launch_failure(
                    temp.path(),
                    id,
                    Code::LegacyEndpointOccupied,
                )
                .unwrap();
                Ok(())
            },
        );
        assert_eq!(result.err(), Some(Code::LegacyEndpointOccupied));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn retry_replaces_failed_attempt_and_invalidates_old_results() {
        let coordinator = coordinator();
        coordinator.0.lock().unwrap().status =
            status(LifecycleState::Failed, Some(Code::SpawnFailed));
        let (epoch, _) = coordinator.begin_attempt().unwrap();
        assert_eq!(epoch, 2);
        assert!(!coordinator.current(1));
        assert!(coordinator.current(2));
        assert_eq!(
            coordinator.0.lock().unwrap().status.state,
            LifecycleState::Starting
        );
        coordinator.close();
        assert!(!coordinator.current(2));
        assert!(coordinator.begin_attempt().is_none());
    }
}
