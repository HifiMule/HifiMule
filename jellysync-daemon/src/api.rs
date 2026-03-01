use anyhow::{anyhow, Result};
use reqwest::header::{HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

const CONTAINER_TYPES: &[&str] = &["MusicAlbum", "Playlist", "MusicArtist"];

fn url_encode(s: &str) -> String {
    let mut encoded = String::new();
    for c in s.chars() {
        match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => encoded.push(c),
            ' ' => encoded.push('+'),
            c => {
                let mut buf = [0u8; 4];
                let bytes = c.encode_utf8(&mut buf);
                for b in bytes.bytes() {
                    encoded.push_str(&format!("%{:02X}", b));
                }
            }
        }
    }
    encoded
}
#[cfg(test)]
const MUSIC_ITEM_TYPES: &str = "MusicAlbum,Playlist,MusicArtist,Audio,MusicVideo";

#[derive(Clone)]
pub struct JellyfinClient {
    client: reqwest::Client,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SystemInfo {
    pub server_name: String,
    pub version: String,
    pub id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "PascalCase")]
pub struct JellyfinView {
    pub id: String,
    pub name: String,
    #[serde(rename = "Type")]
    pub view_type: String,
    #[serde(default)]
    pub collection_type: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "PascalCase")]
pub struct MediaSource {
    #[serde(default)]
    pub size: Option<i64>,
    pub container: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "PascalCase")]
