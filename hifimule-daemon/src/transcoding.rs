pub mod local;
use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::Path;

/// A single entry in device-profiles.json.
/// `device_profile` is `None` for the passthrough (no-transcode) profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceProfileEntry {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(rename = "defaultMusicFolder", default)]
    pub default_music_folder: Option<String>,
    #[serde(rename = "defaultPlaylistFolder", default)]
    pub default_playlist_folder: Option<String>,
    #[serde(rename = "deviceProfile")]
    pub device_profile: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct ProfilesFile {
    profiles: Vec<DeviceProfileEntry>,
}

/// Load all profiles from `device-profiles.json` at the given path.
pub fn load_profiles(path: &Path) -> Result<Vec<DeviceProfileEntry>> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| anyhow!("Failed to read device-profiles.json: {}", e))?;
    let file: ProfilesFile = serde_json::from_str(&content)
        .map_err(|e| anyhow!("Failed to parse device-profiles.json: {}", e))?;
    Ok(file.profiles)
}

/// Seed or merge the default device-profiles.json into `dest_path`.
/// Existing customer values win; embedded defaults fill missing attributes and
/// append newly shipped profiles.
pub fn ensure_profiles_file_exists(dest_path: &Path, default_bytes: &[u8]) -> Result<()> {
    if let Some(parent) = dest_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| anyhow!("Failed to create profiles directory: {}", e))?;
    }
    if dest_path.exists() {
        let existing = std::fs::read_to_string(dest_path)
            .map_err(|e| anyhow!("Failed to read device-profiles.json: {}", e))?;
        let merged = merge_profiles_json(&existing, default_bytes)?;
        if merged != existing {
            std::fs::write(dest_path, merged)
                .map_err(|e| anyhow!("Failed to write merged device-profiles.json: {}", e))?;
        }
        return Ok(());
    }
    std::fs::write(dest_path, default_bytes)
        .map_err(|e| anyhow!("Failed to write default device-profiles.json: {}", e))?;
    Ok(())
}

fn merge_profiles_json(existing: &str, default_bytes: &[u8]) -> Result<String> {
    let mut existing_json: Value = serde_json::from_str(existing)
        .map_err(|e| anyhow!("Failed to parse device-profiles.json: {}", e))?;
    let default_json: Value = serde_json::from_slice(default_bytes)
        .map_err(|e| anyhow!("Failed to parse embedded device-profiles.json: {}", e))?;

    let changed = merge_profile_defaults(&mut existing_json, &default_json)?;
    if changed {
        serde_json::to_string_pretty(&existing_json)
            .map(|json| format!("{}\n", json))
            .map_err(|e| anyhow!("Failed to serialize merged device-profiles.json: {}", e))
    } else {
        Ok(existing.to_string())
    }
}

fn merge_profile_defaults(existing: &mut Value, defaults: &Value) -> Result<bool> {
    let existing_profiles = existing
        .get_mut("profiles")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| anyhow!("device-profiles.json must contain a profiles array"))?;
    let default_profiles = defaults
        .get("profiles")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("embedded device-profiles.json must contain a profiles array"))?;

    let mut changed = false;
    for default_profile in default_profiles {
        let Some(default_id) = default_profile.get("id").and_then(Value::as_str) else {
            continue;
        };
        match existing_profiles.iter_mut().find(|profile| {
            profile
                .get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| id == default_id)
        }) {
            Some(existing_profile) => {
                changed |= migrate_legacy_mp3_192(existing_profile, default_profile);
                changed |= migrate_legacy_garmin_m4b(existing_profile, default_profile);
                changed |= migrate_legacy_original_description(existing_profile, default_profile);
                changed |= merge_missing_values(existing_profile, default_profile);
            }
            None => {
                existing_profiles.push(default_profile.clone());
                changed = true;
            }
        }
    }

    Ok(changed)
}

