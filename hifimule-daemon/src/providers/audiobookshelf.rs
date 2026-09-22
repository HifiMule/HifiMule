//! Audiobookshelf v2.36.1 connection and library-scope adapter.
//!
//! This story intentionally exposes no catalogue operations. Authentication,
//! tokens, and upstream library identifiers stay daemon-side.

use super::{
    BrowseCapabilities, Capabilities, MediaProvider, ProviderChangeContext, ProviderError,
    ProviderLibraryRole, ScrobbleRequest, ServerType, TranscodeProfile,
};
use crate::domain::models::{
    Album, AlbumWithTracks, Artist, ArtistWithAlbums, ChangeEvent, Library, Playlist,
    PlaylistWithTracks, SearchResult,
};
use async_trait::async_trait;
use reqwest::{Client, StatusCode};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::Duration;
use tokio::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredLibrary {
    pub id: String,
    pub name: String,
    pub role: ProviderLibraryRole,
}

pub struct AudiobookshelfDiscovery {
    pub provider: AudiobookshelfProvider,
    pub libraries: Vec<DiscoveredLibrary>,
}

impl fmt::Debug for AudiobookshelfDiscovery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AudiobookshelfDiscovery")
            .field("provider", &self.provider)
            .field("library_count", &self.libraries.len())
            .finish()
    }
}

struct AuthSession {
    access_token: SecretString,
    refresh_token: Option<SecretString>,
}

impl fmt::Debug for AuthSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuthSession")
            .field("access_token", &"[redacted]")
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "[redacted]"),
            )
            .finish()
    }
}

pub struct AudiobookshelfProvider {
    client: Client,
    base_url: String,
    session: Mutex<AuthSession>,
    library_id: Option<String>,
    library_role: Option<ProviderLibraryRole>,
    server_version: Option<String>,
}

impl fmt::Debug for AudiobookshelfProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AudiobookshelfProvider")
            .field("base_url", &"[redacted-url]")
            .field("session", &"[redacted]")
            .field("library_scoped", &self.library_id.is_some())
            .field("library_role", &self.library_role)
            .field("server_version", &self.server_version)
            .finish()
    }
}

#[derive(Serialize)]
struct LoginRequest<'a> {
    username: &'a str,
    password: &'a str,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoginResponse {
    user: TokenUser,
    #[serde(default, alias = "server_version")]
    server_version: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenUser {
    #[serde(alias = "access_token")]
    access_token: String,
    #[serde(default, alias = "refresh_token")]
    refresh_token: Option<String>,
}

#[derive(Deserialize)]
struct RefreshResponse {
    user: TokenUser,
}

#[derive(Deserialize)]
struct LibrariesEnvelope {
    libraries: Vec<LibraryDto>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LibraryDto {
    id: String,
    #[serde(default)]
    name: String,
    media_type: String,
}

impl AudiobookshelfProvider {
    fn client() -> Result<Client, ProviderError> {
        Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| ProviderError::Http {
                status: None,
                message: super::sanitize_secret_message(&error.to_string()),
            })
    }

    pub async fn discover(
        url: &str,
        username: &str,
        password: &str,
    ) -> Result<AudiobookshelfDiscovery, ProviderError> {
        let mut provider = Self::login(url, username, password).await?;
        let libraries = provider.fetch_libraries().await?;
        Ok(AudiobookshelfDiscovery {
            provider,
            libraries,
        })
    }

    pub async fn from_stored_config(
        url: &str,
        username: &str,
        password: &str,
        library_id: &str,
        role: ProviderLibraryRole,
    ) -> Result<Self, ProviderError> {
        if library_id.is_empty() {
            return Err(ProviderError::StaleConfiguration(
                "missing Audiobookshelf library scope".into(),
            ));
        }
        let mut discovery = Self::discover(url, username, password).await?;
        let discovered = discovery
            .libraries
            .iter()
            .find(|library| library.id == library_id)
            .ok_or_else(|| {
                ProviderError::StaleConfiguration(
                    "configured Audiobookshelf library is missing".into(),
                )
            })?;
        if discovered.role != role {
            return Err(ProviderError::StaleConfiguration(
                "configured Audiobookshelf library role changed".into(),
            ));
        }
        discovery.provider.library_id = Some(library_id.to_string());
        discovery.provider.library_role = Some(role);
        Ok(discovery.provider)
    }

