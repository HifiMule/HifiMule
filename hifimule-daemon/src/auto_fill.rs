/// Auto-fill module: fetches pre-sorted Audio tracks from Jellyfin and truncates to capacity.
///
/// Requests tracks sorted server-side by:
///   1. IsFavoriteOrLiked DESC (favorites first)
///   2. PlayCount DESC (most-played next)
///   3. DateCreated DESC (newest last)
/// Stops paginating as soon as the device capacity budget is filled.
use crate::api::{CredentialManager, JellyfinClient, JellyfinItem, JellyfinItemsResponse};
use crate::providers::{MediaProvider, ProviderError};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AutoFillItem {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub artist: Option<String>,
    #[serde(default)]
    pub provider_album_id: Option<String>,
    #[serde(default)]
    pub provider_content_type: Option<String>,
    #[serde(default)]
    pub provider_suffix: Option<String>,
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

/// Retry delays (ms) for transient 5xx server errors. Two entries = up to 3 total attempts.
const PAGE_RETRY_DELAYS_MS: &[u64] = &[1_000, 2_000];

/// Runs the auto-fill algorithm: fetches Audio tracks from Jellyfin pre-sorted by priority
/// and stops paginating as soon as `max_fill_bytes` is filled.
///
/// Already-selected items in `params.exclude_item_ids` are filtered client-side rather than
/// passed as URL parameters, avoiding server/proxy URL-length limits on large baskets.
///
/// Returns a capacity-truncated list of tracks ready to populate the basket.
pub async fn run_auto_fill(
    client: &JellyfinClient,
    params: AutoFillParams,
) -> Result<Vec<AutoFillItem>> {
    let (url, token, user_id) =
        CredentialManager::get_credentials().map_err(|e| anyhow::anyhow!("{}", e))?;
    let user_id = user_id.ok_or_else(|| {
        anyhow::anyhow!(
            "No user ID in stored credentials; auto-fill requires an authenticated Jellyfin user"
        )
    })?;
    CredentialManager::validate_url(&url)?;
    CredentialManager::validate_token(&token)?;

    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        "X-Emby-Token",
        reqwest::header::HeaderValue::from_str(&token)
            .map_err(|_| anyhow::anyhow!("Invalid token format"))?,
    );

    // Build a HashSet for O(1) client-side exclusion. Sending ExcludeItemIds in the URL
    // explodes to tens of kilobytes on large baskets, causing Cloudflare/nginx 520/414 errors.
    let AutoFillParams {
        exclude_item_ids,
        max_fill_bytes,
    } = params;
    let exclude_count = exclude_item_ids.len();
    let exclude_set: std::collections::HashSet<String> = exclude_item_ids.into_iter().collect();

    const PAGE_SIZE: u32 = 500;
    // Guard against runaway pagination in case the server misbehaves.
    const MAX_PAGES: u32 = 200;
    let mut result: Vec<AutoFillItem> = Vec::new();
    let mut cumulative_bytes: u64 = 0;
    let mut start_index: u32 = 0;
    // Capture total_record_count from the first page only; re-reading it each page
    // can cause premature termination or missed pages if the library changes mid-fetch.
    let mut total_record_count: Option<u32> = None;

    'pages: loop {
        let page_num = start_index / PAGE_SIZE + 1;
        let endpoint = format!(
            "{}/Items?userId={}&IncludeItemTypes=Audio&Recursive=true\
             &Fields=MediaSources,UserData,DateCreated\
             &SortBy=IsFavoriteOrLiked,PlayCount,DateCreated\
             &SortOrder=Descending,Descending,Descending\
             &StartIndex={}&Limit={}",
            url.trim_end_matches('/'),
            user_id,
            start_index,
            PAGE_SIZE,
        );
        crate::daemon_log!(
            "[AutoFill] Page {}: fetching {} tracks (URL: {} bytes, {} IDs excluded client-side)",
            page_num,
            PAGE_SIZE,
            endpoint.len(),
            exclude_count,
        );

        let page_text = {
            let mut attempt = 0usize;
            loop {
                let response = match client
                    .http_client()
                    .get(&endpoint)
                    .headers(headers.clone())
                    .send()
                    .await
                {
                    Ok(r) => r,
                    Err(e) if e.is_connect() || e.is_timeout() => {
                        if let Some(&delay_ms) = PAGE_RETRY_DELAYS_MS.get(attempt) {
                            crate::daemon_log!(
                                "[AutoFill] Page {}: transport error ({}), retrying in {}ms (attempt {}/{})",
                                page_num,
                                e,
                                delay_ms,
                                attempt + 2,
                                PAGE_RETRY_DELAYS_MS.len() + 1
                            );
                            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                            attempt += 1;
                            continue;
                        }
                        return Err(e.into());
                    }
                    Err(e) => return Err(e.into()),
                };
                let status = response.status();
                if !status.is_success() {
                    let body = response.text().await?;
                    if status.is_server_error() {
                        if let Some(&delay_ms) = PAGE_RETRY_DELAYS_MS.get(attempt) {
                            crate::daemon_log!(
                                "[AutoFill] Page {}: server error {} (URL: {} bytes), retrying in {}ms (attempt {}/{})",
                                page_num,
                                status,
                                endpoint.len(),
                                delay_ms,
                                attempt + 2,
                                PAGE_RETRY_DELAYS_MS.len() + 1
                            );
                            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                            attempt += 1;
                            continue;
                        }
                    }
                    return Err(anyhow::anyhow!(
                        "Page {}: server returned status: {} (URL: {} bytes, {} IDs excluded client-side) - {}",
                        page_num,
                        status,
                        endpoint.len(),
                        exclude_count,
                        body
                    ));
                }
                break response.text().await?;
            }
        };
        let page: JellyfinItemsResponse = serde_json::from_str(&page_text)?;

        let fetched = page.items.len() as u32;
        let total = *total_record_count.get_or_insert(page.total_record_count);

        // Filter already-selected items client-side to keep the request URL short.
        let page_items: Vec<JellyfinItem> = page
            .items
            .into_iter()
            .filter(|item| !exclude_set.contains(&item.id))
            .collect();
        let remaining_budget = max_fill_bytes.saturating_sub(cumulative_bytes);
        let (new_items, capacity_reached) = rank_and_truncate(page_items, remaining_budget);
        cumulative_bytes += new_items.iter().map(|i| i.size_bytes).sum::<u64>();
        result.extend(new_items);

        // Only use total as an exit condition when the server actually reported a
        // non-zero value. If total defaulted to 0 (server omitted the field), rely
        // solely on fetched < PAGE_SIZE (partial page = end of results).
        let total_known = total > 0;
        if capacity_reached
            || fetched < PAGE_SIZE
            || (total_known && start_index + fetched >= total)
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
/// Returns `(items, capacity_reached)`.
pub fn rank_and_truncate(
    tracks: Vec<JellyfinItem>,
    max_fill_bytes: u64,
) -> (Vec<AutoFillItem>, bool) {
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
            return (result, true);
        }

        let is_favorite = track
            .user_data
            .as_ref()
            .map(|u| u.is_favorite)
            .unwrap_or(false);
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
            artist: track
                .album_artist
                .or_else(|| track.artists.and_then(|a| a.into_iter().next())),
            provider_album_id: track.album_id,
            provider_content_type: track
                .media_sources
                .as_ref()
                .and_then(|sources| sources.first())
                .and_then(|source| source.container.clone())
                .or_else(|| track.container.clone())
                .map(|suffix| format!("audio/{suffix}")),
            provider_suffix: track
                .media_sources
                .as_ref()
                .and_then(|sources| sources.first())
                .and_then(|source| source.container.clone())
                .or(track.container),
            size_bytes,
            priority_reason,
        });
    }

    (result, false)
}

