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

fn log_timestamp() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Simple file-based logger for release mode where stdout/stderr are unavailable.
/// Writes to `%APPDATA%/HifiMule/daemon.log`. Truncates at 1 MB.
pub fn log_to_file(msg: &str) {
    if let Ok(dir) = paths::get_app_data_dir() {
        let log_path = dir.join("daemon.log");
        // Truncate if over 1 MB
        if let Ok(meta) = std::fs::metadata(&log_path)
            && meta.len() > LOG_MAX_BYTES
        {
            let _ = std::fs::write(&log_path, "--- log truncated ---\n");
        }
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
        {
            let timestamp = log_timestamp();
            let _ = writeln!(f, "[{}] {}", timestamp, msg);
        }
    }
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
mod notifications;
mod paths;
mod playback;
#[allow(dead_code)]
mod providers;
mod rpc;
mod scrobbler;
mod server_manager;
mod sync;
mod transcoding;
mod vault;

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

enum CoreCommand {
    BeginShutdown(mpsc::Sender<sync::ShutdownSnapshot>),
    FenceFailed,
    CommitShutdown,
}

pub struct DaemonCoreHandle {
    shutdown: Arc<AtomicBool>,
    state_rx: mpsc::Receiver<DaemonState>,
    ready_rx: mpsc::Receiver<Result<(), String>>,
    command_tx: mpsc::Sender<CoreCommand>,
    completed_rx: mpsc::Receiver<()>,
    sync_operation_manager: Arc<sync::SyncOperationManager>,
    native_bridge: playback::native::NativeBridge,
}

async fn finish_shutdown_with_playback(
    operations: &sync::SyncOperationManager,
    playback: &playback::PlaybackSession,
) -> bool {
    operations.begin_session_checkpoint();
    playback::audio::global().control(playback::model::ControlAction::Stop);
    // Fence/queue the final write, then cancel sync without waiting for SQLite.
    let initial = playback.begin_shutdown_checkpoint();
    operations.commit_shutdown().await;
    let checkpoint = async {
        let mut pending = initial.ok();
        if pending.is_none() {
            operations.finish_session_checkpoint(false);
        }
        loop {
            if let Some(reply) = pending.as_ref() {
                match reply.try_recv() {
                    Ok(result) => {
                        operations.finish_session_checkpoint(result.is_ok());
                        pending = None;
                        if result.is_ok() {
                            return;
                        }
                    }
                    Err(mpsc::TryRecvError::Disconnected) => {
                        operations.finish_session_checkpoint(false);
                        pending = None;
                    }
                    Err(mpsc::TryRecvError::Empty) => {}
                }
            }
            if pending.is_none() && operations.take_checkpoint_retry() {
                pending = playback.begin_shutdown_checkpoint().ok();
                if pending.is_none() {
                    operations.finish_session_checkpoint(false);
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    };
    tokio::join!(checkpoint, operations.wait_for_shutdown_drain());
    let audio_joined =
        tokio::task::spawn_blocking(|| playback::audio::global().stop_and_join()).await;
    let playback = playback.clone();
    let joined = tokio::task::spawn_blocking(move || playback.stop_and_join()).await;
    let cleanups_joined = providers::drain_playback_cleanups().await;
    let succeeded =
        matches!(joined, Ok(Ok(()))) && matches!(audio_joined, Ok(Ok(()))) && cleanups_joined;
    if !succeeded {
        operations.fail_playback_teardown();
    }
    succeeded
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

    run_interactive(&args)
}

/// Starts the core daemon logic (RPC server, device observer, event handling)
/// in a background thread. Returns the shutdown signal and state receiver.
/// The caller is responsible for the main thread's event loop (tray icon or service wait).
pub fn start_daemon_core(
    listener: std::net::TcpListener,
    descriptor: hifimule_lifecycle::OwnerDescriptor,
) -> Result<DaemonCoreHandle> {
    let (state_tx, state_rx) = mpsc::channel::<DaemonState>();
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = Arc::clone(&shutdown);
    let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
    let (command_tx, command_rx) = mpsc::channel::<CoreCommand>();
    let (completed_tx, completed_rx) = mpsc::channel::<()>();
    let sync_operation_manager = Arc::new(sync::SyncOperationManager::new());
    let core_operations = Arc::clone(&sync_operation_manager);
    let native_bridge = playback::native::NativeBridge::default();
    let core_native_bridge = native_bridge.clone();

    // Start Tokio runtime in a background thread
    // REQUIRED for macOS: main thread MUST handle the event loop
    thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_multi_thread()
            .worker_threads(daemon_worker_threads())
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => {
                let _ = ready_tx.send(Err(format!("Failed to build Tokio runtime: {error}")));
                let _ = completed_tx.send(());
                return;
            }
        };

        let rpc_thread = rt.block_on(async {
            daemon_log!("HifiMule Daemon tokio runtime started");

            // Initialize database
            let db_path = match paths::get_app_data_dir() {
                Ok(p) => p.join("hifimule.db"),
                Err(e) => {
                    daemon_log!("Failed to get app data directory: {}", e);
                    let _ = ready_tx.send(Err(format!("Cannot resolve app data: {e}")));
                    let _ = state_tx.send(DaemonState::Error);
                    return None;
                }
            };
            let db = match db::Database::new(db_path) {
                Ok(db) => Arc::new(db),
                Err(e) => {
                    daemon_log!("Failed to initialize database: {}", e);
                    let _ = ready_tx.send(Err(format!("Cannot initialize database: {e}")));
                    let _ = state_tx.send(DaemonState::Error);
                    return None;
                }
            };
            let playback_db = Arc::clone(&db);
            let playback_instance = descriptor.instance_id.clone();
            let playback = match tokio::task::spawn_blocking(move || {
                let session = playback::PlaybackSession::restore(playback_db, playback_instance);
                if let Ok(path) = paths::get_app_data_dir() {session.enable_outputs(path.join("playback.json"));}
                session
            }).await {
                Ok(playback) => playback,
                Err(_) => {
                    let _ = ready_tx.send(Err("Playback owner initialization failed".into()));
                    return None;
                }
            };

            // Seed default device-profiles.json if not present
            let profiles_default = include_bytes!("../assets/device-profiles.json");
            if let Ok(profiles_path) = crate::paths::get_device_profiles_path()
                && let Err(e) = crate::transcoding::ensure_profiles_file_exists(&profiles_path, profiles_default)
            {
                daemon_log!("Warning: Failed to seed device-profiles.json: {}", e);
                // Non-fatal — transcoding will be unavailable until the file exists
            }

            // Initial state
            if let Err(e) = state_tx.send(DaemonState::Idle) {
                daemon_log!("Failed to send initial state: {}", e);
                let _ = ready_tx.send(Err(format!("Cannot initialize daemon state: {e}")));
                return None;
            }

            // Start Device Observer
            let (device_tx, mut device_rx) = tokio::sync::mpsc::channel(10);
            let device_tx_msc = device_tx.clone();
            let msc_observer = tokio::spawn(async move {
                device::run_observer(device_tx_msc).await;
            });

            // Start MTP Observer
            let device_tx_mtp = device_tx.clone();
            let mtp_observer = tokio::spawn(async move {
                device::run_mtp_observer(device_tx_mtp).await;
            });

            // Initialize Device Manager
            let device_manager = Arc::new(device::DeviceManager::new(Arc::clone(&db)));

            // Shared scrobbler result state
            let last_scrobbler_result: Arc<
                tokio::sync::RwLock<Option<scrobbler::ScrobblerResult>>,
            > = Arc::new(tokio::sync::RwLock::new(None));

            // Initialize shared sync operation manager
            let sync_operation_manager = core_operations;

            // Start RPC server
            daemon_log!("Starting RPC server on port {}", descriptor.port);
            let db_clone = Arc::clone(&db);
            let dm_clone = Arc::clone(&device_manager);
            let scrobbler_result_rpc = Arc::clone(&last_scrobbler_result);
            let state_tx_rpc = state_tx.clone();
            let som_rpc = Arc::clone(&sync_operation_manager);
            let playback_rpc = playback.clone();
            let rpc_shutdown = Arc::new(AtomicBool::new(false));
            let rpc_shutdown_server = Arc::clone(&rpc_shutdown);
            let rpc_native_bridge = core_native_bridge.clone();
            // Keep authenticated health on a separate runtime until the device/core
            // runtime (including blocking MTP calls) has actually shut down.
            let rpc_thread = thread::spawn(move || {
                let rpc_runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                    Ok(runtime) => runtime,
                    Err(error) => { let _ = ready_tx.send(Err(error.to_string())); return; }
                };
                if let Err(error) = rpc_runtime.block_on(rpc::run_server(
                    rpc::RpcServerConfig { listener, descriptor, ready_tx, shutdown: rpc_shutdown_server },
                    db_clone, dm_clone, scrobbler_result_rpc, state_tx_rpc, som_rpc, playback_rpc,
                    rpc_native_bridge,
                )) { daemon_log!("RPC server stopped with error: {}", error); }
            });

            // Handle Device Events
            let state_tx_clone = state_tx.clone();
            let jellyfin_client = Arc::new(api::JellyfinClient::new());
            let som_events = Arc::clone(&sync_operation_manager);
            let device_events = tokio::spawn(async move {
                while let Some(event) = device_rx.recv().await {
                    // Removal/failure bookkeeping must continue after admission closes so a
                    // disconnected operation cannot be promoted to success during shutdown.
                    let _event_admission = if matches!(&event, device::DeviceEvent::Removed(_)) {
                        None
                    } else {
                        let Some(guard) = som_events.try_admit_mutation() else {
                            daemon_log!("Ignoring new device work while daemon shutdown is committed");
                            continue;
                        };
                        Some(guard)
                    };
                    match event {
                        device::DeviceEvent::Detected { observation_token, path, manifest, device_io } => {
                            daemon_log!("Device detected at {:?}: {:?}", path, manifest);
                            let auto_sync_enabled = manifest.auto_sync_on_connect;
                            let has_basket = !manifest.basket_items.is_empty();
                            let selected_server_id = db
                                .get_server_config()
                                .ok()
                                .flatten()
                                .and_then(|config| config.server_id);
                            let auto_fill_enabled = manifest
                                .auto_fill
                                .enabled_for(selected_server_id.as_deref());
                            let has_synced_items = !manifest.synced_items.is_empty();
                            let manifest_device_id = manifest.device_id.clone();
                            let scrobble_manifest = Arc::new(manifest.clone());
                            let scrobble_device_io = Arc::clone(&device_io);
                            match device_manager.handle_device_detected_at(observation_token, path.clone(), manifest, device_io).await {
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
                                let scrobbler_result_clone = Arc::clone(&last_scrobbler_result);
                                let scrobble_device_id = manifest_device_id.clone();
                                let scrobble_manifest = Arc::clone(&scrobble_manifest);
                                let scrobble_admission = som_events.try_admit_mutation();
                                tokio::spawn(async move {
                                    let Some(_scrobble_admission) = scrobble_admission else {
                                        return;
                                    };
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

                            // Auto-sync trigger: the connected manifest is the source of truth.
                            // SQLite may be stale or absent when the UI has not opened yet.
                            if auto_sync_enabled && (has_basket || auto_fill_enabled || has_synced_items) {
                                let has_active_sync = som_events.has_active_operation().await;

                                if !has_active_sync {
                                    let dm = Arc::clone(&device_manager);
                                    let som = Arc::clone(&som_events);
                                    let state_tx_sync = state_tx_clone.clone();
                                    let device_id = manifest_device_id.clone();

                                    if let Some((provider, server_id)) = get_selected_provider(&db).await {
                                        tokio::spawn(async move {
                                            daemon_log!("[AutoSync] Starting auto-sync via provider");
                                            if let Err(e) = run_auto_sync_via_provider(
                                                provider, server_id, dm, som, state_tx_sync, device_id,
                                            ).await {
                                                daemon_log!("[AutoSync] Provider auto-sync failed: {}", e);
                                            }
                                        });
                                    } else {
                                        daemon_log!("[AutoSync] Skipped: could not connect to selected server");
                                    }
                                }
                            } else if auto_sync_enabled && !has_basket && !auto_fill_enabled {
                                daemon_log!("[AutoSync] Skipped: auto-sync enabled but no basket items configured");
                            }
                        }
                        device::DeviceEvent::Unrecognized { observation_token, path, device_io, friendly_name } => {
                            println!("Unrecognized device at {:?}", path);
                            let new_state = device_manager.handle_device_unrecognized_at(observation_token, path, device_io, friendly_name).await;
                            let _ = state_tx_clone.send(new_state);
                        }
                        device::DeviceEvent::DiscoveryFailed { path, code, display_name } => {
                            device_manager.report_discovery_failure(path, code, display_name).await;
                            let _ = state_tx_clone.send(DaemonState::Idle);
                        }
                        device::DeviceEvent::Removed(path) => {
                            daemon_log!("Device removed at {:?}", path);
                            let removed_device_id = device_manager.get_device_id_for_path(&path).await;
                            // Close admission before scanning: earlier admissions are fully
                            // published, and later ones cannot target the disconnected device.
                            device_manager.handle_device_removed(&path).await;
                            let targets_removed_device = |device_id: Option<&str>| {
                                removed_device_id
                                    .as_deref()
                                    .is_some_and(|removed| device_id == Some(removed))
                            };
                            let matching_running = som_events.get_all_operations().await.into_iter().any(|op| {
                                op.status == sync::SyncStatus::Running
                                    && targets_removed_device(op.device_id.as_deref())
                            });
                            // Only work admitted against the removed target is failed. An unrelated
                            // destination arrival/removal cannot retarget or cancel active work.
                            if matching_running {
                                daemon_log!("[AutoSync] Device removed during active sync — marking failed");
                                let ops_snapshot = som_events.get_all_operations().await;
                                for mut op in ops_snapshot {
                                    if op.status == sync::SyncStatus::Running
                                        && targets_removed_device(op.device_id.as_deref()) {
                                        op.status = sync::SyncStatus::Failed;
                                        op.errors.push(sync::SyncFileError {
                                            jellyfin_id: String::new(),
                                            filename: String::new(),
                                            error_message: hifimule_i18n::t(
                                                "error.device_removed_during_sync",
                                            ),
                                        });
                                        som_events.request_cancel(&op.id).await;
                                        som_events.update_operation(&op.id.clone(), op).await;
                                    }
                                }
                                drop(tokio::task::spawn_blocking(|| {
                                    if let Err(e) = notifications::new_notification()
                                        .summary(&hifimule_i18n::t("app.name"))
                                        .body(&hifimule_i18n::t(
                                            "notification.sync_interrupted_removed",
                                        ))
                                        .show()
                                    {
                                        daemon_log!("[AutoSync] Notification failed: {}", e);
                                    }
                                }));
                                let _ = state_tx_clone.send(DaemonState::Error);
                            } else {
                                let _ = state_tx_clone.send(DaemonState::Idle);
                            }
                        }
                    }
                }
            });

            // Daemon work loop - check for shutdown signal
            while !shutdown_clone.load(Ordering::Relaxed) {
                while let Ok(command) = command_rx.try_recv() {
                    match command {
                        CoreCommand::BeginShutdown(reply) => {
                            let _ = reply.send(sync_operation_manager.begin_shutdown_fence());
                        }
                        CoreCommand::FenceFailed => sync_operation_manager.fail_shutdown_fence(),
                        CoreCommand::CommitShutdown => {
                            if finish_shutdown_with_playback(&sync_operation_manager, &playback).await {
                                shutdown_clone.store(true, Ordering::Release);
                            }
                        }
                    }
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }

            daemon_log!("HifiMule Daemon shutting down gracefully");
            msc_observer.abort();
            mtp_observer.abort();
            device_events.abort();
            let _ = tokio::join!(msc_observer, mtp_observer, device_events);
            Some((rpc_shutdown, rpc_thread))
        });
        finish_runtime_shutdown(rt, rpc_thread);
        let _ = completed_tx.send(());
    });

    Ok(DaemonCoreHandle {
        shutdown,
        state_rx,
        ready_rx,
        command_tx,
        completed_rx,
        sync_operation_manager,
        native_bridge,
    })
}

fn shutdown_tray_message(snapshot: &sync::ShutdownSnapshot) -> &'static str {
    if snapshot.error_code.as_deref() == Some("PLAYBACK_OWNER_FAILED") {
        return "lifecycle.playback_owner_failed";
    }
    if snapshot.session_checkpoint == sync::SessionCheckpointState::Failed {
        return "lifecycle.playback_checkpoint_failed";
    }
    if snapshot.session_checkpoint == sync::SessionCheckpointState::Pending {
        return "lifecycle.playback_checkpoint_pending";
    }
    match snapshot.phase {
        sync::ShutdownPhase::FenceFailed => "lifecycle.quit_persistence_failed",
        sync::ShutdownPhase::Fencing if snapshot.deadline_exceeded => "lifecycle.fencing_delayed",
        sync::ShutdownPhase::Fencing => "lifecycle.fencing_waiting",
        _ if snapshot.deadline_exceeded => "lifecycle.shutdown_delayed",
        _ => "lifecycle.quitting_waiting",
    }
}

fn finish_runtime_shutdown(
    runtime: tokio::runtime::Runtime,
    rpc_thread: Option<(Arc<AtomicBool>, thread::JoinHandle<()>)>,
) {
    // Runtime Drop joins its blocking pool. Health stays available throughout.
    drop(runtime);
    if let Some((rpc_shutdown, rpc_thread)) = rpc_thread {
        rpc_shutdown.store(true, Ordering::Release);
        if rpc_thread.join().is_err() {
            daemon_log!("RPC runtime panicked during shutdown");
        }
    }
}

fn playback_failure_message(code: &str) -> String {
    let key = format!("playback.error.{code}");
    let translated = hifimule_i18n::t(&key);
    if translated == key {
        hifimule_i18n::t("playback.background_resume_failed")
    } else {
        translated
    }
}

fn send_playback_failure_notification(body: String) {
    thread::spawn(move || {
        if let Err(error) = notifications::new_notification()
            .summary(&hifimule_i18n::t("app.name"))
            .body(&body)
            .show()
        {
            daemon_log!("Playback notification failed: {error}");
        }
    });
}

fn daemon_worker_threads() -> usize {
    std::thread::available_parallelism()
        .map(|cores| cores.get().min(MAX_TOKIO_WORKER_THREADS))
        .unwrap_or(1)
}

fn reject_legacy_endpoint(deadline: Instant) -> Result<()> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        anyhow::bail!("STARTUP_TIMEOUT: legacy check exceeded deadline");
    }
    let address = std::net::SocketAddr::from(([127, 0, 0, 1], 19140));
    match std::net::TcpStream::connect_timeout(&address, remaining.min(Duration::from_millis(300)))
    {
        Err(error) if error.kind() == std::io::ErrorKind::ConnectionRefused => return Ok(()),
        Err(_) => {
            // Some Windows firewall configurations time out a loopback connect
            // even when no process owns the port. A successful exclusive bind is
            // authoritative evidence that the legacy endpoint is available.
            match std::net::TcpListener::bind(address) {
                Ok(listener) => {
                    drop(listener);
                    return Ok(());
                }
                Err(_) => {
                    anyhow::bail!("LEGACY_ENDPOINT_OCCUPIED: cannot verify the legacy endpoint")
                }
            }
        }
        Ok(stream) => drop(stream),
    }
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        anyhow::bail!("STARTUP_TIMEOUT: legacy check exceeded deadline");
    }
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(remaining.min(hifimule_lifecycle::HEALTH_TIMEOUT))
        .build()?;
    let response = client
        .post("http://127.0.0.1:19140")
        .json(&serde_json::json!({"jsonrpc":"2.0","method":"daemon.health","params":{},"id":1}))
        .send()
        .and_then(|response| response.json::<serde_json::Value>());
    if response.is_ok_and(|value| {
        value
            .pointer("/result/data/status")
            .and_then(serde_json::Value::as_str)
            == Some("ok")
    }) {
        anyhow::bail!("LEGACY_DAEMON_RUNNING: close the older HifiMule daemon and retry");
    }
    anyhow::bail!("LEGACY_ENDPOINT_OCCUPIED: another process is using 127.0.0.1:19140");
}