pub struct JellyfinItem {
    pub id: String,
    pub name: String,
    #[serde(rename = "Type")]
    pub item_type: String,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub album_artist: Option<String>,
    #[serde(default)]
    pub artists: Option<Vec<String>>,
    #[serde(default)]
    pub index_number: Option<u32>,
    #[serde(default)]
    pub container: Option<String>,
    #[serde(default)]
    pub production_year: Option<u32>,
    #[serde(default)]
    pub recursive_item_count: Option<u32>,
    #[serde(default)]
    pub cumulative_run_time_ticks: Option<u64>,
    #[serde(default)]
    pub media_sources: Option<Vec<MediaSource>>,
    #[serde(default)]
    pub etag: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct JellyfinItemsResponse {
    pub items: Vec<JellyfinItem>,
    pub total_record_count: u32,
    pub start_index: u32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct JellyfinViewsResponse {
    pub items: Vec<JellyfinView>,
    pub total_record_count: u32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct AuthenticateByNameRequest {
    pub username: String,
    pub pw: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct UserDto {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct AuthenticationResult {
    pub access_token: String,
    pub user: UserDto,
}

impl JellyfinClient {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    pub async fn test_connection(&self, url: &str, token: &str) -> Result<SystemInfo> {
        // Validate inputs
        CredentialManager::validate_url(url)?;
        CredentialManager::validate_token(token)?;

        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Emby-Token",
            HeaderValue::from_str(token).map_err(|_| anyhow!("Invalid token format"))?,
        );

        let endpoint = format!("{}/System/Info", url.trim_end_matches('/'));

        let response = self.client.get(&endpoint).headers(headers).send().await?;
        let status = response.status();
        let text = response.text().await?;
        println!("DEBUG: Jellyfin Response [{}] - Body: {}", status, text);

        if !status.is_success() {
            return Err(anyhow!("Server returned status: {}", status));
        }

        let info = serde_json::from_str::<SystemInfo>(&text)?;
        Ok(info)
    }

    pub async fn get_views(
        &self,
        url: &str,
        token: &str,
        user_id: &str,
    ) -> Result<Vec<JellyfinView>> {
        CredentialManager::validate_url(url)?;
        CredentialManager::validate_token(token)?;

        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Emby-Token",
            HeaderValue::from_str(token).map_err(|_| anyhow!("Invalid token format"))?,
        );

        let endpoint = format!("{}/Users/{}/Views", url.trim_end_matches('/'), user_id);

        let response = self.client.get(&endpoint).headers(headers).send().await?;
        let status = response.status();
        let text = response.text().await?;
        println!("DEBUG: Jellyfin Response [{}] - Body: {}", status, text);

        if !status.is_success() {
            return Err(anyhow!("Server returned status: {}", status));
        }

        let views_response = serde_json::from_str::<JellyfinViewsResponse>(&text)?;
        Ok(views_response.items)
    }

    pub async fn get_items(
        &self,
        url: &str,
        token: &str,
        user_id: &str,
        parent_id: Option<&str>,
        include_item_types: Option<&str>,
        start_index: Option<u32>,
        limit: Option<u32>,
    ) -> Result<JellyfinItemsResponse> {
        CredentialManager::validate_url(url)?;
        CredentialManager::validate_token(token)?;

        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Emby-Token",
            HeaderValue::from_str(token).map_err(|_| anyhow!("Invalid token format"))?,
        );

        let mut query_params = vec![];
        if let Some(parent) = parent_id {
            query_params.push(format!("ParentId={}", parent));
        }
        if let Some(types) = include_item_types {
            query_params.push(format!("IncludeItemTypes={}", types));
        }
        if let Some(start) = start_index {
            query_params.push(format!("StartIndex={}", start));
        }
        if let Some(lim) = limit {
            query_params.push(format!("Limit={}", lim));
        }

        let query_string = if query_params.is_empty() {
            String::new()
        } else {
            format!("?{}", query_params.join("&"))
        };

        let endpoint = format!(
            "{}/Users/{}/Items{}",
            url.trim_end_matches('/'),
            user_id,
            query_string
        );

        let response = self.client.get(&endpoint).headers(headers).send().await?;
        let status = response.status();
        let text = response.text().await?;
        println!("DEBUG: Jellyfin Response [{}] - Body: {}", status, text);

        if !status.is_success() {
            return Err(anyhow!("Server returned status: {}", status));
        }

        let items_response = serde_json::from_str::<JellyfinItemsResponse>(&text)?;
        Ok(items_response)
    }

    pub async fn get_item_details(
        &self,
        url: &str,
        token: &str,
        user_id: &str,
        item_id: &str,
    ) -> Result<JellyfinItem> {
        CredentialManager::validate_url(url)?;
        CredentialManager::validate_token(token)?;

        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Emby-Token",
            HeaderValue::from_str(token).map_err(|_| anyhow!("Invalid token format"))?,
        );

        let endpoint = format!(
            "{}/Users/{}/Items/{}",
            url.trim_end_matches('/'),
            user_id,
            item_id
        );

        let response = self.client.get(&endpoint).headers(headers).send().await?;
        let status = response.status();
        let text = response.text().await?;
        println!("DEBUG: Jellyfin Response [{}] - Body: {}", status, text);

        if !status.is_success() {
            return Err(anyhow!("Server returned status: {}", status));
        }

        let item = serde_json::from_str::<JellyfinItem>(&text)?;
        Ok(item)
    }

    pub async fn get_items_by_ids(
        &self,
        url: &str,
        token: &str,
        user_id: &str,
        item_ids: &[&str],
    ) -> Result<Vec<JellyfinItem>> {
        if item_ids.is_empty() {
            return Ok(Vec::new());
        }

        CredentialManager::validate_url(url)?;
        CredentialManager::validate_token(token)?;

        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Emby-Token",
            HeaderValue::from_str(token).map_err(|_| anyhow!("Invalid token format"))?,
        );

        let ids_str = item_ids.join(",");
        let endpoint = format!(
            "{}/Users/{}/Items?Ids={}&Fields=MediaSources",
            url.trim_end_matches('/'),
            user_id,
            ids_str
        );

        let response = self.client.get(&endpoint).headers(headers).send().await?;
        let status = response.status();
        let text = response.text().await?;

        if !status.is_success() {
            return Err(anyhow!("Server returned status: {}", status));
        }

        let items_response = serde_json::from_str::<JellyfinItemsResponse>(&text)?;
        Ok(items_response.items)
    }

    pub async fn get_item_with_media_sources(
        &self,
        url: &str,
        token: &str,
        user_id: &str,
        item_id: &str,
    ) -> Result<JellyfinItem> {
        CredentialManager::validate_url(url)?;
        CredentialManager::validate_token(token)?;

        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Emby-Token",
            HeaderValue::from_str(token).map_err(|_| anyhow!("Invalid token format"))?,
        );

        let endpoint = format!(
            "{}/Users/{}/Items/{}?Fields=MediaSources",
            url.trim_end_matches('/'),
            user_id,
            item_id
        );

        let response = self.client.get(&endpoint).headers(headers).send().await?;
        let status = response.status();
        let text = response.text().await?;
        println!("DEBUG: Jellyfin Response [{}] - Body: {}", status, text);

        if !status.is_success() {
            return Err(anyhow!("Server returned status: {}", status));
        }