/// Accumulates auto-fill items for the provider path, tracking budget and dedup state.
struct ProviderFillState {
    exclude_set: HashSet<String>,
    seen_ids: HashSet<String>,
    result: Vec<AutoFillItem>,
    cumulative_bytes: u64,
    max_fill_bytes: u64,
}

impl ProviderFillState {
    fn new(exclude_set: HashSet<String>, max_fill_bytes: u64) -> Self {
        Self {
            exclude_set,
            seen_ids: HashSet::new(),
            result: Vec::new(),
            cumulative_bytes: 0,
            max_fill_bytes,
        }
    }

    /// Attempts to add a song. Returns `false` if the budget is full (caller should stop iterating).
    fn try_add(&mut self, song: crate::domain::models::Song, priority_reason: String) -> bool {
        if self.exclude_set.contains(&song.id) || !self.seen_ids.insert(song.id.clone()) {
            return true; // skip duplicate/excluded, budget still open
        }
        let size_bytes = song
            .bitrate_kbps
            .map(|kbps| (u64::from(kbps) * 1_000 / 8) * u64::from(song.duration_seconds))
            .unwrap_or(0);
        if size_bytes == 0 {
            return true; // skip unknown-size song
        }
        if self.cumulative_bytes + size_bytes > self.max_fill_bytes {
            return false; // budget full
        }
        self.cumulative_bytes += size_bytes;
        self.result.push(AutoFillItem {
            id: song.id,
            name: song.title,
            album: song.album_title,
            artist: song.artist_name,
            provider_album_id: song.album_id,
            provider_content_type: song.content_type,
            provider_suffix: song.suffix,
            size_bytes,
            priority_reason,
        });
        true
    }

