use super::*;
use crate::domain::models::*;
use crate::playback::model::{
    ApplySessionParams, ControlAction, ControlParams, SessionOperation, TrackSource,
};
use crate::providers::{Capabilities, MediaProvider, ProviderError, ServerType};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Notify;

struct AlbumProvider {
    calls: AtomicUsize,
    entered: Notify,
    release: Notify,
}

#[async_trait::async_trait]
impl MediaProvider for AlbumProvider {
    async fn get_album(&self, id: &str) -> Result<AlbumWithTracks, ProviderError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.entered.notify_one();
        self.release.notified().await;
        if id == "book" {
            let tracks = (1..=3)
                .map(|number| {
                    serde_json::from_value(json!({
                        "id": format!("book-part-{number}"), "title": format!("Part {number}"),
                        "duration": 1, "albumId": id, "trackNumber": number,
                        "suffix": "mp3", "contentType": "audio/mpeg"
                    }))
                    .unwrap()
                })
                .collect();
            return Ok(AlbumWithTracks {
                album: serde_json::from_value(json!({"id": id, "name": "Book", "trackCount": 3}))
                    .unwrap(),
                tracks,
                provider_metadata: Default::default(),
            });
        }
        let mut track: Song = serde_json::from_value(json!({
            "id": format!("{id}-track"), "title": "Track", "duration": 1,
            "albumId": id, "suffix": "flac", "contentType": "audio/flac"
        }))
        .unwrap();
        track.album_loudness = AlbumLoudnessEvidence::open_subsonic(-6.0, 0.5);
        Ok(AlbumWithTracks {
            album: serde_json::from_value(json!({"id": id, "name": "Album", "trackCount": 1}))
                .unwrap(),
            tracks: vec![track],
            provider_metadata: Default::default(),
        })
    }
    async fn list_libraries(&self) -> Result<Vec<Library>, ProviderError> {
        unreachable!()
    }
    async fn list_artists(
        &self,
        _: Option<&str>,
        _: Option<&str>,
        _: u32,
        _: u32,
    ) -> Result<(Vec<Artist>, u32), ProviderError> {
        unreachable!()
    }
    async fn get_artist(&self, _: &str) -> Result<ArtistWithAlbums, ProviderError> {
        unreachable!()
    }
    async fn list_albums(
        &self,
        _: Option<&str>,
        _: Option<&str>,
        _: u32,
        _: u32,
    ) -> Result<(Vec<Album>, u32), ProviderError> {
        unreachable!()
    }
    async fn list_playlists(&self) -> Result<Vec<Playlist>, ProviderError> {
        unreachable!()
    }
    async fn get_playlist(&self, _: &str) -> Result<PlaylistWithTracks, ProviderError> {
        unreachable!()
    }
    async fn search(&self, _: &str) -> Result<SearchResult, ProviderError> {
        unreachable!()
    }
    async fn download_url(
        &self,
        _: &str,
        _: Option<&crate::providers::TranscodeProfile>,
    ) -> Result<String, ProviderError> {
        unreachable!()
    }
    async fn cover_art_url(&self, _: &str) -> Result<String, ProviderError> {
        unreachable!()
    }
    async fn changes_since_with_context(
        &self,
        _: Option<&str>,
        _: &crate::providers::ProviderChangeContext,
    ) -> Result<Vec<ChangeEvent>, ProviderError> {
        unreachable!()
    }
    async fn scrobble(&self, _: crate::providers::ScrobbleRequest) -> Result<(), ProviderError> {
        unreachable!()
    }
    fn server_type(&self) -> ServerType {
        ServerType::OpenSubsonic
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            open_subsonic: true,
            supports_changes_since: false,
            supports_server_transcoding: false,
            supports_playlist_write: false,
            browse: crate::providers::BrowseCapabilities { list_modes: vec![] },
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn book_album_rpc_admits_each_part_in_order_with_portable_source() {
    let f = Fixture::new().await;
    let mut request = f.request();
    request["source"]["albumId"] = "book".into();
    f.provider.release.notify_one();
    handle_playback_play_album(&f.state, Some(request), None)
        .await
        .unwrap();
    let snapshot = f.state.playback.snapshot().unwrap();
    assert_eq!(snapshot.occurrences.len(), 3);
    for (index, occurrence) in snapshot.occurrences.iter().enumerate() {
        assert_eq!(occurrence.source.server_id, f.server_id);
        assert_eq!(
            occurrence.source.track_id,
            format!("book-part-{}", index + 1)
        );
    }
}

struct Fixture {
    state: Arc<AppState>,
    provider: Arc<AlbumProvider>,
    server_id: String,
    _directory: tempfile::TempDir,
}

impl Fixture {
    async fn new() -> Self {
        let mut state =
            super::tests::make_test_state(Arc::new(crate::db::Database::memory().unwrap()));
        state.playback.stop_and_join().unwrap();
        Arc::get_mut(&mut state).unwrap().playback = crate::playback::PlaybackSession::restore(
            state.db.clone(),
            uuid::Uuid::new_v4().to_string(),
        );
        let directory = tempfile::tempdir().unwrap();
        // No selected or default output: a successful album commit cannot start
        // platform audio or launch a provider preparation task in these tests.
        state
            .playback
            .enable_outputs_with(directory.path().join("playback.json"), || {
                crate::playback::devices::Discovery {
                    outputs: vec![],
                    error: None,
                }
            });
        let provider = Arc::new(AlbumProvider {
            calls: AtomicUsize::new(0),
            entered: Notify::new(),
            release: Notify::new(),
        });
        let server_id = {
            let mut manager = state.server_manager.write().await;
            manager.set_test_provider(provider.clone());
            manager.servers[0].server_id.clone().unwrap()
        };
        let snapshot = state.playback.snapshot().unwrap();
        state
            .playback
            .apply(ApplySessionParams {
                schema_version: 1,
                instance_id: snapshot.instance_id,
                session_id: snapshot.session_id,
                command_id: uuid::Uuid::new_v4().to_string(),
                expected_queue_revision: snapshot.queue_revision,
                operation: SessionOperation::ReplaceQueue {
                    sources: vec![TrackSource {
                        server_id: server_id.clone(),
                        track_id: "original".into(),
                    }],
                },
            })
            .unwrap();
        Self {
            state,
            provider,
            server_id,
            _directory: directory,
        }
    }

    fn request(&self) -> Value {
        let s = self.state.playback.snapshot().unwrap();
        json!({"schemaVersion":1,"instanceId":s.instance_id,"sessionId":s.session_id,
            "commandId":uuid::Uuid::new_v4().to_string(),"expectedQueueRevision":s.queue_revision,
            "expectedGenerationId":s.generation_id,"source":{"serverId":self.server_id,"albumId":"album"}})
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.state.playback.stop_and_join().unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn album_rpc_pause_during_provider_resolution_preserves_old_queue() {
    let f = Fixture::new().await;
    let request = f.request();
    let state = f.state.clone();
    let pending =
        tokio::spawn(async move { handle_playback_play_album(&state, Some(request), None).await });
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        f.provider.entered.notified(),
    )
    .await
    .unwrap();
    let before = f.state.playback.snapshot().unwrap();
    f.state
        .playback
        .control_with_guard(
            ControlParams {
                schema_version: 1,
                instance_id: before.instance_id.clone(),
                session_id: before.session_id.clone(),
                command_id: uuid::Uuid::new_v4().to_string(),
                expected_generation_id: before.generation_id.clone(),
                occurrence_id: before.current.as_ref().unwrap().occurrence_id.clone(),
                action: ControlAction::Pause,
            },
            None,
        )
        .unwrap();
    f.provider.release.notify_one();
    let error = tokio::time::timeout(std::time::Duration::from_secs(2), pending)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert_eq!(error.data.unwrap()["code"], "GENERATION_CONFLICT");
    let after = f.state.playback.snapshot().unwrap();
    assert_eq!(after.queue_revision, before.queue_revision);
    assert_eq!(after.current, before.current);
    assert_eq!(after.state, crate::playback::model::TransportState::Paused);
    assert_eq!(f.provider.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn album_rpc_successful_receipt_replay_does_not_resolve_again() {
    let f = Fixture::new().await;
    let request = f.request();
    f.provider.release.notify_one();
    let first = handle_playback_play_album(&f.state, Some(request.clone()), None)
        .await
        .unwrap();
    let replay = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        handle_playback_play_album(&f.state, Some(request), None),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(replay, first);
    assert_eq!(f.provider.calls.load(Ordering::SeqCst), 1);
    let snapshot = f.state.playback.snapshot().unwrap();
    let expected = 10f32.powf(-6.0 / 20.0);
    assert!((f32::from_bits(snapshot.gain_bits) - expected).abs() <= 1e-7);
    let persisted = f.state.db.load_playback_session().unwrap().unwrap();
    assert_eq!(
        persisted.album_context.unwrap().policy.scalar_bits,
        snapshot.gain_bits
    );
    assert_eq!(
        snapshot.queue_revision,
        first["data"]["queueRevision"].as_str().unwrap()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn album_rpc_latest_request_cancels_and_fences_prior_resolution() {
    for replacement_album in ["album", "different-album"] {
        let f = Fixture::new().await;
        let first_request = f.request();
        let state = f.state.clone();
        let first = tokio::spawn(async move {
            handle_playback_play_album(&state, Some(first_request), None).await
        });
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            f.provider.entered.notified(),
        )
        .await
        .unwrap();

        let mut replacement_request = f.request();
        replacement_request["source"]["albumId"] = replacement_album.into();
        let state = f.state.clone();
        let replacement = tokio::spawn(async move {
            handle_playback_play_album(&state, Some(replacement_request), None).await
        });
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            f.provider.entered.notified(),
        )
        .await
        .unwrap();

        let error = tokio::time::timeout(std::time::Duration::from_secs(2), first)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        assert_eq!(error.data.unwrap()["code"], "ALBUM_SUPERSEDED");

        f.provider.release.notify_one();
        tokio::time::timeout(std::time::Duration::from_secs(2), replacement)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let snapshot = f.state.playback.snapshot().unwrap();
        assert_eq!(
            snapshot.current.unwrap().source.track_id,
            format!("{replacement_album}-track")
        );
        assert_eq!(f.provider.calls.load(Ordering::SeqCst), 2);
    }
}

#[tokio::test]
async fn album_rpc_rejects_raw_resolved_sources_on_apply_session() {
    let f = Fixture::new().await;
    let before = f.state.playback.snapshot().unwrap();
    let error = handle_playback_apply_session(&f.state, Some(json!({
        "schemaVersion":1,"instanceId":before.instance_id,"sessionId":before.session_id,
        "commandId":uuid::Uuid::new_v4().to_string(),"expectedQueueRevision":before.queue_revision,
        "operation":{"type":"playAlbum","sources":[{"serverId":f.server_id,"trackId":"bypass"}]}
    })), None).await.unwrap_err();
    assert_eq!(error.code, ERR_INVALID_PARAMS);
    let after = f.state.playback.snapshot().unwrap();
    assert_eq!(after.queue_revision, before.queue_revision);
    assert_eq!(after.current, before.current);
    assert_eq!(f.provider.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn album_rpc_resolution_deadline_includes_provider_manager_acquisition() {
    let f = Fixture::new().await;
    let request: crate::playback::model::PlayAlbumParams =
        serde_json::from_value(f.request()).unwrap();
    let admission = f
        .state
        .playback
        .reserve_album(request.clone(), None)
        .unwrap();
    let crate::playback::session::AlbumAdmission::Resolve(mut reservation) = admission else {
        panic!("new request must reserve provider resolution");
    };
    reservation.deadline = std::time::Instant::now();
    let _held_manager = f.state.server_manager.write().await;
    let error = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        resolve_playback_album(&f.state, &request.source, &reservation),
    )
    .await
    .unwrap()
    .unwrap_err();
    assert_eq!(error.data.unwrap()["code"], "PLAYBACK_TIMEOUT");
    assert_eq!(f.provider.calls.load(Ordering::SeqCst), 0);
}
