use super::model::{PlaybackStatus, SessionSnapshot, TransportState};
use super::{NativeControlIntent, commands::PlaybackCommandService};
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

pub const NATIVE_INGRESS_CAPACITY: usize = 64;

type CommandResult = Result<(), String>;
pub type NativeCommandReceipt = tokio::sync::oneshot::Receiver<CommandResult>;

struct NativeRequest {
    intent: NativeControlIntent,
    reply: Option<tokio::sync::oneshot::Sender<CommandResult>>,
}

struct PendingResume {
    generation_id: String,
    control_epoch: u64,
    seek_operation_id: Option<String>,
    replies: Vec<tokio::sync::oneshot::Sender<CommandResult>>,
}

impl PendingResume {
    fn finish(self, result: CommandResult) {
        for reply in self.replies {
            let _ = reply.send(result.clone());
        }
    }
}

/// Command errors have their own lifetime: sampling transport must not erase
/// a rejection before the desktop loop can announce it.
#[derive(Default)]
struct CommandFeedback {
    failure: Option<String>,
    pending: Option<PendingResume>,
}

impl CommandFeedback {
    fn admitted(&mut self, snapshot: &SessionSnapshot, request: NativeRequest) {
        if let Some(pending) = self.pending.as_mut()
            && pending.generation_id == snapshot.generation_id
            && pending.control_epoch == snapshot.resume_epoch
            && pending.seek_operation_id
                == snapshot
                    .playback
                    .pending_seek
                    .as_ref()
                    .map(|seek| seek.operation_id.clone())
            && (snapshot.playback.status == PlaybackStatus::Loading
                || snapshot.playback.pending_seek.is_some())
        {
            // An idempotent Play joins the existing attempt; it must not
            // complete the original menu receipt before preparation finishes.
            if let Some(reply) = request.reply {
                if pending.replies.len() < NATIVE_INGRESS_CAPACITY {
                    pending.replies.push(reply);
                } else {
                    let _ = reply.send(Err("PLAYBACK_BUSY".into()));
                }
            }
            return;
        }
        if let Some(previous) = self.pending.take() {
            // A later admitted intent supersedes this attempt. It is not a failure.
            previous.finish(Ok(()));
        }
        self.failure = None;
        let pending = PendingResume {
            generation_id: snapshot.generation_id.clone(),
            control_epoch: snapshot.resume_epoch,
            seek_operation_id: snapshot
                .playback
                .pending_seek
                .as_ref()
                .map(|seek| seek.operation_id.clone()),
            replies: request.reply.into_iter().collect(),
        };
        if snapshot.playback.status == PlaybackStatus::Loading
            || snapshot.playback.pending_seek.is_some()
        {
            self.pending = Some(pending);
        } else {
            pending.finish(Ok(()));
        }
    }

    fn rejected(&mut self, request: NativeRequest, code: &str) {
        self.failure = Some(code.to_owned());
        if let Some(reply) = request.reply {
            let _ = reply.send(Err(code.to_owned()));
        }
    }

