use super::model::{PlaybackStatus, SessionSnapshot, TransportState};
use super::{NativeControlIntent, commands::PlaybackCommandService};
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicU64, Ordering},
};

pub const NATIVE_INGRESS_CAPACITY: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NativeCommandMask {
    pub play: bool,
    pub pause: bool,
    pub toggle: bool,
    pub stop: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativePlaybackView {
    pub instance_id: String,
    pub session_id: String,
    pub generation_id: String,
    pub state_sequence: String,
    pub status: PlaybackStatus,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_ms: Option<u64>,
    pub position_ms: u64,
    pub commands: NativeCommandMask,
    pub failure_code: Option<String>,
}

impl Default for NativePlaybackView {
    fn default() -> Self {
        Self {
            instance_id: String::new(),
            session_id: String::new(),
            generation_id: String::new(),
            state_sequence: "0".into(),
            status: PlaybackStatus::Idle,
            title: None,
            artist: None,
            album: None,
            duration_ms: None,
            position_ms: 0,
            commands: NativeCommandMask::default(),
            failure_code: None,
        }
    }
}

impl NativePlaybackView {
    pub fn from_snapshot(snapshot: &SessionSnapshot, stopping: bool) -> Self {
        let current = snapshot.current.is_some();
        let output_usable = snapshot.output.status == "available"
            && snapshot
                .output
                .selected
                .as_ref()
                .is_some_and(|output| output.available)
            && snapshot.output.error.is_none()
            && snapshot.output.pending.is_none();
        let source_usable = snapshot.current.as_ref().is_some_and(|current| {
            !matches!(
                current.availability,
                super::model::SourceAvailability::NotConfigured
            )
        });
        let resumable = !stopping
            && current
            && source_usable
            && output_usable
            && snapshot.restoration.status == "ok";
        let active = matches!(
            snapshot.state,
            TransportState::Playing | TransportState::Buffering
        ) || matches!(
            snapshot.playback.status,
            PlaybackStatus::Active | PlaybackStatus::Loading
        );
        let metadata = snapshot.playback.metadata.as_ref().filter(|metadata| {
            snapshot
                .current
                .as_ref()
                .is_some_and(|current| current.source == metadata.source)
        });
        let status = if active && !output_usable {
            PlaybackStatus::Paused
        } else {
            snapshot.playback.status
        };
        Self {
            instance_id: snapshot.instance_id.clone(),
            session_id: snapshot.session_id.clone(),
            generation_id: snapshot.generation_id.clone(),
            state_sequence: snapshot.state_sequence.clone(),
            status,
            title: metadata.map(|value| value.title.clone()),
            artist: metadata.and_then(|value| value.artist.clone()),
            album: metadata.and_then(|value| value.album.clone()),
            duration_ms: metadata.and(snapshot.playback.duration_ms),
            position_ms: snapshot.position_ms,
            commands: NativeCommandMask {
                play: resumable && !active,
                pause: !stopping && current && active,
                toggle: !stopping && (resumable || active),
                stop: !stopping && current,
            },
            failure_code: snapshot
                .playback
                .error
                .as_ref()
                .map(|value| value.code.clone()),
        }
    }
}

#[derive(Clone)]
pub struct NativeIngress {
    tx: tokio::sync::mpsc::Sender<NativeControlIntent>,
    latest: Arc<Mutex<NativePlaybackView>>,
}

impl NativeIngress {
    pub fn try_send(&self, intent: NativeControlIntent) -> Result<(), NativeIngressError> {
        self.tx.try_send(intent).map_err(|error| match error {
            tokio::sync::mpsc::error::TrySendError::Full(_) => NativeIngressError::Full,
            tokio::sync::mpsc::error::TrySendError::Closed(_) => NativeIngressError::Stopped,
        })
    }

    pub fn latest(&self) -> NativePlaybackView {
        self.latest
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeIngressError {
    Full,
    Stopped,
}

#[derive(Clone, Default)]
pub struct NativeBridge {
    ingress: Arc<OnceLock<NativeIngress>>,
}

impl NativeBridge {
    pub fn publish(&self, ingress: NativeIngress) -> Result<(), NativeIngress> {
        self.ingress.set(ingress)
    }

    pub fn ingress(&self) -> Option<NativeIngress> {
        self.ingress.get().cloned()
    }
}

pub fn start_ingress(service: PlaybackCommandService) -> NativeIngress {
    let (tx, mut rx) = tokio::sync::mpsc::channel(NATIVE_INGRESS_CAPACITY);
    let latest = Arc::new(Mutex::new(
        service
            .playback()
            .snapshot()
            .map(|snapshot| NativePlaybackView::from_snapshot(&snapshot, false))
            .unwrap_or_default(),
    ));
    let worker_latest = latest.clone();
    tokio::spawn(async move {
        let mut refresh = tokio::time::interval(std::time::Duration::from_millis(250));
        loop {
            tokio::select! {
                intent = rx.recv() => {
                    let Some(intent) = intent else { break; };
                    match service.native_control(intent).await {
                        Ok(snapshot) => {
                            *worker_latest.lock().unwrap_or_else(|e| e.into_inner()) =
                                NativePlaybackView::from_snapshot(&snapshot, false);
                        }
                        Err(error) => {
                            let mut view = worker_latest.lock().unwrap_or_else(|e| e.into_inner());
                            view.failure_code = Some(error.code.into());
                        }
                    }
                }
                _ = refresh.tick() => {
                    if let Ok(snapshot) = service.playback().snapshot() {
                        *worker_latest.lock().unwrap_or_else(|e| e.into_inner()) =
                            NativePlaybackView::from_snapshot(
                                &snapshot,
                                service.playback().is_fenced(),
                            );
                    }
                }
            }
        }
        let mut view = worker_latest.lock().unwrap_or_else(|e| e.into_inner());
        view.commands = NativeCommandMask::default();
    });
    NativeIngress { tx, latest }
}

pub struct NativeMediaOwner {
    controls: souvlaki::MediaControls,
    ingress: NativeIngress,
    last: Option<NativePlaybackView>,
    rejected: Arc<AtomicU64>,
}

impl NativeMediaOwner {
    pub fn register(
        ingress: NativeIngress,
        hwnd: Option<*mut std::ffi::c_void>,
    ) -> Result<Self, String> {
        let mut controls = souvlaki::MediaControls::new(souvlaki::PlatformConfig {
            dbus_name: "hifimule",
            display_name: "HifiMule",
            hwnd,
        })
        .map_err(|error| format!("native registration failed: {error}"))?;
        let initial = ingress.latest();
        controls
            .set_capabilities(capabilities(initial.commands))
            .map_err(|error| format!("native capabilities failed: {error}"))?;
        let callback_ingress = ingress.clone();
        let rejected = Arc::new(AtomicU64::new(0));
        let callback_rejected = rejected.clone();
        controls
            .attach_checked(move |event| {
                let intent = event_to_intent(event);
                let accepted =
                    intent.is_some_and(|intent| callback_ingress.try_send(intent).is_ok());
                if !accepted {
                    callback_rejected.fetch_add(1, Ordering::Relaxed);
                }
                accepted
            })
            .map_err(|error| format!("native attachment failed: {error}"))?;
        let mut owner = Self {
            controls,
            ingress,
            last: None,
            rejected,
        };
        owner.refresh()?;
        Ok(owner)
    }

    pub fn refresh(&mut self) -> Result<(), String> {
        let view = self.ingress.latest();
        let rejected = self.rejected.swap(0, Ordering::AcqRel);
        if rejected > 0 {
            crate::daemon_log!(
                "Native controls rejected {rejected} unsupported, full, or stopping command(s)"
            );
        }
        if self.last.as_ref() == Some(&view) {
            return Ok(());
        }
        self.controls
            .set_capabilities(capabilities(view.commands))
            .map_err(|error| format!("native capabilities update failed: {error}"))?;
        self.controls
            .set_metadata(souvlaki::MediaMetadata {
                title: view.title.as_deref(),
                artist: view.artist.as_deref(),
                album: view.album.as_deref(),
                cover_url: None,
                duration: view.duration_ms.map(std::time::Duration::from_millis),
            })
            .map_err(|error| format!("native metadata update failed: {error}"))?;
        let progress = Some(souvlaki::MediaPosition(std::time::Duration::from_millis(
            view.position_ms,
        )));
        let playback = match view.status {
            PlaybackStatus::Active => souvlaki::MediaPlayback::Playing { progress },
            PlaybackStatus::Loading | PlaybackStatus::Paused | PlaybackStatus::Error => {
                souvlaki::MediaPlayback::Paused { progress }
            }
            PlaybackStatus::Idle | PlaybackStatus::Stopped | PlaybackStatus::Completed => {
                souvlaki::MediaPlayback::Stopped
            }
        };
        self.controls
            .set_playback(playback)
            .map_err(|error| format!("native playback update failed: {error}"))?;
        self.last = Some(view);
        Ok(())
    }

    pub fn detach(&mut self) -> Result<(), String> {
        self.controls
            .set_capabilities(souvlaki::MediaControlCapabilities::default())
            .map_err(|error| format!("native disable failed: {error}"))?;
        self.controls
            .set_metadata(souvlaki::MediaMetadata::default())
            .map_err(|error| format!("native metadata clear failed: {error}"))?;
        self.controls
            .set_playback(souvlaki::MediaPlayback::Stopped)
            .map_err(|error| format!("native stop publication failed: {error}"))?;
        self.controls
            .detach()
            .map_err(|error| format!("native detach failed: {error}"))
    }
}

fn capabilities(mask: NativeCommandMask) -> souvlaki::MediaControlCapabilities {
    souvlaki::MediaControlCapabilities {
        play: mask.play,
        pause: mask.pause,
        toggle: mask.toggle,
        stop: mask.stop,
        next: false,
        previous: false,
        seek: false,
        raise: false,
        quit: false,
    }
}

fn event_to_intent(event: souvlaki::MediaControlEvent) -> Option<NativeControlIntent> {
    match event {
        souvlaki::MediaControlEvent::Play => Some(NativeControlIntent::Play),
        souvlaki::MediaControlEvent::Pause => Some(NativeControlIntent::Pause),
        souvlaki::MediaControlEvent::Toggle => Some(NativeControlIntent::Toggle),
        souvlaki::MediaControlEvent::Stop => Some(NativeControlIntent::Stop),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playback::devices::OutputDescriptor;
    use crate::playback::model::*;

    fn snapshot(status: PlaybackStatus, state: TransportState) -> SessionSnapshot {
        SessionSnapshot {
            resume_audio: false,
            schema_version: 1,
            instance_id: "instance".into(),
            session_id: "session".into(),
            queue_revision: "1".into(),
            state_sequence: "2".into(),
            generation_id: "generation".into(),
            state,
            current: Some(Occurrence {
                occurrence_id: "occurrence".into(),
                ordinal: 0,
                source: TrackSource {
                    server_id: "server".into(),
                    track_id: "track".into(),
                },
                availability: SourceAvailability::Unknown,
            }),
            position_ms: 123,
            checkpointed_position_ms: 100,
            persistence: Status {
                status: "ok".into(),
                code: None,
            },
            restoration: Status {
                status: "ok".into(),
                code: None,
            },
            total_occurrence_count: 1,
            occurrences: vec![],
            next_cursor: None,
            playback: PlaybackState {
                status,
                ..Default::default()
            },
            output: OutputState {
                selected: Some(OutputDescriptor {
                    output_id: "output".into(),
                    display_name: "Speakers".into(),
                    detail: "Built in".into(),
                    backend: "test".into(),
                    available: true,
                    is_default: true,
                    identity_confidence: "stable".into(),
                    is_virtual: false,
                    preference: None,
                }),
                status: "available".into(),
                ..Default::default()
            },
        }
    }

    #[test]
    fn projection_advertises_only_story_transport_actions() {
        let paused = NativePlaybackView::from_snapshot(
            &snapshot(PlaybackStatus::Paused, TransportState::Paused),
            false,
        );
        assert_eq!(
            paused.commands,
            NativeCommandMask {
                play: true,
                pause: false,
                toggle: true,
                stop: true
            }
        );
        let active = NativePlaybackView::from_snapshot(
            &snapshot(PlaybackStatus::Active, TransportState::Playing),
            false,
        );
        assert_eq!(
            active.commands,
            NativeCommandMask {
                play: false,
                pause: true,
                toggle: true,
                stop: true
            }
        );
    }

    #[test]
    fn shutdown_projection_disables_every_command() {
        let view = NativePlaybackView::from_snapshot(
            &snapshot(PlaybackStatus::Active, TransportState::Playing),
            true,
        );
        assert_eq!(view.commands, NativeCommandMask::default());
    }

    #[test]
    fn projection_clears_missing_metadata_and_disables_resume_without_selected_output() {
        let mut rich = snapshot(PlaybackStatus::Paused, TransportState::Paused);
        rich.playback.metadata = Some(PlaybackTrackMetadata {
            source: rich.current.as_ref().unwrap().source.clone(),
            title: "Track".into(),
            artist: Some("Artist".into()),
            album: Some("Album".into()),
        });
        rich.playback.duration_ms = Some(42_000);
        let rich_view = NativePlaybackView::from_snapshot(&rich, false);
        assert_eq!(rich_view.artist.as_deref(), Some("Artist"));
        assert_eq!(rich_view.duration_ms, Some(42_000));

        rich.playback.metadata = None;
        rich.playback.duration_ms = None;
        let poor_view = NativePlaybackView::from_snapshot(&rich, false);
        assert_eq!(poor_view.title, None);
        assert_eq!(poor_view.artist, None);
        assert_eq!(poor_view.album, None);
        assert_eq!(poor_view.duration_ms, None);

        rich.playback.metadata = Some(PlaybackTrackMetadata {
            source: TrackSource {
                server_id: "other-server".into(),
                track_id: "other-track".into(),
            },
            title: "Stale track".into(),
            artist: Some("Stale artist".into()),
            album: None,
        });
        rich.playback.duration_ms = Some(99_000);
        let stale_view = NativePlaybackView::from_snapshot(&rich, false);
        assert_eq!(stale_view.title, None);
        assert_eq!(stale_view.duration_ms, None);

        rich.output = OutputState::default();
        let unavailable = NativePlaybackView::from_snapshot(&rich, false);
        assert!(!unavailable.commands.play);
        assert!(!unavailable.commands.toggle);
        assert!(unavailable.commands.stop);

        rich.playback.status = PlaybackStatus::Active;
        rich.state = TransportState::Playing;
        let lost_while_playing = NativePlaybackView::from_snapshot(&rich, false);
        assert_eq!(lost_while_playing.status, PlaybackStatus::Paused);
    }

    #[test]
    fn bounded_ingress_rejects_overflow_and_closed_delivery() {
        let (tx, rx) = tokio::sync::mpsc::channel(NATIVE_INGRESS_CAPACITY);
        let ingress = NativeIngress {
            tx,
            latest: Arc::new(Mutex::new(NativePlaybackView::default())),
        };
        for _ in 0..NATIVE_INGRESS_CAPACITY {
            assert_eq!(ingress.try_send(NativeControlIntent::Toggle), Ok(()));
        }
        assert_eq!(
            ingress.try_send(NativeControlIntent::Toggle),
            Err(NativeIngressError::Full)
        );
        drop(rx);
        assert_eq!(
            ingress.try_send(NativeControlIntent::Stop),
            Err(NativeIngressError::Stopped)
        );
    }

    #[test]
    fn unsupported_native_events_are_not_remapped() {
        assert_eq!(
            event_to_intent(souvlaki::MediaControlEvent::Stop),
            Some(NativeControlIntent::Stop)
        );
        assert_eq!(event_to_intent(souvlaki::MediaControlEvent::Next), None);
        assert_eq!(
            event_to_intent(souvlaki::MediaControlEvent::SetPosition(
                souvlaki::MediaPosition(std::time::Duration::from_secs(1))
            )),
            None
        );
    }
}
