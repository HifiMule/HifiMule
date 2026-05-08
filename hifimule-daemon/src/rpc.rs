use crate::api::{CredentialManager, JellyfinClient};
use axum::{
    extract::{Path, State},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
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
        .summary("Sync Complete. Ready to Run.")
        .show()
    {
        eprintln!("[Notification] Failed to show OS notification: {}", e);
    }
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
        db,
        device_manager,
        last_connection_check: Arc::new(tokio::sync::Mutex::new(None)),
        size_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        sync_operation_manager,
        last_scrobbler_result,
        state_tx,
    });

    let app = Router::new()
        .route("/", post(handler))
        .route("/jellyfin/image/{id}", get(handle_proxy_image))
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
        "login" => handle_login(&state, payload.params).await,
        "save_credentials" => handle_save_credentials(payload.params).await,
        "get_credentials" => handle_get_credentials().await,
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
        "sync_execute" => handle_sync_execute(&state, payload.params).await,
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
        "daemon.health" => Ok(serde_json::json!({ "data": { "status": "ok" } })),
        _ => Err(JsonRpcError {
            code: ERR_METHOD_NOT_FOUND,
            message: "Method not found".to_string(),
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
        message: "Missing url".to_string(),
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

async fn handle_login(state: &AppState, params: Option<Value>) -> Result<Value, JsonRpcError> {
    let params = params.ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Invalid params".to_string(),
        data: None,
    })?;

    let url = params["url"].as_str().ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing url".to_string(),
        data: None,
    })?;

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

    match state
        .jellyfin_client
        .authenticate_by_name(url, username, password)
        .await
    {
        Ok(result) => {
            if let Err(e) = CredentialManager::save_credentials(
                url,
                &result.access_token,
                Some(&result.user.id),
            ) {
                return Err(JsonRpcError {
                    code: ERR_STORAGE_ERROR,
                    message: e.to_string(),
                    data: None,
                });
            }
            Ok(serde_json::to_value(result).unwrap())
        }
        Err(e) => Err(JsonRpcError {
            code: ERR_INVALID_CREDENTIALS,
            message: e.to_string(),
            data: None,
        }),
    }
}

async fn handle_save_credentials(params: Option<Value>) -> Result<Value, JsonRpcError> {
    let params = params.ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Invalid params".to_string(),
        data: None,
    })?;

    let url = params["url"].as_str().ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing url".to_string(),
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

async fn handle_get_credentials() -> Result<Value, JsonRpcError> {
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

    let auto_sync_on_connect = mapping
        .as_ref()
        .map(|m| m.auto_sync_on_connect)
        .unwrap_or(false);

    let auto_fill = device.as_ref().map(|d| {
        serde_json::json!({
            "enabled": d.auto_fill.enabled,
            "maxBytes": d.auto_fill.max_bytes,
        })
    });

    let active_operation_id = state.sync_operation_manager.get_active_operation_id().await;

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
                "deviceClass": match class {
                    crate::device::DeviceClass::Msc => "msc",
                    crate::device::DeviceClass::Mtp => "mtp",
                },
            })
        })
        .collect();

    Ok(serde_json::json!({
        "currentDevice": device,
        "deviceMapping": mapping,
        "serverConnected": server_connected,
        "dirtyManifest": dirty,
        "pendingDevicePath": pending_device_path,
        "pendingDeviceFriendlyName": pending_device_friendly_name,
        "autoSyncOnConnect": auto_sync_on_connect,
        "autoFill": auto_fill,
        "activeOperationId": active_operation_id,
        "connectedDevices": connected_devices_json,
        "selectedDevicePath": selected_device_path,
    }))
}