    fn project(&mut self, mut view: NativePlaybackView) -> NativePlaybackView {
        if let Some(pending) = self.pending.as_ref() {
            let result = if view.generation_id != pending.generation_id
                || view.control_epoch != pending.control_epoch
            {
                Some(Ok(())) // A replacement/Stop superseded the attempt.
            } else if pending.seek_operation_id.is_some()
                && view.pending_seek_operation_id == pending.seek_operation_id
            {
                None
            } else if pending.seek_operation_id.is_some() {
                view.failure_code
                    .clone()
                    .map_or(Some(Ok(())), |code| Some(Err(code)))
            } else if let Some(code) = view.failure_code.as_ref() {
                Some(Err(code.clone()))
            } else if view.status != PlaybackStatus::Loading {
                Some(Ok(())) // Active, or explicitly paused/stopped in the meantime.
            } else {
                None
            };
            if let Some(result) = result {
                if let Err(code) = &result {
                    self.failure = Some(code.clone());
                }
                self.pending.take().unwrap().finish(result);
            }
        }
        if view.status == PlaybackStatus::Active && view.failure_code.is_none() {
            self.failure = None;
        }
        if let Some(code) = self.failure.as_ref() {
            view.failure_code = Some(code.clone());
        }
        view
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NativeCommandMask {
    pub play: bool,
    pub pause: bool,
    pub toggle: bool,
    pub stop: bool,
    pub next: bool,
    pub seek: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativePlaybackView {
    pub instance_id: String,
    pub session_id: String,
    pub generation_id: String,
    pub state_sequence: String,
    pub control_epoch: u64,
    pub status: PlaybackStatus,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_ms: Option<u64>,
    pub position_ms: u64,
    pub occurrence_id: Option<String>,
    pub pending_seek_operation_id: Option<String>,
    pub seeked_position_ms: Option<u64>,
    pub seeked_operation_id: Option<String>,
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
            control_epoch: 0,
            status: PlaybackStatus::Idle,
            title: None,
            artist: None,
            album: None,
            duration_ms: None,
            position_ms: 0,
            occurrence_id: None,
            pending_seek_operation_id: None,
            seeked_position_ms: None,
            seeked_operation_id: None,
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
            control_epoch: snapshot.resume_epoch,
            status,
            title: metadata.map(|value| value.title.clone()),
            artist: metadata.and_then(|value| value.artist.clone()),
            album: metadata.and_then(|value| value.album.clone()),
            duration_ms: metadata.and(snapshot.playback.duration_ms),
            position_ms: snapshot.position_ms,
            occurrence_id: snapshot.current.as_ref().map(|value| {
                format!(
                    "/org/mpris/MediaPlayer2/track/{}",
                    value.occurrence_id.replace('-', "_")
                )
            }),
            pending_seek_operation_id: snapshot
                .playback
                .pending_seek
                .as_ref()
                .map(|seek| seek.operation_id.clone()),
            seeked_position_ms: snapshot.playback.seek_outcome.as_ref().and_then(|outcome| {
                (outcome.status == "succeeded")
                    .then_some(outcome.actual_position_ms)
                    .flatten()
            }),
            seeked_operation_id: snapshot.playback.seek_outcome.as_ref().and_then(|outcome| {
                (outcome.status == "succeeded" && outcome.actual_position_ms.is_some())
                    .then(|| outcome.operation_id.clone())
            }),
            commands: NativeCommandMask {
                play: resumable && !active,
                pause: !stopping && current && active,
                toggle: !stopping && (resumable || active),
                stop: !stopping && current,
                next: !stopping && snapshot.playback.can_go_next,
                seek: !stopping
                    && current
                    && snapshot.playback.seek.available
                    && snapshot
                        .playback
                        .duration_ms
                        .is_some_and(|duration| duration > 0),
            },
            failure_code: snapshot
                .playback
                .error
                .as_ref()
                .or(snapshot.output.error.as_ref())
                .map(|value| value.code.clone()),
        }
    }
}

#[derive(Clone)]
pub struct NativeIngress {
    tx: tokio::sync::mpsc::Sender<NativeRequest>,
    latest: Arc<Mutex<NativePlaybackView>>,
}

impl NativeIngress {
    pub fn try_send(&self, intent: NativeControlIntent) -> Result<(), NativeIngressError> {
        self.enqueue(NativeRequest {
            intent,
            reply: None,
        })
    }

    pub fn try_send_tracked(
        &self,
        intent: NativeControlIntent,
    ) -> Result<NativeCommandReceipt, NativeIngressError> {
        let (reply, receipt) = tokio::sync::oneshot::channel();
        self.enqueue(NativeRequest {
            intent,
            reply: Some(reply),
        })?;
        Ok(receipt)
    }

