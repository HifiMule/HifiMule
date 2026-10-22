use super::*;

#[tokio::test]
async fn compressed_manifest_migrates_legacy_only_after_verified_write() {
    let dir = tempdir().unwrap();
    let mut manifest = make_manifest("compressed-migration", "Été 🎵");
    manifest.dirty = true;
    manifest.pending_item_ids = vec!["unfinished".into(), "unfinished".into()];
    std::fs::write(
        dir.path().join(".hifimule.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    write_manifest(msc(dir.path()), &manifest).await.unwrap();
    let bytes = std::fs::read(dir.path().join(".hifimule.json.zst"))
        .expect("compressed manifest must be written");
    assert_eq!(&bytes[..4], &[0x28, 0xb5, 0x2f, 0xfd]);
    assert!(!dir.path().join(".hifimule.json").exists());
    let loaded = DeviceProber::probe(dir.path()).await.unwrap().unwrap();
    assert_eq!(
        serde_json::to_value(loaded).unwrap(),
        serde_json::to_value(manifest).unwrap()
    );
}

#[tokio::test]
async fn compressed_manifest_corruption_never_falls_back_to_legacy() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join(".hifimule.json"),
        serde_json::to_vec(&make_manifest("stale", "Clean")).unwrap(),
    )
    .unwrap();
    std::fs::write(
        dir.path().join(".hifimule.json.zst"),
        b"broken compressed data",
    )
    .unwrap();
    assert!(
        DeviceProber::probe(dir.path()).await.is_err(),
        "corrupt compressed manifest must fail closed"
    );
}

#[tokio::test]
async fn compressed_mtp_cache_authority_handles_both_physical_and_portable_identities() {
    for identity in ["physical", "portable"] {
        for compressed_cache in [false, true] {
            let dir = tempdir().unwrap();
            let cache_dir = tempdir().unwrap();
            let mirror = make_manifest("portable", "Clean mirror");
            write_manifest(msc(dir.path()), &mirror).await.unwrap();
            let mut authoritative = mirror.clone();
            authoritative.dirty = true;
            authoritative.pending_item_ids = vec!["recovery".into()];
            let path = cache_dir.path().join(format!("{identity}.json.zst"));
            let actual_path = if compressed_cache {
                path.clone()
            } else {
                path.with_extension("")
            };
            let data = if compressed_cache {
                manifest_codec::encode(&authoritative).unwrap()
            } else {
                serde_json::to_vec(&authoritative).unwrap()
            };
            std::fs::write(&actual_path, data).unwrap();
            let (tx, mut rx) = tokio::sync::mpsc::channel(1);
            assert!(
                emit_mtp_probe_event_with_cache_path(
                    &tx,
                    1,
                    PathBuf::from("mtp://physical"),
                    "physical",
                    dummy_mtp_device_info("physical"),
                    "MTP".into(),
                    msc(dir.path()),
                    |id| Ok(cache_dir.path().join(format!("{id}.json.zst")))
                )
                .await
            );
            let DeviceEvent::Detected { manifest, .. } = rx.recv().await.unwrap() else {
                panic!("expected detected device")
            };
            assert!(manifest.dirty);
            assert_eq!(manifest.pending_item_ids, ["recovery"]);
            // A corrupt authoritative compressed copy blocks both the mirror and
            // the valid legacy copy, regardless of which identity maps to it.
            std::fs::write(&path, b"corrupt").unwrap();
            std::fs::write(
                path.with_extension(""),
                serde_json::to_vec(&mirror).unwrap(),
            )
            .unwrap();
            assert!(
                !emit_mtp_probe_event_with_cache_path(
                    &tx,
                    2,
                    PathBuf::from("mtp://physical"),
                    "physical",
                    dummy_mtp_device_info("physical"),
                    "MTP".into(),
                    msc(dir.path()),
                    |id| Ok(cache_dir.path().join(format!("{id}.json.zst")))
                )
                .await
            );
            assert!(matches!(
                rx.recv().await.unwrap(),
                DeviceEvent::DiscoveryFailed {
                    code: "DEVICE_READ_FAILED",
                    ..
                }
            ));
        }
    }
}

#[tokio::test]
async fn compressed_mtp_portable_cache_overrides_physical_identity_cache_and_fails_closed() {
    for physical_compressed in [false, true] {
        let dir = tempdir().unwrap();
        let cache_dir = tempdir().unwrap();
        let physical = cache_dir.path().join("physical.json.zst");
        let portable = cache_dir.path().join("portable.json.zst");
        let old = make_manifest("portable", "Old physical identity cache");
        let path = if physical_compressed {
            physical.clone()
        } else {
            physical.with_extension("")
        };
        let bytes = if physical_compressed {
            manifest_codec::encode(&old).unwrap()
        } else {
            serde_json::to_vec(&old).unwrap()
        };
        std::fs::write(path, bytes).unwrap();
        let mut latest = old.clone();
        latest.dirty = true;
        latest.pending_item_ids = vec!["unfinished".into()];
        std::fs::write(&portable, manifest_codec::encode(&latest).unwrap()).unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        assert!(
            emit_mtp_probe_event_with_cache_path(
                &tx,
                1,
                PathBuf::from("mtp://physical"),
                "physical",
                dummy_mtp_device_info("physical"),
                "MTP".into(),
                msc(dir.path()),
                |id| Ok(cache_dir.path().join(format!("{id}.json.zst")))
            )
            .await
        );
        let DeviceEvent::Detected { manifest, .. } = rx.recv().await.unwrap() else {
            panic!("expected detected")
        };
        assert!(
            manifest.dirty,
            "portable identity is the cache used by all manifest updates"
        );
        assert_eq!(manifest.pending_item_ids, ["unfinished"]);
        std::fs::write(&portable, b"corrupt authoritative portable cache").unwrap();
        std::fs::write(
            portable.with_extension(""),
            serde_json::to_vec(&old).unwrap(),
        )
        .unwrap();
        assert!(
            !emit_mtp_probe_event_with_cache_path(
                &tx,
                2,
                PathBuf::from("mtp://physical"),
                "physical",
                dummy_mtp_device_info("physical"),
                "MTP".into(),
                msc(dir.path()),
                |id| Ok(cache_dir.path().join(format!("{id}.json.zst")))
            )
            .await
        );
        assert!(matches!(
            rx.recv().await.unwrap(),
            DeviceEvent::DiscoveryFailed {
                code: "DEVICE_READ_FAILED",
                ..
            }
        ));
    }
}

#[tokio::test]
async fn compressed_mtp_wpd_identity_uses_portable_cache_without_invalid_filename_lookup() {
    let dir = tempdir().unwrap();
    let cache_dir = tempdir().unwrap();
    let physical = r"\\?\usb#vid_091e&pid_abcd#serial#{guid}";
    let mirror = make_manifest("portable-wpd", "Mirror");
    write_manifest(msc(dir.path()), &mirror).await.unwrap();
    let mut latest = mirror.clone();
    latest.dirty = true;
    latest.pending_item_ids = vec!["recovery".into()];
    std::fs::write(
        cache_dir.path().join("portable-wpd.json.zst"),
        manifest_codec::encode(&latest).unwrap(),
    )
    .unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    assert!(
        emit_mtp_probe_event_with_cache_path(
            &tx,
            1,
            PathBuf::from("mtp://wpd"),
            physical,
            dummy_mtp_device_info(physical),
            "WPD".into(),
            msc(dir.path()),
            |id| {
                if id == physical {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "WPD ID cannot be a cache filename",
                    )
                    .into());
                }
                Ok(cache_dir.path().join(format!("{id}.json.zst")))
            }
        )
        .await,
        "raw WPD identifiers require mirror-to-portable identity resolution"
    );
    let DeviceEvent::Detected { manifest, .. } = rx.recv().await.unwrap() else {
        panic!("expected detected")
    };
    assert!(manifest.dirty);
    assert_eq!(manifest.pending_item_ids, ["recovery"]);
}

#[test]
fn manifest_cache_identity_validation_preserves_platform_names_and_rejects_traversal() {
    assert!(is_addressable_manifest_cache_identity("portable-device"));
    for id in [
        "",
        ".",
        "..",
        "../outside",
        "folder/inside",
        "folder\\inside",
    ] {
        assert!(!is_addressable_manifest_cache_identity(id));
    }
    assert_eq!(
        is_addressable_manifest_cache_identity("1:2"),
        !cfg!(windows)
    );
    assert_eq!(
        is_addressable_manifest_cache_identity("CON"),
        !cfg!(windows)
    );
}

#[tokio::test]
async fn compressed_manifest_failed_write_preserves_legacy_and_reports_cleanup_failure() {
    let dir = tempdir().unwrap();
    let legacy = dir.path().join(".hifimule.json");
    let manifest = make_manifest("migration-failure", "Recovery");
    let before = serde_json::to_vec(&manifest).unwrap();
    std::fs::write(&legacy, &before).unwrap();
    std::fs::create_dir(dir.path().join(".hifimule.json.zst")).unwrap();
    assert!(write_manifest(msc(dir.path()), &manifest).await.is_err());
    assert_eq!(std::fs::read(&legacy).unwrap(), before);
    std::fs::remove_dir(dir.path().join(".hifimule.json.zst")).unwrap();
    std::fs::remove_file(&legacy).unwrap();
    std::fs::create_dir(&legacy).unwrap();
    assert!(
        write_manifest(msc(dir.path()), &manifest).await.is_err(),
        "cleanup failures must be surfaced"
    );
    assert_eq!(
        DeviceProber::probe(dir.path())
            .await
            .unwrap()
            .unwrap()
            .device_id,
        manifest.device_id
    );
}

#[tokio::test]
async fn compressed_local_cache_migration_verifies_and_keeps_legacy_on_failed_commit() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("cache.json.zst");
    let legacy = path.with_extension("");
    let manifest = make_manifest("cache-migration", "MTP");
    let before = serde_json::to_vec(&manifest).unwrap();
    std::fs::write(&legacy, &before).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(
        persist_local_manifest_atomic(&path, &manifest)
            .await
            .is_err()
    );
    assert_eq!(std::fs::read(&legacy).unwrap(), before);
    std::fs::remove_dir(&path).unwrap();
    persist_local_manifest_atomic(&path, &manifest)
        .await
        .unwrap();
    assert!(!legacy.exists());
    assert_eq!(
        read_local_manifest(&path).await.unwrap().unwrap().device_id,
        manifest.device_id
    );
}

#[tokio::test]
async fn compressed_cleanup_failure_keeps_clean_settings_and_basket_updates_clean() {
    for basket in [false, true] {
        let dir = tempdir().unwrap();
        let manager = DeviceManager::new(Arc::new(crate::db::Database::memory().unwrap()));
        let manifest = make_manifest("cleanup-clean", "MSC");
        write_manifest(msc(dir.path()), &manifest).await.unwrap();
        manager
            .handle_device_detected(dir.path().to_path_buf(), manifest, msc(dir.path()))
            .await
            .unwrap();
        std::fs::create_dir(dir.path().join(".hifimule.json")).unwrap();
        assert!(manager.update_manifest_for_device("cleanup-clean", |m| {
            if basket {
                m.basket_items = serde_json::from_str(r#"[{"id":"album","name":"Album","type":"MusicAlbum","childCount":1,"sizeTicks":0,"sizeBytes":1}]"#).unwrap();
            } else {
                m.icon = Some("headphones".into());
            }
        }).await.is_err());
        let memory = manager.get_current_device().await.unwrap();
        let durable = DeviceProber::probe(dir.path()).await.unwrap().unwrap();
        assert!(
            !memory.dirty,
            "ordinary clean updates must not invent recovery state"
        );
        assert!(
            !durable.dirty,
            "rediscovery must remain clean after cleanup failure"
        );
        assert!(memory.pending_item_ids.is_empty() && durable.pending_item_ids.is_empty());
    }
}

#[tokio::test]
async fn compressed_cleanup_failure_preserves_durable_dirty_recovery_after_clean_mutation() {
    let dir = tempdir().unwrap();
    let manager = DeviceManager::new(Arc::new(crate::db::Database::memory().unwrap()));
    let mut manifest = make_manifest("cleanup-recovery", "MSC");
    manifest.dirty = true;
    manifest.pending_item_ids = vec!["unfinished".into()];
    write_manifest(msc(dir.path()), &manifest).await.unwrap();
    manager
        .handle_device_detected(dir.path().to_path_buf(), manifest, msc(dir.path()))
        .await
        .unwrap();
    std::fs::create_dir(dir.path().join(".hifimule.json")).unwrap();
    assert!(
        manager
            .update_manifest_for_device("cleanup-recovery", |m| {
                m.dirty = false;
                m.pending_item_ids.clear();
            })
            .await
            .is_err()
    );
    let durable = DeviceProber::probe(dir.path()).await.unwrap().unwrap();
    assert!(
        durable.dirty,
        "post-commit cleanup failure must retain recovery evidence on authoritative storage"
    );
    assert_eq!(durable.pending_item_ids, ["unfinished"]);
}

#[derive(Debug)]
struct ManifestPublicationFaultIo {
    inner: Arc<dyn crate::device_io::DeviceIO>,
    corrupt_next_write: std::sync::atomic::AtomicBool,
    fail_sync_call: Option<usize>,
    sync_calls: std::sync::atomic::AtomicUsize,
    fail_listing: bool,
}

#[async_trait::async_trait]
impl crate::device_io::DeviceIO for ManifestPublicationFaultIo {
    async fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        self.inner.read_file(path).await
    }
    async fn write_file(&self, path: &str, data: &[u8]) -> Result<()> {
        self.inner.write_file(path, data).await
    }
    async fn write_with_verify(&self, path: &str, data: &[u8]) -> Result<()> {
        if self
            .corrupt_next_write
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            let mut corrupt = data.to_vec();
            *corrupt.last_mut().unwrap() ^= 1;
            self.inner.write_with_verify(path, &corrupt).await
        } else {
            self.inner.write_with_verify(path, data).await
        }
    }
    async fn sync_manifest_parent(&self, path: &str) -> Result<()> {
        let call = self
            .sync_calls
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if self.fail_sync_call == Some(call) {
            anyhow::bail!("injected manifest directory durability failure");
        }
        self.inner.sync_manifest_parent(path).await
    }
    async fn delete_file(&self, path: &str) -> Result<()> {
        self.inner.delete_file(path).await
    }
    async fn list_files(&self, path: &str) -> Result<Vec<crate::device_io::FileEntry>> {
        if self.fail_listing {
            anyhow::bail!("injected root enumeration failure");
        }
        self.inner.list_files(path).await
    }
    async fn scan_recovery_markers(&self, roots: &[String]) -> Result<bool> {
        if self.fail_listing {
            anyhow::bail!("injected root enumeration failure");
        }
        self.inner.scan_recovery_markers(roots).await
    }
    async fn free_space(&self) -> Result<u64> {
        self.inner.free_space().await
    }
    async fn ensure_dir(&self, path: &str) -> Result<()> {
        self.inner.ensure_dir(path).await
    }
    async fn cleanup_empty_subdirs(&self, path: &str) -> Result<()> {
        self.inner.cleanup_empty_subdirs(path).await
    }
}

#[tokio::test]
async fn compressed_publication_verifies_bytes_and_durability_before_retiring_legacy() {
    for corrupt in [true, false] {
        let dir = tempdir().unwrap();
        let manifest = make_manifest("verify-publication", "MSC");
        let legacy = serde_json::to_vec(&manifest).unwrap();
        std::fs::write(dir.path().join(".hifimule.json"), &legacy).unwrap();
        let io = Arc::new(ManifestPublicationFaultIo {
            inner: msc(dir.path()),
            corrupt_next_write: corrupt.into(),
            fail_sync_call: (!corrupt).then_some(0),
            sync_calls: 0.into(),
            fail_listing: false,
        });
        let error = write_manifest(io.clone(), &manifest)
            .await
            .expect_err("reported write success must be verified before deleting legacy");
        assert_eq!(
            std::fs::read(dir.path().join(".hifimule.json")).unwrap(),
            legacy
        );
        assert!(error.to_string().contains("publication"));
        assert_eq!(
            io.sync_calls.load(std::sync::atomic::Ordering::SeqCst),
            usize::from(!corrupt)
        );
    }
}

#[tokio::test]
async fn compressed_publication_uncertainty_preserves_dirty_recovery() {
    for failure_call in [0, 1] {
        let dir = tempdir().unwrap();
        let manager = DeviceManager::new(Arc::new(crate::db::Database::memory().unwrap()));
        let mut manifest = make_manifest("durability-recovery", "MSC");
        manifest.dirty = true;
        manifest.pending_item_ids = vec!["unfinished".into()];
        std::fs::write(
            dir.path().join(".hifimule.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        let io: Arc<dyn crate::device_io::DeviceIO> = Arc::new(ManifestPublicationFaultIo {
            inner: msc(dir.path()),
            corrupt_next_write: false.into(),
            fail_sync_call: Some(failure_call),
            sync_calls: 0.into(),
            fail_listing: false,
        });
        manager
            .handle_device_detected(dir.path().to_path_buf(), manifest, io)
            .await
            .unwrap();
        assert!(
            manager
                .update_manifest_for_device("durability-recovery", |m| {
                    m.dirty = false;
                    m.pending_item_ids.clear();
                })
                .await
                .is_err()
        );
        let memory = manager.get_current_device().await.unwrap();
        let durable = DeviceProber::probe(dir.path()).await.unwrap().unwrap();
        assert!(memory.dirty && durable.dirty);
        assert_eq!(durable.pending_item_ids, ["unfinished"]);
        if failure_call == 0 {
            assert!(dir.path().join(".hifimule.json").exists());
        }
    }
}

