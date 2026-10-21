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
    server: Option<String>,
    album: Option<String>,
    track: String,
    artist: bool,
    known: Option<TrackArtwork>,
}

fn live_folders(manifest: &DeviceManifest) -> BTreeMap<String, Vec<Candidate>> {
    let mut folders: BTreeMap<String, Vec<Candidate>> = BTreeMap::new();
    for track in manifest
        .synced_items
        .iter()
        .filter(|t| t.media_role == MediaRole::Music)
    {
        let path = normalized_device_folder(&track.local_path);
        if validate_device_relative_path(&path).is_err() {
            continue;
        }
        // Physical root membership is checked after the transport resolves aliases.
        if path.split('/').count() < 3 {
            continue;
        }
        let Some((album, _)) = path.rsplit_once('/') else {
            continue;
        };
        let Some((artist, _)) = album.rsplit_once('/') else {
            continue;
        };
        for (folder, is_artist) in [(album, false), (artist, true)] {
            folders
                .entry(folder.to_owned())
                .or_default()
                .push(Candidate {
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
        candidates.dedup_by(|a, b| a.server == b.server && a.album.is_some() && a.album == b.album);
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
    let root = manifest
        .media_path(MediaRole::Music)
        .map(normalized_device_folder)
        .filter(|root| !root.is_empty());
    let paths: std::collections::BTreeSet<_> = raw
        .keys()
        .chain(retained.iter())
        .chain(manifest.folder_artwork_records.keys())
        .chain(root.iter())
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
            let mut ancestor = path.rsplit_once('/').map(|(parent, _)| parent);
            let mut current_root = false;
            while let Some(parent) = ancestor {
                if root
                    .as_ref()
                    .and_then(|root| identities.get(root))
                    .is_some_and(|root_id| identities.get(parent) == Some(root_id))
                {
                    current_root = true;
                    break;
                }
                ancestor = parent.rsplit_once('/').map(|(parent, _)| parent);
            }
            if current_root && let Some(candidates) = raw.remove(path) {
                folders
                    .entry(canonical.clone())
                    .or_default()
                    .extend(candidates);
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
            a.server == b.server && a.album.is_some() && a.album == b.album && a.artist == b.artist
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

/// Metadata ownership is staged immediately after each folder, including cancellation
/// arriving after publication. Audio track checkpoints are not rewritten by artwork.
pub(crate) async fn run(
    manifest: &mut DeviceManifest,
    plan: &crate::sync_plan::SyncPlan,
    io: &Arc<dyn DeviceIO>,
    fallback: &Arc<dyn MediaProvider>,
    providers: &HashMap<String, Arc<dyn MediaProvider>>,
    manager: &SyncOperationManager,
    operation: &str,
    checkpoint: &crate::device::SyncManifestSession,
) -> Vec<String> {
    let mut warnings = Vec::new();
    let mut artist_covers = HashMap::new();
    let (mut folders, live) = match grouped_folders(
        manifest,
        io.as_ref(),
        manager,
        operation,
        checkpoint,
        &mut warnings,
    )
    .await
    {
        Ok(grouped) => grouped,
        Err(_) => {
            warnings.push("[Artwork] Folder grouping interrupted; ownership retained".into());
            return warnings;
        }
    };
    if manifest.folder_artwork.enabled {
        // Additions preserve known identities through the disk-backed plan; retained
        // legacy tracks resolve by provider ID only when their folder needs artwork.
        let mut known_by_id = HashMap::new();
        if let Ok(adds) = plan.adds.iter() {
            for add in adds.flatten() {
                if let Some(known) = add.artwork {
                    known_by_id.insert((add.server_id, add.jellyfin_id), known);
                }
            }
        }
        for values in folders.values_mut() {
            for candidate in values {
                candidate.known = known_by_id
                    .get(&(candidate.server.clone(), candidate.track.clone()))
                    .cloned();
            }
        }
    }
    let obsolete: Vec<_> = manifest
        .folder_artwork_records
        .keys()
        .filter(|folder| !live.contains(*folder))
        .cloned()
        .collect();
    for folder in obsolete {
        if manager.is_cancelled(operation).await {
            return warnings;
        }
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
            return warnings;
        }
    }
    if !manifest.folder_artwork.enabled {
        return warnings;
    }
    for (folder, candidates) in folders {
        if manager.is_cancelled(operation).await {
            break;
        }
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
            let bytes = if let Some(bytes) = bytes { bytes } else {
                let source = if let Some(source) = record.source.clone().filter(|source|source.cover_id.as_ref().is_some_and(|id|!id.is_empty())) { Some(source) } else {
                    let mut sources = Vec::new();
                    for candidate in &candidates {
                        let Some(provider) = provider_for(candidate.server.as_deref(), fallback, providers) else { continue };
                        let resolved = tokio::select! {
                            result = tokio::time::timeout(Duration::from_secs(15), resolve(candidate, provider.as_ref(), &mut artist_covers)) => result,
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
                let Some(source) = source else { return Ok(()) };
                let Some(cover) = source.cover_id.as_deref() else { return Ok(()) };
                let provider = provider_for(source.server_id.as_deref(), fallback, providers).context("Source server unavailable")?;
                // One source winner and one image request for this physical folder.
                let image = tokio::select! {
                    result = tokio::time::timeout(Duration::from_secs(30), fetch(provider.as_ref(), cover)) => result.context("Artwork request timed out")??,
                    _ = wait_for_operation_cancellation(manager, operation) => return Ok(()),
                };
                if manager.is_cancelled(operation).await { return Ok(()); }
                anyhow::ensure!(io.free_space().await? >= image.len() as u64, "Insufficient space for artwork");
                io.publish_artwork(&path, &image).await?;
                record.jpeg_hash = Some(hash(&image)); record.source = Some(source);
                // Always checkpoint the successfully published image, even if cancelled now.
                let saved = record.clone(); let key = folder.clone();
                checkpoint.stage(0, |m|{m.folder_artwork_records.insert(key,saved);}).await?;
                image
            };
            for (kind, enabled) in [("windows",manifest.folder_artwork.windows),("macos",manifest.folder_artwork.macos),("kde",manifest.folder_artwork.kde),("gnome",manifest.folder_artwork.gnome)] {
                if !enabled || manager.is_cancelled(operation).await { continue; }
                let old = record.native.get(kind).map(String::as_str);
                match io.folder_icon(&folder, kind, Some(&bytes), old).await {
                    Ok(Some(token)) => {
                        record.native.insert(kind.into(),token);
                        let saved = record.clone(); let key = folder.clone();
                        checkpoint.stage(0, |m|{m.folder_artwork_records.insert(key,saved);}).await?;
                    }
                    Ok(None) => {},
                    Err(error) => warnings.push(format!("[Artwork] {folder}: {kind}: {error}")),
                }
            }
            Ok(())
        }.await;
        if let Err(error) = result {
            warnings.push(format!("[Artwork] {folder}: {error}; retry next sync"));
        }
        if record.jpeg_hash.is_some() || !record.native.is_empty() {
            manifest.folder_artwork_records.insert(folder, record);
        }
    }
    warnings
}

#[cfg(test)]
mod tests {
    use super::*;
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
        manifest.folder_artwork.kde = true;
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
    #[tokio::test]
    async fn retained_backfill_reuses_images_and_native_options_then_cleans_only_owned_outputs() {
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
        assert_eq!(manifest.folder_artwork_records.len(), 2);
        assert_eq!(
            image::load_from_memory(
                &std::fs::read(root.path().join("Music/Artist/Album/folder.jpg")).unwrap()
            )
            .unwrap()
            .width(),
            500
        );
        assert!(
            run(
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
        manifest.folder_artwork.windows = true;
        let warnings = run(
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
        #[cfg(not(windows))]
        assert_eq!(warnings.len(), 2);
        #[cfg(windows)]
        assert!(warnings.is_empty());
        cover_album.assert_async().await;
        cover_artist.assert_async().await;
        album.assert_async().await;
        song.assert_async().await;
        artist.assert_async().await;
        // Disabling leaves existing files; removing the last track retires owned state.
        manifest.folder_artwork.enabled = false;
        assert!(
            run(
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
        let warnings = run(
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
            self.base.folder_icon(p, kind, jpeg, owned).await
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
            manifest.folder_artwork.kde = false;
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
            let warnings = run(
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
            let warnings = run(
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
                run(
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
            run(
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
            run(
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
        manifest.folder_artwork.kde = false;
        own_jpeg(&mut manifest, root.path(), "Music/Artist");
        own_jpeg(&mut manifest, root.path(), "Music/Artist/Album");
        manifest.synced_items[1].local_path = "music/artist/album/track2.mp3".into();
        let manager = SyncOperationManager::new();
        manager.create_operation("alias".into(), 0).await;
        let provider = inert_provider();
        let plan = empty_plan(&manifest);
        assert!(
            run(
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
            run(
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
            run(
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
        manifest.folder_artwork.kde = false;
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
        let warnings = run(
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
        let warnings = run(
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