    pub fn scope_to(
        mut self,
        library_id: String,
        role: ProviderLibraryRole,
    ) -> Result<Self, ProviderError> {
        if library_id.is_empty() {
            return Err(ProviderError::StaleConfiguration(
                "missing Audiobookshelf library scope".into(),
            ));
        }
        self.library_id = Some(library_id);
        self.library_role = Some(role);
        Ok(self)
    }

    async fn login(url: &str, username: &str, password: &str) -> Result<Self, ProviderError> {
        let client = Self::client()?;
        let base_url = url.trim().trim_end_matches('/').to_string();
        reqwest::Url::parse(&base_url).map_err(|_| ProviderError::Http {
            status: None,
            message: "invalid Audiobookshelf URL".into(),
        })?;
        let response = client
            .post(format!("{base_url}/login"))
            .header("x-return-tokens", "true")
            .json(&LoginRequest { username, password })
            .send()
            .await
            .map_err(transport_error)?;
        check_auth_status(&response)?;
        let login: LoginResponse = response.json().await.map_err(deserialization_error)?;
        if login.user.access_token.trim().is_empty() {
            return Err(ProviderError::Deserialization(
                "Audiobookshelf login omitted access token".into(),
            ));
        }
        Ok(Self {
            client,
            base_url,
            session: Mutex::new(AuthSession {
                access_token: SecretString::new(login.user.access_token),
                refresh_token: login
                    .user
                    .refresh_token
                    .filter(|token| !token.trim().is_empty())
                    .map(SecretString::new),
            }),
            library_id: None,
            library_role: None,
            server_version: login
                .server_version
                .filter(|value| !value.trim().is_empty()),
        })
    }

    async fn fetch_libraries(&mut self) -> Result<Vec<DiscoveredLibrary>, ProviderError> {
        let endpoint = format!("{}/api/libraries", self.base_url);
        let mut session = self.session.lock().await;
        let mut response = self
            .client
            .get(&endpoint)
            .bearer_auth(session.access_token.expose_secret())
            .send()
            .await
            .map_err(transport_error)?;
        if response.status() == StatusCode::UNAUTHORIZED {
            let refresh = session.refresh_token.as_ref().ok_or_else(|| {
                ProviderError::Auth("Audiobookshelf authentication expired".into())
            })?;
            let refreshed = self
                .client
                .post(format!("{}/auth/refresh", self.base_url))
                .header("x-refresh-token", refresh.expose_secret())
                .send()
                .await
                .map_err(transport_error)?;
            check_auth_status(&refreshed)?;
            let tokens: RefreshResponse = refreshed.json().await.map_err(deserialization_error)?;
            if tokens.user.access_token.trim().is_empty() {
                return Err(ProviderError::Deserialization(
                    "Audiobookshelf refresh omitted access token".into(),
                ));
            }
            session.access_token = SecretString::new(tokens.user.access_token);
            if let Some(refresh_token) = tokens
                .user
                .refresh_token
                .filter(|token| !token.trim().is_empty())
            {
                session.refresh_token = Some(SecretString::new(refresh_token));
            }
            response = self
                .client
                .get(&endpoint)
                .bearer_auth(session.access_token.expose_secret())
                .send()
                .await
                .map_err(transport_error)?;
        }
        check_status(&response)?;
        let envelope: LibrariesEnvelope = response.json().await.map_err(deserialization_error)?;
        envelope
            .libraries
            .into_iter()
            .filter_map(|library| {
                let role = match library.media_type.as_str() {
                    "book" => ProviderLibraryRole::Audiobook,
                    "podcast" => ProviderLibraryRole::Podcast,
                    _ => return None,
                };
                Some(DiscoveredLibrary {
                    id: library.id,
                    name: if library.name.trim().is_empty() {
                        match role {
                            ProviderLibraryRole::Audiobook => "Books".into(),
                            ProviderLibraryRole::Podcast => "Podcasts".into(),
                        }
                    } else {
                        library.name
                    },
                    role,
                })
            })
            .collect::<Vec<_>>()
            .pipe(Ok)
    }
}

trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
        f(self)
    }
}
impl<T> Pipe for T {}

fn transport_error(error: reqwest::Error) -> ProviderError {
    ProviderError::Http {
        status: error.status().map(|status| status.as_u16()),
        message: super::sanitize_secret_message(&error.to_string()),
    }
}

fn deserialization_error(error: reqwest::Error) -> ProviderError {
    ProviderError::Deserialization(super::sanitize_secret_message(&error.to_string()))
}

fn check_status(response: &reqwest::Response) -> Result<(), ProviderError> {
    match response.status() {
        status if status.is_success() => Ok(()),
        StatusCode::UNAUTHORIZED => Err(ProviderError::Auth(
            "Audiobookshelf authentication failed".into(),
        )),
        StatusCode::FORBIDDEN => Err(ProviderError::Forbidden),
        StatusCode::NOT_FOUND => Err(ProviderError::StaleConfiguration(
            "configured Audiobookshelf library is missing".into(),
        )),
        StatusCode::TOO_MANY_REQUESTS => Err(ProviderError::RateLimited {
            retry_after_seconds: response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse().ok()),
        }),
        status => Err(ProviderError::Http {
            status: Some(status.as_u16()),
            message: "Audiobookshelf request failed".into(),
        }),
    }
}

fn check_auth_status(response: &reqwest::Response) -> Result<(), ProviderError> {
    match response.status() {
        status if status.is_success() => Ok(()),
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Err(ProviderError::Auth(
            "Audiobookshelf authentication failed".into(),
        )),
        StatusCode::TOO_MANY_REQUESTS => Err(ProviderError::RateLimited {
            retry_after_seconds: response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse().ok()),
        }),
        status => Err(ProviderError::Http {
            status: Some(status.as_u16()),
            message: "Audiobookshelf authentication request failed".into(),
        }),
    }
}

fn unsupported(operation: &str) -> ProviderError {
    ProviderError::UnsupportedCapability(format!(
        "{operation} is not supported for Audiobookshelf in this release"
    ))
}