fn full_field_codec_manifest() -> DeviceManifest {
    let mut manifest = make_manifest("rust-fixture", "Été 🎵");
    manifest.icon = Some("🎧".into());
    manifest.audiobook_path = Some("Books".into());
    manifest.podcast_path = Some("Podcasts".into());
    manifest.playlist_path = Some("Lists".into());
    manifest.dirty = true;
    manifest.pending_item_ids = vec!["same".into(), "same".into()];
    manifest.auto_sync_on_connect = true;
    manifest.last_sync_at = Some(i64::MIN);
    manifest
        .auto_fill
        .pipelines
        .insert("server".into(), rich_pipeline());
    manifest.transcoding_profile_id = Some("profile".into());
    manifest.last_synced_transcoding_profile_id = Some("old-profile".into());
    manifest.transcoding_profile_dirty = true;
    manifest.storage_id = Some("storage".into());
    manifest.folder_ids.insert("Music/Été".into(), u32::MAX);
    manifest.basket_items = serde_json::from_str(r#"[{"id":"same","name":"Basket","type":"MusicAlbum","serverId":"server","artist":"","childCount":4294967295,"sizeTicks":-9223372036854775808,"sizeBytes":18446744073709551615}]"#).unwrap();
    manifest.playlists = serde_json::from_str(r#"[{"jellyfinId":"playlist","filename":"Lists/all.m3u","trackCount":2,"trackIds":["same","same"],"lastModified":"today"}]"#).unwrap();
    let item: SyncedItem = serde_json::from_str(r#"{"isAutoFill":true,"mediaRole":"audiobook","providerItemId":"same","name":"Été 🎵","album":"","artist":null,"localPath":"Books/Été.mp3","sizeBytes":18446744073709551615,"syncedAt":"today","originalName":"original","etag":"etag","providerAlbumId":"album","providerContentType":"audio/mpeg","providerSuffix":"mp3","originalBitrate":4294967295,"originalContainer":"mp3","trackNumber":4294967295,"serverId":"server"}"#).unwrap();
    let mut second = item.clone();
    second.name = "Second occurrence".into();
    second.is_auto_fill = false;
    second.media_role = MediaRole::Podcast;
    second.size_bytes = 0;
    second.original_name = None;
    manifest.synced_items = vec![item, second];
    manifest
}

#[test]
fn compressed_codec_roundtrip_preserves_full_fields_order_duplicates_precision_and_defaults() {
    for manifest in [DeviceManifest::default(), full_field_codec_manifest()] {
        let bytes = manifest_codec::encode(&manifest).unwrap();
        assert!(bytes[4] & 4 != 0, "standard frame must carry checksum");
        let decoded = manifest_codec::decode(&bytes, true).unwrap();
        assert_eq!(
            serde_json::to_value(decoded).unwrap(),
            serde_json::to_value(manifest).unwrap()
        );
    }
}

#[test]
fn compressed_codec_preserves_metadata_numeric_tokens_including_f32_settings() {
    let manifest = full_field_codec_manifest();
    let legacy = serde_json::to_vec(&manifest).unwrap();
    let bytes = manifest_codec::encode(&manifest).unwrap();
    let raw = zstd::stream::decode_all(bytes.as_slice()).unwrap();
    let wrapper: std::collections::BTreeMap<String, Box<serde_json::value::RawValue>> =
        serde_json::from_slice(&raw).unwrap();
    let stored: std::collections::BTreeMap<String, Box<serde_json::value::RawValue>> =
        serde_json::from_str(wrapper["manifest"].get()).unwrap();
    let mut expected: std::collections::BTreeMap<String, Box<serde_json::value::RawValue>> =
        serde_json::from_slice(&legacy).unwrap();
    expected.remove("synced_items");
    assert_eq!(stored.len(), expected.len());
    for (field, value) in expected {
        assert_eq!(
            stored[&field].get(),
            value.get(),
            "metadata value/token changed for {field}"
        );
    }
}

#[test]
fn compressed_codec_rejects_bad_version_count_types_checksum_truncation_and_extra_frames() {
    let bytes = manifest_codec::encode(&full_field_codec_manifest()).unwrap();
    let raw = zstd::stream::decode_all(bytes.as_slice()).unwrap();
    let mut wrapper: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    let frame = |value: &serde_json::Value| {
        let mut encoder = zstd::stream::write::Encoder::new(Vec::new(), 9).unwrap();
        encoder.include_checksum(true).unwrap();
        serde_json::to_writer(&mut encoder, value).unwrap();
        encoder.finish().unwrap()
    };
    wrapper["storageVersion"] = 2.into();
    assert!(manifest_codec::decode(&frame(&wrapper), true).is_err());
    wrapper["storageVersion"] = 1.into();
    wrapper["syncedItems"]["count"] = 3.into();
    assert!(manifest_codec::decode(&frame(&wrapper), true).is_err());
    wrapper["syncedItems"]["count"] = 2.into();
    wrapper["syncedItems"]["columns"]["sizeBytes"][0] = "invalid".into();
    assert!(manifest_codec::decode(&frame(&wrapper), true).is_err());
    let mut corrupt = bytes.clone();
    let last = corrupt.len() - 1;
    corrupt[last] ^= 1;
    assert!(manifest_codec::decode(&corrupt, true).is_err());
    assert!(manifest_codec::decode(&bytes[..bytes.len() - 1], true).is_err());
    assert!(manifest_codec::decode(&[bytes.clone(), bytes].concat(), true).is_err());
    let unchecked = zstd::stream::encode_all(raw.as_slice(), 9).unwrap();
    assert!(manifest_codec::decode(&unchecked, true).is_err());
}

#[test]
#[ignore = "Explicit release-mode benchmark and regeneration of inspector fixtures"]
fn manifest_codec_benchmark_and_rust_fixtures() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let fixture_dir = root.join("scripts/tests/fixtures");
    std::fs::create_dir_all(&fixture_dir).unwrap();
    let fixture = full_field_codec_manifest();
    std::fs::write(
        fixture_dir.join("manifest-rust.json"),
        serde_json::to_vec_pretty(&fixture).unwrap(),
    )
    .unwrap();
    std::fs::write(
        fixture_dir.join("manifest-rust.json.zst"),
        manifest_codec::encode(&fixture).unwrap(),
    )
    .unwrap();
    use sha2::{Digest, Sha256};
    let id =
        |index: usize| format!("{:x}", Sha256::digest(format!("track-{index}")))[..32].to_string();
    let mut manifest = make_manifest("synthetic-benchmark", "Benchmark");
    manifest.dirty = true;
    manifest.pending_item_ids = (0..45522).map(id).collect();
    manifest.synced_items = (0..45522)
        .map(|index| SyncedItem {
            is_auto_fill: false,
            media_role: MediaRole::Music,
            jellyfin_id: id(index),
            name: format!("Existing song {index} with representative music metadata"),
            album: Some(format!(
                "Album {} original edition and bonus tracks",
                index / 10
            )),
            artist: Some(format!("Artist {} and collaborators", index / 100)),
            local_path: format!(
                "Music/Artist {}/Album {}/Old track {index}.mp3",
                index / 100,
                index / 10
            ),
            size_bytes: 5000000 + index as u64 * 53,
            synced_at: "2026-10-11T10:00:00Z".into(),
            original_name: None,
            etag: Some(id(index + 45522)),
            provider_album_id: Some(id(index / 10)),
            provider_content_type: Some("audio/mpeg".into()),
            provider_suffix: Some("mp3".into()),
            original_bitrate: Some(320000),
            original_container: Some("mp3".into()),
            track_number: Some(index as u32 % 10 + 1),
            server_id: Some("subsonic|https://server.example|user".into()),
        })
        .collect();
    assert_eq!(manifest.synced_items.len(), 45522);
    let start = std::time::Instant::now();
    let pretty = serde_json::to_vec_pretty(&manifest).unwrap();
    let pretty_encode_ms = start.elapsed().as_secs_f64() * 1000.0;
    let start = std::time::Instant::now();
    let legacy: DeviceManifest = serde_json::from_slice(&pretty).unwrap();
    let pretty_decode_ms = start.elapsed().as_secs_f64() * 1000.0;
    let start = std::time::Instant::now();
    let compressed = manifest_codec::encode(&manifest).unwrap();
    let compressed_encode_ms = start.elapsed().as_secs_f64() * 1000.0;
    let start = std::time::Instant::now();
    let decoded = manifest_codec::decode(&compressed, true).unwrap();
    let compressed_decode_ms = start.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(
        serde_json::to_value(decoded).unwrap(),
        serde_json::to_value(legacy).unwrap()
    );
    std::fs::write(
        root.join("target/manifest-compression-rust.json.zst"),
        &compressed,
    )
    .unwrap();
    println!(
        "MANIFEST_BENCHMARK tracks=45522 pretty_bytes={} compressed_bytes={} pretty_encode_ms={pretty_encode_ms:.3} pretty_decode_ms={pretty_decode_ms:.3} compressed_encode_ms={compressed_encode_ms:.3} compressed_decode_ms={compressed_decode_ms:.3}",
        pretty.len(),
        compressed.len()
    );
}

#[derive(Debug)]
struct CheckpointGateIo {
    inner: Arc<dyn crate::device_io::DeviceIO>,
    started: tokio::sync::Notify,
    release: tokio::sync::Notify,
    writes: std::sync::atomic::AtomicUsize,
}

#[async_trait::async_trait]
impl crate::device_io::DeviceIO for CheckpointGateIo {
    async fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        self.inner.read_file(path).await
    }
    async fn write_file(&self, path: &str, data: &[u8]) -> Result<()> {
        self.inner.write_file(path, data).await
    }
    async fn write_with_verify(&self, path: &str, data: &[u8]) -> Result<()> {
        self.started.notify_one();
        self.release.notified().await;
        self.inner.write_with_verify(path, data).await?;
        self.writes
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
    async fn delete_file(&self, path: &str) -> Result<()> {
        self.inner.delete_file(path).await
    }
    async fn list_files(&self, path: &str) -> Result<Vec<crate::device_io::FileEntry>> {
        self.inner.list_files(path).await
    }
    async fn free_space(&self) -> Result<u64> {
        self.inner.free_space().await
    }
    async fn ensure_dir(&self, path: &str) -> Result<()> {
        self.inner.ensure_dir(path).await
    }
    async fn cleanup_empty_subdirs(&self, path: &str) -> Result<()> {
        self.inner.cleanup_empty_subdirs(path).await
    }
}

#[tokio::test(start_paused = true)]
async fn sync_checkpoint_delayed_timer_drains_before_shutdown_and_device_removal() {
    let dir = tempdir().unwrap();
    let manager = Arc::new(DeviceManager::new(Arc::new(
        crate::db::Database::memory().unwrap(),
    )));
    let gated = Arc::new(CheckpointGateIo {
        inner: msc(dir.path()),
        started: tokio::sync::Notify::new(),
        release: tokio::sync::Notify::new(),
        writes: std::sync::atomic::AtomicUsize::new(0),
    });
    let io: Arc<dyn crate::device_io::DeviceIO> = gated.clone();
    manager
        .handle_device_detected(
            dir.path().to_path_buf(),
            make_manifest("lease", "Original"),
            Arc::clone(&io),
        )
        .await
        .unwrap();
    let session = SyncManifestSession::new(Arc::clone(&manager), "lease", &io, None)
        .await
        .unwrap();
    session
        .stage(1, |m| m.last_sync_at = Some(1))
        .await
        .unwrap();
    tokio::time::advance(Duration::from_secs(599)).await;
    assert_eq!(gated.writes.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert!(!dir.path().join(".hifimule.json.zst").exists());
    tokio::time::advance(Duration::from_secs(1)).await;
    gated.started.notified().await;
    let original_observation = manager.get_selected_sync_target().await.unwrap().3;
    let finish = tokio::spawn(async move { session.finish("shutdown").await });
    let removing = Arc::clone(&manager);
    let path = dir.path().to_path_buf();
    let removal = tokio::spawn(async move { removing.handle_device_removed(&path).await });
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    assert!(
        !finish.is_finished(),
        "shutdown must drain the active checkpoint"
    );
    assert!(
        !removal.is_finished(),
        "connection removal must serialize behind active persistence"
    );
    assert!(manager.get_current_device().await.is_some());
    gated.release.notify_one();
    finish.await.unwrap().unwrap();
    removal.await.unwrap();
    manager
        .handle_device_detected(
            dir.path().to_path_buf(),
            make_manifest("lease", "Replacement"),
            msc(dir.path()),
        )
        .await
        .unwrap();
    tokio::time::advance(Duration::from_secs(1800)).await;
    assert_eq!(
        gated.writes.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "joined timer must never write later"
    );
    assert_eq!(
        manager.get_current_device().await.unwrap().last_sync_at,
        None
    );
    assert!(
        manager
            .update_manifest_for_connection("lease", original_observation, &io, |m| m.dirty = false)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn sync_checkpoint_stale_queued_removal_preserves_new_connection() {
    let dir = tempdir().unwrap();
    let manager = Arc::new(DeviceManager::new(Arc::new(
        crate::db::Database::memory().unwrap(),
    )));
    let gated = Arc::new(CheckpointGateIo {
        inner: msc(dir.path()),
        started: tokio::sync::Notify::new(),
        release: tokio::sync::Notify::new(),
        writes: std::sync::atomic::AtomicUsize::new(0),
    });
    let io: Arc<dyn crate::device_io::DeviceIO> = gated.clone();
    manager
        .handle_device_detected(
            dir.path().to_path_buf(),
            make_manifest("a-original", "Original"),
            Arc::clone(&io),
        )
        .await
        .unwrap();
    let session = Arc::new(
        SyncManifestSession::new(Arc::clone(&manager), "a-original", &io, None)
            .await
            .unwrap(),
    );
    let staging = Arc::clone(&session);
    let checkpoint =
        tokio::spawn(async move { staging.stage(1000, |m| m.last_sync_at = Some(1000)).await });
    gated.started.notified().await;
    let removing = Arc::clone(&manager);
    let path = dir.path().to_path_buf();
    let first = tokio::spawn(async move { removing.handle_device_removed(&path).await });
    tokio::task::yield_now().await;
    let detecting = Arc::clone(&manager);
    let path = dir.path().to_path_buf();
    let replacement = msc(dir.path());
    let arrival = tokio::spawn(async move {
        detecting
            .handle_device_detected(
                path,
                make_manifest("b-replacement", "Replacement"),
                replacement,
            )
            .await
    });
    tokio::task::yield_now().await;
    let removing = Arc::clone(&manager);
    let path = dir.path().to_path_buf();
    let stale = tokio::spawn(async move { removing.handle_device_removed(&path).await });
    tokio::task::yield_now().await;
    assert!(!first.is_finished());
    assert!(!arrival.is_finished());
    assert!(!stale.is_finished());
    gated.release.notify_one();
    checkpoint.await.unwrap().unwrap();
    first.await.unwrap();
    arrival.await.unwrap().unwrap();
    stale.await.unwrap();
    assert_eq!(
        manager.get_current_device().await.unwrap().device_id,
        "b-replacement",
        "old queued removal must not remove newer arrival"
    );
    Arc::try_unwrap(session)
        .ok()
        .unwrap()
        .finish("cancel")
        .await
        .unwrap();
}

#[tokio::test(start_paused = true)]
async fn sync_checkpoint_count_flush_resets_timer_deadline() {
    let dir = tempdir().unwrap();
    let manager = Arc::new(DeviceManager::new(Arc::new(
        crate::db::Database::memory().unwrap(),
    )));
    let gated = Arc::new(CheckpointGateIo {
        inner: msc(dir.path()),
        started: tokio::sync::Notify::new(),
        release: tokio::sync::Notify::new(),
        writes: std::sync::atomic::AtomicUsize::new(0),
    });
    let io: Arc<dyn crate::device_io::DeviceIO> = gated.clone();
    manager
        .handle_device_detected(
            dir.path().to_path_buf(),
            make_manifest("reset", "Reset"),
            Arc::clone(&io),
        )
        .await
        .unwrap();
    let session = Arc::new(
        SyncManifestSession::with_settings(
            Arc::clone(&manager),
            "reset",
            &io,
            None,
            crate::sync_performance::SyncPerformance {
                checkpoint_track_updates: 100,
                ..Default::default()
            },
        )
        .await
        .unwrap(),
    );
    session
        .stage(1, |m| m.last_sync_at = Some(1))
        .await
        .unwrap();
    tokio::time::advance(Duration::from_secs(400)).await;
    let staging = Arc::clone(&session);
    let checkpoint =
        tokio::spawn(async move { staging.stage(99, |m| m.last_sync_at = Some(100)).await });
    gated.started.notified().await;
    gated.release.notify_one();
    checkpoint.await.unwrap().unwrap();
    session
        .stage(1, |m| m.last_sync_at = Some(101))
        .await
        .unwrap();
    tokio::time::advance(Duration::from_secs(200)).await;
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    assert_eq!(gated.writes.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(
        tokio::time::timeout(Duration::ZERO, gated.started.notified())
            .await
            .is_err(),
        "old timer deadline must not start a second write"
    );
    tokio::time::advance(Duration::from_secs(400)).await;
    gated.started.notified().await;
    gated.release.notify_one();
    Arc::try_unwrap(session)
        .ok()
        .unwrap()
        .finish("completion")
        .await
        .unwrap();
    assert_eq!(gated.writes.load(std::sync::atomic::Ordering::SeqCst), 2);
}

#[tokio::test(start_paused = true)]
async fn sync_checkpoint_custom_threshold_and_timer_preserve_final_flush() {
    let dir = tempdir().unwrap();
    let manager = Arc::new(DeviceManager::new(Arc::new(
        crate::db::Database::memory().unwrap(),
    )));
    let io = msc(dir.path());
    let manifest = make_manifest("custom", "Custom");
    write_manifest(Arc::clone(&io), &manifest).await.unwrap();
    manager
        .handle_device_detected(dir.path().to_path_buf(), manifest, Arc::clone(&io))
        .await
        .unwrap();
    let settings = crate::sync_performance::SyncPerformance {
        checkpoint_track_updates: 5000,
        checkpoint_interval: Duration::from_secs(900),
        ..Default::default()
    };
    let session =
        SyncManifestSession::with_settings(Arc::clone(&manager), "custom", &io, None, settings)
            .await
            .unwrap();
    let read = || {
        manifest_codec::decode_test_manifest::<DeviceManifest>(
            &std::fs::read(dir.path().join(".hifimule.json.zst")).unwrap(),
        )
        .unwrap()
    };
    session
        .stage(4999, |m| m.last_sync_at = Some(1))
        .await
        .unwrap();
    assert_eq!(read().last_sync_at, None);
    session
        .stage(1, |m| m.last_sync_at = Some(2))
        .await
        .unwrap();
    assert_eq!(read().last_sync_at, Some(2));
    session
        .stage(1, |m| m.last_sync_at = Some(3))
        .await
        .unwrap();
    tokio::time::advance(Duration::from_secs(899)).await;
    assert_eq!(read().last_sync_at, Some(2));
    tokio::time::advance(Duration::from_secs(1)).await;
    // Wait for asynchronous filesystem publication without advancing the timer again.
    for _ in 0..10000 {
        if read().last_sync_at == Some(3) {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(read().last_sync_at, Some(3));
    session
        .stage(1, |m| m.last_sync_at = Some(4))
        .await
        .unwrap();
    session.finish("completion").await.unwrap();
    assert_eq!(read().last_sync_at, Some(4));
}

#[tokio::test]
async fn sync_checkpoint_buffers_track_changes_until_threshold() {
    let dir = tempdir().unwrap();
    let manager = Arc::new(DeviceManager::new(Arc::new(
        crate::db::Database::memory().unwrap(),
    )));
    let mut manifest = make_manifest("checkpoint", "Checkpoint");
    manifest.dirty = true;
    manifest.pending_item_ids = vec!["pending".into()];
    write_manifest(msc(dir.path()), &manifest).await.unwrap();
    let io = msc(dir.path());
    manager
        .handle_device_detected(dir.path().to_path_buf(), manifest, Arc::clone(&io))
        .await
        .unwrap();
    let session = SyncManifestSession::new(Arc::clone(&manager), "checkpoint", &io, None)
        .await
        .unwrap();
    for index in 1..1000 {
        session
            .stage(1, |m| m.last_sync_at = Some(index))
            .await
            .unwrap();
    }
    let durable: DeviceManifest = manifest_codec::decode_test_manifest(
        &std::fs::read(dir.path().join(".hifimule.json.zst")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        durable.last_sync_at, None,
        "999 staged updates must not each write the full manifest"
    );
    assert_eq!(
        manager.get_current_device().await.unwrap().last_sync_at,
        Some(999)
    );
    session
        .stage(1, |m| m.last_sync_at = Some(100))
        .await
        .unwrap();
    let read = || {
        manifest_codec::decode_test_manifest::<DeviceManifest>(
            &std::fs::read(dir.path().join(".hifimule.json.zst")).unwrap(),
        )
        .unwrap()
    };
    assert_eq!(read().last_sync_at, Some(100));
    session
        .stage(1, |m| m.last_sync_at = Some(101))
        .await
        .unwrap();
    assert_eq!(read().last_sync_at, Some(100));
    session.finish("completion").await.unwrap();
    assert_eq!(read().last_sync_at, Some(101));
    assert!(read().dirty);
    assert_eq!(read().pending_item_ids, vec!["pending"]);
}

#[tokio::test]
async fn sync_checkpoint_reconnect_with_copied_id_cannot_receive_delayed_state_or_clean_commit() {
    let dir = tempdir().unwrap();
    let manager = Arc::new(DeviceManager::new(Arc::new(
        crate::db::Database::memory().unwrap(),
    )));
    let io = msc(dir.path());
    manager
        .handle_device_detected(
            dir.path().to_path_buf(),
            make_manifest("copied", "Original"),
            Arc::clone(&io),
        )
        .await
        .unwrap();
    let session = SyncManifestSession::new(Arc::clone(&manager), "copied", &io, None)
        .await
        .unwrap();
    session
        .stage(1, |m| m.last_sync_at = Some(99))
        .await
        .unwrap();
    let original_observation = manager.get_selected_sync_target().await.unwrap().3;
    manager
        .handle_device_removed(&dir.path().to_path_buf())
        .await;
    let replacement = msc(dir.path());
    manager
        .handle_device_detected(
            dir.path().to_path_buf(),
            make_manifest("copied", "Replacement"),
            replacement,
        )
        .await
        .unwrap();
    assert!(session.finish("cancel").await.is_err());
    assert!(
        manager
            .update_manifest_for_connection("copied", original_observation, &io, |m| m.dirty =
                false)
            .await
            .is_err()
    );
    let current = manager.get_current_device().await.unwrap();
    assert_eq!(current.name.as_deref(), Some("Replacement"));
    assert_eq!(current.last_sync_at, None);
    assert!(!dir.path().join(".hifimule.json.zst").exists());
}

#[tokio::test]
async fn sync_checkpoint_final_clean_rejects_rediscovery_reusing_same_backend_arc() {
    let dir = tempdir().unwrap();
    let manager = Arc::new(DeviceManager::new(Arc::new(
        crate::db::Database::memory().unwrap(),
    )));
    let io = msc(dir.path());
    manager
        .handle_device_detected(
            dir.path().to_path_buf(),
            make_manifest("reused", "Original"),
            Arc::clone(&io),
        )
        .await
        .unwrap();
    let (_, original_observation) = manager.get_basket_target("reused").await.unwrap();
    manager
        .handle_device_removed(&dir.path().to_path_buf())
        .await;
    let mut replacement = make_manifest("reused", "Replacement");
    replacement.dirty = true;
    replacement.pending_item_ids = vec!["new-arrival-pending".into()];
    manager
        .handle_device_detected(dir.path().to_path_buf(), replacement, Arc::clone(&io))
        .await
        .unwrap();
    assert_ne!(
        manager.get_basket_target("reused").await.unwrap().1,
        original_observation
    );
    assert!(
        manager
            .update_manifest_for_connection(
                "reused",
                original_observation.parse().unwrap(),
                &io,
                |m| {
                    m.dirty = false;
                    m.pending_item_ids.clear();
                }
            )
            .await
            .is_err(),
        "a reused backend is not the original physical observation"
    );
    assert!(manager.get_current_device().await.unwrap().dirty);
    assert_eq!(
        manager.get_current_device().await.unwrap().pending_item_ids,
        vec!["new-arrival-pending"]
    );
}

#[tokio::test]
async fn sync_checkpoint_duplicate_ids_commit_only_exact_captured_connection() {
    let a = tempdir().unwrap();
    let b = tempdir().unwrap();
    let manager = Arc::new(DeviceManager::new(Arc::new(
        crate::db::Database::memory().unwrap(),
    )));
    for path in [a.path(), b.path()] {
        let mut manifest = make_manifest("copied-id", "Copied");
        manifest.dirty = true;
        manifest.pending_item_ids = vec!["unfinished".into()];
        let io = msc(path);
        write_manifest(Arc::clone(&io), &manifest).await.unwrap();
        manager
            .handle_device_detected(path.to_path_buf(), manifest, io)
            .await
            .unwrap();
    }
    let first = manager
        .state
        .read()
        .await
        .connected_devices
        .keys()
        .next()
        .unwrap()
        .clone();
    let selected = if first == a.path() {
        b.path()
    } else {
        a.path()
    };
    assert!(manager.select_device(selected.to_path_buf()).await);
    let (_, _, io, observation) = manager.get_selected_sync_target().await.unwrap();
    let result = manager
        .update_manifest_for_connection("copied-id", observation, &io, |m| {
            m.dirty = false;
            m.pending_item_ids.clear();
        })
        .await;
    assert!(
        result.is_ok(),
        "exact target behind duplicate ID must be found: {result:?}"
    );
    let read = |path: &Path| {
        manifest_codec::decode_test_manifest::<DeviceManifest>(
            &std::fs::read(path.join(".hifimule.json.zst")).unwrap(),
        )
        .unwrap()
    };
    assert!(!read(selected).dirty);
    assert!(read(&first).dirty);
    assert_eq!(read(&first).pending_item_ids, vec!["unfinished"]);
}

#[tokio::test]
async fn sync_checkpoint_pending_publication_removal_waits_for_recognized_commit() {
    let manager = Arc::new(DeviceManager::new(Arc::new(
        crate::db::Database::memory().unwrap(),
    )));
    let dir = tempdir().unwrap();
    let path = dir.path().to_path_buf();
    manager
        .handle_device_unrecognized(path.clone(), msc(dir.path()), None)
        .await;
    let pending = manager.get_unrecognized_device_snapshot().await.unwrap();
    // Capture the removal event before initialization publishes the same arrival.
    let observed = DeviceManager::removal_observation(&*manager.state.read().await, &path);
    let token = manager.begin_destination_mutation();
    let manifest = manager
        .initialize_reserved_pending(pending, "", None, None, "Initialized".into(), None, false, Default::default())
        .await
        .unwrap();
    let lock = manager.manifest_commit_lock(&manifest.device_id);
    let commit = lock.lock().await;
    let removal = {
        let manager = manager.clone();
        let path = path.clone();
        tokio::spawn(async move { manager.remove_observed_device(&path, token, observed).await })
    };
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    assert!(
        !removal.is_finished(),
        "removal must acquire the initialized device's commit lock"
    );
    assert!(manager.get_current_device().await.is_some());
    drop(commit);
    removal.await.unwrap();
    assert!(manager.get_current_device().await.is_none());
    assert!(manager.state.read().await.connected_devices.is_empty());
    assert!(manager.get_pending_devices_snapshot().await.is_empty());
}

#[tokio::test]
async fn sync_checkpoint_old_removal_preserves_new_pending_arrival() {
    let manager = DeviceManager::new(Arc::new(crate::db::Database::memory().unwrap()));
    let dir = tempdir().unwrap();
    let path = dir.path().to_path_buf();
    manager
        .handle_device_unrecognized(path.clone(), msc(dir.path()), None)
        .await;
    let observed = DeviceManager::removal_observation(&*manager.state.read().await, &path);
    let token = manager.begin_destination_mutation();
    manager
        .handle_device_unrecognized(path.clone(), msc(dir.path()), None)
        .await;
    let newer = manager.get_unrecognized_device_snapshot().await.unwrap();
    manager.remove_observed_device(&path, token, observed).await;
    assert_eq!(
        manager
            .get_unrecognized_device_snapshot()
            .await
            .unwrap()
            .pending_id,
        newer.pending_id
    );
    assert!(!*newer.cancelled.borrow());
}

#[cfg(windows)]
#[tokio::test]
async fn sync_checkpoint_failure_retains_staged_metadata_and_durable_recovery() {
    use std::os::windows::fs::OpenOptionsExt;
    let dir = tempdir().unwrap();
    let manager = Arc::new(DeviceManager::new(Arc::new(
        crate::db::Database::memory().unwrap(),
    )));
    let io = msc(dir.path());
    let mut manifest = make_manifest("denied", "Denied");
    manifest.dirty = true;
    manifest.pending_item_ids = vec!["copied-audio".into()];
    write_manifest(Arc::clone(&io), &manifest).await.unwrap();
    manager
        .handle_device_detected(dir.path().to_path_buf(), manifest, Arc::clone(&io))
        .await
        .unwrap();
    let reader = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(3)
        .open(dir.path().join(".hifimule.json.zst"))
        .unwrap();
    let session = SyncManifestSession::new(Arc::clone(&manager), "denied", &io, None)
        .await
        .unwrap();
    assert!(
        session
            .stage(1000, |m| m.last_sync_at = Some(1000))
            .await
            .is_err()
    );
    let memory = manager.get_current_device().await.unwrap();
    assert_eq!(memory.last_sync_at, Some(1000));
    assert!(memory.dirty);
    let durable: DeviceManifest = manifest_codec::decode_test_manifest(
        &std::fs::read(dir.path().join(".hifimule.json.zst")).unwrap(),
    )
    .unwrap();
    assert!(durable.dirty);
    assert_eq!(durable.last_sync_at, None);
    assert_eq!(durable.pending_item_ids, vec!["copied-audio"]);
    drop(reader);
    assert!(
        session.finish("error").await.is_err(),
        "successful recovery flush must not hide earlier checkpoint failure"
    );
    let durable: DeviceManifest = manifest_codec::decode_test_manifest(
        &std::fs::read(dir.path().join(".hifimule.json.zst")).unwrap(),
    )
    .unwrap();
    assert_eq!(durable.last_sync_at, Some(1000));
    assert!(durable.dirty);
}

#[tokio::test]
async fn mtp_cache_path_failure_rolls_back_memory() {
    let dir = tempdir().unwrap();
    let manager = DeviceManager::new(Arc::new(crate::db::Database::memory().unwrap()));
    let path = PathBuf::from("mtp://cache-path-failure");
    let mut manifest = make_manifest("cache-path-failure", "MTP");
    manifest.dirty = true;
    manifest.pending_item_ids = vec!["unfinished".into()];
    write_manifest(msc(dir.path()), &manifest).await.unwrap();
    let mirror_before = std::fs::read(dir.path().join(".hifimule.json.zst")).unwrap();
    manager
        .handle_device_detected(path.clone(), manifest.clone(), msc(dir.path()))
        .await
        .unwrap();
    // The fake backend supports mirror writes; the authoritative path resolver
    // fails independently, after the in-memory clean mutation has been applied.
    let result = manager
        .update_manifest_for_device_with_cache_path(
            &manifest.device_id,
            |updated| {
                updated.dirty = false;
                updated.pending_item_ids.clear();
            },
            |_| anyhow::bail!("injected cache path resolution failure"),
            sync_manifest_directory,
        )
        .await;
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("path resolution failure")
    );
    let restored = manager
        .get_manifest_for_device(&manifest.device_id)
        .await
        .unwrap();
    assert!(restored.dirty);
    assert_eq!(restored.pending_item_ids, manifest.pending_item_ids);
    assert_eq!(
        std::fs::read(dir.path().join(".hifimule.json.zst")).unwrap(),
        mirror_before,
        "failed authority must not publish a clean optional mirror"
    );
}

#[tokio::test]
async fn atomic_mtp_cache_replaces_complete_bytes_without_temporary_files() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("manifest.json");
    std::fs::write(&path, b"old bytes").unwrap();
    let mut manifest = make_manifest("atomic-cache", "MTP");
    manifest.dirty = true;
    manifest.pending_item_ids = vec!["pending".into()];
    persist_local_manifest_atomic(&path, &manifest)
        .await
        .unwrap();
    let persisted: DeviceManifest =
        manifest_codec::decode_test_manifest(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(persisted.device_id, manifest.device_id);
    assert!(persisted.dirty);
    assert_eq!(persisted.pending_item_ids, manifest.pending_item_ids);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[tokio::test]
async fn atomic_mtp_cache_reports_post_rename_durability_uncertainty() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("manifest.json");
    let manifest = make_manifest("uncertain-cache", "MTP");
    let error = persist_local_manifest_atomic_with_sync(&path, &manifest, |_| {
        Err(std::io::Error::other("injected directory sync failure"))
    })
    .await
    .unwrap_err();
    assert!(error.is::<ManifestCacheCommitUncertain>());
    // The failure must not be treated as a pre-rename failure: new bytes exist.
    let persisted: DeviceManifest =
        manifest_codec::decode_test_manifest(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(persisted.device_id, manifest.device_id);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[tokio::test]
async fn mtp_cache_directory_sync_failure_restores_dirty_recovery_evidence() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache.json");
    let manager = DeviceManager::new(Arc::new(crate::db::Database::memory().unwrap()));
    let mut manifest = make_manifest("cache-sync-failure", "MTP");
    manifest.dirty = true;
    manifest.pending_item_ids = vec!["unfinished".into()];
    persist_local_manifest_atomic(&cache, &manifest)
        .await
        .unwrap();
    manager
        .handle_device_detected(
            PathBuf::from("mtp://cache-sync-failure"),
            manifest.clone(),
            msc(dir.path()),
        )
        .await
        .unwrap();
    let result = manager
        .update_manifest_for_device_with_cache_path(
            &manifest.device_id,
            |updated| {
                updated.dirty = false;
                updated.pending_item_ids.clear();
            },
            |_| Ok(cache.clone()),
            |_| Err(std::io::Error::other("injected directory sync failure")),
        )
        .await;
    assert!(result.unwrap_err().is::<ManifestCacheCommitUncertain>());
    let restored = manager
        .get_manifest_for_device(&manifest.device_id)
        .await
        .unwrap();
    let persisted: DeviceManifest =
        manifest_codec::decode_test_manifest(&std::fs::read(&cache).unwrap()).unwrap();
    for recovered in [&restored, &persisted] {
        assert!(recovered.dirty);
        assert_eq!(recovered.pending_item_ids, manifest.pending_item_ids);
    }
}

#[test]
fn auto_fill_slot_id_recognizes_legacy_and_scoped_markers_only() {
    assert!(is_auto_fill_slot_id("__auto_fill_slot__"));
    assert!(is_auto_fill_slot_id("__auto_fill_slot__:server-id"));
    assert!(is_auto_fill_slot_id("__auto_fill_slot__:"));
    assert!(!is_auto_fill_slot_id("__auto_fill_slot__other"));
}
use serde_json;

#[cfg(target_os = "windows")]
fn dummy_mtp_device_info(id: &str) -> mtp::MtpDeviceInfo {
    mtp::MtpDeviceInfo {
        device_id: id.to_string(),
        friendly_name: "Test Device".to_string(),
        inner: mtp::MtpDeviceInner::Wpd {
            wpd_device_id: id.to_string(),
        },
    }
}

#[cfg(unix)]
fn dummy_mtp_device_info(id: &str) -> mtp::MtpDeviceInfo {
    mtp::MtpDeviceInfo {
        device_id: id.to_string(),
        friendly_name: "Test Device".to_string(),
        inner: mtp::MtpDeviceInner::Libmtp {
            bus_location: 0,
            dev_num: 0,
        },
    }
}
use std::fs;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;
use tempfile::tempdir;
use tokio::time::{Duration, timeout};

fn msc(dir: &std::path::Path) -> Arc<dyn crate::device_io::DeviceIO> {
    Arc::new(crate::device_io::MscBackend::new(dir.to_path_buf()))
}

#[tokio::test]
async fn passive_mtp_enumeration_retries_after_an_error_at_the_poll_cadence() {
    let (tx, _rx) = tokio::sync::mpsc::channel(1);
    let calls = Arc::new(AtomicUsize::new(0));
    let invocation_times = Arc::new(std::sync::Mutex::new(Vec::new()));
    let calls_for_enumerator = Arc::clone(&calls);
    let invocation_times_for_enumerator = Arc::clone(&invocation_times);
    let poll_interval = Duration::from_millis(20);
    let observer = tokio::spawn(run_mtp_observer_with_enumerator(
        tx,
        Arc::new(move || {
            invocation_times_for_enumerator
                .lock()
                .unwrap()
                .push(Instant::now());
            let call = calls_for_enumerator.fetch_add(1, Ordering::SeqCst);
            if call == 0 {
                Err(anyhow::anyhow!("injected passive scan failure"))
            } else {
                Ok(Vec::new())
            }
        }),
        poll_interval,
    ));

    timeout(Duration::from_secs(1), async {
        while calls.load(Ordering::SeqCst) < 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("observer should retry a failed passive scan");
    let invocation_times = invocation_times.lock().unwrap();
    assert!(
        invocation_times[1].duration_since(invocation_times[0]) >= poll_interval,
        "the observer must wait for the configured poll interval before retrying"
    );
    observer.abort();
}

#[tokio::test]
async fn aborted_mtp_observer_discards_a_late_passive_scan_result() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let release_rx = Arc::new(std::sync::Mutex::new(release_rx));
    let release_for_enumerator = Arc::clone(&release_rx);
    let observer = tokio::spawn(run_mtp_observer_with_enumerator(
        tx,
        Arc::new(move || {
            started_tx.send(()).unwrap();
            release_for_enumerator.lock().unwrap().recv().unwrap();
            Ok(vec![dummy_mtp_device_info("late-device")])
        }),
        Duration::from_millis(5),
    ));

    tokio::task::spawn_blocking(move || started_rx.recv_timeout(std::time::Duration::from_secs(1)))
        .await
        .unwrap()
        .unwrap();
    observer.abort();
    observer.await.unwrap_err();
    release_tx.send(()).unwrap();
    tokio::time::sleep(Duration::from_millis(25)).await;
    assert!(
        rx.try_recv().is_err(),
        "a late scan must not emit device work"
    );
}

#[cfg(target_os = "windows")]
#[test]
fn test_friendly_name_drive_root_parses_drive_letter() {
    assert_eq!(
        friendly_name_drive_root("g:\\").unwrap(),
        std::path::PathBuf::from("G:\\")
    );
    assert_eq!(
        friendly_name_drive_root(" G:/ ").unwrap(),
        std::path::PathBuf::from("G:\\")
    );
    assert!(friendly_name_drive_root("Garmin Watch").is_none());
}

#[derive(Debug)]
struct StorageIdDeviceIo {
    inner: crate::device_io::MscBackend,
    storage_id: Option<String>,
}

#[derive(Debug)]
struct FailingReadDeviceIo {
    inner: crate::device_io::MscBackend,
}

#[derive(Debug)]
struct FailingMirrorDeviceIo {
    inner: crate::device_io::MscBackend,
}

impl FailingMirrorDeviceIo {
    fn new(root: &std::path::Path) -> Arc<dyn crate::device_io::DeviceIO> {
        Arc::new(Self {
            inner: crate::device_io::MscBackend::new(root.to_path_buf()),
        })
    }
}

#[async_trait::async_trait]
impl crate::device_io::DeviceIO for FailingMirrorDeviceIo {
    async fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        self.inner.read_file(path).await
    }

    async fn write_file(&self, path: &str, data: &[u8]) -> Result<()> {
        self.inner.write_file(path, data).await
    }

    async fn write_with_verify(&self, _path: &str, _data: &[u8]) -> Result<()> {
        Err(anyhow::anyhow!("injected MTP mirror failure"))
    }

    async fn delete_file(&self, path: &str) -> Result<()> {
        self.inner.delete_file(path).await
    }

    async fn list_files(&self, path: &str) -> Result<Vec<crate::device_io::FileEntry>> {
        self.inner.list_files(path).await
    }

    async fn free_space(&self) -> Result<u64> {
        self.inner.free_space().await
    }

    async fn storage_id(&self) -> Result<Option<String>> {
        Ok(Some("mtp-storage".to_string()))
    }

    async fn ensure_dir(&self, path: &str) -> Result<()> {
        self.inner.ensure_dir(path).await
    }

    async fn cleanup_empty_subdirs(&self, path: &str) -> Result<()> {
        self.inner.cleanup_empty_subdirs(path).await
    }
}

impl FailingReadDeviceIo {
    fn new(root: &std::path::Path) -> Arc<dyn crate::device_io::DeviceIO> {
        Arc::new(Self {
            inner: crate::device_io::MscBackend::new(root.to_path_buf()),
        })
    }
}

#[async_trait::async_trait]
impl crate::device_io::DeviceIO for FailingReadDeviceIo {
    async fn read_file(&self, _path: &str) -> Result<Vec<u8>> {
        Err(anyhow::anyhow!("MTP transport temporarily unavailable"))
    }

    async fn write_file(&self, path: &str, data: &[u8]) -> Result<()> {
        self.inner.write_file(path, data).await
    }

    async fn write_with_verify(&self, path: &str, data: &[u8]) -> Result<()> {
        self.inner.write_with_verify(path, data).await
    }

    async fn delete_file(&self, path: &str) -> Result<()> {
        self.inner.delete_file(path).await
    }

    async fn list_files(&self, path: &str) -> Result<Vec<crate::device_io::FileEntry>> {
        self.inner.list_files(path).await
    }

    async fn free_space(&self) -> Result<u64> {
        self.inner.free_space().await
    }

    async fn ensure_dir(&self, path: &str) -> Result<()> {
        self.inner.ensure_dir(path).await
    }

    async fn cleanup_empty_subdirs(&self, path: &str) -> Result<()> {
        self.inner.cleanup_empty_subdirs(path).await
    }
}

impl StorageIdDeviceIo {
    fn new(
        root: &std::path::Path,
        storage_id: Option<String>,
    ) -> Arc<dyn crate::device_io::DeviceIO> {
        Arc::new(Self {
            inner: crate::device_io::MscBackend::new(root.to_path_buf()),
            storage_id,
        })
    }
}

#[async_trait::async_trait]
impl crate::device_io::DeviceIO for StorageIdDeviceIo {
    async fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        self.inner.read_file(path).await
    }

    async fn write_file(&self, path: &str, data: &[u8]) -> Result<()> {
        self.inner.write_file(path, data).await
    }

    async fn write_with_verify(&self, path: &str, data: &[u8]) -> Result<()> {
        self.inner.write_with_verify(path, data).await
    }

    async fn delete_file(&self, path: &str) -> Result<()> {
        self.inner.delete_file(path).await
    }

    async fn list_files(&self, path: &str) -> Result<Vec<crate::device_io::FileEntry>> {
        self.inner.list_files(path).await
    }

    async fn free_space(&self) -> Result<u64> {
        self.inner.free_space().await
    }

    async fn storage_id(&self) -> Result<Option<String>> {
        Ok(self.storage_id.clone())
    }

    async fn ensure_dir(&self, path: &str) -> Result<()> {
        self.inner.ensure_dir(path).await
    }

    async fn cleanup_empty_subdirs(&self, path: &str) -> Result<()> {
        self.inner.cleanup_empty_subdirs(path).await
    }
}

// ===== Story 4.4 Tests =====

#[test]
fn test_dirty_flag_serde_default() {
    let json = r#"{"device_id": "dev-1", "name": "iPod", "version": "1.0"}"#;
    let manifest: DeviceManifest = serde_json::from_str(json).unwrap();
    assert!(!manifest.dirty, "dirty must default to false");
    assert!(
        manifest.pending_item_ids.is_empty(),
        "pending_item_ids must default to []"
    );
}

#[tokio::test]
async fn test_dirty_manifest_roundtrip() {
    let dir = tempdir().unwrap();
    let manifest = DeviceManifest {
        device_id: "dev-1".to_string(),
        name: None,
        icon: None,
        version: "1.0".to_string(),
        managed_paths: vec![],
        synced_items: vec![],
        dirty: true,
        pending_item_ids: vec!["id-1".to_string(), "id-2".to_string()],
        basket_items: vec![],
        folder_artwork: Default::default(),
        folder_artwork_records: Default::default(),
        reconcile_before_sync: false,
        auto_sync_on_connect: false,
        auto_fill: crate::device::AutoFillConfig::default(),
        transcoding_profile_id: None,
        playlists: vec![],
        storage_id: None,
        ..Default::default()
    };
    write_manifest(msc(dir.path()), &manifest).await.unwrap();
    let content = manifest_codec::read_manifest_json(dir.path().join(".hifimule.json.zst"))
        .await
        .unwrap();
    let loaded: DeviceManifest = serde_json::from_str(&content).unwrap();
    assert!(loaded.dirty);
    assert_eq!(loaded.pending_item_ids, vec!["id-1", "id-2"]);
}

#[tokio::test]
async fn test_cleanup_tmp_files_no_music_dir() {
    let dir = tempdir().unwrap();
    let count = cleanup_tmp_files(msc(dir.path()), &["Music".to_string()])
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn test_cleanup_tmp_files_empty_music_dir() {
    let dir = tempdir().unwrap();
    tokio::fs::create_dir(dir.path().join("Music"))
        .await
        .unwrap();
    let count = cleanup_tmp_files(msc(dir.path()), &["Music".to_string()])
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn test_cleanup_tmp_files_finds_and_deletes() {
    let dir = tempdir().unwrap();
    let tmp_path = dir.path().join("Music").join("Artist").join("Album");
    tokio::fs::create_dir_all(&tmp_path).await.unwrap();
    let tmp_file = tmp_path.join("01 - Track.flac.tmp");
    tokio::fs::write(&tmp_file, b"partial").await.unwrap();
    assert!(tmp_file.exists());

    let count = cleanup_tmp_files(msc(dir.path()), &["Music".to_string()])
        .await
        .unwrap();
    assert_eq!(count, 1);
    assert!(!tmp_file.exists(), ".tmp file must be deleted");
}

#[tokio::test]
async fn test_cleanup_tmp_files_nested_multiple() {
    let dir = tempdir().unwrap();
    let music = dir.path().join("Music");
    tokio::fs::create_dir_all(music.join("Artist1").join("Album1"))
        .await
        .unwrap();
    tokio::fs::create_dir_all(music.join("Artist2").join("Album2"))
        .await
        .unwrap();
    tokio::fs::create_dir_all(music.join("Artist3"))
        .await
        .unwrap();

    tokio::fs::write(
        music.join("Artist1").join("Album1").join("01.flac.tmp"),
        b"a",
    )
    .await
    .unwrap();
    tokio::fs::write(
        music.join("Artist2").join("Album2").join("02.flac.tmp"),
        b"b",
    )
    .await
    .unwrap();
    tokio::fs::write(music.join("Artist3").join("03.flac.tmp"), b"c")
        .await
        .unwrap();

    let count = cleanup_tmp_files(msc(dir.path()), &["Music".to_string()])
        .await
        .unwrap();
    assert_eq!(count, 3);
}

#[tokio::test]
async fn test_cleanup_tmp_files_non_tmp_preserved() {
    let dir = tempdir().unwrap();
    let music_dir = dir.path().join("Music").join("Artist");
    tokio::fs::create_dir_all(&music_dir).await.unwrap();

    let flac_file = music_dir.join("track.flac");
    let mp3_file = music_dir.join("track.mp3");
    let tmp_file = music_dir.join("track.flac.tmp");

    tokio::fs::write(&flac_file, b"real flac").await.unwrap();
    tokio::fs::write(&mp3_file, b"real mp3").await.unwrap();
    tokio::fs::write(&tmp_file, b"partial").await.unwrap();

    let count = cleanup_tmp_files(msc(dir.path()), &["Music".to_string()])
        .await
        .unwrap();
    assert_eq!(count, 1, "Only .tmp file should be deleted");
    assert!(flac_file.exists(), ".flac file must be preserved");
    assert!(mp3_file.exists(), ".mp3 file must be preserved");
    assert!(!tmp_file.exists(), ".tmp file must be deleted");
}

#[tokio::test]
async fn test_probe_no_manifest() {
    let dir = tempdir().unwrap();
    let res = DeviceProber::probe(dir.path()).await.unwrap();
    assert!(res.is_none());
}

#[tokio::test]
async fn test_probe_with_manifest() {
    let dir = tempdir().unwrap();
    let manifest_path = dir.path().join(".hifimule.json");
    let manifest_json = r#"{"device_id": "test-device-123", "name": "My iPod", "version": "1.0"}"#;
    fs::write(manifest_path, manifest_json).unwrap();

    let res = DeviceProber::probe(dir.path()).await.unwrap();
    assert!(res.is_some());
    let manifest = res.unwrap();
    assert_eq!(manifest.device_id, "test-device-123");
    assert_eq!(manifest.name, Some("My iPod".to_string()));
    assert_eq!(manifest.version, "1.0".to_string());
    assert!(manifest.managed_paths.is_empty());
}

#[tokio::test]
async fn test_probe_with_managed_paths() {
    let dir = tempdir().unwrap();
    let manifest_path = dir.path().join(".hifimule.json");
    let manifest_json = r#"{"device_id": "test-device-123", "name": "My iPod", "version": "1.0", "managed_paths": ["Music", "Podcasts"]}"#;
    fs::write(manifest_path, manifest_json).unwrap();

    let res = DeviceProber::probe(dir.path()).await.unwrap();
    assert!(res.is_some());
    let manifest = res.unwrap();
    assert_eq!(
        manifest.managed_paths,
        vec!["Music".to_string(), "Podcasts".to_string()]
    );
}

#[tokio::test]
async fn test_probe_invalid_manifest() {
    let dir = tempdir().unwrap();
    let manifest_path = dir.path().join(".hifimule.json");
    let manifest_json = r#"{"id": "test-device-123", "invalid": true"#; // Missing closing brace
    fs::write(manifest_path, manifest_json).unwrap();

    let res = DeviceProber::probe(dir.path()).await;
    assert!(res.is_err());
}

#[tokio::test]
async fn test_list_root_folders_mixed() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // Create directories
    fs::create_dir(root.join("Music")).unwrap();
    fs::create_dir(root.join("Notes")).unwrap();
    fs::create_dir(root.join("Podcasts")).unwrap();
    fs::create_dir(root.join(".hidden")).unwrap();
    fs::create_dir(root.join("System Volume Information")).unwrap();
    fs::write(root.join("file.txt"), "not a dir").unwrap();

    // Create manifest
    let manifest_path = root.join(".hifimule.json");
    let manifest_json = r#"{"device_id": "test-id", "name": "Test Device", "version": "1.0", "managed_paths": ["Music"]}"#;
    fs::write(manifest_path, manifest_json).unwrap();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    // Simulate detection
    let manifest: DeviceManifest = serde_json::from_str(manifest_json).unwrap();
    manager
        .handle_device_detected(root.to_path_buf(), manifest, msc(root))
        .await
        .unwrap();

    let res = manager.list_root_folders().await.unwrap().unwrap();

    assert_eq!(res.folders.len(), 3);
    assert_eq!(res.managed_count, 1);
    assert_eq!(res.unmanaged_count, 2);

    // Sorted alphabetically
    assert_eq!(res.folders[0].name, "Music");
    assert!(res.folders[0].is_managed);

    assert_eq!(res.folders[1].name, "Notes");
    assert!(!res.folders[1].is_managed);

    assert_eq!(res.folders[2].name, "Podcasts");
    assert!(!res.folders[2].is_managed);
}

#[tokio::test]
async fn test_list_root_folders_empty_device() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // Create manifest but no folders (only hidden/system entries)
    let manifest_path = root.join(".hifimule.json");
    let manifest_json = r#"{"device_id": "empty-id", "name": "Empty Device", "version": "1.0"}"#;
    fs::write(manifest_path, manifest_json).unwrap();

    // Add a hidden folder and a file (should be skipped)
    fs::create_dir(root.join(".hidden")).unwrap();
    fs::write(root.join("readme.txt"), "file").unwrap();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    let manifest: DeviceManifest = serde_json::from_str(manifest_json).unwrap();
    manager
        .handle_device_detected(root.to_path_buf(), manifest, msc(root))
        .await
        .unwrap();

    let res = manager.list_root_folders().await.unwrap().unwrap();

    assert_eq!(res.folders.len(), 0);
    assert_eq!(res.managed_count, 0);
    assert_eq!(res.unmanaged_count, 0);
    assert!(res.has_manifest);
    assert_eq!(res.device_name, "Empty Device");
}

#[tokio::test]
async fn test_list_root_folders_no_device() {
    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    // No device detected — should return None
    let res = manager.list_root_folders().await.unwrap();
    assert!(res.is_none());
}

#[test]
fn test_backward_compat_manifest_without_synced_items() {
    // Old-format manifest without synced_items field
    let json = r#"{"device_id": "old-device", "name": "Old iPod", "version": "1.0", "managed_paths": ["Music"]}"#;
    let manifest: DeviceManifest = serde_json::from_str(json).unwrap();

    assert_eq!(manifest.device_id, "old-device");
    assert_eq!(manifest.name, Some("Old iPod".to_string()));
    assert!(manifest.synced_items.is_empty()); // Default empty vec
}

#[test]
fn test_manifest_with_synced_items_deserialization() {
    let json = r#"{
        "device_id": "dev-1",
        "name": "My iPod",
        "version": "1.1",
        "managed_paths": ["Music"],
        "synced_items": [
            {
                "providerItemId": "item-1",
                "name": "Track One",
                "album": "Album A",
                "artist": "Artist X",
                "localPath": "Music/Artist X/Album A/01 - Track One.flac",
                "sizeBytes": 34521088,
                "syncedAt": "2026-02-15T10:30:00Z"
            }
        ]
    }"#;
    let manifest: DeviceManifest = serde_json::from_str(json).unwrap();

    assert_eq!(manifest.synced_items.len(), 1);
    assert_eq!(manifest.synced_items[0].jellyfin_id, "item-1");
    assert_eq!(manifest.synced_items[0].name, "Track One");
    assert_eq!(manifest.synced_items[0].album, Some("Album A".to_string()));
    assert_eq!(
        manifest.synced_items[0].artist,
        Some("Artist X".to_string())
    );
    assert_eq!(
        manifest.synced_items[0].local_path,
        "Music/Artist X/Album A/01 - Track One.flac"
    );
    assert_eq!(manifest.synced_items[0].size_bytes, 34521088);
}

#[tokio::test]
async fn test_write_manifest_creates_files() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    let manifest = DeviceManifest {
        device_id: "test-device".to_string(),
        name: Some("Test".to_string()),
        icon: None,
        version: "1.1".to_string(),
        managed_paths: vec!["Music".to_string()],
        synced_items: vec![SyncedItem {
            is_auto_fill: false,
            media_role: crate::device::MediaRole::Music,
            jellyfin_id: "item-1".to_string(),
            name: "Track".to_string(),
            album: Some("Album".to_string()),
            artist: Some("Artist".to_string()),
            local_path: "Music/Artist/Album/Track.flac".to_string(),
            size_bytes: 1000,
            synced_at: "2026-02-15T10:00:00Z".to_string(),
            original_name: None,
            etag: None,
            provider_album_id: None,
            provider_content_type: None,
            provider_suffix: None,
            original_bitrate: None,
            original_container: None,
            track_number: None,
            server_id: None,
        }],
        dirty: false,
        pending_item_ids: vec![],
        basket_items: vec![],
        folder_artwork: Default::default(),
        folder_artwork_records: Default::default(),
        reconcile_before_sync: false,
        auto_sync_on_connect: false,
        auto_fill: crate::device::AutoFillConfig::default(),
        transcoding_profile_id: None,
        playlists: vec![],
        storage_id: None,
        ..Default::default()
    };

    write_manifest(msc(root), &manifest).await.unwrap();

    // Verify the manifest file exists (not the temp file)
    let manifest_path = root.join(".hifimule.json.zst");
    let tmp_path = root.join(".hifimule.json.zst.tmp");
    assert!(manifest_path.exists());
    assert!(!tmp_path.exists());

    // Verify content can be read back
    let content = manifest_codec::read_manifest_json_sync(&manifest_path).unwrap();
    assert!(
        content.contains("\"providerItemId\""),
        "new manifests must use provider-neutral item IDs"
    );
    assert!(
        !content.contains("\"jellyfinId\""),
        "new manifests must not write the Jellyfin-specific synced item key"
    );
    let loaded: DeviceManifest = serde_json::from_str(&content).unwrap();
    assert_eq!(loaded.device_id, "test-device");
    assert_eq!(loaded.synced_items.len(), 1);
    assert_eq!(loaded.synced_items[0].jellyfin_id, "item-1");
}

#[tokio::test]
async fn test_write_manifest_overwrites_existing() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // Write initial manifest
    let manifest1 = DeviceManifest {
        device_id: "dev-1".to_string(),
        name: Some("First".to_string()),
        icon: None,
        version: "1.0".to_string(),
        managed_paths: vec![],
        synced_items: vec![],
        dirty: false,
        pending_item_ids: vec![],
        basket_items: vec![],
        folder_artwork: Default::default(),
        folder_artwork_records: Default::default(),
        reconcile_before_sync: false,
        auto_sync_on_connect: false,
        auto_fill: crate::device::AutoFillConfig::default(),
        transcoding_profile_id: None,
        playlists: vec![],
        storage_id: None,
        ..Default::default()
    };
    write_manifest(msc(root), &manifest1).await.unwrap();

    // Overwrite with updated manifest
    let manifest2 = DeviceManifest {
        device_id: "dev-1".to_string(),
        name: Some("Updated".to_string()),
        icon: None,
        version: "1.1".to_string(),
        managed_paths: vec!["Music".to_string()],
        synced_items: vec![SyncedItem {
            is_auto_fill: false,
            media_role: crate::device::MediaRole::Music,
            jellyfin_id: "new-item".to_string(),
            name: "New Track".to_string(),
            album: None,
            artist: None,
            local_path: "Music/track2.flac".to_string(),
            size_bytes: 200,
            synced_at: "2026-02-15T13:00:00Z".to_string(),
            original_name: None,
            etag: None,
            provider_album_id: None,
            provider_content_type: None,
            provider_suffix: None,
            original_bitrate: None,
            original_container: None,
            track_number: None,
            server_id: None,
        }],
        dirty: false,
        pending_item_ids: vec![],
        basket_items: vec![],
        folder_artwork: Default::default(),
        folder_artwork_records: Default::default(),
        reconcile_before_sync: false,
        auto_sync_on_connect: false,
        auto_fill: crate::device::AutoFillConfig::default(),
        transcoding_profile_id: None,
        playlists: vec![],
        storage_id: None,
        ..Default::default()
    };
    write_manifest(msc(root), &manifest2).await.unwrap();

    let content = manifest_codec::read_manifest_json_sync(root.join(".hifimule.json.zst")).unwrap();
    let loaded: DeviceManifest = serde_json::from_str(&content).unwrap();
    assert_eq!(loaded.name, Some("Updated".to_string()));
    assert_eq!(loaded.synced_items.len(), 1);
    assert_eq!(loaded.synced_items[0].jellyfin_id, "new-item");
}

// ===== Story 5.4 Tests =====

#[tokio::test]
async fn test_get_discrepancies_missing_file() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // Create managed path but no files in it
    fs::create_dir_all(root.join("Music").join("Artist").join("Album")).unwrap();

    // Write manifest with a synced item that DOESN'T exist on disk
    let manifest = DeviceManifest {
        device_id: "dev-1".to_string(),
        name: Some("Test Device".to_string()),
        icon: None,
        version: "1.0".to_string(),
        managed_paths: vec!["Music".to_string()],
        synced_items: vec![SyncedItem {
            is_auto_fill: false,
            media_role: crate::device::MediaRole::Music,
            jellyfin_id: "item-1".to_string(),
            name: "Track One".to_string(),
            album: Some("Album".to_string()),
            artist: Some("Artist".to_string()),
            local_path: "Music/Artist/Album/01 - Track One.flac".to_string(),
            size_bytes: 1000,
            synced_at: "2026-02-28T10:00:00Z".to_string(),
            original_name: None,
            etag: None,
            provider_album_id: None,
            provider_content_type: None,
            provider_suffix: None,
            original_bitrate: None,
            original_container: None,
            track_number: None,
            server_id: None,
        }],
        dirty: true,
        pending_item_ids: vec![],
        basket_items: vec![],
        folder_artwork: Default::default(),
        folder_artwork_records: Default::default(),
        reconcile_before_sync: false,
        auto_sync_on_connect: false,
        auto_fill: crate::device::AutoFillConfig::default(),
        transcoding_profile_id: None,
        playlists: vec![],
        storage_id: None,
        ..Default::default()
    };
    write_manifest(msc(root), &manifest).await.unwrap();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);
    manager
        .handle_device_detected(root.to_path_buf(), manifest, msc(root))
        .await
        .unwrap();

    let discrepancies = manager.get_discrepancies().await.unwrap().unwrap();
    assert_eq!(
        discrepancies.missing.len(),
        1,
        "Should detect one missing file"
    );
    assert_eq!(discrepancies.missing[0].jellyfin_id, "item-1");
    assert_eq!(discrepancies.orphaned.len(), 0);
}

#[tokio::test]
async fn test_get_discrepancies_orphaned_file() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // Create managed path with a file
    let music_path = root.join("Music").join("Rogue").join("Album");
    fs::create_dir_all(&music_path).unwrap();
    fs::write(music_path.join("track.flac"), b"audio data").unwrap();

    // Write manifest WITHOUT that file
    let manifest = DeviceManifest {
        device_id: "dev-1".to_string(),
        name: Some("Test Device".to_string()),
        icon: None,
        version: "1.0".to_string(),
        managed_paths: vec!["Music".to_string()],
        synced_items: vec![],
        dirty: true,
        pending_item_ids: vec![],
        basket_items: vec![],
        folder_artwork: Default::default(),
        folder_artwork_records: Default::default(),
        reconcile_before_sync: false,
        auto_sync_on_connect: false,
        auto_fill: crate::device::AutoFillConfig::default(),
        transcoding_profile_id: None,
        playlists: vec![],
        storage_id: None,
        ..Default::default()
    };
    write_manifest(msc(root), &manifest).await.unwrap();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);
    manager
        .handle_device_detected(root.to_path_buf(), manifest, msc(root))
        .await
        .unwrap();

    let discrepancies = manager.get_discrepancies().await.unwrap().unwrap();
    assert_eq!(discrepancies.missing.len(), 0);
    assert_eq!(
        discrepancies.orphaned.len(),
        1,
        "Should detect one orphaned file"
    );
    assert_eq!(
        discrepancies.orphaned[0].local_path,
        "Music/Rogue/Album/track.flac"
    );
}

#[tokio::test]
async fn test_get_discrepancies_no_issues() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // Create managed path with files matching the manifest
    let music_path = root.join("Music").join("Artist").join("Album");
    fs::create_dir_all(&music_path).unwrap();
    fs::write(music_path.join("01 - Track.flac"), b"audio data").unwrap();

    let manifest = DeviceManifest {
        device_id: "dev-1".to_string(),
        name: None,
        icon: None,
        version: "1.0".to_string(),
        managed_paths: vec!["Music".to_string()],
        synced_items: vec![SyncedItem {
            is_auto_fill: false,
            media_role: crate::device::MediaRole::Music,
            jellyfin_id: "item-1".to_string(),
            name: "Track".to_string(),
            album: None,
            artist: None,
            local_path: "Music/Artist/Album/01 - Track.flac".to_string(),
            size_bytes: 1000,
            synced_at: "2026-02-28T10:00:00Z".to_string(),
            original_name: None,
            etag: None,
            provider_album_id: None,
            provider_content_type: None,
            provider_suffix: None,
            original_bitrate: None,
            original_container: None,
            track_number: None,
            server_id: None,
        }],
        dirty: false,
        pending_item_ids: vec![],
        basket_items: vec![],
        folder_artwork: Default::default(),
        folder_artwork_records: Default::default(),
        reconcile_before_sync: false,
        auto_sync_on_connect: false,
        auto_fill: crate::device::AutoFillConfig::default(),
        transcoding_profile_id: None,
        playlists: vec![],
        storage_id: None,
        ..Default::default()
    };
    write_manifest(msc(root), &manifest).await.unwrap();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);
    manager
        .handle_device_detected(root.to_path_buf(), manifest, msc(root))
        .await
        .unwrap();

    let discrepancies = manager.get_discrepancies().await.unwrap().unwrap();
    assert_eq!(discrepancies.missing.len(), 0);
    assert_eq!(discrepancies.orphaned.len(), 0);
}

