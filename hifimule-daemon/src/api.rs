use anyhow::{anyhow, Result};
use reqwest::header::{HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

const CONTAINER_TYPES: &[&str] = &["MusicAlbum", "Playlist", "MusicArtist"];

pub(crate) fn url_encode(s: &str) -> String {
    let mut encoded = String::new();
    for c in s.chars() {
        match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => encoded.push(c),
            ' ' => encoded.push('+'),
            c => {
                let mut buf = [0u8; 4];
                let bytes = c.encode_utf8(&mut buf);
                for b in bytes.bytes() {
                    encoded.push_str(&format!("%{:02X}", b));
                }
            }
        }
    }
    encoded
}
#[cfg(test)]
const MUSIC_ITEM_TYPES: &str = "MusicAlbum,Playlist,MusicArtist,Audio,MusicVideo";

#[derive(Clone)]
pub struct JellyfinClient {
    client: reqwest::Client,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SystemInfo {
    pub server_name: String,
    pub version: String,
    pub id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "PascalCase")]
pub struct JellyfinView {
    pub id: String,
    pub name: String,
    #[serde(rename = "Type")]
    pub view_type: String,
    #[serde(default)]
    pub collection_type: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "PascalCase")]
pub struct MediaSource {
    #[serde(default)]
    pub size: Option<i64>,
    pub container: Option<String>,
    #[serde(default)]
    pub bitrate: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "PascalCase")]
pub struct NameIdPair {
    pub name: String,
    pub id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "PascalCase")]
pub struct JellyfinItem {
    pub id: String,
    pub name: String,
    #[serde(rename = "Type")]
    pub item_type: String,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub album_artist: Option<String>,
    #[serde(default)]
    pub artists: Option<Vec<String>>,
    #[serde(default)]
    pub index_number: Option<u32>,
    #[serde(default)]
    pub parent_index_number: Option<u32>,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub album_id: Option<String>,
    #[serde(default)]
    pub artist_items: Option<Vec<NameIdPair>>,
    #[serde(default)]
    pub container: Option<String>,
    #[serde(default)]
    pub production_year: Option<u32>,
    #[serde(default)]
    pub recursive_item_count: Option<u32>,
    #[serde(default)]
    pub cumulative_run_time_ticks: Option<u64>,
    #[serde(default)]
    pub run_time_ticks: Option<u64>,
    #[serde(default)]
    pub bitrate: Option<u32>,
    #[serde(default)]
    pub media_sources: Option<Vec<MediaSource>>,
    #[serde(default)]
    pub image_tags: Option<std::collections::HashMap<String, String>>,
    #[serde(default)]
    pub etag: Option<String>,
    // Auto-fill priority fields
    #[serde(default)]
    pub user_data: Option<JellyfinUserData>,
    #[serde(default)]
    pub date_created: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "PascalCase")]