/// Interactive mode: tray icon + event loop on the main thread
fn run_interactive(args: &[String]) -> Result<()> {
    let deadline = Instant::now() + hifimule_lifecycle::STARTUP_DEADLINE;
    let app_data = hifimule_lifecycle::resolve_app_data_dir()?;
    let supplied_generation = args
        .windows(2)
        .find(|pair| pair[0] == "--launch-generation")
        .and_then(|pair| pair[1].parse::<u64>().ok());
    let supplied_attempt = args
        .windows(2)
        .find(|pair| pair[0] == "--launch-attempt")
        .map(|pair| pair[1].clone());
    if supplied_generation.is_some() != supplied_attempt.is_some() {
        anyhow::bail!("DAEMON_STOPPED: launch generation and attempt must be provided together");
    }
    let expected_generation = match supplied_generation {
        Some(value) => value,
        None => hifimule_lifecycle::read_generation(&app_data)?,
    };
    let attempt_id = match supplied_attempt {
        Some(ref attempt_id) => attempt_id.clone(),
        None => hifimule_lifecycle::create_launch_ticket_until(
            &app_data,
            expected_generation,
            deadline,
        )?,
    };
    let result = run_candidate(&app_data, &attempt_id, expected_generation, deadline);
    if let Err(error) = &result {
        let code = error
            .downcast_ref::<hifimule_lifecycle::LifecycleError>()
            .map(|error| error.code())
            .unwrap_or_else(|| {
                let message = error.to_string();
                if message.starts_with("LEGACY_DAEMON_RUNNING") {
                    hifimule_lifecycle::LifecycleErrorCode::LegacyDaemonRunning
                } else if message.starts_with("LEGACY_ENDPOINT_OCCUPIED") {
                    hifimule_lifecycle::LifecycleErrorCode::LegacyEndpointOccupied
                } else if message.starts_with("DAEMON_STOPPED") {
                    hifimule_lifecycle::LifecycleErrorCode::DaemonStopped
                } else if message.starts_with("STARTUP_TIMEOUT") {
                    hifimule_lifecycle::LifecycleErrorCode::StartupTimeout
                } else {
                    hifimule_lifecycle::LifecycleErrorCode::SpawnFailed
                }
            });
        let _ = hifimule_lifecycle::publish_launch_failure(&app_data, &attempt_id, code);
    }
    // UI consumes its attempt outcome before cancellation; direct launch has no reader.
    if supplied_attempt.is_none() {
        let _ = hifimule_lifecycle::cancel_launch_ticket(&app_data, &attempt_id);
        hifimule_lifecycle::clear_launch_failure(&app_data, &attempt_id);
    }
    result
}

