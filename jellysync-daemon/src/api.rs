use anyhow::{anyhow, Result};
use reqwest::header::{HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

pub struct JellyfinClient {
    client: reqwest::Client,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    pub server_name: String,
    pub version: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "PascalCase")]
pub struct JellyfinView {
    pub id: String,
    pub name: String,
    #[serde(rename = "Type")]
    pub view_type: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "PascalCase")]
pub struct JellyfinItem {
    pub id: String,
    pub name: String,
    #[serde(rename = "Type")]
    pub item_type: String,
    #[serde(default)]
    pub album_artist: Option<String>,
    #[serde(default)]
    pub production_year: Option<u32>,
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

        if !response.status().is_success() {
            return Err(anyhow!("Server returned status: {}", response.status()));
        }

        let info = response.json::<SystemInfo>().await?;
        Ok(info)
    }

    pub async fn get_views(&self, url: &str, token: &str) -> Result<Vec<JellyfinView>> {
        CredentialManager::validate_url(url)?;
        CredentialManager::validate_token(token)?;

        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Emby-Token",
            HeaderValue::from_str(token).map_err(|_| anyhow!("Invalid token format"))?,
        );

        let endpoint = format!("{}/Users/Me/Views", url.trim_end_matches('/'));

        let response = self.client.get(&endpoint).headers(headers).send().await?;

        if !response.status().is_success() {
            return Err(anyhow!("Server returned status: {}", response.status()));
        }

        let views_response = response.json::<JellyfinViewsResponse>().await?;
        Ok(views_response.items)
    }

    pub async fn get_items(
        &self,
        url: &str,
        token: &str,
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
            "{}/Users/Me/Items{}",
            url.trim_end_matches('/'),
            query_string
        );

        let response = self.client.get(&endpoint).headers(headers).send().await?;

        if !response.status().is_success() {
            return Err(anyhow!("Server returned status: {}", response.status()));
        }

        let items_response = response.json::<JellyfinItemsResponse>().await?;
        Ok(items_response)
    }

    pub async fn get_item_details(
        &self,
        url: &str,
        token: &str,
        item_id: &str,
    ) -> Result<JellyfinItem> {
        CredentialManager::validate_url(url)?;
        CredentialManager::validate_token(token)?;

        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Emby-Token",
            HeaderValue::from_str(token).map_err(|_| anyhow!("Invalid token format"))?,
        );

        let endpoint = format!("{}/Users/Me/Items/{}", url.trim_end_matches('/'), item_id);

        let response = self.client.get(&endpoint).headers(headers).send().await?;

        if !response.status().is_success() {
            return Err(anyhow!("Server returned status: {}", response.status()));
        }

        let item = response.json::<JellyfinItem>().await?;
        Ok(item)
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
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Config {
    pub url: String,
}

pub struct CredentialManager;

static CONFIG_FILE_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);
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

    pub fn save_credentials(url: &str, token: &str) -> Result<()> {
        Self::validate_url(url)?;
        Self::validate_token(token)?;

        let config = Config {
            url: url.to_string(),
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

    pub fn get_credentials() -> Result<(String, String)> {
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

        Ok((config.url, token))
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
            .with_body(r#"{"serverName": "TestServer", "version": "10.8.10"}"#)
            .create_async()
            .await;

        let client = JellyfinClient::new();
        let info = client
            .test_connection(&url, token)
            .await
            .expect("Failed to connect");

        assert_eq!(info.server_name, "TestServer");
        assert_eq!(info.version, "10.8.10");
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
            .mock("GET", "/Users/Me/Views")
            .match_header("X-Emby-Token", token)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Items": [{"Id": "lib1", "Name": "Music", "Type": "CollectionFolder"}], "TotalRecordCount": 1}"#)
            .create_async()
            .await;

        let client = JellyfinClient::new();
        let views = client
            .get_views(&url, token)
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
            .mock("GET", "/Users/Me/Items?ParentId=lib1&IncludeItemTypes=MusicAlbum&Limit=50")
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
                Some("lib1"),
                Some("MusicAlbum"),
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
            .mock("GET", "/Users/Me/Items/album1")
            .match_header("X-Emby-Token", token)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Id": "album1", "Name": "Test Album", "Type": "MusicAlbum", "AlbumArtist": "Test Artist"}"#)
            .create_async()
            .await;

        let client = JellyfinClient::new();
        let item = client
            .get_item_details(&url, token, "album1")
            .await
            .expect("Failed to get item details");

        assert_eq!(item.id, "album1");
        assert_eq!(item.name, "Test Album");
        assert_eq!(item.item_type, "MusicAlbum");
        assert_eq!(item.album_artist, Some("Test Artist".to_string()));
    }
}