pub struct JellyfinUserData {
    #[serde(default)]
    pub is_favorite: bool,
    #[serde(default)]
    pub play_count: u32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct JellyfinItemsResponse {
    pub items: Vec<JellyfinItem>,
    #[serde(default)]
    pub total_record_count: u32,
    pub start_index: u32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct JellyfinViewsResponse {
    pub items: Vec<JellyfinView>,
    pub total_record_count: u32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct AuthenticateByNameRequest {
    pub username: String,
    pub pw: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct UserDto {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct AuthenticationResult {
    pub access_token: String,
    pub user: UserDto,
}

impl JellyfinClient {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    pub(crate) fn http_client(&self) -> &reqwest::Client {
        &self.client
    }

    pub async fn test_connection(&self, url: &str, token: &str) -> Result<SystemInfo> {
        // Validate inputs
        CredentialManager::validate_url(url)?;
        CredentialManager::validate_token(token)?;

        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Emby-Token",
            HeaderValue::from_str(token).map_err(|_| anyhow!("Invalid token format"))?,
        );

        let endpoint = format!("{}/System/Info", url.trim_end_matches('/'));

        let response = self.client.get(&endpoint).headers(headers).send().await?;
        let status = response.status();
        let text = response.text().await?;
        println!("DEBUG: Jellyfin Response [{}] - Body: {}", status, text);

        if !status.is_success() {
            return Err(anyhow!("Server returned status: {}", status));
        }

        let info = serde_json::from_str::<SystemInfo>(&text)?;
        Ok(info)
    }

    pub async fn get_views(
        &self,
        url: &str,
        token: &str,
        user_id: &str,
    ) -> Result<Vec<JellyfinView>> {
        CredentialManager::validate_url(url)?;
        CredentialManager::validate_token(token)?;

        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Emby-Token",
            HeaderValue::from_str(token).map_err(|_| anyhow!("Invalid token format"))?,
        );

        let endpoint = format!("{}/UserViews?userId={}", url.trim_end_matches('/'), user_id);

        let response = self.client.get(&endpoint).headers(headers).send().await?;
        let status = response.status();
        let text = response.text().await?;
        println!("DEBUG: Jellyfin Response [{}] - Body: {}", status, text);

        if !status.is_success() {
            return Err(anyhow!("Server returned status: {}", status));
        }

        let views_response = serde_json::from_str::<JellyfinViewsResponse>(&text)?;
        Ok(views_response.items)
    }

    pub async fn get_items(
        &self,
        url: &str,
        token: &str,
        user_id: &str,
        parent_id: Option<&str>,
        include_item_types: Option<&str>,
        start_index: Option<u32>,
        limit: Option<u32>,
        name_starts_with: Option<&str>,
        name_less_than: Option<&str>,
    ) -> Result<JellyfinItemsResponse> {
        CredentialManager::validate_url(url)?;
        CredentialManager::validate_token(token)?;

        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Emby-Token",
            HeaderValue::from_str(token).map_err(|_| anyhow!("Invalid token format"))?,
        );

        let mut query_params = vec![];
        if let Some(parent) = parent_id {
            query_params.push(format!("ParentId={}", parent));
        }
        if let Some(types) = include_item_types {
            query_params.push(format!("IncludeItemTypes={}", types));
        }
        if let Some(start) = start_index {
            query_params.push(format!("StartIndex={}", start));
        }
        if let Some(lim) = limit {
            query_params.push(format!("Limit={}", lim));
        }
        if let Some(starts_with) = name_starts_with {
            query_params.push(format!("NameStartsWith={}", starts_with));
        }
        if let Some(less_than) = name_less_than {
            query_params.push(format!("NameLessThan={}", less_than));
        }

        let query_string = if query_params.is_empty() {
            format!("?userId={}", user_id)
        } else {
            format!("?userId={}&{}", user_id, query_params.join("&"))
        };

        let endpoint = format!("{}/Items{}", url.trim_end_matches('/'), query_string);

        let response = self.client.get(&endpoint).headers(headers).send().await?;
        let status = response.status();
        let text = response.text().await?;
        println!("DEBUG: Jellyfin Response [{}] - Body: {}", status, text);

        if !status.is_success() {
            return Err(anyhow!("Server returned status: {}", status));
        }

        let items_response = serde_json::from_str::<JellyfinItemsResponse>(&text)?;
        Ok(items_response)
    }

    pub async fn get_item_details(
        &self,
        url: &str,
        token: &str,
        user_id: &str,
        item_id: &str,
    ) -> Result<JellyfinItem> {
        CredentialManager::validate_url(url)?;
        CredentialManager::validate_token(token)?;

        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Emby-Token",
            HeaderValue::from_str(token).map_err(|_| anyhow!("Invalid token format"))?,
        );

        let endpoint = format!(
            "{}/Items/{}?userId={}",
            url.trim_end_matches('/'),
            item_id,
            user_id
        );

        let response = self.client.get(&endpoint).headers(headers).send().await?;
        let status = response.status();
        let text = response.text().await?;
        println!("DEBUG: Jellyfin Response [{}] - Body: {}", status, text);

        if !status.is_success() {
            return Err(anyhow!("Server returned status: {}", status));
        }

        let item = serde_json::from_str::<JellyfinItem>(&text)?;
        Ok(item)
    }

    pub async fn get_items_by_ids(
        &self,
        url: &str,
        token: &str,
        user_id: &str,
        item_ids: &[&str],
    ) -> Result<Vec<JellyfinItem>> {
        if item_ids.is_empty() {
            return Ok(Vec::new());
        }

        CredentialManager::validate_url(url)?;
        CredentialManager::validate_token(token)?;

        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Emby-Token",
            HeaderValue::from_str(token).map_err(|_| anyhow!("Invalid token format"))?,
        );

        let ids_str = item_ids.join(",");
        let endpoint = format!(
            "{}/Items?userId={}&Ids={}&Fields=MediaSources",
            url.trim_end_matches('/'),
            user_id,
            ids_str
        );

        let response = self.client.get(&endpoint).headers(headers).send().await?;
        let status = response.status();
        let text = response.text().await?;

        if !status.is_success() {
            return Err(anyhow!("Server returned status: {}", status));
        }

        let items_response = serde_json::from_str::<JellyfinItemsResponse>(&text)?;
        Ok(items_response.items)
    }

    pub async fn get_item_with_media_sources(
        &self,
        url: &str,
        token: &str,
        user_id: &str,
        item_id: &str,
    ) -> Result<JellyfinItem> {
        CredentialManager::validate_url(url)?;
        CredentialManager::validate_token(token)?;

        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Emby-Token",
            HeaderValue::from_str(token).map_err(|_| anyhow!("Invalid token format"))?,
        );

        let endpoint = format!(
            "{}/Items/{}?userId={}&Fields=MediaSources",
            url.trim_end_matches('/'),
            item_id,
            user_id
        );

        let response = self.client.get(&endpoint).headers(headers).send().await?;
        let status = response.status();
        let text = response.text().await?;
        println!("DEBUG: Jellyfin Response [{}] - Body: {}", status, text);

        if !status.is_success() {
            return Err(anyhow!("Server returned status: {}", status));
        }

