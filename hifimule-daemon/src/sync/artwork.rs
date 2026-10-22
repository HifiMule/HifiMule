//! Sequential optional artwork work, independent from audio success/failure.
use super::*;
use crate::device::{FolderArtworkSource, MediaRole};
use crate::device_io::DeviceIO;
use crate::domain::models::Song;
use std::collections::BTreeMap;
use std::io::Cursor;

const MAX_RESPONSE: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrackArtwork {
    pub artist_id: Option<String>,
    pub cover_id: Option<String>,
}
impl TrackArtwork {
    pub(crate) fn from_jellyfin(item: &crate::api::JellyfinItem) -> Self {
        Self {
            artist_id: item
                .artist_items
                .as_ref()
                .and_then(|items| items.first())
                .map(|artist| artist.id.clone()),
            cover_id: item
                .image_tags
                .as_ref()
                .filter(|tags| tags.contains_key("Primary"))
                .map(|_| item.id.clone()),
        }
    }
}
impl From<&Song> for TrackArtwork {
    fn from(song: &Song) -> Self {
        Self {
            artist_id: song.artist_id.clone(),
            cover_id: song.cover_art_id.clone(),
        }
    }
}

#[derive(Clone)]
struct Candidate {
    role: MediaRole,
    server: Option<String>,
    album: Option<String>,
    track: String,
    artist: bool,
    known: Option<TrackArtwork>,
}

fn folder_targets(path: &str, role: MediaRole) -> Vec<(&str, bool)> {
    let Some((content, _)) = path.rsplit_once('/') else { return Vec::new() };
    if role == MediaRole::Podcast {
        return vec![(content, false)];
    }
    let Some((creator, _)) = content.rsplit_once('/') else { return Vec::new() };
    vec![(creator, true), (content, false)]
}

fn managed_roots(manifest: &DeviceManifest) -> std::collections::BTreeSet<String> {
    manifest.media_paths().into_iter().chain(manifest.managed_paths.iter().map(String::as_str))
        .map(normalized_device_folder).filter(|path| !path.is_empty()).collect()
}

fn eligible_folder(
    folder: &str,
    role: MediaRole,
    artist: bool,
    manifest: &DeviceManifest,
    identities: &HashMap<String, String>,
) -> bool {
    let Some(identity) = identities.get(folder) else { return false };
    if managed_roots(manifest).iter().any(|root| identities.get(root) == Some(identity)) {
        return false;
    }
    let Some(root) = manifest.media_path(role).map(normalized_device_folder) else { return false };
    let Some(root_identity) = identities.get(&root) else { return false };
    // Two-level roles require their creator folder below the root as well.
    let anchor = if !artist && role != MediaRole::Podcast {
        let Some((parent, _)) = folder.rsplit_once('/') else { return false };
        parent
    } else { folder };
    if identities.get(anchor) == Some(root_identity) { return false; }
    let mut ancestor = anchor.rsplit_once('/').map(|(parent, _)| parent);
    while let Some(parent) = ancestor {
        if identities.get(parent) == Some(root_identity) { return true; }
        ancestor = parent.rsplit_once('/').map(|(parent, _)| parent);
    }
    false
}

fn live_folders(manifest: &DeviceManifest) -> BTreeMap<String, Vec<Candidate>> {
    let mut folders: BTreeMap<String, Vec<Candidate>> = BTreeMap::new();
    for track in &manifest.synced_items {
        let path = normalized_device_folder(&track.local_path);
        if validate_device_relative_path(&path).is_err() {
            continue;
        }
        for (folder, is_artist) in folder_targets(&path, track.media_role) {
            folders
                .entry(folder.to_owned())
                .or_default()
                .push(Candidate {
                    role: track.media_role,
                    server: track.server_id.clone(),
                    album: track.provider_album_id.clone(),
                    track: track.jellyfin_id.clone(),
                    artist: is_artist,
                    known: None,
                });
        }
    }
    for candidates in folders.values_mut() {
        candidates
            .sort_by(|a, b| (&a.server, &a.album, &a.track).cmp(&(&b.server, &b.album, &b.track)));
        candidates.dedup_by(|a, b| a.role == b.role && a.server == b.server && a.album.is_some() && a.album == b.album);
    }
    folders
}

/// Resolve path aliases through the transport, without imposing filesystem case rules.
/// Existing ownership paths win, so removing a differently-cased track cannot re-elect art.
async fn grouped_folders(
    manifest: &mut DeviceManifest,
    io: &dyn DeviceIO,
    manager: &SyncOperationManager,
    operation: &str,
    checkpoint: &crate::device::SyncManifestSession,
    warnings: &mut Vec<String>,
) -> Result<(BTreeMap<String, Vec<Candidate>>, HashSet<String>)> {
    let mut raw = live_folders(manifest);
    let mut retained = std::collections::BTreeSet::new();
    for track in &manifest.synced_items {
        let path = normalized_device_folder(&track.local_path);
        if validate_device_relative_path(&path).is_err() {
            continue;
        }
        let mut parent = path.rsplit_once('/').map(|(parent, _)| parent);
        while let Some(folder) = parent {
            retained.insert(folder.to_owned());
            parent = folder.rsplit_once('/').map(|(parent, _)| parent);
        }
    }
    let roots = managed_roots(manifest);
    let paths: std::collections::BTreeSet<_> = raw
        .keys()
        .chain(retained.iter())
        .chain(manifest.folder_artwork_records.keys())
        .chain(roots.iter())
        .cloned()
        .collect();
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut identities = HashMap::new();
    let mut unavailable = HashSet::new();
    let mut missing = HashSet::new();
    for path in paths {
        anyhow::ensure!(!manager.is_cancelled(operation).await, "Artwork cancelled");
        let identity = match io.folder_identity(&path).await {
            Ok(Some(identity)) => format!("resolved:{identity}"),
            Ok(None) => {
                missing.insert(path.clone());
                format!("missing:{path}")
            }
            Err(_) => {
                warnings.push(format!(
                    "[Artwork] {path}: folder identity unavailable; ownership retained"
                ));
                unavailable.insert(path.clone());
                format!("unavailable:{path}")
            }
        };
        identities.insert(path.clone(), identity.clone());
        groups.entry(identity).or_default().push(path);
    }
    let mut folders: BTreeMap<String, Vec<Candidate>> = BTreeMap::new();
    let mut live = HashSet::new();
    let root_identity_unavailable = roots.iter().any(|root| unavailable.contains(root));
    for paths in groups.into_values() {
        let owned: Vec<_> = paths
            .iter()
            .filter(|path| manifest.folder_artwork_records.contains_key(*path))
            .cloned()
            .collect();
        let canonical = owned
            .iter()
            .min_by_key(|path| {
                let record = &manifest.folder_artwork_records[*path];
                (record.jpeg_hash.is_none(), record.native.is_empty(), *path)
            })
            .cloned()
            .unwrap_or_else(|| paths[0].clone());
        if paths.iter().any(|path| retained.contains(path)) {
            live.insert(canonical.clone());
        }
        for path in &paths {
            if missing.contains(path) || unavailable.contains(path) {
                continue;
            }
            if !root_identity_unavailable && let Some(candidates) = raw.remove(path) {
                folders
                    .entry(canonical.clone())
                    .or_default()
                    .extend(candidates.into_iter().filter(|candidate| eligible_folder(path, candidate.role, candidate.artist, manifest, &identities)));
            }
        }
        if owned.len() > 1 {
            let mut record = manifest.folder_artwork_records[&canonical].clone();
            for path in owned.iter().filter(|path| **path != canonical) {
                let other = manifest.folder_artwork_records.remove(path).unwrap();
                for (kind, token) in other.native {
                    if kind == "gnome"
                        && let Some(previous) = record.native.get(&kind)
                    {
                        record.native.insert(
                            kind,
                            crate::folder_icons::merge_gnome_ownership(previous, &token)?,
                        );
                    } else {
                        record.native.entry(kind).or_insert(token);
                    }
                }
                if record.source.is_none() {
                    record.source = other.source;
                }
            }
            manifest
                .folder_artwork_records
                .insert(canonical.clone(), record.clone());
            checkpoint
                .stage(0, |m| {
                    for path in &owned {
                        m.folder_artwork_records.remove(path);
                    }
                    m.folder_artwork_records.insert(canonical, record);
                })
                .await?;
        }
    }
    // Unavailable retained paths might alias an owned directory. Do not turn
    // uncertain transport evidence into deletion; retry identity resolution later.
    if unavailable.iter().any(|path| retained.contains(path)) {
        live.extend(manifest.folder_artwork_records.keys().cloned());
    }
    live.extend(
        unavailable
            .into_iter()
            .filter(|path| manifest.folder_artwork_records.contains_key(path)),
    );
    for candidates in folders.values_mut() {
        candidates
            .sort_by(|a, b| (&a.server, &a.album, &a.track).cmp(&(&b.server, &b.album, &b.track)));
        candidates.dedup_by(|a, b| {
            a.role == b.role && a.server == b.server && a.album.is_some() && a.album == b.album && a.artist == b.artist
        });
    }
    Ok((folders, live))
}