        let item = serde_json::from_str::<JellyfinItem>(&text)?;
        Ok(item)
    }

    pub async fn get_child_items_with_sizes(
        &self,
        url: &str,
        token: &str,
        user_id: &str,
        parent_id: &str,
    ) -> Result<Vec<JellyfinItem>> {
        CredentialManager::validate_url(url)?;
        CredentialManager::validate_token(token)?;

        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Emby-Token",
            HeaderValue::from_str(token).map_err(|_| anyhow!("Invalid token format"))?,
        );

        let endpoint = format!(
            "{}/Users/{}/Items?ParentId={}&IncludeItemTypes=Audio,MusicVideo&Fields=MediaSources&Recursive=true",
            url.trim_end_matches('/'),
            user_id,
            parent_id
        );

        let response = self.client.get(&endpoint).headers(headers).send().await?;
        let status = response.status();
        let text = response.text().await?;
        println!("DEBUG: Jellyfin Response [{}] - Body: {}", status, text);

        if !status.is_success() {
            return Err(anyhow!("Server returned status: {}", status));
        }

        let items_response = serde_json::from_str::<JellyfinItemsResponse>(&text)?;
        Ok(items_response.items)
    }

    /// Get total size in bytes for each item. For containers (Albums, Playlists, Artists),
    /// recursively fetches child items and sums their MediaSources sizes.
    pub async fn get_item_sizes(
        &self,
        url: &str,
        token: &str,
        user_id: &str,
        ids: Vec<String>,
    ) -> Vec<(String, u64)> {
        let futures = ids.into_iter().map(|id| {
            let url = url.to_string();
            let token = token.to_string();
            let user_id = user_id.to_string();
            async move {
                let size = self.get_single_item_size(&url, &token, &user_id, &id).await;
                match size {
                    Ok(bytes) => Some((id, bytes)),
                    Err(e) => {
                        println!("Warning: Failed to fetch size for item {}: {}", id, e);
                        None
                    }
                }
            }
        });

        futures::future::join_all(futures)
            .await
            .into_iter()
            .flatten()
            .collect()
    }

    async fn get_single_item_size(
        &self,
        url: &str,
        token: &str,
        user_id: &str,
        item_id: &str,
    ) -> Result<u64> {
        let item = self
            .get_item_with_media_sources(url, token, user_id, item_id)
            .await?;

        if CONTAINER_TYPES.contains(&item.item_type.as_str()) {
            // Container item: fetch children and sum their sizes
            let children = self
                .get_child_items_with_sizes(url, token, user_id, item_id)
                .await?;
            let total: u64 = children
                .iter()
                .filter_map(|child| {
                    child
                        .media_sources
                        .as_ref()
                        .and_then(|sources| sources.first())
                        .and_then(|source| source.size)
                        .map(|s| s as u64)
                })
                .sum();
            Ok(total)
        } else {
            // Individual item: read MediaSources[0].Size
            let size = item
                .media_sources
                .as_ref()
                .and_then(|sources| sources.first())
                .and_then(|source| source.size)
                .unwrap_or(0) as u64;
            Ok(size)
        }
    }

    pub async fn get_image(
        &self,
        url: &str,
        token: &str,
        item_id: &str,
    ) -> Result<reqwest::Response> {
        CredentialManager::validate_url(url)?;
        CredentialManager::validate_token(token)?;

        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Emby-Token",
            HeaderValue::from_str(token).map_err(|_| anyhow!("Invalid token format"))?,
        );

        // Fetch primary image
        let endpoint = format!(
            "{}/Items/{}/Images/Primary",
            url.trim_end_matches('/'),
            item_id
        );

        let response = self.client.get(&endpoint).headers(headers).send().await?;

        if !response.status().is_success() {
            return Err(anyhow!("Server returned status: {}", response.status()));
        }

        Ok(response)
    }

    pub async fn authenticate_by_name(
        &self,
        url: &str,
        username: &str,
        password: &str,
    ) -> Result<AuthenticationResult> {
        CredentialManager::validate_url(url)?;

        let endpoint = format!("{}/Users/AuthenticateByName", url.trim_end_matches('/'));

        // Authorization Header
        // TODO: Use persistent DeviceId
        let device_id = CredentialManager::get_device_id()
            .unwrap_or_else(|_| "JellyfinSync-Desktop-Fallback".to_string());
        let auth_header = format!(
            "MediaBrowser Client=\"JellyfinSync\", Device=\"Desktop\", DeviceId=\"{}\", Version=\"{}\"",
            device_id,
            env!("CARGO_PKG_VERSION")
        );

        let body = AuthenticateByNameRequest {
            username: username.to_string(),
            pw: password.to_string(),
        };

        let response = self
            .client
            .post(&endpoint)
            .header("Authorization", auth_header)
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        let text = response.text().await?;
        println!("DEBUG: Jellyfin Response [{}] - Body: {}", status, text);

        if !status.is_success() {
            return Err(anyhow!("Authentication failed: {}", status));
        }

        let result = serde_json::from_str::<AuthenticationResult>(&text)?;
        Ok(result)
    }

    pub async fn search_audio_items(
        &self,
        url: &str,
        token: &str,
        user_id: &str,
        title: &str,
    ) -> Result<Vec<JellyfinItem>> {
        CredentialManager::validate_url(url)?;
        CredentialManager::validate_token(token)?;

        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Emby-Token",
            HeaderValue::from_str(token).map_err(|_| anyhow!("Invalid token format"))?,
        );

        let encoded_title = url_encode(title);
        let endpoint = format!(
            "{}/Users/{}/Items?SearchTerm={}&IncludeItemTypes=Audio&Limit=10&Fields=Id,Name,Album,AlbumArtist,Artists",
            url.trim_end_matches('/'),
            user_id,
            encoded_title
        );

        let response = self.client.get(&endpoint).headers(headers).send().await?;
        let status = response.status();
        let text = response.text().await?;

        if !status.is_success() {
            return Err(anyhow!("Server returned status: {}", status));
        }

        let items_response = serde_json::from_str::<JellyfinItemsResponse>(&text)?;
        Ok(items_response.items)
    }

    pub async fn report_item_played(
        &self,
        url: &str,
        token: &str,
        user_id: &str,
        item_id: &str,
    ) -> Result<()> {
        CredentialManager::validate_url(url)?;
        CredentialManager::validate_token(token)?;

        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Emby-Token",
            HeaderValue::from_str(token).map_err(|_| anyhow!("Invalid token format"))?,
        );

        let endpoint = format!(
            "{}/Users/{}/PlayedItems/{}",
            url.trim_end_matches('/'),
            user_id,
            item_id
        );

        let response = self
            .client
            .post(&endpoint)
            .headers(headers)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            return Err(anyhow!("Server returned status: {}", status));
        }

        Ok(())
    }

    /// Downloads an item as a streaming response.
    /// Returns a stream of bytes that can be written to disk incrementally.
    pub async fn download_item_stream(
        &self,
        url: &str,
        token: &str,
        item_id: &str,
    ) -> Result<impl futures::Stream<Item = std::result::Result<bytes::Bytes, reqwest::Error>>>
    {
        CredentialManager::validate_url(url)?;
        CredentialManager::validate_token(token)?;

        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Emby-Token",
            HeaderValue::from_str(token).map_err(|_| anyhow!("Invalid token format"))?,
        );

        let endpoint = format!("{}/Items/{}/Download", url.trim_end_matches('/'), item_id);

        let response = self.client.get(&endpoint).headers(headers).send().await?;

        if !response.status().is_success() {
            return Err(anyhow!("Server returned status: {}", response.status()));
        }

        Ok(response.bytes_stream())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Config {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub device_id: Option<String>,
}

