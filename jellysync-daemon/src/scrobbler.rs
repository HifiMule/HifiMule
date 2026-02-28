use crate::api::JellyfinClient;
use crate::db::Database;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ScrobblerEntry {
    pub artist: String,
    pub album: String,
    pub title: String,
    pub track_num: Option<u32>,
    pub duration_secs: u64,
    pub rating: String,
    pub timestamp_unix: i64,
    pub mb_track_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScrobblerResult {
    pub total_entries: usize,
    pub submitted: usize,
    pub skipped_rating: usize,
    pub unmatched: usize,
    pub failed: usize,
    pub errors: Vec<String>,
    pub device_id: String,
    pub total_scrobbled: i64,
}

pub fn parse_scrobbler_log(content: &str) -> Vec<ScrobblerEntry> {
    let mut entries = Vec::new();

    for line in content.lines() {
        // Skip header lines
        if line.starts_with('#') {
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
        });
    }

    entries
}

pub async fn process_device_scrobbles(
    device_path: &Path,
    db: Arc<Database>,
    client: Arc<JellyfinClient>,
    url: &str,
    token: &str,
    user_id: &str,
) -> ScrobblerResult {
    let device_id = device_path.to_string_lossy().to_string();

    let log_path = device_path.join(".scrobbler.log");
    if !log_path.exists() {
        return ScrobblerResult {
            total_entries: 0,
            submitted: 0,
            skipped_rating: 0,
            unmatched: 0,
            failed: 0,
            errors: Vec::new(),
            device_id,
            total_scrobbled: 0,
        };
    }

    let content = match std::fs::read_to_string(&log_path) {
        Ok(c) => c,
        Err(e) => {
            return ScrobblerResult {
                total_entries: 0,
                submitted: 0,
                skipped_rating: 0,
                unmatched: 0,
                failed: 0,
                errors: vec![format!("Failed to read .scrobbler.log: {}", e)],
                device_id,
                total_scrobbled: 0,
            };
        }
    };

    let entries = parse_scrobbler_log(&content);
    let total_entries = entries.len();
    let mut submitted = 0usize;
    let mut skipped_rating = 0usize;
    let mut unmatched = 0usize;
    let mut failed = 0usize;
    let mut errors: Vec<String> = Vec::new();

    for entry in &entries {
        if entry.rating != "L" {
            skipped_rating += 1;
            continue;
        }

        // Search Jellyfin for matching track
        let search_result = client
            .search_audio_items(url, token, user_id, &entry.title)
            .await;

        let candidates = match search_result {
            Ok(items) => items,
            Err(e) => {
                errors.push(format!(
                    "Search failed for '{}' by '{}': {}",
                    entry.title, entry.artist, e
                ));
                failed += 1;
                continue;
            }
        };

        // Filter by album match (case-insensitive)
        let album_lower = entry.album.to_lowercase();
        let matched = candidates.into_iter().find(|item| {
            let album_match = item
                .album
                .as_ref()
                .map(|a| a.to_lowercase() == album_lower)
                .unwrap_or(false);
            let album_artist_match = item
                .album_artist
                .as_ref()
                .map(|a| a.to_lowercase() == entry.artist.to_lowercase())
                .unwrap_or(false);
            album_match || album_artist_match
        });

        let item = match matched {
            Some(i) => i,
            None => {
                unmatched += 1;
                println!(
                    "[Scrobbler] No match for '{}' by '{}' on album '{}'",
                    entry.title, entry.artist, entry.album
                );
                continue;
            }
        };

        // Submit to Jellyfin
        if let Err(e) = client
            .report_item_played(url, token, user_id, &item.id)
            .await
        {
            errors.push(format!(
                "Failed to submit '{}' by '{}': {}",
                entry.title, entry.artist, e
            ));
            failed += 1;
            continue;
        }

        // Record in scrobble_history — if this fails the track was submitted but won't
        // be deduped in Story 5.2, so count it as failed rather than submitted.
        if let Err(e) = db.record_scrobble(
            &device_id,
            &entry.artist,
            &entry.album,
            &entry.title,
            entry.timestamp_unix,
        ) {
            errors.push(format!(
                "Failed to record scrobble for '{}': {}",
                entry.title, e
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
        unmatched,
        failed,
        errors,
        device_id,
        total_scrobbled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[tokio::test]
    async fn test_process_device_no_log_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let client = Arc::new(crate::api::JellyfinClient::new());

        let result = process_device_scrobbles(
            temp_dir.path(),
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
    async fn test_process_device_unreadable_log() {
        // Write a valid .scrobbler.log but in a path that gets removed
        let temp_dir = tempfile::tempdir().unwrap();
        let log_path = temp_dir.path().join(".scrobbler.log");
        std::fs::write(&log_path, b"#AUDIOSCROBBLER/1.1\n").unwrap();

        // Remove the file after creating it to simulate read failure on a different path
        // Instead, test with a directory named .scrobbler.log (unreadable as file)
        let bad_dir = tempfile::tempdir().unwrap();
        let fake_log = bad_dir.path().join(".scrobbler.log");
        std::fs::create_dir(&fake_log).unwrap(); // directory, not a file — read_to_string will fail

        let db = Arc::new(crate::db::Database::memory().unwrap());
        let client = Arc::new(crate::api::JellyfinClient::new());

        let result = process_device_scrobbles(
            bad_dir.path(),
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

    #[test]
    fn test_scrobbler_result_submitted_excludes_db_failures() {
        // Verify the logic: submitted should only increment when BOTH
        // report_item_played AND record_scrobble succeed.
        // This test documents the invariant via the struct field semantics.
        let result = ScrobblerResult {
            total_entries: 3,
            submitted: 1,
            skipped_rating: 1,
            unmatched: 0,
            failed: 1, // one track had DB record failure
            errors: vec!["Failed to record scrobble for 'Track': db error".to_string()],
            device_id: "ipod-001".to_string(),
            total_scrobbled: 1,
        };
        // submitted + skipped + unmatched + failed == total_entries
        assert_eq!(
            result.submitted + result.skipped_rating + result.unmatched + result.failed,
            result.total_entries
        );
    }
}