    fn into_result(self) -> Vec<AutoFillItem> {
        self.result
    }
}

/// Runs the auto-fill algorithm for any MediaProvider (Subsonic, Navidrome, Jellyfin).
///
/// Priority order:
///   1. Favorites (list_favorites) — supported by all servers
///   2. Frequently played (list_frequently_played) — OpenSubsonic / Jellyfin only
///   3. Recently played (list_recently_played) — OpenSubsonic / Jellyfin only
///   4. Full library pagination (list_all_songs_page) — fills remaining space
///
/// Song size is estimated as `(bitrate_kbps * 1_000 / 8) * duration_seconds`.
/// Songs with unknown bitrate (size estimate = 0) are skipped.
/// Stops paginating as soon as `params.max_fill_bytes` is reached.
pub async fn run_auto_fill_provider(
    provider: Arc<dyn MediaProvider>,
    params: AutoFillParams,
) -> Result<Vec<AutoFillItem>> {
    const MAX_PER_LIST: u32 = 2000;
    const PAGE_SIZE: u32 = 500;
    // Guard against runaway pagination if the server never returns a partial page.
    const MAX_BULK_PAGES: u32 = 200;

    let AutoFillParams {
        exclude_item_ids,
        max_fill_bytes,
    } = params;
    let exclude_set: HashSet<String> = exclude_item_ids.into_iter().collect();
    let mut state = ProviderFillState::new(exclude_set, max_fill_bytes);

    // Phase 1: Priority lists (favorites → frequently played → recently played).
    // These are small targeted lists; fetch them all at once then consume in order.
    let mut priority_songs: Vec<(crate::domain::models::Song, String)> = Vec::new();

    match provider.list_favorites(None, 0, MAX_PER_LIST).await {
        Ok((songs, _)) => {
            crate::daemon_log!("[AutoFill] list_favorites returned {} songs", songs.len());
            for s in songs {
                priority_songs.push((s, "favorite".to_string()));
            }
        }
        Err(ProviderError::UnsupportedCapability(_)) => {
            crate::daemon_log!("[AutoFill] list_favorites: UnsupportedCapability, skipping");
        }
        Err(e) => {
            crate::daemon_log!("[AutoFill] list_favorites failed: {}", e);
            return Err(e.into());
        }
    }

    match provider.list_frequently_played(None, 0, MAX_PER_LIST).await {
        Ok((songs, _)) => {
            crate::daemon_log!(
                "[AutoFill] list_frequently_played returned {} songs",
                songs.len()
            );
            for s in songs {
                let reason = format!("playCount:{}", s.play_count.unwrap_or(0));
                priority_songs.push((s, reason));
            }
        }
        Err(ProviderError::UnsupportedCapability(_)) => {
            crate::daemon_log!(
                "[AutoFill] list_frequently_played: UnsupportedCapability, skipping"
            );
        }
        Err(e) => crate::daemon_log!(
            "[AutoFill] list_frequently_played failed (non-fatal): {}",
            e
        ),
    }

    match provider.list_recently_played(None, 0, MAX_PER_LIST).await {
        Ok((songs, _)) => {
            crate::daemon_log!(
                "[AutoFill] list_recently_played returned {} songs",
                songs.len()
            );
            for s in songs {
                priority_songs.push((s, "recentlyPlayed".to_string()));
            }
        }
        Err(ProviderError::UnsupportedCapability(_)) => {
            crate::daemon_log!("[AutoFill] list_recently_played: UnsupportedCapability, skipping");
        }
        Err(e) => crate::daemon_log!("[AutoFill] list_recently_played failed (non-fatal): {}", e),
    }

    crate::daemon_log!(
        "[AutoFill] priority candidates: {}, max_fill_bytes: {}",
        priority_songs.len(),
        max_fill_bytes
    );

    for (song, reason) in priority_songs {
        if !state.try_add(song, reason) {
            break; // budget full
        }
    }

    // Phase 2: Bulk fill — paginate through the full library until budget is met.
    // Mirrors Jellyfin's paginated /Items?IncludeItemTypes=Audio approach.
    // Songs already consumed in phase 1 are skipped via seen_ids.
    let mut offset = 0u32;
    let mut pages_fetched = 0u32;
    'bulk: loop {
        match provider.list_all_songs_page(None, offset, PAGE_SIZE).await {
            Ok((songs, _)) => {
                let page_count = songs.len() as u32;
                crate::daemon_log!(
                    "[AutoFill] bulk page {} ({} songs, offset {})",
                    pages_fetched + 1,
                    page_count,
                    offset
                );
                for song in songs {
                    if !state.try_add(song, "library".to_string()) {
                        break 'bulk; // budget full
                    }
                }
                pages_fetched += 1;
                if page_count < PAGE_SIZE || pages_fetched >= MAX_BULK_PAGES {
                    break 'bulk;
                }
                offset += PAGE_SIZE;
            }
            Err(ProviderError::UnsupportedCapability(_)) => {
                crate::daemon_log!(
                    "[AutoFill] list_all_songs_page: UnsupportedCapability, bulk fill skipped"
                );
                break 'bulk;
            }
            Err(e) => {
                crate::daemon_log!("[AutoFill] list_all_songs_page failed (non-fatal): {}", e);
                break 'bulk;
            }
        }
    }

    let result = state.into_result();
    crate::daemon_log!("[AutoFill] done: {} tracks", result.len());
    Ok(result)
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
            media_sources: Some(vec![MediaSource {
                size: Some(size_bytes),
                container: None,
                bitrate: None,
                media_streams: None,
            }]),
            image_tags: None,
            etag: None,
            user_data: Some(JellyfinUserData {
                is_favorite,
                play_count,
                last_played_date: None,
            }),
            date_created: Some(date_created.to_string()),
            playlist_item_id: None,
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
        let (result, _) = rank_and_truncate(tracks, 5_000_000);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "a");
    }

    #[test]
    fn test_rank_and_truncate_preserves_provider_album_metadata() {
        let mut track = make_track("a", true, 0, "2024-01-01", 3_000_000);
        track.album_id = Some("album1".to_string());
        track.container = Some("fallback-container".to_string());
        track.media_sources = Some(vec![MediaSource {
            size: Some(3_000_000),
            container: Some("mp3".to_string()),
            bitrate: None,
            media_streams: None,
        }]);

        let (result, _) = rank_and_truncate(vec![track], 5_000_000);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].provider_album_id.as_deref(), Some("album1"));
        assert_eq!(
            result[0].provider_content_type.as_deref(),
            Some("audio/mp3")
        );
        assert_eq!(result[0].provider_suffix.as_deref(), Some("mp3"));
    }

    #[test]
    fn test_empty_library() {
        let (result, _) = rank_and_truncate(vec![], 10_000);
        assert!(result.is_empty());
    }

    #[test]
    fn test_negative_size_tracks_skipped() {
        // Jellyfin can return size = -1 for tracks whose size is unknown.
        // Such tracks must be skipped, not added with a u64::MAX budget cost.
        let mut track = make_track("a", true, 0, "2024-01-01", 1000);
        track.media_sources = Some(vec![MediaSource {
            size: Some(-1),
            container: None,
            bitrate: None,
            media_streams: None,
        }]);
        let (result, _) = rank_and_truncate(vec![track], 10_000);
        assert!(
            result.is_empty(),
            "tracks with negative size should be skipped"
        );
    }

    #[test]
    fn test_zero_size_tracks_skipped() {
        // Tracks with no media_sources (size = 0) must not inflate the fill set.
        let mut track = make_track("a", true, 0, "2024-01-01", 0);
        track.media_sources = None;
        let (result, _) = rank_and_truncate(vec![track], 10_000);
        assert!(
            result.is_empty(),
            "tracks with zero/unknown size should be skipped"
        );
    }

    #[test]
    fn test_zero_capacity_returns_empty() {
        let tracks = vec![make_track("a", true, 0, "2024-01-01", 1000)];
        let (result, capacity_reached) = rank_and_truncate(tracks, 0);
        assert!(result.is_empty());
        assert!(capacity_reached);
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
        let (result, capacity_reached) = rank_and_truncate(tracks, 3_000_000);
        assert_eq!(
            result.len(),
            1,
            "only 'a' fits; break stops at 'b', 'c' never considered"
        );
        assert_eq!(result[0].id, "a");
        assert!(capacity_reached);
    }
}
