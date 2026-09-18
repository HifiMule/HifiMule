use super::model::*;
mod album_admission;
pub(crate) use album_admission::{AlbumAdmission, AlbumReservation};
mod output_selection;
use crate::db::Database;
use output_selection::*;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct PlaybackError {
    pub code: &'static str,
    pub message: &'static str,
    pub conflict: bool,
    pub authoritative: Option<Box<SessionMetadata>>,
}
impl PlaybackError {
    fn invalid(code: &'static str, msg: &'static str) -> Self {
        Self {
            code,
            message: msg,
            conflict: false,
            authoritative: None,
        }
    }
    fn conflict(code: &'static str, msg: &'static str) -> Self {
        Self {
            code,
            message: msg,
            conflict: true,
            authoritative: None,
        }
    }
}
type PResult<T> = std::result::Result<T, PlaybackError>;

type PendingEvents = Arc<Mutex<VecDeque<(u64, u64, String, PlaybackEvent)>>>;

#[derive(Clone)]
pub struct PlaybackSession {
    instance_id: String,
    inner: Arc<Mutex<Inner>>,
    generation_serial: Arc<AtomicU64>,
    output_gate: Arc<AtomicBool>,
    control_epoch: Arc<AtomicU64>,
    events: PendingEvents,
    command_tx: mpsc::SyncSender<OwnerCommand>,
    control_tx: mpsc::Sender<OwnerControl>,
    #[allow(dead_code)]
    executing: Arc<AtomicBool>,
    fenced: Arc<AtomicBool>,
    ingress: Arc<Mutex<ProgressIngress>>,
    health: Arc<Mutex<PlaybackHealth>>,
    worker: Arc<Mutex<Option<std::thread::JoinHandle<()>>>>,
}

/// A transport intent from an operating-system control or daemon-owned menu.
/// It deliberately carries no session snapshot: the owner resolves the current
/// occurrence and Toggle action when this command reaches serialized execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeControlIntent {
    Play,
    Pause,
    Toggle,
    Stop,
    Next,
    SeekRelative(i64),
    SeekAbsolute(u64),
}

enum OwnerCommand {
    ReserveAlbum(
        PlayAlbumParams,
        Option<crate::sync::MutationGuard>,
        mpsc::Sender<PResult<AlbumAdmission>>,
    ),
    CommitAlbum(
        AlbumReservation,
        Vec<TrackSource>,
        super::loudness::AlbumLoudnessPolicy,
        mpsc::Sender<PResult<SessionSnapshot>>,
    ),
    ListOutputs(ListOutputsParams, mpsc::Sender<PResult<OutputList>>),
    SelectOutput(
        SelectOutputParams,
        Option<crate::sync::MutationGuard>,
        mpsc::Sender<PResult<SessionSnapshot>>,
    ),
    Snapshot(mpsc::Sender<PResult<SessionSnapshot>>),
    List(ListOccurrencesParams, mpsc::Sender<PResult<OccurrencePage>>),
    Apply(
        ApplySessionParams,
        Option<crate::sync::MutationGuard>,
        mpsc::Sender<PResult<ApplyResult>>,
    ),
    RetryRestore(
        Option<crate::sync::MutationGuard>,
        mpsc::Sender<PResult<SessionSnapshot>>,
    ),
    Control(
        ControlParams,
        Option<crate::sync::MutationGuard>,
        mpsc::Sender<PResult<SessionSnapshot>>,
    ),
    Seek(
        SeekParams,
        Option<crate::sync::MutationGuard>,
        mpsc::Sender<PResult<SessionSnapshot>>,
    ),
    NativeControl(
        NativeControlIntent,
        Option<crate::sync::MutationGuard>,
        mpsc::Sender<PResult<SessionSnapshot>>,
    ),
}

enum OwnerControl {
    #[allow(dead_code)]
    Checkpoint(mpsc::Sender<PResult<()>>),
    ShutdownCheckpoint(mpsc::Sender<PResult<()>>),
    Stop,
}

struct Inner {
    album: album_admission::AlbumAdmissionState,
    pending_terminal: Option<PendingTerminal>,
    output: OutputState,
    outputs: Option<OutputRuntime>,
    output_dedup: HashMap<String, (SelectOutputParams, PResult<SessionSnapshot>)>,
    output_dedup_order: VecDeque<String>,
    output_gate: Arc<AtomicBool>,
    control_epoch: Arc<AtomicU64>,
    db: Arc<Database>,
    instance_id: String,
    session: PersistedSession,
    generation_id: String,
    state_sequence: u64,
    restoration: Status,
    persistence: Status,
    dedup: HashMap<String, DedupEntry>,
    dedup_order: VecDeque<String>,
    control_dedup: HashMap<String, (ControlParams, PResult<SessionSnapshot>)>,
    control_dedup_order: VecDeque<String>,
    seek_dedup: HashMap<String, (SeekParams, PResult<SessionSnapshot>)>,
    seek_dedup_order: VecDeque<String>,
    seek_resume_after_commit: bool,
    dirty: bool,
    checkpointed_position_ms: u64,
    playback: PlaybackState,
}
#[derive(Clone)]
struct PendingTerminal {
    instance_id: String,
    generation_id: String,
    occurrence_id: String,
    session: PersistedSession,
    outcome: &'static str,
    status: PlaybackStatus,
    failure: Option<PlaybackFailure>,
    resolve_successor: bool,
}

#[derive(Clone)]
struct DedupEntry {
    at: Instant,
    payload: ApplySessionParams,
    result: PResult<ApplyResult>,
}
// This lock is never held during database work. Producers only try_lock it.
struct ProgressIngress {
    generation_id: String,
    occurrence_id: Option<String>,
    sequence: Option<u64>,
    position_ms: u64,
    pending: Option<(u64, u64)>,
}

struct OwnerResources {
    events: PendingEvents,
    inner: Arc<Mutex<Inner>>,
    executing: Arc<AtomicBool>,
    ingress: Arc<Mutex<ProgressIngress>>,
    fenced: Arc<AtomicBool>,
    health: Arc<Mutex<PlaybackHealth>>,
    generation_serial: Arc<AtomicU64>,
}

impl PlaybackSession {
    pub(crate) fn successor_candidate(
        &self,
        generation_id: &str,
        control_epoch: u64,
    ) -> Option<super::continuity::SuccessorCandidate> {
        let inner = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        if inner.generation_id != generation_id
            || inner.control_epoch.load(Ordering::Acquire) != control_epoch
            || inner.session.state == TransportState::Idle
        {
            return None;
        }
        let predecessor_id = inner.session.current_occurrence_id.as_ref()?;
        let predecessor = inner
            .db
            .playback_occurrence(&inner.session.session_id, predecessor_id)
            .ok()??;
        let successor = inner
            .db
            .playback_successor(&inner.session.session_id, predecessor.ordinal)
            .ok()??;
        let gain_bits = inner
            .session
            .album_context
            .as_ref()
            .map_or(1.0f32.to_bits(), |context| {
                context.scalar_for(&successor).to_bits()
            });
        Some(super::continuity::SuccessorCandidate {
            instance_id: inner.instance_id.clone(),
            session_id: inner.session.session_id.clone(),
            predecessor_occurrence_id: predecessor_id.clone(),
            successor,
            queue_revision: inner.session.queue_revision,
            control_epoch,
            preparation_generation: self.generation_serial.load(Ordering::Acquire),
            gain_bits,
        })
    }

    pub fn restore(db: Arc<Database>, instance_id: String) -> Self {
        let owner_id = instance_id.clone();
        let loaded = db.load_playback_session();
        let (session, restoration) = match loaded {
            Ok(Some(mut s)) => {
                let valid =
                    Uuid::parse_str(&s.session_id).is_ok() && validate_stored(&db, &s).is_ok();
                if valid {
                    if s.state != TransportState::Idle {
                        s.state = TransportState::Paused;
                    }
                    (
                        s,
                        Status {
                            status: "ok".into(),
                            code: None,
                        },
                    )
                } else {
                    (
                        fresh_session(),
                        Status {
                            status: "error".into(),
                            code: Some("INVALID_SESSION".into()),
                        },
                    )
                }
            }
            Ok(None) => {
                let s = fresh_session();
                let status = match db.persist_playback_structure(&s, &[]) {
                    Ok(()) => Status {
                        status: "ok".into(),
                        code: None,
                    },
                    Err(_) => Status {
                        status: "error".into(),
                        code: Some("PERSISTENCE_FAILED".into()),
                    },
                };
                (s, status)
            }
            Err(e) => {
                let code = if e.to_string().contains("UNSUPPORTED_PLAYBACK_VERSION") {
                    "UNSUPPORTED_PLAYBACK_VERSION"
                } else {
                    "RESTORE_FAILED"
                };
                (
                    fresh_session(),
                    Status {
                        status: "error".into(),
                        code: Some(code.into()),
                    },
                )
            }
        };
        let checkpointed_position_ms = session.position_ms;
        let restored_playback_status = if session.current_occurrence_id.is_some() {
            PlaybackStatus::Paused
        } else {
            PlaybackStatus::Idle
        };
        let output_gate = Arc::new(AtomicBool::new(false));
        let control_epoch = Arc::new(AtomicU64::new(0));
        let inner = Arc::new(Mutex::new(Inner {
            album: Default::default(),
            pending_terminal: None,
            output: OutputState::default(),
            outputs: None,
            output_dedup: HashMap::new(),
            output_dedup_order: VecDeque::new(),
            output_gate: output_gate.clone(),
            control_epoch: control_epoch.clone(),
            db,
            instance_id,
            session,
            generation_id: Uuid::new_v4().to_string(),
            state_sequence: 0,
            persistence: Status {
                status: "ok".into(),
                code: None,
            },
            restoration,
            dedup: HashMap::new(),
            dedup_order: VecDeque::new(),
            control_dedup: HashMap::new(),
            control_dedup_order: VecDeque::new(),
            seek_dedup: HashMap::new(),
            seek_dedup_order: VecDeque::new(),
            seek_resume_after_commit: false,
            dirty: false,
            checkpointed_position_ms,
            playback: PlaybackState {
                status: restored_playback_status,
                ..Default::default()
            },
        }));
        let (ingress, health) = {
            let i = inner.lock().unwrap_or_else(|e| e.into_inner());
            (
                Arc::new(Mutex::new(ProgressIngress {
                    generation_id: i.generation_id.clone(),
                    occurrence_id: i.session.current_occurrence_id.clone(),
                    sequence: None,
                    position_ms: i.session.position_ms,
                    pending: None,
                })),
                Arc::new(Mutex::new(PlaybackHealth {
                    restoration: i.restoration.clone(),
                    persistence: i.persistence.clone(),
                })),
            )
        };
        let fenced = Arc::new(AtomicBool::new(false));
        let generation_serial = Arc::new(AtomicU64::new(0));
        let (command_tx, command_rx) = mpsc::sync_channel(64);
        let events = Arc::new(Mutex::new(VecDeque::with_capacity(3)));
        let (control_tx, control_rx) = mpsc::channel();
        let worker_inner = Arc::clone(&inner);
        let executing = Arc::new(AtomicBool::new(false));
        let worker_executing = Arc::clone(&executing);
        let worker_ingress = ingress.clone();
        let worker_fenced = fenced.clone();
        let worker_health = health.clone();
        let resources = OwnerResources {
            events: events.clone(),
            inner: worker_inner,
            executing: worker_executing,
            ingress: worker_ingress,
            fenced: worker_fenced,
            health: worker_health,
            generation_serial: generation_serial.clone(),
        };
        let worker = std::thread::Builder::new()
            .name("hifimule-playback-owner".into())
            .spawn(move || owner_loop(resources, command_rx, control_rx))
            .expect("playback owner thread must start");
        Self {
            instance_id: owner_id,
            output_gate,
            control_epoch,
            events,
            inner,
            generation_serial,
            command_tx,
            control_tx,
            executing,
            fenced,
            ingress,
            health,
            worker: Arc::new(Mutex::new(Some(worker))),
        }
    }

    pub fn snapshot(&self) -> PResult<SessionSnapshot> {
        let (tx, rx) = mpsc::channel();
        self.command_tx
            .try_send(OwnerCommand::Snapshot(tx))
            .map_err(admission_error)?;
        rx.recv().unwrap_or_else(|_| Err(owner_stopped()))
    }

    pub fn list(&self, p: ListOccurrencesParams) -> PResult<OccurrencePage> {
        let (tx, rx) = mpsc::channel();
        self.command_tx
            .try_send(OwnerCommand::List(p, tx))
            .map_err(admission_error)?;
        rx.recv().unwrap_or_else(|_| Err(owner_stopped()))
    }