#[tokio::test]
async fn test_prune_items() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    fs::create_dir(root.join("Music")).unwrap();

    let manifest = DeviceManifest {
        device_id: "dev-1".to_string(),
        name: None,
        icon: None,
        version: "1.0".to_string(),
        managed_paths: vec!["Music".to_string()],
        synced_items: vec![
            SyncedItem {
                is_auto_fill: false,
                media_role: crate::device::MediaRole::Music,
                jellyfin_id: "item-1".to_string(),
                name: "Track 1".to_string(),
                album: None,
                artist: None,
                local_path: "Music/track1.flac".to_string(),
                size_bytes: 100,
                synced_at: "2026-02-28T10:00:00Z".to_string(),
                original_name: None,
                etag: None,
                provider_album_id: None,
                provider_content_type: None,
                provider_suffix: None,
                original_bitrate: None,
                original_container: None,
                track_number: None,
                server_id: None,
            },
            SyncedItem {
                is_auto_fill: false,
                media_role: crate::device::MediaRole::Music,
                jellyfin_id: "item-2".to_string(),
                name: "Track 2".to_string(),
                album: None,
                artist: None,
                local_path: "Music/track2.flac".to_string(),
                size_bytes: 200,
                synced_at: "2026-02-28T10:00:00Z".to_string(),
                original_name: None,
                etag: None,
                provider_album_id: None,
                provider_content_type: None,
                provider_suffix: None,
                original_bitrate: None,
                original_container: None,
                track_number: None,
                server_id: None,
            },
        ],
        dirty: true,
        pending_item_ids: vec![],
        basket_items: vec![],
        folder_artwork: Default::default(),
        folder_artwork_records: Default::default(),
        reconcile_before_sync: false,
        auto_sync_on_connect: false,
        auto_fill: crate::device::AutoFillConfig::default(),
        transcoding_profile_id: None,
        playlists: vec![],
        storage_id: None,
        ..Default::default()
    };
    write_manifest(msc(root), &manifest).await.unwrap();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);
    manager
        .handle_device_detected(root.to_path_buf(), manifest, msc(root))
        .await
        .unwrap();

    let removed = manager.prune_items(&["item-1".to_string()]).await.unwrap();
    assert_eq!(removed, 1);

    let device = manager.get_current_device().await.unwrap();
    assert_eq!(device.synced_items.len(), 1);
    assert_eq!(device.synced_items[0].jellyfin_id, "item-2");

    // Verify persisted to disk
    let content = manifest_codec::read_manifest_json_sync(root.join(".hifimule.json.zst")).unwrap();
    let loaded: DeviceManifest = serde_json::from_str(&content).unwrap();
    assert_eq!(loaded.synced_items.len(), 1);
}

