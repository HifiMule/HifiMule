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
}
