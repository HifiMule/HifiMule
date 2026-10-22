// Exercise the real provider admission and managed transfer with the shipped preset.
#[tokio::test]
async fn mp3_direct_copy_audiobookshelf_preview_and_transfer() {
    use crate::providers::ProviderLibraryRole;
    let mut server = mockito::Server::new_async().await;
    let _login = server
        .mock("POST", "/login")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"user":{"accessToken":"fixture-token"}}"#)
        .create_async()
        .await;
    let _libraries = server
        .mock("GET", "/api/libraries")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"libraries":[{"id":"l","name":"Podcasts","mediaType":"podcast"}]}"#)
        .create_async()
        .await;
    let provider = Arc::new(
        crate::providers::audiobookshelf::AudiobookshelfProvider::from_stored_config(
            &server.url(),
            "fixture",
            "fixture",
            "l",
            ProviderLibraryRole::Podcast,
        )
        .await
        .unwrap(),
    );
    let audio = include_bytes!("../../tests/fixtures/audiobookshelf/tags/untagged.mp3");
    let _play = server.mock("POST", "/api/items/i/play/e")
        .match_body(mockito::Matcher::Json(serde_json::json!({"forceDirectPlay":true})))
        .with_status(200).with_header("content-type", "application/json")
        .with_body(r#"{"id":"session","serverVersion":"2.37.1","libraryId":"l","libraryItemId":"i","mediaType":"podcast","episodeId":"e","playMethod":0,"audioTracks":[{"ino":"f","contentUrl":"/api/items/i/file/f","mimeType":"audio/mpeg","codec":"mp3"}]}"#)
        .create_async().await;
    let _probe = server
        .mock("GET", "/api/items/i/file/f")
        .match_header("range", "bytes=0-0")
        .with_status(206)
        .with_header("content-type", "audio/mpeg")
        .with_header("accept-ranges", "bytes")
        .with_header("content-range", &format!("bytes 0-0/{}", audio.len()))
        .with_body(vec![audio[0]])
        .create_async()
        .await;
    let download = server
        .mock("GET", "/api/items/i/file/f")
        .match_header("range", mockito::Matcher::Missing)
        .with_status(200)
        .with_header("content-type", "audio/mpeg")
        .with_body(audio)
        .expect(1)
        .create_async()
        .await;
    let _close = server
        .mock("POST", "/api/session/session/close")
        .with_status(200)
        .create_async()
        .await;
    let progress = server
        .mock(
            "PATCH",
            mockito::Matcher::Regex("/api/me/progress/.*".into()),
        )
        .expect(0)
        .create_async()
        .await;
    let mut presets: Value =
        serde_json::from_slice(include_bytes!("../../assets/device-profiles.json")).unwrap();
    let profile = presets["profiles"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|p| p["id"] == "rockbox-mp3-192")
        .unwrap()["deviceProfile"]
        .clone();
    let mut add = add_item("abs-episode-6c.69.6d.65", None);
    add.media_role = crate::device::MediaRole::Podcast;
    add.name = "Episode".into();
    add.album = Some("Show".into());
    add.size_bytes = audio.len() as u64;
    add.duration_seconds = 1;
    // The estimate is deliberately unrelated: only verified audio evidence is authoritative.
    add.original_bitrate = Some(128_000);
    for (bitrate, override_kbps, allowed) in [
        (serde_json::json!(128000), None, true),
        (serde_json::json!(192000), None, true),
        (serde_json::json!(192001), None, false),
        (serde_json::json!(320000), None, false),
        (Value::Null, None, true),
        (serde_json::json!(0), None, true),
        (serde_json::json!(-1), None, true),
        (serde_json::json!(4294967296u64), None, true),
        (serde_json::json!("broken"), None, true),
        (serde_json::json!(128000), Some(96), false),
    ] {
        let detail = server.mock("GET", "/api/items/i").with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::json!({"id":"i","libraryId":"l","mediaType":"podcast","media":{
                "id":"m","metadata":{"title":"Show"},"episodes":[{"id":"e","title":"Episode","audioFile":{"ino":"f","duration":1,"bitRate":bitrate,"channels":2}}]
            }}).to_string()).create_async().await;
        add.max_bitrate_override_kbps = override_kbps;
        let blocked =
            blocked_provider_media_add(&add, provider.as_ref(), Some(&profile), None).await;
        assert_eq!(
            blocked.is_none(),
            allowed,
            "bitrate {bitrate}, override {override_kbps:?}: {blocked:?}"
        );
        detail.remove_async().await;
    }
    add.max_bitrate_override_kbps = None;
    for (bitrate, stale_bitrate, override_kbps, allowed) in [
        (128000, 320000, None, true),
        (320000, 128000, None, false),
        (128000, 128000, Some(96), false),
    ] {
        add.original_bitrate = Some(stale_bitrate);
        add.max_bitrate_override_kbps = override_kbps;
        let detail = server.mock("GET", "/api/items/i").with_status(200)
        .with_header("content-type", "application/json")
        .with_body(serde_json::json!({"id":"i","libraryId":"l","mediaType":"podcast","media":{"id":"m","metadata":{"title":"Show"},"episodes":[{"id":"e","title":"Episode","audioFile":{"ino":"f","duration":1,"bitRate":bitrate,"channels":2}}]}}).to_string()).create_async().await;
        let root = tempfile::tempdir().unwrap();
        let manifest = crate::device::DeviceManifest {
            device_id: "mp3-device".into(),
            managed_paths: vec!["Music".into()],
            podcast_path: Some("Podcasts".into()),
            ..Default::default()
        };
        let io: Arc<dyn crate::device_io::DeviceIO> =
            Arc::new(crate::device_io::MscBackend::new(root.path().to_path_buf()));
        crate::device::write_manifest(io.clone(), &manifest)
            .await
            .unwrap();
        let devices = Arc::new(crate::device::DeviceManager::new(Arc::new(
            crate::db::Database::memory().unwrap(),
        )));
        devices
            .handle_device_detected(root.path().to_path_buf(), manifest, io.clone())
            .await
            .unwrap();
        devices.set_test_sync_io(root.path(), io).await;
        let target: crate::sync::SyncTarget =
            devices.get_selected_sync_target().await.unwrap().into();
        let operations = Arc::new(crate::sync::SyncOperationManager::new());
        operations.create_operation("mp3-copy".into(), 1).await;
        let delta = crate::sync::SyncDelta {
            adds: vec![add.clone()],
            provenance_updates: Default::default(),
            blocked: vec![],
            deletes: vec![],
            id_changes: vec![],
            unchanged: 0,
            playlists: vec![],
            pity_fired_servers: vec![],
        };
        let source = crate::sync::ProviderSyncSource {
            provider: provider.clone(),
            transcoding_profile: Some(profile.clone()),
            providers_by_server: Default::default(),
            local_encoding_servers: Default::default(),
        };
        let (written, errors) = crate::sync::execute_provider_sync(
            &delta,
            &target,
            source,
            operations,
            "mp3-copy".into(),
            devices.clone(),
        )
        .await
        .unwrap();
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(
            written.len(),
            usize::from(allowed),
            "current {bitrate}, stale {stale_bitrate}, override {override_kbps:?}"
        );
        if allowed {
            assert_eq!(written[0].original_bitrate, Some(bitrate));
            assert!(written[0].local_path.starts_with("Podcasts/"));
            assert!(written[0].local_path.ends_with(".mp3"));
            assert_eq!(
                std::fs::read(root.path().join(&written[0].local_path)).unwrap(),
                audio
            );
            assert_eq!(
                devices.get_current_device().await.unwrap().synced_items,
                written
            );
        } else {
            assert!(
                devices
                    .get_current_device()
                    .await
                    .unwrap()
                    .synced_items
                    .is_empty()
            );
        }
        detail.remove_async().await;
    }
    download.assert_async().await;
    progress.assert_async().await;
}