    fn enqueue(&self, request: NativeRequest) -> Result<(), NativeIngressError> {
        self.tx.try_send(request).map_err(|error| match error {
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
    let (tx, mut rx) = tokio::sync::mpsc::channel::<NativeRequest>(NATIVE_INGRESS_CAPACITY);
    let latest = Arc::new(Mutex::new(
        service
            .playback()
            .snapshot()
            .map(|snapshot| NativePlaybackView::from_snapshot(&snapshot, false))
            .unwrap_or_default(),
    ));
    let worker_latest = latest.clone();
    tokio::spawn(async move {
        let mut feedback = CommandFeedback::default();
        let mut refresh = tokio::time::interval(std::time::Duration::from_millis(250));
        refresh.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                request = rx.recv() => {
                    let Some(request) = request else { break; };
                    match service.native_control(request.intent).await {
                        Ok(snapshot) => {
                            feedback.admitted(&snapshot, request);
                            *worker_latest.lock().unwrap_or_else(|e| e.into_inner()) =
                                feedback.project(NativePlaybackView::from_snapshot(&snapshot, false));
                        }
                        Err(error) => {
                            feedback.rejected(request, error.code);
                            let mut view = worker_latest.lock().unwrap_or_else(|e| e.into_inner());
                            view.failure_code = feedback.failure.clone();
                        }
                    }
                }
                _ = refresh.tick() => {
                    if let Ok(snapshot) = service.playback().snapshot() {
                        *worker_latest.lock().unwrap_or_else(|e| e.into_inner()) =
                            feedback.project(NativePlaybackView::from_snapshot(
                                &snapshot,
                                service.playback().is_fenced(),
                            ));
                    }
                }
            }
        }
        let mut view = worker_latest.lock().unwrap_or_else(|e| e.into_inner());
        view.commands = NativeCommandMask::default();
    });
    NativeIngress { tx, latest }
}

/// The adapter boundary is injectable so lifecycle tests exercise the same
/// registration/publication/teardown owner used by the desktop loop.
pub trait NativeBackend {
    fn set_capabilities(&mut self, value: souvlaki::MediaControlCapabilities)
    -> Result<(), String>;
    fn attach(
        &mut self,
        callback: Box<dyn Fn(souvlaki::MediaControlEvent) -> bool + Send>,
    ) -> Result<(), String>;
    fn set_metadata(&mut self, value: souvlaki::MediaMetadata<'_>) -> Result<(), String>;
    fn set_playback(&mut self, value: souvlaki::MediaPlayback) -> Result<(), String>;
    fn set_seeked(&mut self, position_micros: i64) -> Result<(), String>;
    fn detach(&mut self) -> Result<(), String>;
}

impl NativeBackend for souvlaki::MediaControls {
    fn set_capabilities(
        &mut self,
        value: souvlaki::MediaControlCapabilities,
    ) -> Result<(), String> {
        self.set_capabilities(value)
            .map_err(|error| error.to_string())
    }
    fn attach(
        &mut self,
        callback: Box<dyn Fn(souvlaki::MediaControlEvent) -> bool + Send>,
    ) -> Result<(), String> {
        self.attach_checked(callback)
            .map_err(|error| error.to_string())
    }
    fn set_metadata(&mut self, value: souvlaki::MediaMetadata<'_>) -> Result<(), String> {
        self.set_metadata(value).map_err(|error| error.to_string())
    }
    fn set_playback(&mut self, value: souvlaki::MediaPlayback) -> Result<(), String> {
        self.set_playback(value).map_err(|error| error.to_string())
    }
    fn set_seeked(&mut self, position_micros: i64) -> Result<(), String> {
        self.set_seeked(position_micros)
            .map_err(|error| error.to_string())
    }
    fn detach(&mut self) -> Result<(), String> {
        self.detach().map_err(|error| error.to_string())
    }
}

pub struct NativeMediaOwner<B: NativeBackend = souvlaki::MediaControls> {
    controls: Option<B>,
    ingress: NativeIngress,
    last: Option<NativePlaybackView>,
    rejected: Arc<AtomicU64>,
    accepting: Arc<AtomicBool>,
}

impl NativeMediaOwner {
    pub fn register(
        ingress: NativeIngress,
        hwnd: Option<*mut std::ffi::c_void>,
    ) -> Result<Self, String> {
        // A failed hidden-window prerequisite must not reach Souvlaki's HWND
        // assertion or terminate an otherwise functional daemon.
        #[cfg(windows)]
        if hwnd.is_none() {
            return Err("native media window is unavailable".into());
        }
        Self::register_with(ingress, || {
            souvlaki::MediaControls::new(souvlaki::PlatformConfig {
                dbus_name: "hifimule",
                display_name: "HifiMule",
                hwnd,
            })
            .map_err(|error| format!("native registration failed: {error}"))
        })
    }
}

impl<B: NativeBackend> NativeMediaOwner<B> {
    fn register_with(
        ingress: NativeIngress,
        create: impl FnOnce() -> Result<B, String>,
    ) -> Result<Self, String> {
        let mut owner = Self {
            controls: Some(create()?),
            ingress,
            last: None,
            rejected: Arc::new(AtomicU64::new(0)),
            accepting: Arc::new(AtomicBool::new(true)),
        };
        let result = (|| {
            let controls = owner.controls.as_mut().unwrap();
            controls.set_capabilities(capabilities(owner.ingress.latest().commands))?;
            let callback_ingress = owner.ingress.clone();
            let callback_rejected = owner.rejected.clone();
            let accepting = owner.accepting.clone();
            controls.attach(Box::new(move |event| {
                let accepted = accepting.load(Ordering::Acquire)
                    && event_to_intent(event)
                        .is_some_and(|intent| callback_ingress.try_send(intent).is_ok());
                if !accepted {
                    callback_rejected.fetch_add(1, Ordering::Relaxed);
                }
                accepted
            }))?;
            owner.refresh()
        })();
        if let Err(error) = result {
            return match owner.detach() {
                Ok(()) => Err(error),
                Err(cleanup) => Err(format!("{error}; cleanup: {cleanup}")),
            };
        }
        Ok(owner)
    }