fn run_candidate(
    app_data: &std::path::Path,
    attempt_id: &str,
    expected_generation: u64,
    deadline: Instant,
) -> Result<()> {
    hifimule_lifecycle::validate_launch_ticket(app_data, attempt_id, expected_generation)?;
    let mut lifecycle_owner = match hifimule_lifecycle::OwnerGuard::acquire(app_data) {
        Ok(owner) => owner,
        Err(error) if error.code() == hifimule_lifecycle::LifecycleErrorCode::OwnerChanged => {
            // Lock contention never authorizes election again. Confirm the live
            // owner's identity, or return a bounded failure even with stale metadata.
            loop {
                hifimule_lifecycle::validate_launch_ticket(
                    app_data,
                    attempt_id,
                    expected_generation,
                )?;
                if hifimule_lifecycle::read_generation(app_data)? != expected_generation {
                    anyhow::bail!("DAEMON_STOPPED: owner quit while attaching");
                }
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    anyhow::bail!("STARTUP_TIMEOUT: owner did not become ready");
                }
                let descriptor = match hifimule_lifecycle::read_descriptor(app_data) {
                    Ok(descriptor) => Some(descriptor),
                    Err(error)
                        if error.code()
                            == hifimule_lifecycle::LifecycleErrorCode::ProtocolMismatch =>
                    {
                        return Err(error.into());
                    }
                    Err(_) => None,
                };
                if let Some(descriptor) = descriptor {
                    match hifimule_lifecycle::check_owner_health(&descriptor, remaining) {
                        Ok(hifimule_lifecycle::LifecycleState::Ready) => {
                            daemon_log!(
                                "Attached to daemon (pid={}, instance={})",
                                descriptor.pid,
                                descriptor.instance_id
                            );
                            let _ = hifimule_lifecycle::cancel_launch_ticket(app_data, attempt_id);
                            return Ok(());
                        }
                        Ok(_) => anyhow::bail!("DAEMON_STOPPED: owner is stopping"),
                        Err(error) if !error.is_retryable() => {
                            return Err(error.into());
                        }
                        Err(_) => {}
                    }
                }
                thread::sleep(remaining.min(hifimule_lifecycle::POLL_INTERVAL));
            }
        }
        Err(error) => return Err(error.into()),
    };
    lifecycle_owner.verify_expected_generation(expected_generation)?;
    hifimule_lifecycle::validate_launch_ticket(app_data, attempt_id, expected_generation)?;
    reject_legacy_endpoint(deadline)?;
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
    let port = listener.local_addr()?.port();
    let descriptor = lifecycle_owner.prepare_descriptor(port)?;
    hifimule_lifecycle::validate_launch_ticket(app_data, attempt_id, expected_generation)?;
    let core = start_daemon_core(listener, descriptor.clone())?;
    match core
        .ready_rx
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
    {
        Ok(Ok(())) => {
            let launch_fence = hifimule_lifecycle::lock_launch_ticket_until(app_data, deadline)?;
            launch_fence.validate(attempt_id, expected_generation)?;
            lifecycle_owner.publish_descriptor(&descriptor)?;
            drop(launch_fence);
            hifimule_lifecycle::cancel_launch_ticket(app_data, attempt_id)?;
        }
        Ok(Err(error)) => anyhow::bail!("Daemon startup failed: {error}"),
        Err(_) => {
            core.shutdown.store(true, Ordering::Release);
            anyhow::bail!("STARTUP_TIMEOUT: daemon startup exceeded 15 seconds")
        }
    }
    let state_rx = core.state_rx;
    let command_tx = core.command_tx;
    let completed_rx = core.completed_rx;
    let shutdown_operations = core.sync_operation_manager;
    let native_bridge = core.native_bridge;
    let mut lifecycle_owner = Some(lifecycle_owner);
    let mut quit_reply: Option<mpsc::Receiver<sync::ShutdownSnapshot>> = None;
    let mut fence_reply: Option<mpsc::Receiver<Result<u64, String>>> = None;
    let shutdown_app_data = app_data.to_path_buf();
    let mut shutdown_pending = false;
    let mut shutdown_started: Option<Instant> = None;
    let mut shutdown_timeout_reported = false;
    let mut last_shutdown_tray_state = None;

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

    #[cfg(windows)]
    let native_window = {
        use tao::window::WindowBuilder;
        WindowBuilder::new()
            .with_visible(false)
            .with_title("HifiMule media controls")
            .build(&event_loop)
            .map_err(|error| {
                daemon_log!("NATIVE_CONTROLS_UNAVAILABLE: native media window failed: {error}");
            })
            .ok()
    };
    #[cfg(windows)]
    let native_hwnd = {
        use tao::platform::windows::WindowExtWindows;
        native_window
            .as_ref()
            .map(|window| window.hwnd() as *mut std::ffi::c_void)
    };
    #[cfg(not(windows))]
    let native_hwnd = None;
    let native_ingress = native_bridge.ingress();
    let mut native_owner =
        native_ingress.clone().and_then(
            |ingress| match playback::native::NativeMediaOwner::register(ingress, native_hwnd) {
                Ok(owner) => Some(owner),
                Err(error) => {
                    daemon_log!("NATIVE_CONTROLS_UNAVAILABLE: {error}");
                    None
                }
            },
        );

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
    let resume_item = MenuItem::new(hifimule_i18n::t("tray.resume_playback"), false, None);
    let native_status_item = MenuItem::new(
        hifimule_i18n::t(if native_owner.is_some() {
            "playback.native_controls_ready"
        } else {
            "playback.native_controls_unavailable"
        }),
        false,
        None,
    );
    let retry_session_item = MenuItem::new(
        hifimule_i18n::t("lifecycle.retry_saving_session"),
        false,
        None,
    );
    tray_menu
        .append_items(&[
            &open_ui_item,
            &resume_item,
            &native_status_item,
            &retry_session_item,
            &quit_item,
        ])
        .map_err(|e| anyhow::anyhow!("Failed to create tray menu: {}", e))?;

    let mut tray_icon = Some(
        TrayIconBuilder::new()
            .with_menu(Box::new(tray_menu))
            .with_tooltip(hifimule_i18n::t("tray.tooltip.idle"))
            .with_icon((*icon_idle).clone())
            .build()?,
    );

    let menu_channel = MenuEvent::receiver();
    let mut menu_resume_pending: Option<playback::native::NativeCommandReceipt> = None;
    let mut menu_resume_failure: Option<String> = None;

    // 4. Run the event loop
    // This will block the main thread
    event_loop.run(move |_event, _, control_flow| {
        // WaitUntil lets the OS sleep this thread until a native event arrives or the
        // deadline expires. ControlFlow::Poll would spin at 100% CPU when idle.
        *control_flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(250));

        if shutdown_operations.take_quit_retry()
            && !shutdown_pending
            && quit_reply.is_none()
            && fence_reply.is_none()
        {
            let (reply_tx, reply_rx) = mpsc::channel();
            if command_tx
                .send(CoreCommand::BeginShutdown(reply_tx))
                .is_ok()
            {
                quit_reply = Some(reply_rx);
            }
        }

        if let Some(reply) = quit_reply.as_ref() {
            match reply.try_recv() {
                Ok(_snapshot) => {
                    quit_reply = None;
                    rpc::set_lifecycle_stopping(true);
                    let (reply_tx, reply_rx) = mpsc::channel();
                    let app_data = shutdown_app_data.clone();
                    thread::spawn(move || {
                        let result = hifimule_lifecycle::advance_launch_generation(&app_data)
                            .map_err(|error| error.to_string());
                        let _ = reply_tx.send(result);
                    });
                    fence_reply = Some(reply_rx);
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    quit_reply = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }

        if let Some(reply) = fence_reply.as_ref() {
            match reply.try_recv() {
                Ok(Ok(_generation)) => {
                    fence_reply = None;
                    if let Some(owner) = native_owner.as_mut()
                        && let Err(error) = owner.detach()
                    {
                        daemon_log!("Native controls teardown failed: {error}");
                    }
                    native_owner.take();
                    let _ = command_tx.send(CoreCommand::CommitShutdown);
                    shutdown_pending = true;
                    shutdown_started = Some(Instant::now());
                    if let Some(ref mut tray) = tray_icon {
                        let _ =
                            tray.set_tooltip(Some(&hifimule_i18n::t("lifecycle.quitting_waiting")));
                    }
                }
                Ok(Err(error)) => {
                    fence_reply = None;
                    let _ = command_tx.send(CoreCommand::FenceFailed);
                    rpc::set_lifecycle_stopping(false);
                    daemon_log!("QUIT_PERSISTENCE_FAILED: {}", error);
                    if let Some(ref mut tray) = tray_icon {
                        let _ = tray.set_tooltip(Some(&hifimule_i18n::t(
                            "lifecycle.quit_persistence_failed",
                        )));
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    fence_reply = None;
                    let _ = command_tx.send(CoreCommand::FenceFailed);
                    rpc::set_lifecycle_stopping(false);
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }

        if shutdown_pending && completed_rx.try_recv().is_ok() {
            if let Some(owner) = native_owner.as_mut()
                && let Err(error) = owner.detach()
            {
                daemon_log!("Native controls teardown failed: {error}");
            }
            native_owner.take();
            #[cfg(windows)]
            let _ = &native_window;
            lifecycle_owner.take();
            tray_icon.take();
            *control_flow = ControlFlow::Exit;
            return;
        }

        let native_publication_failed = if let Some(owner) = native_owner.as_mut()
            && let Err(error) = owner.refresh()
        {
            daemon_log!("Native controls publication failed: {error}");
            true
        } else {
            false
        };
        if let Some(receipt) = menu_resume_pending.as_mut() {
            match receipt.try_recv() {
                Ok(result) => {
                    menu_resume_pending = None;
                    match result {
                        Ok(()) => menu_resume_failure = None,
                        Err(code) => {
                            send_playback_failure_notification(playback_failure_message(&code));
                            menu_resume_failure = Some(code);
                        }
                    }
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                    menu_resume_pending = None;
                    menu_resume_failure = Some("DAEMON_STOPPED".into());
                    send_playback_failure_notification(playback_failure_message("DAEMON_STOPPED"));
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {}
            }
        }
        if let Some(ingress) = native_ingress.as_ref() {
            let view = ingress.latest();
            if menu_resume_pending.is_none()
                && view.status == playback::model::PlaybackStatus::Active
                && view.failure_code.is_none()
            {
                menu_resume_failure = None;
            }
            resume_item.set_enabled(
                !shutdown_pending && view.commands.play && menu_resume_pending.is_none(),
            );
            if let Some(code) = menu_resume_failure
                .as_deref()
                .or(view.failure_code.as_deref())
            {
                native_status_item.set_text(playback_failure_message(code));
            } else if native_owner.is_some() && !native_publication_failed {
                native_status_item.set_text(hifimule_i18n::t("playback.native_controls_ready"));
            } else {
                native_status_item
                    .set_text(hifimule_i18n::t("playback.native_controls_unavailable"));
            }
        } else {
            resume_item.set_enabled(false);
        }
        if shutdown_pending
            && !shutdown_timeout_reported
            && shutdown_started.is_some_and(|started| started.elapsed() >= Duration::from_secs(5))
        {
            shutdown_timeout_reported = true;
            rpc::set_shutdown_timeout();
            daemon_log!(
                "SHUTDOWN_TIMEOUT: daemon teardown exceeded five seconds; ownership retained"
            );
            if let Some(ref mut tray) = tray_icon {
                let _ = tray.set_tooltip(Some(&hifimule_i18n::t("lifecycle.shutdown_delayed")));
                let _ = tray.set_icon(Some((*icon_error).clone()));
            }
        }

        // Handle state updates from tokio thread
        let shutdown_snapshot = shutdown_operations.shutdown_tray_snapshot();
        retry_session_item.set_enabled(
            shutdown_snapshot
                .as_ref()
                .is_some_and(|s| s.session_checkpoint == sync::SessionCheckpointState::Failed),
        );
        if let Some(snapshot) = shutdown_snapshot.as_ref() {
            let tray_state = (
                snapshot.shutdown_id.clone(),
                snapshot.phase,
                snapshot.deadline_exceeded,
                snapshot.session_checkpoint,
                snapshot.error_code.clone(),
            );
            if last_shutdown_tray_state.as_ref() != Some(&tray_state) {
                if let Some(ref mut tray) = tray_icon {
                    let key = shutdown_tray_message(snapshot);
                    let _ = tray.set_tooltip(Some(&hifimule_i18n::t(key)));
                    if snapshot.deadline_exceeded
                        || snapshot.session_checkpoint == sync::SessionCheckpointState::Failed
                        || snapshot.phase == sync::ShutdownPhase::FenceFailed
                    {
                        let _ = tray.set_icon(Some((*icon_error).clone()));
                    }
                }
                last_shutdown_tray_state = Some(tray_state);
            }
        }
        if !shutdown_pending
            && shutdown_snapshot.is_none()
            && let Ok(state) = state_rx.try_recv()
            && let Some(ref mut tray) = tray_icon
        {
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

        // Handle menu events (Quit, Open UI)
        if let Ok(event) = menu_channel.try_recv() {
            if event.id == quit_item.id() {
                if !shutdown_pending && quit_reply.is_none() && fence_reply.is_none() {
                    let (reply_tx, reply_rx) = mpsc::channel();
                    if command_tx
                        .send(CoreCommand::BeginShutdown(reply_tx))
                        .is_ok()
                    {
                        quit_reply = Some(reply_rx);
                    }
                }
            } else if event.id == retry_session_item.id() {
                if let Some(snapshot) = shutdown_operations.shutdown_tray_snapshot() {
                    shutdown_operations.request_checkpoint_retry(&snapshot.shutdown_id);
                }
            } else if event.id == resume_item.id() {
                if menu_resume_pending.is_none()
                    && let Some(ingress) = native_ingress.as_ref()
                {
                    match ingress.try_send_tracked(playback::NativeControlIntent::Play) {
                        Ok(receipt) => menu_resume_pending = Some(receipt),
                        Err(_) => {
                            let code = "RESUME_UNAVAILABLE";
                            menu_resume_failure = Some(code.into());
                            let message = playback_failure_message(code);
                            native_status_item.set_text(&message);
                            send_playback_failure_notification(message);
                        }
                    }
                }
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
fn push_auto_fill_items(
    items: Vec<auto_fill::AutoFillItem>,
    media_role: device::MediaRole,
    desired_items: &mut Vec<sync::DesiredItem>,
    playlist_sync_items: &mut Vec<sync::PlaylistSyncItem>,
) {
    let mut seen_ids: std::collections::HashSet<String> = desired_items
        .iter()
        .map(|item| item.jellyfin_id.clone())
        .collect();
    let mut tracks = Vec::new();
    for item in items {
        if seen_ids.insert(item.id.clone()) {
            if media_role == device::MediaRole::Music {
                tracks.push(sync::PlaylistTrackInfo {
                    jellyfin_id: item.id.clone(),
                    artist: item.artist.clone(),
                    run_time_seconds: -1,
                });
            }
            desired_items.push(sync::DesiredItem {
                media_role,
                jellyfin_id: item.id,
                name: item.name,
                album: item.album,
                artist: item.artist,
                size_bytes: item.size_bytes,
                etag: None,
                provider_album_id: item.provider_album_id,
                provider_content_type: item.provider_content_type,
                provider_suffix: item.provider_suffix,
                original_bitrate: None,
                track_number: item.track_number,
                server_id: None,
            });
        }
    }
    if !tracks.is_empty() {
        playlist_sync_items.push(sync::PlaylistSyncItem {
            jellyfin_id: "__hifimule_autofill".to_string(),
            name: "Autofill".to_string(),
            tracks,
        });
    }
}

fn auto_sync_delta_has_work(delta: &sync::SyncDelta, total_files: usize) -> bool {
    total_files > 0 || !delta.id_changes.is_empty() || !delta.playlists.is_empty()
}

#[cfg(test)]
mod auto_sync_tests {
    use super::*;

    fn fill_item(id: &str, track_number: Option<u32>) -> auto_fill::AutoFillItem {
        auto_fill::AutoFillItem {
            id: id.to_string(),
            name: id.to_string(),
            album: None,
            artist: Some("artist".to_string()),
            provider_album_id: None,
            provider_content_type: None,
            provider_suffix: None,
            track_number,
            size_bytes: 100,
            priority_reason: "test".to_string(),
            tier: None,
        }
    }

    #[test]
    fn push_auto_fill_items_dedups_and_writes_playlist() {
        let mut desired_items = vec![sync::DesiredItem {
            media_role: crate::device::MediaRole::Music,
            jellyfin_id: "manual".to_string(),
            name: "manual".to_string(),
            album: None,
            artist: None,
            size_bytes: 10,
            etag: None,
            provider_album_id: None,
            provider_content_type: None,
            provider_suffix: None,
            original_bitrate: None,
            track_number: None,
            server_id: None,
        }];
        let mut playlists = Vec::new();

        push_auto_fill_items(
            vec![fill_item("manual", Some(1)), fill_item("fill", Some(9))],
            device::MediaRole::Music,
            &mut desired_items,
            &mut playlists,
        );

        assert_eq!(desired_items.len(), 2);
        assert_eq!(desired_items[1].jellyfin_id, "fill");
        assert_eq!(desired_items[1].track_number, Some(9));
        assert_eq!(playlists.len(), 1);
        assert_eq!(playlists[0].name, "Autofill");
        assert_eq!(playlists[0].tracks.len(), 1);
        assert_eq!(playlists[0].tracks[0].jellyfin_id, "fill");
    }

    #[test]
    fn podcast_auto_fill_items_keep_role_without_music_playlist() {
        let mut desired_items = Vec::new();
        let mut playlists = Vec::new();
        push_auto_fill_items(
            vec![fill_item("episode", Some(20260102))],
            device::MediaRole::Podcast,
            &mut desired_items,
            &mut playlists,
        );
        assert_eq!(desired_items[0].media_role, device::MediaRole::Podcast);
        assert_eq!(desired_items[0].track_number, Some(20260102));
        assert!(playlists.is_empty());
    }

    #[test]
    fn auto_sync_uses_selected_server_pipeline() {
        let mut config = device::AutoFillConfig::default();
        let mut first = auto_fill::AutoFillPipeline::default_legacy(None);
        first.podcast_retention.recent_count = 2;
        let mut second = auto_fill::AutoFillPipeline::default_legacy(None);
        second.podcast_retention.recent_count = 7;
        config.pipelines.insert("first".into(), first);
        config.pipelines.insert("second".into(), second);
        assert_eq!(
            config
                .resolve_pipeline(Some("second"))
                .unwrap()
                .podcast_retention
                .recent_count,
            7
        );
        assert!(config.enabled_for(Some("second")));
    }

    #[test]
    fn auto_sync_delta_has_work_when_only_playlist_changes() {
        let delta = sync::SyncDelta {
            blocked: vec![],
            adds: vec![],
            deletes: vec![],
            id_changes: vec![],
            unchanged: 1,
            playlists: vec![sync::PlaylistSyncItem {
                jellyfin_id: "__hifimule_autofill".to_string(),
                name: "Autofill".to_string(),
                tracks: vec![sync::PlaylistTrackInfo {
                    jellyfin_id: "fill".to_string(),
                    artist: None,
                    run_time_seconds: -1,
                }],
            }],
            pity_fired_servers: vec![],
        };

        assert!(auto_sync_delta_has_work(&delta, 0));
    }
}

/// Returns the selected server's provider from its stored credentials.
async fn get_selected_provider(
    db: &Arc<db::Database>,
) -> Option<(Arc<dyn providers::MediaProvider>, Option<String>)> {
    let config = db.get_server_config().ok()??;
    let server_id = config.server_id.clone();
    let provider = crate::server_manager::connect_provider_for(&config.into())
        .await
        .ok()?;
    Some((provider, server_id))
}

fn scoped_favorite_target_id<'a>(basket_item: &'a device::BasketItem, prefix: &str) -> &'a str {
    basket_item
        .id
        .strip_prefix(prefix)
        .unwrap_or(&basket_item.id)
}

/// Provider-based auto-sync: resolves basket items via MediaProvider, runs auto-fill if needed,
/// then executes sync via execute_provider_sync.
async fn run_auto_sync_via_provider(
    provider: Arc<dyn providers::MediaProvider>,
    server_id: Option<String>,
    device_manager: Arc<device::DeviceManager>,
    sync_op_manager: Arc<sync::SyncOperationManager>,
    state_tx: std::sync::mpsc::Sender<DaemonState>,
    device_id: String,
) -> anyhow::Result<()> {
    let _ = state_tx.send(DaemonState::Syncing);

    // Claim the pipeline lock so concurrent manual syncs or a second auto-sync
    // cannot run their auto-fill in parallel while we are calculating the delta.
    let _pipeline_guard = sync_op_manager
        .try_start_pipeline()
        .ok_or_else(|| anyhow::anyhow!("[AutoSync] Aborting: sync pipeline already active"))?;

    let target: sync::SyncTarget = device_manager
        .get_sync_target_for_device(&device_id)
        .await
        .ok_or_else(|| anyhow::anyhow!("Auto-sync device disconnected"))?
        .into();
    let manifest = &target.manifest;

    let mut desired_items: Vec<sync::DesiredItem> = Vec::new();
    let mut playlist_sync_items: Vec<sync::PlaylistSyncItem> = Vec::new();

    if manifest.basket_items.is_empty() && !manifest.auto_fill.enabled_for(server_id.as_deref()) {
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
    if manifest.auto_fill.enabled_for(server_id.as_deref()) {
        let synced_bytes: u64 = manifest.synced_items.iter().map(|s| s.size_bytes).sum();
        let total_budget = if let Some(mb) = manifest.auto_fill.max_bytes_for(server_id.as_deref())
        {
            mb
        } else {
            match target.io.free_space().await {
                Ok(free_bytes) => free_bytes.saturating_add(synced_bytes),
                Err(_) => {
                    daemon_log!("[AutoSync] Cannot determine device capacity for auto-fill");
                    let _ = state_tx.send(DaemonState::Idle);
                    return Ok(());
                }
            }
        };
        let basket_size: u64 = desired_items.iter().map(|i| i.size_bytes).sum();
        let auto_fill_budget = total_budget.saturating_sub(basket_size);
        if auto_fill_budget > 0 {
            let exclude_item_ids: Vec<String> = desired_items
                .iter()
                .map(|i| i.jellyfin_id.clone())
                .collect();
            let fill_params = auto_fill::AutoFillParams {
                exclude_item_ids,
                max_fill_bytes: auto_fill_budget,
                device_id: manifest.device_id.clone(),
                server_id: server_id.clone().unwrap_or_default(),
                now_unix: crate::rpc::now_unix_secs(),
                history: auto_fill::HistorySnapshot::default(),
                rotation_cursor: 0,
                seed: 0,
                pity_streak: 0,
                // Story 13.5: legacy auto-sync path — civil time inert (Context stage never runs here).
                local: auto_fill::CivilTime::default(),
            };
            let pipeline = manifest.auto_fill.resolve_pipeline(server_id.as_deref());
            match rpc::expand_auto_fill_slot(provider.clone(), pipeline, fill_params).await {
                Ok(items) if items.is_empty() && desired_items.is_empty() => {
                    daemon_log!("[AutoSync] Provider auto-fill returned no items, skipping");
                    let _ = state_tx.send(DaemonState::Idle);
                    return Ok(());
                }
                Ok(items) => {
                    daemon_log!(
                        "[AutoSync] Provider auto-fill resolved {} items",
                        items.len()
                    );
                    push_auto_fill_items(
                        items,
                        device::MediaRole::from_library_role(provider.library_role()),
                        &mut desired_items,
                        &mut playlist_sync_items,
                    );
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
    let media_role = device::MediaRole::from_library_role(provider.library_role());
    for item in &mut desired_items {
        item.media_role = media_role;
    }

    if desired_items.is_empty() && !manifest.basket_items.is_empty() {
        daemon_log!("[AutoSync] No downloadable items resolved from basket, skipping");
        let _ = state_tx.send(DaemonState::Idle);
        return Ok(());
    }

    let mut delta = sync::calculate_delta(&desired_items, manifest);
    delta.playlists = playlist_sync_items;
    let total_files = delta.adds.len() + delta.deletes.len();

    if !auto_sync_delta_has_work(&delta, total_files) {
        daemon_log!("[AutoSync] Device already in sync, nothing to do");
        let _ = state_tx.send(DaemonState::Idle);
        return Ok(());
    }
    if sync_op_manager.is_pipeline_cancelled() {
        daemon_log!("[AutoSync] Cancelled before sync execution");
        let _ = state_tx.send(DaemonState::Idle);
        return Ok(());
    }
    let destructive_cleanup_count = sync::destructive_cleanup_count(&delta, manifest);
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
    device_manager
        .admit_sync_operation(
            &sync_op_manager,
            operation_id.clone(),
            total_files,
            &manifest.device_id,
        )
        .await?;

    let pending_ids: Vec<String> = delta
        .adds
        .iter()
        .map(|a| a.jellyfin_id.clone())
        .chain(delta.id_changes.iter().map(|c| c.new_jellyfin_id.clone()))
        .collect();

    if let Err(error) = device_manager
        .update_manifest_for_device(&manifest.device_id, |m| {
            m.dirty = true;
            m.pending_item_ids = pending_ids;
        })
        .await
    {
        if let Some(mut operation) = sync_op_manager.get_operation(&operation_id).await {
            operation.status = sync::SyncStatus::Failed;
            operation.errors.push(sync::SyncFileError {
                jellyfin_id: String::new(),
                filename: ".hifimule.json".into(),
                error_message: format!("Failed to mark manifest dirty: {error}"),
            });
            sync_op_manager
                .update_operation(&operation_id, operation)
                .await;
        }
        return Err(error);
    }

    let transcoding_profile = if let Some(ref profile_id) = manifest.transcoding_profile_id {
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
        &target,
        sync::ProviderSyncSource {
            provider,
            transcoding_profile,
            providers_by_server: std::collections::HashMap::new(),
        },
        sync_op_manager.clone(),
        operation_id.clone(),
        device_manager.clone(),
    )
    .await;

    match result {
        Ok((_synced_items, errors)) => {
            let (outcome, final_errors) = sync_op_manager
                .finalize_operation(&operation_id, errors, || async {
                    device_manager
                        .update_manifest_for_device(&manifest.device_id, |m| {
                            m.dirty = false;
                            m.pending_item_ids.clear();
                        })
                        .await
                })
                .await;
            if outcome == sync::SyncStatus::Complete {
                daemon_log!("[AutoSync] Sync completed successfully");
                drop(tokio::task::spawn_blocking(|| {
                    if let Err(e) = notifications::new_notification()
                        .summary(&hifimule_i18n::t("app.name"))
                        .body(&hifimule_i18n::t("notification.sync_complete_safe"))
                        .show()
                    {
                        daemon_log!("[AutoSync] Notification failed: {}", e);
                    }
                }));
                let _ = state_tx.send(DaemonState::Idle);
            } else if outcome == sync::SyncStatus::Failed {
                daemon_log!(
                    "[AutoSync] Sync interrupted with {} errors",
                    final_errors.len()
                );
                let error_msg = format!("Sync failed with {} error(s)", final_errors.len());
                drop(tokio::task::spawn_blocking(move || {
                    if let Err(e) = notifications::new_notification()
                        .summary("HifiMule")
                        .body(&error_msg)
                        .show()
                    {
                        daemon_log!("[AutoSync] Notification failed: {}", e);
                    }
                }));
                let _ = state_tx.send(DaemonState::Error);
            } else {
                daemon_log!("[AutoSync] Sync cancelled; recovery evidence retained");
                let _ = state_tx.send(DaemonState::Idle);
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
                sync_op_manager
                    .update_operation(&operation_id, operation)
                    .await;
            }
            let error_msg = format!("Sync failed: {}", e);
            drop(tokio::task::spawn_blocking(move || {
                if let Err(e) = notifications::new_notification()
                    .summary("HifiMule")
                    .body(&error_msg)
                    .show()
                {
                    daemon_log!("[AutoSync] Notification failed: {}", e);
                }
            }));
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

    let (favorite_items, normal_items): (Vec<_>, Vec<_>) = basket_items
        .iter()
        .filter(|b| !device::is_auto_fill_slot_id(&b.id))
        .partition(|b| matches!(b.item_type.as_str(), "FavoriteArtist" | "FavoriteAlbum"));

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
                daemon_log!(
                    "[AutoSync] Failed to resolve favorite item {}: {}",
                    basket_item.id,
                    e
                );
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
                daemon_log!(
                    "[AutoSync] Failed to resolve item {}: {}",
                    basket_item.id,
                    e
                );
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
                        desired_items.extend(
                            album_with_tracks
                                .tracks
                                .iter()
                                .map(provider_song_to_desired),
                        );
                    }
                    Err(e) => {
                        daemon_log!(
                            "[AutoSync] Failed to expand favorite album {}: {}",
                            album.id,
                            e
                        );
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
        return Ok((
            album.tracks.iter().map(provider_song_to_desired).collect(),
            None,
        ));
    }

    if let Ok(playlist) = provider.get_playlist(item_id).await {
        let tracks = playlist
            .tracks
            .iter()
            .map(provider_song_to_desired)
            .collect::<Vec<_>>();
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
                    tracks.extend(
                        album_with_tracks
                            .tracks
                            .iter()
                            .map(provider_song_to_desired),
                    );
                }
                Err(e) => {
                    daemon_log!(
                        "[AutoSync] Failed to expand artist album {}: {}",
                        album.id,
                        e
                    );
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
        media_role: crate::device::MediaRole::Music,
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
        server_id: None,
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

#[cfg(test)]
mod lifecycle_shutdown_tests {
    use super::*;

    #[tokio::test]
    async fn playback_checkpoint_failure_surfaces_and_retry_preserves_shutdown_contract() {
        let db = Arc::new(db::Database::memory().unwrap());
        let playback = playback::PlaybackSession::restore(db.clone(), "owner".into());
        let initial = playback.snapshot().unwrap();
        playback
            .apply(playback::model::ApplySessionParams {
                schema_version: 1,
                instance_id: initial.instance_id.clone(),
                session_id: initial.session_id.clone(),
                command_id: uuid::Uuid::new_v4().to_string(),
                expected_queue_revision: initial.queue_revision.clone(),
                operation: playback::model::SessionOperation::ReplaceQueue {
                    sources: vec![playback::model::TrackSource {
                        server_id: "offline".into(),
                        track_id: "track".into(),
                    }],
                },
            })
            .unwrap();
        let active = playback.snapshot().unwrap();
        playback
            .report_progress(
                &active.generation_id,
                &active.current.as_ref().unwrap().occurrence_id,
                1,
                2500,
            )
            .unwrap();
        db.conn
            .lock()
            .unwrap()
            .execute_batch(
                "CREATE TEMP TRIGGER fail_shutdown_checkpoint
                 BEFORE UPDATE OF position_ms ON playback_sessions
                 BEGIN SELECT RAISE(ABORT, 'injected shutdown checkpoint failure'); END;",
            )
            .unwrap();

        let operations = Arc::new(sync::SyncOperationManager::new());
        let pipeline = operations.try_start_pipeline().unwrap();
        let before = operations.begin_shutdown_fence();
        let task_operations = operations.clone();
        let task_playback = playback.clone();
        let task = tokio::spawn(async move {
            finish_shutdown_with_playback(&task_operations, &task_playback).await
        });
        let failed = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let snapshot = operations.shutdown_snapshot().await.unwrap();
                if snapshot.session_checkpoint == sync::SessionCheckpointState::Failed {
                    break snapshot;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            failed.error_code.as_deref(),
            Some("PLAYBACK_CHECKPOINT_FAILED")
        );
        assert_eq!(failed.shutdown_id, before.shutdown_id);
        assert!(operations.is_pipeline_cancelled());
        assert!(operations.try_admit_mutation().is_none());
        assert!(!operations.request_quit_retry());
        assert!(!task.is_finished());
        assert!(
            failed
                .blockers
                .iter()
                .any(|b| b.reason == "sessionCheckpoint")
        );
        assert!(!operations.request_checkpoint_retry("another-shutdown"));
        db.conn
            .lock()
            .unwrap()
            .execute_batch("DROP TRIGGER fail_shutdown_checkpoint")
            .unwrap();
        assert!(operations.request_checkpoint_retry(&failed.shutdown_id));
        assert!(operations.request_checkpoint_retry(&failed.shutdown_id));
        drop(pipeline);
        assert!(
            tokio::time::timeout(Duration::from_secs(2), task)
                .await
                .unwrap()
                .unwrap()
        );
        let finished = operations.shutdown_snapshot().await.unwrap();
        assert_eq!(finished.shutdown_id, before.shutdown_id);
        assert_eq!(finished.deadline_ms, before.deadline_ms);
        assert_eq!(
            finished.session_checkpoint,
            sync::SessionCheckpointState::Succeeded
        );
        assert!(operations.try_admit_mutation().is_none());
        assert_eq!(
            db.load_playback_session().unwrap().unwrap().position_ms,
            2500
        );
        assert!(
            playback.snapshot().is_err(),
            "playback worker must have been joined"
        );
    }

    #[tokio::test]
    async fn playback_join_failure_is_not_advertised_as_a_checkpoint_retry() {
        let operations = sync::SyncOperationManager::new();
        let before = operations.begin_shutdown_fence();
        operations.begin_session_checkpoint();
        operations.commit_shutdown().await;
        assert!(
            !operations.request_checkpoint_retry(&before.shutdown_id),
            "the original pending save is not a retry attempt"
        );
        operations.finish_session_checkpoint(true);
        operations.fail_playback_teardown();
        let snapshot = operations.shutdown_snapshot().await.unwrap();
        assert_eq!(
            snapshot.error_code.as_deref(),
            Some("PLAYBACK_OWNER_FAILED")
        );
        assert_eq!(
            snapshot.session_checkpoint,
            sync::SessionCheckpointState::Succeeded
        );
        assert!(!operations.request_checkpoint_retry(&before.shutdown_id));
        assert!(!operations.request_quit_retry());
        assert!(operations.try_admit_mutation().is_none());
    }

    #[tokio::test]
    async fn stalled_playback_checkpoint_does_not_delay_sync_cancellation_or_health() {
        let db = Arc::new(db::Database::memory().unwrap());
        let playback = playback::PlaybackSession::restore(db.clone(), "owner".into());
        let s = playback.snapshot().unwrap();
        playback
            .apply(playback::model::ApplySessionParams {
                schema_version: 1,
                instance_id: s.instance_id,
                session_id: s.session_id,
                command_id: uuid::Uuid::new_v4().to_string(),
                expected_queue_revision: s.queue_revision,
                operation: playback::model::SessionOperation::ReplaceQueue {
                    sources: vec![playback::model::TrackSource {
                        server_id: "s".into(),
                        track_id: "t".into(),
                    }],
                },
            })
            .unwrap();
        let s = playback.snapshot().unwrap();
        playback
            .report_progress(&s.generation_id, &s.current.unwrap().occurrence_id, 1, 1000)
            .unwrap();
        let operations = Arc::new(sync::SyncOperationManager::new());
        let pipeline = operations.try_start_pipeline().unwrap();
        operations.begin_shutdown_fence();
        let (locked_tx, locked_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let stalled_db = db.clone();
        let holder = std::thread::spawn(move || {
            let _connection = stalled_db.conn.lock().unwrap();
            locked_tx.send(()).unwrap();
            release_rx.recv().unwrap();
        });
        locked_rx.recv().unwrap();
        let task_operations = operations.clone();
        let task_playback = playback.clone();
        let task = tokio::spawn(async move {
            finish_shutdown_with_playback(&task_operations, &task_playback).await
        });
        tokio::time::timeout(Duration::from_secs(2), async {
            while !operations.is_pipeline_cancelled() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(!task.is_finished());
        assert_eq!(playback.health().restoration.status, "ok");
        assert_eq!(
            operations
                .shutdown_snapshot()
                .await
                .unwrap()
                .session_checkpoint,
            sync::SessionCheckpointState::Pending
        );
        release_tx.send(()).unwrap();
        holder.join().unwrap();
        drop(pipeline);
        assert!(
            tokio::time::timeout(Duration::from_secs(2), task)
                .await
                .unwrap()
                .unwrap()
        );
    }

    #[test]
    fn teardown_completion_waits_for_blocking_device_work() {
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (complete_tx, complete_rx) = mpsc::channel();
        let rpc_shutdown = Arc::new(AtomicBool::new(false));
        let flag = rpc_shutdown.clone();
        let thread = thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .enable_all()
                .build()
                .unwrap();
            runtime.spawn_blocking(move || {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
            });
            let rpc_flag = flag.clone();
            let rpc = thread::spawn(move || {
                while !rpc_flag.load(Ordering::Acquire) {
                    thread::sleep(Duration::from_millis(5));
                }
            });
            finish_runtime_shutdown(runtime, Some((flag, rpc)));
            complete_tx.send(()).unwrap();
        });
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(complete_rx.recv_timeout(Duration::from_millis(75)).is_err());
        assert!(
            !rpc_shutdown.load(Ordering::Acquire),
            "health must remain available during core teardown"
        );
        release_tx.send(()).unwrap();
        complete_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        thread.join().unwrap();
    }

    #[test]
    fn teardown_completion_does_not_wait_for_passive_mtp_discovery() {
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let release_rx = Arc::new(std::sync::Mutex::new(release_rx));
        let (complete_tx, complete_rx) = mpsc::channel();
        let thread = thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .enable_all()
                .build()
                .unwrap();
            let (event_tx, _event_rx) = tokio::sync::mpsc::channel(1);
            let release_for_enumerator = Arc::clone(&release_rx);
            let observer = runtime.spawn(device::run_mtp_observer_with_enumerator(
                event_tx,
                std::sync::Arc::new(move || {
                    started_tx.send(()).unwrap();
                    release_for_enumerator.lock().unwrap().recv().unwrap();
                    Ok(Vec::new())
                }),
                Duration::from_secs(2),
            ));
            started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
            // This mirrors core shutdown: abort the observer while it awaits a
            // non-cancellable passive scan, then drop the core runtime.
            observer.abort();
            let _ = runtime.block_on(observer);
            finish_runtime_shutdown(runtime, None);
            complete_tx.send(()).unwrap();
        });
        complete_rx
            .recv_timeout(Duration::from_millis(250))
            .expect("core teardown must not join passive MTP discovery");
        release_tx.send(()).unwrap();
        thread.join().unwrap();
    }

    #[test]
    fn duplicate_launch_cannot_succeed_from_stale_descriptor() {
        let temp = tempfile::tempdir().unwrap();
        let mut owner = hifimule_lifecycle::OwnerGuard::acquire(temp.path()).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        owner
            .publish_ready(listener.local_addr().unwrap().port())
            .unwrap();
        // Keep the port bound but never answer health: metadata alone is not readiness.
        let ticket = hifimule_lifecycle::create_launch_ticket(temp.path(), 0).unwrap();
        let result = run_candidate(
            temp.path(),
            &ticket,
            0,
            Instant::now() + Duration::from_millis(100),
        );
        assert!(result.is_err());
    }
}
