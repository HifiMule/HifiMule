/// Auto-fill module: fetches pre-sorted Audio tracks from Jellyfin and truncates to capacity.
///
/// Requests tracks sorted server-side by:
///   1. IsFavoriteOrLiked DESC (favorites first)
///   2. PlayCount DESC (most-played next)
///   3. DateCreated DESC (newest last)
/// Stops paginating as soon as the device capacity budget is filled.
use crate::api::{url_encode, CredentialManager, JellyfinClient, JellyfinItem, JellyfinItemsResponse};
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

/// Runs the auto-fill algorithm: fetches Audio tracks from Jellyfin pre-sorted by priority,
/// passing exclusions server-side, and stops paginating as soon as `max_fill_bytes` is filled.
///
/// Returns a capacity-truncated list of tracks ready to populate the basket.
pub async fn run_auto_fill(
    client: &JellyfinClient,
    params: AutoFillParams,
) -> Result<Vec<AutoFillItem>> {
    let (url, token, user_id) =
        CredentialManager::get_credentials().map_err(|e| anyhow::anyhow!("{}", e))?;
    let user_id = user_id.ok_or_else(|| anyhow::anyhow!(
        "No user ID in stored credentials; auto-fill requires an authenticated Jellyfin user"
    ))?;
    CredentialManager::validate_url(&url)?;
    CredentialManager::validate_token(&token)?;

    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        "X-Emby-Token",
        reqwest::header::HeaderValue::from_str(&token)
            .map_err(|_| anyhow::anyhow!("Invalid token format"))?,
    );

    let exclude_param = if params.exclude_item_ids.is_empty() {
        String::new()
    } else {
        let encoded_ids: Vec<String> = params.exclude_item_ids.iter().map(|id| url_encode(id)).collect();
        format!("&ExcludeItemIds={}", encoded_ids.join(","))
    };

    const PAGE_SIZE: u32 = 500;
    // Guard against runaway pagination in case the server misbehaves.
    const MAX_PAGES: u32 = 200;
    let mut result: Vec<AutoFillItem> = Vec::new();
    let mut cumulative_bytes: u64 = 0;
    let mut start_index: u32 = 0;
    // Capture total_record_count from the first page only; re-reading it each page
    // can cause premature termination or missed pages if the library changes mid-fetch.
    let mut total_record_count: Option<u32> = None;
    let mut capacity_reached = false;

    'pages: loop {
        let endpoint = format!(
            "{}/Users/{}/Items?IncludeItemTypes=Audio&Recursive=true\
             &Fields=MediaSources,UserData,DateCreated\
             &SortBy=IsFavoriteOrLiked,PlayCount,DateCreated\
             &SortOrder=Descending,Descending,Descending\
             {}&StartIndex={}&Limit={}",
            url.trim_end_matches('/'),
            user_id,
            exclude_param,
            start_index,
            PAGE_SIZE,
        );

        let response = client
            .http_client()
            .get(&endpoint)
            .headers(headers.clone())
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            let text = response.text().await?;
            return Err(anyhow::anyhow!("Server returned status: {} - {}", status, text));
        }
        let text = response.text().await?;
        let page: JellyfinItemsResponse = serde_json::from_str(&text)?;

        let fetched = page.items.len() as u32;
        let total = *total_record_count.get_or_insert(page.total_record_count);

        for track in page.items {
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

            if cumulative_bytes + size_bytes > params.max_fill_bytes {
                capacity_reached = true;
                break;
            }

            let is_favorite = track.user_data.as_ref().map(|u| u.is_favorite).unwrap_or(false);
            let play_count = track.user_data.as_ref().map(|u| u.play_count).unwrap_or(0);
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
                    track.artists.and_then(|a| a.into_iter().next())
                }),
                size_bytes,
                priority_reason,
            });
        }

        let page_num = start_index / PAGE_SIZE + 1;
        if capacity_reached
            || fetched < PAGE_SIZE
            || start_index + fetched >= total
            || page_num >= MAX_PAGES
        {
            break 'pages;
        }
        start_index += PAGE_SIZE;
    }

    Ok(result)
}

/// Capacity-truncation of a pre-sorted track list — extracted for unit-testability.
/// Assumes tracks arrive in priority order (server-sorted). Stops at the first track
/// that would exceed the remaining capacity budget.
pub fn rank_and_truncate(tracks: Vec<JellyfinItem>, max_fill_bytes: u64) -> Vec<AutoFillItem> {
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

        if cumulative_bytes + size_bytes > max_fill_bytes {
            break;
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
    fn test_capacity_truncation() {
        let tracks = vec![
            make_track("a", true, 0, "2024-01-01", 3_000_000),
            make_track("b", false, 10, "2024-01-01", 3_000_000),
            make_track("c", false, 5, "2024-01-01", 3_000_000),
        ];
        // Only 5MB available — all tracks are 3MB so only the first fits; others are skipped
        let result = rank_and_truncate(tracks, 5_000_000);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "a");
    }

    #[test]
    fn test_empty_library() {
        let result = rank_and_truncate(vec![], 10_000);
        assert!(result.is_empty());
    }

    #[test]
    fn test_negative_size_tracks_skipped() {
        // Jellyfin can return size = -1 for tracks whose size is unknown.
        // Such tracks must be skipped, not added with a u64::MAX budget cost.
        let mut track = make_track("a", true, 0, "2024-01-01", 1000);
        track.media_sources = Some(vec![MediaSource { size: Some(-1), container: None }]);
        let result = rank_and_truncate(vec![track], 10_000);
        assert!(result.is_empty(), "tracks with negative size should be skipped");
    }

    #[test]
    fn test_zero_size_tracks_skipped() {
        // Tracks with no media_sources (size = 0) must not inflate the fill set.
        let mut track = make_track("a", true, 0, "2024-01-01", 0);
        track.media_sources = None;
        let result = rank_and_truncate(vec![track], 10_000);
        assert!(result.is_empty(), "tracks with zero/unknown size should be skipped");
    }

    #[test]
    fn test_zero_capacity_returns_empty() {
        let tracks = vec![make_track("a", true, 0, "2024-01-01", 1000)];
        let result = rank_and_truncate(tracks, 0);
        assert!(result.is_empty());
    }

    #[test]
    fn test_stops_after_first_oversized() {
        // With break semantics: after the first track that exceeds remaining budget,
        // smaller tracks later in the list are NOT included.
        let tracks = vec![
            make_track("a", false, 0, "2024-01-01", 1_000_000), // 1MB — fits
            make_track("b", false, 0, "2024-01-01", 4_000_000), // 4MB — exceeds remaining 2MB → break
            make_track("c", false, 0, "2024-01-01", 500_000),   // 0.5MB — never reached
        ];
        let result = rank_and_truncate(tracks, 3_000_000);
        assert_eq!(result.len(), 1, "only 'a' fits; break stops at 'b', 'c' never considered");
        assert_eq!(result[0].id, "a");
    }
}