#[tokio::test]
async fn test_relink_item() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    fs::create_dir(root.join("Music")).unwrap();

    let manifest = DeviceManifest {
        device_id: "dev-1".to_string(),
        name: None,
        icon: None,
        version: "1.0".to_string(),
        managed_paths: vec!["Music".to_string()],
        synced_items: vec![SyncedItem {
            is_auto_fill: false,
            media_role: crate::device::MediaRole::Music,
            jellyfin_id: "item-1".to_string(),
            name: "Track".to_string(),
            album: None,
            artist: None,
            local_path: "Music/old_path.flac".to_string(),
            size_bytes: 100,
            synced_at: "2026-02-28T10:00:00Z".to_string(),
            original_name: None,
            etag: None,
            provider_album_id: None,
            provider_content_type: None,
            provider_suffix: None,
            original_bitrate: None,
            original_container: None,
            track_number: None,
            server_id: None,
        }],
        dirty: true,
        pending_item_ids: vec![],
        basket_items: vec![],
        folder_artwork: Default::default(),
        folder_artwork_records: Default::default(),
        reconcile_before_sync: false,
        auto_sync_on_connect: false,
        auto_fill: crate::device::AutoFillConfig::default(),
        transcoding_profile_id: None,
        playlists: vec![],
        storage_id: None,
        ..Default::default()
    };
    write_manifest(msc(root), &manifest).await.unwrap();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);
    manager
        .handle_device_detected(root.to_path_buf(), manifest, msc(root))
        .await
        .unwrap();

    fs::write(root.join("Music").join("new_path.flac"), b"audio data").unwrap();

    let found = manager
        .relink_item("item-1", "Music/new_path.flac")
        .await
        .unwrap();
    assert!(found);

    let device = manager.get_current_device().await.unwrap();
    assert_eq!(device.synced_items[0].local_path, "Music/new_path.flac");
    assert_eq!(
        device.synced_items[0].original_name,
        Some("Music/old_path.flac".to_string())
    );
}

#[tokio::test]
async fn test_relink_item_path_traversal() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    fs::create_dir(root.join("Music")).unwrap();

    let manifest = DeviceManifest {
        device_id: "dev-1".to_string(),
        name: None,
        icon: None,
        version: "1.0".to_string(),
        managed_paths: vec!["Music".to_string()],
        synced_items: vec![],
        dirty: false,
        pending_item_ids: vec![],
        basket_items: vec![],
        folder_artwork: Default::default(),
        folder_artwork_records: Default::default(),
        reconcile_before_sync: false,
        auto_sync_on_connect: false,
        auto_fill: crate::device::AutoFillConfig::default(),
        transcoding_profile_id: None,
        playlists: vec![],
        storage_id: None,
        ..Default::default()
    };
    write_manifest(msc(root), &manifest).await.unwrap();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);
    manager
        .handle_device_detected(root.to_path_buf(), manifest, msc(root))
        .await
        .unwrap();

    let res = manager.relink_item("item-1", "../secret.txt").await;
    assert!(res.is_err());
    assert!(res.unwrap_err().to_string().contains("path traversal"));
}

#[tokio::test]
async fn test_clear_dirty_flag() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    let manifest = DeviceManifest {
        device_id: "dev-1".to_string(),
        name: None,
        icon: None,
        version: "1.0".to_string(),
        managed_paths: vec![],
        synced_items: vec![],
        dirty: true,
        pending_item_ids: vec!["pending-1".to_string()],
        basket_items: vec![],
        folder_artwork: Default::default(),
        folder_artwork_records: Default::default(),
        reconcile_before_sync: false,
        auto_sync_on_connect: false,
        auto_fill: crate::device::AutoFillConfig::default(),
        transcoding_profile_id: None,
        playlists: vec![],
        storage_id: None,
        ..Default::default()
    };
    write_manifest(msc(root), &manifest).await.unwrap();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);
    manager
        .handle_device_detected(root.to_path_buf(), manifest, msc(root))
        .await
        .unwrap();

    manager.clear_dirty_flag().await.unwrap();

    let device = manager.get_current_device().await.unwrap();
    assert!(!device.dirty);
    assert!(device.pending_item_ids.is_empty());

    // Verify persisted to disk
    let content = manifest_codec::read_manifest_json_sync(root.join(".hifimule.json.zst")).unwrap();
    let loaded: DeviceManifest = serde_json::from_str(&content).unwrap();
    assert!(!loaded.dirty);
    assert!(loaded.pending_item_ids.is_empty());
}

#[tokio::test]
async fn test_clear_dirty_flag_with_orphaned_files() {
    // Orphaned files (on disk but not in manifest) must NOT block clearing the dirty flag.
    let dir = tempdir().unwrap();
    let root = dir.path();
    fs::create_dir(root.join("Music")).unwrap();
    fs::write(root.join("Music").join("orphan.flac"), b"audio").unwrap();

    let manifest = DeviceManifest {
        device_id: "dev-1".to_string(),
        name: None,
        icon: None,
        version: "1.0".to_string(),
        managed_paths: vec!["Music".to_string()],
        synced_items: vec![], // nothing tracked — orphan.flac is an orphan
        dirty: true,
        pending_item_ids: vec!["pending-1".to_string()],
        basket_items: vec![],
        folder_artwork: Default::default(),
        folder_artwork_records: Default::default(),
        reconcile_before_sync: false,
        auto_sync_on_connect: false,
        auto_fill: crate::device::AutoFillConfig::default(),
        transcoding_profile_id: None,
        playlists: vec![],
        storage_id: None,
        ..Default::default()
    };
    write_manifest(msc(root), &manifest).await.unwrap();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);
    manager
        .handle_device_detected(root.to_path_buf(), manifest, msc(root))
        .await
        .unwrap();

    // Should succeed: no missing files even though orphan.flac exists
    manager.clear_dirty_flag().await.unwrap();

    let device = manager.get_current_device().await.unwrap();
    assert!(!device.dirty);
    assert!(device.pending_item_ids.is_empty());
}

// ===== Story 2.6 Tests =====

#[tokio::test]
async fn test_handle_device_unrecognized_stores_path() {
    let dir = tempdir().unwrap();
    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    // Initially no unrecognized path
    assert!(manager.get_unrecognized_device_path().await.is_none());
    assert!(manager.get_current_device().await.is_none());

    let state = manager
        .handle_device_unrecognized(dir.path().to_path_buf(), msc(dir.path()), None)
        .await;

    // Path should now be set
    let path = manager.get_unrecognized_device_path().await;
    assert!(path.is_some());
    assert_eq!(path.unwrap(), dir.path());

    // State should be DeviceFound with the path string
    match state {
        crate::DaemonState::DeviceFound(s) => {
            assert!(s.contains(&*dir.path().to_string_lossy()));
        }
        _ => panic!("Expected DeviceFound state"),
    }
}

#[tokio::test]
async fn test_handle_device_removed_clears_unrecognized_path() {
    let dir = tempdir().unwrap();
    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    let path = dir.path().to_path_buf();
    manager
        .handle_device_unrecognized(path.clone(), msc(dir.path()), None)
        .await;
    assert!(manager.get_unrecognized_device_path().await.is_some());

    manager.handle_device_removed(&path).await;

    assert!(manager.get_unrecognized_device_path().await.is_none());
    assert!(manager.get_current_device().await.is_none());
}

#[tokio::test]
async fn test_handle_device_detected_clears_unrecognized_path() {
    let dir = tempdir().unwrap();
    let manifest_path = dir.path().join(".hifimule.json");
    let manifest_json = r#"{"device_id": "dev-1", "name": "My Device", "version": "1.0"}"#;
    fs::write(&manifest_path, manifest_json).unwrap();
    let manifest: DeviceManifest = serde_json::from_str(manifest_json).unwrap();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    // Set unrecognized path first
    manager
        .handle_device_unrecognized(dir.path().to_path_buf(), msc(dir.path()), None)
        .await;
    assert!(manager.get_unrecognized_device_path().await.is_some());

    // Detect device (recognized)
    manager
        .handle_device_detected(dir.path().to_path_buf(), manifest, msc(dir.path()))
        .await
        .unwrap();

    // Unrecognized path should be cleared
    assert!(manager.get_unrecognized_device_path().await.is_none());
    assert!(manager.get_current_device().await.is_some());
}

#[tokio::test]
async fn test_list_root_folders_unrecognized_device() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // No manifest — folders should still be listed via unrecognized path fallback
    fs::create_dir(root.join("Music")).unwrap();
    fs::create_dir(root.join("Podcasts")).unwrap();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    manager
        .handle_device_unrecognized(root.to_path_buf(), msc(root), None)
        .await;

    let res = manager.list_root_folders().await.unwrap().unwrap();

    assert_eq!(res.folders.len(), 2);
    assert!(!res.has_manifest, "No manifest should be present");
    assert_eq!(res.managed_count, 0, "No managed paths without manifest");
}

#[tokio::test]
async fn test_list_root_folders_unrecognized_mtp_device() {
    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    let mtp_path = PathBuf::from("mtp://fake-device-id");
    manager
        .handle_device_unrecognized(
            mtp_path,
            msc(std::path::Path::new(".")),
            Some("Garmin Watch".to_string()),
        )
        .await;

    let res = manager.list_root_folders().await.unwrap().unwrap();

    assert!(!res.has_manifest, "Unrecognized MTP device has no manifest");
    assert!(
        res.folders.is_empty(),
        "Unrecognized MTP device has no known folders"
    );
    assert_eq!(res.managed_count, 0);
    assert_eq!(res.unmanaged_count, 0);
    assert_eq!(
        res.device_name, "Garmin Watch",
        "friendly_name should be used as device_name"
    );
}

