//! Storage is independent of the RPC/manifest schema: one checksummed Zstd-9
//! frame containing versioned JSON with named, typed track columns.
use super::{DeviceManifest, MediaRole, SyncedItem};
use anyhow::{Context, Result, bail, ensure};
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

pub(super) const FILE_NAME: &str = ".hifimule.json.zst";
pub(super) const LEGACY_FILE_NAME: &str = ".hifimule.json";
pub(super) const MAX_DECODED_BYTES: usize = 512 * 1024 * 1024;
const MAGIC: &[u8] = &[0x28, 0xb5, 0x2f, 0xfd];
const FORMAT: &str = "hifimule-column-json";
const STORAGE_VERSION: u32 = 1;

struct Column<'a, T> {
    items: &'a [SyncedItem],
    field: fn(&SyncedItem) -> &T,
}
impl<T: Serialize> Serialize for Column<'_, T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.items.len()))?;
        for item in self.items {
            sequence.serialize_element((self.field)(item))?;
        }
        sequence.end()
    }
}

// Keep serialization and reconstruction paired. Missing/unknown columns are
// rejected, including empty manifests; a future change requires a storage version.
macro_rules! columns {
    ($($field:ident: $ty:ty => $name:literal),+ $(,)?) => {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Columns { $(#[serde(rename = $name)] $field: Vec<$ty>),+ }
        struct ColumnView<'a>(&'a [SyncedItem]);
        impl Serialize for ColumnView<'_> {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                let mut map = serializer.serialize_map(None)?;
                $(map.serialize_entry($name, &Column { items: self.0, field: |item| &item.$field })?;)+
                map.end()
            }
        }
        impl Columns {
            fn into_items(self, count: usize) -> Result<Vec<SyncedItem>> {
                $(ensure!(self.$field.len() == count, "Manifest column {} count mismatch", $name);)+
                $(let mut $field = self.$field.into_iter();)+
                Ok((0..count).map(|_| SyncedItem { $($field: $field.next().expect("validated column length")),+ }).collect())
            }
        }
    }
}
columns! {
    is_auto_fill: bool => "isAutoFill",
    media_role: MediaRole => "mediaRole",
    jellyfin_id: String => "providerItemId",
    name: String => "name",
    album: Option<String> => "album",
    artist: Option<String> => "artist",
    local_path: String => "localPath",
    size_bytes: u64 => "sizeBytes",
    synced_at: String => "syncedAt",
    original_name: Option<String> => "originalName",
    etag: Option<String> => "etag",
    provider_album_id: Option<String> => "providerAlbumId",
    provider_content_type: Option<String> => "providerContentType",
    provider_suffix: Option<String> => "providerSuffix",
    original_bitrate: Option<u32> => "originalBitrate",
    original_container: Option<String> => "originalContainer",
    track_number: Option<u32> => "trackNumber",
    server_id: Option<String> => "serverId",
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StoredView<'a> {
    format: &'static str,
    storage_version: u32,
    manifest: std::collections::BTreeMap<String, Box<serde_json::value::RawValue>>,
    synced_items: RowsView<'a>,
}
#[derive(Serialize)]
struct RowsView<'a> {
    count: usize,
    columns: ColumnView<'a>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Stored {
    format: String,
    storage_version: u32,
    manifest: DeviceManifest,
    synced_items: Rows,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Rows {
    count: usize,
    columns: Columns,
}

/// Snapshot ownership keeps CPU-intensive serialization/compression off Tokio
/// workers; callers release state guards while retaining the commit lock.
pub(super) async fn encode_async(manifest: DeviceManifest) -> Result<Vec<u8>> {
    tokio::task::spawn_blocking(move || encode(&manifest)).await?
}
pub(super) async fn decode_async(bytes: Vec<u8>, compressed: bool) -> Result<DeviceManifest> {
    tokio::task::spawn_blocking(move || decode(&bytes, compressed)).await?
}

