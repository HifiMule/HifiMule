use super::*;
use crate::playback::{model::*, native::start_ingress};
use crate::providers::*;
use async_trait::async_trait;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Notify;

#[derive(Default)]
struct SlowProvider {
    entered: Notify,
    release: Notify,
    finished: Arc<Notify>,
    calls: AtomicUsize,
}
struct Finished(Arc<Notify>);
impl Drop for Finished {
    fn drop(&mut self) {
        self.0.notify_one();
    }
}

struct Fixture {
    session: PlaybackSession,
    service: PlaybackCommandService,
    provider: Arc<SlowProvider>,
}
impl Fixture {
    fn new() -> Self {
        let db = Arc::new(Database::memory().unwrap());
        let local_id = db
            .upsert_server(
                "http://fixture.invalid",
                "jellyfin",
                "user",
                None,
                None,
                None,
                None,
            )
            .unwrap();
        let mut manager = ServerManager::new();
        manager.load_from_db(&db);
        let portable_id = manager.servers[0].server_id.clone().unwrap();
        let provider = Arc::new(SlowProvider::default());
        manager.providers.insert(local_id, provider.clone());
        let session = PlaybackSession::restore(db.clone(), "command-test".into());
        let initial = session.snapshot().unwrap();
        session
            .apply(ApplySessionParams {
                schema_version: 1,
                instance_id: initial.instance_id,
                session_id: initial.session_id,
                command_id: uuid::Uuid::new_v4().to_string(),
                expected_queue_revision: initial.queue_revision,
                operation: SessionOperation::ReplaceQueue {
                    sources: vec![TrackSource {
                        server_id: portable_id,
                        track_id: "track".into(),
                    }],
                },
            })
            .unwrap();
        let service = PlaybackCommandService::new(
            session.clone(),
            Arc::new(tokio::sync::RwLock::new(manager)),
            db,
            Arc::new(SyncOperationManager::new()),
        );
        Self {
            session,
            service,
            provider,
        }
    }
    fn rpc_params(&self, action: ControlAction) -> ControlParams {
        let s = self.session.snapshot().unwrap();
        ControlParams {
            schema_version: 1,
            instance_id: s.instance_id,
            session_id: s.session_id,
            command_id: uuid::Uuid::new_v4().to_string(),
            expected_generation_id: s.generation_id,
            occurrence_id: s.current.unwrap().occurrence_id,
            action,
        }
    }

