//! Versioned output preference, independent of the listening-session database.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{Read, Write},
    path::Path,
};

const MAX_FILE_BYTES: usize = 64 * 1024;
const MAX_STRING_BYTES: usize = 4 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OutputPreference {
    pub backend: String,
    pub stable_id: String,
    pub display_name: String,
    pub identity_properties: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlaybackConfig {
    pub schema_version: u32,
    pub output: Option<OutputPreference>,
}

impl Default for PlaybackConfig {
    fn default() -> Self {
        Self {
            schema_version: 1,
            output: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error("OUTPUT_CONFIG_INVALID")]
    Invalid,
    #[error("OUTPUT_PREFERENCE_SAVE_FAILED")]
    SaveFailed,
}

pub fn load(path: &Path) -> Result<PlaybackConfig, ConfigError> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(PlaybackConfig::default()),
        Err(_) => return Err(ConfigError::Invalid),
    };
    let mut bytes = Vec::new();
    file.take((MAX_FILE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| ConfigError::Invalid)?;
    if bytes.len() > MAX_FILE_BYTES {
        return Err(ConfigError::Invalid);
    }
    // Option fields otherwise accept omission; the persisted versioned contract is exact.
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| ConfigError::Invalid)?;
    if value.get("output").is_none() {
        return Err(ConfigError::Invalid);
    }
    let config: PlaybackConfig =
        serde_json::from_slice(&bytes).map_err(|_| ConfigError::Invalid)?;
    validate(&config)?;
    Ok(config)
}

fn validate(config: &PlaybackConfig) -> Result<(), ConfigError> {
    if config.schema_version != 1 {
        return Err(ConfigError::Invalid);
    }
    if let Some(output) = &config.output
        && (!matches!(output.backend.as_str(), "coreaudio" | "wasapi" | "pulse")
            || output.stable_id.is_empty()
            || output.display_name.is_empty()
            || output.identity_properties.keys().any(|key| {
                output.backend != "pulse"
                    || !matches!(
                        key.as_str(),
                        "device.serial"
                            | "device.bus_path"
                            | "device.bus"
                            | "device.vendor.id"
                            | "device.product.id"
                            | "device.profile.name"
                            | "device.string"
                            | "port"
                    )
            })
            || [&output.backend, &output.stable_id, &output.display_name]
                .into_iter()
                .chain(output.identity_properties.keys())
                .chain(output.identity_properties.values())
                .any(|s| s.len() > MAX_STRING_BYTES || s.contains('\0')))
    {
        return Err(ConfigError::Invalid);
    }
    Ok(())
}

/// Call only from the serialized preference worker. Successful return means the
/// replacement has committed; no caller may publish the requested intent earlier.
#[cfg(test)]
pub fn save(
    path: &Path,
    config: &PlaybackConfig,
    replace_invalid: bool,
) -> Result<(), ConfigError> {
    save_if_current(path, config, replace_invalid, || true).map(|_| ())
}

pub fn save_if_current(
    path: &Path,
    config: &PlaybackConfig,
    replace_invalid: bool,
    current: impl FnOnce() -> bool,
) -> Result<bool, ConfigError> {
    validate(config)?;
    let bytes = serde_json::to_vec(config).map_err(|_| ConfigError::Invalid)?;
    if bytes.len() > MAX_FILE_BYTES {
        return Err(ConfigError::Invalid);
    }
    let invalid = load(path).is_err();
    if invalid && !replace_invalid {
        return Err(ConfigError::Invalid);
    }
    let parent = path.parent().ok_or(ConfigError::SaveFailed)?;
    fs::create_dir_all(parent).map_err(|_| ConfigError::SaveFailed)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|_| ConfigError::SaveFailed)?;
    temp.write_all(&bytes)
        .and_then(|_| temp.as_file().sync_all())
        .map_err(|_| ConfigError::SaveFailed)?;
    if invalid {
        // Copy to a uniquely created archive before replacement. A failed copy,
        // flush or archive commit never removes or truncates the original.
        let mut original = File::open(path).map_err(|_| ConfigError::SaveFailed)?;
        let mut archive =
            tempfile::NamedTempFile::new_in(parent).map_err(|_| ConfigError::SaveFailed)?;
        std::io::copy(&mut original, &mut archive)
            .and_then(|_| archive.as_file().sync_all())
            .map_err(|_| ConfigError::SaveFailed)?;
        let archive_path = parent.join(format!("playback.invalid-{}.json", uuid::Uuid::new_v4()));
        archive
            .persist_noclobber(archive_path)
            .map_err(|_| ConfigError::SaveFailed)?;
        sync_directory(parent)?;
    }
    // tempfile uses rename on Unix and MoveFileExW(REPLACE_EXISTING) on Windows.
    // Never delete the destination first, even when replacing invalid evidence.
    if !current() {
        return Ok(false);
    }
    temp.persist(path).map_err(|_| ConfigError::SaveFailed)?;
    sync_directory(parent)?;
    Ok(true)
}

