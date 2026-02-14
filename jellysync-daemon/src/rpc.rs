use crate::api::{CredentialManager, JellyfinClient};
use axum::{
    extract::{Path, State},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
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
        "sync_get_device_status_map" => handle_sync_get_device_status_map(&state).await,
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

    Ok(serde_json::json!({
        "currentDevice": device,
        "deviceMapping": mapping,
        "serverConnected": server_connected,
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

async fn handle_sync_get_device_status_map(state: &AppState) -> Result<Value, JsonRpcError> {
    let device = state.device_manager.get_current_device().await;

    if device.is_some() {
        // TODO: In future stories, the manifest will include a list of synced items
        // For now, return empty list as placeholder
        Ok(serde_json::json!({
            "syncedItemIds": []
        }))
    } else {
        // No device connected, return empty list
        Ok(serde_json::json!({
            "syncedItemIds": []
        }))
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
    async fn test_jellyfin_item_serialization_metadata() {
        // Verify that our JellyfinItem struct correctly handles the metadata we care about
        let json = json!({
            "Id": "item1",
            "Name": "Item 1",
            "Type": "MusicAlbum",
            "RecursiveItemCount": 10,
            "CumulativeRunTimeTicks": 1000000
        });
        let item: crate::api::JellyfinItem = serde_json::from_value(json).unwrap();
        assert_eq!(item.recursive_item_count, Some(10));
        assert_eq!(item.cumulative_run_time_ticks, Some(1000000));
    }
}