        let item = serde_json::from_str::<JellyfinItem>(&text)?;
        Ok(item)
    }

    pub async fn get_child_items_with_sizes(
        &self,
        url: &str,
        token: &str,
        user_id: &str,
        parent_id: &str,
    ) -> Result<Vec<JellyfinItem>> {
        CredentialManager::validate_url(url)?;
        CredentialManager::validate_token(token)?;

        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Emby-Token",
            HeaderValue::from_str(token).map_err(|_| anyhow!("Invalid token format"))?,
        );

        let endpoint = format!(
            "{}/Items?userId={}&ParentId={}&IncludeItemTypes=Audio,MusicVideo&Fields=MediaSources&Recursive=true",
            url.trim_end_matches('/'),
            user_id,
            parent_id
        );

        let response = self.client.get(&endpoint).headers(headers).send().await?;
        let status = response.status();
        let text = response.text().await?;
        println!("DEBUG: Jellyfin Response [{}] - Body: {}", status, text);

        if !status.is_success() {
            return Err(anyhow!("Server returned status: {}", status));
        }

        let items_response = serde_json::from_str::<JellyfinItemsResponse>(&text)?;
        Ok(items_response.items)
    }

    pub async fn get_items_changed_since(
        &self,
        url: &str,
        token: &str,
        user_id: &str,
        min_date_last_saved: Option<&str>,
    ) -> Result<JellyfinItemsResponse> {
        CredentialManager::validate_url(url)?;
        CredentialManager::validate_token(token)?;

        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Emby-Token",
            HeaderValue::from_str(token).map_err(|_| anyhow!("Invalid token format"))?,
        );

        let mut query_params = vec![
            format!("userId={}", user_id),
            "Fields=MediaSources".to_string(),
        ];
        if let Some(date_str) = min_date_last_saved {
            query_params.push(format!("minDateLastSaved={}", url_encode(date_str)));
        }

        let endpoint = format!(
            "{}/Items?{}",
            url.trim_end_matches('/'),
            query_params.join("&")
        );

        let response = self.client.get(&endpoint).headers(headers).send().await?;
        let status = response.status();
        let text = response.text().await?;

        if !status.is_success() {
            return Err(anyhow!("Server returned status: {}", status));
        }

        let items_response = serde_json::from_str::<JellyfinItemsResponse>(&text)?;
        Ok(items_response)
    }

    #[allow(dead_code)]
    pub async fn get_albums_by_artist(
        &self,
        url: &str,
        token: &str,
        user_id: &str,
        artist_id: &str,
    ) -> Result<JellyfinItemsResponse> {
        CredentialManager::validate_url(url)?;
        CredentialManager::validate_token(token)?;

        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Emby-Token",
            HeaderValue::from_str(token).map_err(|_| anyhow!("Invalid token format"))?,
        );

        let endpoint = format!(
            "{}/Items?userId={}&AlbumArtistIds={}&IncludeItemTypes=MusicAlbum&Recursive=true",
            url.trim_end_matches('/'),
            user_id,
            artist_id
        );

        let response = self.client.get(&endpoint).headers(headers).send().await?;
        let status = response.status();
        let text = response.text().await?;

        if !status.is_success() {
            return Err(anyhow!("Server returned status: {}", status));
        }

        let items_response = serde_json::from_str::<JellyfinItemsResponse>(&text)?;
        Ok(items_response)
    }

    /// Get total size in bytes for each item. For containers (Albums, Playlists, Artists),
    /// recursively fetches child items and sums their MediaSources sizes.
    pub async fn get_item_sizes(
        &self,
        url: &str,
        token: &str,
        user_id: &str,
        ids: Vec<String>,
    ) -> Vec<(String, u64)> {
        let futures = ids.into_iter().map(|id| {
            let url = url.to_string();
            let token = token.to_string();
            let user_id = user_id.to_string();
            async move {
                let size = self.get_single_item_size(&url, &token, &user_id, &id).await;
                match size {
                    Ok(bytes) => Some((id, bytes)),
                    Err(e) => {
                        println!("Warning: Failed to fetch size for item {}: {}", id, e);
                        None
                    }
                }
            }
        });

        futures::future::join_all(futures)
            .await
            .into_iter()
            .flatten()
            .collect()
    }

    async fn get_single_item_size(
        &self,
        url: &str,
        token: &str,
        user_id: &str,
        item_id: &str,
    ) -> Result<u64> {
        let item = self
            .get_item_with_media_sources(url, token, user_id, item_id)
            .await?;

        if CONTAINER_TYPES.contains(&item.item_type.as_str()) {
            // Container item: fetch children and sum their sizes
            let children = self
                .get_child_items_with_sizes(url, token, user_id, item_id)
                .await?;
            let total: u64 = children
                .iter()
                .filter_map(|child| {
                    child
                        .media_sources
                        .as_ref()
                        .and_then(|sources| sources.first())
                        .and_then(|source| source.size)
                        .map(|s| s as u64)
                })
                .sum();
            Ok(total)
        } else {
            // Individual item: read MediaSources[0].Size
            let size = item
                .media_sources
                .as_ref()
                .and_then(|sources| sources.first())
                .and_then(|source| source.size)
                .unwrap_or(0) as u64;
            Ok(size)
        }
    }

    pub async fn get_image(
        &self,
        url: &str,
        token: &str,
        item_id: &str,
    ) -> Result<reqwest::Response> {
        CredentialManager::validate_url(url)?;
        CredentialManager::validate_token(token)?;

        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Emby-Token",
            HeaderValue::from_str(token).map_err(|_| anyhow!("Invalid token format"))?,
        );

        // Fetch primary image
        let endpoint = format!(
            "{}/Items/{}/Images/Primary",
            url.trim_end_matches('/'),
            item_id
        );

        let response = self.client.get(&endpoint).headers(headers).send().await?;

        if !response.status().is_success() {
            return Err(anyhow!("Server returned status: {}", response.status()));
        }

        Ok(response)
    }

    pub async fn authenticate_by_name(
        &self,
        url: &str,
        username: &str,
        password: &str,
    ) -> Result<AuthenticationResult> {
        CredentialManager::validate_url(url)?;

        let endpoint = format!("{}/Users/AuthenticateByName", url.trim_end_matches('/'));

        // Authorization Header
        // TODO: Use persistent DeviceId
        let device_id = CredentialManager::get_device_id()
            .unwrap_or_else(|_| "HifiMule-Desktop-Fallback".to_string());
        let auth_header = format!(
            "MediaBrowser Client=\"HifiMule\", Device=\"Desktop\", DeviceId=\"{}\", Version=\"{}\"",
            device_id,
            env!("CARGO_PKG_VERSION")
        );

        let body = AuthenticateByNameRequest {
            username: username.to_string(),
            pw: password.to_string(),
        };

        let response = self
            .client
            .post(&endpoint)
            .header("Authorization", auth_header)
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        let text = response.text().await?;
        println!("DEBUG: Jellyfin Response [{}] - Body: {}", status, text);

        if !status.is_success() {
            return Err(anyhow!("Authentication failed: {}", status));
        }

        let result = serde_json::from_str::<AuthenticationResult>(&text)?;
        Ok(result)
    }

    pub async fn search_audio_items(
        &self,
        url: &str,
        token: &str,
        user_id: &str,
        title: &str,
    ) -> Result<Vec<JellyfinItem>> {
        CredentialManager::validate_url(url)?;
        CredentialManager::validate_token(token)?;

        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Emby-Token",
            HeaderValue::from_str(token).map_err(|_| anyhow!("Invalid token format"))?,
        );

        let encoded_title = url_encode(title);
        let endpoint = format!(
            "{}/Items?userId={}&SearchTerm={}&IncludeItemTypes=Audio&Limit=10&Fields=Id,Name,Album,AlbumArtist,Artists,AlbumId",
            url.trim_end_matches('/'),
            user_id,
            encoded_title
        );

        let response = self.client.get(&endpoint).headers(headers).send().await?;
        let status = response.status();
        let text = response.text().await?;

        if !status.is_success() {
            return Err(anyhow!("Server returned status: {}", status));
        }

        let items_response = serde_json::from_str::<JellyfinItemsResponse>(&text)?;
        Ok(items_response.items)
    }

    pub async fn report_item_played(
        &self,
        url: &str,
        token: &str,
        user_id: &str,
        item_id: &str,
    ) -> Result<()> {
        CredentialManager::validate_url(url)?;
        CredentialManager::validate_token(token)?;

        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Emby-Token",
            HeaderValue::from_str(token).map_err(|_| anyhow!("Invalid token format"))?,
        );

        let endpoint = format!(
            "{}/UserPlayedItems/{}?userId={}",
            url.trim_end_matches('/'),
            item_id,
            user_id
        );

        let response = self.client.post(&endpoint).headers(headers).send().await?;

        let status = response.status();
        if !status.is_success() {
            return Err(anyhow!("Server returned status: {}", status));
        }

        Ok(())
    }

    /// Unified item stream resolver. If `transcoding_profile` is Some, calls
    /// POST /Items/{id}/PlaybackInfo to negotiate the stream URL. Falls back to
    /// /Items/{id}/Download if direct play is supported or PlaybackInfo returns no
    /// transcoding URL. If `transcoding_profile` is None, uses /Download directly.
    ///
    /// Both code paths call `response.bytes_stream()` on a `reqwest::Response`,
    /// so the return type is a single concrete impl Stream (no type erasure needed).
    /// Returns the byte stream and whether the server is actually transcoding the content.
    /// `is_transcoded = false` means the original file bytes are served (direct play/download).
    pub async fn get_item_stream(
        &self,
        base_url: &str,
        token: &str,
        user_id: &str,
        item_id: &str,
        transcoding_profile: Option<&serde_json::Value>,
    ) -> Result<(
        impl futures::Stream<Item = std::result::Result<bytes::Bytes, reqwest::Error>>,
        bool,
    )> {
        CredentialManager::validate_url(base_url)?;
        CredentialManager::validate_token(token)?;

        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Emby-Token",
            HeaderValue::from_str(token).map_err(|_| anyhow!("Invalid token format"))?,
        );

        // Resolve the URL to stream from
        let (stream_url, is_transcoded) = if let Some(profile) = transcoding_profile {
            self.resolve_stream_url(base_url, token, user_id, item_id, profile)
                .await?
        } else {
            (
                format!(
                    "{}/Items/{}/Download",
                    base_url.trim_end_matches('/'),
                    item_id
                ),
                false,
            )
        };

        let response = self.client.get(&stream_url).headers(headers).send().await?;

        if !response.status().is_success() {
            return Err(anyhow!("Stream returned status: {}", response.status()));
        }

        Ok((response.bytes_stream(), is_transcoded))
    }

    /// Calls POST /Items/{itemId}/PlaybackInfo with the given DeviceProfile.
    /// Returns `(url, is_transcoded)` where `is_transcoded` is true when the server will
    /// transcode the content (TranscodingUrl or forced audio stream endpoint), and false
    /// for direct-play downloads where the original file bytes are served unchanged.
    pub async fn resolve_stream_url(
        &self,
        base_url: &str,
        token: &str,
        user_id: &str,
        item_id: &str,
        device_profile: &serde_json::Value,
    ) -> Result<(String, bool)> {
        let endpoint = format!(
            "{}/Items/{}/PlaybackInfo?userId={}",
            base_url.trim_end_matches('/'),
            item_id,
            user_id
        );

        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Emby-Token",
            HeaderValue::from_str(token).map_err(|_| anyhow!("Invalid token format"))?,
        );

        let body = serde_json::json!({
            "DeviceProfile": device_profile,
            "UserId": user_id,
            "IsPlayback": true,
            "AutoOpenLiveStream": true
        });

        let response = self
            .client
            .post(&endpoint)
            .headers(headers)
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(anyhow!(
                "PlaybackInfo returned status: {}",
                response.status()
            ));
        }

        let json: serde_json::Value = response.json().await?;

        eprintln!("[StreamUrl] item={} PlaybackInfo response: {}", item_id, json);

        if let Some(source) = json["MediaSources"].as_array().and_then(|a| a.first()) {
            let supports_direct_play = source["SupportsDirectPlay"].as_bool().unwrap_or(false);
            let transcode_url = source["TranscodingUrl"].as_str();
            eprintln!(
                "[StreamUrl] item={} SupportsDirectPlay={} TranscodingUrl={:?}",
                item_id, supports_direct_play, transcode_url
            );

            // Always prefer TranscodingUrl when present. Jellyfin can return
            // SupportsDirectPlay=true even with an empty DirectPlayProfiles list,
            // but still include a TranscodingUrl — always honour it when it's there.
            if let Some(transcode_path) = transcode_url {
                let full_url = format!("{}{}", base_url.trim_end_matches('/'), transcode_path);
                eprintln!("[StreamUrl] item={} → case 1: using TranscodingUrl: {}", item_id, full_url);
                return Ok((full_url, true));
            }

            if supports_direct_play {
                let profile_forbids_direct_play = device_profile["DirectPlayProfiles"]
                    .as_array()
                    .map(|a| a.is_empty())
                    .unwrap_or(false);
                eprintln!(
                    "[StreamUrl] item={} profile_forbids_direct_play={}",
                    item_id, profile_forbids_direct_play
                );

                if !profile_forbids_direct_play {
                    let url = format!("{}/Items/{}/Download", base_url.trim_end_matches('/'), item_id);
                    eprintln!("[StreamUrl] item={} → case 2: direct play allowed by profile, using Download: {}", item_id, url);
                    return Ok((url, false));
                }
                eprintln!("[StreamUrl] item={} → case 3: profile forbids direct play but Jellyfin gave SupportsDirectPlay=true with no TranscodingUrl — falling through to forced stream", item_id);
            } else {
                eprintln!("[StreamUrl] item={} → SupportsDirectPlay=false with no TranscodingUrl (unexpected) — falling through to forced stream", item_id);
            }
        } else {
            eprintln!("[StreamUrl] item={} → no MediaSources in PlaybackInfo response — falling through to forced stream", item_id);
        }

        // PlaybackInfo gave no usable URL. If the profile targets a specific container,
        // use the Jellyfin audio stream endpoint to force server-side transcoding rather
        // than silently serving the original file via /Download.
        if let Some(container) = device_profile["TranscodingProfiles"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(|p| p["Container"].as_str())
        {
            let bitrate_kbps = device_profile["MusicStreamingTranscodingBitrate"]
                .as_u64()
                .unwrap_or(320000)
                / 1000;
            let codec = device_profile["TranscodingProfiles"]
                .as_array()
                .and_then(|a| a.first())
                .and_then(|p| p["AudioCodec"].as_str())
                .unwrap_or(container);
            let url = format!(
                "{}/Audio/{}/stream.{}?userId={}&audioCodec={}&audioBitRate={}&static=false",
                base_url.trim_end_matches('/'),
                item_id,
                container,
                user_id,
                codec,
                bitrate_kbps,
            );
            eprintln!("[StreamUrl] item={} → case 4: forced audio stream endpoint: {}", item_id, url);
            return Ok((url, true));
        }

        eprintln!("[StreamUrl] item={} → case 5 (last resort): no TranscodingProfiles in profile — falling back to Download", item_id);

        // Last resort: direct download.
        Ok((
            format!(
                "{}/Items/{}/Download",
                base_url.trim_end_matches('/'),
                item_id
            ),
            false,
        ))
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Config {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub device_id: Option<String>,
}

pub struct CredentialManager;

pub(crate) static CONFIG_FILE_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);
#[cfg(test)]
pub(crate) static CREDENTIAL_TEST_MUTEX: Mutex<()> = Mutex::new(());

