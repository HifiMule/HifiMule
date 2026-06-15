use crate::api::{JellyfinClient, JellyfinItem, JellyfinView};
use crate::domain::models::{
    Album, AlbumWithTracks, Artist, ArtistWithAlbums, Bps, ChangeEvent, ChangeType, Genre, ItemRef,
    ItemType, JellyfinTicks, Kbps, Library, Playlist, PlaylistWithTracks, SearchResult, Seconds,
    Song,
};
use crate::providers::{
    BrowseCapabilities, BrowseMode, Capabilities, MediaProvider, ProviderChangeContext,
    ProviderError, ScrobbleRequest, ScrobbleSubmission, ServerType, TrackListFilter, TrackListPage,
    TranscodeProfile,
};
use async_trait::async_trait;
use std::collections::HashMap;

const ALBUM_TYPES: &str = "MusicAlbum";
const AUDIO_TYPES: &str = "Audio,MusicVideo";
const PLAYLIST_TYPES: &str = "Playlist";

#[derive(Clone)]
pub struct JellyfinProvider {
    client: JellyfinClient,
    server_url: String,
    token: String,
    user_id: String,
    server_version: Option<String>,
    /// Server-reported stable id (`System/Info.Id`), captured at connect. Drives the
    /// portable `server_id` `rid:` basis (Story 2.13). `None` when reconstructed from
    /// stored credentials (the reported id is only needed at the initial connect).
    server_reported_id: Option<String>,
}

impl JellyfinProvider {
    pub fn new(
        client: JellyfinClient,
        server_url: impl Into<String>,
        token: impl Into<String>,
        user_id: impl Into<String>,
    ) -> Self {
        Self {
            client,
            server_url: server_url.into(),
            token: token.into(),
            user_id: user_id.into(),
            server_version: None,
            server_reported_id: None,
        }
    }

    pub fn new_with_version(
        client: JellyfinClient,
        server_url: impl Into<String>,
        token: impl Into<String>,
        user_id: impl Into<String>,
        server_version: Option<String>,
    ) -> Self {
        Self {
            client,
            server_url: server_url.into(),
            token: token.into(),
            user_id: user_id.into(),
            server_version,
            server_reported_id: None,
        }
    }

    /// Records the server-reported stable id (`System/Info.Id`) captured at connect.
    pub fn with_reported_id(mut self, server_reported_id: Option<String>) -> Self {
        self.server_reported_id = server_reported_id;
        self
    }

    fn map_error(error: anyhow::Error) -> ProviderError {
        let message = error.to_string();

        if message.contains("Authentication failed") {
            return ProviderError::Auth(message);
        }

        if let Some(status) = status_from_message(&message) {
            return match status {
                401 | 403 => ProviderError::Auth(message),
                404 => ProviderError::NotFound {
                    item_type: "item".to_string(),
                    id: "unknown".to_string(),
                },
                _ => ProviderError::Http {
                    status: Some(status),
                    message,
                },
            };
        }

        if message.contains("expected")
            || message.contains("invalid type")
            || message.contains("missing field")
            || message.contains("EOF")
            || message.contains("at line ")
            || message.contains("trailing characters")
        {
            return ProviderError::Deserialization(message);
        }

        ProviderError::Other(error)
    }

    fn map_not_found(error: anyhow::Error, item_type: &str, id: &str) -> ProviderError {
        let message = error.to_string();
        if let Some(404) = status_from_message(&message) {
            ProviderError::NotFound {
                item_type: item_type.to_string(),
                id: id.to_string(),
            }
        } else {
            Self::map_error(error)
        }
    }

    fn token(&self) -> &str {
        &self.token
    }

    fn url(&self) -> &str {
        &self.server_url
    }

    fn user_id(&self) -> &str {
        &self.user_id
    }
}

#[async_trait]
impl MediaProvider for JellyfinProvider {
    async fn list_libraries(&self) -> Result<Vec<Library>, ProviderError> {
        let views = self
            .client
            .get_views(self.url(), self.token(), self.user_id())
            .await
            .map_err(Self::map_error)?;

        Ok(views.into_iter().filter_map(library_from_view).collect())
    }

    async fn list_artists(
        &self,
        library_id: Option<&str>,
        letter: Option<&str>,
        offset: u32,
        limit: u32,
    ) -> Result<(Vec<Artist>, u32), ProviderError> {
        let limit_param = if limit > 0 { Some(limit) } else { None };
        let response = self
            .client
            .get_album_artists(
                self.url(),
                self.token(),
                self.user_id(),
                library_id,
                letter,
                Some(offset),
                limit_param,
            )
            .await
            .map_err(Self::map_error)?;
        let total = response.total_record_count;
        Ok((
            response.items.into_iter().map(artist_from_item).collect(),
            total,
        ))
    }

    async fn get_artist(&self, artist_id: &str) -> Result<ArtistWithAlbums, ProviderError> {
        let item = self
            .client
            .get_item_details(self.url(), self.token(), self.user_id(), artist_id)
            .await
            .map_err(|err| Self::map_not_found(err, "artist", artist_id))?;
        if item.item_type != "MusicArtist" {
            return Err(ProviderError::NotFound {
                item_type: "artist".to_string(),
                id: artist_id.to_string(),
            });
        }
        let albums = self
            .client
            .get_albums_by_artist(self.url(), self.token(), self.user_id(), artist_id)
            .await
            .map_err(Self::map_error)?
            .items
            .into_iter()
            .map(album_from_item)
            .collect();

        Ok(ArtistWithAlbums {
            artist: artist_from_item(item),
            albums,
        })
    }

    async fn list_albums(
        &self,
        library_id: Option<&str>,
        letter: Option<&str>,
        offset: u32,
        limit: u32,
    ) -> Result<(Vec<Album>, u32), ProviderError> {
        let limit_param = if limit > 0 { Some(limit) } else { None };
        let response = self
            .client
            .get_items(
                self.url(),
                self.token(),
                self.user_id(),
                library_id,
                Some(ALBUM_TYPES),
                Some(offset),
                limit_param,
                letter,
                None,
                None,
                None,
                None,
            )
            .await
            .map_err(Self::map_error)?;
        let total = response.total_record_count;
        Ok((
            response.items.into_iter().map(album_from_item).collect(),
            total,
        ))
    }

    async fn get_album(&self, album_id: &str) -> Result<AlbumWithTracks, ProviderError> {
        let album = self
            .client
            .get_item_details(self.url(), self.token(), self.user_id(), album_id)
            .await
            .map_err(|err| Self::map_not_found(err, "album", album_id))?;
        if album.item_type != "MusicAlbum" {
            return Err(ProviderError::NotFound {
                item_type: "album".to_string(),
                id: album_id.to_string(),
            });
        }
        let tracks = self
            .client
            .get_child_items_with_sizes(self.url(), self.token(), self.user_id(), album_id)
            .await
            .map_err(Self::map_error)?
            .into_iter()
            .map(song_from_item)
            .collect();

        Ok(AlbumWithTracks {
            album: album_from_item(album),
            tracks,
        })
    }