#[tokio::test]
async fn test_list_root_folders_unrecognized_mtp_device_no_friendly_name() {
    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    let mtp_path = PathBuf::from("mtp://fake-device-id");
    manager
        .handle_device_unrecognized(mtp_path, msc(std::path::Path::new(".")), None)
        .await;

    let res = manager.list_root_folders().await.unwrap().unwrap();

    assert!(!res.has_manifest);
    assert_eq!(
        res.device_name, "MTP Device",
        "Should fall back to 'MTP Device' when no friendly_name"
    );
}

#[tokio::test]
async fn test_initialize_device_root() {
    let dir = tempdir().unwrap();
    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    manager
        .handle_device_unrecognized(dir.path().to_path_buf(), msc(dir.path()), None)
        .await;

    // Initialize with root (empty folder_path)
    let manifest = manager
        .initialize_device(
            "",
            None,
            None,
            "My Device".to_string(),
            None,
            msc(dir.path()),
        )
        .await
        .unwrap();

    assert!(manifest.managed_paths.is_empty());
    assert_eq!(manifest.version, "1.0");
    assert!(!manifest.dirty);
    assert!(manifest.synced_items.is_empty());

    // Manifest file should exist on disk
    let manifest_path = dir.path().join(".hifimule.json.zst");
    assert!(manifest_path.exists(), ".hifimule.json.zst must be created");

    // Device should now be current (recognized)
    assert!(manager.get_current_device().await.is_some());
    assert!(manager.get_unrecognized_device_path().await.is_none());
    assert!(manager.get_current_device_path().await.is_some());
}

#[tokio::test]
async fn initialize_rejects_removed_root_without_recreating_it() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("removed-device");
    std::fs::create_dir(&root).unwrap();
    let manager = DeviceManager::new(Arc::new(crate::db::Database::memory().unwrap()));
    manager.handle_device_unrecognized(root.clone(), msc(&root), None).await;
    std::fs::remove_dir(&root).unwrap();
    let error = manager.initialize_device("Music", None, None, "Device".into(), None, msc(&root))
        .await.unwrap_err().to_string();
    assert!(error.contains("Cannot access device root"));
    assert!(error.contains(root.to_str().unwrap()));
    assert!(error.contains("os error"), "RPC message must preserve underlying cause: {error}");
    assert!(!root.exists(), "later writes must not recreate an unavailable mount");
    assert!(manager.get_current_device().await.is_none());
}

#[cfg(unix)]
#[tokio::test]
async fn initialize_succeeds_with_protected_child() {
    let dir = tempdir().unwrap();
    let child = dir.path().join(".Spotlight-V100");
    std::fs::create_dir(&child).unwrap();
    let manager = DeviceManager::new(Arc::new(crate::db::Database::memory().unwrap()));
    manager.handle_device_unrecognized(dir.path().into(), msc(dir.path()), None).await;
    let denied = crate::device_io::tests::DeniedDirectory::new(&child);
    let denial = std::fs::read_dir(&child);
    let result = manager.initialize_device("Music", None, None, "Device".into(), None, msc(dir.path())).await;
    drop(denied);
    match denial {
        Err(error) => assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied),
        Ok(_) => eprintln!("protected-child denial not reproduced: this user can read mode-000 directories"),
    }
    let manifest = result.unwrap();
    assert_eq!(manifest.managed_paths, vec!["Music".to_string()]);
    assert!(dir.path().join("Music").is_dir());
    assert!(dir.path().join(".hifimule.json.zst").is_file());
    assert!(manager.get_current_device().await.is_some());
}

#[cfg(unix)]
#[tokio::test]
async fn initialize_rejects_unreadable_root_before_writing() {
    let dir = tempdir().unwrap();
    let manager = DeviceManager::new(Arc::new(crate::db::Database::memory().unwrap()));
    manager.handle_device_unrecognized(dir.path().into(), msc(dir.path()), None).await;
    let denied = crate::device_io::tests::DeniedDirectory::new(dir.path());
    let denial = std::fs::read_dir(dir.path());
    if denial.is_ok() {
        drop(denied);
        eprintln!("unreadable-root test skipped: this user can read mode-000 directories");
        return;
    }
    let result = manager.initialize_device("Music", None, None, "Device".into(), None, msc(dir.path())).await;
    drop(denied);
    assert_eq!(denial.unwrap_err().kind(), std::io::ErrorKind::PermissionDenied);
    let error = result.unwrap_err().to_string();
    assert!(error.contains("Cannot access device root"));
    assert!(error.contains(dir.path().to_str().unwrap()));
    assert!(error.contains("Permission denied"));
    assert!(manager.get_current_device().await.is_none());
    assert!(!dir.path().join("Music").exists());
    assert!(!dir.path().join(".hifimule.json.zst").exists());
}

#[tokio::test]
async fn initialize_mtp_keeps_authoritative_cache_when_device_mirror_fails() {
    let dir = tempdir().unwrap();
    let cache_dir = tempdir().unwrap();
    let db = Arc::new(crate::db::Database::memory().unwrap());
    let cache_root = cache_dir.path().to_path_buf();
    let manager = DeviceManager::new_with_mtp_manifest_cache_path(
        db,
        Arc::new(move |device_id| Ok(cache_root.join(format!("{device_id}.json")))),
    );
    let device_io = FailingMirrorDeviceIo::new(dir.path());
    manager
        .handle_device_unrecognized(
            PathBuf::from("mtp://mirror-failure"),
            Arc::clone(&device_io),
            Some("Garmin Watch".to_string()),
        )
        .await;

    let pending = manager.get_unrecognized_device_snapshot().await.unwrap();
    let revision = manager.get_destination_snapshot().await.revision;
    let artwork = FolderArtwork { enabled: true };
    let manifest = manager
        .initialize_pending_device(
            &pending.pending_id, revision, "", None, None, "Garmin Watch".to_string(), None,
            true, artwork.clone(),
        )
        .await
        .expect("a failed best-effort MTP mirror must not abort initialization");

    assert!(manager.get_current_device().await.is_some());
    let cache = cache_dir
        .path()
        .join(format!("{}.json", manifest.device_id));
    let cached: DeviceManifest =
        manifest_codec::decode_test_manifest(&std::fs::read(&cache).unwrap()).unwrap();
    assert_eq!(cached.device_id, manifest.device_id);
    assert!(cached.reconcile_before_sync);
    assert_eq!(cached.folder_artwork, artwork);
    assert_eq!(manifest.folder_artwork, artwork);
}

#[tokio::test]
async fn initialize_mtp_cache_failure_does_not_register_the_device() {
    let dir = tempdir().unwrap();
    let manager = DeviceManager::new_with_mtp_manifest_cache_path(
        Arc::new(crate::db::Database::memory().unwrap()),
        Arc::new(|_| anyhow::bail!("injected cache commit failure")),
    );
    let device_io = StorageIdDeviceIo::new(dir.path(), Some("mtp-storage".to_string()));
    manager
        .handle_device_unrecognized(
            PathBuf::from("mtp://cache-failure"),
            Arc::clone(&device_io),
            Some("Garmin Watch".to_string()),
        )
        .await;

    let result = manager
        .initialize_device("", None, None, "Garmin Watch".to_string(), None, device_io)
        .await;

    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("cache commit failure")
    );
    assert!(manager.get_current_device().await.is_none());
    assert_eq!(
        manager.get_unrecognized_device_path().await,
        Some(PathBuf::from("mtp://cache-failure"))
    );
}

#[tokio::test]
async fn test_initialize_device_subfolder() {
    let dir = tempdir().unwrap();
    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    manager
        .handle_device_unrecognized(dir.path().to_path_buf(), msc(dir.path()), None)
        .await;

    // Initialize with a subfolder
    let manifest = manager
        .initialize_device(
            "Music",
            None,
            None,
            "My Device".to_string(),
            None,
            msc(dir.path()),
        )
        .await
        .unwrap();

    assert_eq!(manifest.managed_paths, vec!["Music".to_string()]);

    // Music folder should have been created
    let music_folder = dir.path().join("Music");
    assert!(music_folder.exists(), "Music subfolder should be created");

    // Manifest on disk should have managed_paths = ["Music"]
    let content = manifest_codec::read_manifest_json(dir.path().join(".hifimule.json.zst"))
        .await
        .unwrap();
    let loaded: DeviceManifest = serde_json::from_str(&content).unwrap();
    assert_eq!(loaded.managed_paths, vec!["Music".to_string()]);
}

#[tokio::test]
async fn test_initialize_device_requires_unrecognized_path() {
    let dir = tempdir().unwrap();
    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    // No unrecognized path set → should fail even when a backend is provided
    let res = manager
        .initialize_device(
            "",
            None,
            None,
            "My Device".to_string(),
            None,
            msc(dir.path()),
        )
        .await;
    assert!(res.is_err());
    assert!(
        res.unwrap_err()
            .to_string()
            .contains("No unrecognized device")
    );
}

#[tokio::test]
async fn test_initialize_device_persists_storage_id_from_device_io() {
    let dir = tempdir().unwrap();
    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);
    let device_io = StorageIdDeviceIo::new(dir.path(), Some("storage-123".to_string()));

    manager
        .handle_device_unrecognized(dir.path().to_path_buf(), Arc::clone(&device_io), None)
        .await;

    let manifest = manager
        .initialize_device(
            "Music",
            None,
            None,
            "My Device".to_string(),
            None,
            Arc::clone(&device_io),
        )
        .await
        .unwrap();

    assert_eq!(manifest.storage_id.as_deref(), Some("storage-123"));
    let content = manifest_codec::read_manifest_json(dir.path().join(".hifimule.json.zst"))
        .await
        .unwrap();
    let loaded: DeviceManifest = serde_json::from_str(&content).unwrap();
    assert_eq!(loaded.storage_id.as_deref(), Some("storage-123"));
}

#[tokio::test]
async fn test_initialize_device_rejects_path_traversal() {
    let dir = tempdir().unwrap();
    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    manager
        .handle_device_unrecognized(dir.path().to_path_buf(), msc(dir.path()), None)
        .await;

    // Path traversal with ".."
    let res = manager
        .initialize_device(
            "../escape",
            None,
            None,
            "My Device".to_string(),
            None,
            msc(dir.path()),
        )
        .await;
    assert!(res.is_err());
    assert!(res.unwrap_err().to_string().contains("Invalid folder path"));

    // Absolute path
    let res = manager
        .initialize_device(
            "/etc/hacked",
            None,
            None,
            "My Device".to_string(),
            None,
            msc(dir.path()),
        )
        .await;
    assert!(res.is_err());

    // Multi-level paths with slash separators are now allowed (T9)
    let res = manager
        .initialize_device(
            "Music/SubFolder",
            None,
            None,
            "My Device".to_string(),
            None,
            msc(dir.path()),
        )
        .await;
    assert!(res.is_ok(), "multi-level paths must be accepted: {:?}", res);
}

#[tokio::test]
async fn test_handle_device_unrecognized_preserves_recognized_device() {
    // Story 2.7: When an unrecognized device arrives, recognized devices must NOT be cleared.
    // Other recognized devices may still be connected; only unrecognized_device_path is updated.
    let dir = tempdir().unwrap();
    let manifest_json = r#"{"device_id": "dev-1", "name": "My Device", "version": "1.0"}"#;
    fs::write(dir.path().join(".hifimule.json"), manifest_json).unwrap();
    let manifest: DeviceManifest = serde_json::from_str(manifest_json).unwrap();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    // First detect a recognized device
    manager
        .handle_device_detected(dir.path().to_path_buf(), manifest, msc(dir.path()))
        .await
        .unwrap();
    assert!(manager.get_current_device().await.is_some());
    assert!(manager.get_current_device_path().await.is_some());

    // A genuinely later pending arrival becomes the navigation destination without
    // erasing the recognized device from the physical inventory.
    let dir2 = tempdir().unwrap();
    manager
        .handle_device_unrecognized(dir2.path().to_path_buf(), msc(dir2.path()), None)
        .await;

    assert!(manager.get_current_device().await.is_none());
    assert!(manager.get_current_device_path().await.is_none());
    assert!(
        manager.get_unrecognized_device_path().await.is_some(),
        "unrecognized_device_path must be set"
    );
    // The unrecognized path must NOT appear in connected_devices
    let devices = manager.get_connected_devices().await;
    assert!(
        devices.iter().all(|(p, _, _)| p != dir2.path()),
        "Unrecognized device path must not be in connected_devices"
    );
}

#[tokio::test]
async fn pending_mtp_device_is_not_replaced_by_a_removable_candidate() {
    let watch = tempdir().unwrap();
    let reader = tempdir().unwrap();
    let manager = DeviceManager::new(Arc::new(crate::db::Database::memory().unwrap()));
    let mtp_path = PathBuf::from("mtp://garmin-watch");

    manager
        .handle_device_unrecognized(
            mtp_path.clone(),
            StorageIdDeviceIo::new(watch.path(), Some("watch-storage".to_string())),
            Some("Garmin Watch".to_string()),
        )
        .await;
    manager
        .handle_device_unrecognized(reader.path().to_path_buf(), msc(reader.path()), None)
        .await;

    let pending = manager.get_pending_devices_snapshot().await;
    assert_eq!(pending.len(), 2);
    assert!(pending.iter().any(|entry| entry.path == mtp_path));
    assert!(pending.iter().any(|entry| entry.path == reader.path()));
}

#[tokio::test]
async fn pending_mtp_device_replaces_an_earlier_removable_candidate() {
    let watch = tempdir().unwrap();
    let reader = tempdir().unwrap();
    let manager = DeviceManager::new(Arc::new(crate::db::Database::memory().unwrap()));
    let mtp_path = PathBuf::from("mtp://garmin-watch");

    manager
        .handle_device_unrecognized(reader.path().to_path_buf(), msc(reader.path()), None)
        .await;
    manager
        .handle_device_unrecognized(
            mtp_path.clone(),
            StorageIdDeviceIo::new(watch.path(), Some("watch-storage".to_string())),
            Some("Garmin Watch".to_string()),
        )
        .await;

    assert_eq!(manager.get_unrecognized_device_path().await, Some(mtp_path));
}

// ===== Story 15.12 destination contract =====

#[tokio::test]
async fn playback_is_the_first_and_default_destination() {
    let manager = DeviceManager::new(Arc::new(crate::db::Database::memory().unwrap()));
    let snapshot = manager.get_destination_snapshot().await;
    assert_eq!(snapshot.revision, 0);
    assert_eq!(snapshot.destinations.len(), 1);
    assert!(matches!(
        snapshot.destinations[0],
        Destination::Playback { selected: true, .. }
    ));
    assert!(manager.get_current_device_path().await.is_none());
}

#[tokio::test]
async fn plural_pending_devices_are_distinct_and_target_safe() {
    let first = tempdir().unwrap();
    let second = tempdir().unwrap();
    let manager = DeviceManager::new(Arc::new(crate::db::Database::memory().unwrap()));
    manager
        .handle_device_unrecognized(
            first.path().to_path_buf(),
            msc(first.path()),
            Some("First".into()),
        )
        .await;
    manager
        .handle_device_unrecognized(
            second.path().to_path_buf(),
            msc(second.path()),
            Some("Second".into()),
        )
        .await;

    let pending = manager.get_pending_devices_snapshot().await;
    assert_eq!(pending.len(), 2);
    assert_ne!(pending[0].pending_id, pending[1].pending_id);
    assert_eq!(pending[0].friendly_name.as_deref(), Some("First"));
    assert_eq!(pending[1].friendly_name.as_deref(), Some("Second"));
}

#[tokio::test]
async fn pending_initialization_rejects_a_stale_dialog_without_redirecting() {
    let first = tempdir().unwrap();
    let later = tempdir().unwrap();
    let manager = DeviceManager::new(Arc::new(crate::db::Database::memory().unwrap()));
    manager
        .handle_device_unrecognized(
            first.path().to_path_buf(),
            msc(first.path()),
            Some("First".into()),
        )
        .await;
    let observed = manager.get_destination_snapshot().await;
    let first_id = manager.get_pending_devices_snapshot().await[0]
        .pending_id
        .clone();
    manager
        .handle_device_unrecognized(
            later.path().to_path_buf(),
            msc(later.path()),
            Some("Later".into()),
        )
        .await;
    let result = manager
        .initialize_pending_device(
            &first_id,
            observed.revision,
            "",
            None,
            None,
            "First".into(),
            None,
            false,
            Default::default(),
        )
        .await;
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("revision is stale")
    );
    assert!(!first.path().join(".hifimule.json.zst").exists());
    assert!(!later.path().join(".hifimule.json.zst").exists());
}

#[tokio::test]
async fn discovery_failures_are_sanitized_deduplicated_and_cleared_per_device() {
    let manager = DeviceManager::new(Arc::new(crate::db::Database::memory().unwrap()));
    let failed = PathBuf::from("/media/failed");
    manager
        .report_discovery_failure(
            failed.clone(),
            "raw backend secret",
            Some("Player".repeat(50)),
        )
        .await;
    let first = manager.get_discovery_issues().await;
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].code, "DEVICE_READ_FAILED");
    assert!(first[0].display_name.as_ref().unwrap().len() <= 120);
    manager
        .report_discovery_failure(
            failed.clone(),
            "raw backend secret",
            first[0].display_name.clone(),
        )
        .await;
    assert_eq!(manager.get_discovery_issues().await.len(), 1);
    manager.handle_device_removed(&failed).await;
    assert!(manager.get_discovery_issues().await.is_empty());
}

#[tokio::test]
async fn selected_device_removal_falls_back_to_playback_not_another_device() {
    let first = tempdir().unwrap();
    let second = tempdir().unwrap();
    let manager = DeviceManager::new(Arc::new(crate::db::Database::memory().unwrap()));
    manager
        .handle_device_detected(
            first.path().to_path_buf(),
            make_manifest("a", "A"),
            msc(first.path()),
        )
        .await
        .unwrap();
    manager
        .handle_device_detected(
            second.path().to_path_buf(),
            make_manifest("b", "B"),
            msc(second.path()),
        )
        .await
        .unwrap();
    assert!(manager.select_device(first.path().to_path_buf()).await);
    manager
        .handle_device_removed(&first.path().to_path_buf())
        .await;
    assert!(manager.get_current_device_path().await.is_none());
    let snapshot = manager.get_destination_snapshot().await;
    assert!(matches!(
        snapshot.destinations[0],
        Destination::Playback { selected: true, .. }
    ));
}

#[tokio::test]
async fn older_arrival_cannot_override_a_later_explicit_playback_choice() {
    let dir = tempdir().unwrap();
    let manager = DeviceManager::new(Arc::new(crate::db::Database::memory().unwrap()));
    let observed = manager.begin_destination_mutation();
    manager.select_playback().await;
    manager
        .handle_device_detected_at(
            observed,
            dir.path().to_path_buf(),
            make_manifest("slow", "Slow"),
            msc(dir.path()),
        )
        .await
        .unwrap();
    assert!(manager.get_current_device_path().await.is_none());
}

#[cfg(target_os = "windows")]
#[test]
fn removable_drive_requires_present_media() {
    assert!(!is_usable_removable_drive(2, false));
    assert!(is_usable_removable_drive(2, true));
    assert!(!is_usable_removable_drive(3, true));
}

// ===== Story 2.6 MTP Task Tests =====

#[tokio::test]
async fn test_handle_device_unrecognized_stores_device_io() {
    // Verify that handle_device_unrecognized stores a DeviceIO backend alongside the path.
    let dir = tempdir().unwrap();
    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    assert!(manager.get_unrecognized_device_io().await.is_none());

    manager
        .handle_device_unrecognized(dir.path().to_path_buf(), msc(dir.path()), None)
        .await;

    assert!(
        manager.get_unrecognized_device_io().await.is_some(),
        "IO backend must be stored alongside unrecognized device path"
    );
}

#[tokio::test]
async fn test_initialize_device_uses_stored_io_and_clears_it() {
    // Verify that initialize_device (spec form: device_io as parameter) clears stored IO on success.
    let dir = tempdir().unwrap();
    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    manager
        .handle_device_unrecognized(dir.path().to_path_buf(), msc(dir.path()), None)
        .await;

    assert!(manager.get_unrecognized_device_io().await.is_some());

    let device_io = manager.get_unrecognized_device_io().await.unwrap();
    manager
        .initialize_device("", None, None, "My Device".to_string(), None, device_io)
        .await
        .unwrap();

    assert!(
        manager.get_unrecognized_device_io().await.is_none(),
        "IO backend must be cleared after successful initialization"
    );
    assert!(
        manager.get_unrecognized_device_path().await.is_none(),
        "Unrecognized path must be cleared after successful initialization"
    );
    // Manifest must have been written via the stored IO backend
    assert!(
        dir.path().join(".hifimule.json.zst").exists(),
        "Manifest must be written to the device root"
    );
}

#[tokio::test]
async fn test_initialize_device_uses_coherent_pending_snapshot_not_stale_caller_io() {
    let pending_dir = tempdir().unwrap();
    let stale_dir = tempdir().unwrap();
    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    manager
        .handle_device_unrecognized(
            pending_dir.path().to_path_buf(),
            msc(pending_dir.path()),
            None,
        )
        .await;

    manager
        .initialize_device(
            "",
            None,
            None,
            "My Device".to_string(),
            None,
            msc(stale_dir.path()),
        )
        .await
        .unwrap();

    assert!(
        pending_dir.path().join(".hifimule.json.zst").exists(),
        "manifest must be written through the pending snapshot IO"
    );
    assert!(
        !stale_dir.path().join(".hifimule.json.zst").exists(),
        "stale caller IO must not be paired with the pending path"
    );
}

#[tokio::test]
async fn test_handle_device_removed_clears_unrecognized_io() {
    // Verify that removing the unrecognized device also clears the stored IO backend.
    let dir = tempdir().unwrap();
    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    let path = dir.path().to_path_buf();
    manager
        .handle_device_unrecognized(path.clone(), msc(dir.path()), None)
        .await;

    assert!(manager.get_unrecognized_device_path().await.is_some());
    assert!(manager.get_unrecognized_device_io().await.is_some());

    manager.handle_device_removed(&path).await;

    assert!(
        manager.get_unrecognized_device_path().await.is_none(),
        "Unrecognized path must be cleared on device removal"
    );
    assert!(
        manager.get_unrecognized_device_io().await.is_none(),
        "IO backend must be cleared when unrecognized device is removed"
    );
}

// ===== Basket Selection Tests =====

#[test]
fn test_basket_items_serde_default() {
    let json = r#"{"device_id": "old-device", "name": "Old iPod", "version": "1.0", "managed_paths": ["Music"]}"#;
    let manifest: DeviceManifest = serde_json::from_str(json).unwrap();
    assert!(
        manifest.basket_items.is_empty(),
        "basket_items should default to empty vec"
    );
}

#[tokio::test]
async fn test_save_basket_roundtrip() {
    let dir = tempdir().unwrap();
    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    let manifest = DeviceManifest {
        device_id: "dev-basket".to_string(),
        name: Some("Basket Test".to_string()),
        icon: None,
        version: "1.0".to_string(),
        managed_paths: vec![],
        synced_items: vec![],
        dirty: false,
        pending_item_ids: vec![],
        basket_items: vec![],
        folder_artwork: Default::default(),
        folder_artwork_records: Default::default(),
        reconcile_before_sync: false,
        auto_sync_on_connect: false,
        auto_fill: crate::device::AutoFillConfig::default(),
        transcoding_profile_id: None,
        playlists: vec![],
        storage_id: None,
        ..Default::default()
    };
    write_manifest(msc(dir.path()), &manifest).await.unwrap();

    manager
        .handle_device_detected(dir.path().to_path_buf(), manifest, msc(dir.path()))
        .await
        .unwrap();

    let items = vec![BasketItem {
        id: "basket-1".to_string(),
        name: "Basket Playlist".to_string(),
        item_type: "Playlist".to_string(),
        server_id: None,
        artist: None,
        child_count: 5,
        size_ticks: 1000,
        size_bytes: 500000,
    }];

    manager.save_basket(items.clone()).await.unwrap();

    // Verify in-memory state
    let memory_device = manager.get_current_device().await.unwrap();
    assert_eq!(memory_device.basket_items.len(), 1);
    assert_eq!(memory_device.basket_items[0].id, "basket-1");

    // Verify disk state
    let content =
        manifest_codec::read_manifest_json_sync(dir.path().join(".hifimule.json.zst")).unwrap();
    let loaded: DeviceManifest = serde_json::from_str(&content).unwrap();
    assert_eq!(loaded.basket_items.len(), 1);
    assert_eq!(loaded.basket_items[0].name, "Basket Playlist");
}

