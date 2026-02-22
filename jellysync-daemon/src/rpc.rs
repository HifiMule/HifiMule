use crate::api::{CredentialManager, JellyfinClient};
use axum::{
    extract::{Path, State},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
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
}

pub async fn run_server(
    port: u16,
    db: Arc<crate::db::Database>,
    device_manager: Arc<crate::device::DeviceManager>,
) {
    let state = Arc::new(AppState {
        jellyfin_client: JellyfinClient::new(),
        db,
        device_manager,
        last_connection_check: Arc::new(tokio::sync::Mutex::new(None)),
        size_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        sync_operation_manager: Arc::new(crate::sync::SyncOperationManager::new()),
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
                ])
                .allow_methods([http::Method::POST])
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
        Err(e) => Err(JsonRpcError {
            code: ERR_STORAGE_ERROR,
            message: e.to_string(),
            data: None,
        }),
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

    Ok(serde_json::json!({
        "currentDevice": device,
        "deviceMapping": mapping,
        "serverConnected": server_connected,
        "dirtyManifest": dirty,
    }))
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

    // Fetch item details from Jellyfin in chunks to avoid URL length limits
    let mut results = Vec::new();
    for chunk in item_ids.chunks(100) {
        let chunk_strs: Vec<&str> = chunk.iter().map(|s| s.as_str()).collect();
        match state
            .jellyfin_client
            .get_items_by_ids(&url, &token, &user_id, &chunk_strs)
            .await
        {
            Ok(items) => {
                for item in items {
                    let size_bytes = item
                        .media_sources
                        .as_ref()
                        .and_then(|sources| sources.first())
                        .and_then(|s| s.size)
                        .unwrap_or(0) as u64;
                    results.push(Ok(crate::sync::DesiredItem {
                        jellyfin_id: item.id,
                        name: item.name,
                        album: item.album,
                        artist: item.album_artist,
                        size_bytes,
                        etag: item.etag.clone(),
                    }));
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
    for res in results {
        match res {
            Ok(item) => desired_items.push(item),
            Err(e) => {
                return Err(JsonRpcError {
                    code: ERR_CONNECTION_FAILED,
                    message: format!("Sync aborted: {}", e),
                    data: None,
                })
            }
        }
    }

    let delta = crate::sync::calculate_delta(&desired_items, &manifest);

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

    tokio::spawn(async move {
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
                    operation.errors = errors;
                    op_manager.update_operation(&op_id, operation).await;
                }
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
        (Some(manifest), Some(path)) => {
            let is_dirty = manifest.dirty;
            let pending_ids = manifest.pending_item_ids.clone();

            let cleaned_tmp_files = if is_dirty {
                crate::device::cleanup_tmp_files(&path, &manifest.managed_paths)
                    .await
                    .unwrap_or(0)
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

#[cfg(test)]
mod tests {
    use super::*;
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
        };

        device_manager
            .handle_device_detected(std::path::PathBuf::from("/tmp/test"), manifest)
            .await
            .unwrap();

        let state = AppState {
            jellyfin_client: JellyfinClient::new(),
            db: db.clone(),
            device_manager,
            last_connection_check: Arc::new(tokio::sync::Mutex::new(None)),
            size_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            sync_operation_manager: Arc::new(crate::sync::SyncOperationManager::new()),
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
            version: "1.0".to_string(),
            managed_paths: vec![],
            synced_items: vec![],
            dirty: false,
            pending_item_ids: vec![],
        };
        device_manager
            .handle_device_detected(dir.path().to_path_buf(), manifest)
            .await
            .unwrap();

        let state = AppState {
            jellyfin_client: JellyfinClient::new(),
            db,
            device_manager,
            last_connection_check: Arc::new(tokio::sync::Mutex::new(None)),
            size_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            sync_operation_manager: Arc::new(crate::sync::SyncOperationManager::new()),
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
            version: "1.0".to_string(),
            managed_paths: vec!["Music".to_string()],
            synced_items: vec![],
            dirty: true,
            pending_item_ids: vec!["id-1".to_string()],
        };
        device_manager
            .handle_device_detected(dir.path().to_path_buf(), manifest)
            .await
            .unwrap();

        let state = AppState {
            jellyfin_client: JellyfinClient::new(),
            db,
            device_manager,
            last_connection_check: Arc::new(tokio::sync::Mutex::new(None)),
            size_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            sync_operation_manager: Arc::new(crate::sync::SyncOperationManager::new()),
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
            version: "1.0".to_string(),
            managed_paths: vec![],
            synced_items: vec![],
            dirty: true,
            pending_item_ids: vec!["id-1".to_string()],
        };
        device_manager
            .handle_device_detected(std::path::PathBuf::from("/tmp/dirty"), dirty_manifest)
            .await
            .unwrap();

        let state2 = AppState {
            jellyfin_client: JellyfinClient::new(),
            db,
            device_manager,
            last_connection_check: Arc::new(tokio::sync::Mutex::new(None)),
            size_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            sync_operation_manager: Arc::new(crate::sync::SyncOperationManager::new()),
        };
        let result2 = handle_get_daemon_state(&state2).await.unwrap();
        assert_eq!(
            result2["dirtyManifest"], true,
            "Dirty device → dirtyManifest must be true"
        );
    }

    #[tokio::test]
    async fn test_rpc_sync_calculate_delta_partial_failure() {
        use mockito::Server;
        let mut server = Server::new_async().await;
        let url = server.url();
        let token = "test-token";

        // Mock system info for connection check (implicit or explicit)
        let _mock_info = server
            .mock("GET", "/System/Info")
            .match_header("X-Emby-Token", token)
            .with_status(200)
            .with_body(r#"{"ServerName": "Test", "Version": "1.0", "Id": "1"}"#)
            .create_async()
            .await;

        // Mock item 1 success
        let _mock_item1 = server
            .mock("GET", "/Users/Me/Items/item-1")
            .match_header("X-Emby-Token", token)
            .with_status(200)
            .with_body(
                r#"{"Id": "item-1", "Name": "Item 1", "Type": "Audio", "AlbumArtist": "Artist"}"#,
            )
            .create_async()
            .await;

        // Mock item 2 failure (404/500)
        let _mock_item2 = server
            .mock("GET", "/Users/Me/Items/item-2")
            .match_header("X-Emby-Token", token)
            .with_status(500)
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
            version: "1.0".to_string(),
            managed_paths: vec![],
            synced_items: vec![],
            dirty: false,
            pending_item_ids: vec![],
        };
        device_manager
            .handle_device_detected(std::path::PathBuf::from("/tmp/dev"), manifest)
            .await
            .unwrap();

        let state = Arc::new(AppState {
            jellyfin_client: JellyfinClient::new(),
            db,
            device_manager,
            last_connection_check: Arc::new(tokio::sync::Mutex::new(None)),
            size_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            sync_operation_manager: Arc::new(crate::sync::SyncOperationManager::new()),
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
}