/// Match the old description too, so later direct-play edits are not migrated again.
fn migrate_legacy_mp3_192(existing: &mut Value, defaults: &Value) -> bool {
    const LEGACY_DESCRIPTION: &str =
        "For devices with limited storage. Transcodes everything to MP3 192 kbps.";
    if existing["id"] != "rockbox-mp3-192" || existing["description"] != LEGACY_DESCRIPTION {
        return false;
    }
    let mut legacy = serde_json::json!({
        "Name":"HifiMule-Rockbox-192", "MaxStreamingBitrate":192000,
        "MusicStreamingTranscodingBitrate":192000, "MaxAudioBitDepth":16,
        "DirectPlayProfiles":[], "CodecProfiles":[],
        "TranscodingProfiles":[{"Container":"mp3","Type":"Audio","AudioCodec":"mp3",
            "Protocol":"http","EstimateContentLength":true,"EnableMpegtsM2TsMode":false}]
    });
    let with_depth = existing.get("deviceProfile") == Some(&legacy);
    legacy.as_object_mut().unwrap().remove("MaxAudioBitDepth");
    if !with_depth && existing.get("deviceProfile") != Some(&legacy) {
        return false;
    }
    existing["deviceProfile"]["DirectPlayProfiles"] = defaults["deviceProfile"]["DirectPlayProfiles"].clone();
    existing["description"] = defaults["description"].clone();
    true
}

fn migrate_legacy_original_description(existing: &mut Value, defaults: &Value) -> bool {
    if existing["id"] != "passthrough"
        || existing["description"] != "Download files as-is from Jellyfin without transcoding."
        || existing.get("deviceProfile") != Some(&Value::Null) {
        return false;
    }
    existing["description"] = defaults["description"].clone();
    true
}

fn migrate_legacy_garmin_m4b(existing: &mut Value, defaults: &Value) -> bool {
    const LEGACY_DESCRIPTION: &str = "For Garmin watches with music storage (Forerunner, Fenix, Venu, Vivoactive). Passes through MP3 and AAC/M4A directly; transcodes FLAC, OGG, and other formats to MP3 320 kbps.";
    if existing["id"] != "garmin-music" || existing["description"] != LEGACY_DESCRIPTION {
        return false;
    }
    let legacy = serde_json::json!({
        "Name":"HifiMule-Garmin", "MaxStreamingBitrate":320000,
        "MusicStreamingTranscodingBitrate":320000, "MaxAudioChannels":2, "MaxAudioSampleRate":48000,
        "DirectPlayProfiles":[{"Container":"mp3","Type":"Audio","AudioCodec":"mp3"},
            {"Container":"mp4","Type":"Audio","AudioCodec":"aac"}],
        "TranscodingProfiles":[{"Container":"mp3","Type":"Audio","AudioCodec":"mp3",
            "Protocol":"http","EstimateContentLength":true,"EnableMpegtsM2TsMode":false}],
        "CodecProfiles":[]
    });
    if existing.get("deviceProfile") != Some(&legacy) {
        return false;
    }
    existing["deviceProfile"]["DirectPlayProfiles"] = defaults["deviceProfile"]["DirectPlayProfiles"].clone();
    existing["description"] = defaults["description"].clone();
    true
}

fn merge_missing_values(existing: &mut Value, defaults: &Value) -> bool {
    let (Some(existing_object), Some(default_object)) =
        (existing.as_object_mut(), defaults.as_object())
    else {
        return false;
    };
    merge_missing_object_values(existing_object, default_object)
}

