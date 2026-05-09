use crate::domain::models::{
    Album, AlbumWithTracks, Artist, ArtistWithAlbums, ChangeEvent, Library, Playlist,
    PlaylistWithTracks, SearchResult,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::Arc;
use thiserror::Error;

pub mod jellyfin;
pub mod subsonic;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderChangeContext {
    #[serde(default)]
    pub synced_songs: Vec<ProviderSyncedSong>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSyncedSong {
    pub song_id: String,
    #[serde(default)]
    pub album_id: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub content_type: Option<String>,
    #[serde(default)]
    pub suffix: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderChangeMetadata {
    #[serde(default)]
    pub album_id: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub content_type: Option<String>,
    #[serde(default)]
    pub suffix: Option<String>,
}

#[async_trait]
pub trait MediaProvider: Send + Sync {
    async fn list_libraries(&self) -> Result<Vec<Library>, ProviderError>;

    async fn list_artists(&self, library_id: Option<&str>) -> Result<Vec<Artist>, ProviderError>;

    async fn get_artist(&self, artist_id: &str) -> Result<ArtistWithAlbums, ProviderError>;

    async fn list_albums(&self, library_id: Option<&str>) -> Result<Vec<Album>, ProviderError>;

    async fn get_album(&self, album_id: &str) -> Result<AlbumWithTracks, ProviderError>;

    async fn list_playlists(&self) -> Result<Vec<Playlist>, ProviderError>;

    async fn get_playlist(&self, playlist_id: &str) -> Result<PlaylistWithTracks, ProviderError>;

    async fn search(&self, query: &str) -> Result<SearchResult, ProviderError>;

    async fn download_url(
        &self,
        song_id: &str,
        profile: Option<&TranscodeProfile>,
    ) -> Result<String, ProviderError>;

    async fn cover_art_url(&self, cover_art_id: &str) -> Result<String, ProviderError>;

    async fn changes_since(&self, token: Option<&str>) -> Result<Vec<ChangeEvent>, ProviderError> {
        self.changes_since_with_context(token, &ProviderChangeContext::default())
            .await
    }

    async fn changes_since_with_context(
        &self,
        token: Option<&str>,
        context: &ProviderChangeContext,
    ) -> Result<Vec<ChangeEvent>, ProviderError>;

    async fn scrobble(&self, request: ScrobbleRequest) -> Result<(), ProviderError>;

    fn change_metadata(&self, _event: &ChangeEvent) -> Option<ProviderChangeMetadata> {
        None
    }

    fn server_type(&self) -> ServerType;

    fn server_version(&self) -> Option<&str> {
        None
    }

    fn access_token(&self) -> Option<&str> {
        None
    }

    fn provider_user_id(&self) -> Option<&str> {
        None
    }

    fn capabilities(&self) -> Capabilities;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ServerTypeHint {
    Auto,
    Jellyfin,
    Subsonic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServerType {
    Jellyfin,
    Subsonic,
    OpenSubsonic,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    pub open_subsonic: bool,
    pub supports_changes_since: bool,
    pub supports_server_transcoding: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscodeProfile {
    pub container: Option<String>,
    pub audio_codec: Option<String>,
    pub max_bitrate_kbps: Option<u32>,
}

#[derive(Clone, PartialEq, Eq)]
pub enum CredentialKind {
    Token(String),
    Password { username: String, password: String },
}

impl fmt::Debug for CredentialKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CredentialKind::Token(_) => write!(f, "Token([redacted])"),
            CredentialKind::Password { username, .. } => {
                write!(
                    f,
                    "Password {{ username: {:?}, password: [redacted] }}",
                    username
                )
            }
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ProviderCredentials {
    pub server_url: String,
    pub credential: CredentialKind,
}

impl fmt::Debug for ProviderCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProviderCredentials")
            .field("server_url", &self.server_url)
            .field("credential", &self.credential)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScrobbleRequest {
    pub song_id: String,
    pub submission: ScrobbleSubmission,
    pub position_seconds: Option<u32>,
    pub played_at_unix_seconds: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScrobbleSubmission {
    Playing,
    Played,
}

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("provider HTTP error: status={status:?}, message={message}")]
    Http {
        status: Option<u16>,
        message: String,
    },

    #[error("provider authentication failed: {0}")]
    Auth(String),

    #[error("provider item not found: {item_type} {id}")]
    NotFound { item_type: String, id: String },

    #[error("provider capability is unsupported: {0}")]
    UnsupportedCapability(String),

    #[error("provider response deserialization failed: {0}")]
    Deserialization(String),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub async fn connect(
    url: &str,
    creds: &ProviderCredentials,
    hint: ServerTypeHint,
) -> Result<Arc<dyn MediaProvider>, ProviderError> {
    match hint {
        ServerTypeHint::Auto => {
            if let Ok(provider) = connect_subsonic(url, creds).await {
                return Ok(provider);
            }
            connect_jellyfin(url, creds)
                .await
                .map_err(|_| unknown_type_error())
        }
        ServerTypeHint::Jellyfin => connect_jellyfin(url, creds).await,
        ServerTypeHint::Subsonic => connect_subsonic(url, creds).await,
    }
}

pub fn server_type_slug(server_type: ServerType) -> Option<&'static str> {
    match server_type {
        ServerType::Jellyfin => Some("jellyfin"),
        ServerType::Subsonic => Some("subsonic"),
        ServerType::OpenSubsonic => Some("openSubsonic"),
        ServerType::Unknown => None,
    }
}

fn unknown_type_error() -> ProviderError {
    ProviderError::UnsupportedCapability("Unknown server type at this URL".to_string())
}

async fn connect_subsonic(
    url: &str,
    creds: &ProviderCredentials,
) -> Result<Arc<dyn MediaProvider>, ProviderError> {
    let mut creds = creds.clone();
    creds.server_url = url.to_string();
    let provider = subsonic::SubsonicProvider::connect(creds).await?;
    Ok(Arc::new(provider))
}

async fn connect_jellyfin(
    url: &str,
    creds: &ProviderCredentials,
) -> Result<Arc<dyn MediaProvider>, ProviderError> {
    let crate::providers::CredentialKind::Password { username, password } = &creds.credential
    else {
        return Err(ProviderError::UnsupportedCapability(
            "Jellyfin connection requires username and password".to_string(),
        ));
    };

    let client = crate::api::JellyfinClient::new();
    let auth = client
        .authenticate_by_name(url, username, password)
        .await
        .map_err(|error| ProviderError::Auth(sanitize_secret_message(&error.to_string())))?;
    let info = client
        .test_connection(url, &auth.access_token)
        .await
        .map_err(|error| ProviderError::Http {
            status: None,
            message: sanitize_secret_message(&error.to_string()),
        })?;
    let provider = jellyfin::JellyfinProvider::new_with_version(
        client,
        url,
        auth.access_token,
        auth.user.id,
        Some(info.version),
    );
    Ok(Arc::new(provider))
}

fn sanitize_secret_message(message: &str) -> String {
    let mut sanitized = message.to_string();
    for key in ["password", "pw", "token", "api_key", "u", "p", "t", "s"] {
        let needle = format!("{key}=");
        let mut rebuilt = String::with_capacity(sanitized.len());
        let mut cursor = 0;
        while let Some(relative_start) = sanitized[cursor..].find(&needle) {
            let start = cursor + relative_start;
            let preceded_by_separator = start == 0
                || matches!(
                    sanitized[..start].chars().last(),
                    Some('?' | '&' | ' ' | '\t' | '\n')
                );
            if !preceded_by_separator {
                rebuilt.push_str(&sanitized[cursor..start + needle.len()]);
                cursor = start + needle.len();
                continue;
            }
            rebuilt.push_str(&sanitized[cursor..start + needle.len()]);
            cursor = start + needle.len();
            let value_end = sanitized[cursor..]
                .find(|ch: char| ch == '&' || ch.is_whitespace())
                .map(|offset| cursor + offset)
                .unwrap_or(sanitized.len());
            rebuilt.push_str("[redacted]");
            cursor = value_end;
        }
        rebuilt.push_str(&sanitized[cursor..]);
        sanitized = rebuilt;
    }
    sanitized
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::{Matcher, Server};

    fn password_credentials(url: String) -> ProviderCredentials {
        ProviderCredentials {
            server_url: url,
            credential: CredentialKind::Password {
                username: "alexis".to_string(),
                password: "secret-password".to_string(),
            },
        }
    }

    #[tokio::test]
    async fn factory_auto_detects_open_subsonic_first() {
        let mut server = Server::new_async().await;
        let _ping = server
            .mock("GET", "/rest/ping.view")
            .match_query(Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"subsonic-response":{"status":"ok","version":"1.16.1","openSubsonic":true}}"#,
            )
            .expect(1)
            .create_async()
            .await;
        let _jellyfin = server
            .mock("POST", "/Users/AuthenticateByName")
            .expect(0)
            .create_async()
            .await;

        let provider = connect(
            &server.url(),
            &password_credentials(server.url()),
            ServerTypeHint::Auto,
        )
        .await
        .expect("provider");

        assert_eq!(provider.server_type(), ServerType::OpenSubsonic);
        assert_eq!(provider.server_version(), Some("1.16.1"));
    }

    #[tokio::test]
    async fn factory_auto_detects_classic_subsonic_without_open_flag() {
        let mut server = Server::new_async().await;
        let _ping = server
            .mock("GET", "/rest/ping.view")
            .match_query(Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"subsonic-response":{"status":"ok","version":"1.16.1"}}"#)
            .expect(1)
            .create_async()
            .await;

        let provider = connect(
            &server.url(),
            &password_credentials(server.url()),
            ServerTypeHint::Auto,
        )
        .await
        .expect("provider");

        assert_eq!(provider.server_type(), ServerType::Subsonic);
        assert_eq!(provider.server_version(), Some("1.16.1"));
    }

    #[tokio::test]
    async fn factory_auto_falls_back_to_jellyfin_after_subsonic_failure() {
        let mut server = Server::new_async().await;
        let _ping = server
            .mock("GET", "/rest/ping.view")
            .match_query(Matcher::Any)
            .with_status(404)
            .expect(1)
            .create_async()
            .await;
        let _auth = server
            .mock("POST", "/Users/AuthenticateByName")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"AccessToken":"jellyfin-token-12345","User":{"Id":"user1","Name":"Alexis"}}"#,
            )
            .expect(1)
            .create_async()
            .await;
        let _info = server
            .mock("GET", "/System/Info")
            .match_header("X-Emby-Token", "jellyfin-token-12345")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"ServerName":"Jellyfin","Version":"10.9.0","Id":"server1"}"#)
            .expect(1)
            .create_async()
            .await;

        let provider = connect(
            &server.url(),
            &password_credentials(server.url()),
            ServerTypeHint::Auto,
        )
        .await
        .expect("provider");

        assert_eq!(provider.server_type(), ServerType::Jellyfin);
        assert_eq!(provider.server_version(), Some("10.9.0"));
    }

    #[tokio::test]
    async fn factory_explicit_hints_skip_unrelated_probe_paths() {
        let mut jellyfin = Server::new_async().await;
        let _subsonic_should_not_run = jellyfin
            .mock("GET", "/rest/ping.view")
            .expect(0)
            .create_async()
            .await;
        let _auth = jellyfin
            .mock("POST", "/Users/AuthenticateByName")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"AccessToken":"jellyfin-token-12345","User":{"Id":"user1","Name":"Alexis"}}"#,
            )
            .expect(1)
            .create_async()
            .await;
        let _info = jellyfin
            .mock("GET", "/System/Info")
            .match_header("X-Emby-Token", "jellyfin-token-12345")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"ServerName":"Jellyfin","Version":"10.9.0","Id":"server1"}"#)
            .expect(1)
            .create_async()
            .await;

        let provider = connect(
            &jellyfin.url(),
            &password_credentials(jellyfin.url()),
            ServerTypeHint::Jellyfin,
        )
        .await
        .expect("jellyfin provider");
        assert_eq!(provider.server_type(), ServerType::Jellyfin);

        let mut subsonic = Server::new_async().await;
        let _jellyfin_should_not_run = subsonic
            .mock("POST", "/Users/AuthenticateByName")
            .expect(0)
            .create_async()
            .await;
        let _ping = subsonic
            .mock("GET", "/rest/ping.view")
            .match_query(Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"subsonic-response":{"status":"ok","version":"1.16.1"}}"#)
            .expect(1)
            .create_async()
            .await;

        let provider = connect(
            &subsonic.url(),
            &password_credentials(subsonic.url()),
            ServerTypeHint::Subsonic,
        )
        .await
        .expect("subsonic provider");
        assert_eq!(provider.server_type(), ServerType::Subsonic);
    }

    #[tokio::test]
    async fn factory_auto_all_fail_returns_unknown_type_error() {
        let mut server = Server::new_async().await;
        let _ping = server
            .mock("GET", "/rest/ping.view")
            .match_query(Matcher::Any)
            .with_status(404)
            .expect(1)
            .create_async()
            .await;
        let _auth = server
            .mock("POST", "/Users/AuthenticateByName")
            .with_status(404)
            .expect(1)
            .create_async()
            .await;

        let result = connect(
            &server.url(),
            &password_credentials(server.url()),
            ServerTypeHint::Auto,
        )
        .await;

        assert!(
            matches!(result, Err(ProviderError::UnsupportedCapability(ref message)) if message == "Unknown server type at this URL")
        );
    }

    #[tokio::test]
    async fn factory_subsonic_failure_does_not_leak_credentials() {
        let mut server = Server::new_async().await;
        let _ping = server
            .mock("GET", "/rest/ping.view")
            .match_query(Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"subsonic-response":{"status":"failed","version":"1.16.1","error":{"code":40,"message":"Bad auth u=alexis&p=secret-password&t=token-value&s=salt-value"}}}"#,
            )
            .expect(1)
            .create_async()
            .await;

        let result = connect(
            &server.url(),
            &password_credentials(server.url()),
            ServerTypeHint::Subsonic,
        )
        .await;

        let message = match result {
            Ok(_) => panic!("connect should fail"),
            Err(error) => error.to_string(),
        };
        assert!(!message.contains("alexis"), "username leaked: {message}");
        assert!(
            !message.contains("secret-password"),
            "password leaked: {message}"
        );
        assert!(!message.contains("token-value"), "token leaked: {message}");
        assert!(!message.contains("salt-value"), "salt leaked: {message}");
        assert!(message.contains("[REDACTED]"));
    }

    #[test]
    fn sanitize_secret_message_redacts_query_params_only() {
        assert_eq!(
            sanitize_secret_message("status=ok type=json"),
            "status=ok type=json",
            "mid-word keys must not be redacted"
        );
        assert_eq!(
            sanitize_secret_message("error ?p=secret&t=token123 rest"),
            "error ?p=[redacted]&t=[redacted] rest"
        );
        assert_eq!(
            sanitize_secret_message("password=raw-pass"),
            "password=[redacted]"
        );
        assert_eq!(
            sanitize_secret_message("msg token=abc end"),
            "msg token=[redacted] end"
        );
    }

    #[tokio::test]
    async fn factory_jellyfin_auth_failure_does_not_leak_password() {
        let mut server = Server::new_async().await;
        let _auth = server
            .mock("POST", "/Users/AuthenticateByName")
            .with_status(401)
            .with_header("content-type", "application/json")
            .with_body(r#"{"message":"Invalid username or password"}"#)
            .expect(1)
            .create_async()
            .await;

        let creds = password_credentials(server.url());
        let result = connect(&server.url(), &creds, ServerTypeHint::Jellyfin).await;

        match result {
            Ok(_) => panic!("expected a connection error"),
            Err(e) => {
                let msg = e.to_string();
                assert!(
                    !msg.contains("secret-password"),
                    "password must not appear in error: {msg}"
                );
            }
        }
    }

    #[test]
    fn server_type_slugs_are_ui_contract_values() {
        assert_eq!(server_type_slug(ServerType::Jellyfin), Some("jellyfin"));
        assert_eq!(server_type_slug(ServerType::Subsonic), Some("subsonic"));
        assert_eq!(
            server_type_slug(ServerType::OpenSubsonic),
            Some("openSubsonic")
        );
        assert_eq!(server_type_slug(ServerType::Unknown), None);
    }
}
