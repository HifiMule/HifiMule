pub mod local;
use anyhow::{Result, anyhow, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::{io::Write, path::Path, sync::Mutex};

// All profile read/modify/write transactions share this lock, including startup reconciliation.
static STORE: Mutex<()> = Mutex::new(());
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceProfileEntry {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub ownership: String,
    #[serde(default)]
    pub revision: u64,
    #[serde(rename = "defaultMusicFolder", default)]
    pub default_music_folder: Option<String>,
    #[serde(rename = "defaultPlaylistFolder", default)]
    pub default_playlist_folder: Option<String>,
    #[serde(rename = "defaultAudiobookFolder", default)]
    pub default_audiobook_folder: Option<String>,
    #[serde(rename = "defaultPodcastFolder", default)]
    pub default_podcast_folder: Option<String>,
    #[serde(rename = "defaultFolderArtwork", default)]
    pub default_folder_artwork: bool,
    #[serde(rename = "deviceProfile")]
    pub device_profile: Option<Value>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
fn read_document(path: &Path) -> Result<Value> {
    let doc: Value = serde_json::from_slice(&std::fs::read(path)?).map_err(|e| {
        anyhow!("Cannot read device-profiles.json; restore or repair the file: {e}")
    })?;
    ensure!(
        doc.is_object(),
        "Cannot read device-profiles.json; repair the document object"
    );
    if let Some(version) = doc.get("profileStoreVersion") {
        ensure!(
            version.as_u64() == Some(1),
            "Unsupported device-profiles.json storage version; use the matching HifiMule version or restore a backup"
        );
    }
    for key in ["systemProfileIds", "profileAliases", "legacyAudioBaselines"] {
        if let Some(value) = doc.get(key) {
            let map = value.as_object().ok_or_else(||anyhow!("Cannot read device-profiles.json; repair {key}: expected an object of string values"))?;
            ensure!(
                map.values()
                    .all(|value| value.as_str().is_some_and(|value| !value.is_empty())),
                "Cannot read device-profiles.json; repair {key}: expected nonempty string values"
            );
        }
    }
    let profiles = doc["profiles"]
        .as_array()
        .ok_or_else(|| anyhow!("device-profiles.json requires a profiles array"))?;
    let mut ids = std::collections::HashSet::new();
    for profile in profiles {
        let entry: DeviceProfileEntry = serde_json::from_value(profile.clone())?;
        ensure!(
            !entry.id.is_empty() && ids.insert(entry.id),
            "Duplicate or empty profile identity"
        );
    }
    Ok(doc)
}
fn atomic_write(path: &Path, doc: &Value) -> Result<()> {
    let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap_or(Path::new(".")))?;
    serde_json::to_writer_pretty(&mut file, doc)?;
    file.write_all(b"\n")?;
    file.as_file().sync_all()?;
    file.persist(path)
        .map_err(|e| anyhow!("Cannot save device profiles: {}", e.error))?;
    Ok(())
}
pub fn audio_fingerprint(profile: Option<&Value>) -> String {
    let mut value = profile.cloned().unwrap_or(Value::Null);
    if let Some(object) = value.as_object_mut() {
        object.remove("Name");
    }
    Sha256::digest(value.to_string().as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
pub fn load_profiles(path: &Path) -> Result<Vec<DeviceProfileEntry>> {
    serde_json::from_value(read_document(path)?["profiles"].clone()).map_err(Into::into)
}
pub fn aliases_for(path: &Path, id: &str) -> Vec<String> {
    read_document(path)
        .ok()
        .and_then(|d| d["profileAliases"].as_object().cloned())
        .map(|aliases| {
            aliases
                .into_iter()
                .filter_map(|(alias, value)| (value.as_str() == Some(id)).then_some(alias))
                .collect()
        })
        .unwrap_or_default()
}
pub fn resolve_profile(path: &Path, id: &str) -> Result<DeviceProfileEntry> {
    let doc = read_document(path)?;
    let resolved = doc["profileAliases"][id].as_str().unwrap_or(id);
    doc["profiles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == resolved)
        .cloned()
        .ok_or_else(|| anyhow!("Profile {id} not found"))
        .and_then(|p| serde_json::from_value(p).map_err(Into::into))
}
pub fn legacy_fingerprint(path: &Path, id: Option<&str>) -> Option<String> {
    if id.is_none() || id == Some("passthrough") {
        return Some(audio_fingerprint(None));
    }
    read_document(path).ok()?["legacyAudioBaselines"][id?]
        .as_str()
        .map(str::to_owned)
}
pub fn ensure_profiles_file_exists(path: &Path, default_bytes: &[u8]) -> Result<()> {
    let _lock = STORE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let existed = path.exists();
    let mut doc = if existed {
        read_document(path)?
    } else {
        json!({"profiles":[]})
    };
    let before = doc.clone();
    let defaults: Value = serde_json::from_slice(default_bytes)?;
    let defaults = defaults["profiles"]
        .as_array()
        .ok_or_else(|| anyhow!("Embedded profiles invalid"))?;
    ensure!(
        doc.get("profileStoreVersion").is_none() || doc["profileStoreVersion"] == 1,
        "This device-profiles.json uses a newer storage version; open it with the matching HifiMule version"
    );
    let first = doc["profileStoreVersion"] != 1;
    let mut profiles = doc["profiles"].as_array().unwrap().clone();
    if first {
        let mut baselines = Map::new();
        let mut aliases = Map::new();
        for p in &mut profiles {
            let id = p["id"].as_str().unwrap().to_string();
            baselines.insert(
                id.clone(),
                json!(audio_fingerprint(
                    p.get("deviceProfile").filter(|v| !v.is_null())
                )),
            );
            // Only exact shipped entries are safe to replace; all uncertain data remains custom.
            let exact = defaults.iter().any(|d| {
                let mut d = d.clone();
                d.as_object_mut().unwrap().remove("ownership");
                let mut old = p.clone();
                old.as_object_mut().unwrap().remove("ownership");
                // Older bundled rows predate this creation-only default. Explicit
                // values still participate in comparison to preserve custom choices.
                if old.get("defaultFolderArtwork").is_none() {
                    d.as_object_mut().unwrap().remove("defaultFolderArtwork");
                }
                old == d
            });
            if exact {
                aliases.insert(
                    id.clone(),
                    json!(if id == "passthrough" {
                        id.clone()
                    } else {
                        format!("system:{id}")
                    }),
                );
                p["ownership"] = json!("legacy-stock");
            } else {
                p["ownership"] = json!("custom");
                p["revision"] = json!(1);
            }
        }
        profiles.retain(|p| p["ownership"] != "legacy-stock");
        // passthrough is a reserved identity even when an old hand-edited row used it.
        for p in &mut profiles {
            if p["id"] == "passthrough" {
                p["id"] = json!(format!("custom:{}", uuid::Uuid::new_v4()));
            }
        }
        doc["legacyAudioBaselines"] = json!(baselines);
        doc["profileAliases"] = json!(aliases);
        doc["systemProfileIds"] = json!({});
        doc["profileStoreVersion"] = json!(1);
    }
    for bundled in defaults {
        let original = bundled["id"]
            .as_str()
            .ok_or_else(|| anyhow!("Embedded profile identity missing"))?;
        let preferred = if original == "passthrough" {
            original.to_string()
        } else {
            format!("system:{original}")
        };
        let id = doc["systemProfileIds"][original]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| {
                if profiles
                    .iter()
                    .any(|p| p["id"] == preferred && p["ownership"] != "system")
                {
                    format!("system:{original}:{}", uuid::Uuid::new_v4())
                } else {
                    preferred
                }
            });
        doc["systemProfileIds"][original] = json!(id);
        if first && doc["profileAliases"].get(original).is_some() {
            doc["legacyAudioBaselines"][&id] = doc["legacyAudioBaselines"][original].clone();
        }
        // Only aliases installed for exact stock entries can follow the bundled system.
        if doc["profileAliases"].get(original).is_some() {
            doc["profileAliases"][original] = json!(id);
        }
        let mut system = bundled.clone();
        system["id"] = json!(id);
        system["ownership"] = json!("system");
        let pos = profiles
            .iter()
            .position(|p| p["id"] == id && p["ownership"] == "system");
        let revision = pos
            .and_then(|i| profiles[i]["revision"].as_u64())
            .unwrap_or(1);
        system["revision"] = json!(revision);
        if let Some(i) = pos {
            if profiles[i] != system {
                system["revision"] = json!(revision + 1);
            }
            profiles[i] = system;
        } else {
            profiles.push(system);
        }
    }
    doc["profiles"] = json!(profiles);
    if before != doc {
        if existed && first {
            let backup = path.with_extension("json.pre-profiles-v1.bak");
            if !backup.exists() {
                let mut out = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(backup)?;
                out.write_all(&std::fs::read(path)?)?;
                out.sync_all()?;
            }
        }
        atomic_write(path, &doc)?;
    }
    Ok(())
}

pub fn validate_entry(
    entry: &DeviceProfileEntry,
    previous: Option<&DeviceProfileEntry>,
) -> Result<()> {
    ensure!(
        !entry.name.trim().is_empty() && entry.name.chars().count() <= 120,
        "Profile name must contain 1–120 characters"
    );
    ensure!(
        entry.description.chars().count() <= 2000,
        "Description is too long"
    );
    for folder in [
        &entry.default_music_folder,
        &entry.default_playlist_folder,
        &entry.default_audiobook_folder,
        &entry.default_podcast_folder,
    ]
    .into_iter()
    .flatten()
    {
        ensure!(
            !folder.contains(['\\', ':', '\0'])
                && !folder.starts_with('/')
                && !folder.split('/').any(|s| s == ".." || s == "."),
            "Folders must be relative device paths without . or .."
        );
    }
    if let Some(profile) = &entry.device_profile {
        // Existing unsupported rules remain editable as metadata; never silently drop them.
        if previous.is_some_and(|old| old.device_profile.as_ref() == Some(profile)) {
            return Ok(());
        }
        ensure!(profile.is_object(), "Device capabilities must be an object");
        local::validate_numeric_configuration(profile)?;
        local::validate_profile(profile)?;
        let direct = profile["DirectPlayProfiles"]
            .as_array()
            .ok_or_else(|| anyhow!("DirectPlayProfiles must be an array"))?;
        for rule in direct {
            ensure!(
                rule["Type"].as_str() == Some("Audio"),
                "Direct play rules must describe audio"
            );
            let containers = rule["Container"]
                .as_str()
                .ok_or_else(|| anyhow!("Missing direct play container"))?;
            let codecs = rule["AudioCodec"]
                .as_str()
                .ok_or_else(|| anyhow!("Missing direct play codec"))?;
            for container in containers.split(',') {
                for codec in codecs.split(',') {
                    let valid = match container.trim().to_ascii_lowercase().as_str() {
                        "mp3" => codec == "mp3",
                        "mp4" | "m4a" => matches!(codec, "aac" | "alac"),
                        "m4b" => codec == "aac",
                        "flac" => codec == "flac",
                        "ogg" => matches!(codec, "vorbis" | "opus" | "flac"),
                        "opus" => codec == "opus",
                        "wav" => {
                            matches!(codec, "pcm_s16le" | "pcm_s24le" | "pcm_s32le" | "pcm_f32le")
                        }
                        _ => false,
                    };
                    ensure!(
                        valid,
                        "Unsupported direct play codec/container pair: {codec}/{container}"
                    );
                }
            }
        }
        ensure!(
            profile["TranscodingProfiles"]
                .as_array()
                .is_some_and(|p| p.len() == 1),
            "Choose one audio conversion target"
        );
        ensure!(
            profile["CodecProfiles"].as_array().is_some(),
            "CodecProfiles must be an array"
        );
    }
    Ok(())
}
/// Expected revision is checked under the same lock as the atomic replacement.
pub fn save_profile(
    path: &Path,
    mut entry: DeviceProfileEntry,
    expected: Option<u64>,
    source: Option<&str>,
) -> Result<DeviceProfileEntry> {
    let _lock = STORE.lock().unwrap_or_else(|e| e.into_inner());
    let mut doc = read_document(path)?;
    let profiles = doc["profiles"].as_array_mut().unwrap();
    let source_entry = source
        .map(|source| {
            profiles
                .iter()
                .find(|p| p["id"] == source)
                .cloned()
                .ok_or_else(|| anyhow!("Source profile no longer exists"))
                .and_then(|p| serde_json::from_value::<DeviceProfileEntry>(p).map_err(Into::into))
        })
        .transpose()?;
    let pos = if expected.is_some() {
        profiles.iter().position(|p| p["id"] == entry.id)
    } else {
        None
    };
    let previous = pos
        .map(|i| serde_json::from_value::<DeviceProfileEntry>(profiles[i].clone()))
        .transpose()?;
    if let Some(expected) = expected {
        let previous = previous
            .as_ref()
            .ok_or_else(|| anyhow!("Profile no longer exists"))?;
        ensure!(
            previous.ownership != "system" && previous.id != "passthrough",
            "System profiles are read-only; clone to customize"
        );
        ensure!(
            previous.revision == expected,
            "Profile changed elsewhere; reload before saving"
        );
    } else {
        entry.id = format!("custom:{}", uuid::Uuid::new_v4());
    }
    validate_entry(&entry, source_entry.as_ref().or(previous.as_ref()))?;
    entry.ownership = "custom".into();
    entry.revision = previous.as_ref().map_or(1, |p| p.revision + 1);
    // Preserve future top-level fields even when an older client edits a profile.
    if let Some(old) = previous {
        for (k, v) in old.extra {
            entry.extra.entry(k).or_insert(v);
        }
    }
    let value = serde_json::to_value(&entry)?;
    if let Some(i) = pos {
        profiles[i] = value;
    } else {
        profiles.push(value);
    }
    atomic_write(path, &doc)?;
    Ok(entry)
}

#[cfg(test)]
mod tests {
    use super::*;
    const DEFAULTS: &[u8] = include_bytes!("../assets/device-profiles.json");
    #[test]
    fn migration_preserves_custom_collisions_and_unknown_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("device-profiles.json");
        let defaults: Value = serde_json::from_slice(DEFAULTS).unwrap();
        let stock = defaults["profiles"][1].clone();
        let old = json!({"future":true,"profiles":[stock,{"id":"system:garmin-music","name":"Mine","description":"","deviceProfile":null,"future":42}]});
        std::fs::write(&path, old.to_string()).unwrap();
        ensure_profiles_file_exists(&path, DEFAULTS).unwrap();
        let doc = read_document(&path).unwrap();
        assert_eq!(doc["future"], true);
        assert_eq!(doc["profiles"][0]["future"], 42);
        assert_eq!(
            resolve_profile(&path, "rockbox-mp3-320").unwrap().ownership,
            "system"
        );
        assert_ne!(
            doc["systemProfileIds"]["garmin-music"],
            "system:garmin-music"
        );
        assert!(path.with_extension("json.pre-profiles-v1.bak").exists());
        let bytes = std::fs::read(&path).unwrap();
        ensure_profiles_file_exists(&path, DEFAULTS).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
    #[test]
    fn revisions_immutability_clone_and_failed_validation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles.json");
        ensure_profiles_file_exists(&path, DEFAULTS).unwrap();
        let system = load_profiles(&path).unwrap().remove(0);
        assert!(save_profile(&path, system.clone(), Some(system.revision), None).is_err());
        let mut cloned = save_profile(&path, system.clone(), None, Some(&system.id)).unwrap();
        assert_eq!(cloned.ownership, "custom");
        assert_ne!(cloned.id, system.id);
        let old = cloned.clone();
        cloned.name = "Edited".into();
        let saved = save_profile(&path, cloned, Some(1), None).unwrap();
        assert_eq!(saved.revision, 2);
        assert!(save_profile(&path, old, Some(1), None).is_err());
        let before = std::fs::read(&path).unwrap();
        let mut invalid = saved;
        invalid.name = " ".into();
        assert!(save_profile(&path, invalid, Some(2), None).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
    #[test]
    fn fingerprints_ignore_metadata() {
        assert_eq!(
            audio_fingerprint(Some(&json!({"Name":"A","MaxAudioChannels":2}))),
            audio_fingerprint(Some(&json!({"Name":"B","MaxAudioChannels":2})))
        );
        assert_ne!(audio_fingerprint(None), audio_fingerprint(Some(&json!({}))));
    }
    #[test]
    fn artwork_defaults_migrate_old_stock_and_preserve_explicit_custom_choices() {
        let defaults: Value = serde_json::from_slice(DEFAULTS).unwrap();
        for explicit in [None, Some(false)] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("profiles.json");
            let mut old = defaults["profiles"][1].clone();
            old.as_object_mut().unwrap().remove("defaultFolderArtwork");
            if let Some(value) = explicit {
                old["defaultFolderArtwork"] = json!(value);
            }
            std::fs::write(&path, json!({"profiles":[old]}).to_string()).unwrap();
            ensure_profiles_file_exists(&path, DEFAULTS).unwrap();
            let resolved = resolve_profile(&path, "rockbox-mp3-320").unwrap();
            assert_eq!(
                resolved.ownership,
                if explicit.is_none() { "system" } else { "custom" }
            );
            assert_eq!(resolved.default_folder_artwork, explicit.is_none());
            let system = resolve_profile(&path, "system:rockbox-mp3-320").unwrap();
            assert!(system.default_folder_artwork);
        }
    }
    #[test]
    fn artwork_defaults_persist_clone_and_refresh_without_audio_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles.json");
        ensure_profiles_file_exists(&path, DEFAULTS).unwrap();
        let profiles = load_profiles(&path).unwrap();
        for entry in &profiles {
            assert_eq!(
                entry.default_folder_artwork,
                entry.id.starts_with("system:rockbox-"),
                "{}",
                entry.id
            );
        }
        let system = resolve_profile(&path, "system:rockbox-mp3-320").unwrap();
        let cloned = save_profile(&path, system.clone(), None, Some(&system.id)).unwrap();
        assert!(
            resolve_profile(&path, &cloned.id)
                .unwrap()
                .default_folder_artwork
        );
        let mut edited = cloned.clone();
        edited.default_folder_artwork = false;
        let saved = save_profile(&path, edited, Some(cloned.revision), None).unwrap();
        assert!(
            !resolve_profile(&path, &saved.id)
                .unwrap()
                .default_folder_artwork
        );
        assert_eq!(
            audio_fingerprint(saved.device_profile.as_ref()),
            audio_fingerprint(system.device_profile.as_ref())
        );
        let mut draft = saved.clone();
        draft.default_folder_artwork = true;
        let created = save_profile(&path, draft, None, None).unwrap();
        assert!(
            resolve_profile(&path, &created.id)
                .unwrap()
                .default_folder_artwork
        );
        let before = read_document(&path).unwrap();
        let mut defaults: Value = serde_json::from_slice(DEFAULTS).unwrap();
        defaults["profiles"][1]["defaultFolderArtwork"] = json!(false);
        ensure_profiles_file_exists(&path, defaults.to_string().as_bytes()).unwrap();
        assert!(
            !resolve_profile(&path, &system.id)
                .unwrap()
                .default_folder_artwork
        );
        assert!(
            resolve_profile(&path, &created.id)
                .unwrap()
                .default_folder_artwork
        );
        assert_eq!(
            before["legacyAudioBaselines"],
            read_document(&path).unwrap()["legacyAudioBaselines"]
        );
    }
    #[test]
    fn media_folder_defaults_persist_clone_clear_and_validate_without_audio_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles.json");
        ensure_profiles_file_exists(&path, DEFAULTS).unwrap();
        let system = load_profiles(&path).unwrap().remove(0);
        assert!(system.default_audiobook_folder.is_none());
        assert!(system.default_podcast_folder.is_none());
        let mut draft = system.clone();
        draft.default_audiobook_folder = Some("Books/Authors".into());
        draft.default_podcast_folder = Some("Podcasts".into());
        let created = save_profile(&path, draft, None, None).unwrap();
        let loaded = resolve_profile(&path, &created.id).unwrap();
        assert_eq!(
            loaded.default_audiobook_folder.as_deref(),
            Some("Books/Authors")
        );
        assert_eq!(loaded.default_podcast_folder.as_deref(), Some("Podcasts"));
        let clone = save_profile(&path, loaded.clone(), None, Some(&loaded.id)).unwrap();
        assert_eq!(
            clone.default_audiobook_folder,
            loaded.default_audiobook_folder
        );
        assert_eq!(clone.default_podcast_folder, loaded.default_podcast_folder);
        assert_eq!(
            audio_fingerprint(clone.device_profile.as_ref()),
            audio_fingerprint(system.device_profile.as_ref())
        );
        for field in ["defaultAudiobookFolder", "defaultPodcastFolder"] {
            for invalid in [
                "../Books",
                "/Shows",
                "C:/Books",
                "Shows\\Episodes",
                "Books/./Author",
                "Books\0Bad",
            ] {
                let before = std::fs::read(&path).unwrap();
                let mut value = serde_json::to_value(&loaded).unwrap();
                value[field] = json!(invalid);
                let invalid = serde_json::from_value(value).unwrap();
                assert!(save_profile(&path, invalid, Some(loaded.revision), None).is_err());
                assert_eq!(std::fs::read(&path).unwrap(), before);
            }
        }
        let mut cleared = loaded;
        cleared.default_audiobook_folder = None;
        cleared.default_podcast_folder = None;
        let revision = cleared.revision;
        let saved = save_profile(&path, cleared, Some(revision), None).unwrap();
        let value = read_document(&path).unwrap();
        let entry = value["profiles"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == saved.id)
            .unwrap();
        assert!(entry["defaultAudiobookFolder"].is_null());
        assert!(entry["defaultPodcastFolder"].is_null());
    }
    #[test]
    fn invalid_documents_are_never_overwritten_and_atomic_failure_keeps_destination() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles.json");
        for bytes in [b"broken".as_slice(),br#"{"profiles":{}}"#,br#"{"profiles":[{"id":"same","name":"A","description":"","deviceProfile":null},{"id":"same","name":"B","description":"","deviceProfile":null}]}"#] {
        std::fs::write(&path,bytes).unwrap();assert!(ensure_profiles_file_exists(&path,DEFAULTS).is_err());assert_eq!(std::fs::read(&path).unwrap(),bytes);
    }
        let destination = dir.path().join("directory");
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(destination.join("keep"), b"original").unwrap();
        assert!(atomic_write(&destination, &json!({"profiles":[]})).is_err());
        assert_eq!(
            std::fs::read(destination.join("keep")).unwrap(),
            b"original"
        );
    }
    #[test]
    fn uncertain_old_defaults_and_unsupported_clones_keep_their_payload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles.json");
        let defaults: Value = serde_json::from_slice(DEFAULTS).unwrap();
        let mut old = defaults["profiles"][2].clone();
        old["description"] =
            json!("For devices with limited storage. Transcodes everything to MP3 192 kbps.");
        old["deviceProfile"]["DirectPlayProfiles"] = json!([]);
        old["deviceProfile"]["CodecProfiles"] =
            json!([{"Conditions":[{"Property":"AudioProfile","Value":"custom"}]}]);
        old["future"] = json!(true);
        std::fs::write(&path, json!({"profiles":[old.clone()]}).to_string()).unwrap();
        ensure_profiles_file_exists(&path, DEFAULTS).unwrap();
        let source = resolve_profile(&path, "rockbox-mp3-192").unwrap();
        assert_eq!(source.ownership, "custom");
        assert_eq!(
            source.device_profile.as_ref().unwrap(),
            &old["deviceProfile"]
        );
        let mut draft = source.clone();
        draft.name = "Cloned legacy".into();
        draft.description = "My description".into();
        draft.default_music_folder = Some("Mine".into());
        let cloned = save_profile(&path, draft, None, Some(&source.id)).unwrap();
        assert_ne!(cloned.id, source.id);
        assert_eq!(cloned.description, "My description");
        assert_eq!(cloned.default_music_folder.as_deref(), Some("Mine"));
        assert_eq!(cloned.device_profile, source.device_profile);
        assert_eq!(cloned.extra["future"], true);
    }
    #[test]
    fn system_refresh_preserves_baselines_custom_entries_and_collision_identity() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles.json");
        let mut defaults: Value = serde_json::from_slice(DEFAULTS).unwrap();
        std::fs::write(&path, defaults.to_string()).unwrap();
        ensure_profiles_file_exists(&path, DEFAULTS).unwrap();
        let before = read_document(&path).unwrap();
        let source = resolve_profile(&path, "system:generic-mp3-player").unwrap();
        let custom = save_profile(&path, source.clone(), None, Some(&source.id)).unwrap();
        defaults["profiles"][3]["deviceProfile"]["MusicStreamingTranscodingBitrate"] =
            json!(192000);
        ensure_profiles_file_exists(&path, defaults.to_string().as_bytes()).unwrap();
        let after = read_document(&path).unwrap();
        assert_eq!(
            before["legacyAudioBaselines"],
            after["legacyAudioBaselines"]
        );
        assert_eq!(
            resolve_profile(&path, &custom.id).unwrap().device_profile,
            custom.device_profile
        );
        assert_eq!(
            resolve_profile(&path, "generic-mp3-player")
                .unwrap()
                .device_profile
                .unwrap()["MusicStreamingTranscodingBitrate"],
            192000
        );
    }
    #[test]
    fn malformed_known_store_metadata_returns_error_without_panicking_or_writing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles.json");
        for key in ["systemProfileIds", "profileAliases", "legacyAudioBaselines"] {
            for invalid in [
                json!(4),
                json!([]),
                Value::Null,
                json!({"bad":false}),
                json!({"bad":""}),
            ] {
                let mut doc = json!({"profileStoreVersion":1,"profiles":[]});
                doc[key] = invalid;
                let original = doc.to_string();
                std::fs::write(&path, &original).unwrap();
                let result =
                    std::panic::catch_unwind(|| ensure_profiles_file_exists(&path, DEFAULTS));
                assert!(result.is_ok(), "malformed {key} must never panic");
                assert!(result.unwrap().is_err());
                assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
            }
        }
        std::fs::write(&path, r#"{"profileStoreVersion":2,"profiles":[]}"#).unwrap();
        let original = std::fs::read(&path).unwrap();
        assert!(ensure_profiles_file_exists(&path, DEFAULTS).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }
}
