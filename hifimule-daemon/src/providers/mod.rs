use crate::domain::models::{
    Album, AlbumWithTracks, Artist, ArtistWithAlbums, ChangeEvent, Genre, Library, Playlist,
    PlaylistWithTracks, PodcastEpisode, PodcastSearchResult, PodcastShow, PodcastShowDetail,
    SearchResult, Song,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::Arc;
use std::sync::{Mutex, OnceLock};
use thiserror::Error;

pub mod audiobookshelf;
pub mod jellyfin;
pub mod subsonic;

pub const SUBSONIC_PLAYLISTS_LIBRARY_ID: &str = "playlists";

pub const MAX_PLAYBACK_REPRESENTATIONS: usize = 8;

/// Private action tied to the last owner of an upstream playback request.
/// No upstream identifier or credential is exposed through Debug or RPC.
pub struct PlaybackCleanup(std::sync::Mutex<Option<Box<dyn FnOnce() + Send + 'static>>>);

pub type PlaybackRefresh = dyn Fn() -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Option<reqwest::header::HeaderMap>> + Send>,
    > + Send
    + Sync;

impl PlaybackCleanup {
    pub fn new(action: impl FnOnce() + Send + 'static) -> Self {
        Self(std::sync::Mutex::new(Some(Box::new(action))))
    }
}

impl fmt::Debug for PlaybackCleanup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PlaybackCleanup([redacted])")
    }
}

impl Drop for PlaybackCleanup {
    fn drop(&mut self) {
        if let Some(action) = self.0.lock().unwrap_or_else(|e| e.into_inner()).take() {
            action();
        }
    }
}

static PLAYBACK_CLEANUPS: OnceLock<Mutex<Vec<tokio::task::JoinHandle<()>>>> = OnceLock::new();
static PLAYBACK_CLEANUP_FAILED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub(crate) fn register_playback_cleanup(handle: tokio::task::JoinHandle<bool>) {
    let monitored = tokio::spawn(async move {
        if !matches!(handle.await, Ok(true)) {
            PLAYBACK_CLEANUP_FAILED.store(true, std::sync::atomic::Ordering::Release);
        }
    });
    let registry = PLAYBACK_CLEANUPS.get_or_init(|| Mutex::new(Vec::new()));
    let mut pending = registry.lock().unwrap_or_else(|error| error.into_inner());
    pending.retain(|task| !task.is_finished());
    pending.push(monitored);
}

pub(crate) async fn drain_playback_cleanups() -> bool {
    let Some(registry) = PLAYBACK_CLEANUPS.get() else {
        return true;
    };
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
    let mut succeeded = true;
    loop {
        let tasks = {
            let mut pending = registry.lock().unwrap_or_else(|error| error.into_inner());
            std::mem::take(&mut *pending)
        };
        if tasks.is_empty() {
            let failed = PLAYBACK_CLEANUP_FAILED.swap(false, std::sync::atomic::Ordering::AcqRel);
            return succeeded && !failed;
        }
        for task in tasks {
            match tokio::time::timeout_at(deadline, task).await {
                Ok(Ok(())) => {}
                _ => succeeded = false,
            }
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
    }
}

#[derive(Clone)]
pub struct PlaybackRequest {
    pub url: reqwest::Url,
    pub headers: reqwest::header::HeaderMap,
    pub range_supported: bool,
    pub cleanup: Option<Arc<PlaybackCleanup>>,
    pub refresh: Option<Arc<PlaybackRefresh>>,
    pub expected_content_type: Option<String>,
}

impl PartialEq for PlaybackRequest {
    fn eq(&self, other: &Self) -> bool {
        self.url == other.url
            && self.headers == other.headers
            && self.range_supported == other.range_supported
            && self.expected_content_type == other.expected_content_type
    }
}
impl Eq for PlaybackRequest {}

impl fmt::Debug for PlaybackRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PlaybackRequest")
            .field("origin", &self.url.origin().ascii_serialization())
            .field("path", &"[redacted]")
            .field("headers", &"[redacted]")
            .field("range_supported", &self.range_supported)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackProvenance {
    Original,
    Alternative,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackSeekMechanism {
    JellyfinOriginalPcmWav,
    JellyfinOriginalM4a,
    JellyfinOriginalOpus,
    JellyfinOriginalMp3,
    JellyfinOriginalFlac,
    NavidromeOriginalPcmWav,
    NavidromeOriginalM4a,
    NavidromeOriginalOpus,
    NavidromeOriginalMp3,
    NavidromeOriginalFlac,
    AudiobookshelfDirectMp3,
    AudiobookshelfDirectM4a,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaybackRepresentation {
    pub codec: Option<String>,
    pub container: Option<String>,
    pub bitrate_kbps: Option<u32>,
    pub sample_rate: Option<u32>,
    pub bit_depth: Option<u8>,
    pub provenance: PlaybackProvenance,
    /// Provider-qualified candidate. The decoder must still verify the actual
    /// container and codec before the owner advertises seeking.
    pub seek_mechanism: Option<PlaybackSeekMechanism>,
    pub request: PlaybackRequest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaybackDescription {
    pub song: Song,
    pub representations: Vec<PlaybackRepresentation>,
}

/// Daemon-private representation admitted for managed device sync. Ownership of
/// `request` keeps provider session cleanup alive until the staged read ends.
#[derive(Debug, Clone)]
pub struct SyncMediaRepresentation {
    pub codec: String,
    pub container: String,
    pub request: PlaybackRequest,
}

impl SyncMediaRepresentation {
    pub fn from_playback_representations(
        mut representations: Vec<PlaybackRepresentation>,
    ) -> Result<Self, ProviderError> {
        if representations.len() != 1 {
            return Err(ProviderError::UnsupportedCapability(
                "sync requires one verified direct media representation".into(),
            ));
        }
        let representation = representations.pop().unwrap();
        if representation.provenance != PlaybackProvenance::Original {
            return Err(ProviderError::UnsupportedCapability(
                "sync requires original direct media".into(),
            ));
        }
        let codec = representation
            .codec
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                ProviderError::UnsupportedCapability("sync codec is unverified".into())
            })?;
        let container = representation
            .container
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                ProviderError::UnsupportedCapability("sync container is unverified".into())
            })?;
        if representation.request.expected_content_type.is_none() {
            return Err(ProviderError::UnsupportedCapability(
                "sync content type is unverified".into(),
            ));
        }
        Ok(Self {
            codec,
            container,
            request: representation.request,
        })
    }
}

