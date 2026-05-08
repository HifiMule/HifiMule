use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

/// A single entry in device-profiles.json.
/// `device_profile` is `None` for the passthrough (no-transcode) profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceProfileEntry {
    pub id: String,
    pub name: String,
    pub description: String,
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

/// Seed the default device-profiles.json to `dest_path` if it does not already exist.
/// The default content is the embedded asset bytes.
pub fn ensure_profiles_file_exists(dest_path: &Path, default_bytes: &[u8]) -> Result<()> {
    if dest_path.exists() {
        return Ok(());
    }
    if let Some(parent) = dest_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| anyhow!("Failed to create profiles directory: {}", e))?;
    }
    std::fs::write(dest_path, default_bytes)
        .map_err(|e| anyhow!("Failed to write default device-profiles.json: {}", e))?;
    Ok(())
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
            {"id": "rockbox-mp3-320", "name": "Rockbox 320", "description": "MP3 320", "deviceProfile": {"Name": "Test"}}
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
    fn test_ensure_profiles_file_exists_does_not_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("device-profiles.json");
        std::fs::write(&path, b"original").unwrap();
        ensure_profiles_file_exists(&path, b"new_content").unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "original");
    }
}