    pub fn refresh(&mut self) -> Result<(), String> {
        let Some(controls) = self.controls.as_mut() else {
            return Ok(());
        };
        let view = self.ingress.latest();
        let rejected = self.rejected.swap(0, Ordering::AcqRel);
        if rejected > 0 {
            crate::daemon_log!(
                "Native controls rejected {rejected} unsupported, full, or stopping command(s)"
            );
        }
        let last = self.last.as_ref();
        if last.is_none_or(|last| last.commands != view.commands) {
            controls
                .set_capabilities(capabilities(view.commands))
                .map_err(|error| format!("native capabilities update failed: {error}"))?;
        }
        let metadata_changed = last.is_none_or(|last| {
            last.session_id != view.session_id
                || last.generation_id != view.generation_id
                || last.title != view.title
                || last.artist != view.artist
                || last.album != view.album
                || last.duration_ms != view.duration_ms
        });
        if metadata_changed {
            controls
                .set_metadata(souvlaki::MediaMetadata {
                    // Souvlaki copies this into its owned publication state.
                    track_id: view.occurrence_id.as_deref(),
                    title: view.title.as_deref(),
                    artist: view.artist.as_deref(),
                    album: view.album.as_deref(),
                    cover_url: None,
                    duration: view.duration_ms.map(std::time::Duration::from_millis),
                })
                .map_err(|error| format!("native metadata update failed: {error}"))?;
        }
        // macOS replaces the entire metadata dictionary, including elapsed time.
        if metadata_changed
            || last.is_none_or(|last| {
                last.status != view.status || last.position_ms != view.position_ms
            })
        {
            let progress = Some(souvlaki::MediaPosition(std::time::Duration::from_millis(
                view.position_ms,
            )));
            let playback = match view.status {
                PlaybackStatus::Active => souvlaki::MediaPlayback::Playing { progress },
                PlaybackStatus::Loading | PlaybackStatus::Paused | PlaybackStatus::Error => {
                    souvlaki::MediaPlayback::Paused { progress }
                }
                PlaybackStatus::Completed => souvlaki::MediaPlayback::Paused { progress },
                PlaybackStatus::Idle | PlaybackStatus::Stopped => souvlaki::MediaPlayback::Stopped,
            };
            controls
                .set_playback(playback)
                .map_err(|error| format!("native playback update failed: {error}"))?;
        }
        if let (Some(position_ms), Some(operation_id)) =
            (view.seeked_position_ms, view.seeked_operation_id.as_ref())
            && last.is_none_or(|last| {
                last.instance_id != view.instance_id
                    || last.session_id != view.session_id
                    || last.seeked_operation_id.as_ref() != Some(operation_id)
            })
        {
            let micros = position_ms
                .checked_mul(1000)
                .and_then(|value| i64::try_from(value).ok())
                .ok_or_else(|| "native seek position overflow".to_string())?;
            controls
                .set_seeked(micros)
                .map_err(|error| format!("native seek publication failed: {error}"))?;
        }
        self.last = Some(view);
        Ok(())
    }