#[async_trait]
impl MediaProvider for AudiobookshelfProvider {
    async fn list_libraries(&self) -> Result<Vec<Library>, ProviderError> {
        Err(unsupported("list_libraries"))
    }
    async fn list_artists(
        &self,
        _library_id: Option<&str>,
        _letter: Option<&str>,
        _offset: u32,
        _limit: u32,
    ) -> Result<(Vec<Artist>, u32), ProviderError> {
        Err(unsupported("list_artists"))
    }
    async fn get_artist(&self, _artist_id: &str) -> Result<ArtistWithAlbums, ProviderError> {
        Err(unsupported("get_artist"))
    }
    async fn list_albums(
        &self,
        _library_id: Option<&str>,
        _letter: Option<&str>,
        _offset: u32,
        _limit: u32,
    ) -> Result<(Vec<Album>, u32), ProviderError> {
        Err(unsupported("list_albums"))
    }
    async fn get_album(&self, _album_id: &str) -> Result<AlbumWithTracks, ProviderError> {
        Err(unsupported("get_album"))
    }
    async fn list_playlists(&self) -> Result<Vec<Playlist>, ProviderError> {
        Err(unsupported("list_playlists"))
    }
    async fn get_playlist(&self, _playlist_id: &str) -> Result<PlaylistWithTracks, ProviderError> {
        Err(unsupported("get_playlist"))
    }
    async fn search(&self, _query: &str) -> Result<SearchResult, ProviderError> {
        Err(unsupported("search"))
    }
    async fn download_url(
        &self,
        _song_id: &str,
        _profile: Option<&TranscodeProfile>,
    ) -> Result<String, ProviderError> {
        Err(unsupported("download_url"))
    }
    async fn cover_art_url(&self, _cover_art_id: &str) -> Result<String, ProviderError> {
        Err(unsupported("cover_art_url"))
    }
    async fn changes_since_with_context(
        &self,
        _token: Option<&str>,
        _context: &ProviderChangeContext,
    ) -> Result<Vec<ChangeEvent>, ProviderError> {
        Err(unsupported("changes_since"))
    }
    async fn scrobble(&self, _request: ScrobbleRequest) -> Result<(), ProviderError> {
        Err(unsupported("scrobble"))
    }
    fn server_type(&self) -> ServerType {
        ServerType::Audiobookshelf
    }
    fn library_role(&self) -> Option<ProviderLibraryRole> {
        self.library_role
    }
    fn server_version(&self) -> Option<&str> {
        self.server_version.as_deref()
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            open_subsonic: false,
            supports_changes_since: false,
            supports_server_transcoding: false,
            supports_playlist_write: false,
            browse: BrowseCapabilities::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::{Matcher, Server};

    fn login_body() -> &'static str {
        r#"{"user":{"accessToken":"access-fixture","refreshToken":"refresh-fixture"},"serverVersion":"2.36.1"}"#
    }

    #[tokio::test]
    async fn discovery_logs_in_with_local_credentials_and_maps_safe_roles() {
        let mut server = Server::new_async().await;
        let login = server
            .mock("POST", "/login")
            .match_header("x-return-tokens", "true")
            .match_body(Matcher::PartialJson(serde_json::json!({
                "username": "Alexis",
                "password": "fixture-password"
            })))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        let libraries = server
            .mock("GET", "/api/libraries")
            .match_header("authorization", "Bearer access-fixture")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"libraries":[{"id":"book-id","name":"Fiction","mediaType":"book"},{"id":"pod-id","name":"Talks","mediaType":"podcast"}]}"#)
            .create_async()
            .await;

