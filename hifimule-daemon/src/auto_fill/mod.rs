/// Auto-fill module: fetches pre-sorted Audio tracks from Jellyfin and truncates to capacity.
///
/// Requests tracks sorted server-side by:
///
///   1. IsFavoriteOrLiked DESC (favorites first)
///   2. PlayCount DESC (most-played next)
///   3. DateCreated DESC (newest last)
///
/// Stops paginating as soon as the device capacity budget is filled.
use crate::api::{CredentialManager, JellyfinClient, JellyfinItem, JellyfinItemsResponse};
use crate::providers::{MediaProvider, ProviderError};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;

/// Configurable auto-fill pipeline: pure-function selection model + engine (Epic 12).
/// The existing `run_auto_fill*` functions above remain the live path; `pipeline`
/// generalizes them into a composable algebra validated without UI or network.
pub mod pipeline;
// Re-export so the model + engine are reachable as `crate::auto_fill::*`. Unreferenced by the
// binary until the fetch/RPC/UI wiring lands in Stories 12.3/12.4/12.6/12.7.
#[allow(unused_imports)]
pub use pipeline::*;

/// Async pool-materialization layer (Story 12.4): turns a configured `AutoFillPipeline` into the
/// materialized pools the pure `pipeline::run_pipeline` engine consumes, and decides (via
/// `needs_configurable_expansion`) whether a slot can keep the fast default path.
pub mod fetch;
pub use fetch::*;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AutoFillItem {
    /// The goal-derived bitrate used to reserve this item; execution must reuse it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_bitrate_override_kbps: Option<u32>,
    #[serde(default)]
    pub source_audio: crate::domain::models::SourceAudioProperties,
    #[serde(default)]
    pub disc_number: Option<u32>,
    #[serde(default)]
    pub original_bitrate: Option<u32>,
    #[serde(default)]
    pub duration_seconds: u32,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub artist: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album_artist: Option<String>,
    #[serde(default)]
    pub provider_album_id: Option<String>,
    #[serde(default)]
    pub provider_content_type: Option<String>,
    #[serde(default)]
    pub provider_suffix: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track_number: Option<u32>,
    pub size_bytes: u64,
    pub priority_reason: String,
    /// Story 13.1: source rotation-tier index (as a string) when the pipeline used Memory tiers;
    /// `None` otherwise. Recorded into `autofill_history.tier` at sync completion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
}

#[derive(Debug)]
pub struct AutoFillParams {
    pub preferred_audio_container: Option<String>,
    pub output_profile: Option<serde_json::Value>,
    /// IDs of manually selected items — these are excluded from auto-fill results.
    pub exclude_item_ids: Vec<String>,
    /// Maximum bytes available for auto-fill items (device free bytes minus manual selection size).
    pub max_fill_bytes: u64,
    /// Story 13.1: the device manifest id, for history/rotation keying (`""` on legacy paths).
    pub device_id: String,
    /// Story 13.1: the slot's portable server id, for history/rotation keying (`""` on legacy paths).
    pub server_id: String,
    /// Story 13.1: caller-supplied current Unix seconds. The pure engine never reads the clock —
    /// every time-based Memory decision derives from this value via the [`pipeline::HistorySnapshot`].
    pub now_unix: i64,
    /// Story 13.1: DB-sourced runtime history (per-track `last_synced_at`/`tier`) pre-built by the
    /// RPC layer. Keeps the fetch layer DB-free; per-candidate `last_played_at` is merged in here
    /// from the materialized provider songs. Empty on legacy/default paths (memory stays inert).
    pub history: pipeline::HistorySnapshot,
    /// Story 13.1: the rotation cursor for Memory tiers (read from the DB by the caller). The lead
    /// tier is `cursor mod tiers.len()`; ignored when no tiers are configured.
    pub rotation_cursor: i64,
    /// Story 13.4: the entropy seed minted by the RPC layer (`now as u64`) — deterministic within a
    /// run, varies per run. Threaded into [`pipeline::PipelineInput::seed`]; `0` on legacy paths.
    pub seed: u64,
    /// Story 13.4: the pity dry-streak read from the DB by the caller (sibling of `rotation_cursor`).
    /// Threaded into [`pipeline::PipelineInput::pity_streak`]; `0` on legacy paths.
    pub pity_streak: i64,
    /// Story 13.5: caller-supplied local civil time (minted by `rpc::now_civil()` at the engine fill
    /// sites). Threaded into [`pipeline::HistorySnapshot::local`] to drive the Context stage. Defaults
    /// to `CivilTime::default()` (all-zero, inert) on legacy/Jellyfin paths — mirrors the `seed` split.
    pub local: pipeline::CivilTime,
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

    let headers = crate::api::jellyfin_token_headers(&token)?;