    pub fn detach(&mut self) -> Result<(), String> {
        self.accepting.store(false, Ordering::Release);
        let Some(mut controls) = self.controls.take() else {
            return Ok(());
        };
        // Cleanup must attempt handler removal even if clearing one property
        // fails (for example, when the desktop service has disappeared).
        let mut failures = Vec::new();
        for result in [
            controls.set_capabilities(souvlaki::MediaControlCapabilities::default()),
            controls.set_metadata(souvlaki::MediaMetadata::default()),
            controls.set_playback(souvlaki::MediaPlayback::Stopped),
            controls.detach(),
        ] {
            if let Err(error) = result {
                failures.push(error);
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("; "))
        }
    }
}

impl<B: NativeBackend> Drop for NativeMediaOwner<B> {
    fn drop(&mut self) {
        if let Err(error) = self.detach() {
            crate::daemon_log!("Native controls teardown failed: {error}");
        }
    }
}

fn capabilities(mask: NativeCommandMask) -> souvlaki::MediaControlCapabilities {
    souvlaki::MediaControlCapabilities {
        play: mask.play,
        pause: mask.pause,
        toggle: mask.toggle,
        stop: mask.stop,
        next: mask.next,
        previous: false,
        seek: mask.seek,
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
        souvlaki::MediaControlEvent::Next => Some(NativeControlIntent::Next),
        souvlaki::MediaControlEvent::SetPosition(position) => u64::try_from(position.0.as_millis())
            .ok()
            .map(NativeControlIntent::SeekAbsolute),
        souvlaki::MediaControlEvent::SeekBy(direction, amount) => {
            let millis = i64::try_from(amount.as_millis()).ok()?;
            Some(NativeControlIntent::SeekRelative(match direction {
                souvlaki::SeekDirection::Forward => millis,
                souvlaki::SeekDirection::Backward => -millis,
            }))
        }
        souvlaki::MediaControlEvent::Seek(direction) => {
            Some(NativeControlIntent::SeekRelative(match direction {
                souvlaki::SeekDirection::Forward => 10_000,
                souvlaki::SeekDirection::Backward => -10_000,
            }))
        }
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
            resume_epoch: 0,
            seek_audio: false,
            seek_epoch: 0,
            gain_bits: 1.0f32.to_bits(),
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
                stop: true,
                next: false,
                seek: false,
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
                stop: true,
                next: false,
                seek: false,
            }
        );
    }