    async fn list_playlists(&self) -> Result<Vec<Playlist>, ProviderError> {
        let response = self
            .client
            .get_items(
                self.url(),
                self.token(),
                self.user_id(),
                None,
                Some(PLAYLIST_TYPES),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .await
            .map_err(Self::map_error)?;

        Ok(response.items.into_iter().map(playlist_from_item).collect())
    }

    async fn get_playlist(&self, playlist_id: &str) -> Result<PlaylistWithTracks, ProviderError> {
        let playlist = self
            .client
            .get_item_details(self.url(), self.token(), self.user_id(), playlist_id)
            .await
            .map_err(|err| Self::map_not_found(err, "playlist", playlist_id))?;
        if playlist.item_type != "Playlist" {
            return Err(ProviderError::NotFound {
                item_type: "playlist".to_string(),
                id: playlist_id.to_string(),
            });
        }
        let tracks = self
            .client
            .get_playlist_items_via_user_library(
                self.url(),
                self.token(),
                self.user_id(),
                playlist_id,
            )
            .await
            .map_err(Self::map_error)?
            .into_iter()
            .map(song_from_item)
            .collect();

        Ok(PlaylistWithTracks {
            playlist: playlist_from_item(playlist),
            tracks,
        })
    }

    async fn get_song(&self, song_id: &str) -> Result<Song, ProviderError> {
        let item = self
            .client
            .get_item_details(self.url(), self.token(), self.user_id(), song_id)
            .await
            .map_err(|err| Self::map_not_found(err, "song", song_id))?;
        if item.item_type != "Audio" && item.item_type != "MusicVideo" {
            return Err(ProviderError::NotFound {
                item_type: "song".to_string(),
                id: song_id.to_string(),
            });
        }
        Ok(song_from_item(item))
    }

    async fn create_playlist(
        &self,
        name: &str,
        track_ids: &[String],
    ) -> Result<String, ProviderError> {
        self.client
            .create_playlist(self.url(), self.token(), self.user_id(), name, track_ids)
            .await
            .map_err(Self::map_error)
    }

    async fn add_to_playlist(
        &self,
        playlist_id: &str,
        track_ids: &[String],
    ) -> Result<(), ProviderError> {
        if track_ids.is_empty() {
            return Ok(());
        }
        self.client
            .add_tracks_to_playlist(
                self.url(),
                self.token(),
                self.user_id(),
                playlist_id,
                track_ids,
            )
            .await
            .map_err(Self::map_error)
    }

    async fn remove_from_playlist(
        &self,
        playlist_id: &str,
        track_ids: &[String],
    ) -> Result<(), ProviderError> {
        if track_ids.is_empty() {
            return Ok(());
        }

        let items = self
            .client
            .get_playlist_items(self.url(), self.token(), self.user_id(), playlist_id)
            .await
            .map_err(Self::map_error)?;

        let mut remaining_by_track_id: HashMap<&str, usize> = HashMap::new();
        for track_id in track_ids {
            *remaining_by_track_id.entry(track_id.as_str()).or_insert(0) += 1;
        }

        let mut entry_ids = Vec::new();
        let mut missing_entry_ids = Vec::new();
        for item in items {
            let Some(remaining) = remaining_by_track_id.get_mut(item.id.as_str()) else {
                continue;
            };
            if *remaining == 0 {
                continue;
            }
            *remaining -= 1;

            match item.playlist_item_id {
                Some(entry_id) => entry_ids.push(entry_id),
                None => missing_entry_ids.push(item.id),
            }
        }

        if !missing_entry_ids.is_empty() {
            return Err(ProviderError::Deserialization(format!(
                "Playlist item(s) missing PlaylistItemId: {}",
                missing_entry_ids.join(",")
            )));
        }

        if entry_ids.is_empty() {
            return Ok(());
        }

        self.client
            .delete_playlist_items(self.url(), self.token(), playlist_id, &entry_ids)
            .await
            .map_err(Self::map_error)
    }

    async fn delete_playlist(&self, playlist_id: &str) -> Result<(), ProviderError> {
        self.client
            .delete_item(self.url(), self.token(), playlist_id)
            .await
            .map_err(Self::map_error)
    }

    async fn rename_playlist(
        &self,
        playlist_id: &str,
        new_name: &str,
    ) -> Result<(), ProviderError> {
        self.client
            .update_playlist_name(self.url(), self.token(), playlist_id, new_name)
            .await
            .map_err(Self::map_error)
    }

    async fn reorder_playlist(
        &self,
        playlist_id: &str,
        ordered_track_ids: &[String],
    ) -> Result<(), ProviderError> {
        let items = self
            .client
            .get_playlist_items(self.url(), self.token(), self.user_id(), playlist_id)
            .await
            .map_err(Self::map_error)?;

        let mut current: Vec<(String, String)> = Vec::with_capacity(items.len());
        for item in items {
            match item.playlist_item_id {
                Some(entry_id) => current.push((item.id, entry_id)),
                None => {
                    return Err(ProviderError::Deserialization(format!(
                        "Playlist item missing PlaylistItemId: {}",
                        item.id
                    )));
                }
            }
        }

        for (target_index, wanted_track_id) in ordered_track_ids.iter().enumerate() {
            if target_index >= current.len() {
                break;
            }
            if &current[target_index].0 == wanted_track_id {
                continue;
            }
            let Some(found) =
                (target_index..current.len()).find(|&j| &current[j].0 == wanted_track_id)
            else {
                return Err(ProviderError::UnsupportedCapability(format!(
                    "reorder_playlist: track {wanted_track_id} is not in playlist {playlist_id}"
                )));
            };

            let (_, entry_id) = current[found].clone();
            self.client
                .move_playlist_item(
                    self.url(),
                    self.token(),
                    playlist_id,
                    &entry_id,
                    target_index,
                )
                .await
                .map_err(Self::map_error)?;

            let moved = current.remove(found);
            current.insert(target_index, moved);
        }

        Ok(())
    }

    async fn search(&self, query: &str) -> Result<SearchResult, ProviderError> {
        let songs = self
            .client
            .search_audio_items(self.url(), self.token(), self.user_id(), query)
            .await
            .map_err(Self::map_error)?
            .into_iter()
            .map(song_from_item)
            .collect();

        Ok(SearchResult {
            songs,
            ..SearchResult::default()
        })
    }

    async fn download_url(
        &self,
        song_id: &str,
        profile: Option<&TranscodeProfile>,
    ) -> Result<String, ProviderError> {
        // All returned URLs must be fetchable without auth headers because
        // `execute_provider_sync` uses a plain `reqwest::get(url)`.  Jellyfin
        // supports URL-based auth via `?api_key=<token>`, which we always append
        // so that direct-play downloads and transcoding fallbacks both work in the
        // multi-server provider sync path.
        let url = if let Some(profile) = profile {
            let profile = transcode_profile_to_device_profile(profile);
            self.client
                .resolve_stream_url(self.url(), self.token(), self.user_id(), song_id, &profile)
                .await
                .map(|(url, _is_transcoded)| url)
                .map_err(Self::map_error)?
        } else {
            format!(
                "{}/Items/{}/Download",
                self.url().trim_end_matches('/'),
                song_id
            )
        };
        // Append api_key only when not already present (TranscodingUrl from
        // PlaybackInfo already carries Jellyfin session auth).
        Ok(if url.contains("api_key=") || url.contains("ApiKey=") {
            url
        } else if url.contains('?') {
            format!("{}&api_key={}", url, self.token())
        } else {
            format!("{}?api_key={}", url, self.token())
        })
    }

    async fn cover_art_url(&self, cover_art_id: &str) -> Result<String, ProviderError> {
        Ok(format!(
            "{}/Items/{}/Images/Primary",
            self.url().trim_end_matches('/'),
            cover_art_id
        ))
    }

    async fn changes_since_with_context(
        &self,
        token: Option<&str>,
        _context: &ProviderChangeContext,
    ) -> Result<Vec<ChangeEvent>, ProviderError> {
        let response = self
            .client
            .get_items_changed_since(self.url(), self.token(), self.user_id(), token)
            .await
            .map_err(Self::map_error)?;

        Ok(response
            .items
            .into_iter()
            .filter_map(change_event_from_item)
            .collect())
    }

    async fn scrobble(&self, request: ScrobbleRequest) -> Result<(), ProviderError> {
        match request.submission {
            ScrobbleSubmission::Played => self
                .client
                .report_item_played(self.url(), self.token(), self.user_id(), &request.song_id)
                .await
                .map_err(Self::map_error),
            ScrobbleSubmission::Playing => Err(ProviderError::UnsupportedCapability(
                "Jellyfin now-playing scrobble is not implemented by the existing client"
                    .to_string(),
            )),
        }
    }

    fn server_type(&self) -> ServerType {
        ServerType::Jellyfin
    }

    fn server_version(&self) -> Option<&str> {
        self.server_version.as_deref()
    }

    fn access_token(&self) -> Option<&str> {
        Some(&self.token)
    }

    fn provider_user_id(&self) -> Option<&str> {
        Some(&self.user_id)
    }

    fn server_reported_id(&self) -> Option<&str> {
        self.server_reported_id.as_deref()
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            open_subsonic: false,
            supports_changes_since: true,
            supports_server_transcoding: true,
            supports_playlist_write: true,
            browse: BrowseCapabilities {
                list_modes: vec![
                    BrowseMode::Artists,
                    BrowseMode::Albums,
                    BrowseMode::Playlists,
                    BrowseMode::Tracks,
                    BrowseMode::Genres,
                    BrowseMode::RecentlyAdded,
                    BrowseMode::FrequentlyPlayed,
                    BrowseMode::RecentlyPlayed,
                    BrowseMode::Favorites,
                ],
            },
        }
    }

    async fn list_genres(
        &self,
        library_id: Option<&str>,
        offset: u32,
        limit: u32,
    ) -> Result<(Vec<Genre>, u64), ProviderError> {
        let response = self
            .client
            .get_music_genres(
                self.url(),
                self.token(),
                self.user_id(),
                library_id,
                offset,
                limit,
            )
            .await
            .map_err(Self::map_error)?;
        let total = response.total_record_count as u64;
        let genres = response.items.into_iter().map(genre_from_item).collect();
        Ok((genres, total))
    }

    async fn get_genre_tracks(
        &self,
        genre_id: &str,
        offset: u32,
        limit: u32,
    ) -> Result<(Vec<Song>, u32), ProviderError> {
        let response = self
            .client
            .get_songs_by_genre(
                self.url(),
                self.token(),
                self.user_id(),
                genre_id,
                offset,
                limit,
            )
            .await
            .map_err(Self::map_error)?;
        Ok((
            response.items.into_iter().map(song_from_item).collect(),
            response.total_record_count,
        ))
    }