        let discovered =
            AudiobookshelfProvider::discover(&server.url(), "Alexis", "fixture-password")
                .await
                .unwrap();
        assert_eq!(discovered.libraries.len(), 2);
        assert_eq!(discovered.libraries[0].role, ProviderLibraryRole::Audiobook);
        assert_eq!(discovered.libraries[1].role, ProviderLibraryRole::Podcast);
        assert_eq!(discovered.provider.server_version(), Some("2.36.1"));
        assert!(!format!("{:?}", discovered).contains("fixture-password"));
        assert!(!format!("{:?}", discovered).contains("access-fixture"));
        login.assert_async().await;
        libraries.assert_async().await;
    }

    #[tokio::test]
    async fn discovery_refreshes_once_after_access_401() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        let expired = server
            .mock("GET", "/api/libraries")
            .match_header("authorization", "Bearer access-fixture")
            .with_status(401)
            .expect(1)
            .create_async()
            .await;
        let refresh = server
            .mock("POST", "/auth/refresh")
            .match_header("x-refresh-token", "refresh-fixture")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"user":{"accessToken":"access-refreshed"}}"#)
            .expect(1)
            .create_async()
            .await;
        let retried = server
            .mock("GET", "/api/libraries")
            .match_header("authorization", "Bearer access-refreshed")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"libraries":[]}"#)
            .expect(1)
            .create_async()
            .await;
        AudiobookshelfProvider::discover(&server.url(), "user", "password")
            .await
            .unwrap();
        expired.assert_async().await;
        refresh.assert_async().await;
        retried.assert_async().await;
    }

    #[tokio::test]
    async fn discovery_never_refreshes_more_than_once() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        let expired = server
            .mock("GET", "/api/libraries")
            .with_status(401)
            .expect(2)
            .create_async()
            .await;
        let refresh = server
            .mock("POST", "/auth/refresh")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"user":{"accessToken":"access-refreshed"}}"#)
            .expect(1)
            .create_async()
            .await;

        let error = AudiobookshelfProvider::discover(&server.url(), "user", "password")
            .await
            .unwrap_err();
        assert!(matches!(error, ProviderError::Auth(_)));
        expired.assert_async().await;
        refresh.assert_async().await;
    }

    #[tokio::test]
    async fn discovery_classifies_forbidden_missing_and_server_failures() {
        for (status, expected) in [(403, "forbidden"), (404, "stale"), (500, "http")] {
            let mut server = Server::new_async().await;
            server
                .mock("POST", "/login")
                .with_status(200)
                .with_header("content-type", "application/json")
                .with_body(login_body())
                .create_async()
                .await;
            server
                .mock("GET", "/api/libraries")
                .with_status(status)
                .with_body("provider-secret-body")
                .create_async()
                .await;

            let error = AudiobookshelfProvider::discover(&server.url(), "user", "password")
                .await
                .unwrap_err();
            match expected {
                "forbidden" => assert!(matches!(&error, ProviderError::Forbidden)),
                "stale" => assert!(matches!(&error, ProviderError::StaleConfiguration(_))),
                "http" => {
                    assert!(matches!(
                        &error,
                        ProviderError::Http {
                            status: Some(500),
                            ..
                        }
                    ))
                }
                _ => unreachable!(),
            }
            assert!(!format!("{error:?}").contains("provider-secret-body"));
        }
    }

    #[tokio::test]
    async fn rate_limit_preserves_only_valid_retry_after() {
        let mut server = Server::new_async().await;
        let rate_limited = server
            .mock("POST", "/login")
            .with_status(429)
            .with_header("retry-after", "21")
            .create_async()
            .await;
        let error = AudiobookshelfProvider::discover(&server.url(), "user", "password")
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            ProviderError::RateLimited {
                retry_after_seconds: Some(21)
            }
        ));
        rate_limited.assert_async().await;
    }

    #[tokio::test]
    async fn discovery_rejects_legacy_flattened_token_shape() {
        let mut server = Server::new_async().await;
        let login = server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"accessToken":"must-not-be-accepted","refreshToken":"also-flat"}"#)
            .expect(1)
            .create_async()
            .await;

        let error = AudiobookshelfProvider::discover(&server.url(), "user", "password")
            .await
            .unwrap_err();
        assert!(matches!(error, ProviderError::Deserialization(_)));
        login.assert_async().await;
    }

    #[tokio::test]
    async fn discovery_rejects_empty_nested_login_and_refresh_access_tokens() {
        let mut login_server = Server::new_async().await;
        login_server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"user":{"accessToken":" "}}"#)
            .create_async()
            .await;
        let login_error = AudiobookshelfProvider::discover(&login_server.url(), "user", "password")
            .await
            .unwrap_err();
        assert!(matches!(login_error, ProviderError::Deserialization(_)));

        let mut refresh_server = Server::new_async().await;
        refresh_server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        refresh_server
            .mock("GET", "/api/libraries")
            .with_status(401)
            .expect(1)
            .create_async()
            .await;
        refresh_server
            .mock("POST", "/auth/refresh")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"user":{"accessToken":""}}"#)
            .expect(1)
            .create_async()
            .await;
        let refresh_error =
            AudiobookshelfProvider::discover(&refresh_server.url(), "user", "password")
                .await
                .unwrap_err();
        assert!(matches!(refresh_error, ProviderError::Deserialization(_)));
    }
}