fn sync_directory(path: &Path) -> Result<(), ConfigError> {
    #[cfg(unix)]
    File::open(path)
        .and_then(|f| f.sync_all())
        .map_err(|_| ConfigError::SaveFailed)?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn selected(id: &str) -> PlaybackConfig {
        PlaybackConfig {
            schema_version: 1,
            output: Some(OutputPreference {
                backend: "coreaudio".into(),
                stable_id: id.into(),
                display_name: "Headphones".into(),
                identity_properties: BTreeMap::new(),
            }),
        }
    }

    #[test]
    fn rejects_unrecognized_identity_properties_without_writing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("playback.json");
        let mut config = selected("sink");
        config.output.as_mut().unwrap().backend = "pulse".into();
        config
            .output
            .as_mut()
            .unwrap()
            .identity_properties
            .insert("token".into(), "secret".into());
        assert_eq!(save(&path, &config, false), Err(ConfigError::Invalid));
        assert!(!path.exists());
    }

    #[test]
    fn superseded_commit_preserves_last_saved_choice_and_removes_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("playback.json");
        save(&path, &selected("a"), false).unwrap();
        assert_eq!(
            save_if_current(&path, &selected("b"), false, || false),
            Ok(false)
        );
        assert_eq!(load(&path), Ok(selected("a")));
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
        save(&path, &selected("c"), false).unwrap();
        assert_eq!(load(&path), Ok(selected("c")));
    }

    #[test]
    fn missing_is_unselected_and_does_not_create_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("playback.json");
        assert_eq!(load(&path), Ok(PlaybackConfig::default()));
        assert!(!path.exists());
    }

    #[test]
    fn round_trip_replaces_previous_preference() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("playback.json");
        save(&path, &selected("a"), false).unwrap();
        assert_eq!(load(&path), Ok(selected("a")));
        save(&path, &selected("b"), false).unwrap();
        assert_eq!(load(&path), Ok(selected("b")));
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn invalid_future_and_oversized_files_are_preserved_until_explicit_reset() {
        for bytes in [
            b"broken".to_vec(),
            br#"{"schemaVersion":2,"output":null}"#.to_vec(),
            vec![b' '; MAX_FILE_BYTES + 1],
        ] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("playback.json");
            fs::write(&path, &bytes).unwrap();
            assert_eq!(load(&path), Err(ConfigError::Invalid));
            assert_eq!(
                save(&path, &selected("a"), false),
                Err(ConfigError::Invalid)
            );
            assert_eq!(fs::read(&path).unwrap(), bytes);
            save(&path, &selected("a"), true).unwrap();
            assert_eq!(load(&path), Ok(selected("a")));
            let archives: Vec<_> = fs::read_dir(dir.path())
                .unwrap()
                .map(|e| e.unwrap().path())
                .filter(|p| p != &path)
                .collect();
            assert_eq!(archives.len(), 1);
            assert_eq!(fs::read(&archives[0]).unwrap(), bytes);
        }
    }

    #[test]
    fn rejects_unknown_fields_missing_fields_and_oversized_strings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("playback.json");
        for bytes in [
            br#"{"schemaVersion":1,"output":null,"url":"secret"}"#.to_vec(),
            br#"{"schemaVersion":1}"#.to_vec(),
            serde_json::to_vec(&selected(&"x".repeat(MAX_STRING_BYTES + 1))).unwrap(),
        ] {
            fs::write(&path, bytes).unwrap();
            assert_eq!(load(&path), Err(ConfigError::Invalid));
        }
    }

    #[test]
    fn invalid_write_leaves_last_commit_intact() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("playback.json");
        save(&path, &selected("a"), false).unwrap();
        assert!(save(&path, &selected(&"x".repeat(MAX_STRING_BYTES + 1)), false).is_err());
        assert_eq!(load(&path), Ok(selected("a")));
    }

    #[test]
    fn archive_failure_leaves_original_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("playback.json");
        fs::create_dir(&path).unwrap();
        assert!(save(&path, &selected("a"), true).is_err());
        assert!(path.is_dir());
    }
}