    async fn list_recently_added(
        &self,
        library_id: Option<&str>,
        offset: u32,
        limit: u32,
    ) -> Result<(Vec<Album>, u32), ProviderError> {
        let response = self
            .client
            .get_recently_added_albums(
                self.url(),
                self.token(),
                self.user_id(),
                library_id,
                offset,
                limit,
            )
            .await
            .map_err(Self::map_error)?;
        Ok((
            response.items.into_iter().map(album_from_item).collect(),
            response.total_record_count,
        ))
    }

    async fn list_frequently_played(
        &self,
        library_id: Option<&str>,
        offset: u32,
        limit: u32,
    ) -> Result<(Vec<Song>, u32), ProviderError> {
        let response = self
            .client
            .get_frequently_played_songs(
                self.url(),
                self.token(),
                self.user_id(),
                library_id,
                offset,
                limit,
            )
            .await
            .map_err(Self::map_error)?;
        Ok((
            response.items.into_iter().map(song_from_item).collect(),
            response.total_record_count,
        ))
    }

    async fn list_recently_played(
        &self,
        library_id: Option<&str>,
        offset: u32,
        limit: u32,
    ) -> Result<(Vec<Song>, u32), ProviderError> {
        let response = self
            .client
            .get_recently_played_songs(
                self.url(),
                self.token(),
                self.user_id(),
                library_id,
                offset,
                limit,
            )
            .await
            .map_err(Self::map_error)?;
        Ok((
            response.items.into_iter().map(song_from_item).collect(),
            response.total_record_count,
        ))
    }

    async fn list_favorites(
        &self,
        library_id: Option<&str>,
        offset: u32,
        limit: u32,
    ) -> Result<(Vec<Song>, u32), ProviderError> {
        let response = self
            .client
            .get_favorite_songs(
                self.url(),
                self.token(),
                self.user_id(),
                library_id,
                offset,
                limit,
            )
            .await
            .map_err(Self::map_error)?;
        Ok((
            response.items.into_iter().map(song_from_item).collect(),
            response.total_record_count,
        ))
    }

    async fn list_favorite_items(
        &self,
        library_id: Option<&str>,
    ) -> Result<SearchResult, ProviderError> {
        let response = self
            .client
            .get_favorite_music_items(self.url(), self.token(), self.user_id(), library_id)
            .await
            .map_err(Self::map_error)?;
        let mut result = SearchResult::default();
        for item in response.items {
            match item.item_type.as_str() {
                "MusicArtist" => result.artists.push(artist_from_item(item)),
                "MusicAlbum" => result.albums.push(album_from_item(item)),
                "Audio" => result.songs.push(song_from_item(item)),
                _ => {}
            }
        }
        Ok(result)
    }

    async fn list_tracks(&self, filter: TrackListFilter) -> Result<TrackListPage, ProviderError> {
        let limit_param = if filter.limit > 0 {
            Some(filter.limit)
        } else {
            None
        };
        // Album implies its artist; when album_id is set, do not pass artist_id (AC 4).
        let (album_artist_ids, album_ids) =
            match (filter.album_id.as_deref(), filter.artist_id.as_deref()) {
                (Some(album), _) => (None, Some(album)),
                (None, Some(artist)) => (Some(artist), None),
                (None, None) => (None, None),
            };
        let response = self
            .client
            .get_items(
                self.url(),
                self.token(),
                self.user_id(),
                filter.library_id.as_deref(),
                Some("Audio"),
                Some(filter.start_index),
                limit_param,
                filter.letter.as_deref(),
                None,
                album_artist_ids,
                album_ids,
                Some("Name,Album"),
            )
            .await
            .map_err(Self::map_error)?;
        let total = response.total_record_count;
        let tracks: Vec<Song> = response.items.into_iter().map(song_from_item).collect();
        Ok(TrackListPage {
            tracks,
            total,
            start_index: filter.start_index,
            limit: filter.limit,
        })
    }

    async fn list_all_songs_page(
        &self,
        library_id: Option<&str>,
        offset: u32,
        limit: u32,
    ) -> Result<(Vec<Song>, u32), ProviderError> {
        // Unfiltered audio enumeration via the same /Items endpoint as `list_tracks`.
        // `get_items` already sets Recursive=true; `IncludeItemTypes=Audio` scopes to songs.
        let limit_param = (limit > 0).then_some(limit);
        let response = self
            .client
            .get_items(
                self.url(),
                self.token(),
                self.user_id(),
                library_id,
                Some("Audio"),
                Some(offset),
                limit_param,
                None,
                None,
                None,
                None,
                Some("Name"),
            )
            .await
            .map_err(Self::map_error)?;
        let total = response.total_record_count;
        let songs: Vec<Song> = response.items.into_iter().map(song_from_item).collect();
        Ok((songs, total))
    }
}

pub(crate) fn library_from_view(view: JellyfinView) -> Option<Library> {
    let is_music = view
        .collection_type
        .as_deref()
        .map(|collection| collection.eq_ignore_ascii_case("music"))
        .unwrap_or(false);
    if !is_music {
        return None;
    }

    Some(Library {
        cover_art_id: Some(view.id.clone()),
        id: view.id,
        name: view.name,
        item_type: ItemType::Library,
    })
}

pub(crate) fn artist_from_item(item: JellyfinItem) -> Artist {
    let cover_art_id = cover_art_id(&item);
    Artist {
        id: item.id.clone(),
        name: item.name,
        album_count: item.recursive_item_count,
        song_count: None,
        cover_art_id,
    }
}

pub(crate) fn album_from_item(item: JellyfinItem) -> Album {
    let cover_art_id = cover_art_id(&item);
    Album {
        id: item.id.clone(),
        title: item.name,
        artist_id: item
            .artist_items
            .as_ref()
            .and_then(|items| items.first())
            .map(|artist| artist.id.clone()),
        artist_name: item.album_artist,
        year: item.production_year,
        song_count: item.recursive_item_count,
        duration_seconds: item
            .cumulative_run_time_ticks
            .map(|ticks| u32::from(Seconds::from(JellyfinTicks(ticks)))),
        cover_art_id,
    }
}

pub(crate) fn playlist_from_item(item: JellyfinItem) -> Playlist {
    let cover_art_id = cover_art_id(&item);
    Playlist {
        id: item.id.clone(),
        name: item.name,
        song_count: item.recursive_item_count,
        duration_seconds: item
            .cumulative_run_time_ticks
            .map(|ticks| u32::from(Seconds::from(JellyfinTicks(ticks)))),
        cover_art_id,
    }
}

pub(crate) fn song_from_item(item: JellyfinItem) -> Song {
    let cover_art_id = cover_art_id(&item);
    let bitrate = item
        .bitrate
        .or_else(|| item.media_sources.as_ref()?.first()?.bitrate)
        .map(|bps| u32::from(Kbps::from(Bps(bps))));
    let artist = item.artist_items.as_ref().and_then(|items| items.first());
    let suffix = item
        .media_sources
        .as_ref()
        .and_then(|sources| sources.first())
        .and_then(|source| source.container.clone())
        .or_else(|| item.container.clone());

    Song {
        id: item.id.clone(),
        title: item.name,
        artist_id: artist.map(|artist| artist.id.clone()),
        artist_name: artist.map(|artist| artist.name.clone()).or_else(|| {
            item.artists
                .as_ref()
                .and_then(|artists| artists.first())
                .cloned()
                .or(item.album_artist.clone())
        }),
        album_id: item.album_id,
        album_title: item.album,
        duration_seconds: item
            .run_time_ticks
            .map(|ticks| u32::from(Seconds::from(JellyfinTicks(ticks))))
            .unwrap_or_default(),
        bitrate_kbps: bitrate,
        track_number: item.index_number,
        disc_number: item.parent_index_number,
        cover_art_id,
        date_added: item.date_created.clone(),
        last_played_at: item
            .user_data
            .as_ref()
            .and_then(|ud| ud.last_played_date.clone()),
        play_count: item.user_data.as_ref().map(|ud| ud.play_count),
        is_favorite: item.user_data.as_ref().map(|ud| ud.is_favorite),
        content_type: None,
        suffix,
        size_bytes: item
            .media_sources
            .as_ref()
            .and_then(|sources| sources.first())
            .and_then(|source| source.size)
            .and_then(|s| u64::try_from(s).ok()),
    }
}

pub(crate) fn genre_from_item(item: JellyfinItem) -> Genre {
    let cover_art_id = cover_art_id(&item);
    // /MusicGenres returns SongCount; /Genres (legacy) returns RecursiveItemCount
    let song_count = item.song_count.or(item.recursive_item_count);
    Genre {
        id: item.id,
        name: item.name,
        song_count,
        cover_art_id,
    }
}

fn change_event_from_item(item: JellyfinItem) -> Option<ChangeEvent> {
    let item_type = match item.item_type.as_str() {
        "Audio" | "MusicVideo" => ItemType::Song,
        "MusicAlbum" => ItemType::Album,
        "MusicArtist" => ItemType::Artist,
        "Playlist" => ItemType::Playlist,
        _ => return None,
    };

    Some(ChangeEvent {
        item: ItemRef {
            id: item.id,
            item_type,
        },
        change_type: ChangeType::Updated,
        version: item.etag,
    })
}

