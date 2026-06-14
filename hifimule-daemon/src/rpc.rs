use crate::api::{CredentialManager, JellyfinClient};
use crate::domain::models::{Album, Artist, ChangeType, ItemType, Library, Playlist, Song};
use crate::providers::{
    BrowseMode, CredentialKind, MediaProvider, ProviderCredentials, ProviderError,
    SUBSONIC_PLAYLISTS_LIBRARY_ID, ServerType, ServerTypeHint, TrackListFilter, server_type_slug,
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    response::IntoResponse,
    routing::{get, post},
};
use notify_rust::Notification;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::Arc;

// JSON-RPC 2.0 Error Codes
#[allow(dead_code)] // Reserved for future use
const ERR_PARSE_ERROR: i32 = -32700;
#[allow(dead_code)] // Reserved for future use
const ERR_INVALID_REQUEST: i32 = -32600;
const ERR_METHOD_NOT_FOUND: i32 = -32601;
const ERR_INVALID_PARAMS: i32 = -32602;
#[allow(dead_code)] // Reserved for future use
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
}

fn send_sync_complete_notification() {
    if let Err(e) = Notification::new()
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

pub async fn run_server(
    port: u16,
    db: Arc<crate::db::Database>,
    device_manager: Arc<crate::device::DeviceManager>,
    last_scrobbler_result: Arc<tokio::sync::RwLock<Option<crate::scrobbler::ScrobblerResult>>>,
    state_tx: std::sync::mpsc::Sender<crate::DaemonState>,
    sync_operation_manager: Arc<crate::sync::SyncOperationManager>,
) {
    let state = Arc::new(AppState {
        jellyfin_client: JellyfinClient::new(),
        server_manager: Arc::new(tokio::sync::RwLock::new(
            crate::server_manager::ServerManager::new(),
        )),
        db,
        device_manager,
        last_connection_check: Arc::new(tokio::sync::Mutex::new(None)),
        size_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        sync_operation_manager,
        last_scrobbler_result,
        state_tx,
    });
    // Startup (Story 2.11): migrate a legacy single-server vault to the UUID-keyed
    // multi-server vault (if needed), then load server rows into the manager.
    // Providers connect lazily on first selection/use — never eagerly here (AC14).
    if let Err(e) = CredentialManager::migrate_vault_from_legacy(&state.db) {
        eprintln!("[Startup] Vault migration failed: {}", e);
    }
    state.server_manager.write().await.load_from_db(&state.db);

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
                .allow_headers([http::header::CONTENT_TYPE]),
        )
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    println!("RPC server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<JsonRpcRequest>,
) -> Json<JsonRpcResponse> {
    let result = match payload.method.as_str() {
        "test_connection" => handle_test_connection(&state, payload.params).await,
        "server.connect" => handle_server_connect(&state, payload.params).await,
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
        "sync.setAutoFill" => handle_sync_set_auto_fill(&state, payload.params).await,
        "device_profiles.list" => handle_device_profiles_list().await,
        "device.set_transcoding_profile" => {
            handle_set_transcoding_profile(&state, payload.params).await
        }
        "device.list" => handle_device_list(&state).await,
        "device.select" => handle_device_select(&state, payload.params).await,
        "server.probe" => handle_server_probe(payload.params).await,
        "daemon.health" => Ok(serde_json::json!({ "data": { "status": "ok" } })),
        "browse.listModes" => handle_browse_list_modes(&state).await,
        "browse.listArtists" => handle_browse_list_artists(&state, payload.params).await,
        "browse.getArtist" => handle_browse_get_artist(&state, payload.params).await,
        "browse.listAlbums" => handle_browse_list_albums(&state, payload.params).await,
        "browse.getAlbum" => handle_browse_get_album(&state, payload.params).await,
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
    let provider = require_provider(state).await?;
    let result = provider
        .get_artist(&artist_id)
        .await
        .map_err(provider_error_to_rpc)?;
    Ok(serde_json::json!({ "artist": result.artist, "albums": result.albums }))
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
    let provider = require_provider(state).await?;
    let (albums, total) = provider
        .list_albums(library_id.as_deref(), letter.as_deref(), offset, limit)
        .await
        .map_err(provider_error_to_rpc)?;
    Ok(serde_json::json!({ "albums": albums, "total": total }))
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
    let provider = require_provider(state).await?;
    let result = provider
        .get_album(&album_id)
        .await
        .map_err(provider_error_to_rpc)?;
    Ok(serde_json::json!({ "album": result.album, "tracks": result.tracks }))
}

async fn handle_browse_list_playlists(state: &AppState) -> Result<Value, JsonRpcError> {
    let provider = require_provider(state).await?;
    let playlists = provider
        .list_playlists()
        .await
        .map_err(provider_error_to_rpc)?;
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
    let provider = require_provider(state).await?;
    let result = provider
        .get_playlist(&playlist_id)
        .await
        .map_err(provider_error_to_rpc)?;
    Ok(serde_json::json!({ "playlist": result.playlist, "tracks": result.tracks }))
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
    let provider = require_provider(state).await?;
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
    Ok(serde_json::json!({ "genre": genre, "tracks": tracks, "total": total }))
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
    let provider = require_provider(state).await?;
    let (albums, total) = provider
        .list_recently_added(library_id.as_deref(), offset, limit)
        .await
        .map_err(provider_error_to_rpc)?;
    let total = total as u64;
    Ok(serde_json::json!({ "albums": albums, "total": total }))
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
    let provider = require_provider(state).await?;
    let (tracks, total) = provider
        .list_frequently_played(library_id.as_deref(), offset, limit)
        .await
        .map_err(provider_error_to_rpc)?;
    let total = total as u64;
    Ok(serde_json::json!({ "tracks": tracks, "total": total }))
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
    let provider = require_provider(state).await?;
    let (tracks, total) = provider
        .list_recently_played(library_id.as_deref(), offset, limit)
        .await
        .map_err(provider_error_to_rpc)?;
    let total = total as u64;
    Ok(serde_json::json!({ "tracks": tracks, "total": total }))
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
    let provider = require_provider(state).await?;
    let (tracks, total) = provider
        .list_favorites(library_id.as_deref(), offset, limit)
        .await
        .map_err(provider_error_to_rpc)?;
    let total = total as u64;
    Ok(serde_json::json!({ "tracks": tracks, "total": total }))
}

async fn handle_browse_list_favorite_items(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let library_id = params
        .as_ref()
        .and_then(|p| p["libraryId"].as_str())
        .map(str::to_owned);
    let provider = require_provider(state).await?;
    let favorites = provider
        .list_favorite_items(library_id.as_deref())
        .await
        .map_err(provider_error_to_rpc)?;
    Ok(serde_json::json!({
        "artists": favorites.artists,
        "albums": favorites.albums,
        "tracks": favorites.songs,
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
    let provider = require_provider(state).await?;
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
    Ok(serde_json::json!({
        "tracks": page.tracks,
        "total": page.total,
        "startIndex": page.start_index,
        "limit": page.limit,
    }))
}

async fn handle_browse_search(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let provider = require_provider(state).await?;
    let query = params
        .as_ref()
        .and_then(|p| p["query"].as_str())
        .ok_or(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Missing query".to_string(),
            data: None,
        })?
        .to_owned();
    // An empty/whitespace query would be forwarded to the provider as an
    // unbounded search; short-circuit to an empty result set instead.
    if query.trim().is_empty() {
        return Ok(serde_json::json!({ "tracks": [] }));
    }
    let result = provider
        .search(&query)
        .await
        .map_err(provider_error_to_rpc)?;
    Ok(serde_json::json!({ "tracks": result.songs }))
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
        .filter(|id| id != "__auto_fill_slot__")
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

/// Full logout (UI "log out" / disconnect): removes ALL configured servers,
/// clears the vault and config, and resets the in-memory manager.
async fn handle_server_logout(state: &AppState) -> Result<Value, JsonRpcError> {
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

async fn handle_get_credentials(state: &AppState) -> Result<Value, JsonRpcError> {
    match CredentialManager::get_credentials() {
        Ok((url, token, user_id)) => Ok(serde_json::json!({
            "url": url,
            "token": token,
            "userId": user_id
        })),
        Err(e) => {
            let msg = e.to_string();
            // "No config file found" and "No token found in keyring" are expected
            // "not yet configured" states — return null so callers show the login screen.
            // All other errors (I/O failure, corrupted config, keyring access denied) are
            // real storage faults and should be surfaced so they can be diagnosed.
            if msg.starts_with("No config file found") || msg.contains("No token found") {
                if let Ok(Some(config)) = state.db.get_server_config() {
                    return Ok(serde_json::json!({
                        "url": config.url,
                        "token": null,
                        "userId": config.username,
                        "serverType": config.server_type,
                        "serverVersion": config.server_version,
                    }));
                }
                Ok(Value::Null)
            } else {
                Err(JsonRpcError {
                    code: ERR_STORAGE_ERROR,
                    message: msg,
                    data: None,
                })
            }
        }
    }
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
    let pending_device_snapshot = state
        .device_manager
        .get_unrecognized_device_snapshot()
        .await;
    let pending_device_path = pending_device_snapshot
        .as_ref()
        .map(|s| s.path.to_string_lossy().to_string());
    let pending_device_friendly_name = pending_device_snapshot.and_then(|s| s.friendly_name);

    let auto_sync_on_connect = device
        .as_ref()
        .map(|d| d.auto_sync_on_connect)
        .or_else(|| mapping.as_ref().map(|m| m.auto_sync_on_connect))
        .unwrap_or(false);

    // Story 12.2: auto_fill is now a per-server pipeline map. Resolve the selected server's
    // portable id and read its slot via the server-aware accessors; the emitted JSON shape
    // `{ enabled, maxBytes }` is unchanged (UI is Story 12.6).
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
        })
    });

    let (connected_devices_snapshot, selected_path_buf) =
        state.device_manager.get_multi_device_snapshot().await;
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
        "autoSyncOnConnect": auto_sync_on_connect,
        "autoFill": auto_fill,
        "activeOperationId": active_operation_id,
        "connectedDevices": connected_devices_json,
        "selectedDevicePath": selected_device_path,
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
    let portable_known: HashSet<&str> =
        servers.iter().filter_map(|s| s.server_id.as_deref()).collect();
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
    let selected_portable: Option<String> =
        servers.iter().find(|s| s.selected).and_then(|s| s.server_id.clone());

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
    track
        .bitrate_kbps
        .map(|kbps| (u64::from(kbps) * 1_000 / 8) * u64::from(track.duration_seconds))
        .unwrap_or(0)
}

fn provider_song_to_desired_item(song: &Song) -> crate::sync::DesiredItem {
    crate::sync::DesiredItem {
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

async fn provider_calculate_delta(
    _state: &AppState,
    provider: Arc<dyn MediaProvider>,
    item_ids: &[String],
    manifest: &crate::device::DeviceManifest,
    params: &Value,
) -> Result<Value, JsonRpcError> {
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
            match _state.device_manager.get_device_storage().await {
                Some(info) => {
                    crate::daemon_log!(
                        "[AutoFill] no maxBytes from UI — server fallback: free={} synced={} basket_est={} -> budget={}",
                        info.free_bytes,
                        synced_bytes,
                        basket_size,
                        info.free_bytes
                            .saturating_add(synced_bytes)
                            .saturating_sub(basket_size)
                    );
                    info.free_bytes
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
            let fill_params = crate::auto_fill::AutoFillParams {
                exclude_item_ids: exclude_ids,
                max_fill_bytes: auto_fill_budget,
            };
            // Story 12.4: route the selected server's slot through the shared seam, reading its
            // configured pipeline (portable serverId). Default-equivalent → fast path (AC 8).
            let selected_portable = current_server_portable_id(_state).ok().flatten();
            let pipeline_opt = selected_portable
                .as_deref()
                .and_then(|id| manifest.auto_fill.pipeline_for(id));
            let fill_items = expand_auto_fill_slot(provider, pipeline_opt, fill_params)
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
                if seen_ids.insert(item.id.clone()) {
                    desired_items.push(crate::sync::DesiredItem {
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
                        track_number: None,
                        server_id: None,
                    });
                }
            }
        }
    }

    // Story 2.13: tag untagged items with the selected server's portable id.
    tag_untagged_with_selected_portable(_state, &mut desired_items)?;

    crate::daemon_log!(
        "[Delta] Provider desired set prepared: desired_items={} playlists={}; calculating manifest delta",
        desired_items.len(),
        playlist_sync_items.len()
    );
    let mut delta = crate::sync::calculate_delta(&desired_items, manifest);
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

    if let Some((_, device_io)) = _state.device_manager.get_manifest_and_io().await {
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
    remaining: &mut Option<u64>,
) -> u64 {
    let mut added: u64 = 0;
    for item in fill_items {
        if seen_ids.insert(item.id.clone()) {
            let size = item.size_bytes;
            desired_items.push(crate::sync::DesiredItem {
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
                track_number: None,
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

/// The single shared slot-expansion seam (Story 12.4): every sync-time auto-fill
/// expansion site routes through this so the configurable-vs-default decision lives in
/// exactly one place. When `pipeline` is a configured NON-default pipeline, materialize
/// its pools and run the pure engine (`expand_with_pipeline`); otherwise keep the smart
/// incremental default path (`run_auto_fill_provider`) — byte-for-byte unchanged (AC 8).
async fn expand_auto_fill_slot(
    provider: Arc<dyn MediaProvider>,
    pipeline: Option<&crate::auto_fill::AutoFillPipeline>,
    params: crate::auto_fill::AutoFillParams,
) -> anyhow::Result<Vec<crate::auto_fill::AutoFillItem>> {
    match pipeline {
        Some(p) if crate::auto_fill::needs_configurable_expansion(p) => {
            crate::auto_fill::expand_with_pipeline(provider, p, params).await
        }
        _ => crate::auto_fill::run_auto_fill_provider(provider, params).await,
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
    if !descriptors.is_empty() {
        // Shared budget = device free + already-synced − already-selected bytes
        // (mirrors `provider_calculate_delta`'s server-side derivation at
        // rpc.rs:2599-2616). `None` when device storage is unavailable; that is
        // tolerated only for slots that supply their own `maxBytes` — matching the
        // pre-12.3 multi path, which used the UI `maxBytes` without querying storage.
        let selected_bytes: u64 = desired_items.iter().map(|i| i.size_bytes).sum();
        let mut remaining: Option<u64> = match state.device_manager.get_device_storage().await {
            Some(info) => {
                let synced: u64 = manifest.synced_items.iter().map(|s| s.size_bytes).sum();
                Some(
                    info.free_bytes
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
            let fill_params = crate::auto_fill::AutoFillParams {
                exclude_item_ids: exclude_ids,
                max_fill_bytes: budget,
            };
            // Story 12.4: route this slot through the shared seam — the configurable engine
            // when this server has a non-default pipeline, else the default fast path.
            let pipeline_opt = manifest.auto_fill.pipeline_for(&af_server);
            let fill_items = match expand_auto_fill_slot(provider, pipeline_opt, fill_params).await {
                Ok(items) => items,
                Err(e) => {
                    crate::daemon_log!(
                        "[AutoFill] skipping slot for server {}: expansion failed: {}",
                        af_server,
                        e
                    );
                    continue;
                }
            };
            push_fill_items_dedup(
                fill_items,
                &mut desired_items,
                &mut seen_ids,
                &af_server,
                &mut remaining,
            );
        }
    }

    let mut delta = crate::sync::calculate_delta(&desired_items, manifest);
    delta.playlists = playlist_sync_items;
    if let Some((_, device_io)) = state.device_manager.get_manifest_and_io().await {
        crate::sync::augment_delta_with_existence_check(
            &mut delta,
            &desired_items,
            manifest,
            device_io.as_ref(),
        )
        .await;
    }
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
        return multi_provider_calculate_delta(state, &item_specs, &manifest, &params).await;
    }

    if let Some(provider) = active_non_jellyfin_provider(state).await {
        return provider_calculate_delta(state, provider, &item_ids, &manifest, &params).await;
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
    if let Some(desc) = parse_auto_fill_descriptors(&params).into_iter().next() {
        let max_fill_bytes = if let Some(mb) = desc.max_bytes {
            mb
        } else {
            match state.device_manager.get_device_storage().await {
                Some(info) => info.free_bytes,
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
        let fill_params = crate::auto_fill::AutoFillParams {
            exclude_item_ids: expanded_excludes,
            max_fill_bytes,
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
                    if seen_ids.insert(item.id.clone()) {
                        desired_items.push(crate::sync::DesiredItem {
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
                            track_number: None,
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

    if let Some((_, device_io)) = state.device_manager.get_manifest_and_io().await {
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
    let manifest = state
        .device_manager
        .get_current_device()
        .await
        .ok_or(JsonRpcError {
            code: ERR_CONNECTION_FAILED,
            message: "No device connected".to_string(),
            data: None,
        })?;

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

    let destructive_cleanup_count = crate::sync::destructive_cleanup_count(&delta, &manifest);
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

    // Get current device path
    let device_path = state
        .device_manager
        .get_current_device_path()
        .await
        .ok_or(JsonRpcError {
            code: ERR_CONNECTION_FAILED,
            message: "No device connected".to_string(),
            data: None,
        })?;

    // Guard: reject if a sync is already running (covers the auto-sync race window)
    if state.sync_operation_manager.has_active_operation().await {
        return Err(JsonRpcError {
            code: ERR_SYNC_IN_PROGRESS,
            message: "A sync operation is already in progress".to_string(),
            data: None,
        });
    }

    // Generate unique operation ID
    let operation_id = uuid::Uuid::new_v4().to_string();

    // Create operation in manager
    let total_files = delta.adds.len() + delta.deletes.len();
    state
        .sync_operation_manager
        .create_operation(operation_id.clone(), total_files)
        .await;

    // Mark manifest dirty before sync starts — enables interrupted-sync detection (Story 4.4)
    // Failing to mark dirty MUST abort the sync to prevent undetectable interruptions.
    if let Err(e) = state
        .device_manager
        .update_manifest(|m| {
            m.dirty = true;
            m.pending_item_ids = pending_item_ids.clone();
        })
        .await
    {
        return Err(JsonRpcError {
            code: ERR_STORAGE_ERROR,
            message: format!("Failed to mark manifest dirty, aborting sync: {}", e),
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
    if needs_provider_routing {
        // Resolve every group's provider up front so connection errors surface here.
        let mut group_providers: Vec<(String, Arc<dyn MediaProvider>)> = Vec::new();
        for sid in &add_servers {
            let provider = get_provider_by_server_id_for(state, sid).await?;
            group_providers.push((sid.clone(), provider));
        }

        let op_manager = state.sync_operation_manager.clone();
        let op_id = operation_id.clone();
        let device_manager = state.device_manager.clone();
        let state_tx = state.state_tx.clone();
        let _ = state_tx.send(crate::DaemonState::Syncing);

        tokio::spawn(async move {
            let (sync_manifest, device_io) = match device_manager.get_manifest_and_io().await {
                Some(pair) => pair,
                None => {
                    eprintln!("[Sync] No device available — cannot execute multi-server sync");
                    fail_sync_operation(
                        &op_manager,
                        &op_id,
                        "sync_execute",
                        "No device".to_string(),
                    )
                    .await;
                    let _ = state_tx.send(crate::DaemonState::Error);
                    return;
                }
            };
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

            let mut all_errors: Vec<crate::sync::SyncFileError> = Vec::new();
            let mut first = true;
            for (sid, provider) in group_providers {
                if op_manager.is_cancelled(&op_id).await {
                    break;
                }
                // Each group syncs its own adds; deletes/id-changes/playlists are
                // device-wide and run once, with the first group.
                let group_adds: Vec<crate::sync::SyncAddItem> = delta
                    .adds
                    .iter()
                    .filter(|a| a.server_id.as_deref() == Some(sid.as_str()))
                    .cloned()
                    .collect();
                let sub_delta = crate::sync::SyncDelta {
                    adds: group_adds,
                    deletes: if first {
                        delta.deletes.clone()
                    } else {
                        Vec::new()
                    },
                    id_changes: if first {
                        delta.id_changes.clone()
                    } else {
                        Vec::new()
                    },
                    unchanged: 0,
                    playlists: if first {
                        delta.playlists.clone()
                    } else {
                        Vec::new()
                    },
                };
                first = false;
                let result = crate::sync::execute_provider_sync(
                    &sub_delta,
                    &device_path,
                    crate::sync::ProviderSyncSource {
                        provider,
                        transcoding_profile: transcoding_profile.clone(),
                    },
                    op_manager.clone(),
                    op_id.clone(),
                    device_manager.clone(),
                    device_io.clone(),
                )
                .await;
                match result {
                    Ok((_synced, errors)) => all_errors.extend(errors),
                    Err(e) => all_errors.push(crate::sync::SyncFileError {
                        jellyfin_id: String::new(),
                        filename: format!("sync_execute[{sid}]"),
                        error_message: e.to_string(),
                    }),
                }
            }

            if op_manager.is_cancelled(&op_id).await {
                if let Some(mut operation) = op_manager.get_operation(&op_id).await {
                    operation.status = crate::sync::SyncStatus::Cancelled;
                    op_manager.update_operation(&op_id, operation).await;
                }
                let _ = state_tx.send(crate::DaemonState::Idle);
                return;
            }
            if let Err(e) = device_manager
                .update_manifest(|m| {
                    m.dirty = false;
                    m.pending_item_ids = vec![];
                    if all_errors.is_empty() {
                        m.last_synced_transcoding_profile_id = m.transcoding_profile_id.clone();
                        m.transcoding_profile_dirty = false;
                    }
                })
                .await
            {
                eprintln!("Failed to clear dirty flag on final manifest: {}", e);
            }
            if let Some(mut operation) = op_manager.get_operation(&op_id).await {
                operation.status = if all_errors.is_empty() {
                    crate::sync::SyncStatus::Complete
                } else {
                    crate::sync::SyncStatus::Failed
                };
                operation.errors = all_errors.clone();
                op_manager.update_operation(&op_id, operation).await;
            }
            if all_errors.is_empty() {
                drop(tokio::task::spawn_blocking(send_sync_complete_notification));
            }
            let _ = state_tx.send(crate::DaemonState::Idle);
        });

        return Ok(serde_json::json!({ "operationId": operation_id }));
    }

    if let Some(provider) = active_non_jellyfin_provider(state).await {
        let op_manager = state.sync_operation_manager.clone();
        let op_id = operation_id.clone();
        let device_manager = state.device_manager.clone();
        let state_tx = state.state_tx.clone();
        let _ = state_tx.send(crate::DaemonState::Syncing);

        tokio::spawn(async move {
            let (sync_manifest, device_io) = match device_manager.get_manifest_and_io().await {
                Some(pair) => pair,
                None => {
                    eprintln!("[Sync] No device available — cannot execute sync");
                    if let Some(mut operation) = op_manager.get_operation(&op_id).await {
                        operation.status = crate::sync::SyncStatus::Failed;
                        op_manager.update_operation(&op_id, operation).await;
                    }
                    let _ = state_tx.send(crate::DaemonState::Error);
                    return;
                }
            };

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
                &device_path,
                crate::sync::ProviderSyncSource {
                    provider,
                    transcoding_profile,
                },
                op_manager.clone(),
                op_id.clone(),
                device_manager.clone(),
                device_io,
            )
            .await;

            match result {
                Ok((_synced_items, errors)) => {
                    if op_manager.is_cancelled(&op_id).await {
                        if let Some(mut operation) = op_manager.get_operation(&op_id).await {
                            operation.status = crate::sync::SyncStatus::Cancelled;
                            op_manager.update_operation(&op_id, operation).await;
                        }
                        let _ = state_tx.send(crate::DaemonState::Idle);
                        return;
                    }

                    if let Err(e) = device_manager
                        .update_manifest(|m| {
                            m.dirty = false;
                            m.pending_item_ids = vec![];
                            if errors.is_empty() {
                                m.last_synced_transcoding_profile_id =
                                    m.transcoding_profile_id.clone();
                                m.transcoding_profile_dirty = false;
                            }
                        })
                        .await
                    {
                        eprintln!("Failed to clear dirty flag on final manifest: {}", e);
                    }
                    if let Some(mut operation) = op_manager.get_operation(&op_id).await {
                        operation.status = if errors.is_empty() {
                            crate::sync::SyncStatus::Complete
                        } else {
                            crate::sync::SyncStatus::Failed
                        };
                        operation.errors = errors.clone();
                        op_manager.update_operation(&op_id, operation).await;
                    }
                    if errors.is_empty() {
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

    // Get credentials
    let (url, token, user_id) = CredentialManager::get_credentials().map_err(|e| JsonRpcError {
        code: ERR_STORAGE_ERROR,
        message: format!("Failed to get credentials: {}", e),
        data: None,
    })?;
    let user_id = user_id.unwrap_or_else(|| "Me".to_string());

    // Spawn background task to execute sync
    let jellyfin_client = state.jellyfin_client.clone();
    let op_manager = state.sync_operation_manager.clone();
    let op_id = operation_id.clone();
    let device_manager = state.device_manager.clone();
    let state_tx = state.state_tx.clone();
    let _ = state_tx.send(crate::DaemonState::Syncing);

    tokio::spawn(async move {
        // Atomically fetch manifest + IO backend to avoid TOCTOU if device disconnects
        // between reading the manifest (for transcoding profile) and getting the IO handle.
        let (sync_manifest, device_io) = match device_manager.get_manifest_and_io().await {
            Some(pair) => pair,
            None => {
                eprintln!("[Sync] No device available — cannot execute sync");
                if let Some(mut operation) = op_manager.get_operation(&op_id).await {
                    operation.status = crate::sync::SyncStatus::Failed;
                    op_manager.update_operation(&op_id, operation).await;
                }
                let _ = state_tx.send(crate::DaemonState::Error);
                return;
            }
        };

        // Load transcoding profile from the atomically fetched manifest
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

        let result = crate::sync::execute_sync(
            &delta,
            &device_path,
            &jellyfin_client,
            &url,
            &token,
            &user_id,
            op_manager.clone(),
            op_id.clone(),
            device_manager.clone(),
            transcoding_profile,
            device_io,
        )
        .await;

        match result {
            Ok((_synced_items, errors)) => {
                if op_manager.is_cancelled(&op_id).await {
                    if let Some(mut operation) = op_manager.get_operation(&op_id).await {
                        operation.status = crate::sync::SyncStatus::Cancelled;
                        op_manager.update_operation(&op_id, operation).await;
                    }
                    let _ = state_tx.send(crate::DaemonState::Idle);
                    return;
                }

                // Clear dirty flag after sync completes — per-file updates already wrote all items (Story 4.4)
                if let Err(e) = device_manager
                    .update_manifest(|m| {
                        m.dirty = false;
                        m.pending_item_ids = vec![];
                        if errors.is_empty() {
                            m.last_synced_transcoding_profile_id = m.transcoding_profile_id.clone();
                            m.transcoding_profile_dirty = false;
                        }
                    })
                    .await
                {
                    eprintln!("Failed to clear dirty flag on final manifest: {}", e);
                }

                // Update operation status
                if let Some(mut operation) = op_manager.get_operation(&op_id).await {
                    operation.status = if errors.is_empty() {
                        crate::sync::SyncStatus::Complete
                    } else {
                        crate::sync::SyncStatus::Failed
                    };
                    operation.errors = errors.clone();
                    op_manager.update_operation(&op_id, operation).await;
                }

                // Notify OS and return tray to Idle — Story 5.3
                if errors.is_empty() {
                    // JoinHandle intentionally dropped: fire-and-forget per AC #4.
                    // Err(e) path inside the function logs the failure; panics are silently
                    // absorbed, which is acceptable for a best-effort OS notification.
                    drop(tokio::task::spawn_blocking(send_sync_complete_notification));
                }
                let _ = state_tx.send(crate::DaemonState::Idle);
            }
            Err(e) => {
                // Mark operation as failed
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

    // Return operation ID immediately
    Ok(serde_json::json!({
        "operationId": operation_id
    }))
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

    let operation_id = params["operationId"].as_str().ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing operationId".to_string(),
        data: None,
    })?;

    let found = state
        .sync_operation_manager
        .request_cancel(operation_id)
        .await;

    if found {
        Ok(serde_json::json!({ "cancelled": true }))
    } else {
        Err(JsonRpcError {
            code: ERR_NOT_FOUND,
            message: format!("No operation with id '{}'", operation_id),
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
                    crate::device::cleanup_tmp_files(device_io, &manifest.managed_paths)
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
        let image_url = match provider.cover_art_url(&id).await {
            Ok(url) => url,
            Err(_) => return http::StatusCode::NOT_FOUND.into_response(),
        };
        let response = match reqwest::Client::new().get(image_url).send().await {
            Ok(response) => response,
            Err(_) => return http::StatusCode::BAD_GATEWAY.into_response(),
        };
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
    playlist_folder_path: Option<Option<String>>,
) -> ManifestUpdateOutcome {
    let old_music = manifest.managed_paths.first().cloned();
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

    let new_music = manifest.managed_paths.first().cloned();
    let new_playlist = manifest.resolved_playlist_path().map(str::to_string);
    let music_changed = old_music != new_music;
    let playlist_changed = old_playlist != new_playlist;

    if music_changed || playlist_changed {
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
        if playlist_changed && let Some(old_playlist) = old_playlist.as_deref() {
            for entry in &mut manifest.playlists {
                entry.filename = playlist_filename_with_folder(old_playlist, &entry.filename);
            }
        }
    }

    let tracks_to_remove = if music_changed {
        new_music
            .as_deref()
            .map(|folder| {
                manifest
                    .synced_items
                    .iter()
                    .filter(|item| !path_in_or_equal(&item.local_path, folder))
                    .count()
            })
            .unwrap_or(0)
    } else {
        0
    };
    let bytes_to_remove = if music_changed {
        new_music
            .as_deref()
            .map(|folder| {
                manifest
                    .synced_items
                    .iter()
                    .filter(|item| !path_in_or_equal(&item.local_path, folder))
                    .map(|item| item.size_bytes)
                    .sum()
            })
            .unwrap_or(0)
    } else {
        0
    };
    let playlists_to_remove = if playlist_changed {
        manifest.playlists.len()
    } else {
        0
    };

    ManifestUpdateOutcome {
        relocation_required: music_changed || playlist_changed,
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

    let device_io = state
        .device_manager
        .get_unrecognized_device_io()
        .await
        .ok_or(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "No unrecognized device pending initialization".to_string(),
            data: None,
        })?;

    let manifest = state
        .device_manager
        .initialize_device(
            folder_path,
            playlist_folder_path.as_deref(),
            transcoding_profile_id.clone(),
            device_name,
            device_icon,
            device_io,
        )
        .await
        .map_err(|e| JsonRpcError {
            code: ERR_STORAGE_ERROR,
            message: format!("Failed to initialize device: {}", e),
            data: None,
        })?;

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
/// Params: { deviceId: string, maxBytes?: number, excludeItemIds: string[] }
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

    // Determine available capacity for auto-fill
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

    let fill_params = crate::auto_fill::AutoFillParams {
        exclude_item_ids: expanded_exclude_ids,
        max_fill_bytes,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::credential_test_lock;
    use serde_json::json;
    use std::sync::Mutex;

    fn make_test_state(db: Arc<crate::db::Database>) -> Arc<AppState> {
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
        })
    }

    fn manifest_for_update() -> crate::device::DeviceManifest {
        crate::device::DeviceManifest {
            device_id: "dev-1".to_string(),
            name: Some("Device".to_string()),
            icon: Some("usb-drive".to_string()),
            version: "1.0".to_string(),
            managed_paths: vec!["Music".to_string()],
            playlist_path: None,
            synced_items: vec![crate::device::SyncedItem {
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
        assert_eq!(config.server_id.as_deref(), Some(expected_portable.as_str()));
        assert_eq!(result["serverId"], json!(expected_portable));
        assert_eq!(result["localId"], json!(config.id));

        // get_daemon_state surfaces the portable id on each server row and as
        // selectedServerPortableId; server.list rows carry serverId too.
        let state_json = handle_get_daemon_state(&state).await.unwrap();
        assert_eq!(state_json["selectedServerPortableId"], json!(expected_portable));
        assert_eq!(state_json["servers"][0]["serverId"], json!(expected_portable));
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
            assert_eq!(it.server_id.as_deref(), Some("portable-1"), "item {}", it.id);
        }

        // Idempotent: a second pass over already-portable items is a no-op.
        let again = reconcile_basket_server_ids(out.clone(), &servers);
        assert_eq!(again, out);
    }

    #[tokio::test]
    async fn test_rpc_login_uses_auto_detection_for_subsonic() {
        let _lock = credential_test_lock();
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
            .match_header("X-Emby-Token", token)
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
    async fn test_get_credentials_falls_back_to_server_config_for_subsonic_device_init() {
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
        let state = make_test_state(db);

        let result = handle_get_credentials(&state)
            .await
            .expect("server config identity should be returned");

        assert_eq!(result["url"], "http://subsonic.example");
        assert_eq!(result["token"], Value::Null);
        assert_eq!(result["userId"], "subsonic-user");
        assert_eq!(result["serverType"], "subsonic");
        assert_eq!(result["serverVersion"], "1.16.1");
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
        assert!(
            arr.iter()
                .any(|s| s["id"] == local_a.as_str()
                    && s["serverId"] == id_a.as_str()
                    && s["selected"] == true)
        );

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
        assert_eq!(d[0].exclude_item_ids, vec!["x".to_string(), "y".to_string()]);

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
        fn fill_item(id: &str, size: u64) -> crate::auto_fill::AutoFillItem {
            crate::auto_fill::AutoFillItem {
                id: id.to_string(),
                name: format!("name-{id}"),
                album: None,
                artist: None,
                provider_album_id: None,
                provider_content_type: None,
                provider_suffix: None,
                size_bytes: size,
                priority_reason: "test".to_string(),
            }
        }

        // Manual item "m1" (100 bytes) already resolved on server s1.
        let mut desired_items: Vec<crate::sync::DesiredItem> = vec![crate::sync::DesiredItem {
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

        // Slot 1 (s1): returns the manual id (must be skipped — manual wins) plus
        // two new tracks (300 + 200).
        let added1 = push_fill_items_dedup(
            vec![fill_item("m1", 100), fill_item("f1", 300), fill_item("f2", 200)],
            &mut desired_items,
            &mut seen_ids,
            "s1",
            &mut remaining,
        );
        assert_eq!(added1, 500, "only the two new tracks count");
        assert_eq!(desired_items.len(), 3, "manual + f1 + f2; m1 not duplicated");
        assert_eq!(
            desired_items.iter().filter(|i| i.jellyfin_id == "m1").count(),
            1,
            "manual item present exactly once"
        );
        assert_eq!(remaining, Some(500), "budget decremented by 500");
        // f1/f2 tagged with the slot's server.
        assert!(desired_items
            .iter()
            .filter(|i| i.jellyfin_id == "f1" || i.jellyfin_id == "f2")
            .all(|i| i.server_id.as_deref() == Some("s1")));

        // Slot 2 (s2) with a large maxBytes: the shared remaining (500) truncates
        // it — this is the budget the loop passes to run_auto_fill_provider.
        let slot2_max: Option<u64> = Some(10_000);
        let r = remaining.expect("budget present");
        let slot2_budget = slot2_max.map_or(r, |mb| mb.min(r));
        assert_eq!(slot2_budget, 500, "tiny shared budget caps the second slot");

        // Slot 2 returns f1 (cross-slot dup → skipped) and a new f3 (400 bytes).
        let added2 = push_fill_items_dedup(
            vec![fill_item("f1", 300), fill_item("f3", 400)],
            &mut desired_items,
            &mut seen_ids,
            "s2",
            &mut remaining,
        );
        assert_eq!(added2, 400, "f1 already seen from slot 1; only f3 added");
        assert_eq!(desired_items.len(), 4, "manual + f1 + f2 + f3");
        assert_eq!(
            desired_items.iter().filter(|i| i.jellyfin_id == "f1").count(),
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
            .match_header("X-Emby-Token", token)
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
            .match_header("X-Emby-Token", token)
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
            .match_header("X-Emby-Token", token)
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
        };

        // No unrecognized device registered → ERR_INVALID_PARAMS (caught before reaching storage)
        let params = json!({ "folderPath": "", "profileId": "user-1", "name": "My Device" });
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
        };

        // Initialize with empty folderPath (device root)
        let params = json!({ "folderPath": "", "profileId": "user-abc", "name": "My Device" });
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
        };

        // Initialize with a subfolder
        let params = json!({ "folderPath": "Music", "profileId": "user-xyz", "name": "My Device" });
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
        };
        let result = handle_get_daemon_state(&state).await.unwrap();
        assert_eq!(
            result["activeOperationId"],
            serde_json::Value::Null,
            "No running operation → activeOperationId must be null"
        );
        assert_eq!(result["serverType"], serde_json::Value::Null);
        assert_eq!(result["serverVersion"], serde_json::Value::Null);

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
            .match_header("X-Emby-Token", token)
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
            })
        }
    }

    #[async_trait::async_trait]
    impl MediaProvider for FakeBrowseProvider {
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
            _: u32,
            _: u32,
        ) -> Result<(Vec<crate::domain::models::Album>, u32), ProviderError> {
            unimplemented!()
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
                },
                tracks: vec![song.clone()],
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
        }
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
                    },
                    tracks: tracks.clone(),
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

        // Only item is the auto-fill slot — should be filtered; create_playlist called with empty list.
        let result = handle_playlist_create(
            &state,
            Some(serde_json::json!({ "name": "Auto", "itemIds": ["__auto_fill_slot__"] })),
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
}
