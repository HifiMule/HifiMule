use super::*;

#[cfg(test)]
#[path = "album_admission_tests.rs"]
mod tests;

pub(crate) const ALBUM_RESOLUTION_TIMEOUT: Duration = Duration::from_secs(60);
const RECEIPT_TTL: Duration = Duration::from_secs(600);

#[derive(Default)]
pub(super) struct AlbumAdmissionState {
    pending: Option<PendingAlbum>,
    receipts: HashMap<String, (Instant, PlayAlbumParams)>,
    order: VecDeque<String>,
}

struct PendingAlbum {
    params: PlayAlbumParams,
    token: String,
    epoch: u64,
    deadline: Instant,
    cancelled: Arc<AtomicBool>,
    _guard: Option<crate::sync::MutationGuard>,
}

pub(crate) enum AlbumAdmission {
    Replay(Box<SessionSnapshot>),
    Resolve(AlbumReservation),
}

/// Dropping an abandoned RPC cancels its reservation, including when the owner
/// reply is dropped before the caller receives it. The owner releases the guard.
pub(crate) struct AlbumReservation {
    token: String,
    pub(crate) deadline: Instant,
    cancelled: Arc<AtomicBool>,
}

impl AlbumReservation {
    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

impl Drop for AlbumReservation {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

impl PlaybackSession {
    pub(crate) fn reserve_album(
        &self,
        params: PlayAlbumParams,
        guard: Option<crate::sync::MutationGuard>,
    ) -> PResult<AlbumAdmission> {
        if self.is_fenced() {
            return Err(owner_stopped());
        }
        let (tx, rx) = mpsc::channel();
        self.command_tx
            .try_send(OwnerCommand::ReserveAlbum(params, guard, tx))
            .map_err(admission_error)?;
        rx.recv().unwrap_or_else(|_| Err(owner_stopped()))
    }