    pub fn health(&self) -> PlaybackHealth {
        self.health
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn instance_id(&self) -> String {
        self.instance_id.clone()
    }

    #[allow(dead_code)]
    pub fn apply(&self, p: ApplySessionParams) -> PResult<ApplyResult> {
        self.apply_with_guard(p, None)
    }

    pub fn apply_with_guard(
        &self,
        p: ApplySessionParams,
        guard: Option<crate::sync::MutationGuard>,
    ) -> PResult<ApplyResult> {
        self.admit_apply(p, guard)?
            .recv()
            .unwrap_or_else(|_| Err(owner_stopped()))
    }

    pub fn control_with_guard(
        &self,
        p: ControlParams,
        guard: Option<crate::sync::MutationGuard>,
    ) -> PResult<SessionSnapshot> {
        if self.fenced.load(Ordering::Acquire) {
            return Err(owner_stopped());
        }
        let (tx, rx) = mpsc::channel();
        self.command_tx
            .try_send(OwnerCommand::Control(p, guard, tx))
            .map_err(admission_error)?;
        rx.recv().unwrap_or_else(|_| Err(owner_stopped()))
    }

    pub fn native_control(
        &self,
        intent: NativeControlIntent,
        guard: Option<crate::sync::MutationGuard>,
    ) -> PResult<SessionSnapshot> {
        if self.fenced.load(Ordering::Acquire) {
            return Err(owner_stopped());
        }
        let (tx, rx) = mpsc::channel();
        self.command_tx
            .try_send(OwnerCommand::NativeControl(intent, guard, tx))
            .map_err(admission_error)?;
        rx.recv().unwrap_or_else(|_| Err(owner_stopped()))
    }

    pub fn seek_with_guard(
        &self,
        p: SeekParams,
        guard: Option<crate::sync::MutationGuard>,
    ) -> PResult<SessionSnapshot> {
        if self.fenced.load(Ordering::Acquire) {
            return Err(owner_stopped());
        }
        let (tx, rx) = mpsc::channel();
        self.command_tx
            .try_send(OwnerCommand::Seek(p, guard, tx))
            .map_err(admission_error)?;
        rx.recv().unwrap_or_else(|_| Err(owner_stopped()))
    }

    pub fn publish_event(&self, generation_id: String, event: PlaybackEvent) {
        self.publish_event_at_epoch(generation_id, event, self.control_epoch());
    }

    pub(crate) fn publish_event_at_epoch(
        &self,
        generation_id: String,
        event: PlaybackEvent,
        epoch: u64,
    ) {
        let Some((_, serial)) = self.generation_guard(&generation_id) else {
            return;
        };
        let mut events = self
            .events
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        enqueue_event(&mut events, serial, epoch, generation_id, event);
    }

    /// Installed output workers already own a serial-fenced generation. Avoid
    /// taking the owner lock while it waits for a native pause acknowledgment.
    pub(crate) fn publish_pipeline_event(
        &self,
        serial: u64,
        generation_id: String,
        event: PlaybackEvent,
        epoch: u64,
    ) {
        if self.fenced.load(Ordering::Acquire)
            || self.generation_serial.load(Ordering::Acquire) != serial
        {
            return;
        }
        let mut events = self.events.lock().unwrap_or_else(|e| e.into_inner());
        if self.generation_serial.load(Ordering::Acquire) != serial {
            return;
        }
        enqueue_event(&mut events, serial, epoch, generation_id, event);
    }

    pub(crate) fn output_gate(&self) -> Arc<AtomicBool> {
        self.output_gate.clone()
    }
    pub(crate) fn control_epoch(&self) -> u64 {
        self.control_epoch.load(Ordering::Acquire)
    }

    pub(crate) fn is_fenced(&self) -> bool {
        self.fenced.load(Ordering::Acquire)
    }

    pub(crate) fn generation_guard(&self, generation_id: &str) -> Option<(Arc<AtomicU64>, u64)> {
        let inner = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        if inner.generation_id != generation_id || self.fenced.load(Ordering::Acquire) {
            return None;
        }
        let serial = self.generation_serial.load(Ordering::Acquire);
        Some((self.generation_serial.clone(), serial))
    }

    pub(crate) fn with_current_generation<R>(
        &self,
        generation_id: &str,
        action: impl FnOnce() -> R,
    ) -> Option<R> {
        let inner = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        if inner.generation_id != generation_id || self.fenced.load(Ordering::Acquire) {
            return None;
        }
        Some(action())
    }

    #[allow(dead_code)]
    pub fn retry_restore(&self) -> PResult<SessionSnapshot> {
        self.retry_restore_with_guard(None)
    }

    pub fn retry_restore_with_guard(
        &self,
        guard: Option<crate::sync::MutationGuard>,
    ) -> PResult<SessionSnapshot> {
        if self.fenced.load(Ordering::Acquire) {
            return Err(owner_stopped());
        }
        let (tx, rx) = mpsc::channel();
        self.command_tx
            .try_send(OwnerCommand::RetryRestore(guard, tx))
            .map_err(admission_error)?;
        rx.recv().unwrap_or_else(|_| Err(owner_stopped()))
    }

    #[allow(dead_code)]
    pub fn report_progress(
        &self,
        generation_id: &str,
        occurrence_id: &str,
        sequence: u64,
        position_ms: u64,
    ) -> PResult<()> {
        if position_ms > 9_007_199_254_740_991 {
            return Err(PlaybackError::invalid(
                "STALE_PROGRESS",
                "position is out of range",
            ));
        }
        let mut p = self
            .ingress
            .try_lock()
            .map_err(|_| PlaybackError::conflict("PLAYBACK_BUSY", "progress slot is busy"))?;
        if self.fenced.load(Ordering::Acquire) {
            return Err(owner_stopped());
        }
        if p.generation_id != generation_id
            || p.occurrence_id.as_deref() != Some(occurrence_id)
            || p.sequence.is_some_and(|last| sequence <= last)
            || position_ms < p.position_ms
        {
            return Err(PlaybackError::conflict(
                "STALE_PROGRESS",
                "progress sample is stale",
            ));
        }
        p.sequence = Some(sequence);
        p.position_ms = position_ms;
        p.pending = Some((sequence, position_ms));
        Ok(())
    }

    #[allow(dead_code)]
    pub fn final_checkpoint(&self) -> PResult<()> {
        let (tx, rx) = mpsc::channel();
        self.control_tx
            .send(OwnerControl::Checkpoint(tx))
            .map_err(|_| owner_stopped())?;
        rx.recv().unwrap_or_else(|_| Err(owner_stopped()))
    }

    /// Fence ingress immediately; the reply establishes completion of the frozen write.
    pub fn begin_shutdown_checkpoint(&self) -> PResult<mpsc::Receiver<PResult<()>>> {
        self.output_gate.store(false, Ordering::Release);
        self.generation_serial.fetch_add(1, Ordering::AcqRel);
        {
            let _ingress = self.ingress.lock().unwrap_or_else(|e| e.into_inner());
            self.fenced.store(true, Ordering::Release);
        }
        let (tx, rx) = mpsc::channel();
        self.control_tx
            .send(OwnerControl::ShutdownCheckpoint(tx))
            .map_err(|_| owner_stopped())?;
        Ok(rx)
    }

    #[allow(dead_code)]
    pub fn shutdown_checkpoint(&self) -> PResult<()> {
        self.begin_shutdown_checkpoint()?
            .recv()
            .unwrap_or_else(|_| Err(owner_stopped()))
    }

    pub fn stop_and_join(&self) -> PResult<()> {
        self.fenced.store(true, Ordering::Release);
        self.output_gate.store(false, Ordering::Release);
        self.stop_output_workers();
        self.output_gate.store(false, Ordering::Release);
        self.generation_serial.fetch_add(1, Ordering::AcqRel);
        let mut worker = self.worker.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(thread) = worker.take() {
            self.fenced.store(true, Ordering::Release);
            let _ = self.control_tx.send(OwnerControl::Stop);
            thread.join().map_err(|_| owner_stopped())?;
        }
        Ok(())
    }

    fn admit_apply(
        &self,
        params: ApplySessionParams,
        guard: Option<crate::sync::MutationGuard>,
    ) -> PResult<mpsc::Receiver<PResult<ApplyResult>>> {
        if self.fenced.load(Ordering::Acquire) {
            return Err(owner_stopped());
        }
        let (tx, rx) = mpsc::channel();
        self.command_tx
            .try_send(OwnerCommand::Apply(params, guard, tx))
            .map_err(admission_error)?;
        Ok(rx)
    }
}

fn owner_stopped() -> PlaybackError {
    PlaybackError::conflict("DAEMON_STOPPED", "daemon shutdown rejected playback work")
}

fn list_inner(inner: &Inner, p: ListOccurrencesParams) -> PResult<OccurrencePage> {
    require_schema(p.schema_version)?;
    if p.session_id != inner.session.session_id {
        return Err(PlaybackError::conflict(
            "SESSION_MISMATCH",
            "session identity is stale",
        ));
    }
    if parse_revision(&p.expected_queue_revision)? != inner.session.queue_revision {
        return Err(PlaybackError::conflict(
            "QUEUE_REVISION_CONFLICT",
            "queue revision is stale",
        ));
    }
    let limit = p.limit.unwrap_or(DEFAULT_PAGE_SIZE);
    if !(1..=MAX_PAGE_SIZE).contains(&limit) {
        return Err(PlaybackError::invalid(
            "INVALID_CURSOR",
            "limit must be between 1 and 200",
        ));
    }
    let after = match p.cursor {
        Some(c) => Some(decode_cursor(&c, &inner.session)?),
        None => None,
    };
    let mut rows = inner
        .db
        .playback_page(&inner.session.session_id, after, limit + 1)
        .map_err(storage)?;
    let next = if rows.len() > limit {
        rows.truncate(limit);
        rows.last()
            .map(|o| encode_cursor(&inner.session, o.ordinal))
    } else {
        None
    };
    decorate_availability(&inner.db, &mut rows)?;
    Ok(OccurrencePage {
        occurrences: rows,
        next_cursor: next,
        total_occurrence_count: inner
            .db
            .playback_count(&inner.session.session_id)
            .map_err(storage)?,
    })
}

fn admission_error<T>(error: mpsc::TrySendError<T>) -> PlaybackError {
    match error {
        mpsc::TrySendError::Full(_) => {
            PlaybackError::conflict("PLAYBACK_BUSY", "playback command mailbox is full")
        }
        mpsc::TrySendError::Disconnected(_) => {
            PlaybackError::conflict("PLAYBACK_BUSY", "playback owner is unavailable")
        }
    }
}

fn owner_loop(
    resources: OwnerResources,
    command_rx: mpsc::Receiver<OwnerCommand>,
    control_rx: mpsc::Receiver<OwnerControl>,
) {
    let OwnerResources {
        events,
        inner,
        executing,
        ingress,
        fenced,
        health,
        generation_serial,
    } = resources;
    let mut last_periodic_checkpoint = Instant::now();
    let mut last_sample = Instant::now();
    loop {
        while let Ok(control) = control_rx.try_recv() {
            match control {
                OwnerControl::Stop => {
                    album_admission::cancel_pending(
                        &mut inner.lock().unwrap_or_else(|e| e.into_inner()),
                    );
                    while let Ok(command) = command_rx.try_recv() {
                        reject_unstarted(command);
                    }
                    return;
                }
                OwnerControl::Checkpoint(reply) | OwnerControl::ShutdownCheckpoint(reply) => {
                    // The atomic fence also covers callers admitted before shutdown but
                    // still waiting for a blocking-pool thread to enqueue their command.
                    if fenced.load(Ordering::Acquire) {
                        while let Ok(command) = command_rx.try_recv() {
                            reject_unstarted(command);
                        }
                    }
                    let mut i = inner.lock().unwrap_or_else(|e| e.into_inner());
                    if fenced.load(Ordering::Acquire) {
                        album_admission::cancel_pending(&mut i);
                    }
                    let result =
                        sample_progress(&mut i, &ingress).and_then(|()| checkpoint_inner(&mut i));
                    publish_health(&i, &health);
                    let _ = reply.send(result);
                }
            }
        }
        {
            let mut i = inner.lock().unwrap_or_else(|e| e.into_inner());
            album_admission::prune(&mut i, fenced.load(Ordering::Acquire));
            reconcile_outputs(
                &mut i,
                &ingress,
                &generation_serial,
                fenced.load(Ordering::Acquire),
            );
            let previous = i.session.current_occurrence_id.clone();
            let _ = reconcile_presented_handoff(&mut i, &generation_serial);
            if previous != i.session.current_occurrence_id {
                reset_ingress(&i, &ingress);
            }
            if i.pending_terminal
                .as_ref()
                .is_some_and(|pending| pending.generation_id != i.generation_id)
            {
                i.pending_terminal = None;
            }
        }
        let command = command_rx.recv_timeout(Duration::from_millis(25));
        let pending: Vec<_> = events
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .drain(..)
            .collect();
        if !pending.is_empty() {
            let mut i = inner.lock().unwrap_or_else(|error| error.into_inner());
            for (_, epoch, generation_id, event) in pending {
                if matches!(event_kind(&event), 1 | 3 | 4 | 5)
                    && epoch != i.control_epoch.load(Ordering::Acquire)
                {
                    continue;
                }
                if !fenced.load(Ordering::Acquire) && i.generation_id == generation_id {
                    let event = match event {
                        PlaybackEvent::SeekPipelineFailed {
                            operation_id,
                            code,
                            retryable,
                        } => {
                            if i.playback
                                .pending_seek
                                .as_ref()
                                .is_some_and(|p| p.operation_id == operation_id)
                            {
                                PlaybackEvent::SeekFailed {
                                    operation_id,
                                    code: "SEEK_FAILED".into(),
                                    retryable: true,
                                }
                            } else if i.playback.seek_outcome.as_ref().is_some_and(|s| {
                                s.operation_id == operation_id && s.status == "succeeded"
                            }) {
                                PlaybackEvent::Failed { code, retryable }
                            } else {
                                continue;
                            }
                        }
                        event => event,
                    };
                    let checkpoint_seek = matches!(&event, PlaybackEvent::SeekCommitted { operation_id, .. }
                        if i.playback.pending_seek.as_ref().is_some_and(|p| &p.operation_id == operation_id));
                    let output_lost = matches!(&event, PlaybackEvent::Failed { code, .. } if code == "OUTPUT_LOST");
                    if i.pending_terminal.is_some() {
                        continue;
                    }
                    if let PlaybackEvent::HandoffPresented {
                        token,
                        metadata,
                        duration_ms,
                        representation,
                        successor_offset_frames,
                        sample_rate,
                        seek,
                    } = &event
                    {
                        let _ = adopt_presented_handoff(
                            &mut i,
                            token.clone(),
                            metadata.clone(),
                            *duration_ms,
                            representation.clone(),
                            *successor_offset_frames,
                            *sample_rate,
                            seek.clone(),
                            &generation_serial,
                        );
                        reset_ingress(&i, &ingress);
                        publish_health(&i, &health);
                        continue;
                    }
                    if let PlaybackEvent::Failed { code, retryable } = &event
                        && code != "OUTPUT_LOST"
                        && code != "OUTPUT_RETIREMENT_PENDING"
                        && i.session.current_occurrence_id.is_some()
                    {
                        if i.playback.status == PlaybackStatus::Error {
                            continue;
                        }
                        let _ = sample_progress(&mut i, &ingress);
                        let mut terminal =
                            terminal_plan(&i, "technicalFailure", PlaybackStatus::Error);
                        terminal.session.state = TransportState::Paused;
                        terminal.failure = Some(PlaybackFailure {
                            code: code.clone(),
                            retryable: *retryable,
                        });
                        if commit_terminal(&mut i, terminal, &generation_serial, false).is_err() {
                            reset_ingress(&i, &ingress);
                            publish_health(&i, &health);
                            continue;
                        }
                    }
                    if let PlaybackEvent::Completed { position_ms } = event {
                        if matches!(
                            i.playback.status,
                            PlaybackStatus::Completed | PlaybackStatus::Error
                        ) {
                            continue;
                        }
                        match complete_occurrence(&mut i, position_ms, &generation_serial) {
                            Ok(true) => {
                                reset_ingress(&i, &ingress);
                                publish_health(&i, &health);
                                continue;
                            }
                            Ok(false) => {
                                reset_ingress(&i, &ingress);
                                publish_health(&i, &health);
                                continue;
                            }
                            Err(_) => {
                                if i.pending_terminal.is_none() {
                                    let mut terminal = terminal_plan(
                                        &i,
                                        "naturalCompletion",
                                        PlaybackStatus::Completed,
                                    );
                                    terminal.session.position_ms = position_ms;
                                    terminal.resolve_successor = true;
                                    freeze_terminal(&mut i, terminal);
                                }
                                reset_ingress(&i, &ingress);
                                publish_health(&i, &health);
                                continue;
                            }
                        }
                    }
                    if output_lost {
                        i.output_gate.store(false, Ordering::Release);
                        let _ = sample_progress(&mut i, &ingress);
                        if let Some(position) =
                            super::audio::global().captured_position(&generation_id)
                        {
                            i.session.position_ms = i.session.position_ms.max(position);
                        }
                        supersede_pending_seek(&mut i);
                        generation_serial.fetch_add(1, Ordering::AcqRel);
                        super::audio::global().control(ControlAction::Stop);
                        i.generation_id = Uuid::new_v4().to_string();
                        i.dirty = true;
                    }
                    let previous_position = i.session.position_ms;
                    apply_playback_event(&mut i, event);
                    if checkpoint_seek || i.session.position_ms < previous_position {
                        reset_ingress(&i, &ingress);
                    } else {
                        refresh_ingress(&i, &ingress);
                    }
                    if output_lost || checkpoint_seek {
                        let _ = checkpoint_inner(&mut i);
                    }
                }
            }
        }
        match command {
            Ok(OwnerCommand::ReserveAlbum(params, guard, reply)) => {
                let mut i = inner.lock().unwrap_or_else(|e| e.into_inner());
                let result = if fenced.load(Ordering::Acquire) {
                    Err(owner_stopped())
                } else {
                    album_admission::reserve(&mut i, params, guard)
                };
                let _ = reply.send(with_metadata(result, &i));
            }
            Ok(OwnerCommand::CommitAlbum(reservation, sources, policy, reply)) => {
                let mut i = inner.lock().unwrap_or_else(|e| e.into_inner());
                let result = if fenced.load(Ordering::Acquire) {
                    Err(owner_stopped())
                } else {
                    album_admission::commit(
                        &mut i,
                        reservation,
                        sources,
                        policy,
                        &generation_serial,
                    )
                };
                if result.is_ok() {
                    reset_ingress(&i, &ingress);
                }
                publish_health(&i, &health);
                let _ = reply.send(with_metadata(result, &i));
            }
            Ok(OwnerCommand::ListOutputs(params, reply)) => {
                let i = inner.lock().unwrap_or_else(|e| e.into_inner());
                let result = list_outputs(&i, params);
                let _ = reply.send(with_metadata(result, &i));
            }
            Ok(OwnerCommand::SelectOutput(params, _guard, reply)) => {
                let mut i = inner.lock().unwrap_or_else(|e| e.into_inner());
                let result = if fenced.load(Ordering::Acquire) {
                    Err(owner_stopped())
                } else {
                    select_output(&mut i, params, &ingress, &generation_serial)
                };
                refresh_ingress(&i, &ingress);
                let _ = reply.send(with_metadata(result, &i));
            }
            Ok(OwnerCommand::Snapshot(reply)) => {
                let i = inner.lock().unwrap_or_else(|e| e.into_inner());
                let _ = reply.send(with_metadata(snapshot(&i), &i));
            }
            Ok(OwnerCommand::List(params, reply)) => {
                let i = inner.lock().unwrap_or_else(|e| e.into_inner());
                let _ = reply.send(with_metadata(list_inner(&i, params), &i));
            }
            Ok(OwnerCommand::Apply(params, _mutation_guard, reply)) => {
                executing.store(true, Ordering::Release);
                let mut i = inner.lock().unwrap_or_else(|e| e.into_inner());
                let result = if fenced.load(Ordering::Acquire) {
                    Err(owner_stopped())
                } else {
                    prune_dedup(&mut i);
                    if album_admission::contains_command(&i, &params.command_id)
                        || i.control_dedup.contains_key(&params.command_id)
                        || i.output_dedup.contains_key(&params.command_id)
                        || i.seek_dedup.contains_key(&params.command_id)
                    {
                        Err(PlaybackError::conflict(
                            "COMMAND_ID_REUSED",
                            "command identity was reused with another payload",
                        ))
                    } else if let Some(old) = i.dedup.get(&params.command_id) {
                        if old.payload == params {
                            old.result.clone().map(|mut result| {
                                result.start_audio = false;
                                result
                            })
                        } else {
                            Err(PlaybackError::conflict(
                                "COMMAND_ID_REUSED",
                                "command identity was reused with another payload",
                            ))
                        }
                    } else {
                        let result = sample_progress_if_due(
                            &mut i,
                            &ingress,
                            &mut last_sample,
                            Instant::now(),
                        )
                        .and_then(|()| apply_inner(&mut i, &params, &generation_serial));
                        if result.is_ok() {
                            refresh_ingress(&i, &ingress);
                        }
                        retain_dedup(&mut i, params, result.clone());
                        result
                    }
                };
                let result = with_metadata(result, &i).map(|mut result| {
                    result.current_metadata = metadata(&i);
                    result
                });
                publish_health(&i, &health);
                let _ = reply.send(result);
                executing.store(false, Ordering::Release);
            }
            Ok(OwnerCommand::Control(params, _mutation_guard, reply)) => {
                executing.store(true, Ordering::Release);
                let mut i = inner.lock().unwrap_or_else(|e| e.into_inner());
                prune_dedup(&mut i);
                let result = if fenced.load(Ordering::Acquire) {
                    Err(owner_stopped())
                } else if album_admission::contains_command(&i, &params.command_id)
                    || i.dedup.contains_key(&params.command_id)
                    || i.output_dedup.contains_key(&params.command_id)
                    || i.seek_dedup.contains_key(&params.command_id)
                {
                    Err(PlaybackError::conflict(
                        "COMMAND_ID_REUSED",
                        "command identity was reused with another payload",
                    ))
                } else if let Some((old, result)) = i.control_dedup.get(&params.command_id) {
                    if old == &params {
                        result.clone().map(|mut snapshot| {
                            snapshot.resume_audio = false;
                            snapshot
                        })
                    } else {
                        Err(PlaybackError::conflict(
                            "COMMAND_ID_REUSED",
                            "command identity was reused with another payload",
                        ))
                    }
                } else {
                    let result = sample_progress(&mut i, &ingress)
                        .and_then(|()| control_inner(&mut i, &params, &generation_serial));
                    i.control_dedup
                        .insert(params.command_id.clone(), (params.clone(), result.clone()));
                    i.control_dedup_order.push_back(params.command_id.clone());
                    while i.control_dedup_order.len() > 1024 {
                        if let Some(id) = i.control_dedup_order.pop_front() {
                            i.control_dedup.remove(&id);
                        }
                    }
                    result
                };
                if result.is_ok() {
                    refresh_ingress(&i, &ingress);
                }
                publish_health(&i, &health);
                let _ = reply.send(with_metadata(result, &i));
                executing.store(false, Ordering::Release);
            }
            Ok(OwnerCommand::Seek(params, _mutation_guard, reply)) => {
                executing.store(true, Ordering::Release);
                let mut i = inner.lock().unwrap_or_else(|e| e.into_inner());
                prune_dedup(&mut i);
                let result = if fenced.load(Ordering::Acquire) {
                    Err(owner_stopped())
                } else if album_admission::contains_command(&i, &params.command_id)
                    || i.dedup.contains_key(&params.command_id)
                    || i.output_dedup.contains_key(&params.command_id)
                    || i.control_dedup.contains_key(&params.command_id)
                {
                    Err(PlaybackError::conflict(
                        "COMMAND_ID_REUSED",
                        "command identity was reused with another payload",
                    ))
                } else if let Some((old, result)) = i.seek_dedup.get(&params.command_id) {
                    if old == &params {
                        result.clone().map(|mut snapshot| {
                            snapshot.seek_audio = false;
                            snapshot
                        })
                    } else {
                        Err(PlaybackError::conflict(
                            "COMMAND_ID_REUSED",
                            "command identity was reused with another payload",
                        ))
                    }
                } else {
                    let result = sample_progress(&mut i, &ingress)
                        .and_then(|()| seek_inner(&mut i, &params, &generation_serial));
                    i.seek_dedup
                        .insert(params.command_id.clone(), (params.clone(), result.clone()));
                    i.seek_dedup_order.push_back(params.command_id.clone());
                    while i.seek_dedup_order.len() > 1024 {
                        if let Some(id) = i.seek_dedup_order.pop_front() {
                            i.seek_dedup.remove(&id);
                        }
                    }
                    result
                };
                if result.is_ok() {
                    refresh_ingress(&i, &ingress);
                }
                publish_health(&i, &health);
                let _ = reply.send(with_metadata(result, &i));
                executing.store(false, Ordering::Release);
            }
            Ok(OwnerCommand::NativeControl(intent, _mutation_guard, reply)) => {
                executing.store(true, Ordering::Release);
                let mut i = inner.lock().unwrap_or_else(|e| e.into_inner());
                let result = if fenced.load(Ordering::Acquire) {
                    Err(owner_stopped())
                } else {
                    sample_progress(&mut i, &ingress)
                        .and_then(|()| native_control_inner(&mut i, intent, &generation_serial))
                };
                if result.is_ok() {
                    refresh_ingress(&i, &ingress);
                }
                publish_health(&i, &health);
                let _ = reply.send(with_metadata(result, &i));
                executing.store(false, Ordering::Release);
            }
            Ok(OwnerCommand::RetryRestore(_mutation_guard, reply)) => {
                executing.store(true, Ordering::Release);
                let mut i = inner.lock().unwrap_or_else(|e| e.into_inner());
                let result = if fenced.load(Ordering::Acquire) {
                    Err(owner_stopped())
                } else {
                    retry_restore_inner(&mut i, &generation_serial)
                };
                if result.is_ok() {
                    refresh_ingress(&i, &ingress);
                }
                publish_health(&i, &health);
                let _ = reply.send(with_metadata(result, &i));
                executing.store(false, Ordering::Release);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
        if !fenced.load(Ordering::Acquire)
            && (last_sample.elapsed() >= Duration::from_millis(250)
                || last_periodic_checkpoint.elapsed() >= Duration::from_secs(5))
        {
            let mut i = inner.lock().unwrap_or_else(|e| e.into_inner());
            if last_sample.elapsed() >= Duration::from_millis(250) {
                let _ = sample_progress_if_due(&mut i, &ingress, &mut last_sample, Instant::now());
            }
            if last_periodic_checkpoint.elapsed() >= Duration::from_secs(5) {
                let _ = checkpoint_inner(&mut i);
                last_periodic_checkpoint = Instant::now();
            }
            publish_health(&i, &health);
        }
    }
}

fn sample_progress_if_due(
    i: &mut Inner,
    ingress: &Mutex<ProgressIngress>,
    last: &mut Instant,
    now: Instant,
) -> PResult<()> {
    if now.saturating_duration_since(*last) >= Duration::from_millis(250) {
        sample_progress(i, ingress)?;
        *last = now;
    }
    Ok(())
}

fn sample_progress(i: &mut Inner, ingress: &Mutex<ProgressIngress>) -> PResult<()> {
    let mut p = ingress.lock().unwrap_or_else(|e| e.into_inner());
    if i.pending_terminal.is_some()
        || matches!(
            i.playback.status,
            PlaybackStatus::Completed | PlaybackStatus::Error
        )
    {
        p.pending = None;
        return Ok(());
    }
    if let Some((_, position)) = p.pending {
        let next = i
            .state_sequence
            .checked_add(1)
            .filter(|n| *n <= i64::MAX as u64)
            .ok_or_else(|| PlaybackError::invalid("INVALID_SESSION", "state sequence overflow"))?;
        p.pending = None;
        if p.generation_id == i.generation_id && p.occurrence_id == i.session.current_occurrence_id
        {
            i.session.position_ms = position;
            i.state_sequence = next;
            i.dirty = true;
        }
    }
    Ok(())
}

fn refresh_ingress(i: &Inner, ingress: &Mutex<ProgressIngress>) {
    let mut p = ingress.lock().unwrap_or_else(|e| e.into_inner());
    if p.generation_id != i.generation_id
        || p.occurrence_id != i.session.current_occurrence_id
        || (i.session.state == TransportState::Paused && p.position_ms > i.session.position_ms)
    {
        reset_progress(i, &mut p);
    }
}
fn reset_ingress(i: &Inner, ingress: &Mutex<ProgressIngress>) {
    reset_progress(i, &mut ingress.lock().unwrap_or_else(|e| e.into_inner()));
}
fn reset_progress(i: &Inner, p: &mut ProgressIngress) {
    p.generation_id = i.generation_id.clone();
    p.occurrence_id = i.session.current_occurrence_id.clone();
    p.sequence = None;
    p.position_ms = i.session.position_ms;
    p.pending = None;
}

// Output transitions abandon preparation without carrying its intent into a new generation.
fn supersede_pending_seek(i: &mut Inner) {
    if let Some(pending) = i.playback.pending_seek.take() {
        i.playback.seek_outcome = Some(SeekOutcome {
            operation_id: pending.operation_id,
            requested_position_ms: pending.requested_position_ms,
            actual_position_ms: None,
            status: "superseded".into(),
            error: None,
        });
    }
    i.seek_resume_after_commit = false;
}
fn publish_health(i: &Inner, health: &Mutex<PlaybackHealth>) {
    *health.lock().unwrap_or_else(|e| e.into_inner()) = PlaybackHealth {
        restoration: i.restoration.clone(),
        persistence: i.persistence.clone(),
    };
}
fn metadata(i: &Inner) -> SessionMetadata {
    SessionMetadata {
        instance_id: i.instance_id.clone(),
        session_id: i.session.session_id.clone(),
        queue_revision: i.session.queue_revision.to_string(),
        state_sequence: i.state_sequence.to_string(),
        generation_id: i.generation_id.clone(),
    }
}
fn with_metadata<T>(result: PResult<T>, i: &Inner) -> PResult<T> {
    result.map_err(|mut error| {
        error.authoritative = Some(Box::new(metadata(i)));
        error
    })
}
fn reject_unstarted(command: OwnerCommand) {
    match command {
        OwnerCommand::ReserveAlbum(_, _, reply) => {
            let _ = reply.send(Err(owner_stopped()));
        }
        OwnerCommand::CommitAlbum(_, _, _, reply) => {
            let _ = reply.send(Err(owner_stopped()));
        }
        OwnerCommand::ListOutputs(_, reply) => {
            let _ = reply.send(Err(owner_stopped()));
        }
        OwnerCommand::SelectOutput(_, _guard, reply) => {
            let _ = reply.send(Err(owner_stopped()));
        }
        OwnerCommand::Apply(_, _guard, reply) => {
            let _ = reply.send(Err(owner_stopped()));
        }
        OwnerCommand::RetryRestore(_guard, reply) => {
            let _ = reply.send(Err(owner_stopped()));
        }
        OwnerCommand::Snapshot(reply) => {
            let _ = reply.send(Err(owner_stopped()));
        }
        OwnerCommand::List(_, reply) => {
            let _ = reply.send(Err(owner_stopped()));
        }
        OwnerCommand::Control(_, _guard, reply) => {
            let _ = reply.send(Err(owner_stopped()));
        }
        OwnerCommand::Seek(_, _guard, reply) => {
            let _ = reply.send(Err(owner_stopped()));
        }
        OwnerCommand::NativeControl(_, _guard, reply) => {
            let _ = reply.send(Err(owner_stopped()));
        }
    }
}

fn retry_restore_inner(
    inner: &mut Inner,
    generation_serial: &AtomicU64,
) -> PResult<SessionSnapshot> {
    if inner.restoration.status != "error" || inner.dirty {
        return Err(PlaybackError::conflict(
            "PLAYBACK_BUSY",
            "restore retry is unavailable",
        ));
    }
    let next_sequence = inner
        .state_sequence
        .checked_add(1)
        .filter(|n| *n <= i64::MAX as u64)
        .ok_or_else(|| PlaybackError::invalid("INVALID_SESSION", "state sequence overflow"))?;
    let mut loaded = match inner.db.load_playback_session().map_err(storage)? {
        Some(loaded) => loaded,
        None => {
            // load_playback_session rejects orphaned rows; only a fresh empty
            // schema may reach this branch after a transient initialization failure.
            let fresh = fresh_session();
            inner
                .db
                .persist_playback_structure(&fresh, &[])
                .map_err(storage)?;
            fresh
        }
    };
    validate_stored(&inner.db, &loaded)?;
    if loaded.state != TransportState::Idle {
        loaded.state = TransportState::Paused;
    }
    // Build all fallible response data before accepting the recovered live state.
    let restored_playback_status = if loaded.current_occurrence_id.is_some() {
        PlaybackStatus::Paused
    } else {
        PlaybackStatus::Idle
    };
    let candidate = Inner {
        album: Default::default(),
        pending_terminal: None,
        output: inner.output.clone(),
        outputs: None,
        output_dedup: HashMap::new(),
        output_dedup_order: VecDeque::new(),
        output_gate: inner.output_gate.clone(),
        control_epoch: inner.control_epoch.clone(),
        db: inner.db.clone(),
        instance_id: inner.instance_id.clone(),
        checkpointed_position_ms: loaded.position_ms,
        playback: PlaybackState {
            status: restored_playback_status,
            ..Default::default()
        },
        session: loaded,
        generation_id: Uuid::new_v4().to_string(),
        state_sequence: next_sequence,
        restoration: Status {
            status: "ok".into(),
            code: None,
        },
        persistence: Status {
            status: "ok".into(),
            code: None,
        },
        dedup: HashMap::new(),
        dedup_order: VecDeque::new(),
        control_dedup: HashMap::new(),
        control_dedup_order: VecDeque::new(),
        seek_dedup: HashMap::new(),
        seek_dedup_order: VecDeque::new(),
        seek_resume_after_commit: false,
        dirty: false,
    };
    let response = snapshot(&candidate)?;
    inner.session = candidate.session;
    inner.checkpointed_position_ms = candidate.checkpointed_position_ms;
    inner.generation_id = candidate.generation_id;
    generation_serial.fetch_add(1, Ordering::AcqRel);
    inner.state_sequence = candidate.state_sequence;
    inner.restoration = candidate.restoration;
    inner.persistence = candidate.persistence;
    Ok(response)
}

fn checkpoint_inner(i: &mut Inner) -> PResult<()> {
    if i.restoration.status == "error" || i.pending_terminal.is_some() || !i.dirty {
        return Ok(());
    }
    let seq = i
        .session
        .checkpoint_sequence
        .checked_add(1)
        .filter(|n| *n <= i64::MAX as u64)
        .ok_or_else(|| {
            PlaybackError::invalid("PERSISTENCE_FAILED", "checkpoint sequence overflow")
        })?;
    match i
        .db
        .checkpoint_playback_position(&i.session.session_id, seq, i.session.position_ms)
    {
        Ok(()) => {
            i.session.checkpoint_sequence = seq;
            i.checkpointed_position_ms = i.session.position_ms;
            i.dirty = false;
            i.persistence = Status {
                status: "ok".into(),
                code: None,
            };
            Ok(())
        }
        Err(_) => {
            i.persistence = Status {
                status: "error".into(),
                code: Some("PERSISTENCE_FAILED".into()),
            };
            Err(storage(anyhow::anyhow!("checkpoint failed")))
        }
    }
}

fn fresh_session() -> PersistedSession {
    PersistedSession {
        session_id: Uuid::new_v4().to_string(),
        queue_revision: 0,
        checkpoint_sequence: 0,
        state: TransportState::Idle,
        current_occurrence_id: None,
        position_ms: 0,
        album_context: None,
    }
}
fn require_schema(v: u32) -> PResult<()> {
    if v == SCHEMA_VERSION {
        Ok(())
    } else {
        Err(PlaybackError::invalid(
            "UNSUPPORTED_PLAYBACK_VERSION",
            "unsupported playback schema version",
        ))
    }
}
fn parse_revision(v: &str) -> PResult<u64> {
    if v.is_empty() || !v.bytes().all(|b| b.is_ascii_digit()) || (v.len() > 1 && v.starts_with('0'))
    {
        return Err(PlaybackError::invalid(
            "INVALID_SESSION",
            "revision is not canonical",
        ));
    }
    v.parse::<u64>()
        .ok()
        .filter(|n| *n <= i64::MAX as u64)
        .ok_or_else(|| PlaybackError::invalid("INVALID_SESSION", "revision is invalid"))
}
fn storage(_: anyhow::Error) -> PlaybackError {
    PlaybackError::invalid("PERSISTENCE_FAILED", "playback storage operation failed")
}
fn validate_stored(db: &Database, s: &PersistedSession) -> PResult<()> {
    db.validate_playback_session(s)
        .map_err(|_| PlaybackError::invalid("INVALID_SESSION", "stored session invariants failed"))
}
fn apply_inner(
    i: &mut Inner,
    p: &ApplySessionParams,
    generation_serial: &AtomicU64,
) -> PResult<ApplyResult> {
    apply_inner_with_album_context(i, p, generation_serial, None)
}

fn apply_inner_with_album_context(
    i: &mut Inner,
    p: &ApplySessionParams,
    generation_serial: &AtomicU64,
    album_context: Option<super::model::FrozenAlbumContext>,
) -> PResult<ApplyResult> {
    require_schema(p.schema_version)?;
    if Uuid::parse_str(&p.command_id).is_err() {
        return Err(PlaybackError::invalid(
            "INVALID_SESSION",
            "commandId must be a UUID",
        ));
    }
    if p.instance_id != i.instance_id {
        return Err(PlaybackError::conflict(
            "INSTANCE_MISMATCH",
            "daemon instance changed",
        ));
    }
    if p.session_id != i.session.session_id {
        return Err(PlaybackError::conflict(
            "SESSION_MISMATCH",
            "session identity is stale",
        ));
    }
    if parse_revision(&p.expected_queue_revision)? != i.session.queue_revision {
        return Err(PlaybackError::conflict(
            "QUEUE_REVISION_CONFLICT",
            "queue revision is stale",
        ));
    }
    if i.restoration.status == "error" {
        return Err(PlaybackError::conflict(
            "RESTORE_FAILED",
            "stored playback state must be recovered first",
        ));
    }
    if i.pending_terminal.is_some() && matches!(p.operation, SessionOperation::AppendQueue { .. }) {
        return Err(PlaybackError::conflict(
            "PERSISTENCE_FAILED",
            "retry or replace the pending storage transition before appending",
        ));
    }
    let mut next_session = i.session.clone();
    let mut assigned = Vec::new();
    let next_sequence = i
        .state_sequence
        .checked_add(1)
        .filter(|n| *n <= i64::MAX as u64)
        .ok_or_else(|| PlaybackError::invalid("INVALID_SESSION", "state sequence overflow"))?;
    match &p.operation {
        SessionOperation::ReplaceQueue { sources } | SessionOperation::PlayAlbum { sources } => {
            if matches!(p.operation, SessionOperation::PlayAlbum { .. }) {
                validate_album_sources(sources)?;
            } else {
                validate_sources(sources)?;
            }
            assigned = make_occurrences(sources, 0);
            next_session.album_context =
                if matches!(p.operation, SessionOperation::PlayAlbum { .. }) {
                    album_context
                } else {
                    None
                };
            decorate_availability(&i.db, &mut assigned)?;
            next_session.current_occurrence_id = assigned.first().map(|o| o.occurrence_id.clone());
            next_session.position_ms = 0;
            next_session.queue_revision = next_session
                .queue_revision
                .checked_add(1)
                .filter(|n| *n <= i64::MAX as u64)
                .ok_or_else(|| {
                    PlaybackError::invalid("INVALID_SESSION", "queue revision overflow")
                })?;
            next_session.state = if assigned.is_empty() {
                TransportState::Idle
            } else if matches!(p.operation, SessionOperation::PlayAlbum { .. }) {
                TransportState::Buffering
            } else {
                TransportState::Paused
            };
            i.db.persist_playback_structure(&next_session, &assigned)
                .map_err(storage)?;
        }
        SessionOperation::AppendQueue { sources } => {
            validate_sources(sources)?;
            let next_ordinal =
                i.db.playback_next_ordinal(&i.session.session_id)
                    .map_err(storage)?;
            if next_ordinal
                .checked_add(sources.len() as u64)
                .is_none_or(|n| n > i64::MAX as u64)
            {
                return Err(PlaybackError::invalid(
                    "INVALID_SESSION",
                    "ordinal overflow",
                ));
            }
            assigned = make_occurrences(sources, next_ordinal);
            decorate_availability(&i.db, &mut assigned)?;
            let was_empty = next_session.current_occurrence_id.is_none();
            if was_empty {
                next_session.current_occurrence_id =
                    assigned.first().map(|o| o.occurrence_id.clone());
                next_session.position_ms = 0;
            }
            next_session.queue_revision = next_session
                .queue_revision
                .checked_add(1)
                .filter(|n| *n <= i64::MAX as u64)
                .ok_or_else(|| {
                    PlaybackError::invalid("INVALID_SESSION", "queue revision overflow")
                })?;
            if was_empty {
                next_session.state = if next_session.current_occurrence_id.is_none() {
                    TransportState::Idle
                } else {
                    TransportState::Paused
                };
            }
            i.db.append_playback_occurrences(&next_session, &assigned)
                .map_err(storage)?;
        }
        SessionOperation::SelectCurrent { occurrence_id } => {
            next_session.current_occurrence_id = Some(occurrence_id.clone());
            next_session.position_ms = 0;
            next_session.state = TransportState::Paused;
            i.db.select_playback_current(&next_session).map_err(|_| {
                PlaybackError::invalid("INVALID_SESSION", "selected occurrence is absent")
            })?;
        }
        SessionOperation::Clear => {
            next_session.album_context = None;
            next_session.current_occurrence_id = None;
            next_session.position_ms = 0;
            next_session.queue_revision = next_session
                .queue_revision
                .checked_add(1)
                .filter(|n| *n <= i64::MAX as u64)
                .ok_or_else(|| {
                    PlaybackError::invalid("INVALID_SESSION", "queue revision overflow")
                })?;
            next_session.state = TransportState::Idle;
            i.db.clear_playback_session(&next_session)
                .map_err(storage)?;
        }
        SessionOperation::PlayTrack { source } => {
            next_session.album_context = None;
            source
                .validate()
                .map_err(|m| PlaybackError::invalid("INVALID_SESSION", m))?;
            assigned = make_occurrences(std::slice::from_ref(source), 0);
            decorate_availability(&i.db, &mut assigned)?;
            next_session.current_occurrence_id = Some(assigned[0].occurrence_id.clone());
            next_session.position_ms = 0;
            next_session.queue_revision = next_session
                .queue_revision
                .checked_add(1)
                .filter(|n| *n <= i64::MAX as u64)
                .ok_or_else(|| {
                    PlaybackError::invalid("INVALID_SESSION", "queue revision overflow")
                })?;
            next_session.state = TransportState::Buffering;
            i.db.persist_playback_structure(&next_session, &assigned)
                .map_err(storage)?;
        }
    }
    let preserve_generation = matches!(p.operation, SessionOperation::AppendQueue { .. })
        && i.session.current_occurrence_id == next_session.current_occurrence_id;
    if !preserve_generation {
        i.pending_terminal = None;
        i.control_epoch.fetch_add(1, Ordering::AcqRel);
        generation_serial.fetch_add(1, Ordering::AcqRel);
        super::audio::global().control(ControlAction::Stop);
        i.generation_id = Uuid::new_v4().to_string();
    }
    i.session = next_session;
    i.output_gate.store(
        matches!(
            i.session.state,
            TransportState::Playing | TransportState::Buffering
        ),
        Ordering::Release,
    );
    i.state_sequence = next_sequence;
    i.dirty = false;
    i.checkpointed_position_ms = i.session.position_ms;
    i.persistence = Status {
        status: "ok".into(),
        code: None,
    };
    i.playback = match &p.operation {
        SessionOperation::PlayTrack { .. } | SessionOperation::PlayAlbum { .. } => PlaybackState {
            status: PlaybackStatus::Loading,
            ..Default::default()
        },
        SessionOperation::Clear => PlaybackState::default(),
        SessionOperation::AppendQueue { .. } => i.playback.clone(),
        _ => PlaybackState {
            status: PlaybackStatus::Paused,
            ..Default::default()
        },
    };
    if matches!(
        p.operation,
        SessionOperation::PlayTrack { .. } | SessionOperation::PlayAlbum { .. }
    ) && i.outputs.is_some()
        && let Err(code) = output_policy(i)
    {
        i.output_gate.store(false, Ordering::Release);
        i.session.state = TransportState::Paused;
        i.playback.status = PlaybackStatus::Paused;
        i.output.error = Some(PlaybackFailure {
            code: code.into(),
            retryable: true,
        });
    }
    Ok(ApplyResult {
        start_audio: matches!(
            p.operation,
            SessionOperation::PlayTrack { .. } | SessionOperation::PlayAlbum { .. }
        ) && (i.outputs.is_none() || output_policy(i).is_ok()),
        current_metadata: metadata(i),
        session_id: i.session.session_id.clone(),
        queue_revision: i.session.queue_revision.to_string(),
        state_sequence: i.state_sequence.to_string(),
        generation_id: i.generation_id.clone(),
        assigned_occurrences: assigned,
    })
}
fn validate_sources(s: &[TrackSource]) -> PResult<()> {
    if s.len() > MAX_INSERT_BATCH {
        return Err(PlaybackError::invalid(
            "INVALID_SESSION",
            "insert batch exceeds 200 sources",
        ));
    }
    for v in s {
        v.validate()
            .map_err(|m| PlaybackError::invalid("INVALID_SESSION", m))?;
    }
    Ok(())
}
fn validate_album_sources(s: &[TrackSource]) -> PResult<()> {
    if s.is_empty() || s.len() > super::album::MAX_ALBUM_OCCURRENCES {
        return Err(PlaybackError::invalid(
            "ALBUM_INVALID",
            "album source count is invalid",
        ));
    }
    for source in s {
        source
            .validate()
            .map_err(|message| PlaybackError::invalid("ALBUM_INVALID", message))?;
    }
    Ok(())
}
fn make_occurrences(s: &[TrackSource], start: u64) -> Vec<Occurrence> {
    s.iter()
        .enumerate()
        .map(|(n, source)| Occurrence {
            occurrence_id: Uuid::new_v4().to_string(),
            ordinal: start + n as u64,
            source: source.clone(),
            availability: SourceAvailability::Unknown,
        })
        .collect()
}
fn snapshot(i: &Inner) -> PResult<SessionSnapshot> {
    let failed = i.restoration.status == "error";
    let mut rows = if failed {
        Vec::new()
    } else {
        i.db.playback_page(&i.session.session_id, None, DEFAULT_PAGE_SIZE + 1)
            .map_err(storage)?
    };
    let next = if rows.len() > DEFAULT_PAGE_SIZE {
        rows.truncate(DEFAULT_PAGE_SIZE);
        rows.last().map(|o| encode_cursor(&i.session, o.ordinal))
    } else {
        None
    };
    decorate_availability(&i.db, &mut rows)?;
    let mut current = if let Some(id) = &i.session.current_occurrence_id {
        i.db.playback_occurrence(&i.session.session_id, id)
            .map_err(storage)?
    } else {
        None
    };
    if let Some(current) = current.as_mut() {
        current.availability = availability(&i.db, &current.source.server_id)?;
    }
    Ok(SessionSnapshot {
        resume_audio: false,
        resume_epoch: i.control_epoch.load(Ordering::Acquire),
        seek_audio: false,
        seek_epoch: i.control_epoch.load(Ordering::Acquire),
        gain_bits: current
            .as_ref()
            .and_then(|occurrence| {
                i.session
                    .album_context
                    .as_ref()
                    .map(|context| context.scalar_for(occurrence).to_bits())
            })
            .unwrap_or(1.0f32.to_bits()),
        schema_version: SCHEMA_VERSION,
        instance_id: i.instance_id.clone(),
        session_id: i.session.session_id.clone(),
        queue_revision: i.session.queue_revision.to_string(),
        state_sequence: i.state_sequence.to_string(),
        generation_id: i.generation_id.clone(),
        state: i.session.state,
        current: current.clone(),
        position_ms: i.session.position_ms,
        checkpointed_position_ms: i.checkpointed_position_ms,
        persistence: i.persistence.clone(),
        restoration: i.restoration.clone(),
        total_occurrence_count: if failed {
            0
        } else {
            i.db.playback_count(&i.session.session_id)
                .map_err(storage)?
        },
        occurrences: rows,
        next_cursor: next,
        playback: {
            let mut playback = i.playback.clone();
            playback.can_go_next = current
                .as_ref()
                .and_then(|occurrence| {
                    i.db.playback_successor(&i.session.session_id, occurrence.ordinal)
                        .ok()
                        .flatten()
                })
                .is_some()
                && i.restoration.status != "error"
                && i.pending_terminal.is_none();
            playback
        },
        output: i.output.clone(),
    })
}

fn control_inner(
    i: &mut Inner,
    p: &ControlParams,
    generation_serial: &AtomicU64,
) -> PResult<SessionSnapshot> {
    require_schema(p.schema_version)?;
    if Uuid::parse_str(&p.command_id).is_err() {
        return Err(PlaybackError::invalid(
            "INVALID_SESSION",
            "commandId must be a UUID",
        ));
    }
    if p.instance_id != i.instance_id || p.session_id != i.session.session_id {
        return Err(PlaybackError::conflict(
            "SESSION_MISMATCH",
            "session identity is stale",
        ));
    }
    let before = i.session.current_occurrence_id.clone();
    reconcile_presented_handoff(i, generation_serial)?;
    let mut reconciled = p.clone();
    if matches!(p.action, ControlAction::Pause | ControlAction::Stop)
        && before.as_deref() == Some(p.occurrence_id.as_str())
        && before != i.session.current_occurrence_id
    {
        reconciled.occurrence_id = i.session.current_occurrence_id.clone().unwrap_or_default();
    }
    let p = &reconciled;
    if p.expected_generation_id != i.generation_id
        || i.session.current_occurrence_id.as_deref() != Some(&p.occurrence_id)
    {
        return Err(PlaybackError::conflict(
            "GENERATION_CONFLICT",
            "playback generation is stale",
        ));
    }
    if i.pending_terminal
        .as_ref()
        .is_some_and(|pending| pending.generation_id != i.generation_id)
    {
        i.pending_terminal = None;
    }
    if let Some(pending) = i.pending_terminal.clone() {
        match p.action {
            ControlAction::Retry => {
                return commit_terminal(i, pending, generation_serial, true);
            }
            ControlAction::Stop => {
                i.pending_terminal = None;
            }
            _ => {
                return Err(PlaybackError::conflict(
                    "PERSISTENCE_FAILED",
                    "retry or stop the pending storage transition first",
                ));
            }
        }
    }
    if matches!(p.action, ControlAction::Resume | ControlAction::Retry) && i.outputs.is_some() {
        output_policy(i).map_err(|code| {
            PlaybackError::invalid(code, "choose an available output before resuming")
        })?;
    }
    if p.action == ControlAction::Resume && i.output_gate.load(Ordering::Acquire) {
        return snapshot(i);
    }
    if i.playback.pending_seek.is_some()
        && matches!(p.action, ControlAction::Pause | ControlAction::Resume)
    {
        album_admission::cancel_pending(i);
        i.state_sequence = i
            .state_sequence
            .checked_add(1)
            .ok_or_else(|| PlaybackError::invalid("INVALID_SESSION", "state sequence overflow"))?;
        i.seek_resume_after_commit = p.action == ControlAction::Resume;
        i.output_gate.store(false, Ordering::Release);
        i.session.state = if p.action == ControlAction::Resume {
            TransportState::Buffering
        } else {
            TransportState::Paused
        };
        i.playback.status = if p.action == ControlAction::Resume {
            PlaybackStatus::Loading
        } else {
            PlaybackStatus::Paused
        };
        return snapshot(i);
    }
    if p.action == ControlAction::Next {
        let current =
            i.db.playback_occurrence(&i.session.session_id, &p.occurrence_id)
                .map_err(storage)?
                .ok_or_else(|| {
                    PlaybackError::invalid("INVALID_SESSION", "current occurrence is absent")
                })?;
        if i.db
            .playback_successor(&i.session.session_id, current.ordinal)
            .map_err(storage)?
            .is_none()
        {
            return Err(PlaybackError::invalid(
                "NEXT_UNAVAILABLE",
                "the current occurrence has no successor",
            ));
        }
    }
    if p.action == ControlAction::Retry && i.playback.status != PlaybackStatus::Error {
        return Err(PlaybackError::invalid(
            "RETRY_UNAVAILABLE",
            "the current occurrence has no retryable failure",
        ));
    }
    if matches!(p.action, ControlAction::Resume | ControlAction::Retry)
        && (p.action == ControlAction::Retry
            || matches!(
                i.playback.status,
                PlaybackStatus::Completed | PlaybackStatus::Error
            )
            || i.db
                .playback_outcome(&p.occurrence_id)
                .map_err(storage)?
                .is_some())
    {
        let mut attempt = i.session.clone();
        if p.action == ControlAction::Resume && i.playback.status == PlaybackStatus::Completed {
            attempt.position_ms = 0;
        }
        attempt.checkpoint_sequence = attempt
            .checkpoint_sequence
            .checked_add(1)
            .filter(|n| *n <= i64::MAX as u64)
            .ok_or_else(|| storage(anyhow::anyhow!("checkpoint sequence overflow")))?;
        i.db.reset_playback_attempt(&attempt).map_err(storage)?;
        i.session = attempt;
        i.checkpointed_position_ms = i.session.position_ms;
        i.dirty = false;
    }
    let mut committed_snapshot = None;
    if p.action == ControlAction::Pause {
        // Native stop/cork establishes the cancellation point before the owner
        // changes its epoch. A matching presented receipt wins this race.
        let before = i.session.current_occurrence_id.clone();
        super::audio::global().control(ControlAction::Pause);
        reconcile_presented_handoff(i, generation_serial)?;
        if before == i.session.current_occurrence_id
            && let Some(position) = super::audio::global().captured_position(&i.generation_id)
        {
            i.session.position_ms = position;
        }
    }
    i.control_epoch.fetch_add(1, Ordering::AcqRel);
    if matches!(p.action, ControlAction::Pause | ControlAction::Resume) {
        super::audio::global()
            .retain_control_epoch(&i.generation_id, i.control_epoch.load(Ordering::Acquire));
    }
    i.state_sequence = i
        .state_sequence
        .checked_add(1)
        .ok_or_else(|| PlaybackError::invalid("INVALID_SESSION", "state sequence overflow"))?;
    match p.action {
        ControlAction::Pause => {
            i.output_gate.store(false, Ordering::Release);
            i.session.state = TransportState::Paused;
            i.playback.status = PlaybackStatus::Paused;
        }
        ControlAction::Resume => {
            if matches!(
                i.playback.status,
                PlaybackStatus::Completed | PlaybackStatus::Error
            ) || i.playback.error.is_some()
            {
                generation_serial.fetch_add(1, Ordering::AcqRel);
                super::audio::global().control(ControlAction::Stop);
                if i.playback.status == PlaybackStatus::Completed {
                    i.session.position_ms = 0;
                }
                i.generation_id = Uuid::new_v4().to_string();
            }
            i.session.state = TransportState::Buffering;
            i.output_gate.store(true, Ordering::Release);
            i.playback.status = PlaybackStatus::Loading;
            i.playback.error = None;
        }
        ControlAction::Retry => {
            if i.playback.status != PlaybackStatus::Error {
                return Err(PlaybackError::invalid(
                    "RETRY_UNAVAILABLE",
                    "the current occurrence has no retryable failure",
                ));
            }
            generation_serial.fetch_add(1, Ordering::AcqRel);
            super::audio::global().control(ControlAction::Stop);
            i.generation_id = Uuid::new_v4().to_string();
            i.session.state = TransportState::Buffering;
            i.output_gate.store(true, Ordering::Release);
            i.playback.status = PlaybackStatus::Loading;
            i.playback.error = None;
        }
        ControlAction::Next => {
            let current =
                i.db.playback_occurrence(&i.session.session_id, &p.occurrence_id)
                    .map_err(storage)?
                    .ok_or_else(|| {
                        PlaybackError::invalid("INVALID_SESSION", "current occurrence is absent")
                    })?;
            let successor =
                i.db.playback_successor(&i.session.session_id, current.ordinal)
                    .map_err(storage)?
                    .ok_or_else(|| {
                        PlaybackError::invalid(
                            "NEXT_UNAVAILABLE",
                            "the current occurrence has no successor",
                        )
                    })?;
            let resume = matches!(
                i.session.state,
                TransportState::Playing | TransportState::Buffering
            ) || matches!(
                i.playback.status,
                PlaybackStatus::Active | PlaybackStatus::Loading
            );
            let stopped = matches!(
                i.playback.status,
                PlaybackStatus::Stopped | PlaybackStatus::Completed
            );
            let mut next_session = i.session.clone();
            next_session.current_occurrence_id = Some(successor.occurrence_id);
            next_session.position_ms = 0;
            next_session.state = if resume {
                TransportState::Buffering
            } else {
                TransportState::Paused
            };
            let mut terminal = terminal_plan(
                i,
                "explicitSkip",
                if resume {
                    PlaybackStatus::Loading
                } else if stopped {
                    PlaybackStatus::Stopped
                } else {
                    PlaybackStatus::Paused
                },
            );
            terminal.session = next_session;
            committed_snapshot = Some(commit_terminal(i, terminal, generation_serial, false)?);
        }
        ControlAction::Stop => {
            i.pending_terminal = None;
            if let Some(pending) = i.playback.pending_seek.take() {
                i.playback.seek_outcome = Some(SeekOutcome {
                    operation_id: pending.operation_id,
                    requested_position_ms: pending.requested_position_ms,
                    actual_position_ms: None,
                    status: "superseded".into(),
                    error: None,
                });
            }
            i.output_gate.store(false, Ordering::Release);
            generation_serial.fetch_add(1, Ordering::AcqRel);
            super::audio::global().control(ControlAction::Stop);
            i.session.state = if i.session.current_occurrence_id.is_some() {
                TransportState::Paused
            } else {
                TransportState::Idle
            };
            i.session.position_ms = 0;
            i.generation_id = Uuid::new_v4().to_string();
            i.playback.status = if i.session.current_occurrence_id.is_some() {
                PlaybackStatus::Stopped
            } else {
                PlaybackStatus::Idle
            };
            i.dirty = true;
            checkpoint_inner(i)?;
        }
    }
    committed_snapshot
        .map(Ok)
        .unwrap_or_else(|| snapshot(i))
        .map(|mut snapshot| {
            snapshot.resume_audio = matches!(
                p.action,
                ControlAction::Resume | ControlAction::Retry | ControlAction::Next
            ) && snapshot.state == TransportState::Buffering;
            snapshot.resume_epoch = i.control_epoch.load(Ordering::Acquire);
            snapshot
        })
}

fn seek_inner(
    i: &mut Inner,
    p: &SeekParams,
    generation_serial: &AtomicU64,
) -> PResult<SessionSnapshot> {
    reconcile_presented_handoff(i, generation_serial)?;
    if i.pending_terminal.is_some() {
        return Err(PlaybackError::conflict(
            "PERSISTENCE_FAILED",
            "retry or stop the pending storage transition first",
        ));
    }
    require_schema(p.schema_version)?;
    if Uuid::parse_str(&p.command_id).is_err() {
        return Err(PlaybackError::invalid(
            "INVALID_SEEK",
            "commandId must be a UUID",
        ));
    }
    if p.position_ms > 9_007_199_254_740_991 {
        return Err(PlaybackError::invalid(
            "INVALID_SEEK",
            "positionMs exceeds the safe integer limit",
        ));
    }
    if p.instance_id != i.instance_id || p.session_id != i.session.session_id {
        return Err(PlaybackError::conflict(
            "SESSION_MISMATCH",
            "session identity is stale",
        ));
    }
    if p.expected_generation_id != i.generation_id
        || i.session.current_occurrence_id.as_deref() != Some(&p.occurrence_id)
    {
        return Err(PlaybackError::conflict(
            "GENERATION_CONFLICT",
            "playback generation is stale",
        ));
    }
    if !i.playback.seek.available {
        return Err(PlaybackError::invalid(
            "SEEK_UNAVAILABLE",
            "the current representation has no qualified media-time seek",
        ));
    }
    let duration_ms = i
        .playback
        .duration_ms
        .filter(|duration| *duration > 0)
        .ok_or_else(|| {
            PlaybackError::invalid("SEEK_UNAVAILABLE", "the current duration is unavailable")
        })?;
    if p.position_ms > duration_ms {
        return Err(PlaybackError::invalid(
            "INVALID_SEEK",
            "positionMs exceeds the current duration",
        ));
    }
    let prior = i.session.position_ms;
    let resume = matches!(
        i.session.state,
        TransportState::Playing | TransportState::Buffering
    ) || matches!(
        i.playback.status,
        PlaybackStatus::Active | PlaybackStatus::Loading
    );
    i.control_epoch.fetch_add(1, Ordering::AcqRel);
    generation_serial.fetch_add(1, Ordering::AcqRel);
    i.output_gate.store(false, Ordering::Release);
    super::audio::global().control(ControlAction::Stop);
    i.generation_id = Uuid::new_v4().to_string();
    i.state_sequence = i
        .state_sequence
        .checked_add(1)
        .ok_or_else(|| PlaybackError::invalid("INVALID_SESSION", "state sequence overflow"))?;
    i.seek_resume_after_commit = resume;
    i.playback.pending_seek = Some(PendingSeek {
        operation_id: p.command_id.clone(),
        requested_position_ms: p.position_ms,
        prior_committed_position_ms: prior,
    });
    i.playback.seek_outcome = Some(SeekOutcome {
        operation_id: p.command_id.clone(),
        requested_position_ms: p.position_ms,
        actual_position_ms: None,
        status: "pending".into(),
        error: None,
    });
    i.playback.error = None;
    if p.position_ms == duration_ms {
        i.session.position_ms = duration_ms;
        i.session.state = TransportState::Paused;
        i.playback.status = PlaybackStatus::Completed;
        i.playback.pending_seek = None;
        i.playback.seek_outcome = Some(SeekOutcome {
            operation_id: p.command_id.clone(),
            requested_position_ms: p.position_ms,
            actual_position_ms: Some(duration_ms),
            status: "succeeded".into(),
            error: None,
        });
        i.dirty = true;
        checkpoint_inner(i)?;
    } else {
        i.session.state = if resume {
            TransportState::Buffering
        } else {
            TransportState::Paused
        };
        i.playback.status = if resume {
            PlaybackStatus::Loading
        } else {
            PlaybackStatus::Paused
        };
    }
    snapshot(i).map(|mut snapshot| {
        snapshot.seek_audio = p.position_ms < duration_ms;
        snapshot.seek_epoch = i.control_epoch.load(Ordering::Acquire);
        snapshot
    })
}

fn native_control_inner(
    i: &mut Inner,
    intent: NativeControlIntent,
    generation_serial: &AtomicU64,
) -> PResult<SessionSnapshot> {
    let Some(occurrence_id) = i.session.current_occurrence_id.clone() else {
        return Err(PlaybackError::invalid(
            "RESUME_UNAVAILABLE",
            "there is no current track to control",
        ));
    };
    if let NativeControlIntent::SeekAbsolute(position_ms) = intent {
        let params = SeekParams {
            schema_version: 1,
            instance_id: i.instance_id.clone(),
            session_id: i.session.session_id.clone(),
            command_id: Uuid::new_v4().to_string(),
            expected_generation_id: i.generation_id.clone(),
            occurrence_id,
            position_ms,
        };
        return seek_inner(i, &params, generation_serial);
    }
    if let NativeControlIntent::SeekRelative(offset_ms) = intent {
        let duration = i
            .playback
            .duration_ms
            .filter(|value| *value > 0)
            .ok_or_else(|| {
                PlaybackError::invalid("SEEK_UNAVAILABLE", "the current duration is unavailable")
            })?;
        let target = i128::from(i.session.position_ms) + i128::from(offset_ms);
        if target > i128::from(duration) {
            if !i.playback.seek.available {
                return Err(PlaybackError::invalid(
                    "SEEK_UNAVAILABLE",
                    "the current representation has no qualified media-time seek",
                ));
            }
            if let Some(current) =
                i.db.playback_occurrence(&i.session.session_id, &occurrence_id)
                    .map_err(storage)?
                && i.db
                    .playback_successor(&i.session.session_id, current.ordinal)
                    .map_err(storage)?
                    .is_some()
            {
                let params = ControlParams {
                    schema_version: 1,
                    instance_id: i.instance_id.clone(),
                    session_id: i.session.session_id.clone(),
                    command_id: Uuid::new_v4().to_string(),
                    expected_generation_id: i.generation_id.clone(),
                    occurrence_id,
                    action: ControlAction::Next,
                };
                return control_inner(i, &params, generation_serial);
            }
        }
        let target = target.clamp(0, i128::from(duration)) as u64;
        let params = SeekParams {
            schema_version: 1,
            instance_id: i.instance_id.clone(),
            session_id: i.session.session_id.clone(),
            command_id: Uuid::new_v4().to_string(),
            expected_generation_id: i.generation_id.clone(),
            occurrence_id,
            position_ms: target,
        };
        return seek_inner(i, &params, generation_serial);
    }
    let action = match intent {
        NativeControlIntent::Play => ControlAction::Resume,
        NativeControlIntent::Pause => ControlAction::Pause,
        NativeControlIntent::Stop => ControlAction::Stop,
        NativeControlIntent::Next => ControlAction::Next,
        NativeControlIntent::Toggle => {
            if matches!(
                i.session.state,
                TransportState::Playing | TransportState::Buffering
            ) || matches!(
                i.playback.status,
                PlaybackStatus::Active | PlaybackStatus::Loading
            ) {
                ControlAction::Pause
            } else {
                ControlAction::Resume
            }
        }
        NativeControlIntent::SeekRelative(_) | NativeControlIntent::SeekAbsolute(_) => {
            unreachable!()
        }
    };
    let params = ControlParams {
        schema_version: 1,
        instance_id: i.instance_id.clone(),
        session_id: i.session.session_id.clone(),
        command_id: Uuid::new_v4().to_string(),
        expected_generation_id: i.generation_id.clone(),
        occurrence_id,
        action,
    };
    control_inner(i, &params, generation_serial)
}
fn availability(db: &Database, server_id: &str) -> PResult<SourceAvailability> {
    db.has_portable_server(server_id)
        .map(|configured| {
            if configured {
                SourceAvailability::Unknown
            } else {
                SourceAvailability::NotConfigured
            }
        })
        .map_err(storage)
}
fn decorate_availability(db: &Database, rows: &mut [Occurrence]) -> PResult<()> {
    for occurrence in rows {
        occurrence.availability = availability(db, &occurrence.source.server_id)?;
    }
    Ok(())
}
fn encode_cursor(s: &PersistedSession, ordinal: u64) -> String {
    format!("{}:{}:{}", s.session_id, s.queue_revision, ordinal)
}
fn decode_cursor(v: &str, s: &PersistedSession) -> PResult<u64> {
    let mut x = v.rsplitn(3, ':');
    let ord = x.next().and_then(|v| parse_revision(v).ok());
    let rev = x.next().and_then(|v| parse_revision(v).ok());
    let sid = x.next();
    if sid != Some(s.session_id.as_str()) || rev != Some(s.queue_revision) || ord.is_none() {
        return Err(PlaybackError::conflict(
            "INVALID_CURSOR",
            "cursor does not belong to this snapshot",
        ));
    }
    Ok(ord.unwrap())
}
fn prune_dedup(i: &mut Inner) {
    while let Some(k) = i.dedup_order.front() {
        if i.dedup
            .get(k)
            .is_some_and(|v| v.at.elapsed() > Duration::from_secs(600))
        {
            let k = i.dedup_order.pop_front().unwrap();
            i.dedup.remove(&k);
        } else {
            break;
        }
    }
}
fn retain_dedup(i: &mut Inner, p: ApplySessionParams, r: PResult<ApplyResult>) {
    let k = p.command_id.clone();
    i.dedup.insert(
        k.clone(),
        DedupEntry {
            at: Instant::now(),
            payload: p,
            result: r,
        },
    );
    i.dedup_order.push_back(k);
    while i.dedup_order.len() > 1024 {
        if let Some(k) = i.dedup_order.pop_front() {
            i.dedup.remove(&k);
        }
    }
}

fn enqueue_event(
    events: &mut VecDeque<(u64, u64, String, PlaybackEvent)>,
    serial: u64,
    epoch: u64,
    generation: String,
    event: PlaybackEvent,
) {
    if events.iter().any(|(old, _, _, _)| *old > serial) {
        return;
    }
    let kind = event_kind(&event);
    if kind == 2
        && matches!(event, PlaybackEvent::Completed { .. })
        && events.iter().any(|(old, _, old_generation, prior)| {
            *old == serial
                && old_generation == &generation
                && matches!(prior, PlaybackEvent::Failed { .. })
        })
    {
        return;
    }
    events.retain(|(old, _, _, prior)| *old == serial && event_kind(prior) != kind);
    // Bounded independent slots prevent metadata and successful commits being evicted.
    events.push_back((serial, epoch, generation, event));
}

fn event_kind(event: &PlaybackEvent) -> u8 {
    match event {
        PlaybackEvent::Resolved { .. } => 0,
        PlaybackEvent::SeekQualified { .. } => 4,
        PlaybackEvent::SeekPipelineFailed { .. } => 5,
        PlaybackEvent::Active | PlaybackEvent::Buffering => 1,
        PlaybackEvent::Completed { .. } | PlaybackEvent::Failed { .. } => 2,
        PlaybackEvent::SeekCommitted { .. } | PlaybackEvent::SeekFailed { .. } => 3,
        PlaybackEvent::HandoffPresented { .. } => 6,
    }
}

fn apply_playback_event(i: &mut Inner, event: PlaybackEvent) {
    match event {
        PlaybackEvent::Resolved {
            metadata,
            duration_ms,
            representation,
            seek,
        } => {
            i.playback.metadata = Some(metadata);
            i.playback.duration_ms = duration_ms;
            i.playback.representation = Some(representation);
            if seek.reason.as_deref() != Some("seek.validating")
                && i.playback.pending_seek.is_none()
                && i.playback.status == PlaybackStatus::Loading
                && duration_ms
                    .is_some_and(|duration| duration > 0 && i.session.position_ms == duration)
            {
                i.session.position_ms = 0;
                i.dirty = true;
            }
            i.playback.seek = if duration_ms.is_some_and(|duration| duration > 0) {
                seek
            } else {
                SeekCapability::unavailable("seek.duration_unavailable")
            };
        }
        PlaybackEvent::SeekQualified {
            capability,
            duration_ms,
        } => {
            if let Some(duration) = duration_ms.filter(|duration| *duration > 0) {
                i.playback.duration_ms = Some(duration);
            }
            if i.playback.pending_seek.is_none()
                && i.playback.status == PlaybackStatus::Loading
                && i.playback
                    .duration_ms
                    .is_some_and(|duration| duration > 0 && i.session.position_ms == duration)
            {
                i.session.position_ms = 0;
                i.dirty = true;
            }
            i.playback.seek =
                if capability.available && !duration_ms.is_some_and(|duration| duration > 0) {
                    SeekCapability::unavailable("seek.duration_unavailable")
                } else {
                    capability
                };
        }
        PlaybackEvent::Active if i.output_gate.load(Ordering::Acquire) => {
            if let Some(occurrence_id) = i.session.current_occurrence_id.as_deref()
                && i.db
                    .clear_playback_failure(&i.session.session_id, occurrence_id)
                    .is_err()
            {
                i.persistence = Status {
                    status: "error".into(),
                    code: Some("PERSISTENCE_FAILED".into()),
                };
            }
            i.session.state = TransportState::Playing;
            i.playback.status = PlaybackStatus::Active;
        }
        PlaybackEvent::Buffering if i.output_gate.load(Ordering::Acquire) => {
            i.session.state = TransportState::Buffering;
            i.playback.status = PlaybackStatus::Loading;
        }
        PlaybackEvent::Completed { position_ms } => {
            i.output_gate.store(false, Ordering::Release);
            i.session.position_ms = position_ms;
            i.session.state = TransportState::Paused;
            i.playback.status = PlaybackStatus::Completed;
            i.dirty = true;
        }
        PlaybackEvent::SeekCommitted {
            operation_id,
            requested_position_ms,
            actual_position_ms,
        } if i
            .playback
            .pending_seek
            .as_ref()
            .is_some_and(|pending| pending.operation_id == operation_id) =>
        {
            i.session.position_ms = actual_position_ms;
            i.playback.pending_seek = None;
            i.playback.seek_outcome = Some(SeekOutcome {
                operation_id,
                requested_position_ms,
                actual_position_ms: Some(actual_position_ms),
                status: "succeeded".into(),
                error: None,
            });
            if i.seek_resume_after_commit {
                i.output_gate.store(true, Ordering::Release);
                i.session.state = TransportState::Playing;
                i.playback.status = PlaybackStatus::Active;
            } else {
                i.output_gate.store(false, Ordering::Release);
                i.session.state = TransportState::Paused;
                i.playback.status = PlaybackStatus::Paused;
            }
            i.dirty = true;
        }
        PlaybackEvent::SeekCommitted { .. } => {}
        PlaybackEvent::SeekFailed {
            operation_id,
            code,
            retryable,
        } if i
            .playback
            .pending_seek
            .as_ref()
            .is_some_and(|pending| pending.operation_id == operation_id) =>
        {
            let requested_position_ms = i
                .playback
                .pending_seek
                .as_ref()
                .map(|pending| pending.requested_position_ms)
                .unwrap_or(i.session.position_ms);
            i.playback.pending_seek = None;
            i.output_gate.store(false, Ordering::Release);
            i.session.state = TransportState::Paused;
            i.playback.status = PlaybackStatus::Error;
            let error = PlaybackFailure { code, retryable };
            i.playback.error = Some(error.clone());
            i.playback.seek_outcome = Some(SeekOutcome {
                operation_id,
                requested_position_ms,
                actual_position_ms: None,
                status: "failed".into(),
                error: Some(error),
            });
        }
        PlaybackEvent::SeekFailed { .. } => {}
        PlaybackEvent::SeekPipelineFailed { .. } => unreachable!("normalized by owner"),
        PlaybackEvent::Failed { code, retryable } => {
            // This event has already passed the owner's generation fence. A
            // retirement warning is not terminal: the worker is still owned.
            if code != "OUTPUT_RETIREMENT_PENDING" {
                output_selection::finish_output_preparation(i);
            }
            if code.starts_with("OUTPUT_") {
                // A warning or failed replacement does not prove the old
                // native handle retired. Only its close acknowledgement clears active.
                i.output.status = "error".into();
                i.output.error = Some(PlaybackFailure {
                    code: code.clone(),
                    retryable,
                });
            }
            i.output_gate.store(false, Ordering::Release);
            i.session.state = TransportState::Paused;
            i.playback.status = PlaybackStatus::Error;
            i.playback.error = Some(PlaybackFailure { code, retryable });
        }
        PlaybackEvent::Active
        | PlaybackEvent::Buffering
        | PlaybackEvent::HandoffPresented { .. } => {}
    }
    i.state_sequence = i.state_sequence.saturating_add(1);
}

#[allow(clippy::too_many_arguments)]
fn reconcile_presented_handoff(i: &mut Inner, generation_serial: &AtomicU64) -> PResult<()> {
    if i.pending_terminal.is_some() {
        return Ok(());
    }
    if let Some(PlaybackEvent::HandoffPresented {
        token,
        metadata,
        duration_ms,
        representation,
        successor_offset_frames,
        sample_rate,
        seek,
    }) = super::audio::global().presented_handoff(&i.generation_id)
        && i.session.current_occurrence_id.as_deref()
            == Some(token.predecessor_occurrence_id.as_str())
    {
        adopt_presented_handoff(
            i,
            token,
            metadata,
            duration_ms,
            representation,
            successor_offset_frames,
            sample_rate,
            seek,
            generation_serial,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn adopt_presented_handoff(
    i: &mut Inner,
    token: super::continuity::HandoffToken,
    metadata: PlaybackTrackMetadata,
    duration_ms: u64,
    representation: String,
    successor_offset_frames: u64,
    sample_rate: u32,
    seek: SeekCapability,
    generation_serial: &AtomicU64,
) -> PResult<()> {
    if token.instance_id != i.instance_id
        || token.session_id != i.session.session_id
        || token.queue_revision != i.session.queue_revision
        || token.control_epoch != i.control_epoch.load(Ordering::Acquire)
        || token.preparation_generation != generation_serial.load(Ordering::Acquire)
        || i.session.current_occurrence_id.as_deref()
            != Some(token.predecessor_occurrence_id.as_str())
        || metadata.source
            != i.db
                .playback_occurrence(&i.session.session_id, &token.successor_occurrence_id)
                .map_err(storage)?
                .ok_or_else(|| PlaybackError::invalid("INVALID_SESSION", "successor absent"))?
                .source
    {
        return Err(PlaybackError::conflict(
            "GENERATION_CONFLICT",
            "presented handoff was superseded",
        ));
    }
    let mut next = i.session.clone();
    next.current_occurrence_id = Some(token.successor_occurrence_id.clone());
    next.position_ms = successor_offset_frames.saturating_mul(1000) / u64::from(sample_rate);
    next.state = if i.output_gate.load(Ordering::Acquire) {
        TransportState::Playing
    } else {
        TransportState::Paused
    };
    next.checkpoint_sequence = next
        .checkpoint_sequence
        .checked_add(1)
        .ok_or_else(|| storage(anyhow::anyhow!("checkpoint sequence overflow")))?;
    if let Err(error) = i.db.persist_playback_terminal(
        &next,
        &token.predecessor_occurrence_id,
        "naturalCompletion",
        None,
    ) {
        let mut terminal = terminal_plan(i, "naturalCompletion", PlaybackStatus::Paused);
        terminal.session = next;
        freeze_terminal(i, terminal);
        return Err(storage(error));
    }
    i.session = next;
    i.checkpointed_position_ms = i.session.position_ms;
    i.playback = PlaybackState {
        status: if i.output_gate.load(Ordering::Acquire) {
            PlaybackStatus::Active
        } else {
            PlaybackStatus::Paused
        },
        can_go_next: i
            .db
            .playback_occurrence(&i.session.session_id, &token.successor_occurrence_id)
            .map_err(storage)?
            .and_then(|current| {
                i.db.playback_successor(&i.session.session_id, current.ordinal)
                    .ok()
                    .flatten()
            })
            .is_some(),
        metadata: Some(metadata),
        duration_ms: Some(duration_ms),
        representation: Some(representation),
        seek,
        pending_seek: None,
        seek_outcome: None,
        error: None,
    };
    if i.playback.can_go_next
        && i.output_gate.load(Ordering::Acquire)
        && let Some(outputs) = i.outputs.as_mut()
    {
        outputs.effect = Some(i.generation_id.clone());
    }
    i.state_sequence = i.state_sequence.saturating_add(1);
    i.dirty = false;
    super::audio::global().acknowledge_handoff(&i.generation_id);
    Ok(())
}

fn complete_occurrence(
    i: &mut Inner,
    position_ms: u64,
    generation_serial: &AtomicU64,
) -> PResult<bool> {
    let occurrence_id = i.session.current_occurrence_id.clone().ok_or_else(|| {
        PlaybackError::invalid("INVALID_SESSION", "completed occurrence is absent")
    })?;
    let current =
        i.db.playback_occurrence(&i.session.session_id, &occurrence_id)
            .map_err(storage)?
            .ok_or_else(|| {
                PlaybackError::invalid("INVALID_SESSION", "completed occurrence is absent")
            })?;
    let successor =
        i.db.playback_successor(&i.session.session_id, current.ordinal)
            .map_err(storage)?;
    let mut terminal = terminal_plan(i, "naturalCompletion", PlaybackStatus::Completed);
    terminal.session.position_ms = position_ms;
    terminal.session.state = TransportState::Paused;
    let advances = successor.is_some();
    if let Some(successor) = successor {
        let resume = matches!(
            i.session.state,
            TransportState::Playing | TransportState::Buffering
        ) || matches!(
            i.playback.status,
            PlaybackStatus::Active | PlaybackStatus::Loading
        );
        terminal.session.current_occurrence_id = Some(successor.occurrence_id);
        terminal.session.position_ms = 0;
        terminal.session.state = if resume {
            TransportState::Buffering
        } else {
            TransportState::Paused
        };
        terminal.status = if resume {
            PlaybackStatus::Loading
        } else {
            PlaybackStatus::Paused
        };
    }
    commit_terminal(i, terminal, generation_serial, false)?;
    if advances
        && i.output_gate.load(Ordering::Acquire)
        && let Some(outputs) = i.outputs.as_mut()
    {
        outputs.effect = Some(i.generation_id.clone());
    }
    Ok(advances)
}

fn terminal_plan(i: &Inner, outcome: &'static str, status: PlaybackStatus) -> PendingTerminal {
    PendingTerminal {
        instance_id: i.instance_id.clone(),
        generation_id: i.generation_id.clone(),
        occurrence_id: i
            .session
            .current_occurrence_id
            .clone()
            .expect("terminal current occurrence"),
        session: i.session.clone(),
        outcome,
        status,
        failure: None,
        resolve_successor: false,
    }
}

/// Persist all parts of a terminal transition before publishing or starting its
/// successor. A failed write retains exactly one immutable, identity-fenced plan.
fn commit_terminal(
    i: &mut Inner,
    mut terminal: PendingTerminal,
    generation_serial: &AtomicU64,
    recovery: bool,
) -> PResult<SessionSnapshot> {
    if terminal.instance_id != i.instance_id
        || terminal.session.session_id != i.session.session_id
        || terminal.session.queue_revision != i.session.queue_revision
        || terminal.generation_id != i.generation_id
        || i.session.current_occurrence_id.as_deref() != Some(&terminal.occurrence_id)
    {
        i.pending_terminal = None;
        return Err(PlaybackError::conflict(
            "GENERATION_CONFLICT",
            "terminal recovery was superseded",
        ));
    }
    if terminal.resolve_successor {
        let successor = i
            .db
            .playback_occurrence(&i.session.session_id, &terminal.occurrence_id)
            .and_then(|current| current.ok_or_else(|| anyhow::anyhow!("current occurrence absent")))
            .and_then(|current| {
                i.db.playback_successor(&i.session.session_id, current.ordinal)
            });
        match successor {
            Ok(Some(next)) => {
                terminal.session.current_occurrence_id = Some(next.occurrence_id);
                terminal.session.position_ms = 0;
                terminal.status = PlaybackStatus::Paused;
            }
            Ok(None) => {}
            Err(error) => {
                freeze_terminal(i, terminal);
                return Err(storage(error));
            }
        }
        terminal.resolve_successor = false;
    }
    let sequence = i
        .state_sequence
        .checked_add(1)
        .filter(|n| *n <= i64::MAX as u64)
        .ok_or_else(|| storage(anyhow::anyhow!("state sequence overflow")))?;
    terminal.session.checkpoint_sequence = i
        .session
        .checkpoint_sequence
        .checked_add(1)
        .filter(|n| *n <= i64::MAX as u64)
        .ok_or_else(|| storage(anyhow::anyhow!("checkpoint sequence overflow")))?;
    if recovery {
        terminal.session.state = TransportState::Paused;
        if terminal.status == PlaybackStatus::Loading {
            terminal.status = PlaybackStatus::Paused;
        }
    }
    // Resolve response data before committing: a later projection failure must
    // never turn a durable transition into an apparent rejected command.
    let projection = (|| -> PResult<SessionSnapshot> {
        let mut response = snapshot(i)?;
        response.current = terminal
            .session
            .current_occurrence_id
            .as_deref()
            .map(|id| {
                i.db.playback_occurrence(&i.session.session_id, id)
                    .map_err(storage)
            })
            .transpose()?
            .flatten();
        if let Some(current) = response.current.as_mut() {
            current.availability = availability(&i.db, &current.source.server_id)?;
        }
        response.playback.can_go_next = if let Some(current) = response.current.as_ref() {
            i.db.playback_successor(&i.session.session_id, current.ordinal)
                .map_err(storage)?
                .is_some()
        } else {
            false
        };
        Ok(response)
    })();
    let mut response = match projection {
        Ok(response) => response,
        Err(error) => {
            freeze_terminal(i, terminal);
            return Err(error);
        }
    };
    if let Err(error) = i.db.persist_playback_terminal(
        &terminal.session,
        &terminal.occurrence_id,
        terminal.outcome,
        terminal.failure.as_ref().map(|f| f.code.as_str()),
    ) {
        freeze_terminal(i, terminal);
        return Err(storage(error));
    }
    let changed = terminal.session.current_occurrence_id != i.session.current_occurrence_id;
    if changed || recovery {
        i.control_epoch.fetch_add(1, Ordering::AcqRel);
        generation_serial.fetch_add(1, Ordering::AcqRel);
        super::audio::global().control(ControlAction::Stop);
        i.generation_id = Uuid::new_v4().to_string();
    }
    if changed {
        i.playback = PlaybackState::default();
    }
    i.session = terminal.session;
    i.playback.status = terminal.status;
    i.playback.error = terminal.failure;
    i.output_gate.store(
        i.session.state == TransportState::Buffering,
        Ordering::Release,
    );
    i.pending_terminal = None;
    i.state_sequence = sequence;
    i.dirty = false;
    i.checkpointed_position_ms = i.session.position_ms;
    i.persistence = Status {
        status: "ok".into(),
        code: None,
    };
    response.generation_id = i.generation_id.clone();
    response.state_sequence = i.state_sequence.to_string();
    response.state = i.session.state;
    response.position_ms = i.session.position_ms;
    response.checkpointed_position_ms = i.checkpointed_position_ms;
    response.persistence = i.persistence.clone();
    let can_go_next = response.playback.can_go_next;
    response.playback = i.playback.clone();
    response.playback.can_go_next = can_go_next;
    response.output = i.output.clone();
    response.resume_epoch = i.control_epoch.load(Ordering::Acquire);
    response.seek_epoch = response.resume_epoch;
    Ok(response)
}

fn freeze_terminal(i: &mut Inner, terminal: PendingTerminal) {
    i.pending_terminal = Some(terminal);
    i.control_epoch.fetch_add(1, Ordering::AcqRel);
    i.output_gate.store(false, Ordering::Release);
    super::audio::global().control(ControlAction::Stop);
    output_selection::finish_output_preparation(i);
    i.session.state = TransportState::Paused;
    i.playback.status = PlaybackStatus::Error;
    i.playback.error = Some(PlaybackFailure {
        code: "PERSISTENCE_FAILED".into(),
        retryable: true,
    });
    i.persistence = Status {
        status: "error".into(),
        code: Some("PERSISTENCE_FAILED".into()),
    };
    i.state_sequence = i.state_sequence.saturating_add(1);
}

#[cfg(test)]
mod tests {
    use super::*;

    struct OwnerThreadCleanup(PlaybackSession);

    impl Drop for OwnerThreadCleanup {
        fn drop(&mut self) {
            if let Err(error) = self.0.stop_and_join() {
                if !std::thread::panicking() {
                    panic!("test playback owner did not stop: {error:?}");
                }
            }
        }
    }

    fn params(s: &SessionSnapshot, op: SessionOperation) -> ApplySessionParams {
        ApplySessionParams {
            schema_version: 1,
            instance_id: s.instance_id.clone(),
            session_id: s.session_id.clone(),
            command_id: Uuid::new_v4().to_string(),
            expected_queue_revision: s.queue_revision.clone(),
            operation: op,
        }
    }
    fn queued() -> (Arc<Database>, PlaybackSession, SessionSnapshot) {
        let db = Arc::new(Database::memory().unwrap());
        let p = PlaybackSession::restore(db.clone(), "owner".into());
        let s = p.snapshot().unwrap();
        p.apply(params(
            &s,
            SessionOperation::ReplaceQueue {
                sources: vec![TrackSource {
                    server_id: "offline".into(),
                    track_id: "track".into(),
                }],
            },
        ))
        .unwrap();
        let s = p.snapshot().unwrap();
        (db, p, s)
    }

    #[test]
    fn progress_is_nonblocking_coalesced_monotonic_and_ordered() {
        let (db, p, s) = queued();
        let id = &s.current.as_ref().unwrap().occurrence_id;
        let mut i = p.inner.lock().unwrap();
        let connection = db.conn.lock().unwrap();
        // Holding both owner/storage locks must not block the producer.
        p.report_progress(&s.generation_id, id, 1, 1000).unwrap();
        p.report_progress(&s.generation_id, id, 2, 2000).unwrap();
        assert!(p.report_progress(&s.generation_id, id, 3, 1500).is_err());
        assert!(p.report_progress(&s.generation_id, id, 2, 3000).is_err());
        let start = Instant::now();
        let mut last = start;
        sample_progress_if_due(
            &mut i,
            &p.ingress,
            &mut last,
            start + Duration::from_millis(249),
        )
        .unwrap();
        assert_eq!(i.session.position_ms, 0);
        sample_progress_if_due(
            &mut i,
            &p.ingress,
            &mut last,
            start + Duration::from_millis(250),
        )
        .unwrap();
        assert_eq!(i.session.position_ms, 2000);
        assert_eq!(
            i.state_sequence.to_string(),
            (s.state_sequence.parse::<u64>().unwrap() + 1).to_string()
        );
        assert_eq!(i.session.queue_revision.to_string(), s.queue_revision);
        p.report_progress(&s.generation_id, id, 3, 3000).unwrap();
        sample_progress_if_due(
            &mut i,
            &p.ingress,
            &mut last,
            start + Duration::from_millis(499),
        )
        .unwrap();
        assert_eq!(i.session.position_ms, 2000);
        drop(connection);
        drop(i);
        p.shutdown_checkpoint().unwrap();
        assert_eq!(p.snapshot().unwrap().position_ms, 3000);
        assert!(p.report_progress(&s.generation_id, id, 4, 4000).is_err());
        assert!(p.apply(params(&s, SessionOperation::Clear)).is_err());
        p.stop_and_join().unwrap();
        assert_eq!(
            db.load_playback_session().unwrap().unwrap().position_ms,
            3000
        );
    }

    #[test]
    fn delayed_admitted_command_cannot_commit_after_shutdown_snapshot() {
        let (db, p, s) = queued();
        let operations = crate::sync::SyncOperationManager::new();
        let guard = operations.try_admit_mutation().unwrap();
        let command = params(&s, SessionOperation::Clear);
        p.shutdown_checkpoint().unwrap();
        // Simulates spawn_blocking starting after the shutdown mailbox drain.
        assert_eq!(
            p.apply_with_guard(command, Some(guard)).unwrap_err().code,
            "DAEMON_STOPPED"
        );
        assert_eq!(
            db.load_playback_session()
                .unwrap()
                .unwrap()
                .queue_revision
                .to_string(),
            s.queue_revision
        );
        p.stop_and_join().unwrap();
    }

    #[test]
    fn invalid_stored_state_is_rejected_on_startup_and_retry_without_writes() {
        for sql in [
            "UPDATE playback_sessions SET transport_state='idle'",
            "UPDATE playback_sessions SET position_ms=9007199254740992",
            "PRAGMA ignore_check_constraints=ON; UPDATE playback_sessions SET queue_revision=-1",
            "PRAGMA ignore_check_constraints=ON; UPDATE playback_sessions SET checkpoint_sequence=-1",
            "UPDATE playback_sessions SET session_id='bad'; UPDATE playback_occurrences SET session_id='bad'",
            "DELETE FROM playback_occurrences; UPDATE playback_sessions SET current_occurrence_id=NULL,position_ms=0,transport_state='playing'",
        ] {
            let (db, p, _) = queued();
            p.stop_and_join().unwrap();
            db.conn.lock().unwrap().execute_batch(sql).unwrap();
            let before = db.conn.lock().unwrap().total_changes();
            let restored = PlaybackSession::restore(db.clone(), "new-owner".into());
            assert_eq!(
                restored.snapshot().unwrap().restoration.status,
                "error",
                "{sql}"
            );
            assert!(restored.retry_restore().is_err(), "{sql}");
            restored.final_checkpoint().unwrap();
            assert_eq!(
                db.conn.lock().unwrap().total_changes(),
                before,
                "invalid evidence changed: {sql}"
            );
            restored.stop_and_join().unwrap();
        }
    }

    #[test]
    fn unsupported_schema_without_occurrence_table_has_readable_diagnostics() {
        let db = Arc::new(Database::memory().unwrap());
        db.init_playback().unwrap();
        db.conn
            .lock()
            .unwrap()
            .execute_batch("UPDATE playback_schema SET version=99; DROP TABLE playback_occurrences")
            .unwrap();
        let p = PlaybackSession::restore(db.clone(), "owner".into());
        let s = p.snapshot().unwrap();
        assert_eq!(
            s.restoration.code.as_deref(),
            Some("UNSUPPORTED_PLAYBACK_VERSION")
        );
        assert!(s.occurrences.is_empty());
        assert_eq!(p.health().restoration.status, "error");
        p.shutdown_checkpoint().unwrap();
        assert_eq!(
            db.conn
                .lock()
                .unwrap()
                .query_row("SELECT version FROM playback_schema", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            99
        );
        p.stop_and_join().unwrap();
    }

    #[test]
    fn transient_initial_insert_failure_can_be_retried_but_orphans_are_preserved() {
        let db = Arc::new(Database::memory().unwrap());
        db.init_playback().unwrap();
        db.conn.lock().unwrap().execute_batch("CREATE TRIGGER fail_initial BEFORE INSERT ON playback_sessions BEGIN SELECT RAISE(ABORT,'test'); END").unwrap();
        let p = PlaybackSession::restore(db.clone(), "owner".into());
        assert_eq!(p.snapshot().unwrap().restoration.status, "error");
        db.conn
            .lock()
            .unwrap()
            .execute_batch("DROP TRIGGER fail_initial")
            .unwrap();
        assert_eq!(p.retry_restore().unwrap().restoration.status, "ok");
        p.stop_and_join().unwrap();
        let (db, p, _) = queued();
        p.stop_and_join().unwrap();
        db.conn
            .lock()
            .unwrap()
            .execute_batch("DELETE FROM playback_sessions")
            .unwrap();
        let restored = PlaybackSession::restore(db.clone(), "new-owner".into());
        assert_eq!(restored.snapshot().unwrap().restoration.status, "error");
        assert!(restored.retry_restore().is_err());
        assert_eq!(
            db.conn
                .lock()
                .unwrap()
                .query_row("SELECT COUNT(*) FROM playback_occurrences", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        restored.stop_and_join().unwrap();
    }

    #[test]
    fn failed_restore_response_does_not_publish_partial_live_state() {
        let (db, p, _) = queued();
        p.stop_and_join().unwrap();
        db.conn
            .lock()
            .unwrap()
            .execute_batch("UPDATE playback_schema SET version=99")
            .unwrap();
        let restored = PlaybackSession::restore(db.clone(), "new-owner".into());
        assert_eq!(restored.health().restoration.status, "error");
        db.conn.lock().unwrap().execute_batch("UPDATE playback_schema SET version=1; ALTER TABLE server_config RENAME TO unavailable_servers").unwrap();
        assert!(restored.retry_restore().is_err());
        assert_eq!(restored.health().restoration.status, "error");
        db.conn
            .lock()
            .unwrap()
            .execute_batch("ALTER TABLE unavailable_servers RENAME TO server_config")
            .unwrap();
        let s = restored.retry_restore().unwrap();
        restored
            .report_progress(&s.generation_id, &s.current.unwrap().occurrence_id, 1, 1000)
            .unwrap();
        restored.shutdown_checkpoint().unwrap();
        restored.stop_and_join().unwrap();
        assert_eq!(
            db.load_playback_session().unwrap().unwrap().position_ms,
            1000
        );
    }

    #[test]
    fn failed_availability_lookup_never_reports_failure_after_committing_an_append() {
        let (db, p, s) = queued();
        db.conn
            .lock()
            .unwrap()
            .execute_batch("ALTER TABLE server_config RENAME TO unavailable_servers")
            .unwrap();
        let operation = SessionOperation::AppendQueue {
            sources: vec![TrackSource {
                server_id: "s".into(),
                track_id: "t".into(),
            }],
        };
        assert_eq!(
            p.apply(params(&s, operation.clone())).unwrap_err().code,
            "PERSISTENCE_FAILED"
        );
        assert_eq!(db.playback_count(&s.session_id).unwrap(), 1);
        assert_eq!(
            db.load_playback_session()
                .unwrap()
                .unwrap()
                .queue_revision
                .to_string(),
            s.queue_revision
        );
        db.conn
            .lock()
            .unwrap()
            .execute_batch("ALTER TABLE unavailable_servers RENAME TO server_config")
            .unwrap();
        p.apply(params(&s, operation)).unwrap();
        assert_eq!(db.playback_count(&s.session_id).unwrap(), 2);
        p.stop_and_join().unwrap();
    }

    #[test]
    fn structural_success_clears_checkpoint_failure_and_replay_returns_current_metadata() {
        let (db, p, s) = queued();
        p.report_progress(
            &s.generation_id,
            &s.current.as_ref().unwrap().occurrence_id,
            1,
            1000,
        )
        .unwrap();
        db.conn.lock().unwrap().execute_batch("CREATE TRIGGER fail_position BEFORE UPDATE OF position_ms ON playback_sessions BEGIN SELECT RAISE(ABORT,'test'); END").unwrap();
        assert!(p.final_checkpoint().is_err());
        assert!(p.retry_restore().is_err());
        db.conn
            .lock()
            .unwrap()
            .execute_batch("DROP TRIGGER fail_position")
            .unwrap();
        let command = params(&s, SessionOperation::AppendQueue { sources: vec![] });
        let first = p.apply(command.clone()).unwrap();
        let s = p.snapshot().unwrap();
        assert_eq!(s.persistence.status, "ok");
        assert_eq!(s.position_ms, 1000);
        assert_eq!(s.checkpointed_position_ms, 1000);
        assert_eq!(s.generation_id, first.generation_id);
        p.apply(params(&s, SessionOperation::Clear)).unwrap();
        let current = p.snapshot().unwrap();
        let replay = p.apply(command).unwrap();
        assert_eq!(replay.queue_revision, first.queue_revision);
        assert_eq!(
            replay.current_metadata.queue_revision,
            current.queue_revision
        );
        let conflict = p.apply(params(&s, SessionOperation::Clear)).unwrap_err();
        assert_eq!(
            conflict.authoritative.unwrap().queue_revision,
            current.queue_revision
        );
        p.stop_and_join().unwrap();
    }

    #[test]
    fn cursor_and_revision_reject_noncanonical_or_out_of_range_values() {
        let (_, p, s) = queued();
        for number in [
            "+1",
            "01",
            "18446744073709551615",
            "9223372036854775808",
            "-1",
        ] {
            assert!(parse_revision(number).is_err());
            let result = p.list(ListOccurrencesParams {
                schema_version: 1,
                session_id: s.session_id.clone(),
                expected_queue_revision: s.queue_revision.clone(),
                cursor: Some(format!("{}:{}:{number}", s.session_id, s.queue_revision)),
                limit: Some(1),
            });
            assert_eq!(result.unwrap_err().code, "INVALID_CURSOR");
        }
        p.stop_and_join().unwrap();
    }

    #[test]
    fn paused_round_trip_preserves_repeats_and_identity() {
        let db = Arc::new(Database::memory().unwrap());
        let p = PlaybackSession::restore(db.clone(), "owner-a".into());
        let s = p.snapshot().unwrap();
        let source = TrackSource {
            server_id: "portable".into(),
            track_id: "track".into(),
        };
        let r = p
            .apply(params(
                &s,
                SessionOperation::ReplaceQueue {
                    sources: vec![source.clone(), source],
                },
            ))
            .unwrap();
        assert_ne!(
            r.assigned_occurrences[0].occurrence_id,
            r.assigned_occurrences[1].occurrence_id
        );
        let restored = PlaybackSession::restore(db, "owner-b".into())
            .snapshot()
            .unwrap();
        assert_eq!(restored.state, TransportState::Paused);
        assert_eq!(restored.occurrences, r.assigned_occurrences);
    }
    #[test]
    fn stale_revision_and_command_reuse_are_rejected() {
        let db = Arc::new(Database::memory().unwrap());
        let p = PlaybackSession::restore(db, "owner".into());
        let s = p.snapshot().unwrap();
        let mut cmd = params(&s, SessionOperation::Clear);
        p.apply(cmd.clone()).unwrap();
        assert_eq!(p.apply(cmd.clone()).unwrap().queue_revision, "1");
        cmd.operation = SessionOperation::AppendQueue { sources: vec![] };
        assert_eq!(p.apply(cmd).unwrap_err().code, "COMMAND_ID_REUSED");
        let stale = params(&s, SessionOperation::Clear);
        assert_eq!(p.apply(stale).unwrap_err().code, "QUEUE_REVISION_CONFLICT");
    }
    #[test]
    fn explicit_clear_is_durable_idle() {
        let db = Arc::new(Database::memory().unwrap());
        let p = PlaybackSession::restore(db.clone(), "a".into());
        let s = p.snapshot().unwrap();
        let source = TrackSource {
            server_id: "s".into(),
            track_id: "t".into(),
        };
        p.apply(params(
            &s,
            SessionOperation::ReplaceQueue {
                sources: vec![source],
            },
        ))
        .unwrap();
        let s = p.snapshot().unwrap();
        p.apply(params(&s, SessionOperation::Clear)).unwrap();
        let s = PlaybackSession::restore(db, "b".into()).snapshot().unwrap();
        assert_eq!(s.state, TransportState::Idle);
        assert!(s.current.is_none());
        assert!(s.occurrences.is_empty());
        assert_eq!(s.position_ms, 0);
    }
    #[test]
    fn progress_checkpoint_restores_paused_position() {
        let db = Arc::new(Database::memory().unwrap());
        let p = PlaybackSession::restore(db.clone(), "a".into());
        let s = p.snapshot().unwrap();
        let source = TrackSource {
            server_id: "offline-server".into(),
            track_id: "opaque".into(),
        };
        p.apply(params(
            &s,
            SessionOperation::ReplaceQueue {
                sources: vec![source],
            },
        ))
        .unwrap();
        let s = p.snapshot().unwrap();
        let id = s.current.as_ref().unwrap().occurrence_id.clone();
        p.report_progress(&s.generation_id, &id, 1, 4242).unwrap();
        assert_eq!(p.snapshot().unwrap().checkpointed_position_ms, 0);
        p.final_checkpoint().unwrap();
        let restored = PlaybackSession::restore(db, "b".into()).snapshot().unwrap();
        assert_eq!(restored.position_ms, 4242);
        assert_eq!(restored.state, TransportState::Paused);
    }
    #[test]
    fn pages_are_revision_bound_and_bounded() {
        let db = Arc::new(Database::memory().unwrap());
        let p = PlaybackSession::restore(db, "owner".into());
        let s = p.snapshot().unwrap();
        let sources = (0..200)
            .map(|n| TrackSource {
                server_id: "s".into(),
                track_id: n.to_string(),
            })
            .collect();
        p.apply(params(&s, SessionOperation::ReplaceQueue { sources }))
            .unwrap();
        let s = p.snapshot().unwrap();
        assert_eq!(s.occurrences.len(), 100);
        let page = p
            .list(ListOccurrencesParams {
                schema_version: 1,
                session_id: s.session_id.clone(),
                expected_queue_revision: s.queue_revision.clone(),
                cursor: s.next_cursor.clone(),
                limit: Some(200),
            })
            .unwrap();
        assert_eq!(page.occurrences.len(), 100);
        assert!(page.next_cursor.is_none());
        let bad = p
            .list(ListOccurrencesParams {
                schema_version: 1,
                session_id: s.session_id,
                expected_queue_revision: s.queue_revision,
                cursor: Some("foreign:0:0".into()),
                limit: None,
            })
            .unwrap_err();
        assert_eq!(bad.code, "INVALID_CURSOR");
    }
    #[test]
    fn contract_json_is_exact_camel_case() {
        let value = serde_json::to_value(TrackSource {
            server_id: "s".into(),
            track_id: "t".into(),
        })
        .unwrap();
        assert_eq!(value, serde_json::json!({"serverId":"s","trackId":"t"}));
        assert!(
            serde_json::from_value::<TrackSource>(
                serde_json::json!({"serverId":"s","trackId":"t","url":"secret"})
            )
            .is_err()
        );
    }

    #[test]
    fn real_file_restart_and_ten_thousand_occurrence_paging() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("playback.db");
        let db = Arc::new(Database::new(path.clone()).unwrap());
        db.init_playback().unwrap();
        let session = PersistedSession {
            session_id: Uuid::new_v4().to_string(),
            queue_revision: 7,
            checkpoint_sequence: 3,
            state: TransportState::Playing,
            current_occurrence_id: None,
            position_ms: 0,
            album_context: None,
        };
        let mut occurrences: Vec<_> = (0..10_000)
            .map(|ordinal| Occurrence {
                occurrence_id: Uuid::new_v4().to_string(),
                ordinal,
                source: TrackSource {
                    server_id: "offline".into(),
                    track_id: (ordinal % 17).to_string(),
                },
                availability: SourceAvailability::NotConfigured,
            })
            .collect();
        let mut session = session;
        session.current_occurrence_id = Some(occurrences[4321].occurrence_id.clone());
        session.position_ms = 9876;
        db.persist_playback_structure(&session, &occurrences)
            .unwrap();
        drop(db);

        let reopened = Arc::new(Database::new(path).unwrap());
        let restored = PlaybackSession::restore(reopened.clone(), "new-instance".into());
        let mut snapshot = restored.snapshot().unwrap();
        assert_eq!(snapshot.state, TransportState::Paused);
        assert_eq!(snapshot.position_ms, 9876);
        assert_eq!(snapshot.total_occurrence_count, 10_000);
        assert!(
            snapshot.playback.can_go_next,
            "successor lookup must not depend on the first paged snapshot"
        );
        let mut seen = snapshot.occurrences.len();
        while let Some(cursor) = snapshot.next_cursor.take() {
            let page = restored
                .list(ListOccurrencesParams {
                    schema_version: 1,
                    session_id: snapshot.session_id.clone(),
                    expected_queue_revision: snapshot.queue_revision.clone(),
                    cursor: Some(cursor),
                    limit: Some(200),
                })
                .unwrap();
            seen += page.occurrences.len();
            snapshot.next_cursor = page.next_cursor;
        }
        assert_eq!(seen, 10_000);

        let before_append = reopened.conn.lock().unwrap().total_changes();
        let append_sources = (0..200)
            .map(|ordinal| TrackSource {
                server_id: "offline".into(),
                track_id: format!("appended-{ordinal}"),
            })
            .collect();
        restored
            .apply(params(
                &snapshot,
                SessionOperation::AppendQueue {
                    sources: append_sources,
                },
            ))
            .unwrap();
        let after_append = reopened.conn.lock().unwrap().total_changes();
        assert_eq!(
            after_append - before_append,
            201,
            "append must update one session row and insert only its bounded batch"
        );

        let snapshot_after_append = restored.snapshot().unwrap();
        assert_eq!(snapshot_after_append.total_occurrence_count, 10_200);
        let last_original_id = occurrences[9999].occurrence_id.clone();
        let before_select = reopened.conn.lock().unwrap().total_changes();
        restored
            .apply(params(
                &snapshot_after_append,
                SessionOperation::SelectCurrent {
                    occurrence_id: last_original_id.clone(),
                },
            ))
            .unwrap();
        let after_select = reopened.conn.lock().unwrap().total_changes();
        assert_eq!(after_select - before_select, 1);
        let selected = restored.snapshot().unwrap();
        assert_eq!(selected.current.unwrap().occurrence_id, last_original_id);
        assert_eq!(
            selected.queue_revision, snapshot_after_append.queue_revision,
            "current selection must not revise the queue"
        );
        occurrences.clear();
    }

    #[test]
    fn unsupported_version_preserves_evidence_and_blocks_mutation() {
        let db = Arc::new(Database::memory().unwrap());
        db.init_playback().unwrap();
        {
            let conn = db.conn.lock().unwrap();
            conn.execute("UPDATE playback_schema SET version=99", [])
                .unwrap();
        }
        let playback = PlaybackSession::restore(db.clone(), "owner".into());
        let snapshot = playback.snapshot().unwrap();
        assert_eq!(snapshot.restoration.status, "error");
        assert_eq!(
            snapshot.restoration.code.as_deref(),
            Some("UNSUPPORTED_PLAYBACK_VERSION")
        );
        assert_eq!(
            db.conn
                .lock()
                .unwrap()
                .query_row("SELECT version FROM playback_schema", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            99
        );
        assert_eq!(
            playback
                .apply(params(&snapshot, SessionOperation::Clear))
                .unwrap_err()
                .code,
            "RESTORE_FAILED"
        );
        playback.final_checkpoint().unwrap();
        assert_eq!(
            db.conn
                .lock()
                .unwrap()
                .query_row("SELECT version FROM playback_schema", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            99
        );
    }

    #[test]
    fn mailbox_admits_64_pending_commands_and_rejects_the_next() {
        let db = Arc::new(Database::memory().unwrap());
        let playback = PlaybackSession::restore(db.clone(), "owner".into());
        let _cleanup = OwnerThreadCleanup(playback.clone());
        let snapshot = playback.snapshot().unwrap();
        // Block Clear's database write, not the owner's pre-command maintenance.
        // Holding inner here can prevent the owner from reaching command receive,
        // so executing would never become true even though the command is queued.
        let guard = db.conn.lock().unwrap();

        let first = playback
            .admit_apply(params(&snapshot, SessionOperation::Clear), None)
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        while !playback.executing.load(Ordering::Acquire) {
            assert!(Instant::now() < deadline, "owner did not begin the command");
            std::thread::yield_now();
        }

        let mut pending = Vec::new();
        for _ in 0..64 {
            pending.push(
                playback
                    .admit_apply(params(&snapshot, SessionOperation::Clear), None)
                    .unwrap(),
            );
        }
        let overflow = playback
            .admit_apply(params(&snapshot, SessionOperation::Clear), None)
            .unwrap_err();
        assert_eq!(overflow.code, "PLAYBACK_BUSY");

        let checkpoint_session = playback.clone();
        let checkpoint = std::thread::spawn(move || checkpoint_session.final_checkpoint());
        drop(guard);
        assert!(first.recv().unwrap().is_ok());
        for reply in pending {
            let _ = reply.recv().unwrap();
        }
        checkpoint.join().unwrap().unwrap();
    }

    #[tokio::test]
    async fn dropped_caller_keeps_mutation_admitted_until_owner_finishes() {
        let db = Arc::new(Database::memory().unwrap());
        let playback = PlaybackSession::restore(db.clone(), "owner".into());
        // Declare cleanup before locks so their guards drop before joining the owner.
        let _cleanup = OwnerThreadCleanup(playback.clone());
        let initial = playback.snapshot().unwrap();
        playback
            .apply(params(
                &initial,
                SessionOperation::ReplaceQueue {
                    sources: vec![TrackSource {
                        server_id: "offline".into(),
                        track_id: "track".into(),
                    }],
                },
            ))
            .unwrap();
        let snapshot = playback.snapshot().unwrap();
        let operations = Arc::new(crate::sync::SyncOperationManager::new());
        let mutation_guard = operations.try_admit_mutation().unwrap();
        // Block Clear's database write only after the owner has dequeued it.
        // Locking inner would also block the owner's pre-command maintenance.
        let db_guard = db.conn.lock().unwrap();
        let reply = playback
            .admit_apply(
                params(&snapshot, SessionOperation::Clear),
                Some(mutation_guard),
            )
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        while !playback.executing.load(Ordering::Acquire) {
            assert!(Instant::now() < deadline, "owner did not begin the command");
            std::thread::yield_now();
        }
        drop(reply);
        let fencing = operations.begin_shutdown_fence();
        assert_eq!(fencing.pending_mutation_count, 1);
        drop(db_guard);

        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            let snapshot = operations.shutdown_snapshot().await.unwrap();
            if snapshot.pending_mutation_count == 0 {
                break;
            }
            assert!(Instant::now() < deadline, "mutation guard was not released");
            tokio::task::yield_now().await;
        }

        let completed = playback.snapshot().unwrap();
        assert_eq!(completed.queue_revision, "2");
        assert_eq!(completed.state, TransportState::Idle);
        assert!(completed.current.is_none());
        assert!(completed.occurrences.is_empty());
    }

    #[test]
    fn saturated_mailbox_cannot_block_shutdown_or_commit_queued_work_after_snapshot() {
        let db = Arc::new(Database::memory().unwrap());
        let playback = PlaybackSession::restore(db, "owner".into());
        let snapshot = playback.snapshot().unwrap();
        let guard = playback.inner.lock().unwrap();
        let executing = playback
            .admit_apply(params(&snapshot, SessionOperation::Clear), None)
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        while !playback.executing.load(Ordering::Acquire) {
            assert!(Instant::now() < deadline, "owner did not begin the command");
            std::thread::yield_now();
        }
        let queued: Vec<_> = (0..64)
            .map(|_| {
                playback
                    .admit_apply(params(&snapshot, SessionOperation::Clear), None)
                    .unwrap()
            })
            .collect();
        let shutdown_session = playback.clone();
        let shutdown = std::thread::spawn(move || shutdown_session.shutdown_checkpoint());
        std::thread::sleep(Duration::from_millis(10));
        drop(guard);
        assert_eq!(
            executing.recv().unwrap().unwrap_err().code,
            "DAEMON_STOPPED"
        );
        shutdown.join().unwrap().unwrap();
        for reply in queued {
            assert_eq!(reply.recv().unwrap().unwrap_err().code, "DAEMON_STOPPED");
        }
        assert_eq!(playback.snapshot().unwrap().queue_revision, "0");
        playback.stop_and_join().unwrap();
    }

    #[test]
    fn play_track_is_atomic_and_controls_are_generation_fenced() {
        let db = Arc::new(Database::memory().unwrap());
        let playback = PlaybackSession::restore(db, "owner".into());
        let initial = playback.snapshot().unwrap();
        playback
            .apply(params(
                &initial,
                SessionOperation::PlayTrack {
                    source: TrackSource {
                        server_id: "portable-a".into(),
                        track_id: "same-id".into(),
                    },
                },
            ))
            .unwrap();
        let loading = playback.snapshot().unwrap();
        assert_eq!(loading.total_occurrence_count, 1);
        assert_eq!(loading.state, TransportState::Buffering);
        assert_eq!(loading.playback.status, PlaybackStatus::Loading);
        let occurrence_id = loading.current.as_ref().unwrap().occurrence_id.clone();
        let control = |snapshot: &SessionSnapshot, action| ControlParams {
            schema_version: 1,
            instance_id: snapshot.instance_id.clone(),
            session_id: snapshot.session_id.clone(),
            command_id: Uuid::new_v4().to_string(),
            expected_generation_id: snapshot.generation_id.clone(),
            occurrence_id: occurrence_id.clone(),
            action,
        };
        let paused = playback
            .control_with_guard(control(&loading, ControlAction::Pause), None)
            .unwrap();
        assert_eq!(paused.playback.status, PlaybackStatus::Paused);
        let stopped = playback
            .control_with_guard(control(&paused, ControlAction::Stop), None)
            .unwrap();
        assert_eq!(stopped.position_ms, 0);
        assert_eq!(stopped.playback.status, PlaybackStatus::Stopped);
        assert_eq!(
            playback
                .control_with_guard(control(&paused, ControlAction::Resume), None)
                .unwrap_err()
                .code,
            "GENERATION_CONFLICT"
        );
        playback.stop_and_join().unwrap();
    }

    #[test]
    fn album_completion_advances_once_and_final_completion_is_consumed() {
        let db = Arc::new(Database::memory().unwrap());
        let playback = PlaybackSession::restore(db.clone(), "owner".into());
        let initial = playback.snapshot().unwrap();
        let applied = playback
            .apply(params(
                &initial,
                SessionOperation::PlayAlbum {
                    sources: vec![
                        TrackSource {
                            server_id: "server".into(),
                            track_id: "one".into(),
                        },
                        TrackSource {
                            server_id: "server".into(),
                            track_id: "two".into(),
                        },
                    ],
                },
            ))
            .unwrap();
        let first = applied.assigned_occurrences[0].clone();
        let second = applied.assigned_occurrences[1].clone();
        playback.publish_event(applied.generation_id.clone(), PlaybackEvent::Active);
        playback.publish_event(
            applied.generation_id.clone(),
            PlaybackEvent::Completed { position_ms: 123 },
        );
        std::thread::sleep(Duration::from_millis(50));
        let advanced = playback.snapshot().unwrap();
        assert_eq!(
            advanced.current.as_ref().unwrap().occurrence_id,
            second.occurrence_id
        );
        assert_eq!(advanced.position_ms, 0);
        assert_eq!(
            db.playback_outcome(&first.occurrence_id)
                .unwrap()
                .as_deref(),
            Some("naturalCompletion")
        );
        playback.publish_event(
            applied.generation_id,
            PlaybackEvent::Completed { position_ms: 999 },
        );
        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(
            playback.snapshot().unwrap().current.unwrap().occurrence_id,
            second.occurrence_id
        );
        playback.publish_event(
            advanced.generation_id,
            PlaybackEvent::Completed { position_ms: 456 },
        );
        std::thread::sleep(Duration::from_millis(50));
        let completed = playback.snapshot().unwrap();
        assert_eq!(completed.playback.status, PlaybackStatus::Completed);
        assert_eq!(
            completed.current.unwrap().occurrence_id,
            second.occurrence_id
        );
        assert_eq!(completed.position_ms, 456);
        assert!(!completed.playback.can_go_next);
        assert_eq!(
            db.playback_outcome(&second.occurrence_id)
                .unwrap()
                .as_deref(),
            Some("naturalCompletion")
        );
        playback.stop_and_join().unwrap();
    }

    #[test]
    fn presented_handoff_adopts_successor_without_rotating_generation_or_start_effect() {
        let db = Arc::new(Database::memory().unwrap());
        let playback = PlaybackSession::restore(db.clone(), "owner".into());
        let initial = playback.snapshot().unwrap();
        let applied = playback
            .apply(params(
                &initial,
                SessionOperation::PlayAlbum {
                    sources: vec![
                        TrackSource {
                            server_id: "server".into(),
                            track_id: "same".into(),
                        },
                        TrackSource {
                            server_id: "server".into(),
                            track_id: "same".into(),
                        },
                    ],
                },
            ))
            .unwrap();
        let first = applied.assigned_occurrences[0].clone();
        let second = applied.assigned_occurrences[1].clone();
        let candidate = playback
            .successor_candidate(&applied.generation_id, playback.control_epoch())
            .unwrap();
        assert_eq!(candidate.predecessor_occurrence_id, first.occurrence_id);
        assert_eq!(candidate.successor.occurrence_id, second.occurrence_id);
        let token = super::super::continuity::HandoffToken {
            instance_id: candidate.instance_id,
            session_id: candidate.session_id,
            predecessor_occurrence_id: candidate.predecessor_occurrence_id,
            successor_occurrence_id: candidate.successor.occurrence_id,
            queue_revision: candidate.queue_revision,
            control_epoch: candidate.control_epoch,
            preparation_generation: candidate.preparation_generation,
            output_epoch: 9,
        };
        let event = PlaybackEvent::HandoffPresented {
            token: token.clone(),
            metadata: PlaybackTrackMetadata {
                source: second.source.clone(),
                title: "Second".into(),
                artist: None,
                album: None,
            },
            duration_ms: 1_000,
            representation: "flac".into(),
            successor_offset_frames: 12_000,
            sample_rate: 48_000,
            seek: SeekCapability::jellyfin_pcm_wav(),
        };
        playback.publish_event(applied.generation_id.clone(), event.clone());
        playback.publish_event(applied.generation_id.clone(), event);
        std::thread::sleep(Duration::from_millis(50));
        let adopted = playback.snapshot().unwrap();
        assert_eq!(adopted.generation_id, applied.generation_id);
        assert_eq!(adopted.current.unwrap().occurrence_id, second.occurrence_id);
        assert_eq!(adopted.position_ms, 250);
        assert!(adopted.playback.seek.available);
        let stored = db.load_playback_session().unwrap().unwrap();
        assert_eq!(
            stored.current_occurrence_id.as_deref(),
            Some(second.occurrence_id.as_str())
        );
        assert_eq!(stored.position_ms, 250);
        assert_eq!(
            db.playback_outcome(&first.occurrence_id)
                .unwrap()
                .as_deref(),
            Some("naturalCompletion")
        );
        playback.stop_and_join().unwrap();
    }

    #[test]
    fn retried_failure_and_completed_replay_each_finish_as_a_new_attempt() {
        let (db, playback, _) = queued();
        let cleanup = OwnerThreadCleanup(playback.clone());
        {
            let mut i = playback.inner.lock().unwrap();
            i.session.position_ms = 321;
            let mut terminal = terminal_plan(&i, "technicalFailure", PlaybackStatus::Error);
            terminal.session.state = TransportState::Paused;
            terminal.failure = Some(PlaybackFailure {
                code: "DECODE_FAILED".into(),
                retryable: true,
            });
            commit_terminal(&mut i, terminal, &playback.generation_serial, false).unwrap();
            let retry = transport(&snapshot(&i).unwrap(), ControlAction::Retry);
            let resumed = control_inner(&mut i, &retry, &playback.generation_serial).unwrap();
            assert!(resumed.resume_audio);
            assert_eq!(resumed.position_ms, 321);
            let occurrence = resumed.current.unwrap().occurrence_id;
            assert_eq!(db.playback_outcome(&occurrence).unwrap(), None);
            apply_playback_event(&mut i, PlaybackEvent::Active);
            let cleared_reason: Option<String> = db
                .conn
                .lock()
                .unwrap()
                .query_row(
                    "SELECT failure_code FROM playback_occurrences WHERE occurrence_id=?1",
                    [&occurrence],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(cleared_reason, None);
            let mut failed_again = terminal_plan(&i, "technicalFailure", PlaybackStatus::Error);
            failed_again.session.position_ms = 400;
            failed_again.session.state = TransportState::Paused;
            failed_again.failure = Some(PlaybackFailure {
                code: "SOURCE_UNAVAILABLE".into(),
                retryable: true,
            });
            commit_terminal(&mut i, failed_again, &playback.generation_serial, false).unwrap();
            let second_retry = transport(&snapshot(&i).unwrap(), ControlAction::Retry);
            let second_attempt =
                control_inner(&mut i, &second_retry, &playback.generation_serial).unwrap();
            assert_eq!(second_attempt.position_ms, 400);
            assert_eq!(second_attempt.current.unwrap().occurrence_id, occurrence);
            complete_occurrence(&mut i, 999, &playback.generation_serial).unwrap();
            assert_eq!(i.playback.status, PlaybackStatus::Completed);
            let resume = transport(&snapshot(&i).unwrap(), ControlAction::Resume);
            let replayed = control_inner(&mut i, &resume, &playback.generation_serial).unwrap();
            assert_eq!(replayed.position_ms, 0);
            assert_eq!(db.playback_outcome(&occurrence).unwrap(), None);
            complete_occurrence(&mut i, 999, &playback.generation_serial).unwrap();
            assert_eq!(i.playback.status, PlaybackStatus::Completed);
        }
        drop(cleanup);
        let restarted = PlaybackSession::restore(db.clone(), "restart".into());
        let restored = restarted.snapshot().unwrap();
        assert_eq!(restored.state, TransportState::Paused);
        assert_eq!(restored.position_ms, 999);
        assert_eq!(
            db.playback_outcome(&restored.current.unwrap().occurrence_id)
                .unwrap()
                .as_deref(),
            Some("naturalCompletion")
        );
        restarted.stop_and_join().unwrap();
    }

    #[test]
    fn failed_terminal_write_recovers_exact_cursor_without_audio_and_survives_restart() {
        let (db, playback, before) = queued();
        let cleanup = OwnerThreadCleanup(playback.clone());
        {
            let mut i = playback.inner.lock().unwrap();
            db.conn.lock().unwrap().execute_batch("CREATE TRIGGER terminal_rollback BEFORE UPDATE ON playback_sessions BEGIN SELECT RAISE(ABORT,'injected'); END").unwrap();
            assert!(complete_occurrence(&mut i, 875, &playback.generation_serial).is_err());
            assert_eq!(i.session.position_ms, before.position_ms);
            assert_eq!(i.persistence.code.as_deref(), Some("PERSISTENCE_FAILED"));
            assert_eq!(
                db.playback_outcome(&before.current.as_ref().unwrap().occurrence_id)
                    .unwrap(),
                None
            );
            let failed = snapshot(&i).unwrap();
            assert_eq!(
                apply_inner(
                    &mut i,
                    &params(
                        &failed,
                        SessionOperation::AppendQueue {
                            sources: vec![TrackSource {
                                server_id: "offline".into(),
                                track_id: "later".into()
                            }]
                        }
                    ),
                    &playback.generation_serial
                )
                .unwrap_err()
                .code,
                "PERSISTENCE_FAILED"
            );
            assert_eq!(
                control_inner(
                    &mut i,
                    &transport(&failed, ControlAction::Next),
                    &playback.generation_serial
                )
                .unwrap_err()
                .code,
                "PERSISTENCE_FAILED"
            );
            assert_eq!(
                seek_inner(&mut i, &seek(&failed, 1), &playback.generation_serial)
                    .unwrap_err()
                    .code,
                "PERSISTENCE_FAILED"
            );
            assert!(
                control_inner(
                    &mut i,
                    &transport(&failed, ControlAction::Retry),
                    &playback.generation_serial
                )
                .is_err()
            );
            assert!(i.pending_terminal.is_some());
            db.conn
                .lock()
                .unwrap()
                .execute_batch("DROP TRIGGER terminal_rollback")
                .unwrap();
            let recovered = control_inner(
                &mut i,
                &transport(&failed, ControlAction::Retry),
                &playback.generation_serial,
            )
            .unwrap();
            assert_eq!(recovered.position_ms, 875);
            assert_eq!(recovered.playback.status, PlaybackStatus::Completed);
            assert!(!recovered.resume_audio);
            assert!(!i.output_gate.load(Ordering::Acquire));
            assert!(i.pending_terminal.is_none());
        }
        drop(cleanup);
        let restarted = PlaybackSession::restore(db.clone(), "restart".into());
        assert_eq!(restarted.snapshot().unwrap().position_ms, 875);
        assert_eq!(
            db.playback_outcome(&before.current.unwrap().occurrence_id)
                .unwrap()
                .as_deref(),
            Some("naturalCompletion")
        );
        restarted.stop_and_join().unwrap();
    }

    #[test]
    fn failed_technical_terminal_rolls_back_reason_and_cursor_then_recovers_atomically() {
        let (db, playback, before) = queued();
        let cleanup = OwnerThreadCleanup(playback.clone());
        let mut i = playback.inner.lock().unwrap();
        let mut terminal = terminal_plan(&i, "technicalFailure", PlaybackStatus::Error);
        terminal.session.position_ms = 432;
        terminal.session.state = TransportState::Paused;
        terminal.failure = Some(PlaybackFailure {
            code: "DECODE_FAILED".into(),
            retryable: true,
        });
        db.conn.lock().unwrap().execute_batch("CREATE TRIGGER terminal_rollback BEFORE UPDATE ON playback_sessions BEGIN SELECT RAISE(ABORT,'injected'); END").unwrap();
        assert!(commit_terminal(&mut i, terminal, &playback.generation_serial, false).is_err());
        assert_eq!(db.load_playback_session().unwrap().unwrap().position_ms, 0);
        assert_eq!(
            db.playback_outcome(&before.current.as_ref().unwrap().occurrence_id)
                .unwrap(),
            None
        );
        db.conn
            .lock()
            .unwrap()
            .execute_batch("DROP TRIGGER terminal_rollback")
            .unwrap();
        let failed = snapshot(&i).unwrap();
        let recovered = control_inner(
            &mut i,
            &transport(&failed, ControlAction::Retry),
            &playback.generation_serial,
        )
        .unwrap();
        assert!(!recovered.resume_audio);
        assert_eq!(recovered.playback.error.unwrap().code, "DECODE_FAILED");
        let persisted = db.load_playback_session().unwrap().unwrap();
        assert_eq!(persisted.position_ms, 432);
        assert_eq!(persisted.checkpoint_sequence, 1);
        let occurrence_id = before.current.unwrap().occurrence_id;
        let reason: String = db
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT failure_code FROM playback_occurrences WHERE occurrence_id=?1",
                [&occurrence_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(reason, "DECODE_FAILED");
        drop(i);
        drop(cleanup);
        let restarted = PlaybackSession::restore(db.clone(), "restart".into());
        let restored = restarted.snapshot().unwrap();
        assert_eq!(restored.position_ms, 432);
        assert_eq!(restored.state, TransportState::Paused);
        assert_eq!(
            db.playback_outcome(&occurrence_id).unwrap().as_deref(),
            Some("technicalFailure")
        );
        restarted.stop_and_join().unwrap();
    }

    #[test]
    fn stop_or_replacement_discards_failed_terminal_recovery() {
        for replacement in [false, true] {
            let (db, playback, _) = queued();
            let _cleanup = OwnerThreadCleanup(playback.clone());
            let mut i = playback.inner.lock().unwrap();
            db.conn.lock().unwrap().execute_batch("CREATE TRIGGER terminal_rollback BEFORE UPDATE ON playback_sessions BEGIN SELECT RAISE(ABORT,'injected'); END").unwrap();
            assert!(complete_occurrence(&mut i, 875, &playback.generation_serial).is_err());
            let failed = snapshot(&i).unwrap();
            db.conn
                .lock()
                .unwrap()
                .execute_batch("DROP TRIGGER terminal_rollback")
                .unwrap();
            if replacement {
                apply_inner(
                    &mut i,
                    &params(&failed, SessionOperation::Clear),
                    &playback.generation_serial,
                )
                .unwrap();
            } else {
                control_inner(
                    &mut i,
                    &transport(&failed, ControlAction::Stop),
                    &playback.generation_serial,
                )
                .unwrap();
            }
            assert!(i.pending_terminal.is_none());
            assert_eq!(
                control_inner(
                    &mut i,
                    &transport(&failed, ControlAction::Retry),
                    &playback.generation_serial
                )
                .unwrap_err()
                .code,
                "GENERATION_CONFLICT"
            );
            assert_eq!(i.session.position_ms, 0);
        }
    }

    #[test]
    fn stopped_next_preserves_stopped_projection_and_never_dispatches_audio() {
        let (_, playback, before) = queued();
        let _cleanup = OwnerThreadCleanup(playback.clone());
        let mut i = playback.inner.lock().unwrap();
        apply_inner(
            &mut i,
            &params(
                &before,
                SessionOperation::AppendQueue {
                    sources: vec![TrackSource {
                        server_id: "offline".into(),
                        track_id: "two".into(),
                    }],
                },
            ),
            &playback.generation_serial,
        )
        .unwrap();
        let current = snapshot(&i).unwrap();
        let stopped = control_inner(
            &mut i,
            &transport(&current, ControlAction::Stop),
            &playback.generation_serial,
        )
        .unwrap();
        let next = control_inner(
            &mut i,
            &transport(&stopped, ControlAction::Next),
            &playback.generation_serial,
        )
        .unwrap();
        assert_eq!(next.playback.status, PlaybackStatus::Stopped);
        assert_eq!(next.state, TransportState::Paused);
        assert!(!next.resume_audio);
        assert!(!i.output_gate.load(Ordering::Acquire));
    }

    #[test]
    fn failed_next_and_natural_advance_recover_successor_paused_once() {
        for explicit in [false, true] {
            let (db, playback, before) = queued();
            let _cleanup = OwnerThreadCleanup(playback.clone());
            let mut i = playback.inner.lock().unwrap();
            apply_inner(
                &mut i,
                &params(
                    &before,
                    SessionOperation::AppendQueue {
                        sources: vec![TrackSource {
                            server_id: "offline".into(),
                            track_id: "two".into(),
                        }],
                    },
                ),
                &playback.generation_serial,
            )
            .unwrap();
            i.session.state = TransportState::Playing;
            i.playback.status = PlaybackStatus::Active;
            let current = snapshot(&i).unwrap();
            db.conn.lock().unwrap().execute_batch("CREATE TRIGGER terminal_rollback BEFORE UPDATE ON playback_sessions BEGIN SELECT RAISE(ABORT,'injected'); END").unwrap();
            if explicit {
                assert!(
                    control_inner(
                        &mut i,
                        &transport(&current, ControlAction::Next),
                        &playback.generation_serial
                    )
                    .is_err()
                );
            } else {
                assert!(complete_occurrence(&mut i, 1000, &playback.generation_serial).is_err());
            }
            assert_eq!(
                i.session.current_occurrence_id,
                current.current.as_ref().map(|o| o.occurrence_id.clone())
            );
            assert!(!i.output_gate.load(Ordering::Acquire));
            assert_eq!(i.playback.status, PlaybackStatus::Error);
            db.conn
                .lock()
                .unwrap()
                .execute_batch("DROP TRIGGER terminal_rollback")
                .unwrap();
            let failed = snapshot(&i).unwrap();
            let recovered = control_inner(
                &mut i,
                &transport(&failed, ControlAction::Retry),
                &playback.generation_serial,
            )
            .unwrap();
            assert_eq!(recovered.current.unwrap().source.track_id, "two");
            assert_eq!(recovered.position_ms, 0);
            assert_eq!(recovered.playback.status, PlaybackStatus::Paused);
            assert!(!recovered.resume_audio);
            assert_eq!(recovered.queue_revision, current.queue_revision);
            assert_eq!(
                db.playback_outcome(&current.current.unwrap().occurrence_id)
                    .unwrap()
                    .as_deref(),
                Some(if explicit {
                    "explicitSkip"
                } else {
                    "naturalCompletion"
                })
            );
        }
    }

    #[test]
    fn late_progress_cannot_overwrite_committed_failure_or_final_cursor() {
        for failure in [false, true] {
            let (db, playback, _) = queued();
            let _cleanup = OwnerThreadCleanup(playback.clone());
            let mut i = playback.inner.lock().unwrap();
            if failure {
                let mut terminal = terminal_plan(&i, "technicalFailure", PlaybackStatus::Error);
                terminal.session.state = TransportState::Paused;
                terminal.session.position_ms = 456;
                terminal.failure = Some(PlaybackFailure {
                    code: "DECODE_FAILED".into(),
                    retryable: true,
                });
                commit_terminal(&mut i, terminal, &playback.generation_serial, false).unwrap();
            } else {
                complete_occurrence(&mut i, 456, &playback.generation_serial).unwrap();
            }
            reset_ingress(&i, &playback.ingress);
            playback.ingress.lock().unwrap().pending = Some((99, 900));
            sample_progress(&mut i, &playback.ingress).unwrap();
            checkpoint_inner(&mut i).unwrap();
            assert_eq!(i.session.position_ms, 456);
            assert_eq!(i.checkpointed_position_ms, 456);
            assert_eq!(
                db.load_playback_session().unwrap().unwrap().position_ms,
                456
            );
        }
    }

    #[test]
    fn paused_next_is_silent_and_records_explicit_skip() {
        let db = Arc::new(Database::memory().unwrap());
        let playback = PlaybackSession::restore(db.clone(), "owner".into());
        let initial = playback.snapshot().unwrap();
        let applied = playback
            .apply(params(
                &initial,
                SessionOperation::ReplaceQueue {
                    sources: vec![
                        TrackSource {
                            server_id: "server".into(),
                            track_id: "one".into(),
                        },
                        TrackSource {
                            server_id: "server".into(),
                            track_id: "two".into(),
                        },
                    ],
                },
            ))
            .unwrap();
        let before = playback.snapshot().unwrap();
        assert!(before.playback.can_go_next);
        let next = playback
            .control_with_guard(
                ControlParams {
                    schema_version: 1,
                    instance_id: before.instance_id,
                    session_id: before.session_id,
                    command_id: Uuid::new_v4().to_string(),
                    expected_generation_id: before.generation_id,
                    occurrence_id: before.current.unwrap().occurrence_id,
                    action: ControlAction::Next,
                },
                None,
            )
            .unwrap();
        assert_eq!(next.state, TransportState::Paused);
        assert!(!next.resume_audio);
        assert_eq!(
            next.current.unwrap().occurrence_id,
            applied.assigned_occurrences[1].occurrence_id
        );
        assert_eq!(
            db.playback_outcome(&applied.assigned_occurrences[0].occurrence_id)
                .unwrap()
                .as_deref(),
            Some("explicitSkip")
        );
        playback.stop_and_join().unwrap();
    }

    fn transport(snapshot: &SessionSnapshot, action: ControlAction) -> ControlParams {
        ControlParams {
            schema_version: 1,
            instance_id: snapshot.instance_id.clone(),
            session_id: snapshot.session_id.clone(),
            command_id: Uuid::new_v4().to_string(),
            expected_generation_id: snapshot.generation_id.clone(),
            occurrence_id: snapshot.current.as_ref().unwrap().occurrence_id.clone(),
            action,
        }
    }

    #[test]
    fn review_pause_rejects_late_worker_activity() {
        let (_, playback, selected) = queued();
        playback
            .control_with_guard(transport(&selected, ControlAction::Pause), None)
            .unwrap();
        playback.publish_event(selected.generation_id.clone(), PlaybackEvent::Active);
        playback.publish_event(selected.generation_id.clone(), PlaybackEvent::Buffering);
        assert_eq!(playback.snapshot().unwrap().state, TransportState::Paused);
        playback.stop_and_join().unwrap();
    }

    #[test]
    fn review_resume_rejects_activity_observed_before_pause() {
        let (_, playback, selected) = queued();
        let old_epoch = playback.control_epoch();
        playback
            .control_with_guard(transport(&selected, ControlAction::Pause), None)
            .unwrap();
        playback
            .control_with_guard(transport(&selected, ControlAction::Resume), None)
            .unwrap();
        playback.publish_event_at_epoch(
            selected.generation_id.clone(),
            PlaybackEvent::Active,
            old_epoch,
        );
        assert_eq!(
            playback.snapshot().unwrap().playback.status,
            PlaybackStatus::Loading
        );
        playback.stop_and_join().unwrap();
    }

    #[test]
    fn review_command_identity_is_shared_across_apply_and_control() {
        let (_, playback, selected) = queued();
        let mut pause = transport(&selected, ControlAction::Pause);
        let apply = params(
            &selected,
            SessionOperation::PlayTrack {
                source: TrackSource {
                    server_id: "server".into(),
                    track_id: "track".into(),
                },
            },
        );
        pause.command_id = apply.command_id.clone();
        playback.control_with_guard(pause, None).unwrap();
        assert_eq!(playback.apply(apply).unwrap_err().code, "COMMAND_ID_REUSED");
        let apply = params(
            &selected,
            SessionOperation::PlayTrack {
                source: TrackSource {
                    server_id: "server".into(),
                    track_id: "track".into(),
                },
            },
        );
        let id = apply.command_id.clone();
        playback.apply(apply).unwrap();
        let mut pause = transport(&playback.snapshot().unwrap(), ControlAction::Pause);
        pause.command_id = id;
        assert_eq!(
            playback.control_with_guard(pause, None).unwrap_err().code,
            "COMMAND_ID_REUSED"
        );
        playback.stop_and_join().unwrap();
    }

    #[test]
    fn review_resume_active_keeps_active_state() {
        let (_, playback, selected) = queued();
        playback
            .control_with_guard(transport(&selected, ControlAction::Resume), None)
            .unwrap();
        playback.publish_event(selected.generation_id.clone(), PlaybackEvent::Active);
        let active = playback.snapshot().unwrap();
        let resumed = playback
            .control_with_guard(transport(&active, ControlAction::Resume), None)
            .unwrap();
        assert_eq!(resumed.playback.status, PlaybackStatus::Active);
        assert_eq!(resumed.state_sequence, active.state_sequence);
        playback.stop_and_join().unwrap();
    }

    #[test]
    fn review_replayed_commands_never_reissue_audio_effects() {
        let (_, playback, selected) = queued();
        let play = params(
            &selected,
            SessionOperation::PlayTrack {
                source: TrackSource {
                    server_id: "server".into(),
                    track_id: "track".into(),
                },
            },
        );
        assert!(playback.apply(play.clone()).unwrap().start_audio);
        assert!(!playback.apply(play).unwrap().start_audio);
        let loading = playback.snapshot().unwrap();
        let paused = playback
            .control_with_guard(transport(&loading, ControlAction::Pause), None)
            .unwrap();
        let resume = transport(&paused, ControlAction::Resume);
        let accepted = playback.control_with_guard(resume.clone(), None).unwrap();
        assert!(accepted.resume_audio);
        playback
            .control_with_guard(transport(&accepted, ControlAction::Pause), None)
            .unwrap();
        assert!(
            !playback
                .control_with_guard(resume, None)
                .unwrap()
                .resume_audio
        );
        assert!(!playback.output_gate().load(Ordering::Acquire));
        assert!(
            !serde_json::to_value(accepted)
                .unwrap()
                .as_object()
                .unwrap()
                .contains_key("resumeAudio")
        );
        playback.stop_and_join().unwrap();
    }

    #[test]
    fn review_terminal_event_survives_full_command_mailbox() {
        let (_, playback, selected) = queued();
        let guard = playback.inner.lock().unwrap();
        let mut replies = Vec::new();
        for _ in 0..65 {
            let (tx, rx) = mpsc::channel();
            if playback
                .command_tx
                .try_send(OwnerCommand::Snapshot(tx))
                .is_ok()
            {
                replies.push(rx);
            }
        }
        let sender = playback.clone();
        let generation = selected.generation_id;
        let (done_tx, done_rx) = mpsc::channel();
        let publisher = std::thread::spawn(move || {
            sender.publish_event(
                generation,
                PlaybackEvent::Failed {
                    code: "SOURCE_UNAVAILABLE".into(),
                    retryable: true,
                },
            );
            done_tx.send(()).unwrap();
        });
        let _ = done_rx.recv_timeout(Duration::from_millis(25));
        drop(guard);
        publisher.join().unwrap();
        for reply in replies {
            reply.recv().unwrap().unwrap();
        }
        assert_eq!(
            playback.snapshot().unwrap().playback.status,
            PlaybackStatus::Error
        );
        assert!(playback.events.lock().unwrap().len() <= 3);
        playback.stop_and_join().unwrap();
    }

    #[test]
    fn append_preserves_active_generation_and_control_dedup_is_payload_checked() {
        let (_db, playback, queued) = queued();
        let generation = queued.generation_id.clone();
        let current = queued.current.as_ref().unwrap().occurrence_id.clone();
        playback
            .control_with_guard(transport(&queued, ControlAction::Resume), None)
            .unwrap();
        playback.publish_event(generation.clone(), PlaybackEvent::Active);

        let active = playback.snapshot().unwrap();
        let appended = playback
            .apply(params(
                &active,
                SessionOperation::AppendQueue {
                    sources: vec![TrackSource {
                        server_id: "other-server".into(),
                        track_id: "other-track".into(),
                    }],
                },
            ))
            .unwrap();
        let after_append = playback.snapshot().unwrap();
        assert_eq!(appended.generation_id, generation);
        assert_eq!(after_append.current.unwrap().occurrence_id, current);
        assert_eq!(after_append.state, TransportState::Playing);
        assert_eq!(after_append.playback.status, PlaybackStatus::Active);

        let pause = ControlParams {
            schema_version: 1,
            instance_id: after_append.instance_id.clone(),
            session_id: after_append.session_id.clone(),
            command_id: Uuid::new_v4().to_string(),
            expected_generation_id: generation,
            occurrence_id: current,
            action: ControlAction::Pause,
        };
        let first = playback.control_with_guard(pause.clone(), None).unwrap();
        let replay = playback.control_with_guard(pause.clone(), None).unwrap();
        assert_eq!(first.state_sequence, replay.state_sequence);

        let mut changed = pause;
        changed.action = ControlAction::Resume;
        assert_eq!(
            playback.control_with_guard(changed, None).unwrap_err().code,
            "COMMAND_ID_REUSED"
        );
        playback.stop_and_join().unwrap();
    }

    #[test]
    fn native_toggle_is_resolved_in_owner_order() {
        let (_db, playback, selected) = queued();
        let first = playback
            .native_control(NativeControlIntent::Toggle, None)
            .unwrap();
        assert_eq!(first.playback.status, PlaybackStatus::Loading);
        let second = playback
            .native_control(NativeControlIntent::Toggle, None)
            .unwrap();
        assert_eq!(second.playback.status, PlaybackStatus::Paused);
        assert_eq!(second.current, selected.current);
        playback.stop_and_join().unwrap();
    }

    #[test]
    fn native_controls_use_current_identity_and_preserve_queue() {
        let (_db, playback, selected) = queued();
        let occurrence = selected.current.as_ref().unwrap().occurrence_id.clone();
        let stopped = playback
            .native_control(NativeControlIntent::Stop, None)
            .unwrap();
        assert_eq!(stopped.playback.status, PlaybackStatus::Stopped);
        assert_eq!(stopped.position_ms, 0);
        assert_eq!(stopped.current.unwrap().occurrence_id, occurrence);
        assert_eq!(stopped.total_occurrence_count, 1);
        playback.stop_and_join().unwrap();
    }

    #[test]
    fn native_control_rejects_empty_and_fenced_sessions() {
        let db = Arc::new(Database::memory().unwrap());
        let playback = PlaybackSession::restore(db, "owner".into());
        assert_eq!(
            playback
                .native_control(NativeControlIntent::Play, None)
                .unwrap_err()
                .code,
            "RESUME_UNAVAILABLE"
        );
        let shutdown = playback.begin_shutdown_checkpoint().unwrap();
        shutdown.recv().unwrap().unwrap();
        assert_eq!(
            playback
                .native_control(NativeControlIntent::Pause, None)
                .unwrap_err()
                .code,
            "DAEMON_STOPPED"
        );
        playback.stop_and_join().unwrap();
    }

    fn qualified_seek(playback: &PlaybackSession, snapshot: &SessionSnapshot) -> SessionSnapshot {
        let source = snapshot.current.as_ref().unwrap().source.clone();
        playback.publish_event(
            snapshot.generation_id.clone(),
            PlaybackEvent::Resolved {
                metadata: PlaybackTrackMetadata {
                    source,
                    title: "fixture".into(),
                    artist: None,
                    album: None,
                },
                duration_ms: Some(10_000),
                representation: "wav".into(),
                seek: SeekCapability::jellyfin_pcm_wav(),
            },
        );
        playback.snapshot().unwrap()
    }

    fn seek(snapshot: &SessionSnapshot, position_ms: u64) -> SeekParams {
        SeekParams {
            schema_version: 1,
            instance_id: snapshot.instance_id.clone(),
            session_id: snapshot.session_id.clone(),
            command_id: Uuid::new_v4().to_string(),
            expected_generation_id: snapshot.generation_id.clone(),
            occurrence_id: snapshot.current.as_ref().unwrap().occurrence_id.clone(),
            position_ms,
        }
    }

    #[test]
    fn seek_rejects_unavailable_and_out_of_bounds_without_mutation() {
        let (_, playback, selected) = queued();
        let before = playback.snapshot().unwrap();
        assert_eq!(
            playback
                .seek_with_guard(seek(&before, 1), None)
                .unwrap_err()
                .code,
            "SEEK_UNAVAILABLE"
        );
        let qualified = qualified_seek(&playback, &selected);
        assert_eq!(
            playback
                .seek_with_guard(seek(&qualified, 10_001), None)
                .unwrap_err()
                .code,
            "INVALID_SEEK"
        );
        let after = playback.snapshot().unwrap();
        assert_eq!(after.generation_id, qualified.generation_id);
        assert_eq!(after.position_ms, qualified.position_ms);
        playback.stop_and_join().unwrap();
    }

    #[test]
    fn seek_keeps_requested_target_pending_until_matching_commit() {
        let (_, playback, selected) = queued();
        let qualified = qualified_seek(&playback, &selected);
        let request = seek(&qualified, 4_000);
        let operation = request.command_id.clone();
        let pending = playback.seek_with_guard(request.clone(), None).unwrap();
        assert_eq!(pending.position_ms, qualified.position_ms);
        assert_eq!(pending.queue_revision, qualified.queue_revision);
        assert_eq!(pending.current, qualified.current);
        assert_eq!(
            pending
                .playback
                .pending_seek
                .as_ref()
                .unwrap()
                .requested_position_ms,
            4_000
        );
        let replay = playback.seek_with_guard(request, None).unwrap();
        assert_eq!(replay.generation_id, pending.generation_id);
        assert!(!replay.seek_audio);
        playback.publish_event_at_epoch(
            pending.generation_id.clone(),
            PlaybackEvent::SeekCommitted {
                operation_id: operation,
                requested_position_ms: 4_000,
                actual_position_ms: 4_012,
            },
            pending.seek_epoch,
        );
        let committed = playback.snapshot().unwrap();
        assert_eq!(committed.position_ms, 4_012);
        assert!(committed.playback.pending_seek.is_none());
        assert_eq!(
            committed.playback.seek_outcome.unwrap().actual_position_ms,
            Some(4_012)
        );
        playback.stop_and_join().unwrap();
    }

    #[test]
    fn exact_end_seek_commits_terminal_cursor_and_checkpoints() {
        let (_, playback, selected) = queued();
        let qualified = qualified_seek(&playback, &selected);
        let completed = playback
            .seek_with_guard(seek(&qualified, 10_000), None)
            .unwrap();
        assert_eq!(completed.position_ms, 10_000);
        assert_eq!(completed.checkpointed_position_ms, 10_000);
        assert_eq!(completed.playback.status, PlaybackStatus::Completed);
        assert!(!completed.seek_audio);
        assert_eq!(completed.queue_revision, qualified.queue_revision);
        playback.stop_and_join().unwrap();
    }

    #[test]
    fn restored_exact_end_resume_normalizes_to_zero_after_duration_resolves() {
        let (db, playback, selected) = queued();
        let qualified = qualified_seek(&playback, &selected);
        playback
            .seek_with_guard(seek(&qualified, 10_000), None)
            .unwrap();
        playback.stop_and_join().unwrap();

        let restored = PlaybackSession::restore(db, "seek-end-restore".into());
        let paused = restored.snapshot().unwrap();
        assert_eq!(paused.position_ms, 10_000);
        let loading = restored
            .control_with_guard(
                ControlParams {
                    schema_version: 1,
                    instance_id: paused.instance_id,
                    session_id: paused.session_id,
                    command_id: Uuid::new_v4().to_string(),
                    expected_generation_id: paused.generation_id,
                    occurrence_id: paused.current.as_ref().unwrap().occurrence_id.clone(),
                    action: ControlAction::Resume,
                },
                None,
            )
            .unwrap();
        assert_eq!(loading.position_ms, 10_000);
        restored.publish_event(
            loading.generation_id,
            PlaybackEvent::Resolved {
                metadata: PlaybackTrackMetadata {
                    source: loading.current.unwrap().source,
                    title: "fixture".into(),
                    artist: None,
                    album: None,
                },
                duration_ms: Some(10_000),
                representation: "wav".into(),
                seek: SeekCapability::jellyfin_pcm_wav(),
            },
        );
        assert_eq!(restored.snapshot().unwrap().position_ms, 0);
        restored.stop_and_join().unwrap();
    }

    #[test]
    fn successful_seek_is_durable_but_pending_target_is_not() {
        let (db, playback, selected) = queued();
        let qualified = qualified_seek(&playback, &selected);
        let first = seek(&qualified, 4_000);
        let first_operation = first.command_id.clone();
        let pending = playback.seek_with_guard(first, None).unwrap();
        assert_eq!(db.load_playback_session().unwrap().unwrap().position_ms, 0);
        playback.publish_event_at_epoch(
            pending.generation_id,
            PlaybackEvent::SeekCommitted {
                operation_id: first_operation,
                requested_position_ms: 4_000,
                actual_position_ms: 4_012,
            },
            pending.seek_epoch,
        );
        assert_eq!(playback.snapshot().unwrap().checkpointed_position_ms, 4_012);
        playback.stop_and_join().unwrap();
        let restored = PlaybackSession::restore(db, "seek-restore".into());
        assert_eq!(restored.snapshot().unwrap().position_ms, 4_012);
        restored.stop_and_join().unwrap();
    }

    #[test]
    fn failed_seek_checkpoint_is_observable_and_retryable() {
        let (db, playback, selected) = queued();
        let qualified = qualified_seek(&playback, &selected);
        let request = seek(&qualified, 4_000);
        let operation_id = request.command_id.clone();
        let pending = playback.seek_with_guard(request, None).unwrap();
        db.conn.lock().unwrap().execute_batch("CREATE TRIGGER fail_seek_position BEFORE UPDATE OF position_ms ON playback_sessions BEGIN SELECT RAISE(ABORT,'test'); END").unwrap();
        playback.publish_event_at_epoch(
            pending.generation_id,
            PlaybackEvent::SeekCommitted {
                operation_id,
                requested_position_ms: 4_000,
                actual_position_ms: 4_012,
            },
            pending.seek_epoch,
        );
        let failed = playback.snapshot().unwrap();
        assert_eq!(failed.position_ms, 4_012);
        assert_eq!(failed.checkpointed_position_ms, 0);
        assert_eq!(failed.persistence.status, "error");
        db.conn
            .lock()
            .unwrap()
            .execute_batch("DROP TRIGGER fail_seek_position")
            .unwrap();
        playback.final_checkpoint().unwrap();
        let retried = playback.snapshot().unwrap();
        assert_eq!(retried.checkpointed_position_ms, 4_012);
        assert_eq!(retried.persistence.status, "ok");
        playback.stop_and_join().unwrap();
    }

    #[test]
    fn pause_during_seek_preserves_paused_intent_and_stale_commit_is_fenced() {
        let (_, playback, selected) = queued();
        let qualified = qualified_seek(&playback, &selected);
        let first = seek(&qualified, 2_000);
        let first_operation = first.command_id.clone();
        let first_pending = playback.seek_with_guard(first, None).unwrap();
        playback
            .control_with_guard(
                ControlParams {
                    schema_version: 1,
                    instance_id: first_pending.instance_id.clone(),
                    session_id: first_pending.session_id.clone(),
                    command_id: Uuid::new_v4().to_string(),
                    expected_generation_id: first_pending.generation_id.clone(),
                    occurrence_id: first_pending
                        .current
                        .as_ref()
                        .unwrap()
                        .occurrence_id
                        .clone(),
                    action: ControlAction::Pause,
                },
                None,
            )
            .unwrap();
        let paused = playback.snapshot().unwrap();
        assert!(paused.playback.pending_seek.is_some());
        let second = seek(&paused, 6_000);
        let second_operation = second.command_id.clone();
        let second_pending = playback.seek_with_guard(second, None).unwrap();
        playback.publish_event_at_epoch(
            first_pending.generation_id,
            PlaybackEvent::SeekCommitted {
                operation_id: first_operation,
                requested_position_ms: 2_000,
                actual_position_ms: 2_000,
            },
            first_pending.seek_epoch,
        );
        assert_eq!(playback.snapshot().unwrap().position_ms, 0);
        playback.publish_event_at_epoch(
            second_pending.generation_id,
            PlaybackEvent::SeekCommitted {
                operation_id: second_operation,
                requested_position_ms: 6_000,
                actual_position_ms: 6_000,
            },
            second_pending.seek_epoch,
        );
        let committed = playback.snapshot().unwrap();
        assert_eq!(committed.position_ms, 6_000);
        assert_eq!(committed.state, TransportState::Paused);
        assert_eq!(committed.playback.status, PlaybackStatus::Paused);
        playback.stop_and_join().unwrap();
    }

    #[test]
    fn native_relative_seek_clamps_protocol_boundaries_before_common_admission() {
        let (_, playback, selected) = queued();
        let _ = qualified_seek(&playback, &selected);
        let zero = playback
            .native_control(NativeControlIntent::SeekRelative(-30_000), None)
            .unwrap();
        assert_eq!(zero.playback.pending_seek.unwrap().requested_position_ms, 0);
        playback.publish_event_at_epoch(
            zero.generation_id.clone(),
            PlaybackEvent::SeekCommitted {
                operation_id: zero
                    .playback
                    .seek_outcome
                    .as_ref()
                    .unwrap()
                    .operation_id
                    .clone(),
                requested_position_ms: 0,
                actual_position_ms: 0,
            },
            zero.seek_epoch,
        );
        let committed = playback.snapshot().unwrap();
        let end = playback
            .native_control(NativeControlIntent::SeekRelative(30_000), None)
            .unwrap();
        assert_eq!(end.position_ms, 10_000);
        assert_eq!(end.playback.status, PlaybackStatus::Completed);
        assert_eq!(end.current, committed.current);
        playback.stop_and_join().unwrap();
    }
    #[test]
    fn review_backward_seek_accepts_progress_and_checkpoints_before_old_cursor() {
        let (db, playback, selected) = queued();
        let _cleanup = OwnerThreadCleanup(playback.clone());
        let qualified = qualified_seek(&playback, &selected);
        let occurrence = qualified.current.as_ref().unwrap().occurrence_id.clone();
        playback
            .report_progress(&qualified.generation_id, &occurrence, 1, 8_000)
            .unwrap();
        playback.final_checkpoint().unwrap();
        let before = playback.snapshot().unwrap();
        let pending = playback
            .seek_with_guard(seek(&before, 2_000), None)
            .unwrap();
        playback.publish_event_at_epoch(
            pending.generation_id.clone(),
            PlaybackEvent::SeekCommitted {
                operation_id: pending.playback.pending_seek.unwrap().operation_id,
                requested_position_ms: 2_000,
                actual_position_ms: 2_012,
            },
            pending.seek_epoch,
        );
        assert_eq!(playback.snapshot().unwrap().position_ms, 2_012);
        playback
            .report_progress(&pending.generation_id, &occurrence, 1, 2_100)
            .unwrap();
        playback.final_checkpoint().unwrap();
        assert_eq!(playback.snapshot().unwrap().position_ms, 2_100);
        assert_eq!(
            db.load_playback_session().unwrap().unwrap().position_ms,
            2_100
        );
    }

    #[test]
    fn review_seek_pipeline_failure_after_commit_is_terminal() {
        for code in ["DECODE_FAILED", "SOURCE_UNAVAILABLE", "OUTPUT_LOST"] {
            let (_, playback, selected) = queued();
            let _cleanup = OwnerThreadCleanup(playback.clone());
            let qualified = qualified_seek(&playback, &selected);
            let pending = playback
                .seek_with_guard(seek(&qualified, 2_000), None)
                .unwrap();
            let operation = pending.playback.pending_seek.unwrap().operation_id;
            playback.publish_event_at_epoch(
                pending.generation_id.clone(),
                PlaybackEvent::SeekCommitted {
                    operation_id: operation.clone(),
                    requested_position_ms: 2_000,
                    actual_position_ms: 2_012,
                },
                pending.seek_epoch,
            );
            // Do not drain the commit first: the error must not coalesce it away.
            playback.publish_event_at_epoch(
                pending.generation_id.clone(),
                PlaybackEvent::SeekPipelineFailed {
                    operation_id: operation,
                    code: code.into(),
                    retryable: true,
                },
                pending.seek_epoch,
            );
            let failed = playback.snapshot().unwrap();
            assert_eq!(failed.playback.status, PlaybackStatus::Error);
            assert_eq!(failed.playback.error.unwrap().code, code);
            assert_eq!(failed.position_ms, 2_012);
            assert_eq!(failed.playback.seek_outcome.unwrap().status, "succeeded");
            assert!(!playback.output_gate().load(Ordering::Acquire));
        }
    }

    #[test]
    fn review_seek_pipeline_failure_before_commit_keeps_old_cursor() {
        let (_, playback, selected) = queued();
        let _cleanup = OwnerThreadCleanup(playback.clone());
        let qualified = qualified_seek(&playback, &selected);
        let pending = playback
            .seek_with_guard(seek(&qualified, 2_000), None)
            .unwrap();
        playback.publish_event_at_epoch(
            pending.generation_id,
            PlaybackEvent::SeekPipelineFailed {
                operation_id: pending.playback.pending_seek.unwrap().operation_id,
                code: "DECODE_FAILED".into(),
                retryable: true,
            },
            pending.seek_epoch,
        );
        let failed = playback.snapshot().unwrap();
        assert_eq!(failed.position_ms, qualified.position_ms);
        assert_eq!(failed.playback.seek_outcome.unwrap().status, "failed");
        assert_eq!(failed.playback.error.unwrap().code, "SEEK_FAILED");
    }

    #[test]
    fn review_qualification_and_resolution_have_independent_slots() {
        let (_, playback, selected) = queued();
        let _cleanup = OwnerThreadCleanup(playback.clone());
        let resolved = PlaybackEvent::Resolved {
            metadata: PlaybackTrackMetadata {
                source: selected.current.as_ref().unwrap().source.clone(),
                title: "fixture".into(),
                artist: None,
                album: None,
            },
            duration_ms: Some(10_000),
            representation: "wav".into(),
            seek: SeekCapability::unavailable("seek.validating"),
        };
        let qualified = PlaybackEvent::SeekQualified {
            capability: SeekCapability::jellyfin_pcm_wav(),
            duration_ms: Some(10_500),
        };
        // Exercise coalescing while holding the event queue so the owner cannot drain it.
        let serial = playback
            .generation_guard(&selected.generation_id)
            .unwrap()
            .1;
        {
            let mut events = playback.events.lock().unwrap();
            enqueue_event(
                &mut events,
                serial,
                playback.control_epoch(),
                selected.generation_id.clone(),
                resolved,
            );
            enqueue_event(
                &mut events,
                serial,
                playback.control_epoch(),
                selected.generation_id.clone(),
                qualified,
            );
            assert_eq!(events.len(), 2);
        }
        let snapshot = playback.snapshot().unwrap();
        assert_eq!(
            snapshot.playback.metadata.as_ref().unwrap().title,
            "fixture"
        );
        assert_eq!(snapshot.playback.duration_ms, Some(10_500));
        assert!(snapshot.playback.seek.available);
        let completed = playback
            .seek_with_guard(seek(&snapshot, 10_500), None)
            .unwrap();
        assert_eq!(completed.position_ms, 10_500);
    }

    #[test]
    fn terminal_failure_dominates_completion_in_both_delivery_orders() {
        for failed_first in [true, false] {
            let mut events = VecDeque::new();
            let failed = PlaybackEvent::Failed {
                code: "SOURCE_UNAVAILABLE".into(),
                retryable: true,
            };
            let completed = PlaybackEvent::Completed { position_ms: 123 };
            let ordered = if failed_first {
                [failed, completed]
            } else {
                [completed, failed]
            };
            for event in ordered {
                enqueue_event(&mut events, 7, 11, "generation".into(), event);
            }
            assert_eq!(events.len(), 1);
            assert!(matches!(
                events.front().unwrap().3,
                PlaybackEvent::Failed { .. }
            ));
        }
    }
    #[test]
    fn review_restored_verified_end_restarts_progress_after_media_resolution() {
        let (db, playback, selected) = queued();
        let qualified = qualified_seek(&playback, &selected);
        playback.publish_event(
            qualified.generation_id,
            PlaybackEvent::SeekQualified {
                capability: SeekCapability::jellyfin_pcm_wav(),
                duration_ms: Some(10_500),
            },
        );
        let qualified = playback.snapshot().unwrap();
        playback
            .seek_with_guard(seek(&qualified, 10_500), None)
            .unwrap();
        playback.stop_and_join().unwrap();
        let restored = PlaybackSession::restore(db, "verified-end-restore".into());
        let _cleanup = OwnerThreadCleanup(restored.clone());
        let loading = restored
            .native_control(NativeControlIntent::Play, None)
            .unwrap();
        let occurrence = loading.current.as_ref().unwrap().occurrence_id.clone();
        restored.publish_event(
            loading.generation_id.clone(),
            PlaybackEvent::Resolved {
                metadata: PlaybackTrackMetadata {
                    source: loading.current.unwrap().source,
                    title: "fixture".into(),
                    artist: None,
                    album: None,
                },
                duration_ms: Some(10_000),
                representation: "wav".into(),
                seek: SeekCapability::unavailable("seek.validating"),
            },
        );
        assert_eq!(restored.snapshot().unwrap().position_ms, 10_500);
        restored.publish_event(
            loading.generation_id.clone(),
            PlaybackEvent::SeekQualified {
                capability: SeekCapability::unavailable("seek.platform_unqualified"),
                duration_ms: Some(10_500),
            },
        );
        assert_eq!(restored.snapshot().unwrap().position_ms, 0);
        restored
            .report_progress(&loading.generation_id, &occurrence, 1, 100)
            .unwrap();
        restored.final_checkpoint().unwrap();
        assert_eq!(restored.snapshot().unwrap().position_ms, 100);
    }
}
