use crate::domain::models::{
    Album, AlbumWithTracks, Artist, ArtistWithAlbums, ChangeEvent, ChangeType, ItemRef, ItemType,
    Kbps, Library, Playlist, PlaylistWithTracks, SearchResult, Seconds, Song,
};
use crate::providers::{
    Capabilities, CredentialKind, MediaProvider, ProviderChangeContext, ProviderChangeMetadata,
    ProviderCredentials, ProviderError, ProviderSyncedSong, ScrobbleRequest, ScrobbleSubmission,
    ServerType, TranscodeProfile, SUBSONIC_PLAYLISTS_LIBRARY_ID,
};
use async_trait::async_trait;
use md5::{Digest, Md5};
use reqwest::Url;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use std::{collections::HashMap, fmt};
use uuid::Uuid;

const CLIENT_NAME: &str = "hifimule";
const API_VERSION: &str = "1.16.1";
const REDACTED: &str = "[REDACTED]";
const SUBSONIC_SECRET_QUERY_KEYS: &[&str] = &["password", "u", "p", "t", "s"];
const SONG_CHANGE_PAGE_SIZE: usize = 500;
#[cfg(not(test))]
const MAX_SONG_DUMP_PAGES: usize = 2000;
#[cfg(test)]
const MAX_SONG_DUMP_PAGES: usize = 2;

#[derive(Clone)]
pub struct SubsonicProvider {
    client: SubsonicClient,
    open_subsonic: bool,
    server_version: Option<String>,
}

impl fmt::Debug for SubsonicProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SubsonicProvider")
            .field("client", &self.client)
            .field("open_subsonic", &self.open_subsonic)
            .field("server_version", &self.server_version)
            .finish()
    }
}

impl SubsonicProvider {
    pub async fn connect(credentials: ProviderCredentials) -> Result<Self, ProviderError> {
        let client = SubsonicClient::from_credentials(credentials)?;
        let ping = client.ping().await?;

        Ok(Self {
            client,
            open_subsonic: ping.open_subsonic,
            server_version: ping.server_version,
        })
    }

    pub fn from_stored_config(
        credentials: ProviderCredentials,
        open_subsonic: bool,
        server_version: Option<String>,
    ) -> Result<Self, ProviderError> {
        let client = SubsonicClient::from_credentials(credentials)?;
        Ok(Self {
            client,
            open_subsonic,
            server_version,
        })
    }

    #[cfg(test)]
    fn from_client_for_tests(client: SubsonicClient, open_subsonic: bool) -> Self {
        Self {
            client,
            open_subsonic,
            server_version: None,
        }
    }

    async fn full_song_dump_changes(&self) -> Result<Vec<ChangeEvent>, ProviderError> {
        let mut changes = Vec::new();
        let mut offset = 0usize;
        let mut pages_fetched = 0usize;
        loop {
            let page = self
                .client
                .search3_paged("", Some(SONG_CHANGE_PAGE_SIZE), Some(offset))
                .await?;
            let songs = page.search_result3.song;
            let count = songs.len();
            changes.extend(songs.into_iter().map(song_created_event));
            pages_fetched += 1;
            if count < SONG_CHANGE_PAGE_SIZE {
                break;
            }
            if pages_fetched >= MAX_SONG_DUMP_PAGES {
                return Err(ProviderError::UnsupportedCapability(format!(
                    "Subsonic full-library dump exceeded {MAX_SONG_DUMP_PAGES} pages without a partial page"
                )));
            }
            offset += SONG_CHANGE_PAGE_SIZE;
        }
        Ok(changes)
    }

    async fn album_fallback_changes(
        &self,
        context: &ProviderChangeContext,
    ) -> Result<Vec<ChangeEvent>, ProviderError> {
        let mut by_album: HashMap<String, Vec<&ProviderSyncedSong>> = HashMap::new();
        for song in &context.synced_songs {
            if let Some(album_id) = song.album_id.as_deref().filter(|id| !id.is_empty()) {
                by_album.entry(album_id.to_string()).or_default().push(song);
            }
        }

        let mut changes = Vec::new();
        let mut albums: Vec<_> = by_album.into_iter().collect();
        albums.sort_by(|left, right| left.0.cmp(&right.0));
        for (album_id, expected) in albums {
            let album = self.client.get_album(&album_id).await?;
            changes.extend(album_song_changes(&expected, &album.album.song));
        }
        Ok(changes)
    }

    fn change_metadata_from_version(version: Option<&str>) -> Option<ProviderChangeMetadata> {
        let version = version?;
        let payload = version.strip_prefix("subsonic:")?;
        let parts: Vec<&str> = payload.splitn(5, '|').collect();
        if parts.len() != 5 {
            return None;
        }
        let clean = |value: &str| {
            if value.is_empty() {
                None
            } else {
                Some(value.to_string())
            }
        };
        Some(ProviderChangeMetadata {
            album_id: clean(parts[1]),
            size: parts[2].parse::<u64>().ok(),
            content_type: clean(parts[3]),
            suffix: clean(parts[4]),
        })
    }
}

#[async_trait]
impl MediaProvider for SubsonicProvider {
    async fn list_libraries(&self) -> Result<Vec<Library>, ProviderError> {
        Ok(vec![
            Library {
                id: "all".to_string(),
                name: "All Music".to_string(),
                item_type: ItemType::Library,
                cover_art_id: None,
            },
            Library {
                id: SUBSONIC_PLAYLISTS_LIBRARY_ID.to_string(),
                name: "Playlists".to_string(),
                item_type: ItemType::Library,
                cover_art_id: None,
            },
        ])
    }

    async fn list_artists(&self, _library_id: Option<&str>) -> Result<Vec<Artist>, ProviderError> {
        let artists = self.client.get_artists().await?;
        Ok(artists
            .artists
            .index
            .into_iter()
            .flat_map(|index| index.artist)
            .map(artist_from_dto)
            .collect())
    }

    async fn get_artist(&self, artist_id: &str) -> Result<ArtistWithAlbums, ProviderError> {
        let artist = self.client.get_artist(artist_id).await?;
        let albums = artist
            .artist
            .album
            .iter()
            .cloned()
            .map(album_from_dto)
            .collect();

        Ok(ArtistWithAlbums {
            artist: artist_from_with_albums_dto(artist.artist),
            albums,
        })
    }

    async fn list_albums(&self, _library_id: Option<&str>) -> Result<Vec<Album>, ProviderError> {
        let albums = self.client.get_album_list2().await?;
        Ok(albums
            .album_list2
            .album
            .into_iter()
            .map(album_from_dto)
            .collect())
    }

    async fn get_album(&self, album_id: &str) -> Result<AlbumWithTracks, ProviderError> {
        let album = self.client.get_album(album_id).await?;
        let tracks = album
            .album
            .song
            .iter()
            .cloned()
            .map(song_from_dto)
            .collect();

        Ok(AlbumWithTracks {
            album: album_from_with_songs_dto(album.album),
            tracks,
        })
    }

    async fn list_playlists(&self) -> Result<Vec<Playlist>, ProviderError> {
        let playlists = self.client.get_playlists().await?;
        Ok(playlists
            .playlists
            .playlist
            .into_iter()
            .map(playlist_from_dto)
            .collect())
    }

    async fn get_playlist(&self, playlist_id: &str) -> Result<PlaylistWithTracks, ProviderError> {
        let playlist = self.client.get_playlist(playlist_id).await?;
        let tracks = playlist
            .playlist
            .entry
            .iter()
            .cloned()
            .map(song_from_dto)
            .collect();

        Ok(PlaylistWithTracks {
            playlist: playlist_from_with_songs_dto(playlist.playlist),
            tracks,
        })
    }

    async fn search(&self, query: &str) -> Result<SearchResult, ProviderError> {
        let result = self.client.search3(query).await?.search_result3;

        Ok(SearchResult {
            artists: result.artist.into_iter().map(artist_from_dto).collect(),
            albums: result.album.into_iter().map(album_from_dto).collect(),
            songs: result.song.into_iter().map(song_from_dto).collect(),
            playlists: result.playlist.into_iter().map(playlist_from_dto).collect(),
        })
    }

    async fn download_url(
        &self,
        song_id: &str,
        profile: Option<&TranscodeProfile>,
    ) -> Result<String, ProviderError> {
        match profile {
            Some(profile) => self.client.stream_url(song_id, profile),
            None => self.client.download_url(song_id),
        }
    }

