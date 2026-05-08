use crate::domain::models::{
    Album, AlbumWithTracks, Artist, ArtistWithAlbums, ChangeEvent, Library, Playlist,
    PlaylistWithTracks, SearchResult,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

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

    async fn changes_since(&self, token: Option<&str>) -> Result<Vec<ChangeEvent>, ProviderError>;

    async fn scrobble(&self, request: ScrobbleRequest) -> Result<(), ProviderError>;

    fn server_type(&self) -> ServerType;

    fn capabilities(&self) -> Capabilities;
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderCredentials {
    pub server_url: String,
    pub username: Option<String>,
    pub token: Option<String>,
    pub password: Option<String>,
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
