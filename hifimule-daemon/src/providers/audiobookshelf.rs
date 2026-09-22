//! Audiobookshelf v2.36.1 connection and library-scope adapter.
//!
//! This story intentionally exposes no catalogue operations. Authentication,
//! tokens, and upstream library identifiers stay daemon-side.

use super::{
    BrowseCapabilities, BrowseMode, Capabilities, MediaProvider, ProviderChangeContext,
    ProviderError, ProviderLibraryRole, ScrobbleRequest, ServerType, TranscodeProfile,
};
use crate::domain::models::{
    Album, AlbumWithTracks, Artist, ArtistWithAlbums, ChangeEvent, ChapterMarker, Credit,
    CreditRole, Library, Playlist, PlaylistWithTracks, ProviderIdentity, ProviderItemMetadata,
    ProviderPartIdentity, SearchResult, Song,
};
use async_trait::async_trait;
use reqwest::{Client, StatusCode};
use secrecy::{ExposeSecret, SecretString};
use serde::de::DeserializeOwned;
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CataloguePageDto {
    total: u64,
    #[serde(default)]
    results: Vec<BookDto>,
}

#[derive(Deserialize)]
struct BookSearchDto {
    #[serde(default)]
    book: Vec<BookDto>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BookDto {
    #[serde(deserialize_with = "deserialize_nonempty")]
    id: String,
    #[serde(deserialize_with = "deserialize_nonempty")]
    library_id: String,
    media_type: String,
    media: BookMediaDto,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BookMediaDto {
    #[serde(deserialize_with = "deserialize_nonempty")]
    id: String,
    #[serde(default)]
    cover_path: Option<String>,
    #[serde(default)]
    metadata: BookMetadataDto,
    #[serde(default)]
    audio_files: Vec<AudioFileDto>,
    #[serde(default)]
    chapters: Vec<ChapterDto>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BookMetadataDto {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    authors: Vec<BookAuthorDto>,
    #[serde(default)]
    narrators: Vec<String>,
    #[serde(default)]
    published_year: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct BookAuthorDto {
    id: Option<String>,
    name: String,
}

#[derive(Debug, Deserialize)]
struct AudioFileDto {
    #[serde(deserialize_with = "deserialize_nonempty")]
    id: String,
    index: serde_json::Value,
    #[serde(default)]
    duration: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct ChapterDto {
    #[serde(deserialize_with = "deserialize_nonempty")]
    id: String,
    start: f64,
    end: f64,
}

impl AudiobookshelfProvider {
    const MAX_RESPONSE_BYTES: u64 = 16 * 1024 * 1024;
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

    fn audiobook_library_id(&self) -> Result<&str, ProviderError> {
        match (self.library_id.as_deref(), self.library_role) {
            (Some(id), Some(ProviderLibraryRole::Audiobook)) => Ok(id),
            (_, Some(ProviderLibraryRole::Podcast)) => Err(unsupported("book catalogue")),
            _ => Err(ProviderError::StaleConfiguration(
                "missing Audiobookshelf library scope".into(),
            )),
        }
    }

    async fn protected_get(&self, endpoint: &str) -> Result<reqwest::Response, ProviderError> {
        let mut session = self.session.lock().await;
        let mut response = self
            .client
            .get(endpoint)
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
            response = self
                .client
                .get(endpoint)
                .bearer_auth(session.access_token.expose_secret())
                .send()
                .await
                .map_err(transport_error)?;
        }
        if response
            .content_length()
            .is_some_and(|length| length > Self::MAX_RESPONSE_BYTES)
        {
            return Err(ProviderError::Http {
                status: Some(response.status().as_u16()),
                message: "Audiobookshelf response exceeds configured limit".into(),
            });
        }
        Ok(response)
    }

    async fn catalogue_page(
        &self,
        offset: u32,
        limit: u32,
    ) -> Result<(Vec<Album>, u32), ProviderError> {
        let library = self.audiobook_library_id()?;
        if limit == 0 {
            return Ok((Vec::new(), 0));
        }
        if !offset.is_multiple_of(limit) {
            return Err(ProviderError::UnsupportedCapability(
                "Audiobookshelf catalogue requires page-aligned offsets".into(),
            ));
        }
        let page = offset
            .checked_div(limit)
            .ok_or_else(|| ProviderError::Deserialization("invalid catalogue page".into()))?;
        let endpoint = format!(
            "{}/api/libraries/{library}/items?page={page}&limit={limit}",
            self.base_url
        );
        let response = self.protected_get(&endpoint).await?;
        check_status(&response)?;
        let page: CataloguePageDto = bounded_json(response).await?;
        let total = u32::try_from(page.total).unwrap_or(u32::MAX);
        let albums = page
            .results
            .into_iter()
            .filter(|book| book.media_type == "book")
            .map(|book| book_album(library, book))
            .collect::<Result<Vec<_>, _>>()?;
        Ok((albums, total))
    }

    async fn catalogue_book(&self, public_id: &str) -> Result<AlbumWithTracks, ProviderError> {
        let library = self.audiobook_library_id()?;
        let (encoded_library, item_id, media_id) = parse_opaque_id("album", public_id)?;
        if encoded_library != library {
            return Err(ProviderError::NotFound {
                item_type: "album".into(),
                id: public_id.into(),
            });
        }
        let endpoint = item_endpoint(&self.base_url, &item_id)?;
        let response = self.protected_get(&endpoint).await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Err(ProviderError::NotFound {
                item_type: "album".into(),
                id: public_id.into(),
            });
        }
        check_status(&response)?;
        let book: BookDto = bounded_json(response).await?;
        if book.media_type != "book"
            || book.library_id != library
            || book.id != item_id
            || book.media.id != media_id
        {
            return Err(ProviderError::Deserialization(
                "invalid Audiobookshelf book identity".into(),
            ));
        }
        let album = book_album(
            library,
            BookDto {
                id: book.id.clone(),
                library_id: book.library_id.clone(),
                media_type: book.media_type.clone(),
                media: BookMediaDto {
                    id: book.media.id.clone(),
                    cover_path: book.media.cover_path.clone(),
                    metadata: BookMetadataDto {
                        title: book.media.metadata.title.clone(),
                        authors: book
                            .media
                            .metadata
                            .authors
                            .iter()
                            .map(|a| BookAuthorDto {
                                id: a.id.clone(),
                                name: a.name.clone(),
                            })
                            .collect(),
                        narrators: book.media.metadata.narrators.clone(),
                        published_year: book.media.metadata.published_year,
                    },
                    audio_files: book
                        .media
                        .audio_files
                        .iter()
                        .map(|file| AudioFileDto {
                            id: file.id.clone(),
                            index: file.index.clone(),
                            duration: file.duration,
                        })
                        .collect(),
                    chapters: Vec::new(),
                },
            },
        )?;
        let credits: Vec<Credit> = book
            .media
            .metadata
            .authors
            .iter()
            .map(|author| Credit {
                name: author.name.clone(),
                provider_id: author.id.clone(),
                role: CreditRole::Author,
            })
            .chain(book.media.metadata.narrators.iter().map(|name| Credit {
                name: name.clone(),
                provider_id: None,
                role: CreditRole::Narrator,
            }))
            .collect();
        let cover_reference = cover_reference(library, &book);
        let file_count = book.media.audio_files.len();
        let mut files = book.media.audio_files;
        files.sort_by(|left, right| {
            numeric_index(&left.index)
                .unwrap_or(u32::MAX)
                .cmp(&numeric_index(&right.index).unwrap_or(u32::MAX))
                .then_with(|| left.id.cmp(&right.id))
        });
        let mut part_identities = Vec::new();
        let tracks = files
            .into_iter()
            .filter_map(|file| {
                let index = numeric_index(&file.index)?;
                let id = opaque_id("track", &[library, &book.id, &book.media.id, &file.id]);
                part_identities.push(ProviderPartIdentity {
                    public_id: id.clone(),
                    audio_file_id: file.id.clone(),
                });
                Some(Song {
                    id,
                    title: part_title(book.media.metadata.title.as_deref(), index, file_count),
                    artist_id: book
                        .media
                        .metadata
                        .authors
                        .first()
                        .and_then(|a| a.id.clone()),
                    artist_name: book.media.metadata.authors.first().map(|a| a.name.clone()),
                    album_id: Some(public_id.into()),
                    album_title: book.media.metadata.title.clone(),
                    duration_seconds: duration_seconds(file.duration),
                    bitrate_kbps: None,
                    track_number: Some(index),
                    disc_number: None,
                    cover_art_id: cover_reference.clone(),
                    date_added: None,
                    last_played_at: None,
                    play_count: None,
                    is_favorite: None,
                    content_type: None,
                    suffix: None,
                    size_bytes: None,
                    album_loudness: Default::default(),
                    provider_metadata: ProviderItemMetadata {
                        identity: Some(ProviderIdentity {
                            library_id: library.into(),
                            library_item_id: book.id.clone(),
                            media_id: book.media.id.clone(),
                        }),
                        audio_file_id: Some(file.id.clone()),
                        cover_reference: cover_reference.clone(),
                        credits: credits.clone(),
                        ..Default::default()
                    },
                })
            })
            .collect();
        Ok(AlbumWithTracks {
            album,
            tracks,
            provider_metadata: ProviderItemMetadata {
                identity: Some(ProviderIdentity {
                    library_id: library.into(),
                    library_item_id: book.id.clone(),
                    media_id: book.media.id.clone(),
                }),
                audio_file_id: None,
                chapters: chapter_markers(&book.media.chapters),
                cover_reference,
                part_identities,
                credits,
            },
        })
    }

    async fn search_books(&self, query: &str) -> Result<SearchResult, ProviderError> {
        const SEARCH_LIMIT: u32 = 50;
        let library = self.audiobook_library_id()?;
        let mut url =
            reqwest::Url::parse(&format!("{}/api/libraries/{library}/search", self.base_url))
                .map_err(|_| ProviderError::Http {
                    status: None,
                    message: "invalid Audiobookshelf URL".into(),
                })?;
        url.query_pairs_mut()
            .append_pair("q", query)
            .append_pair("limit", &SEARCH_LIMIT.to_string());
        let response = self.protected_get(url.as_str()).await?;
        check_status(&response)?;
        let results: BookSearchDto = bounded_json(response).await?;
        let possibly_truncated = results.book.len() >= SEARCH_LIMIT as usize;
        let albums = results
            .book
            .into_iter()
            .filter(|book| book.media_type == "book")
            .map(|book| book_album(library, book))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(SearchResult {
            possibly_truncated,
            albums,
            ..Default::default()
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
            .find(|library| library.id == library_id);
        if discovered.is_none() {
            discovery
                .provider
                .validate_library_access(library_id)
                .await?;
            return Err(ProviderError::StaleConfiguration(
                "configured Audiobookshelf library was omitted from discovery".into(),
            ));
        }
        let discovered = discovered.expect("checked above");
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
            // The validated v2.36.1 login contract does not expose a version.
            server_version: None,
        })
    }

    async fn validate_library_access(&self, library_id: &str) -> Result<(), ProviderError> {
        let endpoint = format!(
            "{}/api/libraries/{}/items?page=0&limit=1",
            self.base_url, library_id
        );
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
        check_status(&response)
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

fn deserialize_nonempty<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    if value.trim().is_empty() {
        return Err(serde::de::Error::custom("required identity is empty"));
    }
    Ok(value)
}

async fn bounded_json<T: DeserializeOwned>(
    mut response: reqwest::Response,
) -> Result<T, ProviderError> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(transport_error)? {
        let next_len = body
            .len()
            .checked_add(chunk.len())
            .ok_or_else(|| ProviderError::Http {
                status: Some(response.status().as_u16()),
                message: "Audiobookshelf response exceeds configured limit".into(),
            })?;
        if next_len > AudiobookshelfProvider::MAX_RESPONSE_BYTES as usize {
            return Err(ProviderError::Http {
                status: Some(response.status().as_u16()),
                message: "Audiobookshelf response exceeds configured limit".into(),
            });
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body)
        .map_err(|_| ProviderError::Deserialization("invalid Audiobookshelf response".into()))
}

fn item_endpoint(base_url: &str, item_id: &str) -> Result<String, ProviderError> {
    let mut url =
        reqwest::Url::parse(&format!("{base_url}/api/items")).map_err(|_| ProviderError::Http {
            status: None,
            message: "invalid Audiobookshelf URL".into(),
        })?;
    url.path_segments_mut()
        .map_err(|_| ProviderError::Http {
            status: None,
            message: "invalid Audiobookshelf URL".into(),
        })?
        .push(item_id);
    Ok(url.into())
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
            retry_after_seconds: retry_after_seconds(response),
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
            retry_after_seconds: retry_after_seconds(response),
        }),
        status => Err(ProviderError::Http {
            status: Some(status.as_u16()),
            message: "Audiobookshelf authentication request failed".into(),
        }),
    }
}

fn retry_after_seconds(response: &reqwest::Response) -> Option<u64> {
    let value = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?;
    if let Ok(seconds) = value.parse() {
        return Some(seconds);
    }
    let deadline = chrono::DateTime::parse_from_rfc2822(value).ok()?;
    let remaining = deadline.timestamp() - chrono::Utc::now().timestamp();
    Some(remaining.max(0) as u64)
}

fn unsupported(operation: &str) -> ProviderError {
    ProviderError::UnsupportedCapability(format!(
        "{operation} is not supported for Audiobookshelf in this release"
    ))
}

fn opaque_id(kind: &str, parts: &[&str]) -> String {
    let encoded = parts
        .iter()
        .map(|part| {
            part.as_bytes()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join(".");
    format!("abs-{kind}-{encoded}")
}

fn parse_opaque_id(kind: &str, id: &str) -> Result<(String, String, String), ProviderError> {
    let prefix = format!("abs-{kind}-");
    let parts = id
        .strip_prefix(&prefix)
        .ok_or_else(|| ProviderError::NotFound {
            item_type: "album".into(),
            id: id.into(),
        })?
        .split('.')
        .map(|encoded| {
            if encoded.len() % 2 != 0 {
                return Err(());
            }
            (0..encoded.len())
                .step_by(2)
                .map(|index| u8::from_str_radix(&encoded[index..index + 2], 16).map_err(|_| ()))
                .collect::<Result<Vec<_>, _>>()
                .and_then(|bytes| String::from_utf8(bytes).map_err(|_| ()))
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ProviderError::NotFound {
            item_type: "album".into(),
            id: id.into(),
        })?;
    match parts.as_slice() {
        [library, item, media] => Ok((library.clone(), item.clone(), media.clone())),
        _ => Err(ProviderError::NotFound {
            item_type: "album".into(),
            id: id.into(),
        }),
    }
}

fn numeric_index(value: &serde_json::Value) -> Option<u32> {
    value
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value > 0)
}

fn duration_seconds(value: Option<f64>) -> u32 {
    value
        .filter(|value| value.is_finite() && *value >= 0.0)
        .map(|value| value.round().min(u32::MAX as f64) as u32)
        .unwrap_or_default()
}

fn validated_duration_seconds(value: Option<f64>) -> Option<u32> {
    value
        .filter(|value| value.is_finite() && *value >= 0.0)
        .map(|value| value.round().min(u32::MAX as f64) as u32)
}

fn part_title(book_title: Option<&str>, index: u32, file_count: usize) -> String {
    let title = book_title.unwrap_or_default();
    if file_count <= 1 {
        return title.to_string();
    }
    if title.is_empty() {
        format!("Part {index}")
    } else {
        format!("{title} — Part {index}")
    }
}

fn book_credits(book: &BookDto) -> Vec<Credit> {
    book.media
        .metadata
        .authors
        .iter()
        .map(|author| Credit {
            name: author.name.clone(),
            provider_id: author.id.clone(),
            role: CreditRole::Author,
        })
        .chain(book.media.metadata.narrators.iter().map(|name| Credit {
            name: name.clone(),
            provider_id: None,
            role: CreditRole::Narrator,
        }))
        .collect()
}

fn cover_reference(library_id: &str, book: &BookDto) -> Option<String> {
    book.media
        .cover_path
        .as_deref()
        .filter(|path| !path.trim().is_empty())
        .map(|_| opaque_id("cover", &[library_id, &book.id, &book.media.id]))
}

fn audiobookshelf_browse_capabilities(role: Option<ProviderLibraryRole>) -> BrowseCapabilities {
    match role {
        Some(ProviderLibraryRole::Audiobook) => BrowseCapabilities {
            list_modes: vec![BrowseMode::Albums],
        },
        _ => BrowseCapabilities::default(),
    }
}

fn chapter_markers(chapters: &[ChapterDto]) -> Vec<ChapterMarker> {
    chapters
        .iter()
        .filter_map(|chapter| {
            let start_seconds = duration_seconds(Some(chapter.start));
            let end_seconds = duration_seconds(Some(chapter.end));
            (chapter.start.is_finite()
                && chapter.end.is_finite()
                && chapter.start >= 0.0
                && chapter.end >= chapter.start)
                .then_some(ChapterMarker {
                    id: chapter.id.clone(),
                    start_seconds,
                    end_seconds,
                })
        })
        .collect()
}

fn book_album(library_id: &str, book: BookDto) -> Result<Album, ProviderError> {
    if book.library_id != library_id {
        return Err(ProviderError::Deserialization(
            "invalid Audiobookshelf library identity".into(),
        ));
    }
    let primary = book.media.metadata.authors.first();
    let valid_files = book
        .media
        .audio_files
        .iter()
        .filter(|file| numeric_index(&file.index).is_some())
        .collect::<Vec<_>>();
    let duration_seconds = if valid_files.is_empty() {
        None
    } else {
        valid_files.iter().try_fold(0u32, |total, file| {
            total.checked_add(validated_duration_seconds(file.duration)?)
        })
    };
    let song_count = u32::try_from(valid_files.len()).ok();
    let cover_art_id = cover_reference(library_id, &book);
    let credits = book_credits(&book);
    let mut part_identities = book
        .media
        .audio_files
        .iter()
        .filter_map(|file| {
            numeric_index(&file.index).map(|index| {
                (
                    index,
                    ProviderPartIdentity {
                        public_id: opaque_id(
                            "track",
                            &[library_id, &book.id, &book.media.id, &file.id],
                        ),
                        audio_file_id: file.id.clone(),
                    },
                )
            })
        })
        .collect::<Vec<_>>();
    part_identities.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.1.audio_file_id.cmp(&right.1.audio_file_id))
    });
    let part_identities = part_identities
        .into_iter()
        .map(|(_, identity)| identity)
        .collect();
    let chapters = chapter_markers(&book.media.chapters);
    Ok(Album {
        id: opaque_id("album", &[library_id, &book.id, &book.media.id]),
        title: book.media.metadata.title.unwrap_or_default(),
        artist_id: primary.and_then(|author| author.id.clone()),
        artist_name: primary.map(|author| author.name.clone()),
        year: book.media.metadata.published_year,
        song_count,
        duration_seconds,
        cover_art_id: cover_art_id.clone(),
        provider_metadata: ProviderItemMetadata {
            identity: Some(ProviderIdentity {
                library_id: library_id.into(),
                library_item_id: book.id,
                media_id: book.media.id,
            }),
            cover_reference: cover_art_id,
            credits,
            part_identities,
            chapters,
            ..Default::default()
        },
    })
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
        library_id: Option<&str>,
        letter: Option<&str>,
        offset: u32,
        limit: u32,
    ) -> Result<(Vec<Album>, u32), ProviderError> {
        if library_id.is_some() || letter.is_some_and(|value| !value.is_empty()) {
            return Err(unsupported("list_albums filters"));
        }
        self.catalogue_page(offset, limit).await
    }
    async fn get_album(&self, album_id: &str) -> Result<AlbumWithTracks, ProviderError> {
        self.catalogue_book(album_id).await
    }
    async fn list_playlists(&self) -> Result<Vec<Playlist>, ProviderError> {
        Err(unsupported("list_playlists"))
    }
    async fn get_playlist(&self, _playlist_id: &str) -> Result<PlaylistWithTracks, ProviderError> {
        Err(unsupported("get_playlist"))
    }
    async fn search(&self, query: &str) -> Result<SearchResult, ProviderError> {
        self.search_books(query).await
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
    async fn fetch_cover_art(
        &self,
        cover_art_id: &str,
    ) -> Result<reqwest::Response, ProviderError> {
        let library = self.audiobook_library_id()?;
        let (encoded_library, item_id, _) = parse_opaque_id("cover", cover_art_id)?;
        if encoded_library != library {
            return Err(ProviderError::NotFound {
                item_type: "cover".into(),
                id: cover_art_id.into(),
            });
        }
        let item = item_endpoint(&self.base_url, &item_id)?;
        let endpoint = format!("{item}/cover");
        let response = self.protected_get(&endpoint).await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Err(ProviderError::NotFound {
                item_type: "cover".into(),
                id: cover_art_id.into(),
            });
        }
        check_status(&response)?;
        Ok(response)
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
            browse: audiobookshelf_browse_capabilities(self.library_role),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::BrowseMode;
    use mockito::{Matcher, Server};

    fn login_body() -> &'static str {
        r#"{"user":{"accessToken":"access-fixture","refreshToken":"refresh-fixture"}}"#
    }

    #[test]
    fn opaque_catalogue_ids_are_deterministic_and_delimiter_safe() {
        let id = opaque_id("album", &["library.a", "item/with.dot", "media:1"]);
        assert_eq!(
            parse_opaque_id("album", &id).unwrap(),
            ("library.a".into(), "item/with.dot".into(), "media:1".into())
        );
        assert!(parse_opaque_id("album", "abs-album-not-hex").is_err());
    }

    #[test]
    fn duration_and_part_index_mapping_reject_invalid_values() {
        assert_eq!(duration_seconds(Some(1.5)), 2);
        assert_eq!(duration_seconds(Some(-1.0)), 0);
        assert_eq!(duration_seconds(Some(f64::NAN)), 0);
        assert_eq!(numeric_index(&serde_json::json!(10)), Some(10));
        assert_eq!(numeric_index(&serde_json::json!(0)), None);
        assert_eq!(numeric_index(&serde_json::json!("10")), None);
        let chapter_dtos = vec![
            ChapterDto {
                id: "keep".into(),
                start: 0.0,
                end: 24.5,
            },
            ChapterDto {
                id: "drop".into(),
                start: 2.0,
                end: 1.0,
            },
        ];
        let chapters = chapter_markers(&chapter_dtos);
        assert_eq!(
            chapters,
            vec![ChapterMarker {
                id: "keep".into(),
                start_seconds: 0,
                end_seconds: 25
            }]
        );
    }

    #[test]
    fn item_endpoint_encodes_untrusted_identity_as_one_path_segment() {
        let endpoint = item_endpoint("https://example.test", "../admin?token=leak").unwrap();
        assert_eq!(
            endpoint,
            "https://example.test/api/items/..%2Fadmin%3Ftoken=leak"
        );
    }

    #[test]
    fn audiobook_scope_publishes_only_album_browsing() {
        assert_eq!(
            audiobookshelf_browse_capabilities(Some(ProviderLibraryRole::Audiobook)).list_modes,
            vec![BrowseMode::Albums]
        );
        assert!(
            audiobookshelf_browse_capabilities(Some(ProviderLibraryRole::Podcast))
                .list_modes
                .is_empty()
        );
    }

    #[tokio::test]
    async fn cover_fetch_uses_bearer_auth_and_refreshes_once() {
        let mut server = Server::new_async().await;
        let _login = server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        let _expired_cover = server
            .mock("GET", "/api/items/book-1/cover")
            .match_header("authorization", "Bearer access-fixture")
            .with_status(401)
            .expect(1)
            .create_async()
            .await;
        let _refresh = server
            .mock("POST", "/auth/refresh")
            .match_header("x-refresh-token", "refresh-fixture")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"user":{"accessToken":"refreshed"}}"#)
            .expect(1)
            .create_async()
            .await;
        let _cover = server
            .mock("GET", "/api/items/book-1/cover")
            .match_header("authorization", "Bearer refreshed")
            .with_status(200)
            .with_header("content-type", "image/jpeg")
            .with_body(vec![1_u8, 2, 3])
            .expect(1)
            .create_async()
            .await;

        let provider = AudiobookshelfProvider::login(&server.url(), "user", "password")
            .await
            .unwrap()
            .scope_to("books".into(), ProviderLibraryRole::Audiobook)
            .unwrap();
        let cover = opaque_id("cover", &["books", "book-1", "media-1"]);
        let response = provider.fetch_cover_art(&cover).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.bytes().await.unwrap().as_ref(), &[1, 2, 3]);
    }

    #[test]
    fn fixture_mapping_is_stable_private_and_sparse_metadata_stays_absent() {
        let multipart =
            include_str!("../../tests/fixtures/audiobookshelf/2.36.1/book-multipart.json");
        let first = book_album("book-id", serde_json::from_str(multipart).unwrap()).unwrap();
        let second = book_album("book-id", serde_json::from_str(multipart).unwrap()).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.song_count, Some(10));
        assert_eq!(first.duration_seconds, Some(600));
        assert_eq!(first.provider_metadata.credits.len(), 4);
        assert_eq!(
            first
                .provider_metadata
                .identity
                .as_ref()
                .unwrap()
                .library_id,
            "book-id"
        );
        let wire = serde_json::to_value(&first).unwrap();
        assert!(wire.get("providerMetadata").is_none());
        assert!(!wire.to_string().contains("book-id"));

        let sparse = include_str!(
            "../../tests/fixtures/audiobookshelf/2.36.1/book-single-missing-credits.json"
        );
        let sparse = book_album("book-id", serde_json::from_str(sparse).unwrap()).unwrap();
        assert_eq!(sparse.song_count, Some(1));
        assert_eq!(sparse.duration_seconds, Some(60));
        assert!(sparse.cover_art_id.is_none());
        assert!(sparse.provider_metadata.cover_reference.is_none());
        assert!(sparse.provider_metadata.credits.is_empty());
    }

    #[test]
    fn malformed_or_cross_library_identities_are_rejected_safely() {
        for body in [
            r#"{"id":"","libraryId":"book-id","mediaType":"book","media":{"id":"media"}}"#,
            r#"{"id":"item","libraryId":"book-id","mediaType":"book","media":{"id":""}}"#,
            r#"{"id":"item","libraryId":"book-id","mediaType":"book","media":{"id":"media","audioFiles":[{"id":"","index":1}]}}"#,
            r#"{"id":"item","libraryId":"book-id","mediaType":"book","media":{"id":"media","chapters":[{"id":"","start":0,"end":1}]}}"#,
        ] {
            let error = serde_json::from_str::<BookDto>(body).unwrap_err();
            assert!(error.to_string().contains("required identity is empty"));
            assert!(!error.to_string().contains("access-fixture"));
        }

        let foreign =
            r#"{"id":"item","libraryId":"other","mediaType":"book","media":{"id":"media"}}"#;
        let error = book_album("book-id", serde_json::from_str(foreign).unwrap()).unwrap_err();
        assert!(matches!(error, ProviderError::Deserialization(_)));

        let mixed_indices = r#"{"id":"item","libraryId":"book-id","mediaType":"book","media":{"id":"media","audioFiles":[{"id":"b","index":2,"duration":20},{"id":"invalid","index":0,"duration":99},{"id":"a","index":2,"duration":10}]}}"#;
        let album = book_album("book-id", serde_json::from_str(mixed_indices).unwrap()).unwrap();
        assert_eq!(album.song_count, Some(2));
        assert_eq!(album.duration_seconds, Some(30));
        assert_eq!(
            album
                .provider_metadata
                .part_identities
                .iter()
                .map(|part| part.audio_file_id.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b"]
        );
    }

    #[tokio::test]
    async fn list_albums_uses_only_the_persisted_audiobook_library() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        let catalogue = server.mock("GET", "/api/libraries/book-id/items")
            .match_query(Matcher::AllOf(vec![Matcher::UrlEncoded("page".into(), "0".into()), Matcher::UrlEncoded("limit".into(), "2".into())]))
            .match_header("authorization", "Bearer access-fixture")
            .with_status(200).with_header("content-type", "application/json")
            .with_body(r#"{"total":1,"results":[{"id":"item","libraryId":"book-id","mediaType":"book","media":{"id":"media","coverPath":"present","metadata":{"title":"Book"}}}]}"#)
            .expect(1).create_async().await;
        let provider = AudiobookshelfProvider::login(&server.url(), "user", "password")
            .await
            .unwrap()
            .scope_to("book-id".into(), ProviderLibraryRole::Audiobook)
            .unwrap();
        let (albums, total) = provider.list_albums(None, None, 0, 2).await.unwrap();
        assert_eq!(total, 1);
        assert_eq!(albums.len(), 1);
        assert!(
            provider
                .list_albums(Some("other"), None, 0, 2)
                .await
                .is_err()
        );
        catalogue.assert_async().await;
    }

    #[tokio::test]
    async fn list_albums_zero_limit_is_empty_without_a_request() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        let provider = AudiobookshelfProvider::login(&server.url(), "user", "password")
            .await
            .unwrap()
            .scope_to("book-id".into(), ProviderLibraryRole::Audiobook)
            .unwrap();
        assert_eq!(
            provider.list_albums(None, None, 0, 0).await.unwrap(),
            (Vec::new(), 0)
        );
        assert_eq!(
            provider.list_albums(None, Some(""), 0, 0).await.unwrap(),
            (Vec::new(), 0)
        );
        assert!(matches!(
            provider.list_albums(None, Some("A"), 0, 0).await,
            Err(ProviderError::UnsupportedCapability(_))
        ));
        assert!(provider.list_albums(None, None, 1, 2).await.is_err());
    }

    #[tokio::test]
    async fn podcast_scope_rejects_book_catalogue_before_network_io() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        let provider = AudiobookshelfProvider::login(&server.url(), "user", "password")
            .await
            .unwrap()
            .scope_to("podcast-id".into(), ProviderLibraryRole::Podcast)
            .unwrap();
        let error = provider.list_albums(None, None, 0, 10).await.unwrap_err();
        assert!(matches!(error, ProviderError::UnsupportedCapability(_)));
        let error = provider.list_albums(None, None, 0, 0).await.unwrap_err();
        assert!(matches!(error, ProviderError::UnsupportedCapability(_)));
    }

    #[tokio::test]
    async fn catalogue_refreshes_once_after_401() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        let expired = server
            .mock("GET", "/api/libraries/book-id/items")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("page".into(), "0".into()),
                Matcher::UrlEncoded("limit".into(), "1".into()),
            ]))
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
            .with_body(r#"{"user":{"accessToken":"refreshed"}}"#)
            .expect(1)
            .create_async()
            .await;
        let retried = server
            .mock("GET", "/api/libraries/book-id/items")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("page".into(), "0".into()),
                Matcher::UrlEncoded("limit".into(), "1".into()),
            ]))
            .match_header("authorization", "Bearer refreshed")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"total":0,"results":[]}"#)
            .expect(1)
            .create_async()
            .await;
        let provider = AudiobookshelfProvider::login(&server.url(), "user", "password")
            .await
            .unwrap()
            .scope_to("book-id".into(), ProviderLibraryRole::Audiobook)
            .unwrap();
        assert_eq!(
            provider.list_albums(None, None, 0, 1).await.unwrap(),
            (Vec::new(), 0)
        );
        expired.assert_async().await;
        refresh.assert_async().await;
        retried.assert_async().await;
    }

    #[tokio::test]
    async fn empty_final_page_preserves_authoritative_total() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        let page = server
            .mock("GET", "/api/libraries/book-id/items")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("page".into(), "4".into()),
                Matcher::UrlEncoded("limit".into(), "3".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"total":13,"results":[]}"#)
            .expect(1)
            .create_async()
            .await;
        let provider = AudiobookshelfProvider::login(&server.url(), "user", "password")
            .await
            .unwrap()
            .scope_to("book-id".into(), ProviderLibraryRole::Audiobook)
            .unwrap();
        assert_eq!(
            provider.list_albums(None, None, 12, 3).await.unwrap(),
            (Vec::new(), 13)
        );
        page.assert_async().await;
    }

    #[tokio::test]
    async fn search_is_single_bounded_books_request() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        let search = server.mock("GET", "/api/libraries/book-id/search")
            .match_query(Matcher::AllOf(vec![Matcher::UrlEncoded("q".into(), "needle".into()), Matcher::UrlEncoded("limit".into(), "50".into())]))
            .match_header("authorization", "Bearer access-fixture")
            .with_status(200).with_header("content-type", "application/json")
            .with_body(r#"{"book":[{"id":"item","libraryId":"book-id","mediaType":"book","media":{"id":"media","coverPath":"present","metadata":{"title":"Result"}}}],"authors":[{"name":"not-an-album"}]}"#)
            .expect(1).create_async().await;
        let provider = AudiobookshelfProvider::login(&server.url(), "user", "password")
            .await
            .unwrap()
            .scope_to("book-id".into(), ProviderLibraryRole::Audiobook)
            .unwrap();
        let result = provider.search("needle").await.unwrap();
        assert_eq!(result.albums.len(), 1);
        assert!(result.songs.is_empty());
        search.assert_async().await;
    }

    #[tokio::test]
    async fn search_truncation_uses_upstream_category_count_before_filtering() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        let books = (0..50)
            .map(|index| {
                let media_type = if index == 49 { "podcast" } else { "book" };
                format!(
                    r#"{{"id":"item-{index}","libraryId":"book-id","mediaType":"{media_type}","media":{{"id":"media-{index}"}}}}"#
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        server
            .mock("GET", "/api/libraries/book-id/search")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("q".into(), "needle".into()),
                Matcher::UrlEncoded("limit".into(), "50".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(format!(r#"{{"book":[{books}]}}"#))
            .create_async()
            .await;
        let provider = AudiobookshelfProvider::login(&server.url(), "user", "password")
            .await
            .unwrap()
            .scope_to("book-id".into(), ProviderLibraryRole::Audiobook)
            .unwrap();
        let result = provider.search("needle").await.unwrap();
        assert_eq!(result.albums.len(), 49);
        assert!(result.possibly_truncated);
    }

    #[tokio::test]
    async fn chunked_catalogue_body_is_bounded_without_content_length() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        server
            .mock("GET", "/api/libraries/book-id/items")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("page".into(), "0".into()),
                Matcher::UrlEncoded("limit".into(), "1".into()),
            ]))
            .with_status(200)
            .with_chunked_body(|writer| {
                let chunk = vec![b'x'; 1024 * 1024];
                for _ in 0..17 {
                    writer.write_all(&chunk)?;
                }
                Ok(())
            })
            .create_async()
            .await;
        let provider = AudiobookshelfProvider::login(&server.url(), "user", "password")
            .await
            .unwrap()
            .scope_to("book-id".into(), ProviderLibraryRole::Audiobook)
            .unwrap();
        let error = provider.list_albums(None, None, 0, 1).await.unwrap_err();
        assert!(
            matches!(error, ProviderError::Http { message, .. } if message.contains("exceeds configured limit"))
        );
    }

    #[tokio::test]
    async fn get_album_maps_detail_404_to_album_not_found() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        let missing = server
            .mock("GET", "/api/items/item")
            .match_header("authorization", "Bearer access-fixture")
            .with_status(404)
            .expect(1)
            .create_async()
            .await;
        let provider = AudiobookshelfProvider::login(&server.url(), "user", "password")
            .await
            .unwrap()
            .scope_to("book-id".into(), ProviderLibraryRole::Audiobook)
            .unwrap();
        let id = opaque_id("album", &["book-id", "item", "media"]);
        let error = provider.get_album(&id).await.unwrap_err();
        assert!(matches!(error, ProviderError::NotFound { item_type, .. } if item_type == "album"));
        missing.assert_async().await;
    }

    #[tokio::test]
    async fn get_album_sorts_ten_audio_files_numerically() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        let detail = server
            .mock("GET", "/api/items/book-item-ordered")
            .match_header("authorization", "Bearer access-fixture")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(include_str!(
                "../../tests/fixtures/audiobookshelf/2.36.1/book-multipart.json"
            ))
            .expect(1)
            .create_async()
            .await;
        let provider = AudiobookshelfProvider::login(&server.url(), "user", "password")
            .await
            .unwrap()
            .scope_to("book-id".into(), ProviderLibraryRole::Audiobook)
            .unwrap();
        let result = provider
            .get_album(&opaque_id(
                "album",
                &["book-id", "book-item-ordered", "book-media-stable"],
            ))
            .await
            .unwrap();
        assert_eq!(result.tracks.len(), 10);
        assert_eq!(result.album.song_count, Some(10));
        assert_eq!(result.album.duration_seconds, Some(600));
        assert_eq!(result.tracks.first().unwrap().track_number, Some(1));
        assert_eq!(result.tracks.last().unwrap().track_number, Some(10));
        assert_eq!(result.tracks.first().unwrap().title, "Part 1");
        assert_eq!(
            result
                .tracks
                .first()
                .unwrap()
                .provider_metadata
                .audio_file_id
                .as_deref(),
            Some("audio-file-1")
        );
        assert_eq!(result.provider_metadata.chapters.len(), 24);
        assert_eq!(result.provider_metadata.part_identities.len(), 10);
        assert_eq!(result.provider_metadata.credits.len(), 4);
        assert_eq!(result.provider_metadata.credits[0].role, CreditRole::Author);
        assert_eq!(
            result.provider_metadata.credits[2].role,
            CreditRole::Narrator
        );
        detail.assert_async().await;
    }

    #[tokio::test]
    async fn get_album_preserves_sparse_cover_and_rejects_foreign_library_detail() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        let sparse = server
            .mock("GET", "/api/items/book-item-single")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(include_str!(
                "../../tests/fixtures/audiobookshelf/2.36.1/book-single-missing-credits.json"
            ))
            .create_async()
            .await;
        let provider = AudiobookshelfProvider::login(&server.url(), "user", "password")
            .await
            .unwrap()
            .scope_to("book-id".into(), ProviderLibraryRole::Audiobook)
            .unwrap();
        let result = provider
            .get_album(&opaque_id(
                "album",
                &["book-id", "book-item-single", "book-media-single"],
            ))
            .await
            .unwrap();
        assert_eq!(result.album.song_count, Some(1));
        assert!(result.album.cover_art_id.is_none());
        assert!(result.tracks[0].cover_art_id.is_none());
        assert!(result.provider_metadata.credits.is_empty());
        sparse.assert_async().await;

        let foreign = server
            .mock("GET", "/api/items/foreign-item")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"id":"foreign-item","libraryId":"other-library","mediaType":"book","media":{"id":"foreign-media"}}"#)
            .create_async()
            .await;
        let error = provider
            .get_album(&opaque_id(
                "album",
                &["book-id", "foreign-item", "foreign-media"],
            ))
            .await
            .unwrap_err();
        assert!(matches!(error, ProviderError::Deserialization(_)));
        foreign.assert_async().await;
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
        assert_eq!(discovered.provider.server_version(), None);
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
    async fn rate_limit_accepts_http_date_retry_after() {
        let retry_at = chrono::Utc::now() + chrono::Duration::seconds(60);
        let header = retry_at.to_rfc2822();
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/login")
            .with_status(429)
            .with_header("retry-after", &header)
            .create_async()
            .await;
        let error = AudiobookshelfProvider::discover(&server.url(), "user", "password")
            .await
            .unwrap_err();
        match error {
            ProviderError::RateLimited {
                retry_after_seconds: Some(seconds),
            } => assert!((58..=60).contains(&seconds)),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn stored_scope_distinguishes_forbidden_from_missing_library() {
        for (status, expected_forbidden) in [(403, true), (404, false)] {
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
                .with_status(200)
                .with_header("content-type", "application/json")
                .with_body(r#"{"libraries":[]}"#)
                .create_async()
                .await;
            server
                .mock("GET", "/api/libraries/withheld/items")
                .match_query(Matcher::AllOf(vec![
                    Matcher::UrlEncoded("page".into(), "0".into()),
                    Matcher::UrlEncoded("limit".into(), "1".into()),
                ]))
                .with_status(status)
                .create_async()
                .await;

            let error = AudiobookshelfProvider::from_stored_config(
                &server.url(),
                "user",
                "password",
                "withheld",
                ProviderLibraryRole::Podcast,
            )
            .await
            .unwrap_err();
            if expected_forbidden {
                assert!(matches!(error, ProviderError::Forbidden));
            } else {
                assert!(matches!(error, ProviderError::StaleConfiguration(_)));
            }
        }
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
