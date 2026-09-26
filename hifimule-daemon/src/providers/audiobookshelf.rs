//! Audiobookshelf v2.36.1 scoped catalogue and direct playback adapter.
//! Authentication, playback sessions, tokens, and upstream identifiers stay daemon-side.

use super::{
    BookPartTiming, BookProgress, BookTiming, BrowseCapabilities, BrowseMode, Capabilities,
    MediaProvider, PlaybackCleanup, PlaybackDescription, PlaybackProvenance, PlaybackRefresh,
    PlaybackRepresentation, PlaybackRequest, PlaybackSeekMechanism, ProviderChangeContext,
    ProviderError, ProviderLibraryRole, ScrobbleRequest, ServerType, TranscodeProfile,
};
use crate::domain::models::{
    Album, AlbumWithTracks, Artist, ArtistWithAlbums, ChangeEvent, ChapterMarker, Credit,
    CreditRole, Library, Playlist, PlaylistWithTracks, PodcastEntityType, PodcastEpisode,
    PodcastSearchResult, PodcastShow, PodcastShowDetail, ProviderIdentity, ProviderItemMetadata,
    ProviderPartIdentity, SearchResult, Song,
};
use async_trait::async_trait;
use futures::stream::{self, StreamExt};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use reqwest::{Client, StatusCode};
use secrecy::{ExposeSecret, SecretString};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};
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
    session: Arc<Mutex<AuthSession>>,
    library_id: Option<String>,
    library_role: Option<ProviderLibraryRole>,
    server_version: Option<String>,
    author_list_cache: Mutex<Option<(Instant, Vec<Artist>)>>,
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
#[serde(rename_all = "camelCase")]
struct GroupPageDto {
    total: u64,
    #[serde(default)]
    results: Vec<GroupDto>,
}