pub struct CredentialManager;

pub(crate) static CONFIG_FILE_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);
#[cfg(test)]
pub(crate) static CREDENTIAL_TEST_MUTEX: Mutex<()> = Mutex::new(());

const KEYRING_SERVICE: &str = "jellyfinsync-daemon";
const KEYRING_USER: &str = "jellyfin-token";

impl CredentialManager {
    #[cfg(test)]
    pub fn set_config_path(path: PathBuf) {
        let mut p = CONFIG_FILE_PATH.lock().unwrap();
        *p = Some(path);
    }

    fn get_config_path() -> Result<PathBuf> {
        let p = CONFIG_FILE_PATH.lock().unwrap();
        if let Some(ref path) = *p {
            return Ok(path.clone());
        }

        Ok(crate::paths::get_app_data_dir()?.join("config.json"))
    }

    pub(crate) fn validate_url(url: &str) -> Result<()> {
        if url.trim().is_empty() {
            return Err(anyhow!("URL cannot be empty"));
        }
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(anyhow!("URL must start with http:// or https://"));
        }
        Ok(())
    }

    pub(crate) fn validate_token(token: &str) -> Result<()> {
        if token.trim().is_empty() {
            return Err(anyhow!("Token cannot be empty"));
        }
        if token.len() < 10 {
            return Err(anyhow!("Token appears to be invalid (too short)"));
        }
        Ok(())
    }

    pub fn save_credentials(url: &str, token: &str, user_id: Option<&str>) -> Result<()> {
        Self::validate_url(url)?;
        Self::validate_token(token)?;

        let config = Config {
            url: url.to_string(),
            user_id: user_id.map(|s| s.to_string()),
            device_id: Self::get_device_id().ok(),
        };
        let json = serde_json::to_string_pretty(&config)?;
        let path = Self::get_config_path()?;
        fs::write(&path, json).map_err(|e| anyhow!("Failed to write config file: {}", e))?;

        let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
            .map_err(|e| anyhow!("Failed to access keyring: {}", e))?;
        entry
            .set_password(token)
            .map_err(|e| anyhow!("Failed to save token to keyring: {}", e))?;

        Ok(())
    }

    pub fn get_credentials() -> Result<(String, String, Option<String>)> {
        let path = Self::get_config_path()?;
        if !path.exists() {
            return Err(anyhow!("No config file found"));
        }
        let content =
            fs::read_to_string(&path).map_err(|e| anyhow!("Failed to read config file: {}", e))?;
        let config: Config = serde_json::from_str(&content)
            .map_err(|e| anyhow!("Failed to parse config file: {}", e))?;

        let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
            .map_err(|e| anyhow!("Failed to access keyring: {}", e))?;
        let token = entry
            .get_password()
            .map_err(|e| anyhow!("No token found in keyring: {}", e))?;

        Ok((config.url, token, config.user_id))
    }

    pub fn get_device_id() -> Result<String> {
        let path = Self::get_config_path()?;

        // Try to read existing config
        let mut config = if path.exists() {
            let content = fs::read_to_string(&path)
                .map_err(|e| anyhow!("Failed to read config file: {}", e))?;
            serde_json::from_str::<Config>(&content).unwrap_or(Config {
                url: "".to_string(),
                user_id: None,
                device_id: None,
            })
        } else {
            Config {
                url: "".to_string(),
                user_id: None,
                device_id: None,
            }
        };

        if let Some(id) = &config.device_id {
            return Ok(id.clone());
        }

        // Generate new ID
        let new_id = format!(
            "JellyfinSync-Desktop-{:x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        );

        config.device_id = Some(new_id.clone());

        // Save back if path exists or we have other data,
        // but if it's a fresh file we might just perform a save if we have a valid path
        // For simplicity, we only save if we can, ignoring errors if directory doesn't exist yet
        // (CredentialManager::save_credentials handles dir creation usually via get_app_data_dir)

        // We only save if we have a valid path. The actual save_credentials checks for dir existence.
        // Here we just try to write if the file/dir structure is plausible.
        // Actually, let's just write checking for errors.

        if let Ok(json) = serde_json::to_string_pretty(&config) {
            // Ensure dir exists
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::write(&path, json);
        }

        Ok(new_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;

    #[tokio::test]
    async fn test_jellyfin_connection_success() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let token = "test-token-1234567890";

        let _mock = server
            .mock("GET", "/System/Info")
            .match_header("X-Emby-Token", token)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"ServerName": "TestServer", "Version": "10.8.10", "Id": "8d5613157d2547e9b35fd762fe1f253e"}"#)
            .create_async()
            .await;

        let client = JellyfinClient::new();
        let info = client
            .test_connection(&url, token)
            .await
            .expect("Failed to connect");

        assert_eq!(info.server_name, "TestServer");
        assert_eq!(info.version, "10.8.10");
        assert_eq!(info.id, "8d5613157d2547e9b35fd762fe1f253e");
    }

    #[tokio::test]
    async fn test_jellyfin_connection_failure() {
        let mut server = Server::new_async().await;
        let url = server.url();

        let _mock = server
            .mock("GET", "/System/Info")
            .with_status(401)
            .create_async()
            .await;

        let client = JellyfinClient::new();
        let res = client.test_connection(&url, "bad-token-1234567890").await;

        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("401"));
    }

    #[tokio::test]
    async fn test_validation_empty_url() {
        let client = JellyfinClient::new();
        let res = client.test_connection("", "valid-token-1234567890").await;

        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("URL cannot be empty"));
    }

    #[tokio::test]
    async fn test_validation_invalid_url_scheme() {
        let client = JellyfinClient::new();
        let res = client
            .test_connection("ftp://invalid.com", "valid-token-1234567890")
            .await;

        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .to_string()
            .contains("must start with http"));
    }

    #[test]
    fn test_credential_validation() {
        assert!(CredentialManager::validate_url("http://localhost:8096").is_ok());
        assert!(CredentialManager::validate_url("https://jellyfin.example.com").is_ok());
        assert!(CredentialManager::validate_token("valid-token-1234567890").is_ok());

        assert!(CredentialManager::validate_url("").is_err());
        assert!(CredentialManager::validate_url("   ").is_err());
        assert!(CredentialManager::validate_url("ftp://invalid.com").is_err());
        assert!(CredentialManager::validate_url("localhost:8096").is_err());

        assert!(CredentialManager::validate_token("").is_err());
        assert!(CredentialManager::validate_token("   ").is_err());
        assert!(CredentialManager::validate_token("short").is_err());
    }

    #[tokio::test]
    async fn test_get_views_success() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let token = "test-token-1234567890";

        let _mock = server
            .mock("GET", "/Users/user1/Views")
            .match_header("X-Emby-Token", token)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Items": [{"Id": "lib1", "Name": "Music", "Type": "CollectionFolder"}], "TotalRecordCount": 1}"#)
            .create_async()
            .await;

        let client = JellyfinClient::new();
        let views = client
            .get_views(&url, token, "user1")
            .await
            .expect("Failed to get views");

        assert_eq!(views.len(), 1);
        assert_eq!(views[0].id, "lib1");
        assert_eq!(views[0].name, "Music");
        assert_eq!(views[0].view_type, "CollectionFolder");
    }

    #[tokio::test]
    async fn test_get_items_success() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let token = "test-token-1234567890";

        let _mock = server
            .mock("GET", "/Users/user1/Items?ParentId=lib1&IncludeItemTypes=MusicAlbum,Playlist,MusicArtist,Audio,MusicVideo&Limit=50")
            .match_header("X-Emby-Token", token)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Items": [{"Id": "album1", "Name": "Test Album", "Type": "MusicAlbum", "AlbumArtist": "Test Artist", "ProductionYear": 2023}], "TotalRecordCount": 1, "StartIndex": 0}"#)
            .create_async()
            .await;

        let client = JellyfinClient::new();
        let response = client
            .get_items(
                &url,
                token,
                "user1",
                Some("lib1"),
                Some(MUSIC_ITEM_TYPES),
                None,
                Some(50),
            )
            .await
            .expect("Failed to get items");

        assert_eq!(response.items.len(), 1);
        assert_eq!(response.items[0].id, "album1");
        assert_eq!(response.items[0].name, "Test Album");
        assert_eq!(response.items[0].item_type, "MusicAlbum");
        assert_eq!(
            response.items[0].album_artist,
            Some("Test Artist".to_string())
        );
        assert_eq!(response.items[0].production_year, Some(2023));
        assert_eq!(response.total_record_count, 1);
    }

    #[tokio::test]
    async fn test_get_item_details_success() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let token = "test-token-1234567890";

        let _mock = server
            .mock("GET", "/Users/user1/Items/album1")
            .match_header("X-Emby-Token", token)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Id": "album1", "Name": "Test Album", "Type": "MusicAlbum", "AlbumArtist": "Test Artist"}"#)
            .create_async()
            .await;

        let client = JellyfinClient::new();
        let item = client
            .get_item_details(&url, token, "user1", "album1")
            .await
            .expect("Failed to get item details");

        assert_eq!(item.name, "Test Album");
        assert_eq!(item.item_type, "MusicAlbum");
        assert_eq!(item.album_artist, Some("Test Artist".to_string()));
    }

    #[tokio::test]
    async fn test_get_item_metadata_serialization() {
        let json = r#"{
            "Id": "item1",
            "Name": "Item 1",
            "Type": "MusicAlbum",
            "RecursiveItemCount": 12,
            "CumulativeRunTimeTicks": 123456789
        }"#;
        let item: JellyfinItem = serde_json::from_str(json).unwrap();
        assert_eq!(item.recursive_item_count, Some(12));
        assert_eq!(item.cumulative_run_time_ticks, Some(123456789));
    }

    #[tokio::test]
    async fn test_authenticate_by_name_success() {
        let mut server = Server::new_async().await;
        let url = server.url();

        let _mock = server
            .mock("POST", "/Users/AuthenticateByName")
            .match_header("Authorization", mockito::Matcher::Regex(r#"MediaBrowser Client="JellyfinSync".*"#.to_string()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"AccessToken": "new-token-123", "User": {"Id": "user1", "Name": "Alice"}, "SessionInfo": {}}"#)
            .create_async()
            .await;

        let client = JellyfinClient::new();
        let result = client
            .authenticate_by_name(&url, "Alice", "secret")
            .await
            .expect("Failed to authenticate");

        assert_eq!(result.access_token, "new-token-123");
        assert_eq!(result.user.id, "user1");
        assert_eq!(result.user.name, "Alice");
    }

    #[tokio::test]
    async fn test_authenticate_by_name_failure() {
        let mut server = Server::new_async().await;
        let url = server.url();

        let _mock = server
            .mock("POST", "/Users/AuthenticateByName")
            .with_status(401)
            .create_async()
            .await;

        let client = JellyfinClient::new();
        let result = client
            .authenticate_by_name(&url, "Alice", "wrong-password")
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("401"));
    }

    #[test]
    fn test_media_source_deserialization() {
        let json = r#"{
            "Id": "track1",
            "Name": "Track 1",
            "Type": "Audio",
            "MediaSources": [{"Size": 5242880}]
        }"#;
        let item: JellyfinItem = serde_json::from_str(json).unwrap();
        assert!(item.media_sources.is_some());
        let sources = item.media_sources.unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].size, Some(5242880));
    }

    #[test]
    fn test_media_source_missing() {
        let json = r#"{
            "Id": "track1",
            "Name": "Track 1",
            "Type": "Audio"
        }"#;
        let item: JellyfinItem = serde_json::from_str(json).unwrap();
        assert!(item.media_sources.is_none());
    }

    #[tokio::test]
    async fn test_get_item_sizes_individual_track() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let token = "test-token-1234567890";

        // Mock: fetch individual Audio item with MediaSources
        let _mock = server
            .mock("GET", "/Users/user1/Items/track1?Fields=MediaSources")
            .match_header("X-Emby-Token", token)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Id": "track1", "Name": "Track 1", "Type": "Audio", "MediaSources": [{"Size": 5242880}]}"#)
            .create_async()
            .await;

        let client = JellyfinClient::new();
        let results = client
            .get_item_sizes(&url, token, "user1", vec!["track1".to_string()])
            .await;

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "track1");
        assert_eq!(results[0].1, 5242880);
    }

    #[tokio::test]
    async fn test_get_item_sizes_album_container() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let token = "test-token-1234567890";

        // Mock: fetch album item (container type)
        let _mock_album = server
            .mock("GET", "/Users/user1/Items/album1?Fields=MediaSources")
            .match_header("X-Emby-Token", token)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Id": "album1", "Name": "Test Album", "Type": "MusicAlbum"}"#)
            .create_async()
            .await;

        // Mock: fetch child items of album
        let _mock_children = server
            .mock("GET", "/Users/user1/Items?ParentId=album1&IncludeItemTypes=Audio,MusicVideo&Fields=MediaSources&Recursive=true")
            .match_header("X-Emby-Token", token)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Items": [
                {"Id": "t1", "Name": "Track 1", "Type": "Audio", "MediaSources": [{"Size": 3000000}]},
                {"Id": "t2", "Name": "Track 2", "Type": "Audio", "MediaSources": [{"Size": 4000000}]}
            ], "TotalRecordCount": 2, "StartIndex": 0}"#)
            .create_async()
            .await;

        let client = JellyfinClient::new();
        let results = client
            .get_item_sizes(&url, token, "user1", vec!["album1".to_string()])
            .await;

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "album1");
        assert_eq!(results[0].1, 7_000_000); // 3MB + 4MB
    }

    #[tokio::test]
    async fn test_get_item_sizes_artist_container() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let token = "test-token-1234567890";

        // Mock: fetch artist item (container type; no MediaSources on container is deliberate)
        let _mock_artist = server
            .mock("GET", "/Users/user1/Items/artist1?Fields=MediaSources")
            .match_header("X-Emby-Token", token)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Id": "artist1", "Name": "Test Artist", "Type": "MusicArtist"}"#)
            .expect(1)
            .create_async()
            .await;

        // Mock: fetch all tracks under artist (Recursive=true flattens Artist → Albums → Tracks)
        let _mock_children = server
            .mock("GET", "/Users/user1/Items?ParentId=artist1&IncludeItemTypes=Audio,MusicVideo&Fields=MediaSources&Recursive=true")
            .match_header("X-Emby-Token", token)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Items": [
                {"Id": "a1", "Name": "Artist Track 1", "Type": "Audio", "MediaSources": [{"Size": 5000000}]},
                {"Id": "a2", "Name": "Artist Track 2", "Type": "Audio", "MediaSources": [{"Size": 6000000}]}
            ], "TotalRecordCount": 2, "StartIndex": 0}"#)
            .expect(1)
            .create_async()
            .await;

        let client = JellyfinClient::new();
        let results = client
            .get_item_sizes(&url, token, "user1", vec!["artist1".to_string()])
            .await;

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "artist1");
        assert_eq!(results[0].1, 11_000_000); // 5MB + 6MB
    }

    #[tokio::test]
    async fn test_get_item_sizes_artist_empty() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let token = "test-token-1234567890";

        let _mock_artist = server
            .mock("GET", "/Users/user1/Items/artist2?Fields=MediaSources")
            .match_header("X-Emby-Token", token)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Id": "artist2", "Name": "Empty Artist", "Type": "MusicArtist"}"#)
            .expect(1)
            .create_async()
            .await;

        let _mock_children = server
            .mock("GET", "/Users/user1/Items?ParentId=artist2&IncludeItemTypes=Audio,MusicVideo&Fields=MediaSources&Recursive=true")
            .match_header("X-Emby-Token", token)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Items": [], "TotalRecordCount": 0, "StartIndex": 0}"#)
            .expect(1)
            .create_async()
            .await;

        let client = JellyfinClient::new();
        let results = client
            .get_item_sizes(&url, token, "user1", vec!["artist2".to_string()])
            .await;

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "artist2");
        assert_eq!(results[0].1, 0); // No tracks means 0 bytes
    }

    #[tokio::test]
    async fn test_get_item_sizes_artist_children_error() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let token = "test-token-1234567890";

        let _mock_artist = server
            .mock("GET", "/Users/user1/Items/artist3?Fields=MediaSources")
            .match_header("X-Emby-Token", token)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Id": "artist3", "Name": "Restricted Artist", "Type": "MusicArtist"}"#)
            .expect(1)
            .create_async()
            .await;

        // Server error on the children endpoint — production code logs and drops the error
        let _mock_children = server
            .mock("GET", "/Users/user1/Items?ParentId=artist3&IncludeItemTypes=Audio,MusicVideo&Fields=MediaSources&Recursive=true")
            .match_header("X-Emby-Token", token)
            .with_status(500)
            .expect(1)
            .create_async()
            .await;

        let client = JellyfinClient::new();
        let results = client
            .get_item_sizes(&url, token, "user1", vec!["artist3".to_string()])
            .await;

        // Error is silently dropped; item is excluded from results
        assert_eq!(results.len(), 0);
    }

    #[tokio::test]
    async fn test_get_item_sizes_no_media_sources() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let token = "test-token-1234567890";

        // Mock: item with no MediaSources
        let _mock = server
            .mock("GET", "/Users/user1/Items/track1?Fields=MediaSources")
            .match_header("X-Emby-Token", token)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Id": "track1", "Name": "Track 1", "Type": "Audio"}"#)
            .create_async()
            .await;

        let client = JellyfinClient::new();
        let results = client
            .get_item_sizes(&url, token, "user1", vec!["track1".to_string()])
            .await;

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "track1");
        assert_eq!(results[0].1, 0); // No MediaSources means 0 bytes
    }

    #[test]
    fn test_file_storage() {
        let _guard = CREDENTIAL_TEST_MUTEX.lock().unwrap();

        let test_url = "http://localhost:8096";
        let test_token = "test-token-1234567890";

        // Use a unique temporary file for testing to prevent collisions
        let temp_dir = tempfile::tempdir().unwrap();
        let temp_config_path = temp_dir.path().join("test_config.json");

        CredentialManager::set_config_path(temp_config_path.clone());

        // Test Save
        CredentialManager::save_credentials(test_url, test_token, Some("test-user-id"))
            .expect("Failed to save");

        assert!(temp_config_path.exists());

        // Test Get
        let (url, token, user_id) =
            CredentialManager::get_credentials().expect("Failed to retrieve");

        assert_eq!(url, test_url);
        assert_eq!(token, test_token);
        assert_eq!(user_id, Some("test-user-id".to_string()));
    }
}
