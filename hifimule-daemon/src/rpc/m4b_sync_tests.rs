// Real ABS admission, preview, local files and manifest publication; only HTTP is simulated.
#[tokio::test]
async fn m4b_audiobookshelf_preview_and_managed_transfer() {
    use crate::providers::ProviderLibraryRole;
    let audio = include_bytes!("../../tests/fixtures/audiobookshelf/tags/chapters-faststart.m4a");
    let presets: Value =
        serde_json::from_slice(include_bytes!("../../assets/device-profiles.json")).unwrap();
    for role in [ProviderLibraryRole::Audiobook, ProviderLibraryRole::Podcast] {
        let book = role == ProviderLibraryRole::Audiobook;
        let mut server = mockito::Server::new_async().await;
        let _login = server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"user":{"accessToken":"fixture-token"}}"#)
            .create_async()
            .await;
        let _libraries = server.mock("GET", "/api/libraries").with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::json!({"libraries":[{"id":"l","name":"Spoken","mediaType":if book {"book"} else {"podcast"}}]}).to_string()).create_async().await;
        let provider = Arc::new(
            crate::providers::audiobookshelf::AudiobookshelfProvider::from_stored_config(
                &server.url(),
                "fixture",
                "fixture",
                "l",
                role,
            )
            .await
            .unwrap(),
        );
        let _play = server.mock("POST", if book {"/api/items/i/play"} else {"/api/items/i/play/e"})
            .match_body(mockito::Matcher::Json(serde_json::json!({"forceDirectPlay":true})))
            .with_status(200).with_header("content-type", "application/json")
            .with_body(serde_json::json!({"id":"session","serverVersion":"2.37.1","libraryId":"l","libraryItemId":"i",
                "mediaType":if book {"book"} else {"podcast"},"episodeId":"e","playMethod":0,
                "audioTracks":[{"ino":"f","contentUrl":"/api/items/i/file/f","mimeType":"audio/mp4","codec":"aac"}]
            }).to_string()).create_async().await;
        let _probe = server
            .mock("GET", "/api/items/i/file/f")
            .match_header("range", "bytes=0-0")
            .with_status(206)
            .with_header("content-type", "audio/mp4")
            .with_header("accept-ranges", "bytes")
            .with_header("content-range", &format!("bytes 0-0/{}", audio.len()))
            .with_body(vec![audio[0]])
            .create_async()
            .await;
        let _download = server
            .mock("GET", "/api/items/i/file/f")
            .match_header("range", mockito::Matcher::Missing)
            .with_status(200)
            .with_header("content-type", "audio/mp4")
            .with_body(audio)
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
        let id = if book {
            "abs-track-6c.69.6d.66"
        } else {
            "abs-episode-6c.69.6d.65"
        };
        let detail_body = |metadata: Value| {
            let file = serde_json::json!({"ino":"f","index":1,"duration":1,"bitRate":128000,"channels":2,"metadata":metadata});
            serde_json::json!({"id":"i","libraryId":"l","mediaType":if book {"book"} else {"podcast"},"media":
                if book {serde_json::json!({"id":"m","metadata":{"title":"Current book","authors":[{"name":"Author"}]},"audioFiles":[file]})}
                else {serde_json::json!({"id":"m","metadata":{"title":"Show"},"episodes":[{"id":"e","title":"Episode","audioFile":file}]})}
            }).to_string()
        };
        for (metadata, expected) in [
            (serde_json::json!({"ext":".m4b"}), "m4b"),
            (serde_json::json!({"ext":".M4B"}), "m4b"),
            (Value::Null, "m4a"),
            (serde_json::json!("broken"), "m4a"),
            (serde_json::json!({"ext":12}), "m4a"),
            (serde_json::json!({"filename":"not-evidence.m4b"}), "m4a"),
            (serde_json::json!({"ext":".m4a"}), "m4a"),
        ] {
            let detail = server
                .mock("GET", "/api/items/i")
                .with_status(200)
                .with_header("content-type", "application/json")
                .with_body(detail_body(metadata.clone()))
                .create_async()
                .await;
            let media = provider.resolve_sync_media(id).await.unwrap();
            assert_eq!(media.container, expected, "{role:?} metadata {metadata}");
            assert_eq!(media.codec, "aac");
            let playback = provider.resolve_playback(id).await.unwrap();
            assert_eq!(
                playback.representations[0].container.as_deref(),
                Some("m4a")
            );
            detail.remove_async().await;
        }
        let _detail = server
            .mock("GET", "/api/items/i")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(detail_body(serde_json::json!({"ext":".m4b"})))
            .create_async()
            .await;
        let mut add = add_item(id, None);
        add.media_role = if book {
            crate::device::MediaRole::Audiobook
        } else {
            crate::device::MediaRole::Podcast
        };
        add.name = "Part".into();
        add.album = Some("Spoken".into());
        add.artist = Some("Author".into());
        add.size_bytes = audio.len() as u64;
        add.duration_seconds = 1;
        for (profile_id, allowed, local_encoding) in [
            ("modern-dap-lossless-m4b", true, false),
            ("rockbox-audiobooks-m4b", true, false),
            ("audiobooks-m4b-mp3", true, false),
            ("pocketbook-audiobooks", true, false),
            ("android-voice-audiobooks", true, false),
            ("garmin-music", true, false),
            ("passthrough", true, false),
            ("modern-dap-lossless", false, false),
            ("sony-walkman-aac-flac", false, false),
            ("spoken-word-mp3-96", false, false),
            ("spoken-word-mp3-96", false, true),
        ] {
            add.max_bitrate_override_kbps = local_encoding.then_some(512);
            let profile = presets["profiles"]
                .as_array()
                .unwrap()
                .iter()
                .find(|p| p["id"] == profile_id)
                .unwrap()["deviceProfile"]
                .clone();
            let profile = (!profile.is_null()).then_some(profile);
            let blocked =
                blocked_provider_media_add(&add, provider.as_ref(), profile.as_ref(), None).await;
            assert_eq!(
                blocked.is_none(),
                allowed,
                "preview {role:?}/{profile_id}: {blocked:?}"
            );
            let root = tempfile::tempdir().unwrap();
            let manifest = crate::device::DeviceManifest {
                device_id: "m4b-device".into(),
                managed_paths: vec!["Music".into()],
                audiobook_path: Some("Audiobooks".into()),
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
            operations.create_operation("m4b-copy".into(), 1).await;
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
            let (written, errors) = crate::sync::execute_provider_sync(
                &delta,
                &target,
                crate::sync::ProviderSyncSource {
                    provider: provider.clone(),
                    transcoding_profile: profile,
                    providers_by_server: Default::default(),
                    local_encoding_servers: if local_encoding {
                        std::collections::HashSet::from([String::new()])
                    } else {
                        Default::default()
                    },
                },
                operations,
                "m4b-copy".into(),
                devices.clone(),
            )
            .await
            .unwrap();
            assert!(errors.is_empty(), "{errors:?}");
            assert_eq!(
                written.len(),
                usize::from(allowed || local_encoding),
                "transfer {role:?}/{profile_id}"
            );
            if allowed || local_encoding {
                let item = &written[0];
                assert!(
                    item.local_path
                        .ends_with(if local_encoding { ".mp3" } else { ".m4b" }),
                    "{}",
                    item.local_path
                );
                assert!(item.local_path.starts_with(if book {
                    "Audiobooks/"
                } else {
                    "Podcasts/"
                }));
                let path = root.path().join(&item.local_path);
                if local_encoding {
                    let context = ffmpeg_next::format::input(&path).unwrap();
                    let stream = context
                        .streams()
                        .best(ffmpeg_next::media::Type::Audio)
                        .unwrap();
                    assert_eq!(stream.parameters().id(), ffmpeg_next::codec::Id::MP3);
                    let decoder =
                        ffmpeg_next::codec::context::Context::from_parameters(stream.parameters())
                            .unwrap()
                            .decoder()
                            .audio()
                            .unwrap();
                    assert!(decoder.bit_rate() <= 100800);
                    assert!(decoder.channels() <= 2);
                    assert!(decoder.rate() <= 44100);
                } else if book {
                    let source = root.path().join("source.m4b");
                    std::fs::write(&source, audio).unwrap();
                    let before = mp4ameta::Tag::read_from_path(&source).unwrap();
                    let after = mp4ameta::Tag::read_from_path(&path).unwrap();
                    assert_eq!(after.album(), Some("Current book"));
                    assert_eq!(before.chapter_list(), after.chapter_list());
                    assert_eq!(before.chapter_track(), after.chapter_track());
                    let packets = |path: &std::path::Path| {
                        let mut context = ffmpeg_next::format::input(path).unwrap();
                        let index = context
                            .streams()
                            .best(ffmpeg_next::media::Type::Audio)
                            .unwrap()
                            .index();
                        context
                            .packets()
                            .filter(|(s, _)| s.index() == index)
                            .map(|(_, p)| p.data().unwrap().to_vec())
                            .collect::<Vec<_>>()
                    };
                    assert_eq!(packets(&source), packets(&path));
                } else {
                    assert_eq!(std::fs::read(&path).unwrap(), audio);
                }
                assert_eq!(item.size_bytes, std::fs::metadata(&path).unwrap().len());
            }
            assert_eq!(
                devices.get_current_device().await.unwrap().synced_items,
                written
            );
            let durable: crate::device::DeviceManifest =
                crate::device::manifest_codec::decode_test_manifest(
                    &std::fs::read(root.path().join(".hifimule.json.zst")).unwrap(),
                )
                .unwrap();
            assert_eq!(durable.synced_items, written);
        }
        _play.remove_async().await;
        _probe.remove_async().await;
        for (mime, codec, expected) in [
            ("audio/mpeg", "mp3", Some("mp3")),
            ("audio/mp4", "alac", None),
        ] {
            let play = server.mock("POST", if book {"/api/items/i/play"} else {"/api/items/i/play/e"})
                .with_status(200).with_header("content-type", "application/json")
                .with_body(serde_json::json!({"id":"session","serverVersion":"2.37.1","libraryId":"l","libraryItemId":"i",
                    "mediaType":if book {"book"} else {"podcast"},"episodeId":"e","playMethod":0,
                    "audioTracks":[{"ino":"f","contentUrl":"/api/items/i/file/f","mimeType":mime,"codec":codec}]
                }).to_string()).create_async().await;
            let probe = server
                .mock("GET", "/api/items/i/file/f")
                .match_header("range", "bytes=0-0")
                .with_status(206)
                .with_header("content-type", mime)
                .with_header("accept-ranges", "bytes")
                .with_header("content-range", "bytes 0-0/100")
                .with_body("x")
                .create_async()
                .await;
            let media = provider.resolve_sync_media(id).await;
            if let Some(container) = expected {
                assert_eq!(
                    media.unwrap().container,
                    container,
                    "a misleading M4B extension must not relabel MP3"
                );
            } else {
                assert!(media.is_err(), "M4B extension must not admit ALAC");
            }
            play.remove_async().await;
            probe.remove_async().await;
        }
        progress.assert_async().await;
    }
}