fn provider_for<'a>(
    server: Option<&str>,
    fallback: &'a Arc<dyn MediaProvider>,
    providers: &'a HashMap<String, Arc<dyn MediaProvider>>,
) -> Option<&'a Arc<dyn MediaProvider>> {
    match server {
        Some(server) => providers.get(server),
        None => Some(fallback),
    }
}

async fn resolve(
    candidate: &Candidate,
    provider: &dyn MediaProvider,
    artist_covers: &mut HashMap<(Option<String>, String), Option<String>>,
) -> Result<Option<FolderArtworkSource>> {
    if candidate.role == MediaRole::Podcast {
        let (id, cover) = if let Some(id) = &candidate.album {
            let cover = match candidate.known.as_ref().and_then(|known| known.cover_id.clone()) {
                Some(cover) => Some(cover),
                None => provider.get_podcast_show(id).await
                    .map_err(|_| anyhow::anyhow!("Podcast artwork metadata unavailable"))?.show.cover_art_id,
            };
            (id.clone(), cover)
        } else {
            let episode = provider.get_podcast_episode(&candidate.track).await
                .map_err(|_| anyhow::anyhow!("Podcast artwork metadata unavailable"))?;
            let cover = match episode.cover_art_id {
                Some(cover) => Some(cover),
                None => provider.get_podcast_show(&episode.show_id).await
                    .map_err(|_| anyhow::anyhow!("Podcast artwork metadata unavailable"))?.show.cover_art_id,
            };
            (episode.show_id, cover)
        };
        return Ok(Some(FolderArtworkSource { server_id: candidate.server.clone(), entity_id: id, artist: false, cover_id: cover }));
    }
    let mut known = candidate.known.clone();
    let mut album_id = candidate.album.clone();
    if album_id.is_none()
        || (candidate.artist && known.as_ref().and_then(|k| k.artist_id.as_ref()).is_none())
    {
        let song = provider
            .get_song(&candidate.track)
            .await
            .map_err(|_| anyhow::anyhow!("Artwork source metadata unavailable"))?;
        album_id = album_id.or(song.album_id.clone());
        known = Some(TrackArtwork::from(&song));
    }
    let (entity_id, cover_id) = if candidate.artist {
        let Some(id) = known.and_then(|k| k.artist_id) else {
            return Ok(None);
        };
        let key = (candidate.server.clone(), id.clone());
        let cover = if let Some(cover) = artist_covers.get(&key) {
            cover.clone()
        } else {
            let cover = provider
                .get_artist(&id)
                .await
                .map_err(|_| anyhow::anyhow!("Artist artwork metadata unavailable"))?
                .artist
                .cover_art_id;
            artist_covers.insert(key, cover.clone());
            cover
        };
        (id, cover)
    } else {
        let Some(id) = album_id else { return Ok(None) };
        let album = provider
            .get_album(&id)
            .await
            .map_err(|_| anyhow::anyhow!("Album artwork metadata unavailable"))?
            .album;
        (id, album.cover_art_id)
    };
    Ok(Some(FolderArtworkSource {
        server_id: candidate.server.clone(),
        entity_id,
        artist: candidate.artist,
        cover_id,
    }))
}

pub(crate) fn jpeg(bytes: &[u8]) -> Result<Vec<u8>> {
    ensure_image_size(bytes)?;
    let mut reader = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(32 * 1024 * 1024);
    reader.limits(limits);
    let decoded = reader.decode()?;
    let small = if decoded.width() > 500 || decoded.height() > 500 {
        decoded.thumbnail(500, 500)
    } else {
        decoded
    };
    let rgba = small.to_rgba8();
    let rgb = image::RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
        let p = rgba.get_pixel(x, y);
        let alpha = u32::from(p[3]);
        image::Rgb(
            [0, 1, 2].map(|i| ((u32::from(p[i]) * alpha + 255 * (255 - alpha) + 127) / 255) as u8),
        )
    });
    let mut output = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut output, 85).encode_image(&rgb)?;
    Ok(output)
}
fn ensure_image_size(bytes: &[u8]) -> Result<()> {
    anyhow::ensure!(bytes.len() <= MAX_RESPONSE, "Artwork exceeds 8 MiB");
    let (width, height) = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()?
        .into_dimensions()?;
    anyhow::ensure!(
        width > 0 && height > 0 && u64::from(width) * u64::from(height) <= 16_000_000,
        "Artwork exceeds 16 million pixels"
    );
    Ok(())
}

async fn fetch(provider: &dyn MediaProvider, cover: &str) -> Result<Vec<u8>> {
    let mut response = provider
        .fetch_cover_art(cover)
        .await
        .map_err(|_| anyhow::anyhow!("Artwork request failed"))?;
    anyhow::ensure!(
        response.status().is_success(),
        "Artwork server returned an error"
    );
    anyhow::ensure!(
        response
            .content_length()
            .is_none_or(|n| n <= MAX_RESPONSE as u64),
        "Artwork exceeds 8 MiB"
    );
    let mut data = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| anyhow::anyhow!("Artwork response interrupted"))?
    {
        anyhow::ensure!(
            data.len().saturating_add(chunk.len()) <= MAX_RESPONSE,
            "Artwork exceeds 8 MiB"
        );
        data.extend_from_slice(&chunk);
    }
    tokio::task::spawn_blocking(move || jpeg(&data))
        .await?
        .map_err(|_| anyhow::anyhow!("Invalid or oversized artwork"))
}
fn hash(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}
enum OwnedBytes {
    Missing,
    Modified,
    Unchanged(Vec<u8>),
}
async fn owned_bytes(io: &dyn DeviceIO, path: &str, expected: &str) -> Result<OwnedBytes> {
    let Some(size) = io.file_length(path).await? else {
        return Ok(OwnedBytes::Missing);
    };
    if size > MAX_RESPONSE as u64 {
        return Ok(OwnedBytes::Modified);
    }
    let bytes = io.read_file(path).await?;
    if hash(&bytes) != expected {
        return Ok(OwnedBytes::Modified);
    }
    Ok(OwnedBytes::Unchanged(bytes))
}

/// Incremental artwork state, initialized once. Folder identities and candidate
/// metadata are indexed once rather than scanning the plan or manifest per track.
pub(crate) struct Incremental {
    manifest: DeviceManifest,
    folders: BTreeMap<String, Vec<Candidate>>,
    identities: HashMap<String, String>,
    attempted: HashSet<String>,
    paths: HashMap<String, String>,
    artist_covers: HashMap<(Option<String>, String), Option<String>>,
}
impl Incremental {
    pub(crate) async fn new(
        mut manifest: DeviceManifest,
        io: &Arc<dyn DeviceIO>,
        manager: &SyncOperationManager,
        operation: &str,
        checkpoint: &crate::device::SyncManifestSession,
    ) -> (Self, Vec<String>) {
        let mut warnings = Vec::new();
        let folders = if manifest.folder_artwork.enabled {
            match grouped_folders(
                &mut manifest,
                io.as_ref(),
                manager,
                operation,
                checkpoint,
                &mut warnings,
            )
            .await
            {
                Ok((folders, _)) => folders,
                Err(_) => {
                    warnings
                        .push("[Artwork] Folder grouping interrupted; ownership retained".into());
                    BTreeMap::new()
                }
            }
        } else {
            BTreeMap::new()
        };
        let mut identities = HashMap::new();
        if manifest.folder_artwork.enabled {
            for folder in folders.keys().chain(manifest.folder_artwork_records.keys()) {
                if let Ok(Some(identity)) = io.folder_identity(folder).await {
                    identities.entry(identity).or_insert_with(|| folder.clone());
                }
            }
        }
        // Retained descendants are only needed by the final cleanup snapshot.
        manifest.synced_items.clear();
        (
            Self {
                manifest,
                folders,
                identities,
                attempted: HashSet::new(),
                paths: HashMap::new(),
                artist_covers: HashMap::new(),
            },
            warnings,
        )
    }

    async fn identity(&mut self, folder: &str, io: &dyn DeviceIO) -> Result<Option<String>> {
        if let Some(identity) = self.paths.get(folder) {
            return Ok(Some(identity.clone()));
        }
        let identity = io.folder_identity(folder).await?;
        if let Some(identity) = &identity {
            self.paths.insert(folder.to_owned(), identity.clone());
        }
        Ok(identity)
    }