/// Daemon-private book progress. Times are integer milliseconds at this
/// boundary; the Audiobookshelf adapter alone converts to whole-book seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BookProgress {
    pub current_ms: u64,
    pub duration_ms: u64,
    pub is_finished: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookPartTiming {
    pub track_id: String,
    pub audio_file_id: String,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookTiming {
    pub identity: crate::domain::models::ProviderIdentity,
    pub parts: Vec<BookPartTiming>,
}

pub(crate) fn select_playback_representation(
    mut representations: Vec<PlaybackRepresentation>,
) -> Result<PlaybackRepresentation, ProviderError> {
    if representations.len() > MAX_PLAYBACK_REPRESENTATIONS {
        return Err(ProviderError::UnsupportedCapability(
            "provider advertised more than eight playback representations".into(),
        ));
    }
    representations.retain(representation_is_supported);
    representations.sort_by(|left, right| {
        let left_group = representation_group(left);
        let right_group = representation_group(right);
        left_group.cmp(&right_group).then_with(|| match left_group {
            1 => right
                .sample_rate
                .cmp(&left.sample_rate)
                .then_with(|| right.bit_depth.cmp(&left.bit_depth)),
            2 if left.codec == right.codec => right.bitrate_kbps.cmp(&left.bitrate_kbps),
            _ => std::cmp::Ordering::Equal,
        })
    });
    representations.into_iter().next().ok_or_else(|| {
        ProviderError::UnsupportedCapability("no supported playback representation".into())
    })
}

fn representation_group(representation: &PlaybackRepresentation) -> u8 {
    if representation.provenance == PlaybackProvenance::Original {
        return 0;
    }
    if representation.codec.as_deref().is_some_and(|codec| {
        matches!(
            codec.to_ascii_lowercase().as_str(),
            "flac"
                | "alac"
                | "wmalossless"
                | "pcm_s16le"
                | "pcm_s24le"
                | "pcm_s32le"
                | "pcm_s16be"
                | "pcm_s24be"
                | "pcm_s32be"
                | "aif"
                | "aiff"
        )
    }) {
        1
    } else {
        2
    }
}

fn representation_is_supported(representation: &PlaybackRepresentation) -> bool {
    let Some(codec) = representation.codec.as_deref() else {
        return true;
    };
    matches!(
        codec.to_ascii_lowercase().as_str(),
        "pcm_s16le"
            | "pcm_s24le"
            | "pcm_s32le"
            | "flac"
            | "alac"
            | "mp3"
            | "aac"
            | "m4a"
            | "m4r"
            | "mp4"
            | "wav"
            | "opus"
            | "aif"
            | "aiff"
            | "ogg"
            | "oga"
            | "vorbis"
            | "wma"
            | "asf"
            | "wmav1"
            | "wmav2"
            | "wmapro"
            | "wmalossless"
            | "pcm_s16be"
            | "pcm_s24be"
            | "pcm_s32be"
    )
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderChangeContext {
    #[serde(default)]
    pub synced_songs: Vec<ProviderSyncedSong>,
    #[serde(default)]
    pub synced_album_ids: Vec<String>,
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
    async fn list_podcast_shows(
        &self,
        _offset: u32,
        _limit: u32,
    ) -> Result<(Vec<PodcastShow>, u32), ProviderError> {
        Err(ProviderError::UnsupportedCapability(
            "podcast shows unavailable".into(),
        ))
    }

    async fn list_recent_podcast_episodes(
        &self,
        _offset: u32,
        _limit: u32,
    ) -> Result<(Vec<PodcastEpisode>, u32, u32), ProviderError> {
        Err(ProviderError::UnsupportedCapability(
            "recent podcast episodes unavailable".into(),
        ))
    }

    async fn get_podcast_show(&self, _id: &str) -> Result<PodcastShowDetail, ProviderError> {
        Err(ProviderError::UnsupportedCapability(
            "podcast show unavailable".into(),
        ))
    }

    async fn get_podcast_episode(&self, _id: &str) -> Result<PodcastEpisode, ProviderError> {
        Err(ProviderError::UnsupportedCapability(
            "podcast episode unavailable".into(),
        ))
    }

    async fn get_playback_display_song(&self, id: &str) -> Result<Song, ProviderError> {
        self.get_song(id).await
    }

    async fn search_podcasts(&self, _query: &str) -> Result<PodcastSearchResult, ProviderError> {
        Err(ProviderError::UnsupportedCapability(
            "podcast search unavailable".into(),
        ))
    }

    async fn book_timing(&self, _album_id: &str) -> Result<Option<BookTiming>, ProviderError> {
        Ok(None)
    }

    async fn book_timing_for_track(
        &self,
        _track_id: &str,
    ) -> Result<Option<BookTiming>, ProviderError> {
        Ok(None)
    }

    async fn read_book_progress(
        &self,
        _identity: &crate::domain::models::ProviderIdentity,
    ) -> Result<Option<BookProgress>, ProviderError> {
        Err(ProviderError::UnsupportedCapability(
            "book progress unavailable".into(),
        ))
    }

    async fn write_book_progress(
        &self,
        _expected: &BookTiming,
        _progress: BookProgress,
    ) -> Result<(), ProviderError> {
        Err(ProviderError::UnsupportedCapability(
            "book progress unavailable".into(),
        ))
    }
    async fn list_libraries(&self) -> Result<Vec<Library>, ProviderError>;

    async fn list_artists(
        &self,
        library_id: Option<&str>,
        letter: Option<&str>,
        offset: u32,
        limit: u32,
    ) -> Result<(Vec<Artist>, u32), ProviderError>;

    async fn get_artist(&self, artist_id: &str) -> Result<ArtistWithAlbums, ProviderError>;

    async fn list_albums(
        &self,
        library_id: Option<&str>,
        letter: Option<&str>,
        offset: u32,
        limit: u32,
    ) -> Result<(Vec<Album>, u32), ProviderError>;

    async fn get_album(&self, album_id: &str) -> Result<AlbumWithTracks, ProviderError>;

    async fn get_song(&self, song_id: &str) -> Result<Song, ProviderError> {
        Err(ProviderError::UnsupportedCapability(format!(
            "get_song is not supported by this provider for song {song_id}"
        )))
    }

    async fn list_playlists(&self) -> Result<Vec<Playlist>, ProviderError>;

    async fn get_playlist(&self, playlist_id: &str) -> Result<PlaylistWithTracks, ProviderError>;

    async fn search(&self, query: &str) -> Result<SearchResult, ProviderError>;

    async fn download_url(
        &self,
        song_id: &str,
        profile: Option<&TranscodeProfile>,
    ) -> Result<String, ProviderError>;

    /// A private authenticated media request for device sync. Providers without
    /// this capability continue using `download_url` for music.
    async fn resolve_sync_media(
        &self,
        _id: &str,
    ) -> Result<SyncMediaRepresentation, ProviderError> {
        Err(ProviderError::UnsupportedCapability(
            "direct sync media is unavailable".into(),
        ))
    }

    async fn resolve_playback(&self, _song_id: &str) -> Result<PlaybackDescription, ProviderError> {
        Err(ProviderError::UnsupportedCapability(
            "resolve_playback is not supported by this provider".to_string(),
        ))
    }

    async fn cover_art_url(&self, cover_art_id: &str) -> Result<String, ProviderError>;

    /// Retrieves artwork through the provider boundary. Providers with authenticated
    /// artwork (Audiobookshelf) override this so credentials never become a URL sent
    /// to the UI; legacy providers retain their existing URL-based behavior.
    async fn fetch_cover_art(
        &self,
        cover_art_id: &str,
    ) -> Result<reqwest::Response, ProviderError> {
        let url = self.cover_art_url(cover_art_id).await?;
        reqwest::Client::new()
            .get(url)
            .send()
            .await
            .map_err(|error| ProviderError::Http {
                status: error.status().map(|status| status.as_u16()),
                message: sanitize_secret_message(&error.to_string()),
            })
    }

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

    async fn list_genres(
        &self,
        _library_id: Option<&str>,
        _offset: u32,
        _limit: u32,
    ) -> Result<(Vec<Genre>, u64), ProviderError> {
        Err(ProviderError::UnsupportedCapability(
            "list_genres is not supported by this provider".to_string(),
        ))
    }

    async fn get_genre_tracks(
        &self,
        _genre_id_or_name: &str,
        _offset: u32,
        _limit: u32,
    ) -> Result<(Vec<Song>, u32), ProviderError> {
        Err(ProviderError::UnsupportedCapability(
            "get_genre_tracks is not supported by this provider".to_string(),
        ))
    }

    async fn list_recently_added(
        &self,
        _library_id: Option<&str>,
        _offset: u32,
        _limit: u32,
    ) -> Result<(Vec<Album>, u32), ProviderError> {
        Err(ProviderError::UnsupportedCapability(
            "list_recently_added is not supported by this provider".to_string(),
        ))
    }

    async fn list_frequently_played(
        &self,
        _library_id: Option<&str>,
        _offset: u32,
        _limit: u32,
    ) -> Result<(Vec<Song>, u32), ProviderError> {
        Err(ProviderError::UnsupportedCapability(
            "list_frequently_played is not supported by this provider".to_string(),
        ))
    }

    async fn list_recently_played(
        &self,
        _library_id: Option<&str>,
        _offset: u32,
        _limit: u32,
    ) -> Result<(Vec<Song>, u32), ProviderError> {
        Err(ProviderError::UnsupportedCapability(
            "list_recently_played is not supported by this provider".to_string(),
        ))
    }

    async fn list_favorites(
        &self,
        _library_id: Option<&str>,
        _offset: u32,
        _limit: u32,
    ) -> Result<(Vec<Song>, u32), ProviderError> {
        Err(ProviderError::UnsupportedCapability(
            "list_favorites is not supported by this provider".to_string(),
        ))
    }

    async fn list_favorite_items(
        &self,
        _library_id: Option<&str>,
    ) -> Result<SearchResult, ProviderError> {
        Err(ProviderError::UnsupportedCapability(
            "list_favorite_items is not supported by this provider".to_string(),
        ))
    }

    async fn list_tracks(&self, _filter: TrackListFilter) -> Result<TrackListPage, ProviderError> {
        Err(ProviderError::UnsupportedCapability(
            "list_tracks is not supported by this provider".to_string(),
        ))
    }

    /// Paginated listing of all songs in the library, in any server-defined order.
    /// Used as the bulk-fill pass in provider auto-fill, after priority lists are exhausted.
    async fn list_all_songs_page(
        &self,
        _library_id: Option<&str>,
        _offset: u32,
        _limit: u32,
    ) -> Result<(Vec<Song>, u32), ProviderError> {
        Err(ProviderError::UnsupportedCapability(
            "list_all_songs_page is not supported by this provider".to_string(),
        ))
    }

    async fn create_playlist(
        &self,
        _name: &str,
        _track_ids: &[String],
    ) -> Result<String, ProviderError> {
        Err(ProviderError::UnsupportedCapability(
            "create_playlist is not supported by this provider".to_string(),
        ))
    }

    async fn add_to_playlist(
        &self,
        _playlist_id: &str,
        _track_ids: &[String],
    ) -> Result<(), ProviderError> {
        Err(ProviderError::UnsupportedCapability(
            "add_to_playlist is not supported by this provider".to_string(),
        ))
    }

    async fn remove_from_playlist(
        &self,
        _playlist_id: &str,
        _track_ids: &[String],
    ) -> Result<(), ProviderError> {
        Err(ProviderError::UnsupportedCapability(
            "remove_from_playlist is not supported by this provider".to_string(),
        ))
    }

    async fn delete_playlist(&self, _playlist_id: &str) -> Result<(), ProviderError> {
        Err(ProviderError::UnsupportedCapability(
            "delete_playlist is not supported by this provider".to_string(),
        ))
    }

    async fn rename_playlist(
        &self,
        _playlist_id: &str,
        _new_name: &str,
    ) -> Result<(), ProviderError> {
        Err(ProviderError::UnsupportedCapability(
            "rename_playlist is not supported by this provider".to_string(),
        ))
    }

    async fn reorder_playlist(
        &self,
        _playlist_id: &str,
        _ordered_track_ids: &[String],
    ) -> Result<(), ProviderError> {
        Err(ProviderError::UnsupportedCapability(
            "reorder_playlist is not supported by this provider".to_string(),
        ))
    }

    fn change_metadata(&self, _event: &ChangeEvent) -> Option<ProviderChangeMetadata> {
        None
    }

    fn server_type(&self) -> ServerType;

    /// Provider-neutral persisted library role. Existing music providers are
    /// unscoped and therefore return `None`.
    fn library_role(&self) -> Option<ProviderLibraryRole> {
        None
    }

    fn server_version(&self) -> Option<&str> {
        None
    }

    fn access_token(&self) -> Option<&str> {
        None
    }

    fn provider_user_id(&self) -> Option<&str> {
        None
    }

    /// Server-reported stable id (e.g. Jellyfin `System/Info.Id`), when the provider
    /// exposes one. Drives the portable `server_id` `rid:` basis (Story 2.13).
    /// Subsonic/OpenSubsonic has no such concept → `None` (URL basis).
    fn server_reported_id(&self) -> Option<&str> {
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
    Audiobookshelf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServerType {
    Jellyfin,
    Subsonic,
    OpenSubsonic,
    Audiobookshelf,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderLibraryRole {
    Audiobook,
    Podcast,
}

impl ProviderLibraryRole {
    pub fn slug(self) -> &'static str {
        match self {
            Self::Audiobook => "audiobook",
            Self::Podcast => "podcast",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BrowseMode {
    Podcasts,
    RecentEpisodes,
    Artists,
    Albums,
    Playlists,
    Tracks,
    Genres,
    RecentlyAdded,
    FrequentlyPlayed,
    RecentlyPlayed,
    Favorites,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowseCapabilities {
    pub list_modes: Vec<BrowseMode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    pub open_subsonic: bool,
    pub supports_changes_since: bool,
    pub supports_server_transcoding: bool,
    pub supports_playlist_write: bool,
    pub browse: BrowseCapabilities,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackListFilter {
    pub library_id: Option<String>,
    pub artist_id: Option<String>,
    pub album_id: Option<String>,
    pub letter: Option<String>,
    pub start_index: u32,
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackListPage {
    pub tracks: Vec<Song>,
    pub total: u32,
    pub start_index: u32,
    pub limit: u32,
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

    #[error("provider permission denied")]
    Forbidden,

    #[error("provider configuration is stale: {0}")]
    StaleConfiguration(String),

    #[error("provider rate limited")]
    RateLimited { retry_after_seconds: Option<u64> },

    #[error("provider capability is unsupported: {0}")]
    UnsupportedCapability(String),

    #[error("provider response deserialization failed: {0}")]
    Deserialization(String),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// Probes a server URL without credentials to detect its type.
/// Uses Audiobookshelf's status marker, Subsonic's unauthenticated ping envelope,
/// and Jellyfin's public info endpoint.
pub async fn probe_url(url: &str) -> ServerType {
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
    {
        Ok(c) => c,
        Err(_) => return ServerType::Unknown,
    };
    let base = url.trim().trim_end_matches('/');

    // Audiobookshelf exposes a public status document at the configured base.
    // Requiring both success and the exact app marker avoids identifying generic
    // JSON endpoints as Audiobookshelf servers.
    let audiobookshelf_url = format!("{base}/status");
    let subsonic_url = format!("{}/rest/ping.view?v=1.16.1&c=hifimule-probe&f=json", base);
    let jellyfin_url = format!("{}/System/Info/Public", base);

    // Run all unauthenticated probes concurrently so adding a provider never
    // adds another full timeout to existing provider detection.
    let audiobookshelf = async {
        let resp = client.get(&audiobookshelf_url).send().await.ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let status = resp.json::<serde_json::Value>().await.ok()?;
        (status.get("app").and_then(|value| value.as_str()) == Some("audiobookshelf"))
            .then_some(ServerType::Audiobookshelf)
    };
    let subsonic = async {
        let text = client
            .get(&subsonic_url)
            .send()
            .await
            .ok()?
            .text()
            .await
            .ok()?;
        if !text.contains("subsonic-response") {
            return None;
        }
        let open_subsonic = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|json| {
                json.pointer("/subsonic-response/openSubsonic")
                    .and_then(|value| value.as_bool())
            })
            .unwrap_or(false);
        Some(if open_subsonic {
            ServerType::OpenSubsonic
        } else {
            ServerType::Subsonic
        })
    };
    let jellyfin = async {
        let resp = client.get(&jellyfin_url).send().await.ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let text = resp.text().await.ok()?;
        (text.contains("\"ServerName\"") || text.contains("\"Version\""))
            .then_some(ServerType::Jellyfin)
    };
    let (audiobookshelf, subsonic, jellyfin) = tokio::join!(audiobookshelf, subsonic, jellyfin);
    if let Some(server_type) = audiobookshelf.or(subsonic).or(jellyfin) {
        return server_type;
    }

    ServerType::Unknown
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
        ServerTypeHint::Audiobookshelf => Err(ProviderError::UnsupportedCapability(
            "LIBRARY_SELECTION_REQUIRED".to_string(),
        )),
    }
}

pub fn server_type_slug(server_type: ServerType) -> Option<&'static str> {
    match server_type {
        ServerType::Jellyfin => Some("jellyfin"),
        ServerType::Subsonic => Some("subsonic"),
        ServerType::OpenSubsonic => Some("openSubsonic"),
        ServerType::Audiobookshelf => Some("audiobookshelf"),
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
    )
    .with_reported_id(Some(info.id));
    Ok(Arc::new(provider))
}

pub(crate) fn sanitize_secret_message(message: &str) -> String {
    let mut sanitized = String::with_capacity(message.len());
    let mut remainder = message;
    loop {
        let lowercase_remainder = remainder.to_ascii_lowercase();
        let next_url = ["https://", "http://"]
            .into_iter()
            .filter_map(|scheme| lowercase_remainder.find(scheme))
            .min();
        let Some(start) = next_url else {
            sanitized.push_str(remainder);
            break;
        };
        sanitized.push_str(&remainder[..start]);
        sanitized.push_str("[redacted-url]");
        let url_end = remainder[start..]
            .find(char::is_whitespace)
            .map(|offset| start + offset)
            .unwrap_or(remainder.len());
        remainder = &remainder[url_end..];
    }
    for key in [
        "password",
        "pw",
        "token",
        "api_key",
        "ApiKey",
        "title",
        "trackId",
        "track_id",
        "serverId",
        "server_id",
        "songId",
        "song_id",
        "itemId",
        "item_id",
        "id",
        "u",
        "p",
        "t",
        "s",
    ] {
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
    async fn probe_detects_audiobookshelf_at_root_or_prefixed_base() {
        for prefix in ["", "/audiobookshelf"] {
            let mut server = Server::new_async().await;
            let status = server
                .mock("GET", format!("{prefix}/status").as_str())
                .with_status(200)
                .with_header("content-type", "application/json")
                .with_body(r#"{"app":"audiobookshelf","serverVersion":"2.36.1"}"#)
                .expect(1)
                .create_async()
                .await;

            assert_eq!(
                probe_url(format!("{}{prefix}/", server.url()).as_str()).await,
                ServerType::Audiobookshelf
            );
            status.assert_async().await;
        }
    }

    #[tokio::test]
    async fn probe_requires_exact_audiobookshelf_status_marker_and_preserves_fallbacks() {
        for (status_code, body) in [
            (200, r#"{"app":"not-audiobookshelf"}"#),
            (200, "not-json"),
            (404, r#"{"app":"audiobookshelf"}"#),
        ] {
            let mut server = Server::new_async().await;
            let status = server
                .mock("GET", "/status")
                .with_status(status_code)
                .with_header("content-type", "application/json")
                .with_body(body)
                .expect(1)
                .create_async()
                .await;
            let subsonic = server
                .mock("GET", "/rest/ping.view")
                .match_query(Matcher::Any)
                .with_status(200)
                .with_header("content-type", "application/json")
                .with_body(r#"{"subsonic-response":{"status":"ok","version":"1.16.1"}}"#)
                .expect(1)
                .create_async()
                .await;

            assert_eq!(probe_url(&server.url()).await, ServerType::Subsonic);
            status.assert_async().await;
            subsonic.assert_async().await;
        }
    }

    // The common wire contract is documented by Jellyfin PRs 7080 (ApiKey,
    // shipped in 10.8) and 13306 (MediaBrowser Authorization + ApiKey remain
    // accepted with legacy auth disabled in 10.11). These are protocol mocks,
    // not executions of the historical servers. No version branching is needed.
    #[tokio::test]
    async fn jellyfin_common_auth_contract_10_8_through_12() {
        let _guard = crate::api::credential_test_lock();
        for version in ["10.8.13", "10.9.11", "10.10.7", "10.11.0", "12.0.0"] {
            let mut server = Server::new_async().await;
            let token = "jellyfin-token-12345";
            let authorization = format!("MediaBrowser Token=\"{token}\"");
            let login = server.mock("POST", "/Users/AuthenticateByName")
                .match_header("Authorization", Matcher::Regex(r#"^MediaBrowser Client="HifiMule", Device=".*", DeviceId=".*", Version=".*"$"#.into()))
                .with_status(200).with_body(r#"{"AccessToken":"jellyfin-token-12345","User":{"Id":"user1","Name":"Alexis"}}"#)
                .expect(1).create_async().await;
            let info = server.mock("GET", "/System/Info")
                .match_header("Authorization", authorization.as_str())
                .match_header("X-Emby-Token", Matcher::Missing)
                .with_status(200).with_body(serde_json::json!({"ServerName":"Jellyfin", "Version":version,"Id":"stable-id"}).to_string())
                .expect(2).create_async().await;
            let browse = server.mock("GET", "/UserViews")
                .match_query(Matcher::UrlEncoded("userId".into(), "user1".into()))
                .match_header("Authorization", authorization.as_str())
                .with_status(200).with_body(r#"{"Items":[{"Id":"music","Name":"Music","Type":"CollectionFolder","CollectionType":"music"}],"TotalRecordCount":1}"#)
                .expect(2).create_async().await;
            let download = server
                .mock("GET", "/Items/song1/Download")
                .match_query(Matcher::UrlEncoded("ApiKey".into(), token.into()))
                .with_status(200)
                .with_body("audio bytes")
                .expect(2)
                .create_async()
                .await;
            let fresh = connect(
                &server.url(),
                &password_credentials(server.url()),
                ServerTypeHint::Jellyfin,
            )
            .await
            .expect("fresh login");
            assert_eq!(fresh.server_version(), Some(version));
            assert_eq!(fresh.server_reported_id(), Some("stable-id"));
            assert_eq!(fresh.server_type(), ServerType::Jellyfin);
            // Stored sessions reconstruct a provider directly; the password-only
            // connection factory is intentionally not used for reconnect.
            let client = crate::api::JellyfinClient::new();
            let metadata = client
                .test_connection(&server.url(), token)
                .await
                .expect("stored token");
            assert_eq!(metadata.version, version);
            let stored = jellyfin::JellyfinProvider::new(client, server.url(), token, "user1");
            for provider in [fresh.as_ref(), &stored as &dyn MediaProvider] {
                assert_eq!(provider.list_libraries().await.expect("browse").len(), 1);
                let url = provider
                    .download_url("song1", None)
                    .await
                    .expect("download URL");
                let response = reqwest::get(url).await.expect("download");
                assert_eq!(response.status(), 200);
                assert_eq!(response.text().await.unwrap(), "audio bytes");
            }
            login.assert_async().await;
            info.assert_async().await;
            browse.assert_async().await;
            download.assert_async().await;
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
    async fn factory_auto_falls_back_to_jellyfin_12_after_successful_login() {
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
            .match_header(
                "Authorization",
                "MediaBrowser Token=\"jellyfin-token-12345\"",
            )
            .match_header("X-Emby-Token", Matcher::Missing)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"ServerName":"Jellyfin","Version":"12.0.0","Id":"server1"}"#)
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
        assert_eq!(provider.server_version(), Some("12.0.0"));
        assert_eq!(provider.server_reported_id(), Some("server1"));
        _ping.assert_async().await;
        _auth.assert_async().await;
        _info.assert_async().await;
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
            .match_header(
                "Authorization",
                format!("MediaBrowser Token=\"{}\"", "jellyfin-token-12345").as_str(),
            )
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
            sanitize_secret_message("error ?ApiKey=secret&api_key=old&format=mp3"),
            "error ?ApiKey=[redacted]&api_key=[redacted]&format=mp3"
        );
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

    #[test]
    fn sanitize_secret_message_removes_urls_titles_and_playback_identifiers() {
        let sanitized = sanitize_secret_message(
            "GET https://music.example/Items/server-secret?token=credential title=private-title trackId=track-secret serverId=server-secret",
        );
        assert!(sanitized.contains("[redacted-url]"));
        for private in [
            "https://",
            "music.example",
            "credential",
            "private-title",
            "track-secret",
            "server-secret",
        ] {
            assert!(
                !sanitized.contains(private),
                "leaked {private}: {sanitized}"
            );
        }
    }

    #[test]
    fn sanitize_secret_message_removes_mixed_case_urls() {
        let sanitized = sanitize_secret_message("GET HTTPS://music.example/private?token=secret");
        assert_eq!(sanitized, "GET [redacted-url]");
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

    #[test]
    fn browse_mode_serializes_to_camel_case_wire_values() {
        assert_eq!(
            serde_json::to_string(&BrowseMode::Artists).unwrap(),
            "\"artists\""
        );
        assert_eq!(
            serde_json::to_string(&BrowseMode::Albums).unwrap(),
            "\"albums\""
        );
        assert_eq!(
            serde_json::to_string(&BrowseMode::Playlists).unwrap(),
            "\"playlists\""
        );
        assert_eq!(
            serde_json::to_string(&BrowseMode::Tracks).unwrap(),
            "\"tracks\""
        );
        assert_eq!(
            serde_json::to_string(&BrowseMode::Genres).unwrap(),
            "\"genres\""
        );
        assert_eq!(
            serde_json::to_string(&BrowseMode::RecentlyAdded).unwrap(),
            "\"recentlyAdded\""
        );
        assert_eq!(
            serde_json::to_string(&BrowseMode::FrequentlyPlayed).unwrap(),
            "\"frequentlyPlayed\""
        );
        assert_eq!(
            serde_json::to_string(&BrowseMode::RecentlyPlayed).unwrap(),
            "\"recentlyPlayed\""
        );
        assert_eq!(
            serde_json::to_string(&BrowseMode::Favorites).unwrap(),
            "\"favorites\""
        );
    }

    #[test]
    fn browse_capabilities_preserves_mode_list_order_in_json() {
        let caps = BrowseCapabilities {
            list_modes: vec![
                BrowseMode::Artists,
                BrowseMode::Albums,
                BrowseMode::Playlists,
            ],
        };
        let json = serde_json::to_value(&caps).unwrap();
        let modes = json["listModes"].as_array().unwrap();
        assert_eq!(modes[0], "artists");
        assert_eq!(modes[1], "albums");
        assert_eq!(modes[2], "playlists");
    }

    struct MinimalProvider;

    #[async_trait]
    impl MediaProvider for MinimalProvider {
        fn server_type(&self) -> ServerType {
            ServerType::Unknown
        }

        fn capabilities(&self) -> Capabilities {
            Capabilities {
                open_subsonic: false,
                supports_changes_since: false,
                supports_server_transcoding: false,
                supports_playlist_write: false,
                browse: BrowseCapabilities { list_modes: vec![] },
            }
        }

        async fn list_libraries(
            &self,
        ) -> Result<Vec<crate::domain::models::Library>, ProviderError> {
            Err(ProviderError::UnsupportedCapability(
                "list_libraries".to_string(),
            ))
        }

        async fn list_artists(
            &self,
            _library_id: Option<&str>,
            _letter: Option<&str>,
            _offset: u32,
            _limit: u32,
        ) -> Result<(Vec<crate::domain::models::Artist>, u32), ProviderError> {
            Err(ProviderError::UnsupportedCapability(
                "list_artists".to_string(),
            ))
        }

        async fn get_artist(
            &self,
            _artist_id: &str,
        ) -> Result<crate::domain::models::ArtistWithAlbums, ProviderError> {
            Err(ProviderError::UnsupportedCapability(
                "get_artist".to_string(),
            ))
        }

        async fn list_albums(
            &self,
            _library_id: Option<&str>,
            _letter: Option<&str>,
            _offset: u32,
            _limit: u32,
        ) -> Result<(Vec<crate::domain::models::Album>, u32), ProviderError> {
            Err(ProviderError::UnsupportedCapability(
                "list_albums".to_string(),
            ))
        }

        async fn get_album(
            &self,
            _album_id: &str,
        ) -> Result<crate::domain::models::AlbumWithTracks, ProviderError> {
            Err(ProviderError::UnsupportedCapability(
                "get_album".to_string(),
            ))
        }

        async fn list_playlists(
            &self,
        ) -> Result<Vec<crate::domain::models::Playlist>, ProviderError> {
            Err(ProviderError::UnsupportedCapability(
                "list_playlists".to_string(),
            ))
        }

        async fn get_playlist(
            &self,
            _playlist_id: &str,
        ) -> Result<crate::domain::models::PlaylistWithTracks, ProviderError> {
            Err(ProviderError::UnsupportedCapability(
                "get_playlist".to_string(),
            ))
        }

        async fn search(
            &self,
            _query: &str,
        ) -> Result<crate::domain::models::SearchResult, ProviderError> {
            Err(ProviderError::UnsupportedCapability("search".to_string()))
        }

        async fn download_url(
            &self,
            _song_id: &str,
            _profile: Option<&TranscodeProfile>,
        ) -> Result<String, ProviderError> {
            Err(ProviderError::UnsupportedCapability(
                "download_url".to_string(),
            ))
        }

        async fn cover_art_url(&self, _cover_art_id: &str) -> Result<String, ProviderError> {
            Err(ProviderError::UnsupportedCapability(
                "cover_art_url".to_string(),
            ))
        }

        async fn changes_since_with_context(
            &self,
            _token: Option<&str>,
            _context: &ProviderChangeContext,
        ) -> Result<Vec<crate::domain::models::ChangeEvent>, ProviderError> {
            Err(ProviderError::UnsupportedCapability(
                "changes_since_with_context".to_string(),
            ))
        }

        async fn scrobble(&self, _request: ScrobbleRequest) -> Result<(), ProviderError> {
            Err(ProviderError::UnsupportedCapability("scrobble".to_string()))
        }
    }

    #[tokio::test]
    async fn trait_default_create_playlist_returns_unsupported() {
        let provider = MinimalProvider;
        let result = provider.create_playlist("My Playlist", &[]).await;
        let Err(ProviderError::UnsupportedCapability(msg)) = result else {
            panic!("expected UnsupportedCapability, got {result:?}");
        };
        assert!(
            msg.contains("create_playlist"),
            "message should name the method: {msg}"
        );
    }

    #[tokio::test]
    async fn trait_default_add_to_playlist_returns_unsupported() {
        let provider = MinimalProvider;
        let result = provider.add_to_playlist("playlist-1", &[]).await;
        let Err(ProviderError::UnsupportedCapability(msg)) = result else {
            panic!("expected UnsupportedCapability, got {result:?}");
        };
        assert!(
            msg.contains("add_to_playlist"),
            "message should name the method: {msg}"
        );
    }

    #[tokio::test]
    async fn trait_default_remove_from_playlist_returns_unsupported() {
        let provider = MinimalProvider;
        let result = provider.remove_from_playlist("playlist-1", &[]).await;
        let Err(ProviderError::UnsupportedCapability(msg)) = result else {
            panic!("expected UnsupportedCapability, got {result:?}");
        };
        assert!(
            msg.contains("remove_from_playlist"),
            "message should name the method: {msg}"
        );
    }

    #[tokio::test]
    async fn trait_default_delete_playlist_returns_unsupported() {
        let provider = MinimalProvider;
        let result = provider.delete_playlist("playlist-1").await;
        let Err(ProviderError::UnsupportedCapability(msg)) = result else {
            panic!("expected UnsupportedCapability, got {result:?}");
        };
        assert!(
            msg.contains("delete_playlist"),
            "message should name the method: {msg}"
        );
    }

    #[tokio::test]
    async fn trait_default_rename_playlist_returns_unsupported() {
        let provider = MinimalProvider;
        let result = provider.rename_playlist("playlist-1", "New Name").await;
        let Err(ProviderError::UnsupportedCapability(msg)) = result else {
            panic!("expected UnsupportedCapability, got {result:?}");
        };
        assert!(
            msg.contains("rename_playlist"),
            "message should name the method: {msg}"
        );
    }

    #[tokio::test]
    async fn trait_default_reorder_playlist_returns_unsupported() {
        let provider = MinimalProvider;
        let result = provider
            .reorder_playlist("playlist-1", &["a".to_string(), "b".to_string()])
            .await;
        let Err(ProviderError::UnsupportedCapability(msg)) = result else {
            panic!("expected UnsupportedCapability, got {result:?}");
        };
        assert!(
            msg.contains("reorder_playlist"),
            "message should name the method: {msg}"
        );
    }

    #[tokio::test]
    async fn trait_default_playback_resolution_is_explicitly_unsupported() {
        let result = MinimalProvider.resolve_playback("song-1").await;
        let Err(ProviderError::UnsupportedCapability(message)) = result else {
            panic!("expected UnsupportedCapability, got {result:?}");
        };
        assert!(message.contains("resolve_playback"));
    }

    #[test]
    fn playback_request_debug_redacts_headers_and_query_credentials() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::from_static("Bearer very-secret"),
        );
        let request = PlaybackRequest {
            url: reqwest::Url::parse("https://music.example/stream/song?token=also-secret")
                .unwrap(),
            headers,
            range_supported: true,
            cleanup: None,
            refresh: None,
            expected_content_type: None,
        };
        let debug = format!("{request:?}");
        assert!(debug.contains("[redacted]"));
        assert!(!debug.contains("very-secret"));
        assert!(!debug.contains("also-secret"));
        assert!(!debug.contains("token="));
    }

    #[test]
    fn playback_cleanup_runs_once_after_last_request_owner_retires() {
        let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed = count.clone();
        let request = PlaybackRequest {
            url: reqwest::Url::parse("https://music.example/private/session").unwrap(),
            headers: reqwest::header::HeaderMap::new(),
            range_supported: false,
            cleanup: Some(Arc::new(PlaybackCleanup::new(move || {
                observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }))),
            refresh: None,
            expected_content_type: None,
        };
        let stale = request.clone();
        drop(request);
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 0);
        drop(stale);
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn shutdown_drain_waits_for_cleanup_registered_while_draining() {
        let (release_first, first) = tokio::sync::oneshot::channel::<()>();
        register_playback_cleanup(tokio::spawn(async move { first.await.is_ok() }));
        let drain = tokio::spawn(drain_playback_cleanups());
        tokio::task::yield_now().await;
        let (release_second, second) = tokio::sync::oneshot::channel::<()>();
        register_playback_cleanup(tokio::spawn(async move { second.await.is_ok() }));
        release_first.send(()).unwrap();
        tokio::task::yield_now().await;
        assert!(!drain.is_finished());
        release_second.send(()).unwrap();
        assert!(drain.await.unwrap());
    }

    fn representation(
        codec: &str,
        provenance: PlaybackProvenance,
        bitrate_kbps: Option<u32>,
        sample_rate: Option<u32>,
        bit_depth: Option<u8>,
    ) -> PlaybackRepresentation {
        PlaybackRepresentation {
            codec: Some(codec.into()),
            container: None,
            bitrate_kbps,
            sample_rate,
            bit_depth,
            provenance,
            seek_mechanism: None,
            request: PlaybackRequest {
                url: reqwest::Url::parse(&format!("https://music.example/{codec}")).unwrap(),
                headers: reqwest::header::HeaderMap::new(),
                range_supported: false,
                cleanup: None,
                refresh: None,
                expected_content_type: None,
            },
        }
    }

    #[test]
    fn sync_media_requires_one_verified_original_representation() {
        let mut direct = representation("mp3", PlaybackProvenance::Original, None, None, None);
        direct.container = Some("mp3".into());
        direct.request.expected_content_type = Some("audio/mpeg".into());
        let admitted = SyncMediaRepresentation::from_playback_representations(vec![direct.clone()])
            .expect("one original representation");
        assert_eq!(admitted.codec, "mp3");
        assert_eq!(admitted.container, "mp3");
        assert!(
            SyncMediaRepresentation::from_playback_representations(vec![direct.clone(), direct,])
                .is_err()
        );
        let alternative = representation("mp3", PlaybackProvenance::Alternative, None, None, None);
        assert!(SyncMediaRepresentation::from_playback_representations(vec![alternative]).is_err());
    }

    #[test]
    fn new_audio_labels_preserve_original_preference() {
        for label in [
            "aif",
            "aiff",
            "ogg",
            "oga",
            "vorbis",
            "opus",
            "wma",
            "asf",
            "wmav1",
            "wmav2",
            "wmapro",
            "wmalossless",
            "pcm_s16be",
            "pcm_s24be",
            "pcm_s32be",
        ] {
            for codec in [label.to_owned(), label.to_ascii_uppercase()] {
                let selected = select_playback_representation(vec![
                    representation(
                        "flac",
                        PlaybackProvenance::Alternative,
                        None,
                        Some(96_000),
                        Some(24),
                    ),
                    representation(&codec, PlaybackProvenance::Original, None, None, None),
                ])
                .unwrap();
                assert_eq!(selected.codec.as_deref(), Some(codec.as_str()));
                assert_eq!(selected.provenance, PlaybackProvenance::Original);
            }
        }
        assert!(
            select_playback_representation(vec![representation(
                "h264",
                PlaybackProvenance::Original,
                None,
                None,
                None
            )])
            .is_err()
        );
    }

    #[test]
    fn big_endian_pcm_alternatives_use_lossless_quality_ranking() {
        for codec in [
            "pcm_s16be",
            "pcm_s24be",
            "pcm_s32be",
            "AIFF",
            "AIF",
            "wmalossless",
            "WMALOSSLESS",
        ] {
            let selected = select_playback_representation(vec![
                representation(
                    "aac",
                    PlaybackProvenance::Alternative,
                    Some(320),
                    None,
                    None,
                ),
                representation(
                    codec,
                    PlaybackProvenance::Alternative,
                    None,
                    Some(96_000),
                    Some(24),
                ),
                representation(
                    "flac",
                    PlaybackProvenance::Alternative,
                    None,
                    Some(48_000),
                    Some(24),
                ),
            ])
            .unwrap();
            assert_eq!(selected.codec.as_deref(), Some(codec));
        }
    }

    #[test]
    fn wav_original_representation_is_supported() {
        let selected = select_playback_representation(vec![representation(
            "wav",
            PlaybackProvenance::Original,
            Some(1_536),
            Some(48_000),
            Some(16),
        )])
        .expect("a WAV container is playable by the production decoder");

        assert_eq!(selected.codec.as_deref(), Some("wav"));
    }

    #[test]
    fn m4r_original_representation_is_supported() {
        for codec in ["m4r", "M4R"] {
            let selected = select_playback_representation(vec![
                representation(
                    "flac",
                    PlaybackProvenance::Alternative,
                    None,
                    Some(96_000),
                    Some(24),
                ),
                representation(
                    codec,
                    PlaybackProvenance::Original,
                    Some(256),
                    Some(44_100),
                    None,
                ),
            ])
            .expect("an original M4R container is playable by the production decoder");

            assert_eq!(selected.codec.as_deref(), Some(codec));
            assert_eq!(selected.provenance, PlaybackProvenance::Original);
        }
    }

    #[test]
    fn playback_representation_ranking_is_bounded_and_deterministic() {
        let selected = select_playback_representation(vec![representation(
            "m4a",
            PlaybackProvenance::Original,
            Some(256),
            Some(44_100),
            None,
        )])
        .expect("an original M4A container is playable by the production decoder");
        assert_eq!(selected.codec.as_deref(), Some("m4a"));

        let selected = select_playback_representation(vec![
            representation(
                "aac",
                PlaybackProvenance::Alternative,
                Some(320),
                None,
                None,
            ),
            representation(
                "flac",
                PlaybackProvenance::Alternative,
                None,
                Some(96_000),
                Some(24),
            ),
            representation("mp3", PlaybackProvenance::Original, Some(192), None, None),
        ])
        .unwrap();
        assert_eq!(selected.codec.as_deref(), Some("mp3"));

        let selected = select_playback_representation(vec![
            representation(
                "flac",
                PlaybackProvenance::Alternative,
                None,
                Some(48_000),
                Some(24),
            ),
            representation(
                "flac",
                PlaybackProvenance::Alternative,
                None,
                Some(96_000),
                Some(24),
            ),
        ])
        .unwrap();
        assert_eq!(selected.sample_rate, Some(96_000));

        let too_many = (0..=MAX_PLAYBACK_REPRESENTATIONS)
            .map(|_| representation("aac", PlaybackProvenance::Alternative, None, None, None))
            .collect();
        assert!(matches!(
            select_playback_representation(too_many),
            Err(ProviderError::UnsupportedCapability(_))
        ));
        assert!(matches!(
            select_playback_representation(vec![representation(
                "h264",
                PlaybackProvenance::Original,
                None,
                None,
                None,
            )]),
            Err(ProviderError::UnsupportedCapability(_))
        ));
    }
}
