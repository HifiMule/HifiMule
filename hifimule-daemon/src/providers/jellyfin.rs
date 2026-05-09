use crate::api::{JellyfinClient, JellyfinItem, JellyfinView};
use crate::domain::models::{
    Album, AlbumWithTracks, Artist, ArtistWithAlbums, Bps, ChangeEvent, ChangeType, ItemRef,
    ItemType, JellyfinTicks, Kbps, Library, Playlist, PlaylistWithTracks, SearchResult, Seconds,
    Song,
};
use crate::providers::{
    Capabilities, MediaProvider, ProviderChangeContext, ProviderError, ScrobbleRequest,
    ScrobbleSubmission, ServerType, TranscodeProfile,
};
use async_trait::async_trait;

const ARTIST_TYPES: &str = "MusicArtist";
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
        }
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

    async fn list_artists(&self, library_id: Option<&str>) -> Result<Vec<Artist>, ProviderError> {
        let response = self
            .client
            .get_items(
                self.url(),
                self.token(),
                self.user_id(),
                library_id,
                Some(ARTIST_TYPES),
                None,
                None,
                None,
                None,
            )
            .await
            .map_err(Self::map_error)?;

        Ok(response.items.into_iter().map(artist_from_item).collect())
    }

    async fn get_artist(&self, artist_id: &str) -> Result<ArtistWithAlbums, ProviderError> {
        let item = self
            .client
            .get_item_details(self.url(), self.token(), self.user_id(), artist_id)
            .await
            .map_err(|err| Self::map_not_found(err, "artist", artist_id))?;
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

    async fn list_albums(&self, library_id: Option<&str>) -> Result<Vec<Album>, ProviderError> {
        let response = self
            .client
            .get_items(
                self.url(),
                self.token(),
                self.user_id(),
                library_id,
                Some(ALBUM_TYPES),
                None,
                None,
                None,
                None,
            )
            .await
            .map_err(Self::map_error)?;

        Ok(response.items.into_iter().map(album_from_item).collect())
    }

    async fn get_album(&self, album_id: &str) -> Result<AlbumWithTracks, ProviderError> {
        let album = self
            .client
            .get_item_with_media_sources(self.url(), self.token(), self.user_id(), album_id)
            .await
            .map_err(|err| Self::map_not_found(err, "album", album_id))?;
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
        let tracks = self
            .client
            .get_child_items_with_sizes(self.url(), self.token(), self.user_id(), playlist_id)
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
        if let Some(profile) = profile {
            let profile = transcode_profile_to_device_profile(profile);
            self.client
                .resolve_stream_url(self.url(), self.token(), self.user_id(), song_id, &profile)
                .await
                .map_err(Self::map_error)
        } else {
            Ok(format!(
                "{}/Items/{}/Download",
                self.url().trim_end_matches('/'),
                song_id
            ))
        }
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

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            open_subsonic: false,
            supports_changes_since: true,
            supports_server_transcoding: true,
        }
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
    use crate::providers::{Capabilities, MediaProvider, ScrobbleSubmission, ServerType};
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
            cumulative_run_time_ticks: None,
            run_time_ticks: Some(215_000_0000),
            bitrate: Some(1_411_200),
            media_sources: Some(vec![MediaSource {
                size: Some(5_242_880),
                container: Some("flac".to_string()),
                bitrate: Some(1_411_200),
            }]),
            image_tags: Some(std::collections::HashMap::from([(
                "Primary".to_string(),
                "image-etag".to_string(),
            )])),
            etag: Some("song-etag".to_string()),
            user_data: None,
            date_created: None,
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
            cumulative_run_time_ticks: None,
            run_time_ticks: None,
            bitrate: None,
            media_sources: None,
            image_tags: None,
            etag: None,
            user_data: None,
            date_created: None,
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
                Matcher::UrlEncoded("Fields".into(), "MediaSources".into()),
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
                Matcher::UrlEncoded("Limit".into(), "10".into()),
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
            .mock("GET", "/Items")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("userId".into(), USER_ID.into()),
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
    async fn provider_download_url_uses_direct_download_without_profile() {
        let provider = JellyfinProvider::new(
            JellyfinClient::new(),
            "http://host/jellyfin/",
            TOKEN,
            USER_ID,
        );

        let url = provider.download_url("song1", None).await.expect("url");

        assert_eq!(url, "http://host/jellyfin/Items/song1/Download");
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
            .mock("GET", "/Items")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("userId".into(), USER_ID.into()),
                Matcher::UrlEncoded("IncludeItemTypes".into(), "MusicArtist".into()),
            ]))
            .match_header("X-Emby-Token", TOKEN)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Items":[{"Id":"artist1","Name":"The Beatles","Type":"MusicArtist","RecursiveItemCount":13}],"TotalRecordCount":1,"StartIndex":0}"#)
            .create_async()
            .await;

        let provider = JellyfinProvider::new(JellyfinClient::new(), url, TOKEN, USER_ID);

        let artists = provider.list_artists(None).await.expect("artists");

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
}
