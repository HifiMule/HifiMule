use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::device::DeviceManifest;

/// An item desired for sync (from the UI basket / Jellyfin API).
#[derive(Debug, Clone)]
pub struct DesiredItem {
    pub jellyfin_id: String,
    pub name: String,
    pub album: Option<String>,
    pub artist: Option<String>,
    pub size_bytes: u64,
}

/// An item to be added to the device.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SyncAddItem {
    pub jellyfin_id: String,
    pub name: String,
    pub album: Option<String>,
    pub artist: Option<String>,
    pub size_bytes: u64,
}

/// An item to be deleted from the device.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SyncDeleteItem {
    pub jellyfin_id: String,
    pub local_path: String,
    pub name: String,
}

/// The result of a delta calculation between desired items and current manifest.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SyncDelta {
    pub adds: Vec<SyncAddItem>,
    pub deletes: Vec<SyncDeleteItem>,
    pub unchanged: Vec<String>,
}

/// Calculates the delta between desired items (from basket) and the current manifest.
///
/// Performs server ID change detection: if an item in adds matches a delete by
/// (name, album, artist) metadata, it's treated as an ID reassignment rather than
/// a separate add+delete.
pub fn calculate_delta(desired_items: &[DesiredItem], manifest: &DeviceManifest) -> SyncDelta {
    let current_ids: HashSet<&str> = manifest
        .synced_items
        .iter()
        .map(|i| i.jellyfin_id.as_str())
        .collect();

    let desired_ids: HashSet<&str> = desired_items
        .iter()
        .map(|i| i.jellyfin_id.as_str())
        .collect();

    // Initial adds: desired items not in current manifest
    let adds: Vec<SyncAddItem> = desired_items
        .iter()
        .filter(|i| !current_ids.contains(i.jellyfin_id.as_str()))
        .map(|i| SyncAddItem {
            jellyfin_id: i.jellyfin_id.clone(),
            name: i.name.clone(),
            album: i.album.clone(),
            artist: i.artist.clone(),
            size_bytes: i.size_bytes,
        })
        .collect();

    // Initial deletes: manifest items not in desired set
    // AND build the metadata map in the same pass
    let mut deletes: Vec<SyncDeleteItem> = Vec::new();
    let mut delete_by_metadata: HashMap<(String, Option<String>, Option<String>), usize> =
        HashMap::new();

    for item in &manifest.synced_items {
        if !desired_ids.contains(item.jellyfin_id.as_str()) {
            let idx = deletes.len();
            deletes.push(SyncDeleteItem {
                jellyfin_id: item.jellyfin_id.clone(),
                local_path: item.local_path.clone(),
                name: item.name.clone(),
            });

            let key = (
                item.name.to_lowercase(),
                item.album.as_ref().map(|a| a.to_lowercase()),
                item.artist.as_ref().map(|a| a.to_lowercase()),
            );
            delete_by_metadata.insert(key, idx);
        }
    }

    // Find adds that match a delete by metadata (ID change detection)
    let mut matched_add_indices: HashSet<usize> = HashSet::new();
    let mut matched_delete_indices: HashSet<usize> = HashSet::new();

    for (add_idx, add) in adds.iter().enumerate() {
        let key = (
            add.name.to_lowercase(),
            add.album.as_ref().map(|a| a.to_lowercase()),
            add.artist.as_ref().map(|a| a.to_lowercase()),
        );

        if let Some(&del_idx) = delete_by_metadata.get(&key) {
            if !matched_delete_indices.contains(&del_idx) {
                matched_add_indices.insert(add_idx);
                matched_delete_indices.insert(del_idx);
            }
        }
    }

    // Remove matched pairs — these are ID reassignments, not real adds/deletes
    // The add still stays (new ID needs to be recorded), but the delete is removed
    // since the file content is equivalent.
    // Actually, for ID changes: we keep the add (new ID) and remove the delete
    // so the file stays on disk but gets updated in manifest with new ID.
    let deletes: Vec<SyncDeleteItem> = deletes
        .into_iter()
        .enumerate()
        .filter(|(idx, _)| !matched_delete_indices.contains(idx))
        .map(|(_, d)| d)
        .collect();

    // Keep all adds — even ID-changed ones need to be re-recorded with new ID
    // But actually, matched adds should also stay since the manifest needs updating.
    // The adds remain as-is; only the deletes are suppressed for matched pairs.

    // Unchanged: items in both sets
    let unchanged: Vec<String> = desired_items
        .iter()
        .filter(|i| current_ids.contains(i.jellyfin_id.as_str()))
        .map(|i| i.jellyfin_id.clone())
        .collect();

    SyncDelta {
        adds,
        deletes,
        unchanged,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::{DeviceManifest, SyncedItem};

    fn empty_manifest() -> DeviceManifest {
        DeviceManifest {
            device_id: "test-device".to_string(),
            name: Some("Test".to_string()),
            version: "1.0".to_string(),
            managed_paths: vec!["Music".to_string()],
            synced_items: vec![],
        }
    }

    fn make_synced_item(
        id: &str,
        name: &str,
        album: Option<&str>,
        artist: Option<&str>,
    ) -> SyncedItem {
        SyncedItem {
            jellyfin_id: id.to_string(),
            name: name.to_string(),
            album: album.map(|s| s.to_string()),
            artist: artist.map(|s| s.to_string()),
            local_path: format!("Music/{}/{}.flac", artist.unwrap_or("Unknown"), name),
            size_bytes: 10_000_000,
            synced_at: "2026-02-15T10:00:00Z".to_string(),
        }
    }

    fn make_desired(
        id: &str,
        name: &str,
        album: Option<&str>,
        artist: Option<&str>,
    ) -> DesiredItem {
        DesiredItem {
            jellyfin_id: id.to_string(),
            name: name.to_string(),
            album: album.map(|s| s.to_string()),
            artist: artist.map(|s| s.to_string()),
            size_bytes: 10_000_000,
        }
    }

    #[test]
    fn test_delta_empty_manifest() {
        let manifest = empty_manifest();
        let desired = vec![
            make_desired("a", "Track A", Some("Album"), Some("Artist")),
            make_desired("b", "Track B", Some("Album"), Some("Artist")),
        ];

        let delta = calculate_delta(&desired, &manifest);
        assert_eq!(delta.adds.len(), 2);
        assert_eq!(delta.deletes.len(), 0);
        assert_eq!(delta.unchanged.len(), 0);
    }

    #[test]
    fn test_delta_full_overlap() {
        let mut manifest = empty_manifest();
        manifest.synced_items = vec![
            make_synced_item("a", "Track A", Some("Album"), Some("Artist")),
            make_synced_item("b", "Track B", Some("Album"), Some("Artist")),
        ];

        let desired = vec![
            make_desired("a", "Track A", Some("Album"), Some("Artist")),
            make_desired("b", "Track B", Some("Album"), Some("Artist")),
        ];

        let delta = calculate_delta(&desired, &manifest);
        assert_eq!(delta.adds.len(), 0);
        assert_eq!(delta.deletes.len(), 0);
        assert_eq!(delta.unchanged.len(), 2);
    }

    #[test]
    fn test_delta_partial_overlap() {
        let mut manifest = empty_manifest();
        manifest.synced_items = vec![
            make_synced_item("a", "Track A", Some("Album"), Some("Artist")),
            make_synced_item("b", "Track B", Some("Album"), Some("Artist")),
        ];

        let desired = vec![
            make_desired("a", "Track A", Some("Album"), Some("Artist")),
            make_desired("c", "Track C", Some("Album"), Some("Artist")),
        ];

        let delta = calculate_delta(&desired, &manifest);
        assert_eq!(delta.adds.len(), 1);
        assert_eq!(delta.adds[0].jellyfin_id, "c");
        assert_eq!(delta.deletes.len(), 1);
        assert_eq!(delta.deletes[0].jellyfin_id, "b");
        assert_eq!(delta.unchanged.len(), 1);
        assert_eq!(delta.unchanged[0], "a");
    }

    #[test]
    fn test_delta_complete_replacement() {
        let mut manifest = empty_manifest();
        manifest.synced_items = vec![
            make_synced_item("a", "Track A", Some("Album"), Some("Artist")),
            make_synced_item("b", "Track B", Some("Album"), Some("Artist")),
        ];

        let desired = vec![
            make_desired("c", "Track C", Some("Album2"), Some("Artist2")),
            make_desired("d", "Track D", Some("Album2"), Some("Artist2")),
        ];

        let delta = calculate_delta(&desired, &manifest);
        assert_eq!(delta.adds.len(), 2);
        assert_eq!(delta.deletes.len(), 2);
        assert_eq!(delta.unchanged.len(), 0);
    }

    #[test]
    fn test_delta_server_id_change_detection() {
        let mut manifest = empty_manifest();
        manifest.synced_items = vec![make_synced_item(
            "old-id-1",
            "My Song",
            Some("My Album"),
            Some("My Artist"),
        )];

        // Same metadata but different Jellyfin ID (server re-scanned)
        let desired = vec![make_desired(
            "new-id-1",
            "My Song",
            Some("My Album"),
            Some("My Artist"),
        )];

        let delta = calculate_delta(&desired, &manifest);
        // The delete should be suppressed (ID change detected via metadata match)
        assert_eq!(delta.deletes.len(), 0);
        // The add remains so the manifest gets updated with the new ID
        assert_eq!(delta.adds.len(), 1);
        assert_eq!(delta.adds[0].jellyfin_id, "new-id-1");
        assert_eq!(delta.unchanged.len(), 0);
    }

    #[test]
    fn test_delta_id_change_case_insensitive() {
        let mut manifest = empty_manifest();
        manifest.synced_items = vec![make_synced_item(
            "old-id",
            "my song",
            Some("my album"),
            Some("my artist"),
        )];

        let desired = vec![make_desired(
            "new-id",
            "My Song",
            Some("My Album"),
            Some("My Artist"),
        )];

        let delta = calculate_delta(&desired, &manifest);
        assert_eq!(delta.deletes.len(), 0);
        assert_eq!(delta.adds.len(), 1);
    }
}