    pub(crate) async fn touch(
        &mut self,
        item: &crate::device::SyncedItem,
        known: Option<TrackArtwork>,
        io: &Arc<dyn DeviceIO>,
        fallback: &Arc<dyn MediaProvider>,
        providers: &HashMap<String, Arc<dyn MediaProvider>>,
        manager: &SyncOperationManager,
        operation: &str,
        checkpoint: &crate::device::SyncManifestSession,
    ) -> Vec<String> {
        if !self.manifest.folder_artwork.enabled {
            return Vec::new();
        }
        let path = normalized_device_folder(&item.local_path);
        if validate_device_relative_path(&path).is_err() {
            return Vec::new();
        }
        let mut touched = BTreeMap::new();
        let mut warnings = Vec::new();
        for root in managed_roots(&self.manifest) {
            if self.identity(&root, io.as_ref()).await.is_err() {
                warnings.push(format!("[Artwork] {root}: root identity unavailable; ownership retained"));
                return warnings;
            }
        }
        for (folder, is_artist) in folder_targets(&path, item.media_role) {
            let mut ancestor = folder.rsplit_once('/').map(|(parent, _)| parent);
            while let Some(parent) = ancestor {
                let _ = self.identity(parent, io.as_ref()).await;
                ancestor = parent.rsplit_once('/').map(|(parent, _)| parent);
            }
            if manager.is_cancelled(operation).await {
                break;
            }
            let identity = match self.identity(folder, io.as_ref()).await {
                Ok(Some(identity)) => identity,
                Ok(None) => continue,
                Err(_) => {
                    warnings.push(format!(
                        "[Artwork] {folder}: folder identity unavailable; retry next sync"
                    ));
                    continue;
                }
            };
            if !eligible_folder(folder, item.media_role, is_artist, &self.manifest, &self.paths) {
                continue;
            }
            let canonical = self
                .identities
                .entry(identity)
                .or_insert_with(|| folder.to_owned())
                .clone();
            if !self.attempted.insert(canonical.clone()) {
                continue;
            }
            let mut candidates = self.folders.remove(&canonical).unwrap_or_default();
            // Current track metadata supersedes the pre-sync indexed candidate,
            // including album identity changes at an unchanged physical path.
            candidates.retain(|candidate| {
                candidate.server != item.server_id || candidate.track != item.jellyfin_id
            });
            candidates.push(Candidate {
                role: item.media_role,
                server: item.server_id.clone(),
                album: item.provider_album_id.clone(),
                track: item.jellyfin_id.clone(),
                artist: is_artist,
                known: known.clone(),
            });
            candidates.sort_by(|a, b| {
                (&a.server, &a.album, &a.track).cmp(&(&b.server, &b.album, &b.track))
            });
            candidates.dedup_by(|a, b| {
                a.role == b.role
                    && a.server == b.server
                    && a.album.is_some()
                    && a.album == b.album
                    && a.artist == b.artist
            });
            touched.insert(canonical, candidates);
        }
        if touched.is_empty() {
            return warnings;
        }
        warnings.extend(
            create_folders(
                &mut self.manifest,
                touched,
                &mut self.artist_covers,
                io,
                fallback,
                providers,
                manager,
                operation,
                checkpoint,
            )
            .await,
        );
        warnings
    }
}

/// Final artwork work retires obsolete ownership only; unchanged folders are not
/// backfilled. Resolve aliases again after audio deletions before pruning folders.
pub(crate) async fn cleanup(
    manifest: &mut DeviceManifest,
    io: &Arc<dyn DeviceIO>,
    manager: &SyncOperationManager,
    operation: &str,
    checkpoint: &crate::device::SyncManifestSession,
) -> Vec<String> {
    let mut progress = crate::sync_logging::Step::current("folder_artwork_cleanup");
    let mut warnings = Vec::new();
    let live = match grouped_folders(
        manifest,
        io.as_ref(),
        manager,
        operation,
        checkpoint,
        &mut warnings,
    )
    .await
    {
        Ok((_, live)) => live,
        Err(_) => {
            warnings.push("[Artwork] Folder grouping interrupted; ownership retained".into());
            return warnings;
        }
    };
    let obsolete: Vec<_> = manifest
        .folder_artwork_records
        .keys()
        .filter(|folder| !live.contains(*folder))
        .cloned()
        .collect();
    for folder in obsolete {
        if manager.is_cancelled(operation).await {
            progress.finish("cancelled", 0);
            return warnings;
        }
        crate::daemon_log!("[Artwork] {folder}: retiring owned artwork");
        let mut record = manifest.folder_artwork_records[&folder].clone();
        // Retire native state first, because Finder may create Icon\r and Windows
        // may activate folder attributes. Unsupported hosts keep evidence for later.
        for (kind, token) in record.native.clone() {
            match io.folder_icon(&folder, &kind, None, Some(&token)).await {
                Ok(Some(pending)) => {
                    record.native.insert(kind, pending);
                }
                Ok(None) => {
                    record.native.remove(&kind);
                }
                Err(_) => warnings.push(format!(
                    "[Artwork] {folder}: {kind} cleanup pending or modified custom icon preserved"
                )),
            }
        }
        if record.native.is_empty() {
            if let Some(expected) = &record.jpeg_hash {
                let path = format!("{folder}/folder.jpg");
                match owned_bytes(io.as_ref(), &path, expected).await {
                    Ok(OwnedBytes::Unchanged(_)) => match io.delete_file(&path).await {
                        Ok(()) => record.jpeg_hash = None,
                        Err(_) => {
                            warnings.push(format!("[Artwork] {folder}: artwork cleanup failed"))
                        }
                    },
                    Ok(OwnedBytes::Missing) => record.jpeg_hash = None,
                    Ok(OwnedBytes::Modified) => {
                        warnings.push(format!("[Artwork] {folder}: modified artwork preserved"));
                        record.jpeg_hash = None;
                    }
                    Err(_) => warnings.push(format!(
                        "[Artwork] {folder}: artwork cleanup inspection failed; retry next sync"
                    )),
                }
            }
        }
        if record.native.is_empty() && record.jpeg_hash.is_none() {
            manifest.folder_artwork_records.remove(&folder);
        } else {
            manifest
                .folder_artwork_records
                .insert(folder.clone(), record);
        }
        let value = manifest.folder_artwork_records.get(&folder).cloned();
        if checkpoint
            .stage(0, |m| {
                if let Some(value) = value {
                    m.folder_artwork_records.insert(folder.clone(), value);
                } else {
                    m.folder_artwork_records.remove(&folder);
                }
            })
            .await
            .is_err()
        {
            warnings.push("[Artwork] Could not checkpoint artwork cleanup".into());
            progress.finish("failed", 0);
            return warnings;
        }
    }
    progress.finish(
        if warnings.is_empty() {
            "success"
        } else {
            "warnings"
        },
        0,
    );
    warnings
}