const KEYRING_SERVICE: &str = "hifimule.github.io";
const KEYRING_SECRETS_ACCOUNT: &str = "secrets";

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct Secrets {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    token: Option<String>,
    #[serde(default)]
    server_secrets: std::collections::HashMap<String, String>,
}

impl CredentialManager {
    fn load_secrets() -> Result<Secrets> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_SECRETS_ACCOUNT)
            .map_err(|e| anyhow!("Failed to access keyring: {}", e))?;
        match entry.get_password() {
            Ok(json) => serde_json::from_str(&json)
                .map_err(|e| anyhow!("Failed to parse secrets blob: {}", e)),
            Err(_) => Ok(Secrets::default()),
        }
    }

    fn save_secrets(secrets: &Secrets) -> Result<()> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_SECRETS_ACCOUNT)
            .map_err(|e| anyhow!("Failed to access keyring: {}", e))?;
        let json = serde_json::to_string(secrets)?;
        entry
            .set_password(&json)
            .map_err(|e| anyhow!("Failed to save secrets to keyring: {}", e))
    }

    #[cfg(test)]
    pub fn set_config_path(path: PathBuf) {
        let mut p = CONFIG_FILE_PATH.lock().unwrap();
        *p = Some(path);
    }

    fn get_config_path() -> Result<PathBuf> {
        let p = CONFIG_FILE_PATH.lock().unwrap();
        if let Some(ref path) = *p {
            return Ok(path.clone());
        }

        Ok(crate::paths::get_app_data_dir()?.join("config.json"))
    }

    pub(crate) fn validate_url(url: &str) -> Result<()> {
        if url.trim().is_empty() {
            return Err(anyhow!("URL cannot be empty"));
        }
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(anyhow!("URL must start with http:// or https://"));
        }
        Ok(())
    }

    pub(crate) fn validate_token(token: &str) -> Result<()> {
        if token.trim().is_empty() {
            return Err(anyhow!("Token cannot be empty"));
        }
        if token.len() < 10 {
            return Err(anyhow!("Token appears to be invalid (too short)"));
        }
        Ok(())
    }

    pub fn save_credentials(url: &str, token: &str, user_id: Option<&str>) -> Result<()> {
        Self::validate_url(url)?;
        Self::validate_token(token)?;

        let config = Config {
            url: url.to_string(),
            user_id: user_id.map(|s| s.to_string()),
            device_id: Self::get_device_id().ok(),
        };
        let json = serde_json::to_string_pretty(&config)?;
        let path = Self::get_config_path()?;
        fs::write(&path, json).map_err(|e| anyhow!("Failed to write config file: {}", e))?;

        let mut secrets = Self::load_secrets().unwrap_or_default();
        secrets.token = Some(token.to_string());
        Self::save_secrets(&secrets)?;

        Ok(())
    }

    pub fn save_server_secret(server_type: &str, secret: &str) -> Result<()> {
        if secret.trim().is_empty() {
            return Err(anyhow!("Secret cannot be empty"));
        }
        let mut secrets = Self::load_secrets().unwrap_or_default();
        secrets
            .server_secrets
            .insert(server_type.to_string(), secret.to_string());
        Self::save_secrets(&secrets)
    }

    pub fn get_server_secret(server_type: &str) -> Result<String> {
        let secrets = Self::load_secrets()?;
        secrets
            .server_secrets
            .get(server_type)
            .cloned()
            .ok_or_else(|| anyhow!("No server secret found in keyring: {}", server_type))
    }

    pub fn clear_credentials() -> Result<()> {
        let path = Self::get_config_path()?;
        if path.exists() {
            fs::remove_file(&path).map_err(|e| anyhow!("Failed to remove config file: {}", e))?;
        }

        if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, KEYRING_SECRETS_ACCOUNT) {
            let _ = entry.delete_password();
        }

        Ok(())
    }

    pub fn get_credentials() -> Result<(String, String, Option<String>)> {
        let path = Self::get_config_path()?;
        if !path.exists() {
            return Err(anyhow!("No config file found"));
        }
        let content =
            fs::read_to_string(&path).map_err(|e| anyhow!("Failed to read config file: {}", e))?;
        let config: Config = serde_json::from_str(&content)
            .map_err(|e| anyhow!("Failed to parse config file: {}", e))?;

        let secrets = Self::load_secrets()?;
        let token = secrets
            .token
            .ok_or_else(|| anyhow!("No token found in keyring"))?;

        Ok((config.url, token, config.user_id))
    }

    pub fn get_device_id() -> Result<String> {
        let path = Self::get_config_path()?;

        // Try to read existing config
        let mut config = if path.exists() {
            let content = fs::read_to_string(&path)
                .map_err(|e| anyhow!("Failed to read config file: {}", e))?;
            serde_json::from_str::<Config>(&content).unwrap_or(Config {
                url: "".to_string(),
                user_id: None,
                device_id: None,
            })
        } else {
            Config {
                url: "".to_string(),
                user_id: None,
                device_id: None,
            }
        };

        if let Some(id) = &config.device_id {
            return Ok(id.clone());
        }

        // Generate new ID
        let new_id = format!(
            "HifiMule-Desktop-{:x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        );

        config.device_id = Some(new_id.clone());

        // Save back if path exists or we have other data,
        // but if it's a fresh file we might just perform a save if we have a valid path
        // For simplicity, we only save if we can, ignoring errors if directory doesn't exist yet
        // (CredentialManager::save_credentials handles dir creation usually via get_app_data_dir)

        // We only save if we have a valid path. The actual save_credentials checks for dir existence.
        // Here we just try to write if the file/dir structure is plausible.
        // Actually, let's just write checking for errors.

        if let Ok(json) = serde_json::to_string_pretty(&config) {
            // Ensure dir exists
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::write(&path, json);
        }

        Ok(new_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;

    #[tokio::test]
    async fn test_jellyfin_connection_success() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let token = "test-token-1234567890";

        let _mock = server
            .mock("GET", "/System/Info")
            .match_header("X-Emby-Token", token)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"ServerName": "TestServer", "Version": "10.8.10", "Id": "8d5613157d2547e9b35fd762fe1f253e"}"#)
            .create_async()
            .await;

        let client = JellyfinClient::new();
        let info = client
            .test_connection(&url, token)
            .await
            .expect("Failed to connect");

        assert_eq!(info.server_name, "TestServer");
        assert_eq!(info.version, "10.8.10");
        assert_eq!(info.id, "8d5613157d2547e9b35fd762fe1f253e");
    }

    #[tokio::test]
    async fn test_jellyfin_connection_failure() {
        let mut server = Server::new_async().await;
        let url = server.url();

        let _mock = server
            .mock("GET", "/System/Info")
            .with_status(401)
            .create_async()
            .await;

        let client = JellyfinClient::new();
        let res = client.test_connection(&url, "bad-token-1234567890").await;

        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("401"));
    }

    #[tokio::test]
    async fn test_validation_empty_url() {
        let client = JellyfinClient::new();
        let res = client.test_connection("", "valid-token-1234567890").await;

        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("URL cannot be empty"));
    }

    #[tokio::test]
    async fn test_validation_invalid_url_scheme() {
        let client = JellyfinClient::new();
        let res = client
            .test_connection("ftp://invalid.com", "valid-token-1234567890")
            .await;

        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .to_string()
            .contains("must start with http"));
    }

    #[test]
    fn test_credential_validation() {
        assert!(CredentialManager::validate_url("http://localhost:8096").is_ok());
        assert!(CredentialManager::validate_url("https://jellyfin.example.com").is_ok());
        assert!(CredentialManager::validate_token("valid-token-1234567890").is_ok());

        assert!(CredentialManager::validate_url("").is_err());
        assert!(CredentialManager::validate_url("   ").is_err());
        assert!(CredentialManager::validate_url("ftp://invalid.com").is_err());
        assert!(CredentialManager::validate_url("localhost:8096").is_err());

        assert!(CredentialManager::validate_token("").is_err());
        assert!(CredentialManager::validate_token("   ").is_err());
        assert!(CredentialManager::validate_token("short").is_err());
    }

    #[tokio::test]
    async fn test_get_views_success() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let token = "test-token-1234567890";

        let _mock = server
            .mock("GET", "/UserViews?userId=user1")
            .match_header("X-Emby-Token", token)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Items": [{"Id": "lib1", "Name": "Music", "Type": "CollectionFolder"}], "TotalRecordCount": 1}"#)
            .create_async()
            .await;

        let client = JellyfinClient::new();
        let views = client
            .get_views(&url, token, "user1")
            .await
            .expect("Failed to get views");

        assert_eq!(views.len(), 1);
        assert_eq!(views[0].id, "lib1");
        assert_eq!(views[0].name, "Music");
        assert_eq!(views[0].view_type, "CollectionFolder");
    }

    #[tokio::test]
    async fn test_get_items_success() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let token = "test-token-1234567890";

        let _mock = server
            .mock("GET", "/Items?userId=user1&ParentId=lib1&IncludeItemTypes=MusicAlbum,Playlist,MusicArtist,Audio,MusicVideo&Limit=50")
            .match_header("X-Emby-Token", token)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Items": [{"Id": "album1", "Name": "Test Album", "Type": "MusicAlbum", "AlbumArtist": "Test Artist", "ProductionYear": 2023}], "TotalRecordCount": 1, "StartIndex": 0}"#)
            .create_async()
            .await;

        let client = JellyfinClient::new();
        let response = client
            .get_items(
                &url,
                token,
                "user1",
                Some("lib1"),
                Some(MUSIC_ITEM_TYPES),
                None,
                Some(50),
                None,
                None,
            )
            .await
            .expect("Failed to get items");

        assert_eq!(response.items.len(), 1);
        assert_eq!(response.items[0].id, "album1");
        assert_eq!(response.items[0].name, "Test Album");
        assert_eq!(response.items[0].item_type, "MusicAlbum");
        assert_eq!(
            response.items[0].album_artist,
            Some("Test Artist".to_string())
        );
        assert_eq!(response.items[0].production_year, Some(2023));
        assert_eq!(response.total_record_count, 1);
    }

    #[tokio::test]
    async fn test_get_item_details_success() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let token = "test-token-1234567890";

        let _mock = server
            .mock("GET", "/Items/album1?userId=user1")
            .match_header("X-Emby-Token", token)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Id": "album1", "Name": "Test Album", "Type": "MusicAlbum", "AlbumArtist": "Test Artist"}"#)
            .create_async()
            .await;

        let client = JellyfinClient::new();
        let item = client
            .get_item_details(&url, token, "user1", "album1")
            .await
            .expect("Failed to get item details");

        assert_eq!(item.name, "Test Album");
        assert_eq!(item.item_type, "MusicAlbum");
        assert_eq!(item.album_artist, Some("Test Artist".to_string()));
    }

    #[tokio::test]
    async fn test_get_item_metadata_serialization() {
        let json = r#"{
            "Id": "item1",
            "Name": "Item 1",
            "Type": "MusicAlbum",
            "RecursiveItemCount": 12,
            "CumulativeRunTimeTicks": 123456789
        }"#;
        let item: JellyfinItem = serde_json::from_str(json).unwrap();
        assert_eq!(item.recursive_item_count, Some(12));
        assert_eq!(item.cumulative_run_time_ticks, Some(123456789));
    }

    #[tokio::test]
    async fn test_authenticate_by_name_success() {
        let mut server = Server::new_async().await;
        let url = server.url();

        let _mock = server
            .mock("POST", "/Users/AuthenticateByName")
            .match_header("Authorization", mockito::Matcher::Regex(r#"MediaBrowser Client="HifiMule".*"#.to_string()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"AccessToken": "new-token-123", "User": {"Id": "user1", "Name": "Alice"}, "SessionInfo": {}}"#)
            .create_async()
            .await;

        let client = JellyfinClient::new();
        let result = client
            .authenticate_by_name(&url, "Alice", "secret")
            .await
            .expect("Failed to authenticate");

        assert_eq!(result.access_token, "new-token-123");
        assert_eq!(result.user.id, "user1");
        assert_eq!(result.user.name, "Alice");
    }

    #[tokio::test]
    async fn test_authenticate_by_name_failure() {
        let mut server = Server::new_async().await;
        let url = server.url();

        let _mock = server
            .mock("POST", "/Users/AuthenticateByName")
            .with_status(401)
            .create_async()
            .await;

        let client = JellyfinClient::new();
        let result = client
            .authenticate_by_name(&url, "Alice", "wrong-password")
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("401"));
    }

    #[test]
    fn test_media_source_deserialization() {
        let json = r#"{
            "Id": "track1",
            "Name": "Track 1",
            "Type": "Audio",
            "MediaSources": [{"Size": 5242880}]
        }"#;
        let item: JellyfinItem = serde_json::from_str(json).unwrap();
        assert!(item.media_sources.is_some());
        let sources = item.media_sources.unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].size, Some(5242880));
    }

    #[test]
    fn test_media_source_missing() {
        let json = r#"{
            "Id": "track1",
            "Name": "Track 1",
            "Type": "Audio"
        }"#;
        let item: JellyfinItem = serde_json::from_str(json).unwrap();
        assert!(item.media_sources.is_none());
    }

    #[tokio::test]
    async fn test_get_item_sizes_individual_track() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let token = "test-token-1234567890";

        // Mock: fetch individual Audio item with MediaSources
        let _mock = server
            .mock("GET", "/Items/track1?userId=user1&Fields=MediaSources")
            .match_header("X-Emby-Token", token)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Id": "track1", "Name": "Track 1", "Type": "Audio", "MediaSources": [{"Size": 5242880}]}"#)
            .create_async()
            .await;

        let client = JellyfinClient::new();
        let results = client
            .get_item_sizes(&url, token, "user1", vec!["track1".to_string()])
            .await;

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "track1");
        assert_eq!(results[0].1, 5242880);
    }

    #[tokio::test]
    async fn test_get_item_sizes_album_container() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let token = "test-token-1234567890";

        // Mock: fetch album item (container type)
        let _mock_album = server
            .mock("GET", "/Items/album1?userId=user1&Fields=MediaSources")
            .match_header("X-Emby-Token", token)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Id": "album1", "Name": "Test Album", "Type": "MusicAlbum"}"#)
            .create_async()
            .await;

        // Mock: fetch child items of album
        let _mock_children = server
            .mock("GET", "/Items?userId=user1&ParentId=album1&IncludeItemTypes=Audio,MusicVideo&Fields=MediaSources&Recursive=true")
            .match_header("X-Emby-Token", token)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Items": [
                {"Id": "t1", "Name": "Track 1", "Type": "Audio", "MediaSources": [{"Size": 3000000}]},
                {"Id": "t2", "Name": "Track 2", "Type": "Audio", "MediaSources": [{"Size": 4000000}]}
            ], "TotalRecordCount": 2, "StartIndex": 0}"#)
            .create_async()
            .await;

        let client = JellyfinClient::new();
        let results = client
            .get_item_sizes(&url, token, "user1", vec!["album1".to_string()])
            .await;

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "album1");
        assert_eq!(results[0].1, 7_000_000); // 3MB + 4MB
    }

    #[tokio::test]
    async fn test_get_item_sizes_artist_container() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let token = "test-token-1234567890";

        // Mock: fetch artist item (container type; no MediaSources on container is deliberate)
        let _mock_artist = server
            .mock("GET", "/Items/artist1?userId=user1&Fields=MediaSources")
            .match_header("X-Emby-Token", token)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Id": "artist1", "Name": "Test Artist", "Type": "MusicArtist"}"#)
            .expect(1)
            .create_async()
            .await;

        // Mock: fetch all tracks under artist (Recursive=true flattens Artist → Albums → Tracks)
        let _mock_children = server
            .mock("GET", "/Items?userId=user1&ParentId=artist1&IncludeItemTypes=Audio,MusicVideo&Fields=MediaSources&Recursive=true")
            .match_header("X-Emby-Token", token)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Items": [
                {"Id": "a1", "Name": "Artist Track 1", "Type": "Audio", "MediaSources": [{"Size": 5000000}]},
                {"Id": "a2", "Name": "Artist Track 2", "Type": "Audio", "MediaSources": [{"Size": 6000000}]}
            ], "TotalRecordCount": 2, "StartIndex": 0}"#)
            .expect(1)
            .create_async()
            .await;

        let client = JellyfinClient::new();
        let results = client
            .get_item_sizes(&url, token, "user1", vec!["artist1".to_string()])
            .await;

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "artist1");
        assert_eq!(results[0].1, 11_000_000); // 5MB + 6MB
    }

    #[tokio::test]
    async fn test_get_item_sizes_artist_empty() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let token = "test-token-1234567890";

        let _mock_artist = server
            .mock("GET", "/Items/artist2?userId=user1&Fields=MediaSources")
            .match_header("X-Emby-Token", token)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Id": "artist2", "Name": "Empty Artist", "Type": "MusicArtist"}"#)
            .expect(1)
            .create_async()
            .await;

        let _mock_children = server
            .mock("GET", "/Items?userId=user1&ParentId=artist2&IncludeItemTypes=Audio,MusicVideo&Fields=MediaSources&Recursive=true")
            .match_header("X-Emby-Token", token)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Items": [], "TotalRecordCount": 0, "StartIndex": 0}"#)
            .expect(1)
            .create_async()
            .await;

        let client = JellyfinClient::new();
        let results = client
            .get_item_sizes(&url, token, "user1", vec!["artist2".to_string()])
            .await;

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "artist2");
        assert_eq!(results[0].1, 0); // No tracks means 0 bytes
    }

    #[tokio::test]
    async fn test_get_item_sizes_artist_children_error() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let token = "test-token-1234567890";

        let _mock_artist = server
            .mock("GET", "/Items/artist3?userId=user1&Fields=MediaSources")
            .match_header("X-Emby-Token", token)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Id": "artist3", "Name": "Restricted Artist", "Type": "MusicArtist"}"#)
            .expect(1)
            .create_async()
            .await;

        // Server error on the children endpoint — production code logs and drops the error
        let _mock_children = server
            .mock("GET", "/Items?userId=user1&ParentId=artist3&IncludeItemTypes=Audio,MusicVideo&Fields=MediaSources&Recursive=true")
            .match_header("X-Emby-Token", token)
            .with_status(500)
            .expect(1)
            .create_async()
            .await;

        let client = JellyfinClient::new();
        let results = client
            .get_item_sizes(&url, token, "user1", vec!["artist3".to_string()])
            .await;

        // Error is silently dropped; item is excluded from results
        assert_eq!(results.len(), 0);
    }

    #[tokio::test]
    async fn test_get_item_sizes_no_media_sources() {
        let mut server = Server::new_async().await;
        let url = server.url();
        let token = "test-token-1234567890";

        // Mock: item with no MediaSources
        let _mock = server
            .mock("GET", "/Items/track1?userId=user1&Fields=MediaSources")
            .match_header("X-Emby-Token", token)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Id": "track1", "Name": "Track 1", "Type": "Audio"}"#)
            .create_async()
            .await;

        let client = JellyfinClient::new();
        let results = client
            .get_item_sizes(&url, token, "user1", vec!["track1".to_string()])
            .await;

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "track1");
        assert_eq!(results[0].1, 0); // No MediaSources means 0 bytes
    }

    #[test]
    fn test_file_storage() {
        let _guard = CREDENTIAL_TEST_MUTEX.lock().unwrap();

        let test_url = "http://localhost:8096";
        let test_token = "test-token-1234567890";

        // Use a unique temporary file for testing to prevent collisions
        let temp_dir = tempfile::tempdir().unwrap();
        let temp_config_path = temp_dir.path().join("test_config.json");

        CredentialManager::set_config_path(temp_config_path.clone());

        // Test Save
        CredentialManager::save_credentials(test_url, test_token, Some("test-user-id"))
            .expect("Failed to save");

        assert!(temp_config_path.exists());

        // Test Get
        let (url, token, user_id) =
            CredentialManager::get_credentials().expect("Failed to retrieve");

        assert_eq!(url, test_url);
        assert_eq!(token, test_token);
        assert_eq!(user_id, Some("test-user-id".to_string()));
    }
}