    #[test]
    fn projection_and_native_units_enable_seek_only_for_qualified_tracks() {
        let mut qualified = snapshot(PlaybackStatus::Paused, TransportState::Paused);
        qualified.playback.duration_ms = Some(42_000);
        qualified.playback.seek = SeekCapability::jellyfin_pcm_wav();
        let view = NativePlaybackView::from_snapshot(&qualified, false);
        assert!(view.commands.seek);
        assert_eq!(
            event_to_intent(souvlaki::MediaControlEvent::SetPosition(
                souvlaki::MediaPosition(std::time::Duration::from_micros(1_234_000)),
            )),
            Some(NativeControlIntent::SeekAbsolute(1_234))
        );
        assert_eq!(
            event_to_intent(souvlaki::MediaControlEvent::SeekBy(
                souvlaki::SeekDirection::Backward,
                std::time::Duration::from_micros(2_500_000),
            )),
            Some(NativeControlIntent::SeekRelative(-2_500))
        );
        assert_eq!(
            event_to_intent(souvlaki::MediaControlEvent::Seek(
                souvlaki::SeekDirection::Forward,
            )),
            Some(NativeControlIntent::SeekRelative(10_000))
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
    fn next_is_mapped_but_previous_remains_unsupported() {
        assert_eq!(
            event_to_intent(souvlaki::MediaControlEvent::Stop),
            Some(NativeControlIntent::Stop)
        );
        assert_eq!(
            event_to_intent(souvlaki::MediaControlEvent::Next),
            Some(NativeControlIntent::Next)
        );
        assert_eq!(event_to_intent(souvlaki::MediaControlEvent::Previous), None);
    }

    #[test]
    fn request_receipts_never_consume_a_previous_failure() {
        let mut feedback = CommandFeedback::default();
        let (reply, mut rejected) = tokio::sync::oneshot::channel();
        feedback.rejected(
            NativeRequest {
                intent: NativeControlIntent::Play,
                reply: Some(reply),
            },
            "OUTPUT_UNAVAILABLE",
        );
        let paused = snapshot(PlaybackStatus::Paused, TransportState::Paused);
        for _ in 0..10 {
            assert_eq!(
                feedback
                    .project(NativePlaybackView::from_snapshot(&paused, false))
                    .failure_code
                    .as_deref(),
                Some("OUTPUT_UNAVAILABLE")
            );
        }
        assert_eq!(
            rejected.try_recv().unwrap(),
            Err("OUTPUT_UNAVAILABLE".into())
        );

        let (reply, mut retry) = tokio::sync::oneshot::channel();
        let mut loading = snapshot(PlaybackStatus::Loading, TransportState::Buffering);
        feedback.admitted(
            &loading,
            NativeRequest {
                intent: NativeControlIntent::Play,
                reply: Some(reply),
            },
        );
        assert_eq!(
            feedback
                .project(NativePlaybackView::from_snapshot(&loading, false))
                .failure_code,
            None
        );
        assert_eq!(
            retry.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        );
        loading.playback.status = PlaybackStatus::Error;
        loading.playback.error = Some(PlaybackFailure {
            code: "RESUME_UNAVAILABLE".into(),
            retryable: true,
        });
        feedback.project(NativePlaybackView::from_snapshot(&loading, false));
        assert_eq!(retry.try_recv().unwrap(), Err("RESUME_UNAVAILABLE".into()));
        assert_eq!(
            feedback
                .project(NativePlaybackView::from_snapshot(&paused, false))
                .failure_code
                .as_deref(),
            Some("RESUME_UNAVAILABLE")
        );
    }

    #[tokio::test]
    async fn worker_preserves_rejection_across_refresh_until_a_successful_command() {
        let db = Arc::new(crate::db::Database::memory().unwrap());
        let playback = super::super::PlaybackSession::restore(db.clone(), "feedback-owner".into());
        let ingress = start_ingress(PlaybackCommandService::new(
            playback.clone(),
            Arc::new(tokio::sync::RwLock::new(
                crate::server_manager::ServerManager::new(),
            )),
            db,
            Arc::new(crate::sync::SyncOperationManager::new()),
        ));
        let receipt = ingress.try_send_tracked(NativeControlIntent::Play).unwrap();
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(2), receipt)
                .await
                .unwrap()
                .unwrap(),
            Err("RESUME_UNAVAILABLE".into())
        );
        // Several real worker refreshes occur before the desktop reads its status.
        tokio::time::sleep(std::time::Duration::from_millis(550)).await;
        assert_eq!(
            ingress.latest().failure_code.as_deref(),
            Some("RESUME_UNAVAILABLE")
        );
        let current = playback.snapshot().unwrap();
        playback
            .apply(ApplySessionParams {
                schema_version: 1,
                instance_id: current.instance_id,
                session_id: current.session_id,
                command_id: uuid::Uuid::new_v4().to_string(),
                expected_queue_revision: current.queue_revision,
                operation: SessionOperation::ReplaceQueue {
                    sources: vec![TrackSource {
                        server_id: "offline".into(),
                        track_id: "track".into(),
                    }],
                },
            })
            .unwrap();
        let receipt = ingress.try_send_tracked(NativeControlIntent::Stop).unwrap();
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(2), receipt)
                .await
                .unwrap()
                .unwrap(),
            Ok(())
        );
        // The receipt and projection are published in the same worker iteration.
        tokio::task::yield_now().await;
        assert_eq!(ingress.latest().failure_code, None);
        drop(ingress);
        playback.stop_and_join().unwrap();
    }

    #[derive(Debug, PartialEq)]
    enum Call {
        Capabilities(souvlaki::MediaControlCapabilities),
        Attach,
        Metadata(
            Option<String>,
            Option<String>,
            Option<String>,
            Option<std::time::Duration>,
        ),
        Playback(souvlaki::MediaPlayback),
        Seeked(i64),
        Detach,
        Drop,
    }

    #[derive(Default)]
    struct AdapterState {
        calls: Vec<Call>,
        callback: Option<Box<dyn Fn(souvlaki::MediaControlEvent) -> bool + Send>>,
        fail_attach: bool,
        fail_metadata: bool,
        fail_detach: bool,
    }

    struct TestBackend(Arc<Mutex<AdapterState>>);
    impl NativeBackend for TestBackend {
        fn set_capabilities(
            &mut self,
            value: souvlaki::MediaControlCapabilities,
        ) -> Result<(), String> {
            self.0.lock().unwrap().calls.push(Call::Capabilities(value));
            Ok(())
        }
        fn attach(
            &mut self,
            callback: Box<dyn Fn(souvlaki::MediaControlEvent) -> bool + Send>,
        ) -> Result<(), String> {
            let mut state = self.0.lock().unwrap();
            state.calls.push(Call::Attach);
            state.callback = Some(callback);
            if state.fail_attach {
                Err("partial attach".into())
            } else {
                Ok(())
            }
        }
        fn set_metadata(&mut self, value: souvlaki::MediaMetadata<'_>) -> Result<(), String> {
            let mut state = self.0.lock().unwrap();
            state.calls.push(Call::Metadata(
                value.title.map(str::to_owned),
                value.artist.map(str::to_owned),
                value.album.map(str::to_owned),
                value.duration,
            ));
            if state.fail_metadata {
                Err("metadata unavailable".into())
            } else {
                Ok(())
            }
        }
        fn set_playback(&mut self, value: souvlaki::MediaPlayback) -> Result<(), String> {
            self.0.lock().unwrap().calls.push(Call::Playback(value));
            Ok(())
        }
        fn set_seeked(&mut self, position_micros: i64) -> Result<(), String> {
            self.0
                .lock()
                .unwrap()
                .calls
                .push(Call::Seeked(position_micros));
            Ok(())
        }
        fn detach(&mut self) -> Result<(), String> {
            let mut state = self.0.lock().unwrap();
            state.calls.push(Call::Detach);
            if state.fail_detach {
                Err("detach unavailable".into())
            } else {
                Ok(())
            }
        }
    }
    impl Drop for TestBackend {
        fn drop(&mut self) {
            self.0.lock().unwrap().calls.push(Call::Drop);
        }
    }

    fn test_ingress() -> (NativeIngress, tokio::sync::mpsc::Receiver<NativeRequest>) {
        let (tx, rx) = tokio::sync::mpsc::channel(NATIVE_INGRESS_CAPACITY);
        (
            NativeIngress {
                tx,
                latest: Arc::new(Mutex::new(NativePlaybackView::from_snapshot(
                    &snapshot(PlaybackStatus::Active, TransportState::Playing),
                    false,
                ))),
            },
            rx,
        )
    }

    #[test]
    fn native_owner_registers_once_coalesces_fields_and_rejects_late_callbacks() {
        let (ingress, mut rx) = test_ingress();
        let state = Arc::new(Mutex::new(AdapterState::default()));
        let mut owner =
            NativeMediaOwner::register_with(ingress.clone(), || Ok(TestBackend(state.clone())))
                .unwrap();
        assert!((state.lock().unwrap().callback.as_ref().unwrap())(
            souvlaki::MediaControlEvent::Pause
        ));
        assert_eq!(rx.try_recv().unwrap().intent, NativeControlIntent::Pause);
        assert!((state.lock().unwrap().callback.as_ref().unwrap())(
            souvlaki::MediaControlEvent::Next
        ));
        assert_eq!(rx.try_recv().unwrap().intent, NativeControlIntent::Next);
        let initial_calls = state.lock().unwrap().calls.len();
        // UI open/close has no native registration effect; unchanged owner refreshes do no work.
        for _ in 0..5 {
            owner.refresh().unwrap();
        }
        assert_eq!(state.lock().unwrap().calls.len(), initial_calls);
        ingress.latest.lock().unwrap().position_ms += 250;
        owner.refresh().unwrap();
        assert_eq!(state.lock().unwrap().calls.len(), initial_calls + 1);
        assert!(matches!(
            state.lock().unwrap().calls.last(),
            Some(Call::Playback(_))
        ));
        {
            let mut view = ingress.latest.lock().unwrap();
            view.title = Some("Rich track".into());
            view.artist = Some("Artist".into());
            view.album = Some("Album".into());
            view.duration_ms = Some(10_000);
        }
        owner.refresh().unwrap();
        {
            let mut view = ingress.latest.lock().unwrap();
            view.generation_id = "replacement".into();
            view.title = Some("Poor track".into());
            view.artist = None;
            view.album = None;
            view.duration_ms = None;
        }
        owner.refresh().unwrap();
        assert!(state.lock().unwrap().calls.contains(&Call::Metadata(
            Some("Poor track".into()),
            None,
            None,
            None
        )));
        {
            let state = state.lock().unwrap();
            assert!(matches!(
                state.calls[state.calls.len() - 2],
                Call::Metadata(_, _, _, _)
            ));
            assert!(matches!(state.calls.last(), Some(Call::Playback(_))));
        }
        owner.detach().unwrap();
        let detached_calls = state.lock().unwrap().calls.len();
        owner.detach().unwrap();
        owner.refresh().unwrap();
        drop(owner);
        assert_eq!(state.lock().unwrap().calls.len(), detached_calls);
        assert!(!(state.lock().unwrap().callback.as_ref().unwrap())(
            souvlaki::MediaControlEvent::Play
        ));
        assert!(rx.try_recv().is_err());
        let state = state.lock().unwrap();
        assert_eq!(
            state
                .calls
                .iter()
                .filter(|call| matches!(call, Call::Attach))
                .count(),
            1
        );
        assert_eq!(
            &state.calls[state.calls.len() - 5..],
            &[
                Call::Capabilities(souvlaki::MediaControlCapabilities::default()),
                Call::Metadata(None, None, None, None),
                Call::Playback(souvlaki::MediaPlayback::Stopped),
                Call::Detach,
                Call::Drop,
            ]
        );
    }

    #[test]
    fn native_registration_and_cleanup_failures_still_remove_handlers_before_drop() {
        let (ingress, mut rx) = test_ingress();
        assert!(
            NativeMediaOwner::<TestBackend>::register_with(ingress.clone(), || Err(
                "window creation failed".into()
            ))
            .is_err()
        );
        // Native creation failure leaves the command bridge usable by the menu.
        ingress.try_send(NativeControlIntent::Stop).unwrap();
        assert_eq!(rx.try_recv().unwrap().intent, NativeControlIntent::Stop);
        let state = Arc::new(Mutex::new(AdapterState {
            fail_attach: true,
            fail_metadata: true,
            fail_detach: true,
            ..Default::default()
        }));
        let result = NativeMediaOwner::register_with(ingress, || Ok(TestBackend(state.clone())));
        let error = result.err().unwrap();
        assert!(error.contains("partial attach"));
        assert!(error.contains("detach unavailable"));
        let state = state.lock().unwrap();
        assert_eq!(
            &state.calls[state.calls.len() - 2..],
            &[Call::Detach, Call::Drop]
        );
        assert!(!(state.callback.as_ref().unwrap())(
            souvlaki::MediaControlEvent::Play
        ));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn native_owner_publishes_only_new_committed_discontinuities() {
        let (ingress, _rx) = test_ingress();
        let state = Arc::new(Mutex::new(AdapterState::default()));
        let mut owner =
            NativeMediaOwner::register_with(ingress.clone(), || Ok(TestBackend(state.clone())))
                .unwrap();
        state.lock().unwrap().calls.clear();
        {
            let mut view = ingress.latest.lock().unwrap();
            view.seeked_position_ms = Some(4_012);
            view.seeked_operation_id = Some("first-seek".into());
        }
        owner.refresh().unwrap();
        owner.refresh().unwrap();
        assert_eq!(
            state
                .lock()
                .unwrap()
                .calls
                .iter()
                .filter(|call| matches!(call, Call::Seeked(4_012_000)))
                .count(),
            1
        );
        // Publication may coalesce away pending state. Same cursor, distinct operation.
        ingress.latest.lock().unwrap().seeked_operation_id = Some("second-seek".into());
        owner.refresh().unwrap();
        owner.refresh().unwrap();
        assert_eq!(
            state
                .lock()
                .unwrap()
                .calls
                .iter()
                .filter(|call| matches!(call, Call::Seeked(4_012_000)))
                .count(),
            2
        );
    }

    #[test]
    fn duplicate_play_keeps_receipts_until_the_same_attempt_finishes() {
        let mut feedback = CommandFeedback::default();
        let loading = snapshot(PlaybackStatus::Loading, TransportState::Buffering);
        let (first, mut first_receipt) = tokio::sync::oneshot::channel();
        feedback.admitted(
            &loading,
            NativeRequest {
                intent: NativeControlIntent::Play,
                reply: Some(first),
            },
        );
        feedback.admitted(
            &loading,
            NativeRequest {
                intent: NativeControlIntent::Play,
                reply: None,
            },
        );
        let (second, mut second_receipt) = tokio::sync::oneshot::channel();
        feedback.admitted(
            &loading,
            NativeRequest {
                intent: NativeControlIntent::Play,
                reply: Some(second),
            },
        );
        assert_eq!(
            first_receipt.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        );
        assert_eq!(
            second_receipt.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        );
        let mut failed = NativePlaybackView::from_snapshot(&loading, false);
        failed.status = PlaybackStatus::Error;
        failed.failure_code = Some("RESUME_UNAVAILABLE".into());
        feedback.project(failed);
        assert_eq!(
            first_receipt.try_recv().unwrap(),
            Err("RESUME_UNAVAILABLE".into())
        );
        assert_eq!(
            second_receipt.try_recv().unwrap(),
            Err("RESUME_UNAVAILABLE".into())
        );
    }

    #[test]
    fn receipt_does_not_report_a_newer_rpc_attempts_failure() {
        let mut feedback = CommandFeedback::default();
        let loading = snapshot(PlaybackStatus::Loading, TransportState::Buffering);
        let (reply, mut receipt) = tokio::sync::oneshot::channel();
        feedback.admitted(
            &loading,
            NativeRequest {
                intent: NativeControlIntent::Play,
                reply: Some(reply),
            },
        );
        let mut failed = NativePlaybackView::from_snapshot(&loading, false);
        failed.control_epoch += 2; // RPC Pause then Resume, same generation.
        failed.status = PlaybackStatus::Error;
        failed.failure_code = Some("RESUME_UNAVAILABLE".into());
        feedback.project(failed);
        assert_eq!(receipt.try_recv().unwrap(), Ok(()));
    }
}