    async fn cover_art_url(&self, cover_art_id: &str) -> Result<String, ProviderError> {
        self.client.cover_art_url(cover_art_id)
    }

    async fn changes_since_with_context(
        &self,
        token: Option<&str>,
        context: &ProviderChangeContext,
    ) -> Result<Vec<ChangeEvent>, ProviderError> {
        let token = token.map(str::trim).filter(|token| !token.is_empty());
        if matches!(token, None | Some("0")) {
            return self.full_song_dump_changes().await;
        }

        let if_modified_since = match token {
            Some(token) => token.parse::<i64>().map_err(|_| {
                ProviderError::UnsupportedCapability(
                    "Subsonic changes_since token must be epoch milliseconds".to_string(),
                )
            })?,
            None => unreachable!("initial tokens are handled before numeric parsing"),
        };
        let indexes = self.client.get_indexes(Some(if_modified_since)).await?;
        let index_changes: Vec<ChangeEvent> = indexes
            .indexes
            .index
            .into_iter()
            .flat_map(|index| index.artist)
            .map(|artist| ChangeEvent {
                item: ItemRef {
                    id: artist.id,
                    item_type: ItemType::Artist,
                },
                change_type: ChangeType::Updated,
                version: None,
            })
            .collect();

        if index_changes.is_empty() {
            self.album_fallback_changes(context).await
        } else {
            Ok(index_changes)
        }
    }

    async fn scrobble(&self, request: ScrobbleRequest) -> Result<(), ProviderError> {
        match request.submission {
            ScrobbleSubmission::Played => {
                self.client.scrobble(&request.song_id, true).await?;
                Ok(())
            }
            ScrobbleSubmission::Playing => Err(ProviderError::UnsupportedCapability(
                "Subsonic now-playing scrobble is not implemented by this adapter".to_string(),
            )),
        }
    }

    fn change_metadata(&self, event: &ChangeEvent) -> Option<ProviderChangeMetadata> {
        Self::change_metadata_from_version(event.version.as_deref())
    }

    fn server_type(&self) -> ServerType {
        if self.open_subsonic {
            ServerType::OpenSubsonic
        } else {
            ServerType::Subsonic
        }
    }

    fn server_version(&self) -> Option<&str> {
        self.server_version.as_deref()
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            open_subsonic: self.open_subsonic,
            supports_changes_since: true,
            supports_server_transcoding: self.open_subsonic,
        }
    }
}

#[derive(Clone)]
struct SubsonicClient {
    http: reqwest::Client,
    server_url: Url,
    username: String,
    password: String,
}

impl fmt::Debug for SubsonicClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SubsonicClient")
            .field("server_url", &self.server_url.as_str())
            .field("username", &self.username)
            .field("password", &"[redacted]")
            .finish()
    }
}

impl SubsonicClient {
    fn from_credentials(credentials: ProviderCredentials) -> Result<Self, ProviderError> {
        match credentials.credential {
            CredentialKind::Password { username, password } => {
                Self::new(credentials.server_url, username, password)
            }
            CredentialKind::Token(_) => Err(ProviderError::UnsupportedCapability(
                "Subsonic token credentials are not supported; use username and password"
                    .to_string(),
            )),
        }
    }

    fn new(
        server_url: impl AsRef<str>,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> Result<Self, ProviderError> {
        let server_url = Url::parse(server_url.as_ref()).map_err(|error| {
            ProviderError::Auth(format!("invalid Subsonic server URL: {error}"))
        })?;

        Ok(Self {
            http: reqwest::Client::new(),
            server_url,
            username: username.into(),
            password: password.into(),
        })
    }

    async fn ping(&self) -> Result<PingResult, ProviderError> {
        let envelope: SubsonicEnvelope<NoBody> = self.get_envelope("ping", &[]).await?;
        Ok(PingResult {
            open_subsonic: envelope.response.open_subsonic.unwrap_or(false),
            server_version: envelope.response.server_version,
        })
    }

    async fn get_artists(&self) -> Result<ArtistsBody, ProviderError> {
        self.get("getArtists", &[]).await
    }

    async fn get_artist(&self, id: &str) -> Result<ArtistWithAlbumsBody, ProviderError> {
        self.get("getArtist", &[("id", id)]).await
    }

    async fn get_album_list2(&self) -> Result<AlbumList2Body, ProviderError> {
        const PAGE_SIZE: usize = 500;
        let mut all_albums = Vec::new();
        let mut offset = 0usize;
        loop {
            let size_str = PAGE_SIZE.to_string();
            let offset_str = offset.to_string();
            let page: AlbumList2Body = self
                .get(
                    "getAlbumList2",
                    &[
                        ("type", "alphabeticalByName"),
                        ("size", &size_str),
                        ("offset", &offset_str),
                    ],
                )
                .await?;
            let count = page.album_list2.album.len();
            all_albums.extend(page.album_list2.album);
            if count < PAGE_SIZE {
                break;
            }
            offset += PAGE_SIZE;
        }
        Ok(AlbumList2Body {
            album_list2: AlbumListDto { album: all_albums },
        })
    }

    async fn get_album(&self, id: &str) -> Result<AlbumWithSongsBody, ProviderError> {
        self.get("getAlbum", &[("id", id)]).await
    }

    async fn get_playlists(&self) -> Result<PlaylistsBody, ProviderError> {
        self.get("getPlaylists", &[]).await
    }

    async fn get_playlist(&self, id: &str) -> Result<PlaylistWithSongsBody, ProviderError> {
        self.get("getPlaylist", &[("id", id)]).await
    }

    async fn search3(&self, query: &str) -> Result<Search3Body, ProviderError> {
        self.search3_paged(query, None, None).await
    }

    async fn search3_paged(
        &self,
        query: &str,
        song_count: Option<usize>,
        song_offset: Option<usize>,
    ) -> Result<Search3Body, ProviderError> {
        let song_count = song_count.map(|value| value.to_string());
        let song_offset = song_offset.map(|value| value.to_string());
        let mut params = vec![("query", query)];
        if let Some(value) = song_count.as_deref() {
            params.push(("songCount", value));
        }
        if let Some(value) = song_offset.as_deref() {
            params.push(("songOffset", value));
        }
        self.get("search3", &params).await
    }

    async fn get_indexes(
        &self,
        if_modified_since: Option<i64>,
    ) -> Result<IndexesBody, ProviderError> {
        let if_modified_since = if_modified_since.map(|value| value.to_string());
        let mut params = Vec::new();
        if let Some(value) = if_modified_since.as_deref() {
            params.push(("ifModifiedSince", value));
        }
        self.get("getIndexes", &params).await
    }

    async fn scrobble(&self, id: &str, submission: bool) -> Result<(), ProviderError> {
        let submission = if submission { "true" } else { "false" };
        let _: NoBody = self
            .get("scrobble", &[("id", id), ("submission", submission)])
            .await?;
        Ok(())
    }

    fn download_url(&self, id: &str) -> Result<String, ProviderError> {
        self.signed_url("download", &[("id", id)])
    }

    fn stream_url(&self, id: &str, profile: &TranscodeProfile) -> Result<String, ProviderError> {
        let format = profile.container.as_deref().unwrap_or("mp3");
        let max_bit_rate = profile.max_bitrate_kbps.map(|kbps| kbps.to_string());
        let mut params = vec![("id", id), ("format", format)];
        if let Some(value) = max_bit_rate.as_deref() {
            params.push(("maxBitRate", value));
        }
        self.signed_url("stream", &params)
    }

    fn cover_art_url(&self, id: &str) -> Result<String, ProviderError> {
        self.signed_url("getCoverArt", &[("id", id)])
    }

    async fn get<T: DeserializeOwned + Default>(
        &self,
        endpoint: &str,
        params: &[(&str, &str)],
    ) -> Result<T, ProviderError> {
        let url = self.signed_url(endpoint, params)?;
        let envelope: SubsonicEnvelope<T> = self.get_envelope_url(url).await?;
        Ok(envelope.response.body)
    }

    async fn get_envelope<T: DeserializeOwned + Default>(
        &self,
        endpoint: &str,
        params: &[(&str, &str)],
    ) -> Result<SubsonicEnvelope<T>, ProviderError> {
        let url = self.signed_url(endpoint, params)?;
        self.get_envelope_url(url).await
    }

    async fn get_envelope_url<T: DeserializeOwned + Default>(
        &self,
        url: String,
    ) -> Result<SubsonicEnvelope<T>, ProviderError> {
        let response = self.http.get(url).send().await.map_err(map_reqwest_error)?;
        let status = response.status();
        let bytes = response.bytes().await.map_err(map_reqwest_error)?;

        if !status.is_success() {
            return Err(match status.as_u16() {
                401 | 403 => ProviderError::Auth(format!("Subsonic request failed with {status}")),
                404 => ProviderError::NotFound {
                    item_type: "item".to_string(),
                    id: "unknown".to_string(),
                },
                _ => ProviderError::Http {
                    status: Some(status.as_u16()),
                    message: format!("Subsonic request failed with {status}"),
                },
            });
        }

        let value: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|error| ProviderError::Deserialization(error.to_string()))?;
        if let Some(response) = value.get("subsonic-response") {
            let status = response
                .get("status")
                .and_then(|status| status.as_str())
                .unwrap_or_default();
            if status.eq_ignore_ascii_case("failed") {
                let error = response.get("error");
                let code = error
                    .and_then(|error| error.get("code"))
                    .and_then(|code| code.as_i64())
                    .and_then(|code| i32::try_from(code).ok());
                let message = error
                    .and_then(|error| error.get("message"))
                    .and_then(|message| message.as_str())
                    .map(sanitize_subsonic_message)
                    .unwrap_or_else(|| "Subsonic API request failed".to_string());
                return Err(match code {
                    Some(40 | 41) => ProviderError::Auth(message),
                    Some(70) => ProviderError::NotFound {
                        item_type: "item".to_string(),
                        id: "unknown".to_string(),
                    },
                    _ => ProviderError::Http {
                        status: None,
                        message,
                    },
                });
            }
        }

        let envelope: SubsonicEnvelope<T> = serde_json::from_value(value)
            .map_err(|error| ProviderError::Deserialization(error.to_string()))?;

        Ok(envelope)
    }