fn merge_missing_object_values(
    existing_object: &mut Map<String, Value>,
    default_object: &Map<String, Value>,
) -> bool {
    let mut changed = false;
    for (key, default_value) in default_object {
        match existing_object.get_mut(key) {
            Some(existing_value) if existing_value.is_object() && default_value.is_object() => {
                changed |= merge_missing_values(existing_value, default_value);
            }
            Some(_) => {}
            None => {
                existing_object.insert(key.clone(), default_value.clone());
                changed = true;
            }
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const EMBEDDED: &[u8] = include_bytes!("../assets/device-profiles.json");

    fn mp3_192_profile(value: &mut Value) -> &mut Value {
        value["profiles"].as_array_mut().unwrap().iter_mut()
            .find(|entry| entry["id"] == "rockbox-mp3-192").unwrap()
    }

    #[test]
    fn m4b_original_description_migration_preserves_custom_profiles() {
        for (description, device_profile, updated) in [
            ("Download files as-is from Jellyfin without transcoding.", Value::Null, true),
            ("My originals", Value::Null, false),
            ("Download files as-is from Jellyfin without transcoding.", serde_json::json!({"DirectPlayProfiles":[]}), false),
        ] {
            let existing = serde_json::json!({"profiles":[{"id":"passthrough","name":"My choice",
                "description":description,"deviceProfile":device_profile,"defaultMusicFolder":"CustomMusic"}]}).to_string();
            let merged = merge_profiles_json(&existing, EMBEDDED).unwrap();
            let value: Value = serde_json::from_str(&merged).unwrap();
            let entry = &value["profiles"][0];
            assert_eq!(entry["name"], "My choice");
            assert_eq!(entry["defaultMusicFolder"], "CustomMusic");
            assert_eq!(entry["description"] != description, updated);
            assert_eq!(merge_profiles_json(&merged, EMBEDDED).unwrap(), merged);
        }
    }

    #[test]
    fn m4b_garmin_stock_migration_preserves_customizations_and_is_idempotent() {
        let defaults: Value = serde_json::from_slice(EMBEDDED).unwrap();
        let current = defaults["profiles"].as_array().unwrap().iter().find(|p| p["id"] == "garmin-music").unwrap();
        let mut legacy = current.clone();
        legacy["description"] = "For Garmin watches with music storage (Forerunner, Fenix, Venu, Vivoactive). Passes through MP3 and AAC/M4A directly; transcodes FLAC, OGG, and other formats to MP3 320 kbps.".into();
        legacy["deviceProfile"]["DirectPlayProfiles"] = serde_json::json!([
            {"Container":"mp3","Type":"Audio","AudioCodec":"mp3"},
            {"Container":"mp4","Type":"Audio","AudioCodec":"aac"}
        ]);
        legacy["name"] = "My watch".into();
        legacy["defaultMusicFolder"] = "CustomMusic".into();
        for edit in [None, Some("DirectPlayProfiles"), Some("MaxAudioChannels"), Some("customFlag")] {
            let mut existing = legacy.clone();
            if let Some(key) = edit { existing["deviceProfile"][key] = serde_json::json!([]); }
            let document = serde_json::json!({"profiles":[existing.clone()]}).to_string();
            let merged = merge_profiles_json(&document, EMBEDDED).unwrap();
            let value: Value = serde_json::from_str(&merged).unwrap();
            let profiles = value["profiles"].as_array().unwrap();
            let garmin = profiles.iter().find(|p| p["id"] == "garmin-music").unwrap();
            assert_eq!(garmin["name"], "My watch");
            assert_eq!(garmin["defaultMusicFolder"], "CustomMusic");
            if edit.is_none() {
                assert_eq!(garmin["deviceProfile"], current["deviceProfile"]);
                assert!(garmin["deviceProfile"]["DirectPlayProfiles"].as_array().unwrap().iter().any(|p| p["Container"] == "m4b"));
            } else { assert_eq!(garmin["deviceProfile"], existing["deviceProfile"]); }
            for id in ["modern-dap-lossless-m4b", "rockbox-audiobooks-m4b", "audiobooks-m4b-mp3", "pocketbook-audiobooks", "android-voice-audiobooks"] {
                assert_eq!(profiles.iter().filter(|p| p["id"] == id).count(), 1);
            }
            assert_eq!(merge_profiles_json(&merged, EMBEDDED).unwrap(), merged);
        }
        // A user removes M4B after upgrading: the updated description prevents re-migration.
        let mut modified = current.clone();
        modified["deviceProfile"]["DirectPlayProfiles"] = legacy["deviceProfile"]["DirectPlayProfiles"].clone();
        let merged: Value = serde_json::from_str(&merge_profiles_json(&serde_json::json!({"profiles":[modified.clone()]}).to_string(), EMBEDDED).unwrap()).unwrap();
        assert_eq!(merged["profiles"][0], modified);
    }

    #[test]
    fn mp3_direct_copy_preset_seed_and_legacy_migration() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("device-profiles.json");
        ensure_profiles_file_exists(&path, EMBEDDED).unwrap();
        let mut seeded: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let current = mp3_192_profile(&mut seeded).clone();
        assert_eq!(current["deviceProfile"]["DirectPlayProfiles"], serde_json::json!([
            {"Container":"mp3","Type":"Audio","AudioCodec":"mp3"}
        ]));
        for depth in [true, false] {
            let mut existing = seeded.clone();
            let legacy = mp3_192_profile(&mut existing);
            legacy["name"] = "My portable".into();
            legacy["defaultMusicFolder"] = "CustomMusic".into();
            legacy["description"] =
                "For devices with limited storage. Transcodes everything to MP3 192 kbps.".into();
            legacy["deviceProfile"]["DirectPlayProfiles"] = serde_json::json!([]);
            if !depth { legacy["deviceProfile"].as_object_mut().unwrap().remove("MaxAudioBitDepth"); }
            let merged = merge_profiles_json(&existing.to_string(), EMBEDDED).unwrap();
            let mut merged_json: Value = serde_json::from_str(&merged).unwrap();
            let repaired = mp3_192_profile(&mut merged_json);
            assert_eq!(repaired["deviceProfile"], current["deviceProfile"]);
            assert_eq!(repaired["name"], "My portable");
            assert_eq!(repaired["defaultMusicFolder"], "CustomMusic");
            assert_eq!(merge_profiles_json(&merged, EMBEDDED).unwrap(), merged);
        }
    }

    #[test]
    fn mp3_direct_copy_migration_preserves_later_direct_play_edit() {
        let mut existing: Value = serde_json::from_slice(EMBEDDED).unwrap();
        let entry = mp3_192_profile(&mut existing);
        entry["deviceProfile"]["DirectPlayProfiles"] = serde_json::json!([]);
        let expected = entry.clone();
        let mut merged: Value = serde_json::from_str(
            &merge_profiles_json(&existing.to_string(), EMBEDDED).unwrap(),
        ).unwrap();
        assert_eq!(*mp3_192_profile(&mut merged), expected);
    }

    #[test]
    fn mp3_direct_copy_migration_preserves_custom_force_transcode() {
        let mut current: Value = serde_json::from_slice(EMBEDDED).unwrap();
        for edit in ["Name", "MusicStreamingTranscodingBitrate", "customFlag"] {
            let mut existing = current.clone();
            let entry = mp3_192_profile(&mut existing);
            entry["deviceProfile"]["DirectPlayProfiles"] = serde_json::json!([]);
            entry["deviceProfile"][edit] = serde_json::json!(123);
            let expected = entry.clone();
            let mut merged: Value = serde_json::from_str(&merge_profiles_json(&existing.to_string(), EMBEDDED).unwrap()).unwrap();
            assert_eq!(*mp3_192_profile(&mut merged), expected);
        }
        let preset = mp3_192_profile(&mut current);
        preset["id"] = "custom-force".into();
        preset["deviceProfile"]["DirectPlayProfiles"] = serde_json::json!([]);
        let expected = preset.clone();
        let merged: Value = serde_json::from_str(&merge_profiles_json(&current.to_string(), EMBEDDED).unwrap()).unwrap();
        assert!(merged["profiles"].as_array().unwrap().contains(&expected));
    }

    fn write_temp_profiles(content: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("device-profiles.json");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        (dir, path)
    }

    const VALID_JSON: &str = r#"{
        "profiles": [
            {"id": "passthrough", "name": "No Transcoding", "description": "Pass.", "deviceProfile": null},
            {"id": "rockbox-mp3-320", "name": "Rockbox 320", "description": "MP3 320", "defaultMusicFolder": "Music", "defaultPlaylistFolder": "Playlists", "deviceProfile": {"Name": "Test"}}
        ]
    }"#;

    #[test]
    fn test_load_profiles_valid() {
        let (_dir, path) = write_temp_profiles(VALID_JSON);
        let profiles = load_profiles(&path).unwrap();
        assert_eq!(profiles.len(), 2);
        assert_eq!(profiles[0].id, "passthrough");
        assert!(profiles[0].device_profile.is_none());
        assert_eq!(profiles[1].id, "rockbox-mp3-320");
        assert_eq!(profiles[1].default_music_folder.as_deref(), Some("Music"));
        assert_eq!(
            profiles[1].default_playlist_folder.as_deref(),
            Some("Playlists")
        );
        assert!(profiles[1].device_profile.is_some());
    }

    #[test]
    fn test_load_profiles_malformed() {
        let (_dir, path) = write_temp_profiles("not json");
        let result = load_profiles(&path);
        assert!(result.is_err());
    }

    #[test]
    fn test_load_profiles_missing_file() {
        let path = std::path::Path::new("/nonexistent/device-profiles.json");
        let result = load_profiles(path);
        assert!(result.is_err());
    }

    #[test]
    fn test_ensure_profiles_file_exists_creates_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("device-profiles.json");
        assert!(!path.exists());
        ensure_profiles_file_exists(&path, b"hello").unwrap();
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "hello");
    }

    #[test]
    fn test_ensure_profiles_file_exists_preserves_existing_values() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("device-profiles.json");
        std::fs::write(
            &path,
            r#"{
                "profiles": [
                    {"id": "passthrough", "name": "Custom Name", "description": "Custom description", "deviceProfile": null}
                ]
            }"#,
        )
        .unwrap();

        ensure_profiles_file_exists(&path, VALID_JSON.as_bytes()).unwrap();

        let merged: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let passthrough = merged["profiles"]
            .as_array()
            .unwrap()
            .iter()
            .find(|profile| profile["id"] == "passthrough")
            .unwrap();
        assert_eq!(passthrough["name"], "Custom Name");
        assert_eq!(passthrough["description"], "Custom description");
    }

    #[test]
    fn test_ensure_profiles_file_exists_merges_new_profiles_and_missing_attributes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("device-profiles.json");
        std::fs::write(
            &path,
            r#"{
                "profiles": [
                    {
                        "id": "rockbox-mp3-320",
                        "name": "Custom Rockbox",
                        "description": "Customer edit",
                        "customFlag": true,
                        "deviceProfile": {
                            "Name": "Custom-Device",
                            "DirectPlayProfiles": []
                        }
                    }
                ]
            }"#,
        )
        .unwrap();

        let defaults = br#"{
            "profiles": [
                {
                    "id": "rockbox-mp3-320",
                    "name": "Default Rockbox",
                    "description": "Default description",
                    "defaultMusicFolder": "Music",
                    "deviceProfile": {
                        "Name": "Default-Device",
                        "MaxStreamingBitrate": 320000,
                        "TranscodingProfiles": []
                    }
                },
                {
                    "id": "new-profile",
                    "name": "New Profile",
                    "description": "New default",
                    "deviceProfile": null
                }
            ]
        }"#;

        ensure_profiles_file_exists(&path, defaults).unwrap();

        let merged: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let profiles = merged["profiles"].as_array().unwrap();
        assert_eq!(profiles.len(), 2);

        let rockbox = profiles
            .iter()
            .find(|profile| profile["id"] == "rockbox-mp3-320")
            .unwrap();
        assert_eq!(rockbox["name"], "Custom Rockbox");
        assert_eq!(rockbox["description"], "Customer edit");
        assert_eq!(rockbox["customFlag"], true);
        assert_eq!(rockbox["defaultMusicFolder"], "Music");
        assert_eq!(rockbox["deviceProfile"]["Name"], "Custom-Device");
        assert_eq!(rockbox["deviceProfile"]["MaxStreamingBitrate"], 320000);
        assert!(rockbox["deviceProfile"]["TranscodingProfiles"].is_array());

        assert!(
            profiles
                .iter()
                .any(|profile| profile["id"] == "new-profile")
        );
    }

    #[test]
    fn test_ensure_profiles_file_exists_leaves_current_file_when_defaults_have_no_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("device-profiles.json");
        std::fs::write(&path, VALID_JSON).unwrap();
        ensure_profiles_file_exists(&path, VALID_JSON.as_bytes()).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, VALID_JSON);
    }
}