// ===== Story 2.3b Tests: Auto-Sync on Connect =====

#[tokio::test]
async fn test_handle_device_detected_syncs_auto_sync_manifest_to_db() {
    let dir = tempdir().unwrap();
    let db = Arc::new(crate::db::Database::memory().unwrap());
    let device_id = "dev-auto-sync-detect";
    db.upsert_device_mapping(device_id, Some("Auto Sync Device"), Some("user-1"), None)
        .unwrap();
    assert!(
        !db.get_device_mapping(device_id)
            .unwrap()
            .unwrap()
            .auto_sync_on_connect
    );

    let manager = DeviceManager::new(db.clone());
    let manifest = DeviceManifest {
        device_id: device_id.to_string(),
        name: Some("Auto Sync Device".to_string()),
        icon: None,
        version: "1.0".to_string(),
        managed_paths: vec![],
        synced_items: vec![],
        dirty: false,
        pending_item_ids: vec![],
        basket_items: vec![],
        folder_artwork: Default::default(),
        folder_artwork_records: Default::default(),
        reconcile_before_sync: false,
        auto_sync_on_connect: true,
        auto_fill: crate::device::AutoFillConfig::default(),
        transcoding_profile_id: None,
        playlists: vec![],
        storage_id: None,
        ..Default::default()
    };
    write_manifest(msc(dir.path()), &manifest).await.unwrap();

    manager
        .handle_device_detected(dir.path().to_path_buf(), manifest, msc(dir.path()))
        .await
        .unwrap();

    let mapping = db.get_device_mapping(device_id).unwrap().unwrap();
    assert!(
        mapping.auto_sync_on_connect,
        "device detection must copy manifest auto_sync_on_connect into SQLite so UI state and auto-sync gating agree"
    );
}

#[test]
fn test_auto_sync_on_connect_serde_default() {
    let json = r#"{"device_id": "dev-1", "name": "iPod", "version": "1.0"}"#;
    let manifest: DeviceManifest = serde_json::from_str(json).unwrap();
    assert_eq!(manifest.last_sync_at, None);
    assert!(
        !manifest.auto_sync_on_connect,
        "auto_sync_on_connect must default to false"
    );
}

#[tokio::test]
async fn test_auto_sync_on_connect_roundtrip() {
    let dir = tempdir().unwrap();
    let manifest = DeviceManifest {
        device_id: "dev-auto".to_string(),
        last_sync_at: Some(1_800_000_000),
        name: Some("Auto Device".to_string()),
        icon: None,
        version: "1.0".to_string(),
        managed_paths: vec![],
        synced_items: vec![],
        dirty: false,
        pending_item_ids: vec![],
        basket_items: vec![],
        folder_artwork: Default::default(),
        folder_artwork_records: Default::default(),
        reconcile_before_sync: false,
        auto_sync_on_connect: true,
        auto_fill: crate::device::AutoFillConfig::default(),
        transcoding_profile_id: None,
        playlists: vec![],
        storage_id: None,
        ..Default::default()
    };
    write_manifest(msc(dir.path()), &manifest).await.unwrap();

    let content = manifest_codec::read_manifest_json(dir.path().join(".hifimule.json.zst"))
        .await
        .unwrap();
    let loaded: DeviceManifest = serde_json::from_str(&content).unwrap();
    assert!(loaded.auto_sync_on_connect);
    assert_eq!(loaded.last_sync_at, manifest.last_sync_at);

    // Verify snake_case serialization (DeviceManifest doesn't use rename_all)
    let raw: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(raw["auto_sync_on_connect"], true);
}

#[test]
fn test_auto_sync_on_connect_explicit_false() {
    let json = r#"{"device_id": "dev-1", "version": "1.0", "auto_sync_on_connect": false}"#;
    let manifest: DeviceManifest = serde_json::from_str(json).unwrap();
    assert!(!manifest.auto_sync_on_connect);
}

#[test]
fn test_auto_sync_on_connect_explicit_true() {
    let json = r#"{"device_id": "dev-1", "version": "1.0", "auto_sync_on_connect": true}"#;
    let manifest: DeviceManifest = serde_json::from_str(json).unwrap();
    assert!(manifest.auto_sync_on_connect);
}

// ===== Story 2.7 Tests =====

fn make_manifest(device_id: &str, name: &str) -> DeviceManifest {
    DeviceManifest {
        device_id: device_id.to_string(),
        name: Some(name.to_string()),
        icon: None,
        version: "1.0".to_string(),
        managed_paths: vec![],
        synced_items: vec![],
        dirty: false,
        pending_item_ids: vec![],
        basket_items: vec![],
        folder_artwork: Default::default(),
        folder_artwork_records: Default::default(),
        reconcile_before_sync: false,
        auto_sync_on_connect: false,
        auto_fill: crate::device::AutoFillConfig::default(),
        transcoding_profile_id: None,
        playlists: vec![],
        storage_id: None,
        ..Default::default()
    }
}

#[tokio::test]
async fn test_handle_device_detected_two_sequential_devices() {
    let dir1 = tempdir().unwrap();
    let dir2 = tempdir().unwrap();
    let path1 = dir1.path().to_path_buf();
    let path2 = dir2.path().to_path_buf();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    let manifest1 = make_manifest("device-1", "iPod");
    let manifest2 = make_manifest("device-2", "Walkman");

    // Write manifests so handle_device_detected can store them
    write_manifest(msc(&path1), &manifest1).await.unwrap();
    write_manifest(msc(&path2), &manifest2).await.unwrap();

    manager
        .handle_device_detected(path1.clone(), manifest1, msc(&path1))
        .await
        .unwrap();
    manager
        .handle_device_detected(path2.clone(), manifest2, msc(&path2))
        .await
        .unwrap();

    let devices = manager.get_connected_devices().await;
    assert_eq!(
        devices.len(),
        2,
        "Both devices must be in connected_devices"
    );

    // The genuinely later arrival is selected by the destination mutation order.
    let selected = manager.get_current_device_path().await;
    assert_eq!(
        selected,
        Some(path2.clone()),
        "Later device must be selected"
    );

    // get_current_device returns the manifest for the selected path
    let current = manager.get_current_device().await;
    assert!(current.is_some());
    assert_eq!(current.unwrap().device_id, "device-2");
}

#[tokio::test]
async fn test_handle_device_removed_selected_with_remaining_autoselects() {
    let dir1 = tempdir().unwrap();
    let dir2 = tempdir().unwrap();
    let path1 = dir1.path().to_path_buf();
    let path2 = dir2.path().to_path_buf();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    let manifest1 = make_manifest("device-1", "iPod");
    let manifest2 = make_manifest("device-2", "Walkman");

    write_manifest(msc(&path1), &manifest1).await.unwrap();
    write_manifest(msc(&path2), &manifest2).await.unwrap();

    manager
        .handle_device_detected(path1.clone(), manifest1, msc(&path1))
        .await
        .unwrap();
    manager
        .handle_device_detected(path2.clone(), manifest2, msc(&path2))
        .await
        .unwrap();

    // Selected is path1 — remove it
    manager.handle_device_removed(&path1).await;

    let devices = manager.get_connected_devices().await;
    assert_eq!(devices.len(), 1, "Only one device should remain");

    // Remaining device (path2) must be auto-selected
    let selected = manager.get_current_device_path().await;
    assert_eq!(
        selected,
        Some(path2.clone()),
        "Remaining device must be auto-selected"
    );
}

#[tokio::test]
async fn test_handle_device_removed_non_selected_selection_unchanged() {
    let dir1 = tempdir().unwrap();
    let dir2 = tempdir().unwrap();
    let path1 = dir1.path().to_path_buf();
    let path2 = dir2.path().to_path_buf();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    let manifest1 = make_manifest("device-1", "iPod");
    let manifest2 = make_manifest("device-2", "Walkman");

    write_manifest(msc(&path1), &manifest1).await.unwrap();
    write_manifest(msc(&path2), &manifest2).await.unwrap();

    manager
        .handle_device_detected(path1.clone(), manifest1, msc(&path1))
        .await
        .unwrap();
    manager
        .handle_device_detected(path2.clone(), manifest2, msc(&path2))
        .await
        .unwrap();

    // Selected is path2 — remove path1 (non-selected)
    manager.handle_device_removed(&path1).await;

    let devices = manager.get_connected_devices().await;
    assert_eq!(devices.len(), 1, "Only one device should remain");

    // path2 must still be selected
    let selected = manager.get_current_device_path().await;
    assert_eq!(
        selected,
        Some(path2.clone()),
        "Selection must remain unchanged"
    );
}

#[tokio::test]
async fn test_select_device_valid_path() {
    let dir1 = tempdir().unwrap();
    let dir2 = tempdir().unwrap();
    let path1 = dir1.path().to_path_buf();
    let path2 = dir2.path().to_path_buf();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    let manifest1 = make_manifest("device-1", "iPod");
    let manifest2 = make_manifest("device-2", "Walkman");

    write_manifest(msc(&path1), &manifest1).await.unwrap();
    write_manifest(msc(&path2), &manifest2).await.unwrap();

    manager
        .handle_device_detected(path1.clone(), manifest1, msc(&path1))
        .await
        .unwrap();
    manager
        .handle_device_detected(path2.clone(), manifest2, msc(&path2))
        .await
        .unwrap();

    // Switch to path2
    let ok = manager.select_device(path2.clone()).await;
    assert!(ok, "select_device must return true for a connected device");

    let selected = manager.get_current_device_path().await;
    assert_eq!(selected, Some(path2.clone()));

    let current = manager.get_current_device().await;
    assert_eq!(current.unwrap().device_id, "device-2");
}

#[tokio::test]
async fn test_select_device_unknown_path_returns_false() {
    let dir = tempdir().unwrap();
    let path = dir.path().to_path_buf();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    // No devices connected — selecting an unknown path must fail
    let ok = manager.select_device(path).await;
    assert!(
        !ok,
        "select_device must return false for an unconnected path"
    );

    assert!(manager.get_current_device_path().await.is_none());
}

#[tokio::test]
async fn test_handle_device_removed_selected_with_multiple_remaining_autoselects() {
    let dir1 = tempdir().unwrap();
    let dir2 = tempdir().unwrap();
    let dir3 = tempdir().unwrap();
    let path1 = dir1.path().to_path_buf();
    let path2 = dir2.path().to_path_buf();
    let path3 = dir3.path().to_path_buf();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);

    let manifest1 = make_manifest("device-1", "iPod");
    let manifest2 = make_manifest("device-2", "Walkman");
    let manifest3 = make_manifest("device-3", "Zune");

    write_manifest(msc(&path1), &manifest1).await.unwrap();
    write_manifest(msc(&path2), &manifest2).await.unwrap();
    write_manifest(msc(&path3), &manifest3).await.unwrap();

    manager
        .handle_device_detected(path1.clone(), manifest1, msc(&path1))
        .await
        .unwrap();
    manager
        .handle_device_detected(path2.clone(), manifest2, msc(&path2))
        .await
        .unwrap();
    manager
        .handle_device_detected(path3.clone(), manifest3, msc(&path3))
        .await
        .unwrap();

    manager.handle_device_removed(&path1).await;

    let selected = manager.get_current_device_path().await;
    assert!(
        selected.as_ref() == Some(&path2) || selected.as_ref() == Some(&path3),
        "one remaining device must be selected after selected removal"
    );
}

#[tokio::test]
async fn test_select_device_and_update_manifest_do_not_deadlock() {
    let dir1 = tempdir().unwrap();
    let dir2 = tempdir().unwrap();
    let path1 = dir1.path().to_path_buf();
    let path2 = dir2.path().to_path_buf();

    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = Arc::new(DeviceManager::new(db));

    let manifest1 = make_manifest("device-1", "iPod");
    let manifest2 = make_manifest("device-2", "Walkman");
    write_manifest(msc(&path1), &manifest1).await.unwrap();
    write_manifest(msc(&path2), &manifest2).await.unwrap();

    manager
        .handle_device_detected(path1.clone(), manifest1, msc(&path1))
        .await
        .unwrap();
    manager
        .handle_device_detected(path2.clone(), manifest2, msc(&path2))
        .await
        .unwrap();

    let selecting = {
        let manager = Arc::clone(&manager);
        let path2 = path2.clone();
        tokio::spawn(async move { manager.select_device(path2).await })
    };
    let updating = {
        let manager = Arc::clone(&manager);
        tokio::spawn(async move {
            manager
                .update_manifest(|manifest| manifest.name = Some("Updated".to_string()))
                .await
        })
    };

    timeout(Duration::from_secs(2), async {
        let _ = tokio::join!(selecting, updating);
    })
    .await
    .expect("select_device and update_manifest must not deadlock");
}

#[tokio::test]
async fn test_unrecognized_state_concurrent_set_and_remove_is_coherent() {
    let dir = tempdir().unwrap();
    let path = dir.path().to_path_buf();
    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = Arc::new(DeviceManager::new(db));

    let setter = {
        let manager = Arc::clone(&manager);
        let path = path.clone();
        tokio::spawn(async move {
            for _ in 0..100 {
                manager
                    .handle_device_unrecognized(path.clone(), msc(&path), Some("Pending".into()))
                    .await;
            }
        })
    };
    let remover = {
        let manager = Arc::clone(&manager);
        let path = path.clone();
        tokio::spawn(async move {
            for _ in 0..100 {
                manager.handle_device_removed(&path).await;
            }
        })
    };

    timeout(Duration::from_secs(2), async {
        let _ = tokio::join!(setter, remover);
    })
    .await
    .expect("concurrent unrecognized set/remove must complete");

    if let Some(snapshot) = manager.get_unrecognized_device_snapshot().await {
        assert_eq!(snapshot.path, path);
        assert!(snapshot.io.free_space().await.is_ok());
        assert_eq!(snapshot.friendly_name.as_deref(), Some("Pending"));
    }
}

#[tokio::test]
async fn test_handle_device_unrecognized_removes_connected_entry_for_same_path() {
    let dir = tempdir().unwrap();
    let path = dir.path().to_path_buf();
    let db = Arc::new(crate::db::Database::memory().unwrap());
    let manager = DeviceManager::new(db);
    let manifest = make_manifest("device-1", "iPod");

    write_manifest(msc(&path), &manifest).await.unwrap();
    manager
        .handle_device_detected(path.clone(), manifest, msc(&path))
        .await
        .unwrap();
    assert_eq!(manager.get_connected_devices().await.len(), 1);

    manager
        .handle_device_unrecognized(path.clone(), msc(&path), None)
        .await;

    assert!(
        manager.get_connected_devices().await.is_empty(),
        "same-path unrecognized probe must remove the connected entry"
    );
    assert_eq!(manager.get_unrecognized_device_path().await, Some(path));
}

#[tokio::test]
async fn test_mtp_probe_missing_manifest_emits_unrecognized_and_marks_known() {
    let dir = tempdir().unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    let backend = msc(dir.path());

    let inserted = emit_mtp_probe_event(
        &tx,
        1,
        PathBuf::from("mtp://missing-manifest"),
        "missing-manifest",
        dummy_mtp_device_info("missing-manifest"),
        "Fresh Device".to_string(),
        backend,
    )
    .await;

    assert!(
        inserted,
        "missing manifest emits Unrecognized and can be marked known"
    );
    match rx.recv().await.unwrap() {
        DeviceEvent::Unrecognized {
            path,
            friendly_name,
            ..
        } => {
            assert_eq!(path, PathBuf::from("mtp://missing-manifest"));
            assert_eq!(friendly_name.as_deref(), Some("Fresh Device"));
        }
        _ => panic!("expected unrecognized event"),
    }
}

#[tokio::test]
async fn test_mtp_probe_read_failure_does_not_mark_known() {
    let dir = tempdir().unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    let backend = FailingReadDeviceIo::new(dir.path());

    let inserted = emit_mtp_probe_event(
        &tx,
        1,
        PathBuf::from("mtp://retry-me"),
        "retry-me",
        dummy_mtp_device_info("retry-me"),
        "Retry Device".to_string(),
        backend,
    )
    .await;

    assert!(
        !inserted,
        "manifest read failure must leave device retryable"
    );
    assert!(matches!(
        rx.recv().await.unwrap(),
        DeviceEvent::DiscoveryFailed {
            code: "DEVICE_READ_FAILED",
            ..
        }
    ));
}

#[tokio::test]
async fn test_mtp_probe_parse_failure_emits_retryable_issue() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".hifimule.json"), b"not-json").unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    let backend = msc(dir.path());

    let inserted = emit_mtp_probe_event(
        &tx,
        1,
        PathBuf::from("mtp://unrecognized"),
        "unrecognized",
        dummy_mtp_device_info("unrecognized"),
        "Unrecognized Device".to_string(),
        backend,
    )
    .await;

    assert!(!inserted, "parse failure remains retryable");
    match rx.recv().await.unwrap() {
        DeviceEvent::DiscoveryFailed {
            display_name, code, ..
        } => {
            assert_eq!(display_name.as_deref(), Some("Unrecognized Device"));
            assert_eq!(code, "DEVICE_READ_FAILED");
        }
        _ => panic!("expected discovery failure event"),
    }
}

// ===== Story 7.3 Tests =====

#[test]
fn test_empty_device_name_falls_back_to_device_id() {
    let device_id = "abc-123-def".to_string();
    // Simulates the name resolution logic in handle_get_daemon_state connected_devices_json
    let name_some_empty: Option<String> = Some("".to_string());
    let resolved = name_some_empty
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| device_id.clone());
    assert_eq!(
        resolved, device_id,
        "empty name must fall back to device_id"
    );

    let name_none: Option<String> = None;
    let resolved_none = name_none
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| device_id.clone());
    assert_eq!(
        resolved_none, device_id,
        "None name must fall back to device_id"
    );

    let name_real: Option<String> = Some("My Garmin".to_string());
    let resolved_real = name_real
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| device_id.clone());
    assert_eq!(resolved_real, "My Garmin", "real name must be preserved");
}

// ===== Story 7.4 Tests =====

#[test]
fn test_boot_volume_device_is_skipped_when_device_id_matches_root() {
    assert!(
        is_boot_volume_device(Some(42), 42),
        "candidate with the root device ID must be skipped"
    );
}

#[test]
fn test_boot_volume_device_allows_different_device_id() {
    assert!(
        !is_boot_volume_device(Some(43), 42),
        "candidate with a different device ID can be considered a removable mount"
    );
}