    fn signed_url(&self, endpoint: &str, params: &[(&str, &str)]) -> Result<String, ProviderError> {
        let mut url = self
            .server_url
            .join(&format!("rest/{endpoint}.view"))
            .map_err(|error| ProviderError::Other(error.into()))?;
        let salt = Uuid::new_v4().simple().to_string();
        let token = auth_token(&self.password, &salt);

        {
            let mut query = url.query_pairs_mut();
            query
                .append_pair("u", &self.username)
                .append_pair("t", &token)
                .append_pair("s", &salt)
                .append_pair("v", API_VERSION)
                .append_pair("c", CLIENT_NAME)
                .append_pair("f", "json");
            for (key, value) in params {
                query.append_pair(key, value);
            }
        }

        tracing::debug!(url = %sanitize_subsonic_url(&url), "Subsonic request");
        Ok(url.into())
    }
}

fn auth_token(password: &str, salt: &str) -> String {
    let mut hasher = Md5::new();
    hasher.update(password.as_bytes());
    hasher.update(salt.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn map_reqwest_error(error: reqwest::Error) -> ProviderError {
    if error.is_decode() {
        ProviderError::Deserialization(sanitize_subsonic_message(&error.to_string()))
    } else if let Some(status) = error.status() {
        match status.as_u16() {
            401 | 403 => ProviderError::Auth(sanitize_subsonic_message(&error.to_string())),
            404 => ProviderError::NotFound {
                item_type: "item".to_string(),
                id: "unknown".to_string(),
            },
            _ => ProviderError::Http {
                status: Some(status.as_u16()),
                message: sanitize_subsonic_message(&error.to_string()),
            },
        }
    } else {
        ProviderError::Http {
            status: None,
            message: sanitize_subsonic_message(&error.to_string()),
        }
    }
}

pub(crate) fn sanitize_subsonic_url(url: &Url) -> String {
    let Some(query) = url.query() else {
        return url.to_string();
    };
    let new_query = query
        .split('&')
        .map(|pair| match pair.split_once('=') {
            Some((key, _)) if SUBSONIC_SECRET_QUERY_KEYS.contains(&key) => {
                format!("{key}={REDACTED}")
            }
            _ => pair.to_string(),
        })
        .collect::<Vec<_>>()
        .join("&");
    let mut sanitized = url.clone();
    sanitized.set_query(Some(&new_query));
    sanitized.into()
}

pub(crate) fn sanitize_subsonic_message(message: &str) -> String {
    let mut sanitized = message.to_string();
    for key in SUBSONIC_SECRET_QUERY_KEYS {
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
            let value_start = start + needle.len();
            let value_end = sanitized[value_start..]
                .find(|ch: char| ch == '&' || ch.is_whitespace())
                .map(|offset| value_start + offset)
                .unwrap_or_else(|| sanitized.len());
            rebuilt.push_str(&sanitized[cursor..value_start]);
            rebuilt.push_str(REDACTED);
            cursor = value_end;
        }
        rebuilt.push_str(&sanitized[cursor..]);
        sanitized = rebuilt;
    }
    sanitized
}

fn artist_from_dto(artist: ArtistDto) -> Artist {
    Artist {
        id: artist.id,
        name: artist.name,
        album_count: non_negative_i32(artist.album_count),
        song_count: non_negative_i32(artist.song_count),
        cover_art_id: artist.cover_art,
    }
}

fn artist_from_with_albums_dto(artist: ArtistWithAlbumsDto) -> Artist {
    Artist {
        id: artist.id,
        name: artist.name,
        album_count: non_negative_i32(artist.album_count).or(Some(artist.album.len() as u32)),
        song_count: None,
        cover_art_id: artist.cover_art,
    }
}

fn album_from_dto(album: AlbumDto) -> Album {
    Album {
        id: album.id,
        title: album.name,
        artist_id: album.artist_id,
        artist_name: album.artist,
        year: non_negative_i32(album.year),
        song_count: non_negative_i32(album.song_count),
        duration_seconds: non_negative_i64(album.duration)
            .map(|seconds| u32::from(Seconds(seconds))),
        cover_art_id: album.cover_art,
    }
}

fn album_from_with_songs_dto(album: AlbumWithSongsDto) -> Album {
    Album {
        id: album.id,
        title: album.name,
        artist_id: album.artist_id,
        artist_name: album.artist,
        year: non_negative_i32(album.year),
        song_count: non_negative_i32(album.song_count).or(Some(album.song.len() as u32)),
        duration_seconds: non_negative_i64(album.duration)
            .map(|seconds| u32::from(Seconds(seconds))),
        cover_art_id: album.cover_art,
    }
}

fn playlist_from_dto(playlist: PlaylistDto) -> Playlist {
    Playlist {
        id: playlist.id,
        name: playlist.name,
        song_count: non_negative_i32(playlist.song_count),
        duration_seconds: non_negative_i32(playlist.duration),
        cover_art_id: playlist.cover_art,
    }
}

fn playlist_from_with_songs_dto(playlist: PlaylistWithSongsDto) -> Playlist {
    Playlist {
        id: playlist.id,
        name: playlist.name,
        song_count: non_negative_i32(playlist.song_count).or(Some(playlist.entry.len() as u32)),
        duration_seconds: non_negative_i32(playlist.duration),
        cover_art_id: playlist.cover_art,
    }
}

fn song_from_dto(song: SongDto) -> Song {
    let artist = song.artists.as_ref().and_then(|artists| artists.first());

    Song {
        id: song.id,
        title: song.title,
        artist_id: song
            .artist_id
            .or_else(|| artist.map(|artist| artist.id.clone())),
        artist_name: song
            .artist
            .or_else(|| artist.map(|artist| artist.name.clone())),
        album_id: song.album_id,
        album_title: song.album,
        duration_seconds: non_negative_i64(song.duration)
            .map(|seconds| u32::from(Seconds(seconds)))
            .unwrap_or_default(),
        bitrate_kbps: non_negative_i32(song.bit_rate).map(|kbps| u32::from(Kbps(kbps))),
        track_number: non_negative_i32(song.track),
        disc_number: non_negative_i32(song.disc_number),
        cover_art_id: song.cover_art,
    }
}

fn song_created_event(song: SongDto) -> ChangeEvent {
    ChangeEvent {
        item: ItemRef {
            id: song.id.clone(),
            item_type: ItemType::Song,
        },
        change_type: ChangeType::Created,
        version: subsonic_song_version(&song),
    }
}

fn song_metadata_changed(expected: &ProviderSyncedSong, actual: &SongDto) -> bool {
    let mut compared = false;
    let mut changed = false;

    if expected.size.is_some() && actual.size.is_some() {
        compared = true;
        changed |= expected.size != actual.size;
    }
    if expected.content_type.is_some() && actual.content_type.is_some() {
        compared = true;
        changed |= expected.content_type.as_deref() != actual.content_type.as_deref();
    }
    if expected.suffix.is_some() && actual.suffix.is_some() {
        compared = true;
        changed |= expected.suffix.as_deref() != actual.suffix.as_deref();
    }

    compared && changed
}

fn album_song_changes(expected: &[&ProviderSyncedSong], actual: &[SongDto]) -> Vec<ChangeEvent> {
    let expected_by_id: HashMap<&str, &ProviderSyncedSong> = expected
        .iter()
        .map(|song| (song.song_id.as_str(), *song))
        .collect();
    let actual_by_id: HashMap<&str, &SongDto> =
        actual.iter().map(|song| (song.id.as_str(), song)).collect();
    let mut changes = Vec::new();

    let mut actual_songs: Vec<&SongDto> = actual.iter().collect();
    actual_songs.sort_by(|left, right| left.id.cmp(&right.id));
    for song in actual_songs {
        match expected_by_id.get(song.id.as_str()) {
            None => changes.push(song_created_event(song.clone())),
            Some(expected) if song_metadata_changed(expected, song) => changes.push(ChangeEvent {
                item: ItemRef {
                    id: song.id.clone(),
                    item_type: ItemType::Song,
                },
                change_type: ChangeType::Updated,
                version: subsonic_song_version(song),
            }),
            Some(_) => {}
        }
    }

    let mut removed: Vec<&ProviderSyncedSong> = expected
        .iter()
        .copied()
        .filter(|song| !actual_by_id.contains_key(song.song_id.as_str()))
        .collect();
    removed.sort_by(|left, right| left.song_id.cmp(&right.song_id));
    for song in removed {
        changes.push(ChangeEvent {
            item: ItemRef {
                id: song.song_id.clone(),
                item_type: ItemType::Song,
            },
            change_type: ChangeType::Deleted,
            version: song.version.clone(),
        });
    }

    changes
}

fn subsonic_song_version(song: &SongDto) -> Option<String> {
    match (&song.album_id, &song.size, &song.content_type, &song.suffix) {
        (Some(album_id), Some(size), Some(content_type), Some(suffix)) => Some(format!(
            "subsonic:{}|{}|{}|{}|{}",
            song.id, album_id, size, content_type, suffix
        )),
        _ => None,
    }
}

fn non_negative_i32(value: Option<i32>) -> Option<u32> {
    value.and_then(|value| u32::try_from(value).ok())
}

fn non_negative_i64(value: Option<i64>) -> Option<u32> {
    value.and_then(|value| u32::try_from(value).ok())
}

#[derive(Debug, Deserialize)]
#[serde(bound(deserialize = "T: Deserialize<'de> + Default"))]
struct SubsonicEnvelope<T> {
    #[serde(rename = "subsonic-response")]
    response: SubsonicResponse<T>,
}

#[derive(Debug, Deserialize)]
#[serde(bound(deserialize = "T: Deserialize<'de> + Default"))]
struct SubsonicResponse<T> {
    status: String,
    #[serde(default, rename = "version")]
    server_version: Option<String>,
    #[serde(rename = "openSubsonic")]
    open_subsonic: Option<bool>,
    error: Option<ApiErrorDto>,
    #[serde(default)]
    #[serde(flatten)]
    body: T,
}

struct PingResult {
    open_subsonic: bool,
    server_version: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiErrorDto {
    code: Option<i32>,
    message: String,
}

#[derive(Debug, Default, Deserialize)]
struct NoBody {}

#[derive(Debug, Default, Deserialize)]
struct ArtistsBody {
    artists: ArtistsDto,
}

#[derive(Debug, Default, Deserialize)]
struct ArtistsDto {
    #[serde(default)]
    index: Vec<ArtistIndexDto>,
}

#[derive(Debug, Default, Deserialize)]
struct ArtistIndexDto {
    #[serde(default)]
    artist: Vec<ArtistDto>,
}

#[derive(Debug, Clone, Deserialize)]
struct ArtistDto {
    id: String,
    name: String,
    #[serde(rename = "albumCount")]
    album_count: Option<i32>,
    #[serde(rename = "songCount")]
    song_count: Option<i32>,
    #[serde(rename = "coverArt")]
    cover_art: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct ArtistWithAlbumsBody {
    artist: ArtistWithAlbumsDto,
}

#[derive(Debug, Default, Deserialize)]
struct ArtistWithAlbumsDto {
    id: String,
    name: String,
    #[serde(rename = "albumCount")]
    album_count: Option<i32>,
    #[serde(rename = "coverArt")]
    cover_art: Option<String>,
    #[serde(default)]
    album: Vec<AlbumDto>,
}

#[derive(Debug, Default, Deserialize)]
struct AlbumList2Body {
    #[serde(rename = "albumList2")]
    album_list2: AlbumListDto,
}

#[derive(Debug, Default, Deserialize)]
struct AlbumListDto {
    #[serde(default)]
    album: Vec<AlbumDto>,
}

#[derive(Debug, Clone, Deserialize)]
struct AlbumDto {
    id: String,
    name: String,
    artist: Option<String>,
    #[serde(rename = "artistId")]
    artist_id: Option<String>,
    year: Option<i32>,
    #[serde(rename = "songCount")]
    song_count: Option<i32>,
    duration: Option<i64>,
    #[serde(rename = "coverArt")]
    cover_art: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct AlbumWithSongsBody {
    album: AlbumWithSongsDto,
}

#[derive(Debug, Default, Deserialize)]
struct AlbumWithSongsDto {
    id: String,
    name: String,
    artist: Option<String>,
    #[serde(rename = "artistId")]
    artist_id: Option<String>,
    year: Option<i32>,
    #[serde(rename = "songCount")]
    song_count: Option<i32>,
    duration: Option<i64>,
    #[serde(rename = "coverArt")]
    cover_art: Option<String>,
    #[serde(default)]
    song: Vec<SongDto>,
}

#[derive(Debug, Default, Deserialize)]
struct PlaylistsBody {
    playlists: PlaylistsDto,
}

#[derive(Debug, Default, Deserialize)]
struct PlaylistsDto {
    #[serde(default)]
    playlist: Vec<PlaylistDto>,
}

#[derive(Debug, Clone, Deserialize)]
struct PlaylistDto {
    id: String,
    name: String,
    #[serde(rename = "songCount")]
    song_count: Option<i32>,
    duration: Option<i32>,
    #[serde(rename = "coverArt")]
    cover_art: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct PlaylistWithSongsBody {
    playlist: PlaylistWithSongsDto,
}

#[derive(Debug, Default, Deserialize)]
struct PlaylistWithSongsDto {
    id: String,
    name: String,
    #[serde(rename = "songCount")]
    song_count: Option<i32>,
    duration: Option<i32>,
    #[serde(rename = "coverArt")]
    cover_art: Option<String>,
    #[serde(default)]
    entry: Vec<SongDto>,
}

#[derive(Debug, Default, Deserialize)]
struct Search3Body {
    #[serde(rename = "searchResult3")]
    search_result3: Search3Dto,
}

#[derive(Debug, Default, Deserialize)]
struct Search3Dto {
    #[serde(default)]
    artist: Vec<ArtistDto>,
    #[serde(default)]
    album: Vec<AlbumDto>,
    #[serde(default)]
    song: Vec<SongDto>,
    #[serde(default)]
    playlist: Vec<PlaylistDto>,
}

#[derive(Debug, Clone, Deserialize)]
struct SongDto {
    id: String,
    title: String,
    album: Option<String>,
    artist: Option<String>,
    #[serde(rename = "albumId")]
    album_id: Option<String>,
    #[serde(rename = "artistId")]
    artist_id: Option<String>,
    duration: Option<i64>,
    #[serde(rename = "bitRate")]
    bit_rate: Option<i32>,
    track: Option<i32>,
    #[serde(rename = "discNumber")]
    disc_number: Option<i32>,
    #[serde(rename = "coverArt")]
    cover_art: Option<String>,
    size: Option<u64>,
    #[serde(rename = "contentType")]
    content_type: Option<String>,
    suffix: Option<String>,
    #[serde(default)]
    artists: Option<Vec<ArtistDto>>,
}

#[derive(Debug, Default, Deserialize)]
struct IndexesBody {
    indexes: IndexesDto,
}

#[derive(Debug, Default, Deserialize)]
struct IndexesDto {
    #[serde(default)]
    index: Vec<ArtistIndexDto>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::ScrobbleSubmission;
    use mockito::{Matcher, Server};

    const USERNAME: &str = "arthur";
    const PASSWORD: &str = "raw-password";

    fn ok(body: &str) -> String {
        format!(
            r#"{{"subsonic-response":{{"status":"ok","version":"1.16.1","openSubsonic":true,{body}}}}}"#
        )
    }

    async fn provider(server: &Server) -> SubsonicProvider {
        let client = SubsonicClient::new(server.url(), USERNAME, PASSWORD).expect("client");
        SubsonicProvider::from_client_for_tests(client, true)
    }

    fn auth_matchers() -> Vec<Matcher> {
        vec![
            Matcher::UrlEncoded("u".into(), USERNAME.into()),
            Matcher::UrlEncoded("v".into(), API_VERSION.into()),
            Matcher::UrlEncoded("c".into(), CLIENT_NAME.into()),
            Matcher::UrlEncoded("f".into(), "json".into()),
        ]
    }

    fn query_value(url: &Url, key: &str) -> Option<String> {
        url.query_pairs()
            .find(|(candidate, _)| candidate == key)
            .map(|(_, value)| value.into_owned())
    }

    #[test]
    fn song_conversion_preserves_subsonic_units_and_optional_fields() {
        let song = song_from_dto(SongDto {
            id: "song-id".to_string(),
            title: "Track".to_string(),
            album: Some("Album".to_string()),
            artist: Some("Artist".to_string()),
            album_id: Some("album-id".to_string()),
            artist_id: Some("artist-id".to_string()),
            duration: Some(319),
            bit_rate: Some(256),
            track: Some(7),
            disc_number: Some(2),
            cover_art: Some("cover-id".to_string()),
            size: Some(1_234),
            content_type: Some("audio/flac".to_string()),
            suffix: Some("flac".to_string()),
            artists: None,
        });

        assert_eq!(song.id, "song-id");
        assert_eq!(song.duration_seconds, 319);
        assert_eq!(song.bitrate_kbps, Some(256));
        assert_eq!(song.album_id.as_deref(), Some("album-id"));
        assert_eq!(song.artist_id.as_deref(), Some("artist-id"));
        assert_eq!(song.cover_art_id.as_deref(), Some("cover-id"));
        assert_ne!(song.cover_art_id.as_deref(), Some("song-id"));
    }

    #[test]
    fn song_conversion_keeps_missing_optional_fields_none() {
        let song = song_from_dto(SongDto {
            id: "song-id".to_string(),
            title: "Track".to_string(),
            album: None,
            artist: None,
            album_id: None,
            artist_id: None,
            duration: None,
            bit_rate: None,
            track: None,
            disc_number: None,
            cover_art: None,
            size: None,
            content_type: None,
            suffix: None,
            artists: None,
        });

        assert_eq!(song.duration_seconds, 0);
        assert_eq!(song.bitrate_kbps, None);
        assert_eq!(song.cover_art_id, None);
    }

    #[tokio::test]
    async fn connect_pings_once_and_caches_capabilities() {
        let mut server = Server::new_async().await;
        let _ping = server
            .mock("GET", "/rest/ping.view")
            .match_query(Matcher::AllOf(auth_matchers()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"subsonic-response":{"status":"ok","version":"1.16.1","openSubsonic":true}}"#,
            )
            .expect(1)
            .create_async()
            .await;

        let provider = SubsonicProvider::connect(ProviderCredentials {
            server_url: server.url(),
            credential: CredentialKind::Password {
                username: USERNAME.to_string(),
                password: PASSWORD.to_string(),
            },
        })
        .await
        .expect("provider");

        assert_eq!(provider.server_type(), ServerType::OpenSubsonic);
        assert_eq!(
            provider.capabilities(),
            Capabilities {
                open_subsonic: true,
                supports_changes_since: true,
                supports_server_transcoding: true,
            }
        );
        assert!(provider.capabilities().open_subsonic);
    }

    #[tokio::test]
    async fn rejects_token_credentials() {
        let result = SubsonicProvider::connect(ProviderCredentials {
            server_url: "http://localhost".to_string(),
            credential: CredentialKind::Token("token".to_string()),
        })
        .await;

        assert!(matches!(
            result,
            Err(ProviderError::UnsupportedCapability(_))
        ));
    }

    #[tokio::test]
    async fn lists_artists_from_get_artists() {
        let mut server = Server::new_async().await;
        let _mock = server
            .mock("GET", "/rest/getArtists.view")
            .match_query(Matcher::AllOf(auth_matchers()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(&ok(
                r#""artists":{"index":[{"name":"A","artist":[{"id":"artist1","name":"Artist","albumCount":2,"coverArt":"artist-cover"}]}]}"#,
            ))
            .create_async()
            .await;
        let provider = provider(&server).await;

        let artists = provider
            .list_artists(Some("ignored"))
            .await
            .expect("artists");

        assert_eq!(artists[0].id, "artist1");
        assert_eq!(artists[0].album_count, Some(2));
        assert_eq!(artists[0].cover_art_id.as_deref(), Some("artist-cover"));
    }

    #[tokio::test]
    async fn get_album_maps_tracks_from_get_album() {
        let mut server = Server::new_async().await;
        let _mock = server
            .mock("GET", "/rest/getAlbum.view")
            .match_query(Matcher::AllOf({
                let mut matchers = auth_matchers();
                matchers.push(Matcher::UrlEncoded("id".into(), "album1".into()));
                matchers
            }))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(&ok(
                r#""album":{"id":"album1","name":"Album","artist":"Artist","artistId":"artist1","songCount":1,"duration":319,"coverArt":"album-cover","song":[{"id":"song1","title":"Track","album":"Album","artist":"Artist","albumId":"album1","artistId":"artist1","duration":319,"bitRate":320,"coverArt":"song-cover"}]}"#,
            ))
            .create_async()
            .await;
        let provider = provider(&server).await;

        let album = provider.get_album("album1").await.expect("album");

        assert_eq!(album.album.id, "album1");
        assert_eq!(album.album.duration_seconds, Some(319));
        assert_eq!(album.tracks[0].bitrate_kbps, Some(320));
    }

    #[tokio::test]
    async fn get_artist_and_list_albums_use_id3_browse_endpoints() {
        let mut server = Server::new_async().await;
        let _artist = server
            .mock("GET", "/rest/getArtist.view")
            .match_query(Matcher::AllOf({
                let mut matchers = auth_matchers();
                matchers.push(Matcher::UrlEncoded("id".into(), "artist1".into()));
                matchers
            }))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(&ok(
                r#""artist":{"id":"artist1","name":"Artist","albumCount":1,"coverArt":"artist-cover","album":[{"id":"album1","name":"Album","artist":"Artist","artistId":"artist1"}]}"#,
            ))
            .create_async()
            .await;
        let _albums = server
            .mock("GET", "/rest/getAlbumList2.view")
            .match_query(Matcher::AllOf({
                let mut matchers = auth_matchers();
                matchers.push(Matcher::UrlEncoded("type".into(), "alphabeticalByName".into()));
                matchers.push(Matcher::UrlEncoded("size".into(), "500".into()));
                matchers
            }))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(&ok(
                r#""albumList2":{"album":[{"id":"album1","name":"Album","artist":"Artist","artistId":"artist1","songCount":4,"coverArt":"album-cover"}]}"#,
            ))
            .create_async()
            .await;
        let provider = provider(&server).await;

        let artist = provider.get_artist("artist1").await.expect("artist");
        let albums = provider.list_albums(Some("ignored")).await.expect("albums");

        assert_eq!(artist.artist.cover_art_id.as_deref(), Some("artist-cover"));
        assert_eq!(artist.albums[0].id, "album1");
        assert_eq!(albums[0].song_count, Some(4));
        assert_eq!(albums[0].cover_art_id.as_deref(), Some("album-cover"));
    }

    #[tokio::test]
    async fn lists_and_gets_playlists() {
        let mut server = Server::new_async().await;
        let _playlists = server
            .mock("GET", "/rest/getPlaylists.view")
            .match_query(Matcher::AllOf(auth_matchers()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(&ok(
                r#""playlists":{"playlist":[{"id":"playlist1","name":"Road","songCount":1,"duration":60,"coverArt":"playlist-cover"}]}"#,
            ))
            .create_async()
            .await;
        let _playlist = server
            .mock("GET", "/rest/getPlaylist.view")
            .match_query(Matcher::AllOf({
                let mut matchers = auth_matchers();
                matchers.push(Matcher::UrlEncoded("id".into(), "playlist1".into()));
                matchers
            }))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(&ok(
                r#""playlist":{"id":"playlist1","name":"Road","songCount":1,"duration":60,"entry":[{"id":"song1","title":"Track","duration":60}]}"#,
            ))
            .create_async()
            .await;
        let provider = provider(&server).await;

        let playlists = provider.list_playlists().await.expect("playlists");
        let playlist = provider.get_playlist("playlist1").await.expect("playlist");

        assert_eq!(playlists[0].cover_art_id.as_deref(), Some("playlist-cover"));
        assert_eq!(playlist.tracks[0].id, "song1");
    }

    #[tokio::test]
    async fn search3_maps_available_result_types() {
        let mut server = Server::new_async().await;
        let _mock = server
            .mock("GET", "/rest/search3.view")
            .match_query(Matcher::AllOf({
                let mut matchers = auth_matchers();
                matchers.push(Matcher::UrlEncoded("query".into(), "road".into()));
                matchers
            }))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(&ok(
                r#""searchResult3":{"artist":[{"id":"artist1","name":"Artist"}],"album":[{"id":"album1","name":"Album"}],"song":[{"id":"song1","title":"Track"}],"playlist":[{"id":"playlist1","name":"Road"}]}"#,
            ))
            .create_async()
            .await;
        let provider = provider(&server).await;

        let result = provider.search("road").await.expect("search");

        assert_eq!(result.artists.len(), 1);
        assert_eq!(result.albums.len(), 1);
        assert_eq!(result.songs.len(), 1);
        assert_eq!(result.playlists.len(), 1);
    }

    #[tokio::test]
    async fn list_libraries_returns_synthetic_all_music() {
        let provider = SubsonicProvider::from_client_for_tests(
            SubsonicClient::new("http://localhost", USERNAME, PASSWORD).expect("client"),
            false,
        );

        let libraries = provider.list_libraries().await.expect("libraries");

        assert_eq!(libraries.len(), 2);
        assert_eq!(libraries[0].id, "all");
        assert_eq!(libraries[0].name, "All Music");
        assert_eq!(libraries[0].item_type, ItemType::Library);
        assert_eq!(libraries[0].cover_art_id, None);
        assert_eq!(libraries[1].id, "playlists");
        assert_eq!(libraries[1].name, "Playlists");
        assert_eq!(libraries[1].item_type, ItemType::Library);
        assert_eq!(libraries[1].cover_art_id, None);
    }

    #[tokio::test]
    async fn download_stream_and_cover_urls_are_signed_without_raw_password() {
        let provider = SubsonicProvider::from_client_for_tests(
            SubsonicClient::new("http://music.example", USERNAME, PASSWORD).expect("client"),
            false,
        );

        let download = provider
            .download_url("song1", None)
            .await
            .expect("download");
        let stream = provider
            .download_url(
                "song1",
                Some(&TranscodeProfile {
                    container: Some("mp3".to_string()),
                    audio_codec: Some("mp3".to_string()),
                    max_bitrate_kbps: Some(192),
                }),
            )
            .await
            .expect("stream");
        let cover = provider.cover_art_url("cover1").await.expect("cover");

        assert!(download.contains("/rest/download.view"));
        assert!(stream.contains("/rest/stream.view"));
        assert!(cover.contains("/rest/getCoverArt.view"));
        assert!(stream.contains("format=mp3"));
        assert!(stream.contains("maxBitRate=192"));
        assert!(
            !stream.contains("maxBitRate=192000"),
            "maxBitRate must be kbps, not bps"
        );
        assert!(!download.contains(PASSWORD));
        assert!(!stream.contains(PASSWORD));
        assert!(!cover.contains(PASSWORD));
    }

    #[test]
    fn sanitize_subsonic_url_redacts_only_auth_query_values() {
        let url = Url::parse(
            "http://music.example/rest/download.view?u=arthur&p=raw-password&t=token-value&s=salt-value&id=song1&v=1.16.1&c=hifimule&f=json",
        )
        .expect("url");

        let sanitized = sanitize_subsonic_url(&url);
        let parsed = Url::parse(&sanitized).expect("sanitized url remains parseable");

        for key in ["u", "p", "t", "s"] {
            assert_eq!(query_value(&parsed, key).as_deref(), Some(REDACTED));
        }
        assert_eq!(query_value(&parsed, "id").as_deref(), Some("song1"));
        assert_eq!(query_value(&parsed, "v").as_deref(), Some(API_VERSION));
        assert_eq!(query_value(&parsed, "c").as_deref(), Some(CLIENT_NAME));
        assert_eq!(query_value(&parsed, "f").as_deref(), Some("json"));
        assert!(!sanitized.contains("arthur"));
        assert!(!sanitized.contains("raw-password"));
        assert!(!sanitized.contains("token-value"));
        assert!(!sanitized.contains("salt-value"));
    }

    #[test]
    fn sanitize_subsonic_url_preserves_stream_profile_parameters() {
        let url = Url::parse(
            "http://music.example/rest/stream.view?u=arthur&t=token-value&s=salt-value&id=song1&format=mp3&maxBitRate=192&v=1.16.1&c=hifimule&f=json",
        )
        .expect("url");

        let sanitized = sanitize_subsonic_url(&url);
        let parsed = Url::parse(&sanitized).expect("sanitized url remains parseable");

        assert_eq!(query_value(&parsed, "format").as_deref(), Some("mp3"));
        assert_eq!(query_value(&parsed, "maxBitRate").as_deref(), Some("192"));
        assert_eq!(query_value(&parsed, "id").as_deref(), Some("song1"));
        assert_eq!(query_value(&parsed, "t").as_deref(), Some(REDACTED));
        assert_eq!(query_value(&parsed, "s").as_deref(), Some(REDACTED));
    }

    #[test]
    fn sanitize_subsonic_url_preserves_cover_art_id() {
        let url = Url::parse(
            "http://music.example/rest/getCoverArt.view?u=arthur&t=token-value&s=salt-value&id=cover1&v=1.16.1&c=hifimule&f=json",
        )
        .expect("url");

        let sanitized = sanitize_subsonic_url(&url);
        let parsed = Url::parse(&sanitized).expect("sanitized url remains parseable");

        assert_eq!(query_value(&parsed, "id").as_deref(), Some("cover1"));
        assert_eq!(query_value(&parsed, "u").as_deref(), Some(REDACTED));
    }

    #[test]
    fn sanitize_subsonic_message_avoids_mid_word_false_positives() {
        let message = "status=ok type=json artist=The Smiths already u=[REDACTED] ?t=token&s=salt";

        let sanitized = sanitize_subsonic_message(message);

        assert!(sanitized.contains("status=ok"));
        assert!(sanitized.contains("type=json"));
        assert!(sanitized.contains("artist=The Smiths"));
        assert!(sanitized.contains("u=[REDACTED]"));
        assert!(sanitized.contains("?t=[REDACTED]&s=[REDACTED]"));
        assert!(!sanitized.contains("token"));
        assert!(!sanitized.contains("salt"));
    }

    #[test]
    fn sanitize_subsonic_message_handles_malformed_relative_text() {
        let message =
            "failed relative/rest/download.view?u=arthur&p=raw-password&t=token&s=salt id=song1";

        let sanitized = sanitize_subsonic_message(message);

        assert!(sanitized.contains("relative/rest/download.view?u=[REDACTED]"));
        assert!(sanitized.contains("id=song1"));
        assert!(!sanitized.contains("arthur"));
        assert!(!sanitized.contains("raw-password"));
        assert!(!sanitized.contains("token"));
        assert!(!sanitized.contains("salt"));
    }

    #[tokio::test]
    async fn changes_since_sends_epoch_milliseconds_to_get_indexes() {
        let mut server = Server::new_async().await;
        let _mock = server
            .mock("GET", "/rest/getIndexes.view")
            .match_query(Matcher::AllOf({
                let mut matchers = auth_matchers();
                matchers.push(Matcher::UrlEncoded(
                    "ifModifiedSince".into(),
                    "1710000000000".into(),
                ));
                matchers
            }))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(&ok(
                r#""indexes":{"index":[{"name":"A","artist":[{"id":"artist1","name":"Artist"}]}]}"#,
            ))
            .create_async()
            .await;
        let provider = provider(&server).await;

        let changes = provider
            .changes_since(Some("1710000000000"))
            .await
            .expect("changes");

        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].item.id, "artist1");
        assert_eq!(changes[0].item.item_type, ItemType::Artist);
        assert_eq!(changes[0].change_type, ChangeType::Updated);
    }

    #[tokio::test]
    async fn changes_since_initial_full_dump_pages_search3_songs() {
        let mut server = Server::new_async().await;
        let first_page_songs = (0..500)
            .map(|idx| {
                format!(
                    r#"{{"id":"song{idx}","title":"Track {idx}","albumId":"album1","size":{size},"contentType":"audio/mpeg","suffix":"mp3"}}"#,
                    size = 1_000 + idx
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let second_page_songs = (500..503)
            .map(|idx| {
                format!(
                    r#"{{"id":"song{idx}","title":"Track {idx}","albumId":"album1","size":{size},"contentType":"audio/mpeg","suffix":"mp3"}}"#,
                    size = 1_000 + idx
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let _first = server
            .mock("GET", "/rest/search3.view")
            .match_query(Matcher::AllOf({
                let mut matchers = auth_matchers();
                matchers.push(Matcher::UrlEncoded("query".into(), "".into()));
                matchers.push(Matcher::UrlEncoded("songCount".into(), "500".into()));
                matchers.push(Matcher::UrlEncoded("songOffset".into(), "0".into()));
                matchers
            }))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(&ok(&format!(
                r#""searchResult3":{{"song":[{first_page_songs}]}}"#
            )))
            .expect(1)
            .create_async()
            .await;
        let _second = server
            .mock("GET", "/rest/search3.view")
            .match_query(Matcher::AllOf({
                let mut matchers = auth_matchers();
                matchers.push(Matcher::UrlEncoded("query".into(), "".into()));
                matchers.push(Matcher::UrlEncoded("songCount".into(), "500".into()));
                matchers.push(Matcher::UrlEncoded("songOffset".into(), "500".into()));
                matchers
            }))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(&ok(&format!(
                r#""searchResult3":{{"song":[{second_page_songs}]}}"#
            )))
            .expect(1)
            .create_async()
            .await;
        let _indexes = server
            .mock("GET", "/rest/getIndexes.view")
            .expect(0)
            .create_async()
            .await;
        let provider = provider(&server).await;

        let changes = provider.changes_since(None).await.expect("changes");

        assert_eq!(changes.len(), 503);
        assert_eq!(changes[0].item.item_type, ItemType::Song);
        assert_eq!(changes[0].change_type, ChangeType::Created);
        assert_eq!(
            changes[0].version.as_deref(),
            Some("subsonic:song0|album1|1000|audio/mpeg|mp3")
        );
    }

    #[tokio::test]
    async fn changes_since_initial_full_dump_errors_when_page_cap_is_hit() {
        let mut server = Server::new_async().await;
        for offset in [0, 500] {
            let songs = (offset..offset + 500)
                .map(|idx| format!(r#"{{"id":"song{idx}","title":"Track {idx}"}}"#))
                .collect::<Vec<_>>()
                .join(",");
            let _page = server
                .mock("GET", "/rest/search3.view")
                .match_query(Matcher::AllOf({
                    let mut matchers = auth_matchers();
                    matchers.push(Matcher::UrlEncoded("query".into(), "".into()));
                    matchers.push(Matcher::UrlEncoded("songCount".into(), "500".into()));
                    matchers.push(Matcher::UrlEncoded("songOffset".into(), offset.to_string()));
                    matchers
                }))
                .with_status(200)
                .with_header("content-type", "application/json")
                .with_body(&ok(&format!(r#""searchResult3":{{"song":[{songs}]}}"#)))
                .expect(1)
                .create_async()
                .await;
        }
        let provider = provider(&server).await;

        let result = provider.changes_since(None).await;

        assert!(matches!(
            result,
            Err(ProviderError::UnsupportedCapability(message))
                if message.contains("full-library dump exceeded")
        ));
    }

    #[tokio::test]
    async fn changes_since_zero_uses_search3_not_get_indexes() {
        let mut server = Server::new_async().await;
        let _search = server
            .mock("GET", "/rest/search3.view")
            .match_query(Matcher::AllOf({
                let mut matchers = auth_matchers();
                matchers.push(Matcher::UrlEncoded("query".into(), "".into()));
                matchers.push(Matcher::UrlEncoded("songCount".into(), "500".into()));
                matchers.push(Matcher::UrlEncoded("songOffset".into(), "0".into()));
                matchers
            }))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(&ok(
                r#""searchResult3":{"song":[{"id":"song1","title":"Track","albumId":"album1"}]}"#,
            ))
            .expect(1)
            .create_async()
            .await;
        let _indexes = server
            .mock("GET", "/rest/getIndexes.view")
            .expect(0)
            .create_async()
            .await;
        let provider = provider(&server).await;

        let changes = provider.changes_since(Some("0")).await.expect("changes");

        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].item.id, "song1");
    }

    #[tokio::test]
    async fn changes_since_album_fallback_detects_created_deleted_and_metadata_updates() {
        let mut server = Server::new_async().await;
        let _indexes = server
            .mock("GET", "/rest/getIndexes.view")
            .match_query(Matcher::AllOf({
                let mut matchers = auth_matchers();
                matchers.push(Matcher::UrlEncoded(
                    "ifModifiedSince".into(),
                    "1710000000000".into(),
                ));
                matchers
            }))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(&ok(r#""indexes":{"index":[]}"#))
            .expect(1)
            .create_async()
            .await;
        let _album = server
            .mock("GET", "/rest/getAlbum.view")
            .match_query(Matcher::AllOf({
                let mut matchers = auth_matchers();
                matchers.push(Matcher::UrlEncoded("id".into(), "album1".into()));
                matchers
            }))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(&ok(
                r#""album":{"id":"album1","name":"Album","song":[{"id":"song1","title":"Existing","albumId":"album1","size":1200,"contentType":"audio/mpeg","suffix":"mp3"},{"id":"song3","title":"New","albumId":"album1","size":3000,"contentType":"audio/flac","suffix":"flac"}]}"#,
            ))
            .expect(1)
            .create_async()
            .await;
        let provider = provider(&server).await;
        let context = ProviderChangeContext {
            synced_songs: vec![
                ProviderSyncedSong {
                    song_id: "song1".to_string(),
                    album_id: Some("album1".to_string()),
                    size: Some(1000),
                    content_type: Some("audio/mpeg".to_string()),
                    suffix: Some("mp3".to_string()),
                    version: Some("old-v1".to_string()),
                },
                ProviderSyncedSong {
                    song_id: "song2".to_string(),
                    album_id: Some("album1".to_string()),
                    size: Some(2000),
                    content_type: Some("audio/mpeg".to_string()),
                    suffix: Some("mp3".to_string()),
                    version: Some("old-v2".to_string()),
                },
            ],
        };

        let changes = provider
            .changes_since_with_context(Some("1710000000000"), &context)
            .await
            .expect("changes");

        assert_eq!(changes.len(), 3);
        assert!(changes
            .iter()
            .any(|change| change.item.id == "song1" && change.change_type == ChangeType::Updated));
        assert!(changes
            .iter()
            .any(|change| change.item.id == "song2" && change.change_type == ChangeType::Deleted));
        assert!(changes
            .iter()
            .any(|change| change.item.id == "song3" && change.change_type == ChangeType::Created));
    }

    #[tokio::test]
    async fn changes_since_album_fallback_propagates_album_fetch_errors() {
        let mut server = Server::new_async().await;
        let _indexes = server
            .mock("GET", "/rest/getIndexes.view")
            .match_query(Matcher::AllOf({
                let mut matchers = auth_matchers();
                matchers.push(Matcher::UrlEncoded(
                    "ifModifiedSince".into(),
                    "1710000000000".into(),
                ));
                matchers
            }))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(&ok(r#""indexes":{"index":[]}"#))
            .expect(1)
            .create_async()
            .await;
        let _album = server
            .mock("GET", "/rest/getAlbum.view")
            .match_query(Matcher::AllOf({
                let mut matchers = auth_matchers();
                matchers.push(Matcher::UrlEncoded("id".into(), "album1".into()));
                matchers
            }))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"subsonic-response":{"status":"failed","version":"1.16.1","error":{"code":70,"message":"Album missing"}}}"#,
            )
            .expect(1)
            .create_async()
            .await;
        let provider = provider(&server).await;
        let context = ProviderChangeContext {
            synced_songs: vec![ProviderSyncedSong {
                song_id: "song1".to_string(),
                album_id: Some("album1".to_string()),
                size: Some(1000),
                content_type: Some("audio/mpeg".to_string()),
                suffix: Some("mp3".to_string()),
                version: Some("old-v1".to_string()),
            }],
        };

        let result = provider
            .changes_since_with_context(Some("1710000000000"), &context)
            .await;

        assert!(matches!(result, Err(ProviderError::NotFound { .. })));
    }

    #[test]
    fn album_song_changes_ignores_missing_legacy_metadata_when_actual_has_metadata() {
        let expected = ProviderSyncedSong {
            song_id: "song1".to_string(),
            album_id: Some("album1".to_string()),
            size: None,
            content_type: None,
            suffix: None,
            version: Some("old-v1".to_string()),
        };
        let actual = SongDto {
            id: "song1".to_string(),
            title: "Track".to_string(),
            album: None,
            artist: None,
            album_id: Some("album1".to_string()),
            artist_id: None,
            duration: None,
            bit_rate: None,
            track: None,
            disc_number: None,
            cover_art: None,
            size: Some(1000),
            content_type: Some("audio/mpeg".to_string()),
            suffix: Some("mp3".to_string()),
            artists: None,
        };

        let changes = album_song_changes(&[&expected], &[actual]);

        assert!(changes.is_empty());
    }

    #[test]
    fn change_metadata_from_version_filters_empty_fields() {
        let metadata = SubsonicProvider::change_metadata_from_version(Some(
            "subsonic:song1|album1|3000|audio/flac|flac",
        ))
        .expect("metadata");

        assert_eq!(metadata.album_id.as_deref(), Some("album1"));
        assert_eq!(metadata.size, Some(3000));
        assert_eq!(metadata.content_type.as_deref(), Some("audio/flac"));
        assert_eq!(metadata.suffix.as_deref(), Some("flac"));

        let sparse = SubsonicProvider::change_metadata_from_version(Some("subsonic:song1||||"))
            .expect("sparse metadata");
        assert_eq!(sparse.album_id, None);
        assert_eq!(sparse.size, None);
        assert_eq!(sparse.content_type, None);
        assert_eq!(sparse.suffix, None);
    }

    #[tokio::test]
    async fn malformed_changes_token_returns_focused_error() {
        let provider = SubsonicProvider::from_client_for_tests(
            SubsonicClient::new("http://localhost", USERNAME, PASSWORD).expect("client"),
            false,
        );

        let result = provider.changes_since(Some("not-a-number")).await;

        assert!(matches!(
            result,
            Err(ProviderError::UnsupportedCapability(_))
        ));
    }

    #[tokio::test]
    async fn maps_subsonic_api_and_json_errors() {
        let mut server = Server::new_async().await;
        let _auth = server
            .mock("GET", "/rest/getArtists.view")
            .match_query(Matcher::AllOf(auth_matchers()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"subsonic-response":{"status":"failed","version":"1.16.1","error":{"code":40,"message":"Wrong password=raw-password"}}}"#,
            )
            .create_async()
            .await;
        let auth_provider = provider(&server).await;

        let auth = auth_provider.list_artists(None).await;

        assert!(
            matches!(auth, Err(ProviderError::Auth(ref message)) if !message.contains(PASSWORD)),
            "Subsonic code 40 should map to sanitized auth error, got: {:?}",
            auth
        );

        let mut server = Server::new_async().await;
        let _json = server
            .mock("GET", "/rest/getArtists.view")
            .match_query(Matcher::AllOf(auth_matchers()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body("{not json")
            .create_async()
            .await;
        let json_provider = provider(&server).await;

        let malformed = json_provider.list_artists(None).await;

        assert!(matches!(malformed, Err(ProviderError::Deserialization(_))));
    }

    #[tokio::test]
    async fn scrobble_played_calls_submission_true() {
        let mut server = Server::new_async().await;
        let _mock = server
            .mock("GET", "/rest/scrobble.view")
            .match_query(Matcher::AllOf({
                let mut matchers = auth_matchers();
                matchers.push(Matcher::UrlEncoded("id".into(), "song1".into()));
                matchers.push(Matcher::UrlEncoded("submission".into(), "true".into()));
                matchers
            }))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"subsonic-response":{"status":"ok","version":"1.16.1"}}"#)
            .create_async()
            .await;
        let provider = provider(&server).await;

        provider
            .scrobble(ScrobbleRequest {
                song_id: "song1".to_string(),
                submission: ScrobbleSubmission::Played,
                position_seconds: None,
                played_at_unix_seconds: None,
            })
            .await
            .expect("scrobble");
    }

    #[tokio::test]
    async fn scrobble_playing_returns_unsupported_capability() {
        let provider = SubsonicProvider::from_client_for_tests(
            SubsonicClient::new("http://localhost", USERNAME, PASSWORD).expect("client"),
            false,
        );

        let result = provider
            .scrobble(ScrobbleRequest {
                song_id: "song1".to_string(),
                submission: ScrobbleSubmission::Playing,
                position_seconds: None,
                played_at_unix_seconds: None,
            })
            .await;

        assert!(matches!(
            result,
            Err(ProviderError::UnsupportedCapability(_))
        ));
    }

    #[tokio::test]
    async fn changes_since_empty_token_uses_initial_search3_dump() {
        let mut server = Server::new_async().await;
        let _mock = server
            .mock("GET", "/rest/search3.view")
            .match_query(Matcher::AllOf({
                let mut matchers = auth_matchers();
                matchers.push(Matcher::UrlEncoded("query".into(), "".into()));
                matchers.push(Matcher::UrlEncoded("songCount".into(), "500".into()));
                matchers.push(Matcher::UrlEncoded("songOffset".into(), "0".into()));
                matchers
            }))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(&ok(r#""searchResult3":{"song":[]}"#))
            .expect(1)
            .create_async()
            .await;
        let provider = provider(&server).await;

        let changes_empty = provider
            .changes_since(Some(""))
            .await
            .expect("changes(empty)");

        assert!(changes_empty.is_empty());
    }

    #[tokio::test]
    async fn maps_http_error_status_codes_to_provider_errors() {
        let mut server = Server::new_async().await;
        let _auth_mock = server
            .mock("GET", "/rest/getArtists.view")
            .match_query(Matcher::AllOf(auth_matchers()))
            .with_status(401)
            .create_async()
            .await;
        let provider = provider(&server).await;

        let auth_result = provider.list_artists(None).await;

        assert!(
            matches!(auth_result, Err(ProviderError::Auth(_))),
            "HTTP 401 should map to Auth error, got: {auth_result:?}"
        );

        let mut server2 = Server::new_async().await;
        let _not_found_mock = server2
            .mock("GET", "/rest/getAlbum.view")
            .match_query(Matcher::AllOf({
                let mut matchers = auth_matchers();
                matchers.push(Matcher::UrlEncoded("id".into(), "missing".into()));
                matchers
            }))
            .with_status(404)
            .create_async()
            .await;
        let client2 = SubsonicClient::new(server2.url(), USERNAME, PASSWORD).expect("client");
        let provider2 = SubsonicProvider::from_client_for_tests(client2, true);

        let not_found_result = provider2.get_album("missing").await;

        assert!(
            matches!(not_found_result, Err(ProviderError::NotFound { .. })),
            "HTTP 404 should map to NotFound error, got: {not_found_result:?}"
        );
    }

    #[tokio::test]
    async fn debug_and_errors_redact_password() {
        let client = SubsonicClient::new("http://localhost", USERNAME, PASSWORD).expect("client");
        let debug = format!("{client:?}");
        let credential_debug = format!(
            "{:?}",
            ProviderCredentials {
                server_url: "http://localhost".to_string(),
                credential: CredentialKind::Token("secret-token".to_string()),
            }
        );
        let sanitized = sanitize_subsonic_message(
            "failed u=alexis p=raw-password t=token-value s=salt-value password=raw-password",
        );

        assert!(!debug.contains(PASSWORD));
        assert!(!credential_debug.contains("secret-token"));
        assert!(!sanitized.contains(PASSWORD));
        assert!(!sanitized.contains("alexis"));
        assert!(!sanitized.contains("token-value"));
        assert!(!sanitized.contains("salt-value"));
        assert!(debug.contains("[redacted]"));
    }
}
