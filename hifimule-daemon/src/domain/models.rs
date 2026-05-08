use serde::{Deserialize, Serialize};

const JELLYFIN_TICKS_PER_SECOND: u64 = 10_000_000;
const BITS_PER_KILOBIT: u32 = 1_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Library {
    pub id: String,
    pub name: String,
    pub item_type: ItemType,
    pub cover_art_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Song {
    pub id: String,
    pub title: String,
    pub artist_id: Option<String>,
    pub artist_name: Option<String>,
    pub album_id: Option<String>,
    pub album_title: Option<String>,
    pub duration_seconds: u32,
    pub bitrate_kbps: Option<u32>,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub cover_art_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Album {
    pub id: String,
    pub title: String,
    pub artist_id: Option<String>,
    pub artist_name: Option<String>,
    pub year: Option<u32>,
    pub song_count: Option<u32>,
    pub duration_seconds: Option<u32>,
    pub cover_art_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artist {
    pub id: String,
    pub name: String,
    pub album_count: Option<u32>,
    pub song_count: Option<u32>,
    pub cover_art_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub song_count: Option<u32>,
    pub duration_seconds: Option<u32>,
    pub cover_art_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtistWithAlbums {
    pub artist: Artist,
    pub albums: Vec<Album>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlbumWithTracks {
    pub album: Album,
    pub tracks: Vec<Song>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaylistWithTracks {
    pub playlist: Playlist,
    pub tracks: Vec<Song>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SearchResult {
    pub artists: Vec<Artist>,
    pub albums: Vec<Album>,
    pub songs: Vec<Song>,
    pub playlists: Vec<Playlist>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeEvent {
    pub item: ItemRef,
    pub change_type: ChangeType,
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChangeType {
    Created,
    Updated,
    Deleted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemRef {
    pub id: String,
    pub item_type: ItemType,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ItemType {
    Library,
    Artist,
    Album,
    Song,
    Playlist,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JellyfinTicks(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seconds(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bps(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Kbps(pub u32);

impl From<JellyfinTicks> for Seconds {
    fn from(value: JellyfinTicks) -> Self {
        let seconds = value.0 / JELLYFIN_TICKS_PER_SECOND;
        Seconds(seconds.min(u32::MAX as u64) as u32)
    }
}

impl From<Seconds> for u32 {
    fn from(value: Seconds) -> Self {
        value.0
    }
}

impl From<Bps> for Kbps {
    fn from(value: Bps) -> Self {
        Kbps(value.0 / BITS_PER_KILOBIT)
    }
}

impl From<Kbps> for u32 {
    fn from(value: Kbps) -> Self {
        value.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jellyfin_ticks_convert_to_seconds() {
        let seconds = Seconds::from(JellyfinTicks(215 * JELLYFIN_TICKS_PER_SECOND));

        assert_eq!(u32::from(seconds), 215);
    }

    #[test]
    fn seconds_passthrough_preserves_subsonic_duration() {
        let seconds = Seconds(319);

        assert_eq!(u32::from(seconds), 319);
    }

    #[test]
    fn bps_convert_to_kbps() {
        let kbps = Kbps::from(Bps(1_411_200));

        assert_eq!(u32::from(kbps), 1_411);
    }

    #[test]
    fn kbps_passthrough_preserves_subsonic_bitrate() {
        let kbps = Kbps(256);

        assert_eq!(u32::from(kbps), 256);
    }

    #[test]
    fn string_ids_preserve_navidrome_style_values() {
        let song = Song {
            id: "9f86d081884c7d659a2feaa0c55ad015".to_string(),
            title: "A Neutral Track".to_string(),
            artist_id: Some("artist-md5-id".to_string()),
            artist_name: Some("Artist".to_string()),
            album_id: Some("album-md5-id".to_string()),
            album_title: Some("Album".to_string()),
            duration_seconds: 188,
            bitrate_kbps: Some(320),
            track_number: Some(1),
            disc_number: None,
            cover_art_id: Some("cover-art-md5-id".to_string()),
        };

        assert_eq!(song.id, "9f86d081884c7d659a2feaa0c55ad015");
        assert_eq!(song.artist_id.as_deref(), Some("artist-md5-id"));
        assert_eq!(song.album_id.as_deref(), Some("album-md5-id"));
    }

    #[test]
    fn cover_art_id_is_preserved_separately_from_item_id() {
        let album = Album {
            id: "album-123".to_string(),
            title: "Album".to_string(),
            artist_id: None,
            artist_name: None,
            year: None,
            song_count: None,
            duration_seconds: None,
            cover_art_id: Some("cover-456".to_string()),
        };

        assert_eq!(album.id, "album-123");
        assert_eq!(album.cover_art_id.as_deref(), Some("cover-456"));
    }
}
