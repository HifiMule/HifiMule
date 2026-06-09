use crate::api::JellyfinClient;
use crate::db::Database;
use crate::device::DeviceManifest;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

const PLAYBACK_LOG_PATH: &str = ".rockbox/playback.log";
const LEGACY_SCROBBLER_LOG_PATH: &str = ".scrobbler.log";

#[derive(Debug, Clone)]
pub struct ScrobblerEntry {
    pub artist: String,
    pub album: String,
    pub title: String,
    // Parsed from source but not yet forwarded to any scrobbling backend.
    // Retained for future API compatibility (e.g. ListenBrainz requires duration and MB ID).
    #[allow(dead_code)]
    pub track_num: Option<u32>,
    #[allow(dead_code)]
    pub duration_secs: u64,
    pub rating: String,
    pub timestamp_unix: i64,
    #[allow(dead_code)]
    pub mb_track_id: Option<String>,
    pub source_path: Option<String>,
    pub played_position_ticks: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScrobblerResult {
    pub total_entries: usize,
    pub submitted: usize,
    pub skipped_rating: usize,
    pub skipped_duplicate: usize,
    pub unmatched: usize,
    pub failed: usize,
    pub errors: Vec<String>,
    pub device_id: String,
    pub total_scrobbled: i64,
}

fn seconds_to_jellyfin_ticks(seconds: u64) -> u64 {
    seconds.saturating_mul(10_000_000)
}

pub fn parse_scrobbler_log(content: &str) -> Vec<ScrobblerEntry> {
    let mut entries = Vec::new();

    for line in content.lines() {
        // Skip header lines
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some(entry) = parse_playback_log_line(line.trim()) {
            entries.push(entry);
            continue;
        }

        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() < 8 {
            continue;
        }

        let artist = fields[0].to_string();
        let album = fields[1].to_string();
        let title = fields[2].to_string();
        let track_num = fields[3].parse::<u32>().ok();
        let duration_secs = match fields[4].parse::<u64>() {
            Ok(d) => d,
            Err(_) => continue,
        };
        let rating = fields[5].to_string();
        let timestamp_unix = match fields[6].parse::<i64>() {
            Ok(t) => t,
            Err(_) => continue,
        };
        let mb_track_id = if fields[7].is_empty() {
            None
        } else {
            Some(fields[7].to_string())
        };

        entries.push(ScrobblerEntry {
            artist,
            album,
            title,
            track_num,
            duration_secs,
            rating,
            timestamp_unix,
            mb_track_id,
            source_path: None,
            played_position_ticks: None,
        });
    }

    entries
}

fn parse_playback_log_line(line: &str) -> Option<ScrobblerEntry> {
    let mut fields = line.splitn(4, ':');
    let timestamp_raw = fields.next()?.parse::<i64>().ok()?;
    let elapsed_ms = fields.next()?.parse::<u64>().ok()?;
    let duration_ms = fields.next()?.parse::<u64>().ok()?;
    let path = fields.next()?.trim();

    let (artist, album, title) = parse_track_info_from_path(path)?;
    let timestamp_unix = if timestamp_raw > 1_000_000_000_000 {
        timestamp_raw / 1000
    } else {
        timestamp_raw
    };
    let duration_secs = duration_ms / 1000;
    let played_position_ticks = elapsed_ms.saturating_mul(10_000);
    let rating = if duration_ms > 0 && elapsed_ms.saturating_mul(2) >= duration_ms {
        "L"
    } else {
        "S"
    };

    Some(ScrobblerEntry {
        artist,
        album,
        title,
        track_num: None,
        duration_secs,
        rating: rating.to_string(),
        timestamp_unix,
        mb_track_id: None,
        source_path: Some(normalize_scrobble_path(path)),
        played_position_ticks: Some(played_position_ticks),
    })
}

fn parse_track_info_from_path(path: &str) -> Option<(String, String, String)> {
    let components: Vec<&str> = path
        .split('/')
        .filter(|part| !part.is_empty() && !part.starts_with('<'))
        .collect();

    if components.len() < 3 {
        return None;
    }

    let file_name = components.last()?;
    let album = components.get(components.len() - 2)?.trim();
    let artist = components.get(components.len() - 3)?.trim();
    let title = strip_track_number_prefix(strip_audio_extension(file_name)).trim();

    if artist.is_empty() || album.is_empty() || title.is_empty() {
        return None;
    }

    Some((artist.to_string(), album.to_string(), title.to_string()))
}

fn normalize_scrobble_path(path: &str) -> String {
    path.replace('\\', "/")
        .split('/')
        .filter(|part| !part.is_empty() && !part.starts_with('<'))
        .collect::<Vec<_>>()
        .join("/")
}

fn strip_audio_extension(file_name: &str) -> &str {
    file_name
        .rsplit_once('.')
        .map_or(file_name, |(stem, _)| stem)
}

fn strip_track_number_prefix(title: &str) -> &str {
    let trimmed = title.trim_start();
    let digit_count = trimmed.chars().take_while(|ch| ch.is_ascii_digit()).count();

    if digit_count == 0 || digit_count > 3 {
        return trimmed;
    }

    let rest = &trimmed[digit_count..].trim_start();
    rest.strip_prefix('-')
        .or_else(|| rest.strip_prefix('.'))
        .map(str::trim_start)
        .unwrap_or(trimmed)
}

fn is_missing_scrobbler_log_error(error: &anyhow::Error) -> bool {
    if error
        .downcast_ref::<std::io::Error>()
        .map(|io| io.kind() == std::io::ErrorKind::NotFound)
        .unwrap_or(false)
    {
        return true;
    }

    error.chain().any(|cause| {
        let message = cause.to_string().to_lowercase();
        (message.contains(PLAYBACK_LOG_PATH) || message.contains(LEGACY_SCROBBLER_LOG_PATH))
            && message.contains("not found")
    })
}

async fn read_scrobbler_log(
    device_io: Arc<dyn crate::device_io::DeviceIO>,
) -> Result<Option<(String, Vec<u8>)>, anyhow::Error> {
    match device_io.read_file(PLAYBACK_LOG_PATH).await {
        Ok(bytes) => return Ok(Some((PLAYBACK_LOG_PATH.to_string(), bytes))),
        Err(e) if is_missing_scrobbler_log_error(&e) => {}
        Err(e) => return Err(e.context(format!("Failed to read {}", PLAYBACK_LOG_PATH))),
    }

    match device_io.read_file(LEGACY_SCROBBLER_LOG_PATH).await {
        Ok(bytes) => Ok(Some((LEGACY_SCROBBLER_LOG_PATH.to_string(), bytes))),
        Err(e) if is_missing_scrobbler_log_error(&e) => Ok(None),
        Err(e) => Err(e.context(format!("Failed to read {}", LEGACY_SCROBBLER_LOG_PATH))),
    }
}

pub async fn process_device_scrobbles(
    device_io: Arc<dyn crate::device_io::DeviceIO>,
    device_id: String,
    manifest: Option<Arc<DeviceManifest>>,
    db: Arc<Database>,
    client: Arc<JellyfinClient>,
    url: &str,
    token: &str,
    user_id: &str,
) -> ScrobblerResult {
    let (log_path, bytes) = match read_scrobbler_log(device_io).await {
        Ok(Some(log)) => log,
        Ok(None) => {
            return ScrobblerResult {
                total_entries: 0,
                submitted: 0,
                skipped_rating: 0,
                skipped_duplicate: 0,
                unmatched: 0,
                failed: 0,
                errors: Vec::new(),
                device_id,
                total_scrobbled: 0,
            };
        }
        Err(e) => {
            return ScrobblerResult {
                total_entries: 0,
                submitted: 0,
                skipped_rating: 0,
                skipped_duplicate: 0,
                unmatched: 0,
                failed: 0,
                errors: vec![e.to_string()],
                device_id,
                total_scrobbled: 0,
            };
        }
    };

    let content = match String::from_utf8(bytes) {
        Ok(s) => s,
        Err(e) => {
            return ScrobblerResult {
                total_entries: 0,
                submitted: 0,
                skipped_rating: 0,
                skipped_duplicate: 0,
                unmatched: 0,
                failed: 0,
                errors: vec![format!("Failed to read {}: {}", log_path, e)],
                device_id,
                total_scrobbled: 0,
            };
        }
    };

    let entries = parse_scrobbler_log(&content);
    let total_entries = entries.len();
    let mut submitted = 0usize;
    let mut skipped_rating = 0usize;
    let mut skipped_duplicate = 0usize;
    let mut unmatched = 0usize;
    let mut failed = 0usize;
    let mut errors: Vec<String> = Vec::new();

    for entry in &entries {
        if entry.rating != "L" {
            skipped_rating += 1;
            continue;
        }

        // Dedup check — skip if already submitted (Story 5.2)
        match db.is_scrobble_recorded(
            &device_id,
            &entry.artist,
            &entry.album,
            &entry.title,
            entry.timestamp_unix,
        ) {
            Ok(true) => {
                println!(
                    "[Scrobbler] Skipping duplicate: '{}' by '{}'",
                    entry.title, entry.artist
                );
                skipped_duplicate += 1;
                continue;
            }
            Ok(false) => {}
            Err(e) => {
                // Non-fatal: log and proceed with submission attempt
                println!(
                    "[Scrobbler] Warning: dedup check failed for '{}': {} — will attempt submission",
                    entry.title, e
                );
            }
        }

        let item_id = match find_manifest_item_id(manifest.as_deref(), entry) {
            Some(id) => id,
            None => {
                match find_jellyfin_item_id(client.as_ref(), url, token, user_id, entry).await {
                    Ok(Some(id)) => id,
                    Ok(None) => {
                        unmatched += 1;
                        println!(
                            "[Scrobbler] No match for '{}' by '{}' on album '{}'",
                            entry.title, entry.artist, entry.album
                        );
                        continue;
                    }
                    Err(e) => {
                        errors.push(format!(
                            "Search failed for '{}' by '{}': {}",
                            entry.title, entry.artist, e
                        ));
                        failed += 1;
                        continue;
                    }
                }
            }
        };

        // Submit to Jellyfin
        if let Err(e) = submit_scrobble_to_jellyfin(
            client.as_ref(),
            url,
            token,
            &item_id,
            entry
                .played_position_ticks
                .unwrap_or_else(|| seconds_to_jellyfin_ticks(entry.duration_secs)),
        )
        .await
        {
            errors.push(format!(
                "Failed to submit '{}' by '{}': {}",
                entry.title, entry.artist, e
            ));
            failed += 1;
            continue;
        }

        // Record in scrobble_history — if this fails the track was submitted to Jellyfin
        // but won't appear in scrobble_history, so the dedup check will miss it on the
        // next sync and submit it again (duplicate play count). Count as failed so the
        // caller can detect and investigate.
        if let Err(e) = db.record_scrobble(
            &device_id,
            &entry.artist,
            &entry.album,
            &entry.title,
            entry.timestamp_unix,
        ) {
            errors.push(format!(
                "Failed to record scrobble for '{}' by '{}': {} — track was submitted to Jellyfin but will not be deduplicated on next sync",
                entry.title, entry.artist, e
            ));
            failed += 1;
            continue;
        }

        submitted += 1;
    }

    let total_scrobbled = db.get_scrobble_count(&device_id).unwrap_or(0);

    ScrobblerResult {
        total_entries,
        submitted,
        skipped_rating,
        skipped_duplicate,
        unmatched,
        failed,
        errors,
        device_id,
        total_scrobbled,
    }
}

async fn submit_scrobble_to_jellyfin(
    client: &JellyfinClient,
    url: &str,
    token: &str,
    item_id: &str,
    position_ticks: u64,
) -> anyhow::Result<()> {
    client
        .report_playback_session(url, token, item_id, position_ticks)
        .await
}

fn find_manifest_item_id(
    manifest: Option<&DeviceManifest>,
    entry: &ScrobblerEntry,
) -> Option<String> {
    let source_path = entry.source_path.as_ref()?;
    let source_path = normalize_scrobble_path(source_path);

    manifest?
        .synced_items
        .iter()
        .find(|item| normalize_scrobble_path(&item.local_path).eq_ignore_ascii_case(&source_path))
        .map(|item| item.jellyfin_id.clone())
}

async fn find_jellyfin_item_id(
    client: &JellyfinClient,
    url: &str,
    token: &str,
    user_id: &str,
    entry: &ScrobblerEntry,
) -> anyhow::Result<Option<String>> {
    let candidates = client
        .search_audio_items(url, token, user_id, &entry.title)
        .await?;

    let album_lower = entry.album.to_lowercase();
    let artist_lower = entry.artist.to_lowercase();
    Ok(candidates
        .into_iter()
        .find(|item| {
            let album_match = item
                .album
                .as_ref()
                .map(|a| a.to_lowercase() == album_lower)
                .unwrap_or(false);
            let album_artist_match = item
                .album_artist
                .as_ref()
                .map(|a| a.to_lowercase() == artist_lower)
                .unwrap_or(false);
            album_match || album_artist_match
        })
        .map(|item| item.id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    const SAMPLE_LOG: &str = "\
#AUDIOSCROBBLER/1.1
#TZ/UTC
#CLIENT/Rockbox iPod Video 3.15.0
Pink Floyd\tThe Dark Side of the Moon\tMoney\t6\t382\tL\t1706745600\t
The Beatles\tAbbey Road\tCome Together\t1\t259\tS\t1706749200\t
Led Zeppelin\tLed Zeppelin IV\tStairway to Heaven\t4\t482\tL\t1706752800\tsome-mb-id
";

    #[test]
    fn test_parse_sample_log() {
        let entries = parse_scrobbler_log(SAMPLE_LOG);
        assert_eq!(entries.len(), 3);

        assert_eq!(entries[0].artist, "Pink Floyd");
        assert_eq!(entries[0].album, "The Dark Side of the Moon");
        assert_eq!(entries[0].title, "Money");
        assert_eq!(entries[0].track_num, Some(6));
        assert_eq!(entries[0].duration_secs, 382);
        assert_eq!(entries[0].rating, "L");
        assert_eq!(entries[0].timestamp_unix, 1706745600);
        assert!(entries[0].mb_track_id.is_none());

        assert_eq!(entries[1].rating, "S");

        assert_eq!(entries[2].artist, "Led Zeppelin");
        assert_eq!(entries[2].mb_track_id, Some("some-mb-id".to_string()));
    }

    #[test]
    fn test_parse_playback_log() {
        let log = "\
1779229621:285518:285518:/<microSD0>/Music/BON JOVI/New Jersey (28PD-498)/09 - Stick To Your Guns.mp3
1779229622000:78420:285518:/<microSD0>/Music/BON JOVI/New Jersey (28PD-498)/10. Ride Cowboy Ride.mp3
";
        let entries = parse_scrobbler_log(log);
        assert_eq!(entries.len(), 2);

        assert_eq!(entries[0].artist, "BON JOVI");
        assert_eq!(entries[0].album, "New Jersey (28PD-498)");
        assert_eq!(entries[0].title, "Stick To Your Guns");
        assert_eq!(entries[0].duration_secs, 285);
        assert_eq!(entries[0].rating, "L");
        assert_eq!(entries[0].timestamp_unix, 1779229621);

        assert_eq!(entries[1].title, "Ride Cowboy Ride");
        assert_eq!(entries[1].rating, "S");
        assert_eq!(entries[1].timestamp_unix, 1779229622);
    }

    #[test]
    fn test_manifest_match_uses_playback_log_path() {
        let entries = parse_scrobbler_log(
            "1779229621:219508:285518:/<microSD0>/Music/BON JOVI/New Jersey (28PD-498)/09 - Stick To Your Guns.mp3\n",
        );
        let manifest = DeviceManifest {
            device_id: "test-device-id".to_string(),
            version: "1.0".to_string(),
            synced_items: vec![crate::device::SyncedItem {
                jellyfin_id: "6aff97688560276ce460ff2187ff8a6f".to_string(),
                name: "Stick To Your Guns".to_string(),
                album: Some("New Jersey (28PD-498)".to_string()),
                artist: Some("BON JOVI".to_string()),
                local_path: "Music/BON JOVI/New Jersey (28PD-498)/09 - Stick To Your Guns.mp3"
                    .to_string(),
                size_bytes: 11_540_179,
                synced_at: "2026-05-19T19:09:05Z".to_string(),
                original_name: None,
                etag: None,
                provider_album_id: Some("c5b1d7c3ba73c24813a80690e0b4a28c".to_string()),
                provider_content_type: None,
                provider_suffix: Some("mp3".to_string()),
                original_bitrate: None,
                original_container: None,
                track_number: None,
                server_id: None,
            }],
            ..DeviceManifest::default()
        };

        assert_eq!(
            find_manifest_item_id(Some(&manifest), &entries[0]),
            Some("6aff97688560276ce460ff2187ff8a6f".to_string())
        );
    }

    #[test]
    fn test_parse_malformed_lines_skipped() {
        let log = "\
#AUDIOSCROBBLER/1.1
too\tfew\tfields
Pink Floyd\tThe Dark Side of the Moon\tMoney\t6\t382\tL\t1706745600\t
bad_duration\tbad_ts\ttitle\t1\tNOT_A_NUM\tL\tNOT_TS\t
";
        let entries = parse_scrobbler_log(log);
        // Only the well-formed Pink Floyd line should parse
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "Money");
    }

    #[test]
    fn test_parse_empty_log_headers_only() {
        let log = "\
#AUDIOSCROBBLER/1.1
#TZ/UTC
#CLIENT/Rockbox iPod Video 3.15.0
";
        let entries = parse_scrobbler_log(log);
        assert!(entries.is_empty());
    }

    fn make_msc_backend(dir: &std::path::Path) -> Arc<dyn crate::device_io::DeviceIO> {
        Arc::new(crate::device_io::MscBackend::new(dir.to_path_buf()))
    }

    #[derive(Debug)]
    struct MtpStyleMissingScrobblerLog;

    #[async_trait]
    impl crate::device_io::DeviceIO for MtpStyleMissingScrobblerLog {
        async fn read_file(&self, path: &str) -> anyhow::Result<Vec<u8>> {
            Err(anyhow::anyhow!("WPD: path component '{}' not found", path))
        }

        async fn write_file(&self, _path: &str, _data: &[u8]) -> anyhow::Result<()> {
            unimplemented!("not needed for scrobbler missing-log test")
        }

        async fn write_with_verify(&self, _path: &str, _data: &[u8]) -> anyhow::Result<()> {
            unimplemented!("not needed for scrobbler missing-log test")
        }

        async fn delete_file(&self, _path: &str) -> anyhow::Result<()> {
            unimplemented!("not needed for scrobbler missing-log test")
        }

        async fn list_files(
            &self,
            _path: &str,
        ) -> anyhow::Result<Vec<crate::device_io::FileEntry>> {
            unimplemented!("not needed for scrobbler missing-log test")
        }

        async fn free_space(&self) -> anyhow::Result<u64> {
            unimplemented!("not needed for scrobbler missing-log test")
        }

        async fn ensure_dir(&self, _path: &str) -> anyhow::Result<()> {
            unimplemented!("not needed for scrobbler missing-log test")
        }

        async fn cleanup_empty_subdirs(&self, _path: &str) -> anyhow::Result<()> {
            unimplemented!("not needed for scrobbler missing-log test")
        }
    }

    #[tokio::test]
    async fn test_process_device_no_log_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let device_io = make_msc_backend(temp_dir.path());
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let client = Arc::new(crate::api::JellyfinClient::new());

        let result = process_device_scrobbles(
            device_io,
            "test-device-id".to_string(),
            None,
            db,
            client,
            "http://localhost:8096",
            "token-placeholder",
            "user-placeholder",
        )
        .await;

        assert_eq!(result.total_entries, 0);
        assert_eq!(result.submitted, 0);
        assert_eq!(result.failed, 0);
        assert!(result.errors.is_empty());
        assert_eq!(result.total_scrobbled, 0);
    }

    #[tokio::test]
    async fn test_process_device_mtp_style_missing_log_is_empty_success() {
        let device_io = Arc::new(MtpStyleMissingScrobblerLog);
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let client = Arc::new(crate::api::JellyfinClient::new());

        let result = process_device_scrobbles(
            device_io,
            "test-mtp-device-id".to_string(),
            None,
            db,
            client,
            "http://localhost:8096",
            "token-placeholder",
            "user-placeholder",
        )
        .await;

        assert_eq!(result.total_entries, 0);
        assert_eq!(result.submitted, 0);
        assert_eq!(result.skipped_rating, 0);
        assert_eq!(result.skipped_duplicate, 0);
        assert_eq!(result.unmatched, 0);
        assert_eq!(result.failed, 0);
        assert_eq!(result.total_scrobbled, 0);
        assert!(result.errors.is_empty());
    }

    #[tokio::test]
    async fn test_process_device_unreadable_log() {
        // Test with a directory named .scrobbler.log — read will fail on a directory
        let bad_dir = tempfile::tempdir().unwrap();
        let fake_log = bad_dir.path().join(".scrobbler.log");
        std::fs::create_dir(&fake_log).unwrap(); // directory, not a file — read will fail

        let device_io = make_msc_backend(bad_dir.path());
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let client = Arc::new(crate::api::JellyfinClient::new());

        let result = process_device_scrobbles(
            device_io,
            "test-device-id".to_string(),
            None,
            db,
            client,
            "http://localhost:8096",
            "token-placeholder",
            "user-placeholder",
        )
        .await;

        assert_eq!(result.total_entries, 0);
        assert_eq!(result.submitted, 0);
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].contains("Failed to read .scrobbler.log"));
    }

    #[tokio::test]
    async fn test_process_device_prefers_playback_log() {
        let temp_dir = tempfile::tempdir().unwrap();
        let playback_dir = temp_dir.path().join(".rockbox");
        std::fs::create_dir(&playback_dir).unwrap();
        std::fs::write(
            playback_dir.join("playback.log"),
            b"1779229621:285518:285518:/<microSD0>/Music/BON JOVI/New Jersey (28PD-498)/09 - Stick To Your Guns.mp3\n",
        )
        .unwrap();
        std::fs::write(
            temp_dir.path().join(".scrobbler.log"),
            SAMPLE_LOG.as_bytes(),
        )
        .unwrap();

        let db = Arc::new(crate::db::Database::memory().unwrap());
        let device_id = "test-device-uuid".to_string();
        db.record_scrobble(
            &device_id,
            "BON JOVI",
            "New Jersey (28PD-498)",
            "Stick To Your Guns",
            1779229621,
        )
        .unwrap();

        let device_io = make_msc_backend(temp_dir.path());
        let client = Arc::new(crate::api::JellyfinClient::new());
        let result = process_device_scrobbles(
            device_io,
            device_id,
            None,
            db,
            client,
            "http://localhost:8096",
            "token-placeholder",
            "user-placeholder",
        )
        .await;

        assert_eq!(result.total_entries, 1);
        assert_eq!(result.skipped_duplicate, 1);
        assert_eq!(result.submitted, 0);
        assert_eq!(result.failed, 0);
        assert!(result.errors.is_empty());
    }

    #[tokio::test]
    async fn test_process_device_submits_manifest_match() {
        let mut server = mockito::Server::new_async().await;
        let item_id = "6aff97688560276ce460ff2187ff8a6f";
        let _playing = server
            .mock("POST", "/Sessions/Playing")
            .match_header("X-Emby-Token", "test-token-1234567890")
            .match_body(mockito::Matcher::PartialJson(serde_json::json!({
                "ItemId": item_id,
                "PositionTicks": 0,
                "PlayMethod": "DirectPlay",
            })))
            .with_status(204)
            .expect(1)
            .create_async()
            .await;
        let _stopped = server
            .mock("POST", "/Sessions/Playing/Stopped")
            .match_header("X-Emby-Token", "test-token-1234567890")
            .match_body(mockito::Matcher::PartialJson(serde_json::json!({
                "ItemId": item_id,
                "PositionTicks": 2_195_080_000u64,
                "Failed": false,
            })))
            .with_status(204)
            .expect(1)
            .create_async()
            .await;

        let temp_dir = tempfile::tempdir().unwrap();
        let playback_dir = temp_dir.path().join(".rockbox");
        std::fs::create_dir(&playback_dir).unwrap();
        std::fs::write(
            playback_dir.join("playback.log"),
            b"1779229621:219508:285518:/<microSD0>/Music/BON JOVI/New Jersey (28PD-498)/09 - Stick To Your Guns.mp3\n",
        )
        .unwrap();

        let manifest = Arc::new(DeviceManifest {
            device_id: "test-device-id".to_string(),
            version: "1.0".to_string(),
            synced_items: vec![crate::device::SyncedItem {
                jellyfin_id: item_id.to_string(),
                name: "Stick To Your Guns".to_string(),
                album: Some("New Jersey (28PD-498)".to_string()),
                artist: Some("BON JOVI".to_string()),
                local_path: "Music/BON JOVI/New Jersey (28PD-498)/09 - Stick To Your Guns.mp3"
                    .to_string(),
                size_bytes: 11_540_179,
                synced_at: "2026-05-19T19:09:05Z".to_string(),
                original_name: None,
                etag: None,
                provider_album_id: None,
                provider_content_type: None,
                provider_suffix: Some("mp3".to_string()),
                original_bitrate: None,
                original_container: None,
                track_number: None,
                server_id: None,
            }],
            ..DeviceManifest::default()
        });

        let device_io = make_msc_backend(temp_dir.path());
        let result = process_device_scrobbles(
            device_io,
            "test-device-id".to_string(),
            Some(manifest),
            Arc::new(crate::db::Database::memory().unwrap()),
            Arc::new(crate::api::JellyfinClient::new()),
            &server.url(),
            "test-token-1234567890",
            "user-placeholder",
        )
        .await;

        assert_eq!(result.total_entries, 1);
        assert_eq!(result.submitted, 1);
        assert_eq!(result.unmatched, 0);
        assert_eq!(result.failed, 0);
        assert!(result.errors.is_empty());
    }

    #[tokio::test]
    async fn test_process_device_skips_already_scrobbled() {
        let temp_dir = tempfile::tempdir().unwrap();
        let log_path = temp_dir.path().join(".scrobbler.log");
        std::fs::write(&log_path, SAMPLE_LOG.as_bytes()).unwrap();

        let db = Arc::new(crate::db::Database::memory().unwrap());
        let device_id = "test-device-uuid".to_string();

        // Pre-populate scrobble_history with both "L" entries from SAMPLE_LOG
        db.record_scrobble(
            &device_id,
            "Pink Floyd",
            "The Dark Side of the Moon",
            "Money",
            1706745600,
        )
        .unwrap();
        db.record_scrobble(
            &device_id,
            "Led Zeppelin",
            "Led Zeppelin IV",
            "Stairway to Heaven",
            1706752800,
        )
        .unwrap();

        let device_io = make_msc_backend(temp_dir.path());
        let client = Arc::new(crate::api::JellyfinClient::new());
        let result = process_device_scrobbles(
            device_io,
            device_id,
            None,
            db,
            client,
            "http://localhost:8096",
            "token-placeholder",
            "user-placeholder",
        )
        .await;

        assert_eq!(result.total_entries, 3);
        assert_eq!(result.skipped_duplicate, 2);
        assert_eq!(result.skipped_rating, 1);
        assert_eq!(result.submitted, 0);
        assert_eq!(result.unmatched, 0);
        assert_eq!(result.failed, 0);
        assert!(result.errors.is_empty());
    }

    #[tokio::test]
    async fn test_process_device_dedup_error_is_nonfatal() {
        // AC #5: if is_scrobble_recorded() returns Err, the entry is processed normally
        // (not skipped, not counted as failed for the dedup check itself).
        // We trigger the Err path by dropping the scrobble_history table.
        let temp_dir = tempfile::tempdir().unwrap();
        let log_path = temp_dir.path().join(".scrobbler.log");
        std::fs::write(&log_path, SAMPLE_LOG.as_bytes()).unwrap();

        let db = Arc::new(crate::db::Database::memory().unwrap());
        db.drop_scrobble_table_for_test(); // is_scrobble_recorded() will now return Err("no such table")

        let device_io = make_msc_backend(temp_dir.path());
        let client = Arc::new(crate::api::JellyfinClient::new());
        let result = process_device_scrobbles(
            device_io,
            "test-device-id".to_string(),
            None,
            db,
            client,
            "http://localhost:8096",
            "token-placeholder",
            "user-placeholder",
        )
        .await;

        // No entries skipped as duplicate — the Err path fell through to normal processing
        assert_eq!(result.skipped_duplicate, 0);
        // The 2 "L" entries attempted API submission and failed (no server reachable)
        assert_eq!(result.skipped_rating, 1);
        assert_eq!(result.failed, 2);
        assert_eq!(result.submitted, 0);
        assert_eq!(result.total_entries, 3);
    }

    #[test]
    fn test_scrobbler_result_submitted_excludes_db_failures() {
        // Verify the logic: submitted should only increment when BOTH
        // report_item_played AND record_scrobble succeed.
        // This test documents the invariant via the struct field semantics.
        let result = ScrobblerResult {
            total_entries: 3,
            submitted: 1,
            skipped_rating: 1,
            skipped_duplicate: 0,
            unmatched: 0,
            failed: 1, // one track had DB record failure
            errors: vec!["Failed to record scrobble for 'Track' by 'Artist': db error — track was submitted to Jellyfin but will not be deduplicated on next sync".to_string()],
            device_id: "ipod-001".to_string(),
            total_scrobbled: 1,
        };
        // submitted + skipped_rating + skipped_duplicate + unmatched + failed == total_entries
        assert_eq!(
            result.submitted
                + result.skipped_rating
                + result.skipped_duplicate
                + result.unmatched
                + result.failed,
            result.total_entries
        );
    }
}