/// Checkpoint every newly published output before track completion is reported.
async fn create_folders(
    manifest: &mut DeviceManifest,
    folders: BTreeMap<String, Vec<Candidate>>,
    artist_covers: &mut HashMap<(Option<String>, String), Option<String>>,
    io: &Arc<dyn DeviceIO>,
    fallback: &Arc<dyn MediaProvider>,
    providers: &HashMap<String, Arc<dyn MediaProvider>>,
    manager: &SyncOperationManager,
    operation: &str,
    checkpoint: &crate::device::SyncManifestSession,
) -> Vec<String> {
    let mut progress = crate::sync_logging::Step::current("folder_artwork");
    let mut warnings = Vec::new();
    let total = folders.len();
    let mut processed = 0;
    crate::daemon_log!("[Artwork] Checking {total} folders for thumbnails");
    for (index, (folder, candidates)) in folders.into_iter().enumerate() {
        if manager.is_cancelled(operation).await {
            break;
        }
        processed = index + 1;
        crate::daemon_log!("[Artwork] Folder {processed}/{total}: {folder}");
        let mut record = manifest
            .folder_artwork_records
            .get(&folder)
            .cloned()
            .unwrap_or_default();
        let result: Result<()> = async {
            let path = format!("{folder}/folder.jpg");
            let bytes = match record.jpeg_hash.as_deref() {
                Some(expected) => match owned_bytes(io.as_ref(), &path, expected).await? {
                    OwnedBytes::Unchanged(bytes) => Some(bytes), OwnedBytes::Missing => None,
                    OwnedBytes::Modified => anyhow::bail!("User-modified artwork preserved"),
                },
                None => { anyhow::ensure!(io.file_length(&path).await?.is_none(), "Existing artwork preserved"); None }
            };
            let bytes = if let Some(bytes) = bytes {
                crate::daemon_log!("[Artwork] {folder}: reusing folder.jpg");
                bytes
            } else {
                crate::daemon_log!("[Artwork] {folder}: resolving artwork source");
                let source = if let Some(source) = record.source.clone().filter(|source|source.cover_id.as_ref().is_some_and(|id|!id.is_empty())) { Some(source) } else {
                    let mut sources = Vec::new();
                    for candidate in &candidates {
                        let Some(provider) = provider_for(candidate.server.as_deref(), fallback, providers) else { continue };
                        let resolved = tokio::select! {
                            result = tokio::time::timeout(Duration::from_secs(15), resolve(candidate, provider.as_ref(), artist_covers)) => result,
                            _ = wait_for_operation_cancellation(manager, operation) => return Ok(()),
                        };
                        match resolved {
                            Ok(Ok(Some(source))) if source.cover_id.as_ref().is_some_and(|id|!id.is_empty()) => sources.push(source),
                            Ok(Ok(_)) => {},
                            _ => warnings.push(format!("[Artwork] {folder}: source metadata unavailable; considering other known sources")),
                        }
                    }
                    sources.sort_by(|a,b| (&a.server_id,&a.entity_id).cmp(&(&b.server_id,&b.entity_id)));
                    sources.into_iter().next()
                };
                let Some(source) = source else {
                    crate::daemon_log!("[Artwork] {folder}: skipped; no artwork source available");
                    return Ok(());
                };
                let Some(cover) = source.cover_id.as_deref() else { return Ok(()) };
                let provider = provider_for(source.server_id.as_deref(), fallback, providers).context("Source server unavailable")?;
                // One source winner and one image request for this physical folder.
                crate::daemon_log!("[Artwork] {folder}: fetching and resizing server image");
                let image = tokio::select! {
                    result = tokio::time::timeout(Duration::from_secs(30), fetch(provider.as_ref(), cover)) => result.context("Artwork request timed out")??,
                    _ = wait_for_operation_cancellation(manager, operation) => return Ok(()),
                };
                if manager.is_cancelled(operation).await { return Ok(()); }
                anyhow::ensure!(io.free_space().await? >= image.len() as u64, "Insufficient space for artwork");
                crate::daemon_log!("[Artwork] {folder}: writing folder.jpg ({} bytes)", image.len());
                io.publish_artwork(&path, &image).await?;
                record.jpeg_hash = Some(hash(&image)); record.source = Some(source);
                // Always checkpoint the successfully published image, even if cancelled now.
                let saved = record.clone(); let key = folder.clone();
                checkpoint.stage(0, |m|{m.folder_artwork_records.insert(key,saved);}).await?;
                crate::daemon_log!("[Artwork] {folder}: folder.jpg saved");
                image
            };
            for kind in ["windows", "macos", "kde", "gnome"] {
                if manager.is_cancelled(operation).await { continue; }
                let old = record.native.get(kind).map(String::as_str);
                crate::daemon_log!("[Artwork] {folder}: applying {kind} folder icon");
                match io.folder_icon(&folder, kind, Some(&bytes), old).await {
                    Ok(Some(token)) => {
                        record.native.insert(kind.into(),token);
                        let saved = record.clone(); let key = folder.clone();
                        checkpoint.stage(0, |m|{m.folder_artwork_records.insert(key,saved);}).await?;
                        crate::daemon_log!("[Artwork] {folder}: {kind} folder icon ready");
                    }
                    Ok(None) => crate::daemon_log!("[Artwork] {folder}: {kind} folder icon not applied"),
                    Err(error) => {
                        crate::daemon_log!("[Artwork] {folder}: {kind} folder icon failed: {error:#}");
                        warnings.push(format!("[Artwork] {folder}: {kind}: {error}"));
                    },
                }
            }
            Ok(())
        }.await;
        if let Err(error) = result {
            crate::daemon_log!("[Artwork] {folder}: failed: {error:#}; retry next sync");
            warnings.push(format!("[Artwork] {folder}: {error:#}; retry next sync"));
        }
        if record.jpeg_hash.is_some() || !record.native.is_empty() {
            manifest.folder_artwork_records.insert(folder, record);
        }
    }
    let cancelled = manager.is_cancelled(operation).await;
    crate::daemon_log!(
        "[Artwork] {}: checked {processed}/{total} folders; {} warnings",
        if cancelled { "Cancelled" } else { "Finished" },
        warnings.len()
    );
    progress.finish(
        if cancelled { "cancelled" } else if warnings.is_empty() { "success" } else { "warnings" },
        processed,
    );
    warnings
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn artwork_targets_include_books_and_podcasts_without_role_roots() {
        let root = tempfile::tempdir().unwrap();
        let (devices, _, checkpoint) = fixture(root.path()).await;
        let mut manifest = devices.get_current_device().await.unwrap();
        let mut book = manifest.synced_items[0].clone();
        book.media_role = MediaRole::Audiobook;
        book.local_path = "Books/Author/Title/part.mp3".into();
        let mut episode = book.clone();
        episode.media_role = MediaRole::Podcast;
        episode.local_path = "Podcasts/Show/episode.mp3".into();
        manifest.synced_items.extend([book, episode]);
        let folders = live_folders(&manifest);
        for folder in ["Music/Artist", "Music/Artist/Album", "Books/Author", "Books/Author/Title", "Podcasts/Show"] {
            assert!(folders.contains_key(folder), "missing {folder}");
        }
        assert!(!folders.contains_key("Books"));
        assert!(!folders.contains_key("Podcasts"));
        checkpoint.finish("test").await.unwrap();
    }

    #[tokio::test]
    async fn audiobookshelf_artwork_uses_shared_publication_reuse_and_cleanup_for_each_folder_type() {
        use crate::providers::{audiobookshelf::AudiobookshelfProvider, ProviderLibraryRole};
        for role in [MediaRole::Audiobook, MediaRole::Podcast] {
            let root = tempfile::tempdir().unwrap();
            let (devices, io, checkpoint) = fixture(root.path()).await;
            let mut manifest = devices.get_current_device().await.unwrap();
            let mut item = manifest.synced_items[0].clone();
            let mut server = mockito::Server::new_async().await;
            server.mock("POST", "/login").with_status(200)
                .with_body(r#"{"user":{"accessToken":"access-fixture","refreshToken":"refresh-fixture"}}"#).create_async().await;
            server.mock("GET", "/api/libraries").with_status(200)
                .with_body(r#"{"libraries":[]}"#).create_async().await;
            let (library_role, body, folders) = if role == MediaRole::Audiobook {
                (ProviderLibraryRole::Audiobook, serde_json::json!({"id":"item","libraryId":"library","mediaType":"book","media":{"id":"media","coverPath":"private-cover","metadata":{"title":"Title","authors":[{"id":"author","name":"Author"}]},"audioFiles":[{"ino":"file","index":1,"duration":60}]}}),
                    vec!["Content/Books/Author", "Content/Books/Author/Title"])
            } else {
                (ProviderLibraryRole::Podcast, serde_json::json!({"id":"item","libraryId":"library","mediaType":"podcast","media":{"id":"media","coverPath":"private-cover","metadata":{"title":"Show"},"episodes":[{"id":"episode","title":"Episode"}]}}),
                    vec!["Content/Podcasts/Show"])
            };
            let item_metadata = server.mock("GET", "/api/items/item").match_query(mockito::Matcher::Any)
                .match_header("authorization", "Bearer access-fixture")
                .with_status(200).with_body(body.to_string()).create_async().await;
            let author_metadata = server.mock("GET", "/api/authors/author").match_query(mockito::Matcher::Any)
                .with_status(200).with_body(serde_json::json!({"id":"author","name":"Author","imagePath":"private-portrait","libraryItems":[body]}).to_string())
                .create_async().await;
            let mut image = Cursor::new(Vec::new());
            image::RgbImage::new(700, 350).write_to(&mut image, image::ImageFormat::Png).unwrap();
            let cover = server.mock("GET", "/api/items/item/cover")
                .match_query("format=jpeg&width=500")
                .match_header("authorization", "Bearer access-fixture")
                .with_status(200).with_body(image.get_ref()).expect(1).create_async().await;
            let portrait = server.mock("GET", "/api/authors/author/image")
                .match_query("format=jpeg&width=500")
                .match_header("authorization", "Bearer access-fixture")
                .with_status(200).with_body(image.get_ref()).expect(usize::from(role == MediaRole::Audiobook)).create_async().await;
            let provider: Arc<dyn MediaProvider> = Arc::new(AudiobookshelfProvider::discover(&server.url(), "user", "password").await.unwrap()
                .provider.scope_to("library".into(), library_role).unwrap());
            let public_id = |kind: &str| format!("abs-{kind}-6c696272617279.6974656d.6d65646961");
            if role == MediaRole::Audiobook {
                let album = provider.get_album(&public_id("album")).await.unwrap();
                item.jellyfin_id = album.tracks[0].id.clone();
                item.provider_album_id = Some(album.album.id);
                manifest.audiobook_path = Some("Content/Books".into());
            } else {
                let show = provider.get_podcast_show(&public_id("show")).await.unwrap();
                item.jellyfin_id = show.episodes[0].id.clone();
                // Retained episode manifests may have no show/cover hint.
                item.provider_album_id = None;
                manifest.podcast_path = Some("Content/Podcasts".into());
            }
            item.media_role = role;
            item.local_path = format!("{}/audio.mp3", folders.last().unwrap());
            std::fs::create_dir_all(root.path().join(folders.last().unwrap())).unwrap();
            manifest.synced_items = vec![item.clone()];
            let manager = SyncOperationManager::new();
            manager.create_operation("abs-artwork".into(), 1).await;
            let plan = empty_plan(&manifest);
            let providers = HashMap::from([("source".into(), provider.clone())]);
            // Disabled and unchanged execution never fetch previews.
            manifest.folder_artwork.enabled = false;
            assert!(touch_all(&mut manifest, &plan, &io, &provider, &providers, &manager, "abs-artwork", &checkpoint).await.is_empty());
            assert!(manifest.folder_artwork_records.is_empty());
            manifest.folder_artwork.enabled = true;
            assert!(cleanup(&mut manifest, &io, &manager, "abs-artwork", &checkpoint).await.is_empty());
            assert!(manifest.folder_artwork_records.is_empty());
            for _ in 0..2 {
                let warnings = touch_all(&mut manifest, &plan, &io, &provider, &providers, &manager, "abs-artwork", &checkpoint).await;
                assert!(warnings.is_empty(), "{warnings:?}");
                assert_eq!(manifest.folder_artwork_records.len(), folders.len());
                for folder in &folders {
                    let bytes = std::fs::read(root.path().join(folder).join("folder.jpg")).unwrap();
                    assert_eq!(image::load_from_memory(&bytes).unwrap().width(), 500);
                    assert!(root.path().join(folder).join("desktop.ini").exists());
                    assert!(root.path().join(folder).join(".hifimule-folder.ico").exists());
                }
            }
            cover.assert_async().await;
            portrait.assert_async().await;
            manifest.synced_items.clear();
            assert!(cleanup(&mut manifest, &io, &manager, "abs-artwork", &checkpoint).await.is_empty());
            assert!(manifest.folder_artwork_records.is_empty());
            for folder in folders { assert!(!root.path().join(folder).join("folder.jpg").exists()); }
            item_metadata.remove_async().await;
            author_metadata.remove_async().await;
            let mut missing = body;
            missing["media"]["coverPath"] = serde_json::Value::Null;
            server.mock("GET", "/api/items/item").match_query(mockito::Matcher::Any)
                .with_status(200).with_body(missing.to_string()).create_async().await;
            server.mock("GET", "/api/authors/author").match_query(mockito::Matcher::Any)
                .with_status(200).with_body(serde_json::json!({"id":"author","name":"Author","libraryItems":[missing]}).to_string()).create_async().await;
            manifest.synced_items = vec![item];
            assert!(touch_all(&mut manifest, &plan, &io, &provider, &providers, &manager, "abs-artwork", &checkpoint).await.is_empty());
            assert!(manifest.folder_artwork_records.is_empty());
            checkpoint.finish("test").await.unwrap();
        }
    }

    #[test]
    fn artwork_physical_role_boundaries_exclude_every_managed_root_and_alias() {
        let manifest = DeviceManifest { managed_paths: vec!["Media".into(), "Media/Books/Author/Title".into()], audiobook_path: Some("Media/Books".into()), podcast_path: Some("Media/Books/Author".into()), ..Default::default() };
        let identities: HashMap<String, String> = [
            ("Media", "root"), ("Media/Books", "books"), ("Media/Books/Author", "podcasts"),
            ("Media/Books/Author/Title", "title"), ("Media/Books/Other", "other-author"),
            ("media", "root"), ("media/books", "books"), ("media/books/other", "other-author"),
            ("Outside", "outside"), ("Outside/Author", "outside-author"),
        ].into_iter().map(|(path, id)| (path.into(), id.into())).collect();
        assert!(eligible_folder("Media/Books/Other", MediaRole::Audiobook, true, &manifest, &identities));
        assert!(eligible_folder("media/books/other", MediaRole::Audiobook, true, &manifest, &identities));
        assert!(!eligible_folder("Media/Books/Author", MediaRole::Audiobook, true, &manifest, &identities));
        assert!(!eligible_folder("Media/Books/Author/Title", MediaRole::Podcast, false, &manifest, &identities));
        assert!(!eligible_folder("Media/Books/Other", MediaRole::Podcast, false, &manifest, &identities));
        assert!(!eligible_folder("Outside/Author", MediaRole::Audiobook, true, &manifest, &identities));
    }
    async fn fixture(
        root: &Path,
    ) -> (
        Arc<crate::device::DeviceManager>,
        Arc<dyn DeviceIO>,
        crate::device::SyncManifestSession,
    ) {
        let mut manifest = DeviceManifest {
            device_id: "artwork-device".into(),
            version: "1.0".into(),
            managed_paths: vec!["Music".into()],
            ..Default::default()
        };
        manifest.folder_artwork.enabled = true;
        for id in ["track", "track2"] {
            manifest.synced_items.push(serde_json::from_value(serde_json::json!({
                "providerItemId":id,"name":id,"album":"Album","artist":"Artist",
                "localPath":format!("Music/Artist/Album/{id}.mp3"),"sizeBytes":5,"syncedAt":"2026-10-21",
                "providerAlbumId":"album","serverId":"source"
            })).unwrap());
        }
        std::fs::create_dir_all(root.join("Music/Artist/Album")).unwrap();
        let io: Arc<dyn DeviceIO> = Arc::new(crate::device_io::MscBackend::new(root.to_owned()));
        crate::device::write_manifest(io.clone(), &manifest)
            .await
            .unwrap();
        let devices = Arc::new(crate::device::DeviceManager::new(Arc::new(
            crate::db::Database::memory().unwrap(),
        )));
        devices
            .handle_device_detected(root.to_owned(), manifest, io.clone())
            .await
            .unwrap();
        let checkpoint =
            crate::device::SyncManifestSession::new(devices.clone(), "artwork-device", &io, None)
                .await
                .unwrap();
        (devices, io, checkpoint)
    }
    fn empty_plan(manifest: &DeviceManifest) -> crate::sync_plan::SyncPlan {
        let delta = serde_json::from_value(
            serde_json::json!({"adds":[],"deletes":[],"idChanges":[],"unchanged":2}),
        )
        .unwrap();
        crate::sync_plan::SyncPlan::from_delta(delta, manifest).unwrap()
    }
    // Explicitly simulate updates to existing tracks for focused lifecycle tests.
    // Production creates artwork only through Incremental::touch in the audio writer.
    async fn touch_all(
        manifest: &mut DeviceManifest,
        _plan: &crate::sync_plan::SyncPlan,
        io: &Arc<dyn DeviceIO>,
        fallback: &Arc<dyn MediaProvider>,
        providers: &HashMap<String, Arc<dyn MediaProvider>>,
        manager: &SyncOperationManager,
        operation: &str,
        checkpoint: &crate::device::SyncManifestSession,
    ) -> Vec<String> {
        let (mut state, mut warnings) =
            Incremental::new(manifest.clone(), io, manager, operation, checkpoint).await;
        for item in &manifest.synced_items {
            warnings.extend(
                state
                    .touch(
                        item, None, io, fallback, providers, manager, operation, checkpoint,
                    )
                    .await,
            );
        }
        manifest.folder_artwork_records = state.manifest.folder_artwork_records;
        if !manager.is_cancelled(operation).await {
            warnings.extend(cleanup(manifest, io, manager, operation, checkpoint).await);
        }
        warnings
    }
    #[tokio::test]
    async fn current_album_identity_replaces_indexed_metadata_at_same_path() {
        let root = tempfile::tempdir().unwrap();
        let (devices, base, checkpoint) = fixture(root.path()).await;
        let io: Arc<dyn DeviceIO> = Arc::new(FaultIo {
            base,
            free: u64::MAX,
            cancel: None,
            fail_read: false.into(),
            fail_stat: false.into(),
            fold_case: false,
        });
        let mut manifest = devices.get_current_device().await.unwrap();
        manifest.synced_items.truncate(1);
        own_jpeg(&mut manifest, root.path(), "Music/Artist");
        let manager = SyncOperationManager::new();
        manager.create_operation("album-change".into(), 1).await;
        let (mut state, warnings) =
            Incremental::new(manifest.clone(), &io, &manager, "album-change", &checkpoint).await;
        assert!(warnings.is_empty());
        let mut server = mockito::Server::new_async().await;
        let old = server
            .mock("GET", "/Items/album")
            .match_query(mockito::Matcher::Any)
            .expect(0)
            .create_async()
            .await;
        let current = server
            .mock("GET", "/Items/z-new")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_body(
                r#"{"Id":"z-new","Name":"Album","Type":"MusicAlbum","ImageTags":{"Primary":"cover"}}"#,
            )
            .expect(1)
            .create_async()
            .await;
        let _children = server
            .mock("GET", "/Items")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_body(r#"{"Items":[],"TotalRecordCount":0,"StartIndex":0}"#)
            .create_async()
            .await;
        let mut jpeg = Cursor::new(Vec::new());
        image::RgbImage::new(20, 10)
            .write_to(&mut jpeg, image::ImageFormat::Jpeg)
            .unwrap();
        let cover = server
            .mock("GET", "/Items/z-new/Images/Primary")
            .with_status(200)
            .with_body(jpeg.get_ref())
            .expect(1)
            .create_async()
            .await;
        let provider: Arc<dyn MediaProvider> =
            Arc::new(crate::providers::jellyfin::JellyfinProvider::new(
                crate::api::JellyfinClient::new(),
                server.url(),
                "artwork-test-token",
                "user",
            ));
        let mut item = manifest.synced_items[0].clone();
        item.provider_album_id = Some("z-new".into());
        let warnings = state
            .touch(
                &item,
                None,
                &io,
                &provider,
                &HashMap::from([("source".into(), provider.clone())]),
                &manager,
                "album-change",
                &checkpoint,
            )
            .await;
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(
            state.manifest.folder_artwork_records["Music/Artist/Album"]
                .source
                .as_ref()
                .unwrap()
                .entity_id,
            "z-new"
        );
        old.assert_async().await;
        current.assert_async().await;
        cover.assert_async().await;
        checkpoint.finish("test").await.unwrap();
    }

    #[tokio::test]
    async fn touched_folders_require_artist_strictly_below_physical_music_root() {
        let root = tempfile::tempdir().unwrap();
        let (devices, io, checkpoint) = fixture(root.path()).await;
        let manifest = devices.get_current_device().await.unwrap();
        let manager = SyncOperationManager::new();
        manager.create_operation("root-boundary".into(), 2).await;
        let provider = inert_provider();
        let (mut state, warnings) = Incremental::new(
            manifest.clone(),
            &io,
            &manager,
            "root-boundary",
            &checkpoint,
        )
        .await;
        assert!(warnings.is_empty());
        for path in ["Music/Album/track.mp3", "Outside/Artist/Album/track.mp3"] {
            std::fs::create_dir_all(root.path().join(path).parent().unwrap()).unwrap();
            let mut item = manifest.synced_items[0].clone();
            item.local_path = path.into();
            assert!(
                state
                    .touch(
                        &item,
                        None,
                        &io,
                        &provider,
                        &HashMap::new(),
                        &manager,
                        "root-boundary",
                        &checkpoint
                    )
                    .await
                    .is_empty()
            );
        }
        assert!(state.attempted.is_empty());
        assert!(state.manifest.folder_artwork_records.is_empty());
        checkpoint.finish("test").await.unwrap();
    }

    #[tokio::test]
    async fn unchanged_sync_skips_backfill_then_updates_reuse_images_and_cleanup_owned_outputs() {
        let root = tempfile::tempdir().unwrap();
        let (devices, io, checkpoint) = fixture(root.path()).await;
        let mut server = mockito::Server::new_async().await;
        let mut image = Cursor::new(Vec::new());
        image::RgbImage::new(700, 350)
            .write_to(&mut image, image::ImageFormat::Png)
            .unwrap();
        let album=server.mock("GET","/Items/album").match_query(mockito::Matcher::Any).with_status(200).with_header("content-type","application/json").with_body(r#"{"Id":"album","Name":"Album","Type":"MusicAlbum","ImageTags":{"Primary":"cover"}}"#).create_async().await;
        let song=server.mock("GET","/Items/track").match_query(mockito::Matcher::Any).with_status(200).with_header("content-type","application/json").with_body(r#"{"Id":"track","Name":"Track","Type":"Audio","ArtistItems":[{"Id":"artist","Name":"Artist"}],"AlbumId":"album"}"#).create_async().await;
        let artist=server.mock("GET","/Items/artist").match_query(mockito::Matcher::Any).with_status(200).with_header("content-type","application/json").with_body(r#"{"Id":"artist","Name":"Artist","Type":"MusicArtist","ImageTags":{"Primary":"cover"}}"#).create_async().await;
        let _children = server
            .mock("GET", "/Items")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"Items":[],"TotalRecordCount":0,"StartIndex":0}"#)
            .expect(2)
            .create_async()
            .await;
        let cover_album = server
            .mock("GET", "/Items/album/Images/Primary")
            .match_header("authorization", "MediaBrowser Token=\"artwork-test-token\"")
            .with_status(200)
            .with_body(image.get_ref())
            .expect(1)
            .create_async()
            .await;
        let cover_artist = server
            .mock("GET", "/Items/artist/Images/Primary")
            .match_header("authorization", "MediaBrowser Token=\"artwork-test-token\"")
            .with_status(200)
            .with_body(image.get_ref())
            .expect(1)
            .create_async()
            .await;
        let provider: Arc<dyn MediaProvider> =
            Arc::new(crate::providers::jellyfin::JellyfinProvider::new(
                crate::api::JellyfinClient::new(),
                server.url(),
                "artwork-test-token",
                "user",
            ));
        let providers = HashMap::from([("source".into(), provider.clone())]);
        let manager = Arc::new(SyncOperationManager::new());
        manager.create_operation("artwork".into(), 0).await;
        let mut manifest = devices.get_current_device().await.unwrap();
        let plan = empty_plan(&manifest);
        checkpoint.finish("fixture").await.unwrap();
        let target: super::super::SyncTarget = devices
            .get_sync_target_for_device("artwork-device")
            .await
            .unwrap()
            .into();
        let (count, errors) = super::super::execute_plan_sync_with_protection(
            &plan,
            &target,
            ProviderSyncSource {
                local_encoding_servers: HashSet::new(),
                provider: provider.clone(),
                transcoding_profile: None,
                providers_by_server: providers.clone(),
            },
            manager.clone(),
            "artwork".into(),
            devices.clone(),
            None,
        )
        .await
        .unwrap();
        assert_eq!(count, 0);
        assert!(errors.is_empty(), "{errors:?}");
        let warnings = manager.get_operation("artwork").await.unwrap().warnings;
        assert!(warnings.is_empty(), "{warnings:?}");
        manifest = devices.get_current_device().await.unwrap();
        let checkpoint =
            crate::device::SyncManifestSession::new(devices.clone(), "artwork-device", &io, None)
                .await
                .unwrap();
        assert!(manifest.folder_artwork_records.is_empty());
        assert!(!root.path().join("Music/Artist/Album/folder.jpg").exists());
        // Fixture tracks are metadata only; the unchanged run prunes their empty
        // directories. A subsequent audio update recreates them before artwork.
        std::fs::create_dir_all(root.path().join("Music/Artist/Album")).unwrap();
        assert!(touch_all(&mut manifest, &plan, &io, &provider, &providers, &manager, "artwork", &checkpoint).await.is_empty());
        assert_eq!(manifest.folder_artwork_records.len(), 2);
        assert!(root.path().join("Music/Artist/Album/desktop.ini").exists());
        assert!(root.path().join("Music/Artist/Album/.hifimule-folder.ico").exists());
        assert_eq!(
            image::load_from_memory(
                &std::fs::read(root.path().join("Music/Artist/Album/folder.jpg")).unwrap()
            )
            .unwrap()
            .width(),
            500
        );
        assert!(
            touch_all(
                &mut manifest,
                &plan,
                &io,
                &provider,
                &providers,
                &manager,
                "artwork",
                &checkpoint
            )
            .await
            .is_empty()
        );
        let warnings = touch_all(
            &mut manifest,
            &plan,
            &io,
            &provider,
            &providers,
            &manager,
            "artwork",
            &checkpoint,
        )
        .await;
        assert!(warnings.is_empty(), "{warnings:?}");
        cover_album.assert_async().await;
        cover_artist.assert_async().await;
        album.assert_async().await;
        song.assert_async().await;
        artist.assert_async().await;
        // Disabling leaves existing files; removing the last track retires owned state.
        manifest.folder_artwork.enabled = false;
        assert!(
            touch_all(
                &mut manifest,
                &plan,
                &io,
                &provider,
                &providers,
                &manager,
                "artwork",
                &checkpoint
            )
            .await
            .is_empty()
        );
        std::fs::write(
            root.path().join("Music/Artist/folder.jpg"),
            b"user replacement",
        )
        .unwrap();
        manifest.synced_items.clear();
        let warnings = touch_all(
            &mut manifest,
            &plan,
            &io,
            &provider,
            &providers,
            &manager,
            "artwork",
            &checkpoint,
        )
        .await;
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("modified artwork preserved"))
        );
        assert_eq!(
            std::fs::read(root.path().join("Music/Artist/folder.jpg")).unwrap(),
            b"user replacement"
        );
        assert!(!root.path().join("Music/Artist/Album/folder.jpg").exists());
        assert!(!root.path().join("Music/Artist/Album/.directory").exists());
        assert!(manifest.folder_artwork_records.is_empty());
        checkpoint.finish("test").await.unwrap();
    }
    struct FaultIo {
        base: Arc<dyn DeviceIO>,
        free: u64,
        cancel: Option<Arc<SyncOperationManager>>,
        fail_read: std::sync::atomic::AtomicBool,
        fail_stat: std::sync::atomic::AtomicBool,
        fold_case: bool,
    }
    impl std::fmt::Debug for FaultIo {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("FaultIo")
        }
    }
    #[async_trait::async_trait]
    impl DeviceIO for FaultIo {
        async fn folder_icon(
            &self,
            p: &str,
            kind: &str,
            jpeg: Option<&[u8]>,
            owned: Option<&str>,
        ) -> Result<Option<String>> {
            // Emulate a host-local adapter retaining another host's pending evidence.
            if kind == "gnome" && owned == Some("foreign-host") {
                return Ok(owned.map(str::to_owned));
            }
            let _ = (p, jpeg);
            Ok(owned.map(str::to_owned))
        }
        async fn folder_identity(&self, p: &str) -> Result<Option<String>> {
            if self.fold_case {
                Ok(Some(format!("object:{}", p.to_ascii_lowercase())))
            } else {
                self.base.folder_identity(p).await
            }
        }
        async fn read_file(&self, p: &str) -> Result<Vec<u8>> {
            anyhow::ensure!(
                !self.fail_read.load(Ordering::Relaxed),
                "Injected read failure"
            );
            self.base.read_file(p).await
        }
        async fn write_file(&self, p: &str, b: &[u8]) -> Result<()> {
            self.base.write_file(p, b).await
        }
        async fn write_with_verify(&self, p: &str, b: &[u8]) -> Result<()> {
            self.base.write_with_verify(p, b).await
        }
        async fn delete_file(&self, p: &str) -> Result<()> {
            self.base.delete_file(p).await
        }
        async fn list_files(&self, p: &str) -> Result<Vec<crate::device_io::FileEntry>> {
            self.base.list_files(p).await
        }
        async fn file_length(&self, p: &str) -> Result<Option<u64>> {
            anyhow::ensure!(
                !self.fail_stat.load(Ordering::Relaxed),
                "Injected stat failure"
            );
            self.base.file_length(p).await
        }
        async fn free_space(&self) -> Result<u64> {
            Ok(self.free)
        }
        async fn ensure_dir(&self, p: &str) -> Result<()> {
            self.base.ensure_dir(p).await
        }
        async fn cleanup_empty_subdirs(&self, p: &str) -> Result<()> {
            self.base.cleanup_empty_subdirs(p).await
        }
        async fn publish_artwork(&self, p: &str, b: &[u8]) -> Result<()> {
            self.base.publish_artwork(p, b).await?;
            if let Some(manager) = &self.cancel {
                let _ = manager.request_cancel("artwork").await;
            }
            Ok(())
        }
    }
    #[tokio::test]
    async fn cancellation_after_publication_checkpoints_ownership_and_disk_full_is_nonfatal() {
        for cancel in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let (devices, base, checkpoint) = fixture(root.path()).await;
            let manager = Arc::new(SyncOperationManager::new());
            manager.create_operation("artwork".into(), 0).await;
            let io: Arc<dyn DeviceIO> = Arc::new(FaultIo {
                base,
                free: if cancel { u64::MAX } else { 0 },
                cancel: cancel.then(|| manager.clone()),
                fail_read: false.into(),
                fail_stat: false.into(),
                fold_case: false,
            });
            let mut server = mockito::Server::new_async().await;
            let mut image = Cursor::new(Vec::new());
            image::RgbImage::new(20, 10)
                .write_to(&mut image, image::ImageFormat::Jpeg)
                .unwrap();
            let cover = server
                .mock("GET", "/Items/cover/Images/Primary")
                .with_status(200)
                .with_body(image.get_ref())
                .expect(if cancel { 1 } else { 2 })
                .create_async()
                .await;
            let provider: Arc<dyn MediaProvider> =
                Arc::new(crate::providers::jellyfin::JellyfinProvider::new(
                    crate::api::JellyfinClient::new(),
                    server.url(),
                    "artwork-test-token",
                    "user",
                ));
            let providers = HashMap::from([("source".into(), provider.clone())]);
            let mut manifest = devices.get_current_device().await.unwrap();
                for folder in ["Music/Artist", "Music/Artist/Album"] {
                manifest.folder_artwork_records.insert(
                    folder.into(),
                    crate::device::FolderArtworkRecord {
                        source: Some(FolderArtworkSource {
                            server_id: Some("source".into()),
                            entity_id: "entity".into(),
                            artist: folder == "Music/Artist",
                            cover_id: Some("cover".into()),
                        }),
                        ..Default::default()
                    },
                );
            }
            let plan = empty_plan(&manifest);
            let warnings = touch_all(
                &mut manifest,
                &plan,
                &io,
                &provider,
                &providers,
                &manager,
                "artwork",
                &checkpoint,
            )
            .await;
            if cancel {
                assert!(warnings.is_empty(), "{warnings:?}");
                let saved = devices.get_current_device().await.unwrap();
                assert!(
                    saved.folder_artwork_records["Music/Artist"]
                        .jpeg_hash
                        .is_some()
                );
                assert!(root.path().join("Music/Artist/folder.jpg").exists());
                assert!(!root.path().join("Music/Artist/Album/folder.jpg").exists());
            } else {
                assert_eq!(warnings.len(), 2);
                assert!(!root.path().join("Music/Artist/folder.jpg").exists());
                assert!(!manager.is_cancelled("artwork").await);
            }
            cover.assert_async().await;
            checkpoint.finish("test").await.unwrap();
        }
    }
    fn inert_provider() -> Arc<dyn MediaProvider> {
        Arc::new(crate::providers::jellyfin::JellyfinProvider::new(
            crate::api::JellyfinClient::new(),
            "http://127.0.0.1:1",
            "artwork-test-token",
            "user",
        ))
    }
    fn own_jpeg(manifest: &mut DeviceManifest, root: &Path, folder: &str) {
        std::fs::create_dir_all(root.join(folder)).unwrap();
        std::fs::write(root.join(folder).join("folder.jpg"), b"owned image").unwrap();
        manifest.folder_artwork_records.insert(
            folder.into(),
            crate::device::FolderArtworkRecord {
                jpeg_hash: Some(hash(b"owned image")),
                source: Some(FolderArtworkSource {
                    server_id: Some("winner".into()),
                    entity_id: "stable-album".into(),
                    artist: false,
                    cover_id: Some("cover".into()),
                }),
                ..Default::default()
            },
        );
    }
    #[tokio::test]
    async fn cleanup_retries_transient_stat_and_read_errors_without_losing_ownership() {
        for stat_failure in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let (devices, base, checkpoint) = fixture(root.path()).await;
            let fault = Arc::new(FaultIo {
                base,
                free: u64::MAX,
                cancel: None,
                fail_read: (!stat_failure).into(),
                fail_stat: stat_failure.into(),
                fold_case: false,
            });
            let io: Arc<dyn DeviceIO> = fault.clone();
            let manager = SyncOperationManager::new();
            manager.create_operation("retry".into(), 0).await;
            let mut manifest = devices.get_current_device().await.unwrap();
            manifest.folder_artwork.enabled = false;
            manifest.synced_items.clear();
            own_jpeg(&mut manifest, root.path(), "Music/Artist/Album");
            let plan = empty_plan(&manifest);
            let provider = inert_provider();
            let warnings = touch_all(
                &mut manifest,
                &plan,
                &io,
                &provider,
                &HashMap::new(),
                &manager,
                "retry",
                &checkpoint,
            )
            .await;
            assert!(warnings.iter().any(|w| w.contains("inspection failed")));
            assert!(
                manifest.folder_artwork_records["Music/Artist/Album"]
                    .jpeg_hash
                    .is_some()
            );
            fault.fail_read.store(false, Ordering::Relaxed);
            fault.fail_stat.store(false, Ordering::Relaxed);
            assert!(
                touch_all(
                    &mut manifest,
                    &plan,
                    &io,
                    &provider,
                    &HashMap::new(),
                    &manager,
                    "retry",
                    &checkpoint
                )
                .await
                .is_empty()
            );
            assert!(manifest.folder_artwork_records.is_empty());
            assert!(!root.path().join("Music/Artist/Album/folder.jpg").exists());
            checkpoint.finish("test").await.unwrap();
        }
    }
    #[tokio::test]
    async fn retained_tracks_outside_new_root_protect_artwork_until_final_descendant_is_removed() {
        let root = tempfile::tempdir().unwrap();
        let (devices, io, checkpoint) = fixture(root.path()).await;
        let mut manifest = devices.get_current_device().await.unwrap();
        manifest.folder_artwork.enabled = false;
        own_jpeg(&mut manifest, root.path(), "Music/Artist/Album");
        manifest.managed_paths = vec!["NewMusic".into()];
        let manager = SyncOperationManager::new();
        manager.create_operation("old-root".into(), 0).await;
        let provider = inert_provider();
        let plan = empty_plan(&manifest);
        assert!(
            touch_all(
                &mut manifest,
                &plan,
                &io,
                &provider,
                &HashMap::new(),
                &manager,
                "old-root",
                &checkpoint
            )
            .await
            .is_empty()
        );
        assert!(
            manifest
                .folder_artwork_records
                .contains_key("Music/Artist/Album")
        );
        assert!(root.path().join("Music/Artist/Album/folder.jpg").exists());
        manifest.synced_items.clear();
        assert!(
            touch_all(
                &mut manifest,
                &plan,
                &io,
                &provider,
                &HashMap::new(),
                &manager,
                "old-root",
                &checkpoint
            )
            .await
            .is_empty()
        );
        assert!(manifest.folder_artwork_records.is_empty());
        checkpoint.finish("test").await.unwrap();
    }
    #[tokio::test]
    async fn insensitive_object_aliases_reuse_stable_record_when_original_alias_track_disappears() {
        let root = tempfile::tempdir().unwrap();
        let (devices, base, checkpoint) = fixture(root.path()).await;
        let io: Arc<dyn DeviceIO> = Arc::new(FaultIo {
            base,
            free: u64::MAX,
            cancel: None,
            fail_read: false.into(),
            fail_stat: false.into(),
            fold_case: true,
        });
        let mut manifest = devices.get_current_device().await.unwrap();
        own_jpeg(&mut manifest, root.path(), "Music/Artist");
        own_jpeg(&mut manifest, root.path(), "Music/Artist/Album");
        manifest.synced_items[1].local_path = "music/artist/album/track2.mp3".into();
        let manager = SyncOperationManager::new();
        manager.create_operation("alias".into(), 0).await;
        let provider = inert_provider();
        let plan = empty_plan(&manifest);
        assert!(
            touch_all(
                &mut manifest,
                &plan,
                &io,
                &provider,
                &HashMap::new(),
                &manager,
                "alias",
                &checkpoint
            )
            .await
            .is_empty()
        );
        manifest.synced_items.remove(0);
        assert!(
            touch_all(
                &mut manifest,
                &plan,
                &io,
                &provider,
                &HashMap::new(),
                &manager,
                "alias",
                &checkpoint
            )
            .await
            .is_empty()
        );
        assert_eq!(manifest.folder_artwork_records.len(), 2);
        assert_eq!(
            manifest.folder_artwork_records["Music/Artist/Album"]
                .source
                .as_ref()
                .unwrap()
                .entity_id,
            "stable-album"
        );
        assert_eq!(
            std::fs::read(root.path().join("Music/Artist/Album/folder.jpg")).unwrap(),
            b"owned image"
        );
        checkpoint.finish("test").await.unwrap();
    }
    #[tokio::test]
    async fn cleanup_keeps_pending_foreign_host_native_evidence_and_its_jpeg() {
        let root = tempfile::tempdir().unwrap();
        let (devices, base, checkpoint) = fixture(root.path()).await;
        let io: Arc<dyn DeviceIO> = Arc::new(FaultIo {
            base,
            free: u64::MAX,
            cancel: None,
            fail_read: false.into(),
            fail_stat: false.into(),
            fold_case: false,
        });
        let mut manifest = devices.get_current_device().await.unwrap();
        manifest.folder_artwork.enabled = false;
        manifest.synced_items.clear();
        own_jpeg(&mut manifest, root.path(), "Music/Artist/Album");
        manifest
            .folder_artwork_records
            .get_mut("Music/Artist/Album")
            .unwrap()
            .native
            .insert("gnome".into(), "foreign-host".into());
        let manager = SyncOperationManager::new();
        manager.create_operation("foreign".into(), 0).await;
        let provider = inert_provider();
        let plan = empty_plan(&manifest);
        assert!(
            touch_all(
                &mut manifest,
                &plan,
                &io,
                &provider,
                &HashMap::new(),
                &manager,
                "foreign",
                &checkpoint
            )
            .await
            .is_empty()
        );
        assert_eq!(
            manifest.folder_artwork_records["Music/Artist/Album"].native["gnome"],
            "foreign-host"
        );
        assert!(root.path().join("Music/Artist/Album/folder.jpg").exists());
        checkpoint.finish("test").await.unwrap();
    }
    #[tokio::test]
    async fn source_election_ignores_missing_cover_and_failed_losing_candidate() {
        let root = tempfile::tempdir().unwrap();
        let (devices, io, checkpoint) = fixture(root.path()).await;
        let mut manifest = devices.get_current_device().await.unwrap();
        let template = manifest.synced_items[0].clone();
        manifest.synced_items.clear();
        for album in ["a-no-cover", "b-failed", "z-healthy"] {
            let mut track = template.clone();
            track.jellyfin_id = album.into();
            track.provider_album_id = Some(album.into());
            manifest.synced_items.push(track);
        }
        let mut server = mockito::Server::new_async().await;
        let _no_cover = server
            .mock("GET", "/Items/a-no-cover")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_body(r#"{"Id":"a-no-cover","Name":"Album","Type":"MusicAlbum"}"#)
            .create_async()
            .await;
        let _bad = server
            .mock("GET", "/Items/b-failed")
            .match_query(mockito::Matcher::Any)
            .with_status(404)
            .create_async()
            .await;
        let _good=server.mock("GET","/Items/z-healthy").match_query(mockito::Matcher::Any).with_status(200).with_body(r#"{"Id":"z-healthy","Name":"Album","Type":"MusicAlbum","ImageTags":{"Primary":"cover"}}"#).create_async().await;
        let _children = server
            .mock("GET", "/Items")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_body(r#"{"Items":[],"TotalRecordCount":0,"StartIndex":0}"#)
            .create_async()
            .await;
        let mut image = Cursor::new(Vec::new());
        image::RgbImage::new(20, 10)
            .write_to(&mut image, image::ImageFormat::Jpeg)
            .unwrap();
        let cover = server
            .mock("GET", "/Items/z-healthy/Images/Primary")
            .with_status(200)
            .with_body(image.get_ref())
            .expect(1)
            .create_async()
            .await;
        let provider: Arc<dyn MediaProvider> =
            Arc::new(crate::providers::jellyfin::JellyfinProvider::new(
                crate::api::JellyfinClient::new(),
                server.url(),
                "artwork-test-token",
                "user",
            ));
        let providers = HashMap::from([("source".into(), provider.clone())]);
        let manager = SyncOperationManager::new();
        manager.create_operation("election".into(), 0).await;
        let plan = empty_plan(&manifest);
        let warnings = touch_all(
            &mut manifest,
            &plan,
            &io,
            &provider,
            &providers,
            &manager,
            "election",
            &checkpoint,
        )
        .await;
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("considering other known sources"))
        );
        assert_eq!(
            manifest.folder_artwork_records["Music/Artist/Album"]
                .source
                .as_ref()
                .unwrap()
                .entity_id,
            "z-healthy"
        );
        cover.assert_async().await;
        checkpoint.finish("test").await.unwrap();
    }
    #[tokio::test]
    async fn unmanaged_jpeg_and_missing_tagged_provider_are_preserved() {
        let root = tempfile::tempdir().unwrap();
        let (devices, io, checkpoint) = fixture(root.path()).await;
        std::fs::write(root.path().join("Music/Artist/Album/folder.jpg"), b"user").unwrap();
        let provider: Arc<dyn MediaProvider> =
            Arc::new(crate::providers::jellyfin::JellyfinProvider::new(
                crate::api::JellyfinClient::new(),
                "http://127.0.0.1:1",
                "artwork-test-token",
                "user",
            ));
        let manager = SyncOperationManager::new();
        manager.create_operation("artwork".into(), 0).await;
        let mut manifest = devices.get_current_device().await.unwrap();
        let plan = empty_plan(&manifest);
        let warnings = touch_all(
            &mut manifest,
            &plan,
            &io,
            &provider,
            &HashMap::new(),
            &manager,
            "artwork",
            &checkpoint,
        )
        .await;
        assert!(!warnings.is_empty());
        assert!(manifest.folder_artwork_records.is_empty());
        assert_eq!(
            std::fs::read(root.path().join("Music/Artist/Album/folder.jpg")).unwrap(),
            b"user"
        );
        checkpoint.finish("test").await.unwrap();
    }
    #[test]
    fn jpeg_bounds_no_upscale_alpha_and_invalid_input() {
        let rgba = image::RgbaImage::from_pixel(1200, 600, image::Rgba([0, 0, 0, 0]));
        let mut png = Cursor::new(Vec::new());
        rgba.write_to(&mut png, image::ImageFormat::Png).unwrap();
        let bytes = jpeg(png.get_ref()).unwrap();
        let output = image::load_from_memory(&bytes).unwrap().to_rgb8();
        assert_eq!(output.dimensions(), (500, 250));
        assert!(output.get_pixel(0, 0)[0] > 250);
        let mut png = Cursor::new(Vec::new());
        image::RgbImage::new(20, 10)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        assert_eq!(
            image::load_from_memory(&jpeg(png.get_ref()).unwrap())
                .unwrap()
                .width(),
            20
        );
        assert!(jpeg(b"invalid").is_err());
        assert!(jpeg(&vec![0; MAX_RESPONSE + 1]).is_err());
    }
}
