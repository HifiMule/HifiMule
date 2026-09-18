use crate::domain::models::Song;

pub const MAX_ALBUM_OCCURRENCES: usize = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlbumValidationError {
    Empty,
    TooLarge,
    Invalid,
}

/// Validates and deterministically orders a complete provider album without
/// deduplicating deliberate repeated occurrences.
pub fn order_album_tracks(mut tracks: Vec<Song>) -> Result<Vec<Song>, AlbumValidationError> {
    if tracks.is_empty() {
        return Err(AlbumValidationError::Empty);
    }
    if tracks.len() > MAX_ALBUM_OCCURRENCES {
        return Err(AlbumValidationError::TooLarge);
    }
    if tracks
        .iter()
        .any(|track| track.id.is_empty() || track.id.len() > super::model::MAX_ID_BYTES)
    {
        return Err(AlbumValidationError::Invalid);
    }
    let mut enumerated: Vec<_> = tracks.drain(..).enumerate().collect();
    enumerated.sort_by_key(|(ordinal, track)| {
        let disc = track.disc_number.filter(|value| *value > 0).unwrap_or(1);
        let number = track.track_number.filter(|value| *value > 0);
        (disc, number.is_none(), number.unwrap_or(0), *ordinal)
    });
    Ok(enumerated.into_iter().map(|(_, track)| track).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(id: &str, disc: Option<u32>, track: Option<u32>) -> Song {
        Song {
            id: id.into(),
            title: id.into(),
            artist_id: None,
            artist_name: None,
            album_id: None,
            album_title: None,
            duration_seconds: 1,
            bitrate_kbps: None,
            track_number: track,
            disc_number: disc,
            cover_art_id: None,
            date_added: None,
            last_played_at: None,
            play_count: None,
            is_favorite: None,
            content_type: None,
            suffix: None,
            size_bytes: None,
            album_loudness: Default::default(),
        }
    }

    #[test]
    fn orders_discs_numbered_then_missing_with_provider_ordinal_ties() {
        let ordered = order_album_tracks(vec![
            song("d2-2", Some(2), Some(2)),
            song("missing-b", None, None),
            song("d1-2b", Some(1), Some(2)),
            song("d1-1", Some(1), Some(1)),
            song("d1-2a", Some(1), Some(2)),
            song("missing-a", Some(0), None),
            song("d2-1", Some(2), Some(1)),
        ])
        .unwrap();
        assert_eq!(
            ordered.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            [
                "d1-1",
                "d1-2b",
                "d1-2a",
                "missing-b",
                "missing-a",
                "d2-1",
                "d2-2"
            ]
        );
    }

    #[test]
    fn keeps_repeated_ids_and_enforces_bounds_atomically() {
        let ordered = order_album_tracks(vec![
            song("same", None, Some(1)),
            song("same", None, Some(1)),
        ])
        .unwrap();
        assert_eq!(ordered.len(), 2);
        assert_eq!(
            order_album_tracks(Vec::new()),
            Err(AlbumValidationError::Empty)
        );
        assert_eq!(
            order_album_tracks(vec![song("x", None, None); MAX_ALBUM_OCCURRENCES + 1]),
            Err(AlbumValidationError::TooLarge)
        );
        assert_eq!(
            order_album_tracks(vec![song("", None, None)]),
            Err(AlbumValidationError::Invalid)
        );
    }

    #[test]
    fn accepts_large_complete_albums_without_applying_the_insert_batch_limit() {
        let medium = (0..201)
            .map(|ordinal| song(&format!("track-{ordinal}"), Some(1), None))
            .collect();
        assert_eq!(order_album_tracks(medium).unwrap().len(), 201);

        let boundary = (0..MAX_ALBUM_OCCURRENCES)
            .map(|ordinal| song(&format!("track-{ordinal}"), Some(1), None))
            .collect();
        let ordered = order_album_tracks(boundary).unwrap();
        assert_eq!(ordered.len(), MAX_ALBUM_OCCURRENCES);
        assert_eq!(ordered.first().unwrap().id, "track-0");
        assert_eq!(ordered.last().unwrap().id, "track-9999");
    }
}
