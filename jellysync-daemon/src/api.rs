use anyhow::{anyhow, Result};
use lazy_static::lazy_static;
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
        FileCredentialManager::validate_url(url)?;
        FileCredentialManager::validate_token(token)?;

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
struct Credentials {
    pub url: String,
    pub token: String,
}

pub struct FileCredentialManager;

lazy_static! {
    static ref CRED_FILE_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);
}

impl FileCredentialManager {
    // Allows overriding the path for testing
    #[cfg(test)]
    pub fn set_credentials_path(path: PathBuf) {
        let mut p = CRED_FILE_PATH.lock().unwrap();
        *p = Some(path);
    }

    fn get_credentials_path() -> PathBuf {
        let p = CRED_FILE_PATH.lock().unwrap();
        if let Some(ref path) = *p {
            return path.clone();
        }

        // Use platform-standard directories
        #[cfg(target_os = "windows")]
        {
            if let Ok(appdata) = std::env::var("APPDATA") {
                let mut path = PathBuf::from(appdata);
                path.push("JellyfinSync");
                if !path.exists() {
                    let _ = std::fs::create_dir_all(&path);
                }
                path.push("credentials.json");
                return path;
            }
        }

        #[cfg(target_os = "macos")]
        {
            if let Ok(home) = std::env::var("HOME") {
                let mut path = PathBuf::from(home);
                path.push("Library");
                path.push("Application Support");
                path.push("JellyfinSync");
                if !path.exists() {
                    let _ = std::fs::create_dir_all(&path);
                }
                path.push("credentials.json");
                return path;
            }
        }

        #[cfg(target_os = "linux")]
        {
            if let Ok(xdg_data) = std::env::var("XDG_DATA_HOME") {
                let mut path = PathBuf::from(xdg_data);
                path.push("jellyfinsync");
                if !path.exists() {
                    let _ = std::fs::create_dir_all(&path);
                }
                path.push("credentials.json");
                return path;
            } else if let Ok(home) = std::env::var("HOME") {
                let mut path = PathBuf::from(home);
                path.push(".local");
                path.push("share");
                path.push("jellyfinsync");
                if !path.exists() {
                    let _ = std::fs::create_dir_all(&path);
                }
                path.push("credentials.json");
                return path;
            }
        }

        // Fallback to current directory
        PathBuf::from("credentials.json")
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

        let creds = Credentials {
            url: url.to_string(),
            token: token.to_string(),
        };

        let json = serde_json::to_string_pretty(&creds)?;
        let path = Self::get_credentials_path();

        fs::write(&path, json).map_err(|e| anyhow!("Failed to write credentials file: {}", e))?;

        Ok(())
    }

    pub fn get_credentials() -> Result<(String, String)> {
        let path = Self::get_credentials_path();

        if !path.exists() {
            return Err(anyhow!("No credentials file found"));
        }

        let content = fs::read_to_string(&path)
            .map_err(|e| anyhow!("Failed to read credentials file: {}", e))?;

        let creds: Credentials = serde_json::from_str(&content)
            .map_err(|e| anyhow!("Failed to parse credentials file: {}", e))?;

        Ok((creds.url, creds.token))
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
        let token = "test-token";

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

    #[tokio::test]
    async fn test_validation_empty_token() {
        let client = JellyfinClient::new();
        let res = client.test_connection("http://localhost:8096", "").await;

        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .to_string()
            .contains("Token cannot be empty"));
    }

    #[tokio::test]
    async fn test_validation_short_token() {
        let client = JellyfinClient::new();
        let res = client
            .test_connection("http://localhost:8096", "short")
            .await;

        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("too short"));
    }

    #[test]
    fn test_credential_validation() {
        // Valid inputs
        assert!(FileCredentialManager::validate_url("http://localhost:8096").is_ok());
        assert!(FileCredentialManager::validate_url("https://jellyfin.example.com").is_ok());
        assert!(FileCredentialManager::validate_token("valid-token-1234567890").is_ok());

        // Invalid URLs
        assert!(FileCredentialManager::validate_url("").is_err());
        assert!(FileCredentialManager::validate_url("   ").is_err());
        assert!(FileCredentialManager::validate_url("ftp://invalid.com").is_err());
        assert!(FileCredentialManager::validate_url("localhost:8096").is_err());

        // Invalid tokens
        assert!(FileCredentialManager::validate_token("").is_err());
        assert!(FileCredentialManager::validate_token("   ").is_err());
        assert!(FileCredentialManager::validate_token("short").is_err());
    }
}