async fn handle_manifest_get_basket(state: &AppState) -> Result<Value, JsonRpcError> {
    let device = state.device_manager.get_current_device().await;
    let basket_items = device
        .as_ref()
        .map(|d| d.basket_items.clone())
        .unwrap_or_default();
    Ok(serde_json::json!({
        "basketItems": basket_items
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
    const CACHE_DURATION_SECS: u64 = 5;

    let mut cache = state.last_connection_check.lock().await;

    // Check if we have a recent cached result
    if let Some((timestamp, result)) = *cache {
        if timestamp.elapsed().as_secs() < CACHE_DURATION_SECS {
            return result;
        }
    }

    // Perform actual connection check
    let is_connected = match CredentialManager::get_credentials() {
        Ok((url, token, _)) => {
            // Actually test the connection
            state
                .jellyfin_client
                .test_connection(&url, &token)
                .await
                .is_ok()
        }
        Err(_) => false,
    };

    // Update cache
    *cache = Some((std::time::Instant::now(), is_connected));

    is_connected
}

async fn handle_jellyfin_get_views(
    state: &AppState,
    _params: Option<Value>,
) -> Result<Value, JsonRpcError> {
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
    let (url, token, user_id) = CredentialManager::get_credentials().map_err(|e| JsonRpcError {
        code: ERR_STORAGE_ERROR,
        message: format!("Failed to get credentials: {}", e),
        data: None,
    })?;

    let user_id = user_id.unwrap_or_else(|| "Me".to_string());

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
                    Ok(item) => Some(serde_json::json!({
                        "id": item.id,
                        "recursiveItemCount": item.recursive_item_count.unwrap_or(0),
                        "cumulativeRunTimeTicks": item.cumulative_run_time_ticks.unwrap_or(0),
                    })),
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

async fn handle_sync_calculate_delta(
    state: &AppState,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let params = params.ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing params".to_string(),
        data: None,
    })?;

    let item_ids = params["itemIds"].as_array().ok_or(JsonRpcError {
        code: ERR_INVALID_PARAMS,
        message: "Missing or invalid itemIds array".to_string(),
        data: None,
    })?;

    let item_ids: Vec<String> = item_ids
        .iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect();

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
        crate::sync::DesiredItem {
            jellyfin_id: item.id,
            name: item.name,
            album: item.album,
            artist: item.album_artist,
            size_bytes,
            etag: item.etag,
        }
    };

    // Fetch item details from Jellyfin in chunks to avoid URL length limits.
    // Container items (playlist/album/artist) are expanded to individual tracks.
    let mut playlist_sync_items: Vec<crate::sync::PlaylistSyncItem> = Vec::new();
    let mut results = Vec::new();
    for chunk in item_ids.chunks(100) {
        let chunk_strs: Vec<&str> = chunk.iter().map(|s| s.as_str()).collect();
        match state
            .jellyfin_client
            .get_items_by_ids(&url, &token, &user_id, &chunk_strs)
            .await
        {
            Ok(items) => {
                let mut fetched_ids: HashSet<String> = HashSet::new();
                for item in items {
                    fetched_ids.insert(item.id.clone());

                    if is_downloadable_item_type(&item.item_type) {
                        results.push(Ok(to_desired_item(item)));
                        continue;
                    }

                    let is_playlist = item.item_type == "Playlist";
                    let item_id = item.id.clone();
                    let item_name = item.name.clone();

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
                })
            }
        }
    }

    // Auto-fill expansion (Story 3.8): if the basket contained an auto-fill slot,
    // run the priority algorithm now and merge results with manual items.
    if let Some(af) = params.get("autoFill") {
        if af["enabled"].as_bool().unwrap_or(false) {
            let max_fill_bytes = if let Some(mb) = af["maxBytes"].as_u64() {
                mb
            } else {
                match state.device_manager.get_device_storage().await {
                    Some(info) => info.free_bytes,
                    None => {
                        return Err(JsonRpcError {
                            code: ERR_CONNECTION_FAILED,
                            message: "Auto-fill: could not determine device free space".to_string(),
                            data: None,
                        })
                    }
                }
            };
            let exclude_ids: Vec<String> = af["excludeItemIds"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let expanded_excludes = expand_exclude_ids(&state.jellyfin_client, exclude_ids).await;
            let fill_params = crate::auto_fill::AutoFillParams {
                exclude_item_ids: expanded_excludes,
                max_fill_bytes,
            };
            match crate::auto_fill::run_auto_fill(&state.jellyfin_client, fill_params).await {
                Ok(af_items) => {
                    for item in af_items {
                        if seen_ids.insert(item.id.clone()) {
                            desired_items.push(crate::sync::DesiredItem {
                                jellyfin_id: item.id,
                                name: item.name,
                                album: item.album,
                                artist: item.artist,
                                size_bytes: item.size_bytes,
                                etag: None,
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
    }

    let mut delta = crate::sync::calculate_delta(&desired_items, &manifest);
    delta.playlists = playlist_sync_items;

    Ok(serde_json::to_value(delta).unwrap())
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
    let delta: crate::sync::SyncDelta =
        serde_json::from_value(params["delta"].clone()).map_err(|e| JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: format!("Invalid delta parameter: {}", e),
            data: None,
        })?;

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

    // Get credentials
    let (url, token, user_id) = CredentialManager::get_credentials().map_err(|e| JsonRpcError {
        code: ERR_STORAGE_ERROR,
        message: format!("Failed to get credentials: {}", e),
        data: None,
    })?;
    let user_id = user_id.unwrap_or_else(|| "Me".to_string());

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
        let transcoding_profile = if let Some(ref profile_id) = sync_manifest.transcoding_profile_id
        {
            match crate::paths::get_device_profiles_path()
                .and_then(|p| crate::transcoding::find_device_profile(&p, profile_id))
            {
                Ok(profile) => profile,
                Err(e) => {
                    eprintln!(
                        "[Sync] Failed to load transcoding profile '{}': {}",
                        profile_id, e
                    );
                    None
                }
            }
        } else {
            None
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
                // Clear dirty flag after sync completes — per-file updates already wrote all items (Story 4.4)
                if let Err(e) = device_manager
                    .update_manifest(|m| {
                        m.dirty = false;
                        m.pending_item_ids = vec![];
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
                    let _ = tokio::task::spawn_blocking(send_sync_complete_notification);
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
    let (url, token, _) = match CredentialManager::get_credentials() {
        Ok(creds) => creds,
        Err(_) => return http::StatusCode::UNAUTHORIZED.into_response(),
    };

    match state.jellyfin_client.get_image(&url, &token, &id).await {
        Ok(resp) => {
            let status = http::StatusCode::from_u16(resp.status().as_u16())
                .unwrap_or(http::StatusCode::INTERNAL_SERVER_ERROR);
            let mut builder = axum::response::Response::builder().status(status);

            if let Some(ct) = resp.headers().get(reqwest::header::CONTENT_TYPE) {
                builder = builder.header(http::header::CONTENT_TYPE, ct);
            }

            // Buffer the body
            match resp.bytes().await {
                Ok(bytes) => builder
                    .body(axum::body::Body::from(bytes))
                    .unwrap_or(http::StatusCode::INTERNAL_SERVER_ERROR.into_response()),
                Err(_) => http::StatusCode::INTERNAL_SERVER_ERROR.into_response(),
            }
        }
        Err(_) => http::StatusCode::NOT_FOUND.into_response(),
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
    if device_name.trim().is_empty() {
        return Err(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Name cannot be empty".to_string(),
            data: None,
        });
    }
    if device_name.chars().count() > 40 {
        return Err(JsonRpcError {
            code: ERR_INVALID_PARAMS,
            message: "Device name exceeds 40 characters".to_string(),
            data: None,
        });
    }
    const VALID_ICONS: &[&str] = &[
        "usb-drive",
        "phone-fill",
        "watch",
        "sd-card",
        "headphones",
        "music-note-list",
    ];
    let device_icon = params["icon"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    if let Some(ref ic) = device_icon {
        if !VALID_ICONS.contains(&ic.as_str()) {
            return Err(JsonRpcError {
                code: ERR_INVALID_PARAMS,
                message: format!("Invalid icon '{}'", ic),
                data: None,
            });
        }
    }

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
    if let Some(ref d) = current_device {
        if d.device_id == device_id {
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

    // Persist both auto_fill prefs and auto_sync_on_connect in a single atomic
    // write-temp-rename operation to prevent inconsistent manifest state on crash.
    state
        .device_manager
        .update_manifest(|m| {
            m.auto_fill.enabled = auto_fill_enabled;
            m.auto_fill.max_bytes = max_fill_bytes;
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

    // profile_id can be null to clear transcoding
    let profile_id = params["profileId"].as_str();

    // Validate profile_id exists in profiles file (unless null/passthrough)
    if let Some(id) = profile_id {
        if id != "passthrough" {
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
            if !profiles.iter().any(|p| p.id == id) {
                return Err(JsonRpcError {
                    code: ERR_INVALID_PARAMS,
                    message: format!("Profile '{}' not found in device-profiles.json", id),
                    data: None,
                });
            }
        }
    }

    // Persist to SQLite DB
    state
        .db
        .set_transcoding_profile(device_id, profile_id)
        .map_err(|e| JsonRpcError {
            code: ERR_STORAGE_ERROR,
            message: e.to_string(),
            data: None,
        })?;

    // Update in-memory device manifest
    state
        .device_manager
        .update_manifest(|m| {
            m.transcoding_profile_id = profile_id.map(|s| s.to_string());
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
    use crate::api::CREDENTIAL_TEST_MUTEX;
    use serde_json::json;

    #[tokio::test]
    async fn test_rpc_test_connection_params() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let device_manager = Arc::new(crate::device::DeviceManager::new(db.clone()));
        let state = Arc::new(AppState {
            jellyfin_client: JellyfinClient::new(),
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
    #[tokio::test]
    async fn test_rpc_get_items_params() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let device_manager = Arc::new(crate::device::DeviceManager::new(db.clone()));
        let state = Arc::new(AppState {
            jellyfin_client: JellyfinClient::new(),
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
    async fn test_rpc_sync_get_device_status_map_no_device() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let device_manager = Arc::new(crate::device::DeviceManager::new(db.clone()));
        let state = Arc::new(AppState {
            jellyfin_client: JellyfinClient::new(),
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
                },
            ],
            dirty: false,
            pending_item_ids: vec![],
            basket_items: vec![],
            auto_sync_on_connect: false,
            auto_fill: crate::device::AutoFillPrefs::default(),
            transcoding_profile_id: None,
            playlists: vec![],
            storage_id: None,
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
            auto_fill: crate::device::AutoFillPrefs::default(),
            transcoding_profile_id: None,
            playlists: vec![],
            storage_id: None,
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
            auto_fill: crate::device::AutoFillPrefs::default(),
            transcoding_profile_id: None,
            playlists: vec![],
            storage_id: None,
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
            auto_fill: crate::device::AutoFillPrefs::default(),
            transcoding_profile_id: None,
            playlists: vec![],
            storage_id: None,
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
        let _credentials_guard = CREDENTIAL_TEST_MUTEX.lock().unwrap();

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
            .with_body(r#"{"Items":[{"Id":"track-1","Name":"Track 1","Type":"Audio","Album":"Album A","AlbumArtist":"Artist A","MediaSources":[{"Size":12345}],"Etag":"track-etag"}],"TotalRecordCount":1,"StartIndex":0}"#)
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
                    auto_fill: crate::device::AutoFillPrefs::default(),
                    transcoding_profile_id: None,
                    playlists: vec![],
                    storage_id: None,
                },
                std::sync::Arc::new(crate::device_io::MscBackend::new(std::path::PathBuf::from(
                    "/tmp/dev",
                ))),
            )
            .await
            .unwrap();

        let state = Arc::new(AppState {
            jellyfin_client: JellyfinClient::new(),
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
    }

    #[tokio::test]
    async fn test_rpc_sync_calculate_delta_partial_failure() {
        use mockito::Server;
        let _credentials_guard = CREDENTIAL_TEST_MUTEX.lock().unwrap();
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
            auto_fill: crate::device::AutoFillPrefs::default(),
            transcoding_profile_id: None,
            playlists: vec![],
            storage_id: None,
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
        assert!(device_manager
            .get_unrecognized_device_path()
            .await
            .is_none());

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
            auto_fill: crate::device::AutoFillPrefs::default(),
            transcoding_profile_id: None,
            playlists: vec![],
            storage_id: None,
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
        db.set_auto_sync_on_connect(device_id, true).unwrap();

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
            auto_fill: crate::device::AutoFillPrefs::default(),
            transcoding_profile_id: None,
            playlists: vec![],
            storage_id: None,
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

        // Running operation → activeOperationId should be the operation UUID
        let manager = Arc::new(crate::sync::SyncOperationManager::new());
        let op_id = "test-uuid-1234".to_string();
        manager.create_operation(op_id.clone(), 5).await;

        let state2 = AppState {
            jellyfin_client: JellyfinClient::new(),
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
            auto_fill: crate::device::AutoFillPrefs::default(),
            transcoding_profile_id: None,
            playlists: vec![],
            storage_id: None,
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
            auto_fill: crate::device::AutoFillPrefs::default(),
            transcoding_profile_id: None,
            playlists: vec![],
            storage_id: None,
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
            auto_fill: crate::device::AutoFillPrefs::default(),
            transcoding_profile_id: None,
            playlists: vec![],
            storage_id: None,
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
}