#[test]
fn test_boot_volume_device_metadata_error_is_fail_safe_skip() {
    assert!(
        is_boot_volume_device(None, 42),
        "metadata failures must skip the candidate rather than risk selecting the boot volume"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn test_mount_discovery_flags() {
    for flags in [0, libc::MNT_LOCAL, libc::MNT_LOCAL | libc::MNT_JOURNALED] {
        assert!(
            mount_flags_allow_discovery(flags as u32),
            "allowed flags: {flags:#x}"
        );
    }
    for flags in [
        libc::MNT_RDONLY,
        libc::MNT_DONTBROWSE,
        libc::MNT_RDONLY | libc::MNT_DONTBROWSE,
        libc::MNT_DONTBROWSE | libc::MNT_LOCAL | libc::MNT_JOURNALED,
    ] {
        assert!(
            !mount_flags_allow_discovery(flags as u32),
            "excluded flags: {flags:#x}"
        );
    }
}

#[cfg(target_os = "macos")]
#[test]
fn test_mount_discovery_metadata_errors_skip_candidate() {
    let dir = tempdir().unwrap();
    assert!(!is_discoverable_mount(&dir.path().join("missing")));
    assert!(!is_discoverable_mount(Path::new("/Volumes/invalid\0path")));
}

#[cfg(target_os = "macos")]
#[test]
fn test_mount_discovery_uses_flags_regardless_of_name_or_manifest() {
    let dir = tempdir().unwrap();
    let expected = mount_flags_allow_discovery(mount_flags(dir.path()).unwrap());
    for name in ["Music Player", "Recovery"] {
        let path = dir.path().join(name);
        std::fs::create_dir(&path).unwrap();
        assert_eq!(is_discoverable_mount(&path), expected);
        std::fs::write(path.join(".hifimule.json"), b"{}").unwrap();
        assert_eq!(is_discoverable_mount(&path), expected);
    }
}

#[cfg(target_os = "macos")]
#[test]
fn test_mount_discovery_excludes_local_hidden_recovery_when_present() {
    let recovery = Path::new("/Volumes/Recovery");
    if !recovery.exists() || !is_mount_point(recovery) {
        return;
    }
    let flags = mount_flags(recovery).expect("read existing Recovery mount metadata");
    if flags & libc::MNT_DONTBROWSE as u32 == 0 {
        return;
    }
    assert!(!is_discoverable_mount(recovery));
    assert!(!get_mounts().iter().any(|path| path == recovery));
}

#[tokio::test]
async fn test_cleanup_tmp_files_at_device_root() {
    // T8: root-level .tmp files must be swept even with empty managed_paths.
    let dir = tempdir().unwrap();
    let tmp_file = dir.path().join("partial.flac.tmp");
    tokio::fs::write(&tmp_file, b"partial").await.unwrap();
    assert!(tmp_file.exists());

    let count = cleanup_tmp_files(msc(dir.path()), &[]).await.unwrap();
    assert_eq!(count, 1, "root-level .tmp file must be deleted");
    assert!(!tmp_file.exists());
}

#[tokio::test]
async fn test_cleanup_tmp_files_root_and_managed() {
    // T8: both root and managed-path .tmp files are swept.
    let dir = tempdir().unwrap();
    let music_dir = dir.path().join("Music");
    tokio::fs::create_dir_all(&music_dir).await.unwrap();
    let root_tmp = dir.path().join("partial.tmp");
    let music_tmp = music_dir.join("track.flac.tmp");
    tokio::fs::write(&root_tmp, b"a").await.unwrap();
    tokio::fs::write(&music_tmp, b"b").await.unwrap();

    let count = cleanup_tmp_files(msc(dir.path()), &["Music".to_string()])
        .await
        .unwrap();
    assert_eq!(
        count, 2,
        "both root and managed-path .tmp files must be deleted"
    );
    assert!(!root_tmp.exists());
    assert!(!music_tmp.exists());
}

// ===== Story 8.6 Tests =====

#[test]
fn test_synced_item_provider_metadata_defaults_for_old_manifests() {
    let json = r#"{
        "providerItemId": "song1",
        "name": "Track",
        "album": "Album",
        "artist": "Artist",
        "localPath": "Music/Artist/Album/Track.mp3",
        "sizeBytes": 1234,
        "syncedAt": "2026-05-09T10:00:00Z",
        "etag": "v1"
    }"#;

    let item: SyncedItem =
        serde_json::from_str(json).expect("synced item with provider-neutral item id");

    assert_eq!(item.jellyfin_id, "song1");
    assert_eq!(item.provider_album_id, None);
    assert_eq!(item.provider_content_type, None);
    assert_eq!(item.provider_suffix, None);
}

#[test]
fn test_synced_item_provider_metadata_serializes_camel_case_and_builds_context() {
    let item = SyncedItem {
        is_auto_fill: false,
        media_role: crate::device::MediaRole::Music,
        jellyfin_id: "song1".to_string(),
        name: "Track".to_string(),
        album: Some("Album".to_string()),
        artist: Some("Artist".to_string()),
        local_path: "Music/Artist/Album/Track.mp3".to_string(),
        size_bytes: 1234,
        synced_at: "2026-05-09T10:00:00Z".to_string(),
        original_name: None,
        etag: Some("v1".to_string()),
        provider_album_id: Some("album1".to_string()),
        provider_content_type: Some("audio/mpeg".to_string()),
        provider_suffix: Some("mp3".to_string()),
        original_bitrate: None,
        original_container: None,
        track_number: None,
        server_id: None,
    };
    let value = serde_json::to_value(&item).expect("synced item json");

    assert_eq!(value["providerItemId"].as_str(), Some("song1"));
    assert!(value.get("jellyfinId").is_none());
    assert_eq!(value["providerAlbumId"].as_str(), Some("album1"));
    assert_eq!(value["providerContentType"].as_str(), Some("audio/mpeg"));
    assert_eq!(value["providerSuffix"].as_str(), Some("mp3"));
    assert!(value.get("provider_album_id").is_none());

    let mut manifest = DeviceManifest {
        device_id: "dev1".to_string(),
        name: None,
        icon: None,
        version: "1.1".to_string(),
        managed_paths: vec!["Music".to_string()],
        synced_items: vec![item],
        dirty: false,
        pending_item_ids: vec![],
        basket_items: vec![],
        folder_artwork: Default::default(),
        folder_artwork_records: Default::default(),
        reconcile_before_sync: false,
        auto_sync_on_connect: false,
        auto_fill: crate::device::AutoFillConfig::default(),
        transcoding_profile_id: None,
        playlists: vec![],
        storage_id: None,
        ..Default::default()
    };
    let context = manifest.provider_change_context();

    assert_eq!(context.synced_songs.len(), 1);
    assert_eq!(context.synced_songs[0].song_id, "song1");
    assert_eq!(context.synced_songs[0].album_id.as_deref(), Some("album1"));
    assert_eq!(context.synced_songs[0].size, Some(1234));
    assert_eq!(
        context.synced_songs[0].content_type.as_deref(),
        Some("audio/mpeg")
    );
    assert_eq!(context.synced_songs[0].suffix.as_deref(), Some("mp3"));

    manifest.synced_items.clear();
    assert!(manifest.provider_change_context().synced_songs.is_empty());
}

/// Story 2.13: manifest reconciliation rewrites synced + basket `server_id` tags
/// from a 2.11 machine-local UUID OR pre-2.11 composite to the portable id, leaves
/// already-portable / untagged items alone, and is idempotent.
#[test]
fn reconcile_manifest_server_ids_maps_legacy_to_portable_idempotently() {
    fn synced(id: &str, server_id: Option<&str>) -> SyncedItem {
        SyncedItem {
            is_auto_fill: false,
            media_role: crate::device::MediaRole::Music,
            jellyfin_id: id.to_string(),
            name: id.to_string(),
            album: None,
            artist: None,
            local_path: format!("Music/{id}.mp3"),
            size_bytes: 1,
            synced_at: "2026-06-09T00:00:00Z".to_string(),
            original_name: None,
            etag: None,
            provider_album_id: None,
            provider_content_type: None,
            provider_suffix: None,
            original_bitrate: None,
            original_container: None,
            track_number: None,
            server_id: server_id.map(str::to_string),
        }
    }
    fn basket(id: &str, server_id: Option<&str>) -> BasketItem {
        BasketItem {
            id: id.to_string(),
            name: id.to_string(),
            item_type: "Audio".to_string(),
            server_id: server_id.map(str::to_string),
            artist: None,
            child_count: 0,
            size_ticks: 0,
            size_bytes: 1,
        }
    }

    let local = "local-uuid-123";
    let composite = "jellyfin|http://media.example|alexis";
    let portable = "portablehex";
    let mut remap = std::collections::HashMap::new();
    remap.insert(local.to_string(), portable.to_string());
    remap.insert(composite.to_string(), portable.to_string());

    let mut manifest = DeviceManifest {
        synced_items: vec![
            synced("s-local", Some(local)),
            synced("s-composite", Some(composite)),
            synced("s-portable", Some(portable)),
            synced("s-untagged", None),
        ],
        basket_items: vec![
            basket("b-local", Some(local)),
            basket("b-portable", Some(portable)),
        ],
        ..Default::default()
    };

    assert!(reconcile_manifest_server_ids(&mut manifest, &remap));
    assert_eq!(
        manifest.synced_items[0].server_id.as_deref(),
        Some(portable)
    );
    assert_eq!(
        manifest.synced_items[1].server_id.as_deref(),
        Some(portable)
    );
    assert_eq!(
        manifest.synced_items[2].server_id.as_deref(),
        Some(portable)
    );
    assert_eq!(manifest.synced_items[3].server_id, None);
    assert_eq!(
        manifest.basket_items[0].server_id.as_deref(),
        Some(portable)
    );
    assert_eq!(
        manifest.basket_items[1].server_id.as_deref(),
        Some(portable)
    );

    // Idempotent: a second pass changes nothing.
    assert!(!reconcile_manifest_server_ids(&mut manifest, &remap));
}

// ===================================================================
// Story 12.2 — AutoFillConfig serde, migration, and accessor parity (AC 2, 3, 4, 8).
// ===================================================================

#[test]
fn autofill_config_deserializes_legacy_block() {
    let cfg: AutoFillConfig =
        serde_json::from_str(r#"{ "enabled": true, "maxBytes": 8000000000 }"#).unwrap();
    assert_eq!(
        cfg.legacy,
        Some(AutoFillPrefs {
            enabled: true,
            max_bytes: Some(8_000_000_000),
        })
    );
    assert!(cfg.pipelines.is_empty());
}

#[test]
fn autofill_config_empty_default_legacy_is_none() {
    // The empty default is "no config" — it must not park a legacy block (no spurious migration).
    let cfg: AutoFillConfig =
        serde_json::from_str(r#"{ "enabled": false, "maxBytes": null }"#).unwrap();
    assert_eq!(cfg.legacy, None);
    assert!(cfg.pipelines.is_empty());
    // An absent/empty object also yields the empty config.
    let empty: AutoFillConfig = serde_json::from_str("{}").unwrap();
    assert_eq!(empty, AutoFillConfig::default());
}

#[test]
fn autofill_config_migrates_legacy_onto_selected_server() {
    // A meaningful legacy block (maxBytes set, enabled false) migrates onto the selected id,
    // carrying `enabled` distinctly from default_legacy's `true`.
    let mut cfg: AutoFillConfig =
        serde_json::from_str(r#"{ "enabled": false, "maxBytes": 5000 }"#).unwrap();
    assert!(cfg.migrate_legacy_to("srv-portable"));
    let expected = {
        let mut p = crate::auto_fill::AutoFillPipeline::default_legacy(Some(5000));
        p.enabled = false;
        p
    };
    assert_eq!(cfg.pipelines.get("srv-portable"), Some(&expected));
    assert!(
        !cfg.pipelines["srv-portable"].enabled,
        "enabled carried over"
    );
    assert_eq!(cfg.pipelines["srv-portable"].budget.max_bytes, Some(5000));
    assert_eq!(cfg.legacy, None);
    // Idempotent: nothing left to migrate.
    assert!(!cfg.migrate_legacy_to("srv-portable"));
}

#[test]
fn autofill_config_migration_never_overwrites_existing_pipeline() {
    let mut cfg = AutoFillConfig::default();
    cfg.set_for("srv-portable", false, Some(999));
    // Synthetically park a legacy block alongside the existing pipeline.
    cfg.legacy = Some(AutoFillPrefs {
        enabled: true,
        max_bytes: Some(1),
    });
    assert!(!cfg.migrate_legacy_to("srv-portable"));
    assert_eq!(cfg.pipelines["srv-portable"].budget.max_bytes, Some(999));
    assert!(!cfg.pipelines["srv-portable"].enabled);
}

#[test]
fn autofill_config_round_trips_per_server_map() {
    let json = r#"{ "srv-abc": { "enabled": true, "ordering": ["favorite"], "budget": { "maxBytes": 1000 } } }"#;
    let cfg: AutoFillConfig = serde_json::from_str(json).unwrap();
    let pipeline = cfg.pipelines.get("srv-abc").expect("pipeline present");
    assert!(pipeline.enabled);
    assert_eq!(
        pipeline.ordering,
        vec![crate::auto_fill::OrderingKey::Favorite]
    );
    assert_eq!(pipeline.budget.max_bytes, Some(1000));
    assert!(cfg.legacy.is_none());
    // Serialize re-emits the per-server map shape and round-trips.
    let reser = serde_json::to_string(&cfg).unwrap();
    assert!(reser.contains("srv-abc"));
    let back: AutoFillConfig = serde_json::from_str(&reser).unwrap();
    assert_eq!(cfg, back);
}

#[test]
fn autofill_config_default_serializes_as_legacy_empty_block() {
    // Pin the empty/default serialization so get_daemon_state and new-device manifests are stable.
    let json = serde_json::to_string(&AutoFillConfig::default()).unwrap();
    assert_eq!(json, r#"{"enabled":false,"maxBytes":null}"#);
    // Byte-for-byte identical to the pre-12.2 AutoFillPrefs::default() output.
    assert_eq!(
        json,
        serde_json::to_string(&AutoFillPrefs::default()).unwrap()
    );
}

#[test]
fn autofill_config_accessor_parity() {
    // Keyed read of a per-server pipeline.
    let mut keyed = AutoFillConfig::default();
    keyed.set_for("srv-1", true, Some(4242));
    assert!(keyed.enabled_for(Some("srv-1")));
    assert_eq!(keyed.max_bytes_for(Some("srv-1")), Some(4242));
    // Single-entry fallback for unkeyed (no-server-context) callers.
    assert!(keyed.enabled_for(None));
    assert_eq!(keyed.max_bytes_for(None), Some(4242));

    // Legacy-block fallback when no pipelines exist.
    let mut legacy_cfg = AutoFillConfig::default();
    legacy_cfg.set_legacy(true, Some(77));
    assert!(legacy_cfg.enabled_for(None));
    assert_eq!(legacy_cfg.max_bytes_for(None), Some(77));
    assert!(legacy_cfg.enabled_for(Some("srv-x")));

    // Empty config: disabled, no budget.
    let empty = AutoFillConfig::default();
    assert!(!empty.enabled_for(None));
    assert_eq!(empty.max_bytes_for(None), None);

    // set_for clears any parked legacy block.
    let mut mixed = AutoFillConfig::default();
    mixed.set_legacy(true, Some(1));
    mixed.set_for("srv-2", false, None);
    assert_eq!(mixed.legacy, None);
    assert!(!mixed.enabled_for(Some("srv-2")));
}

#[test]
fn manifest_reads_legacy_autofill_block() {
    let json = r#"{
        "device_id": "dev-1",
        "version": "1.0",
        "auto_fill": { "enabled": true, "maxBytes": 8000 }
    }"#;
    let manifest: DeviceManifest = serde_json::from_str(json).unwrap();
    assert_eq!(
        manifest.auto_fill.legacy,
        Some(AutoFillPrefs {
            enabled: true,
            max_bytes: Some(8000),
        })
    );
    assert!(manifest.auto_fill.enabled_for(None));
    assert_eq!(manifest.auto_fill.max_bytes_for(None), Some(8000));
}

#[test]
fn manifest_round_trips_per_server_autofill_map() {
    let json = r#"{
        "device_id": "dev-1",
        "version": "1.0",
        "auto_fill": { "srv-xyz": { "enabled": true, "budget": { "maxBytes": 500 } } }
    }"#;
    let manifest: DeviceManifest = serde_json::from_str(json).unwrap();
    assert!(manifest.auto_fill.enabled_for(Some("srv-xyz")));
    assert_eq!(manifest.auto_fill.max_bytes_for(Some("srv-xyz")), Some(500));
    // Re-serialize: auto_fill stays a per-server map.
    let reser = serde_json::to_string(&manifest).unwrap();
    let back: DeviceManifest = serde_json::from_str(&reser).unwrap();
    assert!(back.auto_fill.pipelines.contains_key("srv-xyz"));
}

// --- Code-review regression tests (2026-06-14) ---

#[test]
fn autofill_config_deserializes_partial_legacy_block() {
    // Review fix: a partial legacy block (missing `enabled`) must not fail the whole manifest.
    // `#[serde(default)]` on AutoFillPrefs supplies `enabled = false`.
    let cfg: AutoFillConfig = serde_json::from_str(r#"{ "maxBytes": 123 }"#).unwrap();
    assert_eq!(
        cfg.legacy,
        Some(AutoFillPrefs {
            enabled: false,
            max_bytes: Some(123),
        })
    );
    assert!(cfg.pipelines.is_empty());
    // A whole manifest carrying the partial block also loads.
    let manifest: DeviceManifest = serde_json::from_str(
        r#"{ "device_id": "d", "version": "1.0", "auto_fill": { "maxBytes": 123 } }"#,
    )
    .unwrap();
    assert_eq!(manifest.auto_fill.max_bytes_for(None), Some(123));
}

#[test]
fn set_legacy_updates_sole_pipeline_instead_of_parking_dropped_legacy() {
    // Review fix: with one existing pipeline and no selected server, set_legacy must update the
    // pipeline in place (a parked legacy block would be silently dropped by serialize).
    let mut cfg = AutoFillConfig::default();
    cfg.set_for("srv-1", true, Some(1000));
    cfg.set_legacy(false, Some(2000));
    assert!(cfg.legacy.is_none(), "no dangling legacy block");
    assert_eq!(cfg.pipelines.len(), 1);
    assert!(
        !cfg.pipelines["srv-1"].enabled,
        "new value applied in place"
    );
    assert_eq!(cfg.pipelines["srv-1"].budget.max_bytes, Some(2000));
    // The write survives a serialize → deserialize round-trip.
    let back: AutoFillConfig = serde_json::from_str(&serde_json::to_string(&cfg).unwrap()).unwrap();
    assert_eq!(back, cfg);
    // With no pipelines configured, set_legacy still parks a legacy block (unchanged fallback).
    let mut empty = AutoFillConfig::default();
    empty.set_legacy(true, Some(50));
    assert_eq!(
        empty.legacy,
        Some(AutoFillPrefs {
            enabled: true,
            max_bytes: Some(50)
        })
    );
    assert!(empty.pipelines.is_empty());
}

// --- Story 12.6: AutoFillConfig::set_pipeline (full per-server pipeline write) ---

/// Builds a non-default pipeline exercising every stage (filter/sources/unit/ordering/memory/
/// budget/fallback) so round-trip and "untouched" assertions cover the full shape.
fn rich_pipeline() -> AutoFillPipeline {
    use crate::auto_fill::{
        BudgetStage, ContextRule, ContextStage, ContextWindow, FilterStage, MemoryStage,
        OrderingKey, PityStage, QualityStage, RarityStage, SourceEntry, SourceKind, Unit,
    };
    AutoFillPipeline {
        enabled: true,
        podcast_retention: Default::default(),
        filter: FilterStage {
            include_genres: vec!["Jazz".to_string()],
            exclude_genres: vec!["Metal".to_string()],
            ..FilterStage::default()
        },
        sources: vec![
            SourceEntry {
                kind: SourceKind::Playlist,
                ref_id: Some("pl-42".to_string()),
                share: Some(0.7),
            },
            SourceEntry {
                kind: SourceKind::Favorites,
                ref_id: None,
                share: Some(0.3),
            },
        ],
        unit: Unit::Album,
        ordering: vec![OrderingKey::Quality, OrderingKey::PlayCount],
        memory: MemoryStage {
            cooldown_weeks: Some(4),
            played_exclusion: true,
            ..MemoryStage::default()
        },
        budget: BudgetStage {
            max_bytes: Some(8_000_000_000),
            target_duration_secs: Some(3600),
            headroom_bytes: Some(100),
            encoding_from_goals: true,
        },
        fallback: vec![SourceEntry::new(SourceKind::Library)],
        quality: QualityStage {
            best_version: true,
            version_preference: vec![crate::auto_fill::VersionTrait::Live],
        },
        rarity: RarityStage {
            enabled: true,
            legendary_weight: 8.0,
            rare_weight: 3.0,
            common_weight: 1.0,
            rare_max_plays: 5,
        },
        pity: PityStage {
            enabled: true,
            threshold_syncs: 3,
            guaranteed_ratio: 0.25,
            discovery_max_plays: 0,
        },
        context: ContextStage {
            enabled: true,
            rules: vec![
                ContextRule {
                    window: ContextWindow::TimeOfDay {
                        start_hour: 6,
                        end_hour: 11,
                    },
                    source_refs: vec!["pl-42".to_string()],
                    weight: Some(2.0),
                    ..ContextRule::default()
                },
                ContextRule {
                    window: ContextWindow::Months { months: vec![12] },
                    include_genres: vec!["Christmas".to_string()],
                    ..ContextRule::default()
                },
            ],
        },
        promotion: crate::auto_fill::pipeline::PromotionStage {
            spotlight: true,
            spotlight_share: Some(0.5),
            album_track_ratio: Some(0.3),
            promote_album_min_favorites: Some(2),
            coherence: true,
        },
    }
}

#[test]
fn set_pipeline_upserts_and_clears_parked_legacy() {
    let mut cfg = AutoFillConfig::default();
    // Park a legacy block so we can prove set_pipeline clears it.
    cfg.set_legacy(true, Some(123));
    assert!(cfg.legacy.is_some());

    let pipeline = rich_pipeline();
    cfg.set_pipeline("srv-1", pipeline.clone());
    assert_eq!(cfg.pipelines.get("srv-1"), Some(&pipeline));
    assert!(
        cfg.legacy.is_none(),
        "parked legacy cleared on set_pipeline"
    );

    // Upsert replaces the same server's entry.
    let mut replacement = AutoFillPipeline::default_legacy(Some(42));
    replacement.enabled = false;
    cfg.set_pipeline("srv-1", replacement.clone());
    assert_eq!(cfg.pipelines.get("srv-1"), Some(&replacement));
    assert_eq!(cfg.pipelines.len(), 1);
}

#[test]
fn set_pipeline_leaves_other_servers_untouched() {
    let mut cfg = AutoFillConfig::default();
    let a = rich_pipeline();
    let b = AutoFillPipeline::default_legacy(Some(999));
    cfg.set_pipeline("srv-a", a.clone());
    cfg.set_pipeline("srv-b", b.clone());

    // Rewrite server A; server B must be byte-for-byte unchanged.
    let mut a2 = AutoFillPipeline::default_legacy(Some(1));
    a2.enabled = false;
    cfg.set_pipeline("srv-a", a2.clone());
    assert_eq!(cfg.pipelines.get("srv-a"), Some(&a2));
    assert_eq!(cfg.pipelines.get("srv-b"), Some(&b), "server B untouched");
}

#[test]
fn set_pipeline_round_trips_full_pipeline_with_camelcase_and_ref() {
    let mut cfg = AutoFillConfig::default();
    cfg.set_pipeline("srv-x", rich_pipeline());
    let json = serde_json::to_string(&cfg).unwrap();
    // The playlist source id serializes as `ref` (not refId); enum variants are camelCase.
    assert!(json.contains(r#""ref":"pl-42""#));
    assert!(json.contains(r#""kind":"playlist""#));
    assert!(json.contains(r#""kind":"favorites""#));
    assert!(json.contains(r#""playCount""#));
    assert!(json.contains(r#""quality""#));
    assert!(json.contains(r#""unit":"album""#));
    assert!(json.contains(r#""targetDurationSecs":3600"#));
    assert!(json.contains(r#""headroomBytes":100"#));
    // Full deserialize round-trips the whole pipeline.
    let back: AutoFillConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back, cfg);
}

#[tokio::test]
async fn destination_wire_preserves_distinct_pending_camelcase_identities() {
    let manager = DeviceManager::new(Arc::new(crate::db::Database::memory().unwrap()));
    let first = tempdir().unwrap();
    let second = tempdir().unwrap();
    for dir in [&first, &second] {
        manager
            .handle_device_unrecognized(dir.path().into(), msc(dir.path()), None)
            .await;
    }
    let wire = serde_json::to_value(manager.get_destination_snapshot().await).unwrap();
    let rows = wire["destinations"].as_array().unwrap();
    assert_eq!(rows[0]["kind"], "playback");
    assert!(rows[1]["pendingId"].is_string());
    assert!(rows[2]["pendingId"].is_string());
    assert_ne!(rows[1]["pendingId"], rows[2]["pendingId"]);
    manager
        .handle_device_detected(
            first.path().into(),
            make_manifest("managed", "Managed"),
            msc(first.path()),
        )
        .await
        .unwrap();
    let wire = serde_json::to_value(manager.get_destination_snapshot().await).unwrap();
    let device = wire["destinations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["kind"] == "device")
        .unwrap();
    assert_eq!(device["deviceId"], "managed");
    assert!(device.get("device_id").is_none());
}

#[tokio::test]
async fn failed_discovery_retry_keeps_arrival_order_and_disappearance_clears_issue() {
    let manager = DeviceManager::new(Arc::new(crate::db::Database::memory().unwrap()));
    let dir = tempdir().unwrap();
    let path = dir.path().to_path_buf();
    let mut observed = DiscoveryObservations::default();
    assert!(observed.reconcile([path.clone()]).is_empty());
    let original = observed.token(&path).unwrap();
    manager
        .report_discovery_failure(path.clone(), "DEVICE_READ_FAILED", None)
        .await;
    manager.select_playback().await;
    assert!(observed.reconcile([path.clone()]).is_empty());
    assert_eq!(observed.token(&path), Some(original));
    manager
        .handle_device_detected_at(
            observed.token(&path).unwrap(),
            path.clone(),
            make_manifest("retried", "Retried"),
            msc(dir.path()),
        )
        .await
        .unwrap();
    assert!(manager.get_current_device_path().await.is_none());
    manager
        .report_discovery_failure(path.clone(), "DEVICE_READ_FAILED", None)
        .await;
    let removed = observed.reconcile([]);
    assert_eq!(removed, vec![path.clone()]);
    for missing in removed {
        manager.handle_device_removed(&missing).await;
    }
    assert!(manager.get_discovery_issues().await.is_empty());
    observed.reconcile([path.clone()]);
    assert!(observed.token(&path).unwrap() > original);
    let reconnected = observed.token(&path).unwrap();
    observed.forget(&path); // MTP storage loss without USB disappearance.
    observed.reconcile([path.clone()]);
    assert!(observed.token(&path).unwrap() > reconnected);
}

#[derive(Debug)]
struct GatedInitDeviceIo {
    inner: crate::device_io::MscBackend,
    manifest_writes: tokio::sync::Mutex<Vec<Vec<u8>>>,
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
}

#[async_trait::async_trait]
impl crate::device_io::DeviceIO for GatedInitDeviceIo {
    async fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        self.inner.read_file(path).await
    }
    async fn write_file(&self, path: &str, data: &[u8]) -> Result<()> {
        self.inner.write_file(path, data).await
    }
    async fn write_with_verify(&self, path: &str, data: &[u8]) -> Result<()> {
        self.manifest_writes.lock().await.push(data.to_vec());
        self.entered.notify_one();
        self.release.notified().await;
        self.inner.write_with_verify(path, data).await
    }
    async fn delete_file(&self, path: &str) -> Result<()> {
        self.inner.delete_file(path).await
    }
    async fn list_files(&self, path: &str) -> Result<Vec<crate::device_io::FileEntry>> {
        self.inner.list_files(path).await
    }
    async fn free_space(&self) -> Result<u64> {
        self.inner.free_space().await
    }
    async fn ensure_dir(&self, path: &str) -> Result<()> {
        self.inner.ensure_dir(path).await
    }
    async fn cleanup_empty_subdirs(&self, path: &str) -> Result<()> {
        self.inner.cleanup_empty_subdirs(path).await
    }
}

#[tokio::test]
async fn device_initialize_preferences_commit_to_first_write_of_unselected_pending_target() {
    let manager = Arc::new(DeviceManager::new(Arc::new(crate::db::Database::memory().unwrap())));
    let selected_dir = tempdir().unwrap();
    manager.handle_device_detected(
        selected_dir.path().into(), make_manifest("selected", "Selected"), msc(selected_dir.path()),
    ).await.unwrap();
    let dir = tempdir().unwrap();
    let io = Arc::new(GatedInitDeviceIo {
        inner: crate::device_io::MscBackend::new(dir.path().into()),
        manifest_writes: tokio::sync::Mutex::new(vec![]),
        entered: tokio::sync::Notify::new(),
        release: tokio::sync::Notify::new(),
    });
    manager.handle_device_unrecognized(dir.path().into(), io.clone(), None).await;
    assert!(manager.select_device(selected_dir.path().into()).await);
    let pending_id = manager.get_pending_devices_snapshot().await[0].pending_id.clone();
    let revision = manager.get_destination_snapshot().await.revision;
    let artwork = FolderArtwork { enabled: false };
    let task = {
        let manager = manager.clone();
        let artwork = artwork.clone();
        tokio::spawn(async move {
            manager.initialize_pending_device(
                &pending_id, revision, "Music", None, None, "New player".into(), None, true, artwork,
            ).await
        })
    };
    tokio::time::timeout(std::time::Duration::from_secs(2), io.entered.notified()).await.unwrap();
    let writes = io.manifest_writes.lock().await;
    assert_eq!(writes.len(), 1);
    let first: DeviceManifest = manifest_codec::decode_test_manifest(&writes[0]).unwrap();
    assert!(first.reconcile_before_sync);
    assert_eq!(first.folder_artwork, artwork);
    assert!(first.folder_artwork_records.is_empty());
    drop(writes);
    io.release.notify_one();
    let initialized = task.await.unwrap().unwrap();
    assert_eq!(serde_json::to_value(&initialized).unwrap(), serde_json::to_value(&first).unwrap());
    let persisted: DeviceManifest = manifest_codec::decode_test_manifest(
        &std::fs::read(dir.path().join(".hifimule.json.zst")).unwrap(),
    ).unwrap();
    assert_eq!(serde_json::to_value(&persisted).unwrap(), serde_json::to_value(&first).unwrap());
    let selected = manager.get_current_device().await.unwrap();
    assert_eq!(selected.device_id, "selected");
    assert!(!selected.reconcile_before_sync);
    assert_eq!(selected.folder_artwork, FolderArtwork::default());
    assert!(!selected_dir.path().join(".hifimule.json.zst").exists());
    assert_eq!(io.manifest_writes.lock().await.len(), 1);
    // Reconnecting reloads the preferences from the initial manifest.
    manager.handle_device_removed(&dir.path().to_path_buf()).await;
    manager.handle_device_detected(dir.path().into(), persisted, msc(dir.path())).await.unwrap();
    assert!(manager.select_device(dir.path().into()).await);
    let reconnected = manager.get_current_device().await.unwrap();
    assert!(reconnected.reconcile_before_sync);
    assert_eq!(reconnected.folder_artwork, artwork);
}

#[tokio::test]
async fn pending_setup_rejects_duplicate_submission_and_removal_during_commit() {
    let manager = Arc::new(DeviceManager::new(Arc::new(
        crate::db::Database::memory().unwrap(),
    )));
    let dir = tempdir().unwrap();
    let io = Arc::new(GatedInitDeviceIo {
        inner: crate::device_io::MscBackend::new(dir.path().into()),
        manifest_writes: tokio::sync::Mutex::new(vec![]),
        entered: tokio::sync::Notify::new(),
        release: tokio::sync::Notify::new(),
    });
    manager
        .handle_device_unrecognized(dir.path().into(), io.clone(), None)
        .await;
    let pending = manager.get_unrecognized_device_snapshot().await.unwrap();
    let revision = manager.get_destination_snapshot().await.revision;
    let task = {
        let manager = manager.clone();
        let id = pending.pending_id.clone();
        tokio::spawn(async move {
            manager
                .initialize_pending_device(
                    &id, revision, "", None, None, "First".into(), None, false, Default::default(),
                )
                .await
        })
    };
    tokio::time::timeout(std::time::Duration::from_secs(2), io.entered.notified())
        .await
        .unwrap();
    let duplicate = manager
        .initialize_pending_device(
            &pending.pending_id,
            revision,
            "",
            None,
            None,
            "Duplicate".into(),
            None,
            false,
            Default::default(),
        )
        .await;
    assert!(
        duplicate
            .unwrap_err()
            .to_string()
            .contains("already initializing")
    );
    manager
        .handle_device_removed(&dir.path().to_path_buf())
        .await;
    io.release.notify_one();
    assert!(
        task.await
            .unwrap()
            .unwrap_err()
            .to_string()
            .contains("no longer available")
    );
    assert!(manager.get_pending_devices_snapshot().await.is_empty());
    assert_eq!(
        manager.get_destination_snapshot().await.destinations.len(),
        1
    );
}

#[tokio::test]
async fn failed_pending_setup_releases_reservation_for_retry() {
    let manager = DeviceManager::new(Arc::new(crate::db::Database::memory().unwrap()));
    let dir = tempdir().unwrap();
    manager
        .handle_device_unrecognized(
            dir.path().into(),
            FailingMirrorDeviceIo::new(dir.path()),
            None,
        )
        .await;
    let pending = manager.get_unrecognized_device_snapshot().await.unwrap();
    let revision = manager.get_destination_snapshot().await.revision;
    for _ in 0..2 {
        let error = manager
            .initialize_pending_device(
                &pending.pending_id,
                revision,
                "",
                None,
                None,
                "Retry".into(),
                None,
                false,
                Default::default(),
            )
            .await
            .unwrap_err();
        assert!(format!("{error:#}").contains("injected MTP mirror failure"));
    }
    assert_eq!(manager.get_pending_devices_snapshot().await.len(), 1);
}

#[tokio::test]
async fn removed_sync_target_cannot_admit_new_operations() {
    let manager = DeviceManager::new(Arc::new(crate::db::Database::memory().unwrap()));
    let operations = crate::sync::SyncOperationManager::new();
    let dir = tempdir().unwrap();
    manager
        .handle_device_detected(
            dir.path().into(),
            make_manifest("target", "Target"),
            msc(dir.path()),
        )
        .await
        .unwrap();
    let admitted = manager
        .admit_sync_operation(&operations, "before-removal".into(), 1, "target")
        .await
        .unwrap();
    assert_eq!(admitted.device_id.as_deref(), Some("target"));
    manager
        .handle_device_removed(&dir.path().to_path_buf())
        .await;
    assert!(
        manager
            .admit_sync_operation(&operations, "after-removal".into(), 1, "target")
            .await
            .is_err()
    );
    assert!(operations.get_operation("after-removal").await.is_none());
}

#[tokio::test]
async fn manifest_migration_regression_detection_persists_only_proven_alias_repair() {
    for (history, dirty, pending, current, legacy) in [
        (Some("passthrough"), false, false),
        (Some("passthrough"), true, false),
        (Some("passthrough"), false, true),
        (Some("passthrough"), true, true),
        (None, false, false),
        (Some("rockbox-mp3-320"), false, false),
    ]
    .into_iter()
    .flat_map(|(h, dirty, pending)| {
        [
            (h, dirty, pending, None, false),
            (h, dirty, pending, None, true),
            (h, dirty, pending, Some("passthrough"), false),
            (h, dirty, pending, Some("passthrough"), true),
        ]
    }) {
        let dir = tempdir().unwrap();
        let mut manifest = make_manifest("profile-recovery", "MSC");
        manifest.last_synced_transcoding_profile_id = history.map(str::to_string);
        manifest.transcoding_profile_id = current.map(str::to_string);
        manifest.transcoding_profile_dirty = true;
        manifest.dirty = dirty;
        if pending {
            manifest.pending_item_ids = vec!["unfinished".into()];
        }
        manifest.synced_items = serde_json::from_str(
            r#"[{"providerItemId":"track","name":"Track","localPath":"Music/Track.flac",
                "sizeBytes":1234,"syncedAt":"2026-10-11T00:00:00Z"}]"#,
        )
        .unwrap();
        if legacy {
            std::fs::write(
                dir.path().join(".hifimule.json"),
                serde_json::to_vec(&manifest).unwrap(),
            )
            .unwrap();
        } else {
            write_manifest(msc(dir.path()), &manifest).await.unwrap();
        }
        let probed = DeviceProber::probe(dir.path()).await.unwrap().unwrap();
        let manager = DeviceManager::new(Arc::new(crate::db::Database::memory().unwrap()));
        manager
            .handle_device_detected(dir.path().to_path_buf(), probed, msc(dir.path()))
            .await
            .unwrap();
        for loaded in [
            manager.get_current_device().await.unwrap(),
            DeviceProber::probe(dir.path()).await.unwrap().unwrap(),
        ] {
            assert!(
                loaded.transcoding_profile_dirty,
                "populated manifests have unknown historical provenance"
            );
            assert_eq!(loaded.dirty, manifest.dirty);
            assert_eq!(loaded.pending_item_ids, manifest.pending_item_ids);
            assert_eq!(
                loaded.transcoding_profile_id,
                manifest.transcoding_profile_id
            );
            assert_eq!(
                loaded.last_synced_transcoding_profile_id,
                manifest.last_synced_transcoding_profile_id
            );
            assert_eq!(
                serde_json::to_value(loaded.synced_items).unwrap(),
                serde_json::to_value(&manifest.synced_items).unwrap()
            );
        }
    }
}

#[test]
fn manifest_migration_regression_windows_discovery_excludes_only_optical_drives() {
    // C fixed, D empty optical, G managed removable, N network, R RAM drive.
    let mask = [2, 3, 6, 13, 17]
        .into_iter()
        .fold(0, |mask, bit| mask | (1 << bit));
    let mounts = windows_mounts_from_mask(mask, |path| path == Path::new("D:\\"));
    let expected: Vec<PathBuf> = ["C:\\", "G:\\", "N:\\", "R:\\"]
        .into_iter()
        .map(PathBuf::from)
        .collect();
    assert_eq!(mounts, expected);
}

#[tokio::test]
async fn manifest_migration_regression_detection_marker_preserves_profile_recovery() {
    let dir = tempdir().unwrap();
    let mut manifest = make_manifest("marker-profile-recovery", "MSC");
    manifest.last_synced_transcoding_profile_id = Some("passthrough".into());
    manifest.transcoding_profile_dirty = true;
    write_manifest(msc(dir.path()), &manifest).await.unwrap();
    std::fs::write(dir.path().join("interrupted.mp3.dirty"), b"unfinished").unwrap();
    let manager = DeviceManager::new(Arc::new(crate::db::Database::memory().unwrap()));
    manager
        .handle_device_detected(dir.path().into(), manifest, msc(dir.path()))
        .await
        .unwrap();
    let loaded = manager.get_current_device().await.unwrap();
    assert!(loaded.dirty);
    assert!(
        loaded.needs_transcoding_profile_rewrite(),
        "a physical interruption marker prevents alias repair"
    );
    assert!(
        DeviceProber::probe(dir.path())
            .await
            .unwrap()
            .unwrap()
            .transcoding_profile_dirty
    );
}

#[tokio::test]
async fn manifest_migration_regression_unknown_root_scan_keeps_profile_evidence() {
    for populated in [false, true] {
        let dir = tempdir().unwrap();
        let mut manifest = make_manifest("scan-profile-recovery", "MSC");
        manifest.last_synced_transcoding_profile_id = Some("passthrough".into());
        manifest.transcoding_profile_dirty = true;
        if populated {
            manifest.synced_items = serde_json::from_str(r#"[{"providerItemId":"track","name":"Track","localPath":"Music/Track.mp3","sizeBytes":1234,"syncedAt":"2026-10-11T00:00:00Z"}]"#).unwrap();
        }
        write_manifest(msc(dir.path()), &manifest).await.unwrap();
        let io = Arc::new(ManifestPublicationFaultIo {
            inner: msc(dir.path()),
            corrupt_next_write: false.into(),
            fail_sync_call: None,
            sync_calls: 0.into(),
            fail_listing: true,
        });
        let manager = DeviceManager::new(Arc::new(crate::db::Database::memory().unwrap()));
        manager
            .handle_device_detected(dir.path().into(), manifest, io)
            .await
            .unwrap();
        let loaded = manager.get_current_device().await.unwrap();
        assert!(
            loaded.transcoding_profile_dirty && loaded.needs_transcoding_profile_rewrite(),
            "failed enumeration means unknown recovery"
        );
        for profile in [None, Some("rockbox-mp3-320"), None] {
            manager
                .update_manifest(|m| m.set_transcoding_profile(profile.map(str::to_string)))
                .await
                .unwrap();
            assert!(
                manager
                    .get_current_device()
                    .await
                    .unwrap()
                    .needs_transcoding_profile_rewrite(),
                "settings cannot establish clean enumeration"
            );
        }
        assert!(
            DeviceProber::probe(dir.path())
                .await
                .unwrap()
                .unwrap()
                .transcoding_profile_dirty
        );
    }
}

#[test]
fn manifest_migration_regression_recovery_roots_include_configured_and_recorded_paths() {
    let mut manifest = make_manifest("recovery-scope", "MSC");
    manifest.managed_paths = vec!["Music".into()];
    manifest.audiobook_path = Some("Books".into());
    manifest.podcast_path = Some("Podcasts".into());
    manifest.playlist_path = Some("Lists".into());
    manifest.synced_items = serde_json::from_str(r#"[{"providerItemId":"track","name":"Track","localPath":"Archive/Track.flac","sizeBytes":1234,"syncedAt":"2026-10-11T00:00:00Z"}]"#).unwrap();
    manifest.playlists = serde_json::from_str(r#"[{"jellyfinId":"playlist","filename":"OldLists/Old.m3u","trackCount":1,"trackIds":["track"],"lastModified":"2026-10-11T00:00:00Z"}]"#).unwrap();
    let roots: std::collections::BTreeSet<_> = manifest.recovery_scan_roots().into_iter().collect();
    let expected = ["Archive", "Books", "Lists", "Music", "OldLists", "Podcasts"]
        .into_iter()
        .map(str::to_string)
        .collect();
    assert_eq!(roots, expected);
}

#[tokio::test]
async fn manifest_migration_regression_empty_alias_repair_requires_known_clean_scan() {
    for current in [None, Some("passthrough")] {
        let dir = tempdir().unwrap();
        let mut manifest = make_manifest("empty-profile-recovery", "MSC");
        manifest.last_synced_transcoding_profile_id = Some("passthrough".into());
        manifest.transcoding_profile_id = current.map(str::to_string);
        manifest.transcoding_profile_dirty = true;
        write_manifest(msc(dir.path()), &manifest).await.unwrap();
        let manager = DeviceManager::new(Arc::new(crate::db::Database::memory().unwrap()));
        manager
            .handle_device_detected(dir.path().into(), manifest, msc(dir.path()))
            .await
            .unwrap();
        assert!(
            !DeviceProber::probe(dir.path())
                .await
                .unwrap()
                .unwrap()
                .transcoding_profile_dirty
        );
    }
}

#[tokio::test]
async fn manifest_migration_regression_older_auto_upgrade_keeps_uncertain_dirty() {
    for current in [Some("rockbox-mp3-320"), None] {
        let dir = tempdir().unwrap();
        let mut manifest = make_manifest("older-auto-profile", "MSC");
        manifest.transcoding_profile_id = current.map(str::to_string);
        manifest.last_synced_transcoding_profile_id = Some("passthrough".into());
        manifest.transcoding_profile_dirty = true;
        manifest.last_sync_at = Some(1_775_000_000);
        manifest.synced_items = serde_json::from_str(r#"[{"providerItemId":"track","name":"Converted Track","localPath":"Music/Track.mp3","sizeBytes":1234,"originalContainer":"flac","syncedAt":"2026-04-01T00:00:00Z"}]"#).unwrap();
        write_manifest(msc(dir.path()), &manifest).await.unwrap();
        let manager = DeviceManager::new(Arc::new(crate::db::Database::memory().unwrap()));
        manager
            .handle_device_detected(dir.path().into(), manifest.clone(), msc(dir.path()))
            .await
            .unwrap();
        manager
            .update_manifest(|m| m.set_transcoding_profile(None))
            .await
            .unwrap();
        let durable = DeviceProber::probe(dir.path()).await.unwrap().unwrap();
        assert!(
            durable.needs_transcoding_profile_rewrite(),
            "older automatic completion history is untrustworthy"
        );
        assert_eq!(
            serde_json::to_value(durable.synced_items).unwrap(),
            serde_json::to_value(manifest.synced_items).unwrap()
        );
    }
}

#[tokio::test]
async fn manifest_migration_regression_explicit_confirmation_is_durable() {
    let dir = tempdir().unwrap();
    let mut manifest = make_manifest("confirm-profile", "MSC");
    manifest.last_synced_transcoding_profile_id = Some("passthrough".into());
    manifest.transcoding_profile_dirty = true;
    manifest.synced_items = serde_json::from_str(r#"[{"providerItemId":"track","name":"Track","localPath":"Music/Track.flac","sizeBytes":1234,"syncedAt":"2026-10-11T00:00:00Z"}]"#).unwrap();
    write_manifest(msc(dir.path()), &manifest).await.unwrap();
    let manager = DeviceManager::new(Arc::new(crate::db::Database::memory().unwrap()));
    manager
        .handle_device_detected(dir.path().into(), manifest.clone(), msc(dir.path()))
        .await
        .unwrap();
    let (_, _, io, observation) = manager
        .get_sync_target_for_device(&manifest.device_id)
        .await
        .unwrap();
    manager
        .confirm_passthrough_profile_for_connection(
            &manifest.device_id,
            observation,
            &io,
            &manifest.recovery_scan_roots(),
        )
        .await
        .unwrap();
    let durable = DeviceProber::probe(dir.path()).await.unwrap().unwrap();
    manifest.transcoding_profile_dirty = false;
    manifest.last_synced_transcoding_profile_id = None;
    assert_eq!(
        serde_json::to_value(durable).unwrap(),
        serde_json::to_value(manifest).unwrap()
    );
    assert!(
        !manager
            .get_current_device()
            .await
            .unwrap()
            .needs_transcoding_profile_rewrite()
    );
}

#[tokio::test]
async fn manifest_migration_regression_explicit_confirmation_rejects_changed_state() {
    for scenario in ["profile", "dirty", "pending", "connection", "roots"] {
        let dir = tempdir().unwrap();
        let mut manifest = make_manifest("reject-profile-confirm", "MSC");
        manifest.last_synced_transcoding_profile_id = Some("passthrough".into());
        manifest.transcoding_profile_dirty = true;
        if scenario == "profile" {
            manifest.transcoding_profile_id = Some("rockbox-mp3-320".into());
        }
        if scenario == "dirty" {
            manifest.dirty = true;
        }
        if scenario == "pending" {
            manifest.pending_item_ids = vec!["unfinished".into()];
        }
        write_manifest(msc(dir.path()), &manifest).await.unwrap();
        let manager = DeviceManager::new(Arc::new(crate::db::Database::memory().unwrap()));
        manager
            .handle_device_detected(dir.path().into(), manifest, msc(dir.path()))
            .await
            .unwrap();
        let (_, mut before, io, observation) = manager
            .get_sync_target_for_device("reject-profile-confirm")
            .await
            .unwrap();
        let recovery_roots = before.recovery_scan_roots();
        if scenario == "roots" {
            manager
                .update_manifest(|m| m.managed_paths = vec!["ChangedMusic".into()])
                .await
                .unwrap();
            before = manager.get_current_device().await.unwrap();
        }
        let bytes = std::fs::read(dir.path().join(".hifimule.json.zst")).unwrap();
        let result = manager
            .confirm_passthrough_profile_for_connection(
                "reject-profile-confirm",
                observation + u64::from(scenario == "connection"),
                &io,
                &recovery_roots,
            )
            .await;
        assert!(result.is_err(), "reject {scenario}");
        assert_eq!(
            std::fs::read(dir.path().join(".hifimule.json.zst")).unwrap(),
            bytes
        );
        assert_eq!(
            serde_json::to_value(manager.get_current_device().await.unwrap()).unwrap(),
            serde_json::to_value(before).unwrap()
        );
    }
}

#[test]
fn reconciliation_defaults_off_and_survives_manifest_roundtrip_and_polling() {
    let legacy = serde_json::json!({"device_id":"legacy", "version":"1.0"});
    let mut manifest: DeviceManifest = serde_json::from_value(legacy).unwrap();
    assert!(!manifest.reconcile_before_sync);
    assert!(!DeviceManifest::default().reconcile_before_sync);
    manifest.reconcile_before_sync = true;
    let serialized = serde_json::to_value(&manifest).unwrap();
    assert_eq!(serialized["reconcileBeforeSync"], true);
    let restored: DeviceManifest = serde_json::from_value(serialized).unwrap();
    assert!(restored.reconcile_before_sync);
    assert!(restored.polling_metadata().reconcile_before_sync);
}

#[test]
fn folder_artwork_legacy_defaults_and_compressed_ownership_roundtrip() {
    let mut manifest = DeviceManifest { device_id:"artwork".into(), version:"1.0".into(), ..Default::default() };
    let mut legacy = serde_json::to_value(&manifest).unwrap();
    legacy.as_object_mut().unwrap().remove("folderArtwork");
    let old: DeviceManifest = serde_json::from_value(legacy).unwrap();
    assert_eq!(old.folder_artwork, FolderArtwork::default());
    manifest.folder_artwork.enabled=true;
    manifest.folder_artwork_records.insert("Music/Artist/Album".into(), FolderArtworkRecord {
        source:Some(FolderArtworkSource {server_id:Some("source".into()),entity_id:"album".into(),artist:false,cover_id:Some("cover".into())}),
        jpeg_hash:Some("hash".into()),native:std::collections::BTreeMap::from([("kde".into(),"token".into())]),
    });
    let decoded: DeviceManifest=manifest_codec::decode_test_manifest(&manifest_codec::encode(&manifest).unwrap()).unwrap();
    assert_eq!(decoded.folder_artwork,manifest.folder_artwork);
    assert_eq!(decoded.folder_artwork_records,manifest.folder_artwork_records);
    let mut legacy = serde_json::to_value(&manifest).unwrap();
    legacy["folderArtwork"] = serde_json::json!({"enabled": true, "windows": true, "macos": false, "kde": false, "gnome": true});
    let loaded: DeviceManifest = serde_json::from_value(legacy.clone()).unwrap();
    assert_eq!(loaded.folder_artwork_records, manifest.folder_artwork_records);
    assert_eq!(serde_json::to_value(&loaded.folder_artwork).unwrap(), serde_json::json!({"enabled": true}));
    let saved = manifest_codec::encode(&loaded).unwrap();
    let saved: DeviceManifest = manifest_codec::decode_test_manifest(&saved).unwrap();
    assert_eq!(saved.folder_artwork, FolderArtwork { enabled: true });
    for invalid in [serde_json::json!({"enabled": true, "windows": "true"}), serde_json::json!({"enabled": true, "other": true})] {
        legacy["folderArtwork"] = invalid;
        assert!(serde_json::from_value::<DeviceManifest>(legacy.clone()).is_err());
    }
    let mut target = crate::sync::SyncTarget { path: std::path::PathBuf::from("test"), manifest:decoded, io:Arc::new(crate::device_io::MscBackend::new("test".into())), observation:1 };
    crate::sync_plan::compact_target(&mut target);
    assert_eq!(target.manifest.folder_artwork_records,manifest.folder_artwork_records);
}