    // Build a HashSet for O(1) client-side exclusion. Sending ExcludeItemIds in the URL
    // explodes to tens of kilobytes on large baskets, causing Cloudflare/nginx 520/414 errors.
    let AutoFillParams {
        exclude_item_ids,
        max_fill_bytes,
        output_profile,
        preferred_audio_container,
        ..
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
    let mut seen_ids = HashSet::new();
    if max_fill_bytes == 0 { return Ok(result); }

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
                    if status.is_server_error()
                        && let Some(&delay_ms) = PAGE_RETRY_DELAYS_MS.get(attempt)
                    {
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
        let prior_seen = seen_ids.len();
        let page_items: Vec<JellyfinItem> = page
            .items
            .into_iter()
            .filter(|item| seen_ids.insert(item.id.clone()) && !exclude_set.contains(&item.id))
            .collect();
        let remaining_budget = max_fill_bytes.saturating_sub(cumulative_bytes);
        let (new_items, capacity_reached) = rank_and_truncate_with_profile(page_items, remaining_budget, output_profile.as_ref(), preferred_audio_container.as_deref());
        cumulative_bytes += new_items.iter().map(|i| i.size_bytes).sum::<u64>();
        result.extend(new_items);

        // Only use total as an exit condition when the server actually reported a
        // non-zero value. If total defaulted to 0 (server omitted the field), rely
        // solely on fetched < PAGE_SIZE (partial page = end of results).
        let total_known = total > 0;
        if capacity_reached
            || (prior_seen == seen_ids.len() && fetched > 0)
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
/// Assumes tracks arrive in priority order (server-sorted). Skips tracks that do not
/// fit so smaller later candidates can use the remaining capacity.
/// Returns `(items, capacity_reached)`.
pub fn rank_and_truncate(
    tracks: Vec<JellyfinItem>,
    max_fill_bytes: u64,
) -> (Vec<AutoFillItem>, bool) {
    rank_and_truncate_with_profile(tracks, max_fill_bytes, None, None)
}

fn rank_and_truncate_with_profile(
    tracks: Vec<JellyfinItem>,
    max_fill_bytes: u64,
    profile: Option<&serde_json::Value>,
    preferred: Option<&str>,
) -> (Vec<AutoFillItem>, bool) {
    let mut state = ProviderFillState::new(HashSet::new(), max_fill_bytes);
    state.output_profile = profile.cloned();
    state.preferred_audio_container = preferred.map(str::to_owned);
    if max_fill_bytes == 0 {
        return (state.into_result(), true);
    }
    for track in tracks {
        // Keep the legacy basket artist and MIME hint while sharing song budgeting.
        let artist = track.album_artist.clone().or_else(|| {
            track.artists.as_ref().and_then(|artists| artists.first()).cloned()
        });
        let mut song = crate::providers::jellyfin::song_from_item(track);
        song.artist_name = artist;
        song.content_type = song.suffix.as_ref().map(|suffix| format!("audio/{suffix}"));
        let priority_reason = if song.is_favorite.unwrap_or(false) {
            "favorite".to_string()
        } else if song.play_count.unwrap_or(0) > 0 {
            format!("playCount:{}", song.play_count.unwrap())
        } else {
            "new".to_string()
        };
        if !state.try_add(song, priority_reason) {
            break;
        }
    }
    let full = state.cumulative_bytes >= max_fill_bytes;
    (state.into_result(), full)
}

/// Accumulates auto-fill items for the provider path, tracking budget and dedup state.
struct ProviderFillState {
    preferred_audio_container: Option<String>,
    output_profile: Option<serde_json::Value>,
    exclude_set: HashSet<String>,
    seen_ids: HashSet<String>,
    result: Vec<AutoFillItem>,
    cumulative_bytes: u64,
    max_fill_bytes: u64,
}

impl ProviderFillState {
    fn new(exclude_set: HashSet<String>, max_fill_bytes: u64) -> Self {
        Self {
            output_profile: None,
            preferred_audio_container: None,
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
        let size_bytes = crate::sync::planned_song_bytes_with_preferred(&song, self.output_profile.as_ref(), self.preferred_audio_container.as_deref(), None);
        if size_bytes == 0 {
            return true; // skip unknown-size song
        }
        if self.cumulative_bytes >= self.max_fill_bytes { return false; }
        if size_bytes > self.max_fill_bytes.saturating_sub(self.cumulative_bytes) {
            return true; // skip this candidate; smaller later tracks may still fit
        }
        self.cumulative_bytes += size_bytes;
        self.result.push(AutoFillItem { max_bitrate_override_kbps: None, source_audio: crate::sync::source_audio_for_song(&song), disc_number: song.disc_number,
            album_artist: song.album_artist.clone(),
            original_bitrate: crate::sync::song_audio_bitrate_bps(&song),
            duration_seconds: song.duration_seconds,
        id: song.id,
            name: song.title,
            album: song.album_title,
            artist: song.artist_name,
            provider_album_id: song.album_id,
            provider_content_type: song.content_type,
            provider_suffix: song.suffix,
            track_number: song.track_number,
            size_bytes,
            priority_reason,
            tier: None,
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
    const PAGE_SIZE: u32 = 500;
    // Guard against runaway pagination if the server never returns a partial page.
    const MAX_BULK_PAGES: u32 = 200;

    let AutoFillParams {
        exclude_item_ids,
        max_fill_bytes,
        ..
    } = params;
    let exclude_set: HashSet<String> = exclude_item_ids.into_iter().collect();
    let mut state = ProviderFillState::new(exclude_set, max_fill_bytes);
    state.output_profile = params.output_profile;
    state.preferred_audio_container = params.preferred_audio_container;
    if max_fill_bytes == 0 { return Ok(Vec::new()); }

    // Stream priority pages until capacity is truly exhausted. Oversized tracks
    // are skipped, while accepted tracks retain provider priority order.
    for kind in [fetch::PriorityList::Favorites, fetch::PriorityList::Frequent, fetch::PriorityList::Recent] {
        let mut offset = 0u32;
        let mut seen = HashSet::new();
        for _ in 0..MAX_BULK_PAGES {
            let (songs, total) = match fetch::priority_page(provider.as_ref(), kind, offset).await {
                Ok(page) => page,
                Err(ProviderError::UnsupportedCapability(_)) => break,
                Err(error) if matches!(kind, fetch::PriorityList::Favorites) => return Err(error.into()),
                Err(error) => {
                    crate::daemon_log!("[AutoFill] priority fetch failed (non-fatal): {}", error);
                    break;
                }
            };
            let count = songs.len() as u32;
            let before = seen.len();
            for song in songs {
                if !seen.insert(song.id.clone()) { continue; }
                let reason = match kind {
                    fetch::PriorityList::Favorites => "favorite".to_string(),
                    fetch::PriorityList::Frequent => format!("playCount:{}", song.play_count.unwrap_or(0)),
                    fetch::PriorityList::Recent => "recentlyPlayed".to_string(),
                };
                state.try_add(song, reason);
                if state.cumulative_bytes >= max_fill_bytes { return Ok(state.into_result()); }
            }
            offset = offset.saturating_add(count);
            if count == 0 || seen.len() == before || (total == 0 && count < fetch::MAX_PER_LIST) || (total > 0 && offset >= total) { break; }
        }
    }

    // Phase 2: Bulk fill — paginate through the full library until budget is met.
    // Mirrors Jellyfin's paginated /Items?IncludeItemTypes=Audio approach.
    // Songs already consumed in phase 1 are skipped via seen_ids.
    let mut offset = 0u32;
    let mut pages_fetched = 0u32;
    let mut bulk_seen = HashSet::new();
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
                let prior_seen = bulk_seen.len();
                for song in songs {
                    if !bulk_seen.insert(song.id.clone()) { continue; }
                    if !state.try_add(song, "library".to_string()) {
                        break 'bulk; // budget full
                    }
                }
                pages_fetched += 1;
                if page_count < PAGE_SIZE || pages_fetched >= MAX_BULK_PAGES || prior_seen == bulk_seen.len() {
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
    #[test]
    fn provider_fill_preserves_album_artist_and_performer() {
        let track = make_track("guest", true, 0, "2024-01-01", 1000);
        let mut song = crate::providers::jellyfin::song_from_item(track);
        song.artist_name = Some("Main feat. Guest".into());
        song.album_artist = Some("Main".into());
        let mut state = ProviderFillState::new(HashSet::new(), 10_000);
        assert!(state.try_add(song, "favorite".into()));
        let result = state.into_result();
        assert_eq!(result[0].artist.as_deref(), Some("Main feat. Guest"));
        assert_eq!(result[0].album_artist.as_deref(), Some("Main"));
        let mut legacy = serde_json::to_value(&result[0]).unwrap();
        legacy.as_object_mut().unwrap().remove("albumArtist");
        let legacy: AutoFillItem = serde_json::from_value(legacy).unwrap();
        assert_eq!(legacy.album_artist, None);
    }

    use super::*;
    use crate::api::{JellyfinItem, JellyfinUserData, MediaSource};

    #[tokio::test]
    async fn legacy_auto_fill_uses_supported_jellyfin_authorization() {
        let _guard = crate::api::credential_test_lock();
        let temp = tempfile::tempdir().unwrap();
        CredentialManager::set_config_path(temp.path().join("config.json"));
        let mut server = mockito::Server::new_async().await;
        CredentialManager::save_credentials(&server.url(), "test-token-1234567890", Some("user1"))
            .unwrap();
        let request = server
            .mock("GET", "/Items")
            .match_query(mockito::Matcher::Any)
            .match_header(
                "Authorization",
                "MediaBrowser Token=\"test-token-1234567890\"",
            )
            .match_header("X-Emby-Token", mockito::Matcher::Missing)
            .with_status(200)
            .with_body(r#"{"Items":[],"TotalRecordCount":0,"StartIndex":0}"#)
            .create_async()
            .await;
        let result = run_auto_fill(
            &JellyfinClient::new(),
            AutoFillParams {
                preferred_audio_container: None,
                output_profile: None,
                exclude_item_ids: vec![],
                max_fill_bytes: 1000,
                device_id: String::new(),
                server_id: String::new(),
                now_unix: 0,
                history: Default::default(),
                rotation_cursor: 0,
                seed: 0,
                pity_streak: 0,
                local: Default::default(),
            },
        )
        .await
        .expect("auto-fill");
        assert!(result.is_empty());
        request.assert_async().await;
    }

    fn make_track(
        id: &str,
        is_favorite: bool,
        play_count: u32,
        date_created: &str,
        size_bytes: i64,
    ) -> JellyfinItem {
        JellyfinItem {
            id: id.to_string(),
            provider_ids: None,
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
        track.album_artist = Some("Album artist".to_string());
        track.artists = Some(vec!["Track artist".to_string()]);
        track.index_number = Some(7);
        track.parent_index_number = Some(2);
        track.run_time_ticks = Some(420_000_000);
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
        assert_eq!(result[0].artist.as_deref(), Some("Album artist"));
        assert_eq!(result[0].album_artist.as_deref(), Some("Album artist"));
        assert_eq!(result[0].track_number, Some(7));
        assert_eq!(result[0].disc_number, Some(2));
        assert_eq!(result[0].duration_seconds, 42);
        assert_eq!(result[0].priority_reason, "favorite");
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
    fn test_skips_oversized_preserving_remaining_priority() {
        // A high-priority oversized track must not block later fitting tracks.
        let tracks = vec![
            make_track("a", false, 0, "2024-01-01", 1_000_000), // 1MB — fits
            make_track("b", false, 0, "2024-01-01", 4_000_000), // 4MB — exceeds remaining 2MB → skip
            make_track("c", true, 0, "2024-01-01", 500_000),   // 0.5MB — fits the remaining space
        ];
        let (result, capacity_reached) = rank_and_truncate(tracks, 3_000_000);
        assert_eq!(
            result.len(),
            2,
            "a and the later fitting c should be selected"
        );
        assert_eq!(result[0].id, "a");
        assert_eq!(result[1].id, "c");
        assert!(!capacity_reached);
    }
}

#[cfg(test)]
mod output_capacity_tests {
    #[test]
    fn legacy_jellyfin_capacity_reserves_encoded_output_and_preserves_direct_sizes() {
        let track: crate::api::JellyfinItem = serde_json::from_value(serde_json::json!({
            "Id":"one", "Name":"Track", "Type":"Audio", "RunTimeTicks":2400000000_i64,
            "Container":"flac", "MediaSources":[{"Size":50000000,"Container":"flac","Bitrate":1600000}]
        })).unwrap();
        let profile = serde_json::json!({"DirectPlayProfiles":[{"Type":"Audio","Container":"mp3","AudioCodec":"mp3"}],"TranscodingProfiles":[{"Type":"Audio","Container":"mp3","AudioCodec":"mp3"}],"MusicStreamingTranscodingBitrate":192000});
        let (encoded, full) = super::rank_and_truncate_with_profile(vec![track.clone()], 7000000, Some(&profile), None);
        assert!(!full);
        assert_eq!(encoded.len(), 1);
        assert!(encoded[0].size_bytes < 7000000);
        assert_eq!(encoded[0].original_bitrate, Some(1600000));
        assert_eq!(encoded[0].size_bytes, 5_883_136);
        let (payload_only, _) = super::rank_and_truncate_with_profile(vec![track.clone()], 5_760_001, Some(&profile), None);
        assert!(payload_only.is_empty(), "capacity must include one percent plus fixed overhead");
        let (full_reservation, _) = super::rank_and_truncate_with_profile(vec![track.clone()], 5_883_136, Some(&profile), None);
        assert_eq!(full_reservation.len(), 1);
        let (direct, full) = super::rank_and_truncate(vec![track], 7000000);
        assert!(!full, "an oversized candidate does not exhaust a nonempty budget");
        assert!(direct.is_empty());
    }
}
