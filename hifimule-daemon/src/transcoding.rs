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

/// Find a profile by ID. Returns None if not found or if the profile is passthrough (null payload).
pub fn find_device_profile(path: &Path, profile_id: &str) -> Result<Option<Value>> {
    let profiles = load_profiles(path)?;
    let entry = profiles.into_iter().find(|p| p.id == profile_id);
    Ok(entry.and_then(|e| e.device_profile))
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
    fn test_find_device_profile_known_id() {
        let (_dir, path) = write_temp_profiles(VALID_JSON);
        let result = find_device_profile(&path, "rockbox-mp3-320").unwrap();
        assert!(result.is_some());
        let val = result.unwrap();
        assert_eq!(val["Name"], "Test");
    }

    #[test]
    fn test_find_device_profile_passthrough_returns_none() {
        let (_dir, path) = write_temp_profiles(VALID_JSON);
        let result = find_device_profile(&path, "passthrough").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_find_device_profile_unknown_id() {
        let (_dir, path) = write_temp_profiles(VALID_JSON);
        let result = find_device_profile(&path, "does-not-exist").unwrap();
        assert!(result.is_none());
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
