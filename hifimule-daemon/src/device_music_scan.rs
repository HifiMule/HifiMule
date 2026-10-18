//! Read-only discovery. A completed scan is a proposal; callers publish it only
//! after checking the captured destination, server and manifest again.
use crate::{
    device::{BasketItem, MediaRole, SyncedItem},
    device_io::{DeviceIO, FileEntry},
    domain::models::Song,
    providers::{MediaProvider, TrackListFilter},
};
use anyhow::{Context,Result};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

#[derive(Default)]
pub struct Progress {
    pub cancelled: AtomicBool,
    pub commit_gate: tokio::sync::Mutex<()>,
    pub committed: AtomicBool,
    pub scanned: AtomicUsize,
    pub total: AtomicUsize,
}
impl Progress {
    pub fn check(&self) -> Result<()> {
        anyhow::ensure!(
            !self.cancelled.load(Ordering::Relaxed),
            "Music scan cancelled"
        );
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub struct Tags {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub track: Option<u32>,
    pub disc: Option<u32>,
    pub duration: u32,
    pub codec: String,
    pub bitrate: Option<u32>,
}
#[derive(Default)]
pub struct ScanResult {
    pub matches: Vec<(Song, FileEntry)>,
    pub unmatched: usize,
    pub ambiguous: usize,
    pub incompatible: usize,
}
pub fn audio_file(path: &str) -> bool {
    // Device paths use forward slashes. macOS AppleDouble sidecars (._*)
    // and other dotfiles are metadata, even when they have an audio extension.
    let filename = path.rsplit('/').next().unwrap_or("");
    if filename.starts_with('.') {
        return false;
    }
    matches!(
        path.rsplit('.')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str(),
        "mp3" | "flac" | "m4a" | "mp4" | "aac" | "ogg" | "opus" | "wav" | "wma" | "aiff" | "alac"
    )
}
fn normal(s: &str) -> String {
    s.trim().to_lowercase()
}
pub fn matches(tags: &Tags, song: &Song) -> bool {
    !tags.title.is_empty()
        && !tags.artist.is_empty()
        && !tags.album.is_empty()
        && normal(&tags.title) == normal(&song.title)
        && song
            .artist_name
            .as_deref()
            .is_some_and(|s| normal(s) == normal(&tags.artist))
        && song
            .album_title
            .as_deref()
            .is_some_and(|s| normal(s) == normal(&tags.album))
        && tags
            .track
            .zip(song.track_number)
            .is_none_or(|(a, b)| a == b)
        && tags.disc.zip(song.disc_number).is_none_or(|(a, b)| a == b)
        && (tags.duration == 0
            || song.duration_seconds == 0
            || tags.duration.abs_diff(song.duration_seconds) <= 2)
}
pub async fn probe(io: &dyn DeviceIO, file: &FileEntry, progress: Arc<Progress>) -> Result<Tags> {
    progress.check()?;
    let mut staged = None;
    let path = if let Some(path) = io.metadata_path(&file.path).await? {
        path
    } else {
        // Object backends stage one file through a bounded streaming buffer.
        let temp = tempfile::Builder::new()
            .suffix(&format!(
                ".{}",
                file.path.rsplit('.').next().unwrap_or("audio")
            ))
            .tempfile()?;
        io.stage_metadata_file(&file.path, temp.path(), file.size, progress.clone())
            .await?;
        let path = temp.path().to_path_buf();
        staged = Some(temp);
        path
    };
    let result = tokio::task::spawn_blocking(move || -> Result<Tags> {
        let _staged = staged;
        ffmpeg_next::init()?;
        let cancel = progress.clone();
        let input = ffmpeg_next::format::input_with_interrupt(&path, move || {
            cancel.cancelled.load(Ordering::Relaxed)
        })?;
        let stream = input
            .streams()
            .best(ffmpeg_next::media::Type::Audio)
            .ok_or(ffmpeg_next::Error::StreamNotFound)?;
        let codec = stream.parameters().id().name().to_string();
        let context = ffmpeg_next::codec::context::Context::from_parameters(stream.parameters())?;
        let decoder = context.decoder().audio()?;
        let tags = input.metadata();
        let stream_tags = stream.metadata();
        let tag = |key: &str| {
            tags.get(key)
                .or_else(|| stream_tags.get(key))
                .unwrap_or("")
                .to_string()
        };
        let number = |s: String| s.split('/').next().and_then(|s| s.parse::<u32>().ok());
        Ok(Tags {
            title: tag("title"),
            artist: tag("artist"),
            album: tag("album"),
            track: number(tag("track")),
            disc: number(tag("disc")),
            duration: (input.duration().max(0) / 1_000_000) as u32,
            codec,
            bitrate: u32::try_from(decoder.bit_rate()).ok().filter(|b| *b > 0),
        })
    })
    .await??;
    Ok(result)
}
pub async fn scan(
    io: &dyn DeviceIO,
    folder: &str,
    provider: &dyn MediaProvider,
    profile: Option<&serde_json::Value>,
    progress: Arc<Progress>,
) -> Result<ScanResult> {
    let files = io.list_files(folder).await?;
    progress.total.store(
        files.iter().filter(|f| audio_file(&f.path)).count(),
        Ordering::Relaxed,
    );
    let mut result = ScanResult::default();
    let mut cache = HashMap::new();
    let mut duplicate = HashSet::new();
    for file in files.into_iter().filter(|f| audio_file(&f.path)) {
        progress.check()?;
        anyhow::ensure!(
            folder.is_empty()
                || file
                    .path
                    .starts_with(&format!("{}/", folder.trim_end_matches('/'))),
            "Scan escaped music folder"
        );
        let tags = match probe(io, &file, progress.clone()).await {
            Ok(tags) => tags,
            Err(error) if error.downcast_ref::<ffmpeg_next::Error>().is_some() => {
                result.incompatible += 1;
                continue;
            }
            Err(error) => return Err(error).with_context(|| format!("Cannot scan {}", file.path)),
        };
        progress.scanned.fetch_add(1, Ordering::Relaxed);
        let ext = file.path.rsplit('.').next().unwrap_or("");
        if !crate::sync::scanned_audio_compatible(
            profile,
            io.preferred_audio_container(),
            ext,
            &tags.codec,
            tags.bitrate,
        ) {
            result.incompatible += 1;
            continue;
        }
        let query = tags.title.clone();
        if query.is_empty() {
            result.unmatched += 1;
            continue;
        }
        if !cache.contains_key(&query) {
            if cache.len() >= 128 {
                cache.clear()
            };
            cache.insert(query.clone(), provider.search(&query).await?);
        }
        let search = &cache[&query];
        if search.possibly_truncated {
            result.ambiguous += 1;
            continue;
        }
        let candidates: Vec<_> = search.songs.iter().filter(|s| matches(&tags, s)).collect();
        if candidates.len() != 1 {
            if candidates.is_empty() {
                result.unmatched += 1
            } else {
                result.ambiguous += 1
            };
            continue;
        }
        let song = candidates[0].clone();
        if duplicate.contains(&song.id) {
            result.ambiguous += 1;
            continue;
        }
        if let Some(index) = result.matches.iter().position(|(s, _)| s.id == song.id) {
            result.matches.remove(index);
            duplicate.insert(song.id);
            result.ambiguous += 2;
            continue;
        }
        result.matches.push((song, file));
    }
    progress.check()?;
    Ok(result)
}
pub fn synced(song: &Song, file: &FileEntry, server: &str) -> SyncedItem {
    SyncedItem {
        is_auto_fill: false,
        media_role: MediaRole::Music,
        jellyfin_id: song.id.clone(),
        name: song.title.clone(),
        album: song.album_title.clone(),
        artist: song.artist_name.clone(),
        local_path: file.path.clone(),
        size_bytes: file.size,
        synced_at: chrono::Utc::now().to_rfc3339(),
        original_name: None,
        etag: None,
        provider_album_id: song.album_id.clone(),
        provider_content_type: song.content_type.clone(),
        provider_suffix: song.suffix.clone(),
        original_bitrate: song.bitrate_kbps.map(|b| b.saturating_mul(1000)),
        original_container: song.suffix.clone(),
        track_number: song.track_number,
        server_id: Some(server.into()),
    }
}
async fn membership(
    provider: &dyn MediaProvider,
    artist: Option<String>,
    album: Option<String>,
    progress: &Progress,
) -> Result<Option<HashSet<String>>> {
    let mut ids = HashSet::new();
    let mut offset = 0;
    loop {
        progress.check()?;
        let page = match provider
            .list_tracks(TrackListFilter {
                artist_id: artist.clone(),
                album_id: album.clone(),
                start_index: offset,
                limit: 400,
                ..Default::default()
            })
            .await
        {
            Ok(page) => page,
            Err(crate::providers::ProviderError::UnsupportedCapability(_)) => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        if page.start_index != offset || page.tracks.is_empty() {
            return Ok(None);
        }
        let count = page.tracks.len() as u32;
        for song in page.tracks {
            if !ids.insert(song.id) {
                return Ok(None);
            }
        }
        offset += count;
        if offset == page.total {
            return Ok(Some(ids));
        }
        if offset > page.total || offset > 100_000 {
            return Ok(None);
        }
    }
}
pub async fn group(
    result: &ScanResult,
    provider: &dyn MediaProvider,
    server: &str,
    progress: &Progress,
) -> Result<Vec<BasketItem>> {
    let mut remaining: HashSet<_> = result.matches.iter().map(|(s, _)| s.id.clone()).collect();
    let mut basket = Vec::new();
    for artist in [true, false] {
        let groups: HashMap<_, _> = result
            .matches
            .iter()
            .filter_map(|(s, _)| {
                if artist {
                    s.artist_id.clone().zip(s.artist_name.clone())
                } else {
                    s.album_id.clone().zip(s.album_title.clone())
                }
            })
            .collect();
        for (id, name) in groups {
            let Some(ids) = membership(
                provider,
                artist.then(|| id.clone()),
                (!artist).then(|| id.clone()),
                progress,
            )
            .await?
            else {
                continue;
            };
            if ids.is_empty() || !ids.is_subset(&remaining) {
                continue;
            }
            let records: Vec<_> = result
                .matches
                .iter()
                .filter(|(s, _)| ids.contains(&s.id))
                .collect();
            basket.push(BasketItem {
                id,
                name,
                item_type: if artist { "MusicArtist" } else { "MusicAlbum" }.into(),
                server_id: Some(server.into()),
                artist: None,
                child_count: ids.len() as u32,
                size_ticks: records
                    .iter()
                    .map(|(s, _)| i64::from(s.duration_seconds) * 10_000_000)
                    .sum(),
                size_bytes: records.iter().map(|(_, f)| f.size).sum(),
            });
            remaining.retain(|id| !ids.contains(id));
        }
    }
    for (song, file) in &result.matches {
        if remaining.contains(&song.id) {
            basket.push(BasketItem {
                id: song.id.clone(),
                name: song.title.clone(),
                item_type: "Audio".into(),
                server_id: Some(server.into()),
                artist: song.artist_name.clone(),
                child_count: 1,
                size_ticks: i64::from(song.duration_seconds) * 10_000_000,
                size_bytes: file.size,
            })
        }
    }
    Ok(basket)
}

/// Reuse only selected additions, never import unrelated files during later syncs.
/// Self-rekeys carry the adoption through the existing transactional plan path.
pub async fn reconcile(
    delta: &mut crate::sync::SyncDelta,
    manifest: &crate::device::DeviceManifest,
    io: &dyn DeviceIO,
    profile: Option<&serde_json::Value>,
    manager: Option<&Arc<crate::sync::SyncOperationManager>>,
) -> Result<()> {
    let mut lifecycle = crate::sync_logging::Step::current("reconciliation");
    if !manifest.reconcile_before_sync {
        lifecycle.finish("skipped", 0);
        return Ok(());
    }
    let mut summary = ReconciliationSummary::default();
    crate::daemon_log!("[Sync] Reconciliation started: device={}", manifest.device_id);
    let result = reconcile_enabled(delta, manifest, io, profile, manager, &mut summary).await;
    crate::daemon_log!("{}", summary.terminal_message(&manifest.device_id, &result, manager.is_some_and(|m| m.is_pipeline_cancelled())));
    lifecycle.finish(if manager.is_some_and(|m| m.is_pipeline_cancelled()) { "cancelled" } else if result.is_err() { "failed" } else { "success" }, summary.reused);
    result
}

#[derive(Default, Debug, PartialEq)]
struct ReconciliationSummary {
    reused: usize,
    unmatched: usize,
    ambiguous: usize,
    incompatible: usize,
}

impl ReconciliationSummary {
    fn terminal_message(&self, device: &str, result: &Result<()>, cancelled: bool) -> String {
        match result {
            Ok(()) => format!("[Sync] Reconciliation completed: device={device} reused={} unmatched={} ambiguous={} incompatible={}", self.reused, self.unmatched, self.ambiguous, self.incompatible),
            Err(error) => format!("[Sync] Reconciliation ended: device={device} outcome={} error={error}", if cancelled { "cancelled" } else { "failed" }),
        }
    }
}

async fn reconcile_enabled(
    delta: &mut crate::sync::SyncDelta,
    manifest: &crate::device::DeviceManifest,
    io: &dyn DeviceIO,
    profile: Option<&serde_json::Value>,
    manager: Option<&Arc<crate::sync::SyncOperationManager>>,
    summary: &mut ReconciliationSummary,
) -> Result<()> {
    let progress = Arc::new(Progress::default());
    if manager.is_some_and(|m| m.is_pipeline_cancelled()) {
        progress.cancelled.store(true, Ordering::Relaxed);
    }
    progress.check()?;
    struct CancelBridge(Option<tokio::task::JoinHandle<()>>);
    impl Drop for CancelBridge {
        fn drop(&mut self) {
            if let Some(task) = self.0.take() {
                task.abort();
            }
        }
    }
    let _bridge = CancelBridge(manager.map(|manager| {
        let manager = manager.clone();
        let progress = progress.clone();
        tokio::spawn(async move {
            loop {
                if manager.is_pipeline_cancelled() {
                    progress.cancelled.store(true, Ordering::Relaxed);
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        })
    }));
    for item in &manifest.synced_items {
        progress.check()?;
        if delta
            .adds
            .iter()
            .any(|a| a.jellyfin_id == item.jellyfin_id && a.server_id == item.server_id)
            || delta
                .deletes
                .iter()
                .any(|d| d.jellyfin_id == item.jellyfin_id)
            || delta
                .id_changes
                .iter()
                .any(|d| d.old_jellyfin_id == item.jellyfin_id)
        {
            continue;
        }
        if let Some(size) = io.file_length(&item.local_path).await? {
            if size != item.size_bytes {
                delta.id_changes.push(crate::sync::SyncIdChangeItem {
                    is_auto_fill: Some(item.is_auto_fill),
                    media_role: item.media_role,
                    old_jellyfin_id: item.jellyfin_id.clone(),
                    new_jellyfin_id: item.jellyfin_id.clone(),
                    old_local_path: item.local_path.clone(),
                    name: item.name.clone(),
                    album: item.album.clone(),
                    artist: item.artist.clone(),
                    size_bytes: size,
                    etag: item.etag.clone(),
                    provider_album_id: item.provider_album_id.clone(),
                    provider_content_type: item.provider_content_type.clone(),
                    provider_suffix: item.provider_suffix.clone(),
                    original_name: item.original_name.clone(),
                    reason_code: Some("device-file-reused".into()),
                    reason: Some("Refresh measured device size".into()),
                    source_server_id: item.server_id.clone(),
                });
                delta.unchanged = delta.unchanged.saturating_sub(1);
            }
        }
    }
    if delta.adds.is_empty() {
        progress.check()?;
        anyhow::ensure!(!manager.is_some_and(|m| m.is_pipeline_cancelled()), "Music scan cancelled");
        return Ok(());
    }
    let folder = manifest.media_path(MediaRole::Music).unwrap_or("");
    let files = io.list_files(folder).await?;
    let mut found: HashMap<usize, Option<FileEntry>> = HashMap::new();
    for file in files.into_iter().filter(|f| audio_file(&f.path)) {
        progress.check()?;
        // Already-owned files are eligible only for their own track. Unrelated
        // managed music must not acquire a second owner through matching tags.
        let owner = manifest
            .synced_items
            .iter()
            .find(|s| s.local_path == file.path);
        if owner.is_some_and(|s| {
            !delta
                .adds
                .iter()
                .any(|a| a.jellyfin_id == s.jellyfin_id && a.server_id == s.server_id)
        }) {
            continue;
        }
        let tags = match probe(io, &file, progress.clone()).await {
            Ok(tags) => tags,
            Err(error) if error.downcast_ref::<ffmpeg_next::Error>().is_some() => {
                summary.incompatible += 1;
                crate::daemon_log!("[Sync] Cannot reuse '{}': audio metadata probe failed: {}", file.path, error);
                continue;
            },
            Err(error) => return Err(error),
        };
        if !crate::sync::scanned_audio_compatible(
            profile,
            io.preferred_audio_container(),
            file.path.rsplit('.').next().unwrap_or(""),
            &tags.codec,
            tags.bitrate,
        ) {
            summary.incompatible += 1;
            crate::daemon_log!("[Sync] Cannot reuse '{}': encoding is incompatible with selected profile", file.path);
            continue;
        }
        let candidates: Vec<_> = delta
            .adds
            .iter()
            .enumerate()
            .filter(|(_, a)| {
                a.media_role == MediaRole::Music
                    && owner.is_none_or(|s| {
                        s.jellyfin_id == a.jellyfin_id && s.server_id == a.server_id
                    })
                    && !tags.artist.is_empty()
                    && !tags.album.is_empty()
                    && normal(&a.name) == normal(&tags.title)
                    && a.artist
                        .as_deref()
                        .is_some_and(|s| normal(s) == normal(&tags.artist))
                    && a.album
                        .as_deref()
                        .is_some_and(|s| normal(s) == normal(&tags.album))
                    && tags.track.zip(a.track_number).is_none_or(|(a, b)| a == b)
                    && tags.disc.zip(a.disc_number).is_none_or(|(a,b)| a == b)
                    && (a.duration_seconds == 0
                        || tags.duration == 0
                        || a.duration_seconds.abs_diff(tags.duration) <= 2)
                    && a.max_bitrate_override_kbps.is_none_or(|b| {
                        tags.bitrate
                            .is_some_and(|bps| bps <= b.saturating_mul(1050))
                    })
            })
            .map(|(i, _)| i)
            .collect();
        if candidates.is_empty() { summary.unmatched += 1; }
        else if candidates.len() > 1 { summary.ambiguous += 1; }
        if candidates.len() != 1 {
            crate::daemon_log!("[Sync] Cannot reuse '{}': {} selected tracks match its metadata (need exactly one)", file.path, candidates.len());
        }
        if candidates.len() == 1 {
            let i = candidates[0];
            if let Some(previous) = found.get(&i) {
                summary.ambiguous += if previous.is_some() { 2 } else { 1 };
                found.insert(i, None);
            } else {
                found.insert(i, Some(file));
            }
        }
    }
    progress.check()?;
    if manager.is_some_and(|m| m.is_pipeline_cancelled()) {
        anyhow::bail!("Music scan cancelled");
    }
    let mut retained = Vec::new();
    for (i, a) in delta.adds.drain(..).enumerate() {
        if let Some(Some(file)) = found.remove(&i) {
            summary.reused += 1;
            // An incompatible old owned copy may be removed after this adoption;
            // never schedule deletion of the very file being reused.
            delta.deletes.retain(|d| d.local_path != file.path);
            delta
                .id_changes
                .retain(|c| c.new_jellyfin_id != a.jellyfin_id);
            delta.id_changes.push(crate::sync::SyncIdChangeItem {
                is_auto_fill: Some(a.is_auto_fill),
                media_role: a.media_role,
                old_jellyfin_id: a.jellyfin_id.clone(),
                new_jellyfin_id: a.jellyfin_id,
                old_local_path: file.path,
                name: a.name,
                album: a.album,
                artist: a.artist,
                size_bytes: file.size,
                etag: a.etag,
                provider_album_id: a.provider_album_id,
                provider_content_type: a.provider_content_type,
                provider_suffix: a.provider_suffix,
                original_name: None,
                reason_code: Some("device-file-reused".into()),
                reason: Some("Reuse compatible music already on device".into()),
                source_server_id: a.server_id,
            });
        } else {
            retained.push(a)
        }
    }
    delta.adds = retained;
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::providers::{
        BrowseCapabilities, Capabilities, ProviderChangeContext, ProviderError, ScrobbleRequest,
        ServerType, TranscodeProfile,
    };
    use async_trait::async_trait;
    pub(crate) struct FixtureProvider {
        pub songs: Vec<Song>,
        pub truncated: bool,
    }
    #[async_trait]
    impl MediaProvider for FixtureProvider {
        fn server_type(&self) -> ServerType {
            ServerType::Unknown
        }

        fn capabilities(&self) -> Capabilities {
            Capabilities {
                open_subsonic: false,
                supports_changes_since: false,
                supports_server_transcoding: false,
                supports_playlist_write: false,
                browse: BrowseCapabilities { list_modes: vec![] },
            }
        }

        async fn list_tracks(
            &self,
            filter: TrackListFilter,
        ) -> Result<crate::providers::TrackListPage, ProviderError> {
            let tracks: Vec<_> = self
                .songs
                .iter()
                .filter(|s| {
                    filter
                        .artist_id
                        .as_ref()
                        .is_none_or(|id| s.artist_id.as_ref() == Some(id))
                        && filter
                            .album_id
                            .as_ref()
                            .is_none_or(|id| s.album_id.as_ref() == Some(id))
                })
                .cloned()
                .collect();
            Ok(crate::providers::TrackListPage {
                total: tracks.len() as u32,
                tracks: tracks
                    .into_iter()
                    .skip(filter.start_index as usize)
                    .take(filter.limit as usize)
                    .collect(),
                start_index: filter.start_index,
                limit: filter.limit,
            })
        }
        async fn list_libraries(
            &self,
        ) -> Result<Vec<crate::domain::models::Library>, ProviderError> {
            Err(ProviderError::UnsupportedCapability(
                "list_libraries".to_string(),
            ))
        }

        async fn list_artists(
            &self,
            _library_id: Option<&str>,
            _letter: Option<&str>,
            _offset: u32,
            _limit: u32,
        ) -> Result<(Vec<crate::domain::models::Artist>, u32), ProviderError> {
            Err(ProviderError::UnsupportedCapability(
                "list_artists".to_string(),
            ))
        }

        async fn get_artist(
            &self,
            _artist_id: &str,
        ) -> Result<crate::domain::models::ArtistWithAlbums, ProviderError> {
            Err(ProviderError::UnsupportedCapability(
                "get_artist".to_string(),
            ))
        }

        async fn list_albums(
            &self,
            _library_id: Option<&str>,
            _letter: Option<&str>,
            _offset: u32,
            _limit: u32,
        ) -> Result<(Vec<crate::domain::models::Album>, u32), ProviderError> {
            Err(ProviderError::UnsupportedCapability(
                "list_albums".to_string(),
            ))
        }

        async fn get_album(
            &self,
            _album_id: &str,
        ) -> Result<crate::domain::models::AlbumWithTracks, ProviderError> {
            Err(ProviderError::UnsupportedCapability(
                "get_album".to_string(),
            ))
        }

        async fn list_playlists(
            &self,
        ) -> Result<Vec<crate::domain::models::Playlist>, ProviderError> {
            Err(ProviderError::UnsupportedCapability(
                "list_playlists".to_string(),
            ))
        }

        async fn get_playlist(
            &self,
            _playlist_id: &str,
        ) -> Result<crate::domain::models::PlaylistWithTracks, ProviderError> {
            Err(ProviderError::UnsupportedCapability(
                "get_playlist".to_string(),
            ))
        }

        async fn search(
            &self,
            _query: &str,
        ) -> Result<crate::domain::models::SearchResult, ProviderError> {
            Ok(crate::domain::models::SearchResult {
                songs: self.songs.clone(),
                possibly_truncated: self.truncated,
                ..Default::default()
            })
        }

        async fn download_url(
            &self,
            _song_id: &str,
            _profile: Option<&TranscodeProfile>,
        ) -> Result<String, ProviderError> {
            Err(ProviderError::UnsupportedCapability(
                "download_url".to_string(),
            ))
        }

        async fn cover_art_url(&self, _cover_art_id: &str) -> Result<String, ProviderError> {
            Err(ProviderError::UnsupportedCapability(
                "cover_art_url".to_string(),
            ))
        }

        async fn changes_since_with_context(
            &self,
            _token: Option<&str>,
            _context: &ProviderChangeContext,
        ) -> Result<Vec<crate::domain::models::ChangeEvent>, ProviderError> {
            Err(ProviderError::UnsupportedCapability(
                "changes_since_with_context".to_string(),
            ))
        }

        async fn scrobble(&self, _request: ScrobbleRequest) -> Result<(), ProviderError> {
            Err(ProviderError::UnsupportedCapability("scrobble".to_string()))
        }
    }

    pub(crate) fn song() -> Song {
        serde_json::from_value(serde_json::json!({"id":"track-1","title":"Track","artistId":"artist-1","artistName":"Artist","albumId":"album-1","albumName":"Album","duration":4,"trackNumber":1,"discNumber":1,"bitrateKbps":1000,"suffix":"flac","sizeBytes":50000000})).unwrap()
    }
    fn tags() -> Tags {
        Tags {
            title: "Track".into(),
            artist: "Artist".into(),
            album: "Album".into(),
            track: Some(1),
            disc: Some(1),
            duration: 4,
            codec: "mp3".into(),
            bitrate: Some(192000),
        }
    }
    fn profile() -> serde_json::Value {
        serde_json::json!({"DirectPlayProfiles":[{"Type":"Audio","Container":"mp3","AudioCodec":"mp3"}],"TranscodingProfiles":[{"Type":"Audio","Container":"mp3","AudioCodec":"mp3"}],"MusicStreamingTranscodingBitrate":192000})
    }
    #[test]
    fn matching_requires_consistent_metadata_and_rejects_editions() {
        assert!(matches(&tags(), &song()));
        let mut other = song();
        other.disc_number = Some(2);
        assert!(!matches(&tags(), &other));
        other = song();
        other.duration_seconds = 40;
        assert!(!matches(&tags(), &other));
        let mut absent = tags();
        absent.album.clear();
        assert!(!matches(&absent, &song()));
    }
    #[test]
    fn profile_checks_extension_codec_and_compression() {
        let p = profile();
        assert!(crate::sync::scanned_audio_compatible(
            Some(&p),
            None,
            "mp3",
            "mp3",
            Some(192000)
        ));
        assert!(!crate::sync::scanned_audio_compatible(
            Some(&p),
            None,
            "flac",
            "flac",
            Some(1000000)
        ));
        assert!(!crate::sync::scanned_audio_compatible(
            Some(&p),
            None,
            "mp3",
            "aac",
            Some(192000)
        ));
        assert!(!crate::sync::scanned_audio_compatible(
            Some(&p),
            None,
            "mp3",
            "mp3",
            Some(320000)
        ));
        assert!(!crate::sync::scanned_audio_compatible(
            Some(&p),
            None,
            "mp3",
            "mp3",
            None
        ));
    }
    #[test]
    fn estimates_target_output_for_any_source_without_encoding() {
        let p = profile();
        let mut s = song();
        s.duration_seconds = 240;
        assert_eq!(
            crate::sync::planned_song_bytes(&s, Some(&p), None),
            5_998_336
        );
        s.suffix = Some("mp3".into());
        s.bitrate_kbps = Some(320);
        assert_eq!(
            crate::sync::planned_song_bytes(&s, Some(&p), None),
            5_998_336
        );
        s.bitrate_kbps = Some(128);
        assert_eq!(
            crate::sync::planned_song_bytes(&s, Some(&p), None),
            50_000_000
        );
        assert_eq!(crate::sync::planned_song_bytes(&s, None, None), 50_000_000);
        assert!(crate::sync::planned_song_bytes(&s, Some(&p), Some(96)) < 5_998_336);
        s.duration_seconds = 0;
        s.bitrate_kbps = Some(320);
        s.size_bytes = Some(1);
        assert!(crate::sync::planned_song_bytes(&s, Some(&p), None) > 80_000_000);
    }
    pub(crate) async fn fixture() -> (tempfile::TempDir, crate::device_io::MscBackend, FileEntry) {
        let root = tempfile::tempdir().unwrap();
        tokio::fs::create_dir(root.path().join("Music"))
            .await
            .unwrap();
        let original = include_bytes!("../tests/fixtures/generated-seek-mp3.mp3");
        let mut frames = Vec::new();
        for (id, value) in [
            ("TIT2", "Track"),
            ("TPE1", "Artist"),
            ("TALB", "Album"),
            ("TRCK", "1"),
            ("TPOS", "1"),
        ] {
            frames.extend_from_slice(id.as_bytes());
            frames.extend_from_slice(&((value.len() + 1) as u32).to_be_bytes());
            frames.extend_from_slice(&[0, 0, 0]);
            frames.extend_from_slice(value.as_bytes());
        }
        let n = frames.len();
        let mut bytes = b"ID3\x03\x00\x00".to_vec();
        bytes.extend_from_slice(&[
            ((n >> 21) & 127) as u8,
            ((n >> 14) & 127) as u8,
            ((n >> 7) & 127) as u8,
            (n & 127) as u8,
        ]);
        bytes.extend_from_slice(&frames);
        let old_len = if original.starts_with(b"ID3") {
            10 + original[6..10]
                .iter()
                .fold(0usize, |n, b| (n << 7) | usize::from(*b))
        } else {
            0
        };
        bytes.extend_from_slice(&original[old_len..]);
        tokio::fs::write(root.path().join("Music/existing.mp3"), &bytes)
            .await
            .unwrap();
        let io = crate::device_io::MscBackend::new(root.path().to_path_buf());
        let file = FileEntry {
            path: "Music/existing.mp3".into(),
            name: "existing.mp3".into(),
            size: bytes.len() as u64,
        };
        (root, io, file)
    }
    #[test]
    fn reconciliation_terminal_logs_distinguish_success_failure_and_cancel() {
        let summary = ReconciliationSummary::default();
        let success = summary.terminal_message("device-a", &Ok(()), false);
        assert!(success.contains("device=device-a reused=0 unmatched=0 ambiguous=0 incompatible=0"));
        let error = Err(anyhow::anyhow!("disconnected"));
        for (cancelled, outcome) in [(false, "failed"), (true, "cancelled")] {
            let message = summary.terminal_message("device-a", &error, cancelled);
            assert!(message.contains(&format!("outcome={outcome}")));
            assert!(!message.contains("completed"));
        }
    }

    fn pending_delta() -> crate::sync::SyncDelta {
        serde_json::from_value(serde_json::json!({"adds":[{"jellyfinId":"track-1","name":"Track","album":"Album","artist":"Artist","sizeBytes":100,"providerSuffix":"mp3"}],"deletes":[],"idChanges":[],"unchanged":0,"playlists":[]})).unwrap()
    }

    #[tokio::test]
    async fn disabled_reconciliation_skips_invalid_device_and_cancellation() {
        let io = crate::device_io::MscBackend::new(std::path::PathBuf::from("/missing-hifimule-device"));
        let mut manifest = crate::device::DeviceManifest::default();
        manifest.managed_paths = vec!["../escape".into()];
        let mut delta = pending_delta();
        let manager = Arc::new(crate::sync::SyncOperationManager::new());
        let _guard = manager.try_start_pipeline().unwrap();
        assert!(manager.request_pipeline_cancel());
        reconcile(&mut delta, &manifest, &io, None, Some(&manager)).await.unwrap();
        assert_eq!(delta.adds.len(), 1);
        manifest.reconcile_before_sync = true;
        assert!(reconcile(&mut delta, &manifest, &io, None, Some(&manager)).await.unwrap_err().to_string().contains("cancelled"));
        assert!(reconcile(&mut delta, &manifest, &io, None, None).await.is_err());
    }

    #[tokio::test]
    async fn summary_counts_final_matches_duplicates_unmatched_and_incompatible() {
        let (root, io, file) = fixture().await;
        let manifest = crate::device::DeviceManifest { reconcile_before_sync: true, managed_paths: vec!["Music".into()], ..Default::default() };
        let mut summary = ReconciliationSummary::default();
        reconcile_enabled(&mut pending_delta(), &manifest, &io, Some(&profile()), None, &mut summary).await.unwrap();
        assert_eq!(summary, ReconciliationSummary { reused: 1, ..Default::default() });
        for name in ["duplicate", "third"] {
            tokio::fs::copy(root.path().join(&file.path), root.path().join(format!("Music/{name}.mp3"))).await.unwrap();
        }
        let mut summary = ReconciliationSummary::default();
        reconcile_enabled(&mut pending_delta(), &manifest, &io, Some(&profile()), None, &mut summary).await.unwrap();
        assert_eq!(summary, ReconciliationSummary { ambiguous: 3, ..Default::default() });
        let mut delta = pending_delta(); delta.adds[0].name = "Missing".into();
        let mut summary = ReconciliationSummary::default();
        reconcile_enabled(&mut delta, &manifest, &io, Some(&profile()), None, &mut summary).await.unwrap();
        assert_eq!(summary.unmatched, 3);
        tokio::fs::write(root.path().join("Music/broken.mp3"), b"not audio").await.unwrap();
        let mut summary = ReconciliationSummary::default();
        reconcile_enabled(&mut pending_delta(), &manifest, &io, Some(&profile()), None, &mut summary).await.unwrap();
        assert_eq!(summary.incompatible, 1);
        let mut delta = pending_delta(); delta.adds.clear();
        let mut summary = ReconciliationSummary::default();
        reconcile_enabled(&mut delta, &manifest, &io, Some(&profile()), None, &mut summary).await.unwrap();
        assert_eq!(summary, ReconciliationSummary::default());
        // Refreshing an owned file's measured size is not a newly matched file.
        let mut manifest = manifest;
        let mut owned = synced(&song(), &file, "server-a");
        owned.size_bytes = 1;
        manifest.synced_items.push(owned);
        delta.unchanged = 1;
        reconcile_enabled(&mut delta, &manifest, &io, Some(&profile()), None, &mut summary).await.unwrap();
        assert_eq!(delta.id_changes.len(), 1);
        assert_eq!(delta.id_changes[0].size_bytes, file.size);
        assert_eq!(summary, ReconciliationSummary::default());
    }

    #[tokio::test]
    async fn tagged_fixture_is_probed_without_copying_and_cancellation_stops() {
        let (_root, io, file) = fixture().await;
        let progress = Arc::new(Progress::default());
        let metadata = probe(&io, &file, progress.clone()).await.unwrap();
        assert_eq!(metadata.title, "Track");
        assert_eq!(metadata.artist, "Artist");
        assert_eq!(metadata.album, "Album");
        assert_eq!(metadata.codec, "mp3");
        progress.cancelled.store(true, Ordering::Relaxed);
        assert!(probe(&io, &file, progress).await.is_err());
    }
    #[tokio::test]
    async fn incompatible_and_mismatched_existing_files_remain_pending() {
        let (root, io, file) = fixture().await;
        let manifest = crate::device::DeviceManifest { reconcile_before_sync: true, managed_paths: vec!["Music".into()], ..Default::default() };
        let original: crate::sync::SyncDelta = serde_json::from_value(serde_json::json!({
            "adds":[{"jellyfinId":"track-1","name":"Track","album":"Album","artist":"Artist","sizeBytes":100,"providerSuffix":"mp3"}],
            "deletes":[],"idChanges":[],"unchanged":0,"playlists":[]
        })).unwrap();
        let before = tokio::fs::read(root.path().join(&file.path)).await.unwrap();
        let mut mismatch = original.clone();
        mismatch.adds[0].name = "Different Track".into();
        reconcile(&mut mismatch, &manifest, &io, Some(&profile()), None).await.unwrap();
        assert_eq!(mismatch.adds.len(), 1);
        assert!(mismatch.id_changes.is_empty());
        let mut incompatible = original;
        incompatible.adds[0].max_bitrate_override_kbps = Some(1);
        reconcile(&mut incompatible, &manifest, &io, Some(&profile()), None).await.unwrap();
        assert_eq!(incompatible.adds.len(), 1);
        assert!(incompatible.id_changes.is_empty());
        assert_eq!(tokio::fs::read(root.path().join(&file.path)).await.unwrap(), before);
    }

    #[tokio::test]
    async fn later_file_becomes_read_only_adoption_and_duplicates_stay_unmanaged() {
        let (root, io, file) = fixture().await;
        let manifest:crate::device::DeviceManifest=serde_json::from_value(serde_json::json!({"device_id":"device","version":"1.0","reconcileBeforeSync":true,"managedPaths":["Music"],"syncedItems":[]})).unwrap();
        let mut delta:crate::sync::SyncDelta=serde_json::from_value(serde_json::json!({"adds":[{"jellyfinId":"track-1","name":"Track","album":"Album","artist":"Artist","sizeBytes":50000000,"providerSuffix":"flac","serverId":"server-a"}],"deletes":[],"idChanges":[],"unchanged":0,"playlists":[]})).unwrap();
        let mut wrong_disc=delta.clone();wrong_disc.adds[0].disc_number=Some(2);
        reconcile(&mut wrong_disc,&manifest,&io,Some(&profile()),None).await.unwrap();
        assert_eq!(wrong_disc.adds.len(),1);
        assert!(wrong_disc.id_changes.is_empty());
        let original = delta.clone();
        reconcile(&mut delta, &manifest, &io, Some(&profile()), None)
            .await
            .unwrap();
        assert!(delta.adds.is_empty());
        assert_eq!(delta.id_changes[0].size_bytes, file.size);
        assert_eq!(
            delta.id_changes[0].source_server_id.as_deref(),
            Some("server-a")
        );
        assert!(manifest.synced_items.is_empty());
        tokio::fs::copy(
            root.path().join(&file.path),
            root.path().join("Music/duplicate.mp3"),
        )
        .await
        .unwrap();
        let mut duplicate = original;
        reconcile(&mut duplicate, &manifest, &io, Some(&profile()), None)
            .await
            .unwrap();
        assert_eq!(duplicate.adds.len(), 1);
        assert!(duplicate.id_changes.is_empty());
    }
    #[tokio::test]
    async fn video_only_content_is_skipped_without_aborting_scan() {
        let (root,io,_)=fixture().await;
        let image=include_bytes!("../tests/fixtures/generated-video-only.mp4");
        tokio::fs::write(root.path().join("Music/video.mp4"),&image).await.unwrap();
        let file=FileEntry{path:"Music/video.mp4".into(),name:"video.mp4".into(),size:image.len() as u64};
        let error=probe(&io,&file,Arc::new(Progress::default())).await.unwrap_err();
        assert!(matches!(error.downcast_ref::<ffmpeg_next::Error>(),Some(ffmpeg_next::Error::StreamNotFound)));
        let provider=FixtureProvider{songs:vec![song()],truncated:false};
        let result=scan(&io,"Music",&provider,Some(&profile()),Arc::new(Progress::default())).await.unwrap();
        assert_eq!(result.incompatible,1);
    }
    #[tokio::test]
    async fn strict_io_distinguishes_absence_and_rejects_escape() {
        let (root, io, _) = fixture().await;
        assert_eq!(io.file_length("Music/missing.mp3").await.unwrap(), None);
        assert!(io.file_length("Music").await.is_err());
        #[cfg(unix)]
        {
            let outside = tempfile::tempdir().unwrap();
            std::fs::write(outside.path().join("outside.mp3"), b"audio").unwrap();
            std::os::unix::fs::symlink(outside.path(), root.path().join("escape")).unwrap();
            assert!(io.metadata_path("escape/outside.mp3").await.is_err());
        }
    }
    #[tokio::test]
    async fn scan_rejects_incomplete_search_and_groups_only_complete_memberships() {
        let (_root, io, _) = fixture().await;
        let mut first = song();
        first.duration_seconds = 2;
        let provider = FixtureProvider {
            songs: vec![first.clone()],
            truncated: false,
        };
        let result = scan(
            &io,
            "Music",
            &provider,
            Some(&profile()),
            Arc::new(Progress::default()),
        )
        .await
        .unwrap();
        assert_eq!(result.matches.len(), 1);
        let basket = group(&result, &provider, "server-a", &Progress::default())
            .await
            .unwrap();
        assert_eq!(basket.len(), 1);
        assert_eq!(basket[0].item_type, "MusicArtist");
        assert_eq!(basket[0].server_id.as_deref(), Some("server-a"));
        let mut second = first.clone();
        second.id = "other-track".into();
        second.album_id = Some("other-album".into());
        let partial = FixtureProvider {
            songs: vec![first.clone(), second],
            truncated: false,
        };
        let basket = group(&result, &partial, "server-a", &Progress::default())
            .await
            .unwrap();
        assert_eq!(basket[0].item_type, "MusicAlbum");
        let mut second = first.clone();
        second.id = "missing-album-track".into();
        let partial = FixtureProvider {
            songs: vec![first, second],
            truncated: false,
        };
        let basket = group(&result, &partial, "server-a", &Progress::default())
            .await
            .unwrap();
        assert_eq!(basket[0].item_type, "Audio");
        let incomplete = FixtureProvider {
            songs: provider.songs,
            truncated: true,
        };
        let result = scan(
            &io,
            "Music",
            &incomplete,
            Some(&profile()),
            Arc::new(Progress::default()),
        )
        .await
        .unwrap();
        assert!(result.matches.is_empty());
        assert_eq!(result.ambiguous, 1);
    }
}