fn cover_art_id(item: &JellyfinItem) -> Option<String> {
    item.image_tags
        .as_ref()
        .and_then(|tags| tags.get("Primary"))
        .map(|_| item.id.clone())
}

fn transcode_profile_to_device_profile(profile: &TranscodeProfile) -> serde_json::Value {
    let container = profile.container.as_deref().unwrap_or("mp3");
    let audio_codec = profile.audio_codec.as_deref().unwrap_or(container);
    serde_json::json!({
        "MaxStreamingBitrate": profile.max_bitrate_kbps.map(|kbps| kbps * 1_000),
        "MusicStreamingTranscodingBitrate": profile.max_bitrate_kbps.map(|kbps| kbps * 1_000),
        "DirectPlayProfiles": [],
        "TranscodingProfiles": [
            {
                "Container": container,
                "Type": "Audio",
                "AudioCodec": audio_codec,
                "Protocol": "http",
                "EstimateContentLength": true,
                "EnableMpegtsM2TsMode": false
            }
        ],
        "CodecProfiles": []
    })
}

fn status_from_message(message: &str) -> Option<u16> {
    message.split("status: ").nth(1).and_then(|tail| {
        tail.split_whitespace().next().and_then(|s| {
            s.trim_end_matches(|c: char| !c.is_ascii_digit())
                .parse::<u16>()
                .ok()
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{JellyfinClient, JellyfinItem, JellyfinView, MediaSource};
    use crate::domain::models::{ChangeType, ItemType};
    use crate::providers::{
        BrowseCapabilities, BrowseMode, Capabilities, MediaProvider, ScrobbleSubmission, ServerType,
    };
    use mockito::{Matcher, Server};

    const TOKEN: &str = "test-token-1234567890";
    const USER_ID: &str = "user1";

    #[test]
    fn jellyfin_view_maps_music_library() {
        let view = JellyfinView {
            id: "lib1".to_string(),
            name: "Music".to_string(),
            view_type: "CollectionFolder".to_string(),
            collection_type: Some("music".to_string()),
        };

        let library = library_from_view(view).expect("music view should map");

        assert_eq!(library.id, "lib1");
        assert_eq!(library.name, "Music");
        assert_eq!(library.item_type, ItemType::Library);
        assert_eq!(library.cover_art_id.as_deref(), Some("lib1"));
    }

    #[test]
    fn jellyfin_item_maps_song_normalized_fields() {
        let item = JellyfinItem {
            id: "song-uuid".to_string(),
            name: "Track 1".to_string(),
            item_type: "Audio".to_string(),
            album: Some("Album A".to_string()),
            album_artist: Some("Artist A".to_string()),
            artists: Some(vec!["Artist A".to_string()]),
            index_number: Some(3),
            parent_index_number: Some(2),
            parent_id: Some("album-id".to_string()),
            album_id: Some("album-id".to_string()),
            artist_items: Some(vec![crate::api::NameIdPair {
                id: "artist-id".to_string(),
                name: "Artist A".to_string(),
            }]),
            container: None,
            production_year: None,
            recursive_item_count: None,
            song_count: None,
            cumulative_run_time_ticks: None,
            run_time_ticks: Some(215_000_0000),
            bitrate: Some(1_411_200),
            media_sources: Some(vec![MediaSource {
                size: Some(5_242_880),
                container: Some("flac".to_string()),
                bitrate: Some(1_411_200),
                media_streams: None,
            }]),
            image_tags: Some(std::collections::HashMap::from([(
                "Primary".to_string(),
                "image-etag".to_string(),
            )])),
            etag: Some("song-etag".to_string()),
            user_data: None,
            date_created: None,
            playlist_item_id: None,
        };

        let song = song_from_item(item);

        assert_eq!(song.id, "song-uuid");
        assert_eq!(song.duration_seconds, 215);
        assert_eq!(song.bitrate_kbps, Some(1_411));
        assert_eq!(song.track_number, Some(3));
        assert_eq!(song.disc_number, Some(2));
        assert_eq!(song.album_id.as_deref(), Some("album-id"));
        assert_eq!(song.artist_id.as_deref(), Some("artist-id"));
        assert_eq!(song.cover_art_id.as_deref(), Some("song-uuid"));
    }

    #[test]
    fn jellyfin_item_missing_optional_fields_remain_none() {
        let item = JellyfinItem {
            id: "song-uuid".to_string(),
            name: "Track 1".to_string(),
            item_type: "Audio".to_string(),
            album: None,
            album_artist: None,
            artists: None,
            index_number: None,
            parent_index_number: None,
            parent_id: None,
            album_id: None,
            artist_items: None,
            container: None,
            production_year: None,
            recursive_item_count: None,
            song_count: None,
            cumulative_run_time_ticks: None,
            run_time_ticks: None,
            bitrate: None,
            media_sources: None,
            image_tags: None,
            etag: None,
            user_data: None,
            date_created: None,
            playlist_item_id: None,
        };

        let song = song_from_item(item);

        assert_eq!(song.duration_seconds, 0);
        assert_eq!(song.bitrate_kbps, None);
        assert_eq!(song.track_number, None);
        assert_eq!(song.album_id, None);
        assert_eq!(song.artist_id, None);
        assert_eq!(song.cover_art_id, None);
    }

    #[tokio::test]
    async fn provider_exposes_capabilities() {
        let provider =
            JellyfinProvider::new(JellyfinClient::new(), "http://localhost", TOKEN, USER_ID);

        assert_eq!(provider.server_type(), ServerType::Jellyfin);
        assert_eq!(
            provider.capabilities(),
            Capabilities {
                open_subsonic: false,
                supports_changes_since: true,
                supports_server_transcoding: true,
                supports_playlist_write: true,
                browse: BrowseCapabilities {
                    list_modes: vec![
                        BrowseMode::Artists,
                        BrowseMode::Albums,
                        BrowseMode::Playlists,
                        BrowseMode::Tracks,
                        BrowseMode::Genres,
                        BrowseMode::RecentlyAdded,
                        BrowseMode::FrequentlyPlayed,
                        BrowseMode::RecentlyPlayed,
                        BrowseMode::Favorites,
                    ],
                },
            }
        );
    }

    #[tokio::test]
    async fn provider_lists_libraries_from_user_views() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let _mock = server
            .mock("GET", "/UserViews")
            .match_query(Matcher::UrlEncoded("userId".into(), USER_ID.into()))
            .match_header("X-Emby-Token", TOKEN)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Items":[{"Id":"lib1","Name":"Music","Type":"CollectionFolder","CollectionType":"music"},{"Id":"tv1","Name":"TV","Type":"CollectionFolder","CollectionType":"tvshows"}],"TotalRecordCount":2}"#)
            .create_async()
            .await;

        let provider = JellyfinProvider::new(JellyfinClient::new(), url, TOKEN, USER_ID);

        let libraries = provider.list_libraries().await.expect("libraries");

        assert_eq!(libraries.len(), 1);
        assert_eq!(libraries[0].id, "lib1");
    }

    #[tokio::test]
    async fn provider_get_album_returns_tracks() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let _album = server
            .mock("GET", "/Items/album1")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("userId".into(), USER_ID.into()),
                Matcher::UrlEncoded("Fields".into(), "RecursiveItemCount,CumulativeRunTimeTicks".into()),
            ]))
            .match_header("X-Emby-Token", TOKEN)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Id":"album1","Name":"Album","Type":"MusicAlbum","AlbumArtist":"Artist","ProductionYear":2024,"RecursiveItemCount":1,"CumulativeRunTimeTicks":100000000}"#)
            .create_async()
            .await;
        let _tracks = server
            .mock("GET", "/Items")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("userId".into(), USER_ID.into()),
                Matcher::UrlEncoded("ParentId".into(), "album1".into()),
                Matcher::UrlEncoded("IncludeItemTypes".into(), "Audio,MusicVideo".into()),
                Matcher::UrlEncoded("Fields".into(), "MediaSources".into()),
                Matcher::UrlEncoded("Recursive".into(), "true".into()),
            ]))
            .match_header("X-Emby-Token", TOKEN)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Items":[{"Id":"song1","Name":"Track","Type":"Audio","RunTimeTicks":100000000,"MediaSources":[{"Size":1000,"Bitrate":320000}]}],"TotalRecordCount":1,"StartIndex":0}"#)
            .create_async()
            .await;

        let provider = JellyfinProvider::new(JellyfinClient::new(), url, TOKEN, USER_ID);

        let album = provider.get_album("album1").await.expect("album");

        assert_eq!(album.album.id, "album1");
        assert_eq!(album.tracks.len(), 1);
        assert_eq!(album.tracks[0].bitrate_kbps, Some(320));
    }

    #[tokio::test]
    async fn provider_search_maps_song_results() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let _mock = server
            .mock("GET", "/Items")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("userId".into(), USER_ID.into()),
                Matcher::UrlEncoded("SearchTerm".into(), "Track".into()),
                Matcher::UrlEncoded("IncludeItemTypes".into(), "Audio".into()),
                Matcher::UrlEncoded("Recursive".into(), "true".into()),
                Matcher::UrlEncoded("Limit".into(), "25".into()),
                Matcher::UrlEncoded("Fields".into(), "Id,Name,Album,AlbumArtist,Artists,AlbumId".into()),
            ]))
            .match_header("X-Emby-Token", TOKEN)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Items":[{"Id":"song1","Name":"Track","Type":"Audio","Album":"Album","AlbumArtist":"Artist","RunTimeTicks":100000000}],"TotalRecordCount":1,"StartIndex":0}"#)
            .create_async()
            .await;

        let provider = JellyfinProvider::new(JellyfinClient::new(), url, TOKEN, USER_ID);

        let result = provider.search("Track").await.expect("search");

        assert_eq!(result.songs.len(), 1);
        assert_eq!(result.songs[0].title, "Track");
        assert!(result.artists.is_empty());
        assert!(result.albums.is_empty());
    }

    #[tokio::test]
    async fn list_all_songs_page_enumerates_audio() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let _mock = server
            .mock("GET", "/Items")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("userId".into(), USER_ID.into()),
                Matcher::UrlEncoded("IncludeItemTypes".into(), "Audio".into()),
                Matcher::UrlEncoded("Recursive".into(), "true".into()),
                Matcher::UrlEncoded("StartIndex".into(), "0".into()),
                Matcher::UrlEncoded("Limit".into(), "200".into()),
                Matcher::UrlEncoded("SortBy".into(), "Name".into()),
            ]))
            .match_header("X-Emby-Token", TOKEN)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Items":[{"Id":"song1","Name":"Track","Type":"Audio","RunTimeTicks":100000000}],"TotalRecordCount":42,"StartIndex":0}"#)
            .create_async()
            .await;

        let provider = JellyfinProvider::new(JellyfinClient::new(), url, TOKEN, USER_ID);

        let (songs, total) = provider
            .list_all_songs_page(None, 0, 200)
            .await
            .expect("list_all_songs_page");

        assert_eq!(total, 42);
        assert_eq!(songs.len(), 1);
        assert_eq!(songs[0].id, "song1");
        assert_eq!(songs[0].title, "Track");
    }

    #[tokio::test]
    async fn provider_lists_and_gets_playlist_tracks() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let _playlists = server
            .mock("GET", "/Items")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("userId".into(), USER_ID.into()),
                Matcher::UrlEncoded("IncludeItemTypes".into(), "Playlist".into()),
            ]))
            .match_header("X-Emby-Token", TOKEN)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Items":[{"Id":"playlist1","Name":"Road Trip","Type":"Playlist","RecursiveItemCount":1,"CumulativeRunTimeTicks":100000000}],"TotalRecordCount":1,"StartIndex":0}"#)
            .create_async()
            .await;
        let _playlist = server
            .mock("GET", "/Items/playlist1")
            .match_query(Matcher::UrlEncoded("userId".into(), USER_ID.into()))
            .match_header("X-Emby-Token", TOKEN)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Id":"playlist1","Name":"Road Trip","Type":"Playlist","RecursiveItemCount":1,"CumulativeRunTimeTicks":100000000}"#)
            .create_async()
            .await;
        let _tracks = server
            .mock("GET", "/Users/user1/Items")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("ParentId".into(), "playlist1".into()),
                Matcher::UrlEncoded("IncludeItemTypes".into(), "Audio,MusicVideo".into()),
                Matcher::UrlEncoded("Fields".into(), "MediaSources".into()),
                Matcher::UrlEncoded("Recursive".into(), "true".into()),
            ]))
            .match_header("X-Emby-Token", TOKEN)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Items":[{"Id":"song1","Name":"Track","Type":"Audio","RunTimeTicks":100000000}],"TotalRecordCount":1,"StartIndex":0}"#)
            .create_async()
            .await;

        let provider = JellyfinProvider::new(JellyfinClient::new(), url, TOKEN, USER_ID);

        let playlists = provider.list_playlists().await.expect("playlists");
        let playlist = provider.get_playlist("playlist1").await.expect("playlist");

        assert_eq!(playlists[0].name, "Road Trip");
        assert_eq!(playlist.playlist.duration_seconds, Some(10));
        assert_eq!(playlist.tracks[0].id, "song1");
    }

    #[tokio::test]
    async fn provider_creates_playlist_returns_server_id() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let _mock = server
            .mock("POST", "/Playlists")
            .match_header("X-Emby-Token", TOKEN)
            .match_body(Matcher::PartialJson(serde_json::json!({
                "Name": "Road Trip",
                "MediaType": "Audio",
                "Ids": ["song1", "song2"],
                "UserId": USER_ID,
            })))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Id":"playlist99","Name":"Road Trip"}"#)
            .create_async()
            .await;

        let provider = JellyfinProvider::new(JellyfinClient::new(), url, TOKEN, USER_ID);
        let id = provider
            .create_playlist("Road Trip", &["song1".to_string(), "song2".to_string()])
            .await
            .expect("create_playlist");

        assert_eq!(id, "playlist99");
    }

    #[tokio::test]
    async fn provider_renames_playlist_with_playlist_update_dto() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let _mock = server
            .mock("POST", "/Playlists/playlist99")
            .match_header("X-Emby-Token", TOKEN)
            .match_body(Matcher::Json(serde_json::json!({
                "Name": "Renamed Road Trip",
            })))
            .with_status(204)
            .expect(1)
            .create_async()
            .await;

        let provider = JellyfinProvider::new(JellyfinClient::new(), url, TOKEN, USER_ID);
        provider
            .rename_playlist("playlist99", "Renamed Road Trip")
            .await
            .expect("rename_playlist");
    }

    #[tokio::test]
    async fn provider_add_to_playlist_posts_ids() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let _mock = server
            .mock("POST", "/Playlists/playlist99/Items")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("Ids".into(), "song1,song2".into()),
                Matcher::UrlEncoded("userId".into(), USER_ID.into()),
            ]))
            .match_header("X-Emby-Token", TOKEN)
            .with_status(204)
            .create_async()
            .await;

        let provider = JellyfinProvider::new(JellyfinClient::new(), url, TOKEN, USER_ID);
        provider
            .add_to_playlist("playlist99", &["song1".to_string(), "song2".to_string()])
            .await
            .expect("add_to_playlist");
    }

    #[tokio::test]
    async fn provider_remove_from_playlist_resolves_entry_ids_then_deletes() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let _get = server
            .mock("GET", "/Playlists/playlist99/Items")
            .match_query(Matcher::UrlEncoded("userId".into(), USER_ID.into()))
            .match_header("X-Emby-Token", TOKEN)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"Items":[
                    {"Id":"song1","Name":"Track 1","Type":"Audio","PlaylistItemId":"entry-a"},
                    {"Id":"song2","Name":"Track 2","Type":"Audio","PlaylistItemId":"entry-b"},
                    {"Id":"song3","Name":"Track 3","Type":"Audio","PlaylistItemId":"entry-c"}
                ],"TotalRecordCount":3,"StartIndex":0}"#,
            )
            .create_async()
            .await;
        let _delete = server
            .mock("DELETE", "/Playlists/playlist99/Items")
            .match_query(Matcher::UrlEncoded(
                "EntryIds".into(),
                "entry-a,entry-b".into(),
            ))
            .match_header("X-Emby-Token", TOKEN)
            .with_status(204)
            .create_async()
            .await;

        let provider = JellyfinProvider::new(JellyfinClient::new(), url, TOKEN, USER_ID);
        provider
            .remove_from_playlist("playlist99", &["song1".to_string(), "song2".to_string()])
            .await
            .expect("remove_from_playlist");
    }

    #[tokio::test]
    async fn provider_remove_from_playlist_respects_requested_duplicate_count() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let _get = server
            .mock("GET", "/Playlists/playlist99/Items")
            .match_query(Matcher::UrlEncoded("userId".into(), USER_ID.into()))
            .match_header("X-Emby-Token", TOKEN)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"Items":[
                    {"Id":"song1","Name":"Track 1","Type":"Audio","PlaylistItemId":"entry-a"},
                    {"Id":"song1","Name":"Track 1","Type":"Audio","PlaylistItemId":"entry-b"},
                    {"Id":"song2","Name":"Track 2","Type":"Audio","PlaylistItemId":"entry-c"}
                ],"TotalRecordCount":3,"StartIndex":0}"#,
            )
            .create_async()
            .await;
        let _delete = server
            .mock("DELETE", "/Playlists/playlist99/Items")
            .match_query(Matcher::UrlEncoded("EntryIds".into(), "entry-a".into()))
            .match_header("X-Emby-Token", TOKEN)
            .with_status(204)
            .create_async()
            .await;

        let provider = JellyfinProvider::new(JellyfinClient::new(), url, TOKEN, USER_ID);
        provider
            .remove_from_playlist("playlist99", &["song1".to_string()])
            .await
            .expect("remove_from_playlist should delete one duplicate");
    }

    #[tokio::test]
    async fn provider_remove_from_playlist_errors_when_match_lacks_playlist_item_id() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let _get = server
            .mock("GET", "/Playlists/playlist99/Items")
            .match_query(Matcher::UrlEncoded("userId".into(), USER_ID.into()))
            .match_header("X-Emby-Token", TOKEN)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"Items":[
                    {"Id":"song1","Name":"Track 1","Type":"Audio"}
                ],"TotalRecordCount":1,"StartIndex":0}"#,
            )
            .create_async()
            .await;

        let provider = JellyfinProvider::new(JellyfinClient::new(), url, TOKEN, USER_ID);
        let result = provider
            .remove_from_playlist("playlist99", &["song1".to_string()])
            .await;

        assert!(
            matches!(result, Err(ProviderError::Deserialization(_))),
            "missing PlaylistItemId should be treated as malformed provider response, got: {:?}",
            result
        );
    }

    #[tokio::test]
    async fn provider_remove_from_playlist_skips_delete_when_no_entries_match() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let _get = server
            .mock("GET", "/Playlists/playlist99/Items")
            .match_query(Matcher::UrlEncoded("userId".into(), USER_ID.into()))
            .match_header("X-Emby-Token", TOKEN)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Items":[{"Id":"song3","Name":"Track 3","Type":"Audio","PlaylistItemId":"entry-c"}],"TotalRecordCount":1,"StartIndex":0}"#)
            .create_async()
            .await;
        // No DELETE mock — if DELETE is issued the test would fail (unexpected request)

        let provider = JellyfinProvider::new(JellyfinClient::new(), url, TOKEN, USER_ID);
        // Removing tracks that are NOT in the playlist → should silently succeed
        provider
            .remove_from_playlist("playlist99", &["song1".to_string()])
            .await
            .expect("remove_from_playlist when no match should succeed");
    }

    #[tokio::test]
    async fn provider_delete_playlist_issues_delete_items_endpoint() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let _mock = server
            .mock("DELETE", "/Items/playlist99")
            .match_header("X-Emby-Token", TOKEN)
            .with_status(204)
            .create_async()
            .await;

        let provider = JellyfinProvider::new(JellyfinClient::new(), url, TOKEN, USER_ID);
        provider
            .delete_playlist("playlist99")
            .await
            .expect("delete_playlist");
    }

    #[tokio::test]
    async fn provider_download_url_uses_direct_download_without_profile() {
        let provider = JellyfinProvider::new(
            JellyfinClient::new(),
            "http://host/jellyfin/",
            TOKEN,
            USER_ID,
        );

        let url = provider.download_url("song1", None).await.expect("url");

        assert_eq!(url, format!("http://host/jellyfin/Items/song1/Download?api_key={TOKEN}"));
    }

    #[tokio::test]
    async fn provider_download_url_uses_playback_info_transcoding_url() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let _mock = server
            .mock("POST", "/Items/song1/PlaybackInfo")
            .match_query(Matcher::UrlEncoded("userId".into(), USER_ID.into()))
            .match_header("X-Emby-Token", TOKEN)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"MediaSources":[{"SupportsDirectPlay":false,"TranscodingUrl":"/Videos/song1/stream.mp3?api_key=redacted"}]}"#,
            )
            .create_async()
            .await;

        let provider = JellyfinProvider::new(JellyfinClient::new(), url.clone(), TOKEN, USER_ID);

        let resolved = provider
            .download_url(
                "song1",
                Some(&crate::providers::TranscodeProfile {
                    container: Some("mp3".to_string()),
                    audio_codec: Some("mp3".to_string()),
                    max_bitrate_kbps: Some(320),
                }),
            )
            .await
            .expect("url");

        assert_eq!(
            resolved,
            format!("{url}/Videos/song1/stream.mp3?api_key=redacted")
        );
    }

    #[tokio::test]
    async fn provider_cover_art_url_uses_primary_image_endpoint() {
        let provider = JellyfinProvider::new(
            JellyfinClient::new(),
            "http://host/jellyfin",
            TOKEN,
            USER_ID,
        );

        let url = provider.cover_art_url("item1").await.expect("url");

        assert_eq!(url, "http://host/jellyfin/Items/item1/Images/Primary");
    }

    #[tokio::test]
    async fn provider_scrobble_reports_played() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let _mock = server
            .mock("POST", "/UserPlayedItems/song1")
            .match_query(Matcher::UrlEncoded("userId".into(), USER_ID.into()))
            .match_header("X-Emby-Token", TOKEN)
            .with_status(204)
            .create_async()
            .await;

        let provider = JellyfinProvider::new(JellyfinClient::new(), url, TOKEN, USER_ID);

        provider
            .scrobble(crate::providers::ScrobbleRequest {
                song_id: "song1".to_string(),
                submission: ScrobbleSubmission::Played,
                position_seconds: None,
                played_at_unix_seconds: None,
            })
            .await
            .expect("scrobble");
    }

    #[tokio::test]
    async fn provider_changes_since_sends_min_date_last_saved() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let _mock = server
            .mock("GET", "/Items")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("userId".into(), USER_ID.into()),
                Matcher::UrlEncoded("minDateLastSaved".into(), "2026-05-09T10:00:00Z".into()),
                Matcher::UrlEncoded("Fields".into(), "MediaSources".into()),
            ]))
            .match_header("X-Emby-Token", TOKEN)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Items":[{"Id":"song1","Name":"Track","Type":"Audio","Etag":"v1"},{"Id":"album1","Name":"Album","Type":"MusicAlbum"}],"TotalRecordCount":2,"StartIndex":0}"#)
            .create_async()
            .await;

        let provider = JellyfinProvider::new(JellyfinClient::new(), url, TOKEN, USER_ID);

        let changes = provider
            .changes_since(Some("2026-05-09T10:00:00Z"))
            .await
            .expect("changes");

        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].item.item_type, ItemType::Song);
        assert_eq!(changes[0].change_type, ChangeType::Updated);
        assert_eq!(changes[0].version.as_deref(), Some("v1"));
        assert_eq!(changes[1].item.item_type, ItemType::Album);
    }

    #[tokio::test]
    async fn provider_changes_since_accepts_missing_token() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let _mock = server
            .mock("GET", "/Items")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("userId".into(), USER_ID.into()),
                Matcher::UrlEncoded("Fields".into(), "MediaSources".into()),
            ]))
            .match_header("X-Emby-Token", TOKEN)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Items":[],"TotalRecordCount":0,"StartIndex":0}"#)
            .create_async()
            .await;

        let provider = JellyfinProvider::new(JellyfinClient::new(), url, TOKEN, USER_ID);

        let changes = provider.changes_since(None).await.expect("changes");

        assert!(changes.is_empty());
    }

    #[tokio::test]
    async fn provider_changes_since_invalid_token_returns_error() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let _mock = server
            .mock("GET", "/Items")
            .match_query(Matcher::Any)
            .match_header("X-Emby-Token", TOKEN)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body("not valid json{{{")
            .create_async()
            .await;

        let provider = JellyfinProvider::new(JellyfinClient::new(), url, TOKEN, USER_ID);

        let result = provider
            .changes_since(Some("not-a-valid-iso-timestamp"))
            .await;

        assert!(
            matches!(result, Err(ProviderError::Deserialization(_))),
            "invalid response body should map to Deserialization error, got: {:?}",
            result
        );
    }

    #[tokio::test]
    async fn provider_list_artists_returns_artist_list() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let _mock = server
            .mock("GET", "/Artists/AlbumArtists")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("userId".into(), USER_ID.into()),
                Matcher::UrlEncoded("SortBy".into(), "SortName".into()),
            ]))
            .match_header("X-Emby-Token", TOKEN)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Items":[{"Id":"artist1","Name":"The Beatles","Type":"MusicArtist","RecursiveItemCount":13}],"TotalRecordCount":1,"StartIndex":0}"#)
            .create_async()
            .await;

        let provider = JellyfinProvider::new(JellyfinClient::new(), url, TOKEN, USER_ID);

        let (artists, total) = provider
            .list_artists(None, None, 0, 0)
            .await
            .expect("artists");

        assert_eq!(total, 1);
        assert_eq!(artists.len(), 1);
        assert_eq!(artists[0].id, "artist1");
        assert_eq!(artists[0].name, "The Beatles");
        assert_eq!(artists[0].album_count, Some(13));
    }

    #[tokio::test]
    async fn provider_get_artist_uses_album_artist_ids_query() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let _artist = server
            .mock("GET", "/Items/artist1")
            .match_query(Matcher::UrlEncoded("userId".into(), USER_ID.into()))
            .match_header("X-Emby-Token", TOKEN)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Id":"artist1","Name":"The Beatles","Type":"MusicArtist","RecursiveItemCount":13}"#)
            .create_async()
            .await;
        let _albums = server
            .mock("GET", "/Items")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("userId".into(), USER_ID.into()),
                Matcher::UrlEncoded("AlbumArtistIds".into(), "artist1".into()),
                Matcher::UrlEncoded("IncludeItemTypes".into(), "MusicAlbum".into()),
                Matcher::UrlEncoded("Recursive".into(), "true".into()),
            ]))
            .match_header("X-Emby-Token", TOKEN)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Items":[{"Id":"album1","Name":"Abbey Road","Type":"MusicAlbum","ProductionYear":1969}],"TotalRecordCount":1,"StartIndex":0}"#)
            .create_async()
            .await;

        let provider = JellyfinProvider::new(JellyfinClient::new(), url, TOKEN, USER_ID);

        let artist = provider.get_artist("artist1").await.expect("artist");

        assert_eq!(artist.artist.id, "artist1");
        assert_eq!(artist.albums.len(), 1);
        assert_eq!(artist.albums[0].id, "album1");
    }

    #[tokio::test]
    async fn provider_maps_401_to_auth_error() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let _mock = server
            .mock("GET", "/UserViews")
            .match_query(Matcher::Any)
            .match_header("X-Emby-Token", TOKEN)
            .with_status(401)
            .with_body("Unauthorized")
            .create_async()
            .await;

        let provider = JellyfinProvider::new(JellyfinClient::new(), url, TOKEN, USER_ID);

        let result = provider.list_libraries().await;

        assert!(
            matches!(result, Err(ProviderError::Auth(_))),
            "HTTP 401 should map to ProviderError::Auth, got: {:?}",
            result
        );
    }

    #[tokio::test]
    async fn provider_maps_404_to_not_found_error() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let _mock = server
            .mock("GET", "/Items/missing-id")
            .match_query(Matcher::Any)
            .match_header("X-Emby-Token", TOKEN)
            .with_status(404)
            .with_body("Not Found")
            .create_async()
            .await;

        let provider = JellyfinProvider::new(JellyfinClient::new(), url, TOKEN, USER_ID);

        let result = provider.get_album("missing-id").await;

        assert!(
            matches!(result, Err(ProviderError::NotFound { .. })),
            "HTTP 404 should map to ProviderError::NotFound, got: {:?}",
            result
        );
    }

    #[tokio::test]
    async fn provider_maps_malformed_json_to_deserialization_error() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let _mock = server
            .mock("GET", "/UserViews")
            .match_query(Matcher::Any)
            .match_header("X-Emby-Token", TOKEN)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body("{not valid json}")
            .create_async()
            .await;

        let provider = JellyfinProvider::new(JellyfinClient::new(), url, TOKEN, USER_ID);

        let result = provider.list_libraries().await;

        assert!(
            matches!(result, Err(ProviderError::Deserialization(_))),
            "malformed JSON should map to ProviderError::Deserialization, got: {:?}",
            result
        );
    }

    #[tokio::test]
    async fn provider_scrobble_playing_returns_unsupported_capability() {
        let provider =
            JellyfinProvider::new(JellyfinClient::new(), "http://localhost", TOKEN, USER_ID);

        let result = provider
            .scrobble(crate::providers::ScrobbleRequest {
                song_id: "song1".to_string(),
                submission: ScrobbleSubmission::Playing,
                position_seconds: None,
                played_at_unix_seconds: None,
            })
            .await;

        assert!(
            matches!(result, Err(ProviderError::UnsupportedCapability(_))),
            "ScrobbleSubmission::Playing should return UnsupportedCapability, got: {:?}",
            result
        );
    }

    #[tokio::test]
    async fn provider_list_genres_calls_music_genres_endpoint() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let _mock = server
            .mock("GET", "/MusicGenres")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("userId".into(), USER_ID.into()),
                Matcher::UrlEncoded("Recursive".into(), "true".into()),
                Matcher::UrlEncoded("StartIndex".into(), "0".into()),
                Matcher::UrlEncoded("Limit".into(), "50".into()),
                Matcher::UrlEncoded("Fields".into(), "RecursiveItemCount".into()),
            ]))
            .match_header("X-Emby-Token", TOKEN)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Items":[{"Id":"genre1","Name":"Rock","Type":"MusicGenre","RecursiveItemCount":42,"ImageTags":{"Primary":"abc123"}}],"TotalRecordCount":1,"StartIndex":0}"#)
            .create_async()
            .await;

        let provider = JellyfinProvider::new(JellyfinClient::new(), url, TOKEN, USER_ID);
        let (genres, total) = provider.list_genres(None, 0, 50).await.expect("genres");

        assert_eq!(total, 1);
        assert_eq!(genres.len(), 1);
        assert_eq!(genres[0].id, "genre1");
        assert_eq!(genres[0].name, "Rock");
        assert_eq!(genres[0].song_count, Some(42));
        assert_eq!(genres[0].cover_art_id.as_deref(), Some("genre1")); // ImageTags.Primary present → id used
    }

    #[tokio::test]
    async fn provider_get_genre_tracks_uses_genre_ids_param() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let _mock = server
            .mock("GET", "/Items")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("userId".into(), USER_ID.into()),
                Matcher::UrlEncoded("GenreIds".into(), "genre1".into()),
                Matcher::UrlEncoded("Fields".into(), "MediaSources,UserData,DateCreated".into()),
            ]))
            .match_header("X-Emby-Token", TOKEN)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Items":[{"Id":"song1","Name":"Rock Track","Type":"Audio","RunTimeTicks":200000000}],"TotalRecordCount":1,"StartIndex":0}"#)
            .create_async()
            .await;

        let provider = JellyfinProvider::new(JellyfinClient::new(), url, TOKEN, USER_ID);
        let (tracks, total) = provider
            .get_genre_tracks("genre1", 0, 50)
            .await
            .expect("tracks");

        assert_eq!(total, 1);
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].id, "song1");
    }

    #[tokio::test]
    async fn provider_list_recently_added_returns_albums_sorted_by_date_created_descending() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let _mock = server
            .mock("GET", "/Items")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("userId".into(), USER_ID.into()),
                Matcher::UrlEncoded("IncludeItemTypes".into(), "MusicAlbum".into()),
                Matcher::UrlEncoded("SortBy".into(), "DateCreated".into()),
                Matcher::UrlEncoded("SortOrder".into(), "Descending".into()),
            ]))
            .match_header("X-Emby-Token", TOKEN)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Items":[{"Id":"album1","Name":"New Album","Type":"MusicAlbum","AlbumArtist":"The Artist","ProductionYear":2024}],"TotalRecordCount":1,"StartIndex":0}"#)
            .create_async()
            .await;

        let provider = JellyfinProvider::new(JellyfinClient::new(), url, TOKEN, USER_ID);
        let (albums, total) = provider
            .list_recently_added(None, 0, 50)
            .await
            .expect("albums");

        assert_eq!(total, 1);
        assert_eq!(albums.len(), 1);
        assert_eq!(albums[0].id, "album1");
        assert_eq!(albums[0].title, "New Album");
        assert_eq!(albums[0].artist_name.as_deref(), Some("The Artist"));
    }

    #[tokio::test]
    async fn provider_list_frequently_played_sorts_by_play_count_descending() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let _mock = server
            .mock("GET", "/Items")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("userId".into(), USER_ID.into()),
                Matcher::UrlEncoded("SortBy".into(), "PlayCount".into()),
                Matcher::UrlEncoded("SortOrder".into(), "Descending".into()),
                Matcher::UrlEncoded("Fields".into(), "MediaSources,UserData,DateCreated".into()),
            ]))
            .match_header("X-Emby-Token", TOKEN)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Items":[{"Id":"song1","Name":"Played Track","Type":"Audio","UserData":{"PlayCount":15,"IsFavorite":false}}],"TotalRecordCount":1,"StartIndex":0}"#)
            .create_async()
            .await;

        let provider = JellyfinProvider::new(JellyfinClient::new(), url, TOKEN, USER_ID);
        let (tracks, total) = provider
            .list_frequently_played(None, 0, 50)
            .await
            .expect("tracks");

        assert_eq!(total, 1);
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].id, "song1");
        assert_eq!(tracks[0].play_count, Some(15));
    }

    #[tokio::test]
    async fn provider_list_recently_played_sorts_by_date_played_descending() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let _mock = server
            .mock("GET", "/Items")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("userId".into(), USER_ID.into()),
                Matcher::UrlEncoded("SortBy".into(), "DatePlayed".into()),
                Matcher::UrlEncoded("SortOrder".into(), "Descending".into()),
                Matcher::UrlEncoded("Fields".into(), "MediaSources,UserData,DateCreated".into()),
            ]))
            .match_header("X-Emby-Token", TOKEN)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Items":[{"Id":"song1","Name":"Played Track","Type":"Audio","UserData":{"PlayCount":3,"IsFavorite":false,"LastPlayedDate":"2024-04-01T10:00:00Z"}}],"TotalRecordCount":1,"StartIndex":0}"#)
            .create_async()
            .await;

        let provider = JellyfinProvider::new(JellyfinClient::new(), url, TOKEN, USER_ID);
        let (tracks, total) = provider
            .list_recently_played(None, 0, 50)
            .await
            .expect("tracks");

        assert_eq!(total, 1);
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].id, "song1");
        assert_eq!(
            tracks[0].last_played_at.as_deref(),
            Some("2024-04-01T10:00:00Z")
        );
    }

    #[tokio::test]
    async fn provider_list_favorites_filters_is_favorite_true() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let _mock = server
            .mock("GET", "/Items")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("userId".into(), USER_ID.into()),
                Matcher::UrlEncoded("IsFavorite".into(), "true".into()),
                Matcher::UrlEncoded("Fields".into(), "MediaSources,UserData,DateCreated".into()),
            ]))
            .match_header("X-Emby-Token", TOKEN)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Items":[{"Id":"fav1","Name":"Favorite Track","Type":"Audio","UserData":{"PlayCount":0,"IsFavorite":true}}],"TotalRecordCount":1,"StartIndex":0}"#)
            .create_async()
            .await;

        let provider = JellyfinProvider::new(JellyfinClient::new(), url, TOKEN, USER_ID);
        let (tracks, total) = provider.list_favorites(None, 0, 50).await.expect("tracks");

        assert_eq!(total, 1);
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].id, "fav1");
        assert_eq!(tracks[0].is_favorite, Some(true));
    }

    #[tokio::test]
    async fn provider_list_favorite_items_maps_artists_albums_and_songs() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let _mock = server
            .mock("GET", "/Items")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("userId".into(), USER_ID.into()),
                Matcher::UrlEncoded("IncludeItemTypes".into(), "MusicArtist,MusicAlbum,Audio".into()),
                Matcher::UrlEncoded("IsFavorite".into(), "true".into()),
                Matcher::UrlEncoded("SortBy".into(), "SortName".into()),
                Matcher::UrlEncoded("SortOrder".into(), "Ascending".into()),
            ]))
            .match_header("X-Emby-Token", TOKEN)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"Items":[
                    {"Id":"artist1","Name":"Favorite Artist","Type":"MusicArtist","RecursiveItemCount":2},
                    {"Id":"album1","Name":"Favorite Album","Type":"MusicAlbum","AlbumArtist":"Favorite Artist","ArtistItems":[{"Id":"artist1","Name":"Favorite Artist"}],"RecursiveItemCount":10},
                    {"Id":"song1","Name":"Favorite Track","Type":"Audio","AlbumId":"album1","Album":"Favorite Album","ArtistItems":[{"Id":"artist1","Name":"Favorite Artist"}],"RunTimeTicks":200000000,"UserData":{"IsFavorite":true}}
                ],"TotalRecordCount":3,"StartIndex":0}"#,
            )
            .create_async()
            .await;

        let provider = JellyfinProvider::new(JellyfinClient::new(), url, TOKEN, USER_ID);
        let favorites = provider
            .list_favorite_items(None)
            .await
            .expect("favorite items");

        assert_eq!(favorites.artists.len(), 1);
        assert_eq!(favorites.albums.len(), 1);
        assert_eq!(favorites.songs.len(), 1);
        assert_eq!(favorites.artists[0].id, "artist1");
        assert_eq!(favorites.albums[0].id, "album1");
        assert_eq!(favorites.albums[0].artist_id.as_deref(), Some("artist1"));
        assert_eq!(favorites.songs[0].id, "song1");
        assert_eq!(favorites.songs[0].album_id.as_deref(), Some("album1"));
        assert_eq!(favorites.songs[0].artist_id.as_deref(), Some("artist1"));
        assert_eq!(favorites.songs[0].is_favorite, Some(true));
    }

    #[tokio::test]
    async fn provider_reorder_playlist_issues_move_calls_in_selection_sort_order() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let _get = server
            .mock("GET", "/Playlists/playlist99/Items")
            .match_query(Matcher::UrlEncoded("userId".into(), USER_ID.into()))
            .match_header("X-Emby-Token", TOKEN)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"Items":[
                    {"Id":"song1","Name":"Track 1","Type":"Audio","PlaylistItemId":"entry-a"},
                    {"Id":"song2","Name":"Track 2","Type":"Audio","PlaylistItemId":"entry-b"}
                ],"TotalRecordCount":2,"StartIndex":0}"#,
            )
            .create_async()
            .await;
        // Want order: song2, song1 → selection sort moves entry-b to index 0
        let _move = server
            .mock("POST", "/Playlists/playlist99/Items/entry-b/Move/0")
            .match_header("X-Emby-Token", TOKEN)
            .with_status(204)
            .create_async()
            .await;

        let provider = JellyfinProvider::new(JellyfinClient::new(), url, TOKEN, USER_ID);
        provider
            .reorder_playlist("playlist99", &["song2".to_string(), "song1".to_string()])
            .await
            .expect("reorder_playlist");
    }

    #[tokio::test]
    async fn provider_reorder_playlist_issues_no_moves_when_already_sorted() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let _get = server
            .mock("GET", "/Playlists/playlist99/Items")
            .match_query(Matcher::UrlEncoded("userId".into(), USER_ID.into()))
            .match_header("X-Emby-Token", TOKEN)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"Items":[
                    {"Id":"song1","Name":"Track 1","Type":"Audio","PlaylistItemId":"entry-a"},
                    {"Id":"song2","Name":"Track 2","Type":"Audio","PlaylistItemId":"entry-b"}
                ],"TotalRecordCount":2,"StartIndex":0}"#,
            )
            .create_async()
            .await;
        // No POST mock — if a Move is issued the test would fail (unexpected request)

        let provider = JellyfinProvider::new(JellyfinClient::new(), url, TOKEN, USER_ID);
        provider
            .reorder_playlist("playlist99", &["song1".to_string(), "song2".to_string()])
            .await
            .expect("reorder_playlist already sorted should succeed without moves");
    }

    #[tokio::test]
    async fn provider_reorder_playlist_multi_move_keeps_local_mirror_in_sync() {
        // 3 entries requiring 2 moves — exercises the local-mirror index math across
        // successive moves (a single-move test cannot catch a mirror/server desync). (Code review 11.9)
        let mut server = Server::new_async().await;
        let url = server.url();
        let _get = server
            .mock("GET", "/Playlists/playlist99/Items")
            .match_query(Matcher::UrlEncoded("userId".into(), USER_ID.into()))
            .match_header("X-Emby-Token", TOKEN)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"Items":[
                    {"Id":"song1","Name":"Track 1","Type":"Audio","PlaylistItemId":"entry-a"},
                    {"Id":"song2","Name":"Track 2","Type":"Audio","PlaylistItemId":"entry-b"},
                    {"Id":"song3","Name":"Track 3","Type":"Audio","PlaylistItemId":"entry-c"}
                ],"TotalRecordCount":3,"StartIndex":0}"#,
            )
            .create_async()
            .await;
        // Want order song3, song2, song1 → selection sort issues exactly:
        //   move entry-c to index 0, then (mirror updated) move entry-b to index 1.
        let move_c = server
            .mock("POST", "/Playlists/playlist99/Items/entry-c/Move/0")
            .match_header("X-Emby-Token", TOKEN)
            .with_status(204)
            .expect(1)
            .create_async()
            .await;
        let move_b = server
            .mock("POST", "/Playlists/playlist99/Items/entry-b/Move/1")
            .match_header("X-Emby-Token", TOKEN)
            .with_status(204)
            .expect(1)
            .create_async()
            .await;

        let provider = JellyfinProvider::new(JellyfinClient::new(), url, TOKEN, USER_ID);
        provider
            .reorder_playlist(
                "playlist99",
                &[
                    "song3".to_string(),
                    "song2".to_string(),
                    "song1".to_string(),
                ],
            )
            .await
            .expect("reorder_playlist");

        move_c.assert_async().await;
        move_b.assert_async().await;
    }

    #[tokio::test]
    async fn provider_list_tracks_by_artist_uses_album_artist_ids() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let _mock = server
            .mock("GET", "/Items")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("userId".into(), USER_ID.into()),
                Matcher::UrlEncoded("AlbumArtistIds".into(), "artist1".into()),
                Matcher::UrlEncoded("IncludeItemTypes".into(), "Audio".into()),
                Matcher::UrlEncoded("Recursive".into(), "true".into()),
            ]))
            .match_header("X-Emby-Token", TOKEN)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Items":[{"Id":"track1","Name":"Highway to Hell","Type":"Audio","Artists":["AC/DC"],"AlbumArtist":"AC/DC","Album":"Highway to Hell","Duration":208}],"TotalRecordCount":1,"StartIndex":0}"#)
            .create_async()
            .await;

        let provider = JellyfinProvider::new(JellyfinClient::new(), url, TOKEN, USER_ID);

        let page = provider
            .list_tracks(TrackListFilter {
                artist_id: Some("artist1".to_string()),
                album_id: None,
                library_id: None,
                letter: None,
                start_index: 0,
                limit: 50,
            })
            .await
            .expect("list_tracks");

        assert_eq!(page.tracks.len(), 1);
        assert_eq!(page.tracks[0].id, "track1");
        assert_eq!(page.total, 1);
    }
}