#[derive(Deserialize)]
struct AuthorsEnvelope {
    authors: Vec<AuthorDto>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuthorDto {
    #[serde(deserialize_with = "deserialize_nonempty")]
    id: String,
    #[serde(deserialize_with = "deserialize_nonempty")]
    name: String,
    #[serde(default)]
    num_books: Option<u32>,
    #[serde(default)]
    library_items: Vec<BookDto>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GroupDto {
    #[serde(deserialize_with = "deserialize_nonempty")]
    id: String,
    #[serde(deserialize_with = "deserialize_nonempty")]
    library_id: String,
    name: String,
    #[serde(default, deserialize_with = "deserialize_group_books")]
    books: Vec<GroupBookDto>,
}

fn deserialize_group_books<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<GroupBookDto>, D::Error> {
    Ok(Vec::<GroupBookDto>::deserialize(deserializer)?
        .into_iter()
        .filter(|book| !book.is_missing)
        .collect())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GroupBookDto {
    #[serde(deserialize_with = "deserialize_nonempty")]
    id: String,
    #[serde(default)]
    library_id: Option<String>,
    #[serde(default)]
    media_type: Option<String>,
    #[serde(default)]
    is_missing: bool,
    #[serde(default)]
    num_audio_files: Option<u32>,
    #[serde(default)]
    media: Option<serde_json::Value>,
}

impl GroupBookDto {
    fn audio_file_count(&self) -> u32 {
        self.num_audio_files
            .or_else(|| {
                self.media
                    .as_ref()
                    .and_then(|media| media.get("numAudioFiles"))
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|count| u32::try_from(count).ok())
            })
            .unwrap_or_else(|| {
                self.media
                    .as_ref()
                    .and_then(|media| media.get("audioFiles"))
                    .and_then(serde_json::Value::as_array)
                    .map(|files| {
                        files
                            .iter()
                            .filter(|file| {
                                file.get("ino")
                                    .and_then(serde_json::Value::as_str)
                                    .is_some()
                                    && file.get("index").and_then(numeric_index).is_some()
                            })
                            .count() as u32
                    })
                    .unwrap_or(0)
            })
    }

    fn full_book(&self) -> Option<BookDto> {
        let media = self.media.clone()?;
        media.get("audioFiles")?.as_array()?;
        serde_json::from_value(serde_json::json!({
            "id": self.id, "libraryId": self.library_id, "mediaType": self.media_type,
            "isMissing": self.is_missing, "media": media
        }))
        .ok()
    }
}

#[derive(Deserialize)]
struct BookSearchDto {
    #[serde(default)]
    book: Vec<BookSearchHit>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum BookSearchHit {
    Wrapped {
        #[serde(rename = "libraryItem")]
        library_item: SearchLibraryItemDto,
    },
    Item(SearchLibraryItemDto),
}

impl BookSearchHit {
    fn into_item(self) -> SearchLibraryItemDto {
        match self {
            Self::Wrapped { library_item } => library_item,
            Self::Item(item) => item,
        }
    }
}

// Search results are expanded library items, but their media metadata varies by
// Audiobookshelf version and library scanner. Parse only the identity and display
// fields needed for a search card; the item detail endpoint remains authoritative.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchLibraryItemDto {
    #[serde(deserialize_with = "deserialize_nonempty")]
    id: String,
    #[serde(deserialize_with = "deserialize_nonempty")]
    library_id: String,
    media_type: String,
    media: SearchMediaDto,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchMediaDto {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    library_item_id: Option<String>,
    #[serde(default)]
    cover_path: Option<String>,
    #[serde(default)]
    num_episodes: serde_json::Value,
    #[serde(default)]
    metadata: serde_json::Value,
}

impl SearchLibraryItemDto {
    fn into_book(self) -> BookDto {
        let media_id = self.media_id();
        let metadata = &self.media.metadata;
        let authors = metadata["authors"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|author| {
                Some(BookAuthorDto {
                    id: author["id"].as_str().map(str::to_owned),
                    name: author["name"].as_str()?.to_owned(),
                })
            })
            .collect();
        let published_year = metadata["publishedYear"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| {
                metadata["publishedYear"]
                    .as_u64()
                    .map(|year| year.to_string())
            });
        BookDto {
            id: self.id,
            library_id: self.library_id,
            media_type: self.media_type,
            is_missing: false,
            media: BookMediaDto {
                id: media_id,
                library_item_id: None,
                cover_path: self.media.cover_path,
                metadata: BookMetadataDto {
                    title: metadata["title"].as_str().map(str::to_owned),
                    authors,
                    narrators: Vec::new(),
                    published_year,
                },
                audio_files: Vec::new(),
                num_audio_files: None,
                chapters: Vec::new(),
            },
        }
    }

    fn into_podcast(self) -> PodcastDto {
        let media_id = self.media_id();
        let metadata = &self.media.metadata;
        PodcastDto {
            id: self.id,
            library_id: self.library_id,
            media_type: self.media_type,
            media: PodcastMediaDto {
                id: media_id,
                cover_path: self.media.cover_path,
                metadata: PodcastMetadataDto {
                    title: metadata["title"].as_str().map(str::to_owned),
                    description: metadata["description"].as_str().map(str::to_owned),
                    image_url: metadata["imageUrl"].as_str().map(str::to_owned),
                },
                episodes: Vec::new(),
                num_episodes: self
                    .media
                    .num_episodes
                    .as_u64()
                    .and_then(|count| u32::try_from(count).ok()),
            },
        }
    }

    fn media_id(&self) -> String {
        self.media
            .id
            .as_deref()
            .or(self.media.library_item_id.as_deref())
            .filter(|id| !id.trim().is_empty())
            .unwrap_or(&self.id)
            .to_owned()
    }
}

#[derive(Deserialize)]
struct PodcastPageDto {
    total: u64,
    #[serde(default)]
    results: Vec<PodcastDto>,
}

struct RecentPodcastPageDto {
    total: u64,
    episodes: Vec<serde_json::Value>,
}

fn recent_podcast_page(value: serde_json::Value) -> Result<RecentPodcastPageDto, ProviderError> {
    if let serde_json::Value::Array(episodes) = &value {
        return Ok(RecentPodcastPageDto {
            total: episodes.len() as u64,
            episodes: episodes.clone(),
        });
    }
    let mut object = value
        .as_object()
        .ok_or_else(|| {
            ProviderError::Deserialization("invalid Audiobookshelf recent episode page".into())
        })?
        .clone();
    let episodes = match object.remove("episodes") {
        Some(serde_json::Value::Array(episodes)) => episodes,
        Some(serde_json::Value::Null) => Vec::new(),
        _ => {
            return Err(ProviderError::Deserialization(
                "invalid Audiobookshelf recent episode list".into(),
            ));
        }
    };
    let total = object
        .remove("total")
        .and_then(|total| match total {
            serde_json::Value::Number(number) => number.as_u64(),
            serde_json::Value::String(text) => text.parse().ok(),
            _ => None,
        })
        .unwrap_or(episodes.len() as u64);
    Ok(RecentPodcastPageDto { total, episodes })
}

struct RecentPodcastEpisodeDto {
    library_item_id: String,
    episode: PodcastEpisodeDto,
}

fn recent_podcast_episode(value: serde_json::Value) -> Option<RecentPodcastEpisodeDto> {
    let field = |key: &str| {
        value
            .get(key)
            .and_then(serde_json::Value::as_str)
            .filter(|text| !text.trim().is_empty())
            .map(str::to_owned)
    };
    Some(RecentPodcastEpisodeDto {
        library_item_id: field("libraryItemId")?,
        episode: PodcastEpisodeDto {
            id: field("id")?,
            title: field("title"),
            description: field("description"),
            duration: value.get("duration").and_then(serde_json::Value::as_f64),
            pub_date: field("pubDate"),
            published_at: value.get("publishedAt").and_then(serde_json::Value::as_i64),
            updated_at: value.get("updatedAt").and_then(serde_json::Value::as_i64),
            audio_file: value
                .get("audioFile")
                .and_then(|file| serde_json::from_value(file.clone()).ok()),
        },
    })
}

#[derive(Deserialize)]
struct PodcastSearchDto {
    #[serde(default)]
    podcast: Vec<PodcastSearchHit>,
    #[serde(default)]
    episodes: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum PodcastSearchHit {
    Wrapped {
        #[serde(rename = "libraryItem")]
        library_item: SearchLibraryItemDto,
    },
    Item(SearchLibraryItemDto),
}

fn podcast_media_identity(value: &mut serde_json::Value) {
    let id = value
        .get("id")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    if let (Some(id), Some(media)) = (
        id,
        value
            .get_mut("media")
            .and_then(serde_json::Value::as_object_mut),
    ) {
        if !media.contains_key("id") {
            let media_id = media
                .get("libraryItemId")
                .and_then(serde_json::Value::as_str)
                .filter(|id| !id.is_empty())
                .unwrap_or(&id)
                .to_owned();
            media.insert("id".into(), serde_json::Value::String(media_id));
        }
    }
}

struct PodcastDto {
    id: String,
    library_id: String,
    media_type: String,
    media: PodcastMediaDto,
}

impl<'de> Deserialize<'de> for PodcastDto {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct RawPodcastDto {
            #[serde(deserialize_with = "deserialize_nonempty")]
            id: String,
            #[serde(deserialize_with = "deserialize_nonempty")]
            library_id: String,
            media_type: String,
            media: PodcastMediaDto,
        }
        let mut value = serde_json::Value::deserialize(deserializer)?;
        podcast_media_identity(&mut value);
        let raw: RawPodcastDto = serde_json::from_value(value).map_err(D::Error::custom)?;
        Ok(Self {
            id: raw.id,
            library_id: raw.library_id,
            media_type: raw.media_type,
            media: raw.media,
        })
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PodcastMediaDto {
    #[serde(deserialize_with = "deserialize_nonempty")]
    id: String,
    #[serde(default)]
    cover_path: Option<String>,
    #[serde(default)]
    metadata: PodcastMetadataDto,
    #[serde(default)]
    episodes: Vec<PodcastEpisodeDto>,
    #[serde(default)]
    num_episodes: Option<u32>,
}

const MAX_PODCAST_BROWSE_EPISODES: usize = 5_000;
const MAX_PODCAST_DETAIL_RESPONSE_BYTES: usize = 128 * 1024 * 1024;

struct PodcastBrowseDto {
    id: String,
    library_id: String,
    media_type: String,
    media: PodcastBrowseMediaDto,
}

impl<'de> Deserialize<'de> for PodcastBrowseDto {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct RawPodcastBrowseDto {
            #[serde(deserialize_with = "deserialize_nonempty")]
            id: String,
            #[serde(deserialize_with = "deserialize_nonempty")]
            library_id: String,
            media_type: String,
            media: PodcastBrowseMediaDto,
        }
        let mut value = serde_json::Value::deserialize(deserializer)?;
        podcast_media_identity(&mut value);
        let raw: RawPodcastBrowseDto = serde_json::from_value(value).map_err(D::Error::custom)?;
        Ok(Self {
            id: raw.id,
            library_id: raw.library_id,
            media_type: raw.media_type,
            media: raw.media,
        })
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PodcastBrowseMediaDto {
    #[serde(deserialize_with = "deserialize_nonempty")]
    id: String,
    #[serde(default)]
    cover_path: Option<String>,
    #[serde(default)]
    metadata: PodcastMetadataDto,
    #[serde(default)]
    episodes: BoundedPodcastEpisodes,
    #[serde(default)]
    num_episodes: Option<u32>,
}

#[derive(Default)]
struct BoundedPodcastEpisodes {
    episodes: Vec<PodcastEpisodeDto>,
    possibly_truncated: bool,
}

impl<'de> Deserialize<'de> for BoundedPodcastEpisodes {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct EpisodeVisitor;

        impl<'de> serde::de::Visitor<'de> for EpisodeVisitor {
            type Value = BoundedPodcastEpisodes;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("an array of podcast episodes")
            }

            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> Result<Self::Value, A::Error> {
                let mut newest = std::collections::BTreeMap::new();
                let mut seen = std::collections::HashSet::new();
                let mut count = 0usize;
                while let Some(episode) = sequence.next_element::<PodcastEpisodeDto>()? {
                    if !seen.insert(episode.id.clone()) {
                        return Err(serde::de::Error::custom(
                            "duplicate podcast episode identity",
                        ));
                    }
                    count = count.saturating_add(1);
                    let published = episode
                        .pub_date
                        .as_deref()
                        .and_then(podcast_pub_date_millis)
                        .or(episode.published_at)
                        .or(episode.updated_at)
                        .unwrap_or(i64::MIN);
                    newest.insert((published, std::cmp::Reverse(episode.id.clone())), episode);
                    if newest.len() > MAX_PODCAST_BROWSE_EPISODES {
                        newest.pop_first();
                    }
                }
                Ok(BoundedPodcastEpisodes {
                    episodes: newest.into_values().rev().collect(),
                    possibly_truncated: count > MAX_PODCAST_BROWSE_EPISODES,
                })
            }
        }

        deserializer.deserialize_seq(EpisodeVisitor)
    }
}

#[derive(Default, Deserialize)]
struct PodcastMetadataDto {
    title: Option<String>,
    description: Option<String>,
    #[serde(rename = "imageUrl")]
    image_url: Option<String>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PodcastEpisodeDto {
    #[serde(deserialize_with = "deserialize_nonempty")]
    id: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    duration: Option<f64>,
    #[serde(default)]
    pub_date: Option<String>,
    #[serde(default)]
    published_at: Option<i64>,
    #[serde(default)]
    updated_at: Option<i64>,
    #[serde(default)]
    audio_file: Option<PodcastAudioFileDto>,
}

fn podcast_pub_date_millis(value: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|date| date.timestamp_millis())
        .or_else(|| {
            Some(
                chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
                    .ok()?
                    .and_hms_opt(0, 0, 0)?
                    .and_utc()
                    .timestamp_millis(),
            )
        })
}

#[derive(Clone, Deserialize)]
struct PodcastAudioFileDto {
    #[serde(deserialize_with = "deserialize_nonempty")]
    ino: String,
    #[serde(default)]
    duration: Option<f64>,
}

#[derive(Debug)]
struct BookDto {
    id: String,
    library_id: String,
    media_type: String,
    is_missing: bool,
    media: BookMediaDto,
}

impl<'de> Deserialize<'de> for BookDto {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct RawBookDto {
            #[serde(deserialize_with = "deserialize_nonempty")]
            id: String,
            #[serde(deserialize_with = "deserialize_nonempty")]
            library_id: String,
            media_type: String,
            #[serde(default)]
            is_missing: bool,
            media: BookMediaDto,
        }
        let raw = RawBookDto::deserialize(deserializer)?;
        let mut media = raw.media;
        if media
            .library_item_id
            .as_deref()
            .is_some_and(|id| !id.is_empty() && id != raw.id)
        {
            return Err(D::Error::custom("book media belongs to another item"));
        }
        if media.id.is_empty() {
            media.id = media
                .library_item_id
                .as_deref()
                .filter(|id| !id.is_empty())
                .unwrap_or(&raw.id)
                .to_owned();
        }
        Ok(Self {
            id: raw.id,
            library_id: raw.library_id,
            media_type: raw.media_type,
            is_missing: raw.is_missing,
            media,
        })
    }
}

/* Book media may omit its own ID; the library item ID is its stable fallback. */
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BookMediaDto {
    #[serde(default, deserialize_with = "deserialize_media_id")]
    id: String,
    #[serde(default)]
    library_item_id: Option<String>,
    #[serde(default)]
    cover_path: Option<String>,
    #[serde(default, deserialize_with = "deserialize_nullable_book_metadata")]
    metadata: BookMetadataDto,
    #[serde(default, deserialize_with = "deserialize_nullable_audio_files")]
    audio_files: Vec<AudioFileDto>,
    #[serde(default)]
    num_audio_files: Option<u32>,
    #[serde(default, deserialize_with = "deserialize_valid_chapters")]
    chapters: Vec<ChapterDto>,
}

fn deserialize_nullable_book_metadata<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<BookMetadataDto, D::Error> {
    Ok(Option::<BookMetadataDto>::deserialize(deserializer)?.unwrap_or_default())
}

fn deserialize_nullable_audio_files<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<AudioFileDto>, D::Error> {
    Ok(Option::<Vec<AudioFileDto>>::deserialize(deserializer)?.unwrap_or_default())
}

fn deserialize_media_id<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<String, D::Error> {
    use serde::de::Error;
    match Option::<String>::deserialize(deserializer)? {
        Some(id) if id.trim().is_empty() => Err(D::Error::custom("required identity is empty")),
        Some(id) => Ok(id),
        None => Ok(String::new()),
    }
}

fn deserialize_valid_chapters<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<ChapterDto>, D::Error> {
    let values = Option::<Vec<serde_json::Value>>::deserialize(deserializer)?;
    Ok(values
        .unwrap_or_default()
        .into_iter()
        .filter_map(|value| serde_json::from_value(value).ok())
        .collect())
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BookMetadataDto {
    #[serde(default)]
    title: Option<String>,
    #[serde(default, deserialize_with = "deserialize_book_authors")]
    authors: Vec<BookAuthorDto>,
    #[serde(default, deserialize_with = "deserialize_string_list")]
    narrators: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_optional_string")]
    published_year: Option<String>,
}

fn deserialize_book_authors<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<BookAuthorDto>, D::Error> {
    let values = Option::<Vec<serde_json::Value>>::deserialize(deserializer)?;
    Ok(values
        .unwrap_or_default()
        .into_iter()
        .filter_map(|value| match value {
            serde_json::Value::String(name) if !name.trim().is_empty() => {
                Some(BookAuthorDto { id: None, name })
            }
            value => serde_json::from_value(value).ok(),
        })
        .collect())
}

fn deserialize_string_list<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<String>, D::Error> {
    let values = Option::<Vec<serde_json::Value>>::deserialize(deserializer)?;
    Ok(values
        .unwrap_or_default()
        .into_iter()
        .filter_map(|value| value.as_str().map(str::to_owned))
        .collect())
}

fn deserialize_optional_string<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value.and_then(|value| match value {
        serde_json::Value::String(text) => Some(text),
        serde_json::Value::Number(number) => Some(number.to_string()),
        _ => None,
    }))
}

#[derive(Debug, Deserialize)]
struct BookAuthorDto {
    id: Option<String>,
    name: String,
}

#[derive(Debug, Deserialize)]
struct AudioFileDto {
    #[serde(deserialize_with = "deserialize_nonempty")]
    ino: String,
    index: serde_json::Value,
    #[serde(default)]
    duration: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct ChapterDto {
    id: serde_json::Value,
    start: f64,
    end: f64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlaySessionDto {
    id: String,
    server_version: String,
    library_id: String,
    library_item_id: String,
    media_type: String,
    #[serde(default)]
    episode_id: Option<String>,
    play_method: u8,
    audio_tracks: Vec<PlayTrackDto>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlayTrackDto {
    ino: Option<String>,
    content_url: String,
    mime_type: String,
    codec: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BookProgressDto {
    library_item_id: String,
    #[serde(default)]
    media_id: Option<String>,
    current_time: f64,
    duration: f64,
    is_finished: bool,
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

    fn podcast_library_id(&self) -> Result<&str, ProviderError> {
        match (self.library_id.as_deref(), self.library_role) {
            (Some(id), Some(ProviderLibraryRole::Podcast)) => Ok(id),
            (_, Some(ProviderLibraryRole::Audiobook)) => Err(unsupported("podcast catalogue")),
            _ => Err(ProviderError::StaleConfiguration(
                "missing Audiobookshelf library scope".into(),
            )),
        }
    }

    async fn podcast_detail(&self, public_id: &str) -> Result<PodcastDto, ProviderError> {
        let library = self.podcast_library_id()?;
        let (encoded_library, item, media) = parse_opaque_id("show", public_id)?;
        if encoded_library != library || item.is_empty() || media.is_empty() {
            return Err(ProviderError::NotFound {
                item_type: "show".into(),
                id: "unavailable".into(),
            });
        }
        let response = self
            .protected_get(&item_endpoint(&self.base_url, &item)?)
            .await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Err(ProviderError::NotFound {
                item_type: "show".into(),
                id: "unavailable".into(),
            });
        }
        check_status(&response)?;
        let show: PodcastDto = bounded_json_with_limit(
            response,
            "podcast detail",
            MAX_PODCAST_DETAIL_RESPONSE_BYTES,
        )
        .await?;
        if show.media_type != "podcast"
            || show.library_id != library
            || show.id != item
            || show.media.id != media
        {
            return Err(ProviderError::StaleConfiguration(
                "Audiobookshelf podcast identity changed".into(),
            ));
        }
        let mut episodes = std::collections::HashSet::new();
        if !show
            .media
            .episodes
            .iter()
            .all(|episode| episodes.insert(&episode.id))
        {
            return Err(ProviderError::Deserialization(
                "duplicate Audiobookshelf episode identity".into(),
            ));
        }
        Ok(show)
    }

    async fn podcast_browse_detail(
        &self,
        public_id: &str,
    ) -> Result<(PodcastDto, bool), ProviderError> {
        let library = self.podcast_library_id()?;
        let (encoded_library, item, media) = parse_opaque_id("show", public_id)?;
        if encoded_library != library {
            return Err(ProviderError::NotFound {
                item_type: "show".into(),
                id: "unavailable".into(),
            });
        }
        let response = self
            .protected_get(&item_endpoint(&self.base_url, &item)?)
            .await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Err(ProviderError::NotFound {
                item_type: "show".into(),
                id: "unavailable".into(),
            });
        }
        check_status(&response)?;
        let browse: PodcastBrowseDto = bounded_json_with_limit(
            response,
            "podcast browse detail",
            MAX_PODCAST_DETAIL_RESPONSE_BYTES,
        )
        .await?;
        if browse.media_type != "podcast"
            || browse.library_id != library
            || browse.id != item
            || browse.media.id != media
        {
            return Err(ProviderError::StaleConfiguration(
                "Audiobookshelf podcast identity changed".into(),
            ));
        }
        let possibly_truncated = browse.media.episodes.possibly_truncated;
        Ok((
            PodcastDto {
                id: browse.id,
                library_id: browse.library_id,
                media_type: browse.media_type,
                media: PodcastMediaDto {
                    id: browse.media.id,
                    cover_path: browse.media.cover_path,
                    metadata: browse.media.metadata,
                    episodes: browse.media.episodes.episodes,
                    num_episodes: browse.media.num_episodes,
                },
            },
            possibly_truncated,
        ))
    }

    async fn podcast_episode_detail(
        &self,
        id: &str,
    ) -> Result<(PodcastDto, PodcastEpisodeDto), ProviderError> {
        let (library, item, media, episode) = parse_episode_id(id)?;
        if library != self.podcast_library_id()? {
            return Err(ProviderError::NotFound {
                item_type: "episode".into(),
                id: "unavailable".into(),
            });
        }
        let show = self
            .podcast_detail(&opaque_id("show", &[&library, &item, &media]))
            .await?;
        let matched = show
            .media
            .episodes
            .iter()
            .find(|candidate| candidate.id == episode)
            .cloned()
            .ok_or_else(|| ProviderError::NotFound {
                item_type: "episode".into(),
                id: "unavailable".into(),
            })?;
        Ok((show, matched))
    }

    async fn resolve_podcast_playback(
        &self,
        id: &str,
    ) -> Result<PlaybackDescription, ProviderError> {
        let (library, item_id, media_id, episode_id) = parse_episode_id(id)?;
        let (item, episode) = self.podcast_episode_detail(id).await?;
        let audio_file = episode.audio_file.as_ref().ok_or_else(|| {
            ProviderError::UnsupportedCapability(
                "Audiobookshelf episode has no direct audio file".into(),
            )
        })?;
        let show = podcast_show(&library, &item)?;
        let episode_public = podcast_episode(&show, &library, &item, &episode);
        let mut endpoint =
            reqwest::Url::parse(&item_endpoint(&self.base_url, &item_id)?).map_err(|_| {
                ProviderError::Deserialization("invalid Audiobookshelf episode URL".into())
            })?;
        endpoint
            .path_segments_mut()
            .map_err(|_| {
                ProviderError::Deserialization("invalid Audiobookshelf episode URL".into())
            })?
            .push("play")
            .push(&episode_id);
        let response = self
            .protected_post(
                endpoint.as_str(),
                &serde_json::json!({"forceDirectPlay": true}),
            )
            .await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Err(ProviderError::NotFound {
                item_type: "episode".into(),
                id: "unavailable".into(),
            });
        }
        check_status(&response)?;
        let (value, cleanup) = self.read_playback_session(response).await?;
        let session: PlaySessionDto = serde_json::from_value(value).map_err(|_| {
            ProviderError::Deserialization("invalid Audiobookshelf episode playback session".into())
        })?;
        if session.server_version != "2.36.1" || session.play_method != 0 {
            return Err(ProviderError::UnsupportedCapability(
                "Audiobookshelf episode playback method is unverified".into(),
            ));
        }
        if session.library_id != library
            || session.library_item_id != item_id
            || session.media_type != "podcast"
            || session.episode_id.as_deref() != Some(&episode_id)
        {
            return Err(ProviderError::StaleConfiguration(
                "Audiobookshelf episode playback identity changed".into(),
            ));
        }
        let verified = self
            .podcast_detail(&opaque_id("show", &[&library, &item_id, &media_id]))
            .await?;
        if !verified.media.episodes.iter().any(|candidate| {
            candidate.id == episode_id
                && candidate
                    .audio_file
                    .as_ref()
                    .is_some_and(|file| file.ino == audio_file.ino)
        }) {
            return Err(ProviderError::StaleConfiguration(
                "Audiobookshelf episode media changed during playback admission".into(),
            ));
        }
        let mut matches = session
            .audio_tracks
            .into_iter()
            .filter(|track| track.ino.as_deref() == Some(&audio_file.ino));
        let track = matches.next().ok_or_else(|| ProviderError::NotFound {
            item_type: "episode".into(),
            id: "unavailable".into(),
        })?;
        if matches.next().is_some() {
            return Err(ProviderError::Deserialization(
                "duplicate Audiobookshelf episode audio".into(),
            ));
        }
        let (codec, container) = match (track.mime_type.as_str(), track.codec.as_deref()) {
            ("audio/mpeg", Some("mp3")) => ("mp3", "mp3"),
            ("audio/mp4", Some("aac")) => ("aac", "m4a"),
            _ => {
                return Err(ProviderError::UnsupportedCapability(
                    "Audiobookshelf episode format is unverified".into(),
                ));
            }
        };
        if !track.content_url.starts_with('/') || track.content_url.starts_with("//") {
            return Err(ProviderError::Deserialization(
                "invalid Audiobookshelf episode media URL".into(),
            ));
        }
        let base = reqwest::Url::parse(&self.base_url)
            .map_err(|_| ProviderError::Deserialization("invalid Audiobookshelf origin".into()))?;
        let url = base.join(&track.content_url).map_err(|_| {
            ProviderError::Deserialization("invalid Audiobookshelf episode media URL".into())
        })?;
        let item_url = item_endpoint(&self.base_url, &item_id)?;
        let mut expected_file = reqwest::Url::parse(&item_url).map_err(|_| {
            ProviderError::Deserialization("invalid Audiobookshelf episode media URL".into())
        })?;
        expected_file
            .path_segments_mut()
            .map_err(|_| {
                ProviderError::Deserialization("invalid Audiobookshelf episode media URL".into())
            })?
            .push("file")
            .push(&audio_file.ino);
        if url.origin() != base.origin()
            || url.query().is_some()
            || url.fragment().is_some()
            || url != expected_file
        {
            return Err(ProviderError::Deserialization(
                "Audiobookshelf episode media URL left selected show".into(),
            ));
        }
        let headers = self.verify_direct_media(&url, &track.mime_type).await?;
        Ok(PlaybackDescription {
            song: podcast_episode_song(&episode_public),
            representations: vec![PlaybackRepresentation {
                codec: Some(codec.into()),
                container: Some(container.into()),
                bitrate_kbps: None,
                sample_rate: None,
                bit_depth: None,
                provenance: PlaybackProvenance::Original,
                seek_mechanism: Some(match codec {
                    "mp3" => PlaybackSeekMechanism::AudiobookshelfDirectMp3,
                    "aac" => PlaybackSeekMechanism::AudiobookshelfDirectM4a,
                    _ => unreachable!("format was validated above"),
                }),
                request: PlaybackRequest {
                    url,
                    headers,
                    range_supported: true,
                    cleanup: Some(cleanup),
                    refresh: Some(self.playback_refresh()),
                    expected_content_type: Some(track.mime_type),
                },
            }],
        })
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
            refresh_auth(&self.client, &self.base_url, &mut session).await?;
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

    async fn protected_post(
        &self,
        endpoint: &str,
        body: &serde_json::Value,
    ) -> Result<reqwest::Response, ProviderError> {
        let mut session = self.session.lock().await;
        let mut response = self
            .client
            .post(endpoint)
            .bearer_auth(session.access_token.expose_secret())
            .json(body)
            .send()
            .await
            .map_err(transport_error)?;
        if response.status() == StatusCode::UNAUTHORIZED {
            refresh_auth(&self.client, &self.base_url, &mut session).await?;
            response = self
                .client
                .post(endpoint)
                .bearer_auth(session.access_token.expose_secret())
                .json(body)
                .send()
                .await
                .map_err(transport_error)?;
        }
        Ok(response)
    }

    async fn protected_patch(
        &self,
        endpoint: &str,
        body: &serde_json::Value,
    ) -> Result<reqwest::Response, ProviderError> {
        let mut session = self.session.lock().await;
        let mut response = self
            .client
            .patch(endpoint)
            .bearer_auth(session.access_token.expose_secret())
            .json(body)
            .send()
            .await
            .map_err(transport_error)?;
        if response.status() == StatusCode::UNAUTHORIZED {
            refresh_auth(&self.client, &self.base_url, &mut session).await?;
            response = self
                .client
                .patch(endpoint)
                .bearer_auth(session.access_token.expose_secret())
                .json(body)
                .send()
                .await
                .map_err(transport_error)?;
        }
        Ok(response)
    }

    fn verify_book_identity(&self, identity: &ProviderIdentity) -> Result<(), ProviderError> {
        if self.audiobook_library_id()? != identity.library_id
            || identity.library_item_id.is_empty()
            || identity.media_id.is_empty()
        {
            return Err(ProviderError::StaleConfiguration(
                "Audiobookshelf book identity changed; re-link the library".into(),
            ));
        }
        Ok(())
    }

    fn progress_endpoint(&self, identity: &ProviderIdentity) -> Result<String, ProviderError> {
        self.verify_book_identity(identity)?;
        let mut url =
            reqwest::Url::parse(&format!("{}/api/me/progress", self.base_url)).map_err(|_| {
                ProviderError::Http {
                    status: None,
                    message: "invalid Audiobookshelf URL".into(),
                }
            })?;
        url.path_segments_mut()
            .map_err(|_| ProviderError::Http {
                status: None,
                message: "invalid Audiobookshelf URL".into(),
            })?
            .push(&identity.library_item_id);
        Ok(url.into())
    }

    async fn read_playback_session(
        &self,
        mut response: reqwest::Response,
    ) -> Result<(serde_json::Value, Arc<PlaybackCleanup>), ProviderError> {
        let mut body = Vec::new();
        let mut cleanup = None;
        while let Some(chunk) = response.chunk().await.map_err(transport_error)? {
            if body.len().saturating_add(chunk.len()) > Self::MAX_RESPONSE_BYTES as usize {
                let remaining = (Self::MAX_RESPONSE_BYTES as usize).saturating_sub(body.len());
                body.extend_from_slice(&chunk[..remaining]);
                if cleanup.is_none() {
                    let _cleanup =
                        playback_session_id_prefix(&body).map(|id| self.playback_cleanup(id));
                }
                return Err(ProviderError::Deserialization(
                    "Audiobookshelf playback response too large".into(),
                ));
            }
            body.extend_from_slice(&chunk);
            if cleanup.is_none() {
                cleanup = playback_session_id_prefix(&body).map(|id| self.playback_cleanup(id));
            }
        }
        let value: serde_json::Value = serde_json::from_slice(&body).map_err(|_| {
            ProviderError::Deserialization("invalid Audiobookshelf playback session".into())
        })?;
        let session_id = value
            .get("id")
            .and_then(|id| id.as_str())
            .filter(|id| !id.is_empty())
            .ok_or_else(|| {
                ProviderError::Deserialization(
                    "Audiobookshelf playback omitted session identity".into(),
                )
            })?;
        if let Some(cleanup) = cleanup {
            if playback_session_id_prefix(&body).as_deref() != Some(session_id) {
                return Err(ProviderError::Deserialization(
                    "invalid Audiobookshelf playback session identity".into(),
                ));
            }
            return Ok((value, cleanup));
        }
        let cleanup = self.playback_cleanup(session_id.to_owned());
        Ok((value, cleanup))
    }

    fn playback_cleanup(&self, session_id: String) -> Arc<PlaybackCleanup> {
        let client = self.client.clone();
        let auth = self.session.clone();
        let base = self.base_url.clone();
        let runtime = tokio::runtime::Handle::current();
        Arc::new(PlaybackCleanup::new(move || {
            super::register_playback_cleanup(runtime.spawn(async move {
                let Ok(mut url) = reqwest::Url::parse(&format!("{base}/api/session")) else {
                    return false;
                };
                let Ok(mut segments) = url.path_segments_mut() else {
                    return false;
                };
                segments.push(&session_id).push("close");
                drop(segments);
                let mut session = auth.lock().await;
                let mut result = client
                    .post(url.clone())
                    .bearer_auth(session.access_token.expose_secret())
                    .send()
                    .await;
                if result
                    .as_ref()
                    .is_ok_and(|response| response.status() == StatusCode::UNAUTHORIZED)
                {
                    if refresh_auth(&client, &base, &mut session).await.is_err() {
                        return false;
                    }
                    result = client
                        .post(url)
                        .bearer_auth(session.access_token.expose_secret())
                        .send()
                        .await;
                }
                result.is_ok_and(|response| response.status().is_success())
            }));
        }))
    }

    fn playback_refresh(&self) -> Arc<PlaybackRefresh> {
        let client = self.client.clone();
        let auth = self.session.clone();
        let base = self.base_url.clone();
        Arc::new(move || {
            let client = client.clone();
            let auth = auth.clone();
            let base = base.clone();
            Box::pin(async move {
                let mut session = auth.lock().await;
                refresh_auth(&client, &base, &mut session).await.ok()?;
                let token = HeaderValue::from_str(&format!(
                    "Bearer {}",
                    session.access_token.expose_secret()
                ))
                .ok()?;
                let mut headers = HeaderMap::new();
                headers.insert(AUTHORIZATION, token);
                Some(headers)
            })
        })
    }

    async fn verify_direct_media(
        &self,
        url: &reqwest::Url,
        mime: &str,
    ) -> Result<HeaderMap, ProviderError> {
        let mut session = self.session.lock().await;
        let mut response = self
            .client
            .get(url.clone())
            .bearer_auth(session.access_token.expose_secret())
            .header(reqwest::header::RANGE, "bytes=0-0")
            .send()
            .await
            .map_err(transport_error)?;
        if response.status() == StatusCode::UNAUTHORIZED {
            refresh_auth(&self.client, &self.base_url, &mut session).await?;
            response = self
                .client
                .get(url.clone())
                .bearer_auth(session.access_token.expose_secret())
                .header(reqwest::header::RANGE, "bytes=0-0")
                .send()
                .await
                .map_err(transport_error)?;
        }
        if response.status() == StatusCode::NOT_FOUND {
            return Err(ProviderError::NotFound {
                item_type: "part".into(),
                id: "unavailable".into(),
            });
        }
        check_status(&response)?;
        let actual_mime = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .map(str::trim);
        let valid_range = response
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("bytes 0-0/"))
            .and_then(|value| value.parse::<u64>().ok())
            .is_some_and(|length| length > 0);
        if response.status() != StatusCode::PARTIAL_CONTENT
            || actual_mime != Some(mime)
            || !valid_range
            || response
                .headers()
                .get(reqwest::header::ACCEPT_RANGES)
                .and_then(|value| value.to_str().ok())
                != Some("bytes")
        {
            return Err(ProviderError::UnsupportedCapability(
                "Audiobookshelf direct response is incompatible".into(),
            ));
        }
        let token =
            HeaderValue::from_str(&format!("Bearer {}", session.access_token.expose_secret()))
                .map_err(|_| ProviderError::Auth("Audiobookshelf token is invalid".into()))?;
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, token);
        Ok(headers)
    }

    async fn grouping_page(&self, kind: &str, page: u32) -> Result<GroupPageDto, ProviderError> {
        let library = self.audiobook_library_id()?;
        let endpoint = format!(
            "{}/api/libraries/{library}/{kind}?page={page}&limit=100",
            self.base_url
        );
        let response = self.protected_get(&endpoint).await?;
        check_status(&response)?;
        bounded_json(response, "grouping page").await
    }

    async fn all_groupings(&self, kind: &str) -> Result<Vec<GroupDto>, ProviderError> {
        let library = self.audiobook_library_id()?;
        let mut groups = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut fetched = 0_u64;
        for page in 0..100 {
            let response = self.grouping_page(kind, page).await?;
            let total = response.total;
            let count = response.results.len();
            fetched += count as u64;
            for group in response.results {
                validate_group(library, &group)?;
                if seen.insert(group.id.clone()) {
                    groups.push(group);
                }
            }
            if count == 0 || fetched >= total {
                return Ok(groups);
            }
        }
        Err(ProviderError::UnsupportedCapability(
            "Audiobookshelf grouping exceeds bounded browse limit".into(),
        ))
    }

    async fn find_series(&self, id: &str) -> Result<Option<GroupDto>, ProviderError> {
        let library = self.audiobook_library_id()?;
        let mut fetched = 0_u64;
        for page in 0..100 {
            let response = self.grouping_page("series", page).await?;
            let total = response.total;
            let count = response.results.len();
            fetched += count as u64;
            for group in response.results {
                validate_group(library, &group)?;
                if group.id == id {
                    return Ok(Some(group));
                }
            }
            if count == 0 || fetched >= total {
                return Ok(None);
            }
        }
        Err(ProviderError::UnsupportedCapability(
            "Audiobookshelf grouping exceeds bounded browse limit".into(),
        ))
    }

    async fn collection_detail(&self, id: &str) -> Result<GroupDto, ProviderError> {
        let mut url = reqwest::Url::parse(&format!("{}/api/collections", self.base_url))
            .map_err(|_| ProviderError::Deserialization("invalid collection origin".into()))?;
        url.path_segments_mut()
            .map_err(|_| ProviderError::Deserialization("invalid collection path".into()))?
            .push(id);
        let response = self.protected_get(url.as_str()).await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Err(ProviderError::NotFound {
                item_type: "playlist".into(),
                id: "unavailable".into(),
            });
        }
        check_status(&response)?;
        bounded_json(response, "collection detail").await
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
        let page: CataloguePageDto = bounded_json(response, "catalogue").await?;
        let total = u32::try_from(page.total).map_err(|_| {
            ProviderError::UnsupportedCapability("Audiobookshelf catalogue is too large".into())
        })?;
        let albums = page
            .results
            .into_iter()
            .filter(|book| book.media_type == "book")
            .map(|book| book_album(library, book))
            .collect::<Result<Vec<_>, _>>()?;
        Ok((albums, total))
    }

    async fn library_authors(&self) -> Result<Vec<Artist>, ProviderError> {
        let library = self.audiobook_library_id()?;
        let endpoint = format!("{}/api/libraries/{library}/authors", self.base_url);
        let response = self.protected_get(&endpoint).await?;
        check_status(&response)?;
        let envelope: AuthorsEnvelope = bounded_json(response, "authors").await?;
        let mut seen = std::collections::HashSet::new();
        let mut artists = Vec::with_capacity(envelope.authors.len());
        for author in envelope.authors {
            if !seen.insert(author.id.clone()) {
                return Err(ProviderError::Deserialization(
                    "Audiobookshelf returned duplicate authors".into(),
                ));
            }
            artists.push(Artist {
                id: opaque_id("author", &[library, &author.id, "id"]),
                name: author.name,
                album_count: author.num_books,
                song_count: None,
                cover_art_id: None,
            });
        }
        artists.sort_by(|left, right| {
            left.name
                .to_lowercase()
                .cmp(&right.name.to_lowercase())
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(artists)
    }

    async fn author_detail(&self, source_id: &str) -> Result<ArtistWithAlbums, ProviderError> {
        let library = self.audiobook_library_id()?;
        let mut url = reqwest::Url::parse(&format!("{}/api/authors", self.base_url))
            .map_err(|_| ProviderError::Deserialization("invalid author origin".into()))?;
        url.path_segments_mut()
            .map_err(|_| ProviderError::Deserialization("invalid author path".into()))?
            .push(source_id);
        url.query_pairs_mut().append_pair("include", "items");
        let response = self.protected_get(url.as_str()).await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Err(ProviderError::NotFound {
                item_type: "author".into(),
                id: "unavailable".into(),
            });
        }
        check_status(&response)?;
        let author: AuthorDto = bounded_json(response, "author detail").await?;
        if author.id != source_id {
            return Err(ProviderError::Deserialization(
                "Audiobookshelf author detail identity mismatch".into(),
            ));
        }
        let mut seen = std::collections::HashSet::new();
        let mut albums = Vec::new();
        for book in author.library_items {
            if book.library_id != library || book.media_type != "book" {
                continue;
            }
            if book.is_missing || !seen.insert(book.id.clone()) {
                continue;
            }
            let mut album = book_album(library, book)?;
            if album.artist_name.is_none() {
                album.artist_name = Some(author.name.clone());
            }
            albums.push(album);
        }
        if albums.is_empty() {
            return Err(ProviderError::NotFound {
                item_type: "author".into(),
                id: "unavailable".into(),
            });
        }
        albums.sort_by(|left, right| {
            left.title
                .cmp(&right.title)
                .then_with(|| left.id.cmp(&right.id))
        });
        let song_count = albums
            .iter()
            .try_fold(0_u32, |total, album| total.checked_add(album.song_count?));
        Ok(ArtistWithAlbums {
            artist: Artist {
                id: opaque_id("author", &[library, source_id, "id"]),
                name: author.name,
                album_count: Some(u32::try_from(albums.len()).map_err(|_| {
                    ProviderError::UnsupportedCapability(
                        "Audiobookshelf author has too many books".into(),
                    )
                })?),
                song_count,
                cover_art_id: albums.first().and_then(|album| album.cover_art_id.clone()),
            },
            albums,
        })
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
        let book: BookDto = bounded_json(response, "book detail").await?;
        if book.media_type != "book"
            || book.library_id != library
            || book.id != item_id
            || (book.media.id != media_id && media_id != item_id)
        {
            return Err(ProviderError::Deserialization(
                "invalid Audiobookshelf book identity".into(),
            ));
        }
        map_book_detail(library, public_id, book)
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
        let results: BookSearchDto = bounded_json(response, "book search").await?;
        let possibly_truncated = results.book.len() >= SEARCH_LIMIT as usize;
        let albums = results
            .book
            .into_iter()
            .map(BookSearchHit::into_item)
            .filter(|book| book.media_type == "book")
            .map(|book| book_album(library, book.into_book()))
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
        self.author_list_cache = Mutex::new(None);
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
            session: Arc::new(Mutex::new(AuthSession {
                access_token: SecretString::new(login.user.access_token),
                refresh_token: login
                    .user
                    .refresh_token
                    .filter(|token| !token.trim().is_empty())
                    .map(SecretString::new),
            })),
            library_id: None,
            library_role: None,
            // The validated v2.36.1 login contract does not expose a version.
            server_version: None,
            author_list_cache: Mutex::new(None),
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
        message: "Audiobookshelf transport failed".into(),
    }
}

fn deserialization_error(_error: reqwest::Error) -> ProviderError {
    ProviderError::Deserialization("invalid Audiobookshelf response".into())
}

async fn refresh_auth(
    client: &Client,
    base_url: &str,
    session: &mut AuthSession,
) -> Result<(), ProviderError> {
    let refresh = session
        .refresh_token
        .as_ref()
        .ok_or_else(|| ProviderError::Auth("Audiobookshelf authentication expired".into()))?;
    let response = client
        .post(format!("{base_url}/auth/refresh"))
        .header("x-refresh-token", refresh.expose_secret())
        .send()
        .await
        .map_err(transport_error)?;
    check_auth_status(&response)?;
    let tokens: RefreshResponse = response.json().await.map_err(deserialization_error)?;
    if tokens.user.access_token.trim().is_empty() {
        return Err(ProviderError::Deserialization(
            "Audiobookshelf refresh omitted access token".into(),
        ));
    }
    session.access_token = SecretString::new(tokens.user.access_token);
    if let Some(refresh) = tokens
        .user
        .refresh_token
        .filter(|value| !value.trim().is_empty())
    {
        session.refresh_token = Some(SecretString::new(refresh));
    }
    Ok(())
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

// The pinned playback response starts with its session ID. Capture that field
// before reading the remaining body so even a rejected oversized response can
// retire a session that the server has already created.
fn playback_session_id_prefix(body: &[u8]) -> Option<String> {
    fn quoted(bytes: &[u8], offset: &mut usize) -> Option<String> {
        let start = *offset;
        if bytes.get(*offset) != Some(&b'"') {
            return None;
        }
        *offset += 1;
        let mut escaped = false;
        while let Some(&byte) = bytes.get(*offset) {
            *offset += 1;
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                return serde_json::from_slice(&bytes[start..*offset]).ok();
            }
        }
        None
    }
    fn whitespace(bytes: &[u8], offset: &mut usize) {
        while bytes.get(*offset).is_some_and(u8::is_ascii_whitespace) {
            *offset += 1;
        }
    }
    let mut offset = 0;
    whitespace(body, &mut offset);
    if body.get(offset) != Some(&b'{') {
        return None;
    }
    offset += 1;
    whitespace(body, &mut offset);
    if quoted(body, &mut offset)?.as_str() != "id" {
        return None;
    }
    whitespace(body, &mut offset);
    if body.get(offset) != Some(&b':') {
        return None;
    }
    offset += 1;
    whitespace(body, &mut offset);
    quoted(body, &mut offset).filter(|id| !id.is_empty())
}

async fn bounded_json<T: DeserializeOwned>(
    response: reqwest::Response,
    context: &'static str,
) -> Result<T, ProviderError> {
    bounded_json_with_limit(
        response,
        context,
        AudiobookshelfProvider::MAX_RESPONSE_BYTES as usize,
    )
    .await
}

async fn bounded_json_with_limit<T: DeserializeOwned>(
    mut response: reqwest::Response,
    context: &'static str,
    max_response_bytes: usize,
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
        if next_len > max_response_bytes {
            return Err(ProviderError::Http {
                status: Some(response.status().as_u16()),
                message: "Audiobookshelf response exceeds configured limit".into(),
            });
        }
        body.extend_from_slice(&chunk);
    }
    let mut deserializer = serde_json::Deserializer::from_slice(&body);
    serde_path_to_error::deserialize(&mut deserializer).map_err(|error| {
        // A field path gives actionable diagnostics without retaining a response
        // body, provider identity, endpoint, or parser value in logs or RPC.
        let path = error.path().to_string();
        let safe_path: String = path
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '[' | ']' | '_'))
            .take(120)
            .collect();
        let safe_path = if safe_path.is_empty() {
            "root"
        } else {
            &safe_path
        };
        eprintln!(
            "Audiobookshelf {context} response rejected at {safe_path} ({:?}, line {}, column {})",
            error.inner().classify(),
            error.inner().line(),
            error.inner().column()
        );
        ProviderError::Deserialization(format!(
            "invalid Audiobookshelf {context} response at {safe_path}"
        ))
    })
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
            if !encoded.is_ascii() || encoded.len() % 2 != 0 {
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

fn parse_track_id(id: &str) -> Result<(String, String, String, String), ProviderError> {
    parse_four_part_id("track", id)
}

fn parse_episode_id(id: &str) -> Result<(String, String, String, String), ProviderError> {
    parse_four_part_id("episode", id)
}

fn parse_four_part_id(
    kind: &str,
    id: &str,
) -> Result<(String, String, String, String), ProviderError> {
    let prefix = format!("abs-{kind}-");
    let encoded = id
        .strip_prefix(&prefix)
        .ok_or_else(|| ProviderError::NotFound {
            item_type: "part".into(),
            id: "unavailable".into(),
        })?;
    let parts = encoded
        .split('.')
        .map(|part| {
            if part.is_empty() || !part.is_ascii() || part.len() % 2 != 0 {
                return Err(());
            }
            (0..part.len())
                .step_by(2)
                .map(|index| u8::from_str_radix(&part[index..index + 2], 16).map_err(|_| ()))
                .collect::<Result<Vec<_>, _>>()
                .and_then(|bytes| String::from_utf8(bytes).map_err(|_| ()))
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ProviderError::NotFound {
            item_type: "part".into(),
            id: "unavailable".into(),
        })?;
    match parts.as_slice() {
        [library, item, media, file]
            if !library.is_empty() && !item.is_empty() && !media.is_empty() && !file.is_empty() =>
        {
            Ok((library.clone(), item.clone(), media.clone(), file.clone()))
        }
        _ => Err(ProviderError::NotFound {
            item_type: "part".into(),
            id: "unavailable".into(),
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

fn seconds_to_millis(value: f64) -> Option<u64> {
    if !value.is_finite() || value < 0.0 || value > (i64::MAX as f64) / 1000.0 {
        return None;
    }
    Some((value * 1000.0).round() as u64)
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

fn podcast_show(library: &str, item: &PodcastDto) -> Result<PodcastShow, ProviderError> {
    if item.media_type != "podcast" || item.library_id != library {
        return Err(ProviderError::StaleConfiguration(
            "Audiobookshelf podcast library role changed".into(),
        ));
    }
    Ok(PodcastShow {
        item_type: PodcastEntityType::Show,
        id: opaque_id("show", &[library, &item.id, &item.media.id]),
        title: item
            .media
            .metadata
            .title
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "Untitled show".into()),
        description: item.media.metadata.description.clone(),
        cover_art_id: item
            .media
            .cover_path
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| {
                item.media
                    .metadata
                    .image_url
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
            })
            .map(|_| opaque_id("podcast-cover", &[library, &item.id, &item.media.id])),
        episode_count: item.media.num_episodes.or_else(|| {
            (!item.media.episodes.is_empty()).then_some(item.media.episodes.len() as u32)
        }),
    })
}

fn podcast_episode(
    show: &PodcastShow,
    library: &str,
    item: &PodcastDto,
    episode: &PodcastEpisodeDto,
) -> PodcastEpisode {
    PodcastEpisode {
        item_type: PodcastEntityType::Episode,
        id: opaque_id("episode", &[library, &item.id, &item.media.id, &episode.id]),
        show_id: show.id.clone(),
        show_title: Some(show.title.clone()),
        title: episode
            .title
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "Untitled episode".into()),
        description: episode.description.clone(),
        duration_seconds: episode
            .duration
            .or_else(|| episode.audio_file.as_ref().and_then(|file| file.duration))
            .and_then(|value| {
                if value.is_finite() && value >= 0.0 && value <= u32::MAX as f64 {
                    Some(value.round() as u32)
                } else {
                    None
                }
            }),
        published_at: episode
            .pub_date
            .as_deref()
            .filter(|date| podcast_pub_date_millis(date).is_some())
            .map(str::to_owned)
            .or_else(|| {
                episode
                    .published_at
                    .and_then(chrono::DateTime::<chrono::Utc>::from_timestamp_millis)
                    .or_else(|| {
                        episode
                            .updated_at
                            .and_then(chrono::DateTime::<chrono::Utc>::from_timestamp_millis)
                    })
                    .map(|date| date.to_rfc3339())
            }),
        cover_art_id: show.cover_art_id.clone(),
    }
}

fn podcast_episode_song(episode: &PodcastEpisode) -> Song {
    Song {
        id: episode.id.clone(),
        title: episode.title.clone(),
        artist_id: None,
        artist_name: None,
        album_id: None,
        album_title: None,
        duration_seconds: episode.duration_seconds.unwrap_or(0),
        bitrate_kbps: None,
        track_number: None,
        disc_number: None,
        cover_art_id: episode.cover_art_id.clone(),
        date_added: episode.published_at.clone(),
        last_played_at: None,
        play_count: None,
        is_favorite: None,
        content_type: None,
        suffix: None,
        size_bytes: None,
        album_loudness: Default::default(),
        provider_metadata: Default::default(),
    }
}

fn audiobookshelf_browse_capabilities(role: Option<ProviderLibraryRole>) -> BrowseCapabilities {
    match role {
        Some(ProviderLibraryRole::Audiobook) => BrowseCapabilities {
            list_modes: vec![
                BrowseMode::Albums,
                BrowseMode::Authors,
                BrowseMode::Series,
                BrowseMode::Collections,
            ],
        },
        Some(ProviderLibraryRole::Podcast) => BrowseCapabilities {
            list_modes: vec![BrowseMode::Podcasts, BrowseMode::RecentEpisodes],
        },
        _ => BrowseCapabilities::default(),
    }
}

fn chapter_markers(chapters: &[ChapterDto]) -> Vec<ChapterMarker> {
    chapters
        .iter()
        .filter_map(|chapter| {
            let id = match &chapter.id {
                serde_json::Value::Number(number) if number.as_u64().is_some() => {
                    number.to_string()
                }
                serde_json::Value::String(id) if !id.trim().is_empty() => id.clone(),
                _ => return None,
            };
            let start_seconds = duration_seconds(Some(chapter.start));
            let end_seconds = duration_seconds(Some(chapter.end));
            (chapter.start.is_finite()
                && chapter.end.is_finite()
                && chapter.start >= 0.0
                && chapter.end >= chapter.start)
                .then_some(ChapterMarker {
                    id,
                    start_seconds,
                    end_seconds,
                })
        })
        .collect()
}

fn map_book_detail(
    library: &str,
    public_id: &str,
    book: BookDto,
) -> Result<AlbumWithTracks, ProviderError> {
    let album = book_album(
        library,
        BookDto {
            id: book.id.clone(),
            library_id: book.library_id.clone(),
            media_type: book.media_type.clone(),
            is_missing: book.is_missing,
            media: BookMediaDto {
                id: book.media.id.clone(),
                library_item_id: None,
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
                    published_year: book.media.metadata.published_year.clone(),
                },
                audio_files: book
                    .media
                    .audio_files
                    .iter()
                    .map(|file| AudioFileDto {
                        ino: file.ino.clone(),
                        index: file.index.clone(),
                        duration: file.duration,
                    })
                    .collect(),
                num_audio_files: book.media.num_audio_files,
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
            .then_with(|| left.ino.cmp(&right.ino))
    });
    let mut part_identities = Vec::new();
    let tracks = files
        .into_iter()
        .filter_map(|file| {
            let index = numeric_index(&file.index)?;
            let id = opaque_id("track", &[library, &book.id, &book.media.id, &file.ino]);
            part_identities.push(ProviderPartIdentity {
                public_id: id.clone(),
                audio_file_id: file.ino.clone(),
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
                    audio_file_id: Some(file.ino.clone()),
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

fn group_playlist(library: &str, kind: &str, group: &GroupDto) -> Playlist {
    let mut seen = std::collections::HashSet::new();
    Playlist {
        id: opaque_id(kind, &[library, &group.id, kind]),
        name: group.name.clone(),
        song_count: Some(
            group
                .books
                .iter()
                .filter(|book| !book.is_missing && seen.insert(&book.id))
                .map(GroupBookDto::audio_file_count)
                .sum(),
        ),
        duration_seconds: None,
        cover_art_id: None,
    }
}

fn validate_group(library: &str, group: &GroupDto) -> Result<(), ProviderError> {
    if group.library_id != library {
        return Err(ProviderError::Deserialization(
            "Audiobookshelf grouping left selected library".into(),
        ));
    }
    if group.books.iter().any(|book| {
        book.library_id.as_deref().is_some_and(|id| id != library)
            || book
                .media_type
                .as_deref()
                .is_some_and(|kind| kind != "book")
    }) {
        return Err(ProviderError::Deserialization(
            "Audiobookshelf grouping has foreign member".into(),
        ));
    }
    Ok(())
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
    let song_count = if book.media.audio_files.is_empty() {
        book.media.num_audio_files.or(Some(0))
    } else {
        u32::try_from(valid_files.len()).ok()
    };
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
                            &[library_id, &book.id, &book.media.id, &file.ino],
                        ),
                        audio_file_id: file.ino.clone(),
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
        year: book
            .media
            .metadata
            .published_year
            .as_deref()
            .and_then(|year| year.parse().ok()),
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
    async fn list_recent_podcast_episodes(
        &self,
        offset: u32,
        limit: u32,
    ) -> Result<(Vec<PodcastEpisode>, u32, u32), ProviderError> {
        let library = self.podcast_library_id()?;
        if limit == 0 || limit > 100 || !offset.is_multiple_of(limit) {
            return Err(ProviderError::UnsupportedCapability(
                "invalid podcast page".into(),
            ));
        }
        let endpoint = format!(
            "{}/api/libraries/{library}/recent-episodes?page={}&limit={limit}",
            self.base_url,
            offset / limit
        );
        let response = self.protected_get(&endpoint).await?;
        check_status(&response)?;
        let payload: serde_json::Value = bounded_json(response, "recent podcast episodes").await?;
        let page = recent_podcast_page(payload)?;
        if page.episodes.len() > limit as usize {
            return Err(ProviderError::Deserialization(
                "oversized recent podcast page".into(),
            ));
        }
        let source_count = page.episodes.len() as u32;
        let recent = page
            .episodes
            .into_iter()
            .filter_map(recent_podcast_episode)
            .collect::<Vec<_>>();
        if source_count > 0 && recent.is_empty() {
            return Err(ProviderError::Deserialization(
                "Audiobookshelf recent episodes lack stable identities".into(),
            ));
        }
        let item_ids = recent
            .iter()
            .map(|recent| recent.library_item_id.clone())
            .collect::<std::collections::HashSet<_>>();
        let resolved = stream::iter(item_ids.into_iter().map(|item_id| async move {
            let response = self
                .protected_get(&item_endpoint(&self.base_url, &item_id)?)
                .await?;
            if response.status() == StatusCode::NOT_FOUND {
                return Ok::<_, ProviderError>(None);
            }
            check_status(&response)?;
            let browse: PodcastBrowseDto = bounded_json_with_limit(
                response,
                "recent podcast show",
                MAX_PODCAST_DETAIL_RESPONSE_BYTES,
            )
            .await?;
            if browse.id != item_id
                || browse.library_id != library
                || browse.media_type != "podcast"
            {
                return Err(ProviderError::StaleConfiguration(
                    "Audiobookshelf recent episode left selected library".into(),
                ));
            }
            let item = PodcastDto {
                id: browse.id,
                library_id: browse.library_id,
                media_type: browse.media_type,
                media: PodcastMediaDto {
                    id: browse.media.id,
                    cover_path: browse.media.cover_path,
                    metadata: browse.media.metadata,
                    episodes: Vec::new(),
                    num_episodes: browse.media.num_episodes,
                },
            };
            Ok::<_, ProviderError>(Some((item_id, item)))
        }))
        .buffer_unordered(5)
        .collect::<Vec<_>>()
        .await;
        let mut shows = std::collections::HashMap::new();
        for result in resolved {
            if let Some((id, item)) = result? {
                shows.insert(id, item);
            }
        }
        let mut episodes = Vec::with_capacity(recent.len());
        for entry in recent {
            let Some(item) = shows.get(&entry.library_item_id) else {
                continue;
            };
            let show = podcast_show(library, item)?;
            episodes.push(podcast_episode(&show, library, item, &entry.episode));
        }
        Ok((
            episodes,
            u32::try_from(page.total).unwrap_or(u32::MAX),
            source_count,
        ))
    }

    async fn list_podcast_shows(
        &self,
        offset: u32,
        limit: u32,
    ) -> Result<(Vec<PodcastShow>, u32), ProviderError> {
        let library = self.podcast_library_id()?;
        if limit == 0 || limit > 100 || !offset.is_multiple_of(limit) {
            return Err(ProviderError::UnsupportedCapability(
                "invalid podcast page".into(),
            ));
        }
        let endpoint = format!(
            "{}/api/libraries/{library}/items?page={}&limit={limit}",
            self.base_url,
            offset / limit
        );
        let response = self.protected_get(&endpoint).await?;
        check_status(&response)?;
        let page: PodcastPageDto = bounded_json(response, "podcast page").await?;
        let shows = page
            .results
            .iter()
            .map(|item| podcast_show(library, item))
            .collect::<Result<Vec<_>, _>>()?;
        Ok((shows, u32::try_from(page.total).unwrap_or(u32::MAX)))
    }

    async fn get_podcast_show(&self, id: &str) -> Result<PodcastShowDetail, ProviderError> {
        let (item, possibly_truncated) = self.podcast_browse_detail(id).await?;
        let library = self.podcast_library_id()?;
        let show = podcast_show(library, &item)?;
        let episodes = item
            .media
            .episodes
            .iter()
            .map(|episode| podcast_episode(&show, library, &item, episode))
            .collect::<Vec<_>>();
        Ok(PodcastShowDetail {
            show,
            episodes,
            possibly_truncated,
        })
    }

    async fn get_podcast_episode(&self, id: &str) -> Result<PodcastEpisode, ProviderError> {
        let (item, episode) = self.podcast_episode_detail(id).await?;
        let show = podcast_show(self.podcast_library_id()?, &item)?;
        Ok(podcast_episode(
            &show,
            self.podcast_library_id()?,
            &item,
            &episode,
        ))
    }

    async fn search_podcasts(&self, query: &str) -> Result<PodcastSearchResult, ProviderError> {
        let library = self.podcast_library_id()?;
        if query.trim().is_empty() {
            return Ok(PodcastSearchResult::default());
        }
        let mut url =
            reqwest::Url::parse(&format!("{}/api/libraries/{library}/search", self.base_url))
                .map_err(|_| {
                    ProviderError::Deserialization("invalid Audiobookshelf search URL".into())
                })?;
        const LIMIT: usize = 50;
        url.query_pairs_mut()
            .append_pair("q", query)
            .append_pair("limit", "50");
        let response = self.protected_get(url.as_str()).await?;
        check_status(&response)?;
        let hits: PodcastSearchDto = bounded_json(response, "podcast search").await?;
        let possibly_truncated = hits.podcast.len() >= LIMIT || hits.episodes.len() >= LIMIT;
        let shows = hits
            .podcast
            .into_iter()
            .take(LIMIT)
            .map(|hit| match hit {
                PodcastSearchHit::Wrapped { library_item } => {
                    podcast_show(library, &library_item.into_podcast())
                }
                PodcastSearchHit::Item(item) => podcast_show(library, &item.into_podcast()),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let episodes = hits
            .episodes
            .into_iter()
            .take(LIMIT)
            .filter_map(|hit| {
                let item_value = hit.get("libraryItem").or_else(|| hit.get("podcast"))?;
                let item =
                    serde_json::from_value::<SearchLibraryItemDto>(item_value.clone()).ok()?;
                let episode = hit
                    .get("episode")
                    .or_else(|| item_value.get("recentEpisode"))
                    .or_else(|| hit.get("recentEpisode"))
                    .unwrap_or(&hit);
                let id = episode["id"].as_str().filter(|id| !id.trim().is_empty())?;
                let episode = PodcastEpisodeDto {
                    id: id.to_owned(),
                    title: episode["title"].as_str().map(str::to_owned),
                    description: episode["description"].as_str().map(str::to_owned),
                    duration: episode["duration"].as_f64(),
                    pub_date: episode["pubDate"].as_str().map(str::to_owned),
                    published_at: episode["publishedAt"].as_i64(),
                    updated_at: episode["updatedAt"].as_i64(),
                    audio_file: None,
                };
                Some((item.into_podcast(), episode))
            })
            .map(|(item, episode)| {
                let show = podcast_show(library, &item)?;
                Ok::<PodcastEpisode, ProviderError>(podcast_episode(
                    &show, library, &item, &episode,
                ))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(PodcastSearchResult {
            shows,
            episodes,
            possibly_truncated,
        })
    }
    async fn book_timing_for_track(
        &self,
        track_id: &str,
    ) -> Result<Option<BookTiming>, ProviderError> {
        let (library, item, media, file) = parse_track_id(track_id)?;
        if library != self.audiobook_library_id()? {
            return Err(ProviderError::StaleConfiguration(
                "Audiobookshelf book library changed; re-link the library".into(),
            ));
        }
        let album = opaque_id("album", &[&library, &item, &media]);
        let timing = self.book_timing(&album).await?;
        if timing.as_ref().is_some_and(|timing| {
            !timing
                .parts
                .iter()
                .any(|part| part.audio_file_id == file && part.track_id == track_id)
        }) {
            return Err(ProviderError::StaleConfiguration(
                "Audiobookshelf book file changed; refresh the library".into(),
            ));
        }
        Ok(timing)
    }
    async fn book_timing(&self, album_id: &str) -> Result<Option<BookTiming>, ProviderError> {
        let library = self.audiobook_library_id()?;
        let (encoded_library, item_id, media_id) = parse_opaque_id("album", album_id)?;
        if encoded_library != library {
            return Err(ProviderError::StaleConfiguration(
                "Audiobookshelf book library changed; re-link the library".into(),
            ));
        }
        let endpoint = item_endpoint(&self.base_url, &item_id)?;
        let response = self.protected_get(&endpoint).await?;
        check_status(&response)?;
        let book: BookDto = bounded_json(response, "book detail").await?;
        if book.id != item_id
            || book.library_id != library
            || book.media.id != media_id
            || book.media_type != "book"
        {
            return Err(ProviderError::StaleConfiguration(
                "Audiobookshelf book identity changed; refresh the library".into(),
            ));
        }
        let mut files = book.media.audio_files;
        files.sort_by(|a, b| {
            numeric_index(&a.index)
                .unwrap_or(u32::MAX)
                .cmp(&numeric_index(&b.index).unwrap_or(u32::MAX))
                .then_with(|| a.ino.cmp(&b.ino))
        });
        let mut seen_index = std::collections::HashSet::new();
        let mut seen_file = std::collections::HashSet::new();
        let mut parts = Vec::with_capacity(files.len());
        for file in files {
            let Some(index) = numeric_index(&file.index) else {
                return Ok(None);
            };
            let Some(duration_ms) = file.duration.and_then(seconds_to_millis).filter(|v| *v > 0)
            else {
                return Ok(None);
            };
            if !seen_index.insert(index) || !seen_file.insert(file.ino.clone()) {
                return Ok(None);
            }
            parts.push(BookPartTiming {
                track_id: opaque_id("track", &[library, &item_id, &media_id, &file.ino]),
                audio_file_id: file.ino,
                duration_ms,
            });
        }
        if parts.is_empty() {
            return Ok(None);
        }
        Ok(Some(BookTiming {
            identity: ProviderIdentity {
                library_id: library.into(),
                library_item_id: item_id,
                media_id,
            },
            parts,
        }))
    }

    async fn read_book_progress(
        &self,
        identity: &ProviderIdentity,
    ) -> Result<Option<BookProgress>, ProviderError> {
        let endpoint = self.progress_endpoint(identity)?;
        let response = self.protected_get(&endpoint).await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        check_status(&response)?;
        let progress: BookProgressDto = bounded_json(response, "book progress").await?;
        if progress.library_item_id != identity.library_item_id
            || progress
                .media_id
                .as_deref()
                .is_some_and(|media| media != identity.media_id)
        {
            return Err(ProviderError::StaleConfiguration(
                "Audiobookshelf book progress identity changed; refresh the library".into(),
            ));
        }
        let current_ms = seconds_to_millis(progress.current_time).ok_or_else(|| {
            ProviderError::Deserialization("invalid Audiobookshelf book progress time".into())
        })?;
        let duration_ms = seconds_to_millis(progress.duration)
            .filter(|duration| *duration > 0 && current_ms <= *duration)
            .ok_or_else(|| {
                ProviderError::Deserialization(
                    "invalid Audiobookshelf book progress duration".into(),
                )
            })?;
        Ok(Some(BookProgress {
            current_ms,
            duration_ms,
            is_finished: progress.is_finished,
        }))
    }

    async fn write_book_progress(
        &self,
        expected: &BookTiming,
        progress: BookProgress,
    ) -> Result<(), ProviderError> {
        if progress.duration_ms == 0 || progress.current_ms > progress.duration_ms {
            return Err(ProviderError::Deserialization(
                "invalid Audiobookshelf book progress position".into(),
            ));
        }
        let identity = &expected.identity;
        let album_id = opaque_id(
            "album",
            &[
                &identity.library_id,
                &identity.library_item_id,
                &identity.media_id,
            ],
        );
        let timing = self.book_timing(&album_id).await?.ok_or_else(|| {
            ProviderError::StaleConfiguration(
                "Audiobookshelf book timing changed; refresh the library".into(),
            )
        })?;
        let total = timing
            .parts
            .iter()
            .try_fold(0u64, |sum, part| sum.checked_add(part.duration_ms))
            .ok_or_else(|| {
                ProviderError::StaleConfiguration(
                    "Audiobookshelf book timing changed; refresh the library".into(),
                )
            })?;
        if timing != *expected || total != progress.duration_ms {
            return Err(ProviderError::StaleConfiguration(
                "Audiobookshelf book identity changed; refresh the library".into(),
            ));
        }
        let endpoint = self.progress_endpoint(identity)?;
        let response = self
            .protected_patch(
                &endpoint,
                &serde_json::json!({
                    "currentTime": progress.current_ms as f64 / 1000.0,
                    "duration": progress.duration_ms as f64 / 1000.0,
                    "isFinished": progress.is_finished,
                }),
            )
            .await?;
        check_status(&response)
    }
    async fn list_libraries(&self) -> Result<Vec<Library>, ProviderError> {
        Err(unsupported("list_libraries"))
    }
    async fn list_artists(
        &self,
        library_id: Option<&str>,
        letter: Option<&str>,
        offset: u32,
        limit: u32,
    ) -> Result<(Vec<Artist>, u32), ProviderError> {
        let library = self.audiobook_library_id()?;
        if library_id.is_some_and(|id| id != library) {
            return Err(unsupported("list_artists library filter"));
        }
        let cached = self
            .author_list_cache
            .lock()
            .await
            .as_ref()
            .and_then(|(loaded, artists)| {
                (loaded.elapsed() < Duration::from_secs(30)).then(|| artists.clone())
            });
        let all_artists = if let Some(artists) = cached {
            artists
        } else {
            let artists = self.library_authors().await?;
            *self.author_list_cache.lock().await = Some((Instant::now(), artists.clone()));
            artists
        };
        let artists = all_artists
            .into_iter()
            .filter(|artist| {
                letter.is_none_or(|letter| {
                    artist
                        .name
                        .to_lowercase()
                        .starts_with(&letter.to_lowercase())
                })
            })
            .collect::<Vec<_>>();
        let total = u32::try_from(artists.len()).map_err(|_| {
            ProviderError::UnsupportedCapability("Audiobookshelf has too many authors".into())
        })?;
        Ok((
            artists
                .into_iter()
                .skip(offset as usize)
                .take(limit as usize)
                .collect(),
            total,
        ))
    }
    async fn get_artist(&self, artist_id: &str) -> Result<ArtistWithAlbums, ProviderError> {
        let library = self.audiobook_library_id()?;
        let (encoded_library, source, kind) = parse_opaque_id("author", artist_id)?;
        if encoded_library != library || !matches!(kind.as_str(), "id" | "name") {
            return Err(ProviderError::NotFound {
                item_type: "author".into(),
                id: "unavailable".into(),
            });
        }
        if kind == "id" {
            return self.author_detail(&source).await;
        }
        // Resolve an older name-based basket selection only when the current
        // library has exactly one author with that name.
        let mut matches = self
            .library_authors()
            .await?
            .into_iter()
            .filter(|artist| artist.name.trim().to_lowercase() == source);
        let artist = matches
            .next()
            .filter(|_| matches.next().is_none())
            .ok_or_else(|| ProviderError::NotFound {
                item_type: "author".into(),
                id: "unavailable".into(),
            })?;
        let (_, resolved_id, _) = parse_opaque_id("author", &artist.id)?;
        self.author_detail(&resolved_id).await
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
    async fn get_song(&self, song_id: &str) -> Result<Song, ProviderError> {
        if self.library_role == Some(ProviderLibraryRole::Podcast) {
            return Err(unsupported("podcast episode as song"));
        }
        let library = self.audiobook_library_id()?;
        let (encoded_library, item, media, _) = parse_track_id(song_id)?;
        if encoded_library != library {
            return Err(ProviderError::NotFound {
                item_type: "part".into(),
                id: "unavailable".into(),
            });
        }
        let album_id = opaque_id("album", &[library, &item, &media]);
        self.catalogue_book(&album_id)
            .await?
            .tracks
            .into_iter()
            .find(|track| track.id == song_id)
            .ok_or_else(|| ProviderError::NotFound {
                item_type: "part".into(),
                id: "unavailable".into(),
            })
    }
    async fn get_playback_display_song(&self, id: &str) -> Result<Song, ProviderError> {
        if self.library_role == Some(ProviderLibraryRole::Podcast) {
            let episode = self.get_podcast_episode(id).await?;
            return Ok(podcast_episode_song(&episode));
        }
        self.get_song(id).await
    }
    async fn list_playlists(&self) -> Result<Vec<Playlist>, ProviderError> {
        let mut playlists = Vec::new();
        playlists.extend(self.list_series().await?);
        playlists.extend(self.list_collections().await?);
        Ok(playlists)
    }
    async fn list_series(&self) -> Result<Vec<Playlist>, ProviderError> {
        let library = self.audiobook_library_id()?;
        Ok(self
            .all_groupings("series")
            .await?
            .iter()
            .map(|group| group_playlist(library, "series", group))
            .collect())
    }
    async fn list_collections(&self) -> Result<Vec<Playlist>, ProviderError> {
        let library = self.audiobook_library_id()?;
        Ok(self
            .all_groupings("collections")
            .await?
            .iter()
            .map(|group| group_playlist(library, "collection", group))
            .collect())
    }
    async fn get_playlist(&self, playlist_id: &str) -> Result<PlaylistWithTracks, ProviderError> {
        let library = self.audiobook_library_id()?;
        let group = if playlist_id.starts_with("abs-series-") {
            let (encoded_library, id, encoded_kind) = parse_opaque_id("series", playlist_id)?;
            if encoded_library != library || encoded_kind != "series" {
                return Err(ProviderError::NotFound {
                    item_type: "playlist".into(),
                    id: "unavailable".into(),
                });
            }
            self.find_series(&id).await?
        } else if playlist_id.starts_with("abs-collection-") {
            let (encoded_library, id, encoded_kind) = parse_opaque_id("collection", playlist_id)?;
            if encoded_library != library || encoded_kind != "collection" {
                return Err(ProviderError::NotFound {
                    item_type: "playlist".into(),
                    id: "unavailable".into(),
                });
            }
            let group = self.collection_detail(&id).await?;
            if group.id != id {
                return Err(ProviderError::Deserialization(
                    "Audiobookshelf collection detail identity mismatch".into(),
                ));
            }
            Some(group)
        } else {
            None
        }
        .ok_or_else(|| ProviderError::NotFound {
            item_type: "playlist".into(),
            id: "unavailable".into(),
        })?;
        if group.library_id != library {
            return Err(ProviderError::Deserialization(
                "Audiobookshelf grouping left selected library".into(),
            ));
        }
        let kind = if playlist_id.starts_with("abs-series-") {
            "series"
        } else {
            "collection"
        };
        let playlist = group_playlist(library, kind, &group);
        let mut seen = std::collections::HashSet::new();
        let mut tracks = Vec::new();
        for book in group.books {
            if book.library_id.as_deref().is_some_and(|id| id != library)
                || book
                    .media_type
                    .as_deref()
                    .is_some_and(|kind| kind != "book")
            {
                return Err(ProviderError::Deserialization(
                    "Audiobookshelf grouping has foreign member".into(),
                ));
            }
            if book.is_missing || !seen.insert(book.id.clone()) {
                continue;
            }
            let detail: BookDto = if let Some(full) = book.full_book() {
                full
            } else {
                let endpoint = item_endpoint(&self.base_url, &book.id)?;
                let response = self.protected_get(&endpoint).await?;
                check_status(&response)?;
                bounded_json(response, "book detail").await?
            };
            if detail.id != book.id || detail.library_id != library || detail.media_type != "book" {
                return Err(ProviderError::Deserialization(
                    "Audiobookshelf grouping has foreign member".into(),
                ));
            }
            let album_id = opaque_id("album", &[library, &detail.id, &detail.media.id]);
            tracks.extend(map_book_detail(library, &album_id, detail)?.tracks);
        }
        Ok(PlaylistWithTracks { playlist, tracks })
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

    async fn resolve_sync_media(
        &self,
        id: &str,
    ) -> Result<crate::providers::SyncMediaRepresentation, ProviderError> {
        // A direct session supplies authenticated media and cleanup, without
        // reading or writing the player's progress record.
        let playback = self.resolve_playback(id).await?;
        crate::providers::SyncMediaRepresentation::from_playback_representations(
            playback.representations,
        )
    }
    async fn resolve_playback(&self, song_id: &str) -> Result<PlaybackDescription, ProviderError> {
        if self.library_role == Some(ProviderLibraryRole::Podcast) {
            return self.resolve_podcast_playback(song_id).await;
        }
        let library = self.audiobook_library_id()?;
        let (encoded_library, item, _media, file) = parse_track_id(song_id)?;
        if encoded_library != library {
            return Err(ProviderError::NotFound {
                item_type: "part".into(),
                id: "unavailable".into(),
            });
        }
        let song = self.get_song(song_id).await?;
        if song.provider_metadata.audio_file_id.as_deref() != Some(file.as_str()) {
            return Err(ProviderError::NotFound {
                item_type: "part".into(),
                id: "unavailable".into(),
            });
        }
        let endpoint = format!("{}/play", item_endpoint(&self.base_url, &item)?);
        let response = self
            .protected_post(&endpoint, &serde_json::json!({"forceDirectPlay": true}))
            .await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Err(ProviderError::NotFound {
                item_type: "book".into(),
                id: "unavailable".into(),
            });
        }
        check_status(&response)?;
        let (value, cleanup) = self.read_playback_session(response).await?;
        let session_id = value
            .get("id")
            .and_then(|id| id.as_str())
            .filter(|id| !id.is_empty())
            .ok_or_else(|| {
                ProviderError::Deserialization(
                    "Audiobookshelf playback omitted session identity".into(),
                )
            })?
            .to_owned();
        let playback: PlaySessionDto = serde_json::from_value(value).map_err(|_| {
            ProviderError::Deserialization("invalid Audiobookshelf playback session".into())
        })?;
        if playback.server_version != "2.36.1" {
            return Err(ProviderError::UnsupportedCapability(
                "Audiobookshelf playback server version is unverified".into(),
            ));
        }
        if playback.id != session_id
            || playback.library_id != library
            || playback.library_item_id != item
            || playback.media_type != "book"
        {
            return Err(ProviderError::Deserialization(
                "invalid Audiobookshelf playback scope".into(),
            ));
        }
        if playback.play_method != 0 {
            return Err(ProviderError::UnsupportedCapability(
                "Audiobookshelf selected an unsupported playback method".into(),
            ));
        }
        let mut matches = playback
            .audio_tracks
            .into_iter()
            .filter(|track| track.ino.as_deref() == Some(file.as_str()));
        let track = matches.next().ok_or_else(|| ProviderError::NotFound {
            item_type: "part".into(),
            id: "unavailable".into(),
        })?;
        if matches.next().is_some() {
            return Err(ProviderError::Deserialization(
                "duplicate Audiobookshelf part in playback session".into(),
            ));
        }
        let (codec, container) = match (track.mime_type.as_str(), track.codec.as_deref()) {
            ("audio/mpeg", Some("mp3")) => ("mp3", "mp3"),
            ("audio/mp4", Some("aac")) => ("aac", "m4a"),
            _ => {
                return Err(ProviderError::UnsupportedCapability(
                    "Audiobookshelf audio format is not verified".into(),
                ));
            }
        };
        if !track.content_url.starts_with('/') || track.content_url.starts_with("//") {
            return Err(ProviderError::Deserialization(
                "invalid Audiobookshelf media URL".into(),
            ));
        }
        let base = reqwest::Url::parse(&self.base_url)
            .map_err(|_| ProviderError::Deserialization("invalid Audiobookshelf origin".into()))?;
        let url = base.join(&track.content_url).map_err(|_| {
            ProviderError::Deserialization("invalid Audiobookshelf media URL".into())
        })?;
        let item_url = item_endpoint(&self.base_url, &item)?;
        if url.origin() != base.origin()
            || url.query().is_some()
            || url.fragment().is_some()
            || !url.as_str().starts_with(&format!("{item_url}/"))
        {
            return Err(ProviderError::Deserialization(
                "Audiobookshelf media URL left selected item".into(),
            ));
        }
        let headers = self.verify_direct_media(&url, &track.mime_type).await?;
        Ok(PlaybackDescription {
            song,
            representations: vec![PlaybackRepresentation {
                codec: Some(codec.into()),
                container: Some(container.into()),
                bitrate_kbps: None,
                sample_rate: None,
                bit_depth: None,
                provenance: PlaybackProvenance::Original,
                seek_mechanism: Some(match codec {
                    "mp3" => PlaybackSeekMechanism::AudiobookshelfDirectMp3,
                    "aac" => PlaybackSeekMechanism::AudiobookshelfDirectM4a,
                    _ => unreachable!("format was validated above"),
                }),
                request: PlaybackRequest {
                    url,
                    headers,
                    range_supported: true,
                    cleanup: Some(cleanup),
                    refresh: Some(self.playback_refresh()),
                    expected_content_type: Some(track.mime_type),
                },
            }],
        })
    }
    async fn cover_art_url(&self, _cover_art_id: &str) -> Result<String, ProviderError> {
        Err(unsupported("cover_art_url"))
    }
    async fn fetch_cover_art(
        &self,
        cover_art_id: &str,
    ) -> Result<reqwest::Response, ProviderError> {
        let (library, kind) = match self.library_role {
            Some(ProviderLibraryRole::Audiobook) => (self.audiobook_library_id()?, "cover"),
            Some(ProviderLibraryRole::Podcast) => (self.podcast_library_id()?, "podcast-cover"),
            None => {
                return Err(ProviderError::StaleConfiguration(
                    "missing Audiobookshelf library scope".into(),
                ));
            }
        };
        let (encoded_library, item_id, media_id) = parse_opaque_id(kind, cover_art_id)?;
        if encoded_library != library {
            return Err(ProviderError::NotFound {
                item_type: "cover".into(),
                id: cover_art_id.into(),
            });
        }
        if self.library_role == Some(ProviderLibraryRole::Podcast) {
            let item_url = item_endpoint(&self.base_url, &item_id)?;
            let item_response = self.protected_get(&item_url).await?;
            check_status(&item_response)?;
            let show: SearchLibraryItemDto =
                bounded_json(item_response, "podcast cover item").await?;
            if show.library_id != library
                || show.id != item_id
                || show.media_type != "podcast"
                || show.media_id() != media_id
            {
                return Err(ProviderError::StaleConfiguration(
                    "Audiobookshelf podcast identity changed".into(),
                ));
            }
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

    #[tokio::test]
    async fn book_progress_reads_and_writes_scoped_whole_item_position() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        let read = server.mock("GET", "/api/me/progress/book-item")
            .match_header("authorization", "Bearer access-fixture")
            .with_status(200).with_header("content-type", "application/json")
            .with_body(r#"{"libraryItemId":"book-item","currentTime":33.25,"duration":100,"isFinished":false}"#)
            .expect(1).create_async().await;
        let detail = server.mock("GET", "/api/items/book-item")
            .with_status(200).with_header("content-type", "application/json")
            .with_body(r#"{"id":"book-item","libraryId":"book-id","mediaType":"book","media":{"id":"media","audioFiles":[{"ino":"part","index":1,"duration":100}]}}"#)
            .expect(2).create_async().await;
        let write = server
            .mock("PATCH", "/api/me/progress/book-item")
            .match_header("authorization", "Bearer access-fixture")
            .match_body(Matcher::PartialJson(serde_json::json!({
                "currentTime": 33.25, "duration": 100.0, "isFinished": false
            })))
            .with_status(200)
            .expect(2)
            .create_async()
            .await;
        let provider = AudiobookshelfProvider::login(&server.url(), "user", "password")
            .await
            .unwrap()
            .scope_to("book-id".into(), ProviderLibraryRole::Audiobook)
            .unwrap();
        let identity = ProviderIdentity {
            library_id: "book-id".into(),
            library_item_id: "book-item".into(),
            media_id: "media".into(),
        };
        let progress = provider
            .read_book_progress(&identity)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(progress.current_ms, 33_250);
        assert_eq!(progress.duration_ms, 100_000);
        let timing = BookTiming {
            identity: identity.clone(),
            parts: vec![BookPartTiming {
                track_id: opaque_id("track", &["book-id", "book-item", "media", "part"]),
                audio_file_id: "part".into(),
                duration_ms: 100_000,
            }],
        };
        provider
            .write_book_progress(&timing, progress)
            .await
            .unwrap();
        provider
            .write_book_progress(&timing, progress)
            .await
            .unwrap();
        read.assert_async().await;
        detail.assert_async().await;
        write.assert_async().await;
    }

    #[tokio::test]
    async fn book_progress_absence_denial_and_failures_are_distinct_and_redacted() {
        for status in [404, 403, 429, 500] {
            let mut server = Server::new_async().await;
            server
                .mock("POST", "/login")
                .with_status(200)
                .with_header("content-type", "application/json")
                .with_body(login_body())
                .create_async()
                .await;
            server
                .mock("GET", "/api/me/progress/secret-item")
                .with_status(status)
                .with_body("secret-upstream-body")
                .create_async()
                .await;
            let provider = AudiobookshelfProvider::login(&server.url(), "user", "password")
                .await
                .unwrap()
                .scope_to("library".into(), ProviderLibraryRole::Audiobook)
                .unwrap();
            let identity = ProviderIdentity {
                library_id: "library".into(),
                library_item_id: "secret-item".into(),
                media_id: "media".into(),
            };
            let result = provider.read_book_progress(&identity).await;
            match status {
                404 => assert_eq!(result.as_ref().unwrap(), &None),
                403 => assert!(matches!(&result, Err(ProviderError::Forbidden))),
                429 => assert!(matches!(&result, Err(ProviderError::RateLimited { .. }))),
                _ => assert!(matches!(
                    &result,
                    Err(ProviderError::Http {
                        status: Some(500),
                        ..
                    })
                )),
            }
            assert!(!format!("{result:?}").contains("secret-upstream-body"));
            assert!(!format!("{result:?}").contains("secret-item"));
        }
    }

    #[tokio::test]
    async fn book_progress_refreshes_once_and_rejects_malformed_time() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        server
            .mock("GET", "/api/me/progress/item")
            .match_header("authorization", "Bearer access-fixture")
            .with_status(401)
            .expect(1)
            .create_async()
            .await;
        server
            .mock("POST", "/auth/refresh")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"user":{"accessToken":"access-refreshed"}}"#)
            .expect(1)
            .create_async()
            .await;
        server
            .mock("GET", "/api/me/progress/item")
            .match_header("authorization", "Bearer access-refreshed")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"libraryItemId":"item","currentTime":-3,"duration":100,"isFinished":false}"#,
            )
            .expect(1)
            .create_async()
            .await;
        let provider = AudiobookshelfProvider::login(&server.url(), "user", "password")
            .await
            .unwrap()
            .scope_to("library".into(), ProviderLibraryRole::Audiobook)
            .unwrap();
        let identity = ProviderIdentity {
            library_id: "library".into(),
            library_item_id: "item".into(),
            media_id: "media".into(),
        };
        assert!(matches!(
            provider.read_book_progress(&identity).await,
            Err(ProviderError::Deserialization(_))
        ));
    }

    #[tokio::test]
    async fn changed_book_file_blocks_progress_patch() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        server.mock("GET", "/api/items/item")
            .with_status(200).with_header("content-type", "application/json")
            .with_body(r#"{"id":"item","libraryId":"library","mediaType":"book","media":{"id":"media","audioFiles":[{"ino":"replacement","index":1,"duration":100}]}}"#)
            .create_async().await;
        let patch = server
            .mock("PATCH", "/api/me/progress/item")
            .expect(0)
            .create_async()
            .await;
        let provider = AudiobookshelfProvider::login(&server.url(), "user", "password")
            .await
            .unwrap()
            .scope_to("library".into(), ProviderLibraryRole::Audiobook)
            .unwrap();
        let expected = BookTiming {
            identity: ProviderIdentity {
                library_id: "library".into(),
                library_item_id: "item".into(),
                media_id: "media".into(),
            },
            parts: vec![BookPartTiming {
                track_id: opaque_id("track", &["library", "item", "media", "original"]),
                audio_file_id: "original".into(),
                duration_ms: 100_000,
            }],
        };
        let error = provider
            .write_book_progress(
                &expected,
                BookProgress {
                    current_ms: 50_000,
                    duration_ms: 100_000,
                    is_finished: false,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(error, ProviderError::StaleConfiguration(_)));
        patch.assert_async().await;
    }

    fn login_body() -> &'static str {
        r#"{"user":{"accessToken":"access-fixture","refreshToken":"refresh-fixture"}}"#
    }

    fn playback_book() -> &'static str {
        r#"{"id":"item-1","libraryId":"book-id","mediaType":"book","media":{"id":"media-1","metadata":{"title":"Fixture"},"audioFiles":[{"ino":"ino-1","index":1,"duration":60}]}}"#
    }

    #[test]
    fn malformed_unicode_identity_is_rejected_without_panicking() {
        assert!(parse_track_id("abs-track-aéz.00.00.00").is_err());
        assert!(parse_opaque_id("album", "abs-album-aéz.00.00").is_err());
    }

    #[test]
    fn playback_prefix_only_accepts_the_first_json_id() {
        assert_eq!(
            playback_session_id_prefix(br#" { "id": "session-secret", "tracks": ["#),
            Some("session-secret".into())
        );
        assert_eq!(playback_session_id_prefix(br#"{"other":1,"id":"x"}"#), None);
    }

    #[tokio::test]
    async fn oversized_playback_body_closes_captured_session() {
        let mut server = Server::new_async().await;
        let _login = server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        let body = format!(
            "{{\"id\":\"session-secret\",\"padding\":\"{}\"}}",
            "x".repeat(AudiobookshelfProvider::MAX_RESPONSE_BYTES as usize)
        );
        let _play = server
            .mock("POST", "/api/items/item-1/play")
            .with_status(200)
            .with_body(body)
            .create_async()
            .await;
        let close = server
            .mock("POST", "/api/session/session-secret/close")
            .with_status(200)
            .expect(1)
            .create_async()
            .await;
        let provider = AudiobookshelfProvider::login(&server.url(), "fixture", "fixture")
            .await
            .unwrap();
        let response = provider
            .protected_post(
                &format!("{}/api/items/item-1/play", server.url()),
                &serde_json::json!({}),
            )
            .await
            .unwrap();
        assert!(provider.read_playback_session(response).await.is_err());
        for _ in 0..50 {
            if close.matched_async().await {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        close.assert_async().await;
    }

    #[tokio::test]
    async fn rejected_session_close_is_reported_by_shutdown_drain() {
        let mut server = Server::new_async().await;
        let _login = server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        let close = server
            .mock("POST", "/api/session/session-secret/close")
            .with_status(500)
            .expect(1)
            .create_async()
            .await;
        let provider = AudiobookshelfProvider::login(&server.url(), "fixture", "fixture")
            .await
            .unwrap();
        drop(provider.playback_cleanup("session-secret".into()));
        assert!(!crate::providers::drain_playback_cleanups().await);
        close.assert_async().await;
    }

    #[tokio::test]
    async fn direct_part_uses_scoped_identity_and_closes_session_after_request_retirement() {
        let mut server = Server::new_async().await;
        let _login = server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        let detail = server
            .mock("GET", "/api/items/item-1")
            .match_header("authorization", "Bearer access-fixture")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(playback_book())
            .expect(1)
            .create_async()
            .await;
        let play = server.mock("POST", "/api/items/item-1/play")
            .match_header("authorization", "Bearer access-fixture")
            .match_body(Matcher::PartialJson(serde_json::json!({"forceDirectPlay": true})))
            .with_status(200).with_header("content-type", "application/json")
            .with_body(r#"{"id":"session-secret","serverVersion":"2.36.1","libraryId":"book-id","libraryItemId":"item-1","mediaType":"book","playMethod":0,"audioTracks":[{"ino":"ino-1","contentUrl":"/api/items/item-1/file/ino-1","mimeType":"audio/mpeg","codec":"mp3"}]}"#)
            .expect(1).create_async().await;
        let media = server
            .mock("GET", "/api/items/item-1/file/ino-1")
            .match_header("authorization", "Bearer access-fixture")
            .match_header("range", "bytes=0-0")
            .with_status(206)
            .with_header("content-type", "audio/mpeg")
            .with_header("accept-ranges", "bytes")
            .with_header("content-range", "bytes 0-0/100")
            .with_body("x")
            .expect(1)
            .create_async()
            .await;
        let close = server
            .mock("POST", "/api/session/session-secret/close")
            .match_header("authorization", "Bearer access-fixture")
            .with_status(200)
            .expect(1)
            .create_async()
            .await;
        let provider = AudiobookshelfProvider::login(&server.url(), "fixture", "fixture")
            .await
            .unwrap()
            .scope_to("book-id".into(), ProviderLibraryRole::Audiobook)
            .unwrap();
        let id = opaque_id("track", &["book-id", "item-1", "media-1", "ino-1"]);
        let description = provider.resolve_playback(&id).await.unwrap();
        assert_eq!(description.song.id, id);
        assert_eq!(description.representations.len(), 1);
        assert_eq!(description.representations[0].codec.as_deref(), Some("mp3"));
        assert!(description.representations[0].request.range_supported);
        assert_eq!(
            description.representations[0].seek_mechanism,
            Some(PlaybackSeekMechanism::AudiobookshelfDirectMp3)
        );
        let debug = format!("{:?}", description.representations[0].request);
        assert!(!debug.contains("session-secret"));
        assert!(!debug.contains("access-fixture"));
        drop(description);
        for _ in 0..50 {
            if close.matched_async().await {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        detail.assert_async().await;
        play.assert_async().await;
        media.assert_async().await;
        close.assert_async().await;
    }

    #[tokio::test]
    async fn wrong_library_part_is_rejected_before_any_request() {
        let mut server = Server::new_async().await;
        let _login = server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        let provider = AudiobookshelfProvider::login(&server.url(), "fixture", "fixture")
            .await
            .unwrap()
            .scope_to("book-id".into(), ProviderLibraryRole::Audiobook)
            .unwrap();
        let id = opaque_id("track", &["other-library", "item-1", "media-1", "ino-1"]);
        assert!(matches!(
            provider.resolve_playback(&id).await,
            Err(ProviderError::NotFound { .. })
        ));
    }

    #[tokio::test]
    async fn hls_fallback_is_rejected_and_its_session_is_closed() {
        let mut server = Server::new_async().await;
        let _login = server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        let _detail = server
            .mock("GET", "/api/items/item-1")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(playback_book())
            .create_async()
            .await;
        let _play = server.mock("POST", "/api/items/item-1/play")
            .with_status(200).with_header("content-type", "application/json")
            .with_body(r#"{"id":"session-secret","serverVersion":"2.36.1","libraryId":"book-id","libraryItemId":"item-1","mediaType":"book","playMethod":2,"audioTracks":[{"contentUrl":"/api/session/session-secret/playlist.m3u8","mimeType":"application/vnd.apple.mpegurl"}]}"#)
            .create_async().await;
        let close = server
            .mock("POST", "/api/session/session-secret/close")
            .with_status(200)
            .expect(1)
            .create_async()
            .await;
        let provider = AudiobookshelfProvider::login(&server.url(), "fixture", "fixture")
            .await
            .unwrap()
            .scope_to("book-id".into(), ProviderLibraryRole::Audiobook)
            .unwrap();
        let id = opaque_id("track", &["book-id", "item-1", "media-1", "ino-1"]);
        assert!(matches!(
            provider.resolve_playback(&id).await,
            Err(ProviderError::UnsupportedCapability(_))
        ));
        for _ in 0..50 {
            if close.matched_async().await {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        close.assert_async().await;
    }

    #[tokio::test]
    async fn foreign_media_url_is_rejected_before_any_authenticated_read() {
        let mut server = Server::new_async().await;
        let _login = server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        let _detail = server
            .mock("GET", "/api/items/item-1")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(playback_book())
            .create_async()
            .await;
        let _play = server.mock("POST", "/api/items/item-1/play")
            .with_status(200).with_header("content-type", "application/json")
            .with_body(r#"{"id":"session-secret","serverVersion":"2.36.1","libraryId":"book-id","libraryItemId":"item-1","mediaType":"book","playMethod":0,"audioTracks":[{"ino":"ino-1","contentUrl":"https://foreign.invalid/steal","mimeType":"audio/mpeg","codec":"mp3"}]}"#)
            .create_async().await;
        let close = server
            .mock("POST", "/api/session/session-secret/close")
            .with_status(200)
            .expect(1)
            .create_async()
            .await;
        let provider = AudiobookshelfProvider::login(&server.url(), "fixture", "fixture")
            .await
            .unwrap()
            .scope_to("book-id".into(), ProviderLibraryRole::Audiobook)
            .unwrap();
        let id = opaque_id("track", &["book-id", "item-1", "media-1", "ino-1"]);
        assert!(matches!(
            provider.resolve_playback(&id).await,
            Err(ProviderError::Deserialization(_))
        ));
        for _ in 0..50 {
            if close.matched_async().await {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        close.assert_async().await;
    }

    #[tokio::test]
    async fn direct_media_refreshes_once_after_401_and_uses_new_bearer() {
        let mut server = Server::new_async().await;
        let _login = server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        let old = server
            .mock("GET", "/api/items/item-1/file/ino-1")
            .match_header("authorization", "Bearer access-fixture")
            .match_header("range", "bytes=0-0")
            .with_status(401)
            .expect(1)
            .create_async()
            .await;
        let refresh = server
            .mock("POST", "/auth/refresh")
            .match_header("x-refresh-token", "refresh-fixture")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"user":{"accessToken":"new-access","refreshToken":"new-refresh"}}"#)
            .expect(1)
            .create_async()
            .await;
        let fresh = server
            .mock("GET", "/api/items/item-1/file/ino-1")
            .match_header("authorization", "Bearer new-access")
            .match_header("range", "bytes=0-0")
            .with_status(206)
            .with_header("content-type", "audio/mpeg")
            .with_header("accept-ranges", "bytes")
            .with_header("content-range", "bytes 0-0/100")
            .with_body("x")
            .expect(1)
            .create_async()
            .await;
        let provider = AudiobookshelfProvider::login(&server.url(), "fixture", "fixture")
            .await
            .unwrap()
            .scope_to("book-id".into(), ProviderLibraryRole::Audiobook)
            .unwrap();
        let url =
            reqwest::Url::parse(&format!("{}/api/items/item-1/file/ino-1", server.url())).unwrap();
        let headers = provider
            .verify_direct_media(&url, "audio/mpeg")
            .await
            .unwrap();
        assert_eq!(headers.get(AUTHORIZATION).unwrap(), "Bearer new-access");
        old.assert_async().await;
        refresh.assert_async().await;
        fresh.assert_async().await;
    }

    #[tokio::test]
    async fn session_close_refreshes_expired_access_once() {
        let mut server = Server::new_async().await;
        let _login = server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        let old = server
            .mock("POST", "/api/session/session-secret/close")
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
            .with_body(r#"{"user":{"accessToken":"new-access"}}"#)
            .expect(1)
            .create_async()
            .await;
        let close = server
            .mock("POST", "/api/session/session-secret/close")
            .match_header("authorization", "Bearer new-access")
            .with_status(200)
            .expect(1)
            .create_async()
            .await;
        let provider = AudiobookshelfProvider::login(&server.url(), "fixture", "fixture")
            .await
            .unwrap();
        drop(provider.playback_cleanup("session-secret".into()));
        for _ in 0..50 {
            if close.matched_async().await {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        old.assert_async().await;
        refresh.assert_async().await;
        close.assert_async().await;
    }

    #[tokio::test]
    async fn vanished_media_is_retryable_without_admitting_a_different_part() {
        let mut server = Server::new_async().await;
        let _login = server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        let _detail = server
            .mock("GET", "/api/items/item-1")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(playback_book())
            .create_async()
            .await;
        let _play = server.mock("POST", "/api/items/item-1/play")
            .with_status(200).with_header("content-type", "application/json")
            .with_body(r#"{"id":"session-secret","serverVersion":"2.36.1","libraryId":"book-id","libraryItemId":"item-1","mediaType":"book","playMethod":0,"audioTracks":[{"ino":"ino-1","contentUrl":"/api/items/item-1/file/ino-1","mimeType":"audio/mpeg","codec":"mp3"}]}"#)
            .create_async().await;
        let _missing = server
            .mock("GET", "/api/items/item-1/file/ino-1")
            .match_header("range", "bytes=0-0")
            .with_status(404)
            .expect(1)
            .create_async()
            .await;
        let close = server
            .mock("POST", "/api/session/session-secret/close")
            .with_status(200)
            .expect(1)
            .create_async()
            .await;
        let provider = AudiobookshelfProvider::login(&server.url(), "fixture", "fixture")
            .await
            .unwrap()
            .scope_to("book-id".into(), ProviderLibraryRole::Audiobook)
            .unwrap();
        let id = opaque_id("track", &["book-id", "item-1", "media-1", "ino-1"]);
        assert!(matches!(
            provider.resolve_playback(&id).await,
            Err(ProviderError::NotFound { .. })
        ));
        for _ in 0..50 {
            if close.matched_async().await {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        close.assert_async().await;
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
                id: serde_json::json!(0),
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
                id: "0".into(),
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
    fn audiobook_scope_publishes_books_and_read_only_groupings() {
        assert_eq!(
            audiobookshelf_browse_capabilities(Some(ProviderLibraryRole::Audiobook)).list_modes,
            vec![
                BrowseMode::Albums,
                BrowseMode::Authors,
                BrowseMode::Series,
                BrowseMode::Collections
            ]
        );
        assert_eq!(
            audiobookshelf_browse_capabilities(Some(ProviderLibraryRole::Podcast)).list_modes,
            vec![BrowseMode::Podcasts, BrowseMode::RecentEpisodes]
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

    #[tokio::test]
    async fn podcast_cover_uses_item_identity_without_parsing_episode_catalog() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        let item = r#"{"id":"show-1","libraryId":"pod-id","mediaType":"podcast","media":{"id":"media-1","libraryItemId":"show-1","coverPath":null,"metadata":{"title":"Talks","imageUrl":"https://example.test/art.jpg"},"episodes":[{"id":null}]}}"#;
        server
            .mock("GET", "/api/items/show-1")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(item)
            .expect(1)
            .create_async()
            .await;
        server
            .mock("GET", "/api/items/show-1/cover")
            .with_status(200)
            .with_header("content-type", "image/jpeg")
            .with_body(vec![1_u8, 2, 3])
            .expect(1)
            .create_async()
            .await;
        let provider = AudiobookshelfProvider::login(&server.url(), "user", "password")
            .await
            .unwrap()
            .scope_to("pod-id".into(), ProviderLibraryRole::Podcast)
            .unwrap();
        let show = serde_json::from_str::<SearchLibraryItemDto>(item)
            .unwrap()
            .into_podcast();
        let cover = podcast_show("pod-id", &show).unwrap().cover_art_id.unwrap();
        let response = provider.fetch_cover_art(&cover).await.unwrap();
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
    fn grouping_observation_fixture_preserves_page_and_overlap_evidence() {
        let record: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/audiobookshelf/2.36.1/grouping-observations.json"
        ))
        .unwrap();
        assert_eq!(record["seriesPages"][0]["total"], 4);
        assert_eq!(
            record["collectionPages"][1]["results"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
        assert_eq!(
            record["collectionDetailMemberOrder"],
            record["collectionPages"][0]["results"][0]["books"]
        );
        assert_eq!(record["seriesDetailHasBooks"], false);
    }

    #[test]
    fn grouping_skips_missing_members_without_media_and_counts_playable_parts() {
        let group: GroupDto = serde_json::from_str(
            r#"{"id":"series-1","libraryId":"books","name":"Series","books":[{"id":"gone","libraryId":"books","mediaType":"book","isMissing":true},{"id":"live","libraryId":"books","mediaType":"book","media":{"id":"media-live","audioFiles":[{"ino":"valid","index":1},{"ino":"bad","index":"invalid"}]}}]}"#,
        )
        .unwrap();
        assert_eq!(group.books.len(), 1);
        assert_eq!(
            group_playlist("books", "series", &group).song_count,
            Some(1)
        );
    }

    #[test]
    fn minified_group_members_and_book_media_without_id_are_accepted() {
        let group: GroupDto = serde_json::from_str(
            r#"{"id":"series-1","libraryId":"books","name":"Series","books":[{"id":"book-1","mediaType":"book","media":{"numAudioFiles":2}}]}"#,
        ).unwrap();
        assert_eq!(
            group_playlist("books", "series", &group).song_count,
            Some(2)
        );
        validate_group("books", &group).unwrap();
        let book: BookDto = serde_json::from_str(
            r#"{"id":"book-1","libraryId":"books","mediaType":"book","media":{"metadata":{"title":"Book","authors":["Author"],"publishedYear":2024},"audioFiles":[{"ino":"file-1","index":1,"duration":30}],"chapters":null}}"#,
        ).unwrap();
        assert_eq!(book.media.id, "book-1");
        assert_eq!(book.media.metadata.authors[0].name, "Author");
        assert_eq!(book.media.audio_files.len(), 1);
        let book: BookDto = serde_json::from_str(
            r#"{"id":"book-1","libraryId":"books","mediaType":"book","media":{"id":"media-1","libraryItemId":"book-1","metadata":{"title":"Book"},"audioFiles":[{"ino":"file-1","index":1,"duration":30}]}}"#,
        ).unwrap();
        assert_eq!(book.media.id, "media-1");
        assert_eq!(book_album("books", book).unwrap().song_count, Some(1));
        let book: BookDto = serde_json::from_str(
            r#"{"id":"ebook-1","libraryId":"books","mediaType":"book","media":{"libraryItemId":"ebook-1","metadata":null,"audioFiles":null,"chapters":null}}"#,
        ).unwrap();
        assert!(book.media.audio_files.is_empty());
        assert_eq!(book_album("books", book).unwrap().song_count, Some(0));
    }

    #[tokio::test]
    async fn authors_include_coauthors_once_and_stay_in_the_selected_library() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        let book = |id: &str| {
            serde_json::json!({
                "id": id, "libraryId": "book-id", "mediaType": "book",
                "media": {"metadata": {"title": id, "authorName": "Bob"}, "numAudioFiles": 1}
            })
        };
        let authors_mock = server
            .mock("GET", "/api/libraries/book-id/authors")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"authors":[{"id":"a","name":"Alice","numBooks":1},{"id":"b","name":"Bob","numBooks":2}]}"#)
            .expect(2)
            .create_async()
            .await;
        let detail_mock = server
            .mock("GET", "/api/authors/b")
            .match_query(Matcher::UrlEncoded("include".into(), "items".into()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::json!({"id":"b","name":"Bob","libraryItems":[
                book("first"), book("second"),
                {"id":"foreign","libraryId":"other","mediaType":"book","media":{"metadata":{"title":"Foreign"},"numAudioFiles":1}}
            ]}).to_string())
            .expect(2)
            .create_async()
            .await;
        server
            .mock("GET", "/api/authors/a")
            .match_query(Matcher::UrlEncoded("include".into(), "items".into()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                serde_json::json!({"id":"a","name":"Alice","libraryItems":[book("first")]})
                    .to_string(),
            )
            .expect(1)
            .create_async()
            .await;
        server
            .mock("GET", "/api/items/first")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"id":"first","libraryId":"book-id","mediaType":"book","media":{"id":"media-first","libraryItemId":"first","metadata":{"title":"first","authors":[{"id":"b","name":"Bob"}]},"audioFiles":[{"ino":"file-first","index":1,"duration":10}]}}"#)
            .expect(1)
            .create_async()
            .await;
        let provider = AudiobookshelfProvider::login(&server.url(), "user", "password")
            .await
            .unwrap()
            .scope_to("book-id".into(), ProviderLibraryRole::Audiobook)
            .unwrap();
        let (authors, total) = provider.list_artists(None, None, 0, 50).await.unwrap();
        assert_eq!(total, 2);
        assert_eq!(
            authors
                .iter()
                .map(|author| author.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Alice", "Bob"]
        );
        let bob = authors.iter().find(|author| author.name == "Bob").unwrap();
        assert_eq!(bob.album_count, Some(2));
        let alice = authors
            .iter()
            .find(|author| author.name == "Alice")
            .unwrap();
        assert_eq!(
            provider.get_artist(&alice.id).await.unwrap().albums.len(),
            1
        );
        let detail = provider.get_artist(&bob.id).await.unwrap();
        assert_eq!(detail.artist.song_count, Some(2));
        assert_eq!(
            provider
                .get_album(&detail.albums[0].id)
                .await
                .unwrap()
                .tracks
                .len(),
            1
        );
        assert_eq!(
            detail
                .albums
                .iter()
                .map(|album| album.title.as_str())
                .collect::<Vec<_>>(),
            vec!["first", "second"]
        );
        let old_name_id = opaque_id("author", &["book-id", "bob", "name"]);
        assert_eq!(
            provider
                .get_artist(&old_name_id)
                .await
                .unwrap()
                .albums
                .len(),
            2
        );
        let foreign = opaque_id("author", &["foreign", "b", "id"]);
        assert!(matches!(
            provider.get_artist(&foreign).await,
            Err(ProviderError::NotFound { .. })
        ));
        authors_mock.assert_async().await;
        detail_mock.assert_async().await;
    }

    #[tokio::test]
    async fn author_list_uses_author_endpoint_and_rejects_duplicates() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        server
            .mock("GET", "/api/libraries/book-id/authors")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"authors":[{"id":"a","name":"Author","numBooks":101}]}"#)
            .create_async()
            .await;
        let provider = AudiobookshelfProvider::login(&server.url(), "user", "password")
            .await
            .unwrap()
            .scope_to("book-id".into(), ProviderLibraryRole::Audiobook)
            .unwrap();
        let (authors, total) = provider.list_artists(None, None, 0, 50).await.unwrap();
        assert_eq!(total, 1);
        assert_eq!(authors[0].album_count, Some(101));

        let mut duplicates = Server::new_async().await;
        duplicates
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        duplicates
            .mock("GET", "/api/libraries/book-id/authors")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"authors":[{"id":"a","name":"Author"},{"id":"a","name":"Other"}]}"#)
            .create_async()
            .await;
        let provider = AudiobookshelfProvider::login(&duplicates.url(), "user", "password")
            .await
            .unwrap()
            .scope_to("book-id".into(), ProviderLibraryRole::Audiobook)
            .unwrap();
        assert!(provider.list_artists(None, None, 0, 50).await.is_err());

        let mut empty = Server::new_async().await;
        empty
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        empty
            .mock("GET", "/api/libraries/book-id/authors")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"authors":[]}"#)
            .create_async()
            .await;
        let provider = AudiobookshelfProvider::login(&empty.url(), "user", "password")
            .await
            .unwrap()
            .scope_to("book-id".into(), ProviderLibraryRole::Audiobook)
            .unwrap();
        assert_eq!(
            provider.list_artists(None, None, 0, 50).await.unwrap(),
            (vec![], 0)
        );
    }

    #[test]
    fn expanded_recent_episode_metadata_does_not_reject_the_page() {
        let payload: serde_json::Value = serde_json::from_str(
            r#"{"total":1,"episodes":[{"libraryItemId":"show-1","id":"ep-1","title":"New","duration":12.5,"podcast":{"metadata":{"title":"Show"}},"audioTrack":{"index":1},"audioFile":null}]}"#,
        ).unwrap();
        let page = recent_podcast_page(payload).unwrap();
        let episode = recent_podcast_episode(page.episodes.into_iter().next().unwrap()).unwrap();
        assert_eq!(episode.library_item_id, "show-1");
        assert_eq!(episode.episode.id, "ep-1");
        let page = recent_podcast_page(serde_json::json!({"episodes": [{"id": "ep-2"}]})).unwrap();
        assert_eq!(page.total, 1);
        let page = recent_podcast_page(serde_json::json!({"total": "2", "episodes": []})).unwrap();
        assert_eq!(page.total, 2);
        let page = recent_podcast_page(serde_json::json!([{"id": "ep-3"}])).unwrap();
        assert_eq!(page.total, 1);
        let show: PodcastBrowseDto = serde_json::from_str(
            r#"{"id":"show-1","libraryId":"podcasts","mediaType":"podcast","media":{"metadata":{"title":"Show"},"episodes":[]}}"#,
        ).unwrap();
        assert_eq!(show.media.id, "show-1");
    }

    #[test]
    fn podcast_detail_accepts_media_id_alongside_library_item_id() {
        let detail = serde_json::json!({
            "id": "show-1",
            "libraryId": "pod-id",
            "mediaType": "podcast",
            "media": {
                "id": "media-1",
                "libraryItemId": "show-1",
                "metadata": {"title": "Talks"},
                "episodes": [{"id": "ep-1", "title": "First"}]
            }
        });
        let playback: PodcastDto = serde_json::from_value(detail.clone()).unwrap();
        let browse: PodcastBrowseDto = serde_json::from_value(detail).unwrap();
        assert_eq!(playback.media.id, "media-1");
        assert_eq!(browse.media.id, "media-1");

        let fallback = serde_json::json!({
            "id": "show-1",
            "libraryId": "pod-id",
            "mediaType": "podcast",
            "media": {"libraryItemId": "show-1", "episodes": []}
        });
        let playback: PodcastDto = serde_json::from_value(fallback.clone()).unwrap();
        let browse: PodcastBrowseDto = serde_json::from_value(fallback).unwrap();
        assert_eq!(playback.media.id, "show-1");
        assert_eq!(browse.media.id, "show-1");
    }

    #[tokio::test]
    async fn groupings_are_scoped_read_only_and_keep_member_order() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        let book = |id: &str, ino: &str| {
            format!(
                r#"{{"id":"{id}","libraryId":"book-id","mediaType":"book","media":{{"id":"media-{id}","metadata":{{"title":"Same name"}},"audioFiles":[{{"ino":"{ino}","index":1,"duration":10}}]}}}}"#
            )
        };
        let series = format!(
            r#"{{"total":1,"results":[{{"id":"shared","libraryId":"book-id","name":"Same name","books":[{},{},{},{{"id":"stale","libraryId":"book-id","mediaType":"book","isMissing":true,"media":{{"id":"stale-media"}}}}]}}]}}"#,
            book("first", "f"),
            book("second", "s"),
            book("first", "f")
        );
        let collection = format!(
            r#"{{"total":1,"results":[{{"id":"shared","libraryId":"book-id","name":"Same name","books":[{}]}}]}}"#,
            book("second", "s")
        );
        server
            .mock("GET", "/api/libraries/book-id/series")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("page".into(), "0".into()),
                Matcher::UrlEncoded("limit".into(), "100".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(series)
            .expect(3)
            .create_async()
            .await;
        server
            .mock("GET", "/api/libraries/book-id/collections")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("page".into(), "0".into()),
                Matcher::UrlEncoded("limit".into(), "100".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(collection)
            .expect(2)
            .create_async()
            .await;
        server
            .mock("GET", "/api/collections/shared")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(format!(
                r#"{{"id":"shared","libraryId":"book-id","name":"Same name","books":[{}]}}"#,
                book("second", "s")
            ))
            .expect(1)
            .create_async()
            .await;
        let provider = AudiobookshelfProvider::login(&server.url(), "user", "password")
            .await
            .unwrap()
            .scope_to("book-id".into(), ProviderLibraryRole::Audiobook)
            .unwrap();
        let playlists = provider.list_playlists().await.unwrap();
        assert_eq!(playlists.len(), 2);
        assert_ne!(playlists[0].id, playlists[1].id);
        assert_eq!(playlists[0].song_count, Some(2));
        assert_eq!(
            provider.list_series().await.unwrap(),
            vec![playlists[0].clone()]
        );
        assert_eq!(
            provider.list_collections().await.unwrap(),
            vec![playlists[1].clone()]
        );
        assert!(!provider.capabilities().supports_playlist_write);
        assert!(matches!(
            provider.create_playlist("no", &[]).await,
            Err(ProviderError::UnsupportedCapability(_))
        ));
        assert!(matches!(
            provider.rename_playlist(&playlists[0].id, "no").await,
            Err(ProviderError::UnsupportedCapability(_))
        ));
        let detail = provider.get_playlist(&playlists[0].id).await.unwrap();
        assert_eq!(detail.tracks.len(), 2);
        assert_eq!(
            detail.tracks[0].album_id.as_deref(),
            Some(opaque_id("album", &["book-id", "first", "media-first"]).as_str())
        );
        let collection_detail = provider.get_playlist(&playlists[1].id).await.unwrap();
        assert_eq!(collection_detail.tracks.len(), 1);
        assert_eq!(
            collection_detail.tracks[0].album_id.as_deref(),
            Some(opaque_id("album", &["book-id", "second", "media-second"]).as_str())
        );
        assert!(
            provider
                .get_playlist(&opaque_id("series", &["other", "shared", "series"]))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn minified_series_and_collection_members_load_book_details_with_both_media_ids() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        server
            .mock("GET", "/api/libraries/book-id/series")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("page".into(), "0".into()),
                Matcher::UrlEncoded("limit".into(), "100".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"total":1,"results":[{"id":"series-1","libraryId":"book-id","name":"Series","books":[{"id":"book-1","libraryId":"book-id","mediaType":"book","media":{"numAudioFiles":1}}]}]}"#)
            .create_async()
            .await;
        server
            .mock("GET", "/api/collections/collection-1")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"id":"collection-1","libraryId":"book-id","name":"Collection","books":[{"id":"book-1","libraryId":"book-id","mediaType":"book","media":{"numAudioFiles":1}}]}"#)
            .create_async()
            .await;
        let item = server
            .mock("GET", "/api/items/book-1")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"id":"book-1","libraryId":"book-id","mediaType":"book","media":{"id":"media-1","libraryItemId":"book-1","metadata":{"title":"Book","authors":[{"id":"a","name":"Author"}]},"audioFiles":[{"ino":"file-1","index":1,"duration":30}],"chapters":[]}}"#)
            .expect(2)
            .create_async()
            .await;
        let provider = AudiobookshelfProvider::login(&server.url(), "user", "password")
            .await
            .unwrap()
            .scope_to("book-id".into(), ProviderLibraryRole::Audiobook)
            .unwrap();
        for (kind, id) in [("series", "series-1"), ("collection", "collection-1")] {
            let playlist_id = opaque_id(kind, &["book-id", id, kind]);
            let detail = provider.get_playlist(&playlist_id).await.unwrap();
            assert_eq!(detail.tracks.len(), 1);
            assert_eq!(detail.tracks[0].title, "Book");
        }
        item.assert_async().await;
    }

    #[tokio::test]
    async fn grouping_pages_stop_at_total_and_reject_foreign_members() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        for (page, id) in [(0, "first"), (1, "second")] {
            server.mock("GET", "/api/libraries/book-id/series")
                .match_query(Matcher::AllOf(vec![
                    Matcher::UrlEncoded("page".into(), page.to_string()),
                    Matcher::UrlEncoded("limit".into(), "100".into()),
                ]))
                .with_status(200).with_header("content-type", "application/json")
                .with_body(format!(r#"{{"total":2,"results":[{{"id":"{id}","libraryId":"book-id","name":"Shared","books":[]}}]}}"#))
                .expect(1).create_async().await;
        }
        server
            .mock("GET", "/api/libraries/book-id/collections")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("page".into(), "0".into()),
                Matcher::UrlEncoded("limit".into(), "100".into()),
            ]))
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
        let groups = provider.list_playlists().await.unwrap();
        assert_eq!(groups.len(), 2);
        assert_ne!(groups[0].id, groups[1].id);
    }

    #[tokio::test]
    async fn grouping_foreign_library_is_rejected_without_member_resolution() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        server.mock("GET", "/api/libraries/book-id/series")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("page".into(), "0".into()),
                Matcher::UrlEncoded("limit".into(), "100".into()),
            ]))
            .with_status(200).with_header("content-type", "application/json")
            .with_body(r#"{"total":1,"results":[{"id":"foreign","libraryId":"book-id","name":"Foreign","books":[{"id":"foreign-book","libraryId":"other","mediaType":"book","media":{"id":"foreign-media"}}]}]}"#)
            .create_async().await;
        let provider = AudiobookshelfProvider::login(&server.url(), "user", "password")
            .await
            .unwrap()
            .scope_to("book-id".into(), ProviderLibraryRole::Audiobook)
            .unwrap();
        assert!(matches!(
            provider.list_playlists().await,
            Err(ProviderError::Deserialization(_))
        ));
    }

    #[tokio::test]
    async fn grouping_permission_failure_is_not_hidden_as_empty() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        server
            .mock("GET", "/api/libraries/book-id/series")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("page".into(), "0".into()),
                Matcher::UrlEncoded("limit".into(), "100".into()),
            ]))
            .with_status(403)
            .create_async()
            .await;
        let provider = AudiobookshelfProvider::login(&server.url(), "user", "password")
            .await
            .unwrap()
            .scope_to("book-id".into(), ProviderLibraryRole::Audiobook)
            .unwrap();
        assert!(matches!(
            provider.list_playlists().await,
            Err(ProviderError::Forbidden)
        ));
    }

    #[test]
    fn malformed_or_cross_library_identities_are_rejected_safely() {
        for body in [
            r#"{"id":"","libraryId":"book-id","mediaType":"book","media":{"id":"media"}}"#,
            r#"{"id":"item","libraryId":"book-id","mediaType":"book","media":{"id":""}}"#,
            r#"{"id":"item","libraryId":"book-id","mediaType":"book","media":{"id":"media","audioFiles":[{"ino":"","index":1}]}}"#,
        ] {
            let error = serde_json::from_str::<BookDto>(body).unwrap_err();
            assert!(error.to_string().contains("required identity is empty"));
            assert!(!error.to_string().contains("access-fixture"));
        }
        assert!(serde_json::from_str::<BookDto>(
            r#"{"id":"item","libraryId":"book-id","mediaType":"book","media":{"id":"media","libraryItemId":"foreign"}}"#
        )
        .is_err());

        let foreign =
            r#"{"id":"item","libraryId":"other","mediaType":"book","media":{"id":"media"}}"#;
        let error = book_album("book-id", serde_json::from_str(foreign).unwrap()).unwrap_err();
        assert!(matches!(error, ProviderError::Deserialization(_)));

        let mixed_indices = r#"{"id":"item","libraryId":"book-id","mediaType":"book","media":{"id":"media","audioFiles":[{"ino":"b","index":2,"duration":20},{"ino":"invalid","index":0,"duration":99},{"ino":"a","index":2,"duration":10}]}}"#;
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
            .with_body(r#"{"total":1,"results":[{"id":"item","libraryId":"book-id","mediaType":"book","media":{"id":"media","coverPath":"present","metadata":{"title":"Book","publishedYear":"2008"}}}]}"#)
            .expect(1).create_async().await;
        let provider = AudiobookshelfProvider::login(&server.url(), "user", "password")
            .await
            .unwrap()
            .scope_to("book-id".into(), ProviderLibraryRole::Audiobook)
            .unwrap();
        let (albums, total) = provider.list_albums(None, None, 0, 2).await.unwrap();
        assert_eq!(total, 1);
        assert_eq!(albums.len(), 1);
        assert_eq!(albums[0].year, Some(2008));
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
    async fn search_accepts_wrapped_book_with_library_item_media_id() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        server.mock("GET", "/api/libraries/book-id/search")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("q".into(), "result".into()),
                Matcher::UrlEncoded("limit".into(), "50".into()),
            ]))
            .with_status(200).with_header("content-type", "application/json")
            .with_body(r#"{"book":[{"libraryItem":{"id":"book-1","libraryId":"book-id","mediaType":"book","media":{"libraryItemId":"book-1","metadata":{"title":"Result","publishedYear":2024},"audioFiles":[{"ino":42}],"chapters":[{"id":0}]}},"matchKey":"title"}]}"#)
            .create_async().await;
        let provider = AudiobookshelfProvider::login(&server.url(), "user", "password")
            .await
            .unwrap()
            .scope_to("book-id".into(), ProviderLibraryRole::Audiobook)
            .unwrap();
        let result = provider.search("result").await.unwrap();
        assert_eq!(result.albums.len(), 1);
        assert_eq!(result.albums[0].title, "Result");
        assert_eq!(result.albums[0].year, Some(2024));
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
    async fn podcast_browse_catalog_keeps_newest_episodes_and_reports_truncation() {
        let episodes = (0..=MAX_PODCAST_BROWSE_EPISODES)
            .map(|index| {
                serde_json::json!({
                    "id": format!("episode-{index}"),
                    "title": format!("Episode {index}"),
                    "publishedAt": index as i64,
                })
            })
            .collect::<Vec<_>>();
        let bounded: BoundedPodcastEpisodes =
            serde_json::from_value(serde_json::Value::Array(episodes)).unwrap();
        assert!(bounded.possibly_truncated);
        assert_eq!(bounded.episodes.len(), MAX_PODCAST_BROWSE_EPISODES);
        assert_eq!(bounded.episodes[0].id, "episode-5000");
        assert!(
            !bounded
                .episodes
                .iter()
                .any(|episode| episode.id == "episode-0")
        );
    }

    #[test]
    fn podcast_episode_uses_updated_at_when_publication_date_is_missing() {
        let episode: PodcastEpisodeDto = serde_json::from_value(serde_json::json!({
            "id": "episode-1",
            "title": "First",
            "updatedAt": 1767225600000i64
        }))
        .unwrap();
        assert_eq!(episode.updated_at, Some(1767225600000));
        assert_eq!(podcast_pub_date_millis("2026-01-02"), Some(1767312000000));
        let show = PodcastShow {
            item_type: PodcastEntityType::Show,
            id: "show-1".into(),
            title: "Talks".into(),
            description: None,
            cover_art_id: None,
            episode_count: Some(1),
        };
        let item: PodcastDto = serde_json::from_value(serde_json::json!({
            "id": "show-1",
            "libraryId": "pod-id",
            "mediaType": "podcast",
            "media": { "id": "media-1" }
        }))
        .unwrap();
        assert_eq!(
            podcast_episode(&show, "pod-id", &item, &episode).published_at,
            Some("2026-01-01T00:00:00+00:00".into())
        );
    }

    #[tokio::test]
    async fn podcast_catalogue_keeps_show_and_episode_identity_separate() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        server
            .mock("GET", "/api/libraries/pod-id/items")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("page".into(), "0".into()),
                Matcher::UrlEncoded("limit".into(), "1".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(include_str!(
                "../../tests/fixtures/audiobookshelf/synthetic/podcast-page.json"
            ))
            .create_async()
            .await;
        server
            .mock("GET", "/api/items/show-1")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(include_str!(
                "../../tests/fixtures/audiobookshelf/synthetic/podcast-show.json"
            ))
            .create_async()
            .await;
        let provider = AudiobookshelfProvider::login(&server.url(), "user", "password")
            .await
            .unwrap()
            .scope_to("pod-id".into(), ProviderLibraryRole::Podcast)
            .unwrap();
        let (shows, total) = provider.list_podcast_shows(0, 1).await.unwrap();
        assert_eq!(total, 1);
        assert_eq!(shows[0].title, "Talks");
        let detail = provider.get_podcast_show(&shows[0].id).await.unwrap();
        assert_eq!(detail.episodes.len(), 2);
        assert_ne!(detail.episodes[0].id, detail.episodes[1].id);
        assert_eq!(detail.episodes[0].show_id, shows[0].id);
        assert!(provider.get_album(&shows[0].id).await.is_err());
        assert!(provider.get_song(&detail.episodes[0].id).await.is_err());
        assert_eq!(
            provider
                .get_playback_display_song(&detail.episodes[0].id)
                .await
                .unwrap()
                .title,
            detail.episodes[0].title
        );
    }

    #[tokio::test]
    async fn recent_podcast_episodes_keep_playable_identity_and_page_order() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        server.mock("GET", "/api/libraries/pod-id/recent-episodes?page=0&limit=2")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"total":3,"episodes":[{"libraryItemId":"show-1","id":"ep-2","title":"Later","publishedAt":1767312000000},{"libraryItemId":"show-1","id":"ep-1","title":"Earlier","publishedAt":1767225600000}]}"#)
            .create_async().await;
        server
            .mock(
                "GET",
                "/api/libraries/pod-id/recent-episodes?page=1&limit=2",
            )
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"total":3,"episodes":[]}"#)
            .create_async()
            .await;
        server
            .mock("GET", "/api/items/show-1")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(include_str!(
                "../../tests/fixtures/audiobookshelf/synthetic/podcast-show.json"
            ))
            .create_async()
            .await;
        let provider = AudiobookshelfProvider::login(&server.url(), "user", "password")
            .await
            .unwrap()
            .scope_to("pod-id".into(), ProviderLibraryRole::Podcast)
            .unwrap();
        let (episodes, total, source_count) =
            provider.list_recent_podcast_episodes(0, 2).await.unwrap();
        assert_eq!(total, 3);
        assert_eq!(source_count, 2);
        assert_eq!(
            episodes
                .iter()
                .map(|episode| episode.title.as_str())
                .collect::<Vec<_>>(),
            vec!["Later", "Earlier"]
        );
        assert_eq!(
            episodes[0].id,
            opaque_id("episode", &["pod-id", "show-1", "media-1", "ep-2"])
        );
        assert_eq!(
            episodes[0].show_id,
            opaque_id("show", &["pod-id", "show-1", "media-1"])
        );
        assert!(provider.list_recent_podcast_episodes(1, 2).await.is_err());
        assert!(provider.list_recent_podcast_episodes(0, 0).await.is_err());
        assert!(
            provider
                .list_recent_podcast_episodes(2, 2)
                .await
                .unwrap()
                .0
                .is_empty()
        );
    }

    #[tokio::test]
    async fn recent_podcast_episodes_skip_a_removed_show_without_losing_other_episodes() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        server.mock("GET", "/api/libraries/pod-id/recent-episodes?page=0&limit=2")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"total":2,"episodes":[{"libraryItemId":"removed","id":"ep-old"},{"libraryItemId":"show-1","id":"ep-2","title":"Available"}]}"#)
            .create_async().await;
        server
            .mock("GET", "/api/items/removed")
            .with_status(404)
            .create_async()
            .await;
        server
            .mock("GET", "/api/items/show-1")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(include_str!(
                "../../tests/fixtures/audiobookshelf/synthetic/podcast-show.json"
            ))
            .create_async()
            .await;
        let provider = AudiobookshelfProvider::login(&server.url(), "user", "password")
            .await
            .unwrap()
            .scope_to("pod-id".into(), ProviderLibraryRole::Podcast)
            .unwrap();
        let (episodes, total, source_count) =
            provider.list_recent_podcast_episodes(0, 2).await.unwrap();
        assert_eq!(total, 2);
        assert_eq!(source_count, 2);
        assert_eq!(episodes.len(), 1);
        assert_eq!(episodes[0].title, "Available");
    }

    #[tokio::test]
    async fn podcast_detail_rejects_cross_library_malformed_and_replaced_media() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        let replaced = server.mock("GET", "/api/items/show-1")
            .with_status(200).with_header("content-type", "application/json")
            .with_body(r#"{"id":"show-1","libraryId":"pod-id","mediaType":"podcast","media":{"id":"replacement","episodes":[]}}"#)
            .expect(1).create_async().await;
        let provider = AudiobookshelfProvider::login(&server.url(), "user", "password")
            .await
            .unwrap()
            .scope_to("pod-id".into(), ProviderLibraryRole::Podcast)
            .unwrap();
        assert!(
            provider
                .get_podcast_show("abs-show-malformed")
                .await
                .is_err()
        );
        assert!(
            provider
                .get_podcast_show(&opaque_id("show", &["other", "show-1", "media-1"]))
                .await
                .is_err()
        );
        assert!(
            provider
                .get_podcast_episode(&opaque_id(
                    "episode",
                    &["other", "show-1", "media-1", "ep-1"]
                ))
                .await
                .is_err()
        );
        assert!(matches!(
            provider
                .get_podcast_show(&opaque_id("show", &["pod-id", "show-1", "media-1"]))
                .await,
            Err(ProviderError::StaleConfiguration(_))
        ));
        replaced.assert_async().await;
    }

    #[tokio::test]
    async fn podcast_episode_playback_requires_matching_episode_session() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        server.mock("GET", "/api/items/show-1").with_status(200).with_header("content-type", "application/json")
            .with_body(r#"{"id":"show-1","libraryId":"pod-id","mediaType":"podcast","media":{"id":"media-1","metadata":{"title":"Talks"},"episodes":[{"id":"ep-1","title":"First","duration":20,"audioFile":{"ino":"ino-1"}}]}}"#)
            .create_async().await;
        let play = server.mock("POST", "/api/items/show-1/play/ep-1")
            .match_body(Matcher::PartialJson(serde_json::json!({"forceDirectPlay": true})))
            .with_status(200).with_header("content-type", "application/json")
            .with_body(r#"{"id":"session-secret","serverVersion":"2.36.1","libraryId":"pod-id","libraryItemId":"show-1","episodeId":"wrong-episode","mediaType":"podcast","playMethod":0,"audioTracks":[{"ino":"ino-1","contentUrl":"/api/items/show-1/file/ino-1","mimeType":"audio/mpeg","codec":"mp3"}]}"#)
            .expect(1).create_async().await;
        let close = server
            .mock("POST", "/api/session/session-secret/close")
            .with_status(200)
            .expect(1)
            .create_async()
            .await;
        let provider = AudiobookshelfProvider::login(&server.url(), "user", "password")
            .await
            .unwrap()
            .scope_to("pod-id".into(), ProviderLibraryRole::Podcast)
            .unwrap();
        let id = opaque_id("episode", &["pod-id", "show-1", "media-1", "ep-1"]);
        assert!(provider.resolve_playback(&id).await.is_err());
        for _ in 0..50 {
            if close.matched_async().await {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        play.assert_async().await;
        close.assert_async().await;
    }

    #[tokio::test]
    async fn podcast_episode_rejects_a_different_file_url_with_matching_ino() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        server.mock("GET", "/api/items/show-1").with_status(200).with_header("content-type", "application/json")
            .with_body(r#"{"id":"show-1","libraryId":"pod-id","mediaType":"podcast","media":{"id":"media-1","metadata":{"title":"Talks"},"episodes":[{"id":"ep-1","audioFile":{"ino":"ino-1"}}]}}"#)
            .create_async().await;
        server.mock("POST", "/api/items/show-1/play/ep-1")
            .with_status(200).with_header("content-type", "application/json")
            .with_body(r#"{"id":"session-secret","serverVersion":"2.36.1","libraryId":"pod-id","libraryItemId":"show-1","episodeId":"ep-1","mediaType":"podcast","playMethod":0,"audioTracks":[{"ino":"ino-1","contentUrl":"/api/items/show-1/file/other-ino","mimeType":"audio/mpeg","codec":"mp3"}]}"#)
            .create_async().await;
        let close = server
            .mock("POST", "/api/session/session-secret/close")
            .with_status(200)
            .expect(1)
            .create_async()
            .await;
        let provider = AudiobookshelfProvider::login(&server.url(), "user", "password")
            .await
            .unwrap()
            .scope_to("pod-id".into(), ProviderLibraryRole::Podcast)
            .unwrap();
        let id = opaque_id("episode", &["pod-id", "show-1", "media-1", "ep-1"]);
        assert!(provider.resolve_playback(&id).await.is_err());
        for _ in 0..50 {
            if close.matched_async().await {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        close.assert_async().await;
    }

    #[tokio::test]
    async fn podcast_episode_direct_mp3_uses_episode_scoped_endpoint_and_closes_session() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        server.mock("GET", "/api/items/show-1").with_status(200).with_header("content-type", "application/json")
            .with_body(r#"{"id":"show-1","libraryId":"pod-id","mediaType":"podcast","media":{"id":"media-1","metadata":{"title":"Talks"},"episodes":[{"id":"ep-1","title":"First","duration":20,"audioFile":{"ino":"ino-1"}}]}}"#)
            .create_async().await;
        let play = server.mock("POST", "/api/items/show-1/play/ep-1")
            .match_body(Matcher::PartialJson(serde_json::json!({"forceDirectPlay": true})))
            .with_status(200).with_header("content-type", "application/json")
            .with_body(r#"{"id":"session-secret","serverVersion":"2.36.1","libraryId":"pod-id","libraryItemId":"show-1","episodeId":"ep-1","mediaType":"podcast","playMethod":0,"audioTracks":[{"ino":"ino-1","contentUrl":"/api/items/show-1/file/ino-1","mimeType":"audio/mpeg","codec":"mp3"}]}"#)
            .expect(1).create_async().await;
        let media = server
            .mock("GET", "/api/items/show-1/file/ino-1")
            .match_header("authorization", "Bearer access-fixture")
            .match_header("range", "bytes=0-0")
            .with_status(206)
            .with_header("content-type", "audio/mpeg")
            .with_header("accept-ranges", "bytes")
            .with_header("content-range", "bytes 0-0/100")
            .with_body("x")
            .expect(1)
            .create_async()
            .await;
        let close = server
            .mock("POST", "/api/session/session-secret/close")
            .with_status(200)
            .expect(1)
            .create_async()
            .await;
        let provider = AudiobookshelfProvider::login(&server.url(), "user", "password")
            .await
            .unwrap()
            .scope_to("pod-id".into(), ProviderLibraryRole::Podcast)
            .unwrap();
        let id = opaque_id("episode", &["pod-id", "show-1", "media-1", "ep-1"]);
        let description = provider.resolve_playback(&id).await.unwrap();
        assert_eq!(description.song.id, id);
        assert_eq!(description.representations[0].codec.as_deref(), Some("mp3"));
        assert!(description.song.album_id.is_none());
        assert_eq!(
            description.representations[0].seek_mechanism,
            Some(PlaybackSeekMechanism::AudiobookshelfDirectMp3)
        );
        drop(description);
        for _ in 0..50 {
            if close.matched_async().await {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        play.assert_async().await;
        media.assert_async().await;
        close.assert_async().await;
    }

    #[tokio::test]
    async fn podcast_episode_media_read_404_retires_session() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        server.mock("GET", "/api/items/show-1").with_status(200).with_header("content-type", "application/json")
            .with_body(r#"{"id":"show-1","libraryId":"pod-id","mediaType":"podcast","media":{"id":"media-1","metadata":{"title":"Talks"},"episodes":[{"id":"ep-1","title":"First","audioFile":{"ino":"ino-1"}}]}}"#)
            .create_async().await;
        server.mock("POST", "/api/items/show-1/play/ep-1")
            .with_status(200).with_header("content-type", "application/json")
            .with_body(r#"{"id":"session-secret","serverVersion":"2.36.1","libraryId":"pod-id","libraryItemId":"show-1","episodeId":"ep-1","mediaType":"podcast","playMethod":0,"audioTracks":[{"ino":"ino-1","contentUrl":"/api/items/show-1/file/ino-1","mimeType":"audio/mpeg","codec":"mp3"}]}"#)
            .create_async().await;
        let media = server
            .mock("GET", "/api/items/show-1/file/ino-1")
            .match_header("range", "bytes=0-0")
            .with_status(404)
            .expect(1)
            .create_async()
            .await;
        let close = server
            .mock("POST", "/api/session/session-secret/close")
            .with_status(200)
            .expect(1)
            .create_async()
            .await;
        let provider = AudiobookshelfProvider::login(&server.url(), "user", "password")
            .await
            .unwrap()
            .scope_to("pod-id".into(), ProviderLibraryRole::Podcast)
            .unwrap();
        let id = opaque_id("episode", &["pod-id", "show-1", "media-1", "ep-1"]);
        assert!(matches!(
            provider.resolve_playback(&id).await,
            Err(ProviderError::NotFound { .. })
        ));
        for _ in 0..50 {
            if close.matched_async().await {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        media.assert_async().await;
        close.assert_async().await;
    }

    #[tokio::test]
    async fn podcast_episode_rejects_transcoded_session_before_media_read() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        server.mock("GET", "/api/items/show-1").with_status(200).with_header("content-type", "application/json")
            .with_body(r#"{"id":"show-1","libraryId":"pod-id","mediaType":"podcast","media":{"id":"media-1","episodes":[{"id":"ep-1","audioFile":{"ino":"ino-1"}}]}}"#)
            .create_async().await;
        server.mock("POST", "/api/items/show-1/play/ep-1").with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"id":"session-secret","serverVersion":"2.36.1","libraryId":"pod-id","libraryItemId":"show-1","episodeId":"ep-1","mediaType":"podcast","playMethod":2,"audioTracks":[{"contentUrl":"/api/session/session-secret/playlist.m3u8","mimeType":"application/vnd.apple.mpegurl"}]}"#)
            .create_async().await;
        let close = server
            .mock("POST", "/api/session/session-secret/close")
            .with_status(200)
            .expect(1)
            .create_async()
            .await;
        let provider = AudiobookshelfProvider::login(&server.url(), "user", "password")
            .await
            .unwrap()
            .scope_to("pod-id".into(), ProviderLibraryRole::Podcast)
            .unwrap();
        let id = opaque_id("episode", &["pod-id", "show-1", "media-1", "ep-1"]);
        assert!(matches!(
            provider.resolve_playback(&id).await,
            Err(ProviderError::UnsupportedCapability(_))
        ));
        for _ in 0..50 {
            if close.matched_async().await {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        close.assert_async().await;
    }

    #[tokio::test]
    async fn podcast_episode_replacement_during_admission_closes_session() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        let original = server.mock("GET", "/api/items/show-1")
            .with_status(200).with_header("content-type", "application/json")
            .with_body(r#"{"id":"show-1","libraryId":"pod-id","mediaType":"podcast","media":{"id":"media-1","episodes":[{"id":"ep-1","audioFile":{"ino":"ino-1"}}]}}"#)
            .expect(1).create_async().await;
        server.mock("POST", "/api/items/show-1/play/ep-1").with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"id":"session-secret","serverVersion":"2.36.1","libraryId":"pod-id","libraryItemId":"show-1","episodeId":"ep-1","mediaType":"podcast","playMethod":0,"audioTracks":[{"ino":"ino-1","contentUrl":"/api/items/show-1/file/ino-1","mimeType":"audio/mpeg","codec":"mp3"}]}"#)
            .create_async().await;
        let replaced = server.mock("GET", "/api/items/show-1")
            .with_status(200).with_header("content-type", "application/json")
            .with_body(r#"{"id":"show-1","libraryId":"pod-id","mediaType":"podcast","media":{"id":"media-2","episodes":[{"id":"ep-1","audioFile":{"ino":"ino-2"}}]}}"#)
            .expect(1).create_async().await;
        let close = server
            .mock("POST", "/api/session/session-secret/close")
            .with_status(200)
            .expect(1)
            .create_async()
            .await;
        let provider = AudiobookshelfProvider::login(&server.url(), "user", "password")
            .await
            .unwrap()
            .scope_to("pod-id".into(), ProviderLibraryRole::Podcast)
            .unwrap();
        let id = opaque_id("episode", &["pod-id", "show-1", "media-1", "ep-1"]);
        assert!(matches!(
            provider.resolve_playback(&id).await,
            Err(ProviderError::StaleConfiguration(_))
        ));
        for _ in 0..50 {
            if close.matched_async().await {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        original.assert_async().await;
        replaced.assert_async().await;
        close.assert_async().await;
    }

    #[tokio::test]
    async fn podcast_search_uses_one_bounded_limit_without_page_and_reports_truncation() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        let hits = (0..50).map(|index| format!(r#"{{"libraryItem":{{"id":"show-{index}","libraryId":"pod-id","mediaType":"podcast","media":{{"libraryItemId":"media-{index}","metadata":{{"title":"Talks"}},"numEpisodes":2,"episodes":[{{"id":null}}]}}}}}}"#)).collect::<Vec<_>>().join(",");
        let search = server
            .mock("GET", "/api/libraries/pod-id/search")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("q".into(), "talk".into()),
                Matcher::UrlEncoded("limit".into(), "50".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(format!(r#"{{"podcast":[{hits}],"episodes":[]}}"#))
            .expect(1)
            .create_async()
            .await;
        let provider = AudiobookshelfProvider::login(&server.url(), "user", "password")
            .await
            .unwrap()
            .scope_to("pod-id".into(), ProviderLibraryRole::Podcast)
            .unwrap();
        let result = provider.search_podcasts("talk").await.unwrap();
        assert_eq!(result.shows.len(), 50);
        assert_eq!(result.shows[0].episode_count, Some(2));
        assert!(result.possibly_truncated);
        assert!(result.episodes.is_empty());
        search.assert_async().await;
    }

    #[tokio::test]
    async fn podcast_search_maps_episode_identity_without_album_alias() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        server.mock("GET", "/api/libraries/pod-id/search")
            .match_query(Matcher::AllOf(vec![Matcher::UrlEncoded("q".into(), "first".into()), Matcher::UrlEncoded("limit".into(), "50".into())]))
            .with_status(200).with_header("content-type", "application/json")
            .with_body(r#"{"podcast":[],"episodes":[{"unknown":"future search hit"},{"libraryItem":{"id":"show-1","libraryId":"pod-id","mediaType":"podcast","media":{"id":"media-1","libraryItemId":"show-1","metadata":{"title":"Talks"},"episodes":[{"id":null}]},"recentEpisode":{"id":"episode-1","title":"First","publishedAt":1767225600000}}}]}"#)
            .create_async().await;
        let provider = AudiobookshelfProvider::login(&server.url(), "user", "password")
            .await
            .unwrap()
            .scope_to("pod-id".into(), ProviderLibraryRole::Podcast)
            .unwrap();
        let result = provider.search_podcasts("first").await.unwrap();
        assert!(result.shows.is_empty());
        assert_eq!(result.episodes.len(), 1);
        assert_eq!(
            result.episodes[0].show_id,
            opaque_id("show", &["pod-id", "show-1", "media-1"])
        );
        assert_eq!(
            result.episodes[0].id,
            opaque_id("episode", &["pod-id", "show-1", "media-1", "episode-1"])
        );
        assert!(result.episodes[0].published_at.is_some());
    }

    #[tokio::test]
    async fn podcast_list_classifies_permission_missing_rate_limit_and_server_failure() {
        for status in [403, 404, 429, 500] {
            let mut server = Server::new_async().await;
            server
                .mock("POST", "/login")
                .with_status(200)
                .with_header("content-type", "application/json")
                .with_body(login_body())
                .create_async()
                .await;
            server
                .mock("GET", "/api/libraries/pod-id/items")
                .match_query(Matcher::AllOf(vec![
                    Matcher::UrlEncoded("page".into(), "0".into()),
                    Matcher::UrlEncoded("limit".into(), "1".into()),
                ]))
                .with_status(status)
                .with_body("private-provider-body")
                .create_async()
                .await;
            let provider = AudiobookshelfProvider::login(&server.url(), "user", "password")
                .await
                .unwrap()
                .scope_to("pod-id".into(), ProviderLibraryRole::Podcast)
                .unwrap();
            let error = provider.list_podcast_shows(0, 1).await.unwrap_err();
            assert!(!format!("{error:?}").contains("private-provider-body"));
            match status {
                403 => assert!(matches!(error, ProviderError::Forbidden), "{error:?}"),
                404 => assert!(matches!(error, ProviderError::StaleConfiguration(_))),
                429 => assert!(matches!(error, ProviderError::RateLimited { .. })),
                500 => assert!(matches!(
                    error,
                    ProviderError::Http {
                        status: Some(500),
                        ..
                    }
                )),
                _ => unreachable!(),
            }
        }
    }

    #[tokio::test]
    async fn podcast_list_refreshes_once_after_401_without_exposing_tokens() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(login_body())
            .create_async()
            .await;
        let expired = server
            .mock("GET", "/api/libraries/pod-id/items")
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
            .with_body(r#"{"user":{"accessToken":"access-refreshed"}}"#)
            .expect(1)
            .create_async()
            .await;
        let page = server
            .mock("GET", "/api/libraries/pod-id/items")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("page".into(), "0".into()),
                Matcher::UrlEncoded("limit".into(), "1".into()),
            ]))
            .match_header("authorization", "Bearer access-refreshed")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"total":0,"results":[]}"#)
            .expect(1)
            .create_async()
            .await;
        let provider = AudiobookshelfProvider::login(&server.url(), "user", "password")
            .await
            .unwrap()
            .scope_to("pod-id".into(), ProviderLibraryRole::Podcast)
            .unwrap();
        assert_eq!(provider.list_podcast_shows(0, 1).await.unwrap().1, 0);
        assert!(!format!("{provider:?}").contains("access-refreshed"));
        expired.assert_async().await;
        refresh.assert_async().await;
        page.assert_async().await;
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