pub(super) fn encode(manifest: &DeviceManifest) -> Result<Vec<u8>> {
    encode_with_compressed_limit(manifest, MAX_DECODED_BYTES)
}
fn encode_with_compressed_limit(manifest: &DeviceManifest, limit: usize) -> Result<Vec<u8>> {
    // Metadata retains ordinary serialization's numeric tokens (including f32
    // settings) through RawValue. Track records are streamed from typed fields,
    // never cloned into a second object graph or column vectors.
    // Exhaustive destructuring makes adding a model field a compile-time prompt
    // to preserve it in storage, instead of silently omitting future metadata.
    let DeviceManifest {
        device_id,
        name,
        icon,
        version,
        managed_paths,
        audiobook_path,
        podcast_path,
        playlist_path,
        synced_items: _,
        dirty,
        pending_item_ids,
        basket_items,
        auto_sync_on_connect,
        reconcile_before_sync,
        last_sync_at,
        auto_fill,
        transcoding_profile_id,
        last_synced_transcoding_profile_id,
        transcoding_profile_dirty,
        playlists,
        storage_id,
        folder_ids,
    } = manifest;
    let metadata = DeviceManifest {
        device_id: device_id.clone(),
        name: name.clone(),
        icon: icon.clone(),
        version: version.clone(),
        managed_paths: managed_paths.clone(),
        audiobook_path: audiobook_path.clone(),
        podcast_path: podcast_path.clone(),
        playlist_path: playlist_path.clone(),
        synced_items: Vec::new(),
        dirty: *dirty,
        pending_item_ids: pending_item_ids.clone(),
        basket_items: basket_items.clone(),
        auto_sync_on_connect: *auto_sync_on_connect,
        reconcile_before_sync: *reconcile_before_sync,
        last_sync_at: *last_sync_at,
        auto_fill: auto_fill.clone(),
        transcoding_profile_id: transcoding_profile_id.clone(),
        last_synced_transcoding_profile_id: last_synced_transcoding_profile_id.clone(),
        transcoding_profile_dirty: *transcoding_profile_dirty,
        playlists: playlists.clone(),
        storage_id: storage_id.clone(),
        folder_ids: folder_ids.clone(),
    };
    let mut metadata: std::collections::BTreeMap<String, Box<serde_json::value::RawValue>> =
        serde_json::from_slice(&serde_json::to_vec(&metadata)?)?;
    metadata.remove("synced_items");
    let view = StoredView {
        format: FORMAT,
        storage_version: STORAGE_VERSION,
        manifest: metadata,
        synced_items: RowsView {
            count: manifest.synced_items.len(),
            columns: ColumnView(&manifest.synced_items),
        },
    };
    let mut encoder = zstd::stream::write::Encoder::new(Vec::new(), 9)?;
    encoder.include_checksum(true)?;
    // The writer enforces the same decoded ceiling before publishing a frame.
    struct LimitedWriter<'a, W> {
        inner: &'a mut W,
        written: usize,
    }
    impl<W: Write> Write for LimitedWriter<'_, W> {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if self.written.saturating_add(bytes.len()) > MAX_DECODED_BYTES {
                return Err(std::io::Error::other("Manifest JSON exceeds 512 MiB limit"));
            }
            let count = self.inner.write(bytes)?;
            self.written += count;
            Ok(count)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            self.inner.flush()
        }
    }
    let mut writer = std::io::BufWriter::with_capacity(
        128 * 1024,
        LimitedWriter {
            inner: &mut encoder,
            written: 0,
        },
    );
    serde_json::to_writer(&mut writer, &view)?;
    writer.flush()?;
    drop(writer);
    encoder.flush()?;
    let bytes = encoder.finish()?;
    ensure!(
        bytes.len() <= limit,
        "Compressed manifest exceeds {} byte input limit",
        limit
    );
    Ok(bytes)
}

pub(super) fn decode(bytes: &[u8], compressed: bool) -> Result<DeviceManifest> {
    decode_with_limit(bytes, compressed, MAX_DECODED_BYTES)
}
fn decode_with_limit(bytes: &[u8], compressed: bool, limit: usize) -> Result<DeviceManifest> {
    ensure!(
        bytes.len() <= MAX_DECODED_BYTES,
        "Manifest input exceeds 512 MiB limit"
    );
    if !compressed {
        ensure!(
            !bytes.starts_with(MAGIC),
            "Compressed manifest requires .json.zst filename"
        );
        return serde_json::from_slice(bytes).context("Invalid legacy manifest JSON");
    }
    ensure!(bytes.starts_with(MAGIC), "Invalid Zstd manifest magic");
    ensure!(
        bytes.get(4).is_some_and(|descriptor| descriptor & 4 != 0),
        "Manifest frame checksum is required"
    );
    let frame_len = zstd::zstd_safe::find_frame_compressed_size(bytes).map_err(|code| {
        anyhow::anyhow!(
            "Invalid Zstd frame: {}",
            zstd::zstd_safe::get_error_name(code)
        )
    })?;
    ensure!(
        frame_len == bytes.len(),
        "Manifest must contain exactly one Zstd frame"
    );
    let mut decoder = zstd::stream::read::Decoder::new(bytes)?;
    decoder.window_log_max(29)?;
    let mut json = Vec::new();
    decoder
        .take(limit as u64 + 1)
        .read_to_end(&mut json)
        .context("Invalid compressed manifest or checksum")?;
    ensure!(
        json.len() <= limit,
        "Decoded manifest exceeds {} byte limit",
        limit
    );
    let mut stored: Stored =
        serde_json::from_slice(&json).context("Invalid manifest storage JSON")?;
    if stored.format != FORMAT || stored.storage_version != STORAGE_VERSION {
        bail!("Unsupported manifest storage format/version");
    }
    ensure!(
        stored.manifest.synced_items.is_empty(),
        "Manifest metadata cannot contain track records"
    );
    stored.manifest.synced_items = stored
        .synced_items
        .columns
        .into_items(stored.synced_items.count)?;
    Ok(stored.manifest)
}

#[cfg(test)]
#[test]
fn encoding_accepts_exact_frame_limit_and_rejects_one_byte_over() {
    let manifest = DeviceManifest::default();
    let size = encode(&manifest).unwrap().len();
    assert!(encode_with_compressed_limit(&manifest, size).is_ok());
    assert!(encode_with_compressed_limit(&manifest, size - 1).is_err());
}

#[cfg(test)]
#[test]
fn bounded_decoding_accepts_exact_limit_and_rejects_one_byte_over() {
    let bytes = encode(&DeviceManifest::default()).unwrap();
    let size = zstd::stream::decode_all(bytes.as_slice()).unwrap().len();
    assert!(decode_with_limit(&bytes, true, size).is_ok());
    assert!(
        decode_with_limit(&bytes, true, size - 1)
            .unwrap_err()
            .to_string()
            .contains("limit")
    );
}

#[cfg(test)]
pub(crate) fn decode_test_manifest<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    let manifest = decode(bytes, bytes.starts_with(MAGIC))?;
    Ok(serde_json::from_value(serde_json::to_value(manifest)?)?)
}
#[cfg(test)]
pub(crate) fn read_manifest_json_sync(path: impl AsRef<std::path::Path>) -> Result<String> {
    let bytes = std::fs::read(path)?;
    let manifest = decode(&bytes, bytes.starts_with(MAGIC))?;
    Ok(serde_json::to_string_pretty(&manifest)?)
}
#[cfg(test)]
pub(crate) async fn read_manifest_json(path: impl AsRef<std::path::Path>) -> Result<String> {
    read_manifest_json_sync(path)
}
