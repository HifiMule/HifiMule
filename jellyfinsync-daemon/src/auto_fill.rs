/// Auto-fill module: priority ranking algorithm for automatic basket population.
///
/// Fetches all Audio tracks from the Jellyfin library, ranks them by:
///   1. IsFavorite DESC (favorites first)
///   2. PlayCount DESC (most-played next)
///   3. DateCreated DESC (newest last)
/// Then truncates the ranked list to fit within the available device capacity.
use crate::api::{CredentialManager, JellyfinClient, JellyfinItem};
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AutoFillItem {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub artist: Option<String>,
    pub size_bytes: u64,
    pub priority_reason: String,
}

#[derive(Debug)]
pub struct AutoFillParams {
    /// IDs of manually selected items — these are excluded from auto-fill results.
    pub exclude_item_ids: Vec<String>,
    /// Maximum bytes available for auto-fill items (device free bytes minus manual selection size).
    pub max_fill_bytes: u64,
}

/// Runs the auto-fill priority algorithm.
///
/// Returns a ranked, capacity-truncated list of tracks ready to populate the basket.
pub async fn run_auto_fill(
    client: &JellyfinClient,
    params: AutoFillParams,
) -> Result<Vec<AutoFillItem>> {
    let (url, token, user_id) =
        CredentialManager::get_credentials().map_err(|e| anyhow::anyhow!("{}", e))?;
    let user_id = user_id.ok_or_else(|| anyhow::anyhow!(
        "No user ID in stored credentials; auto-fill requires an authenticated Jellyfin user"
    ))?;

    let tracks = client
        .get_audio_tracks_for_autofill(&url, &token, &user_id)
        .await?;

    Ok(rank_and_truncate(tracks, params))
}

/// Truncate a date string to `YYYY-MM-DDTHH:MM:SS` for consistent lexicographic comparison.
fn date_sort_key(date: &str) -> String {
    date.chars().take(19).collect()
}