    pub(crate) fn commit_album(
        &self,
        reservation: AlbumReservation,
        sources: Vec<TrackSource>,
    ) -> PResult<SessionSnapshot> {
        let (tx, rx) = mpsc::channel();
        self.command_tx
            .try_send(OwnerCommand::CommitAlbum(reservation, sources, tx))
            .map_err(admission_error)?;
        rx.recv().unwrap_or_else(|_| Err(owner_stopped()))
    }
}

pub(super) fn contains_command(i: &Inner, id: &str) -> bool {
    i.album.receipts.contains_key(id)
        || i.album
            .pending
            .as_ref()
            .is_some_and(|p| p.params.command_id == id)
}

pub(super) fn cancel_pending(i: &mut Inner) {
    if let Some(pending) = i.album.pending.take() {
        pending.cancelled.store(true, Ordering::Release);
    }
}

pub(super) fn prune(i: &mut Inner, fenced: bool) {
    if i.album.pending.as_ref().is_some_and(|p| {
        fenced
            || p.cancelled.load(Ordering::Acquire)
            || Instant::now() >= p.deadline
            || p.epoch != i.control_epoch.load(Ordering::Acquire)
            || p.params.instance_id != i.instance_id
            || p.params.session_id != i.session.session_id
            || p.params.expected_generation_id != i.generation_id
            || p.params.expected_queue_revision != i.session.queue_revision.to_string()
    }) {
        cancel_pending(i);
    }
    while let Some(id) = i.album.order.front() {
        if i.album
            .receipts
            .get(id)
            .is_some_and(|(at, _)| at.elapsed() <= RECEIPT_TTL)
            && i.album.order.len() <= 1024
        {
            break;
        }
        let id = i.album.order.pop_front().unwrap();
        i.album.receipts.remove(&id);
    }
}

pub(super) fn reserve(
    i: &mut Inner,
    params: PlayAlbumParams,
    guard: Option<crate::sync::MutationGuard>,
) -> PResult<AlbumAdmission> {
    prune(i, false);
    prune_dedup(i);
    require_schema(params.schema_version)?;
    params
        .source
        .validate()
        .map_err(|m| PlaybackError::invalid("ALBUM_INVALID", m))?;
    if [
        &params.command_id,
        &params.instance_id,
        &params.session_id,
        &params.expected_generation_id,
    ]
    .iter()
    .any(|id| Uuid::parse_str(id).is_err())
    {
        return Err(PlaybackError::invalid(
            "ALBUM_INVALID",
            "album identities must be UUIDs",
        ));
    }
    parse_revision(&params.expected_queue_revision)?;
    if i.dedup.contains_key(&params.command_id)
        || i.control_dedup.contains_key(&params.command_id)
        || i.seek_dedup.contains_key(&params.command_id)
        || i.output_dedup.contains_key(&params.command_id)
    {
        return Err(PlaybackError::conflict(
            "COMMAND_ID_REUSED",
            "command identity was reused",
        ));
    }
    if let Some((_, prior)) = i.album.receipts.get(&params.command_id) {
        return if prior == &params {
            snapshot(i).map(|snapshot| AlbumAdmission::Replay(Box::new(snapshot)))
        } else {
            Err(PlaybackError::conflict(
                "COMMAND_ID_REUSED",
                "album command payload changed",
            ))
        };
    }
    if let Some(pending) = &i.album.pending {
        if pending.params.command_id == params.command_id && pending.params != params {
            return Err(PlaybackError::conflict(
                "COMMAND_ID_REUSED",
                "album command payload changed",
            ));
        }
        return Err(PlaybackError::conflict(
            "PLAYBACK_BUSY",
            "an album resolution is pending",
        ));
    }
    if i.restoration.status == "error" {
        return Err(PlaybackError::conflict(
            "RESTORE_FAILED",
            "restore playback before resolving an album",
        ));
    }
    if params.instance_id != i.instance_id
        || params.session_id != i.session.session_id
        || params.expected_generation_id != i.generation_id
        || params.expected_queue_revision != i.session.queue_revision.to_string()
    {
        return Err(PlaybackError::conflict(
            "GENERATION_CONFLICT",
            "album admission is stale",
        ));
    }
    let token = Uuid::new_v4().to_string();
    let cancelled = Arc::new(AtomicBool::new(false));
    let deadline = Instant::now() + ALBUM_RESOLUTION_TIMEOUT;
    i.album.pending = Some(PendingAlbum {
        params,
        token: token.clone(),
        epoch: i.control_epoch.load(Ordering::Acquire),
        deadline,
        cancelled: cancelled.clone(),
        _guard: guard,
    });
    Ok(AlbumAdmission::Resolve(AlbumReservation {
        token,
        deadline,
        cancelled,
    }))
}

pub(super) fn commit(
    i: &mut Inner,
    reservation: AlbumReservation,
    sources: Vec<TrackSource>,
    serial: &AtomicU64,
) -> PResult<SessionSnapshot> {
    prune(i, false);
    if !i
        .album
        .pending
        .as_ref()
        .is_some_and(|p| p.token == reservation.token)
    {
        return Err(PlaybackError::conflict(
            "GENERATION_CONFLICT",
            "album resolution was superseded",
        ));
    }
    let pending = i.album.pending.take().unwrap();
    let p = &pending.params;
    let result = apply_inner(
        i,
        &ApplySessionParams {
            schema_version: p.schema_version,
            instance_id: p.instance_id.clone(),
            session_id: p.session_id.clone(),
            command_id: p.command_id.clone(),
            expected_queue_revision: p.expected_queue_revision.clone(),
            operation: SessionOperation::PlayAlbum { sources },
        },
        serial,
    )?;
    // The assigned rows were validated/decorated before the transaction. Build
    // the response without additional fallible DB reads after successful commit.
    let count = result.assigned_occurrences.len();
    let current = result.assigned_occurrences.first().cloned();
    let rows: Vec<_> = result
        .assigned_occurrences
        .into_iter()
        .take(DEFAULT_PAGE_SIZE)
        .collect();
    let next_cursor = if count > DEFAULT_PAGE_SIZE {
        rows.last()
            .map(|row| encode_cursor(&i.session, row.ordinal))
    } else {
        None
    };
    let mut playback = i.playback.clone();
    playback.can_go_next = count > 1;
    let response = SessionSnapshot {
        schema_version: SCHEMA_VERSION,
        instance_id: i.instance_id.clone(),
        session_id: i.session.session_id.clone(),
        queue_revision: i.session.queue_revision.to_string(),
        state_sequence: i.state_sequence.to_string(),
        generation_id: i.generation_id.clone(),
        state: i.session.state,
        current,
        position_ms: i.session.position_ms,
        checkpointed_position_ms: i.checkpointed_position_ms,
        persistence: i.persistence.clone(),
        restoration: i.restoration.clone(),
        total_occurrence_count: count as u64,
        occurrences: rows,
        next_cursor,
        playback,
        output: i.output.clone(),
        resume_audio: result.start_audio,
        resume_epoch: i.control_epoch.load(Ordering::Acquire),
        seek_audio: false,
        seek_epoch: i.control_epoch.load(Ordering::Acquire),
    };
    i.album.order.push_back(p.command_id.clone());
    i.album
        .receipts
        .insert(p.command_id.clone(), (Instant::now(), p.clone()));
    prune(i, false);
    Ok(response)
}
