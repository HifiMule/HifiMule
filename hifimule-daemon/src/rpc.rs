use crate::api::{CredentialManager, JellyfinClient};
use crate::domain::models::{Album, Artist, ChangeType, ItemType, Library, Playlist, Song};
use crate::providers::{
    BrowseMode, CredentialKind, MediaProvider, ProviderCredentials, ProviderError,
    SUBSONIC_PLAYLISTS_LIBRARY_ID, ServerType, ServerTypeHint, TrackListFilter, server_type_slug,
};
use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Path, State},
    http::{Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use rand::RngCore;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering as AtomicOrdering};

// JSON-RPC 2.0 Error Codes
const ERR_METHOD_NOT_FOUND: i32 = -32601;
const ERR_INVALID_PARAMS: i32 = -32602;
const ERR_INTERNAL_ERROR: i32 = -32603;

// Application-specific error codes
const ERR_CONNECTION_FAILED: i32 = -1;
#[allow(dead_code)] // Reserved for future use
const ERR_INVALID_CREDENTIALS: i32 = -2;
const ERR_STORAGE_ERROR: i32 = -3;
const ERR_NOT_FOUND: i32 = -4;
const ERR_UNSUPPORTED_CAPABILITY: i32 = -5;
const ERR_SYNC_IN_PROGRESS: i32 = -6;
/// A playlist operation referenced items from a server other than the selected
/// one (Story 2.11 AC33). JSON-RPC negative code; "cross-server" conveyed via msg.
const ERR_CROSS_SERVER_CONFLICT: i32 = -7;
/// The selected server's stored credential is expired/invalid (Story 2.11 AC11).
/// Distinct from generic connection failures so the UI can scope a re-auth prompt.
const ERR_UNAUTHORIZED: i32 = -8;
const ERR_SYNC_CANCELLED: i32 = -9;
const AUDIOBOOKSHELF_SETUP_TTL: std::time::Duration = std::time::Duration::from_secs(5 * 60);
const MAX_PENDING_AUDIOBOOKSHELF_SETUPS: usize = 8;
const JELLYFIN_TICKS_PER_SECOND: u64 = 10_000_000;
const GENRE_TRACK_PAGE_SIZE: u32 = 500;
const GENRE_TRACK_MAX_PAGES: u32 = 200;
const SERVER_NAME_MAX_LEN: usize = 40;
const SERVER_ICON_IDS: &[&str] = &[
    "hdd-network",
    "server",
    "music-note-list",
    "music-note-beamed",
    "headphones",
    "collection-play",
    "disc",
    "broadcast-pin",
    "book",
];

fn sync_cancelled_error() -> JsonRpcError {
    JsonRpcError {
        code: ERR_SYNC_CANCELLED,
        message: "Sync cancelled".to_string(),
        data: None,
    }
}

#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    #[allow(dead_code)] // Required by JSON-RPC 2.0 spec but not used in handler
    pub jsonrpc: String,
    pub method: String,
    pub params: Option<Value>,
    pub id: Value,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub result: Option<Value>,
    pub error: Option<JsonRpcError>,
    pub id: Value,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    pub data: Option<Value>,
}

pub struct AppState {
    pub jellyfin_client: JellyfinClient,
    /// Multi-server runtime state (Story 2.11): replaces the former single
    /// `provider`/`server_type`/`server_version` fields.
    pub server_manager: Arc<tokio::sync::RwLock<crate::server_manager::ServerManager>>,
    pub db: Arc<crate::db::Database>,
    pub device_manager: Arc<crate::device::DeviceManager>,
    pub last_connection_check: Arc<tokio::sync::Mutex<Option<(std::time::Instant, bool)>>>,
    pub size_cache: Arc<tokio::sync::RwLock<HashMap<String, u64>>>,
    pub sync_operation_manager: Arc<crate::sync::SyncOperationManager>,
    pub last_scrobbler_result: Arc<tokio::sync::RwLock<Option<crate::scrobbler::ScrobblerResult>>>,
    pub state_tx: std::sync::mpsc::Sender<crate::DaemonState>,
    pub playback: crate::playback::PlaybackSession,
    #[cfg(not(test))]
    pub pending_audiobookshelf_setups:
        Arc<tokio::sync::Mutex<HashMap<String, PendingAudiobookshelfSetup>>>,
}

impl AppState {
    fn pending_audiobookshelf_setups(
        &self,
    ) -> Arc<tokio::sync::Mutex<HashMap<String, PendingAudiobookshelfSetup>>> {
        #[cfg(not(test))]
        {
            self.pending_audiobookshelf_setups.clone()
        }
        #[cfg(test)]
        {
            type SetupMap = Arc<tokio::sync::Mutex<HashMap<String, PendingAudiobookshelfSetup>>>;
            type TestSetupEntry = (std::sync::Weak<crate::db::Database>, SetupMap);
            static TEST_SETUPS: OnceLock<std::sync::Mutex<HashMap<usize, TestSetupEntry>>> =
                OnceLock::new();
            let state_key = Arc::as_ptr(&self.db) as usize;
            let mut registry = TEST_SETUPS
                .get_or_init(|| std::sync::Mutex::new(HashMap::new()))
                .lock()
                .expect("test setup registry lock");
            if let Some((owner, setups)) = registry.get(&state_key)
                && owner
                    .upgrade()
                    .is_some_and(|owner| Arc::ptr_eq(&owner, &self.db))
            {
                return setups.clone();
            }
            let setups = Arc::new(tokio::sync::Mutex::new(HashMap::new()));
            registry.insert(state_key, (Arc::downgrade(&self.db), setups.clone()));
            setups
        }
    }
}

pub(crate) struct PendingAudiobookshelfSetup {
    created_at: std::time::Instant,
    url: String,
    username: String,
    password: SecretString,
    provider: crate::providers::audiobookshelf::AudiobookshelfProvider,
    choices: HashMap<String, crate::providers::audiobookshelf::DiscoveredLibrary>,
}

impl std::fmt::Debug for PendingAudiobookshelfSetup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingAudiobookshelfSetup")
            .field("created_at", &self.created_at)
            .field("url", &"[redacted-url]")
            .field("username", &"[redacted]")
            .field("password", &"[redacted]")
            .field("provider", &"[redacted]")
            .field("choice_count", &self.choices.len())
            .finish()
    }
}

static LIFECYCLE_IDENTITY: OnceLock<hifimule_lifecycle::OwnerDescriptor> = OnceLock::new();
static LIFECYCLE_STATE: AtomicU8 = AtomicU8::new(0);
static ACTIVE_LOCAL_REQUESTS: AtomicUsize = AtomicUsize::new(0);
struct LocalRequestGuard;
impl Drop for LocalRequestGuard {
    fn drop(&mut self) {
        ACTIVE_LOCAL_REQUESTS.fetch_sub(1, AtomicOrdering::AcqRel);
    }
}
pub fn set_shutdown_timeout() {
    LIFECYCLE_STATE.store(3, AtomicOrdering::Release);
}

pub fn set_lifecycle_stopping(stopping: bool) {
    LIFECYCLE_STATE.store(if stopping { 2 } else { 1 }, AtomicOrdering::Release);
}

async fn daemon_health_result(
    operation_manager: &crate::sync::SyncOperationManager,
    playback: &crate::playback::PlaybackSession,
) -> Value {
    let descriptor = LIFECYCLE_IDENTITY.get();
    let shutdown = operation_manager.shutdown_snapshot().await;
    let lifecycle_state = LIFECYCLE_STATE.load(AtomicOrdering::Acquire);
    let error_code = shutdown
        .as_ref()
        .and_then(|snapshot| snapshot.error_code.clone())
        .or_else(|| (lifecycle_state == 3).then(|| "SHUTDOWN_TIMEOUT".to_string()));
    serde_json::json!({
        "data": {
            "status": if lifecycle_state >= 2 || operation_manager.is_shutdown_committed() { "stopping" } else { "ok" },
            "protocolVersion": hifimule_lifecycle::PROTOCOL_VERSION,
            "instanceId": descriptor.map(|value| value.instance_id.as_str()).unwrap_or("test-instance"),
            "pid": descriptor.map(|value| value.pid).unwrap_or_else(std::process::id),
            "daemonVersion": env!("CARGO_PKG_VERSION"),
            "errorCode": error_code,
            "shutdown": shutdown,
            "playback": playback.health(),
            "audioRuntime": crate::playback::audio::runtime_identity()
        }
    })
}

async fn authenticate_local_request(
    State(expected_token): State<Arc<String>>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let supplied = request
        .headers()
        .get(http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    if supplied.is_none_or(|token| {
        !hifimule_lifecycle::constant_time_token_eq(expected_token.as_bytes(), token.as_bytes())
    }) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if LIFECYCLE_STATE.load(AtomicOrdering::Acquire) >= 2 && request.uri().path() != "/" {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    ACTIVE_LOCAL_REQUESTS.fetch_add(1, AtomicOrdering::AcqRel);
    let _guard = LocalRequestGuard;
    next.run(request).await
}

fn send_sync_complete_notification() {
    if let Err(e) = crate::notifications::new_notification()
        .summary(&hifimule_i18n::t("notification.sync_complete_ready"))
        .show()
    {
        eprintln!("[Notification] Failed to show OS notification: {}", e);
    }
}

/// On a Subsonic auth failure, evicts the selected server's cached provider and
/// rebuilds it from stored credentials, returning the fresh provider.
async fn reconnect_subsonic_provider_from_config(
    state: &AppState,
) -> Option<Arc<dyn MediaProvider>> {
    let (id, server_type) = {
        let guard = state.server_manager.read().await;
        let rec = guard.selected_record()?;
        (rec.id.clone(), rec.server_type.clone())
    };
    if !matches!(server_type.as_str(), "subsonic" | "openSubsonic") {
        return None;
    }
    state.server_manager.write().await.providers.remove(&id);
    *state.last_connection_check.lock().await = None;
    crate::server_manager::get_provider(&state.server_manager, &state.db, &id)
        .await
        .ok()
}

pub struct RpcServerConfig {
    pub listener: std::net::TcpListener,
    pub descriptor: hifimule_lifecycle::OwnerDescriptor,
    pub ready_tx: std::sync::mpsc::Sender<Result<(), String>>,
    pub shutdown: Arc<std::sync::atomic::AtomicBool>,
}

#[allow(clippy::too_many_arguments)]
pub async fn run_server(
    config: RpcServerConfig,
    db: Arc<crate::db::Database>,
    device_manager: Arc<crate::device::DeviceManager>,
    last_scrobbler_result: Arc<tokio::sync::RwLock<Option<crate::scrobbler::ScrobblerResult>>>,
    state_tx: std::sync::mpsc::Sender<crate::DaemonState>,
    sync_operation_manager: Arc<crate::sync::SyncOperationManager>,
    playback: crate::playback::PlaybackSession,
    native_bridge: crate::playback::native::NativeBridge,
) -> Result<(), String> {
    let token = Arc::new(config.descriptor.token.clone());
    let _ = LIFECYCLE_IDENTITY.set(config.descriptor);
    let server_manager = Arc::new(tokio::sync::RwLock::new(
        crate::server_manager::ServerManager::new(),
    ));
    let playback_commands = crate::playback::commands::PlaybackCommandService::new(
        playback.clone(),
        server_manager.clone(),
        db.clone(),
        sync_operation_manager.clone(),
    );
    let state = Arc::new(AppState {
        jellyfin_client: JellyfinClient::new(),
        server_manager,
        db,
        device_manager,
        last_connection_check: Arc::new(tokio::sync::Mutex::new(None)),
        size_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        sync_operation_manager,
        last_scrobbler_result,
        state_tx,
        playback,
        #[cfg(not(test))]
        pending_audiobookshelf_setups: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
    });
    // Startup (Story 2.11): migrate a legacy single-server vault to the UUID-keyed
    // multi-server vault (if needed), then load server rows into the manager.
    // Providers connect lazily on first selection/use — never eagerly here (AC14).
    if let Err(e) = CredentialManager::migrate_vault_from_legacy(&state.db) {
        eprintln!("[Startup] Vault migration failed: {}", e);
    }
    state.server_manager.write().await.load_from_db(&state.db);
    let _book_reporter = tokio::spawn(crate::playback::book_progress::run_reporter(
        state.playback.clone(),
        state.db.clone(),
        state.server_manager.clone(),
        config.shutdown.clone(),
    ));
    native_bridge
        .publish(crate::playback::native::start_ingress(playback_commands))
        .map_err(|_| "native playback ingress was initialized twice".to_string())?;

    let output_state = state.clone();
    let output_shutdown = config.shutdown.clone();
    let output_stop = config.shutdown.clone();
    let output_dispatcher = tokio::spawn(async move {
        while !output_shutdown.load(AtomicOrdering::Acquire) {
            if let Some(snapshot) = output_state.playback.take_output_effect() {
                let playback = output_state.playback.clone();
                let generation = snapshot.generation_id;
                if crate::playback::audio::global().has_active_generation(&generation) {
                    crate::playback::commands::spawn_successor_preparation(
                        playback,
                        output_state.server_manager.clone(),
                        output_state.db.clone(),
                        generation,
                    );
                    continue;
                }
                let resume_epoch = snapshot.resume_epoch;
                let gain = f32::from_bits(snapshot.gain_bits);
                let admitted_suffix = snapshot.qualified_suffix.clone();
                let result = if let Some(current) = snapshot.current {
                    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
                    let resolve = async {
                        let provider = crate::server_manager::get_provider_by_server_id(
                            &output_state.server_manager,
                            &output_state.db,
                            &current.source.server_id,
                        )
                        .await?;
                        provider.resolve_playback(&current.source.track_id).await
                    };
                    let resolved = tokio::select! {
                        result = tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), resolve) => Some(result),
                        _ = async {
                            while playback.generation_guard(&generation).is_some() {
                                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                            }
                        } => None,
                    };
                    match resolved {
                        Some(Ok(Ok(description))) => {
                            crate::playback::audio::global()
                                .start_at_epoch_with_gain(
                                    description,
                                    current.source,
                                    snapshot.position_ms,
                                    generation.clone(),
                                    playback.clone(),
                                    deadline,
                                    resume_epoch,
                                    gain,
                                    admitted_suffix,
                                )
                                .await
                        }
                        Some(Ok(Err(error))) => Err(
                            crate::playback::audio::PlaybackPipelineError::from_provider_error(
                                error,
                            ),
                        ),
                        Some(Err(_)) => {
                            playback.publish_event(
                                generation.clone(),
                                crate::playback::model::PlaybackEvent::Failed {
                                    code: "PLAYBACK_TIMEOUT".into(),
                                    retryable: true,
                                },
                            );
                            continue;
                        }
                        None => continue,
                    }
                } else {
                    crate::playback::audio::global()
                        .validate_selected_output(playback.clone(), generation.clone())
                        .await
                };
                if let Err(error) = result {
                    crate::playback::audio::publish_pipeline_failure(&playback, generation, error);
                } else {
                    crate::playback::commands::spawn_successor_preparation(
                        playback,
                        output_state.server_manager.clone(),
                        output_state.db.clone(),
                        generation,
                    );
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    });

    let app = Router::new()
        .route("/", post(handler))
        .route("/jellyfin/image/{*id}", get(handle_proxy_image))
        .layer(DefaultBodyLimit::max(50 * 1024 * 1024))
        .layer(
            tower_http::cors::CorsLayer::new()
                .allow_origin([
                    "http://localhost:1420"
                        .parse::<http::HeaderValue>()
                        .unwrap(),
                    "http://127.0.0.1:1420"
                        .parse::<http::HeaderValue>()
                        .unwrap(),
                    "tauri://localhost".parse::<http::HeaderValue>().unwrap(),
                    "https://tauri.localhost"
                        .parse::<http::HeaderValue>()
                        .unwrap(),
                ])
                .allow_methods([http::Method::POST, http::Method::GET])
                .allow_headers([http::header::CONTENT_TYPE, http::header::AUTHORIZATION]),
        )
        .layer(middleware::from_fn_with_state(
            token,
            authenticate_local_request,
        ))
        .with_state(state.clone());

    config
        .listener
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;
    let addr = config
        .listener
        .local_addr()
        .map_err(|error| error.to_string())?;
    println!("RPC server listening on {}", addr);
    let listener =
        tokio::net::TcpListener::from_std(config.listener).map_err(|error| error.to_string())?;
    LIFECYCLE_STATE.store(1, AtomicOrdering::Release);
    let _ = config.ready_tx.send(Ok(()));
    let result = axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            while !config.shutdown.load(std::sync::atomic::Ordering::Acquire)
                || ACTIVE_LOCAL_REQUESTS.load(AtomicOrdering::Acquire) != 0
            {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        })
        .await
        .map_err(|error| error.to_string());
    state.pending_audiobookshelf_setups().lock().await.clear();
    output_stop.store(true, AtomicOrdering::Release);
    let _ = output_dispatcher.await;
    result
}

async fn handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<JsonRpcRequest>,
) -> Json<JsonRpcResponse> {
    if (LIFECYCLE_STATE.load(AtomicOrdering::Acquire) >= 2
        || state.sync_operation_manager.is_shutdown_committed())
        && !matches!(
            payload.method.as_str(),
            "daemon.health" | "playback.retryCheckpoint"
        )
    {
        return Json(JsonRpcResponse {
            jsonrpc: "2.0".into(),
            result: None,
            id: payload.id,
            error: Some(JsonRpcError {
                code: ERR_SYNC_IN_PROGRESS,
                message: "The daemon is stopping".into(),
                data: Some(serde_json::json!({"errorCode":"DAEMON_STOPPED"})),
            }),
        });
    }
    let mut mutation_guard = if is_mutating_method(&payload.method) {
        match state.sync_operation_manager.try_admit_mutation() {
            Some(guard) => Some(guard),
            None => {
                return Json(JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    result: None,
                    error: Some(JsonRpcError {
                        code: ERR_SYNC_IN_PROGRESS,
                        message: "The daemon is stopping and is not accepting new work".to_string(),
                        data: Some(serde_json::json!({ "errorCode": "DAEMON_STOPPED" })),
                    }),
                    id: payload.id,
                });
            }
        }
    } else {
        None
    };
    let result = match payload.method.as_str() {
        "test_connection" => handle_test_connection(&state, payload.params).await,
        "server.connect" => handle_server_connect(&state, payload.params).await,
        "server.audiobookshelf.discover" => {
            handle_audiobookshelf_discover(&state, payload.params).await
        }
        "server.audiobookshelf.commit" => {
            handle_audiobookshelf_commit(&state, payload.params).await
        }
        "server.audiobookshelf.cancelSetup" => {
            handle_audiobookshelf_cancel(&state, payload.params).await
        }
        "server.reauthenticate" => handle_server_reauthenticate(&state, payload.params).await,
        "server.logout" => handle_server_logout(&state).await,
        "server.list" => handle_server_list(&state).await,
        "server.select" => handle_server_select(&state, payload.params).await,
        "server.update" => handle_server_update(&state, payload.params).await,
        "server.remove" => handle_server_remove(&state, payload.params).await,
        "login" => handle_login(&state, payload.params).await,
        "save_credentials" => handle_save_credentials(payload.params).await,
        "get_credentials" => handle_get_credentials(&state).await,
        "set_device_profile" => handle_set_device_profile(&state, payload.params).await,
        "get_daemon_state" => handle_get_daemon_state(&state).await,
        "jellyfin_get_views" => handle_jellyfin_get_views(&state, payload.params).await,
        "jellyfin_get_items" => handle_jellyfin_get_items(&state, payload.params).await,
        "jellyfin_get_item_details" => {
            handle_jellyfin_get_item_details(&state, payload.params).await
        }
        "jellyfin_get_item_counts" => handle_jellyfin_get_item_counts(&state, payload.params).await,
        "jellyfin_get_item_sizes" => handle_jellyfin_get_item_sizes(&state, payload.params).await,
        "device_get_storage_info" => handle_device_get_storage_info(&state).await,
        "device_list_root_folders" => handle_device_list_root_folders(&state).await,
        "sync_get_device_status_map" => handle_sync_get_device_status_map(&state).await,
        "sync_calculate_delta" => handle_sync_calculate_delta(&state, payload.params).await,
        "sync_detect_changes" => handle_sync_detect_changes(&state, payload.params).await,
        "sync_execute" => handle_sync_execute(&state, payload.params).await,
        "sync_cancel" => handle_sync_cancel(&state, payload.params).await,
        "sync_get_operation_status" => {
            handle_sync_get_operation_status(&state, payload.params).await
        }
        "sync_get_resume_state" => handle_sync_get_resume_state(&state).await,
        "scrobbler_get_last_result" => handle_scrobbler_get_last_result(&state).await,
        "manifest_get_discrepancies" => handle_manifest_get_discrepancies(&state).await,
        "manifest_prune" => handle_manifest_prune(&state, payload.params).await,
        "manifest_relink" => handle_manifest_relink(&state, payload.params).await,
        "manifest_clear_dirty" => handle_manifest_clear_dirty(&state).await,
        "manifest_get_basket" => handle_manifest_get_basket(&state).await,
        "manifest_save_basket" => handle_manifest_save_basket(&state, payload.params).await,
        "device_initialize" => handle_device_initialize(&state, payload.params).await,
        "device.update_manifest" => handle_device_update_manifest(&state, payload.params).await,
        "device_set_auto_sync_on_connect" => {
            handle_device_set_auto_sync_on_connect(&state, payload.params).await
        }
        "basket.autoFill" => handle_basket_auto_fill(&state, payload.params).await,
        "autoFill.setPipeline" => handle_auto_fill_set_pipeline(&state, payload.params).await,
        "sync.setAutoFill" => handle_sync_set_auto_fill(&state, payload.params).await,
        "device_profiles.list" => handle_device_profiles_list().await,
        "device.set_transcoding_profile" => {
            handle_set_transcoding_profile(&state, payload.params).await
        }
        "device.list" => handle_device_list(&state).await,
        "device.select" => handle_device_select(&state, payload.params).await,
        "destination.select" => handle_destination_select(&state, payload.params).await,
        "server.probe" => handle_server_probe(payload.params).await,
        "daemon.health" => {
            Ok(daemon_health_result(&state.sync_operation_manager, &state.playback).await)
        }
        "playback.retryCheckpoint" => handle_playback_retry_checkpoint(&state, payload.params),
        "playback.getSession" => handle_playback_get_session(&state, payload.params).await,
        "playback.listOccurrences" => {
            handle_playback_list_occurrences(&state, payload.params).await
        }
        "playback.describeOccurrences" => {
            handle_playback_describe_occurrences(&state, payload.params).await
        }
        "playback.applySession" => {
            handle_playback_apply_session(&state, payload.params, mutation_guard.take()).await
        }
        "playback.playEpisode" => {
            handle_playback_play_episode(&state, payload.params, mutation_guard.take()).await
        }
        "playback.playAlbum" => {
            handle_playback_play_album(&state, payload.params, mutation_guard.take()).await
        }
        "playback.previewTrack" => {
            handle_playback_preview_track(&state, payload.params, mutation_guard.take()).await
        }
        "playback.listOutputs" => handle_playback_list_outputs(&state, payload.params).await,
        "playback.selectOutput" => {
            handle_playback_select_output(&state, payload.params, mutation_guard.take()).await
        }
        "playback.control" => {
            handle_playback_control(&state, payload.params, mutation_guard.take()).await
        }
        "playback.seek" => {
            handle_playback_seek(&state, payload.params, mutation_guard.take()).await
        }
        "playback.retryRestore" => {
            handle_playback_retry_restore(&state, payload.params, mutation_guard.take()).await
        }
        "daemon.retryQuit" => {
            if state.sync_operation_manager.request_quit_retry() {
                Ok(serde_json::json!({ "data": { "accepted": true } }))
            } else {
                Err(JsonRpcError {
                    code: ERR_SYNC_IN_PROGRESS,
                    message: "Quit can only be retried after launch-fence persistence failed"
                        .into(),
                    data: Some(serde_json::json!({ "errorCode": "QUIT_RETRY_UNAVAILABLE" })),
                })
            }
        }
        "browse.listModes" => handle_browse_list_modes(&state).await,
        "browse.listArtists" => handle_browse_list_artists(&state, payload.params).await,
        "browse.getArtist" => handle_browse_get_artist(&state, payload.params).await,
        "browse.listAlbums" => handle_browse_list_albums(&state, payload.params).await,
        "browse.getAlbum" => handle_browse_get_album(&state, payload.params).await,
        "browse.listPodcastShows" => handle_browse_list_podcast_shows(&state, payload.params).await,
        "browse.listRecentPodcastEpisodes" => {
            handle_browse_list_recent_podcast_episodes(&state, payload.params).await
        }
        "browse.getPodcastShow" => handle_browse_get_podcast_show(&state, payload.params).await,
        "browse.getPodcastEpisode" => {
            handle_browse_get_podcast_episode(&state, payload.params).await
        }
        "browse.listPlaylists" => handle_browse_list_playlists(&state).await,
        "browse.getPlaylist" => handle_browse_get_playlist(&state, payload.params).await,
        "browse.listGenres" => handle_browse_list_genres(&state, payload.params).await,
        "browse.getGenre" => handle_browse_get_genre(&state, payload.params).await,
        "browse.listRecentlyAdded" => {
            handle_browse_list_recently_added(&state, payload.params).await
        }
        "browse.listFrequentlyPlayed" => {
            handle_browse_list_frequently_played(&state, payload.params).await
        }
        "browse.listRecentlyPlayed" => {
            handle_browse_list_recently_played(&state, payload.params).await
        }
        "browse.listFavorites" => handle_browse_list_favorites(&state, payload.params).await,
        "browse.listFavoriteItems" => {
            handle_browse_list_favorite_items(&state, payload.params).await
        }
        "browse.listTracks" => handle_browse_list_tracks(&state, payload.params).await,
        "browse.search" => handle_browse_search(&state, payload.params).await,
        "playlist.create" => handle_playlist_create(&state, payload.params).await,
        "playlist.addItems" => handle_playlist_add_items(&state, payload.params).await,
        "playlist.addTracks" => handle_playlist_add_tracks(&state, payload.params).await,
        "playlist.removeTracks" => handle_playlist_remove_tracks(&state, payload.params).await,
        "playlist.delete" => handle_playlist_delete(&state, payload.params).await,
        "playlist.rename" => handle_playlist_rename(&state, payload.params).await,
        "playlist.reorder" => handle_playlist_reorder(&state, payload.params).await,
        _ => Err(JsonRpcError {
            code: ERR_METHOD_NOT_FOUND,
            message: hifimule_i18n::t("error.method_not_found"),
            data: None,
        }),
    };

    match result {
        Ok(res) => Json(JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            result: Some(res),
            error: None,
            id: payload.id,
        }),
        Err(err) => Json(JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            result: None,
            error: Some(err),
            id: payload.id,
        }),
    }
}

fn is_mutating_method(method: &str) -> bool {
    matches!(
        method,
        "server.connect"
            | "server.audiobookshelf.commit"
            | "server.audiobookshelf.cancelSetup"
            | "server.reauthenticate"
            | "daemon.retryQuit"
            | "server.logout"
            | "server.select"
            | "server.update"
            | "server.remove"
            | "login"
            | "save_credentials"
            | "set_device_profile"
            | "sync_calculate_delta"
            | "sync_execute"
            | "sync_cancel"
            | "sync_get_resume_state"
            | "manifest_prune"
            | "manifest_relink"
            | "manifest_clear_dirty"
            | "manifest_save_basket"
            | "device_initialize"
            | "device.update_manifest"
            | "device_set_auto_sync_on_connect"
            | "basket.autoFill"
            | "autoFill.setPipeline"
            | "sync.setAutoFill"
            | "device_profiles.list"
            | "device.set_transcoding_profile"
            | "device.select"
            | "destination.select"
            | "playlist.create"
            | "playback.applySession"
            | "playback.playEpisode"
            | "playback.playAlbum"
            | "playback.previewTrack"
            | "playback.control"
            | "playback.seek"
            | "playback.selectOutput"
            | "playback.retryRestore"
            | "playlist.addItems"
            | "playlist.addTracks"
            | "playlist.removeTracks"
            | "playlist.delete"
            | "playlist.rename"
            | "playlist.reorder"
    )
}

fn playback_error(error: crate::playback::session::PlaybackError) -> JsonRpcError {
    let mut data =
        serde_json::json!({ "code": error.code, "retryable": error.code == "PLAYBACK_BUSY" });
    if let Some(metadata) = &error.authoritative {
        data["instanceId"] = serde_json::json!(metadata.instance_id);
        data["sessionId"] = serde_json::json!(metadata.session_id);
        data["queueRevision"] = serde_json::json!(metadata.queue_revision);
        data["stateSequence"] = serde_json::json!(metadata.state_sequence);
        data["generationId"] = serde_json::json!(metadata.generation_id);
    }
    JsonRpcError {
        code: if error.conflict {
            409
        } else if error.code == "PERSISTENCE_FAILED" {
            -32603
        } else {
            ERR_INVALID_PARAMS
        },
        message: error.message.into(),
        data: Some(data),
    }
}

fn playback_task_error(_: tokio::task::JoinError) -> JsonRpcError {
    JsonRpcError {
        code: -32603,
        message: "Playback owner task failed".into(),
        data: Some(serde_json::json!({"code":"PERSISTENCE_FAILED"})),
    }
}

fn handle_playback_retry_checkpoint(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Params {
        schema_version: u32,
        instance_id: String,
        shutdown_id: String,
    }
    let p: Params =
        serde_json::from_value(params.unwrap_or(Value::Null)).map_err(|_| JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Invalid playback.retryCheckpoint parameters".into(),
            data: Some(serde_json::json!({"code":"INVALID_SESSION"})),
        })?;
    let instance_id = state.playback.instance_id();
    let snapshot = state.sync_operation_manager.shutdown_tray_snapshot();
    let code = if p.schema_version != crate::playback::model::SCHEMA_VERSION {
        Some("UNSUPPORTED_PLAYBACK_VERSION")
    } else if p.instance_id != instance_id {
        Some("INSTANCE_MISMATCH")
    } else if !state
        .sync_operation_manager
        .request_checkpoint_retry(&p.shutdown_id)
    {
        Some("PLAYBACK_BUSY")
    } else {
        None
    };
    if let Some(code) = code {
        return Err(JsonRpcError {
            code: if code == "UNSUPPORTED_PLAYBACK_VERSION" {
                ERR_INVALID_PARAMS
            } else {
                409
            },
            message: "Session checkpoint retry is unavailable".into(),
            data: Some(
                serde_json::json!({"code":code,"instanceId":instance_id,"shutdownId":snapshot.map(|s|s.shutdown_id)}),
            ),
        });
    }
    Ok(
        serde_json::json!({"data":{"accepted":true,"instanceId":instance_id,"shutdownId":p.shutdown_id}}),
    )
}

async fn handle_playback_get_session(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Params {
        schema_version: u32,
    }
    let p: Params =
        serde_json::from_value(params.unwrap_or(Value::Null)).map_err(|_| JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Invalid playback.getSession parameters".into(),
            data: Some(serde_json::json!({"code":"INVALID_SESSION"})),
        })?;
    if p.schema_version != crate::playback::model::SCHEMA_VERSION {
        return Err(playback_error(crate::playback::session::PlaybackError {
            code: "UNSUPPORTED_PLAYBACK_VERSION",
            message: "unsupported playback schema version",
            conflict: false,
            authoritative: None,
        }));
    }
    let playback = state.playback.clone();
    tokio::task::spawn_blocking(move || playback.snapshot())
        .await
        .map_err(playback_task_error)?
        .map(|data| serde_json::json!({"data":data}))
        .map_err(playback_error)
}

async fn handle_playback_list_occurrences(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let p = serde_json::from_value::<crate::playback::model::ListOccurrencesParams>(
        params.unwrap_or(Value::Null),
    )
    .map_err(|_| JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Invalid playback.listOccurrences parameters".into(),
        data: Some(serde_json::json!({"code":"INVALID_CURSOR"})),
    })?;
    let playback = state.playback.clone();
    tokio::task::spawn_blocking(move || playback.list(p))
        .await
        .map_err(playback_task_error)?
        .map(|data| serde_json::json!({"data":data}))
        .map_err(playback_error)
}

const OCCURRENCE_DISPLAY_PROVIDER_CONCURRENCY: usize = 8;
static OCCURRENCE_DISPLAY_PERMITS: std::sync::LazyLock<Arc<tokio::sync::Semaphore>> =
    std::sync::LazyLock::new(|| {
        Arc::new(tokio::sync::Semaphore::new(
            OCCURRENCE_DISPLAY_PROVIDER_CONCURRENCY,
        ))
    });

async fn handle_playback_describe_occurrences(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    use crate::playback::model::{
        DescribeOccurrencesParams, OccurrenceDisplay, OccurrenceDisplayStatus,
    };
    let p = serde_json::from_value::<DescribeOccurrencesParams>(params.unwrap_or(Value::Null))
        .map_err(|_| JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Invalid playback.describeOccurrences parameters".into(),
            data: Some(serde_json::json!({"code":"INVALID_OCCURRENCES"})),
        })?;
    if p.schema_version != crate::playback::model::SCHEMA_VERSION
        || p.occurrence_ids.is_empty()
        || p.occurrence_ids.len() > crate::playback::model::MAX_PAGE_SIZE
    {
        return Err(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Occurrence display request is outside page bounds".into(),
            data: Some(serde_json::json!({"code":"INVALID_OCCURRENCES"})),
        });
    }
    let playback = state.playback.clone();
    let snapshot = tokio::task::spawn_blocking(move || playback.snapshot())
        .await
        .map_err(playback_task_error)?
        .map_err(playback_error)?;
    if snapshot.session_id != p.session_id || snapshot.queue_revision != p.expected_queue_revision {
        return Err(JsonRpcError {
            code: ERR_CROSS_SERVER_CONFLICT,
            message: "Playback session or queue revision changed".into(),
            data: Some(serde_json::json!({"code":"QUEUE_CONFLICT", "authoritative": snapshot})),
        });
    }

    let mut occurrences = Vec::with_capacity(p.occurrence_ids.len());
    for occurrence_id in &p.occurrence_ids {
        let occurrence = state
            .db
            .playback_occurrence(&p.session_id, occurrence_id)
            .map_err(|_| JsonRpcError {
                code: ERR_STORAGE_ERROR,
                message: "Could not read playback queue".into(),
                data: None,
            })?
            .ok_or(JsonRpcError {
                code: ERR_INVALID_PARAMS,
                message: "Occurrence does not belong to this queue".into(),
                data: Some(serde_json::json!({"code":"INVALID_OCCURRENCES"})),
            })?;
        occurrences.push(occurrence);
    }

    let semaphore = Arc::clone(&OCCURRENCE_DISPLAY_PERMITS);
    let mut unique = HashMap::new();
    for occurrence in &occurrences {
        unique
            .entry((
                occurrence.source.server_id.clone(),
                occurrence.source.track_id.clone(),
            ))
            .or_insert_with(|| occurrence.source.clone());
    }
    let mut tasks = tokio::task::JoinSet::new();
    for ((server_id, track_id), source) in unique {
        let permit = Arc::clone(&semaphore);
        let manager = Arc::clone(&state.server_manager);
        let db = Arc::clone(&state.db);
        tasks.spawn(async move {
            let _permit = permit.acquire_owned().await.ok();
            let resolved =
                match crate::server_manager::get_provider_by_server_id(&manager, &db, &server_id)
                    .await
                {
                    Ok(provider) => match provider.get_playback_display_song(&track_id).await {
                        Ok(song) => (Some(song), OccurrenceDisplayStatus::Available),
                        Err(_) => (None, OccurrenceDisplayStatus::TrackUnavailable),
                    },
                    Err(_) => (None, OccurrenceDisplayStatus::SourceUnavailable),
                };
            ((server_id, track_id), source, resolved)
        });
    }
    let mut descriptions = HashMap::new();
    while let Some(Ok((key, source, (song, status)))) = tasks.join_next().await {
        descriptions.insert(key, (source, song, status));
    }
    let rows: Vec<_> = occurrences
        .into_iter()
        .map(|occurrence| {
            let key = (
                occurrence.source.server_id.clone(),
                occurrence.source.track_id.clone(),
            );
            let (source, song, status) = descriptions.get(&key).cloned().unwrap_or((
                occurrence.source.clone(),
                None,
                OccurrenceDisplayStatus::SourceUnavailable,
            ));
            OccurrenceDisplay {
                occurrence_id: occurrence.occurrence_id,
                source,
                title: song.as_ref().map(|song| song.title.clone()),
                artist: song.as_ref().and_then(|song| song.artist_name.clone()),
                album: song.as_ref().and_then(|song| song.album_title.clone()),
                duration_ms: song
                    .as_ref()
                    .map(|song| u64::from(song.duration_seconds) * 1000),
                status,
            }
        })
        .collect();
    Ok(serde_json::json!({"data": {"occurrences": rows}}))
}

async fn handle_playback_list_outputs(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let params = serde_json::from_value::<crate::playback::model::ListOutputsParams>(
        params.unwrap_or(Value::Null),
    )
    .map_err(|_| JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Invalid playback.listOutputs parameters".into(),
        data: None,
    })?;
    let playback = state.playback.clone();
    tokio::task::spawn_blocking(move || playback.list_outputs(params))
        .await
        .map_err(playback_task_error)?
        .map(|data| serde_json::json!({"data":data}))
        .map_err(playback_error)
}

async fn handle_playback_select_output(
    state: &AppState,
    params: Option<Value>,
    guard: Option<crate::sync::MutationGuard>,
) -> Result<Value, JsonRpcError> {
    let params = serde_json::from_value::<crate::playback::model::SelectOutputParams>(
        params.unwrap_or(Value::Null),
    )
    .map_err(|_| JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Invalid playback.selectOutput parameters".into(),
        data: None,
    })?;
    let playback = state.playback.clone();
    tokio::task::spawn_blocking(move || playback.select_output(params, guard))
        .await
        .map_err(playback_task_error)?
        .map(|data| serde_json::json!({"data":data}))
        .map_err(playback_error)
}

async fn handle_playback_apply_session(
    state: &AppState,
    params: Option<Value>,
    mutation_guard: Option<crate::sync::MutationGuard>,
) -> Result<Value, JsonRpcError> {
    handle_playback_apply_session_inner(state, params, mutation_guard, false).await
}

async fn handle_playback_apply_session_inner(
    state: &AppState,
    params: Option<Value>,
    mutation_guard: Option<crate::sync::MutationGuard>,
    allow_podcast_episode: bool,
) -> Result<Value, JsonRpcError> {
    let p = serde_json::from_value::<crate::playback::model::ApplySessionParams>(
        params.unwrap_or(Value::Null),
    )
    .map_err(|_| JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Invalid playback.applySession parameters".into(),
        data: Some(serde_json::json!({"code":"INVALID_SESSION"})),
    })?;
    let added_sources: Vec<_> = match &p.operation {
        crate::playback::model::SessionOperation::PlayTrack { source } => vec![source],
        crate::playback::model::SessionOperation::ReplaceQueue { sources }
        | crate::playback::model::SessionOperation::AppendQueue { sources }
        | crate::playback::model::SessionOperation::PlayAlbum { sources } => {
            sources.iter().collect()
        }
        _ => Vec::new(),
    };
    for source in added_sources {
        if !source.track_id.starts_with("abs-episode-") {
            continue;
        }
        let provider = crate::server_manager::get_provider_by_server_id(
            &state.server_manager,
            &state.db,
            &source.server_id,
        )
        .await
        .map_err(provider_error_to_rpc)?;
        if provider.library_role() == Some(crate::providers::ProviderLibraryRole::Podcast)
            && !allow_podcast_episode
        {
            return Err(JsonRpcError {
                code: ERR_UNSUPPORTED_CAPABILITY,
                message: "Use playback.playEpisode for podcast episodes".into(),
                data: None,
            });
        }
    }
    let source = match &p.operation {
        crate::playback::model::SessionOperation::PlayTrack { source } => Some(source.clone()),
        crate::playback::model::SessionOperation::PlayAlbum { sources } => sources.first().cloned(),
        _ => None,
    };
    let playback = state.playback.clone();
    let owner = playback.clone();
    let result = tokio::task::spawn_blocking(move || owner.apply_with_guard(p, mutation_guard))
        .await
        .map_err(|_| JsonRpcError {
            code: -32603,
            message: "Playback owner task failed".into(),
            data: Some(serde_json::json!({"code":"PERSISTENCE_FAILED"})),
        })?
        .map_err(playback_error)?;
    if let Some(source) = source.filter(|_| result.start_audio) {
        let manager = state.server_manager.clone();
        let db = state.db.clone();
        let generation = result.generation_id.clone();
        let occurrence = result
            .assigned_occurrences
            .iter()
            .find(|item| item.source == source)
            .cloned();
        let session_id = result.session_id.clone();
        tokio::spawn(async move {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
            let resolved =
                tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), async {
                    match crate::server_manager::get_provider_by_server_id(
                        &manager,
                        &db,
                        &source.server_id,
                    )
                    .await
                    {
                        Ok(provider) => {
                            // Explicit Part start keeps its requested beginning. The
                            // remote read qualifies later write-back but never seeks it.
                            if provider.library_role()
                                != Some(crate::providers::ProviderLibraryRole::Podcast)
                                && let Some(occurrence) = occurrence.as_ref()
                                && let Ok(Some((timing, remote))) =
                                    tokio::time::timeout(std::time::Duration::from_secs(3), async {
                                        let timing = provider
                                            .book_timing_for_track(&source.track_id)
                                            .await
                                            .ok()??;
                                        let remote = provider
                                            .read_book_progress(&timing.identity)
                                            .await
                                            .ok()?;
                                        Some((timing, remote))
                                    })
                                    .await
                            {
                                use crate::playback::book_progress::{
                                    BookMap, BookOccurrenceRecord, BookPart,
                                };
                                let map = BookMap::new(
                                    timing
                                        .parts
                                        .iter()
                                        .map(|part| {
                                            BookPart::new(
                                                &part.track_id,
                                                &part.audio_file_id,
                                                part.duration_ms,
                                            )
                                        })
                                        .collect(),
                                );
                                if let Some(map) = map
                                    && remote.as_ref().is_none_or(|progress| {
                                        progress.duration_ms.abs_diff(map.duration_ms()) <= 1_000
                                            && progress.current_ms <= map.duration_ms()
                                    })
                                    && let Some((part, offset)) = map.part(&source.track_id)
                                {
                                    let _ = db.save_book_continuity_with_timing(
                                        &BookOccurrenceRecord {
                                            session_id: session_id.clone(),
                                            occurrence_id: occurrence.occurrence_id.clone(),
                                            server_id: source.server_id.clone(),
                                            track_id: source.track_id.clone(),
                                            identity: timing.identity.clone(),
                                            audio_file_id: part.file_id.clone(),
                                            part_offset_ms: offset,
                                            duration_ms: map.duration_ms(),
                                            whole_ms: offset,
                                            mapping_valid: true,
                                        },
                                        Some(&timing),
                                    );
                                }
                            }
                            provider.resolve_playback(&source.track_id).await
                        }
                        Err(error) => Err(error),
                    }
                })
                .await;
            let resolved = match resolved {
                Ok(result) => result,
                Err(_) => {
                    playback.publish_event(
                        generation,
                        crate::playback::model::PlaybackEvent::Failed {
                            code: "PLAYBACK_TIMEOUT".into(),
                            retryable: true,
                        },
                    );
                    return;
                }
            };
            let outcome = match resolved {
                Ok(description) => {
                    crate::playback::audio::global()
                        .start(
                            description,
                            source,
                            0,
                            generation.clone(),
                            playback.clone(),
                            deadline,
                        )
                        .await
                }
                Err(error) => {
                    Err(crate::playback::audio::PlaybackPipelineError::from_provider_error(error))
                }
            };
            if let Err(error) = outcome {
                crate::playback::audio::publish_pipeline_failure(&playback, generation, error);
            } else {
                crate::playback::commands::spawn_successor_preparation(
                    playback, manager, db, generation,
                );
            }
        });
    }
    Ok(serde_json::json!({"data":result}))
}

async fn handle_playback_play_episode(
    state: &AppState,
    params: Option<Value>,
    mutation_guard: Option<crate::sync::MutationGuard>,
) -> Result<Value, JsonRpcError> {
    let mut payload = params.unwrap_or(Value::Null);
    let server_id = payload
        .get("serverId")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .ok_or(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Missing serverId".into(),
            data: None,
        })?
        .to_owned();
    let episode_id = payload
        .get("episodeId")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .ok_or(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Missing episodeId".into(),
            data: None,
        })?
        .to_owned();
    let provider = crate::server_manager::get_provider_by_server_id(
        &state.server_manager,
        &state.db,
        &server_id,
    )
    .await
    .map_err(provider_error_to_rpc)?;
    if provider.library_role() != Some(crate::providers::ProviderLibraryRole::Podcast) {
        return Err(JsonRpcError {
            code: ERR_UNSUPPORTED_CAPABILITY,
            message: "Podcast playback unavailable".into(),
            data: None,
        });
    }
    provider
        .get_podcast_episode(&episode_id)
        .await
        .map_err(provider_error_to_rpc)?;
    let object = payload.as_object_mut().ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Invalid episode playback request".into(),
        data: None,
    })?;
    object.remove("serverId");
    object.remove("episodeId");
    object.insert(
        "operation".into(),
        serde_json::json!({
            "type": "playTrack", "source": { "serverId": server_id, "trackId": episode_id }
        }),
    );
    handle_playback_apply_session_inner(state, Some(payload), mutation_guard, true).await
}

async fn handle_playback_play_album(
    state: &AppState,
    params: Option<Value>,
    mutation_guard: Option<crate::sync::MutationGuard>,
) -> Result<Value, JsonRpcError> {
    use crate::playback::album::{AlbumValidationError, prepare_album};
    use crate::playback::model::PlayAlbumParams;
    use crate::playback::session::AlbumAdmission;

    let p =
        serde_json::from_value::<PlayAlbumParams>(params.unwrap_or(Value::Null)).map_err(|_| {
            JsonRpcError {
                code: ERR_INVALID_PARAMS,
                message: "Invalid playback.playAlbum parameters".into(),
                data: Some(serde_json::json!({"code":"ALBUM_INVALID"})),
            }
        })?;
    let playback = state.playback.clone();
    let request = p.clone();
    let admission =
        tokio::task::spawn_blocking(move || playback.reserve_album(request, mutation_guard))
            .await
            .map_err(playback_task_error)?
            .map_err(playback_error)?;
    let mut reservation = match admission {
        AlbumAdmission::Replay(snapshot) => return Ok(serde_json::json!({"data":snapshot})),
        AlbumAdmission::Resolve(reservation) => reservation,
    };
    let album = resolve_playback_album(state, &p.source, &reservation).await?;
    // The provider owns upstream identity and progress. Resolve it while the
    // album reservation is pending, before any file can become audible.
    if let Ok(provider) = crate::server_manager::get_provider_by_server_id(
        &state.server_manager,
        &state.db,
        &p.source.server_id,
    )
    .await
    {
        if let Ok(Ok(Some(timing))) = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            provider.book_timing(&p.source.album_id),
        )
        .await
        {
            use crate::playback::book_progress::{BookResumeDecision, decide_resume};
            let album_matches = album.provider_metadata.identity.as_ref() == Some(&timing.identity)
                && album.tracks.len() == timing.parts.len()
                && album.tracks.iter().zip(&timing.parts).all(|(track, part)| {
                    track.id == part.track_id
                        && track.provider_metadata.audio_file_id.as_deref()
                            == Some(part.audio_file_id.as_str())
                });
            if album_matches
                && let Ok(Ok(remote)) = tokio::time::timeout(
                    std::time::Duration::from_secs(3),
                    provider.read_book_progress(&timing.identity),
                )
                .await
            {
                match decide_resume(&timing, remote) {
                    BookResumeDecision::Beginning => reservation.book_timing = Some(timing),
                    BookResumeDecision::At {
                        part_index,
                        local_ms,
                    } => {
                        reservation.book_start = Some((part_index, local_ms));
                        reservation.book_timing = Some(timing);
                    }
                    BookResumeDecision::Unavailable => {}
                }
            }
        }
    }
    let plan = prepare_album(album, &p.source).map_err(|error| {
        let code = match error {
            AlbumValidationError::Empty => "ALBUM_EMPTY",
            AlbumValidationError::TooLarge => "ALBUM_TOO_LARGE",
            AlbumValidationError::Invalid => "ALBUM_INVALID",
        };
        JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Album cannot be queued".into(),
            data: Some(serde_json::json!({"code":code})),
        }
    })?;
    let service = crate::playback::commands::PlaybackCommandService::new(
        state.playback.clone(),
        state.server_manager.clone(),
        state.db.clone(),
        state.sync_operation_manager.clone(),
    );
    let runtime = tokio::runtime::Handle::current();
    let snapshot = tokio::task::spawn_blocking(move || {
        let _runtime = runtime.enter();
        service.commit_album_with_policy(
            reservation,
            plan.sources,
            plan.policy,
            plan.representations,
        )
    })
    .await
    .map_err(playback_task_error)?
    .map_err(playback_error)?;
    Ok(serde_json::json!({"data":snapshot}))
}

async fn resolve_playback_album(
    state: &AppState,
    source: &crate::playback::model::AlbumSource,
    reservation: &crate::playback::session::AlbumReservation,
) -> Result<crate::domain::models::AlbumWithTracks, JsonRpcError> {
    let stale_error = || {
        let code = if reservation.is_superseded() {
            "ALBUM_SUPERSEDED"
        } else {
            "GENERATION_CONFLICT"
        };
        JsonRpcError {
            code: 409,
            message: if code == "ALBUM_SUPERSEDED" {
                "Album playback was superseded".into()
            } else {
                "Album admission is stale".into()
            },
            data: Some(serde_json::json!({"code":code})),
        }
    };
    let resolve = async {
        let provider = crate::server_manager::get_provider_by_server_id(
            &state.server_manager,
            &state.db,
            &source.server_id,
        )
        .await?;
        provider.get_album(&source.album_id).await
    };
    let result = tokio::select! {
        biased;
        _ = async {
            while !reservation.is_cancelled() {
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
        } => return Err(stale_error()),
        result = tokio::time::timeout_at(tokio::time::Instant::from_std(reservation.deadline), resolve) => result,
    };
    if reservation.is_cancelled() {
        return Err(stale_error());
    }
    result
        .map_err(|_| JsonRpcError {
            code: ERR_CONNECTION_FAILED,
            message: "Album resolution timed out".into(),
            data: Some(serde_json::json!({"code":"PLAYBACK_TIMEOUT"})),
        })?
        .map_err(provider_error_to_rpc)
}

#[cfg(test)]
#[path = "rpc/album_tests.rs"]
mod album_review_tests;

async fn handle_playback_control(
    state: &AppState,
    params: Option<Value>,
    mutation_guard: Option<crate::sync::MutationGuard>,
) -> Result<Value, JsonRpcError> {
    let p = serde_json::from_value::<crate::playback::model::ControlParams>(
        params.unwrap_or(Value::Null),
    )
    .map_err(|_| JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Invalid playback.control parameters".into(),
        data: Some(serde_json::json!({"code":"INVALID_SESSION"})),
    })?;
    let result = crate::playback::commands::PlaybackCommandService::new(
        state.playback.clone(),
        state.server_manager.clone(),
        state.db.clone(),
        state.sync_operation_manager.clone(),
    )
    .rpc_control(p, mutation_guard)
    .await
    .map_err(playback_error)?;
    Ok(serde_json::json!({"data":result}))
}

async fn handle_playback_preview_track(
    state: &AppState,
    params: Option<Value>,
    mutation_guard: Option<crate::sync::MutationGuard>,
) -> Result<Value, JsonRpcError> {
    let p = serde_json::from_value::<crate::playback::model::PreviewTrackParams>(
        params.unwrap_or(Value::Null),
    )
    .map_err(|_| JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Invalid playback.previewTrack parameters".into(),
        data: Some(serde_json::json!({"code":"INVALID_SESSION"})),
    })?;
    let result = crate::playback::commands::PlaybackCommandService::new(
        state.playback.clone(),
        state.server_manager.clone(),
        state.db.clone(),
        state.sync_operation_manager.clone(),
    )
    .rpc_preview(p, mutation_guard)
    .await
    .map_err(playback_error)?;
    Ok(serde_json::json!({"data":result}))
}

async fn handle_playback_seek(
    state: &AppState,
    params: Option<Value>,
    mutation_guard: Option<crate::sync::MutationGuard>,
) -> Result<Value, JsonRpcError> {
    let p =
        serde_json::from_value::<crate::playback::model::SeekParams>(params.unwrap_or(Value::Null))
            .map_err(|_| JsonRpcError {
                code: ERR_INVALID_PARAMS,
                message: "Invalid playback.seek parameters".into(),
                data: Some(serde_json::json!({"code":"INVALID_SEEK"})),
            })?;
    let result = crate::playback::commands::PlaybackCommandService::new(
        state.playback.clone(),
        state.server_manager.clone(),
        state.db.clone(),
        state.sync_operation_manager.clone(),
    )
    .rpc_seek(p, mutation_guard)
    .await
    .map_err(playback_error)?;
    Ok(serde_json::json!({"data":result}))
}

async fn handle_playback_retry_restore(
    state: &AppState,
    params: Option<Value>,
    mutation_guard: Option<crate::sync::MutationGuard>,
) -> Result<Value, JsonRpcError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Params {
        schema_version: u32,
    }
    let p = serde_json::from_value::<Params>(params.unwrap_or(Value::Null)).map_err(|_| {
        JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Invalid playback.retryRestore parameters".into(),
            data: Some(serde_json::json!({"code":"INVALID_SESSION"})),
        }
    })?;
    if p.schema_version != crate::playback::model::SCHEMA_VERSION {
        return Err(playback_error(crate::playback::session::PlaybackError {
            code: "UNSUPPORTED_PLAYBACK_VERSION",
            message: "unsupported playback schema version",
            conflict: false,
            authoritative: None,
        }));
    }
    let playback = state.playback.clone();
    tokio::task::spawn_blocking(move || playback.retry_restore_with_guard(mutation_guard))
        .await
        .map_err(|_| JsonRpcError {
            code: -32603,
            message: "Playback owner task failed".into(),
            data: Some(serde_json::json!({"code":"PERSISTENCE_FAILED"})),
        })?
        .map(|data| serde_json::json!({"data":data}))
        .map_err(playback_error)
}

async fn handle_server_probe(params: Option<Value>) -> Result<Value, JsonRpcError> {
    let url = params
        .as_ref()
        .and_then(|p| p["url"].as_str())
        .ok_or(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: hifimule_i18n::t("error.missing_url"),
            data: None,
        })?;

    let server_type = crate::providers::probe_url(url).await;
    let slug = server_type_slug(server_type);
    Ok(serde_json::json!({ "serverType": slug }))
}

fn validate_server_icon(icon: &str) -> Result<(), JsonRpcError> {
    if SERVER_ICON_IDS.contains(&icon) {
        Ok(())
    } else {
        Err(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Unsupported server icon".to_string(),
            data: Some(serde_json::json!({ "allowedIcons": SERVER_ICON_IDS })),
        })
    }
}

fn optional_server_name(params: &Value) -> Result<Option<String>, JsonRpcError> {
    match params.get("name") {
        Some(Value::String(value)) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                return Err(JsonRpcError {
                    code: ERR_INVALID_PARAMS,
                    message: "Server name must not be empty".to_string(),
                    data: None,
                });
            }
            if trimmed.chars().count() > SERVER_NAME_MAX_LEN {
                return Err(JsonRpcError {
                    code: ERR_INVALID_PARAMS,
                    message: format!(
                        "Server name must be {SERVER_NAME_MAX_LEN} characters or fewer"
                    ),
                    data: None,
                });
            }
            Ok(Some(trimmed.to_string()))
        }
        Some(Value::Null) | None => Ok(None),
        Some(_) => Err(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Server name must be a string".to_string(),
            data: None,
        }),
    }
}

fn optional_server_icon_for_connect(params: &Value) -> Result<Option<String>, JsonRpcError> {
    match params.get("icon") {
        Some(Value::String(value)) => {
            validate_server_icon(value)?;
            Ok(Some(value.to_string()))
        }
        Some(Value::Null) | None => Ok(None),
        Some(_) => Err(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Server icon must be a string or null".to_string(),
            data: None,
        }),
    }
}

/// The currently selected server config row (`selected = 1`), if any.
fn current_server_config(
    state: &AppState,
) -> Result<Option<crate::db::ServerConfig>, JsonRpcError> {
    state.db.get_server_config().map_err(|error| JsonRpcError {
        code: ERR_STORAGE_ERROR,
        message: error.to_string(),
        data: None,
    })
}

/// The currently selected server's machine-local UUID, if any.
#[allow(dead_code)]
fn current_server_id(state: &AppState) -> Result<Option<String>, JsonRpcError> {
    Ok(current_server_config(state)?.map(|c| c.id))
}

/// The currently selected server's PORTABLE id (Story 2.13), if any. Used to tag
/// newly synced items and to route untagged basket items so manifest tags are
/// always portable.
fn current_server_portable_id(state: &AppState) -> Result<Option<String>, JsonRpcError> {
    Ok(current_server_config(state)?.and_then(|c| c.server_id))
}

async fn require_browse_provider(
    state: &AppState,
) -> Result<(Arc<dyn MediaProvider>, Option<String>), JsonRpcError> {
    let (local_id, portable_id) = {
        let manager = state.server_manager.read().await;
        let record = manager.selected_record().ok_or(JsonRpcError {
            code: ERR_CONNECTION_FAILED,
            message: hifimule_i18n::t("error.no_active_media_provider"),
            data: None,
        })?;
        (record.id.clone(), record.server_id.clone())
    };
    let provider = crate::server_manager::get_provider(&state.server_manager, &state.db, &local_id)
        .await
        .map_err(provider_error_to_rpc)?;
    Ok((provider, portable_id))
}

fn playback_tagged_tracks(server_id: Option<&str>, tracks: Vec<Song>) -> Vec<Value> {
    tracks
        .into_iter()
        .map(|track| {
            let mut value = serde_json::to_value(track).expect("Song serialization is infallible");
            if let Some(server_id) = server_id {
                value
                    .as_object_mut()
                    .expect("Song serializes as an object")
                    .insert("serverId".into(), Value::String(server_id.to_string()));
            }
            value
        })
        .collect()
}

fn playback_tagged_albums(server_id: Option<&str>, albums: Vec<Album>) -> Vec<Value> {
    albums
        .into_iter()
        .map(|album| {
            let mut value =
                serde_json::to_value(&album).expect("Album serialization is infallible");
            if let Some(server_id) = server_id {
                value
                    .as_object_mut()
                    .expect("Album serializes as an object")
                    .insert("serverId".into(), Value::String(server_id.to_string()));
            }
            // Credits are intentionally reduced to public display names and roles.
            // Provider identities, library scope, and cover references remain private.
            let credits = album
                .provider_metadata
                .credits
                .iter()
                .map(|credit| serde_json::json!({ "name": credit.name, "role": credit.role }))
                .collect::<Vec<_>>();
            if !credits.is_empty() {
                value
                    .as_object_mut()
                    .expect("Album serializes as an object")
                    .insert("presentationCredits".into(), Value::Array(credits));
            }
            value
        })
        .collect()
}

fn playback_tagged_album(server_id: Option<&str>, album: Album) -> Value {
    playback_tagged_albums(server_id, vec![album])
        .pop()
        .expect("one album")
}

/// Story 2.13: tag every untagged DesiredItem with the selected server's portable
/// id so manifest entries always carry the portable identity. Shared by the
/// single-server delta paths in both `provider_calculate_delta` and
/// `handle_sync_calculate_delta` — keep one definition to prevent the two from
/// drifting.
fn tag_untagged_with_selected_portable(
    state: &AppState,
    desired_items: &mut [crate::sync::DesiredItem],
) -> Result<(), JsonRpcError> {
    if let Some(portable) = current_server_portable_id(state)? {
        for item in desired_items.iter_mut() {
            if item.server_id.is_none() {
                item.server_id = Some(portable.clone());
            }
        }
    }
    Ok(())
}

async fn handle_test_connection(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let params = params.ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Invalid params".to_string(),
        data: None,
    })?;
    let url = params["url"].as_str().ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: hifimule_i18n::t("error.missing_url"),
        data: None,
    })?;

    let token = params["token"].as_str().ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing token".to_string(),
        data: None,
    })?;

    match state.jellyfin_client.test_connection(url, token).await {
        Ok(info) => Ok(serde_json::to_value(info).unwrap()),
        Err(e) => Err(JsonRpcError {
            code: ERR_CONNECTION_FAILED,
            message: e.to_string(),
            data: None,
        }),
    }
}

/// Returns the selected server's provider (lazily connecting on first use), or
/// `NotConnected` if no server is selected. All existing `browse.*`/`sync.*`/
/// playlist/scrobble call sites keep calling this unchanged (AC13).
pub async fn require_provider(state: &AppState) -> Result<Arc<dyn MediaProvider>, JsonRpcError> {
    match crate::server_manager::selected_provider(&state.server_manager, &state.db).await {
        Some(Ok(provider)) => Ok(provider),
        Some(Err(error)) => Err(provider_error_to_rpc(error)),
        None => Err(JsonRpcError {
            code: ERR_CONNECTION_FAILED,
            message: hifimule_i18n::t("error.no_active_media_provider"),
            data: None,
        }),
    }
}

/// Returns the provider for a specific server id (routing primitive for
/// multi-provider sync/auto-fill/playlist work). Lazily connects on first use.
pub async fn get_provider_for_server(
    state: &AppState,
    server_id: &str,
) -> Result<Arc<dyn MediaProvider>, JsonRpcError> {
    crate::server_manager::get_provider(&state.server_manager, &state.db, server_id)
        .await
        .map_err(provider_error_to_rpc)
}

/// Resolves a PORTABLE `server_id` (Story 2.13) to its provider, mapping portable →
/// machine-local id and reusing the existing per-local-id provider cache. Sync
/// routing uses this so basket/manifest items tagged with the portable id reach the
/// correct provider.
pub async fn get_provider_by_server_id_for(
    state: &AppState,
    server_id: &str,
) -> Result<Arc<dyn MediaProvider>, JsonRpcError> {
    crate::server_manager::get_provider_by_server_id(&state.server_manager, &state.db, server_id)
        .await
        .map_err(provider_error_to_rpc)
}

fn storage_error_to_rpc(error: impl std::fmt::Display) -> JsonRpcError {
    JsonRpcError {
        code: ERR_STORAGE_ERROR,
        message: error.to_string(),
        data: None,
    }
}

fn provider_error_to_rpc(error: ProviderError) -> JsonRpcError {
    match error {
        ProviderError::Auth(msg) => JsonRpcError {
            // AC11: a distinct code lets the UI surface a server-scoped re-auth
            // prompt instead of a generic connection error.
            code: ERR_UNAUTHORIZED,
            message: msg,
            data: Some(serde_json::json!({
                "unauthorized": true,
                "i18nKey": "error.unauthorized",
            })),
        },
        ProviderError::UnsupportedCapability(msg) => JsonRpcError {
            code: ERR_UNSUPPORTED_CAPABILITY,
            message: msg,
            data: None,
        },
        ProviderError::NotFound { item_type, id } => JsonRpcError {
            code: ERR_NOT_FOUND,
            message: format!("{item_type} not found: {id}"),
            data: None,
        },
        ProviderError::Forbidden => JsonRpcError {
            code: ERR_CONNECTION_FAILED,
            message: "Provider permission denied".into(),
            data: Some(serde_json::json!({ "errorCode": "PROVIDER_FORBIDDEN" })),
        },
        ProviderError::StaleConfiguration(_) => JsonRpcError {
            code: ERR_NOT_FOUND,
            message: "Provider configuration is stale".into(),
            data: Some(serde_json::json!({ "errorCode": "STALE_CONFIGURATION" })),
        },
        ProviderError::RateLimited {
            retry_after_seconds,
        } => JsonRpcError {
            code: ERR_CONNECTION_FAILED,
            message: "Provider rate limit reached".into(),
            data: Some(serde_json::json!({
                "errorCode": "RATE_LIMITED",
                "retryAfterSeconds": retry_after_seconds
            })),
        },
        _ => JsonRpcError {
            code: ERR_INTERNAL_ERROR,
            message: error.to_string(),
            data: None,
        },
    }
}

fn server_connect_error_to_rpc(error: ProviderError) -> JsonRpcError {
    match error {
        ProviderError::UnsupportedCapability(message)
            if message == "Unknown server type at this URL" =>
        {
            JsonRpcError {
                code: ERR_CONNECTION_FAILED,
                message: hifimule_i18n::t("error.unknown_server_type"),
                data: Some(serde_json::json!({ "i18nKey": "error.unknown_server_type" })),
            }
        }
        other => JsonRpcError {
            code: ERR_CONNECTION_FAILED,
            message: crate::providers::subsonic::sanitize_subsonic_message(&other.to_string()),
            data: None,
        },
    }
}

fn browse_pagination(params: &Option<Value>) -> (u32, u32) {
    let offset = params
        .as_ref()
        .and_then(|p| p["startIndex"].as_u64())
        .unwrap_or(0) as u32;
    let limit = params
        .as_ref()
        .and_then(|p| p["limit"].as_u64())
        .unwrap_or(50) as u32;
    (offset, limit)
}

async fn handle_browse_list_modes(state: &AppState) -> Result<Value, JsonRpcError> {
    let provider = require_provider(state).await?;
    let caps = provider.capabilities();
    let modes: Vec<Value> = caps
        .browse
        .list_modes
        .iter()
        .map(|m| serde_json::to_value(m).unwrap_or(Value::Null))
        .collect();
    Ok(serde_json::json!({ "modes": modes }))
}

async fn handle_browse_list_artists(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let library_id = params
        .as_ref()
        .and_then(|p| p["libraryId"].as_str())
        .map(str::to_owned);
    let letter = params
        .as_ref()
        .and_then(|p| p["letter"].as_str())
        .map(str::to_owned);
    let offset = params
        .as_ref()
        .and_then(|p| p["startIndex"].as_u64())
        .unwrap_or(0) as u32;
    let limit = params
        .as_ref()
        .and_then(|p| p["limit"].as_u64())
        .unwrap_or(50) as u32;
    let provider = require_provider(state).await?;
    let (artists, total) = provider
        .list_artists(library_id.as_deref(), letter.as_deref(), offset, limit)
        .await
        .map_err(provider_error_to_rpc)?;
    Ok(serde_json::json!({ "artists": artists, "total": total }))
}

async fn handle_browse_get_artist(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let artist_id = params
        .as_ref()
        .and_then(|p| p["artistId"].as_str())
        .ok_or(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Missing artistId".to_string(),
            data: None,
        })?
        .to_owned();
    let (provider, server_id) = require_browse_provider(state).await?;
    let result = provider
        .get_artist(&artist_id)
        .await
        .map_err(provider_error_to_rpc)?;
    Ok(
        serde_json::json!({ "artist": result.artist, "albums": playback_tagged_albums(server_id.as_deref(), result.albums) }),
    )
}

async fn handle_browse_list_albums(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let library_id = params
        .as_ref()
        .and_then(|p| p["libraryId"].as_str())
        .map(str::to_owned);
    let letter = params
        .as_ref()
        .and_then(|p| p["letter"].as_str())
        .map(str::to_owned);
    let offset = params
        .as_ref()
        .and_then(|p| p["startIndex"].as_u64())
        .unwrap_or(0) as u32;
    let limit = params
        .as_ref()
        .and_then(|p| p["limit"].as_u64())
        .unwrap_or(50) as u32;
    let (provider, server_id) = require_browse_provider(state).await?;
    let (albums, total) = provider
        .list_albums(library_id.as_deref(), letter.as_deref(), offset, limit)
        .await
        .map_err(provider_error_to_rpc)?;
    Ok(
        serde_json::json!({ "albums": playback_tagged_albums(server_id.as_deref(), albums), "total": total }),
    )
}

async fn handle_browse_get_album(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let album_id = params
        .as_ref()
        .and_then(|p| p["albumId"].as_str())
        .ok_or(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Missing albumId".to_string(),
            data: None,
        })?
        .to_owned();
    let (provider, server_id) = require_browse_provider(state).await?;
    let result = provider
        .get_album(&album_id)
        .await
        .map_err(provider_error_to_rpc)?;
    let chapters = result.provider_metadata.chapters.iter().map(|chapter| {
        serde_json::json!({ "startSeconds": chapter.start_seconds, "endSeconds": chapter.end_seconds })
    }).collect::<Vec<_>>();
    let mut value = serde_json::json!({
        "album": playback_tagged_album(server_id.as_deref(), result.album),
        "tracks": playback_tagged_tracks(server_id.as_deref(), result.tracks),
    });
    if !chapters.is_empty() {
        value["chapters"] = serde_json::json!(chapters);
    }
    Ok(value)
}

async fn podcast_browse_provider(state: &AppState) -> Result<Arc<dyn MediaProvider>, JsonRpcError> {
    let provider = require_provider(state).await?;
    if !provider
        .capabilities()
        .browse
        .list_modes
        .contains(&BrowseMode::Podcasts)
    {
        return Err(JsonRpcError {
            code: ERR_UNSUPPORTED_CAPABILITY,
            message: "Podcast browsing unavailable".into(),
            data: None,
        });
    }
    Ok(provider)
}

async fn handle_browse_list_podcast_shows(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let offset = params
        .as_ref()
        .and_then(|p| p["startIndex"].as_u64())
        .unwrap_or(0);
    let limit = params
        .as_ref()
        .and_then(|p| p["limit"].as_u64())
        .unwrap_or(50);
    if offset > u32::MAX as u64 || !(1..=100).contains(&limit) || offset % limit != 0 {
        return Err(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Invalid podcast page".into(),
            data: None,
        });
    }
    let (shows, total) = podcast_browse_provider(state)
        .await?
        .list_podcast_shows(offset as u32, limit as u32)
        .await
        .map_err(provider_error_to_rpc)?;
    Ok(serde_json::json!({ "shows": shows, "total": total }))
}

async fn handle_browse_list_recent_podcast_episodes(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let offset = params
        .as_ref()
        .and_then(|p| p["startIndex"].as_u64())
        .unwrap_or(0);
    let limit = params
        .as_ref()
        .and_then(|p| p["limit"].as_u64())
        .unwrap_or(50);
    if offset > u32::MAX as u64 || !(1..=100).contains(&limit) || offset % limit != 0 {
        return Err(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Invalid podcast page".into(),
            data: None,
        });
    }
    let provider = podcast_browse_provider(state).await?;
    let (episodes, total, source_count) = provider
        .list_recent_podcast_episodes(offset as u32, limit as u32)
        .await
        .map_err(provider_error_to_rpc)?;
    Ok(serde_json::json!({ "episodes": episodes, "total": total, "sourceCount": source_count }))
}

async fn handle_browse_get_podcast_show(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let id = params
        .as_ref()
        .and_then(|p| p["showId"].as_str())
        .filter(|id| !id.is_empty())
        .ok_or(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Missing showId".into(),
            data: None,
        })?;
    let offset = params
        .as_ref()
        .and_then(|p| p["startIndex"].as_u64())
        .unwrap_or(0);
    let limit = params
        .as_ref()
        .and_then(|p| p["limit"].as_u64())
        .unwrap_or(50);
    if offset > u32::MAX as u64 || !(1..=5_000).contains(&limit) {
        return Err(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Invalid episode page".into(),
            data: None,
        });
    }
    let detail = podcast_browse_provider(state)
        .await?
        .get_podcast_show(id)
        .await
        .map_err(provider_error_to_rpc)?;
    let total = detail.episodes.len();
    let episodes = detail
        .episodes
        .into_iter()
        .skip(offset as usize)
        .take(limit as usize)
        .collect::<Vec<_>>();
    Ok(
        serde_json::json!({ "show": detail.show, "episodes": episodes, "total": total, "possiblyTruncated": detail.possibly_truncated }),
    )
}

async fn handle_browse_get_podcast_episode(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let id = params
        .as_ref()
        .and_then(|p| p["episodeId"].as_str())
        .filter(|id| !id.is_empty())
        .ok_or(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Missing episodeId".into(),
            data: None,
        })?;
    let episode = podcast_browse_provider(state)
        .await?
        .get_podcast_episode(id)
        .await
        .map_err(provider_error_to_rpc)?;
    Ok(serde_json::json!({ "episode": episode }))
}

async fn handle_browse_list_playlists(state: &AppState) -> Result<Value, JsonRpcError> {
    let (provider, server_id) = require_browse_provider(state).await?;
    let playlists = provider
        .list_playlists()
        .await
        .map_err(provider_error_to_rpc)?;
    let playlists = playlists
        .into_iter()
        .map(|playlist| {
            let mut value = serde_json::to_value(playlist).expect("playlist is serializable");
            if let Some(server_id) = server_id.as_deref() {
                value["serverId"] = serde_json::json!(server_id);
            }
            value
        })
        .collect::<Vec<_>>();
    Ok(serde_json::json!({ "playlists": playlists }))
}

async fn handle_browse_get_playlist(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let playlist_id = params
        .as_ref()
        .and_then(|p| p["playlistId"].as_str())
        .ok_or(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Missing playlistId".to_string(),
            data: None,
        })?
        .to_owned();
    let (provider, server_id) = require_browse_provider(state).await?;
    let result = provider
        .get_playlist(&playlist_id)
        .await
        .map_err(provider_error_to_rpc)?;
    Ok(
        serde_json::json!({ "playlist": result.playlist, "tracks": playback_tagged_tracks(server_id.as_deref(), result.tracks) }),
    )
}

async fn handle_browse_list_genres(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let library_id = params
        .as_ref()
        .and_then(|p| p["libraryId"].as_str())
        .map(str::to_owned);
    let (offset, limit) = browse_pagination(&params);
    let provider = require_provider(state).await?;

    let t = std::time::Instant::now();
    let (genres, total) = provider
        .list_genres(library_id.as_deref(), offset, limit)
        .await
        .map_err(provider_error_to_rpc)?;
    crate::daemon_log!(
        "[browse.listGenres] {}ms total={} page={} offset={} limit={}",
        t.elapsed().as_millis(),
        total,
        genres.len(),
        offset,
        limit
    );

    Ok(serde_json::json!({ "genres": genres, "total": total }))
}

async fn handle_browse_get_genre(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let genre_id = params
        .as_ref()
        .and_then(|p| p["genreId"].as_str())
        .ok_or(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Missing genreId".to_string(),
            data: None,
        })?
        .to_owned();
    let (offset, limit) = browse_pagination(&params);
    let (provider, server_id) = require_browse_provider(state).await?;
    let (genres, _) = provider
        .list_genres(None, 0, 10_000)
        .await
        .map_err(provider_error_to_rpc)?;
    let genre = genres
        .into_iter()
        .find(|g| g.id == genre_id)
        .ok_or(JsonRpcError {
            code: ERR_NOT_FOUND,
            message: format!("Genre not found: {genre_id}"),
            data: None,
        })?;
    let (tracks, total) = provider
        .get_genre_tracks(&genre_id, offset, limit)
        .await
        .map_err(provider_error_to_rpc)?;
    let total = total as u64;
    Ok(
        serde_json::json!({ "genre": genre, "tracks": playback_tagged_tracks(server_id.as_deref(), tracks), "total": total }),
    )
}

async fn handle_browse_list_recently_added(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let library_id = params
        .as_ref()
        .and_then(|p| p["libraryId"].as_str())
        .map(str::to_owned);
    let (offset, limit) = browse_pagination(&params);
    let (provider, server_id) = require_browse_provider(state).await?;
    let (albums, total) = provider
        .list_recently_added(library_id.as_deref(), offset, limit)
        .await
        .map_err(provider_error_to_rpc)?;
    let total = total as u64;
    Ok(
        serde_json::json!({ "albums": playback_tagged_albums(server_id.as_deref(), albums), "total": total }),
    )
}

async fn handle_browse_list_frequently_played(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let library_id = params
        .as_ref()
        .and_then(|p| p["libraryId"].as_str())
        .map(str::to_owned);
    let (offset, limit) = browse_pagination(&params);
    let (provider, server_id) = require_browse_provider(state).await?;
    let (tracks, total) = provider
        .list_frequently_played(library_id.as_deref(), offset, limit)
        .await
        .map_err(provider_error_to_rpc)?;
    let total = total as u64;
    Ok(
        serde_json::json!({ "tracks": playback_tagged_tracks(server_id.as_deref(), tracks), "total": total }),
    )
}

async fn handle_browse_list_recently_played(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let library_id = params
        .as_ref()
        .and_then(|p| p["libraryId"].as_str())
        .map(str::to_owned);
    let (offset, limit) = browse_pagination(&params);
    let (provider, server_id) = require_browse_provider(state).await?;
    let (tracks, total) = provider
        .list_recently_played(library_id.as_deref(), offset, limit)
        .await
        .map_err(provider_error_to_rpc)?;
    let total = total as u64;
    Ok(
        serde_json::json!({ "tracks": playback_tagged_tracks(server_id.as_deref(), tracks), "total": total }),
    )
}

async fn handle_browse_list_favorites(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let library_id = params
        .as_ref()
        .and_then(|p| p["libraryId"].as_str())
        .map(str::to_owned);
    let (offset, limit) = browse_pagination(&params);
    let (provider, server_id) = require_browse_provider(state).await?;
    let (tracks, total) = provider
        .list_favorites(library_id.as_deref(), offset, limit)
        .await
        .map_err(provider_error_to_rpc)?;
    let total = total as u64;
    Ok(
        serde_json::json!({ "tracks": playback_tagged_tracks(server_id.as_deref(), tracks), "total": total }),
    )
}

async fn handle_browse_list_favorite_items(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let library_id = params
        .as_ref()
        .and_then(|p| p["libraryId"].as_str())
        .map(str::to_owned);
    let (provider, server_id) = require_browse_provider(state).await?;
    let favorites = provider
        .list_favorite_items(library_id.as_deref())
        .await
        .map_err(provider_error_to_rpc)?;
    Ok(serde_json::json!({
        "artists": favorites.artists,
        "albums": playback_tagged_albums(server_id.as_deref(), favorites.albums),
        "tracks": playback_tagged_tracks(server_id.as_deref(), favorites.songs),
    }))
}

async fn handle_browse_list_tracks(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let library_id = params
        .as_ref()
        .and_then(|p| p["libraryId"].as_str())
        .map(str::to_owned);
    let artist_id = params
        .as_ref()
        .and_then(|p| p["artistId"].as_str())
        .map(str::to_owned);
    let album_id = params
        .as_ref()
        .and_then(|p| p["albumId"].as_str())
        .map(str::to_owned);
    let letter = params
        .as_ref()
        .and_then(|p| p["letter"].as_str())
        .map(str::to_owned);
    let (start_index, limit) = browse_pagination(&params);
    let (provider, server_id) = require_browse_provider(state).await?;
    if !provider
        .capabilities()
        .browse
        .list_modes
        .contains(&BrowseMode::Tracks)
    {
        return Err(JsonRpcError {
            code: ERR_UNSUPPORTED_CAPABILITY,
            message: hifimule_i18n::t("error.tracks_mode_unsupported"),
            data: None,
        });
    }
    let filter = TrackListFilter {
        library_id,
        artist_id,
        album_id,
        letter,
        start_index,
        limit,
    };
    let page = provider
        .list_tracks(filter)
        .await
        .map_err(provider_error_to_rpc)?;
    let tracks = playback_tagged_tracks(server_id.as_deref(), page.tracks);
    Ok(serde_json::json!({
        "tracks": tracks,
        "total": page.total,
        "startIndex": page.start_index,
        "limit": page.limit,
    }))
}

async fn handle_browse_search(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let (provider, server_id) = require_browse_provider(state).await?;
    let query = params
        .as_ref()
        .and_then(|p| p["query"].as_str())
        .ok_or(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Missing query".to_string(),
            data: None,
        })?
        .to_owned();
    if query.len() > 256 {
        return Err(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Search query too long".into(),
            data: None,
        });
    }
    if provider
        .capabilities()
        .browse
        .list_modes
        .contains(&BrowseMode::Podcasts)
    {
        if query.len() > 256 {
            return Err(JsonRpcError {
                code: ERR_INVALID_PARAMS,
                message: "Podcast query too long".into(),
                data: None,
            });
        }
        let result = provider
            .search_podcasts(&query)
            .await
            .map_err(provider_error_to_rpc)?;
        return Ok(serde_json::json!({
            "shows": result.shows, "episodes": result.episodes,
            "possiblyTruncated": result.possibly_truncated,
        }));
    }
    // An empty/whitespace query would be forwarded to the provider as an
    // unbounded search; short-circuit to an empty result set instead.
    if query.trim().is_empty() {
        return Ok(
            serde_json::json!({ "artists": [], "albums": [], "tracks": [], "possiblyTruncated": false }),
        );
    }
    let result = provider
        .search(&query)
        .await
        .map_err(provider_error_to_rpc)?;
    Ok(serde_json::json!({
        "artists": result.artists,
        "tracks": playback_tagged_tracks(server_id.as_deref(), result.songs),
        "albums": playback_tagged_albums(server_id.as_deref(), result.albums),
        "possiblyTruncated": result.possibly_truncated,
    }))
}

async fn handle_playlist_create(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let provider = require_provider(state).await?;
    if !provider.capabilities().supports_playlist_write {
        return Err(JsonRpcError {
            code: ERR_UNSUPPORTED_CAPABILITY,
            message: "Connected provider does not support playlist write".to_string(),
            data: None,
        });
    }
    let params = params.ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing params".to_string(),
        data: None,
    })?;
    let name = params["name"]
        .as_str()
        .ok_or(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Missing name".to_string(),
            data: None,
        })?
        .to_owned();
    let raw_ids = params["itemIds"].as_array().ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing or invalid itemIds array".to_string(),
        data: None,
    })?;
    let item_ids: Vec<String> = raw_ids
        .iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .filter(|id| !crate::device::is_auto_fill_slot_id(id))
        .collect();

    // Cross-server scope check (AC33): when the caller supplies per-item serverIds
    // (`items: [{ id, serverId }]`), every item must belong to the selected server.
    // The UI pre-filters (AC34) so this normally never trips; it is the daemon-side
    // guard against a basket holding items from another server.
    if let Some(items) = params.get("items").and_then(Value::as_array) {
        // Items carry the portable serverId (Story 2.13) — compare against the
        // selected server's portable id.
        let selected_id = current_server_portable_id(state)?;
        for item in items {
            let item_server = item.get("serverId").and_then(Value::as_str);
            if let (Some(item_server), Some(selected)) = (item_server, selected_id.as_deref())
                && item_server != selected
            {
                return Err(JsonRpcError {
                    code: ERR_CROSS_SERVER_CONFLICT,
                    message: "Playlist creation requires all items to be from the selected server. Switch server or remove cross-server items.".to_string(),
                    data: Some(serde_json::json!({ "i18nKey": "error.cross_server_playlist" })),
                });
            }
        }
    }

    let mut track_ids: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut skipped_item_ids: Vec<String> = Vec::new();
    for item_id in &item_ids {
        // Skip-and-continue: one unresolvable item (deleted entity, transient
        // error, stale basket entry) must not abort the whole create. Record it
        // and report the skipped IDs in the response instead.
        let (tracks, _playlist) = match provider_sync_items_for_id(provider.clone(), item_id).await
        {
            Ok(resolved) => resolved,
            Err(e) => {
                eprintln!(
                    "[Playlist] Skipping unresolvable item '{}' during playlist.create: {}",
                    item_id, e.message
                );
                skipped_item_ids.push(item_id.clone());
                continue;
            }
        };
        for track in tracks {
            if seen.insert(track.jellyfin_id.clone()) {
                track_ids.push(track.jellyfin_id);
            }
        }
    }

    if !item_ids.is_empty() && track_ids.is_empty() {
        return Err(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "No valid tracks found in selection to create a playlist".to_string(),
            data: None,
        });
    }

    let playlist_id = provider
        .create_playlist(&name, &track_ids)
        .await
        .map_err(provider_error_to_rpc)?;
    Ok(serde_json::json!({ "playlistId": playlist_id, "skippedItemIds": skipped_item_ids }))
}

async fn handle_playlist_add_items(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let provider = require_provider(state).await?;
    if !provider.capabilities().supports_playlist_write {
        return Err(JsonRpcError {
            code: ERR_UNSUPPORTED_CAPABILITY,
            message: "Connected provider does not support playlist write".to_string(),
            data: None,
        });
    }
    let params = params.ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing params".to_string(),
        data: None,
    })?;
    let playlist_id = params["playlistId"]
        .as_str()
        .ok_or(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Missing playlistId".to_string(),
            data: None,
        })?
        .to_owned();
    let raw_ids = params["itemIds"].as_array().ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing or invalid itemIds array".to_string(),
        data: None,
    })?;
    let item_ids: Vec<String> = raw_ids
        .iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect();

    let mut track_ids: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for item_id in &item_ids {
        let (tracks, _) = match provider_sync_items_for_id(provider.clone(), item_id).await {
            Ok(resolved) => resolved,
            Err(e) => {
                eprintln!(
                    "[Playlist] Skipping unresolvable item '{}' during playlist.addItems: {}",
                    item_id, e.message
                );
                continue;
            }
        };
        for track in tracks {
            if seen.insert(track.jellyfin_id.clone()) {
                track_ids.push(track.jellyfin_id);
            }
        }
    }

    if track_ids.is_empty() {
        return Err(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "No valid tracks found in selection".to_string(),
            data: None,
        });
    }

    provider
        .add_to_playlist(&playlist_id, &track_ids)
        .await
        .map_err(provider_error_to_rpc)?;
    Ok(serde_json::json!({ "ok": true }))
}

async fn handle_playlist_add_tracks(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let provider = require_provider(state).await?;
    if !provider.capabilities().supports_playlist_write {
        return Err(JsonRpcError {
            code: ERR_UNSUPPORTED_CAPABILITY,
            message: "Connected provider does not support playlist write".to_string(),
            data: None,
        });
    }
    let params = params.ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing params".to_string(),
        data: None,
    })?;
    let playlist_id = params["playlistId"]
        .as_str()
        .ok_or(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Missing playlistId".to_string(),
            data: None,
        })?
        .to_owned();
    let raw_ids = params["trackIds"].as_array().ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing or invalid trackIds array".to_string(),
        data: None,
    })?;
    let track_ids: Vec<String> = raw_ids
        .iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect();
    provider
        .add_to_playlist(&playlist_id, &track_ids)
        .await
        .map_err(provider_error_to_rpc)?;
    Ok(serde_json::json!({ "ok": true }))
}

async fn handle_playlist_remove_tracks(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let provider = require_provider(state).await?;
    if !provider.capabilities().supports_playlist_write {
        return Err(JsonRpcError {
            code: ERR_UNSUPPORTED_CAPABILITY,
            message: "Connected provider does not support playlist write".to_string(),
            data: None,
        });
    }
    let params = params.ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing params".to_string(),
        data: None,
    })?;
    let playlist_id = params["playlistId"]
        .as_str()
        .ok_or(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Missing playlistId".to_string(),
            data: None,
        })?
        .to_owned();
    let raw_ids = params["trackIds"].as_array().ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing or invalid trackIds array".to_string(),
        data: None,
    })?;
    let track_ids: Vec<String> = raw_ids
        .iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect();
    provider
        .remove_from_playlist(&playlist_id, &track_ids)
        .await
        .map_err(provider_error_to_rpc)?;
    Ok(serde_json::json!({ "ok": true }))
}

async fn handle_playlist_delete(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let provider = require_provider(state).await?;
    if !provider.capabilities().supports_playlist_write {
        return Err(JsonRpcError {
            code: ERR_UNSUPPORTED_CAPABILITY,
            message: "Connected provider does not support playlist write".to_string(),
            data: None,
        });
    }
    let params = params.ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing params".to_string(),
        data: None,
    })?;
    let playlist_id = params["playlistId"]
        .as_str()
        .ok_or(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Missing playlistId".to_string(),
            data: None,
        })?
        .to_owned();
    provider
        .delete_playlist(&playlist_id)
        .await
        .map_err(provider_error_to_rpc)?;
    Ok(serde_json::json!({ "ok": true }))
}

async fn handle_playlist_rename(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let provider = require_provider(state).await?;
    if !provider.capabilities().supports_playlist_write {
        return Err(JsonRpcError {
            code: ERR_UNSUPPORTED_CAPABILITY,
            message: "Connected provider does not support playlist write".to_string(),
            data: None,
        });
    }
    let params = params.ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing params".to_string(),
        data: None,
    })?;
    let playlist_id = params["playlistId"]
        .as_str()
        .ok_or(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Missing playlistId".to_string(),
            data: None,
        })?
        .to_owned();
    let name = params["name"]
        .as_str()
        .ok_or(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Missing name".to_string(),
            data: None,
        })?
        .trim()
        .to_owned();
    if name.is_empty() {
        return Err(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Playlist name must not be empty".to_string(),
            data: None,
        });
    }
    provider
        .rename_playlist(&playlist_id, &name)
        .await
        .map_err(provider_error_to_rpc)?;
    Ok(serde_json::json!({ "ok": true }))
}

async fn handle_playlist_reorder(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let provider = require_provider(state).await?;
    if !provider.capabilities().supports_playlist_write {
        return Err(JsonRpcError {
            code: ERR_UNSUPPORTED_CAPABILITY,
            message: "Connected provider does not support playlist write".to_string(),
            data: None,
        });
    }
    let params = params.ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing params".to_string(),
        data: None,
    })?;
    let playlist_id = params["playlistId"]
        .as_str()
        .ok_or(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Missing playlistId".to_string(),
            data: None,
        })?
        .to_owned();
    let raw_ids = params["trackIds"].as_array().ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing or invalid trackIds array".to_string(),
        data: None,
    })?;
    // Reject non-string entries rather than silently dropping them: a dropped id would
    // shrink the requested order and, on the Subsonic replace path, could remove a track.
    let mut track_ids: Vec<String> = Vec::with_capacity(raw_ids.len());
    for v in raw_ids {
        let s = v.as_str().ok_or(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "trackIds must contain only strings".to_string(),
            data: None,
        })?;
        track_ids.push(s.to_string());
    }
    provider
        .reorder_playlist(&playlist_id, &track_ids)
        .await
        .map_err(provider_error_to_rpc)?;
    Ok(serde_json::json!({ "ok": true }))
}

async fn handle_server_connect(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let params = params.ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Invalid params".to_string(),
        data: None,
    })?;

    let url = params["url"].as_str().ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: hifimule_i18n::t("error.missing_url"),
        data: None,
    })?;
    let server_type = params["serverType"].as_str().unwrap_or("auto");
    if server_type == "audiobookshelf" {
        return Err(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Audiobookshelf requires library selection".to_string(),
            data: Some(serde_json::json!({
                "errorCode": "LIBRARY_SELECTION_REQUIRED",
                "i18nKey": "error.audiobookshelf.library_selection_required"
            })),
        });
    }
    let username = params["username"].as_str().ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing username".to_string(),
        data: None,
    })?;
    let password = params["password"].as_str().ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing password".to_string(),
        data: None,
    })?;
    let name = optional_server_name(&params)?;
    let icon = optional_server_icon_for_connect(&params)?;

    let hint = parse_server_type_hint(server_type)?;
    let credentials = ProviderCredentials {
        server_url: url.to_string(),
        credential: CredentialKind::Password {
            username: username.to_string(),
            password: password.to_string(),
        },
    };
    let provider = crate::providers::connect(url, &credentials, hint)
        .await
        .map_err(server_connect_error_to_rpc)?;
    let normalized_type = server_type_slug(provider.server_type())
        .ok_or(JsonRpcError {
            code: ERR_CONNECTION_FAILED,
            message: hifimule_i18n::t("error.unknown_server_type"),
            data: Some(serde_json::json!({ "i18nKey": "error.unknown_server_type" })),
        })?
        .to_string();
    let version = provider.server_version().map(str::to_string);
    // Server-reported stable id (Jellyfin System/Info.Id) drives the portable
    // server_id `rid:` basis (Story 2.13). Subsonic/OpenSubsonic → None (URL basis).
    let reported_id = provider.server_reported_id().map(str::to_string);

    // Persist the server row (upsert by normalized URL, AC5) and obtain its
    // machine-local UUID. upsert_server also (re-)derives the portable server_id.
    let local_id = state
        .db
        .upsert_server(
            url,
            &normalized_type,
            username,
            version.as_deref(),
            name.as_deref(),
            icon.as_deref(),
            reported_id.as_deref(),
        )
        .map_err(|error| JsonRpcError {
            code: ERR_STORAGE_ERROR,
            message: error.to_string(),
            data: None,
        })?;

    // Did this server end up selected? (upsert auto-selects the first-ever server.)
    let is_selected = current_server_config(state)?
        .map(|c| c.id == local_id)
        .unwrap_or(false);

    // The deterministic portable id persisted by upsert (Story 2.13). Must be
    // Some after a successful upsert — any DB error or missing row is a server
    // state inconsistency: the UI keys its active-server + basket tagging on this
    // value, so returning a null serverId silently breaks tagging for the session.
    let portable_id = state
        .db
        .get_server(&local_id)
        .map_err(storage_error_to_rpc)?
        .and_then(|c| c.server_id)
        .ok_or(JsonRpcError {
            code: ERR_STORAGE_ERROR,
            message: "server upsert did not persist a portable server_id".to_string(),
            data: None,
        })?;

    // Store the credential in the UUID-keyed vault (AC18); update config.json only
    // when this server is the selected one, so the static get_credentials() resolves
    // the active Jellyfin session correctly.
    match provider.server_type() {
        crate::providers::ServerType::Jellyfin => {
            let token = provider
                .access_token()
                .ok_or(JsonRpcError {
                    code: ERR_CONNECTION_FAILED,
                    message: "Jellyfin provider missing access token".to_string(),
                    data: None,
                })?
                .to_string();
            let user_id = provider
                .provider_user_id()
                .ok_or(JsonRpcError {
                    code: ERR_CONNECTION_FAILED,
                    message: "Jellyfin provider missing user ID".to_string(),
                    data: None,
                })?
                .to_string();
            if is_selected {
                CredentialManager::save_jellyfin_session(&local_id, url, &token, Some(&user_id))
                    .map_err(storage_error_to_rpc)?;
            } else {
                CredentialManager::save_server_credential(
                    &local_id,
                    &crate::api::ServerCredentials {
                        token_or_password: token,
                        user_id: Some(user_id),
                    },
                )
                .map_err(storage_error_to_rpc)?;
            }
        }
        crate::providers::ServerType::Subsonic | crate::providers::ServerType::OpenSubsonic => {
            CredentialManager::save_server_credential(
                &local_id,
                &crate::api::ServerCredentials {
                    token_or_password: password.to_string(),
                    user_id: None,
                },
            )
            .map_err(storage_error_to_rpc)?;
            if is_selected {
                CredentialManager::set_config_selected_server(&local_id)
                    .map_err(storage_error_to_rpc)?;
            }
        }
        crate::providers::ServerType::Audiobookshelf => {
            return Err(JsonRpcError {
                code: ERR_INVALID_PARAMS,
                message: "Audiobookshelf requires library selection".to_string(),
                data: Some(serde_json::json!({
                    "errorCode": "LIBRARY_SELECTION_REQUIRED"
                })),
            });
        }
        crate::providers::ServerType::Unknown => {}
    }

    // Refresh the manager: reload rows, set selection, and cache the live provider.
    {
        let mut mgr = state.server_manager.write().await;
        mgr.load_from_db(&state.db);
        mgr.providers.insert(local_id.clone(), provider.clone());
    }
    *state.last_connection_check.lock().await = None;

    // Story 2.13: `serverId` now carries the PORTABLE id (semantic flip), and
    // `localId` exposes the machine-local UUID for callers that key on it.
    Ok(serde_json::json!({
        "ok": true,
        "serverId": portable_id,
        "localId": local_id,
        "serverType": normalized_type,
        "serverVersion": version,
    }))
}

fn random_setup_id() -> String {
    let mut bytes = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn purge_expired_setups(setups: &mut HashMap<String, PendingAudiobookshelfSetup>) {
    setups.retain(|_, setup| setup.created_at.elapsed() < AUDIOBOOKSHELF_SETUP_TTL);
}

fn audiobookshelf_failure_log_line(method: &str, error: &ProviderError) -> String {
    let (category, status) = match error {
        ProviderError::Auth(_) => ("authentication", None),
        ProviderError::Forbidden => ("forbidden", Some(403)),
        ProviderError::StaleConfiguration(_) | ProviderError::NotFound { .. } => {
            ("stale_configuration", Some(404))
        }
        ProviderError::RateLimited { .. } => ("rate_limited", Some(429)),
        ProviderError::UnsupportedCapability(_) => ("unsupported", None),
        ProviderError::Deserialization(_) => ("response_shape", None),
        ProviderError::Http { status, .. } => (
            if status.is_some() {
                "http"
            } else {
                "transport"
            },
            *status,
        ),
        ProviderError::Other(_) => ("internal", None),
    };
    let status = status
        .map(|value| value.to_string())
        .unwrap_or_else(|| "none".to_string());
    format!("RPC {method} failed: provider=audiobookshelf category={category} status={status}")
}

fn log_audiobookshelf_failure(method: &str, error: &ProviderError) {
    crate::daemon_log!("{}", audiobookshelf_failure_log_line(method, error));
}

fn audiobookshelf_error_to_rpc(error: ProviderError) -> JsonRpcError {
    match error {
        ProviderError::Auth(_) => JsonRpcError {
            code: ERR_UNAUTHORIZED,
            message: "Audiobookshelf authentication failed".into(),
            data: Some(serde_json::json!({
                "errorCode": "AUDIOBOOKSHELF_AUTH_FAILED",
                "i18nKey": "error.audiobookshelf.authentication"
            })),
        },
        ProviderError::Forbidden => JsonRpcError {
            code: ERR_CONNECTION_FAILED,
            message: "Audiobookshelf library access is forbidden".into(),
            data: Some(serde_json::json!({
                "errorCode": "AUDIOBOOKSHELF_LIBRARY_FORBIDDEN",
                "i18nKey": "error.audiobookshelf.forbidden"
            })),
        },
        ProviderError::StaleConfiguration(_) | ProviderError::NotFound { .. } => JsonRpcError {
            code: ERR_NOT_FOUND,
            message: "Audiobookshelf library configuration is stale".into(),
            data: Some(serde_json::json!({
                "errorCode": "AUDIOBOOKSHELF_LIBRARY_STALE",
                "i18nKey": "error.audiobookshelf.stale"
            })),
        },
        ProviderError::RateLimited {
            retry_after_seconds,
        } => JsonRpcError {
            code: ERR_CONNECTION_FAILED,
            message: "Audiobookshelf rate limit reached".into(),
            data: Some(serde_json::json!({
                "errorCode": "AUDIOBOOKSHELF_RATE_LIMITED",
                "i18nKey": "error.audiobookshelf.rate_limited",
                "retryAfterSeconds": retry_after_seconds
            })),
        },
        other => JsonRpcError {
            code: ERR_CONNECTION_FAILED,
            message: crate::providers::sanitize_secret_message(&other.to_string()),
            data: Some(serde_json::json!({
                "errorCode": "AUDIOBOOKSHELF_CONNECTION_FAILED",
                "i18nKey": "error.audiobookshelf.connection"
            })),
        },
    }
}

async fn handle_audiobookshelf_discover(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let params = params.ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Invalid params".into(),
        data: None,
    })?;
    let url = params["url"].as_str().ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: hifimule_i18n::t("error.missing_url"),
        data: None,
    })?;
    let username = params["username"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Missing username".into(),
            data: None,
        })?;
    let password = params["password"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Missing password".into(),
            data: None,
        })?;
    let discovery =
        crate::providers::audiobookshelf::AudiobookshelfProvider::discover(url, username, password)
            .await
            .map_err(|error| {
                log_audiobookshelf_failure("server.audiobookshelf.discover", &error);
                audiobookshelf_error_to_rpc(error)
            })?;
    if discovery.libraries.is_empty() {
        crate::daemon_log!(
            "RPC server.audiobookshelf.discover failed: provider=audiobookshelf category=no_libraries status=none"
        );
        return Err(JsonRpcError {
            code: ERR_NOT_FOUND,
            message: "No accessible Audiobookshelf libraries".into(),
            data: Some(serde_json::json!({
                "errorCode": "AUDIOBOOKSHELF_NO_LIBRARIES",
                "i18nKey": "error.audiobookshelf.no_libraries"
            })),
        });
    }

    let setup_id = random_setup_id();
    let mut choices = HashMap::new();
    let mut safe_choices = Vec::with_capacity(discovery.libraries.len());
    for library in discovery.libraries {
        let choice_id = random_setup_id();
        safe_choices.push(serde_json::json!({
            "choiceId": choice_id,
            "name": library.name,
            "role": library.role.slug(),
        }));
        choices.insert(choice_id, library);
    }
    let pending = PendingAudiobookshelfSetup {
        created_at: std::time::Instant::now(),
        url: url.to_string(),
        username: username.to_string(),
        password: SecretString::new(password.to_string()),
        provider: discovery.provider,
        choices,
    };
    let pending_setups = state.pending_audiobookshelf_setups();
    let mut setups = pending_setups.lock().await;
    purge_expired_setups(&mut setups);
    if setups.len() >= MAX_PENDING_AUDIOBOOKSHELF_SETUPS {
        return Err(JsonRpcError {
            code: ERR_CONNECTION_FAILED,
            message: "Too many pending Audiobookshelf setups".into(),
            data: Some(serde_json::json!({
                "errorCode": "AUDIOBOOKSHELF_SETUP_LIMIT",
                "i18nKey": "error.audiobookshelf.setup_limit"
            })),
        });
    }
    setups.insert(setup_id.clone(), pending);
    Ok(serde_json::json!({ "setupId": setup_id, "libraries": safe_choices }))
}

async fn handle_audiobookshelf_cancel(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let setup_id = params
        .as_ref()
        .and_then(|params| params["setupId"].as_str())
        .ok_or(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Missing setupId".into(),
            data: None,
        })?;
    let pending_setups = state.pending_audiobookshelf_setups();
    let mut setups = pending_setups.lock().await;
    purge_expired_setups(&mut setups);
    setups.remove(setup_id);
    Ok(serde_json::json!({ "ok": true }))
}

async fn handle_audiobookshelf_commit(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let params = params.ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Invalid params".into(),
        data: None,
    })?;
    let setup_id = params["setupId"].as_str().ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing setupId".into(),
        data: None,
    })?;
    let choice_id = params["choiceId"].as_str().ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing choiceId".into(),
        data: None,
    })?;
    let name = optional_server_name(&params)?;
    let icon = optional_server_icon_for_connect(&params)?;

    // Compare-and-remove while holding one lock: commit, replay, and cancel can
    // have only one winner. Every commit attempt consumes the setup.
    let pending = {
        let pending_setups = state.pending_audiobookshelf_setups();
        let mut setups = pending_setups.lock().await;
        purge_expired_setups(&mut setups);
        setups.remove(setup_id).ok_or(JsonRpcError {
            code: ERR_NOT_FOUND,
            message: "Audiobookshelf setup expired or was already used".into(),
            data: Some(serde_json::json!({
                "errorCode": "AUDIOBOOKSHELF_SETUP_EXPIRED",
                "i18nKey": "error.audiobookshelf.setup_expired"
            })),
        })?
    };
    let library = pending.choices.get(choice_id).ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Invalid Audiobookshelf library choice".into(),
        data: Some(serde_json::json!({
            "errorCode": "AUDIOBOOKSHELF_INVALID_CHOICE",
            "i18nKey": "error.audiobookshelf.invalid_choice"
        })),
    })?;
    let db_role = match library.role {
        crate::providers::ProviderLibraryRole::Audiobook => {
            crate::db::AudiobookshelfLibraryRole::Audiobook
        }
        crate::providers::ProviderLibraryRole::Podcast => {
            crate::db::AudiobookshelfLibraryRole::Podcast
        }
    };
    let default_name = format!(
        "{} — {}",
        library.name,
        match library.role {
            crate::providers::ProviderLibraryRole::Audiobook => "Books",
            crate::providers::ProviderLibraryRole::Podcast => "Podcasts",
        }
    );
    // Serializes scoped commits through the same lock used to publish the
    // resulting provider, keeping the preflight row lookup and DB upsert one
    // logical operation for concurrent commits of the same library.
    let mut manager = state.server_manager.write().await;
    let prior_row = state
        .db
        .find_audiobookshelf_server(&pending.url, &pending.username, &library.id)
        .map_err(storage_error_to_rpc)?;
    if let Some(row) = &prior_row
        && row.provider_library_role.as_deref() != Some(db_role.slug())
    {
        return Err(audiobookshelf_error_to_rpc(
            ProviderError::StaleConfiguration("library role changed".into()),
        ));
    }
    let local_id = prior_row
        .as_ref()
        .map(|row| row.id.clone())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let prior_credential =
        CredentialManager::find_server_credential(&local_id).map_err(storage_error_to_rpc)?;
    let server_version = pending.provider.server_version().map(str::to_owned);
    let provider = pending
        .provider
        .scope_to(library.id.clone(), library.role)
        .map_err(audiobookshelf_error_to_rpc)?;
    let portable_id = crate::db::derive_audiobookshelf_server_id(
        &crate::db::normalized_server_url(&pending.url),
        &pending.username,
        &library.id,
        db_role,
    );
    let new_credential = crate::api::ServerCredentials {
        token_or_password: pending.password.expose_secret().to_string(),
        user_id: None,
    };
    CredentialManager::save_server_credential(&local_id, &new_credential)
        .map_err(storage_error_to_rpc)?;
    let persisted = state.db.upsert_audiobookshelf_server_with_id(
        &pending.url,
        &pending.username,
        &library.id,
        db_role,
        server_version.as_deref(),
        if prior_row.is_some() {
            name.as_deref()
        } else {
            name.as_deref().or(Some(default_name.as_str()))
        },
        icon.as_deref(),
        &local_id,
    );
    if let Err(error) = persisted {
        let compensation = match prior_credential {
            Some(ref credential) => {
                CredentialManager::save_server_credential(&local_id, credential)
            }
            None => CredentialManager::remove_server_credential(&local_id),
        };
        if compensation.is_err() {
            crate::daemon_log!(
                "RPC server.audiobookshelf.commit failed: provider=audiobookshelf category=credential_compensation status=none"
            );
            return Err(storage_error_to_rpc(
                "Audiobookshelf commit and credential compensation failed",
            ));
        }
        return Err(storage_error_to_rpc(error));
    }
    manager.load_from_db(&state.db);
    manager
        .providers
        .insert(local_id.clone(), Arc::new(provider));
    drop(manager);
    *state.last_connection_check.lock().await = None;
    Ok(serde_json::json!({
        "ok": true,
        "localId": local_id,
        "serverId": portable_id,
        "serverType": "audiobookshelf",
        "libraryRole": library.role.slug(),
    }))
}

async fn handle_server_reauthenticate(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let params = params.ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Invalid params".into(),
        data: None,
    })?;
    let id = params["id"].as_str().ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing id".into(),
        data: None,
    })?;
    let password = params["password"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Missing password".into(),
            data: None,
        })?;
    let row = state
        .db
        .get_server(id)
        .map_err(storage_error_to_rpc)?
        .ok_or(JsonRpcError {
            code: ERR_NOT_FOUND,
            message: "Server not found".into(),
            data: None,
        })?;
    if row.server_type != "audiobookshelf" {
        return Err(JsonRpcError {
            code: ERR_UNSUPPORTED_CAPABILITY,
            message: "Scoped password-only re-authentication is for Audiobookshelf".into(),
            data: None,
        });
    }
    let library_id = row.provider_library_id.as_deref().ok_or_else(|| {
        audiobookshelf_error_to_rpc(ProviderError::StaleConfiguration("missing library".into()))
    })?;
    let role = match row.provider_library_role.as_deref() {
        Some("audiobook") => crate::providers::ProviderLibraryRole::Audiobook,
        Some("podcast") => crate::providers::ProviderLibraryRole::Podcast,
        _ => {
            return Err(audiobookshelf_error_to_rpc(
                ProviderError::StaleConfiguration("invalid library role".into()),
            ));
        }
    };
    let provider = crate::providers::audiobookshelf::AudiobookshelfProvider::from_stored_config(
        &row.url,
        &row.username,
        password,
        library_id,
        role,
    )
    .await
    .map_err(|error| {
        log_audiobookshelf_failure("server.reauthenticate", &error);
        audiobookshelf_error_to_rpc(error)
    })?;
    // Take the publication lock before changing the durable credential. Once
    // the save succeeds, cache publication is synchronous and cannot be
    // cancelled at an await point.
    let mut manager = state.server_manager.write().await;
    CredentialManager::save_server_credential(
        id,
        &crate::api::ServerCredentials {
            token_or_password: password.to_string(),
            user_id: None,
        },
    )
    .map_err(storage_error_to_rpc)?;
    manager.providers.insert(id.to_string(), Arc::new(provider));
    drop(manager);
    *state.last_connection_check.lock().await = None;
    Ok(serde_json::json!({ "ok": true }))
}

/// Full logout (UI "log out" / disconnect): removes ALL configured servers,
/// clears the vault and config, and resets the in-memory manager.
async fn handle_server_logout(state: &AppState) -> Result<Value, JsonRpcError> {
    state.pending_audiobookshelf_setups().lock().await.clear();
    {
        let mut mgr = state.server_manager.write().await;
        *mgr = crate::server_manager::ServerManager::new();
    }
    *state.last_connection_check.lock().await = None;

    state
        .db
        .clear_server_config()
        .map_err(storage_error_to_rpc)?;
    CredentialManager::clear_credentials().map_err(storage_error_to_rpc)?;

    Ok(serde_json::json!({ "ok": true }))
}

fn server_row_to_json(config: &crate::db::ServerConfig) -> Value {
    serde_json::json!({
        "id": config.id,
        "serverId": config.server_id,
        "url": config.url,
        "serverType": config.server_type,
        "username": config.username,
        "name": config.name,
        "icon": config.icon,
        "selected": config.selected,
        "libraryRole": config.provider_library_role,
    })
}

/// AC20: `server.list → Array<{ id, url, serverType, username, selected }>`.
async fn handle_server_list(state: &AppState) -> Result<Value, JsonRpcError> {
    let servers = state.db.list_servers().map_err(storage_error_to_rpc)?;
    let json: Vec<Value> = servers.iter().map(server_row_to_json).collect();
    Ok(serde_json::json!(json))
}

async fn handle_server_update(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let params = params.ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Invalid params".to_string(),
        data: None,
    })?;
    if params.get("url").is_some() {
        return Err(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Server URL cannot be changed by server.update".to_string(),
            data: None,
        });
    }
    let id = params["id"].as_str().ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing id".to_string(),
        data: None,
    })?;
    if state
        .db
        .get_server(id)
        .map_err(storage_error_to_rpc)?
        .is_none()
    {
        return Err(JsonRpcError {
            code: ERR_NOT_FOUND,
            message: format!("Server not found: {id}"),
            data: None,
        });
    }
    let name = optional_server_name(&params)?;
    let icon = match params.get("icon") {
        Some(Value::String(value)) => {
            validate_server_icon(value)?;
            Some(Some(value.as_str()))
        }
        Some(Value::Null) => Some(None),
        None => None,
        Some(_) => {
            return Err(JsonRpcError {
                code: ERR_INVALID_PARAMS,
                message: "Server icon must be a string or null".to_string(),
                data: None,
            });
        }
    };
    if name.is_none() && icon.is_none() {
        return Err(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "No server identity fields provided".to_string(),
            data: None,
        });
    }

    state
        .db
        .update_server_identity(id, name.as_deref(), icon)
        .map_err(storage_error_to_rpc)?;
    state.server_manager.write().await.load_from_db(&state.db);
    Ok(serde_json::json!({ "ok": true }))
}

/// Updates config.json to reflect `id` as the active server so the static
/// `get_credentials()` (Jellyfin paths) resolves the right session.
fn sync_selected_config(state: &AppState, id: &str) -> Result<(), JsonRpcError> {
    let Some(record) = state.db.get_server(id).map_err(storage_error_to_rpc)? else {
        return Ok(());
    };
    if record.server_type == "jellyfin" {
        if let Ok(creds) = CredentialManager::get_server_credential(id) {
            CredentialManager::save_jellyfin_session(
                id,
                &record.url,
                &creds.token_or_password,
                creds.user_id.as_deref(),
            )
            .map_err(storage_error_to_rpc)?;
        } else {
            CredentialManager::set_config_selected_server(id).map_err(storage_error_to_rpc)?;
        }
    } else {
        CredentialManager::set_config_selected_server(id).map_err(storage_error_to_rpc)?;
    }
    Ok(())
}

/// AC2: `server.select({ id })` — persists selection, refreshes the manager, and
/// lazily connects the newly selected server's provider.
async fn handle_server_select(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let params = params.ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing params".to_string(),
        data: None,
    })?;
    let id = params["id"].as_str().ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing id".to_string(),
        data: None,
    })?;

    state.db.set_selected(id).map_err(|e| JsonRpcError {
        code: ERR_NOT_FOUND,
        message: e.to_string(),
        data: None,
    })?;
    sync_selected_config(state, id)?;

    state.server_manager.write().await.load_from_db(&state.db);
    *state.last_connection_check.lock().await = None;

    // Lazily connect (and cache) the selected provider so the library can reload.
    get_provider_for_server(state, id).await?;

    Ok(serde_json::json!({ "ok": true }))
}

/// AC6/AC8: `server.remove({ id })` — deletes the row, evicts the vault entry and
/// the cached provider, and reselects the first remaining server (or none) if the
/// removed server was selected.
async fn handle_server_remove(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let params = params.ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing params".to_string(),
        data: None,
    })?;
    let id = params["id"]
        .as_str()
        .ok_or(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Missing id".to_string(),
            data: None,
        })?
        .to_string();

    let was_selected = current_server_config(state)?
        .map(|c| c.id == id)
        .unwrap_or(false);

    let removed = state.db.remove_server(&id).map_err(storage_error_to_rpc)?;
    if !removed {
        return Err(JsonRpcError {
            code: ERR_NOT_FOUND,
            message: format!("Server not found: {id}"),
            data: None,
        });
    }
    // Evict credential + cached provider before returning (enforcement rule).
    let _ = CredentialManager::remove_server_credential(&id);
    state.server_manager.write().await.providers.remove(&id);

    // Reselect when the removed server was the active one (AC8).
    let mut reselected: Option<String> = None;
    if was_selected {
        let remaining = state.db.list_servers().map_err(storage_error_to_rpc)?;
        if let Some(next) = remaining.first() {
            state
                .db
                .set_selected(&next.id)
                .map_err(storage_error_to_rpc)?;
            sync_selected_config(state, &next.id)?;
            reselected = Some(next.id.clone());
        }
    }

    state.server_manager.write().await.load_from_db(&state.db);
    *state.last_connection_check.lock().await = None;

    Ok(serde_json::json!({
        "ok": true,
        "removedServerId": id,
        "reselectedServerId": reselected,
    }))
}

fn parse_server_type_hint(value: &str) -> Result<ServerTypeHint, JsonRpcError> {
    match value {
        "auto" => Ok(ServerTypeHint::Auto),
        "jellyfin" => Ok(ServerTypeHint::Jellyfin),
        "subsonic" => Ok(ServerTypeHint::Subsonic),
        "audiobookshelf" => Ok(ServerTypeHint::Audiobookshelf),
        _ => Err(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Invalid serverType".to_string(),
            data: None,
        }),
    }
}

async fn handle_login(state: &AppState, params: Option<Value>) -> Result<Value, JsonRpcError> {
    let params = params.ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Invalid params".to_string(),
        data: None,
    })?;

    let mut params = params.as_object().cloned().ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Invalid params".to_string(),
        data: None,
    })?;
    params
        .entry("serverType".to_string())
        .or_insert_with(|| Value::String("auto".to_string()));

    handle_server_connect(state, Some(Value::Object(params))).await
}

async fn handle_save_credentials(params: Option<Value>) -> Result<Value, JsonRpcError> {
    let params = params.ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Invalid params".to_string(),
        data: None,
    })?;

    let url = params["url"].as_str().ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: hifimule_i18n::t("error.missing_url"),
        data: None,
    })?;

    let token = params["token"].as_str().ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing token".to_string(),
        data: None,
    })?;

    let user_id = params["userId"].as_str();

    match CredentialManager::save_credentials(url, token, user_id) {
        Ok(_) => Ok(serde_json::Value::Bool(true)),
        Err(e) => Err(JsonRpcError {
            code: ERR_STORAGE_ERROR,
            message: e.to_string(),
            data: None,
        }),
    }
}

fn selected_credentials_response(db: &crate::db::Database) -> Result<Option<Value>, JsonRpcError> {
    if let Some(server) = db.get_server_config().map_err(storage_error_to_rpc)? {
        let credential =
            CredentialManager::find_server_credential(&server.id).map_err(storage_error_to_rpc)?;
        let user_id = match server.server_type.as_str() {
            "jellyfin" => credential.as_ref().and_then(|creds| creds.user_id.clone()),
            "subsonic" | "openSubsonic" => Some(server.username.clone()),
            "audiobookshelf" => Some(server.username.clone()),
            other => {
                return Err(storage_error_to_rpc(anyhow::anyhow!(
                    "Unsupported selected server type: {}",
                    other
                )));
            }
        };
        let token = if server.server_type == "audiobookshelf" {
            None
        } else {
            credential.map(|creds| creds.token_or_password)
        };
        return Ok(Some(serde_json::json!({
            "url": server.url,
            "token": token,
            "userId": user_id,
            "serverType": server.server_type,
            "serverVersion": server.server_version,
        })));
    }
    Ok(None)
}

fn legacy_credentials_response(credentials: Option<(String, String, Option<String>)>) -> Value {
    match credentials {
        Some((url, token, user_id)) => serde_json::json!({
            "url": url,
            "token": token,
            "userId": user_id
        }),
        None => Value::Null,
    }
}

async fn handle_get_credentials(state: &AppState) -> Result<Value, JsonRpcError> {
    if let Some(response) = selected_credentials_response(&state.db)? {
        return Ok(response);
    }
    let credentials = CredentialManager::find_legacy_credentials().map_err(storage_error_to_rpc)?;
    Ok(legacy_credentials_response(credentials))
}

async fn handle_set_device_profile(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let params = params.ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Invalid params".to_string(),
        data: None,
    })?;

    let device_id = params["deviceId"].as_str().ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing deviceId".to_string(),
        data: None,
    })?;

    let profile_id = params["profileId"].as_str().ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing profileId".to_string(),
        data: None,
    })?;

    let rules = params["syncRules"].as_str(); // Optional

    match state
        .db
        .upsert_device_mapping(device_id, None, Some(profile_id), rules)
    {
        Ok(_) => Ok(Value::Bool(true)),
        Err(e) => Err(JsonRpcError {
            code: ERR_STORAGE_ERROR,
            message: e.to_string(),
            data: None,
        }),
    }
}

async fn handle_get_daemon_state(state: &AppState) -> Result<Value, JsonRpcError> {
    let device = state.device_manager.get_current_device().await;
    let mapping = if let Some(ref d) = device {
        state.db.get_device_mapping(&d.device_id).unwrap_or(None)
    } else {
        None
    };

    // Check server connection with caching (cache for 5 seconds)
    let server_connected = check_server_connection_cached(state).await;

    // Capture dirty before device is moved into json!()
    let dirty = device.as_ref().map(|d| d.dirty).unwrap_or(false);

    // Include pending device path and friendly name for unrecognized devices awaiting initialization
    let pending_device_snapshots = state.device_manager.get_pending_devices_snapshot().await;
    let pending_device_path = pending_device_snapshots
        .last()
        .map(|s| s.path.to_string_lossy().to_string());
    let pending_device_friendly_name = pending_device_snapshots
        .last()
        .and_then(|s| s.friendly_name.clone());
    let pending_devices: Vec<_> = pending_device_snapshots.iter().map(|pending| serde_json::json!({
        "pendingId": pending.pending_id,
        "name": pending.friendly_name.clone().unwrap_or_else(|| "Unconfigured device".to_string()),
    })).collect();

    let auto_sync_on_connect = device
        .as_ref()
        .map(|d| d.auto_sync_on_connect)
        .or_else(|| mapping.as_ref().map(|m| m.auto_sync_on_connect))
        .unwrap_or(false);

    // Story 12.2: auto_fill is now a per-server pipeline map. Resolve the selected server's
    // portable id and read its slot via the server-aware accessors; the `{ enabled, maxBytes }`
    // fields are retained unchanged for the legacy read path. Story 12.6 additionally exposes the
    // full per-server `pipelines` map so the pipeline-builder UI can hydrate every server's config
    // (empty `{}` for a legacy device with no per-server map).
    let selected_portable_id = state
        .db
        .get_server_config()
        .ok()
        .flatten()
        .and_then(|s| s.server_id);
    let auto_fill = device.as_ref().map(|d| {
        serde_json::json!({
            "enabled": d.auto_fill.enabled_for(selected_portable_id.as_deref()),
            "maxBytes": d.auto_fill.max_bytes_for(selected_portable_id.as_deref()),
            "pipelines": &d.auto_fill.pipelines,
        })
    });

    let active_operation_id = state.sync_operation_manager.get_active_operation_id().await;

    // Multi-server snapshot (AC15): full server list + selected id, plus the
    // legacy `currentServer`/`serverType`/`serverVersion` fields kept for existing
    // consumers (mapped to the selected server). Read from the in-memory manager
    // (source of truth, kept in sync with the DB on every mutation).
    let (servers_snapshot, selected_server_id) = {
        let mgr = state.server_manager.read().await;
        (mgr.servers.clone(), mgr.selected_server_id.clone())
    };
    let servers_json: Vec<Value> = servers_snapshot
        .iter()
        .map(|s| {
            serde_json::json!({
                "id": s.id,
                "serverId": s.server_id,
                "url": s.url,
                "serverType": s.server_type,
                "username": s.username,
                "name": s.name,
                "icon": s.icon,
                "selected": s.selected,
                "libraryRole": s.provider_library_role,
            })
        })
        .collect();
    let selected_server = selected_server_id
        .as_deref()
        .and_then(|id| servers_snapshot.iter().find(|s| s.id == id));
    // Portable id of the selected server (Story 2.13) — the UI's active-server key.
    let selected_server_portable_id = selected_server.and_then(|s| s.server_id.clone());
    let server_type = selected_server.map(|c| c.server_type.clone());
    let server_version = selected_server.and_then(|c| c.server_version.clone());
    let current_server = selected_server.map(|config| {
        // Story 2.13: `serverId` here carries the PORTABLE id to match the rest
        // of the contract (server.list, server.connect, daemon_state.servers[]).
        // `localId` exposes the machine-local UUID for callers that need it.
        serde_json::json!({
            "serverId": config.server_id,
            "localId": config.id,
            "url": config.url,
            "username": config.username,
            "serverType": config.server_type,
            "serverVersion": config.server_version,
            "libraryRole": config.provider_library_role,
        })
    });

    let (connected_devices_snapshot, selected_path_buf) =
        state.device_manager.get_multi_device_snapshot().await;
    let destination_snapshot = state.device_manager.get_destination_snapshot().await;
    let device_discovery_issues = state.device_manager.get_discovery_issues().await;
    let selected_device_path = selected_path_buf.map(|p| p.to_string_lossy().to_string());
    let connected_devices_json: Vec<_> = connected_devices_snapshot
        .iter()
        .map(|(p, m, class)| {
            serde_json::json!({
                "path": p.to_string_lossy(),
                "deviceId": m.device_id,
                "name": m.name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| m.device_id.clone()),
                "icon": m.icon.clone(),
                "managedPaths": m.managed_paths.clone(),
                "audiobookPath": m.audiobook_path.clone(),
                "podcastPath": m.podcast_path.clone(),
                "playlistPath": m.playlist_path.clone(),
                "transcodingProfileId": m.transcoding_profile_id.clone(),
                "deviceClass": match class {
                    crate::device::DeviceClass::Msc => "msc",
                    crate::device::DeviceClass::Mtp => "mtp",
                },
            })
        })
        .collect();

    // Capabilities of the selected provider (lazily connecting it on first read).
    let supports_playlist_write =
        match crate::server_manager::selected_provider(&state.server_manager, &state.db).await {
            Some(Ok(provider)) => provider.capabilities().supports_playlist_write,
            _ => false,
        };

    Ok(serde_json::json!({
        "currentDevice": device,
        "deviceMapping": mapping,
        "serverConnected": server_connected,
        "serverType": server_type,
        "serverVersion": server_version,
        "currentServer": current_server,
        "servers": servers_json,
        "selectedServerId": selected_server_id,
        "selectedServerPortableId": selected_server_portable_id,
        "dirtyManifest": dirty,
        "pendingDevicePath": pending_device_path,
        "pendingDeviceFriendlyName": pending_device_friendly_name,
        "pendingDevices": pending_devices,
        "autoSyncOnConnect": auto_sync_on_connect,
        "autoFill": auto_fill,
        "activeOperationId": active_operation_id,
        "syncPipelineActive": state.sync_operation_manager.is_pipeline_active(),
        "connectedDevices": connected_devices_json,
        "selectedDevicePath": selected_device_path,
        "destinationRevision": destination_snapshot.revision.to_string(),
        "destinations": destination_snapshot.destinations,
        "deviceDiscoveryIssues": device_discovery_issues,
        "supportsPlaylistWrite": supports_playlist_write,
    }))
}

/// Reconciles basket items to the PORTABLE server id (Story 2.13, supersedes AC22):
/// items carrying a pre-2.11 composite serverId (`type|url|username`) **or** a 2.11
/// machine-local UUID are remapped to the matching server's portable id; items with
/// no serverId are assigned to the selected server's portable id; items referencing
/// an unknown/removed server are dropped (so a removed server's items do not linger).
/// Items already tagged with a known portable id are retained as-is. Idempotent:
/// re-running over already-portable items is a no-op (never maps portable → other).
fn reconcile_basket_server_ids(
    items: Vec<crate::device::BasketItem>,
    servers: &[crate::db::ServerConfig],
) -> Vec<crate::device::BasketItem> {
    // Set of valid portable ids (the only tags we keep untouched).
    let portable_known: HashSet<&str> = servers
        .iter()
        .filter_map(|s| s.server_id.as_deref())
        .collect();
    // { legacy-composite → portable, machine-local UUID → portable }.
    let mut remap: HashMap<String, String> = HashMap::new();
    for s in servers {
        if let Some(portable) = s.server_id.clone() {
            remap.insert(s.id.clone(), portable.clone());
            remap.insert(
                crate::db::legacy_composite_server_id(&s.server_type, &s.url, &s.username),
                portable,
            );
        }
    }
    let selected_portable: Option<String> = servers
        .iter()
        .find(|s| s.selected)
        .and_then(|s| s.server_id.clone());

    items
        .into_iter()
        .filter_map(|mut item| match item.server_id.clone() {
            // Untagged item: adopt the selected server's portable id if any; otherwise
            // keep it untagged (it will be reconciled once a server is selected).
            None => {
                item.server_id = selected_portable.clone();
                Some(item)
            }
            // Already a known portable id — keep as-is (idempotent).
            Some(s) if portable_known.contains(s.as_str()) => Some(item),
            // Legacy local-UUID or composite that maps to a known server → portable.
            Some(s) => match remap.get(&s) {
                Some(portable) => {
                    item.server_id = Some(portable.clone());
                    Some(item)
                }
                // Belongs to an unknown/removed server — drop it.
                None => None,
            },
        })
        .collect()
}

async fn handle_manifest_get_basket(state: &AppState) -> Result<Value, JsonRpcError> {
    let device = state.device_manager.get_current_device().await;
    let servers = state.db.list_servers().map_err(storage_error_to_rpc)?;
    let selected_portable = servers
        .iter()
        .find(|s| s.selected)
        .and_then(|s| s.server_id.clone());
    let basket_items = device
        .as_ref()
        .map(|d| d.basket_items.clone())
        .unwrap_or_default();
    // Return items from ALL servers (mixed basket, AC3); only reconcile ids.
    let basket_items = reconcile_basket_server_ids(basket_items, &servers);
    Ok(serde_json::json!({
        "basketItems": basket_items,
        "serverId": selected_portable,
    }))
}

async fn handle_manifest_save_basket(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let mut params = params.ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing params".to_string(),
        data: None,
    })?;

    let basket_items_value = params
        .get_mut("basketItems")
        .ok_or(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Missing basketItems".to_string(),
            data: None,
        })?
        .take();

    let items: Vec<crate::device::BasketItem> = serde_json::from_value(basket_items_value)
        .map_err(|e| JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: format!("Invalid basketItems format: {}", e),
            data: None,
        })?;
    // Persist items from ALL known servers (mixed basket, AC3/AC25); reconcile
    // legacy/composite/local serverIds to portable and drop items from unknown/
    // removed servers (Story 2.13).
    let servers = state.db.list_servers().map_err(storage_error_to_rpc)?;
    let items = reconcile_basket_server_ids(items, &servers);

    match state.device_manager.save_basket(items).await {
        Ok(_) => Ok(Value::Bool(true)),
        Err(e) => Err(JsonRpcError {
            code: ERR_STORAGE_ERROR,
            message: e.to_string(),
            data: None,
        }),
    }
}

async fn check_server_connection_cached(state: &AppState) -> bool {
    // A selected server means its credentials are stored and a provider can be
    // lazily connected — treat that as connected without a network round-trip.
    state
        .server_manager
        .read()
        .await
        .selected_server_id
        .is_some()
}

async fn active_non_jellyfin_provider(state: &AppState) -> Option<Arc<dyn MediaProvider>> {
    let provider = require_provider(state).await.ok()?;
    if provider.server_type() == ServerType::Jellyfin {
        None
    } else {
        Some(provider)
    }
}

fn legacy_view_from_library(library: &Library) -> Value {
    let collection_type = if library.id == SUBSONIC_PLAYLISTS_LIBRARY_ID {
        SUBSONIC_PLAYLISTS_LIBRARY_ID
    } else {
        "music"
    };
    serde_json::json!({
        "Id": library.id,
        "Name": library.name,
        "Type": "CollectionFolder",
        "CollectionType": collection_type,
    })
}

fn ticks_from_seconds(seconds: Option<u32>) -> Option<u64> {
    seconds.map(|value| u64::from(value) * JELLYFIN_TICKS_PER_SECOND)
}

fn legacy_artist_item(artist: &Artist) -> Value {
    serde_json::json!({
        "Id": artist.id,
        "Name": artist.name,
        "Type": "MusicArtist",
        "ImageId": artist.cover_art_id,
        "RecursiveItemCount": artist.album_count.or(artist.song_count),
    })
}

fn legacy_album_item(album: &Album) -> Value {
    serde_json::json!({
        "Id": album.id,
        "Name": album.title,
        "Type": "MusicAlbum",
        "AlbumArtist": album.artist_name,
        "ProductionYear": album.year,
        "ImageId": album.cover_art_id,
        "RecursiveItemCount": album.song_count,
        "CumulativeRunTimeTicks": ticks_from_seconds(album.duration_seconds),
    })
}

fn legacy_playlist_item(playlist: &Playlist) -> Value {
    serde_json::json!({
        "Id": playlist.id,
        "Name": playlist.name,
        "Type": "Playlist",
        "ImageId": playlist.cover_art_id,
        "RecursiveItemCount": playlist.song_count,
        "CumulativeRunTimeTicks": ticks_from_seconds(playlist.duration_seconds),
    })
}

fn legacy_song_item(song: &Song) -> Value {
    serde_json::json!({
        "Id": song.id,
        "Name": song.title,
        "Type": "Audio",
        "Album": song.album_title,
        "AlbumArtist": song.artist_name,
        "IndexNumber": song.track_number,
        "ParentIndexNumber": song.disc_number,
        "ParentId": song.album_id,
        "AlbumId": song.album_id,
        "ImageId": song.cover_art_id,
        "RunTimeTicks": ticks_from_seconds(Some(song.duration_seconds)),
        "Bitrate": song.bitrate_kbps,
    })
}

fn legacy_item_count_from_value(item: &Value) -> Value {
    let id = item.get("Id").and_then(Value::as_str).unwrap_or_default();
    serde_json::json!({
        "id": id,
        "recursiveItemCount": item
            .get("RecursiveItemCount")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        "cumulativeRunTimeTicks": item
            .get("CumulativeRunTimeTicks")
            .and_then(Value::as_u64)
            .or_else(|| item.get("RunTimeTicks").and_then(Value::as_u64))
            .unwrap_or(0),
    })
}

async fn provider_legacy_item_value(
    provider: Arc<dyn MediaProvider>,
    item_id: &str,
) -> Result<Value, JsonRpcError> {
    if let Ok(artist) = provider.get_artist(item_id).await {
        return Ok(legacy_artist_item(&artist.artist));
    }
    if let Ok(album) = provider.get_album(item_id).await {
        return Ok(legacy_album_item(&album.album));
    }
    if let Ok(playlist) = provider.get_playlist(item_id).await {
        return Ok(legacy_playlist_item(&playlist.playlist));
    }
    Err(JsonRpcError {
        code: ERR_CONNECTION_FAILED,
        message: "Provider item not found".to_string(),
        data: None,
    })
}

async fn provider_legacy_item_size(
    provider: Arc<dyn MediaProvider>,
    item_id: &str,
) -> Result<Value, JsonRpcError> {
    if let Ok(album) = provider.get_album(item_id).await {
        let total = album.tracks.iter().map(provider_track_size).sum::<u64>();
        return Ok(serde_json::json!({
            "id": item_id,
            "totalSizeBytes": total,
        }));
    }
    if let Ok(playlist) = provider.get_playlist(item_id).await {
        let total = playlist.tracks.iter().map(provider_track_size).sum::<u64>();
        return Ok(serde_json::json!({
            "id": item_id,
            "totalSizeBytes": total,
        }));
    }
    if let Ok(artist) = provider.get_artist(item_id).await {
        let mut total = 0_u64;
        for album in artist.albums {
            if let Ok(album) = provider.get_album(&album.id).await {
                total += album.tracks.iter().map(provider_track_size).sum::<u64>();
            }
        }
        return Ok(serde_json::json!({
            "id": item_id,
            "totalSizeBytes": total,
        }));
    }
    if let Ok((tracks, _)) = provider.get_genre_tracks(item_id, 0, 10_000).await {
        let total = tracks.iter().map(provider_track_size).sum::<u64>();
        return Ok(serde_json::json!({
            "id": item_id,
            "totalSizeBytes": total,
        }));
    }
    Ok(serde_json::json!({
        "id": item_id,
        "totalSizeBytes": 0,
    }))
}

async fn provider_legacy_item_count(
    provider: Arc<dyn MediaProvider>,
    item_id: &str,
) -> Result<Value, JsonRpcError> {
    if let Ok(album) = provider.get_album(item_id).await {
        let duration = album
            .tracks
            .iter()
            .map(|track| u64::from(track.duration_seconds))
            .sum::<u64>();
        return Ok(serde_json::json!({
            "id": item_id,
            "recursiveItemCount": album.tracks.len() as u64,
            "cumulativeRunTimeTicks": duration * JELLYFIN_TICKS_PER_SECOND,
        }));
    }
    if let Ok(playlist) = provider.get_playlist(item_id).await {
        let duration = playlist
            .tracks
            .iter()
            .map(|track| u64::from(track.duration_seconds))
            .sum::<u64>();
        return Ok(serde_json::json!({
            "id": item_id,
            "recursiveItemCount": playlist.tracks.len() as u64,
            "cumulativeRunTimeTicks": duration * JELLYFIN_TICKS_PER_SECOND,
        }));
    }
    if let Ok((tracks, _)) = provider.get_genre_tracks(item_id, 0, 10_000).await {
        let duration = tracks
            .iter()
            .map(|track| u64::from(track.duration_seconds))
            .sum::<u64>();
        return Ok(serde_json::json!({
            "id": item_id,
            "recursiveItemCount": tracks.len() as u64,
            "cumulativeRunTimeTicks": duration * JELLYFIN_TICKS_PER_SECOND,
        }));
    }
    let item = provider_legacy_item_value(provider, item_id).await?;
    Ok(legacy_item_count_from_value(&item))
}

fn provider_track_size(track: &Song) -> u64 {
    track.size_bytes.unwrap_or_else(|| {
        track
            .bitrate_kbps
            .map(|kbps| (u64::from(kbps) * 1_000 / 8) * u64::from(track.duration_seconds))
            .unwrap_or_else(|| {
                if track.provider_metadata.identity.is_some() {
                    16_000
                        * u64::from(if track.duration_seconds == 0 {
                            3_600
                        } else {
                            track.duration_seconds
                        })
                } else {
                    0
                }
            })
    })
}

fn provider_song_to_desired_item(song: &Song) -> crate::sync::DesiredItem {
    crate::sync::DesiredItem {
        media_role: crate::device::MediaRole::Music,
        jellyfin_id: song.id.clone(),
        name: song.title.clone(),
        album: song.album_title.clone(),
        artist: song.artist_name.clone(),
        size_bytes: provider_track_size(song),
        etag: None,
        provider_album_id: song.album_id.clone(),
        provider_content_type: song.content_type.clone(),
        provider_suffix: song.suffix.clone(),
        original_bitrate: song.bitrate_kbps.map(|kbps| kbps * 1000),
        track_number: song.track_number,
        server_id: None,
    }
}

fn provider_media_role(provider: &dyn MediaProvider) -> crate::device::MediaRole {
    crate::device::MediaRole::from_library_role(provider.library_role())
}

async fn blocked_provider_media_add(
    add: &crate::sync::SyncAddItem,
    provider: &dyn MediaProvider,
    profile: Option<&Value>,
    preferred_container: Option<&str>,
) -> Option<crate::sync::SyncBlockedItem> {
    if add.media_role == crate::device::MediaRole::Music {
        return None;
    }
    let (reason_code, reason) = match provider.resolve_sync_media(&add.jellyfin_id).await {
        Ok(media)
            if crate::sync::sync_direct_media_compatible(profile, preferred_container, &media) =>
        {
            return None;
        }
        Ok(_) => (
            "incompatible-direct-format",
            "Direct audio format is unsupported by this device; no verified file transcode is available",
        ),
        Err(ProviderError::Auth(_)) => {
            ("provider-auth", "Audiobookshelf authentication is required")
        }
        Err(ProviderError::NotFound { .. }) => {
            ("remote-item-missing", "Remote media is unavailable")
        }
        Err(_) => (
            "direct-media-unavailable",
            "A verified direct audio representation is unavailable",
        ),
    };
    Some(crate::sync::SyncBlockedItem {
        provider_item_id: add.jellyfin_id.clone(),
        name: add.name.clone(),
        server_id: add.server_id.clone(),
        media_role: add.media_role,
        reason_code: reason_code.into(),
        reason: reason.into(),
    })
}

fn apply_blocked_media_adds(
    delta: &mut crate::sync::SyncDelta,
    blocked: Vec<crate::sync::SyncBlockedItem>,
) {
    let blocked_ids: HashSet<&str> = blocked
        .iter()
        .map(|item| item.provider_item_id.as_str())
        .collect();
    delta
        .adds
        .retain(|add| !blocked_ids.contains(add.jellyfin_id.as_str()));
    // A failed replacement must not delete its previously managed copy.
    delta
        .deletes
        .retain(|delete| !blocked_ids.contains(delete.jellyfin_id.as_str()));
    delta.blocked.extend(blocked);
}

async fn preflight_provider_media_adds(
    delta: &mut crate::sync::SyncDelta,
    provider: Arc<dyn MediaProvider>,
    profile: Option<&Value>,
    preferred_container: Option<&str>,
) {
    let mut blocked = Vec::new();
    for add in &mut delta.adds {
        if add.media_role == crate::device::MediaRole::Music {
            continue;
        }
        if let Some(item) =
            blocked_provider_media_add(add, provider.as_ref(), profile, preferred_container).await
        {
            blocked.push(item);
        } else {
            add.reason_code = Some("verified-direct-format".into());
            add.reason =
                Some("Verified direct audio representation is compatible with this device".into());
        }
    }
    apply_blocked_media_adds(delta, blocked);
}

async fn preview_media_profile(
    state: &AppState,
    manifest: &crate::device::DeviceManifest,
) -> Result<(Option<Value>, Option<&'static str>), JsonRpcError> {
    let profile = load_selected_transcoding_profile(manifest.transcoding_profile_id.as_deref())
        .map_err(|message| JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message,
            data: None,
        })?;
    let preferred = if profile.is_some() {
        None
    } else {
        state
            .device_manager
            .get_sync_target_for_device(&manifest.device_id)
            .await
            .and_then(|(_, _, io)| io.preferred_audio_container())
    };
    Ok((profile, preferred))
}

async fn preflight_state_media_adds(
    state: &AppState,
    manifest: &crate::device::DeviceManifest,
    delta: &mut crate::sync::SyncDelta,
    default_provider: Option<Arc<dyn MediaProvider>>,
) -> Result<(), JsonRpcError> {
    if !delta
        .adds
        .iter()
        .any(|add| add.media_role != crate::device::MediaRole::Music)
    {
        return Ok(());
    }
    let (profile, preferred) = preview_media_profile(state, manifest).await?;
    if let Some(provider) = default_provider {
        preflight_provider_media_adds(delta, provider, profile.as_ref(), preferred).await;
        return Ok(());
    }
    let mut providers: HashMap<String, Arc<dyn MediaProvider>> = HashMap::new();
    let mut blocked = Vec::new();
    for add in &mut delta.adds {
        if add.media_role == crate::device::MediaRole::Music {
            continue;
        }
        let Some(server_id) = add.server_id.as_deref() else {
            blocked.push(crate::sync::SyncBlockedItem {
                provider_item_id: add.jellyfin_id.clone(),
                name: add.name.clone(),
                server_id: None,
                media_role: add.media_role,
                reason_code: "server-unavailable".into(),
                reason: "Source server identity is unavailable".into(),
            });
            continue;
        };
        let provider = if let Some(provider) = providers.get(server_id) {
            provider.clone()
        } else {
            match get_provider_by_server_id_for(state, server_id).await {
                Ok(provider) => {
                    providers.insert(server_id.to_string(), provider.clone());
                    provider
                }
                Err(_) => {
                    blocked.push(crate::sync::SyncBlockedItem {
                        provider_item_id: add.jellyfin_id.clone(),
                        name: add.name.clone(),
                        server_id: add.server_id.clone(),
                        media_role: add.media_role,
                        reason_code: "server-unavailable".into(),
                        reason: "Source server is unavailable".into(),
                    });
                    continue;
                }
            }
        };
        if let Some(item) =
            blocked_provider_media_add(add, provider.as_ref(), profile.as_ref(), preferred).await
        {
            blocked.push(item);
        } else {
            add.reason_code = Some("verified-direct-format".into());
            add.reason =
                Some("Verified direct audio representation is compatible with this device".into());
        }
    }
    apply_blocked_media_adds(delta, blocked);
    Ok(())
}

fn podcast_episode_to_desired(
    episode: &crate::domain::models::PodcastEpisode,
    show_name: &str,
) -> crate::sync::DesiredItem {
    // Episode catalog details do not expose a byte size. Reserve 128 kbit/s,
    // using one hour when duration is missing; transfer checks actual capacity.
    crate::sync::DesiredItem {
        media_role: crate::device::MediaRole::Podcast,
        jellyfin_id: episode.id.clone(),
        name: episode.title.clone(),
        album: Some(show_name.to_string()),
        artist: None,
        size_bytes: podcast_estimated_size_bytes(episode.duration_seconds),
        etag: None,
        provider_album_id: Some(episode.show_id.clone()),
        provider_content_type: None,
        provider_suffix: None,
        original_bitrate: None,
        track_number: podcast_date_number(&episode.published_at),
        server_id: None,
    }
}

fn podcast_date_number(value: &Option<String>) -> Option<u32> {
    use chrono::Datelike;
    value
        .as_deref()
        .and_then(|date| {
            chrono::DateTime::parse_from_rfc3339(date)
                .ok()
                .map(|date| date.date_naive())
                .or_else(|| chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").ok())
        })
        .and_then(|date| {
            (1..=9999)
                .contains(&date.year())
                .then(|| (date.year() as u32) * 10_000 + date.month() * 100 + date.day())
        })
}

#[cfg(test)]
#[test]
fn podcast_publication_date_uses_source_calendar_day() {
    assert_eq!(
        podcast_date_number(&Some("2026-09-24T23:30:00-04:00".into())),
        Some(20260924)
    );
    assert_eq!(podcast_date_number(&None), None);
    assert_eq!(
        podcast_date_number(&Some("2026-01-02".into())),
        Some(20260102)
    );
    assert_eq!(podcast_date_number(&Some("invalid".into())), None);
}

fn podcast_estimated_size_bytes(duration_seconds: Option<u32>) -> u64 {
    u64::from(
        duration_seconds
            .filter(|seconds| *seconds > 0)
            .unwrap_or(3_600),
    ) * 16_000
}

#[cfg(test)]
#[test]
fn podcast_unknown_duration_reserves_capacity() {
    assert_eq!(podcast_estimated_size_bytes(None), 3_600 * 16_000);
    assert_eq!(podcast_estimated_size_bytes(Some(0)), 3_600 * 16_000);
    assert_eq!(podcast_estimated_size_bytes(Some(60)), 60 * 16_000);
}

fn load_selected_transcoding_profile(profile_id: Option<&str>) -> Result<Option<Value>, String> {
    let Some(profile_id) = profile_id else {
        return Ok(None);
    };
    if profile_id == "passthrough" {
        return Ok(None);
    }

    let profiles_path = crate::paths::get_device_profiles_path().map_err(|e| e.to_string())?;
    let profiles = crate::transcoding::load_profiles(&profiles_path).map_err(|e| e.to_string())?;
    let entry = profiles
        .into_iter()
        .find(|profile| profile.id == profile_id)
        .ok_or_else(|| {
            format!(
                "Transcoding profile '{}' not found in device-profiles.json",
                profile_id
            )
        })?;

    Ok(entry.device_profile)
}

async fn fail_sync_operation(
    op_manager: &Arc<crate::sync::SyncOperationManager>,
    op_id: &str,
    filename: &str,
    message: String,
) {
    if let Some(mut operation) = op_manager.get_operation(op_id).await {
        operation.status = crate::sync::SyncStatus::Failed;
        operation.errors.push(crate::sync::SyncFileError {
            jellyfin_id: String::new(),
            filename: filename.to_string(),
            error_message: message,
        });
        op_manager.update_operation(op_id, operation).await;
    }
}

fn scoped_favorite_target_id<'a>(
    basket_item: &'a crate::device::BasketItem,
    prefix: &str,
) -> &'a str {
    basket_item
        .id
        .strip_prefix(prefix)
        .unwrap_or(&basket_item.id)
}

async fn provider_favorite_sync_items_for_basket_item(
    provider: Arc<dyn MediaProvider>,
    basket_item: &crate::device::BasketItem,
) -> Result<Vec<crate::sync::DesiredItem>, JsonRpcError> {
    let favorites = provider
        .list_favorite_items(None)
        .await
        .map_err(provider_error_to_rpc)?;

    match basket_item.item_type.as_str() {
        "FavoriteAlbum" => {
            let album_id = scoped_favorite_target_id(basket_item, "favorites:album:");
            Ok(favorites
                .songs
                .iter()
                .filter(|song| song.album_id.as_deref() == Some(album_id))
                .map(provider_song_to_desired_item)
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
                let album = provider
                    .get_album(&album.id)
                    .await
                    .map_err(provider_error_to_rpc)?;
                desired_items.extend(album.tracks.iter().map(provider_song_to_desired_item));
            }
            desired_items.extend(
                favorites
                    .songs
                    .iter()
                    .filter(|song| song.artist_id.as_deref() == Some(artist_id))
                    .map(provider_song_to_desired_item),
            );
            Ok(desired_items)
        }
        _ => Ok(Vec::new()),
    }
}

async fn provider_genre_sync_items_for_id(
    provider: Arc<dyn MediaProvider>,
    item_id: &str,
) -> Result<Option<Vec<crate::sync::DesiredItem>>, JsonRpcError> {
    let mut desired_items = Vec::new();
    let mut start_index = 0;

    for page_index in 0..GENRE_TRACK_MAX_PAGES {
        let tracks = match provider
            .get_genre_tracks(item_id, start_index, GENRE_TRACK_PAGE_SIZE)
            .await
        {
            Ok((tracks, _)) => tracks,
            Err(ProviderError::UnsupportedCapability(_)) if desired_items.is_empty() => {
                return Ok(None);
            }
            Err(ProviderError::NotFound { .. }) if desired_items.is_empty() => {
                return Ok(None);
            }
            Err(error) => return Err(provider_error_to_rpc(error)),
        };

        let fetched = tracks.len() as u32;
        if fetched == 0 {
            break;
        }

        desired_items.extend(tracks.iter().map(provider_song_to_desired_item));

        if fetched < GENRE_TRACK_PAGE_SIZE {
            break;
        }

        if page_index + 1 >= GENRE_TRACK_MAX_PAGES {
            return Err(JsonRpcError {
                code: ERR_CONNECTION_FAILED,
                message: format!(
                    "Sync aborted: Genre {item_id} exceeded pagination guard after {} tracks",
                    desired_items.len()
                ),
                data: None,
            });
        }

        start_index = start_index.saturating_add(fetched);
    }

    Ok(Some(desired_items))
}

async fn provider_sync_items_for_id(
    provider: Arc<dyn MediaProvider>,
    item_id: &str,
) -> Result<
    (
        Vec<crate::sync::DesiredItem>,
        Option<crate::sync::PlaylistSyncItem>,
    ),
    JsonRpcError,
> {
    let role = provider_media_role(provider.as_ref());
    if role == crate::device::MediaRole::Podcast {
        return provider_podcast_sync_items_for_id(provider, item_id).await;
    }
    let (mut items, playlist) = provider_sync_items_for_id_inner(provider, item_id).await?;
    for item in &mut items {
        item.media_role = role;
    }
    Ok((items, playlist))
}

async fn provider_podcast_sync_items_for_id(
    provider: Arc<dyn MediaProvider>,
    item_id: &str,
) -> Result<
    (
        Vec<crate::sync::DesiredItem>,
        Option<crate::sync::PlaylistSyncItem>,
    ),
    JsonRpcError,
> {
    match provider.get_podcast_show(item_id).await {
        Ok(show) => {
            if show.possibly_truncated {
                return Err(JsonRpcError {
                    code: ERR_CONNECTION_FAILED,
                    message: "Podcast show is incomplete; select episodes individually".into(),
                    data: None,
                });
            }
            return Ok((
                show.episodes
                    .iter()
                    .map(|episode| podcast_episode_to_desired(episode, &show.show.title))
                    .collect(),
                None,
            ));
        }
        Err(ProviderError::NotFound { .. }) | Err(ProviderError::UnsupportedCapability(_)) => {}
        Err(error) => return Err(provider_error_to_rpc(error)),
    }
    let episode = provider
        .get_podcast_episode(item_id)
        .await
        .map_err(provider_error_to_rpc)?;
    let show = provider
        .get_podcast_show(&episode.show_id)
        .await
        .map_err(provider_error_to_rpc)?;
    Ok((
        vec![podcast_episode_to_desired(&episode, &show.show.title)],
        None,
    ))
}

async fn provider_sync_items_for_id_inner(
    provider: Arc<dyn MediaProvider>,
    item_id: &str,
) -> Result<
    (
        Vec<crate::sync::DesiredItem>,
        Option<crate::sync::PlaylistSyncItem>,
    ),
    JsonRpcError,
> {
    if let Ok(album) = provider.get_album(item_id).await {
        return Ok((
            album
                .tracks
                .iter()
                .map(provider_song_to_desired_item)
                .collect(),
            None,
        ));
    }

    if let Ok(playlist) = provider.get_playlist(item_id).await {
        let tracks = playlist
            .tracks
            .iter()
            .map(provider_song_to_desired_item)
            .collect::<Vec<_>>();
        let playlist_item = crate::sync::PlaylistSyncItem {
            jellyfin_id: playlist.playlist.id.clone(),
            name: playlist.playlist.name.clone(),
            tracks: playlist
                .tracks
                .iter()
                .map(|track| crate::sync::PlaylistTrackInfo {
                    jellyfin_id: track.id.clone(),
                    artist: track.artist_name.clone(),
                    run_time_seconds: i64::from(track.duration_seconds),
                })
                .collect(),
        };
        return Ok((tracks, Some(playlist_item)));
    }

    if let Ok(artist) = provider.get_artist(item_id).await {
        let mut tracks = Vec::new();
        for album in artist.albums {
            let album = provider
                .get_album(&album.id)
                .await
                .map_err(provider_error_to_rpc)?;
            tracks.extend(album.tracks.iter().map(provider_song_to_desired_item));
        }
        return Ok((tracks, None));
    }

    match provider.get_song(item_id).await {
        Ok(song) => return Ok((vec![provider_song_to_desired_item(&song)], None)),
        Err(ProviderError::UnsupportedCapability(_)) | Err(ProviderError::NotFound { .. }) => {}
        Err(error) => return Err(provider_error_to_rpc(error)),
    }

    if let Some(tracks) = provider_genre_sync_items_for_id(provider.clone(), item_id).await? {
        return Ok((tracks, None));
    }

    Err(JsonRpcError {
        code: ERR_CONNECTION_FAILED,
        message: format!("Sync aborted: Failed to fetch item {item_id}: Not found"),
        data: None,
    })
}

async fn free_bytes_for_sync_device(state: &AppState, device_id: &str) -> Option<u64> {
    let (_, _, io) = state
        .device_manager
        .get_sync_target_for_device(device_id)
        .await?;
    io.free_space().await.ok()
}

async fn provider_calculate_delta(
    _state: &AppState,
    provider: Arc<dyn MediaProvider>,
    item_ids: &[String],
    manifest: &crate::device::DeviceManifest,
    params: &Value,
) -> Result<Value, JsonRpcError> {
    let media_role = provider_media_role(provider.as_ref());
    // Story 12.3: normalize both the legacy single object and the new array form
    // (AC1). This single-server fast path is reached only when routing resolved
    // every auto-fill slot to the selected server, so the relevant descriptor (if
    // any) is for this server. For the legacy object this is byte-for-byte
    // identical to the old `autoFill.enabled` / `autoFill.maxBytes` reads.
    let auto_fill_descriptor = parse_auto_fill_descriptors(params).into_iter().next();
    let auto_fill_enabled = auto_fill_descriptor.is_some();

    let mut desired_items = Vec::new();
    let mut playlist_sync_items = Vec::new();
    let mut seen_ids = HashSet::new();

    // Step 1: Resolve basket items (always, regardless of auto-fill).
    let basket_items = basket_items_from_params_or_manifest(params, manifest);
    let favorite_basket_by_id: HashMap<String, crate::device::BasketItem> = basket_items
        .into_iter()
        .filter(|item| matches!(item.item_type.as_str(), "FavoriteArtist" | "FavoriteAlbum"))
        .map(|item| (item.id.clone(), item))
        .collect();

    for item_id in item_ids {
        if let Some(basket_item) = favorite_basket_by_id.get(item_id) {
            let tracks =
                provider_favorite_sync_items_for_basket_item(provider.clone(), basket_item).await?;
            for item in tracks {
                if seen_ids.insert(item.jellyfin_id.clone()) {
                    desired_items.push(item);
                }
            }
            continue;
        }

        let (tracks, playlist) = provider_sync_items_for_id(provider.clone(), item_id).await?;
        if let Some(playlist) = playlist {
            playlist_sync_items.push(playlist);
        }
        for item in tracks {
            if seen_ids.insert(item.jellyfin_id.clone()) {
                desired_items.push(item);
            }
        }
    }

    // Step 2: If auto-fill is enabled, fill remaining space after basket items.
    // When basket is empty this is a pure auto-fill (device fully managed by auto-fill).
    // When basket has items this augments them — playlists/albums stay, free space is filled.
    // Story 13.1: rotation-tier index per emitted track, patched onto delta.adds below.
    let mut af_tier_map: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    let mut af_item_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    // Story 13.5 #20: encoding-from-goals derived per-slot max-bitrate (kbps) per emitted auto-fill
    // track, patched onto delta.adds below (mirrors `af_tier_map`). Only populated when a transcode
    // profile is active for the slot — passthrough tracks never get a forced re-encode.
    let mut af_bitrate_map: std::collections::HashMap<String, u32> =
        std::collections::HashMap::new();
    // Story 13.4 review: portable server ids whose pity discovery reserve genuinely fired this run
    // (shared `pity_reserve_bytes` gate, computed with the per-run budget). Set on the delta below so
    // the sync-completion recorder resets the dry-streak only for servers that actually fired.
    let mut af_pity_fired: Vec<String> = Vec::new();
    let mut autofill_playlist_tracks = Vec::new();
    if auto_fill_enabled {
        // If the UI provided maxBytes, use it directly — it already represents the intended
        // auto-fill budget (UI subtracted manual-item sizes from free space). If not provided,
        // compute server-side as (free + existing synced - basket).
        let auto_fill_budget: u64 = if let Some(mb) =
            auto_fill_descriptor.as_ref().and_then(|d| d.max_bytes)
        {
            crate::daemon_log!(
                "[AutoFill] budget from UI maxBytes: {} bytes ({:.1} GB)",
                mb,
                mb as f64 / 1_073_741_824.0
            );
            mb
        } else {
            let synced_bytes: u64 = manifest.synced_items.iter().map(|s| s.size_bytes).sum();
            let basket_size: u64 = desired_items.iter().map(|i| i.size_bytes).sum();
            match free_bytes_for_sync_device(_state, &manifest.device_id).await {
                Some(free_bytes) => {
                    crate::daemon_log!(
                        "[AutoFill] no maxBytes from UI — server fallback: free={} synced={} basket_est={} -> budget={}",
                        free_bytes,
                        synced_bytes,
                        basket_size,
                        free_bytes
                            .saturating_add(synced_bytes)
                            .saturating_sub(basket_size)
                    );
                    free_bytes
                        .saturating_add(synced_bytes)
                        .saturating_sub(basket_size)
                }
                None => {
                    return Err(JsonRpcError {
                        code: ERR_CONNECTION_FAILED,
                        message: "Cannot determine device capacity for auto-fill".to_string(),
                        data: None,
                    });
                }
            }
        };
        crate::daemon_log!(
            "[AutoFill] basket_items={} desired_items={} auto_fill_budget={} bytes",
            item_ids.len(),
            desired_items.len(),
            auto_fill_budget
        );
        if auto_fill_budget > 0 {
            let exclude_ids: Vec<String> = desired_items
                .iter()
                .map(|i| i.jellyfin_id.clone())
                .collect();
            // Story 12.4: route the selected server's slot through the shared seam, reading its
            // configured pipeline (portable serverId). Default-equivalent → fast path (AC 8).
            let selected_portable = current_server_portable_id(_state).ok().flatten();
            // Story 13.1: supply the DB-sourced history snapshot + rotation cursor so the Memory
            // stage (cooldown/played/stable-core/tiers) is live for this slot.
            let now = now_unix_secs();
            let (history, rotation_cursor, pity_streak) = match selected_portable.as_deref() {
                Some(sid) => build_autofill_history(&_state.db, &manifest.device_id, sid, now),
                None => (
                    crate::auto_fill::HistorySnapshot {
                        now,
                        ..Default::default()
                    },
                    0,
                    0,
                ),
            };
            let fill_params = crate::auto_fill::AutoFillParams {
                exclude_item_ids: exclude_ids,
                max_fill_bytes: auto_fill_budget,
                device_id: manifest.device_id.clone(),
                server_id: selected_portable.clone().unwrap_or_default(),
                now_unix: now,
                history,
                rotation_cursor,
                // Story 13.4: mint the seed from `now` (varies per run, deterministic within it).
                seed: now as u64,
                pity_streak,
                // Story 13.5: mint local civil time at this engine fill site (drives the Context stage).
                local: now_civil(),
            };
            let pipeline_ref = selected_portable
                .as_deref()
                .and_then(|id| manifest.auto_fill.pipeline_for(id));
            // Story 13.5 #20: derive the encoding-from-goals per-slot transcode bitrate (only when a
            // transcode profile is active), and in passthrough clear the flag so the byte estimate
            // stays source-based. The override travels onto each emitted track via `af_bitrate_map`.
            let transcode_active =
                transcode_profile_active(manifest.transcoding_profile_id.as_deref());
            let af_bitrate_override = encoding_override_kbps(pipeline_ref, transcode_active);
            let encoding_cleared = encoding_passthrough_clear(pipeline_ref, transcode_active);
            let pipeline_opt = encoding_cleared.as_ref().or(pipeline_ref);
            // Story 13.4 review: record whether the pity reserve genuinely fired for this slot
            // (same gate the engine uses, with this run's budget) so the recorder resets the
            // dry-streak only on a real fire.
            if let (Some(sid), Some(pipeline)) = (selected_portable.as_deref(), pipeline_opt)
                && crate::auto_fill::pipeline::pity_reserve_bytes(
                    &pipeline.pity,
                    pity_streak,
                    auto_fill_budget,
                ) > 0
            {
                af_pity_fired.push(sid.to_string());
            }
            let fill_items = expand_auto_fill_slot(provider.clone(), pipeline_opt, fill_params)
                .await
                .map_err(|e| JsonRpcError {
                    code: ERR_CONNECTION_FAILED,
                    message: format!("Auto-fill failed: {}", e),
                    data: None,
                })?;
            crate::daemon_log!(
                "[AutoFill] slot expansion returned {} tracks",
                fill_items.len()
            );
            for item in fill_items {
                if let Some(tier) = item.tier.clone() {
                    af_tier_map.insert(item.id.clone(), tier);
                }
                // Story 13.5 #20: stamp the slot's derived bitrate onto this auto-fill track so the
                // sync transcode applies it to this item only (manual items carry nothing).
                if let Some(kbps) = af_bitrate_override {
                    af_bitrate_map.insert(item.id.clone(), kbps);
                }
                let item_id = item.id.clone();
                if seen_ids.insert(item_id) {
                    af_item_ids.insert(item.id.clone());
                    autofill_playlist_tracks.push(autofill_playlist_track(&item));
                    desired_items.push(crate::sync::DesiredItem {
                        media_role: crate::device::MediaRole::Music,
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
        }
    }
    if !autofill_playlist_tracks.is_empty() {
        playlist_sync_items.push(autofill_playlist_item(autofill_playlist_tracks));
    }

    // Story 2.13: tag untagged items with the selected server's portable id.
    tag_untagged_with_selected_portable(_state, &mut desired_items)?;
    for item in &mut desired_items {
        item.media_role = media_role;
    }

    crate::daemon_log!(
        "[Delta] Provider desired set prepared: desired_items={} playlists={}; calculating manifest delta",
        desired_items.len(),
        playlist_sync_items.len()
    );
    let mut delta = crate::sync::calculate_delta(&desired_items, manifest);
    patch_delta_tiers(&mut delta, &af_tier_map);
    patch_delta_bitrate_overrides(&mut delta, &af_bitrate_map);
    delta.pity_fired_servers = af_pity_fired;
    delta.playlists = playlist_sync_items;
    crate::daemon_log!(
        "[Delta] Provider delta calculated: adds={} deletes={} id_changes={} unchanged={} playlists={} reasons={}",
        delta.adds.len(),
        delta.deletes.len(),
        delta.id_changes.len(),
        delta.unchanged,
        delta.playlists.len(),
        crate::sync::format_change_reason_summary(&delta)
    );
    if !delta.id_changes.is_empty() {
        crate::daemon_log!(
            "[Delta] Provider id-change sample: {}",
            crate::sync::format_id_change_diagnostics(&delta, 5)
        );
    }

    if let Some((_, _, device_io)) = _state
        .device_manager
        .get_sync_target_for_device(&manifest.device_id)
        .await
    {
        crate::daemon_log!(
            "[Delta] Provider existence check starting for {} desired item(s)",
            desired_items.len()
        );
        crate::sync::augment_delta_with_existence_check(
            &mut delta,
            &desired_items,
            manifest,
            device_io.as_ref(),
        )
        .await;
        crate::daemon_log!(
            "[Delta] Provider existence check complete: adds={} deletes={} id_changes={} unchanged={} reasons={}",
            delta.adds.len(),
            delta.deletes.len(),
            delta.id_changes.len(),
            delta.unchanged,
            crate::sync::format_change_reason_summary(&delta)
        );
        if !delta.id_changes.is_empty() {
            crate::daemon_log!(
                "[Delta] Provider id-change sample after existence check: {}",
                crate::sync::format_id_change_diagnostics(&delta, 5)
            );
        }
    }
    patch_delta_auto_fill(&mut delta, &af_item_ids);
    preflight_state_media_adds(_state, manifest, &mut delta, Some(provider)).await?;

    Ok(delta_value_with_cleanup_metadata(&delta, manifest))
}

fn delta_value_with_cleanup_metadata(
    delta: &crate::sync::SyncDelta,
    manifest: &crate::device::DeviceManifest,
) -> Value {
    let mut value = serde_json::to_value(delta).unwrap();
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "destructiveCleanupCount".to_string(),
            serde_json::json!(crate::sync::destructive_cleanup_count(delta, manifest)),
        );
        object.insert(
            "destructiveCleanupThreshold".to_string(),
            serde_json::json!(crate::sync::DESTRUCTIVE_CLEANUP_THRESHOLD),
        );
        object.insert(
            "changeReasons".to_string(),
            serde_json::to_value(crate::sync::change_reason_summary(delta)).unwrap(),
        );
    }
    value
}

fn basket_items_from_params_or_manifest(
    params: &Value,
    manifest: &crate::device::DeviceManifest,
) -> Vec<crate::device::BasketItem> {
    params
        .get("basketItems")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_else(|| manifest.basket_items.clone())
}

fn jellyfin_item_to_desired_item(item: crate::api::JellyfinItem) -> crate::sync::DesiredItem {
    let size_bytes = item
        .media_sources
        .as_ref()
        .and_then(|sources| sources.first())
        .and_then(|s| s.size)
        .unwrap_or(0) as u64;
    let provider_suffix = item
        .media_sources
        .as_ref()
        .and_then(|sources| sources.first())
        .and_then(|source| source.container.clone())
        .or_else(|| item.container.clone());
    let original_bitrate = item
        .media_sources
        .as_ref()
        .and_then(|sources| sources.first())
        .and_then(|s| {
            // Prefer the container-level bitrate; fall back to the audio stream's
            // BitRate, which Jellyfin populates even when the container field is
            // absent (common for M4A/AAC files).
            s.bitrate.or_else(|| {
                s.media_streams
                    .as_ref()?
                    .iter()
                    .find(|ms| ms.stream_type == "Audio")
                    .and_then(|ms| ms.bit_rate)
            })
        })
        .or(item.bitrate);
    crate::sync::DesiredItem {
        media_role: crate::device::MediaRole::Music,
        jellyfin_id: item.id,
        name: item.name,
        album: item.album,
        artist: item.album_artist,
        size_bytes,
        etag: item.etag,
        provider_album_id: item.album_id,
        provider_content_type: None,
        provider_suffix,
        original_bitrate,
        track_number: item.index_number,
        server_id: None,
    }
}

async fn jellyfin_favorite_sync_items_for_basket_item(
    client: &JellyfinClient,
    url: &str,
    token: &str,
    user_id: &str,
    basket_item: &crate::device::BasketItem,
) -> Result<Vec<crate::sync::DesiredItem>, JsonRpcError> {
    let favorites = client
        .get_favorite_music_items(url, token, user_id, None)
        .await
        .map_err(|error| JsonRpcError {
            code: ERR_CONNECTION_FAILED,
            message: error.to_string(),
            data: None,
        })?;

    match basket_item.item_type.as_str() {
        "FavoriteAlbum" => {
            let album_id = scoped_favorite_target_id(basket_item, "favorites:album:");
            Ok(favorites
                .items
                .into_iter()
                .filter(|item| {
                    matches!(item.item_type.as_str(), "Audio" | "MusicVideo")
                        && item.album_id.as_deref() == Some(album_id)
                })
                .map(jellyfin_item_to_desired_item)
                .collect())
        }
        "FavoriteArtist" => {
            let artist_id = scoped_favorite_target_id(basket_item, "favorites:artist:");
            let mut desired_items = Vec::new();
            let mut favorite_album_ids = Vec::new();
            for item in favorites.items {
                match item.item_type.as_str() {
                    "MusicAlbum" => {
                        let item_artist_id = item
                            .artist_items
                            .as_ref()
                            .and_then(|items| items.first())
                            .map(|artist| artist.id.as_str());
                        if item_artist_id == Some(artist_id) {
                            favorite_album_ids.push(item.id);
                        }
                    }
                    "Audio" | "MusicVideo" => {
                        let item_artist_id = item
                            .artist_items
                            .as_ref()
                            .and_then(|items| items.first())
                            .map(|artist| artist.id.as_str());
                        if item_artist_id == Some(artist_id) {
                            desired_items.push(jellyfin_item_to_desired_item(item));
                        }
                    }
                    _ => {}
                }
            }

            for album_id in favorite_album_ids {
                let children = client
                    .get_child_items_with_sizes(url, token, user_id, &album_id)
                    .await
                    .map_err(|error| JsonRpcError {
                        code: ERR_CONNECTION_FAILED,
                        message: error.to_string(),
                        data: None,
                    })?;
                desired_items.extend(
                    children
                        .into_iter()
                        .filter(|child| matches!(child.item_type.as_str(), "Audio" | "MusicVideo"))
                        .map(jellyfin_item_to_desired_item),
                );
            }
            Ok(desired_items)
        }
        _ => Ok(Vec::new()),
    }
}

fn paginate_values(mut items: Vec<Value>, start_index: u32, limit: u32) -> Value {
    let total = items.len() as u32;
    let start = start_index.min(total) as usize;
    let end = (start + limit as usize).min(items.len());
    let page = items.drain(start..end).collect::<Vec<_>>();
    serde_json::json!({
        "Items": page,
        "TotalRecordCount": total,
        "StartIndex": start_index,
    })
}

fn apply_name_filter(
    items: Vec<Value>,
    name_starts_with: Option<&str>,
    name_less_than: Option<&str>,
) -> Vec<Value> {
    items
        .into_iter()
        .filter(|item| {
            let Some(name) = item.get("Name").and_then(|name| name.as_str()) else {
                return true;
            };
            if let Some(prefix) = name_starts_with {
                return name
                    .chars()
                    .next()
                    .map(|ch| ch.eq_ignore_ascii_case(&prefix.chars().next().unwrap()))
                    .unwrap_or(false);
            }
            if name_less_than == Some("A") {
                return name
                    .chars()
                    .next()
                    .map(|ch| !ch.is_ascii_alphabetic())
                    .unwrap_or(true);
            }
            true
        })
        .collect()
}

async fn provider_items_response(
    provider: Arc<dyn MediaProvider>,
    parent_id: Option<&str>,
    start_index: u32,
    limit: u32,
    name_starts_with: Option<&str>,
    name_less_than: Option<&str>,
) -> Result<Value, JsonRpcError> {
    let parent_id = parent_id.filter(|id| !id.is_empty());
    let mut items = if parent_id.is_none() || parent_id == Some("all") {
        let (artists, _) = provider
            .list_artists(parent_id, name_starts_with, start_index, limit)
            .await
            .map_err(provider_error_to_rpc)?;
        artists.iter().map(legacy_artist_item).collect::<Vec<_>>()
    } else if parent_id == Some(SUBSONIC_PLAYLISTS_LIBRARY_ID) {
        provider
            .list_playlists()
            .await
            .map_err(provider_error_to_rpc)?
            .iter()
            .map(legacy_playlist_item)
            .collect::<Vec<_>>()
    } else if let Some(id) = parent_id {
        if let Ok(artist) = provider.get_artist(id).await {
            artist
                .albums
                .iter()
                .map(legacy_album_item)
                .collect::<Vec<_>>()
        } else if let Ok(album) = provider.get_album(id).await {
            album
                .tracks
                .iter()
                .map(legacy_song_item)
                .collect::<Vec<_>>()
        } else if let Ok(playlist) = provider.get_playlist(id).await {
            playlist
                .tracks
                .iter()
                .map(legacy_song_item)
                .collect::<Vec<_>>()
        } else {
            return Err(JsonRpcError {
                code: ERR_CONNECTION_FAILED,
                message: "Provider item not found".to_string(),
                data: None,
            });
        }
    } else {
        vec![]
    };
    items = apply_name_filter(items, name_starts_with, name_less_than);
    Ok(paginate_values(items, start_index, limit))
}

fn is_auth_rpc_error(error: &JsonRpcError) -> bool {
    error.message.contains("authentication failed") || error.message.contains("Wrong username")
}

async fn provider_items_response_with_auth_retry(
    state: &AppState,
    provider: Arc<dyn MediaProvider>,
    parent_id: Option<&str>,
    start_index: u32,
    limit: u32,
    name_starts_with: Option<&str>,
    name_less_than: Option<&str>,
) -> Result<Value, JsonRpcError> {
    match provider_items_response(
        provider,
        parent_id,
        start_index,
        limit,
        name_starts_with,
        name_less_than,
    )
    .await
    {
        Ok(response) => Ok(response),
        Err(error) if is_auth_rpc_error(&error) => {
            if let Some(provider) = reconnect_subsonic_provider_from_config(state).await {
                provider_items_response(
                    provider,
                    parent_id,
                    start_index,
                    limit,
                    name_starts_with,
                    name_less_than,
                )
                .await
            } else {
                Err(error)
            }
        }
        Err(error) => Err(error),
    }
}

async fn provider_item_details(
    provider: Arc<dyn MediaProvider>,
    item_id: &str,
) -> Result<Value, JsonRpcError> {
    provider_legacy_item_value(provider, item_id).await
}

async fn handle_jellyfin_get_views(
    state: &AppState,
    _params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    if let Some(provider) = active_non_jellyfin_provider(state).await {
        let libraries = provider
            .list_libraries()
            .await
            .map_err(provider_error_to_rpc)?;
        return Ok(Value::Array(
            libraries.iter().map(legacy_view_from_library).collect(),
        ));
    }

    let (url, token, user_id) = CredentialManager::get_credentials().map_err(|e| JsonRpcError {
        code: ERR_STORAGE_ERROR,
        message: format!("Failed to get credentials: {}", e),
        data: None,
    })?;

    let user_id = user_id.unwrap_or_else(|| "Me".to_string());

    match state
        .jellyfin_client
        .get_views(&url, &token, &user_id)
        .await
    {
        Ok(views) => Ok(serde_json::to_value(views).unwrap()),
        Err(e) => Err(JsonRpcError {
            code: ERR_CONNECTION_FAILED,
            message: e.to_string(),
            data: None,
        }),
    }
}

async fn handle_jellyfin_get_items(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let params = params.unwrap_or(serde_json::json!({}));
    let parent_id = params["parentId"].as_str();
    let include_item_types = params["includeItemTypes"].as_str();
    let start_index = params["startIndex"].as_u64().map(|v| v as u32);
    let limit = params["limit"].as_u64().map(|v| v as u32);
    let name_starts_with = params["nameStartsWith"]
        .as_str()
        .filter(|s| s.len() == 1 && s.chars().all(|c| c.is_ascii_alphabetic()));
    let name_less_than = params["nameLessThan"]
        .as_str()
        .filter(|s| s.len() == 1 && s.chars().all(|c| c.is_ascii_alphabetic()));

    if let Some(provider) = active_non_jellyfin_provider(state).await {
        let response = provider_items_response_with_auth_retry(
            state,
            provider,
            parent_id,
            start_index.unwrap_or(0),
            limit.unwrap_or(50),
            name_starts_with,
            name_less_than,
        )
        .await?;
        return Ok(response);
    }

    let (url, token, user_id) = CredentialManager::get_credentials().map_err(|e| JsonRpcError {
        code: ERR_STORAGE_ERROR,
        message: format!("Failed to get credentials: {}", e),
        data: None,
    })?;

    let user_id = user_id.unwrap_or_else(|| "Me".to_string());

    match state
        .jellyfin_client
        .get_items(
            &url,
            &token,
            &user_id,
            parent_id,
            include_item_types,
            start_index,
            limit,
            name_starts_with,
            name_less_than,
            None,
            None,
            None,
        )
        .await
    {
        Ok(response) => Ok(serde_json::to_value(response).unwrap()),
        Err(e) => Err(JsonRpcError {
            code: ERR_CONNECTION_FAILED,
            message: e.to_string(),
            data: None,
        }),
    }
}

async fn handle_jellyfin_get_item_details(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let params = params.ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Invalid params".to_string(),
        data: None,
    })?;

    let item_id = params["itemId"].as_str().ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing itemId".to_string(),
        data: None,
    })?;

    if let Some(provider) = active_non_jellyfin_provider(state).await {
        return provider_item_details(provider, item_id).await;
    }

    let (url, token, user_id) = CredentialManager::get_credentials().map_err(|e| JsonRpcError {
        code: ERR_STORAGE_ERROR,
        message: format!("Failed to get credentials: {}", e),
        data: None,
    })?;

    let user_id = user_id.unwrap_or_else(|| "Me".to_string());

    match state
        .jellyfin_client
        .get_item_details(&url, &token, &user_id, item_id)
        .await
    {
        Ok(item) => Ok(serde_json::to_value(item).unwrap()),
        Err(e) => Err(JsonRpcError {
            code: ERR_CONNECTION_FAILED,
            message: e.to_string(),
            data: None,
        }),
    }
}

async fn handle_jellyfin_get_item_counts(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let params = params.ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Invalid params".to_string(),
        data: None,
    })?;

    let ids = params["itemIds"].as_array().ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing or invalid itemIds list".to_string(),
        data: None,
    })?;

    if let Some(provider) = active_non_jellyfin_provider(state).await {
        let mut results = Vec::new();
        for id in ids.iter().filter_map(Value::as_str) {
            if let Ok(count) = provider_legacy_item_count(provider.clone(), id).await {
                results.push(count);
            }
        }
        return Ok(Value::Array(results));
    }

    let (url, token, user_id) = CredentialManager::get_credentials().map_err(|e| JsonRpcError {
        code: ERR_STORAGE_ERROR,
        message: format!("Failed to get credentials: {}", e),
        data: None,
    })?;

    let user_id = user_id.unwrap_or_else(|| "Me".to_string());
    let futures = ids.iter().filter_map(|id_val| {
        id_val.as_str().map(|id| {
            let client = &state.jellyfin_client;
            let url = &url;
            let token = &token;
            let user_id = &user_id;
            async move {
                match client.get_item_details(url, token, user_id, id).await {
                    Ok(item) => {
                        if item.item_type == "MusicGenre" {
                            match client.get_songs_by_genre(url, token, user_id, id, 0, 10_000).await {
                                Ok(response) => Some(serde_json::json!({
                                    "id": id,
                                    "recursiveItemCount": response.items.len() as u64,
                                    "cumulativeRunTimeTicks": response.items.iter()
                                        .filter_map(|t| t.run_time_ticks)
                                        .sum::<u64>(),
                                })),
                                Err(e) => {
                                    println!("Warning: Failed to fetch genre tracks for {}: {}", id, e);
                                    None
                                }
                            }
                        } else {
                            Some(serde_json::json!({
                                "id": item.id,
                                "recursiveItemCount": item.recursive_item_count.unwrap_or(0),
                                "cumulativeRunTimeTicks": item.cumulative_run_time_ticks.unwrap_or(0),
                            }))
                        }
                    },
                    Err(e) => {
                        println!("Warning: Failed to fetch metadata for item {}: {}", id, e);
                        None
                    }
                }
            }
        })
    });

    let results: Vec<Value> = futures::future::join_all(futures)
        .await
        .into_iter()
        .flatten()
        .collect();

    Ok(serde_json::to_value(results).unwrap())
}

async fn handle_jellyfin_get_item_sizes(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let params = params.ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Invalid params".to_string(),
        data: None,
    })?;

    let ids = params["itemIds"].as_array().ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing or invalid itemIds list".to_string(),
        data: None,
    })?;

    if let Some(provider) = active_non_jellyfin_provider(state).await {
        let mut results = Vec::new();
        for id in ids.iter().filter_map(Value::as_str) {
            results.push(provider_legacy_item_size(provider.clone(), id).await?);
        }
        return Ok(Value::Array(results));
    }

    let (url, token, user_id) = CredentialManager::get_credentials().map_err(|e| JsonRpcError {
        code: ERR_STORAGE_ERROR,
        message: format!("Failed to get credentials: {}", e),
        data: None,
    })?;

    let user_id = user_id.unwrap_or_else(|| "Me".to_string());

    // Check cache for already-known sizes, collect uncached IDs
    let cache = state.size_cache.read().await;
    let mut results: Vec<Value> = Vec::new();
    let mut uncached_ids: Vec<String> = Vec::new();

    for id_val in ids {
        if let Some(id) = id_val.as_str() {
            if let Some(&cached_size) = cache.get(id) {
                results.push(serde_json::json!({
                    "id": id,
                    "totalSizeBytes": cached_size,
                }));
            } else {
                uncached_ids.push(id.to_string());
            }
        }
    }
    drop(cache);

    // Fetch uncached sizes
    if !uncached_ids.is_empty() {
        let fetched = state
            .jellyfin_client
            .get_item_sizes(&url, &token, &user_id, uncached_ids)
            .await;

        // Update cache and results
        let mut cache = state.size_cache.write().await;
        for (id, size) in fetched {
            cache.insert(id.clone(), size);
            results.push(serde_json::json!({
                "id": id,
                "totalSizeBytes": size,
            }));
        }
    }

    Ok(serde_json::to_value(results).unwrap())
}

async fn handle_device_get_storage_info(state: &AppState) -> Result<Value, JsonRpcError> {
    match state.device_manager.get_device_storage().await {
        Some(info) => Ok(serde_json::to_value(info).unwrap()),
        None => Ok(Value::Null),
    }
}

async fn handle_device_list_root_folders(state: &AppState) -> Result<Value, JsonRpcError> {
    match state.device_manager.list_root_folders().await {
        Ok(Some(response)) => Ok(serde_json::to_value(response).unwrap()),
        Ok(None) => Ok(Value::Null),
        Err(e) => Err(JsonRpcError {
            code: ERR_STORAGE_ERROR,
            message: e.to_string(),
            data: None,
        }),
    }
}

async fn handle_sync_get_device_status_map(state: &AppState) -> Result<Value, JsonRpcError> {
    let device = state.device_manager.get_current_device().await;

    match device {
        Some(manifest) => {
            let synced_ids: Vec<&str> = manifest
                .synced_items
                .iter()
                .map(|item| item.jellyfin_id.as_str())
                .collect();
            Ok(serde_json::json!({
                "syncedItemIds": synced_ids
            }))
        }
        None => Ok(serde_json::json!({
            "syncedItemIds": []
        })),
    }
}

/// Parses the `itemIds` param which may be legacy `string[]` or the multi-server
/// `Array<{ id, serverId }>` shape (AC27). Returns (id, optional serverId) pairs.
fn parse_item_specs(raw: &[Value]) -> Vec<(String, Option<String>)> {
    raw.iter()
        .filter_map(|v| {
            if let Some(s) = v.as_str() {
                Some((s.to_string(), None))
            } else if let Some(obj) = v.as_object() {
                obj.get("id").and_then(Value::as_str).map(|id| {
                    (
                        id.to_string(),
                        obj.get("serverId")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                    )
                })
            } else {
                None
            }
        })
        .collect()
}

/// One per-server auto-fill slot (Story 12.3). `server_id` is the PORTABLE id
/// (Story 2.13); `None` means "fall back to the selected server" and is resolved
/// at the call site where the selected id is known (mirrors `parse_item_specs`,
/// which also leaves the selected fallback to the caller).
struct AutoFillDescriptor {
    server_id: Option<String>,
    max_bytes: Option<u64>,
    exclude_item_ids: Vec<String>,
}

/// Normalizes the `autoFill` sync param into a `Vec<AutoFillDescriptor>` (AC1).
///
/// Accepts BOTH the legacy single object `{ enabled, maxBytes?, serverId?,
/// excludeItemIds? }` and the new array `[{ serverId, maxBytes?, enabled?,
/// excludeItemIds? }, …]`:
/// - object form yields a single-element vec only when `enabled == true`;
///   disabled / absent → empty (no auto-fill).
/// - array form maps each object element to a descriptor, keeping only those
///   whose `enabled != false` — in array form a descriptor's *presence* means
///   "this server has a slot", so a missing `enabled` is treated as enabled.
/// - any other shape (`null`/absent/scalar) → empty.
///
/// The selected-server fallback for a `None` `server_id` is intentionally NOT
/// applied here (resolved by callers that hold `selected_id`).
fn parse_auto_fill_descriptors(params: &Value) -> Vec<AutoFillDescriptor> {
    let af = match params.get("autoFill") {
        Some(v) => v,
        None => return Vec::new(),
    };

    let read_descriptor = |el: &Value| AutoFillDescriptor {
        // A blank/whitespace-only serverId is treated as missing so the
        // selected-server fallback applies at the call site — `Some("")` would
        // otherwise bypass the fallback and fail provider resolution.
        server_id: el
            .get("serverId")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        // `maxBytes` is an optional budget ceiling. Absent → `None` (fill up to
        // the shared remaining budget). Present-but-not-a-valid-u64 (negative,
        // float, overflow, or non-numeric) must NOT silently fall through to
        // "no cap" (which would fill the device); floor non-negative numbers and
        // clamp anything else to 0 so a malformed cap skips the slot.
        max_bytes: el.get("maxBytes").map(|v| {
            if let Some(n) = v.as_u64() {
                n
            } else if let Some(f) = v.as_f64() {
                f.max(0.0).min(u64::MAX as f64) as u64
            } else {
                0
            }
        }),
        exclude_item_ids: el
            .get("excludeItemIds")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
    };

    if let Some(arr) = af.as_array() {
        arr.iter()
            .filter(|el| el.is_object())
            .filter(|el| el.get("enabled").and_then(Value::as_bool).unwrap_or(true))
            .map(read_descriptor)
            .collect()
    } else if af.is_object() {
        if af.get("enabled").and_then(Value::as_bool).unwrap_or(false) {
            vec![read_descriptor(af)]
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    }
}

/// Merges one slot's auto-fill results into the running desired set (Story 12.3).
/// Manual items and earlier slots have already populated `seen_ids`, so any id
/// seen before is skipped — manual items win dedup and slots dedup across each
/// other (AC3). `remaining`, when present, is decremented by each newly added
/// item's size so the next slot sees the shrunken shared budget (AC4). Each added
/// item is tagged with the slot's portable `server_id`. Returns the bytes this
/// slot actually added.
fn push_fill_items_dedup(
    fill_items: Vec<crate::auto_fill::AutoFillItem>,
    desired_items: &mut Vec<crate::sync::DesiredItem>,
    seen_ids: &mut HashSet<String>,
    server_id: &str,
    media_role: crate::device::MediaRole,
    remaining: &mut Option<u64>,
    autofill_playlist_tracks: &mut Vec<crate::sync::PlaylistTrackInfo>,
) -> u64 {
    let mut added: u64 = 0;
    for item in fill_items {
        if seen_ids.insert(item.id.clone()) {
            let size = item.size_bytes;
            autofill_playlist_tracks.push(autofill_playlist_track(&item));
            desired_items.push(crate::sync::DesiredItem {
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
                server_id: Some(server_id.to_string()),
            });
            if let Some(r) = remaining.as_mut() {
                *r = r.saturating_sub(size);
            }
            added = added.saturating_add(size);
        }
    }
    added
}

fn autofill_playlist_track(
    item: &crate::auto_fill::AutoFillItem,
) -> crate::sync::PlaylistTrackInfo {
    crate::sync::PlaylistTrackInfo {
        jellyfin_id: item.id.clone(),
        artist: item.artist.clone(),
        run_time_seconds: -1,
    }
}

fn autofill_playlist_item(
    tracks: Vec<crate::sync::PlaylistTrackInfo>,
) -> crate::sync::PlaylistSyncItem {
    crate::sync::PlaylistSyncItem {
        jellyfin_id: "__hifimule_autofill".to_string(),
        name: "Autofill".to_string(),
        tracks,
    }
}

/// The single shared slot-expansion seam (Story 12.4): every sync-time auto-fill
/// expansion site routes through this so the configurable-vs-default decision lives in
/// exactly one place. When `pipeline` is a configured NON-default pipeline, materialize
/// its pools and run the pure engine (`expand_with_pipeline`); otherwise keep the smart
/// incremental default path (`run_auto_fill_provider`) — byte-for-byte unchanged (AC 8).
pub(crate) async fn expand_auto_fill_slot(
    provider: Arc<dyn MediaProvider>,
    pipeline: Option<&crate::auto_fill::AutoFillPipeline>,
    params: crate::auto_fill::AutoFillParams,
) -> anyhow::Result<Vec<crate::auto_fill::AutoFillItem>> {
    if provider.library_role() == Some(crate::providers::ProviderLibraryRole::Podcast) {
        return expand_podcast_auto_fill(provider, pipeline, params).await;
    }
    if provider.library_role() == Some(crate::providers::ProviderLibraryRole::Audiobook) {
        return expand_audiobook_auto_fill(provider, params).await;
    }
    match pipeline {
        Some(p) if crate::auto_fill::needs_configurable_expansion(p) => {
            crate::auto_fill::expand_with_pipeline(provider, p, params).await
        }
        _ => crate::auto_fill::run_auto_fill_provider(provider, params).await,
    }
}

async fn expand_audiobook_auto_fill(
    provider: Arc<dyn MediaProvider>,
    params: crate::auto_fill::AutoFillParams,
) -> anyhow::Result<Vec<crate::auto_fill::AutoFillItem>> {
    if params.max_fill_bytes == 0 {
        return Ok(Vec::new());
    }
    let excluded: std::collections::HashSet<&str> =
        params.exclude_item_ids.iter().map(String::as_str).collect();
    let mut result = Vec::new();
    let mut remaining = params.max_fill_bytes;
    let mut offset = 0u32;
    for _ in 0..200 {
        let (albums, total) = provider.list_albums(None, None, offset, 100).await?;
        offset = offset.saturating_add(100);
        for album in albums {
            let detail = provider.get_album(&album.id).await?;
            let parts: Vec<_> = detail
                .tracks
                .into_iter()
                .filter(|track| !excluded.contains(track.id.as_str()))
                .collect();
            let book_size: u64 = parts.iter().map(provider_track_size).sum();
            if book_size == 0 || book_size > remaining {
                continue;
            }
            for part in parts {
                result.push(crate::auto_fill::AutoFillItem {
                    id: part.id.clone(),
                    name: part.title.clone(),
                    album: part.album_title.clone(),
                    artist: part.artist_name.clone(),
                    provider_album_id: part.album_id.clone(),
                    provider_content_type: part.content_type.clone(),
                    provider_suffix: part.suffix.clone(),
                    track_number: part.track_number,
                    size_bytes: provider_track_size(&part),
                    priority_reason: "audiobook".into(),
                    tier: None,
                });
            }
            remaining -= book_size;
        }
        if offset >= total {
            break;
        }
    }
    Ok(result)
}

async fn expand_podcast_auto_fill(
    provider: Arc<dyn MediaProvider>,
    pipeline: Option<&crate::auto_fill::AutoFillPipeline>,
    params: crate::auto_fill::AutoFillParams,
) -> anyhow::Result<Vec<crate::auto_fill::AutoFillItem>> {
    let retention = pipeline
        .map(|p| &p.podcast_retention)
        .cloned()
        .unwrap_or_default();
    let limit = retention.recent_count.min(100);
    if limit == 0 || params.max_fill_bytes == 0 {
        return Ok(Vec::new());
    }
    let selected_shows: std::collections::HashSet<&str> =
        retention.show_ids.iter().map(String::as_str).collect();
    if retention.mode == crate::auto_fill::PodcastSelectionMode::SelectedShows
        && selected_shows.is_empty()
    {
        return Ok(Vec::new());
    }
    let mut shows = Vec::new();
    let mut offset = 0u32;
    loop {
        let (page, total) = provider.list_podcast_shows(offset, 100).await?;
        if page.is_empty() {
            if offset < total {
                anyhow::bail!("Podcast show list ended before the declared total");
            }
            break;
        }
        offset = offset.saturating_add(page.len() as u32);
        shows.extend(page);
        if offset >= total {
            break;
        }
        if shows.len() >= 10_000 {
            anyhow::bail!("Podcast show list exceeds the safe page limit");
        }
    }
    shows.sort_by(|a, b| a.id.cmp(&b.id));
    let excluded: std::collections::HashSet<_> =
        params.exclude_item_ids.iter().map(String::as_str).collect();
    let mut candidates = Vec::new();
    for show in shows {
        if retention.mode == crate::auto_fill::PodcastSelectionMode::SelectedShows
            && !selected_shows.contains(show.id.as_str())
        {
            continue;
        }
        let detail = provider.get_podcast_show(&show.id).await?;
        if detail.possibly_truncated {
            anyhow::bail!("Podcast episode list is truncated; retention cannot safely reconcile");
        }
        let mut episodes = detail.episodes;
        // Parse offsets to UTC; missing or malformed dates come last, then stable episode ID.
        episodes.sort_by(|a, b| {
            podcast_published_at(&b.published_at)
                .cmp(&podcast_published_at(&a.published_at))
                .then_with(|| a.id.cmp(&b.id))
        });
        for episode in episodes
            .into_iter()
            .filter(|episode| !excluded.contains(episode.id.as_str()))
            .take(limit)
        {
            let size_bytes = podcast_estimated_size_bytes(episode.duration_seconds);
            candidates.push((
                podcast_published_at(&episode.published_at),
                episode.id.clone(),
                crate::auto_fill::AutoFillItem {
                    id: episode.id,
                    name: episode.title,
                    album: Some(detail.show.title.clone()),
                    artist: None,
                    provider_album_id: Some(detail.show.id.clone()),
                    provider_content_type: None,
                    provider_suffix: None,
                    track_number: podcast_date_number(&episode.published_at),
                    size_bytes,
                    priority_reason: if retention.unplayed_only {
                        "recent episode (play state unavailable)".into()
                    } else {
                        "recent episode".into()
                    },
                    tier: None,
                },
            ));
        }
    }
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    let mut remaining = params.max_fill_bytes;
    if retention.mode == crate::auto_fill::PodcastSelectionMode::Latest {
        candidates.truncate(limit);
    }
    Ok(candidates
        .into_iter()
        .filter_map(|(_, _, item)| {
            if item.size_bytes <= remaining {
                remaining -= item.size_bytes;
                Some(item)
            } else {
                None
            }
        })
        .collect())
}

fn podcast_published_at(value: &Option<String>) -> Option<i64> {
    value.as_deref().and_then(|date| {
        chrono::DateTime::parse_from_rfc3339(date)
            .ok()
            .map(|date| date.timestamp())
            .or_else(|| {
                Some(
                    chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
                        .ok()?
                        .and_hms_opt(0, 0, 0)?
                        .and_utc()
                        .timestamp(),
                )
            })
    })
}

/// Story 13.1: current time as Unix seconds — the single clock read for auto-fill. The pure engine
/// never reads the clock; it consumes this value via the history snapshot.
pub(crate) fn now_unix_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Story 13.5: mint the caller-supplied **local civil time** for the auto-fill Context stage — the
/// single clock-reading sibling of [`now_unix_secs`]. The pure engine never reads the clock; it
/// consumes these fields via [`crate::auto_fill::HistorySnapshot::local`]. Uses `chrono::Local::now()`
/// (`localtime_r` under the hood — a sound local offset; the `time` crate's `local-offset` feature is
/// deliberately avoided as it returns `None`/errs in this multi-threaded daemon by design).
pub(crate) fn now_civil() -> crate::auto_fill::CivilTime {
    use chrono::{Datelike, Local, Timelike};
    let now = Local::now();
    crate::auto_fill::CivilTime {
        hour: now.hour() as u8,
        month: now.month() as u8,
        day: now.day() as u8,
        // chrono: Mon=0 .. Sun=6 via `num_days_from_monday` — matches our `0=Mon..=6=Sun` convention.
        weekday: now.weekday().num_days_from_monday() as u8,
    }
}

/// Story 13.5 #20: whether the device's selected transcoding profile implies a real (non-passthrough)
/// transcode — i.e. there is something to re-encode and a per-slot bitrate override can take effect.
fn transcode_profile_active(profile_id: Option<&str>) -> bool {
    profile_id.is_some_and(|id| !id.is_empty() && id != "passthrough")
}

/// Story 13.5 #20: a cloned pipeline with `encoding_from_goals` cleared when the flag is set but no
/// transcode profile is active (passthrough). Returns `None` (keep the original borrow) otherwise.
/// Suppressing the flag in passthrough keeps the engine's bitrate-aware byte estimate aligned with
/// reality — there is nothing to re-encode, so the estimate must stay source-based (AC 9 caveat).
fn encoding_passthrough_clear(
    pipeline: Option<&crate::auto_fill::AutoFillPipeline>,
    transcode_active: bool,
) -> Option<crate::auto_fill::AutoFillPipeline> {
    match pipeline {
        Some(p) if p.budget.encoding_from_goals && !transcode_active => {
            let mut cleared = p.clone();
            cleared.budget.encoding_from_goals = false;
            Some(cleared)
        }
        _ => None,
    }
}

/// Story 13.5 #20: the derived per-slot max-bitrate (kbps) to override on the transcode profile for a
/// slot's auto-fill downloads — `None` unless the pipeline enabled encoding-from-goals (with both a
/// byte ceiling and a positive duration goal) AND a transcode profile is active (passthrough can't
/// re-encode). Best-effort and per-slot: it never mutates the device-wide `transcoding_profile_id`.
fn encoding_override_kbps(
    pipeline: Option<&crate::auto_fill::AutoFillPipeline>,
    transcode_active: bool,
) -> Option<u32> {
    if !transcode_active {
        return None;
    }
    pipeline.and_then(|p| crate::auto_fill::pipeline::target_bitrate_kbps(&p.budget))
}

/// Story 13.1: build the DB-sourced auto-fill history snapshot + rotation cursor for a
/// `(device, portable server)` pair. Best-effort: a DB read error yields an empty snapshot so the
/// Memory stage becomes inert rather than aborting the slot.
fn build_autofill_history(
    db: &crate::db::Database,
    device_id: &str,
    server_id: &str,
    now: i64,
) -> (crate::auto_fill::HistorySnapshot, i64, i64) {
    let mut entries = std::collections::HashMap::new();
    match db.get_autofill_history(device_id, server_id) {
        Ok(rows) => {
            for (track_id, last_synced_at, tier) in rows {
                entries.insert(
                    track_id,
                    crate::auto_fill::TrackHistory {
                        last_synced_at,
                        last_played_at: None,
                        tier,
                    },
                );
            }
        }
        Err(e) => crate::daemon_log!("[AutoFill] history read failed (memory inert): {}", e),
    }
    let cursor = db.get_rotation_cursor(device_id, server_id).unwrap_or(0);
    // Story 13.4: the pity dry-streak (best-effort, default 0) drives the discovery reserve.
    let pity_streak = db.get_pity_streak(device_id, server_id).unwrap_or(0);
    // Story 13.5: `local` is set at the engine fill site (overwritten in `expand_with_pipeline`); the
    // DB-sourced snapshot carries the default (civil time is runtime, never persisted).
    (
        crate::auto_fill::HistorySnapshot {
            now,
            entries,
            local: crate::auto_fill::CivilTime::default(),
        },
        cursor,
        pity_streak,
    )
}

/// Story 13.1: copy the rotation-tier index from auto-fill results onto the matching `delta.adds`
/// entries (keyed by provider track id), so the tier survives the delta round-trip to sync-execute
/// where it is recorded into `autofill_history.tier`.
fn patch_delta_tiers(
    delta: &mut crate::sync::SyncDelta,
    tier_by_track: &std::collections::HashMap<String, String>,
) {
    if tier_by_track.is_empty() {
        return;
    }
    for add in &mut delta.adds {
        if let Some(tier) = tier_by_track.get(&add.jellyfin_id) {
            add.tier = Some(tier.clone());
        }
    }
}

/// Mark every item actually added by Auto-Fill, including non-tiered fills.
fn patch_delta_auto_fill(
    delta: &mut crate::sync::SyncDelta,
    auto_fill_ids: &std::collections::HashSet<String>,
) {
    for add in &mut delta.adds {
        add.is_auto_fill = auto_fill_ids.contains(&add.jellyfin_id);
    }
}

/// Story 13.5 #20: copy the encoding-from-goals derived per-slot max-bitrate (kbps) onto the matching
/// `delta.adds` entries (keyed by provider track id), so the sync transcode applies it to those
/// auto-fill downloads only. Mirrors [`patch_delta_tiers`]. Manual items are never in the map, so the
/// override is naturally scoped to the slot's auto-fill items and never touches manual selections or
/// the device-wide profile.
fn patch_delta_bitrate_overrides(
    delta: &mut crate::sync::SyncDelta,
    kbps_by_track: &std::collections::HashMap<String, u32>,
) {
    if kbps_by_track.is_empty() {
        return;
    }
    for add in &mut delta.adds {
        if let Some(kbps) = kbps_by_track.get(&add.jellyfin_id) {
            add.max_bitrate_override_kbps = Some(*kbps);
        }
    }
}

/// Story 13.1: record auto-fill runtime history at sync completion (best-effort; never fails the
/// sync). For each add that carries a portable `server_id`, upsert `(device, server, track)` with
/// `last_synced_at = now` and the add's tier; then advance the rotation cursor once per server whose
/// configured pipeline uses Memory tiers, and prune rows older than the retention window.
fn record_autofill_history_after_sync(
    db: &crate::db::Database,
    manifest: &crate::device::DeviceManifest,
    delta: &crate::sync::SyncDelta,
    errors: &[crate::sync::SyncFileError],
    now: i64,
) {
    use std::collections::{HashMap, HashSet};
    let device_id = manifest.device_id.as_str();

    // Per-track transfer failures: these tracks were NOT written to the device, so they must not be
    // recorded as synced (AC 2 — "actually written to the device").
    let failed_ids: HashSet<&str> = errors
        .iter()
        .map(|e| e.jellyfin_id.as_str())
        .filter(|s| !s.is_empty())
        .collect();

    // Every portable server id this sync touches: freshly-added items, id-changed items, and the
    // resident on-device set (needed to refresh the stable core — see step 3).
    let mut servers: HashSet<String> = HashSet::new();
    for add in &delta.adds {
        if let Some(s) = add.server_id.as_deref().filter(|s| !s.is_empty()) {
            servers.insert(s.to_string());
        }
    }
    for ch in &delta.id_changes {
        if let Some(s) = ch.source_server_id.as_deref().filter(|s| !s.is_empty()) {
            servers.insert(s.to_string());
        }
    }
    for item in &manifest.synced_items {
        if let Some(s) = item.server_id.as_deref().filter(|s| !s.is_empty()) {
            servers.insert(s.to_string());
        }
    }
    if servers.is_empty() {
        return; // legacy/Jellyfin items have no portable id → not tracked for cooldown
    }

    // Load existing history once per server: track_id -> tier. Reused to preserve the tier when
    // refreshing resident rows and when carrying history across an id change.
    let mut history: HashMap<String, HashMap<String, Option<String>>> = HashMap::new();
    for server_id in &servers {
        let rows = db
            .get_autofill_history(device_id, server_id)
            .unwrap_or_default();
        let map = rows
            .into_iter()
            .map(|(track, _last, tier)| (track, tier))
            .collect();
        history.insert(server_id.clone(), map);
    }

    // Tracks leaving the device this sync — never refresh these.
    let removed: HashSet<&str> = delta
        .deletes
        .iter()
        .map(|d| d.jellyfin_id.as_str())
        .chain(delta.id_changes.iter().map(|c| c.old_jellyfin_id.as_str()))
        .collect();

    // Servers that actually had a track written this run — gates the rotation-cursor advance so a
    // fully-failed sync does not rotate the lead tier (AC 8 — "completed sync").
    let mut servers_wrote: HashSet<String> = HashSet::new();

    // 1. Freshly-added auto-fill tracks (skip ones that failed to transfer).
    for add in &delta.adds {
        let Some(server_id) = add.server_id.as_deref().filter(|s| !s.is_empty()) else {
            continue;
        };
        if failed_ids.contains(add.jellyfin_id.as_str()) {
            continue;
        }
        if let Err(e) = db.upsert_autofill_history(
            device_id,
            server_id,
            &add.jellyfin_id,
            Some(now),
            add.tier.as_deref(),
        ) {
            crate::daemon_log!("[AutoFill] history record failed (non-fatal): {}", e);
        }
        servers_wrote.insert(server_id.to_string());
    }

    // 2. Id-changed tracks: carry history (last_synced_at + tier) from the old id to the new id, so a
    //    server re-key does not reset cooldown / stable-core membership / tier for that track.
    for ch in &delta.id_changes {
        let Some(server_id) = ch.source_server_id.as_deref().filter(|s| !s.is_empty()) else {
            continue;
        };
        if failed_ids.contains(ch.new_jellyfin_id.as_str()) {
            continue;
        }
        // Only carry forward tracks we were already tracking (preserves their tier).
        let Some(tier) = history
            .get(server_id)
            .and_then(|m| m.get(&ch.old_jellyfin_id))
            .cloned()
        else {
            continue;
        };
        if let Err(e) = db.upsert_autofill_history(
            device_id,
            server_id,
            &ch.new_jellyfin_id,
            Some(now),
            tier.as_deref(),
        ) {
            crate::daemon_log!(
                "[AutoFill] history id-change carry failed (non-fatal): {}",
                e
            );
        }
    }

    // 3. Refresh `last_synced_at` for resident on-device tracks we already track, so a long-lived
    //    stable core is not pruned out from under itself by the retention cutoff. Preserves the
    //    existing tier and never creates rows for manual/untracked items.
    for item in &manifest.synced_items {
        let Some(server_id) = item.server_id.as_deref().filter(|s| !s.is_empty()) else {
            continue;
        };
        if removed.contains(item.jellyfin_id.as_str()) {
            continue;
        }
        let Some(tier) = history
            .get(server_id)
            .and_then(|m| m.get(&item.jellyfin_id))
            .cloned()
        else {
            continue;
        };
        if let Err(e) = db.upsert_autofill_history(
            device_id,
            server_id,
            &item.jellyfin_id,
            Some(now),
            tier.as_deref(),
        ) {
            crate::daemon_log!("[AutoFill] history refresh failed (non-fatal): {}", e);
        }
    }

    // 4. Retention prune (~1 year) + rotation-cursor advance, per touched server.
    const RETENTION_SECS: i64 = 52 * 7 * 86_400;
    let cutoff = now.saturating_sub(RETENTION_SECS);
    for server_id in &servers {
        let _ = db.prune_autofill_history(device_id, server_id, cutoff);
        // Advance only for tier-using pipelines that actually wrote a track this run. `pipeline_uses_tiers`
        // matches `parse_tiers`, so a malformed/empty `tiers` value (which produced no rotation) does
        // not drift the cursor.
        let uses_tiers = manifest
            .auto_fill
            .pipeline_for(server_id)
            .is_some_and(crate::auto_fill::fetch::pipeline_uses_tiers);
        if uses_tiers
            && servers_wrote.contains(server_id)
            && let Err(e) = db.advance_rotation_cursor(device_id, server_id)
        {
            crate::daemon_log!(
                "[AutoFill] rotation cursor advance failed (non-fatal): {}",
                e
            );
        }

        // Story 13.4 (+ review): pity dry-streak reset/increment, gated like the rotation advance —
        // only a pity-enabled server that actually wrote a track this run. Reset to 0 **only** when
        // the discovery reserve *genuinely fired* this run (`delta.pity_fired_servers`, set at fill
        // time from the shared `pity_reserve_bytes` gate — enabled + streak ≥ threshold + bounded
        // budget + positive reserve). Otherwise advance by 1, so a streak that crossed the threshold
        // while the budget was unbounded or the ratio rounded to zero stays armed (and keeps growing
        // past the threshold) until a run where the reserve actually fires — rather than silently
        // consuming the timer without delivering. Best-effort (never fails the sync).
        if servers_wrote.contains(server_id)
            && manifest
                .auto_fill
                .pipeline_for(server_id)
                .is_some_and(|p| p.pity.enabled)
        {
            let fired = delta.pity_fired_servers.iter().any(|s| s == server_id);
            let next = if fired {
                0 // the discovery reserve genuinely fired this run → dry spell broken
            } else {
                db.get_pity_streak(device_id, server_id).unwrap_or(0) + 1
            };
            if let Err(e) = db.set_pity_streak(device_id, server_id, next) {
                crate::daemon_log!("[AutoFill] pity streak update failed (non-fatal): {}", e);
            }
        }
    }
}

/// Story 12.4: true when any auto-fill slot's resolved server has a configured NON-default
/// pipeline. The Jellyfin-client fast path cannot run a configurable pipeline, so this forces the
/// per-provider routing path. `auto_fill_servers` are the resolved portable serverIds
/// (selected-server fallback already applied by the caller).
fn auto_fill_needs_configurable_routing(
    manifest: &crate::device::DeviceManifest,
    auto_fill_servers: &[String],
) -> bool {
    auto_fill_servers.iter().any(|sid| {
        manifest
            .auto_fill
            .pipeline_for(sid)
            .is_some_and(crate::auto_fill::needs_configurable_expansion)
    })
}

/// True when the basket items resolve to more than one distinct server (each
/// item's serverId, or the selected server when unspecified).
/// True when the sync must route items to per-server providers rather than the
/// single-server dispatch. The single-server path always uses the *selected*
/// provider, so it is only correct when every resolved item (and EVERY auto-fill
/// slot) belongs to the selected server. Routing is therefore needed when items
/// or auto-fill slots span multiple servers OR the sole server is not the
/// selected one — the latter happens with a basket holding only locked,
/// other-server items (AC26/AC28), or an auto-fill slot for a non-selected
/// server (Story 12.3 AC5). `auto_fill_servers` holds the resolved serverIds of
/// every enabled descriptor (selected-fallback already applied by the caller).
fn sync_needs_provider_routing(
    item_specs: &[(String, Option<String>)],
    selected_id: Option<&str>,
    auto_fill_servers: &[String],
) -> bool {
    let mut servers: HashSet<String> = HashSet::new();
    for (_, server) in item_specs {
        let resolved = server.as_deref().or(selected_id);
        if let Some(s) = resolved {
            servers.insert(s.to_string());
        }
    }
    for af in auto_fill_servers {
        servers.insert(af.clone());
    }
    match selected_id {
        Some(sel) => servers.iter().any(|s| s != sel),
        // Nothing selected: any concrete server means we must route explicitly.
        None => !servers.is_empty(),
    }
}

/// Resolves a mixed-server basket by routing each item to its originating
/// provider (AC28). Each resolved DesiredItem is tagged with its `server_id` so
/// execute can download from the correct server. Works for any provider type
/// (Jellyfin + Subsonic) via the generic `provider_sync_items_for_id`.
async fn multi_provider_calculate_delta(
    state: &AppState,
    item_specs: &[(String, Option<String>)],
    manifest: &crate::device::DeviceManifest,
    params: &Value,
) -> Result<Value, JsonRpcError> {
    // Items carry the portable serverId (Story 2.13); group + route by portable id
    // and fall back to the selected server's portable id for untagged items so
    // manifest tags are always portable.
    let selected_id = current_server_portable_id(state)?;
    let basket_items = basket_items_from_params_or_manifest(params, manifest);
    let favorite_basket_by_id: HashMap<String, crate::device::BasketItem> = basket_items
        .into_iter()
        .filter(|item| matches!(item.item_type.as_str(), "FavoriteArtist" | "FavoriteAlbum"))
        .map(|item| (item.id.clone(), item))
        .collect();

    // Group ids by their resolved serverId, preserving order. Items lacking a
    // serverId fall back to the selected server's portable id; if neither is
    // available we surface a clear error instead of silently dropping items
    // (which previously masked first-launch races where the UI sent untagged
    // ids before `selectedServerPortableId` was populated).
    let mut groups: Vec<(String, Vec<String>)> = Vec::new();
    for (id, server) in item_specs {
        let server_id = match server.clone().or_else(|| selected_id.clone()) {
            Some(s) => s,
            None => {
                return Err(JsonRpcError {
                    code: ERR_INVALID_PARAMS,
                    message: format!(
                        "Item {} has no serverId and no server is selected; cannot route sync request",
                        id
                    ),
                    data: None,
                });
            }
        };
        match groups.iter_mut().find(|(s, _)| *s == server_id) {
            Some((_, ids)) => ids.push(id.clone()),
            None => groups.push((server_id, vec![id.clone()])),
        }
    }

    let mut desired_items: Vec<crate::sync::DesiredItem> = Vec::new();
    let mut playlist_sync_items = Vec::new();
    let mut seen_ids = HashSet::new();

    for (server_id, ids) in &groups {
        let provider = get_provider_by_server_id_for(state, server_id).await?;
        for id in ids {
            if let Some(basket_item) = favorite_basket_by_id.get(id) {
                let tracks =
                    provider_favorite_sync_items_for_basket_item(provider.clone(), basket_item)
                        .await?;
                for mut item in tracks {
                    if seen_ids.insert(item.jellyfin_id.clone()) {
                        item.server_id = Some(server_id.clone());
                        item.media_role = provider_media_role(provider.as_ref());
                        desired_items.push(item);
                    }
                }
                continue;
            }
            let (tracks, playlist) = provider_sync_items_for_id(provider.clone(), id).await?;
            if let Some(playlist) = playlist {
                playlist_sync_items.push(playlist);
            }
            for mut item in tracks {
                if seen_ids.insert(item.jellyfin_id.clone()) {
                    item.server_id = Some(server_id.clone());
                    item.media_role = provider_media_role(provider.as_ref());
                    desired_items.push(item);
                }
            }
        }
    }

    // Auto-fill: one slot per server (Story 12.3, AC2/AC3/AC4). Each descriptor
    // expands against its OWN provider (routed by portable serverId), tagging its
    // items with that server's id. Manual items already own their ids in
    // `seen_ids` and win dedup; slots run in descriptor order, each excluding all
    // already-selected ids (manual + earlier slots). Slots share one remaining
    // capacity budget so combined fill never oversubscribes the device (AC4).
    let descriptors = parse_auto_fill_descriptors(params);
    // Story 13.1: rotation-tier index per emitted track across all slots, patched onto delta.adds.
    let mut af_tier_map: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    let mut af_item_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    // Story 13.5 #20: encoding-from-goals derived per-slot max-bitrate (kbps) per emitted auto-fill
    // track across all slots, patched onto delta.adds (mirrors `af_tier_map`).
    let mut af_bitrate_map: std::collections::HashMap<String, u32> =
        std::collections::HashMap::new();
    // Story 13.4 review: portable server ids whose pity discovery reserve genuinely fired this run.
    let mut af_pity_fired: Vec<String> = Vec::new();
    let mut autofill_playlist_tracks = Vec::new();
    if !descriptors.is_empty() {
        // Shared budget = device free + already-synced − already-selected bytes
        // (mirrors `provider_calculate_delta`'s server-side derivation at
        // rpc.rs:2599-2616). `None` when device storage is unavailable; that is
        // tolerated only for slots that supply their own `maxBytes` — matching the
        // pre-12.3 multi path, which used the UI `maxBytes` without querying storage.
        let selected_bytes: u64 = desired_items.iter().map(|i| i.size_bytes).sum();
        let mut remaining: Option<u64> =
            match free_bytes_for_sync_device(state, &manifest.device_id).await {
                Some(free_bytes) => {
                    let synced: u64 = manifest.synced_items.iter().map(|s| s.size_bytes).sum();
                    Some(
                        free_bytes
                            .saturating_add(synced)
                            .saturating_sub(selected_bytes),
                    )
                }
                None => None,
            };

        for desc in &descriptors {
            let af_server = match desc.server_id.clone().or_else(|| selected_id.clone()) {
                Some(s) => s,
                None => {
                    crate::daemon_log!(
                        "[AutoFill] skipping slot: no serverId and no server selected"
                    );
                    continue;
                }
            };

            // Effective slot budget: cap the descriptor's own maxBytes (if any) by
            // the shared remaining capacity. For a single slot this is byte-for-byte
            // identical to today (min(maxBytes, free − manual) == maxBytes).
            let budget = match (desc.max_bytes, remaining) {
                (Some(mb), Some(r)) => mb.min(r),
                (Some(mb), None) => mb,
                (None, Some(r)) => r,
                (None, None) => {
                    return Err(JsonRpcError {
                        code: ERR_CONNECTION_FAILED,
                        message: "Cannot determine device capacity for auto-fill".to_string(),
                        data: None,
                    });
                }
            };
            if budget == 0 {
                continue;
            }

            // Auto-fill slots are best-effort: a single unresolvable/offline server
            // must not abort the whole multi-server delta (manual items + healthy
            // slots). Log and skip the failed slot instead of propagating the error.
            let provider = match get_provider_by_server_id_for(state, &af_server).await {
                Ok(p) => p,
                Err(e) => {
                    if manifest.synced_items.iter().any(|item| {
                        item.server_id.as_deref() == Some(&af_server)
                            && item.media_role == crate::device::MediaRole::Podcast
                    }) {
                        return Err(JsonRpcError { code: ERR_CONNECTION_FAILED, message: "Podcast source unavailable; sync cannot safely reconcile retained episodes".into(), data: None });
                    }
                    crate::daemon_log!(
                        "[AutoFill] skipping slot for server {}: provider unavailable: {}",
                        af_server,
                        e.message
                    );
                    continue;
                }
            };
            // Exclude every already-selected id (manual items + earlier slots) plus
            // any ids the descriptor explicitly excludes.
            let mut exclude_ids: Vec<String> = desired_items
                .iter()
                .map(|i| i.jellyfin_id.clone())
                .collect();
            exclude_ids.extend(desc.exclude_item_ids.iter().cloned());
            // Story 13.1: DB-sourced history snapshot + rotation cursor for this slot's server.
            let now = now_unix_secs();
            let (history, rotation_cursor, pity_streak) =
                build_autofill_history(&state.db, &manifest.device_id, &af_server, now);
            let fill_params = crate::auto_fill::AutoFillParams {
                exclude_item_ids: exclude_ids,
                max_fill_bytes: budget,
                device_id: manifest.device_id.clone(),
                server_id: af_server.clone(),
                now_unix: now,
                history,
                rotation_cursor,
                seed: now as u64,
                pity_streak,
                // Story 13.5: engine fill site → mint local civil time for the Context stage.
                local: now_civil(),
            };
            // Story 12.4: route this slot through the shared seam — the configurable engine
            // when this server has a non-default pipeline, else the default fast path.
            let pipeline_ref = manifest.auto_fill.pipeline_for(&af_server);
            // Story 13.5 #20: derive this slot's encoding-from-goals transcode bitrate (only when a
            // transcode profile is active) and gate the bitrate-aware estimate in passthrough.
            let transcode_active =
                transcode_profile_active(manifest.transcoding_profile_id.as_deref());
            let af_bitrate_override = encoding_override_kbps(pipeline_ref, transcode_active);
            let encoding_cleared = encoding_passthrough_clear(pipeline_ref, transcode_active);
            let pipeline_opt = encoding_cleared.as_ref().or(pipeline_ref);
            // Story 13.4 review: did the pity reserve genuinely fire for this slot (same gate the
            // engine uses, with this slot's budget)? Drives the dry-streak reset at sync completion.
            if let Some(pipeline) = pipeline_opt
                && crate::auto_fill::pipeline::pity_reserve_bytes(
                    &pipeline.pity,
                    pity_streak,
                    budget,
                ) > 0
            {
                af_pity_fired.push(af_server.clone());
            }
            let fill_items =
                match expand_auto_fill_slot(provider.clone(), pipeline_opt, fill_params).await {
                    Ok(items) => items,
                    Err(e) => {
                        if provider_media_role(provider.as_ref())
                            == crate::device::MediaRole::Podcast
                        {
                            return Err(JsonRpcError {
                                code: ERR_CONNECTION_FAILED,
                                message: format!("Podcast retention unavailable: {e}"),
                                data: None,
                            });
                        }
                        crate::daemon_log!(
                            "[AutoFill] skipping slot for server {}: expansion failed: {}",
                            af_server,
                            e
                        );
                        continue;
                    }
                };
            for item in &fill_items {
                if let Some(tier) = item.tier.clone() {
                    af_tier_map.insert(item.id.clone(), tier);
                }
                // Story 13.5 #20: stamp the slot's derived bitrate onto each auto-fill track (manual
                // items carry nothing), so the sync transcode applies it to these items only.
                if let Some(kbps) = af_bitrate_override {
                    af_bitrate_map.insert(item.id.clone(), kbps);
                }
            }
            let first_added = desired_items.len();
            push_fill_items_dedup(
                fill_items,
                &mut desired_items,
                &mut seen_ids,
                &af_server,
                provider_media_role(provider.as_ref()),
                &mut remaining,
                &mut autofill_playlist_tracks,
            );
            af_item_ids.extend(
                desired_items[first_added..]
                    .iter()
                    .map(|item| item.jellyfin_id.clone()),
            );
        }
    }
    if !autofill_playlist_tracks.is_empty() {
        playlist_sync_items.push(autofill_playlist_item(autofill_playlist_tracks));
    }

    let mut delta = crate::sync::calculate_delta(&desired_items, manifest);
    patch_delta_tiers(&mut delta, &af_tier_map);
    patch_delta_bitrate_overrides(&mut delta, &af_bitrate_map);
    delta.pity_fired_servers = af_pity_fired;
    delta.playlists = playlist_sync_items;
    if let Some((_, _, device_io)) = state
        .device_manager
        .get_sync_target_for_device(&manifest.device_id)
        .await
    {
        crate::sync::augment_delta_with_existence_check(
            &mut delta,
            &desired_items,
            manifest,
            device_io.as_ref(),
        )
        .await;
    }
    patch_delta_auto_fill(&mut delta, &af_item_ids);
    preflight_state_media_adds(state, manifest, &mut delta, None).await?;
    Ok(delta_value_with_cleanup_metadata(&delta, manifest))
}

async fn handle_sync_calculate_delta(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    // Claim the pipeline lock for the full duration of this call (auto-fill can run for
    // several seconds; without this, a concurrent auto-sync would double-paginate the library).
    let _pipeline_guard =
        state
            .sync_operation_manager
            .try_start_pipeline()
            .ok_or(JsonRpcError {
                code: ERR_SYNC_IN_PROGRESS,
                message: "A sync operation is already in progress".to_string(),
                data: None,
            })?;

    let params = params.ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing params".to_string(),
        data: None,
    })?;

    let raw_item_ids = params["itemIds"].as_array().ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing or invalid itemIds array".to_string(),
        data: None,
    })?;

    // itemIds accepts either legacy `string[]` or `Array<{ id, serverId }>` (AC27).
    let item_specs: Vec<(String, Option<String>)> = parse_item_specs(raw_item_ids);
    let item_ids: Vec<String> = item_specs.iter().map(|(id, _)| id.clone()).collect();

    // Get current device manifest
    let manifest = state
        .device_manager
        .get_current_device()
        .await
        .ok_or(JsonRpcError {
            code: ERR_CONNECTION_FAILED,
            message: "No device connected".to_string(),
            data: None,
        })?;

    // Multi-server routing (AC27/AC28): when the basket (or an auto-fill slot bound
    // to a different server) spans more than one server, resolve each item against
    // its originating provider. Single-server baskets keep the existing dispatch
    // unchanged (AC21). Item/auto-fill serverIds are portable (Story 2.13), so the
    // routing decision compares against the selected server's portable id.
    let selected_id = current_server_portable_id(state)?;
    // Story 12.3: every auto-fill slot counts toward the routing decision. Resolve
    // each enabled descriptor's serverId (selected-server fallback for `None`), so
    // a single-server basket carrying auto-fill slots for other servers still routes
    // through the per-provider path (AC5).
    let auto_fill_servers: Vec<String> = parse_auto_fill_descriptors(&params)
        .into_iter()
        .filter_map(|d| d.server_id.or_else(|| selected_id.clone()))
        .collect();
    // Story 12.4: the Jellyfin-client fast path (`run_auto_fill`) cannot express a configurable
    // pipeline, so when any auto-fill slot's server has a configured NON-default pipeline, force
    // the per-provider path (which routes each slot through `expand_auto_fill_slot`). The pure
    // default case still takes the Jellyfin-direct path unchanged (AC 6, AC 8).
    let configured_pipeline_applies =
        auto_fill_needs_configurable_routing(&manifest, &auto_fill_servers);
    if configured_pipeline_applies
        || sync_needs_provider_routing(&item_specs, selected_id.as_deref(), &auto_fill_servers)
    {
        let result = multi_provider_calculate_delta(state, &item_specs, &manifest, &params).await;
        if state.sync_operation_manager.is_pipeline_cancelled() {
            return Err(sync_cancelled_error());
        }
        return result;
    }

    if let Some(provider) = active_non_jellyfin_provider(state).await {
        let result = provider_calculate_delta(state, provider, &item_ids, &manifest, &params).await;
        if state.sync_operation_manager.is_pipeline_cancelled() {
            return Err(sync_cancelled_error());
        }
        return result;
    }

    // Fetch item details from Jellyfin for each desired ID
    let (url, token, user_id) = CredentialManager::get_credentials().map_err(|e| JsonRpcError {
        code: ERR_STORAGE_ERROR,
        message: format!("Failed to get credentials: {}", e),
        data: None,
    })?;
    let user_id = user_id.unwrap_or_else(|| "Me".to_string());

    let is_downloadable_item_type = |item_type: &str| matches!(item_type, "Audio" | "MusicVideo");

    let to_desired_item = |item: crate::api::JellyfinItem| {
        let size_bytes = item
            .media_sources
            .as_ref()
            .and_then(|sources| sources.first())
            .and_then(|s| s.size)
            .unwrap_or(0) as u64;
        let provider_suffix = item
            .media_sources
            .as_ref()
            .and_then(|sources| sources.first())
            .and_then(|source| source.container.clone())
            .or_else(|| item.container.clone());
        let original_bitrate = item
            .media_sources
            .as_ref()
            .and_then(|sources| sources.first())
            .and_then(|s| {
                s.bitrate.or_else(|| {
                    s.media_streams
                        .as_ref()?
                        .iter()
                        .find(|ms| ms.stream_type == "Audio")
                        .and_then(|ms| ms.bit_rate)
                })
            })
            .or(item.bitrate);
        crate::sync::DesiredItem {
            media_role: crate::device::MediaRole::Music,
            jellyfin_id: item.id,
            name: item.name,
            album: item.album,
            artist: item.album_artist,
            size_bytes,
            etag: item.etag,
            provider_album_id: item.album_id,
            provider_content_type: None,
            provider_suffix,
            original_bitrate,
            track_number: item.index_number,
            server_id: None,
        }
    };

    crate::daemon_log!(
        "[Delta] Calculating delta for {} item(s): {:?}",
        item_ids.len(),
        item_ids
    );

    // Fetch item details from Jellyfin in chunks to avoid URL length limits.
    // Container items (playlist/album/artist) are expanded to individual tracks.
    let basket_items = basket_items_from_params_or_manifest(&params, &manifest);
    let favorite_basket_by_id: HashMap<String, crate::device::BasketItem> = basket_items
        .into_iter()
        .filter(|item| matches!(item.item_type.as_str(), "FavoriteArtist" | "FavoriteAlbum"))
        .map(|item| (item.id.clone(), item))
        .collect();
    let normal_item_ids: Vec<String> = item_ids
        .iter()
        .filter(|id| !favorite_basket_by_id.contains_key(*id))
        .cloned()
        .collect();
    let mut playlist_sync_items: Vec<crate::sync::PlaylistSyncItem> = Vec::new();
    let mut results = Vec::new();
    for item_id in item_ids
        .iter()
        .filter(|id| favorite_basket_by_id.contains_key(*id))
    {
        if state.sync_operation_manager.is_pipeline_cancelled() {
            return Err(sync_cancelled_error());
        }
        if let Some(favorite_item) = favorite_basket_by_id.get(item_id) {
            match jellyfin_favorite_sync_items_for_basket_item(
                &state.jellyfin_client,
                &url,
                &token,
                &user_id,
                favorite_item,
            )
            .await
            {
                Ok(items) => results.extend(items.into_iter().map(Ok)),
                Err(error) => results.push(Err(error.message)),
            }
        }
    }

    for chunk in normal_item_ids.chunks(100) {
        if state.sync_operation_manager.is_pipeline_cancelled() {
            return Err(sync_cancelled_error());
        }
        let chunk_strs: Vec<&str> = chunk.iter().map(|s| s.as_str()).collect();
        match state
            .jellyfin_client
            .get_items_by_ids(&url, &token, &user_id, &chunk_strs)
            .await
        {
            Ok(items) => {
                let mut fetched_ids: HashSet<String> = HashSet::new();
                for item in items {
                    crate::daemon_log!(
                        "[Delta] Resolved item '{}' (id={}, type={})",
                        item.name,
                        item.id,
                        item.item_type
                    );
                    fetched_ids.insert(item.id.clone());

                    if is_downloadable_item_type(&item.item_type) {
                        results.push(Ok(to_desired_item(item)));
                        continue;
                    }

                    let is_playlist = item.item_type == "Playlist";
                    let item_id = item.id.clone();
                    let item_name = item.name.clone();

                    // Genre items use GenreIds query — ParentId expansion via get_child_items_with_sizes doesn't work for Jellyfin genre entities.
                    // Jellyfin returns genre items with ItemType "MusicGenre" (not "Genre"), so we match both.
                    if matches!(item.item_type.as_str(), "Genre" | "MusicGenre") {
                        crate::daemon_log!(
                            "[Genre Sync] Expanding genre '{}' (id={}, type={})",
                            item_name,
                            item_id,
                            item.item_type
                        );
                        let mut start_index = 0;
                        let mut total_record_count: Option<u32> = None;

                        for page_index in 0..GENRE_TRACK_MAX_PAGES {
                            if state.sync_operation_manager.is_pipeline_cancelled() {
                                return Err(sync_cancelled_error());
                            }
                            crate::daemon_log!(
                                "[Genre Sync] Fetching page {} for genre '{}' (offset={})",
                                page_index,
                                item_name,
                                start_index
                            );
                            match state
                                .jellyfin_client
                                .get_songs_by_genre(
                                    &url,
                                    &token,
                                    &user_id,
                                    &item_id,
                                    start_index,
                                    GENRE_TRACK_PAGE_SIZE,
                                )
                                .await
                            {
                                Ok(response) => {
                                    let fetched = response.items.len() as u32;
                                    let total = *total_record_count
                                        .get_or_insert(response.total_record_count);

                                    crate::daemon_log!(
                                        "[Genre Sync] Page {}: got {}/{} tracks for genre '{}'",
                                        page_index,
                                        fetched,
                                        total,
                                        item_name
                                    );

                                    if fetched == 0 {
                                        if total > 0 && start_index < total {
                                            results.push(Err(format!(
                                                "Failed to expand genre {item_id}: empty page at offset {start_index} before total {total}"
                                            )));
                                        }
                                        break;
                                    }

                                    for track in response.items {
                                        if is_downloadable_item_type(&track.item_type) {
                                            results.push(Ok(to_desired_item(track)));
                                        }
                                    }

                                    let next_index = start_index.saturating_add(fetched);
                                    let reached_end = if total > 0 {
                                        next_index >= total
                                    } else {
                                        fetched < GENRE_TRACK_PAGE_SIZE
                                    };
                                    if reached_end {
                                        crate::daemon_log!(
                                            "[Genre Sync] Finished expanding genre '{}': {} tracks total",
                                            item_name,
                                            next_index
                                        );
                                        break;
                                    }

                                    if page_index + 1 >= GENRE_TRACK_MAX_PAGES {
                                        results.push(Err(format!(
                                            "Failed to expand genre {item_id}: exceeded pagination guard after {next_index} tracks"
                                        )));
                                        break;
                                    }

                                    start_index = next_index;
                                }
                                Err(e) => {
                                    crate::daemon_log!(
                                        "[Genre Sync] Error fetching page {} for genre '{}': {}",
                                        page_index,
                                        item_name,
                                        e
                                    );
                                    results.push(Err(format!(
                                        "Failed to expand genre {item_id}: {e}"
                                    )));
                                    break;
                                }
                            }
                        }
                        continue;
                    }

                    match state
                        .jellyfin_client
                        .get_child_items_with_sizes(&url, &token, &user_id, &item.id)
                        .await
                    {
                        Ok(children) => {
                            if is_playlist {
                                let tracks: Vec<crate::sync::PlaylistTrackInfo> = children
                                    .iter()
                                    .filter(|c| is_downloadable_item_type(&c.item_type))
                                    .map(|c| crate::sync::PlaylistTrackInfo {
                                        jellyfin_id: c.id.clone(),
                                        artist: c.album_artist.clone(),
                                        run_time_seconds: c
                                            .run_time_ticks
                                            .map(|t| (t / 10_000_000) as i64)
                                            .unwrap_or(-1),
                                    })
                                    .collect();
                                playlist_sync_items.push(crate::sync::PlaylistSyncItem {
                                    jellyfin_id: item_id,
                                    name: item_name,
                                    tracks,
                                });
                            }
                            for child in children {
                                if is_downloadable_item_type(&child.item_type) {
                                    results.push(Ok(to_desired_item(child)));
                                }
                            }
                        }
                        Err(e) => {
                            results.push(Err(format!("Failed to expand item {}: {}", item.id, e)));
                        }
                    }
                }

                for requested_id in chunk {
                    if !fetched_ids.contains(requested_id) {
                        results.push(Err(format!(
                            "Failed to fetch item {}: Not found",
                            requested_id
                        )));
                    }
                }
            }
            Err(e) => {
                // If a chunk fails, record the error for these items
                for id in chunk {
                    results.push(Err(format!("Failed to fetch item {}: {}", id, e)));
                }
            }
        }
    }

    // Check for errors - if ANY item fails, we must abort to prevent data loss (deleting valid items)
    let mut desired_items = Vec::with_capacity(results.len());
    let mut seen_ids = HashSet::new();
    for res in results {
        match res {
            Ok(item) => {
                if seen_ids.insert(item.jellyfin_id.clone()) {
                    desired_items.push(item);
                }
            }
            Err(e) => {
                return Err(JsonRpcError {
                    code: ERR_CONNECTION_FAILED,
                    message: format!("Sync aborted: {}", e),
                    data: None,
                });
            }
        }
    }

    // Auto-fill expansion (Story 3.8): if the basket contained an auto-fill slot,
    // run the priority algorithm now and merge results with manual items.
    // Story 12.3: normalize both the legacy single object and the array form
    // (AC1). This Jellyfin fast path is reached only when routing resolved every
    // auto-fill slot to the selected server. For the legacy object this is
    // byte-for-byte identical to the old `enabled`/`maxBytes`/`excludeItemIds` reads.
    let mut autofill_playlist_tracks = Vec::new();
    let mut af_item_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    if let Some(desc) = parse_auto_fill_descriptors(&params).into_iter().next() {
        let max_fill_bytes = if let Some(mb) = desc.max_bytes {
            mb
        } else {
            match free_bytes_for_sync_device(state, &manifest.device_id).await {
                Some(free_bytes) => free_bytes,
                None => {
                    return Err(JsonRpcError {
                        code: ERR_CONNECTION_FAILED,
                        message: "Auto-fill: could not determine device free space".to_string(),
                        data: None,
                    });
                }
            }
        };
        let exclude_ids: Vec<String> = desc.exclude_item_ids;
        let expanded_excludes = expand_exclude_ids(&state.jellyfin_client, exclude_ids).await;
        // Legacy Jellyfin fast path: Memory features don't apply (Story 13.1/13.4 fields are inert).
        let fill_params = crate::auto_fill::AutoFillParams {
            exclude_item_ids: expanded_excludes,
            max_fill_bytes,
            device_id: String::new(),
            server_id: String::new(),
            now_unix: now_unix_secs(),
            history: crate::auto_fill::HistorySnapshot::default(),
            rotation_cursor: 0,
            seed: 0,
            pity_streak: 0,
            // Story 13.5: legacy Jellyfin path — civil time inert (Context stage never runs here).
            local: crate::auto_fill::CivilTime::default(),
        };
        match crate::auto_fill::run_auto_fill(&state.jellyfin_client, fill_params).await {
            Ok(af_items) => {
                let af_total_bytes: u64 = af_items.iter().map(|i| i.size_bytes).sum();
                crate::daemon_log!(
                    "[AutoFill] Pagination complete: {} tracks, {} MB",
                    af_items.len(),
                    af_total_bytes / 1_048_576,
                );
                for item in af_items {
                    let item_id = item.id.clone();
                    if seen_ids.insert(item_id) {
                        af_item_ids.insert(item.id.clone());
                        autofill_playlist_tracks.push(autofill_playlist_track(&item));
                        desired_items.push(crate::sync::DesiredItem {
                            media_role: crate::device::MediaRole::Music,
                            jellyfin_id: item.id,
                            name: item.name,
                            album: item.album,
                            artist: item.artist,
                            size_bytes: item.size_bytes,
                            etag: None,
                            provider_album_id: None,
                            provider_content_type: item.provider_content_type,
                            provider_suffix: item.provider_suffix,
                            original_bitrate: None,
                            track_number: item.track_number,
                            server_id: None,
                        });
                    }
                }
            }
            Err(e) => {
                return Err(JsonRpcError {
                    code: ERR_CONNECTION_FAILED,
                    message: format!("Auto-fill expansion failed at sync time: {}", e),
                    data: None,
                });
            }
        }
    }
    if !autofill_playlist_tracks.is_empty() {
        playlist_sync_items.push(autofill_playlist_item(autofill_playlist_tracks));
    }

    // Story 2.13: tag untagged items with the selected server's portable id.
    tag_untagged_with_selected_portable(state, &mut desired_items)?;

    crate::daemon_log!(
        "[Sync] Computing delta: {} desired items vs {} synced in manifest",
        desired_items.len(),
        manifest.synced_items.len(),
    );
    let mut delta = crate::sync::calculate_delta(&desired_items, &manifest);
    delta.playlists = playlist_sync_items;
    crate::daemon_log!(
        "[Sync] Delta computed: {} adds, {} deletes, {} id-changes, {} unchanged, reasons={}",
        delta.adds.len(),
        delta.deletes.len(),
        delta.id_changes.len(),
        delta.unchanged,
        crate::sync::format_change_reason_summary(&delta)
    );
    if !delta.id_changes.is_empty() {
        crate::daemon_log!(
            "[Sync] Id-change sample: {}",
            crate::sync::format_id_change_diagnostics(&delta, 5)
        );
    }

    if let Some((_, _, device_io)) = state
        .device_manager
        .get_sync_target_for_device(&manifest.device_id)
        .await
    {
        crate::daemon_log!(
            "[Sync] Checking device file existence for {} synced items",
            manifest.synced_items.len(),
        );
        crate::sync::augment_delta_with_existence_check(
            &mut delta,
            &desired_items,
            &manifest,
            device_io.as_ref(),
        )
        .await;
        crate::daemon_log!(
            "[Sync] Existence check complete: {} adds after recovery, reasons={}",
            delta.adds.len(),
            crate::sync::format_change_reason_summary(&delta)
        );
    }
    patch_delta_auto_fill(&mut delta, &af_item_ids);

    if state.sync_operation_manager.is_pipeline_cancelled() {
        return Err(sync_cancelled_error());
    }
    Ok(delta_value_with_cleanup_metadata(&delta, &manifest))
}

async fn handle_sync_detect_changes(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let params = params.ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing params".to_string(),
        data: None,
    })?;
    if !params.is_object() || params.get("syncToken").is_none() {
        return Err(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Missing syncToken".to_string(),
            data: None,
        });
    }
    let token = match params.get("syncToken") {
        Some(Value::Null) => None,
        Some(Value::String(token)) => Some(token.clone()),
        _ => {
            return Err(JsonRpcError {
                code: ERR_INVALID_PARAMS,
                message: "syncToken must be a string or null".to_string(),
                data: None,
            });
        }
    };

    let manifest = state
        .device_manager
        .get_current_device()
        .await
        .ok_or(JsonRpcError {
            code: ERR_CONNECTION_FAILED,
            message: "No device connected".to_string(),
            data: None,
        })?;

    let provider = require_provider(state).await?;

    let context = manifest.provider_change_context();
    let changes = match provider
        .changes_since_with_context(token.as_deref(), &context)
        .await
    {
        Ok(changes) => changes,
        // Subsonic API error 70 ("data not found") means the change-log entry
        // for this sync token has expired or was never recorded.  Treat it as
        // an empty delta — the UI will fall back to a full-basket sync, which
        // is always safe.  Any other error is a genuine failure worth surfacing.
        Err(crate::providers::ProviderError::NotFound { .. }) => {
            eprintln!(
                "[SyncDetect] Sync token stale or not found — returning empty changes (full sync required)"
            );
            vec![]
        }
        Err(e) => {
            return Err(JsonRpcError {
                code: ERR_INTERNAL_ERROR,
                message: format!("Change detection failed: {}", e),
                data: None,
            });
        }
    };

    // Enrich each change with metadata parsed from the Subsonic version string
    // ("subsonic:{id}|{size}|{contentType}|{suffix}"), so callers can populate
    // provider_content_type/provider_suffix in SyncAddItems without extra API calls.
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct DetectedChange {
        id: String,
        item_type: String,
        change_type: String,
        version: Option<String>,
        provider_album_id: Option<String>,
        provider_size: Option<u64>,
        provider_content_type: Option<String>,
        provider_suffix: Option<String>,
    }

    fn item_type_wire(item_type: ItemType) -> &'static str {
        match item_type {
            ItemType::Library => "library",
            ItemType::Artist => "artist",
            ItemType::Album => "album",
            ItemType::Song => "song",
            ItemType::Playlist => "playlist",
        }
    }

    fn change_type_wire(change_type: ChangeType) -> &'static str {
        match change_type {
            ChangeType::Created => "created",
            ChangeType::Updated => "updated",
            ChangeType::Deleted => "deleted",
        }
    }

    let enriched: Vec<DetectedChange> = changes
        .into_iter()
        .map(|event| {
            let metadata = match event.change_type {
                ChangeType::Created | ChangeType::Updated => provider.change_metadata(&event),
                ChangeType::Deleted => None,
            };
            DetectedChange {
                id: event.item.id,
                item_type: item_type_wire(event.item.item_type).to_string(),
                change_type: change_type_wire(event.change_type).to_string(),
                version: event.version,
                provider_album_id: metadata
                    .as_ref()
                    .and_then(|metadata| metadata.album_id.clone()),
                provider_size: metadata.as_ref().and_then(|metadata| metadata.size),
                provider_content_type: metadata
                    .as_ref()
                    .and_then(|metadata| metadata.content_type.clone()),
                provider_suffix: metadata.and_then(|metadata| metadata.suffix),
            }
        })
        .collect();

    Ok(serde_json::to_value(enriched).unwrap())
}

async fn handle_sync_execute(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let params = params.ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing params".to_string(),
        data: None,
    })?;
    let pipeline_guard = state
        .sync_operation_manager
        .try_start_pipeline()
        .ok_or(JsonRpcError {
            code: ERR_SYNC_IN_PROGRESS,
            message: "A sync operation is already in progress or the daemon is stopping"
                .to_string(),
            data: None,
        })?;

    // Extract delta from params
    let mut delta: crate::sync::SyncDelta = serde_json::from_value(params["delta"].clone())
        .map_err(|e| JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: format!("Invalid delta parameter: {}", e),
            data: None,
        })?;
    let destructive_cleanup_confirmed = params
        .get("confirmDestructiveCleanup")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let force_sync = params
        .get("force")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let target: crate::sync::SyncTarget = state
        .device_manager
        .get_selected_sync_target()
        .await
        .ok_or(JsonRpcError {
            code: ERR_CONNECTION_FAILED,
            message: "No device connected".to_string(),
            data: None,
        })?
        .into();
    let manifest = &target.manifest;

    // Force sync: promote all currently-synced items to adds+deletes, bypassing the delta.
    if force_sync {
        let delete_ids: std::collections::HashSet<String> = delta
            .deletes
            .iter()
            .map(|d| d.jellyfin_id.clone())
            .collect();
        let mut force_adds: Vec<crate::sync::SyncAddItem> = Vec::new();
        let mut force_deletes: Vec<crate::sync::SyncDeleteItem> = Vec::new();
        for item in &manifest.synced_items {
            if delete_ids.contains(&item.jellyfin_id) {
                continue;
            }
            force_adds.push(crate::sync::SyncAddItem {
                media_role: item.media_role,
                jellyfin_id: item.jellyfin_id.clone(),
                name: item.name.clone(),
                album: item.album.clone(),
                artist: item.artist.clone(),
                size_bytes: item.size_bytes,
                etag: item.etag.clone(),
                provider_album_id: item.provider_album_id.clone(),
                provider_content_type: item.provider_content_type.clone(),
                provider_suffix: item.provider_suffix.clone(),
                original_bitrate: None,
                track_number: item.track_number,
                reason_code: Some("force-sync".to_string()),
                reason: Some("force sync requested".to_string()),
                server_id: item.server_id.clone(),
                tier: None,
                is_auto_fill: false,
                // Story 13.5 #20: force-sync re-adds an existing managed item — not an auto-fill slot
                // expansion — so it never carries an encoding-from-goals override.
                max_bitrate_override_kbps: None,
            });
            force_deletes.push(crate::sync::SyncDeleteItem {
                jellyfin_id: item.jellyfin_id.clone(),
                local_path: item.local_path.clone(),
                name: item.name.clone(),
                reason_code: Some("force-sync".to_string()),
                reason: Some("force sync requested".to_string()),
            });
        }
        delta.adds.extend(force_adds);
        delta.deletes.extend(force_deletes);
        delta.id_changes.clear();
        delta.unchanged = 0;
    }

    let destructive_cleanup_count = crate::sync::destructive_cleanup_count(&delta, manifest);
    if destructive_cleanup_count > crate::sync::DESTRUCTIVE_CLEANUP_THRESHOLD
        && !destructive_cleanup_confirmed
    {
        return Err(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: format!(
                "Sync would delete {} managed files; explicit confirmation is required for more than {} deletions",
                destructive_cleanup_count,
                crate::sync::DESTRUCTIVE_CLEANUP_THRESHOLD
            ),
            data: Some(serde_json::json!({
                "requiresDestructiveCleanupConfirmation": true,
                "deleteCount": destructive_cleanup_count,
                "threshold": crate::sync::DESTRUCTIVE_CLEANUP_THRESHOLD
            })),
        });
    }

    // Derive basket IDs that need downloading — used for dirty-resume (Story 4.4)
    let pending_item_ids: Vec<String> = delta
        .adds
        .iter()
        .map(|a| a.jellyfin_id.clone())
        .chain(delta.id_changes.iter().map(|c| c.new_jellyfin_id.clone()))
        .collect();

    if state
        .sync_operation_manager
        .get_active_operation_id()
        .await
        .is_some()
    {
        return Err(JsonRpcError {
            code: ERR_SYNC_IN_PROGRESS,
            message: "A sync operation is already in progress".to_string(),
            data: None,
        });
    }

    // Multi-server execute (AC28/AC29): when the delta's adds belong to a server
    // other than the selected one — whether they span multiple servers or sit on a
    // single non-selected server — route each server's items to its own provider.
    // Single-server syncs (no tagged adds, or all adds on the selected server) keep
    // the existing dispatch unchanged (AC21).
    let add_servers: Vec<String> = {
        let mut seen = HashSet::new();
        let mut ordered = Vec::new();
        for add in &delta.adds {
            if let Some(sid) = add.server_id.clone()
                && seen.insert(sid.clone())
            {
                ordered.push(sid);
            }
        }
        ordered
    };
    // Adds carry the portable serverId (Story 2.13); compare/route against the
    // selected server's portable id.
    let selected_for_exec = current_server_portable_id(state)?;
    let needs_provider_routing = match selected_for_exec.as_deref() {
        _ if add_servers.is_empty() => false,
        Some(sel) => add_servers.iter().any(|s| s != sel),
        None => true,
    };
    let selected_provider = if needs_provider_routing {
        None
    } else {
        Some(require_provider(state).await?)
    };

    // Create the operation only after single-server provider resolution succeeds.
    let operation_id = uuid::Uuid::new_v4().to_string();
    let total_files = delta.adds.len() + delta.deletes.len();
    state
        .device_manager
        .admit_sync_operation(
            &state.sync_operation_manager,
            operation_id.clone(),
            total_files,
            &manifest.device_id,
        )
        .await
        .map_err(|_| JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Sync target is no longer connected".into(),
            data: None,
        })?;

    // Mark manifest dirty before sync starts — enables interrupted-sync detection (Story 4.4)
    // Failing to mark dirty MUST abort the sync to prevent undetectable interruptions.
    if let Err(e) = state
        .device_manager
        .update_manifest_for_device(&manifest.device_id, |m| {
            m.dirty = true;
            m.pending_item_ids = pending_item_ids.clone();
        })
        .await
    {
        fail_sync_operation(
            &state.sync_operation_manager,
            &operation_id,
            "sync_execute",
            format!("Failed to mark manifest dirty: {e}"),
        )
        .await;
        return Err(JsonRpcError {
            code: ERR_STORAGE_ERROR,
            message: format!("Failed to mark manifest dirty, aborting sync: {}", e),
            data: None,
        });
    }

    if needs_provider_routing {
        // Resolve every group's provider up front so connection errors surface here.
        let mut group_providers: Vec<(String, Arc<dyn MediaProvider>)> = Vec::new();
        for sid in &add_servers {
            let provider = match get_provider_by_server_id_for(state, sid).await {
                Ok(provider) => provider,
                Err(error) => {
                    fail_sync_operation(
                        &state.sync_operation_manager,
                        &operation_id,
                        "sync_execute",
                        format!("Provider resolution failed for {sid}: {}", error.message),
                    )
                    .await;
                    return Err(error);
                }
            };
            group_providers.push((sid.clone(), provider));
        }

        let op_manager = state.sync_operation_manager.clone();
        let op_id = operation_id.clone();
        let device_manager = state.device_manager.clone();
        let state_tx = state.state_tx.clone();
        let db = state.db.clone();
        let _ = state_tx.send(crate::DaemonState::Syncing);

        tokio::spawn(async move {
            let _pipeline_guard = pipeline_guard;
            let sync_manifest = &target.manifest;
            let transcoding_profile = match load_selected_transcoding_profile(
                sync_manifest.transcoding_profile_id.as_deref(),
            ) {
                Ok(profile) => profile,
                Err(e) => {
                    fail_sync_operation(
                        &op_manager,
                        &op_id,
                        "sync_execute",
                        format!("Failed to load transcoding profile: {}", e),
                    )
                    .await;
                    let _ = state_tx.send(crate::DaemonState::Error);
                    return;
                }
            };

            let (default_provider, providers_by_server) = {
                let mut groups = group_providers.into_iter();
                let (default_server, default_provider) = groups
                    .next()
                    .expect("provider routing requires at least one resolved server");
                let mut providers = std::collections::HashMap::new();
                providers.insert(default_server, Arc::clone(&default_provider));
                for (server_id, provider) in groups {
                    providers.insert(server_id, provider);
                }
                (default_provider, providers)
            };
            let result = crate::sync::execute_provider_sync(
                &delta,
                &target,
                crate::sync::ProviderSyncSource {
                    provider: default_provider,
                    transcoding_profile,
                    providers_by_server,
                },
                op_manager.clone(),
                op_id.clone(),
                device_manager.clone(),
            )
            .await;
            let all_errors = match result {
                Ok((_synced, errors)) => errors,
                Err(e) => vec![crate::sync::SyncFileError {
                    jellyfin_id: String::new(),
                    filename: "sync_execute[multi-server]".to_string(),
                    error_message: e.to_string(),
                }],
            };

            let (outcome, final_errors) = op_manager
                .finalize_operation(&op_id, all_errors, || async {
                    device_manager
                        .update_manifest_for_device(&sync_manifest.device_id, |m| {
                            m.dirty = false;
                            m.pending_item_ids.clear();
                            m.last_synced_transcoding_profile_id = m.transcoding_profile_id.clone();
                            m.transcoding_profile_dirty = false;
                        })
                        .await
                })
                .await;
            if outcome == crate::sync::SyncStatus::Complete {
                record_autofill_history_after_sync(
                    &db,
                    sync_manifest,
                    &delta,
                    &final_errors,
                    now_unix_secs(),
                );
                drop(tokio::task::spawn_blocking(send_sync_complete_notification));
            }
            let _ = state_tx.send(crate::DaemonState::Idle);
        });

        return Ok(serde_json::json!({ "operationId": operation_id }));
    }

    let provider = selected_provider.expect("single-server provider resolved before operation");
    {
        let op_manager = state.sync_operation_manager.clone();
        let op_id = operation_id.clone();
        let device_manager = state.device_manager.clone();
        let state_tx = state.state_tx.clone();
        let db = state.db.clone();
        let _ = state_tx.send(crate::DaemonState::Syncing);

        tokio::spawn(async move {
            let _pipeline_guard = pipeline_guard;
            let sync_manifest = &target.manifest;
            let transcoding_profile = match load_selected_transcoding_profile(
                sync_manifest.transcoding_profile_id.as_deref(),
            ) {
                Ok(profile) => profile,
                Err(e) => {
                    eprintln!("[Sync] Failed to load transcoding profile: {}", e);
                    fail_sync_operation(
                        &op_manager,
                        &op_id,
                        "sync_execute",
                        format!("Failed to load transcoding profile: {}", e),
                    )
                    .await;
                    let _ = state_tx.send(crate::DaemonState::Error);
                    return;
                }
            };

            let result = crate::sync::execute_provider_sync(
                &delta,
                &target,
                crate::sync::ProviderSyncSource {
                    provider,
                    transcoding_profile,
                    providers_by_server: std::collections::HashMap::new(),
                },
                op_manager.clone(),
                op_id.clone(),
                device_manager.clone(),
            )
            .await;

            match result {
                Ok((_synced_items, errors)) => {
                    let (outcome, final_errors) = op_manager
                        .finalize_operation(&op_id, errors, || async {
                            device_manager
                                .update_manifest_for_device(&sync_manifest.device_id, |m| {
                                    m.dirty = false;
                                    m.pending_item_ids.clear();
                                    m.last_synced_transcoding_profile_id =
                                        m.transcoding_profile_id.clone();
                                    m.transcoding_profile_dirty = false;
                                })
                                .await
                        })
                        .await;
                    if outcome == crate::sync::SyncStatus::Complete {
                        record_autofill_history_after_sync(
                            &db,
                            sync_manifest,
                            &delta,
                            &final_errors,
                            now_unix_secs(),
                        );
                        drop(tokio::task::spawn_blocking(send_sync_complete_notification));
                    }
                    let _ = state_tx.send(crate::DaemonState::Idle);
                }
                Err(e) => {
                    if let Some(mut operation) = op_manager.get_operation(&op_id).await {
                        operation.status = crate::sync::SyncStatus::Failed;
                        operation.errors.push(crate::sync::SyncFileError {
                            jellyfin_id: String::new(),
                            filename: String::from("sync_execute"),
                            error_message: e.to_string(),
                        });
                        op_manager.update_operation(&op_id, operation).await;
                    }
                    let _ = state_tx.send(crate::DaemonState::Error);
                }
            }
        });

        return Ok(serde_json::json!({
            "operationId": operation_id
        }));
    }
}

async fn handle_sync_get_operation_status(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let params = params.ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing params".to_string(),
        data: None,
    })?;

    let operation_id = params["operationId"].as_str().ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing operationId".to_string(),
        data: None,
    })?;

    match state
        .sync_operation_manager
        .get_operation(operation_id)
        .await
    {
        Some(operation) => Ok(serde_json::to_value(operation).unwrap()),
        None => Err(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Operation not found".to_string(),
            data: None,
        }),
    }
}

async fn handle_sync_cancel(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let params = params.ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing params".to_string(),
        data: None,
    })?;

    let operation_id = params["operationId"].as_str();
    let found = if let Some(operation_id) = operation_id {
        state
            .sync_operation_manager
            .request_cancel(operation_id)
            .await
    } else {
        state.sync_operation_manager.request_pipeline_cancel()
    };

    if found {
        Ok(serde_json::json!({ "cancelled": true }))
    } else {
        Err(JsonRpcError {
            code: ERR_NOT_FOUND,
            message: operation_id
                .map(|id| format!("No operation with id '{}'", id))
                .unwrap_or_else(|| "No active sync pipeline".to_string()),
            data: None,
        })
    }
}

async fn handle_sync_get_resume_state(state: &AppState) -> Result<Value, JsonRpcError> {
    let device = state.device_manager.get_current_device().await;
    let device_path = state.device_manager.get_current_device_path().await;

    match (device, device_path) {
        (Some(manifest), Some(_path)) => {
            let is_dirty = manifest.dirty;
            let pending_ids = manifest.pending_item_ids.clone();

            let cleaned_tmp_files = if is_dirty {
                if let Some(device_io) = state.device_manager.get_device_io().await {
                    // Delete any leftover .dirty markers (from interrupted MTP write_with_verify)
                    if let Ok(files) = device_io.list_files("").await {
                        for f in files.iter().filter(|f| f.name.ends_with(".dirty")) {
                            let _ = device_io.delete_file(&f.path).await;
                        }
                    }
                    crate::device::cleanup_tmp_files(
                        device_io,
                        &manifest
                            .media_paths()
                            .into_iter()
                            .map(str::to_string)
                            .collect::<Vec<_>>(),
                    )
                    .await
                    .unwrap_or(0)
                } else {
                    0
                }
            } else {
                0
            };

            Ok(serde_json::json!({
                "isDirty": is_dirty,
                "pendingItemIds": pending_ids,
                "cleanedTmpFiles": cleaned_tmp_files,
            }))
        }
        _ => Ok(serde_json::json!({
            "isDirty": false,
            "pendingItemIds": [],
            "cleanedTmpFiles": 0,
        })),
    }
}

async fn handle_proxy_image(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Some(provider) = active_non_jellyfin_provider(&state).await {
        let is_audiobookshelf = provider.server_type() == ServerType::Audiobookshelf;
        let response = match provider.fetch_cover_art(&id).await {
            Ok(response) => response,
            Err(ProviderError::RateLimited {
                retry_after_seconds,
            }) => {
                let mut response = http::StatusCode::TOO_MANY_REQUESTS.into_response();
                if let Some(seconds) = retry_after_seconds {
                    if let Ok(value) = http::HeaderValue::from_str(&seconds.to_string()) {
                        response
                            .headers_mut()
                            .insert(http::header::RETRY_AFTER, value);
                    }
                }
                return response;
            }
            Err(error) => {
                return match error {
                    ProviderError::NotFound { .. } | ProviderError::StaleConfiguration(_) => {
                        http::StatusCode::NOT_FOUND
                    }
                    ProviderError::Forbidden => http::StatusCode::FORBIDDEN,
                    ProviderError::Auth(_) => http::StatusCode::UNAUTHORIZED,
                    _ => http::StatusCode::BAD_GATEWAY,
                }
                .into_response();
            }
        };
        if is_audiobookshelf {
            return proxy_bounded_image_response(response, 16 * 1024 * 1024).await;
        }
        return proxy_http_image_response(response).await;
    }

    let (url, token, _) = match CredentialManager::get_credentials() {
        Ok(creds) => creds,
        Err(_) => return http::StatusCode::UNAUTHORIZED.into_response(),
    };

    match state.jellyfin_client.get_image(&url, &token, &id).await {
        Ok(resp) => proxy_http_image_response(resp).await,
        Err(_) => http::StatusCode::NOT_FOUND.into_response(),
    }
}

async fn proxy_http_image_response(resp: reqwest::Response) -> axum::response::Response {
    let status = http::StatusCode::from_u16(resp.status().as_u16())
        .unwrap_or(http::StatusCode::INTERNAL_SERVER_ERROR);
    let mut builder = axum::response::Response::builder().status(status);

    if let Some(ct) = resp.headers().get(reqwest::header::CONTENT_TYPE) {
        builder = builder.header(http::header::CONTENT_TYPE, ct);
    }

    match resp.bytes().await {
        Ok(bytes) => builder
            .body(axum::body::Body::from(bytes))
            .unwrap_or(http::StatusCode::INTERNAL_SERVER_ERROR.into_response()),
        Err(_) => http::StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn proxy_bounded_image_response(
    mut resp: reqwest::Response,
    max_bytes: usize,
) -> axum::response::Response {
    let mut body = Vec::new();
    loop {
        match resp.chunk().await {
            Ok(Some(chunk)) => {
                if !append_bounded_image_chunk(&mut body, &chunk, max_bytes) {
                    return http::StatusCode::PAYLOAD_TOO_LARGE.into_response();
                }
            }
            Ok(None) => break,
            Err(_) => return http::StatusCode::BAD_GATEWAY.into_response(),
        }
    }
    let content_type = resp.headers().get(reqwest::header::CONTENT_TYPE).cloned();
    let mut builder = axum::response::Response::builder().status(http::StatusCode::OK);
    if let Some(content_type) = content_type {
        builder = builder.header(http::header::CONTENT_TYPE, content_type);
    }
    builder
        .body(axum::body::Body::from(body))
        .unwrap_or(http::StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

fn append_bounded_image_chunk(body: &mut Vec<u8>, chunk: &[u8], max_bytes: usize) -> bool {
    if body
        .len()
        .checked_add(chunk.len())
        .is_none_or(|len| len > max_bytes)
    {
        return false;
    }
    body.extend_from_slice(chunk);
    true
}

async fn handle_scrobbler_get_last_result(state: &AppState) -> Result<Value, JsonRpcError> {
    let result = state.last_scrobbler_result.read().await;
    match result.as_ref() {
        Some(r) => Ok(serde_json::to_value(r).unwrap()),
        None => Ok(serde_json::json!({
            "status": "none",
            "message": "No scrobble submission has been performed yet."
        })),
    }
}

async fn broadcast_device_state(state: &AppState) {
    if let Some(manifest) = state.device_manager.get_current_device().await {
        let name = manifest
            .name
            .clone()
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| manifest.device_id.clone());
        let mapping = state
            .db
            .get_device_mapping(&manifest.device_id)
            .unwrap_or(None);
        let daemon_state = if let Some(m) = mapping {
            if let Some(profile_id) = m.jellyfin_user_id {
                crate::DaemonState::DeviceRecognized { name, profile_id }
            } else {
                crate::DaemonState::DeviceFound(name)
            }
        } else {
            crate::DaemonState::DeviceFound(name)
        };
        let _ = state.state_tx.send(daemon_state);
    }
}

async fn handle_manifest_get_discrepancies(state: &AppState) -> Result<Value, JsonRpcError> {
    match state.device_manager.get_discrepancies().await {
        Ok(Some(discrepancies)) => Ok(serde_json::to_value(discrepancies).unwrap()),
        Ok(None) => Err(JsonRpcError {
            code: ERR_CONNECTION_FAILED,
            message: "No device connected".to_string(),
            data: None,
        }),
        Err(e) => Err(JsonRpcError {
            code: ERR_STORAGE_ERROR,
            message: format!("Failed to scan device: {}", e),
            data: None,
        }),
    }
}

async fn handle_manifest_prune(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let params = params.ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing params".to_string(),
        data: None,
    })?;

    let item_ids = params["itemIds"]
        .as_array()
        .ok_or(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Missing or invalid itemIds array".to_string(),
            data: None,
        })?
        .iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect::<Vec<String>>();

    match state.device_manager.prune_items(&item_ids).await {
        Ok(removed) => {
            broadcast_device_state(state).await;
            Ok(serde_json::json!({ "removed": removed }))
        }
        Err(e) => Err(JsonRpcError {
            code: ERR_STORAGE_ERROR,
            message: format!("Failed to prune items: {}", e),
            data: None,
        }),
    }
}

async fn handle_manifest_relink(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let params = params.ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing params".to_string(),
        data: None,
    })?;

    let jellyfin_id = params["jellyfinId"].as_str().ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing jellyfinId".to_string(),
        data: None,
    })?;

    let new_local_path = params["newLocalPath"].as_str().ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing newLocalPath".to_string(),
        data: None,
    })?;

    match state
        .device_manager
        .relink_item(jellyfin_id, new_local_path)
        .await
    {
        Ok(found) => {
            broadcast_device_state(state).await;
            Ok(serde_json::json!({ "success": found }))
        }
        Err(e) => Err(JsonRpcError {
            code: ERR_STORAGE_ERROR,
            message: format!("Failed to relink item: {}", e),
            data: None,
        }),
    }
}

async fn handle_manifest_clear_dirty(state: &AppState) -> Result<Value, JsonRpcError> {
    match state.device_manager.clear_dirty_flag().await {
        Ok(()) => {
            broadcast_device_state(state).await;
            Ok(serde_json::json!({ "success": true }))
        }
        Err(e) => Err(JsonRpcError {
            code: ERR_STORAGE_ERROR,
            message: format!("Failed to clear dirty flag: {}", e),
            data: None,
        }),
    }
}

const VALID_DEVICE_ICONS: &[&str] = &[
    "usb-drive",
    "phone-fill",
    "watch",
    "sd-card",
    "headphones",
    "music-note-list",
];

#[derive(Debug, Clone, PartialEq)]
struct ManifestUpdateOutcome {
    relocation_required: bool,
    tracks_to_remove: usize,
    playlists_to_remove: usize,
    bytes_to_remove: u64,
}

fn normalize_editable_folder_path(value: &str) -> Result<String, JsonRpcError> {
    let trimmed = value.trim().replace('\\', "/");
    if trimmed.is_empty() {
        return Err(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Folder path cannot be empty".to_string(),
            data: None,
        });
    }
    if trimmed == "." || trimmed == "/" || trimmed.starts_with('/') {
        return Err(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Folder path must be device-relative and below a folder".to_string(),
            data: None,
        });
    }
    if std::path::Path::new(&trimmed).is_absolute()
        || trimmed
            .split('/')
            .next()
            .is_some_and(|component| component.ends_with(':'))
    {
        return Err(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Folder path must not be absolute".to_string(),
            data: None,
        });
    }
    if trimmed.split('/').any(|component| {
        component.is_empty() || component == "." || component == ".." || component.contains(':')
    }) {
        return Err(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message:
                "Folder path must not contain empty, current, parent, or drive-prefix components"
                    .to_string(),
            data: None,
        });
    }
    Ok(trimmed)
}

fn string_param<'a>(params: &'a Value, key: &str) -> Result<Option<&'a str>, JsonRpcError> {
    match params.get(key) {
        None => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.as_str())),
        Some(Value::Null) => Ok(None),
        Some(_) => Err(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: format!("{} must be a string or null", key),
            data: None,
        }),
    }
}

fn validate_device_name_and_icon(
    name: Option<&str>,
    icon: Option<&str>,
) -> Result<(), JsonRpcError> {
    if let Some(name) = name {
        if name.trim().is_empty() {
            return Err(JsonRpcError {
                code: ERR_INVALID_PARAMS,
                message: "Name cannot be empty".to_string(),
                data: None,
            });
        }
        if name.chars().count() > 40 {
            return Err(JsonRpcError {
                code: ERR_INVALID_PARAMS,
                message: "Device name exceeds 40 characters".to_string(),
                data: None,
            });
        }
    }
    if let Some(icon) = icon.filter(|icon| !icon.is_empty())
        && !VALID_DEVICE_ICONS.contains(&icon)
    {
        return Err(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: format!("Invalid icon '{}'", icon),
            data: None,
        });
    }
    Ok(())
}

fn validate_transcoding_profile_id(
    profile_id: Option<&str>,
) -> Result<Option<String>, JsonRpcError> {
    match profile_id {
        None | Some("passthrough") => Ok(None),
        Some(id) => {
            let path = crate::paths::get_device_profiles_path().map_err(|e| JsonRpcError {
                code: ERR_STORAGE_ERROR,
                message: e.to_string(),
                data: None,
            })?;
            let profiles = crate::transcoding::load_profiles(&path).map_err(|e| JsonRpcError {
                code: ERR_STORAGE_ERROR,
                message: e.to_string(),
                data: None,
            })?;
            if profiles.iter().any(|p| p.id == id) {
                Ok(Some(id.to_string()))
            } else {
                Err(JsonRpcError {
                    code: ERR_INVALID_PARAMS,
                    message: format!("Profile '{}' not found in device-profiles.json", id),
                    data: None,
                })
            }
        }
    }
}

fn path_in_or_equal(path: &str, folder: &str) -> bool {
    let path = path.replace('\\', "/");
    let folder = folder.trim_matches('/').replace('\\', "/");
    if folder.is_empty() {
        return true;
    }
    path == folder || path.starts_with(&format!("{folder}/"))
}

fn playlist_filename_with_folder(folder: &str, filename: &str) -> String {
    let folder = folder.replace('\\', "/").trim_matches('/').to_string();
    let filename = filename.replace('\\', "/").trim_matches('/').to_string();
    if folder.is_empty() || filename.contains('/') {
        filename
    } else {
        format!("{folder}/{filename}")
    }
}

fn remove_folder_id_cache_for_path_change(
    folder_ids: &mut HashMap<String, u32>,
    old_path: Option<&str>,
    new_path: Option<&str>,
) {
    let affected: Vec<String> = folder_ids
        .keys()
        .filter(|path| {
            old_path.is_some_and(|old| path_in_or_equal(path, old))
                || new_path.is_some_and(|new| path_in_or_equal(path, new))
        })
        .cloned()
        .collect();
    for path in affected {
        folder_ids.remove(&path);
    }
}

fn apply_manifest_settings_update(
    manifest: &mut crate::device::DeviceManifest,
    name: Option<String>,
    icon: Option<Option<String>>,
    transcoding_profile_id: Option<Option<String>>,
    music_folder_path: Option<String>,
    audiobook_folder_path: Option<Option<String>>,
    podcast_folder_path: Option<Option<String>>,
    playlist_folder_path: Option<Option<String>>,
) -> ManifestUpdateOutcome {
    let old_music = manifest.managed_paths.first().cloned();
    let old_audiobook = manifest
        .media_path(crate::device::MediaRole::Audiobook)
        .map(str::to_string);
    let old_podcast = manifest
        .media_path(crate::device::MediaRole::Podcast)
        .map(str::to_string);
    let old_playlist = manifest.resolved_playlist_path().map(str::to_string);

    if let Some(name) = name {
        manifest.name = Some(name.trim().to_string()).filter(|name| !name.is_empty());
    }
    if let Some(icon) = icon {
        manifest.icon = icon.filter(|icon| !icon.is_empty());
    }
    if let Some(transcoding_profile_id) = transcoding_profile_id {
        if manifest.transcoding_profile_id != transcoding_profile_id {
            manifest.transcoding_profile_dirty = match &manifest.last_synced_transcoding_profile_id
            {
                Some(last_synced) => {
                    transcoding_profile_id.as_deref() != Some(last_synced.as_str())
                }
                None => !manifest.synced_items.is_empty(),
            };
        }
        manifest.transcoding_profile_id = transcoding_profile_id;
    }
    if let Some(music_folder_path) = music_folder_path {
        if manifest.managed_paths.is_empty() {
            manifest.managed_paths.push(music_folder_path);
        } else {
            manifest.managed_paths[0] = music_folder_path;
        }
    }
    if let Some(playlist_folder_path) = playlist_folder_path {
        manifest.playlist_path = playlist_folder_path.filter(|path| !path.trim().is_empty());
    }
    if let Some(path) = audiobook_folder_path {
        manifest.audiobook_path = path;
    }
    if let Some(path) = podcast_folder_path {
        manifest.podcast_path = path;
    }

    let new_music = manifest.managed_paths.first().cloned();
    let new_audiobook = manifest
        .media_path(crate::device::MediaRole::Audiobook)
        .map(str::to_string);
    let new_podcast = manifest
        .media_path(crate::device::MediaRole::Podcast)
        .map(str::to_string);
    let new_playlist = manifest.resolved_playlist_path().map(str::to_string);
    let music_changed = old_music != new_music;
    let playlist_changed = old_playlist != new_playlist;

    if music_changed
        || playlist_changed
        || old_audiobook != new_audiobook
        || old_podcast != new_podcast
    {
        remove_folder_id_cache_for_path_change(
            &mut manifest.folder_ids,
            old_music.as_deref(),
            new_music.as_deref(),
        );
        remove_folder_id_cache_for_path_change(
            &mut manifest.folder_ids,
            old_playlist.as_deref(),
            new_playlist.as_deref(),
        );
        remove_folder_id_cache_for_path_change(
            &mut manifest.folder_ids,
            old_audiobook.as_deref(),
            new_audiobook.as_deref(),
        );
        remove_folder_id_cache_for_path_change(
            &mut manifest.folder_ids,
            old_podcast.as_deref(),
            new_podcast.as_deref(),
        );
        if playlist_changed && let Some(old_playlist) = old_playlist.as_deref() {
            for entry in &mut manifest.playlists {
                entry.filename = playlist_filename_with_folder(old_playlist, &entry.filename);
            }
        }
    }

    let moved_items: Vec<_> = manifest
        .synced_items
        .iter()
        .filter(|item| {
            manifest
                .media_path(item.media_role)
                .is_some_and(|folder| !path_in_or_equal(&item.local_path, folder))
        })
        .collect();
    let tracks_to_remove = moved_items.len();
    let bytes_to_remove = moved_items.iter().map(|item| item.size_bytes).sum();
    let playlists_to_remove = if playlist_changed {
        manifest.playlists.len()
    } else {
        0
    };

    ManifestUpdateOutcome {
        relocation_required: music_changed
            || playlist_changed
            || old_audiobook != new_audiobook
            || old_podcast != new_podcast,
        tracks_to_remove,
        playlists_to_remove,
        bytes_to_remove,
    }
}

async fn handle_device_update_manifest(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let params = params.ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing params".to_string(),
        data: None,
    })?;

    let device_id = params["deviceId"].as_str().ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing deviceId".to_string(),
        data: None,
    })?;
    let current = state
        .device_manager
        .get_current_device()
        .await
        .ok_or(JsonRpcError {
            code: ERR_NOT_FOUND,
            message: "No selected device".to_string(),
            data: None,
        })?;
    if current.device_id != device_id {
        return Err(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Selected device does not match deviceId".to_string(),
            data: None,
        });
    }

    let name = string_param(&params, "name")?;
    let icon = match params.get("icon") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(Value::String(value)) => Some(Some(value.clone())),
        Some(_) => {
            return Err(JsonRpcError {
                code: ERR_INVALID_PARAMS,
                message: "icon must be a string or null".to_string(),
                data: None,
            });
        }
    };
    let icon_for_validation = icon.as_ref().and_then(|icon| icon.as_deref());
    validate_device_name_and_icon(name, icon_for_validation)?;

    let music_folder_path = string_param(&params, "musicFolderPath")?
        .map(normalize_editable_folder_path)
        .transpose()?;
    let optional_media_folder = |key| -> Result<Option<Option<String>>, JsonRpcError> {
        if params.get(key).is_none() {
            return Ok(None);
        }
        let path = string_param(&params, key)?.unwrap_or("").trim();
        Ok(Some(if path.is_empty() {
            None
        } else {
            Some(normalize_editable_folder_path(path)?)
        }))
    };
    let audiobook_folder_path = optional_media_folder("audiobookFolderPath")?;
    let podcast_folder_path = optional_media_folder("podcastFolderPath")?;
    if let Some(device_io) = state.device_manager.get_device_io().await {
        for path in [
            audiobook_folder_path.as_ref().and_then(|p| p.as_deref()),
            podcast_folder_path.as_ref().and_then(|p| p.as_deref()),
        ]
        .into_iter()
        .flatten()
        {
            device_io.ensure_dir(path).await.map_err(|e| JsonRpcError {
                code: ERR_STORAGE_ERROR,
                message: format!("Failed to create media folder: {e}"),
                data: None,
            })?;
        }
    }
    let playlist_folder_path = if params.get("playlistFolderPath").is_some() {
        let raw = string_param(&params, "playlistFolderPath")?
            .unwrap_or("")
            .trim();
        Some(if raw.is_empty() {
            None
        } else {
            Some(normalize_editable_folder_path(raw)?)
        })
    } else {
        None
    };
    let name_update = name.map(str::to_string);
    let icon_update = icon;
    let transcoding_profile_update = if params.get("transcodingProfileId").is_some() {
        Some(validate_transcoding_profile_id(string_param(
            &params,
            "transcodingProfileId",
        )?)?)
    } else {
        None
    };
    let transcoding_profile_for_db = transcoding_profile_update.clone();
    let mut outcome = ManifestUpdateOutcome {
        relocation_required: false,
        tracks_to_remove: 0,
        playlists_to_remove: 0,
        bytes_to_remove: 0,
    };

    if let Some(profile_update) = transcoding_profile_for_db {
        state
            .db
            .set_transcoding_profile(device_id, profile_update.as_deref())
            .map_err(|e| JsonRpcError {
                code: ERR_STORAGE_ERROR,
                message: format!("Failed to store transcoding profile: {}", e),
                data: None,
            })?;
    }

    state
        .device_manager
        .update_manifest(|manifest| {
            outcome = apply_manifest_settings_update(
                manifest,
                name_update,
                icon_update,
                transcoding_profile_update,
                music_folder_path,
                audiobook_folder_path,
                podcast_folder_path,
                playlist_folder_path,
            );
        })
        .await
        .map_err(|e| {
            if params.get("transcodingProfileId").is_some() {
                let _ = state
                    .db
                    .set_transcoding_profile(device_id, current.transcoding_profile_id.as_deref());
            }
            JsonRpcError {
                code: ERR_STORAGE_ERROR,
                message: format!("Failed to update device manifest: {}", e),
                data: None,
            }
        })?;

    broadcast_device_state(state).await;
    Ok(serde_json::json!({
        "ok": true,
        "relocationRequired": outcome.relocation_required,
        "cleanupPreview": {
            "tracksToRemove": outcome.tracks_to_remove,
            "playlistsToRemove": outcome.playlists_to_remove,
            "bytesToRemove": outcome.bytes_to_remove,
        }
    }))
}

async fn handle_device_initialize(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let params = params.ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing params".to_string(),
        data: None,
    })?;
    let allowed = [
        "pendingId",
        "observedDestinationRevision",
        "folderPath",
        "playlistFolderPath",
        "audiobookFolderPath",
        "podcastFolderPath",
        "profileId",
        "transcodingProfileId",
        "name",
        "icon",
    ];
    if params
        .as_object()
        .is_none_or(|object| object.keys().any(|key| !allowed.contains(&key.as_str())))
    {
        return Err(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Invalid device_initialize parameters".to_string(),
            data: None,
        });
    }

    let pending_id = params["pendingId"].as_str().ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing pendingId".to_string(),
        data: None,
    })?;
    let observed_destination_revision = params["observedDestinationRevision"]
        .as_str()
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Missing or invalid observedDestinationRevision".to_string(),
            data: None,
        })?;

    let folder_path = params["folderPath"].as_str().ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing folderPath".to_string(),
        data: None,
    })?;
    let playlist_folder_path = params
        .get("playlistFolderPath")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(normalize_editable_folder_path)
        .transpose()?;
    let audiobook_folder_path = string_param(&params, "audiobookFolderPath")?
        .filter(|path| !path.trim().is_empty())
        .map(normalize_editable_folder_path)
        .transpose()?;
    let podcast_folder_path = string_param(&params, "podcastFolderPath")?
        .filter(|path| !path.trim().is_empty())
        .map(normalize_editable_folder_path)
        .transpose()?;

    let profile_id = params["profileId"].as_str().ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing profileId".to_string(),
        data: None,
    })?;

    // Optional — if not provided, device uses passthrough (no transcoding)
    let transcoding_profile_id = params["transcodingProfileId"]
        .as_str()
        .map(|s| s.to_string());

    let device_name = params["name"]
        .as_str()
        .ok_or(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Missing name".to_string(),
            data: None,
        })?
        .to_string();
    let device_icon = params["icon"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    validate_device_name_and_icon(Some(&device_name), device_icon.as_deref())?;

    // Validate the transcoding profile ID exists in device-profiles.json (if provided)
    if let Some(ref tpid) = transcoding_profile_id {
        let profiles_path = crate::paths::get_device_profiles_path().map_err(|e| JsonRpcError {
            code: ERR_STORAGE_ERROR,
            message: e.to_string(),
            data: None,
        })?;
        let profiles =
            crate::transcoding::load_profiles(&profiles_path).map_err(|e| JsonRpcError {
                code: ERR_STORAGE_ERROR,
                message: format!("Failed to load device profiles: {}", e),
                data: None,
            })?;
        if !profiles.iter().any(|p| p.id == *tpid) {
            return Err(JsonRpcError {
                code: ERR_INVALID_PARAMS,
                message: format!(
                    "Transcoding profile '{}' not found in device-profiles.json",
                    tpid
                ),
                data: None,
            });
        }
    }

    let mut manifest = state
        .device_manager
        .initialize_pending_device(
            pending_id,
            observed_destination_revision,
            folder_path,
            playlist_folder_path.as_deref(),
            transcoding_profile_id.clone(),
            device_name,
            device_icon,
        )
        .await
        .map_err(|e| {
            let message = e.to_string();
            let target_error = message.contains("Pending destination")
                || message.contains("No unrecognized device");
            JsonRpcError {
                code: if target_error {
                    ERR_INVALID_PARAMS
                } else {
                    ERR_STORAGE_ERROR
                },
                message: format!("Failed to initialize device: {}", message),
                data: None,
            }
        })?;

    if audiobook_folder_path.is_some() || podcast_folder_path.is_some() {
        if let Some(device_io) = state.device_manager.get_device_io().await {
            for path in [
                audiobook_folder_path.as_deref(),
                podcast_folder_path.as_deref(),
            ]
            .into_iter()
            .flatten()
            {
                device_io.ensure_dir(path).await.map_err(|e| JsonRpcError {
                    code: ERR_STORAGE_ERROR,
                    message: format!("Failed to create media folder: {e}"),
                    data: None,
                })?;
            }
        }
        let book_path = audiobook_folder_path.clone();
        let podcast_path = podcast_folder_path.clone();
        state
            .device_manager
            .update_manifest(|current| {
                current.audiobook_path = book_path;
                current.podcast_path = podcast_path;
            })
            .await
            .map_err(|e| JsonRpcError {
                code: ERR_STORAGE_ERROR,
                message: e.to_string(),
                data: None,
            })?;
        manifest.audiobook_path = audiobook_folder_path;
        manifest.podcast_path = podcast_folder_path;
    }

    state
        .db
        .upsert_device_mapping(&manifest.device_id, None, Some(profile_id), None)
        .map_err(|e| JsonRpcError {
            code: ERR_STORAGE_ERROR,
            message: format!("Failed to store device mapping: {}", e),
            data: None,
        })?;

    if let Some(ref tpid) = transcoding_profile_id {
        state
            .db
            .set_transcoding_profile(&manifest.device_id, Some(tpid))
            .map_err(|e| JsonRpcError {
                code: ERR_STORAGE_ERROR,
                message: format!("Failed to store transcoding profile: {}", e),
                data: None,
            })?;
    }

    // Derive a human-readable name from the device path rather than using the UUID
    let device_name = state
        .device_manager
        .get_current_device_path()
        .await
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| manifest.device_id.clone());

    let _ = state.state_tx.send(crate::DaemonState::DeviceRecognized {
        name: device_name,
        profile_id: profile_id.to_string(),
    });

    Ok(serde_json::json!({
        "status": "success",
        "data": {
            "deviceId": manifest.device_id,
            "version": manifest.version,
            "managedPaths": manifest.managed_paths,
            "audiobookPath": manifest.audiobook_path,
            "podcastPath": manifest.podcast_path,
            "playlistPath": manifest.playlist_path,
            "transcodingProfileId": manifest.transcoding_profile_id,
        }
    }))
}

async fn handle_device_set_auto_sync_on_connect(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let params = params.ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing params".to_string(),
        data: None,
    })?;

    let device_id = params["deviceId"].as_str().ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing deviceId".to_string(),
        data: None,
    })?;

    let enabled = params["enabled"].as_bool().ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing or invalid enabled (boolean)".to_string(),
        data: None,
    })?;

    // Update SQLite device profile
    state
        .db
        .set_auto_sync_on_connect(device_id, enabled)
        .map_err(|e| JsonRpcError {
            code: ERR_STORAGE_ERROR,
            message: format!("Failed to update auto_sync_on_connect in DB: {}", e),
            data: None,
        })?;

    // Update device manifest on disk (if this device is currently connected)
    let current_device = state.device_manager.get_current_device().await;
    if let Some(ref d) = current_device
        && d.device_id == device_id
    {
        state
            .device_manager
            .update_manifest(|m| {
                m.auto_sync_on_connect = enabled;
            })
            .await
            .map_err(|e| JsonRpcError {
                code: ERR_STORAGE_ERROR,
                message: format!("Failed to update manifest: {}", e),
                data: None,
            })?;
    }

    Ok(serde_json::json!({
        "status": "success",
        "autoSyncOnConnect": enabled,
    }))
}

/// basket.autoFill — runs the priority ranking algorithm and returns ranked items.
///
/// Params: `{ deviceId?: string, maxBytes?: number, excludeItemIds?: string[], serverId?: string,
/// pipeline?: AutoFillPipeline }`.
///
/// Story 12.6: when `serverId` (a portable id) is supplied, the preview is computed for that
/// server's provider (resolved via `get_provider_by_server_id_for`) through the shared sync-time
/// seam [`expand_auto_fill_slot`] — using the supplied inline `pipeline` if given, else the
/// persisted `pipelines[serverId]`, else the default-legacy pipeline. When `serverId` is absent the
/// behavior is unchanged: the legacy Jellyfin `run_auto_fill` path (with container-id exclude
/// expansion). An unknown/unroutable `serverId` returns `ERR_CONNECTION_FAILED`, never a panic.
async fn handle_basket_auto_fill(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let params = params.unwrap_or(serde_json::json!({}));

    let max_bytes_param = params["maxBytes"].as_u64();

    let exclude_item_ids: Vec<String> = params["excludeItemIds"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    // Story 12.6: per-server routing. A non-blank serverId routes the preview through the
    // resolved provider + shared expansion seam, so non-Jellyfin servers and configured pipelines
    // are previewed exactly as they'll fill at sync time.
    let server_id = params
        .get("serverId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    if let Some(server_id) = server_id {
        // Pipeline precedence: inline (supplied by the UI for a live, unsaved preview) →
        // persisted per-server pipeline → default-legacy. The default-legacy pipeline keeps the
        // fast `run_auto_fill_provider` path inside the seam (a bare budget needs no materialization).
        let inline_pipeline: Option<crate::auto_fill::AutoFillPipeline> =
            match params.get("pipeline") {
                Some(v) if !v.is_null() => {
                    Some(serde_json::from_value(v.clone()).map_err(|e| JsonRpcError {
                        code: ERR_INVALID_PARAMS,
                        message: format!("Invalid pipeline: {}", e),
                        data: None,
                    })?)
                }
                _ => None,
            };
        let persisted_pipeline = state
            .device_manager
            .get_current_device()
            .await
            .and_then(|m| m.auto_fill.pipeline_for(&server_id).cloned());
        let pipeline = inline_pipeline
            .or(persisted_pipeline)
            .unwrap_or_else(|| crate::auto_fill::AutoFillPipeline::default_legacy(None));
        let max_fill_bytes = if let Some(mb) = max_bytes_param.or(pipeline.budget.max_bytes) {
            mb
        } else {
            match state.device_manager.get_device_storage().await {
                Some(info) => info.free_bytes,
                None => {
                    return Err(JsonRpcError {
                        code: ERR_INVALID_PARAMS,
                        message: "No device connected and no maxBytes specified".to_string(),
                        data: None,
                    });
                }
            }
        };

        let provider = match get_provider_by_server_id_for(state, &server_id).await {
            Ok(p) => p,
            Err(e) => {
                return Err(JsonRpcError {
                    code: ERR_CONNECTION_FAILED,
                    message: format!(
                        "Auto-fill failed: provider for {server_id} unavailable: {}",
                        e.message
                    ),
                    data: None,
                });
            }
        };

        // Exclude ids are passed through verbatim (the per-provider seam dedups by id, mirroring
        // the sync-time path). Container-id expansion is a Jellyfin-only concern of the legacy path.
        // Story 13.1: build the same DB-sourced history + rotation cursor as sync-time so the
        // preview reflects cooldown/played/stable-core/tiers exactly as the fill will.
        let now = now_unix_secs();
        let device_id = state
            .device_manager
            .get_current_device()
            .await
            .map(|m| m.device_id)
            .unwrap_or_default();
        let (history, rotation_cursor, pity_streak) =
            build_autofill_history(&state.db, &device_id, &server_id, now);
        let fill_params = crate::auto_fill::AutoFillParams {
            exclude_item_ids,
            max_fill_bytes,
            device_id,
            server_id: server_id.clone(),
            now_unix: now,
            history,
            rotation_cursor,
            seed: now as u64,
            pity_streak,
            // Story 13.5: engine preview path → mint local civil time so the preview matches the fill.
            local: now_civil(),
        };
        return match expand_auto_fill_slot(provider, Some(&pipeline), fill_params).await {
            Ok(items) => serde_json::to_value(items).map_err(|e| JsonRpcError {
                code: ERR_INTERNAL_ERROR,
                message: format!("Failed to serialize auto-fill results: {}", e),
                data: None,
            }),
            Err(e) => Err(JsonRpcError {
                code: ERR_CONNECTION_FAILED,
                message: format!("Auto-fill failed: {}", e),
                data: None,
            }),
        };
    }

    // Determine available capacity for the legacy no-serverId Jellyfin preview path.
    let max_fill_bytes = if let Some(mb) = max_bytes_param {
        mb
    } else {
        // Fall back to device free bytes
        match state.device_manager.get_device_storage().await {
            Some(info) => info.free_bytes,
            None => {
                return Err(JsonRpcError {
                    code: ERR_INVALID_PARAMS,
                    message: "No device connected and no maxBytes specified".to_string(),
                    data: None,
                });
            }
        }
    };

    // Expand any container items (albums, playlists) in exclude_item_ids to their
    // constituent track IDs so that tracks inside a manually-added album are correctly
    // excluded from auto-fill results (AC-2).
    let expanded_exclude_ids = expand_exclude_ids(&state.jellyfin_client, exclude_item_ids).await;

    // Legacy Jellyfin preview (no serverId): Memory features don't apply (13.1/13.4 fields inert).
    let fill_params = crate::auto_fill::AutoFillParams {
        exclude_item_ids: expanded_exclude_ids,
        max_fill_bytes,
        device_id: String::new(),
        server_id: String::new(),
        now_unix: now_unix_secs(),
        history: crate::auto_fill::HistorySnapshot::default(),
        rotation_cursor: 0,
        seed: 0,
        pity_streak: 0,
        // Story 13.5: legacy Jellyfin preview — civil time inert (Context stage never runs here).
        local: crate::auto_fill::CivilTime::default(),
    };

    match crate::auto_fill::run_auto_fill(&state.jellyfin_client, fill_params).await {
        Ok(items) => serde_json::to_value(items).map_err(|e| JsonRpcError {
            code: ERR_INTERNAL_ERROR,
            message: format!("Failed to serialize auto-fill results: {}", e),
            data: None,
        }),
        Err(e) => Err(JsonRpcError {
            code: ERR_CONNECTION_FAILED,
            message: format!("Auto-fill failed: {}", e),
            data: None,
        }),
    }
}

/// Expand album/playlist IDs in `ids` to their constituent Audio/MusicVideo track IDs.
/// Track IDs pass through unchanged.
/// Returns an empty Vec on credential or API errors rather than falling back to raw
/// container IDs (which Jellyfin ignores in ExcludeItemIds, causing AC-2 violations).
/// Expands two levels deep to handle playlists whose children are albums.
async fn expand_exclude_ids(client: &crate::api::JellyfinClient, ids: Vec<String>) -> Vec<String> {
    if ids.is_empty() {
        return ids;
    }
    let (url, token, uid) = match crate::api::CredentialManager::get_credentials() {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let user_id = match uid {
        Some(u) => u,
        None => return Vec::new(), // No authenticated user — cannot expand
    };
    // Chunk requests to avoid URL length limits on large baskets.
    let id_refs: Vec<&str> = ids.iter().map(|s| s.as_str()).collect();
    let items = match get_items_by_ids_chunked(client, &url, &token, &user_id, &id_refs).await {
        Ok(i) => i,
        Err(_) => return Vec::new(),
    };
    let mut expanded = Vec::new();
    for item in items {
        if matches!(item.item_type.as_str(), "Audio" | "MusicVideo") {
            expanded.push(item.id);
        } else {
            // Level 1: expand container (album, playlist, artist) → children
            let children = match client
                .get_child_items_with_sizes(&url, &token, &user_id, &item.id)
                .await
            {
                Ok(c) => c,
                Err(_) => continue, // Drop this container silently on error
            };
            for child in children {
                if matches!(child.item_type.as_str(), "Audio" | "MusicVideo") {
                    expanded.push(child.id);
                } else {
                    // Level 2: expand nested container (e.g. playlist → album → tracks)
                    if let Ok(grandchildren) = client
                        .get_child_items_with_sizes(&url, &token, &user_id, &child.id)
                        .await
                    {
                        for gc in grandchildren {
                            if matches!(gc.item_type.as_str(), "Audio" | "MusicVideo") {
                                expanded.push(gc.id);
                            }
                        }
                    }
                    // If level-2 expansion fails, silently drop — better than passing
                    // an unresolvable container ID to Jellyfin's ExcludeItemIds.
                }
            }
        }
    }
    expanded
}

/// Fetches Jellyfin items by ID in chunks of 50 to avoid HTTP URL length limits.
async fn get_items_by_ids_chunked(
    client: &crate::api::JellyfinClient,
    url: &str,
    token: &str,
    user_id: &str,
    ids: &[&str],
) -> anyhow::Result<Vec<crate::api::JellyfinItem>> {
    const CHUNK_SIZE: usize = 50;
    let mut all_items = Vec::new();
    for chunk in ids.chunks(CHUNK_SIZE) {
        let items = client.get_items_by_ids(url, token, user_id, chunk).await?;
        all_items.extend(items);
    }
    Ok(all_items)
}

/// autoFill.setPipeline — persists a full per-server auto-fill pipeline to the device manifest
/// (Story 12.6). Params: `{ serverId: <portable id>, pipeline: <AutoFillPipeline JSON> }`.
///
/// Writes only the given server's slot via [`AutoFillConfig::set_pipeline`], which inserts/replaces
/// that key, clears any parked legacy block, and leaves every other server's pipeline untouched.
/// The manifest is persisted in a single atomic `update_manifest` write. This RPC deliberately does
/// NOT read or write `auto_sync_on_connect` — that stays server-independent and is configured via
/// `device_set_auto_sync_on_connect`. A blank/whitespace `serverId` or a malformed `pipeline`
/// returns `ERR_INVALID_PARAMS`.
async fn handle_auto_fill_set_pipeline(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let params = params.ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing params".to_string(),
        data: None,
    })?;

    let server_id = params
        .get("serverId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Missing or blank serverId".to_string(),
            data: None,
        })?
        .to_string();

    let pipeline_value = params.get("pipeline").cloned().ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing pipeline".to_string(),
        data: None,
    })?;
    let pipeline: crate::auto_fill::AutoFillPipeline = serde_json::from_value(pipeline_value)
        .map_err(|e| JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: format!("Invalid pipeline: {}", e),
            data: None,
        })?;

    state
        .device_manager
        .update_manifest(|m| m.auto_fill.set_pipeline(&server_id, pipeline.clone()))
        .await
        .map_err(|e| JsonRpcError {
            code: ERR_STORAGE_ERROR,
            message: format!("Failed to save pipeline: {}", e),
            data: None,
        })?;

    Ok(serde_json::json!({
        "status": "success",
        "serverId": server_id,
    }))
}

/// sync.setAutoFill — persists auto-fill preferences to the device manifest.
/// Params: { deviceId: string, autoFillEnabled: boolean, maxFillBytes?: number, autoSyncOnConnect: boolean }
async fn handle_sync_set_auto_fill(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let params = params.ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing params".to_string(),
        data: None,
    })?;

    let auto_fill_enabled = params["autoFillEnabled"].as_bool().ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing or invalid autoFillEnabled (boolean)".to_string(),
        data: None,
    })?;

    let max_fill_bytes = params["maxFillBytes"].as_u64();

    let auto_sync_on_connect = params["autoSyncOnConnect"].as_bool().ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing or invalid autoSyncOnConnect (boolean)".to_string(),
        data: None,
    })?;

    // Story 12.2: write into the selected server's portable pipeline slot when a server is
    // selected; otherwise fall back to the legacy block (no portable id available yet).
    let selected_portable_id = state
        .db
        .get_server_config()
        .ok()
        .flatten()
        .and_then(|s| s.server_id);

    // Persist both auto_fill prefs and auto_sync_on_connect in a single atomic
    // write-temp-rename operation to prevent inconsistent manifest state on crash.
    state
        .device_manager
        .update_manifest(|m| {
            match selected_portable_id.as_deref() {
                Some(id) => m.auto_fill.set_for(id, auto_fill_enabled, max_fill_bytes),
                None => m.auto_fill.set_legacy(auto_fill_enabled, max_fill_bytes),
            }
            m.auto_sync_on_connect = auto_sync_on_connect;
        })
        .await
        .map_err(|e| JsonRpcError {
            code: ERR_STORAGE_ERROR,
            message: format!("Failed to save preferences: {}", e),
            data: None,
        })?;

    // Update auto_sync_on_connect in DB if device is connected
    if let Some(device) = state.device_manager.get_current_device().await {
        state
            .db
            .set_auto_sync_on_connect(&device.device_id, auto_sync_on_connect)
            .map_err(|e| JsonRpcError {
                code: ERR_STORAGE_ERROR,
                message: format!("Failed to update auto_sync_on_connect in DB: {}", e),
                data: None,
            })?;
    }

    Ok(serde_json::json!({
        "status": "success",
        "autoFillEnabled": auto_fill_enabled,
        "maxFillBytes": max_fill_bytes,
        "autoSyncOnConnect": auto_sync_on_connect,
    }))
}

async fn handle_device_profiles_list() -> Result<Value, JsonRpcError> {
    let path = crate::paths::get_device_profiles_path().map_err(|e| JsonRpcError {
        code: ERR_STORAGE_ERROR,
        message: format!("Failed to get profiles path: {}", e),
        data: None,
    })?;

    // Seed the default profiles file on-demand if it doesn't exist.
    // This handles the case where the daemon was already running before the
    // seeding code was added (Windows Service / startup app from an older build).
    if !path.exists() {
        let profiles_default = include_bytes!("../assets/device-profiles.json");
        crate::transcoding::ensure_profiles_file_exists(&path, profiles_default).map_err(|e| {
            JsonRpcError {
                code: ERR_STORAGE_ERROR,
                message: format!("Failed to seed device profiles: {}", e),
                data: None,
            }
        })?;
    }

    let profiles = crate::transcoding::load_profiles(&path).map_err(|e| JsonRpcError {
        code: ERR_STORAGE_ERROR,
        message: format!("Failed to load device profiles: {}", e),
        data: None,
    })?;

    // Return id, name, description only — not the full deviceProfile payload
    let summary: Vec<Value> = profiles
        .iter()
        .map(|p| {
            serde_json::json!({
                "id": p.id,
                "name": p.name,
                "description": p.description,
                "defaultMusicFolder": p.default_music_folder,
                "defaultPlaylistFolder": p.default_playlist_folder,
            })
        })
        .collect();

    Ok(Value::Array(summary))
}

async fn handle_set_transcoding_profile(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let params = params.ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing params".to_string(),
        data: None,
    })?;

    let device_id = params["deviceId"].as_str().ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing deviceId".to_string(),
        data: None,
    })?;

    let profile_id = validate_transcoding_profile_id(params["profileId"].as_str())?;

    // Persist to SQLite DB
    state
        .db
        .set_transcoding_profile(device_id, profile_id.as_deref())
        .map_err(|e| JsonRpcError {
            code: ERR_STORAGE_ERROR,
            message: e.to_string(),
            data: None,
        })?;

    // Update in-memory device manifest
    state
        .device_manager
        .update_manifest(|m| {
            if m.transcoding_profile_id != profile_id {
                m.transcoding_profile_dirty = match &m.last_synced_transcoding_profile_id {
                    Some(last_synced) => profile_id.as_deref() != Some(last_synced.as_str()),
                    None => !m.synced_items.is_empty(),
                };
            }
            m.transcoding_profile_id = profile_id.clone();
        })
        .await
        .map_err(|e| JsonRpcError {
            code: ERR_STORAGE_ERROR,
            message: e.to_string(),
            data: None,
        })?;

    Ok(Value::Bool(true))
}

async fn handle_device_list(state: &AppState) -> Result<Value, JsonRpcError> {
    let devices = state.device_manager.get_connected_devices().await;
    let data: Vec<_> = devices
        .iter()
        .map(|(p, m, class)| {
            serde_json::json!({
                "path": p.to_string_lossy(),
                "deviceId": m.device_id,
                "name": m.name.clone().unwrap_or_else(|| m.device_id.clone()),
                "icon": m.icon.clone(),
                "deviceClass": match class {
                    crate::device::DeviceClass::Msc => "msc",
                    crate::device::DeviceClass::Mtp => "mtp",
                },
            })
        })
        .collect();
    Ok(serde_json::json!({ "status": "success", "data": data }))
}

async fn handle_device_select(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let params = params.ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing params".to_string(),
        data: None,
    })?;

    let path_str = params["path"].as_str().ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing path".to_string(),
        data: None,
    })?;

    let path = std::path::PathBuf::from(path_str);
    if !state.device_manager.select_device(path).await {
        return Err(JsonRpcError {
            code: 404,
            message: "Device not connected".to_string(),
            data: None,
        });
    }

    Ok(serde_json::json!({ "status": "success", "data": { "ok": true } }))
}

async fn handle_destination_select(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    #[derive(Deserialize)]
    #[serde(
        tag = "kind",
        rename_all = "camelCase",
        rename_all_fields = "camelCase",
        deny_unknown_fields
    )]
    enum Selection {
        Playback,
        Device { path: String },
        PendingDevice { pending_id: String },
    }
    let selection =
        serde_json::from_value::<Selection>(params.unwrap_or(Value::Null)).map_err(|_| {
            JsonRpcError {
                code: ERR_INVALID_PARAMS,
                message: "Invalid destination selection".to_string(),
                data: None,
            }
        })?;
    match selection {
        Selection::Playback => state.device_manager.select_playback().await,
        Selection::Device { path } => {
            if !state
                .device_manager
                .select_device(std::path::PathBuf::from(path))
                .await
            {
                return Err(JsonRpcError {
                    code: 404,
                    message: "Device not connected".to_string(),
                    data: None,
                });
            }
        }
        Selection::PendingDevice { pending_id } => {
            if !state
                .device_manager
                .select_pending_device(&pending_id)
                .await
            {
                return Err(JsonRpcError {
                    code: 404,
                    message: "Pending device not connected".to_string(),
                    data: None,
                });
            }
        }
    }
    Ok(serde_json::json!({ "status": "success", "data": { "ok": true } }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::credential_test_lock;
    use serde_json::json;
    use std::sync::Mutex;

    // ---- Story 13.5 #20: encoding-from-goals per-slot transcode override (RPC-level). ----

    fn add_item(id: &str, server_id: Option<&str>) -> crate::sync::SyncAddItem {
        crate::sync::SyncAddItem {
            media_role: crate::device::MediaRole::Music,
            jellyfin_id: id.to_string(),
            name: id.to_string(),
            album: None,
            artist: None,
            size_bytes: 1_000,
            etag: None,
            provider_album_id: None,
            provider_content_type: None,
            provider_suffix: None,
            original_bitrate: None,
            track_number: None,
            reason_code: None,
            reason: None,
            server_id: server_id.map(str::to_string),
            tier: None,
            is_auto_fill: false,
            max_bitrate_override_kbps: None,
        }
    }

    #[test]
    fn patch_delta_bitrate_overrides_scopes_to_autofill_items_only() {
        // A manual item (not in the map) and two auto-fill items (in the map) on the same server.
        let mut delta = crate::sync::SyncDelta {
            blocked: vec![],
            adds: vec![
                add_item("manual-1", Some("srv")),
                add_item("af-1", Some("srv")),
                add_item("af-2", Some("srv")),
            ],
            deletes: Vec::new(),
            id_changes: Vec::new(),
            unchanged: 0,
            playlists: Vec::new(),
            pity_fired_servers: Vec::new(),
        };
        let mut map = std::collections::HashMap::new();
        map.insert("af-1".to_string(), 96u32);
        map.insert("af-2".to_string(), 96u32);
        patch_delta_bitrate_overrides(&mut delta, &map);

        let by_id = |id: &str| {
            delta
                .adds
                .iter()
                .find(|a| a.jellyfin_id == id)
                .unwrap()
                .max_bitrate_override_kbps
        };
        assert_eq!(
            by_id("af-1"),
            Some(96),
            "auto-fill item gets the per-slot override"
        );
        assert_eq!(by_id("af-2"), Some(96));
        assert_eq!(
            by_id("manual-1"),
            None,
            "manual item on the same server is untouched"
        );

        // An empty map is a no-op (nothing stamped).
        let mut delta2 = crate::sync::SyncDelta {
            blocked: vec![],
            adds: vec![add_item("x", Some("srv"))],
            deletes: Vec::new(),
            id_changes: Vec::new(),
            unchanged: 0,
            playlists: Vec::new(),
            pity_fired_servers: Vec::new(),
        };
        patch_delta_bitrate_overrides(&mut delta2, &std::collections::HashMap::new());
        assert_eq!(delta2.adds[0].max_bitrate_override_kbps, None);
    }

    #[test]
    fn patch_delta_auto_fill_marks_non_tiered_items_only() {
        let mut delta = crate::sync::SyncDelta {
            blocked: vec![],
            adds: vec![
                add_item("manual", Some("srv")),
                add_item("auto", Some("srv")),
            ],
            deletes: Vec::new(),
            id_changes: Vec::new(),
            unchanged: 0,
            playlists: Vec::new(),
            pity_fired_servers: Vec::new(),
        };
        patch_delta_auto_fill(
            &mut delta,
            &std::collections::HashSet::from(["auto".to_string()]),
        );

        assert!(!delta.adds[0].is_auto_fill);
        assert!(delta.adds[1].is_auto_fill);
    }

    #[test]
    fn encoding_override_gated_on_active_transcode_profile() {
        use crate::auto_fill::AutoFillPipeline;
        use crate::auto_fill::pipeline::BudgetStage;
        let mut enc = AutoFillPipeline::default_legacy(Some(8_000_000));
        enc.budget = BudgetStage {
            max_bytes: Some(8_000_000),
            target_duration_secs: Some(600),
            headroom_bytes: None,
            encoding_from_goals: true,
        };

        // Passthrough / no profile ⇒ no override AND the flag is cleared so the estimate stays source-based.
        assert!(!transcode_profile_active(None));
        assert!(!transcode_profile_active(Some("passthrough")));
        assert_eq!(
            encoding_override_kbps(Some(&enc), false),
            None,
            "no transcode ⇒ no override"
        );
        let cleared =
            encoding_passthrough_clear(Some(&enc), false).expect("flag cleared in passthrough");
        assert!(!cleared.budget.encoding_from_goals);

        // Active profile ⇒ derive the override (8MB/600s ⇒ 106 kbps) and keep the flag.
        assert!(transcode_profile_active(Some("profile-aac-128")));
        assert_eq!(encoding_override_kbps(Some(&enc), true), Some(106));
        assert!(
            encoding_passthrough_clear(Some(&enc), true).is_none(),
            "flag is preserved when a transcode profile is active"
        );

        // A pipeline that didn't enable encoding-from-goals never derives an override.
        let plain = AutoFillPipeline::default_legacy(Some(8_000_000));
        assert_eq!(encoding_override_kbps(Some(&plain), true), None);
        assert!(encoding_passthrough_clear(Some(&plain), false).is_none());
    }

    pub(super) fn make_test_state(db: Arc<crate::db::Database>) -> Arc<AppState> {
        let device_manager = Arc::new(crate::device::DeviceManager::new(db.clone()));
        let playback =
            crate::playback::PlaybackSession::restore(db.clone(), "test-instance".into());
        Arc::new(AppState {
            jellyfin_client: JellyfinClient::new(),
            server_manager: Arc::new(tokio::sync::RwLock::new(
                crate::server_manager::ServerManager::new(),
            )),
            db,
            device_manager,
            last_connection_check: Arc::new(tokio::sync::Mutex::new(None)),
            size_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            sync_operation_manager: Arc::new(crate::sync::SyncOperationManager::new()),
            last_scrobbler_result: Arc::new(tokio::sync::RwLock::new(None)),
            state_tx: std::sync::mpsc::channel::<crate::DaemonState>().0,
            playback,
        })
    }

    #[tokio::test]
    async fn audiobookshelf_generic_connect_requires_library_selection_without_writes() {
        let state = make_test_state(Arc::new(crate::db::Database::memory().unwrap()));
        let error = handle_server_connect(
            &state,
            Some(serde_json::json!({
                "url": "https://abs.example.test",
                "serverType": "audiobookshelf",
                "username": "Alexis",
                "password": "not-persisted"
            })),
        )
        .await
        .unwrap_err();
        assert_eq!(
            error.data.unwrap()["errorCode"],
            "LIBRARY_SELECTION_REQUIRED"
        );
        assert!(state.db.list_servers().unwrap().is_empty());
    }

    #[test]
    fn audiobookshelf_failure_logging_contains_only_safe_classification() {
        let error = ProviderError::Http {
            status: Some(502),
            message: "https://private.example/albums user=alexis token=secret-token".into(),
        };
        let line = audiobookshelf_failure_log_line("server.audiobookshelf.discover", &error);
        assert_eq!(
            line,
            "RPC server.audiobookshelf.discover failed: provider=audiobookshelf category=http status=502"
        );
        for secret in ["private.example", "alexis", "secret-token", "/albums"] {
            assert!(!line.contains(secret));
        }

        let shape = audiobookshelf_failure_log_line(
            "server.audiobookshelf.discover",
            &ProviderError::Deserialization("raw-response-body".into()),
        );
        assert!(shape.contains("category=response_shape status=none"));
        assert!(!shape.contains("raw-response-body"));
    }

    #[tokio::test]
    async fn audiobookshelf_setup_is_one_use_and_hides_upstream_library_id() {
        use mockito::{Matcher, Server};
        let _credential_lock = crate::api::credential_test_lock();
        let pending = make_test_state(Arc::new(crate::db::Database::memory().unwrap()));
        pending.pending_audiobookshelf_setups().lock().await.clear();
        let mut upstream = Server::new_async().await;
        upstream
            .mock("POST", "/login")
            .match_header("x-return-tokens", "true")
            .match_body(Matcher::PartialJson(serde_json::json!({
                "username": "Alexis",
                "password": "fixture-password"
            })))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"user":{"accessToken":"fixture-access","refreshToken":"fixture-refresh"}}"#,
            )
            .expect(2)
            .create_async()
            .await;
        upstream
            .mock("GET", "/api/libraries")
            .match_header("authorization", "Bearer fixture-access")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"libraries":[{"id":"private-library-id","name":"Fiction","mediaType":"book"}]}"#)
            .expect(2)
            .create_async()
            .await;

        let discovered = handle_audiobookshelf_discover(
            &pending,
            Some(serde_json::json!({
                "url": upstream.url(),
                "username": " Alexis ",
                "password": "fixture-password"
            })),
        )
        .await
        .unwrap();
        assert!(pending.db.list_servers().unwrap().is_empty());
        assert!(!discovered.to_string().contains("private-library-id"));
        assert!(!discovered.to_string().contains("fixture-password"));
        let setup_id = discovered["setupId"].as_str().unwrap();
        let choice_id = discovered["libraries"][0]["choiceId"].as_str().unwrap();
        let committed = handle_audiobookshelf_commit(
            &pending,
            Some(serde_json::json!({
                "setupId": setup_id,
                "choiceId": choice_id
            })),
        )
        .await
        .unwrap();
        assert_eq!(committed["libraryRole"], "audiobook");
        assert!(!committed.to_string().contains("private-library-id"));
        let local_id = committed["localId"].as_str().unwrap();
        assert_eq!(pending.db.list_servers().unwrap().len(), 1);
        assert_eq!(
            CredentialManager::get_server_credential(local_id)
                .unwrap()
                .token_or_password,
            "fixture-password"
        );

        // Simulate restart/lazy construction: evict the live provider and require
        // a fresh local login plus exact library-role validation before caching.
        pending
            .server_manager
            .write()
            .await
            .providers
            .remove(local_id);
        let restarted =
            crate::server_manager::get_provider(&pending.server_manager, &pending.db, local_id)
                .await
                .unwrap();
        assert_eq!(
            restarted.library_role(),
            Some(crate::providers::ProviderLibraryRole::Audiobook)
        );

        upstream
            .mock("POST", "/login")
            .match_body(Matcher::PartialJson(serde_json::json!({
                "username": "Alexis",
                "password": "new-password"
            })))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"user":{"accessToken":"new-access","refreshToken":"new-refresh"}}"#)
            .create_async()
            .await;
        upstream
            .mock("GET", "/api/libraries")
            .match_header("authorization", "Bearer new-access")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"libraries":[{"id":"private-library-id","name":"Fiction","mediaType":"book"}]}"#)
            .create_async()
            .await;
        let before = pending.db.get_server(local_id).unwrap().unwrap();
        handle_server_reauthenticate(
            &pending,
            Some(serde_json::json!({ "id": local_id, "password": "new-password" })),
        )
        .await
        .unwrap();
        let after = pending.db.get_server(local_id).unwrap().unwrap();
        assert_eq!(after.id, before.id);
        assert_eq!(after.server_id, before.server_id);
        assert_eq!(after.name, before.name);
        assert_eq!(
            CredentialManager::get_server_credential(local_id)
                .unwrap()
                .token_or_password,
            "new-password"
        );

        let replay = handle_audiobookshelf_commit(
            &pending,
            Some(serde_json::json!({
                "setupId": setup_id,
                "choiceId": choice_id
            })),
        )
        .await
        .unwrap_err();
        assert_eq!(
            replay.data.unwrap()["errorCode"],
            "AUDIOBOOKSHELF_SETUP_EXPIRED"
        );
    }

    #[tokio::test]
    async fn audiobookshelf_setup_expiry_cancel_and_concurrent_consumers_have_one_winner() {
        use mockito::Server;
        let _credential_lock = crate::api::credential_test_lock();
        let state = make_test_state(Arc::new(crate::db::Database::memory().unwrap()));
        state.pending_audiobookshelf_setups().lock().await.clear();
        let mut upstream = Server::new_async().await;
        upstream
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"user":{"accessToken":"fixture-access"}}"#)
            .expect(3)
            .create_async()
            .await;
        upstream
            .mock("GET", "/api/libraries")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"libraries":[{"id":"private-id","name":"Fiction","mediaType":"book"}]}"#)
            .expect(3)
            .create_async()
            .await;

        let discover = || {
            handle_audiobookshelf_discover(
                &state,
                Some(serde_json::json!({
                    "url": upstream.url(), "username": "Alexis", "password": "fixture-password"
                })),
            )
        };

        let expired = discover().await.unwrap();
        let expired_id = expired["setupId"].as_str().unwrap();
        state
            .pending_audiobookshelf_setups()
            .lock()
            .await
            .get_mut(expired_id)
            .unwrap()
            .created_at = std::time::Instant::now()
            .checked_sub(AUDIOBOOKSHELF_SETUP_TTL + std::time::Duration::from_secs(1))
            .unwrap();
        let error = handle_audiobookshelf_commit(
            &state,
            Some(serde_json::json!({
                "setupId": expired_id,
                "choiceId": expired["libraries"][0]["choiceId"]
            })),
        )
        .await
        .unwrap_err();
        assert_eq!(
            error.data.unwrap()["errorCode"],
            "AUDIOBOOKSHELF_SETUP_EXPIRED"
        );
        for _ in 0..2 {
            assert_eq!(
                handle_audiobookshelf_cancel(
                    &state,
                    Some(serde_json::json!({ "setupId": expired_id })),
                )
                .await
                .unwrap()["ok"],
                true
            );
        }

        let raced = discover().await.unwrap();
        let raced_params = serde_json::json!({
            "setupId": raced["setupId"],
            "choiceId": raced["libraries"][0]["choiceId"]
        });
        let cancel_params = serde_json::json!({ "setupId": raced["setupId"] });
        let (commit_result, cancel_result) = tokio::join!(
            handle_audiobookshelf_commit(&state, Some(raced_params)),
            handle_audiobookshelf_cancel(&state, Some(cancel_params))
        );
        assert_eq!(cancel_result.unwrap()["ok"], true);
        if let Err(error) = commit_result {
            assert_eq!(
                error.data.unwrap()["errorCode"],
                "AUDIOBOOKSHELF_SETUP_EXPIRED"
            );
            assert!(state.db.list_servers().unwrap().is_empty());
        } else {
            assert_eq!(state.db.list_servers().unwrap().len(), 1);
        }

        let concurrent = discover().await.unwrap();
        let params = serde_json::json!({
            "setupId": concurrent["setupId"],
            "choiceId": concurrent["libraries"][0]["choiceId"]
        });
        let (first, second) = tokio::join!(
            handle_audiobookshelf_commit(&state, Some(params.clone())),
            handle_audiobookshelf_commit(&state, Some(params))
        );
        assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
        let loser = first.err().or_else(|| second.err()).unwrap();
        assert_eq!(
            loser.data.unwrap()["errorCode"],
            "AUDIOBOOKSHELF_SETUP_EXPIRED"
        );
    }

    #[tokio::test]
    async fn audiobookshelf_commit_compensates_vault_when_db_transaction_fails() {
        use mockito::Server;
        let _credential_lock = crate::api::credential_test_lock();
        let state = make_test_state(Arc::new(crate::db::Database::memory().unwrap()));
        state.pending_audiobookshelf_setups().lock().await.clear();
        let mut upstream = Server::new_async().await;
        upstream
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"user":{"accessToken":"fixture-access"}}"#)
            .create_async()
            .await;
        upstream
            .mock("GET", "/api/libraries")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"libraries":[{"id":"private-id","name":"Fiction","mediaType":"book"}]}"#)
            .create_async()
            .await;
        let setup = handle_audiobookshelf_discover(
            &state,
            Some(serde_json::json!({
                "url": upstream.url(), "username": "Alexis", "password": "fixture-password"
            })),
        )
        .await
        .unwrap();
        {
            let conn = state.db.conn.lock().unwrap();
            conn.execute_batch(
                "CREATE TRIGGER fail_audiobookshelf_insert
                 BEFORE INSERT ON server_config
                 WHEN NEW.server_type = 'audiobookshelf'
                 BEGIN SELECT RAISE(ABORT, 'injected DB failure'); END;",
            )
            .unwrap();
        }
        let error = handle_audiobookshelf_commit(
            &state,
            Some(serde_json::json!({
                "setupId": setup["setupId"],
                "choiceId": setup["libraries"][0]["choiceId"]
            })),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, ERR_STORAGE_ERROR);
        assert!(state.db.list_servers().unwrap().is_empty());
        assert_eq!(CredentialManager::test_vault_entry_count(), 0);
        assert!(state.server_manager.read().await.providers.is_empty());
    }

    #[tokio::test]
    async fn pending_destination_selection_accepts_the_serialized_camelcase_identity() {
        let state = make_test_state(Arc::new(crate::db::Database::memory().unwrap()));
        let dir = tempfile::tempdir().unwrap();
        state
            .device_manager
            .handle_device_unrecognized(
                dir.path().into(),
                Arc::new(crate::device_io::MscBackend::new(dir.path().into())),
                None,
            )
            .await;
        let wire =
            serde_json::to_value(state.device_manager.get_destination_snapshot().await).unwrap();
        let pending_id = wire["destinations"][1]["pendingId"].as_str().unwrap();
        state.device_manager.select_playback().await;
        handle_destination_select(
            &state,
            Some(json!({"kind":"pendingDevice", "pendingId":pending_id})),
        )
        .await
        .unwrap();
        let selected =
            serde_json::to_value(state.device_manager.get_destination_snapshot().await).unwrap();
        assert_eq!(selected["destinations"][1]["selected"], true);
        assert!(
            handle_destination_select(
                &state,
                Some(json!({"kind":"pendingDevice", "pending_id":pending_id}))
            )
            .await
            .is_err()
        );
    }

    #[tokio::test]
    async fn output_rpc_routes_strict_selection_replay_conflict_and_shutdown_admission() {
        use crate::playback::{
            config::OutputPreference,
            devices::{Discovery, OutputDescriptor},
        };
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let mut state = make_test_state(db.clone());
        Arc::get_mut(&mut state).unwrap().playback =
            crate::playback::PlaybackSession::restore(db, uuid::Uuid::new_v4().to_string());
        let directory = tempfile::tempdir().unwrap();
        let preference = OutputPreference {
            backend: "coreaudio".into(),
            stable_id: "rpc-device-id".into(),
            display_name: "Headphones".into(),
            identity_properties: Default::default(),
        };
        let endpoint = OutputDescriptor {
            output_id: crate::playback::devices::output_id(&preference),
            display_name: preference.display_name.clone(),
            detail: "USB".into(),
            backend: preference.backend.clone(),
            available: true,
            // Keep automatic default initialization out of this explicit-selection test.
            is_default: false,
            identity_confidence: "stable".into(),
            is_virtual: false,
            preference: Some(preference),
        };
        let output_id = endpoint.output_id.clone();
        state
            .playback
            .enable_outputs_with(directory.path().join("playback.json"), move || Discovery {
                outputs: vec![endpoint.clone()],
                error: None,
            });
        assert!(is_mutating_method("playback.selectOutput"));
        assert!(!is_mutating_method("playback.listOutputs"));
        let bad_list = handle_playback_list_outputs(
            &state,
            Some(json!({"schemaVersion":1,"unexpected":true})),
        )
        .await
        .unwrap_err();
        assert_eq!(bad_list.code, ERR_INVALID_PARAMS);
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            let list = handle_playback_list_outputs(&state, Some(json!({"schemaVersion":1})))
                .await
                .unwrap();
            if !list["data"]["outputs"].as_array().unwrap().is_empty() {
                assert!(!list.to_string().contains("rpc-device-id"));
                assert!(list["data"]["output"].is_object());
                break;
            }
            assert!(tokio::time::Instant::now() < deadline);
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        let before = state.playback.snapshot().unwrap();
        let params = json!({"schemaVersion":1,"instanceId":before.instance_id,"sessionId":before.session_id,
            "commandId":uuid::Uuid::new_v4().to_string(),"expectedOutputRevision":before.output.revision,
            "expectedGenerationId":before.generation_id,"outputId":output_id});
        let request = |params: Value| {
            serde_json::from_value(
                json!({"jsonrpc":"2.0","method":"playback.selectOutput","params":params,"id":1}),
            )
            .unwrap()
        };
        let Json(accepted) = handler(
            axum::extract::State(state.clone()),
            Json(request(params.clone())),
        )
        .await;
        assert!(accepted.error.is_none());
        let accepted_revision = state.playback.snapshot().unwrap().output.revision;
        assert_eq!(
            accepted_revision.parse::<u64>().unwrap(),
            before.output.revision.parse::<u64>().unwrap() + 1
        );
        let Json(replayed) = handler(
            axum::extract::State(state.clone()),
            Json(request(params.clone())),
        )
        .await;
        assert!(replayed.error.is_none());
        assert_eq!(
            state.playback.snapshot().unwrap().output.revision,
            accepted_revision
        );
        let mut changed = params.clone();
        changed["replaceInvalidConfig"] = json!(true);
        let Json(conflict) =
            handler(axum::extract::State(state.clone()), Json(request(changed))).await;
        assert_eq!(conflict.error.unwrap().code, 409);
        let mut unknown = params.clone();
        unknown["unknown"] = json!(1);
        let Json(invalid) =
            handler(axum::extract::State(state.clone()), Json(request(unknown))).await;
        assert_eq!(invalid.error.unwrap().code, ERR_INVALID_PARAMS);
        state
            .playback
            .begin_shutdown_checkpoint()
            .unwrap()
            .recv()
            .unwrap()
            .unwrap();
        let Json(rejected) =
            handler(axum::extract::State(state.clone()), Json(request(params))).await;
        assert!(rejected.error.is_some());
        state.playback.stop_and_join().unwrap();
    }

    #[tokio::test]
    async fn playback_contract_is_exact_bounded_offline_and_conflict_shaped() {
        let state = make_test_state(Arc::new(crate::db::Database::memory().unwrap()));
        assert!(is_mutating_method("playback.applySession"));
        assert!(is_mutating_method("playback.playEpisode"));
        assert!(is_mutating_method("playback.seek"));
        assert!(is_mutating_method("playback.retryRestore"));
        assert!(!is_mutating_method("playback.getSession"));
        assert!(!is_mutating_method("playback.listOccurrences"));

        let initial = handle_playback_get_session(&state, Some(json!({"schemaVersion": 1})))
            .await
            .unwrap();
        let snapshot = &initial["data"];
        assert_eq!(snapshot["schemaVersion"], 1);
        assert!(snapshot.get("schema_version").is_none());
        let command_id = uuid::Uuid::new_v4().to_string();
        let applied = handle_playback_apply_session(
            &state,
            Some(json!({
                "schemaVersion": 1,
                "instanceId": snapshot["instanceId"],
                "sessionId": snapshot["sessionId"],
                "commandId": command_id,
                "expectedQueueRevision": snapshot["queueRevision"],
                "operation": {
                    "type": "replaceQueue",
                    "sources": [
                        {"serverId": "offline-portable", "trackId": "same-track"},
                        {"serverId": "offline-portable", "trackId": "same-track"}
                    ]
                }
            })),
            None,
        )
        .await
        .unwrap();
        assert_eq!(
            applied["data"]["assignedOccurrences"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_ne!(
            applied["data"]["assignedOccurrences"][0]["occurrenceId"],
            applied["data"]["assignedOccurrences"][1]["occurrenceId"]
        );

        let refreshed = handle_playback_get_session(&state, Some(json!({"schemaVersion": 1})))
            .await
            .unwrap();
        assert_eq!(
            refreshed["data"]["occurrences"][0]["availability"],
            "notConfigured"
        );
        assert!(refreshed.to_string().find("http").is_none());
        let page = handle_playback_list_occurrences(
            &state,
            Some(json!({
                "schemaVersion": 1,
                "sessionId": refreshed["data"]["sessionId"],
                "expectedQueueRevision": refreshed["data"]["queueRevision"],
                "limit": 1
            })),
        )
        .await
        .unwrap();
        assert_eq!(page["data"]["occurrences"].as_array().unwrap().len(), 1);
        assert!(page["data"]["nextCursor"].is_string());

        let occurrence_ids: Vec<_> = applied["data"]["assignedOccurrences"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["occurrenceId"].clone())
            .collect();
        let display_params = Some(json!({
            "schemaVersion": 1,
            "sessionId": refreshed["data"]["sessionId"],
            "expectedQueueRevision": refreshed["data"]["queueRevision"],
            "occurrenceIds": occurrence_ids,
        }));
        // Two independent requests must both respect the same process-wide budget.
        let permits = OCCURRENCE_DISPLAY_PERMITS
            .acquire_many(OCCURRENCE_DISPLAY_PROVIDER_CONCURRENCY as u32)
            .await
            .unwrap();
        let mut first = Box::pin(handle_playback_describe_occurrences(
            &state,
            display_params.clone(),
        ));
        let mut second = Box::pin(handle_playback_describe_occurrences(&state, display_params));
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), async {
                tokio::join!(&mut first, &mut second)
            })
            .await
            .is_err(),
            "requests cannot bypass the shared provider limit"
        );
        drop(permits);
        let (described, second) = tokio::join!(first, second);
        let described = described.unwrap();
        assert_eq!(described, second.unwrap());
        let described_rows = described["data"]["occurrences"].as_array().unwrap();
        assert_eq!(
            described_rows.len(),
            2,
            "deliberate duplicate sources retain both occurrences"
        );
        assert_ne!(
            described_rows[0]["occurrenceId"],
            described_rows[1]["occurrenceId"]
        );
        assert!(
            described_rows
                .iter()
                .all(|row| row["status"] == "sourceUnavailable")
        );

        let conflict = handle_playback_apply_session(
            &state,
            Some(json!({
                "schemaVersion": 1,
                "instanceId": refreshed["data"]["instanceId"],
                "sessionId": refreshed["data"]["sessionId"],
                "commandId": uuid::Uuid::new_v4().to_string(),
                "expectedQueueRevision": "0",
                "operation": {"type": "clear"}
            })),
            None,
        )
        .await
        .unwrap_err();
        assert_eq!(conflict.code, 409);
        let conflict = conflict.data.unwrap();
        assert_eq!(conflict["code"], "QUEUE_REVISION_CONFLICT");
        assert_eq!(
            conflict["queueRevision"],
            refreshed["data"]["queueRevision"]
        );
        assert_eq!(conflict["instanceId"], refreshed["data"]["instanceId"]);
        assert_eq!(conflict["sessionId"], refreshed["data"]["sessionId"]);

        let unknown_field = handle_playback_apply_session(
            &state,
            Some(json!({
                "schemaVersion": 1,
                "instanceId": refreshed["data"]["instanceId"],
                "sessionId": refreshed["data"]["sessionId"],
                "commandId": uuid::Uuid::new_v4().to_string(),
                "expectedQueueRevision": refreshed["data"]["queueRevision"],
                "operation": {"type": "clear"},
                "streamUrl": "https://credential.invalid/secret"
            })),
            None,
        )
        .await
        .unwrap_err();
        assert_eq!(unknown_field.code, ERR_INVALID_PARAMS);
        assert_eq!(unknown_field.data.unwrap()["code"], "INVALID_SESSION");
        assert!(state.server_manager.read().await.providers.is_empty());
    }

    #[tokio::test]
    async fn playback_retry_checkpoint_router_is_authenticated_owner_bound_and_shutdown_scoped() {
        let state = make_test_state(Arc::new(crate::db::Database::memory().unwrap()));
        let operations = state.sync_operation_manager.clone();
        let app = Router::new()
            .route("/", post(handler))
            .layer(middleware::from_fn_with_state(
                Arc::new("checkpoint-token".to_string()),
                authenticate_local_request,
            ))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = reqwest::Client::new();
        let request = |method: &str, params: Value| {
            client
                .post(&url)
                .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
        };
        let before = operations.begin_shutdown_fence();
        let params =
            json!({"schemaVersion":1,"instanceId":"test-instance","shutdownId":before.shutdown_id});
        let precommit: Value = request("playback.retryCheckpoint", params.clone())
            .bearer_auth("checkpoint-token")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(precommit["error"].is_object());
        operations.begin_session_checkpoint();
        operations.commit_shutdown().await;
        operations.finish_session_checkpoint(false);
        assert_eq!(
            request("playback.retryCheckpoint", params.clone())
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            request("playback.retryCheckpoint", params.clone())
                .bearer_auth("wrong")
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        let mut stale = params.clone();
        stale["instanceId"] = json!("old-owner");
        let rejected: Value = request("playback.retryCheckpoint", stale)
            .bearer_auth("checkpoint-token")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(rejected["error"]["data"]["code"], "INSTANCE_MISMATCH");
        let mut stale = params.clone();
        stale["shutdownId"] = json!("old-shutdown");
        let rejected: Value = request("playback.retryCheckpoint", stale)
            .bearer_auth("checkpoint-token")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(rejected["error"].is_object());
        assert!(!operations.take_checkpoint_retry());
        for _ in 0..2 {
            let accepted: Value = request("playback.retryCheckpoint", params.clone())
                .bearer_auth("checkpoint-token")
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            assert_eq!(accepted["result"]["data"]["accepted"], true, "{accepted}");
        }
        assert!(operations.take_checkpoint_retry());
        assert!(!operations.take_checkpoint_retry());
        let rejected: Value = request("playback.retryRestore", json!({"schemaVersion":1}))
            .bearer_auth("checkpoint-token")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(rejected["error"]["data"]["errorCode"], "DAEMON_STOPPED");
        let health: Value = request("daemon.health", json!({}))
            .bearer_auth("checkpoint-token")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(health["result"]["data"]["status"], "stopping");
        assert_eq!(
            health["result"]["data"]["shutdown"]["shutdownId"],
            before.shutdown_id
        );
        operations.finish_session_checkpoint(true);
        let rejected: Value = request("playback.retryCheckpoint", params)
            .bearer_auth("checkpoint-token")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(rejected["error"].is_object());
        server.abort();
        let _ = server.await;
    }

    #[tokio::test]
    async fn playback_read_waiting_for_database_does_not_block_health_executor() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db.clone());
        let (locked_tx, locked_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let stalled_db = db.clone();
        let holder = std::thread::spawn(move || {
            let _connection = stalled_db.conn.lock().unwrap();
            locked_tx.send(()).unwrap();
            release_rx.recv().unwrap();
        });
        locked_rx.recv().unwrap();
        let reading = state.clone();
        let task = tokio::spawn(async move {
            handle_playback_get_session(&reading, Some(json!({"schemaVersion":1}))).await
        });
        tokio::task::yield_now().await;
        let health = tokio::time::timeout(
            std::time::Duration::from_millis(500),
            daemon_health_result(&state.sync_operation_manager, &state.playback),
        )
        .await
        .unwrap();
        assert_eq!(health["data"]["playback"]["restoration"]["status"], "ok");
        assert!(!task.is_finished());
        release_tx.send(()).unwrap();
        holder.join().unwrap();
        assert!(task.await.unwrap().is_ok());
    }

    #[tokio::test]
    async fn retry_quit_router_requires_authentication_and_a_completed_failed_fence() {
        let state = make_test_state(Arc::new(crate::db::Database::memory().unwrap()));
        let operations = Arc::clone(&state.sync_operation_manager);
        let app = Router::new()
            .route("/", post(handler))
            .layer(middleware::from_fn_with_state(
                Arc::new("retry-test-token".to_string()),
                authenticate_local_request,
            ))
            .with_state(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = reqwest::Client::new();
        let request = || {
            client.post(&url).json(&serde_json::json!({
                "jsonrpc": "2.0", "id": 1, "method": "daemon.retryQuit", "params": {}
            }))
        };
        operations.begin_shutdown_fence();
        operations.fail_shutdown_fence();
        assert_eq!(
            request().send().await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );
        assert!(!operations.take_quit_retry());

        let accepted: Value = request()
            .bearer_auth("retry-test-token")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(accepted["result"]["data"]["accepted"], true, "{accepted}");
        assert!(operations.take_quit_retry());
        assert!(!operations.take_quit_retry());

        operations.begin_shutdown_fence();
        let pending: Value = request()
            .bearer_auth("retry-test-token")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(pending["error"].is_object(), "{pending}");
        assert!(!operations.take_quit_retry());
        operations.commit_shutdown().await;
        let committed: Value = request()
            .bearer_auth("retry-test-token")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(committed["error"].is_object(), "{committed}");
        assert!(!operations.take_quit_retry());
        server.abort();
        let _ = server.await;
    }

    fn manifest_for_update() -> crate::device::DeviceManifest {
        crate::device::DeviceManifest {
            device_id: "dev-1".to_string(),
            name: Some("Device".to_string()),
            icon: Some("usb-drive".to_string()),
            version: "1.0".to_string(),
            managed_paths: vec!["Music".to_string()],
            audiobook_path: None,
            podcast_path: None,
            playlist_path: None,
            synced_items: vec![crate::device::SyncedItem {
                media_role: crate::device::MediaRole::Music,
                jellyfin_id: "song-1".to_string(),
                name: "Track".to_string(),
                album: None,
                artist: None,
                local_path: "Music/Artist/Track.flac".to_string(),
                size_bytes: 123,
                synced_at: "now".to_string(),
                original_name: None,
                etag: None,
                provider_album_id: None,
                provider_content_type: None,
                provider_suffix: None,
                original_bitrate: None,
                original_container: None,
                track_number: None,
                server_id: None,
            }],
            dirty: false,
            pending_item_ids: vec![],
            basket_items: vec![],
            auto_sync_on_connect: false,
            auto_fill: crate::device::AutoFillConfig::default(),
            transcoding_profile_id: Some("legacy-profile".to_string()),
            last_synced_transcoding_profile_id: Some("legacy-profile".to_string()),
            transcoding_profile_dirty: false,
            playlists: vec![crate::device::PlaylistManifestEntry {
                jellyfin_id: "playlist-1".to_string(),
                filename: "Road Trip.m3u".to_string(),
                track_count: 1,
                track_ids: vec!["song-1".to_string()],
                last_modified: "now".to_string(),
            }],
            storage_id: None,
            folder_ids: HashMap::from([
                ("Music".to_string(), 1),
                ("Music/Artist".to_string(), 2),
                ("Playlists".to_string(), 3),
                ("Other".to_string(), 4),
            ]),
        }
    }

    #[test]
    fn manifest_playlist_path_deserializes_missing_and_camel_case() {
        let legacy = r#"{"device_id":"dev","version":"1.0","managed_paths":["Music"]}"#;
        let manifest: crate::device::DeviceManifest = serde_json::from_str(legacy).unwrap();
        assert_eq!(manifest.playlist_path, None);
        assert_eq!(manifest.resolved_playlist_path(), Some("Music"));

        let modern = r#"{"device_id":"dev","version":"1.0","managed_paths":["Music"],"playlistPath":"Playlists"}"#;
        let manifest: crate::device::DeviceManifest = serde_json::from_str(modern).unwrap();
        assert_eq!(manifest.playlist_path.as_deref(), Some("Playlists"));
        assert_eq!(manifest.resolved_playlist_path(), Some("Playlists"));
    }

    #[test]
    fn media_folder_paths_inherit_music_and_accept_aliases() {
        let legacy: crate::device::DeviceManifest = serde_json::from_str(
            r#"{"device_id":"dev","version":"1.0","managed_paths":["Music"]}"#,
        )
        .unwrap();
        assert_eq!(
            legacy.media_path(crate::device::MediaRole::Audiobook),
            Some("Music")
        );
        assert_eq!(
            legacy.media_path(crate::device::MediaRole::Podcast),
            Some("Music")
        );
        let modern: crate::device::DeviceManifest = serde_json::from_str(r#"{"device_id":"dev","version":"1.0","managed_paths":["Music"],"audiobookPath":"Books","podcast_path":"Podcasts"}"#).unwrap();
        assert_eq!(
            modern.media_path(crate::device::MediaRole::Audiobook),
            Some("Books")
        );
        assert_eq!(
            modern.media_path(crate::device::MediaRole::Podcast),
            Some("Podcasts")
        );
    }

    #[test]
    fn editable_folder_path_rejects_unsafe_values() {
        for invalid in [
            "",
            "/Music",
            "C:/Music",
            "C:Music",
            "Music:Rock",
            "Music//Rock",
            "Music/../Rock",
            ".",
            "Music/./Rock",
        ] {
            assert!(
                normalize_editable_folder_path(invalid).is_err(),
                "{invalid} must be rejected"
            );
        }
        assert_eq!(
            normalize_editable_folder_path("Music\\Rock").unwrap(),
            "Music/Rock"
        );
    }

    #[test]
    fn manifest_metadata_only_update_does_not_require_relocation() {
        let mut manifest = manifest_for_update();
        let outcome = apply_manifest_settings_update(
            &mut manifest,
            Some("Renamed".to_string()),
            Some(Some("watch".to_string())),
            None,
            None,
            None,
            None,
            None,
        );

        assert!(!outcome.relocation_required);
        assert_eq!(manifest.name.as_deref(), Some("Renamed"));
        assert_eq!(manifest.icon.as_deref(), Some("watch"));
        assert_eq!(manifest.folder_ids.len(), 4);
    }

    #[test]
    fn manifest_folder_update_requires_relocation_and_clears_folder_cache() {
        let mut manifest = manifest_for_update();
        let outcome = apply_manifest_settings_update(
            &mut manifest,
            None,
            None,
            None,
            Some("Audio".to_string()),
            None,
            None,
            Some(Some("Playlists".to_string())),
        );

        assert!(outcome.relocation_required);
        assert_eq!(outcome.tracks_to_remove, 1);
        assert_eq!(outcome.playlists_to_remove, 1);
        assert_eq!(outcome.bytes_to_remove, 123);
        assert_eq!(manifest.managed_paths, vec!["Audio".to_string()]);
        assert_eq!(manifest.playlist_path.as_deref(), Some("Playlists"));
        assert_eq!(manifest.playlists[0].filename, "Music/Road Trip.m3u");
        assert!(!manifest.folder_ids.contains_key("Music"));
        assert!(!manifest.folder_ids.contains_key("Music/Artist"));
        assert!(!manifest.folder_ids.contains_key("Playlists"));
        assert!(manifest.folder_ids.contains_key("Other"));
    }

    #[tokio::test]
    async fn device_update_manifest_rpc_persists_metadata_and_folder_changes() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);
        let dir = tempfile::tempdir().unwrap();
        let manifest = manifest_for_update();
        crate::device::write_manifest(
            Arc::new(crate::device_io::MscBackend::new(dir.path().to_path_buf())),
            &manifest,
        )
        .await
        .unwrap();
        state
            .device_manager
            .handle_device_detected(
                dir.path().to_path_buf(),
                manifest,
                Arc::new(crate::device_io::MscBackend::new(dir.path().to_path_buf())),
            )
            .await
            .unwrap();

        let result = handle_device_update_manifest(
            &state,
            Some(json!({
                "deviceId": "dev-1",
                "name": "Road Player",
                "icon": "headphones",
                "transcodingProfileId": "passthrough",
                "musicFolderPath": "Audio",
                "playlistFolderPath": ""
            })),
        )
        .await
        .unwrap();

        assert_eq!(result["relocationRequired"], true);
        let updated = state.device_manager.get_current_device().await.unwrap();
        assert_eq!(updated.name.as_deref(), Some("Road Player"));
        assert_eq!(updated.icon.as_deref(), Some("headphones"));
        assert_eq!(updated.transcoding_profile_id, None);
        assert!(updated.transcoding_profile_dirty);
        assert_eq!(updated.managed_paths, vec!["Audio".to_string()]);
        assert_eq!(updated.playlist_path, None);
        assert_eq!(updated.resolved_playlist_path(), Some("Audio"));
    }

    #[tokio::test]
    async fn device_update_manifest_rpc_rejects_non_string_edit_fields() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);
        let dir = tempfile::tempdir().unwrap();
        let manifest = manifest_for_update();
        crate::device::write_manifest(
            Arc::new(crate::device_io::MscBackend::new(dir.path().to_path_buf())),
            &manifest,
        )
        .await
        .unwrap();
        state
            .device_manager
            .handle_device_detected(
                dir.path().to_path_buf(),
                manifest,
                Arc::new(crate::device_io::MscBackend::new(dir.path().to_path_buf())),
            )
            .await
            .unwrap();

        for params in [
            json!({ "deviceId": "dev-1", "icon": 7 }),
            json!({ "deviceId": "dev-1", "playlistFolderPath": false }),
            json!({ "deviceId": "dev-1", "transcodingProfileId": { "id": "passthrough" } }),
        ] {
            let err = handle_device_update_manifest(&state, Some(params))
                .await
                .unwrap_err();
            assert_eq!(err.code, ERR_INVALID_PARAMS);
        }
    }

    #[tokio::test]
    async fn test_rpc_server_connect_missing_params() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);

        let result = handle_server_connect(&state, None).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, ERR_INVALID_PARAMS);

        let result = handle_server_connect(&state, Some(json!({ "url": "http://x" }))).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, ERR_INVALID_PARAMS);
    }

    #[tokio::test]
    async fn test_rpc_server_connect_invalid_server_type() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);

        let result = handle_server_connect(
            &state,
            Some(json!({
                "url": "http://example",
                "serverType": "navidrome",
                "username": "user",
                "password": "pass"
            })),
        )
        .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, ERR_INVALID_PARAMS);
    }

    #[tokio::test]
    async fn test_rpc_server_connect_subsonic_failure_redacts_credentials() {
        let mut server = mockito::Server::new_async().await;
        let _ping = server
            .mock("GET", "/rest/ping.view")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"subsonic-response":{"status":"failed","version":"1.16.1","error":{"code":40,"message":"Bad auth u=rpc-user&p=rpc-password&t=rpc-token&s=rpc-salt"}}}"#,
            )
            .expect(1)
            .create_async()
            .await;

        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);

        let error = handle_server_connect(
            &state,
            Some(json!({
                "url": server.url(),
                "serverType": "subsonic",
                "username": "rpc-user",
                "password": "rpc-password"
            })),
        )
        .await
        .expect_err("server.connect should fail");

        assert_eq!(error.code, ERR_CONNECTION_FAILED);
        assert!(
            !error.message.contains("rpc-user"),
            "username leaked: {}",
            error.message
        );
        assert!(
            !error.message.contains("rpc-password"),
            "password leaked: {}",
            error.message
        );
        assert!(
            !error.message.contains("rpc-token"),
            "token leaked: {}",
            error.message
        );
        assert!(
            !error.message.contains("rpc-salt"),
            "salt leaked: {}",
            error.message
        );
        assert!(error.message.contains("[REDACTED]"));
    }

    #[tokio::test]
    async fn test_rpc_server_connect_subsonic_success_updates_state_and_db() {
        let _lock = credential_test_lock();
        let temp_dir = tempfile::tempdir().unwrap();
        CredentialManager::set_config_path(temp_dir.path().join("config.json"));
        let mut server = mockito::Server::new_async().await;
        let _ping = server
            .mock("GET", "/rest/ping.view")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"subsonic-response":{"status":"ok","version":"1.16.1","openSubsonic":true}}"#,
            )
            .expect(1)
            .create_async()
            .await;

        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db.clone());

        let result = handle_server_connect(
            &state,
            Some(json!({
                "url": server.url(),
                "serverType": "subsonic",
                "username": "user",
                "password": "pass"
            })),
        )
        .await
        .expect("server.connect should succeed");

        assert_eq!(result["ok"], true);
        assert_eq!(result["serverType"], "openSubsonic");

        let provider_set = state
            .server_manager
            .read()
            .await
            .selected_server_id
            .is_some();
        assert!(
            provider_set,
            "provider must be set after successful connect"
        );

        let server_type = db.get_server_config().unwrap().map(|c| c.server_type);
        assert_eq!(server_type.as_deref(), Some("openSubsonic"));

        let config = db.get_server_config().unwrap().unwrap();
        assert_eq!(config.server_type, "openSubsonic");
        assert_eq!(config.username, "user");
        assert_eq!(config.url, server.url());

        // Story 2.13: portable id derived (URL basis for Subsonic), persisted, and
        // returned alongside the machine-local id (semantic flip + new localId).
        let expected_portable = crate::db::derive_server_id(
            "openSubsonic",
            &crate::db::normalized_server_url(&server.url()),
            "user",
            None,
        );
        assert_eq!(
            config.server_id.as_deref(),
            Some(expected_portable.as_str())
        );
        assert_eq!(result["serverId"], json!(expected_portable));
        assert_eq!(result["localId"], json!(config.id));

        // get_daemon_state surfaces the portable id on each server row and as
        // selectedServerPortableId; server.list rows carry serverId too.
        let state_json = handle_get_daemon_state(&state).await.unwrap();
        assert_eq!(
            state_json["selectedServerPortableId"],
            json!(expected_portable)
        );
        assert_eq!(
            state_json["servers"][0]["serverId"],
            json!(expected_portable)
        );
        assert_eq!(state_json["selectedServerId"], json!(config.id));
        let list = handle_server_list(&state).await.unwrap();
        assert_eq!(list[0]["serverId"], json!(expected_portable));
        assert_eq!(list[0]["id"], json!(config.id));
    }

    /// Story 2.13: basket reconciliation maps a machine-local UUID and a pre-2.11
    /// composite onto the portable id, keeps already-portable items, adopts the
    /// selected server's portable id for untagged items, drops unknown-server items,
    /// and is idempotent.
    #[test]
    fn reconcile_basket_server_ids_targets_portable_id() {
        fn cfg(id: &str, server_id: &str, url: &str, selected: bool) -> crate::db::ServerConfig {
            crate::db::ServerConfig {
                id: id.to_string(),
                url: url.to_string(),
                server_type: "jellyfin".to_string(),
                username: "alexis".to_string(),
                server_version: None,
                name: None,
                icon: None,
                updated_at: 0,
                selected,
                server_id: Some(server_id.to_string()),
                server_reported_id: None,
                provider_library_id: None,
                provider_library_role: None,
            }
        }
        fn item(id: &str, server_id: Option<&str>) -> crate::device::BasketItem {
            crate::device::BasketItem {
                id: id.to_string(),
                name: id.to_string(),
                item_type: "Audio".to_string(),
                server_id: server_id.map(str::to_string),
                artist: None,
                child_count: 0,
                size_ticks: 0,
                size_bytes: 1,
            }
        }

        let servers = vec![cfg("local-1", "portable-1", "http://media.example", true)];
        let composite =
            crate::db::legacy_composite_server_id("jellyfin", "http://media.example", "alexis");

        let items = vec![
            item("by-local", Some("local-1")),
            item("by-composite", Some(&composite)),
            item("by-portable", Some("portable-1")),
            item("untagged", None),
            item("unknown", Some("ghost-server")),
        ];
        let out = reconcile_basket_server_ids(items, &servers);

        // unknown-server item dropped; the rest kept and mapped to the portable id.
        assert_eq!(out.len(), 4);
        for it in &out {
            assert_eq!(
                it.server_id.as_deref(),
                Some("portable-1"),
                "item {}",
                it.id
            );
        }

        // Idempotent: a second pass over already-portable items is a no-op.
        let again = reconcile_basket_server_ids(out.clone(), &servers);
        assert_eq!(again, out);
    }

    #[tokio::test]
    async fn test_rpc_login_uses_auto_detection_for_subsonic() {
        let _lock = credential_test_lock();
        let temp_dir = tempfile::tempdir().unwrap();
        CredentialManager::set_config_path(temp_dir.path().join("config.json"));
        let mut server = mockito::Server::new_async().await;
        let _ping = server
            .mock("GET", "/rest/ping.view")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"subsonic-response":{"status":"ok","version":"1.16.1"}}"#)
            .expect(1)
            .create_async()
            .await;
        let _jellyfin_auth = server
            .mock("POST", "/Users/AuthenticateByName")
            .expect(0)
            .create_async()
            .await;

        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db.clone());

        let result = handle_login(
            &state,
            Some(json!({
                "url": server.url(),
                "username": "subsonic-user",
                "password": "subsonic-password"
            })),
        )
        .await
        .expect("legacy login should use server auto-detection");

        assert_eq!(result["ok"], true);
        assert_eq!(result["serverType"], "subsonic");
        assert!(
            !state.server_manager.read().await.providers.is_empty(),
            "login must install the detected provider"
        );

        let config = db.get_server_config().unwrap().unwrap();
        assert_eq!(config.server_type, "subsonic");
        assert_eq!(config.username, "subsonic-user");
    }

    #[tokio::test]
    async fn test_legacy_library_rpcs_use_active_subsonic_provider_without_config_file() {
        let _lock = credential_test_lock();
        let temp_dir = tempfile::tempdir().unwrap();
        CredentialManager::set_config_path(temp_dir.path().join("missing-config.json"));

        let mut server = mockito::Server::new_async().await;
        let _ping = server
            .mock("GET", "/rest/ping.view")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"subsonic-response":{"status":"ok","version":"1.16.1"}}"#)
            .expect(1)
            .create_async()
            .await;
        let _artists = server
            .mock("GET", "/rest/getArtists.view")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"subsonic-response":{"status":"ok","version":"1.16.1","artists":{"index":[{"name":"A","artist":[{"id":"artist1","name":"Artist One","albumCount":2,"coverArt":"cover1"}]}]}}}"#,
            )
            .expect(1)
            .create_async()
            .await;

        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);
        handle_server_connect(
            &state,
            Some(json!({
                "url": server.url(),
                "serverType": "subsonic",
                "username": "subsonic-user",
                "password": "subsonic-password"
            })),
        )
        .await
        .expect("connect");

        let views = handle_jellyfin_get_views(&state, None)
            .await
            .expect("views should come from active provider");
        let all_view = views
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["Id"] == "all")
            .expect("should have 'all' view");
        assert_eq!(all_view["CollectionType"], "music");

        let items = handle_jellyfin_get_items(
            &state,
            Some(json!({
                "parentId": "all",
                "startIndex": 0,
                "limit": 50
            })),
        )
        .await
        .expect("items should come from active provider");
        assert_eq!(items["Items"][0]["Id"], "artist1");
        assert_eq!(items["Items"][0]["Type"], "MusicArtist");
        assert_eq!(items["Items"][0]["ImageId"], "cover1");
        assert_eq!(items["TotalRecordCount"], 1);
    }

    #[tokio::test]
    async fn test_legacy_jellyfin_items_keep_parent_folder_browse_path() {
        let _lock = credential_test_lock();
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("config.json");
        CredentialManager::set_config_path(config_path);

        let mut server = mockito::Server::new_async().await;
        let token = "jellyfin-token-12345";
        CredentialManager::save_credentials(&server.url(), token, Some("user1")).unwrap();

        let _items = server
            .mock(
                "GET",
                "/Items?userId=user1&Recursive=true&ParentId=music-folder&IncludeItemTypes=MusicAlbum,Playlist,MusicArtist,Audio,MusicVideo&StartIndex=0&Limit=50",
            )
            .match_header("Authorization", format!("MediaBrowser Token=\"{}\"", token).as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"Items":[{"Id":"artist1","Name":"Artist One","Type":"MusicArtist"}],"TotalRecordCount":1,"StartIndex":0}"#,
            )
            .expect(1)
            .create_async()
            .await;

        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);
        state
            .server_manager
            .write()
            .await
            .set_test_provider(Arc::new(
                crate::providers::jellyfin::JellyfinProvider::new_with_version(
                    JellyfinClient::new(),
                    server.url(),
                    token,
                    "user1",
                    Some("10.9.0".to_string()),
                ),
            ));

        let items = handle_jellyfin_get_items(
            &state,
            Some(json!({
                "parentId": "music-folder",
                "includeItemTypes": "MusicAlbum,Playlist,MusicArtist,Audio,MusicVideo",
                "startIndex": 0,
                "limit": 50
            })),
        )
        .await
        .expect("jellyfin items should use original Jellyfin browse RPC");

        assert_eq!(items["Items"][0]["Id"], "artist1");
        assert_eq!(items["Items"][0]["Type"], "MusicArtist");
    }

    #[tokio::test]
    async fn test_proxy_image_uses_active_subsonic_provider_cover_art_url() {
        let _lock = credential_test_lock();
        let temp_dir = tempfile::tempdir().unwrap();
        CredentialManager::set_config_path(temp_dir.path().join("missing-config.json"));

        let mut server = mockito::Server::new_async().await;
        let _ping = server
            .mock("GET", "/rest/ping.view")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"subsonic-response":{"status":"ok","version":"1.16.1"}}"#)
            .expect(1)
            .create_async()
            .await;
        let _cover = server
            .mock("GET", "/rest/getCoverArt.view")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "image/jpeg")
            .with_body(vec![1_u8, 2, 3, 4])
            .expect(1)
            .create_async()
            .await;

        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);
        handle_server_connect(
            &state,
            Some(json!({
                "url": server.url(),
                "serverType": "subsonic",
                "username": "subsonic-user",
                "password": "subsonic-password"
            })),
        )
        .await
        .expect("connect");

        let response = handle_proxy_image(
            axum::extract::State(state),
            axum::extract::Path("cover1".to_string()),
        )
        .await
        .into_response();

        assert_eq!(response.status(), http::StatusCode::OK);
        assert_eq!(
            response.headers().get(http::header::CONTENT_TYPE).unwrap(),
            "image/jpeg"
        );
    }

    #[tokio::test]
    async fn test_legacy_metadata_rpcs_use_active_subsonic_provider_for_basket_add() {
        let _lock = credential_test_lock();
        let temp_dir = tempfile::tempdir().unwrap();
        CredentialManager::set_config_path(temp_dir.path().join("missing-config.json"));

        let mut server = mockito::Server::new_async().await;
        let _ping = server
            .mock("GET", "/rest/ping.view")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"subsonic-response":{"status":"ok","version":"1.16.1"}}"#)
            .expect(1)
            .create_async()
            .await;
        let _album_for_count = server
            .mock("GET", "/rest/getAlbum.view")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"subsonic-response":{"status":"ok","version":"1.16.1","album":{"id":"album1","name":"Album One","song":[{"id":"song1","title":"Track One","duration":120,"bitRate":320},{"id":"song2","title":"Track Two","duration":60,"bitRate":160}]}}}"#,
            )
            .expect(2)
            .create_async()
            .await;

        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);
        handle_server_connect(
            &state,
            Some(json!({
                "url": server.url(),
                "serverType": "subsonic",
                "username": "subsonic-user",
                "password": "subsonic-password"
            })),
        )
        .await
        .expect("connect");

        let counts = handle_jellyfin_get_item_counts(
            &state,
            Some(json!({
                "itemIds": ["album1"]
            })),
        )
        .await
        .expect("counts should come from provider");
        assert_eq!(counts[0]["id"], "album1");
        assert_eq!(counts[0]["recursiveItemCount"], 2);
        assert_eq!(
            counts[0]["cumulativeRunTimeTicks"],
            180 * JELLYFIN_TICKS_PER_SECOND
        );

        let sizes = handle_jellyfin_get_item_sizes(
            &state,
            Some(json!({
                "itemIds": ["album1"]
            })),
        )
        .await
        .expect("sizes should come from provider");
        assert_eq!(sizes[0]["id"], "album1");
        assert_eq!(sizes[0]["totalSizeBytes"], 6_000_000);
    }

    #[tokio::test]
    async fn test_get_credentials_returns_selected_subsonic_credentials() {
        let _lock = credential_test_lock();
        let temp_dir = tempfile::tempdir().unwrap();
        CredentialManager::set_config_path(temp_dir.path().join("missing-config.json"));

        let db = Arc::new(crate::db::Database::memory().unwrap());
        let id = db
            .upsert_server(
                "http://subsonic.example",
                "subsonic",
                "subsonic-user",
                Some("1.16.1"),
                None,
                None,
                None,
            )
            .unwrap();
        CredentialManager::save_server_credential(
            &id,
            &crate::api::ServerCredentials {
                token_or_password: "subsonic-password".to_string(),
                user_id: None,
            },
        )
        .unwrap();
        let result = selected_credentials_response(&db)
            .expect("selected lookup should succeed")
            .expect("server config identity should be returned");

        assert_eq!(result["url"], "http://subsonic.example");
        assert_eq!(result["token"], "subsonic-password");
        assert_eq!(result["userId"], "subsonic-user");
        assert_eq!(result["serverType"], "subsonic");
        assert_eq!(result["serverVersion"], "1.16.1");
    }

    #[tokio::test]
    async fn test_get_credentials_ignores_stale_legacy_jellyfin_metadata() {
        let _lock = credential_test_lock();
        let temp_dir = tempfile::tempdir().unwrap();
        CredentialManager::set_config_path(temp_dir.path().join("config.json"));
        CredentialManager::save_credentials(
            "http://stale-jellyfin.example",
            "stale-jellyfin-token",
            Some("stale-jellyfin-user-id"),
        )
        .unwrap();

        let db = Arc::new(crate::db::Database::memory().unwrap());
        let id = db
            .upsert_server(
                "http://subsonic.example",
                "openSubsonic",
                "subsonic-user",
                Some("1.16.1"),
                None,
                None,
                None,
            )
            .unwrap();
        CredentialManager::save_server_credential(
            &id,
            &crate::api::ServerCredentials {
                token_or_password: "subsonic-password".to_string(),
                user_id: None,
            },
        )
        .unwrap();

        let result = selected_credentials_response(&db).unwrap().unwrap();
        assert_eq!(result["url"], "http://subsonic.example");
        assert_eq!(result["token"], "subsonic-password");
        assert_eq!(result["userId"], "subsonic-user");
        assert_eq!(result["serverType"], "openSubsonic");
    }

    #[tokio::test]
    async fn test_get_credentials_preserves_selected_jellyfin_provider_user_id() {
        let _lock = credential_test_lock();
        let temp_dir = tempfile::tempdir().unwrap();
        CredentialManager::set_config_path(temp_dir.path().join("missing-config.json"));
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let id = db
            .upsert_server(
                "http://jellyfin.example",
                "jellyfin",
                "login-name-is-not-provider-id",
                Some("10.10.7"),
                None,
                None,
                None,
            )
            .unwrap();
        CredentialManager::save_server_credential(
            &id,
            &crate::api::ServerCredentials {
                token_or_password: "jellyfin-access-token".to_string(),
                user_id: Some("jellyfin-provider-user-id".to_string()),
            },
        )
        .unwrap();

        let result = selected_credentials_response(&db).unwrap().unwrap();
        assert_eq!(result["url"], "http://jellyfin.example");
        assert_eq!(result["token"], "jellyfin-access-token");
        assert_eq!(result["userId"], "jellyfin-provider-user-id");
        assert_ne!(result["userId"], "login-name-is-not-provider-id");
    }

    #[tokio::test]
    async fn test_get_credentials_keeps_missing_selected_credential_as_expected_state() {
        let _lock = credential_test_lock();
        let temp_dir = tempfile::tempdir().unwrap();
        CredentialManager::set_config_path(temp_dir.path().join("missing-config.json"));
        let db = Arc::new(crate::db::Database::memory().unwrap());
        db.upsert_server(
            "http://subsonic.example",
            "subsonic",
            "subsonic-user",
            Some("1.16.1"),
            None,
            None,
            None,
        )
        .unwrap();

        let result = selected_credentials_response(&db).unwrap().unwrap();
        assert_eq!(result["url"], "http://subsonic.example");
        assert_eq!(result["token"], Value::Null);
        assert_eq!(result["userId"], "subsonic-user");
    }

    #[tokio::test]
    async fn test_get_credentials_rejects_unsupported_selected_provider() {
        let _lock = credential_test_lock();
        let temp_dir = tempfile::tempdir().unwrap();
        CredentialManager::set_config_path(temp_dir.path().join("missing-config.json"));
        let db = Arc::new(crate::db::Database::memory().unwrap());
        db.upsert_server(
            "http://unsupported.example",
            "unsupported",
            "user",
            None,
            None,
            None,
            None,
        )
        .unwrap();

        let error = selected_credentials_response(&db).unwrap_err();
        assert_eq!(error.code, ERR_STORAGE_ERROR);
        assert_eq!(
            error.message,
            "Unsupported selected server type: unsupported"
        );
    }

    #[tokio::test]
    async fn test_get_credentials_uses_legacy_session_without_selected_db_row() {
        let _lock = credential_test_lock();
        let temp_dir = tempfile::tempdir().unwrap();
        CredentialManager::set_config_path(temp_dir.path().join("config.json"));
        CredentialManager::save_credentials(
            "http://legacy-jellyfin.example",
            "legacy-jellyfin-token",
            Some("legacy-jellyfin-user-id"),
        )
        .unwrap();

        let db = crate::db::Database::memory().unwrap();
        assert!(selected_credentials_response(&db).unwrap().is_none());
        let result =
            legacy_credentials_response(CredentialManager::find_legacy_credentials().unwrap());
        assert_eq!(result["url"], "http://legacy-jellyfin.example");
        assert_eq!(result["token"], "legacy-jellyfin-token");
        assert_eq!(result["userId"], "legacy-jellyfin-user-id");
        assert!(result.get("serverType").is_none());
    }

    #[tokio::test]
    async fn test_sync_calculate_delta_uses_active_subsonic_provider_without_config_file() {
        let _lock = credential_test_lock();
        let temp_dir = tempfile::tempdir().unwrap();
        CredentialManager::set_config_path(temp_dir.path().join("missing-config.json"));

        let mut server = mockito::Server::new_async().await;
        let _ping = server
            .mock("GET", "/rest/ping.view")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"subsonic-response":{"status":"ok","version":"1.16.1"}}"#)
            .expect(1)
            .create_async()
            .await;
        let _album = server
            .mock("GET", "/rest/getAlbum.view")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"subsonic-response":{"status":"ok","version":"1.16.1","album":{"id":"album1","name":"Album One","song":[{"id":"song1","title":"Track One","album":"Album One","artist":"Artist One","albumId":"album1","duration":120,"bitRate":320}]}}}"#,
            )
            .expect(1)
            .create_async()
            .await;

        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);
        handle_server_connect(
            &state,
            Some(json!({
                "url": server.url(),
                "serverType": "subsonic",
                "username": "subsonic-user",
                "password": "subsonic-password"
            })),
        )
        .await
        .expect("connect");

        let dir = tempfile::tempdir().unwrap();
        let manifest = crate::device::DeviceManifest {
            device_id: "subsonic-sync-dev".to_string(),
            name: Some("Sync Dev".to_string()),
            icon: None,
            version: "1.1".to_string(),
            managed_paths: vec!["Music".to_string()],
            synced_items: vec![],
            dirty: false,
            pending_item_ids: vec![],
            basket_items: vec![],
            auto_sync_on_connect: false,
            auto_fill: crate::device::AutoFillConfig::default(),
            transcoding_profile_id: None,
            playlists: vec![],
            storage_id: None,
            ..Default::default()
        };
        state
            .device_manager
            .handle_device_detected(
                dir.path().to_path_buf(),
                manifest,
                std::sync::Arc::new(crate::device_io::MscBackend::new(dir.path().to_path_buf())),
            )
            .await
            .unwrap();

        let delta = handle_sync_calculate_delta(
            &state,
            Some(json!({
                "itemIds": ["album1"]
            })),
        )
        .await
        .expect("Subsonic delta should use active provider");

        assert_eq!(delta["adds"][0]["jellyfinId"], "song1");
        assert_eq!(delta["adds"][0]["name"], "Track One");
        assert_eq!(delta["adds"][0]["providerAlbumId"], "album1");
    }

    #[tokio::test]
    async fn test_sync_calculate_delta_favorite_album_syncs_only_favorite_tracks() {
        let _lock = credential_test_lock();
        let temp_dir = tempfile::tempdir().unwrap();
        CredentialManager::set_config_path(temp_dir.path().join("missing-config.json"));

        let mut server = mockito::Server::new_async().await;
        let _ping = server
            .mock("GET", "/rest/ping.view")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"subsonic-response":{"status":"ok","version":"1.16.1"}}"#)
            .expect(1)
            .create_async()
            .await;
        let _starred = server
            .mock("GET", "/rest/getStarred2.view")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"subsonic-response":{"status":"ok","version":"1.16.1","starred2":{
                    "song":[
                        {"id":"fav-track","title":"Favorite Track","artist":"Artist","artistId":"artist1","album":"Album","albumId":"album1","duration":120},
                        {"id":"other-track","title":"Other Favorite","artist":"Artist","artistId":"artist1","album":"Other Album","albumId":"album2","duration":130}
                    ]
                }}}"#,
            )
            .expect(1)
            .create_async()
            .await;

        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);
        handle_server_connect(
            &state,
            Some(json!({
                "url": server.url(),
                "serverType": "subsonic",
                "username": "subsonic-user",
                "password": "subsonic-password"
            })),
        )
        .await
        .expect("connect");

        let dir = tempfile::tempdir().unwrap();
        let manifest = crate::device::DeviceManifest {
            device_id: "favorite-album-dev".to_string(),
            name: Some("Favorite Album Dev".to_string()),
            version: "1.1".to_string(),
            managed_paths: vec!["Music".to_string()],
            basket_items: vec![crate::device::BasketItem {
                id: "favorites:album:album1".to_string(),
                name: "Album".to_string(),
                item_type: "FavoriteAlbum".to_string(),
                server_id: None,
                artist: Some("Artist".to_string()),
                child_count: 1,
                size_ticks: 0,
                size_bytes: 0,
            }],
            ..Default::default()
        };
        state
            .device_manager
            .handle_device_detected(
                dir.path().to_path_buf(),
                manifest,
                std::sync::Arc::new(crate::device_io::MscBackend::new(dir.path().to_path_buf())),
            )
            .await
            .unwrap();

        let delta = handle_sync_calculate_delta(
            &state,
            Some(json!({
                "itemIds": ["favorites:album:album1"],
                "basketItems": [{
                    "id": "favorites:album:album1",
                    "name": "Album",
                    "type": "FavoriteAlbum",
                    "artist": "Artist",
                    "childCount": 1,
                    "sizeTicks": 0,
                    "sizeBytes": 0
                }]
            })),
        )
        .await
        .expect("favorite album delta");

        let adds = delta["adds"].as_array().expect("adds array");
        assert_eq!(adds.len(), 1);
        assert_eq!(adds[0]["jellyfinId"], "fav-track");
        assert_eq!(adds[0]["providerAlbumId"], "album1");
    }

    #[tokio::test]
    async fn test_sync_execute_uses_active_subsonic_provider_without_config_file() {
        let _lock = credential_test_lock();
        let temp_dir = tempfile::tempdir().unwrap();
        CredentialManager::set_config_path(temp_dir.path().join("missing-config.json"));

        let mut server = mockito::Server::new_async().await;
        let _ping = server
            .mock("GET", "/rest/ping.view")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"subsonic-response":{"status":"ok","version":"1.16.1"}}"#)
            .expect(1)
            .create_async()
            .await;
        let _download = server
            .mock("GET", "/rest/download.view")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "audio/mpeg")
            .with_body(vec![1_u8, 2, 3, 4])
            .expect(1)
            .create_async()
            .await;

        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);
        handle_server_connect(
            &state,
            Some(json!({
                "url": server.url(),
                "serverType": "subsonic",
                "username": "subsonic-user",
                "password": "subsonic-password"
            })),
        )
        .await
        .expect("connect");

        let dir = tempfile::tempdir().unwrap();
        let manifest = crate::device::DeviceManifest {
            device_id: "subsonic-exec-dev".to_string(),
            name: Some("Exec Dev".to_string()),
            icon: None,
            version: "1.1".to_string(),
            managed_paths: vec!["Music".to_string()],
            synced_items: vec![],
            dirty: false,
            pending_item_ids: vec![],
            basket_items: vec![],
            auto_sync_on_connect: false,
            auto_fill: crate::device::AutoFillConfig::default(),
            transcoding_profile_id: None,
            playlists: vec![],
            storage_id: None,
            ..Default::default()
        };
        state
            .device_manager
            .handle_device_detected(
                dir.path().to_path_buf(),
                manifest,
                std::sync::Arc::new(crate::device_io::MscBackend::new(dir.path().to_path_buf())),
            )
            .await
            .unwrap();

        let delta = json!({
            "adds": [{
                "jellyfinId": "song1",
                "name": "Track One",
                "album": "Album One",
                "artist": "Artist One",
                "sizeBytes": 4,
                "etag": null,
                "providerAlbumId": "album1",
                "providerContentType": "audio/mpeg",
                "providerSuffix": "mp3"
            }],
            "deletes": [],
            "idChanges": [],
            "unchanged": 0,
            "playlists": []
        });

        let result = handle_sync_execute(&state, Some(json!({ "delta": delta })))
            .await
            .expect("Subsonic execute should use active provider");

        assert!(result["operationId"].as_str().is_some());
        for _ in 0..20 {
            let operation = state
                .sync_operation_manager
                .get_operation(result["operationId"].as_str().unwrap())
                .await
                .expect("operation");
            if operation.status != crate::sync::SyncStatus::Running {
                assert_eq!(operation.status, crate::sync::SyncStatus::Complete);
                assert!(operation.errors.is_empty(), "{:?}", operation.errors);
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        panic!("sync operation did not complete");
    }

    #[tokio::test]
    async fn test_sync_execute_uses_active_jellyfin_provider_pipeline() {
        let mut server = mockito::Server::new_async().await;
        let _download = server
            .mock("GET", "/Items/song1/Download")
            .match_query(mockito::Matcher::UrlEncoded(
                "ApiKey".into(),
                "jellyfin-token".into(),
            ))
            .with_status(200)
            .with_header("content-type", "audio/flac")
            .with_body(vec![1_u8, 2, 3, 4])
            .expect(1)
            .create_async()
            .await;

        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);
        state
            .server_manager
            .write()
            .await
            .set_test_provider(Arc::new(crate::providers::jellyfin::JellyfinProvider::new(
                JellyfinClient::new(),
                server.url(),
                "jellyfin-token",
                "user1",
            )));

        let dir = tempfile::tempdir().unwrap();
        state
            .device_manager
            .handle_device_detected(
                dir.path().to_path_buf(),
                crate::device::DeviceManifest {
                    device_id: "jellyfin-exec-dev".to_string(),
                    name: Some("Exec Dev".to_string()),
                    version: "1.1".to_string(),
                    managed_paths: vec!["Music".to_string()],
                    ..Default::default()
                },
                Arc::new(crate::device_io::MscBackend::new(dir.path().to_path_buf())),
            )
            .await
            .unwrap();

        let delta = json!({
            "adds": [{
                "jellyfinId": "song1",
                "name": "Track One",
                "album": "Album One",
                "artist": "Artist One",
                "sizeBytes": 4,
                "etag": null,
                "providerAlbumId": "album1",
                "providerContentType": "audio/flac",
                "providerSuffix": "flac"
            }],
            "deletes": [],
            "idChanges": [],
            "unchanged": 0,
            "playlists": []
        });

        let result = handle_sync_execute(&state, Some(json!({ "delta": delta })))
            .await
            .expect("Jellyfin execute should use active provider pipeline");
        let operation_id = result["operationId"].as_str().unwrap();

        for _ in 0..20 {
            let operation = state
                .sync_operation_manager
                .get_operation(operation_id)
                .await
                .expect("operation");
            if operation.status != crate::sync::SyncStatus::Running {
                assert_eq!(operation.status, crate::sync::SyncStatus::Complete);
                assert!(operation.errors.is_empty(), "{:?}", operation.errors);
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        panic!("sync operation did not complete");
    }

    #[tokio::test]
    async fn test_sync_execute_requires_confirmation_over_destructive_cleanup_threshold() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);
        let dir = tempfile::tempdir().unwrap();
        let manifest = crate::device::DeviceManifest {
            device_id: "threshold-dev".to_string(),
            version: "1.0".to_string(),
            managed_paths: vec!["Music".to_string()],
            ..Default::default()
        };
        state
            .device_manager
            .handle_device_detected(
                dir.path().to_path_buf(),
                manifest,
                std::sync::Arc::new(crate::device_io::MscBackend::new(dir.path().to_path_buf())),
            )
            .await
            .unwrap();
        let deletes: Vec<Value> = (0..=crate::sync::DESTRUCTIVE_CLEANUP_THRESHOLD)
            .map(|idx| {
                json!({
                    "jellyfinId": format!("old-{idx}"),
                    "localPath": format!("Music/Old/{idx}.flac"),
                    "name": format!("Old {idx}")
                })
            })
            .collect();
        let delta = json!({
            "adds": [],
            "deletes": deletes,
            "idChanges": [],
            "unchanged": 0,
            "playlists": []
        });

        let error = handle_sync_execute(&state, Some(json!({ "delta": delta })))
            .await
            .expect_err("large cleanup must require confirmation");

        assert_eq!(error.code, ERR_INVALID_PARAMS);
        assert!(
            error
                .data
                .as_ref()
                .and_then(|data| data["requiresDestructiveCleanupConfirmation"].as_bool())
                .unwrap_or(false),
            "error should tell the UI that explicit confirmation is required"
        );
    }

    #[tokio::test]
    async fn test_rpc_server_connect_replaces_existing_provider() {
        let _lock = credential_test_lock();
        let temp_dir = tempfile::tempdir().unwrap();
        CredentialManager::set_config_path(temp_dir.path().join("config.json"));
        let mut server = mockito::Server::new_async().await;
        let _ping = server
            .mock("GET", "/rest/ping.view")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"subsonic-response":{"status":"ok","version":"1.16.1"}}"#)
            .expect(2)
            .create_async()
            .await;

        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db.clone());

        handle_server_connect(
            &state,
            Some(json!({ "url": server.url(), "serverType": "subsonic", "username": "u1", "password": "p1" })),
        )
        .await
        .unwrap();

        let _lock_cache = state.last_connection_check.lock().await;
        drop(_lock_cache);

        handle_server_connect(
            &state,
            Some(json!({ "url": server.url(), "serverType": "subsonic", "username": "u2", "password": "p2" })),
        )
        .await
        .unwrap();

        let server_type = db.get_server_config().unwrap().map(|c| c.server_type);
        assert_eq!(server_type.as_deref(), Some("subsonic"));
    }

    // AC1/AC2/AC6/AC8/AC20: server.list / server.select / server.remove over a
    // two-server setup, including reselection when the selected server is removed.
    #[tokio::test]
    async fn test_server_list_select_remove_multi_server() {
        let _lock = credential_test_lock();
        let temp_dir = tempfile::tempdir().unwrap();
        CredentialManager::set_config_path(temp_dir.path().join("config.json"));

        let make_subsonic_server = || async {
            let mut server = mockito::Server::new_async().await;
            server
                .mock("GET", "/rest/ping.view")
                .match_query(mockito::Matcher::Any)
                .with_status(200)
                .with_header("content-type", "application/json")
                .with_body(r#"{"subsonic-response":{"status":"ok","version":"1.16.1"}}"#)
                .create_async()
                .await;
            server
        };

        let server_a = make_subsonic_server().await;
        let server_b = make_subsonic_server().await;

        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db.clone());

        let connect = |url: String, user: &str| {
            let user = user.to_string();
            let state = state.clone();
            async move {
                handle_server_connect(
                    &state,
                    Some(json!({ "url": url, "serverType": "subsonic", "username": user, "password": "pw" })),
                )
                .await
                .expect("connect")
            }
        };

        // Story 2.13: server.connect returns the PORTABLE serverId + the machine-
        // local localId. select/remove/list key on the LOCAL id.
        let res_a = connect(server_a.url(), "user-a").await;
        let id_a = res_a["serverId"].as_str().unwrap().to_string();
        let local_a = res_a["localId"].as_str().unwrap().to_string();
        let res_b = connect(server_b.url(), "user-b").await;
        let id_b = res_b["serverId"].as_str().unwrap().to_string();
        let local_b = res_b["localId"].as_str().unwrap().to_string();
        assert_ne!(id_a, id_b);
        assert_ne!(local_a, id_a, "portable id differs from local id");

        // server.list → two entries; the first connected is selected (AC1/AC20).
        let list = handle_server_list(&state).await.unwrap();
        let arr = list.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        let selected_count = arr.iter().filter(|s| s["selected"] == true).count();
        assert_eq!(selected_count, 1);
        assert!(arr.iter().any(|s| s["id"] == local_a.as_str()
            && s["serverId"] == id_a.as_str()
            && s["selected"] == true));

        // server.select(B) switches selection (AC2) — keyed on the local id.
        handle_server_select(&state, Some(json!({ "id": local_b })))
            .await
            .unwrap();
        assert_eq!(
            state
                .server_manager
                .read()
                .await
                .selected_server_id
                .as_deref(),
            Some(local_b.as_str())
        );

        // server.remove(A) — non-selected; row + vault + cache gone (AC6).
        handle_server_remove(&state, Some(json!({ "id": local_a })))
            .await
            .unwrap();
        assert_eq!(db.list_servers().unwrap().len(), 1);
        assert!(CredentialManager::get_server_credential(&local_a).is_err());
        assert!(
            !state
                .server_manager
                .read()
                .await
                .providers
                .contains_key(&local_a)
        );

        // server.remove(B) — the selected one; nothing remains, selection cleared (AC8).
        let removed = handle_server_remove(&state, Some(json!({ "id": local_b })))
            .await
            .unwrap();
        assert_eq!(removed["reselectedServerId"], Value::Null);
        assert_eq!(db.list_servers().unwrap().len(), 0);
        assert_eq!(state.server_manager.read().await.selected_server_id, None);

        // Removing a non-existent server errors.
        let err = handle_server_remove(&state, Some(json!({ "id": "nope" })))
            .await
            .unwrap_err();
        assert_eq!(err.code, ERR_NOT_FOUND);
    }

    #[tokio::test]
    async fn test_server_update_identity_metadata_only() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let id = db
            .upsert_server(
                "http://music.example",
                "subsonic",
                "alexis",
                None,
                None,
                None,
                None,
            )
            .unwrap();
        let state = make_test_state(db.clone());
        state.server_manager.write().await.load_from_db(&db);
        state
            .server_manager
            .write()
            .await
            .providers
            .insert(id.clone(), FakeBrowseProvider::new(vec![], vec![]));

        let result = handle_server_update(
            &state,
            Some(json!({ "id": id, "name": "Kitchen Hi-Fi", "icon": "headphones" })),
        )
        .await
        .expect("server.update");
        assert_eq!(result["ok"], true);

        let row = db.get_server(&id).unwrap().unwrap();
        assert_eq!(row.name.as_deref(), Some("Kitchen Hi-Fi"));
        assert_eq!(row.icon.as_deref(), Some("headphones"));
        assert_eq!(row.url, "http://music.example");
        assert!(
            state
                .server_manager
                .read()
                .await
                .providers
                .contains_key(&id),
            "identity update must not evict provider cache"
        );

        let state_json = handle_get_daemon_state(&state).await.unwrap();
        assert_eq!(state_json["servers"][0]["name"], "Kitchen Hi-Fi");
        assert_eq!(state_json["servers"][0]["icon"], "headphones");

        handle_server_update(&state, Some(json!({ "id": id, "icon": null })))
            .await
            .expect("clear icon");
        assert_eq!(db.get_server(&id).unwrap().unwrap().icon, None);
    }

    #[tokio::test]
    async fn test_server_update_rejects_url_and_invalid_icon() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let id = db
            .upsert_server(
                "http://music.example",
                "subsonic",
                "alexis",
                None,
                None,
                None,
                None,
            )
            .unwrap();
        let state = make_test_state(db);

        let url_err = handle_server_update(
            &state,
            Some(json!({ "id": id, "url": "http://evil.example", "name": "Name" })),
        )
        .await
        .unwrap_err();
        assert_eq!(url_err.code, ERR_INVALID_PARAMS);

        let icon_err =
            handle_server_update(&state, Some(json!({ "id": id, "icon": "not-a-real-icon" })))
                .await
                .unwrap_err();
        assert_eq!(icon_err.code, ERR_INVALID_PARAMS);
    }

    // AC27: itemIds accepts legacy strings and {id, serverId} objects.
    #[test]
    fn test_parse_item_specs_mixed_shapes() {
        let raw = vec![
            json!("legacy-id"),
            json!({ "id": "a", "serverId": "srv-1" }),
            json!({ "id": "b" }),
            json!(42),
        ];
        let specs = parse_item_specs(&raw);
        assert_eq!(
            specs,
            vec![
                ("legacy-id".to_string(), None),
                ("a".to_string(), Some("srv-1".to_string())),
                ("b".to_string(), None),
            ]
        );
    }

    // AC28: detect when a sync must route items to per-server providers
    // (multiple servers, or a single server that isn't the selected one).
    #[test]
    fn test_sync_needs_provider_routing() {
        let single = vec![
            ("a".into(), Some("s1".into())),
            ("b".into(), Some("s1".into())),
        ];
        assert!(!sync_needs_provider_routing(&single, Some("s1"), &[]));

        let mixed = vec![
            ("a".into(), Some("s1".into())),
            ("b".into(), Some("s2".into())),
        ];
        assert!(sync_needs_provider_routing(&mixed, Some("s1"), &[]));

        // Legacy items (no serverId) resolve to the selected server → single.
        let legacy = vec![("a".into(), None), ("b".into(), None)];
        assert!(!sync_needs_provider_routing(&legacy, Some("s1"), &[]));

        // An auto-fill slot bound to another server forces routing.
        let af = vec![("a".into(), Some("s1".into()))];
        assert!(sync_needs_provider_routing(
            &af,
            Some("s1"),
            &["s2".to_string()]
        ));

        // A basket holding only another server's (locked) items while s1 is
        // selected: a single distinct server, but NOT the selected one — must
        // still route to that server's provider, not the selected one.
        let single_other = vec![
            ("a".into(), Some("s2".into())),
            ("b".into(), Some("s2".into())),
        ];
        assert!(sync_needs_provider_routing(&single_other, Some("s1"), &[]));

        // Nothing selected but a concrete server present → route.
        assert!(sync_needs_provider_routing(&single_other, None, &[]));
    }

    // Story 12.3 AC5: every auto-fill slot counts toward the routing decision.
    #[test]
    fn test_sync_needs_provider_routing_multi_auto_fill() {
        // Single-server manual items + 2 auto-fill servers → route (one slot is
        // for a non-selected server).
        let manual = vec![("a".into(), Some("s1".into()))];
        assert!(sync_needs_provider_routing(
            &manual,
            Some("s1"),
            &["s1".to_string(), "s2".to_string()]
        ));

        // A single auto-fill slot on a non-selected server, with all manual items
        // on the selected server → route.
        assert!(sync_needs_provider_routing(
            &manual,
            Some("s1"),
            &["s2".to_string()]
        ));

        // All on the selected server (1 descriptor for s1) → no routing.
        assert!(!sync_needs_provider_routing(
            &manual,
            Some("s1"),
            &["s1".to_string()]
        ));

        // No manual items, single auto-fill slot on the selected server → single.
        assert!(!sync_needs_provider_routing(
            &[],
            Some("s1"),
            &["s1".to_string()]
        ));
    }

    // Story 12.4: a configured non-default pipeline for an auto-fill slot's server forces the
    // per-provider path (off the Jellyfin-client fast path); a default pipeline does not.
    #[test]
    fn test_auto_fill_needs_configurable_routing() {
        use crate::auto_fill::{AutoFillPipeline, SourceEntry, SourceKind};

        let mut manifest = manifest_for_update();

        // Default-legacy pipeline for s1 → fast path stays (no forced routing).
        manifest.auto_fill.pipelines.insert(
            "s1".to_string(),
            AutoFillPipeline::default_legacy(Some(8_000_000_000)),
        );
        assert!(
            !auto_fill_needs_configurable_routing(&manifest, &["s1".to_string()]),
            "default-legacy pipeline must NOT force the provider path"
        );

        // A slot whose server has no configured pipeline → no forced routing.
        assert!(!auto_fill_needs_configurable_routing(
            &manifest,
            &["unknown-server".to_string()]
        ));

        // Configure a NON-default pipeline (playlist source) for s2 → forces routing.
        let mut configured = AutoFillPipeline::default();
        configured.sources = vec![SourceEntry {
            kind: SourceKind::Playlist,
            ref_id: Some("energy".to_string()),
            share: None,
        }];
        manifest
            .auto_fill
            .pipelines
            .insert("s2".to_string(), configured);
        assert!(
            auto_fill_needs_configurable_routing(&manifest, &["s2".to_string()]),
            "a configured non-default pipeline must force the provider path"
        );

        // Mixed: s1 default + s2 configured → still forced (any non-default slot).
        assert!(auto_fill_needs_configurable_routing(
            &manifest,
            &["s1".to_string(), "s2".to_string()]
        ));

        // No auto-fill slots at all → never forced.
        assert!(!auto_fill_needs_configurable_routing(&manifest, &[]));
    }

    // Story 12.5: a headroom/duration budget on a slot's pipeline propagates through the
    // discriminator and forces the per-provider path; a bare maxBytes budget does not.
    #[test]
    fn test_auto_fill_budget_headroom_forces_routing() {
        use crate::auto_fill::AutoFillPipeline;

        let mut manifest = manifest_for_update();

        // Default-legacy + bare maxBytes for s1 → fast path stays.
        manifest.auto_fill.pipelines.insert(
            "s1".to_string(),
            AutoFillPipeline::default_legacy(Some(8_000_000_000)),
        );
        assert!(
            !auto_fill_needs_configurable_routing(&manifest, &["s1".to_string()]),
            "a bare maxBytes budget must NOT force the provider path"
        );

        // Headroom reserve on s2 → forces routing.
        let mut headroom = AutoFillPipeline::default();
        headroom.budget.headroom_bytes = Some(1_000_000_000);
        manifest
            .auto_fill
            .pipelines
            .insert("s2".to_string(), headroom);
        assert!(
            auto_fill_needs_configurable_routing(&manifest, &["s2".to_string()]),
            "a headroom reserve must force the provider path"
        );

        // Duration target on s3 → forces routing.
        let mut duration = AutoFillPipeline::default();
        duration.budget.target_duration_secs = Some(3600);
        manifest
            .auto_fill
            .pipelines
            .insert("s3".to_string(), duration);
        assert!(
            auto_fill_needs_configurable_routing(&manifest, &["s3".to_string()]),
            "a duration target must force the provider path"
        );
    }

    // --- Story 12.6: autoFill.setPipeline + get_daemon_state.pipelines + basket.autoFill routing ---

    /// Detects a device from a freshly-written manifest so handlers that read/write the manifest
    /// have a connected device to operate on.
    async fn connect_device_with_manifest(
        state: &AppState,
        dir: &std::path::Path,
        manifest: crate::device::DeviceManifest,
    ) {
        crate::device::write_manifest(
            Arc::new(crate::device_io::MscBackend::new(dir.to_path_buf())),
            &manifest,
        )
        .await
        .unwrap();
        state
            .device_manager
            .handle_device_detected(
                dir.to_path_buf(),
                manifest,
                Arc::new(crate::device_io::MscBackend::new(dir.to_path_buf())),
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn autofill_set_pipeline_rpc_round_trips_via_get_daemon_state() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);
        let dir = tempfile::tempdir().unwrap();
        connect_device_with_manifest(&state, dir.path(), manifest_for_update()).await;

        let pipeline_json = json!({
            "enabled": true,
            "filter": { "includeGenres": ["Jazz"], "excludeGenres": [], "includeTags": [], "excludeTags": [] },
            "sources": [ { "kind": "playlist", "ref": "pl-7", "share": 0.6 },
                         { "kind": "favorites", "share": 0.4 } ],
            "unit": "album",
            "ordering": ["favorite", "playCount", "quality"],
            "memory": { "cooldownWeeks": 3, "playedExclusion": true },
            "budget": { "maxBytes": 8_000_000_000_u64, "targetDurationSecs": 3600 },
            "fallback": [ { "kind": "library" } ]
        });

        let result = handle_auto_fill_set_pipeline(
            &state,
            Some(json!({ "serverId": "srv-1", "pipeline": pipeline_json.clone() })),
        )
        .await
        .expect("setPipeline succeeds");
        assert_eq!(result["status"], "success");
        assert_eq!(result["serverId"], "srv-1");

        // get_daemon_state exposes the full pipeline under the per-server map and it round-trips.
        let daemon_state = handle_get_daemon_state(&state).await.unwrap();
        let returned = &daemon_state["autoFill"]["pipelines"]["srv-1"];
        let expected: crate::auto_fill::AutoFillPipeline =
            serde_json::from_value(pipeline_json).unwrap();
        let actual: crate::auto_fill::AutoFillPipeline =
            serde_json::from_value(returned.clone()).unwrap();
        assert_eq!(actual, expected);
    }

    #[tokio::test]
    async fn autofill_set_pipeline_rpc_rejects_bad_params() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);
        let dir = tempfile::tempdir().unwrap();
        connect_device_with_manifest(&state, dir.path(), manifest_for_update()).await;

        // Blank/whitespace serverId → ERR_INVALID_PARAMS.
        let err = handle_auto_fill_set_pipeline(
            &state,
            Some(json!({ "serverId": "   ", "pipeline": { "enabled": true } })),
        )
        .await
        .unwrap_err();
        assert_eq!(err.code, ERR_INVALID_PARAMS);

        // Missing pipeline → ERR_INVALID_PARAMS.
        let err = handle_auto_fill_set_pipeline(&state, Some(json!({ "serverId": "srv-1" })))
            .await
            .unwrap_err();
        assert_eq!(err.code, ERR_INVALID_PARAMS);

        // Malformed pipeline (wrong type for a known field) → ERR_INVALID_PARAMS.
        let err = handle_auto_fill_set_pipeline(
            &state,
            Some(json!({ "serverId": "srv-1", "pipeline": { "ordering": "not-an-array" } })),
        )
        .await
        .unwrap_err();
        assert_eq!(err.code, ERR_INVALID_PARAMS);
    }

    #[tokio::test]
    async fn get_daemon_state_legacy_device_has_empty_pipelines_map() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);
        let dir = tempfile::tempdir().unwrap();
        // Legacy device: AutoFillConfig::default() (no per-server map).
        connect_device_with_manifest(&state, dir.path(), manifest_for_update()).await;

        let daemon_state = handle_get_daemon_state(&state).await.unwrap();
        let pipelines = &daemon_state["autoFill"]["pipelines"];
        assert!(pipelines.is_object(), "pipelines is an object");
        assert!(
            pipelines.as_object().unwrap().is_empty(),
            "legacy device → empty pipelines map"
        );
        // Legacy enabled/maxBytes fields retained.
        assert!(daemon_state["autoFill"]["enabled"].is_boolean());
    }

    #[tokio::test]
    async fn basket_auto_fill_unknown_server_errors_cleanly() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);

        // No provider configured for this serverId → clean ERR_CONNECTION_FAILED, no panic.
        let err = handle_basket_auto_fill(
            &state,
            Some(json!({ "serverId": "no-such-server", "maxBytes": 1_000_000 })),
        )
        .await
        .unwrap_err();
        assert_eq!(err.code, ERR_CONNECTION_FAILED);
    }

    #[tokio::test]
    async fn basket_auto_fill_routes_to_subsonic_provider() {
        let _lock = credential_test_lock();
        let temp_dir = tempfile::tempdir().unwrap();
        CredentialManager::set_config_path(temp_dir.path().join("missing-config.json"));

        let mut server = mockito::Server::new_async().await;
        let _ping = server
            .mock("GET", "/rest/ping.view")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"subsonic-response":{"status":"ok","version":"1.16.1"}}"#)
            .create_async()
            .await;
        // Favorites yields one song; this is the only source that must succeed.
        let _starred = server
            .mock("GET", "/rest/getStarred2.view")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"subsonic-response":{"status":"ok","version":"1.16.1","starred2":{"song":[{"id":"fav1","title":"Fav Track","album":"A","artist":"Artist","size":100,"duration":120,"bitRate":320}]}}}"#,
            )
            .create_async()
            .await;
        // Library bulk fill returns nothing (empty search3) so the result is just the favorite.
        let _search = server
            .mock("GET", "/rest/search3.view")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"subsonic-response":{"status":"ok","version":"1.16.1","searchResult3":{}}}"#,
            )
            .create_async()
            .await;

        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);
        handle_server_connect(
            &state,
            Some(json!({
                "url": server.url(),
                "serverType": "subsonic",
                "username": "subsonic-user",
                "password": "subsonic-password"
            })),
        )
        .await
        .expect("connect");

        // The portable serverId is the routing key the UI passes.
        let portable_id =
            handle_get_daemon_state(&state).await.unwrap()["selectedServerPortableId"]
                .as_str()
                .expect("portable id present")
                .to_string();

        // Budget must exceed the song's estimated size (bitrate×duration ≈ 4.8 MB), not its
        // reported `size` — run_auto_fill_provider sizes by bitrate.
        let result = handle_basket_auto_fill(
            &state,
            Some(json!({ "serverId": portable_id, "maxBytes": 100_000_000 })),
        )
        .await
        .expect("routed auto-fill succeeds");

        let items = result.as_array().expect("array of items");
        assert!(
            items.iter().any(|i| i["id"] == "fav1"),
            "favorite from the routed subsonic provider is returned: {result}"
        );
    }

    // --- Story 12.7: contract regression locks (AC9–AC12) ---

    /// AC11: a malformed inline `pipeline` on `basket.autoFill`+serverId surfaces as
    /// `ERR_INVALID_PARAMS` (the preview must show a user-visible error, not a silent failure).
    /// The inline parse runs before provider resolution, so this holds even without a provider.
    #[tokio::test]
    async fn basket_auto_fill_rejects_malformed_inline_pipeline() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);

        let err = handle_basket_auto_fill(
            &state,
            Some(json!({
                "serverId": "srv-1",
                "maxBytes": 1_000_000,
                "pipeline": { "ordering": "not-an-array" }
            })),
        )
        .await
        .unwrap_err();
        assert_eq!(err.code, ERR_INVALID_PARAMS);
    }

    /// AC10 + AC12: `autoFill.setPipeline` inserts/replaces only the targeted server's entry —
    /// every other server's pipeline is left byte-for-byte unchanged — and it never reads or writes
    /// `auto_sync_on_connect` (which stays server-independent, set only via its own RPC).
    #[tokio::test]
    async fn autofill_set_pipeline_isolates_servers_and_leaves_auto_sync_untouched() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);
        let dir = tempfile::tempdir().unwrap();
        // Connect a device whose auto_sync_on_connect is already ON, so we can prove setPipeline
        // doesn't disturb it.
        let mut manifest = manifest_for_update();
        manifest.auto_sync_on_connect = true;
        connect_device_with_manifest(&state, dir.path(), manifest).await;

        let pipeline_a = json!({
            "enabled": true,
            "filter": { "includeGenres": [], "excludeGenres": ["Spoken"], "includeTags": [], "excludeTags": [] },
            "sources": [ { "kind": "library" } ],
            "unit": "track",
            "ordering": ["favorite", "playCount"],
            "memory": { "playedExclusion": true },
            "budget": { "maxBytes": 4_000_000_000_u64 },
            "fallback": []
        });
        let pipeline_b = json!({
            "enabled": true,
            "filter": { "includeGenres": [], "excludeGenres": [], "includeTags": [], "excludeTags": [] },
            "sources": [ { "kind": "favorites" } ],
            "unit": "album",
            "ordering": ["dateCreated"],
            "memory": {},
            "budget": { "maxBytes": 2_000_000_000_u64 },
            "fallback": []
        });

        handle_auto_fill_set_pipeline(
            &state,
            Some(json!({ "serverId": "srv-1", "pipeline": pipeline_a.clone() })),
        )
        .await
        .expect("set srv-1");
        handle_auto_fill_set_pipeline(
            &state,
            Some(json!({ "serverId": "srv-2", "pipeline": pipeline_b.clone() })),
        )
        .await
        .expect("set srv-2");

        let device = state.device_manager.get_current_device().await.unwrap();
        // srv-1's entry is untouched by the srv-2 write — byte-for-byte equal to what we persisted.
        let expected_a: crate::auto_fill::AutoFillPipeline =
            serde_json::from_value(pipeline_a).unwrap();
        assert_eq!(device.auto_fill.pipeline_for("srv-1"), Some(&expected_a));
        assert!(device.auto_fill.pipeline_for("srv-2").is_some());
        // auto_sync_on_connect remains ON — setPipeline neither reads nor writes it.
        assert!(
            device.auto_sync_on_connect,
            "autoFill.setPipeline must not touch auto_sync_on_connect"
        );
    }

    /// AC9: a daemon with no connected device exposes `autoFill: null` (never a pipelines map).
    #[tokio::test]
    async fn get_daemon_state_no_device_has_null_auto_fill() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);

        let daemon_state = handle_get_daemon_state(&state).await.unwrap();
        assert!(
            daemon_state["autoFill"].is_null(),
            "no device → autoFill is null, got: {}",
            daemon_state["autoFill"]
        );
    }

    // Story 12.3 AC1: normalize the dual-shape `autoFill` param into descriptors.
    #[test]
    fn test_parse_auto_fill_descriptors_shapes() {
        // Legacy object, enabled → exactly one descriptor carrying its fields.
        let legacy_enabled = json!({
            "autoFill": { "enabled": true, "maxBytes": 1000, "serverId": "s1",
                          "excludeItemIds": ["x", "y"] }
        });
        let d = parse_auto_fill_descriptors(&legacy_enabled);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].server_id.as_deref(), Some("s1"));
        assert_eq!(d[0].max_bytes, Some(1000));
        assert_eq!(
            d[0].exclude_item_ids,
            vec!["x".to_string(), "y".to_string()]
        );

        // Legacy object, disabled → no descriptors.
        let legacy_disabled = json!({ "autoFill": { "enabled": false, "maxBytes": 1000 } });
        assert!(parse_auto_fill_descriptors(&legacy_disabled).is_empty());

        // Absent / null → no descriptors.
        assert!(parse_auto_fill_descriptors(&json!({})).is_empty());
        assert!(parse_auto_fill_descriptors(&json!({ "autoFill": null })).is_empty());

        // Array of two → two descriptors; missing serverId left None (selected
        // fallback applied at the call site, not here).
        let array = json!({
            "autoFill": [
                { "serverId": "s1", "maxBytes": 500 },
                { "maxBytes": 700 }
            ]
        });
        let d = parse_auto_fill_descriptors(&array);
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].server_id.as_deref(), Some("s1"));
        assert_eq!(d[0].max_bytes, Some(500));
        assert_eq!(d[1].server_id, None);
        assert_eq!(d[1].max_bytes, Some(700));

        // Array form: `enabled: false` element is filtered out; missing `enabled`
        // is treated as enabled (presence = slot).
        let array_mixed = json!({
            "autoFill": [
                { "serverId": "s1" },
                { "serverId": "s2", "enabled": false },
                { "serverId": "s3", "enabled": true }
            ]
        });
        let d = parse_auto_fill_descriptors(&array_mixed);
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].server_id.as_deref(), Some("s1"));
        assert_eq!(d[1].server_id.as_deref(), Some("s3"));

        // Empty array → no descriptors.
        assert!(parse_auto_fill_descriptors(&json!({ "autoFill": [] })).is_empty());
    }

    // Story 12.3 AC3/AC4: manual-wins dedup, cross-slot dedup, and a shared
    // remaining budget that shrinks across slots (so a small budget truncates
    // later slots). Exercises the pure dedup/budget helper used by the loop.
    #[test]
    fn test_multi_slot_dedup_and_shared_budget() {
        fn fill_item(
            id: &str,
            size: u64,
            track_number: Option<u32>,
        ) -> crate::auto_fill::AutoFillItem {
            crate::auto_fill::AutoFillItem {
                id: id.to_string(),
                name: format!("name-{id}"),
                album: None,
                artist: None,
                provider_album_id: None,
                provider_content_type: None,
                provider_suffix: None,
                track_number,
                size_bytes: size,
                priority_reason: "test".to_string(),
                tier: None,
            }
        }

        // Manual item "m1" (100 bytes) already resolved on server s1.
        let mut desired_items: Vec<crate::sync::DesiredItem> = vec![crate::sync::DesiredItem {
            media_role: crate::device::MediaRole::Music,
            jellyfin_id: "m1".to_string(),
            name: "manual".to_string(),
            album: None,
            artist: None,
            size_bytes: 100,
            etag: None,
            provider_album_id: None,
            provider_content_type: None,
            provider_suffix: None,
            original_bitrate: None,
            track_number: None,
            server_id: Some("s1".to_string()),
        }];
        let mut seen_ids: HashSet<String> = desired_items
            .iter()
            .map(|i| i.jellyfin_id.clone())
            .collect();
        let mut remaining: Option<u64> = Some(1000);
        let mut autofill_playlist_tracks = Vec::new();

        // Slot 1 (s1): returns the manual id (must be skipped — manual wins) plus
        // two new tracks (300 + 200).
        let added1 = push_fill_items_dedup(
            vec![
                fill_item("m1", 100, Some(1)),
                fill_item("f1", 300, Some(7)),
                fill_item("f2", 200, Some(8)),
            ],
            &mut desired_items,
            &mut seen_ids,
            "s1",
            crate::device::MediaRole::Music,
            &mut remaining,
            &mut autofill_playlist_tracks,
        );
        assert_eq!(added1, 500, "only the two new tracks count");
        assert_eq!(
            desired_items.len(),
            3,
            "manual + f1 + f2; m1 not duplicated"
        );
        assert_eq!(
            autofill_playlist_tracks
                .iter()
                .map(|t| t.jellyfin_id.as_str())
                .collect::<Vec<_>>(),
            vec!["f1", "f2"],
            "playlist includes deduped autofill tracks only"
        );
        assert_eq!(
            desired_items
                .iter()
                .filter(|i| i.jellyfin_id == "m1")
                .count(),
            1,
            "manual item present exactly once"
        );
        assert_eq!(remaining, Some(500), "budget decremented by 500");
        assert_eq!(
            desired_items
                .iter()
                .find(|i| i.jellyfin_id == "f1")
                .and_then(|i| i.track_number),
            Some(7),
            "autofill desired items keep provider track numbers"
        );
        // f1/f2 tagged with the slot's server.
        assert!(
            desired_items
                .iter()
                .filter(|i| i.jellyfin_id == "f1" || i.jellyfin_id == "f2")
                .all(|i| i.server_id.as_deref() == Some("s1"))
        );

        // Slot 2 (s2) with a large maxBytes: the shared remaining (500) truncates
        // it — this is the budget the loop passes to run_auto_fill_provider.
        let slot2_max: Option<u64> = Some(10_000);
        let r = remaining.expect("budget present");
        let slot2_budget = slot2_max.map_or(r, |mb| mb.min(r));
        assert_eq!(slot2_budget, 500, "tiny shared budget caps the second slot");

        // Slot 2 returns f1 (cross-slot dup → skipped) and a new f3 (400 bytes).
        let added2 = push_fill_items_dedup(
            vec![fill_item("f1", 300, None), fill_item("f3", 400, None)],
            &mut desired_items,
            &mut seen_ids,
            "s2",
            crate::device::MediaRole::Music,
            &mut remaining,
            &mut autofill_playlist_tracks,
        );
        assert_eq!(added2, 400, "f1 already seen from slot 1; only f3 added");
        assert_eq!(desired_items.len(), 4, "manual + f1 + f2 + f3");
        assert_eq!(
            autofill_playlist_tracks
                .iter()
                .map(|t| t.jellyfin_id.as_str())
                .collect::<Vec<_>>(),
            vec!["f1", "f2", "f3"],
            "playlist preserves cross-slot autofill order"
        );
        assert_eq!(
            desired_items
                .iter()
                .filter(|i| i.jellyfin_id == "f1")
                .count(),
            1,
            "f1 not re-added by slot 2"
        );
        assert_eq!(
            desired_items
                .iter()
                .find(|i| i.jellyfin_id == "f3")
                .and_then(|i| i.server_id.as_deref()),
            Some("s2"),
            "f3 tagged with slot 2's server"
        );
        assert_eq!(remaining, Some(100), "500 − 400 = 100 remaining");
    }

    // AC11: provider auth errors map to ERR_UNAUTHORIZED with an `unauthorized`
    // data flag, so the UI distinguishes them from generic connection failures.
    #[test]
    fn test_provider_auth_error_maps_to_unauthorized() {
        let err = provider_error_to_rpc(ProviderError::Auth("token expired".into()));
        assert_eq!(err.code, ERR_UNAUTHORIZED);
        assert_eq!(err.message, "token expired");
        assert_eq!(
            err.data.as_ref().and_then(|d| d["unauthorized"].as_bool()),
            Some(true)
        );

        // Non-auth errors keep their own codes.
        let nf = provider_error_to_rpc(ProviderError::NotFound {
            item_type: "song".into(),
            id: "x".into(),
        });
        assert_eq!(nf.code, ERR_NOT_FOUND);
    }

    #[test]
    fn test_parse_server_type_hint_accepts_supported_values() {
        assert_eq!(
            parse_server_type_hint("auto").unwrap(),
            ServerTypeHint::Auto
        );
        assert_eq!(
            parse_server_type_hint("jellyfin").unwrap(),
            ServerTypeHint::Jellyfin
        );
        assert_eq!(
            parse_server_type_hint("subsonic").unwrap(),
            ServerTypeHint::Subsonic
        );
        assert_eq!(
            parse_server_type_hint("navidrome").unwrap_err().code,
            ERR_INVALID_PARAMS
        );
    }

    #[tokio::test]
    async fn test_rpc_test_connection_params() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let device_manager = Arc::new(crate::device::DeviceManager::new(db.clone()));
        let state = Arc::new(AppState {
            jellyfin_client: JellyfinClient::new(),
            server_manager: Arc::new(tokio::sync::RwLock::new(
                crate::server_manager::ServerManager::new(),
            )),
            db,
            device_manager,
            last_connection_check: Arc::new(tokio::sync::Mutex::new(None)),
            size_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            sync_operation_manager: Arc::new(crate::sync::SyncOperationManager::new()),
            last_scrobbler_result: Arc::new(tokio::sync::RwLock::new(None)),
            state_tx: std::sync::mpsc::channel::<crate::DaemonState>().0,
            playback: crate::playback::PlaybackSession::restore(
                Arc::new(crate::db::Database::memory().unwrap()),
                "test-instance".into(),
            ),
        });

        let params = json!({
            "url": "http://invalid-url-123",
            "token": "test-token"
        });

        // This will attempt a real network call, but we expect it to fail gracefully
        let res = handle_test_connection(&state, Some(params)).await;
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().code, -1);
    }

    #[tokio::test]
    async fn test_rpc_invalid_method() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let device_manager = Arc::new(crate::device::DeviceManager::new(db.clone()));
        let state = Arc::new(AppState {
            jellyfin_client: JellyfinClient::new(),
            server_manager: Arc::new(tokio::sync::RwLock::new(
                crate::server_manager::ServerManager::new(),
            )),
            db,
            device_manager,
            last_connection_check: Arc::new(tokio::sync::Mutex::new(None)),
            size_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            sync_operation_manager: Arc::new(crate::sync::SyncOperationManager::new()),
            last_scrobbler_result: Arc::new(tokio::sync::RwLock::new(None)),
            state_tx: std::sync::mpsc::channel::<crate::DaemonState>().0,
            playback: crate::playback::PlaybackSession::restore(
                Arc::new(crate::db::Database::memory().unwrap()),
                "test-instance".into(),
            ),
        });

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "invalid_method".to_string(),
            params: None,
            id: json!(1),
        };

        let response = handler(axum::extract::State(state), Json(request)).await;
        assert!(response.error.is_some());
        assert_eq!(response.error.as_ref().unwrap().code, -32601);
    }

    #[tokio::test]
    async fn test_rpc_set_device_profile() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let device_manager = Arc::new(crate::device::DeviceManager::new(db.clone()));
        let state = Arc::new(AppState {
            jellyfin_client: JellyfinClient::new(),
            server_manager: Arc::new(tokio::sync::RwLock::new(
                crate::server_manager::ServerManager::new(),
            )),
            db: db.clone(),
            device_manager,
            last_connection_check: Arc::new(tokio::sync::Mutex::new(None)),
            size_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            sync_operation_manager: Arc::new(crate::sync::SyncOperationManager::new()),
            last_scrobbler_result: Arc::new(tokio::sync::RwLock::new(None)),
            state_tx: std::sync::mpsc::channel::<crate::DaemonState>().0,
            playback: crate::playback::PlaybackSession::restore(
                Arc::new(crate::db::Database::memory().unwrap()),
                "test-instance".into(),
            ),
        });

        let params = json!({
            "deviceId": "test-device",
            "profileId": "user-123",
            "syncRules": "{\"playlist_id\": \"abc\"}"
        });

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "set_device_profile".to_string(),
            params: Some(params),
            id: json!(1),
        };

        let response = handler(axum::extract::State(state), Json(request)).await;
        assert!(response.0.result.is_some());
        assert_eq!(response.0.result.as_ref().unwrap(), &true);

        // Verify it was persisted
        let mapping = db.get_device_mapping("test-device").unwrap().unwrap();
        assert_eq!(mapping.jellyfin_user_id, Some("user-123".to_string()));
    }

    #[tokio::test]
    async fn test_rpc_get_item_counts_basic() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let device_manager = Arc::new(crate::device::DeviceManager::new(db.clone()));
        let state = Arc::new(AppState {
            jellyfin_client: JellyfinClient::new(),
            server_manager: Arc::new(tokio::sync::RwLock::new(
                crate::server_manager::ServerManager::new(),
            )),
            db: db.clone(),
            device_manager,
            last_connection_check: Arc::new(tokio::sync::Mutex::new(None)),
            size_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            sync_operation_manager: Arc::new(crate::sync::SyncOperationManager::new()),
            last_scrobbler_result: Arc::new(tokio::sync::RwLock::new(None)),
            state_tx: std::sync::mpsc::channel::<crate::DaemonState>().0,
            playback: crate::playback::PlaybackSession::restore(
                Arc::new(crate::db::Database::memory().unwrap()),
                "test-instance".into(),
            ),
        });

        // We can't easily mock the network call inside the RPC handler without a mock server or traits,
        // but we can test the parameter parsing and error handling for now.
        // If we want to test success, we'd need to mock CredentialManager or use a real mockito server.

        // Test missing params
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "jellyfin_get_item_counts".to_string(),
            params: None,
            id: json!(1),
        };
        let response = handler(axum::extract::State(state.clone()), Json(request)).await;
        assert!(response.0.error.is_some());
        assert_eq!(response.0.error.as_ref().unwrap().code, -32602);

        // Test invalid params (missing itemIds)
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "jellyfin_get_item_counts".to_string(),
            params: Some(json!({})),
            id: json!(1),
        };
        let response = handler(axum::extract::State(state.clone()), Json(request)).await;
        assert!(response.0.error.is_some());
    }

    #[tokio::test]
    async fn test_rpc_get_item_sizes_missing_params() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let device_manager = Arc::new(crate::device::DeviceManager::new(db.clone()));
        let state = Arc::new(AppState {
            jellyfin_client: JellyfinClient::new(),
            server_manager: Arc::new(tokio::sync::RwLock::new(
                crate::server_manager::ServerManager::new(),
            )),
            db: db.clone(),
            device_manager,
            last_connection_check: Arc::new(tokio::sync::Mutex::new(None)),
            size_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            sync_operation_manager: Arc::new(crate::sync::SyncOperationManager::new()),
            last_scrobbler_result: Arc::new(tokio::sync::RwLock::new(None)),
            state_tx: std::sync::mpsc::channel::<crate::DaemonState>().0,
            playback: crate::playback::PlaybackSession::restore(
                Arc::new(crate::db::Database::memory().unwrap()),
                "test-instance".into(),
            ),
        });

        // Test missing params
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "jellyfin_get_item_sizes".to_string(),
            params: None,
            id: json!(1),
        };
        let response = handler(axum::extract::State(state.clone()), Json(request)).await;
        assert!(response.0.error.is_some());
        assert_eq!(response.0.error.as_ref().unwrap().code, -32602);

        // Test invalid params (missing itemIds)
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "jellyfin_get_item_sizes".to_string(),
            params: Some(json!({})),
            id: json!(1),
        };
        let response = handler(axum::extract::State(state.clone()), Json(request)).await;
        assert!(response.0.error.is_some());
        assert_eq!(response.0.error.as_ref().unwrap().code, -32602);
    }

    #[tokio::test]
    async fn test_jellyfin_item_serialization_metadata() {
        // Verify that our JellyfinItem struct correctly handles the metadata we care about
        let json = json!({
            "Id": "item1",
            "Name": "Item 1",
            "Type": "MusicAlbum",
            "RecursiveItemCount": 10,
            "CumulativeRunTimeTicks": 1000000,
            "Etag": "some_etag"
        });
        let item: crate::api::JellyfinItem = serde_json::from_value(json).unwrap();
        assert_eq!(item.recursive_item_count, Some(10));
        assert_eq!(item.cumulative_run_time_ticks, Some(1000000));
        assert_eq!(item.etag, Some("some_etag".to_string()));
    }

    /// Regression test: M4A/AAC items often have `MediaSource.Bitrate: null`
    /// and only expose the bitrate inside `MediaSource.MediaStreams[Audio].BitRate`.
    /// `jellyfin_item_to_desired_item` must fall back to the audio stream value.
    #[test]
    fn test_jellyfin_item_original_bitrate_falls_back_to_audio_media_stream() {
        let json = serde_json::json!({
            "Id": "m4a-track-1",
            "Name": "AAC Track",
            "Type": "Audio",
            "MediaSources": [{
                "Container": "m4a",
                "Bitrate": null,          // absent at container level
                "MediaStreams": [
                    { "Type": "Audio", "BitRate": 256000 }
                ]
            }]
        });
        let item: crate::api::JellyfinItem = serde_json::from_value(json).unwrap();
        let desired = jellyfin_item_to_desired_item(item);
        assert_eq!(
            desired.original_bitrate,
            Some(256000),
            "should fall back to audio MediaStream BitRate when MediaSource.Bitrate is null"
        );
    }

    /// When both `MediaSource.Bitrate` and an audio stream `BitRate` are present,
    /// the container-level bitrate must take precedence.
    #[test]
    fn test_jellyfin_item_original_bitrate_prefers_media_source_bitrate() {
        let json = serde_json::json!({
            "Id": "flac-track-1",
            "Name": "FLAC Track",
            "Type": "Audio",
            "MediaSources": [{
                "Container": "flac",
                "Bitrate": 1411200,
                "MediaStreams": [
                    { "Type": "Audio", "BitRate": 1200000 }
                ]
            }]
        });
        let item: crate::api::JellyfinItem = serde_json::from_value(json).unwrap();
        let desired = jellyfin_item_to_desired_item(item);
        assert_eq!(
            desired.original_bitrate,
            Some(1411200),
            "container-level bitrate should take precedence over audio stream bitrate"
        );
    }

    /// When no bitrate is available at any level, `original_bitrate` must be `None` —
    /// no spurious re-sync should be triggered.
    #[test]
    fn test_jellyfin_item_original_bitrate_none_when_fully_absent() {
        let json = serde_json::json!({
            "Id": "unknown-track",
            "Name": "Track",
            "Type": "Audio",
            "MediaSources": [{
                "Container": "m4a",
                "Bitrate": null,
                "MediaStreams": [
                    { "Type": "Video", "BitRate": 4000000 }   // only a video stream, no audio
                ]
            }]
        });
        let item: crate::api::JellyfinItem = serde_json::from_value(json).unwrap();
        let desired = jellyfin_item_to_desired_item(item);
        assert_eq!(
            desired.original_bitrate, None,
            "should be None when only non-audio streams are present"
        );
    }

    #[tokio::test]
    async fn test_rpc_get_items_params() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let device_manager = Arc::new(crate::device::DeviceManager::new(db.clone()));
        let state = Arc::new(AppState {
            jellyfin_client: JellyfinClient::new(),
            server_manager: Arc::new(tokio::sync::RwLock::new(
                crate::server_manager::ServerManager::new(),
            )),
            db: db.clone(),
            device_manager,
            last_connection_check: Arc::new(tokio::sync::Mutex::new(None)),
            size_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            sync_operation_manager: Arc::new(crate::sync::SyncOperationManager::new()),
            last_scrobbler_result: Arc::new(tokio::sync::RwLock::new(None)),
            state_tx: std::sync::mpsc::channel::<crate::DaemonState>().0,
            playback: crate::playback::PlaybackSession::restore(
                Arc::new(crate::db::Database::memory().unwrap()),
                "test-instance".into(),
            ),
        });

        // Test with specific parameters including includeItemTypes
        let params = json!({
            "parentId": "lib1",
            "includeItemTypes": "MusicAlbum,Audio",
            "startIndex": 0,
            "limit": 20
        });

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "jellyfin_get_items".to_string(),
            params: Some(params),
            id: json!(1),
        };

        // We expect this to fail with connection/storage error (since no real creds),
        // but NOT method not found or invalid params.
        // This confirms the handler captures the params and tries to execute.
        let response = handler(axum::extract::State(state), Json(request)).await;

        // It might be error or result depending on how deep it gets,
        // but definitely shouldn't be "Invalid Params" (-32602) or "Method Not Found" (-32601)
        if let Some(err) = response.0.error {
            assert_ne!(err.code, -32601, "Method should exist");
            assert_ne!(err.code, -32602, "Params should be valid");
        }
    }

    #[tokio::test]
    async fn test_rpc_sync_calculate_delta_missing_params() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let device_manager = Arc::new(crate::device::DeviceManager::new(db.clone()));
        let state = Arc::new(AppState {
            jellyfin_client: JellyfinClient::new(),
            server_manager: Arc::new(tokio::sync::RwLock::new(
                crate::server_manager::ServerManager::new(),
            )),
            db: db.clone(),
            device_manager,
            last_connection_check: Arc::new(tokio::sync::Mutex::new(None)),
            size_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            sync_operation_manager: Arc::new(crate::sync::SyncOperationManager::new()),
            last_scrobbler_result: Arc::new(tokio::sync::RwLock::new(None)),
            state_tx: std::sync::mpsc::channel::<crate::DaemonState>().0,
            playback: crate::playback::PlaybackSession::restore(
                Arc::new(crate::db::Database::memory().unwrap()),
                "test-instance".into(),
            ),
        });

        // Test missing params
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "sync_calculate_delta".to_string(),
            params: None,
            id: json!(1),
        };
        let response = handler(axum::extract::State(state.clone()), Json(request)).await;
        assert!(response.0.error.is_some());
        assert_eq!(response.0.error.as_ref().unwrap().code, ERR_INVALID_PARAMS);

        // Test invalid params (missing itemIds)
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "sync_calculate_delta".to_string(),
            params: Some(json!({})),
            id: json!(1),
        };
        let response = handler(axum::extract::State(state.clone()), Json(request)).await;
        assert!(response.0.error.is_some());
        assert_eq!(response.0.error.as_ref().unwrap().code, ERR_INVALID_PARAMS);
    }

    #[tokio::test]
    async fn test_rpc_sync_calculate_delta_no_device() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let device_manager = Arc::new(crate::device::DeviceManager::new(db.clone()));
        let state = Arc::new(AppState {
            jellyfin_client: JellyfinClient::new(),
            server_manager: Arc::new(tokio::sync::RwLock::new(
                crate::server_manager::ServerManager::new(),
            )),
            db: db.clone(),
            device_manager,
            last_connection_check: Arc::new(tokio::sync::Mutex::new(None)),
            size_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            sync_operation_manager: Arc::new(crate::sync::SyncOperationManager::new()),
            last_scrobbler_result: Arc::new(tokio::sync::RwLock::new(None)),
            state_tx: std::sync::mpsc::channel::<crate::DaemonState>().0,
            playback: crate::playback::PlaybackSession::restore(
                Arc::new(crate::db::Database::memory().unwrap()),
                "test-instance".into(),
            ),
        });

        // No device connected — should return error
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "sync_calculate_delta".to_string(),
            params: Some(json!({ "itemIds": ["item-1", "item-2"] })),
            id: json!(1),
        };
        let response = handler(axum::extract::State(state), Json(request)).await;
        assert!(response.0.error.is_some());
        assert_eq!(
            response.0.error.as_ref().unwrap().code,
            ERR_CONNECTION_FAILED
        );
    }

    #[tokio::test]
    async fn test_rpc_sync_detect_changes_validates_sync_token_params() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);

        for params in [None, Some(json!({})), Some(json!({ "syncToken": 123 }))] {
            let error = handle_sync_detect_changes(&state, params)
                .await
                .expect_err("invalid params should be rejected before device/provider access");
            assert_eq!(error.code, ERR_INVALID_PARAMS);
        }
    }

    #[tokio::test]
    async fn test_rpc_sync_detect_changes_returns_stable_wire_fields_and_metadata() {
        let mut server = mockito::Server::new_async().await;
        let _indexes = server
            .mock("GET", "/rest/getIndexes.view")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("u".into(), "rpc-user".into()),
                mockito::Matcher::UrlEncoded("v".into(), "1.16.1".into()),
                mockito::Matcher::UrlEncoded("c".into(), "hifimule".into()),
                mockito::Matcher::UrlEncoded("f".into(), "json".into()),
                mockito::Matcher::UrlEncoded("ifModifiedSince".into(), "1710000000000".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"subsonic-response":{"status":"ok","version":"1.16.1","openSubsonic":true,"indexes":{"index":[]}}}"#,
            )
            .expect(1)
            .create_async()
            .await;
        let _album = server
            .mock("GET", "/rest/getAlbum.view")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("u".into(), "rpc-user".into()),
                mockito::Matcher::UrlEncoded("v".into(), "1.16.1".into()),
                mockito::Matcher::UrlEncoded("c".into(), "hifimule".into()),
                mockito::Matcher::UrlEncoded("f".into(), "json".into()),
                mockito::Matcher::UrlEncoded("id".into(), "album1".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"subsonic-response":{"status":"ok","version":"1.16.1","openSubsonic":true,"album":{"id":"album1","name":"Album","song":[{"id":"song1","title":"Existing","albumId":"album1","size":1000,"contentType":"audio/mpeg","suffix":"mp3"},{"id":"song2","title":"New","albumId":"album1","size":3000,"contentType":"audio/flac","suffix":"flac"}]}}}"#,
            )
            .expect(1)
            .create_async()
            .await;

        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);
        let provider = crate::providers::subsonic::SubsonicProvider::from_stored_config(
            ProviderCredentials {
                server_url: server.url(),
                credential: CredentialKind::Password {
                    username: "rpc-user".to_string(),
                    password: "rpc-pass".to_string(),
                },
            },
            true,
            Some("1.16.1".to_string()),
        )
        .expect("provider");
        state
            .server_manager
            .write()
            .await
            .set_test_provider(Arc::new(provider) as Arc<dyn MediaProvider>);

        let dir = tempfile::tempdir().unwrap();
        let manifest = crate::device::DeviceManifest {
            device_id: "detect-dev".to_string(),
            name: Some("Detect".to_string()),
            icon: None,
            version: "1.1".to_string(),
            managed_paths: vec!["Music".to_string()],
            synced_items: vec![crate::device::SyncedItem {
                media_role: crate::device::MediaRole::Music,
                jellyfin_id: "song1".to_string(),
                name: "Existing".to_string(),
                album: Some("Album".to_string()),
                artist: None,
                local_path: "Music/existing.mp3".to_string(),
                size_bytes: 1000,
                synced_at: "2026-02-15T10:00:00Z".to_string(),
                original_name: None,
                etag: Some("old-v1".to_string()),
                provider_album_id: Some("album1".to_string()),
                provider_content_type: Some("audio/mpeg".to_string()),
                provider_suffix: Some("mp3".to_string()),
                original_bitrate: None,
                original_container: None,
                track_number: None,
                server_id: None,
            }],
            dirty: false,
            pending_item_ids: vec![],
            basket_items: vec![crate::device::BasketItem {
                id: "album1".to_string(),
                name: "Album".to_string(),
                item_type: "MusicAlbum".to_string(),
                server_id: None,
                artist: None,
                child_count: 2,
                size_ticks: 0,
                size_bytes: 4000,
            }],
            auto_sync_on_connect: false,
            auto_fill: crate::device::AutoFillConfig::default(),
            transcoding_profile_id: None,
            playlists: vec![],
            storage_id: None,
            ..Default::default()
        };
        state
            .device_manager
            .handle_device_detected(
                dir.path().to_path_buf(),
                manifest,
                Arc::new(crate::device_io::MscBackend::new(dir.path().to_path_buf())),
            )
            .await
            .unwrap();

        let result =
            handle_sync_detect_changes(&state, Some(json!({ "syncToken": "1710000000000" })))
                .await
                .expect("changes");
        let changes = result.as_array().expect("changes array");

        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0]["id"], "song2");
        assert_eq!(changes[0]["itemType"], "song");
        assert_eq!(changes[0]["changeType"], "created");
        assert_eq!(changes[0]["providerAlbumId"], "album1");
        assert_eq!(changes[0]["providerSize"], 3000);
        assert_eq!(changes[0]["providerContentType"], "audio/flac");
        assert_eq!(changes[0]["providerSuffix"], "flac");
    }

    #[tokio::test]
    async fn test_rpc_sync_get_device_status_map_no_device() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let device_manager = Arc::new(crate::device::DeviceManager::new(db.clone()));
        let state = Arc::new(AppState {
            jellyfin_client: JellyfinClient::new(),
            server_manager: Arc::new(tokio::sync::RwLock::new(
                crate::server_manager::ServerManager::new(),
            )),
            db: db.clone(),
            device_manager,
            last_connection_check: Arc::new(tokio::sync::Mutex::new(None)),
            size_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            sync_operation_manager: Arc::new(crate::sync::SyncOperationManager::new()),
            last_scrobbler_result: Arc::new(tokio::sync::RwLock::new(None)),
            state_tx: std::sync::mpsc::channel::<crate::DaemonState>().0,
            playback: crate::playback::PlaybackSession::restore(
                Arc::new(crate::db::Database::memory().unwrap()),
                "test-instance".into(),
            ),
        });

        let result = handle_sync_get_device_status_map(&state).await.unwrap();
        let synced_ids = result["syncedItemIds"].as_array().unwrap();
        assert!(synced_ids.is_empty());
    }

    #[tokio::test]
    async fn test_rpc_sync_get_device_status_map_with_synced_items() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let device_manager = Arc::new(crate::device::DeviceManager::new(db.clone()));

        // Simulate device with synced items
        let manifest = crate::device::DeviceManifest {
            device_id: "test-dev".to_string(),
            name: Some("Test".to_string()),
            icon: None,
            version: "1.1".to_string(),
            managed_paths: vec!["Music".to_string()],
            synced_items: vec![
                crate::device::SyncedItem {
                    media_role: crate::device::MediaRole::Music,
                    jellyfin_id: "item-a".to_string(),
                    name: "Track A".to_string(),
                    album: None,
                    artist: None,
                    local_path: "Music/track_a.flac".to_string(),
                    size_bytes: 1000,
                    synced_at: "2026-02-15T10:00:00Z".to_string(),
                    original_name: None,
                    etag: Some("etag-a".to_string()),
                    provider_album_id: None,
                    provider_content_type: None,
                    provider_suffix: None,
                    original_bitrate: None,
                    original_container: None,
                    track_number: None,
                    server_id: None,
                },
                crate::device::SyncedItem {
                    media_role: crate::device::MediaRole::Music,
                    jellyfin_id: "item-b".to_string(),
                    name: "Track B".to_string(),
                    album: None,
                    artist: None,
                    local_path: "Music/track_b.flac".to_string(),
                    size_bytes: 2000,
                    synced_at: "2026-02-15T10:00:00Z".to_string(),
                    original_name: None,
                    etag: Some("etag-b".to_string()),
                    provider_album_id: None,
                    provider_content_type: None,
                    provider_suffix: None,
                    original_bitrate: None,
                    original_container: None,
                    track_number: None,
                    server_id: None,
                },
            ],
            dirty: false,
            pending_item_ids: vec![],
            basket_items: vec![],
            auto_sync_on_connect: false,
            auto_fill: crate::device::AutoFillConfig::default(),
            transcoding_profile_id: None,
            playlists: vec![],
            storage_id: None,
            ..Default::default()
        };

        device_manager
            .handle_device_detected(
                std::path::PathBuf::from("/tmp/test"),
                manifest,
                std::sync::Arc::new(crate::device_io::MscBackend::new(std::path::PathBuf::from(
                    "/tmp/test",
                ))),
            )
            .await
            .unwrap();

        let state = AppState {
            jellyfin_client: JellyfinClient::new(),
            server_manager: Arc::new(tokio::sync::RwLock::new(
                crate::server_manager::ServerManager::new(),
            )),
            db: db.clone(),
            device_manager,
            last_connection_check: Arc::new(tokio::sync::Mutex::new(None)),
            size_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            sync_operation_manager: Arc::new(crate::sync::SyncOperationManager::new()),
            last_scrobbler_result: Arc::new(tokio::sync::RwLock::new(None)),
            state_tx: std::sync::mpsc::channel::<crate::DaemonState>().0,
            playback: crate::playback::PlaybackSession::restore(
                Arc::new(crate::db::Database::memory().unwrap()),
                "test-instance".into(),
            ),
        };

        let result = handle_sync_get_device_status_map(&state).await.unwrap();
        let synced_ids = result["syncedItemIds"].as_array().unwrap();
        assert_eq!(synced_ids.len(), 2);
        assert!(synced_ids.contains(&json!("item-a")));
        assert!(synced_ids.contains(&json!("item-b")));
    }

    // ===== Story 4.4 Tests =====

    #[tokio::test]
    async fn test_rpc_sync_get_resume_state_no_device() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let device_manager = Arc::new(crate::device::DeviceManager::new(db.clone()));
        let state = AppState {
            jellyfin_client: JellyfinClient::new(),
            server_manager: Arc::new(tokio::sync::RwLock::new(
                crate::server_manager::ServerManager::new(),
            )),
            db,
            device_manager,
            last_connection_check: Arc::new(tokio::sync::Mutex::new(None)),
            size_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            sync_operation_manager: Arc::new(crate::sync::SyncOperationManager::new()),
            last_scrobbler_result: Arc::new(tokio::sync::RwLock::new(None)),
            state_tx: std::sync::mpsc::channel::<crate::DaemonState>().0,
            playback: crate::playback::PlaybackSession::restore(
                Arc::new(crate::db::Database::memory().unwrap()),
                "test-instance".into(),
            ),
        };
        let result = handle_sync_get_resume_state(&state).await.unwrap();
        assert_eq!(result["isDirty"], false);
        assert!(result["pendingItemIds"].as_array().unwrap().is_empty());
        assert_eq!(result["cleanedTmpFiles"], 0);
    }

    #[tokio::test]
    async fn test_rpc_sync_get_resume_state_clean_device() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let device_manager = Arc::new(crate::device::DeviceManager::new(db.clone()));

        let dir = tempfile::tempdir().unwrap();
        let manifest = crate::device::DeviceManifest {
            device_id: "clean-dev".to_string(),
            name: Some("Clean".to_string()),
            icon: None,
            version: "1.0".to_string(),
            managed_paths: vec![],
            synced_items: vec![],
            dirty: false,
            pending_item_ids: vec![],
            basket_items: vec![],
            auto_sync_on_connect: false,
            auto_fill: crate::device::AutoFillConfig::default(),
            transcoding_profile_id: None,
            playlists: vec![],
            storage_id: None,
            ..Default::default()
        };
        device_manager
            .handle_device_detected(
                dir.path().to_path_buf(),
                manifest,
                std::sync::Arc::new(crate::device_io::MscBackend::new(dir.path().to_path_buf())),
            )
            .await
            .unwrap();

        let state = AppState {
            jellyfin_client: JellyfinClient::new(),
            server_manager: Arc::new(tokio::sync::RwLock::new(
                crate::server_manager::ServerManager::new(),
            )),
            db,
            device_manager,
            last_connection_check: Arc::new(tokio::sync::Mutex::new(None)),
            size_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            sync_operation_manager: Arc::new(crate::sync::SyncOperationManager::new()),
            last_scrobbler_result: Arc::new(tokio::sync::RwLock::new(None)),
            state_tx: std::sync::mpsc::channel::<crate::DaemonState>().0,
            playback: crate::playback::PlaybackSession::restore(
                Arc::new(crate::db::Database::memory().unwrap()),
                "test-instance".into(),
            ),
        };
        let result = handle_sync_get_resume_state(&state).await.unwrap();
        assert_eq!(result["isDirty"], false);
        assert!(result["pendingItemIds"].as_array().unwrap().is_empty());
        assert_eq!(result["cleanedTmpFiles"], 0);
    }

    #[tokio::test]
    async fn test_rpc_sync_get_resume_state_dirty_device() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let device_manager = Arc::new(crate::device::DeviceManager::new(db.clone()));

        let dir = tempfile::tempdir().unwrap();
        // No .tmp files in Music/ — cleanedTmpFiles should be 0
        tokio::fs::create_dir(dir.path().join("Music"))
            .await
            .unwrap();

        let manifest = crate::device::DeviceManifest {
            device_id: "dirty-dev".to_string(),
            name: Some("Dirty".to_string()),
            icon: None,
            version: "1.0".to_string(),
            managed_paths: vec!["Music".to_string()],
            synced_items: vec![],
            dirty: true,
            pending_item_ids: vec!["id-1".to_string()],
            basket_items: vec![],
            auto_sync_on_connect: false,
            auto_fill: crate::device::AutoFillConfig::default(),
            transcoding_profile_id: None,
            playlists: vec![],
            storage_id: None,
            ..Default::default()
        };
        device_manager
            .handle_device_detected(
                dir.path().to_path_buf(),
                manifest,
                std::sync::Arc::new(crate::device_io::MscBackend::new(dir.path().to_path_buf())),
            )
            .await
            .unwrap();

        let state = AppState {
            jellyfin_client: JellyfinClient::new(),
            server_manager: Arc::new(tokio::sync::RwLock::new(
                crate::server_manager::ServerManager::new(),
            )),
            db,
            device_manager,
            last_connection_check: Arc::new(tokio::sync::Mutex::new(None)),
            size_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            sync_operation_manager: Arc::new(crate::sync::SyncOperationManager::new()),
            last_scrobbler_result: Arc::new(tokio::sync::RwLock::new(None)),
            state_tx: std::sync::mpsc::channel::<crate::DaemonState>().0,
            playback: crate::playback::PlaybackSession::restore(
                Arc::new(crate::db::Database::memory().unwrap()),
                "test-instance".into(),
            ),
        };
        let result = handle_sync_get_resume_state(&state).await.unwrap();
        assert_eq!(result["isDirty"], true);
        let ids = result["pendingItemIds"].as_array().unwrap();
        assert_eq!(ids.len(), 1);
        assert_eq!(ids[0], "id-1");
        assert_eq!(result["cleanedTmpFiles"], 0);
    }

    #[tokio::test]
    async fn test_rpc_get_daemon_state_includes_dirty_manifest_field() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let device_manager = Arc::new(crate::device::DeviceManager::new(db.clone()));

        // No device — dirtyManifest should be false
        let state = AppState {
            jellyfin_client: JellyfinClient::new(),
            server_manager: Arc::new(tokio::sync::RwLock::new(
                crate::server_manager::ServerManager::new(),
            )),
            db: db.clone(),
            device_manager: device_manager.clone(),
            last_connection_check: Arc::new(tokio::sync::Mutex::new(None)),
            size_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            sync_operation_manager: Arc::new(crate::sync::SyncOperationManager::new()),
            last_scrobbler_result: Arc::new(tokio::sync::RwLock::new(None)),
            state_tx: std::sync::mpsc::channel::<crate::DaemonState>().0,
            playback: crate::playback::PlaybackSession::restore(
                Arc::new(crate::db::Database::memory().unwrap()),
                "test-instance".into(),
            ),
        };
        let result = handle_get_daemon_state(&state).await.unwrap();
        assert_eq!(
            result["dirtyManifest"], false,
            "No device → dirtyManifest must be false"
        );

        // Dirty device — dirtyManifest should be true
        let dirty_manifest = crate::device::DeviceManifest {
            device_id: "dirty-dev".to_string(),
            name: Some("Dirty".to_string()),
            icon: None,
            version: "1.0".to_string(),
            managed_paths: vec![],
            synced_items: vec![],
            dirty: true,
            pending_item_ids: vec!["id-1".to_string()],
            basket_items: vec![],
            auto_sync_on_connect: false,
            auto_fill: crate::device::AutoFillConfig::default(),
            transcoding_profile_id: None,
            playlists: vec![],
            storage_id: None,
            ..Default::default()
        };
        device_manager
            .handle_device_detected(
                std::path::PathBuf::from("/tmp/dirty"),
                dirty_manifest,
                std::sync::Arc::new(crate::device_io::MscBackend::new(std::path::PathBuf::from(
                    "/tmp/dirty",
                ))),
            )
            .await
            .unwrap();

        let state2 = AppState {
            jellyfin_client: JellyfinClient::new(),
            server_manager: Arc::new(tokio::sync::RwLock::new(
                crate::server_manager::ServerManager::new(),
            )),
            db,
            device_manager,
            last_connection_check: Arc::new(tokio::sync::Mutex::new(None)),
            size_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            sync_operation_manager: Arc::new(crate::sync::SyncOperationManager::new()),
            last_scrobbler_result: Arc::new(tokio::sync::RwLock::new(None)),
            state_tx: std::sync::mpsc::channel::<crate::DaemonState>().0,
            playback: crate::playback::PlaybackSession::restore(
                Arc::new(crate::db::Database::memory().unwrap()),
                "test-instance".into(),
            ),
        };
        let result2 = handle_get_daemon_state(&state2).await.unwrap();
        assert_eq!(
            result2["dirtyManifest"], true,
            "Dirty device → dirtyManifest must be true"
        );
    }

    #[tokio::test]
    async fn test_rpc_get_daemon_state_includes_pending_device_path() {
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let device_manager = Arc::new(crate::device::DeviceManager::new(db.clone()));

        // No unrecognized device → pendingDevicePath should be null
        let state = AppState {
            jellyfin_client: JellyfinClient::new(),
            server_manager: Arc::new(tokio::sync::RwLock::new(
                crate::server_manager::ServerManager::new(),
            )),
            db: db.clone(),
            device_manager: device_manager.clone(),
            last_connection_check: Arc::new(tokio::sync::Mutex::new(None)),
            size_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            sync_operation_manager: Arc::new(crate::sync::SyncOperationManager::new()),
            last_scrobbler_result: Arc::new(tokio::sync::RwLock::new(None)),
            state_tx: std::sync::mpsc::channel::<crate::DaemonState>().0,
            playback: crate::playback::PlaybackSession::restore(
                Arc::new(crate::db::Database::memory().unwrap()),
                "test-instance".into(),
            ),
        };
        let result = handle_get_daemon_state(&state).await.unwrap();
        assert!(
            result["pendingDevicePath"].is_null(),
            "No unrecognized device → pendingDevicePath must be null"
        );

        // Set an unrecognized device → pendingDevicePath should be present
        device_manager
            .handle_device_unrecognized(
                dir.path().to_path_buf(),
                std::sync::Arc::new(crate::device_io::MscBackend::new(dir.path().to_path_buf())),
                None,
            )
            .await;

        let result2 = handle_get_daemon_state(&state).await.unwrap();
        assert!(
            result2["pendingDevicePath"].is_string(),
            "Unrecognized device → pendingDevicePath must be a string"
        );
        let pending_path = result2["pendingDevicePath"].as_str().unwrap();
        assert!(
            !pending_path.is_empty(),
            "pendingDevicePath must not be empty"
        );
    }

    #[tokio::test]
    async fn test_rpc_sync_calculate_delta_expands_playlist_to_tracks() {
        use mockito::{Matcher, Server};
        let _credentials_guard = credential_test_lock();

        let mut server = Server::new_async().await;
        let url = server.url();
        let token = "test-token-1234567890";

        let _mock_playlist = server
            .mock("GET", "/Items")
            .match_header("Authorization", format!("MediaBrowser Token=\"{}\"", token).as_str())
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("userId".into(), "Me".into()),
                Matcher::UrlEncoded("Ids".into(), "playlist-1".into()),
                Matcher::UrlEncoded("Fields".into(), "MediaSources".into()),
            ]))
            .with_status(200)
            .with_body(r#"{"Items":[{"Id":"playlist-1","Name":"Road Trip","Type":"Playlist","Etag":"pl-etag"}],"TotalRecordCount":1,"StartIndex":0}"#)
            .create_async()
            .await;

        let _mock_playlist_children = server
            .mock("GET", "/Items")
            .match_header("Authorization", format!("MediaBrowser Token=\"{}\"", token).as_str())
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("userId".into(), "Me".into()),
                Matcher::UrlEncoded("ParentId".into(), "playlist-1".into()),
                Matcher::UrlEncoded("IncludeItemTypes".into(), "Audio,MusicVideo".into()),
                Matcher::UrlEncoded("Fields".into(), "MediaSources".into()),
                Matcher::UrlEncoded("Recursive".into(), "true".into()),
            ]))
            .with_status(200)
            .with_body(r#"{"Items":[{"Id":"track-1","Name":"Track 1","Type":"Audio","Album":"Album A","AlbumArtist":"Artist A","RunTimeTicks":2100000000,"MediaSources":[{"Size":12345}],"Etag":"track-etag"}],"TotalRecordCount":1,"StartIndex":0}"#)
            .create_async()
            .await;

        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("config.json");
        crate::api::CredentialManager::set_config_path(config_path);
        crate::api::CredentialManager::save_credentials(&url, token, Some("Me")).unwrap();

        let db = Arc::new(crate::db::Database::memory().unwrap());
        let device_manager = Arc::new(crate::device::DeviceManager::new(db.clone()));
        device_manager
            .handle_device_detected(
                std::path::PathBuf::from("/tmp/dev"),
                crate::device::DeviceManifest {
                    device_id: "dev-1".to_string(),
                    name: Some("Dev 1".to_string()),
                    icon: None,
                    version: "1.0".to_string(),
                    managed_paths: vec![],
                    synced_items: vec![],
                    dirty: false,
                    pending_item_ids: vec![],
                    basket_items: vec![],
                    auto_sync_on_connect: false,
                    auto_fill: crate::device::AutoFillConfig::default(),
                    transcoding_profile_id: None,
                    playlists: vec![],
                    storage_id: None,
                    ..Default::default()
                },
                std::sync::Arc::new(crate::device_io::MscBackend::new(std::path::PathBuf::from(
                    "/tmp/dev",
                ))),
            )
            .await
            .unwrap();

        let state = Arc::new(AppState {
            jellyfin_client: JellyfinClient::new(),
            server_manager: Arc::new(tokio::sync::RwLock::new(
                crate::server_manager::ServerManager::new(),
            )),
            db,
            device_manager,
            last_connection_check: Arc::new(tokio::sync::Mutex::new(None)),
            size_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            sync_operation_manager: Arc::new(crate::sync::SyncOperationManager::new()),
            last_scrobbler_result: Arc::new(tokio::sync::RwLock::new(None)),
            state_tx: std::sync::mpsc::channel::<crate::DaemonState>().0,
            playback: crate::playback::PlaybackSession::restore(
                Arc::new(crate::db::Database::memory().unwrap()),
                "test-instance".into(),
            ),
        });

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "sync_calculate_delta".to_string(),
            params: Some(json!({ "itemIds": ["playlist-1"] })),
            id: json!(1),
        };

        let response = handler(axum::extract::State(state), Json(request)).await.0;
        assert!(
            response.error.is_none(),
            "Unexpected RPC error: {:?}",
            response.error
        );

        let delta: crate::sync::SyncDelta =
            serde_json::from_value(response.result.unwrap()).unwrap();
        assert_eq!(delta.adds.len(), 1);
        assert_eq!(delta.adds[0].jellyfin_id, "track-1");
        assert_eq!(delta.adds[0].name, "Track 1");
        assert_eq!(delta.adds[0].size_bytes, 12345);
        assert_eq!(delta.playlists.len(), 1);
        assert_eq!(delta.playlists[0].jellyfin_id, "playlist-1");
        assert_eq!(delta.playlists[0].name, "Road Trip");
        assert_eq!(delta.playlists[0].tracks.len(), 1);
        assert_eq!(delta.playlists[0].tracks[0].jellyfin_id, "track-1");
        assert_eq!(
            delta.playlists[0].tracks[0].artist.as_deref(),
            Some("Artist A")
        );
        assert_eq!(delta.playlists[0].tracks[0].run_time_seconds, 210);
    }

    #[tokio::test]
    async fn test_rpc_sync_calculate_delta_partial_failure() {
        use mockito::Server;
        let _credentials_guard = credential_test_lock();
        let mut server = Server::new_async().await;
        let url = server.url();
        let token = "test-token";

        // Mock bulk item fetch: returns only item-1; item-2 is absent → partial failure
        let _mock_items = server
            .mock("GET", "/Items")
            .match_header("Authorization", format!("MediaBrowser Token=\"{}\"", token).as_str())
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("userId".into(), "Me".into()),
                mockito::Matcher::UrlEncoded("Fields".into(), "MediaSources".into()),
            ]))
            .with_status(200)
            .with_body(r#"{"Items":[{"Id":"item-1","Name":"Item 1","Type":"Audio","AlbumArtist":"Artist","MediaSources":[{"Size":1000}]}],"TotalRecordCount":1,"StartIndex":0}"#)
            .create_async()
            .await;

        // Setup app state
        // We need to save credentials to the temp config for the RPC handler to pick them up
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("config.json");
        crate::api::CredentialManager::set_config_path(config_path);
        crate::api::CredentialManager::save_credentials(&url, token, Some("Me")).unwrap();

        let db = Arc::new(crate::db::Database::memory().unwrap());
        let device_manager = Arc::new(crate::device::DeviceManager::new(db.clone()));

        // Simulate a connected device
        let manifest = crate::device::DeviceManifest {
            device_id: "dev-1".to_string(),
            name: Some("Dev 1".to_string()),
            icon: None,
            version: "1.0".to_string(),
            managed_paths: vec![],
            synced_items: vec![],
            dirty: false,
            pending_item_ids: vec![],
            basket_items: vec![],
            auto_sync_on_connect: false,
            auto_fill: crate::device::AutoFillConfig::default(),
            transcoding_profile_id: None,
            playlists: vec![],
            storage_id: None,
            ..Default::default()
        };
        device_manager
            .handle_device_detected(
                std::path::PathBuf::from("/tmp/dev"),
                manifest,
                std::sync::Arc::new(crate::device_io::MscBackend::new(std::path::PathBuf::from(
                    "/tmp/dev",
                ))),
            )
            .await
            .unwrap();

        let state = Arc::new(AppState {
            jellyfin_client: JellyfinClient::new(),
            server_manager: Arc::new(tokio::sync::RwLock::new(
                crate::server_manager::ServerManager::new(),
            )),
            db,
            device_manager,
            last_connection_check: Arc::new(tokio::sync::Mutex::new(None)),
            size_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            sync_operation_manager: Arc::new(crate::sync::SyncOperationManager::new()),
            last_scrobbler_result: Arc::new(tokio::sync::RwLock::new(None)),
            state_tx: std::sync::mpsc::channel::<crate::DaemonState>().0,
            playback: crate::playback::PlaybackSession::restore(
                Arc::new(crate::db::Database::memory().unwrap()),
                "test-instance".into(),
            ),
        });

        // Make request
        let params = json!({
            "itemIds": ["item-1", "item-2"]
        });

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "sync_calculate_delta".to_string(),
            params: Some(params),
            id: json!(1),
        };

        let response = handler(axum::extract::State(state), Json(request)).await;

        // Assert ERROR, not partial success
        let response = response.0; // Unwrap Json wrapper
        assert!(response.result.is_none());
        assert!(response.error.is_some());
        let err = response.error.unwrap();
        assert_eq!(err.code, ERR_CONNECTION_FAILED);
        assert!(err.message.contains("Sync aborted"));
        // buffer_unordered makes order non-deterministic, so either item could fail first
        assert!(
            err.message.contains("item-1") || err.message.contains("item-2"),
            "Expected error to mention an item ID, got: {}",
            err.message
        );
    }

    // ===== Story 2.6 Tests =====

    #[tokio::test]
    async fn test_rpc_device_initialize_missing_params() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let device_manager = Arc::new(crate::device::DeviceManager::new(db.clone()));
        let state = AppState {
            jellyfin_client: JellyfinClient::new(),
            server_manager: Arc::new(tokio::sync::RwLock::new(
                crate::server_manager::ServerManager::new(),
            )),
            db,
            device_manager,
            last_connection_check: Arc::new(tokio::sync::Mutex::new(None)),
            size_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            sync_operation_manager: Arc::new(crate::sync::SyncOperationManager::new()),
            last_scrobbler_result: Arc::new(tokio::sync::RwLock::new(None)),
            state_tx: std::sync::mpsc::channel::<crate::DaemonState>().0,
            playback: crate::playback::PlaybackSession::restore(
                Arc::new(crate::db::Database::memory().unwrap()),
                "test-instance".into(),
            ),
        };

        // No params → ERR_INVALID_PARAMS
        let res = handle_device_initialize(&state, None).await;
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().code, ERR_INVALID_PARAMS);

        // Missing profileId → ERR_INVALID_PARAMS
        let res = handle_device_initialize(&state, Some(json!({ "folderPath": "" }))).await;
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().code, ERR_INVALID_PARAMS);

        // Missing folderPath → ERR_INVALID_PARAMS
        let res = handle_device_initialize(&state, Some(json!({ "profileId": "user-1" }))).await;
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().code, ERR_INVALID_PARAMS);
    }

    #[tokio::test]
    async fn test_rpc_device_initialize_no_unrecognized_device() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let device_manager = Arc::new(crate::device::DeviceManager::new(db.clone()));
        let state = AppState {
            jellyfin_client: JellyfinClient::new(),
            server_manager: Arc::new(tokio::sync::RwLock::new(
                crate::server_manager::ServerManager::new(),
            )),
            db,
            device_manager,
            last_connection_check: Arc::new(tokio::sync::Mutex::new(None)),
            size_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            sync_operation_manager: Arc::new(crate::sync::SyncOperationManager::new()),
            last_scrobbler_result: Arc::new(tokio::sync::RwLock::new(None)),
            state_tx: std::sync::mpsc::channel::<crate::DaemonState>().0,
            playback: crate::playback::PlaybackSession::restore(
                Arc::new(crate::db::Database::memory().unwrap()),
                "test-instance".into(),
            ),
        };

        // No unrecognized device registered → ERR_INVALID_PARAMS (caught before reaching storage)
        let params = json!({ "pendingId": "missing", "observedDestinationRevision": "0", "folderPath": "", "profileId": "user-1", "name": "My Device" });
        let res = handle_device_initialize(&state, Some(params)).await;
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().code, ERR_INVALID_PARAMS);
    }

    #[tokio::test]
    async fn test_rpc_device_initialize_success_root() {
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let device_manager = Arc::new(crate::device::DeviceManager::new(db.clone()));

        // Simulate an unrecognized device
        device_manager
            .handle_device_unrecognized(
                dir.path().to_path_buf(),
                std::sync::Arc::new(crate::device_io::MscBackend::new(dir.path().to_path_buf())),
                None,
            )
            .await;

        let state = AppState {
            jellyfin_client: JellyfinClient::new(),
            server_manager: Arc::new(tokio::sync::RwLock::new(
                crate::server_manager::ServerManager::new(),
            )),
            db: db.clone(),
            device_manager: device_manager.clone(),
            last_connection_check: Arc::new(tokio::sync::Mutex::new(None)),
            size_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            sync_operation_manager: Arc::new(crate::sync::SyncOperationManager::new()),
            last_scrobbler_result: Arc::new(tokio::sync::RwLock::new(None)),
            state_tx: std::sync::mpsc::channel::<crate::DaemonState>().0,
            playback: crate::playback::PlaybackSession::restore(
                Arc::new(crate::db::Database::memory().unwrap()),
                "test-instance".into(),
            ),
        };

        // Initialize with empty folderPath (device root)
        let destinations = device_manager.get_destination_snapshot().await;
        let pending_id = device_manager.get_pending_devices_snapshot().await[0]
            .pending_id
            .clone();
        let params = json!({ "pendingId": pending_id, "observedDestinationRevision": destinations.revision.to_string(), "folderPath": "", "profileId": "user-abc", "name": "My Device" });
        let res = handle_device_initialize(&state, Some(params))
            .await
            .unwrap();

        assert_eq!(res["status"], "success");
        let managed_paths = res["data"]["managedPaths"].as_array().unwrap();
        assert!(
            managed_paths.is_empty(),
            "Root init should have no managed paths"
        );

        // Verify manifest was written to disk
        let manifest_path = dir.path().join(".hifimule.json");
        assert!(manifest_path.exists(), ".hifimule.json must exist");

        // Verify device is now recognized
        let current_device = device_manager.get_current_device().await;
        assert!(current_device.is_some(), "Device should now be recognized");
        assert!(
            device_manager
                .get_unrecognized_device_path()
                .await
                .is_none()
        );

        // Verify DB mapping was stored
        let device_id = res["data"]["deviceId"].as_str().unwrap();
        let mapping = db.get_device_mapping(device_id).unwrap();
        assert!(mapping.is_some());
        assert_eq!(
            mapping.unwrap().jellyfin_user_id,
            Some("user-abc".to_string())
        );
    }

    #[tokio::test]
    async fn test_rpc_device_initialize_success_subfolder() {
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let device_manager = Arc::new(crate::device::DeviceManager::new(db.clone()));

        device_manager
            .handle_device_unrecognized(
                dir.path().to_path_buf(),
                std::sync::Arc::new(crate::device_io::MscBackend::new(dir.path().to_path_buf())),
                None,
            )
            .await;

        let state = AppState {
            jellyfin_client: JellyfinClient::new(),
            server_manager: Arc::new(tokio::sync::RwLock::new(
                crate::server_manager::ServerManager::new(),
            )),
            db: db.clone(),
            device_manager: device_manager.clone(),
            last_connection_check: Arc::new(tokio::sync::Mutex::new(None)),
            size_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            sync_operation_manager: Arc::new(crate::sync::SyncOperationManager::new()),
            last_scrobbler_result: Arc::new(tokio::sync::RwLock::new(None)),
            state_tx: std::sync::mpsc::channel::<crate::DaemonState>().0,
            playback: crate::playback::PlaybackSession::restore(
                Arc::new(crate::db::Database::memory().unwrap()),
                "test-instance".into(),
            ),
        };

        // Initialize with a subfolder
        let destinations = device_manager.get_destination_snapshot().await;
        let pending_id = device_manager.get_pending_devices_snapshot().await[0]
            .pending_id
            .clone();
        let params = json!({ "pendingId": pending_id, "observedDestinationRevision": destinations.revision.to_string(), "folderPath": "Music", "profileId": "user-xyz", "name": "My Device" });
        let res = handle_device_initialize(&state, Some(params))
            .await
            .unwrap();

        assert_eq!(res["status"], "success");
        let managed_paths = res["data"]["managedPaths"].as_array().unwrap();
        assert_eq!(managed_paths.len(), 1);
        assert_eq!(managed_paths[0], "Music");

        // Verify Music folder was created on device
        let music_folder = dir.path().join("Music");
        assert!(music_folder.exists(), "Music subfolder should be created");

        // Verify manifest on disk
        let content = tokio::fs::read_to_string(dir.path().join(".hifimule.json"))
            .await
            .unwrap();
        let manifest: crate::device::DeviceManifest = serde_json::from_str(&content).unwrap();
        assert_eq!(manifest.managed_paths, vec!["Music".to_string()]);
        assert!(manifest.synced_items.is_empty());
        assert!(!manifest.dirty);
    }

    #[tokio::test]
    async fn test_rpc_device_set_auto_sync_on_connect() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let device_id = "auto-sync-rpc-test";
        db.upsert_device_mapping(device_id, Some("Test"), Some("user-1"), None)
            .unwrap();

        let device_manager = Arc::new(crate::device::DeviceManager::new(db.clone()));

        // Simulate device connected with a tempdir for manifest writes
        let dir = tempfile::tempdir().unwrap();
        let manifest = crate::device::DeviceManifest {
            device_id: device_id.to_string(),
            name: Some("Test".to_string()),
            icon: None,
            version: "1.0".to_string(),
            managed_paths: vec![],
            synced_items: vec![],
            dirty: false,
            pending_item_ids: vec![],
            basket_items: vec![],
            auto_sync_on_connect: false,
            auto_fill: crate::device::AutoFillConfig::default(),
            transcoding_profile_id: None,
            playlists: vec![],
            storage_id: None,
            ..Default::default()
        };
        crate::device::write_manifest(
            std::sync::Arc::new(crate::device_io::MscBackend::new(dir.path().to_path_buf())),
            &manifest,
        )
        .await
        .unwrap();
        device_manager
            .handle_device_detected(
                dir.path().to_path_buf(),
                manifest,
                std::sync::Arc::new(crate::device_io::MscBackend::new(dir.path().to_path_buf())),
            )
            .await
            .unwrap();

        let state = AppState {
            jellyfin_client: JellyfinClient::new(),
            server_manager: Arc::new(tokio::sync::RwLock::new(
                crate::server_manager::ServerManager::new(),
            )),
            db: db.clone(),
            device_manager: device_manager.clone(),
            last_connection_check: Arc::new(tokio::sync::Mutex::new(None)),
            size_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            sync_operation_manager: Arc::new(crate::sync::SyncOperationManager::new()),
            last_scrobbler_result: Arc::new(tokio::sync::RwLock::new(None)),
            state_tx: std::sync::mpsc::channel::<crate::DaemonState>().0,
            playback: crate::playback::PlaybackSession::restore(
                Arc::new(crate::db::Database::memory().unwrap()),
                "test-instance".into(),
            ),
        };

        // Enable auto-sync via RPC
        let params = Some(json!({
            "deviceId": device_id,
            "enabled": true
        }));
        let result = handle_device_set_auto_sync_on_connect(&state, params)
            .await
            .unwrap();
        assert_eq!(result["status"], "success");
        assert_eq!(result["autoSyncOnConnect"], true);

        // Verify DB was updated
        let mapping = db.get_device_mapping(device_id).unwrap().unwrap();
        assert!(mapping.auto_sync_on_connect);

        // Verify manifest was updated on disk
        let content = tokio::fs::read_to_string(dir.path().join(".hifimule.json"))
            .await
            .unwrap();
        let on_disk: crate::device::DeviceManifest = serde_json::from_str(&content).unwrap();
        assert!(on_disk.auto_sync_on_connect);

        // Verify in-memory manifest was updated
        let in_memory = device_manager.get_current_device().await.unwrap();
        assert!(in_memory.auto_sync_on_connect);

        // Disable auto-sync via RPC
        let params = Some(json!({
            "deviceId": device_id,
            "enabled": false
        }));
        let result = handle_device_set_auto_sync_on_connect(&state, params)
            .await
            .unwrap();
        assert_eq!(result["autoSyncOnConnect"], false);

        // Verify DB disabled
        let mapping = db.get_device_mapping(device_id).unwrap().unwrap();
        assert!(!mapping.auto_sync_on_connect);
    }

    #[tokio::test]
    async fn test_rpc_get_daemon_state_includes_auto_sync_field() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let device_id = "auto-state-test";
        db.upsert_device_mapping(device_id, Some("Test"), Some("user-1"), None)
            .unwrap();

        let device_manager = Arc::new(crate::device::DeviceManager::new(db.clone()));

        let manifest = crate::device::DeviceManifest {
            device_id: device_id.to_string(),
            name: Some("Test".to_string()),
            icon: None,
            version: "1.0".to_string(),
            managed_paths: vec![],
            synced_items: vec![],
            dirty: false,
            pending_item_ids: vec![],
            basket_items: vec![],
            auto_sync_on_connect: true,
            auto_fill: crate::device::AutoFillConfig::default(),
            transcoding_profile_id: None,
            playlists: vec![],
            storage_id: None,
            ..Default::default()
        };
        device_manager
            .handle_device_detected(
                std::path::PathBuf::from("/tmp/auto-state"),
                manifest,
                std::sync::Arc::new(crate::device_io::MscBackend::new(std::path::PathBuf::from(
                    "/tmp/auto-state",
                ))),
            )
            .await
            .unwrap();

        let state = AppState {
            jellyfin_client: JellyfinClient::new(),
            server_manager: Arc::new(tokio::sync::RwLock::new(
                crate::server_manager::ServerManager::new(),
            )),
            db,
            device_manager,
            last_connection_check: Arc::new(tokio::sync::Mutex::new(None)),
            size_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            sync_operation_manager: Arc::new(crate::sync::SyncOperationManager::new()),
            last_scrobbler_result: Arc::new(tokio::sync::RwLock::new(None)),
            state_tx: std::sync::mpsc::channel::<crate::DaemonState>().0,
            playback: crate::playback::PlaybackSession::restore(
                Arc::new(crate::db::Database::memory().unwrap()),
                "test-instance".into(),
            ),
        };

        let result = handle_get_daemon_state(&state).await.unwrap();
        assert_eq!(
            result["autoSyncOnConnect"], true,
            "autoSyncOnConnect should be true for device with flag enabled"
        );
    }

    #[tokio::test]
    async fn test_rpc_get_daemon_state_includes_active_operation_id() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let device_manager = Arc::new(crate::device::DeviceManager::new(db.clone()));

        // No running operation → activeOperationId should be null
        let state = AppState {
            jellyfin_client: JellyfinClient::new(),
            server_manager: Arc::new(tokio::sync::RwLock::new(
                crate::server_manager::ServerManager::new(),
            )),
            db: db.clone(),
            device_manager: device_manager.clone(),
            last_connection_check: Arc::new(tokio::sync::Mutex::new(None)),
            size_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            sync_operation_manager: Arc::new(crate::sync::SyncOperationManager::new()),
            last_scrobbler_result: Arc::new(tokio::sync::RwLock::new(None)),
            state_tx: std::sync::mpsc::channel::<crate::DaemonState>().0,
            playback: crate::playback::PlaybackSession::restore(
                Arc::new(crate::db::Database::memory().unwrap()),
                "test-instance".into(),
            ),
        };
        let result = handle_get_daemon_state(&state).await.unwrap();
        assert_eq!(
            result["activeOperationId"],
            serde_json::Value::Null,
            "No running operation → activeOperationId must be null"
        );
        assert_eq!(result["syncPipelineActive"], false);
        assert_eq!(result["serverType"], serde_json::Value::Null);
        assert_eq!(result["serverVersion"], serde_json::Value::Null);

        let pipeline_guard = state.sync_operation_manager.try_start_pipeline().unwrap();
        let result_preparing = handle_get_daemon_state(&state).await.unwrap();
        assert_eq!(
            result_preparing["activeOperationId"],
            serde_json::Value::Null
        );
        assert_eq!(result_preparing["syncPipelineActive"], true);
        drop(pipeline_guard);

        state
            .server_manager
            .write()
            .await
            .set_test_provider(Arc::new(
                crate::providers::jellyfin::JellyfinProvider::new_with_version(
                    JellyfinClient::new(),
                    "http://localhost",
                    "jellyfin-token-12345",
                    "user1",
                    Some("10.9.0".to_string()),
                ),
            ));

        let result = handle_get_daemon_state(&state).await.unwrap();
        assert_eq!(result["serverConnected"], true);
        assert_eq!(result["serverType"], "jellyfin");
        assert_eq!(result["serverVersion"], "10.9.0");

        // Running operation → activeOperationId should be the operation UUID
        let manager = Arc::new(crate::sync::SyncOperationManager::new());
        let op_id = "test-uuid-1234".to_string();
        manager.create_operation(op_id.clone(), 5).await;

        let state2 = AppState {
            jellyfin_client: JellyfinClient::new(),
            server_manager: Arc::new(tokio::sync::RwLock::new(
                crate::server_manager::ServerManager::new(),
            )),
            db,
            device_manager,
            last_connection_check: Arc::new(tokio::sync::Mutex::new(None)),
            size_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            sync_operation_manager: manager,
            last_scrobbler_result: Arc::new(tokio::sync::RwLock::new(None)),
            state_tx: std::sync::mpsc::channel::<crate::DaemonState>().0,
            playback: crate::playback::PlaybackSession::restore(
                Arc::new(crate::db::Database::memory().unwrap()),
                "test-instance".into(),
            ),
        };
        let result2 = handle_get_daemon_state(&state2).await.unwrap();
        assert_eq!(
            result2["activeOperationId"],
            serde_json::Value::String(op_id),
            "Running operation → activeOperationId must be the operation UUID"
        );
    }

    // ===== Story 2.7: device.list and device.select tests =====

    fn make_app_state_for_device_tests() -> Arc<AppState> {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let device_manager = Arc::new(crate::device::DeviceManager::new(db.clone()));
        Arc::new(AppState {
            jellyfin_client: JellyfinClient::new(),
            server_manager: Arc::new(tokio::sync::RwLock::new(
                crate::server_manager::ServerManager::new(),
            )),
            db,
            device_manager,
            last_connection_check: Arc::new(tokio::sync::Mutex::new(None)),
            size_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            sync_operation_manager: Arc::new(crate::sync::SyncOperationManager::new()),
            last_scrobbler_result: Arc::new(tokio::sync::RwLock::new(None)),
            state_tx: std::sync::mpsc::channel::<crate::DaemonState>().0,
            playback: crate::playback::PlaybackSession::restore(
                Arc::new(crate::db::Database::memory().unwrap()),
                "test-instance".into(),
            ),
        })
    }

    #[tokio::test]
    async fn test_device_list_returns_connected_devices() {
        use tempfile::tempdir;
        let dir1 = tempdir().unwrap();
        let dir2 = tempdir().unwrap();
        let path1 = dir1.path().to_path_buf();
        let path2 = dir2.path().to_path_buf();

        let state = make_app_state_for_device_tests();

        let manifest1 = crate::device::DeviceManifest {
            device_id: "dev-list-1".to_string(),
            name: Some("DeviceA".to_string()),
            icon: None,
            version: "1.0".to_string(),
            managed_paths: vec![],
            synced_items: vec![],
            dirty: false,
            pending_item_ids: vec![],
            basket_items: vec![],
            auto_sync_on_connect: false,
            auto_fill: crate::device::AutoFillConfig::default(),
            transcoding_profile_id: None,
            playlists: vec![],
            storage_id: None,
            ..Default::default()
        };
        let manifest2 = crate::device::DeviceManifest {
            device_id: "dev-list-2".to_string(),
            name: Some("DeviceB".to_string()),
            icon: None,
            version: "1.0".to_string(),
            managed_paths: vec![],
            synced_items: vec![],
            dirty: false,
            pending_item_ids: vec![],
            basket_items: vec![],
            auto_sync_on_connect: false,
            auto_fill: crate::device::AutoFillConfig::default(),
            transcoding_profile_id: None,
            playlists: vec![],
            storage_id: None,
            ..Default::default()
        };

        state
            .device_manager
            .handle_device_detected(
                path1.clone(),
                manifest1,
                std::sync::Arc::new(crate::device_io::MscBackend::new(path1)),
            )
            .await
            .unwrap();
        state
            .device_manager
            .handle_device_detected(
                path2.clone(),
                manifest2,
                std::sync::Arc::new(crate::device_io::MscBackend::new(path2)),
            )
            .await
            .unwrap();

        let result = handle_device_list(&state).await.unwrap();
        let data = result["data"].as_array().unwrap();
        assert_eq!(
            data.len(),
            2,
            "device.list must return all connected devices"
        );

        let ids: Vec<&str> = data
            .iter()
            .map(|d| d["deviceId"].as_str().unwrap())
            .collect();
        assert!(ids.contains(&"dev-list-1"));
        assert!(ids.contains(&"dev-list-2"));
    }

    #[tokio::test]
    async fn test_device_select_valid_path_returns_ok() {
        use tempfile::tempdir;
        let dir1 = tempdir().unwrap();
        let dir2 = tempdir().unwrap();
        let path1 = dir1.path().to_path_buf();
        let path2 = dir2.path().to_path_buf();

        let state = make_app_state_for_device_tests();

        let make_manifest = |id: &str| crate::device::DeviceManifest {
            device_id: id.to_string(),
            name: None,
            icon: None,
            version: "1.0".to_string(),
            managed_paths: vec![],
            synced_items: vec![],
            dirty: false,
            pending_item_ids: vec![],
            basket_items: vec![],
            auto_sync_on_connect: false,
            auto_fill: crate::device::AutoFillConfig::default(),
            transcoding_profile_id: None,
            playlists: vec![],
            storage_id: None,
            ..Default::default()
        };

        state
            .device_manager
            .handle_device_detected(
                path1.clone(),
                make_manifest("sel-dev-1"),
                std::sync::Arc::new(crate::device_io::MscBackend::new(path1.clone())),
            )
            .await
            .unwrap();
        state
            .device_manager
            .handle_device_detected(
                path2.clone(),
                make_manifest("sel-dev-2"),
                std::sync::Arc::new(crate::device_io::MscBackend::new(path2.clone())),
            )
            .await
            .unwrap();

        // Switch to path2
        let params = Some(json!({ "path": path2.to_string_lossy() }));
        let result = handle_device_select(&state, params).await.unwrap();
        assert_eq!(result["status"], "success");
        assert_eq!(result["data"]["ok"], true);

        let selected = state.device_manager.get_current_device_path().await;
        assert_eq!(selected, Some(path2));
    }

    #[tokio::test]
    async fn test_device_select_unknown_path_returns_error() {
        let state = make_app_state_for_device_tests();

        let params = Some(json!({ "path": "/nonexistent/path/device" }));
        let err = handle_device_select(&state, params).await.unwrap_err();
        assert_eq!(err.code, 404, "Unknown path must return 404 error");
        assert!(err.message.contains("not connected"));
    }

    #[tokio::test]
    async fn test_rpc_daemon_health() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let device_manager = Arc::new(crate::device::DeviceManager::new(db.clone()));
        let state = Arc::new(AppState {
            jellyfin_client: JellyfinClient::new(),
            server_manager: Arc::new(tokio::sync::RwLock::new(
                crate::server_manager::ServerManager::new(),
            )),
            db,
            device_manager,
            last_connection_check: Arc::new(tokio::sync::Mutex::new(None)),
            size_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            sync_operation_manager: Arc::new(crate::sync::SyncOperationManager::new()),
            last_scrobbler_result: Arc::new(tokio::sync::RwLock::new(None)),
            state_tx: std::sync::mpsc::channel::<crate::DaemonState>().0,
            playback: crate::playback::PlaybackSession::restore(
                Arc::new(crate::db::Database::memory().unwrap()),
                "test-instance".into(),
            ),
        });

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "daemon.health".to_string(),
            params: None,
            id: json!(1),
        };

        let response = handler(axum::extract::State(state), Json(request)).await;
        assert!(
            response.error.is_none(),
            "daemon.health must not return an error"
        );
        assert!(
            response.result.is_some(),
            "daemon.health must return a result"
        );
        assert_eq!(
            response.result.as_ref().unwrap()["data"]["status"],
            "ok",
            "daemon.health result must be {{ data: {{ status: ok }} }}"
        );
        assert_eq!(
            response.result.as_ref().unwrap()["data"]["protocolVersion"],
            hifimule_lifecycle::PROTOCOL_VERSION
        );
        assert!(response.result.as_ref().unwrap()["data"]["instanceId"].is_string());
        assert!(response.result.as_ref().unwrap()["data"]["pid"].is_number());
        assert_eq!(
            response.result.as_ref().unwrap()["data"]["daemonVersion"],
            env!("CARGO_PKG_VERSION")
        );
    }

    #[tokio::test]
    async fn production_router_rejects_missing_token_before_rpc_dispatch() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let device_manager = Arc::new(crate::device::DeviceManager::new(db.clone()));
        let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let descriptor = hifimule_lifecycle::OwnerDescriptor {
            schema_version: 1,
            protocol_version: 1,
            instance_id: uuid::Uuid::new_v4().to_string(),
            pid: std::process::id(),
            port,
            token: "ab".repeat(32),
            launch_generation: "0".to_string(),
        };
        let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let task_shutdown = shutdown.clone();
        let playback =
            crate::playback::PlaybackSession::restore(db.clone(), descriptor.instance_id.clone());
        let task = tokio::spawn(run_server(
            RpcServerConfig {
                listener,
                descriptor: descriptor.clone(),
                ready_tx,
                shutdown: task_shutdown,
            },
            db,
            device_manager,
            Arc::new(tokio::sync::RwLock::new(None)),
            std::sync::mpsc::channel::<crate::DaemonState>().0,
            Arc::new(crate::sync::SyncOperationManager::new()),
            playback,
            crate::playback::native::NativeBridge::default(),
        ));
        tokio::task::spawn_blocking(move || {
            ready_rx.recv_timeout(std::time::Duration::from_secs(2))
        })
        .await
        .unwrap()
        .unwrap()
        .unwrap();
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let body = json!({"jsonrpc":"2.0","method":"daemon.health","params":{},"id":1});
        let unauthorized = client
            .post(format!("http://127.0.0.1:{port}"))
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), http::StatusCode::UNAUTHORIZED);
        let invalid = client
            .post(format!("http://127.0.0.1:{port}"))
            .bearer_auth("wrong-token")
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(invalid.status(), http::StatusCode::UNAUTHORIZED);
        let image_unauthorized = client
            .get(format!("http://127.0.0.1:{port}/jellyfin/image/example"))
            .send()
            .await
            .unwrap();
        assert_eq!(image_unauthorized.status(), http::StatusCode::UNAUTHORIZED);
        let authorized = client
            .post(format!("http://127.0.0.1:{port}"))
            .bearer_auth(&descriptor.token)
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(authorized.status(), http::StatusCode::OK);
        let playback_body = json!({"jsonrpc":"2.0","method":"playback.getSession","params":{"schemaVersion":1},"id":2});
        let playback_response = client
            .post(format!("http://127.0.0.1:{port}"))
            .bearer_auth(&descriptor.token)
            .json(&playback_body)
            .send()
            .await
            .unwrap();
        assert_eq!(playback_response.status(), http::StatusCode::OK);
        let playback_json: Value = playback_response.json().await.unwrap();
        assert_eq!(playback_json["result"]["data"]["schemaVersion"], 1);
        assert_eq!(
            playback_json["result"]["data"]["instanceId"],
            descriptor.instance_id
        );
        let playback_data = &playback_json["result"]["data"];
        let apply_body = json!({
            "jsonrpc":"2.0",
            "method":"playback.applySession",
            "params":{
                "schemaVersion":1,
                "instanceId":playback_data["instanceId"],
                "sessionId":playback_data["sessionId"],
                "commandId":uuid::Uuid::new_v4().to_string(),
                "expectedQueueRevision":playback_data["queueRevision"],
                "operation":{"type":"replaceQueue","sources":[{"serverId":"offline","trackId":"one"},{"serverId":"offline","trackId":"two"}]}
            },
            "id":3
        });
        let applied: Value = client
            .post(format!("http://127.0.0.1:{port}"))
            .bearer_auth(&descriptor.token)
            .json(&apply_body)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(applied["result"]["data"]["queueRevision"], "1");
        let removed_id =
            applied["result"]["data"]["assignedOccurrences"][1]["occurrenceId"].clone();
        let initial_current_id =
            applied["result"]["data"]["assignedOccurrences"][0]["occurrenceId"].clone();
        let mut edit_body = apply_body.clone();
        edit_body["params"]["commandId"] = json!(uuid::Uuid::new_v4().to_string());
        edit_body["params"]["expectedQueueRevision"] = json!("1");
        edit_body["params"]["operation"] =
            json!({"type":"appendQueue","sources":[{"serverId":"offline","trackId":"two"}]});
        for token in [None, Some("wrong-token")] {
            let request = client
                .post(format!("http://127.0.0.1:{port}"))
                .json(&edit_body);
            let request = if let Some(token) = token {
                request.bearer_auth(token)
            } else {
                request
            };
            assert_eq!(
                request.send().await.unwrap().status(),
                http::StatusCode::UNAUTHORIZED
            );
        }
        let appended: Value = client
            .post(format!("http://127.0.0.1:{port}"))
            .bearer_auth(&descriptor.token)
            .json(&edit_body)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let selected_id =
            appended["result"]["data"]["assignedOccurrences"][0]["occurrenceId"].clone();
        assert_eq!(
            appended["result"]["data"]["queueRevision"], "2",
            "{appended}"
        );
        assert_ne!(
            selected_id, removed_id,
            "repeated source must create a distinct occurrence"
        );
        let replayed: Value = client
            .post(format!("http://127.0.0.1:{port}"))
            .bearer_auth(&descriptor.token)
            .json(&edit_body)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(
            replayed["result"]["data"]["assignedOccurrences"],
            appended["result"]["data"]["assignedOccurrences"]
        );
        assert_eq!(
            replayed["result"]["data"]["queueRevision"], "2",
            "receipt retry must not append twice"
        );
        edit_body["params"]["commandId"] = json!(uuid::Uuid::new_v4().to_string());
        edit_body["params"]["expectedQueueRevision"] = json!("2");
        edit_body["params"]["operation"] = json!({"type":"moveUpcoming","occurrenceId":selected_id.clone(),"beforeOccurrenceId":removed_id.clone()});
        let moved: Value = client
            .post(format!("http://127.0.0.1:{port}"))
            .bearer_auth(&descriptor.token)
            .json(&edit_body)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(moved["result"]["data"]["queueRevision"], "3");
        let reordered: Value = client
            .post(format!("http://127.0.0.1:{port}"))
            .bearer_auth(&descriptor.token)
            .json(&playback_body)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let reordered_ids: Vec<_> = reordered["result"]["data"]["occurrences"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["occurrenceId"].clone())
            .collect();
        assert_eq!(
            reordered_ids,
            [
                initial_current_id.clone(),
                selected_id.clone(),
                removed_id.clone()
            ]
        );
        edit_body["params"]["commandId"] = json!(uuid::Uuid::new_v4().to_string());
        edit_body["params"]["expectedQueueRevision"] = json!("3");
        edit_body["params"]["operation"] =
            json!({"type":"removeUpcoming","occurrenceIds":[removed_id]});
        let removed: Value = client
            .post(format!("http://127.0.0.1:{port}"))
            .bearer_auth(&descriptor.token)
            .json(&edit_body)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(removed["result"]["data"]["queueRevision"], "4");
        let edited_snapshot: Value = client
            .post(format!("http://127.0.0.1:{port}"))
            .bearer_auth(&descriptor.token)
            .json(&playback_body)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let edited = &edited_snapshot["result"]["data"];
        assert_eq!(edited["mainCurrent"]["occurrenceId"], initial_current_id);
        assert_eq!(edited["positionMs"], 0);
        assert_eq!(
            edited["state"], "paused",
            "queue edits must not start output"
        );
        assert_eq!(edited["occurrences"].as_array().unwrap().len(), 2);
        assert_eq!(edited["occurrences"][1]["occurrenceId"], selected_id);
        assert_eq!(
            edited["occurrences"][1]["source"],
            json!({"serverId":"offline","trackId":"two"})
        );
        let mut remove_current = edit_body.clone();
        remove_current["params"]["commandId"] = json!(uuid::Uuid::new_v4().to_string());
        remove_current["params"]["expectedQueueRevision"] = json!("4");
        remove_current["params"]["operation"] =
            json!({"type":"removeUpcoming","occurrenceIds":[initial_current_id]});
        let rejected: Value = client
            .post(format!("http://127.0.0.1:{port}"))
            .bearer_auth(&descriptor.token)
            .json(&remove_current)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(rejected["error"]["data"]["code"], "OCCURRENCE_NOT_UPCOMING");
        assert_eq!(rejected["error"]["data"]["queueRevision"], "4");
        let mut select_body = apply_body.clone();
        select_body["params"]["commandId"] = json!(uuid::Uuid::new_v4().to_string());
        select_body["params"]["expectedQueueRevision"] = json!("4");
        select_body["params"]["operation"] =
            json!({"type":"selectCurrent","occurrenceId":selected_id.clone()});
        let selected: Value = client
            .post(format!("http://127.0.0.1:{port}"))
            .bearer_auth(&descriptor.token)
            .json(&select_body)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(
            selected["result"]["data"]["queueRevision"], "4",
            "{selected}"
        );
        let snapshot: Value = client
            .post(format!("http://127.0.0.1:{port}"))
            .bearer_auth(&descriptor.token)
            .json(&playback_body)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(
            snapshot["result"]["data"]["current"]["occurrenceId"],
            selected_id
        );
        shutdown.store(true, std::sync::atomic::Ordering::Release);
        task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn subsonic_get_views_returns_playlists_collection_with_correct_collection_type() {
        let _lock = credential_test_lock();
        let temp_dir = tempfile::tempdir().unwrap();
        CredentialManager::set_config_path(temp_dir.path().join("missing-config.json"));

        let mut server = mockito::Server::new_async().await;
        let _ping = server
            .mock("GET", "/rest/ping.view")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"subsonic-response":{"status":"ok","version":"1.16.1"}}"#)
            .expect(1)
            .create_async()
            .await;

        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);
        handle_server_connect(
            &state,
            Some(json!({
                "url": server.url(),
                "serverType": "subsonic",
                "username": "subsonic-user",
                "password": "subsonic-password"
            })),
        )
        .await
        .expect("connect");

        let views = handle_jellyfin_get_views(&state, None)
            .await
            .expect("views should come from active subsonic provider");

        assert_eq!(views.as_array().unwrap().len(), 2, "should have two views");

        let all_view = views
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["Id"] == "all")
            .expect("should have 'all' view");
        assert_eq!(all_view["CollectionType"], "music");
        assert_eq!(all_view["Type"], "CollectionFolder");

        let playlists_view = views
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["Id"] == "playlists")
            .expect("should have 'playlists' view");
        assert_eq!(playlists_view["CollectionType"], "playlists");
        assert_eq!(playlists_view["Type"], "CollectionFolder");
        assert_eq!(playlists_view["Name"], "Playlists");
    }

    #[tokio::test]
    async fn subsonic_get_items_playlists_returns_playlist_type_items() {
        let _lock = credential_test_lock();
        let temp_dir = tempfile::tempdir().unwrap();
        CredentialManager::set_config_path(temp_dir.path().join("missing-config.json"));

        let mut server = mockito::Server::new_async().await;
        let _ping = server
            .mock("GET", "/rest/ping.view")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"subsonic-response":{"status":"ok","version":"1.16.1"}}"#)
            .expect(1)
            .create_async()
            .await;
        let _playlists = server
            .mock("GET", "/rest/getPlaylists.view")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"subsonic-response":{"status":"ok","version":"1.16.1","playlists":{"playlist":[{"id":"pl1","name":"Road Trip","songCount":3,"duration":180,"coverArt":"pl-cover"}]}}}"#,
            )
            .expect(1)
            .create_async()
            .await;

        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);
        handle_server_connect(
            &state,
            Some(json!({
                "url": server.url(),
                "serverType": "subsonic",
                "username": "subsonic-user",
                "password": "subsonic-password"
            })),
        )
        .await
        .expect("connect");

        let items = handle_jellyfin_get_items(
            &state,
            Some(json!({
                "parentId": "playlists",
                "startIndex": 0,
                "limit": 50
            })),
        )
        .await
        .expect("items should come from active provider");

        assert_eq!(items["TotalRecordCount"], 1);
        assert_eq!(items["Items"][0]["Id"], "pl1");
        assert_eq!(items["Items"][0]["Name"], "Road Trip");
        assert_eq!(items["Items"][0]["Type"], "Playlist");
        assert_eq!(items["Items"][0]["ImageId"], "pl-cover");
    }

    #[tokio::test]
    async fn subsonic_get_items_playlist_id_returns_tracks() {
        let _lock = credential_test_lock();
        let temp_dir = tempfile::tempdir().unwrap();
        CredentialManager::set_config_path(temp_dir.path().join("missing-config.json"));

        let mut server = mockito::Server::new_async().await;
        let _ping = server
            .mock("GET", "/rest/ping.view")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"subsonic-response":{"status":"ok","version":"1.16.1"}}"#)
            .expect(1)
            .create_async()
            .await;
        // get_artist will fail for "pl1", get_album will fail, then get_playlist succeeds
        let _get_artist = server
            .mock("GET", "/rest/getArtist.view")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"subsonic-response":{"status":"failed","version":"1.16.1","error":{"code":70,"message":"Not found"}}}"#,
            )
            .create_async()
            .await;
        let _get_album = server
            .mock("GET", "/rest/getAlbum.view")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"subsonic-response":{"status":"failed","version":"1.16.1","error":{"code":70,"message":"Not found"}}}"#,
            )
            .create_async()
            .await;
        let _get_playlist = server
            .mock("GET", "/rest/getPlaylist.view")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"subsonic-response":{"status":"ok","version":"1.16.1","playlist":{"id":"pl1","name":"Road Trip","songCount":1,"duration":60,"entry":[{"id":"song1","title":"Track One","duration":60}]}}}"#,
            )
            .expect(1)
            .create_async()
            .await;

        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);
        handle_server_connect(
            &state,
            Some(json!({
                "url": server.url(),
                "serverType": "subsonic",
                "username": "subsonic-user",
                "password": "subsonic-password"
            })),
        )
        .await
        .expect("connect");

        let items = handle_jellyfin_get_items(
            &state,
            Some(json!({
                "parentId": "pl1",
                "startIndex": 0,
                "limit": 50
            })),
        )
        .await
        .expect("items should come from active provider");

        assert_eq!(items["TotalRecordCount"], 1);
        assert_eq!(items["Items"][0]["Id"], "song1");
        assert_eq!(items["Items"][0]["Name"], "Track One");
        assert_eq!(items["Items"][0]["Type"], "Audio");
    }

    #[tokio::test]
    async fn jellyfin_get_views_is_unchanged_for_jellyfin_provider() {
        let _lock = credential_test_lock();
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("config.json");
        CredentialManager::set_config_path(config_path);

        let mut server = mockito::Server::new_async().await;
        let token = "jellyfin-token-abc";
        CredentialManager::save_credentials(&server.url(), token, Some("user1")).unwrap();

        let _views = server
            .mock("GET", "/UserViews")
            .match_query(mockito::Matcher::UrlEncoded("userId".into(), "user1".into()))
            .match_header("Authorization", format!("MediaBrowser Token=\"{}\"", token).as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"Items":[{"Id":"lib1","Name":"Music","Type":"CollectionFolder","CollectionType":"music"}],"TotalRecordCount":1}"#,
            )
            .expect(1)
            .create_async()
            .await;

        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);
        // Jellyfin provider is set so active_non_jellyfin_provider returns None,
        // causing handle_jellyfin_get_views to fall through to the JellyfinClient path.
        state
            .server_manager
            .write()
            .await
            .set_test_provider(Arc::new(
                crate::providers::jellyfin::JellyfinProvider::new_with_version(
                    JellyfinClient::new(),
                    server.url(),
                    token,
                    "user1",
                    Some("10.9.0".to_string()),
                ),
            ));

        let views = handle_jellyfin_get_views(&state, None)
            .await
            .expect("jellyfin views should come from Jellyfin API");

        assert_eq!(
            views.as_array().unwrap().len(),
            1,
            "jellyfin views should not add synthetic playlists library"
        );
        assert_eq!(views[0]["Id"], "lib1");
        assert_eq!(views[0]["CollectionType"], "music");
    }

    // --- Fake provider for browse handler tests ---

    struct FakeBrowseProvider {
        modes: Vec<crate::providers::BrowseMode>,
        genres: Vec<crate::domain::models::Genre>,
        albums: HashMap<String, crate::domain::models::AlbumWithTracks>,
        genre_tracks: HashMap<String, Vec<crate::domain::models::Song>>,
        songs: HashMap<String, crate::domain::models::Song>,
        tracks: Vec<crate::domain::models::Song>,
        song_auth_error: Option<String>,
        podcast_complete: bool,
    }

    impl FakeBrowseProvider {
        fn new(
            modes: Vec<crate::providers::BrowseMode>,
            genres: Vec<crate::domain::models::Genre>,
        ) -> Arc<Self> {
            Arc::new(Self {
                modes,
                genres,
                albums: HashMap::new(),
                genre_tracks: HashMap::new(),
                songs: HashMap::new(),
                tracks: vec![],
                song_auth_error: None,
                podcast_complete: false,
            })
        }

        fn with_genre_tracks(
            genre_id: &str,
            tracks: Vec<crate::domain::models::Song>,
        ) -> Arc<Self> {
            let mut genre_tracks = HashMap::new();
            genre_tracks.insert(genre_id.to_string(), tracks);
            Arc::new(Self {
                modes: vec![crate::providers::BrowseMode::Genres],
                genres: vec![],
                albums: HashMap::new(),
                genre_tracks,
                songs: HashMap::new(),
                tracks: vec![],
                song_auth_error: None,
                podcast_complete: false,
            })
        }

        fn with_song(song: crate::domain::models::Song) -> Arc<Self> {
            let mut songs = HashMap::new();
            songs.insert(song.id.clone(), song);
            Arc::new(Self {
                modes: vec![],
                genres: vec![],
                albums: HashMap::new(),
                genre_tracks: HashMap::new(),
                songs,
                tracks: vec![],
                song_auth_error: None,
                podcast_complete: false,
            })
        }

        fn with_album_and_song(
            album: crate::domain::models::AlbumWithTracks,
            song: crate::domain::models::Song,
        ) -> Arc<Self> {
            let mut albums = HashMap::new();
            albums.insert(album.album.id.clone(), album);
            let mut songs = HashMap::new();
            songs.insert(song.id.clone(), song);
            Arc::new(Self {
                modes: vec![],
                genres: vec![],
                albums,
                genre_tracks: HashMap::new(),
                songs,
                tracks: vec![],
                song_auth_error: None,
                podcast_complete: false,
            })
        }

        fn with_song_auth_error(message: &str) -> Arc<Self> {
            Arc::new(Self {
                modes: vec![],
                genres: vec![],
                albums: HashMap::new(),
                genre_tracks: HashMap::new(),
                songs: HashMap::new(),
                tracks: vec![],
                song_auth_error: Some(message.to_string()),
                podcast_complete: false,
            })
        }

        fn with_tracks(tracks: Vec<crate::domain::models::Song>) -> Arc<Self> {
            Arc::new(Self {
                modes: vec![crate::providers::BrowseMode::Tracks],
                genres: vec![],
                albums: HashMap::new(),
                genre_tracks: HashMap::new(),
                songs: HashMap::new(),
                tracks,
                song_auth_error: None,
                podcast_complete: false,
            })
        }
    }

    #[async_trait::async_trait]
    impl MediaProvider for FakeBrowseProvider {
        async fn resolve_sync_media(
            &self,
            _id: &str,
        ) -> Result<crate::providers::SyncMediaRepresentation, ProviderError> {
            Ok(crate::providers::SyncMediaRepresentation {
                codec: "mp3".into(),
                container: "mp3".into(),
                request: crate::providers::PlaybackRequest {
                    url: reqwest::Url::parse("https://example.invalid/episode").unwrap(),
                    headers: reqwest::header::HeaderMap::new(),
                    range_supported: true,
                    cleanup: None,
                    refresh: None,
                    expected_content_type: Some("audio/mpeg".into()),
                },
            })
        }

        fn library_role(&self) -> Option<crate::providers::ProviderLibraryRole> {
            self.modes
                .contains(&crate::providers::BrowseMode::Podcasts)
                .then_some(crate::providers::ProviderLibraryRole::Podcast)
        }

        async fn get_podcast_episode(
            &self,
            id: &str,
        ) -> Result<crate::domain::models::PodcastEpisode, ProviderError> {
            if id != "episode-opaque" {
                return Err(ProviderError::NotFound {
                    item_type: "episode".into(),
                    id: "unavailable".into(),
                });
            }
            Ok(crate::domain::models::PodcastEpisode {
                item_type: crate::domain::models::PodcastEntityType::Episode,
                id: id.into(),
                show_id: "show-opaque".into(),
                show_title: Some("Talks".into()),
                title: "First".into(),
                description: None,
                duration_seconds: Some(60),
                published_at: None,
                cover_art_id: None,
            })
        }

        async fn list_podcast_shows(
            &self,
            offset: u32,
            _limit: u32,
        ) -> Result<(Vec<crate::domain::models::PodcastShow>, u32), ProviderError> {
            let shows = if offset == 0 {
                vec![crate::domain::models::PodcastShow {
                    item_type: crate::domain::models::PodcastEntityType::Show,
                    id: "show-opaque".into(),
                    title: "Talks".into(),
                    description: None,
                    cover_art_id: None,
                    episode_count: Some(1),
                }]
            } else {
                vec![]
            };
            Ok((shows, 1))
        }
        async fn list_recent_podcast_episodes(
            &self,
            offset: u32,
            _limit: u32,
        ) -> Result<(Vec<crate::domain::models::PodcastEpisode>, u32, u32), ProviderError> {
            let episodes = if offset == 0 {
                vec![self.get_podcast_episode("episode-opaque").await?]
            } else {
                vec![]
            };
            let source_count = episodes.len() as u32;
            Ok((episodes, 1, source_count))
        }
        async fn get_podcast_show(
            &self,
            id: &str,
        ) -> Result<crate::domain::models::PodcastShowDetail, ProviderError> {
            if id != "show-opaque" {
                return Err(ProviderError::NotFound {
                    item_type: "show".into(),
                    id: "unavailable".into(),
                });
            }
            Ok(crate::domain::models::PodcastShowDetail {
                show: crate::domain::models::PodcastShow {
                    item_type: crate::domain::models::PodcastEntityType::Show,
                    id: id.into(),
                    title: "Talks".into(),
                    description: None,
                    cover_art_id: None,
                    episode_count: Some(1),
                },
                episodes: vec![crate::domain::models::PodcastEpisode {
                    item_type: crate::domain::models::PodcastEntityType::Episode,
                    id: "episode-opaque".into(),
                    show_id: id.into(),
                    show_title: Some("Talks".into()),
                    title: "First".into(),
                    description: None,
                    duration_seconds: Some(60),
                    published_at: None,
                    cover_art_id: None,
                }],
                possibly_truncated: !self.podcast_complete,
            })
        }
        async fn search_podcasts(
            &self,
            _query: &str,
        ) -> Result<crate::domain::models::PodcastSearchResult, ProviderError> {
            Ok(crate::domain::models::PodcastSearchResult {
                shows: vec![crate::domain::models::PodcastShow {
                    item_type: crate::domain::models::PodcastEntityType::Show,
                    id: "show-opaque".into(),
                    title: "Talks".into(),
                    description: None,
                    cover_art_id: None,
                    episode_count: Some(1),
                }],
                episodes: vec![],
                possibly_truncated: false,
            })
        }
        async fn list_libraries(
            &self,
        ) -> Result<Vec<crate::domain::models::Library>, ProviderError> {
            unimplemented!()
        }
        async fn list_artists(
            &self,
            _: Option<&str>,
            _: Option<&str>,
            _: u32,
            _: u32,
        ) -> Result<(Vec<crate::domain::models::Artist>, u32), ProviderError> {
            unimplemented!()
        }
        async fn get_artist(
            &self,
            _: &str,
        ) -> Result<crate::domain::models::ArtistWithAlbums, ProviderError> {
            Err(ProviderError::UnsupportedCapability(
                "fake provider has no artists".to_string(),
            ))
        }
        async fn list_albums(
            &self,
            _: Option<&str>,
            _: Option<&str>,
            offset: u32,
            limit: u32,
        ) -> Result<(Vec<crate::domain::models::Album>, u32), ProviderError> {
            let mut albums: Vec<_> = self
                .albums
                .values()
                .map(|detail| detail.album.clone())
                .collect();
            albums.sort_by(|a, b| a.id.cmp(&b.id));
            let total = albums.len() as u32;
            Ok((
                albums
                    .into_iter()
                    .skip(offset as usize)
                    .take(limit as usize)
                    .collect(),
                total,
            ))
        }
        async fn get_album(
            &self,
            album_id: &str,
        ) -> Result<crate::domain::models::AlbumWithTracks, ProviderError> {
            self.albums
                .get(album_id)
                .cloned()
                .ok_or(ProviderError::UnsupportedCapability(
                    "fake provider has no albums".to_string(),
                ))
        }
        async fn get_song(
            &self,
            song_id: &str,
        ) -> Result<crate::domain::models::Song, ProviderError> {
            if let Some(message) = &self.song_auth_error {
                return Err(ProviderError::Auth(message.clone()));
            }
            self.songs
                .get(song_id)
                .cloned()
                .ok_or(ProviderError::UnsupportedCapability(
                    "fake provider has no matching song".to_string(),
                ))
        }
        async fn list_playlists(
            &self,
        ) -> Result<Vec<crate::domain::models::Playlist>, ProviderError> {
            unimplemented!()
        }
        async fn get_playlist(
            &self,
            _: &str,
        ) -> Result<crate::domain::models::PlaylistWithTracks, ProviderError> {
            Err(ProviderError::UnsupportedCapability(
                "fake provider has no playlists".to_string(),
            ))
        }
        async fn search(
            &self,
            _: &str,
        ) -> Result<crate::domain::models::SearchResult, ProviderError> {
            unimplemented!()
        }
        async fn download_url(
            &self,
            _: &str,
            _: Option<&crate::providers::TranscodeProfile>,
        ) -> Result<String, ProviderError> {
            unimplemented!()
        }
        async fn cover_art_url(&self, _: &str) -> Result<String, ProviderError> {
            unimplemented!()
        }
        async fn changes_since_with_context(
            &self,
            _: Option<&str>,
            _: &crate::providers::ProviderChangeContext,
        ) -> Result<Vec<crate::domain::models::ChangeEvent>, ProviderError> {
            unimplemented!()
        }
        async fn scrobble(
            &self,
            _: crate::providers::ScrobbleRequest,
        ) -> Result<(), ProviderError> {
            unimplemented!()
        }
        async fn list_genres(
            &self,
            _library_id: Option<&str>,
            offset: u32,
            limit: u32,
        ) -> Result<(Vec<crate::domain::models::Genre>, u64), ProviderError> {
            let total = self.genres.len() as u64;
            let page = self
                .genres
                .iter()
                .skip(offset as usize)
                .take(limit as usize)
                .cloned()
                .collect();
            Ok((page, total))
        }
        async fn get_genre_tracks(
            &self,
            genre_id_or_name: &str,
            offset: u32,
            limit: u32,
        ) -> Result<(Vec<crate::domain::models::Song>, u32), ProviderError> {
            let tracks =
                self.genre_tracks
                    .get(genre_id_or_name)
                    .ok_or(ProviderError::NotFound {
                        item_type: "Genre".to_string(),
                        id: genre_id_or_name.to_string(),
                    })?;
            let page = tracks
                .iter()
                .skip(offset as usize)
                .take(limit as usize)
                .cloned()
                .collect();
            Ok((page, tracks.len() as u32))
        }
        async fn list_tracks(
            &self,
            filter: crate::providers::TrackListFilter,
        ) -> Result<crate::providers::TrackListPage, ProviderError> {
            let start = filter.start_index as usize;
            let limit = filter.limit as usize;
            let total = self.tracks.len() as u32;
            let page: Vec<crate::domain::models::Song> = if limit > 0 {
                self.tracks
                    .iter()
                    .skip(start)
                    .take(limit)
                    .cloned()
                    .collect()
            } else {
                self.tracks.iter().skip(start).cloned().collect()
            };
            Ok(crate::providers::TrackListPage {
                tracks: page,
                total,
                start_index: filter.start_index,
                limit: filter.limit,
            })
        }
        fn server_type(&self) -> crate::providers::ServerType {
            crate::providers::ServerType::Jellyfin
        }
        fn capabilities(&self) -> crate::providers::Capabilities {
            crate::providers::Capabilities {
                open_subsonic: false,
                supports_changes_since: false,
                supports_server_transcoding: false,
                supports_playlist_write: false,
                browse: crate::providers::BrowseCapabilities {
                    list_modes: self.modes.clone(),
                },
            }
        }
    }

    #[tokio::test]
    async fn browse_list_modes_routes_through_provider_capabilities() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);
        let provider = FakeBrowseProvider::new(
            vec![
                crate::providers::BrowseMode::Artists,
                crate::providers::BrowseMode::Genres,
            ],
            vec![],
        );
        state
            .server_manager
            .write()
            .await
            .set_test_provider(provider as Arc<dyn MediaProvider>);

        let result = handle_browse_list_modes(&state).await.expect("list modes");

        let modes = result["modes"].as_array().expect("modes array");
        assert_eq!(modes.len(), 2);
        assert_eq!(modes[0], "artists");
        assert_eq!(modes[1], "genres");
    }

    #[tokio::test]
    async fn browse_list_genres_returns_genres_from_provider() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);
        let genre = crate::domain::models::Genre {
            id: "rock".to_string(),
            name: "Rock".to_string(),
            song_count: Some(10),
            cover_art_id: None,
        };
        let provider =
            FakeBrowseProvider::new(vec![crate::providers::BrowseMode::Genres], vec![genre]);
        state
            .server_manager
            .write()
            .await
            .set_test_provider(provider as Arc<dyn MediaProvider>);

        let result = handle_browse_list_genres(&state, None)
            .await
            .expect("list genres");

        assert_eq!(result["total"], 1);
        assert_eq!(result["genres"][0]["id"], "rock");
        assert_eq!(result["genres"][0]["name"], "Rock");
    }

    #[tokio::test]
    async fn podcast_rpc_rejects_music_provider_and_unbounded_pages() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);
        let provider = FakeBrowseProvider::new(vec![crate::providers::BrowseMode::Albums], vec![]);
        state
            .server_manager
            .write()
            .await
            .set_test_provider(provider as Arc<dyn MediaProvider>);
        let wrong_role =
            handle_browse_list_podcast_shows(&state, Some(json!({"startIndex":0,"limit":50})))
                .await
                .unwrap_err();
        assert_eq!(wrong_role.code, ERR_UNSUPPORTED_CAPABILITY);
        let invalid_page =
            handle_browse_list_podcast_shows(&state, Some(json!({"startIndex":1,"limit":50})))
                .await
                .unwrap_err();
        assert_eq!(invalid_page.code, ERR_INVALID_PARAMS);
        let oversized =
            handle_browse_list_podcast_shows(&state, Some(json!({"startIndex":0,"limit":1000})))
                .await
                .unwrap_err();
        assert_eq!(oversized.code, ERR_INVALID_PARAMS);
    }

    #[tokio::test]
    async fn podcast_rpc_returns_typed_show_and_episode_fields() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);
        let provider =
            FakeBrowseProvider::new(vec![crate::providers::BrowseMode::Podcasts], vec![]);
        state
            .server_manager
            .write()
            .await
            .set_test_provider(provider as Arc<dyn MediaProvider>);
        let listing =
            handle_browse_list_podcast_shows(&state, Some(json!({"startIndex":0,"limit":50})))
                .await
                .unwrap();
        assert_eq!(listing["shows"][0]["title"], "Talks");
        assert_eq!(listing["shows"][0]["type"], "show");
        assert!(listing.get("albums").is_none());
        let recent = handle_browse_list_recent_podcast_episodes(
            &state,
            Some(json!({"startIndex":0,"limit":50})),
        )
        .await
        .unwrap();
        assert_eq!(recent["episodes"][0]["id"], "episode-opaque");
        assert_eq!(recent["total"], 1);
        assert_eq!(recent["sourceCount"], 1);
        assert_eq!(
            handle_browse_list_recent_podcast_episodes(
                &state,
                Some(json!({"startIndex":1,"limit":50})),
            )
            .await
            .unwrap_err()
            .code,
            ERR_INVALID_PARAMS
        );
        let detail = handle_browse_get_podcast_show(&state, Some(json!({"showId":"show-opaque"})))
            .await
            .unwrap();
        assert_eq!(detail["episodes"][0]["title"], "First");
        assert_eq!(detail["episodes"][0]["type"], "episode");
        assert_eq!(detail["episodes"][0]["showId"], "show-opaque");
        assert_eq!(detail["possiblyTruncated"], true);
        assert!(detail.get("tracks").is_none());
        let search = handle_browse_search(&state, Some(json!({"query":"Talks"})))
            .await
            .unwrap();
        assert_eq!(search["shows"][0]["type"], "show");
        assert!(search.get("albums").is_none());
    }

    #[tokio::test]
    async fn provider_sync_items_for_id_paginates_genre_tracks() {
        let track_count = GENRE_TRACK_PAGE_SIZE + 3;
        let tracks = (0..track_count)
            .map(|idx| crate::domain::models::Song {
                id: format!("song-{idx}"),
                title: format!("Track {idx}"),
                artist_id: None,
                artist_name: Some("Artist".to_string()),
                album_id: Some("album-1".to_string()),
                album_title: Some("Album".to_string()),
                duration_seconds: 60,
                bitrate_kbps: Some(320),
                track_number: Some(idx + 1),
                disc_number: Some(1),
                cover_art_id: None,
                date_added: None,
                last_played_at: None,
                play_count: None,
                is_favorite: None,
                content_type: Some("audio/mpeg".to_string()),
                suffix: Some("mp3".to_string()),
                size_bytes: None,
                album_loudness: Default::default(),
                provider_metadata: Default::default(),
            })
            .collect::<Vec<_>>();
        let provider = FakeBrowseProvider::with_genre_tracks("rock", tracks);

        let (items, playlist) =
            provider_sync_items_for_id(provider as Arc<dyn MediaProvider>, "rock")
                .await
                .expect("genre should resolve");

        assert!(playlist.is_none());
        assert_eq!(items.len(), track_count as usize);
        assert_eq!(items[0].jellyfin_id, "song-0");
        assert_eq!(
            items.last().map(|item| item.jellyfin_id.as_str()),
            Some("song-502")
        );
    }

    #[tokio::test]
    async fn provider_sync_items_for_id_resolves_single_song() {
        let provider = FakeBrowseProvider::with_song(crate::domain::models::Song {
            id: "song1".to_string(),
            title: "Track".to_string(),
            artist_id: Some("artist1".to_string()),
            artist_name: Some("Artist".to_string()),
            album_id: Some("album1".to_string()),
            album_title: Some("Album".to_string()),
            duration_seconds: 319,
            bitrate_kbps: Some(320),
            track_number: Some(1),
            disc_number: None,
            cover_art_id: Some("cover1".to_string()),
            date_added: None,
            last_played_at: None,
            play_count: None,
            is_favorite: None,
            content_type: Some("audio/flac".to_string()),
            suffix: Some("flac".to_string()),
            size_bytes: None,
            album_loudness: Default::default(),
            provider_metadata: Default::default(),
        });

        let (items, playlist) =
            provider_sync_items_for_id(provider as Arc<dyn MediaProvider>, "song1")
                .await
                .expect("song should resolve");

        assert!(playlist.is_none());
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].jellyfin_id, "song1");
        assert_eq!(items[0].name, "Track");
        assert_eq!(items[0].artist.as_deref(), Some("Artist"));
        assert_eq!(items[0].album.as_deref(), Some("Album"));
        assert_eq!(items[0].provider_album_id.as_deref(), Some("album1"));
        assert_eq!(
            items[0].provider_content_type.as_deref(),
            Some("audio/flac")
        );
        assert_eq!(items[0].provider_suffix.as_deref(), Some("flac"));
    }

    #[tokio::test]
    async fn podcast_episode_sync_uses_typed_identity() {
        let provider =
            FakeBrowseProvider::new(vec![crate::providers::BrowseMode::Podcasts], vec![]);
        let (items, playlist) =
            provider_sync_items_for_id(provider as Arc<dyn MediaProvider>, "episode-opaque")
                .await
                .expect("typed episode should be admitted");
        assert!(playlist.is_none());
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].jellyfin_id, "episode-opaque");
        assert_eq!(items[0].media_role, crate::device::MediaRole::Podcast);
        assert_eq!(items[0].provider_album_id.as_deref(), Some("show-opaque"));
        assert!(items[0].size_bytes > 0);
    }

    #[tokio::test]
    async fn podcast_autofill_respects_recent_limit_budget_and_unknown_progress() {
        let mut provider =
            FakeBrowseProvider::new(vec![crate::providers::BrowseMode::Podcasts], vec![]);
        Arc::get_mut(&mut provider).unwrap().podcast_complete = true;
        let mut pipeline = crate::auto_fill::AutoFillPipeline::default_legacy(Some(1_000_000));
        pipeline.podcast_retention.recent_count = 1;
        pipeline.podcast_retention.unplayed_only = true;
        let params = crate::auto_fill::AutoFillParams {
            exclude_item_ids: vec![],
            max_fill_bytes: 1_000_000,
            device_id: "device".into(),
            server_id: "server".into(),
            now_unix: 0,
            history: Default::default(),
            rotation_cursor: 0,
            seed: 0,
            pity_streak: 0,
            local: now_civil(),
        };
        let items =
            expand_auto_fill_slot(provider as Arc<dyn MediaProvider>, Some(&pipeline), params)
                .await
                .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "episode-opaque");
        assert!(items[0].priority_reason.contains("play state unavailable"));
        assert_eq!(items[0].size_bytes, 960_000);
    }

    #[tokio::test]
    async fn podcast_autofill_selected_shows_does_not_expand_other_shows() {
        let mut provider =
            FakeBrowseProvider::new(vec![crate::providers::BrowseMode::Podcasts], vec![]);
        Arc::get_mut(&mut provider).unwrap().podcast_complete = true;
        let params = || crate::auto_fill::AutoFillParams {
            exclude_item_ids: vec![],
            max_fill_bytes: 1_000_000,
            device_id: "device".into(),
            server_id: "server".into(),
            now_unix: 0,
            history: Default::default(),
            rotation_cursor: 0,
            seed: 0,
            pity_streak: 0,
            local: now_civil(),
        };
        let mut pipeline = crate::auto_fill::AutoFillPipeline::default_legacy(None);
        pipeline.podcast_retention.mode = crate::auto_fill::PodcastSelectionMode::SelectedShows;
        pipeline.podcast_retention.show_ids = vec!["different-show".into()];
        assert!(
            expand_podcast_auto_fill(
                provider.clone() as Arc<dyn MediaProvider>,
                Some(&pipeline),
                params()
            )
            .await
            .unwrap()
            .is_empty()
        );
        pipeline.podcast_retention.show_ids = vec!["show-opaque".into()];
        let selected = expand_podcast_auto_fill(
            provider as Arc<dyn MediaProvider>,
            Some(&pipeline),
            params(),
        )
        .await
        .unwrap();
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].id, "episode-opaque");
    }

    #[tokio::test]
    async fn audiobook_autofill_selects_complete_books_within_budget() {
        let mut provider =
            FakeBrowseProvider::new(vec![crate::providers::BrowseMode::Albums], vec![]);
        let mut part_a = make_fake_song("part-a", "Part A");
        part_a.size_bytes = Some(400_000);
        let mut part_b = make_fake_song("part-b", "Part B");
        part_b.size_bytes = Some(400_000);
        Arc::get_mut(&mut provider).unwrap().albums.insert(
            "book-a".into(),
            crate::domain::models::AlbumWithTracks {
                album: crate::domain::models::Album {
                    id: "book-a".into(),
                    title: "Book A".into(),
                    artist_id: None,
                    artist_name: None,
                    year: None,
                    song_count: Some(2),
                    duration_seconds: None,
                    cover_art_id: None,
                    provider_metadata: Default::default(),
                },
                tracks: vec![part_a, part_b],
                provider_metadata: Default::default(),
            },
        );
        let params = crate::auto_fill::AutoFillParams {
            exclude_item_ids: vec![],
            max_fill_bytes: 1_000_000,
            device_id: "device".into(),
            server_id: "server".into(),
            now_unix: 0,
            history: Default::default(),
            rotation_cursor: 0,
            seed: 0,
            pity_streak: 0,
            local: now_civil(),
        };
        let items = expand_audiobook_auto_fill(provider.clone() as Arc<dyn MediaProvider>, params)
            .await
            .unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(
            items.iter().map(|item| item.size_bytes).sum::<u64>(),
            800_000
        );

        let tight = crate::auto_fill::AutoFillParams {
            exclude_item_ids: vec![],
            max_fill_bytes: 700_000,
            device_id: "device".into(),
            server_id: "server".into(),
            now_unix: 0,
            history: Default::default(),
            rotation_cursor: 0,
            seed: 0,
            pity_streak: 0,
            local: now_civil(),
        };
        assert!(
            expand_audiobook_auto_fill(provider as Arc<dyn MediaProvider>, tight)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn podcast_dates_compare_in_utc_with_unknown_last() {
        let a = Some("2026-09-24T02:00:00+02:00".to_string());
        let b = Some("2026-09-24T01:00:00Z".to_string());
        assert!(podcast_published_at(&b) > podcast_published_at(&a));
        assert!(podcast_published_at(&a) > podcast_published_at(&None));
    }

    #[tokio::test]
    async fn incompatible_podcast_is_blocked_before_transfer() {
        let provider =
            FakeBrowseProvider::new(vec![crate::providers::BrowseMode::Podcasts], vec![]);
        let (items, _) = provider_sync_items_for_id(
            provider.clone() as Arc<dyn MediaProvider>,
            "episode-opaque",
        )
        .await
        .unwrap();
        let mut delta = crate::sync::calculate_delta(
            &items,
            &crate::device::DeviceManifest {
                managed_paths: vec!["Music".into()],
                ..Default::default()
            },
        );
        let flac_only = serde_json::json!({"DirectPlayProfiles":[{"Type":"Audio","Container":"flac","AudioCodec":"flac"}]});
        preflight_provider_media_adds(
            &mut delta,
            provider as Arc<dyn MediaProvider>,
            Some(&flac_only),
            None,
        )
        .await;
        assert!(delta.adds.is_empty());
        assert_eq!(delta.blocked.len(), 1);
        assert_eq!(delta.blocked[0].reason_code, "incompatible-direct-format");
        assert_eq!(
            delta.blocked[0].media_role,
            crate::device::MediaRole::Podcast
        );
    }

    #[tokio::test]
    async fn compatible_podcast_preflight_records_verified_direct_reason() {
        let provider =
            FakeBrowseProvider::new(vec![crate::providers::BrowseMode::Podcasts], vec![]);
        let (items, _) = provider_sync_items_for_id(
            provider.clone() as Arc<dyn MediaProvider>,
            "episode-opaque",
        )
        .await
        .unwrap();
        let mut delta = crate::sync::calculate_delta(
            &items,
            &crate::device::DeviceManifest {
                managed_paths: vec!["Music".into()],
                ..Default::default()
            },
        );
        let mp3 = serde_json::json!({"DirectPlayProfiles":[{"Type":"Audio","Container":"mp3","AudioCodec":"mp3"}]});
        preflight_provider_media_adds(
            &mut delta,
            provider as Arc<dyn MediaProvider>,
            Some(&mp3),
            None,
        )
        .await;
        assert!(delta.blocked.is_empty());
        assert_eq!(delta.adds.len(), 1);
        assert_eq!(
            delta.adds[0].reason_code.as_deref(),
            Some("verified-direct-format")
        );
    }

    #[tokio::test]
    async fn provider_sync_items_for_id_propagates_song_lookup_failures() {
        let provider = FakeBrowseProvider::with_song_auth_error("auth failed");

        let err = provider_sync_items_for_id(provider as Arc<dyn MediaProvider>, "song1")
            .await
            .expect_err("auth failure should not be masked as not found");

        // AC11: provider auth failures map to ERR_UNAUTHORIZED so the UI can re-auth.
        assert_eq!(err.code, ERR_UNAUTHORIZED);
        assert_eq!(err.message, "auth failed");
    }

    #[tokio::test]
    async fn provider_calculate_delta_dedupes_album_and_selected_song() {
        let song = crate::domain::models::Song {
            id: "song1".to_string(),
            title: "Track".to_string(),
            artist_id: Some("artist1".to_string()),
            artist_name: Some("Artist".to_string()),
            album_id: Some("album1".to_string()),
            album_title: Some("Album".to_string()),
            duration_seconds: 319,
            bitrate_kbps: Some(320),
            track_number: Some(1),
            disc_number: None,
            cover_art_id: Some("cover1".to_string()),
            date_added: None,
            last_played_at: None,
            play_count: None,
            is_favorite: None,
            content_type: Some("audio/mpeg".to_string()),
            suffix: Some("mp3".to_string()),
            size_bytes: None,
            album_loudness: Default::default(),
            provider_metadata: Default::default(),
        };
        let provider = FakeBrowseProvider::with_album_and_song(
            crate::domain::models::AlbumWithTracks {
                album: crate::domain::models::Album {
                    id: "album1".to_string(),
                    title: "Album".to_string(),
                    artist_id: Some("artist1".to_string()),
                    artist_name: Some("Artist".to_string()),
                    year: None,
                    song_count: Some(1),
                    duration_seconds: Some(319),
                    cover_art_id: Some("cover1".to_string()),
                    provider_metadata: Default::default(),
                },
                tracks: vec![song.clone()],
                provider_metadata: Default::default(),
            },
            song,
        );
        let manifest = crate::device::DeviceManifest::default();
        let item_ids = vec!["album1".to_string(), "song1".to_string()];

        let delta = provider_calculate_delta(
            &make_test_state(Arc::new(crate::db::Database::memory().unwrap())),
            provider as Arc<dyn MediaProvider>,
            &item_ids,
            &manifest,
            &json!({}),
        )
        .await
        .expect("delta");

        let adds = delta["adds"].as_array().expect("adds");
        assert_eq!(adds.len(), 1);
        assert_eq!(adds[0]["jellyfinId"], "song1");
    }

    #[tokio::test]
    async fn browse_unsupported_capability_maps_to_err_unsupported_capability() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);
        let provider = FakeBrowseProvider::new(vec![crate::providers::BrowseMode::Artists], vec![]);
        state
            .server_manager
            .write()
            .await
            .set_test_provider(provider as Arc<dyn MediaProvider>);

        let err = handle_browse_list_recently_added(&state, None)
            .await
            .expect_err("should be unsupported");

        assert_eq!(
            err.code, ERR_UNSUPPORTED_CAPABILITY,
            "UnsupportedCapability must map to ERR_UNSUPPORTED_CAPABILITY, got code {}",
            err.code
        );
    }

    fn make_fake_song(id: &str, title: &str) -> crate::domain::models::Song {
        crate::domain::models::Song {
            id: id.to_string(),
            title: title.to_string(),
            artist_id: Some("artist1".to_string()),
            artist_name: Some("Artist".to_string()),
            album_id: Some("album1".to_string()),
            album_title: Some("Album".to_string()),
            duration_seconds: 0,
            bitrate_kbps: None,
            track_number: Some(1),
            disc_number: None,
            cover_art_id: None,
            date_added: None,
            last_played_at: None,
            play_count: None,
            is_favorite: None,
            content_type: None,
            suffix: None,
            size_bytes: None,
            album_loudness: Default::default(),
            provider_metadata: Default::default(),
        }
    }

    #[test]
    fn audiobook_part_without_reported_bytes_uses_duration_estimate() {
        let mut part = make_fake_song("part", "Part");
        part.duration_seconds = 120;
        part.provider_metadata.identity = Some(crate::domain::models::ProviderIdentity {
            library_id: "library".into(),
            library_item_id: "book".into(),
            media_id: "media".into(),
        });
        assert_eq!(provider_track_size(&part), 1_920_000);
        part.size_bytes = Some(2_000_000);
        assert_eq!(provider_track_size(&part), 2_000_000);
    }

    #[tokio::test]
    async fn browse_list_tracks_returns_tracks_from_provider() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);
        let tracks = vec![
            make_fake_song("s1", "Alpha"),
            make_fake_song("s2", "Beta"),
            make_fake_song("s3", "Gamma"),
        ];
        let provider = FakeBrowseProvider::with_tracks(tracks);
        state
            .server_manager
            .write()
            .await
            .set_test_provider(provider as Arc<dyn MediaProvider>);

        let params = Some(serde_json::json!({
            "startIndex": 0,
            "limit": 2,
        }));
        let result = handle_browse_list_tracks(&state, params)
            .await
            .expect("listTracks should succeed");

        let returned = result["tracks"].as_array().expect("tracks array");
        assert_eq!(returned.len(), 2);
        assert_eq!(returned[0]["id"], "s1");
        assert_eq!(returned[1]["id"], "s2");
        assert_eq!(result["total"], 3);
        assert_eq!(result["startIndex"], 0);
        assert_eq!(result["limit"], 2);
    }

    #[tokio::test]
    async fn browse_list_tracks_rejects_when_capability_missing() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);
        let provider = FakeBrowseProvider::new(vec![crate::providers::BrowseMode::Artists], vec![]);
        state
            .server_manager
            .write()
            .await
            .set_test_provider(provider as Arc<dyn MediaProvider>);

        let err = handle_browse_list_tracks(&state, None)
            .await
            .expect_err("should be unsupported");

        assert_eq!(
            err.code, ERR_UNSUPPORTED_CAPABILITY,
            "listTracks without capability must map to ERR_UNSUPPORTED_CAPABILITY, got code {}",
            err.code
        );
    }

    // --- FakePlaylistProvider for playlist RPC tests ---

    struct FakePlaylistProvider {
        songs: HashMap<String, crate::domain::models::Song>,
        albums: HashMap<String, Vec<crate::domain::models::Song>>,
        playlist_return_id: String,
        create_calls: Mutex<Vec<(String, Vec<String>)>>,
        add_calls: Mutex<Vec<(String, Vec<String>)>>,
        remove_calls: Mutex<Vec<(String, Vec<String>)>>,
        delete_calls: Mutex<Vec<String>>,
    }

    impl FakePlaylistProvider {
        fn new(playlist_return_id: &str) -> Arc<Self> {
            Arc::new(Self {
                songs: HashMap::new(),
                albums: HashMap::new(),
                playlist_return_id: playlist_return_id.to_string(),
                create_calls: Mutex::new(vec![]),
                add_calls: Mutex::new(vec![]),
                remove_calls: Mutex::new(vec![]),
                delete_calls: Mutex::new(vec![]),
            })
        }

        fn with_song(playlist_return_id: &str, song: crate::domain::models::Song) -> Arc<Self> {
            let mut songs = HashMap::new();
            songs.insert(song.id.clone(), song);
            Arc::new(Self {
                songs,
                albums: HashMap::new(),
                playlist_return_id: playlist_return_id.to_string(),
                create_calls: Mutex::new(vec![]),
                add_calls: Mutex::new(vec![]),
                remove_calls: Mutex::new(vec![]),
                delete_calls: Mutex::new(vec![]),
            })
        }

        /// Seeds a provider where `album_id` resolves (via `get_album`) to an
        /// album containing `song`, and the same `song` is also resolvable
        /// standalone (via `get_song`). Used to exercise cross-container dedup
        /// in `playlist.create`.
        fn with_album_and_song(
            playlist_return_id: &str,
            album_id: &str,
            song: crate::domain::models::Song,
        ) -> Arc<Self> {
            let mut songs = HashMap::new();
            songs.insert(song.id.clone(), song.clone());
            let mut albums = HashMap::new();
            albums.insert(album_id.to_string(), vec![song]);
            Arc::new(Self {
                songs,
                albums,
                playlist_return_id: playlist_return_id.to_string(),
                create_calls: Mutex::new(vec![]),
                add_calls: Mutex::new(vec![]),
                remove_calls: Mutex::new(vec![]),
                delete_calls: Mutex::new(vec![]),
            })
        }
    }

    #[async_trait::async_trait]
    impl MediaProvider for FakePlaylistProvider {
        async fn list_libraries(
            &self,
        ) -> Result<Vec<crate::domain::models::Library>, ProviderError> {
            unimplemented!()
        }
        async fn list_artists(
            &self,
            _: Option<&str>,
            _: Option<&str>,
            _: u32,
            _: u32,
        ) -> Result<(Vec<crate::domain::models::Artist>, u32), ProviderError> {
            unimplemented!()
        }
        async fn get_artist(
            &self,
            _: &str,
        ) -> Result<crate::domain::models::ArtistWithAlbums, ProviderError> {
            Err(ProviderError::UnsupportedCapability(
                "no artists".to_string(),
            ))
        }
        async fn list_albums(
            &self,
            _: Option<&str>,
            _: Option<&str>,
            _: u32,
            _: u32,
        ) -> Result<(Vec<crate::domain::models::Album>, u32), ProviderError> {
            unimplemented!()
        }
        async fn get_album(
            &self,
            album_id: &str,
        ) -> Result<crate::domain::models::AlbumWithTracks, ProviderError> {
            match self.albums.get(album_id) {
                Some(tracks) => Ok(crate::domain::models::AlbumWithTracks {
                    album: crate::domain::models::Album {
                        id: album_id.to_string(),
                        title: "Album".to_string(),
                        artist_id: None,
                        artist_name: None,
                        year: None,
                        song_count: Some(tracks.len() as u32),
                        duration_seconds: None,
                        cover_art_id: None,
                        provider_metadata: Default::default(),
                    },
                    tracks: tracks.clone(),
                    provider_metadata: Default::default(),
                }),
                None => Err(ProviderError::UnsupportedCapability(
                    "no albums".to_string(),
                )),
            }
        }
        async fn get_song(
            &self,
            song_id: &str,
        ) -> Result<crate::domain::models::Song, ProviderError> {
            self.songs
                .get(song_id)
                .cloned()
                .ok_or(ProviderError::NotFound {
                    item_type: "Song".to_string(),
                    id: song_id.to_string(),
                })
        }
        async fn list_playlists(
            &self,
        ) -> Result<Vec<crate::domain::models::Playlist>, ProviderError> {
            unimplemented!()
        }
        async fn get_playlist(
            &self,
            _: &str,
        ) -> Result<crate::domain::models::PlaylistWithTracks, ProviderError> {
            Err(ProviderError::UnsupportedCapability(
                "no playlists".to_string(),
            ))
        }
        async fn search(
            &self,
            _: &str,
        ) -> Result<crate::domain::models::SearchResult, ProviderError> {
            unimplemented!()
        }
        async fn download_url(
            &self,
            _: &str,
            _: Option<&crate::providers::TranscodeProfile>,
        ) -> Result<String, ProviderError> {
            unimplemented!()
        }
        async fn cover_art_url(&self, _: &str) -> Result<String, ProviderError> {
            unimplemented!()
        }
        async fn changes_since_with_context(
            &self,
            _: Option<&str>,
            _: &crate::providers::ProviderChangeContext,
        ) -> Result<Vec<crate::domain::models::ChangeEvent>, ProviderError> {
            unimplemented!()
        }
        async fn scrobble(
            &self,
            _: crate::providers::ScrobbleRequest,
        ) -> Result<(), ProviderError> {
            unimplemented!()
        }
        async fn list_genres(
            &self,
            _: Option<&str>,
            _: u32,
            _: u32,
        ) -> Result<(Vec<crate::domain::models::Genre>, u64), ProviderError> {
            Ok((vec![], 0))
        }
        async fn get_genre_tracks(
            &self,
            genre_id: &str,
            _: u32,
            _: u32,
        ) -> Result<(Vec<crate::domain::models::Song>, u32), ProviderError> {
            // No genres in this fake: an unknown id is genuinely unresolvable
            // (NotFound), so `provider_sync_items_for_id` falls through to its
            // final not-found error rather than returning an empty track list.
            Err(ProviderError::NotFound {
                item_type: "Genre".to_string(),
                id: genre_id.to_string(),
            })
        }
        fn server_type(&self) -> crate::providers::ServerType {
            crate::providers::ServerType::Subsonic
        }
        fn capabilities(&self) -> crate::providers::Capabilities {
            crate::providers::Capabilities {
                open_subsonic: false,
                supports_changes_since: false,
                supports_server_transcoding: false,
                supports_playlist_write: true,
                browse: crate::providers::BrowseCapabilities { list_modes: vec![] },
            }
        }
        async fn create_playlist(
            &self,
            name: &str,
            track_ids: &[String],
        ) -> Result<String, ProviderError> {
            self.create_calls
                .lock()
                .unwrap()
                .push((name.to_string(), track_ids.to_vec()));
            Ok(self.playlist_return_id.clone())
        }
        async fn add_to_playlist(
            &self,
            playlist_id: &str,
            track_ids: &[String],
        ) -> Result<(), ProviderError> {
            self.add_calls
                .lock()
                .unwrap()
                .push((playlist_id.to_string(), track_ids.to_vec()));
            Ok(())
        }
        async fn remove_from_playlist(
            &self,
            playlist_id: &str,
            track_ids: &[String],
        ) -> Result<(), ProviderError> {
            self.remove_calls
                .lock()
                .unwrap()
                .push((playlist_id.to_string(), track_ids.to_vec()));
            Ok(())
        }
        async fn delete_playlist(&self, playlist_id: &str) -> Result<(), ProviderError> {
            self.delete_calls
                .lock()
                .unwrap()
                .push(playlist_id.to_string());
            Ok(())
        }
    }

    #[tokio::test]
    async fn playlist_create_resolves_song_and_returns_server_id() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);
        let song = crate::domain::models::Song {
            id: "song1".to_string(),
            title: "Track 1".to_string(),
            artist_id: None,
            artist_name: Some("Artist".to_string()),
            album_id: Some("album-1".to_string()),
            album_title: Some("Album".to_string()),
            duration_seconds: 180,
            bitrate_kbps: Some(320),
            track_number: Some(1),
            disc_number: Some(1),
            cover_art_id: None,
            date_added: None,
            last_played_at: None,
            play_count: None,
            is_favorite: None,
            content_type: Some("audio/mpeg".to_string()),
            suffix: Some("mp3".to_string()),
            size_bytes: None,
            album_loudness: Default::default(),
            provider_metadata: Default::default(),
        };
        let provider = FakePlaylistProvider::with_song("playlist-42", song);
        state
            .server_manager
            .write()
            .await
            .set_test_provider(provider.clone() as Arc<dyn MediaProvider>);

        let result = handle_playlist_create(
            &state,
            Some(serde_json::json!({ "name": "My Playlist", "itemIds": ["song1"] })),
        )
        .await
        .expect("playlist.create");

        assert_eq!(result["playlistId"], "playlist-42");
        let calls = provider.create_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "My Playlist");
        assert_eq!(calls[0].1, vec!["song1"]);
    }

    fn fake_song(id: &str) -> crate::domain::models::Song {
        crate::domain::models::Song {
            id: id.to_string(),
            title: format!("Track {id}"),
            artist_id: None,
            artist_name: Some("Artist".to_string()),
            album_id: Some("album-1".to_string()),
            album_title: Some("Album".to_string()),
            duration_seconds: 180,
            bitrate_kbps: Some(320),
            track_number: Some(1),
            disc_number: Some(1),
            cover_art_id: None,
            date_added: None,
            last_played_at: None,
            play_count: None,
            is_favorite: None,
            content_type: Some("audio/mpeg".to_string()),
            suffix: Some("mp3".to_string()),
            size_bytes: None,
            album_loudness: Default::default(),
            provider_metadata: Default::default(),
        }
    }

    #[tokio::test]
    async fn playlist_create_dedups_overlapping_container_and_track() {
        // "album-1" resolves to [song1]; "song1" also resolves standalone to the
        // same track. AC1 dedup must collapse them to a single track id.
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);
        let provider =
            FakePlaylistProvider::with_album_and_song("playlist-7", "album-1", fake_song("song1"));
        state
            .server_manager
            .write()
            .await
            .set_test_provider(provider.clone() as Arc<dyn MediaProvider>);

        let result = handle_playlist_create(
            &state,
            Some(serde_json::json!({ "name": "Dedup", "itemIds": ["album-1", "song1"] })),
        )
        .await
        .expect("playlist.create");

        assert_eq!(result["playlistId"], "playlist-7");
        let calls = provider.create_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].1,
            vec!["song1"],
            "overlapping container and track must dedup to one track id"
        );
    }

    #[tokio::test]
    async fn playlist_create_skips_unresolvable_items_and_reports_them() {
        // One valid item ("song1") and one unresolvable item ("ghost"): the
        // create must still succeed with the resolved track and report the
        // skipped id rather than aborting the whole operation.
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);
        let provider = FakePlaylistProvider::with_song("playlist-77", fake_song("song1"));
        state
            .server_manager
            .write()
            .await
            .set_test_provider(provider.clone() as Arc<dyn MediaProvider>);

        let result = handle_playlist_create(
            &state,
            Some(serde_json::json!({ "name": "Partial", "itemIds": ["song1", "ghost"] })),
        )
        .await
        .expect("playlist.create should not abort on one unresolvable item");

        assert_eq!(result["playlistId"], "playlist-77");
        assert_eq!(
            result["skippedItemIds"],
            serde_json::json!(["ghost"]),
            "unresolvable item must be reported in skippedItemIds"
        );
        let calls = provider.create_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].1, vec!["song1"]);
    }

    #[tokio::test]
    async fn playlist_create_excludes_auto_fill_slot() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);
        let provider = FakePlaylistProvider::new("playlist-99");
        state
            .server_manager
            .write()
            .await
            .set_test_provider(provider.clone() as Arc<dyn MediaProvider>);

        // Both legacy and scoped auto-fill slots are virtual and must be filtered.
        let result = handle_playlist_create(
            &state,
            Some(serde_json::json!({
                "name": "Auto",
                "itemIds": ["__auto_fill_slot__", "__auto_fill_slot__:server-id"]
            })),
        )
        .await
        .expect("playlist.create with only auto-fill slot");

        assert_eq!(result["playlistId"], "playlist-99");
        let calls = provider.create_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert!(calls[0].1.is_empty(), "auto-fill slot must be excluded");
    }

    #[tokio::test]
    async fn playlist_add_tracks_passes_ids_directly_without_resolution() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);
        let provider = FakePlaylistProvider::new("ignored");
        state
            .server_manager
            .write()
            .await
            .set_test_provider(provider.clone() as Arc<dyn MediaProvider>);

        let result = handle_playlist_add_tracks(
            &state,
            Some(serde_json::json!({ "playlistId": "p1", "trackIds": ["t1", "t2"] })),
        )
        .await
        .expect("playlist.addTracks");

        assert_eq!(result["ok"], true);
        let calls = provider.add_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "p1");
        assert_eq!(calls[0].1, vec!["t1", "t2"]);
    }

    #[tokio::test]
    async fn playlist_remove_tracks_passes_ids_directly_without_resolution() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);
        let provider = FakePlaylistProvider::new("ignored");
        state
            .server_manager
            .write()
            .await
            .set_test_provider(provider.clone() as Arc<dyn MediaProvider>);

        let result = handle_playlist_remove_tracks(
            &state,
            Some(serde_json::json!({ "playlistId": "p2", "trackIds": ["t3"] })),
        )
        .await
        .expect("playlist.removeTracks");

        assert_eq!(result["ok"], true);
        let calls = provider.remove_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "p2");
        assert_eq!(calls[0].1, vec!["t3"]);
    }

    #[tokio::test]
    async fn playlist_delete_passes_playlist_id() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);
        let provider = FakePlaylistProvider::new("ignored");
        state
            .server_manager
            .write()
            .await
            .set_test_provider(provider.clone() as Arc<dyn MediaProvider>);

        let result =
            handle_playlist_delete(&state, Some(serde_json::json!({ "playlistId": "p3" })))
                .await
                .expect("playlist.delete");

        assert_eq!(result["ok"], true);
        let calls = provider.delete_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0], "p3");
    }

    #[tokio::test]
    async fn playlist_write_rpcs_return_unsupported_when_capability_false() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let state = make_test_state(db);
        // FakeBrowseProvider has supports_playlist_write: false
        let provider = FakeBrowseProvider::new(vec![], vec![]);
        state
            .server_manager
            .write()
            .await
            .set_test_provider(provider as Arc<dyn MediaProvider>);

        let dummy_create_params = Some(serde_json::json!({ "name": "x", "itemIds": [] }));
        let dummy_modify_params = Some(serde_json::json!({ "playlistId": "p", "trackIds": [] }));
        let dummy_delete_params = Some(serde_json::json!({ "playlistId": "p" }));

        let create_err = handle_playlist_create(&state, dummy_create_params)
            .await
            .expect_err("create should fail");
        assert_eq!(create_err.code, ERR_UNSUPPORTED_CAPABILITY);

        let add_err = handle_playlist_add_tracks(&state, dummy_modify_params.clone())
            .await
            .expect_err("addTracks should fail");
        assert_eq!(add_err.code, ERR_UNSUPPORTED_CAPABILITY);

        let remove_err = handle_playlist_remove_tracks(&state, dummy_modify_params)
            .await
            .expect_err("removeTracks should fail");
        assert_eq!(remove_err.code, ERR_UNSUPPORTED_CAPABILITY);

        let delete_err = handle_playlist_delete(&state, dummy_delete_params)
            .await
            .expect_err("delete should fail");
        assert_eq!(delete_err.code, ERR_UNSUPPORTED_CAPABILITY);

        let reorder_err = handle_playlist_reorder(
            &state,
            Some(serde_json::json!({ "playlistId": "p", "trackIds": ["t1", "t2"] })),
        )
        .await
        .expect_err("reorder should fail");
        assert_eq!(reorder_err.code, ERR_UNSUPPORTED_CAPABILITY);
    }

    #[test]
    fn playback_track_tags_keep_identical_raw_ids_source_qualified() {
        let first = playback_tagged_tracks(Some("portable-a"), vec![fake_song("same-id")]);
        let second = playback_tagged_tracks(Some("portable-b"), vec![fake_song("same-id")]);
        assert_eq!(first[0]["id"], second[0]["id"]);
        assert_eq!(first[0]["serverId"], "portable-a");
        assert_eq!(second[0]["serverId"], "portable-b");
    }

    #[test]
    fn playback_album_tags_capture_portable_source_with_provider_result() {
        let album = Album {
            id: "same-id".into(),
            title: "Album".into(),
            artist_id: None,
            artist_name: None,
            year: None,
            song_count: Some(2),
            duration_seconds: None,
            cover_art_id: None,
            provider_metadata: Default::default(),
        };
        let first = playback_tagged_albums(Some("portable-a"), vec![album.clone()]);
        let second = playback_tagged_albums(Some("portable-b"), vec![album]);
        assert_eq!(first[0]["id"], second[0]["id"]);
        assert_eq!(first[0]["serverId"], "portable-a");
        assert_eq!(second[0]["serverId"], "portable-b");
    }

    #[test]
    fn album_presentation_credits_are_additive_and_never_expose_provider_ids() {
        let album = Album {
            id: "abs-album-public".into(),
            title: "Book".into(),
            artist_id: None,
            artist_name: Some("Primary author".into()),
            year: None,
            song_count: Some(1),
            duration_seconds: None,
            cover_art_id: Some("abs-cover-public".into()),
            provider_metadata: crate::domain::models::ProviderItemMetadata {
                identity: Some(crate::domain::models::ProviderIdentity {
                    library_id: "private-library".into(),
                    library_item_id: "private-item".into(),
                    media_id: "private-media".into(),
                }),
                credits: vec![crate::domain::models::Credit {
                    name: "Narrator".into(),
                    provider_id: Some("private-person".into()),
                    role: crate::domain::models::CreditRole::Narrator,
                }],
                ..Default::default()
            },
        };
        let result = playback_tagged_albums(Some("portable"), vec![album]);
        assert_eq!(result[0]["presentationCredits"][0]["name"], "Narrator");
        assert_eq!(result[0]["presentationCredits"][0]["role"], "narrator");
        assert!(result[0].get("providerId").is_none());
        assert!(!result[0].to_string().contains("private-library"));
        assert!(!result[0].to_string().contains("private-person"));
    }

    #[test]
    fn authenticated_cover_chunks_stop_at_limit_without_retaining_overflow() {
        let mut body = Vec::new();
        assert!(append_bounded_image_chunk(&mut body, &[1, 2], 3));
        assert!(append_bounded_image_chunk(&mut body, &[3], 3));
        assert!(!append_bounded_image_chunk(&mut body, &[4], 3));
        assert_eq!(body, &[1, 2, 3]);
    }
}