/// Pure ranking and truncation logic — extracted for unit-testability.
pub fn rank_and_truncate(mut tracks: Vec<JellyfinItem>, params: AutoFillParams) -> Vec<AutoFillItem> {
    let exclude_set: std::collections::HashSet<&str> =
        params.exclude_item_ids.iter().map(|s| s.as_str()).collect();

    // Filter out excluded items
    tracks.retain(|t| !exclude_set.contains(t.id.as_str()));

    // Sort: favorites first, then play_count desc, then date_created desc
    tracks.sort_by(|a, b| {
        let a_fav = a.user_data.as_ref().map(|u| u.is_favorite).unwrap_or(false);
        let b_fav = b.user_data.as_ref().map(|u| u.is_favorite).unwrap_or(false);

        if a_fav != b_fav {
            return b_fav.cmp(&a_fav); // true > false
        }

        let a_plays = a.user_data.as_ref().map(|u| u.play_count).unwrap_or(0);
        let b_plays = b.user_data.as_ref().map(|u| u.play_count).unwrap_or(0);
        if a_plays != b_plays {
            return b_plays.cmp(&a_plays); // descending
        }

        // DateCreated descending. Truncate to 19 chars (YYYY-MM-DDTHH:MM:SS) before
        // comparing so that varying sub-second precision and non-UTC timezone offsets
        // (e.g. "+05:30") don't break lexicographic ordering.
        let a_date = date_sort_key(a.date_created.as_deref().unwrap_or(""));
        let b_date = date_sort_key(b.date_created.as_deref().unwrap_or(""));
        b_date.cmp(&a_date)
    });

    // Accumulate up to max_fill_bytes
    let mut result = Vec::new();
    let mut cumulative_bytes: u64 = 0;

    for track in tracks {
        // Treat missing, zero, or negative sizes as unsyncable — skip them rather than
        // adding zero-byte items that would inflate the fill set without consuming budget.
        let size_bytes = track
            .media_sources
            .as_ref()
            .and_then(|ms| ms.first())
            .and_then(|ms| ms.size)
            .and_then(|s| if s > 0 { Some(s as u64) } else { None })
            .unwrap_or(0);

        if size_bytes == 0 {
            continue;
        }

        // Use continue (not break) so that smaller tracks later in the list can still
        // fill the remaining space after a large track is skipped.
        if cumulative_bytes + size_bytes > params.max_fill_bytes {
            continue;
        }

        let is_favorite = track
            .user_data
            .as_ref()
            .map(|u| u.is_favorite)
            .unwrap_or(false);
        let play_count = track
            .user_data
            .as_ref()
            .map(|u| u.play_count)
            .unwrap_or(0);

        let priority_reason = if is_favorite {
            "favorite".to_string()
        } else if play_count > 0 {
            format!("playCount:{}", play_count)
        } else {
            "new".to_string()
        };

        cumulative_bytes += size_bytes;
        result.push(AutoFillItem {
            id: track.id,
            name: track.name,
            album: track.album,
            artist: track.album_artist.or_else(|| {
                track
                    .artists
                    .and_then(|a| a.into_iter().next())
            }),
            size_bytes,
            priority_reason,
        });
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{JellyfinItem, JellyfinUserData, MediaSource};

    fn make_track(
        id: &str,
        is_favorite: bool,
        play_count: u32,
        date_created: &str,
        size_bytes: i64,
    ) -> JellyfinItem {
        JellyfinItem {
            id: id.to_string(),
            name: format!("Track {}", id),
            item_type: "Audio".to_string(),
            album: None,
            album_artist: None,
            artists: None,
            index_number: None,
            container: None,
            production_year: None,
            recursive_item_count: None,
            cumulative_run_time_ticks: None,
            media_sources: Some(vec![MediaSource {
                size: Some(size_bytes),
                container: None,
            }]),
            etag: None,
            user_data: Some(JellyfinUserData {
                is_favorite,
                play_count,
            }),
            date_created: Some(date_created.to_string()),
        }
    }

    #[test]
    fn test_favorites_ranked_first() {
        let tracks = vec![
            make_track("a", false, 100, "2024-01-01", 1000),
            make_track("b", true, 0, "2023-01-01", 1000),
            make_track("c", false, 50, "2024-01-01", 1000),
        ];
        let result = rank_and_truncate(
            tracks,
            AutoFillParams {
                exclude_item_ids: vec![],
                max_fill_bytes: 10_000,
            },
        );
        assert_eq!(result[0].id, "b", "favorite should be first");
        assert_eq!(result[0].priority_reason, "favorite");
    }

    #[test]
    fn test_play_count_secondary_sort() {
        let tracks = vec![
            make_track("a", false, 10, "2024-01-01", 1000),
            make_track("b", false, 50, "2024-01-01", 1000),
            make_track("c", false, 25, "2024-01-01", 1000),
        ];
        let result = rank_and_truncate(
            tracks,
            AutoFillParams {
                exclude_item_ids: vec![],
                max_fill_bytes: 10_000,
            },
        );
        assert_eq!(result[0].id, "b");
        assert_eq!(result[1].id, "c");
        assert_eq!(result[2].id, "a");
        assert!(result[0].priority_reason.starts_with("playCount:"));
    }

    #[test]
    fn test_date_created_tertiary_sort() {
        let tracks = vec![
            make_track("a", false, 0, "2023-01-01", 1000),
            make_track("b", false, 0, "2025-01-01", 1000),
            make_track("c", false, 0, "2024-01-01", 1000),
        ];
        let result = rank_and_truncate(
            tracks,
            AutoFillParams {
                exclude_item_ids: vec![],
                max_fill_bytes: 10_000,
            },
        );
        assert_eq!(result[0].id, "b", "newest should be first");
        assert_eq!(result[0].priority_reason, "new");
    }

    #[test]
    fn test_capacity_truncation() {
        let tracks = vec![
            make_track("a", true, 0, "2024-01-01", 3_000_000),
            make_track("b", false, 10, "2024-01-01", 3_000_000),
            make_track("c", false, 5, "2024-01-01", 3_000_000),
        ];
        // Only 5MB available — all tracks are 3MB so only the first fits; others are skipped
        let result = rank_and_truncate(
            tracks,
            AutoFillParams {
                exclude_item_ids: vec![],
                max_fill_bytes: 5_000_000,
            },
        );
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "a");
    }

    #[test]
    fn test_exclude_item_ids() {
        let tracks = vec![
            make_track("a", true, 100, "2024-01-01", 1000),
            make_track("b", false, 50, "2024-01-01", 1000),
            make_track("c", false, 10, "2024-01-01", 1000),
        ];
        let result = rank_and_truncate(
            tracks,
            AutoFillParams {
                exclude_item_ids: vec!["a".to_string()],
                max_fill_bytes: 10_000,
            },
        );
        assert_eq!(result.len(), 2);
        assert!(result.iter().all(|r| r.id != "a"));
    }

    #[test]
    fn test_empty_library() {
        let result = rank_and_truncate(
            vec![],
            AutoFillParams {
                exclude_item_ids: vec![],
                max_fill_bytes: 10_000,
            },
        );
        assert!(result.is_empty());
    }

    #[test]
    fn test_capacity_skip_large_includes_smaller() {
        // A large track sits above a small one in priority order.
        // After the large track is skipped, the small one should still be included.
        let tracks = vec![
            make_track("big", false, 10, "2024-01-01", 4_000_000),
            make_track("small", false, 5, "2024-01-01", 1_000_000),
        ];
        let result = rank_and_truncate(
            tracks,
            AutoFillParams {
                exclude_item_ids: vec![],
                max_fill_bytes: 3_000_000,
            },
        );
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "small");
    }

    #[test]
    fn test_negative_size_tracks_skipped() {
        // Jellyfin can return size = -1 for tracks whose size is unknown.
        // Such tracks must be skipped, not added with a u64::MAX budget cost.
        let mut track = make_track("a", true, 0, "2024-01-01", 1000);
        track.media_sources = Some(vec![MediaSource { size: Some(-1), container: None }]);
        let result = rank_and_truncate(
            vec![track],
            AutoFillParams {
                exclude_item_ids: vec![],
                max_fill_bytes: 10_000,
            },
        );
        assert!(result.is_empty(), "tracks with negative size should be skipped");
    }

    #[test]
    fn test_zero_size_tracks_skipped() {
        // Tracks with no media_sources (size = 0) must not inflate the fill set.
        let mut track = make_track("a", true, 0, "2024-01-01", 0);
        track.media_sources = None;
        let result = rank_and_truncate(
            vec![track],
            AutoFillParams {
                exclude_item_ids: vec![],
                max_fill_bytes: 10_000,
            },
        );
        assert!(result.is_empty(), "tracks with zero/unknown size should be skipped");
    }

    #[test]
    fn test_zero_capacity_returns_empty() {
        let tracks = vec![make_track("a", true, 0, "2024-01-01", 1000)];
        let result = rank_and_truncate(
            tracks,
            AutoFillParams {
                exclude_item_ids: vec![],
                max_fill_bytes: 0,
            },
        );
        assert!(result.is_empty());
    }
}