    fn qualify_seek(&self) -> SessionSnapshot {
        let snapshot = self.session.snapshot().unwrap();
        self.session.publish_event(
            snapshot.generation_id,
            PlaybackEvent::Resolved {
                metadata: PlaybackTrackMetadata {
                    source: snapshot.current.unwrap().source,
                    title: "fixture".into(),
                    artist: None,
                    album: None,
                },
                duration_ms: Some(10_000),
                representation: "wav".into(),
                seek: SeekCapability::jellyfin_pcm_wav(),
            },
        );
        self.session.snapshot().unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.session.stop_and_join().unwrap();
    }
}
async fn notified(notify: &Notify) {
    tokio::time::timeout(std::time::Duration::from_secs(3), notify.notified())
        .await
        .expect("production task did not reach barrier");
}
async fn status(session: &PlaybackSession, expected: PlaybackStatus) -> SessionSnapshot {
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let snapshot = session.snapshot().unwrap();
            if snapshot.playback.status == expected {
                return snapshot;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("authoritative state did not converge")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_pause_cancels_pending_provider_preparation_without_changing_generation() {
    let f = Fixture::new();
    let ingress = start_ingress(f.service.clone());
    ingress.try_send(NativeControlIntent::Play).unwrap();
    notified(&f.provider.entered).await;
    let loading = status(&f.session, PlaybackStatus::Loading).await;
    ingress.try_send(NativeControlIntent::Pause).unwrap();
    let paused = status(&f.session, PlaybackStatus::Paused).await;
    assert_eq!(loading.generation_id, paused.generation_id);
    assert!(!f.session.output_gate().load(Ordering::Acquire));
    notified(&f.provider.finished).await;
    assert!(f.session.snapshot().unwrap().playback.error.is_none());
    // A later explicit Resume uses the same generation but must start new preparation.
    ingress.try_send(NativeControlIntent::Play).unwrap();
    notified(&f.provider.entered).await;
    assert_eq!(f.provider.calls.load(Ordering::SeqCst), 2);
    ingress.try_send(NativeControlIntent::Stop).unwrap();
    status(&f.session, PlaybackStatus::Stopped).await;
    notified(&f.provider.finished).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_stop_cancels_pending_provider_preparation_and_retires_generation() {
    let f = Fixture::new();
    let ingress = start_ingress(f.service.clone());
    ingress.try_send(NativeControlIntent::Play).unwrap();
    notified(&f.provider.entered).await;
    let loading = status(&f.session, PlaybackStatus::Loading).await;
    ingress.try_send(NativeControlIntent::Stop).unwrap();
    let stopped = status(&f.session, PlaybackStatus::Stopped).await;
    notified(&f.provider.finished).await;
    assert_ne!(loading.generation_id, stopped.generation_id);
    assert_eq!(stopped.position_ms, 0);
    assert!(f.session.generation_guard(&loading.generation_id).is_none());
    assert!(!f.session.output_gate().load(Ordering::Acquire));
    assert!(f.session.snapshot().unwrap().playback.error.is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rpc_and_native_resume_dispatch_the_same_provider_effect_and_failure() {
    for native in [false, true] {
        let f = Fixture::new();
        let ingress = start_ingress(f.service.clone());
        if native {
            ingress.try_send(NativeControlIntent::Play).unwrap();
        } else {
            f.service
                .rpc_control(f.rpc_params(ControlAction::Resume), None)
                .await
                .unwrap();
        }
        notified(&f.provider.entered).await;
        status(&f.session, PlaybackStatus::Loading).await;
        f.provider.release.notify_one();
        let failed = status(&f.session, PlaybackStatus::Error).await;
        assert_eq!(failed.playback.error.unwrap().code, "RESUME_UNAVAILABLE");
        assert_eq!(failed.state, TransportState::Paused);
        assert!(!f.session.output_gate().load(Ordering::Acquire));
        assert_eq!(f.provider.calls.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rpc_and_native_seek_share_owner_admission_provider_effect_and_failure() {
    for native in [false, true] {
        let f = Fixture::new();
        let qualified = f.qualify_seek();
        let ingress = native.then(|| start_ingress(f.service.clone()));
        if native {
            ingress
                .as_ref()
                .unwrap()
                .try_send(NativeControlIntent::SeekAbsolute(4_000))
                .unwrap();
        } else {
            f.service
                .rpc_seek(
                    SeekParams {
                        schema_version: 1,
                        instance_id: qualified.instance_id,
                        session_id: qualified.session_id,
                        command_id: uuid::Uuid::new_v4().to_string(),
                        expected_generation_id: qualified.generation_id,
                        occurrence_id: qualified.current.unwrap().occurrence_id,
                        position_ms: 4_000,
                    },
                    None,
                )
                .await
                .unwrap();
        }
        notified(&f.provider.entered).await;
        let pending = f.session.snapshot().unwrap();
        assert_eq!(pending.position_ms, 0);
        assert_eq!(
            pending.playback.pending_seek.unwrap().requested_position_ms,
            4_000
        );
        f.provider.release.notify_one();
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                let snapshot = f.session.snapshot().unwrap();
                if snapshot
                    .playback
                    .seek_outcome
                    .as_ref()
                    .is_some_and(|outcome| outcome.status == "failed")
                {
                    assert_eq!(snapshot.position_ms, 0);
                    assert!(snapshot.playback.pending_seek.is_none());
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("seek failure did not reach authoritative state");
        assert_eq!(f.provider.calls.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_cancels_native_preparation_and_rejects_later_play() {
    let f = Fixture::new();
    let ingress = start_ingress(f.service.clone());
    ingress.try_send(NativeControlIntent::Play).unwrap();
    notified(&f.provider.entered).await;
    f.session
        .begin_shutdown_checkpoint()
        .unwrap()
        .recv()
        .unwrap()
        .unwrap();
    notified(&f.provider.finished).await;
    assert!(!f.session.output_gate().load(Ordering::Acquire));
    let error = f
        .service
        .native_control(NativeControlIntent::Play)
        .await
        .unwrap_err();
    assert_eq!(error.code, "DAEMON_STOPPED");
    assert_eq!(f.provider.calls.load(Ordering::SeqCst), 1);
}

#[async_trait]
impl MediaProvider for SlowProvider {
    fn server_type(&self) -> ServerType {
        ServerType::Unknown
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            open_subsonic: false,
            supports_changes_since: false,
            supports_server_transcoding: false,
            supports_playlist_write: false,
            browse: BrowseCapabilities { list_modes: vec![] },
        }
    }

    async fn list_libraries(&self) -> Result<Vec<crate::domain::models::Library>, ProviderError> {
        Err(ProviderError::UnsupportedCapability(
            "list_libraries".to_string(),
        ))
    }

    async fn list_artists(
        &self,
        _library_id: Option<&str>,
        _letter: Option<&str>,
        _offset: u32,
        _limit: u32,
    ) -> Result<(Vec<crate::domain::models::Artist>, u32), ProviderError> {
        Err(ProviderError::UnsupportedCapability(
            "list_artists".to_string(),
        ))
    }

    async fn get_artist(
        &self,
        _artist_id: &str,
    ) -> Result<crate::domain::models::ArtistWithAlbums, ProviderError> {
        Err(ProviderError::UnsupportedCapability(
            "get_artist".to_string(),
        ))
    }

    async fn list_albums(
        &self,
        _library_id: Option<&str>,
        _letter: Option<&str>,
        _offset: u32,
        _limit: u32,
    ) -> Result<(Vec<crate::domain::models::Album>, u32), ProviderError> {
        Err(ProviderError::UnsupportedCapability(
            "list_albums".to_string(),
        ))
    }

    async fn get_album(
        &self,
        _album_id: &str,
    ) -> Result<crate::domain::models::AlbumWithTracks, ProviderError> {
        Err(ProviderError::UnsupportedCapability(
            "get_album".to_string(),
        ))
    }

    async fn list_playlists(&self) -> Result<Vec<crate::domain::models::Playlist>, ProviderError> {
        Err(ProviderError::UnsupportedCapability(
            "list_playlists".to_string(),
        ))
    }

    async fn get_playlist(
        &self,
        _playlist_id: &str,
    ) -> Result<crate::domain::models::PlaylistWithTracks, ProviderError> {
        Err(ProviderError::UnsupportedCapability(
            "get_playlist".to_string(),
        ))
    }

    async fn search(
        &self,
        _query: &str,
    ) -> Result<crate::domain::models::SearchResult, ProviderError> {
        Err(ProviderError::UnsupportedCapability("search".to_string()))
    }

    async fn download_url(
        &self,
        _song_id: &str,
        _profile: Option<&TranscodeProfile>,
    ) -> Result<String, ProviderError> {
        Err(ProviderError::UnsupportedCapability(
            "download_url".to_string(),
        ))
    }

    async fn cover_art_url(&self, _cover_art_id: &str) -> Result<String, ProviderError> {
        Err(ProviderError::UnsupportedCapability(
            "cover_art_url".to_string(),
        ))
    }

    async fn changes_since_with_context(
        &self,
        _token: Option<&str>,
        _context: &ProviderChangeContext,
    ) -> Result<Vec<crate::domain::models::ChangeEvent>, ProviderError> {
        Err(ProviderError::UnsupportedCapability(
            "changes_since_with_context".to_string(),
        ))
    }

    async fn scrobble(&self, _request: ScrobbleRequest) -> Result<(), ProviderError> {
        Err(ProviderError::UnsupportedCapability("scrobble".to_string()))
    }
    async fn resolve_playback(&self, song: &str) -> Result<PlaybackDescription, ProviderError> {
        assert_eq!(song, "track");
        self.calls.fetch_add(1, Ordering::SeqCst);
        let _finished = Finished(self.finished.clone());
        self.entered.notify_one();
        self.release.notified().await;
        Err(ProviderError::Auth("fixture source unavailable".into()))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dispatch_of_admitted_resume_after_pause_does_not_restart_preparation() {
    let f = Fixture::new();
    let admitted = f
        .session
        .native_control(NativeControlIntent::Play, None)
        .unwrap();
    f.session
        .native_control(NativeControlIntent::Pause, None)
        .unwrap();
    f.service.dispatch_effect(&admitted);
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(100),
            f.provider.entered.notified()
        )
        .await
        .is_err()
    );
    assert_eq!(f.provider.calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        f.session.snapshot().unwrap().playback.status,
        PlaybackStatus::Paused
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unavailable_output_rejects_rpc_and_native_before_provider_dispatch() {
    let f = Fixture::new();
    let directory = tempfile::tempdir().unwrap();
    f.session
        .enable_outputs_with(directory.path().join("playback.json"), || {
            crate::playback::devices::Discovery {
                outputs: vec![],
                error: None,
            }
        });
    let rpc = f
        .service
        .rpc_control(f.rpc_params(ControlAction::Resume), None)
        .await
        .unwrap_err();
    let ingress = start_ingress(f.service.clone());
    let native = ingress
        .try_send_tracked(NativeControlIntent::Play)
        .unwrap()
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(rpc.code, native);
    assert!(rpc.code.starts_with("OUTPUT_"));
    assert_eq!(f.provider.calls.load(Ordering::SeqCst), 0);
    assert!(!f.session.output_gate().load(Ordering::Acquire));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rpc_and_native_duplicate_resume_share_one_pending_provider_effect() {
    for native_first in [false, true] {
        let f = Fixture::new();
        let ingress = start_ingress(f.service.clone());
        let params = f.rpc_params(ControlAction::Resume);
        if native_first {
            ingress
                .try_send_tracked(NativeControlIntent::Play)
                .unwrap()
                .await
                .unwrap()
                .unwrap();
        } else {
            assert!(
                f.service
                    .rpc_control(params.clone(), None)
                    .await
                    .unwrap()
                    .resume_audio
            );
        }
        notified(&f.provider.entered).await;
        let pending = status(&f.session, PlaybackStatus::Loading).await;
        // Re-delivery and a second transport source must not resolve the source again.
        assert!(
            !f.service
                .rpc_control(params.clone(), None)
                .await
                .unwrap()
                .resume_audio
        );
        assert!(
            !f.service
                .rpc_control(params, None)
                .await
                .unwrap()
                .resume_audio
        );
        ingress
            .try_send_tracked(NativeControlIntent::Play)
            .unwrap()
            .await
            .unwrap()
            .unwrap();
        ingress
            .try_send_tracked(NativeControlIntent::Play)
            .unwrap()
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            f.session.snapshot().unwrap().generation_id,
            pending.generation_id
        );
        f.provider.release.notify_one();
        status(&f.session, PlaybackStatus::Error).await;
        assert_eq!(f.provider.calls.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stopped_and_completed_sessions_resume_through_rpc_and_native_from_zero() {
    for completed in [false, true] {
        for native in [false, true] {
            let f = Fixture::new();
            let ingress = start_ingress(f.service.clone());
            if completed {
                let generation = f.session.snapshot().unwrap().generation_id;
                f.session.publish_event(
                    generation,
                    PlaybackEvent::Completed {
                        position_ms: 42_000,
                    },
                );
                status(&f.session, PlaybackStatus::Completed).await;
            } else {
                ingress
                    .try_send_tracked(NativeControlIntent::Stop)
                    .unwrap()
                    .await
                    .unwrap()
                    .unwrap();
                status(&f.session, PlaybackStatus::Stopped).await;
            }
            let before = f.session.snapshot().unwrap();
            if native {
                ingress
                    .try_send_tracked(NativeControlIntent::Play)
                    .unwrap()
                    .await
                    .unwrap()
                    .unwrap();
            } else {
                f.service
                    .rpc_control(f.rpc_params(ControlAction::Resume), None)
                    .await
                    .unwrap();
            }
            notified(&f.provider.entered).await;
            let resumed = status(&f.session, PlaybackStatus::Loading).await;
            assert_eq!(resumed.position_ms, 0);
            assert_eq!(resumed.current, before.current);
            assert_eq!(
                resumed.total_occurrence_count,
                before.total_occurrence_count
            );
            if completed {
                assert_ne!(resumed.generation_id, before.generation_id);
            } else {
                assert_eq!(resumed.generation_id, before.generation_id);
            }
            assert_eq!(f.provider.calls.load(Ordering::SeqCst), 1);
            ingress
                .try_send_tracked(NativeControlIntent::Stop)
                .unwrap()
                .await
                .unwrap()
                .unwrap();
            notified(&f.provider.finished).await;
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retry_reopens_the_same_occurrence_at_its_committed_cursor_and_reports_repeat_failure() {
    let f = Fixture::new();
    let loading = f
        .session
        .control_with_guard(f.rpc_params(ControlAction::Resume), None)
        .unwrap();
    f.session
        .publish_event(loading.generation_id, PlaybackEvent::Active);
    let before = f.session.snapshot().unwrap();
    let occurrence_id = before.current.as_ref().unwrap().occurrence_id.clone();
    f.session
        .report_progress(&before.generation_id, &occurrence_id, 1, 2_345)
        .unwrap();
    f.session.final_checkpoint().unwrap();
    assert_eq!(f.session.snapshot().unwrap().position_ms, 2_345);
    f.session.publish_event(
        before.generation_id,
        PlaybackEvent::Failed {
            code: "SOURCE_UNAVAILABLE".into(),
            retryable: true,
        },
    );
    let failed = status(&f.session, PlaybackStatus::Error).await;
    assert_eq!(failed.position_ms, 2_345);

    let admitted = f
        .service
        .rpc_control(f.rpc_params(ControlAction::Retry), None)
        .await
        .unwrap();
    assert_eq!(admitted.current.unwrap().occurrence_id, occurrence_id);
    assert_eq!(admitted.position_ms, 2_345);
    assert_ne!(admitted.generation_id, failed.generation_id);
    assert_eq!(admitted.playback.status, PlaybackStatus::Loading);

    notified(&f.provider.entered).await;
    assert_eq!(f.provider.calls.load(Ordering::SeqCst), 1);
    f.provider.release.notify_one();
    let repeated = status(&f.session, PlaybackStatus::Error).await;
    assert_eq!(repeated.current.unwrap().occurrence_id, occurrence_id);
    assert_eq!(repeated.position_ms, 2_345);
    assert_eq!(repeated.playback.error.unwrap().code, "RESUME_UNAVAILABLE");
}
