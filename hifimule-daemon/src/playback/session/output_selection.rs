use super::*;
use crate::playback::{
    config::{self, OutputPreference, PlaybackConfig},
    devices::{
        self, Discovery, OutputDescriptor,
        worker::{DiscoveryWorker, PreferenceWorker, SaveRequest},
    },
};
use std::path::PathBuf;

pub(super) struct OutputRuntime {
    discovery: DiscoveryWorker,
    preferences: PreferenceWorker,
    invalid_config: bool,
    initialize_missing_default: bool,
    discovery_sequence: u64,
    switch_generation: Option<String>,
    pub(super) effect: Option<String>,
    active_generation: Option<String>,
    opening_generation: Option<String>,
}

fn saved_descriptor(preference: OutputPreference) -> OutputDescriptor {
    OutputDescriptor {
        output_id: devices::output_id(&preference),
        display_name: preference.display_name.clone(),
        detail: String::new(),
        backend: preference.backend.clone(),
        available: false,
        is_default: false,
        identity_confidence: "unverified".into(),
        is_virtual: false,
        preference: Some(preference),
    }
}

fn failure(code: &str) -> PlaybackFailure {
    PlaybackFailure {
        code: code.into(),
        retryable: true,
    }
}

impl PlaybackSession {
    pub fn enable_outputs(&self, path: PathBuf) {
        self.enable_outputs_with(path, devices::discover);
    }

    pub fn enable_outputs_with(
        &self,
        path: PathBuf,
        discover: impl Fn() -> Discovery + Send + 'static,
    ) {
        let loaded = config::load_optional(&path);
        let initialize_missing_default = matches!(&loaded, Ok(None));
        let mut i = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        assert!(i.outputs.is_none(), "output services initialized twice");
        i.output.selected = loaded
            .as_ref()
            .ok()
            .and_then(|config| config.as_ref())
            .and_then(|c| c.output.clone())
            .map(saved_descriptor);
        i.output.status = if loaded.is_err() {
            "error"
        } else if i.output.selected.is_some() {
            "unavailable"
        } else {
            "unselected"
        }
        .into();
        i.output.error = loaded
            .as_ref()
            .err()
            .map(|_| failure("OUTPUT_CONFIG_INVALID"));
        i.outputs = Some(OutputRuntime {
            discovery: DiscoveryWorker::start(discover),
            preferences: PreferenceWorker::start(path),
            invalid_config: loaded.is_err(),
            initialize_missing_default,
            discovery_sequence: 0,
            switch_generation: None,
            effect: None,
            active_generation: None,
            opening_generation: None,
        });
    }

    pub fn list_outputs(&self, params: ListOutputsParams) -> PResult<OutputList> {
        let (tx, rx) = mpsc::channel();
        self.command_tx
            .try_send(OwnerCommand::ListOutputs(params, tx))
            .map_err(admission_error)?;
        rx.recv().unwrap_or_else(|_| Err(owner_stopped()))
    }

    pub fn select_output(
        &self,
        params: SelectOutputParams,
        guard: Option<crate::sync::MutationGuard>,
    ) -> PResult<SessionSnapshot> {
        if self.fenced.load(Ordering::Acquire) {
            return Err(owner_stopped());
        }
        let (tx, rx) = mpsc::channel();
        self.command_tx
            .try_send(OwnerCommand::SelectOutput(params, guard, tx))
            .map_err(admission_error)?;
        rx.recv().unwrap_or_else(|_| Err(owner_stopped()))
    }

    pub fn selected_output(&self, generation: &str) -> Result<OutputPreference, &'static str> {
        let i = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if self.fenced.load(Ordering::Acquire) || i.generation_id != generation {
            return Err("GENERATION_CONFLICT");
        }
        output_policy(&i)?;
        i.output
            .selected
            .as_ref()
            .and_then(|o| o.preference.clone())
            .ok_or("OUTPUT_UNAVAILABLE")
    }

    pub fn take_output_effect(&self) -> Option<SessionSnapshot> {
        let mut i = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if self.fenced.load(Ordering::Acquire) {
            return None;
        }
        let generation = i.outputs.as_mut()?.effect.take()?;
        (generation == i.generation_id)
            .then(|| snapshot(&i).ok())
            .flatten()
    }

    pub fn output_opened(&self, generation: &str, preference: &OutputPreference) -> bool {
        let mut i = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if self.fenced.load(Ordering::Acquire)
            || i.generation_id != generation
            || i.output
                .selected
                .as_ref()
                .and_then(|o| o.preference.as_ref())
                .is_none_or(|selected| {
                    selected.backend != preference.backend
                        || selected.stable_id != preference.stable_id
                        || selected.identity_properties != preference.identity_properties
                })
        {
            return false;
        }
        i.output.active = i.output.selected.clone();
        if let Some(runtime) = i.outputs.as_mut() {
            runtime.active_generation = Some(generation.into());
            runtime.opening_generation = None;
        }
        i.output.status = "available".into();
        i.output.error = None;
        i.output_gate.store(
            matches!(
                active_state(&i),
                TransportState::Playing | TransportState::Buffering
            ),
            Ordering::Release,
        );
        i.state_sequence += 1;
        true
    }

    pub fn output_closed(&self, generation: &str) {
        let mut i = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if i.outputs
            .as_ref()
            .is_some_and(|r| r.active_generation.as_deref() == Some(generation))
        {
            i.output.active = None;
            if let Some(runtime) = i.outputs.as_mut() {
                runtime.active_generation = None;
            }
            i.state_sequence += 1;
        }
    }

    pub fn stop_output_workers(&self) {
        let workers = self
            .inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .outputs
            .take();
        // Native calls and file IO finish outside the session mutex.
        drop(workers);
    }
}

pub(super) fn output_policy(i: &Inner) -> Result<(), &'static str> {
    if i.outputs.as_ref().is_some_and(|r| r.invalid_config) {
        return Err("OUTPUT_CONFIG_INVALID");
    }
    if i.output.pending.is_some() {
        return Err("OUTPUT_SWITCH_PENDING");
    }
    if !i.output.selected.as_ref().is_some_and(|o| o.available) {
        return Err("OUTPUT_UNAVAILABLE");
    }
    Ok(())
}

pub(super) fn finish_output_preparation(i: &mut Inner) {
    if let Some(runtime) = i.outputs.as_mut() {
        runtime.opening_generation = None;
        runtime.effect = None;
    }
}

pub(super) fn list_outputs(i: &Inner, params: ListOutputsParams) -> PResult<OutputList> {
    require_schema(params.schema_version)?;
    let inventory = i
        .outputs
        .as_ref()
        .ok_or_else(|| {
            PlaybackError::invalid("OUTPUT_UNAVAILABLE", "output discovery unavailable")
        })?
        .discovery
        .inventory(true);
    Ok(OutputList {
        instance_id: i.instance_id.clone(),
        output_revision: i.output.revision.clone(),
        outputs: inventory.discovery.outputs,
        output: i.output.clone(),
        error: inventory.discovery.error.map(failure),
    })
}

pub(super) fn select_output(
    i: &mut Inner,
    p: SelectOutputParams,
    ingress: &Mutex<ProgressIngress>,
    serial: &AtomicU64,
) -> PResult<SessionSnapshot> {
    require_schema(p.schema_version)?;
    if Uuid::parse_str(&p.command_id).is_err()
        || Uuid::parse_str(&p.instance_id).is_err()
        || Uuid::parse_str(&p.session_id).is_err()
        || Uuid::parse_str(&p.expected_generation_id).is_err()
        || p.output_id.len() != 64
        || !p
            .output_id
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err(PlaybackError::invalid(
            "INVALID_OUTPUT",
            "invalid output selection identity",
        ));
    }
    album_admission::prune(i, false);
    prune_dedup(i);
    if album_admission::contains_command(i, &p.command_id)
        || i.dedup.contains_key(&p.command_id)
        || i.control_dedup.contains_key(&p.command_id)
        || i.seek_dedup.contains_key(&p.command_id)
        || i.preview_dedup.contains_key(&p.command_id)
    {
        return Err(PlaybackError::conflict(
            "COMMAND_ID_REUSED",
            "command identity was reused",
        ));
    }
    if let Some((old, result)) = i.output_dedup.get(&p.command_id) {
        return if old == &p {
            result.clone().and_then(|_| snapshot(i))
        } else {
            Err(PlaybackError::conflict(
                "COMMAND_ID_REUSED",
                "command payload changed",
            ))
        };
    }
    let result = select_inner(i, &p, ingress, serial);
    i.output_dedup
        .insert(p.command_id.clone(), (p.clone(), result.clone()));
    i.output_dedup_order.push_back(p.command_id);
    while i.output_dedup_order.len() > 1024 {
        if let Some(id) = i.output_dedup_order.pop_front() {
            i.output_dedup.remove(&id);
        }
    }
    result
}

fn select_inner(
    i: &mut Inner,
    p: &SelectOutputParams,
    ingress: &Mutex<ProgressIngress>,
    serial: &AtomicU64,
) -> PResult<SessionSnapshot> {
    if p.instance_id != i.instance_id || p.session_id != i.session.session_id {
        return Err(PlaybackError::conflict(
            "SESSION_MISMATCH",
            "session identity is stale",
        ));
    }
    if p.expected_generation_id != i.generation_id {
        return Err(PlaybackError::conflict(
            "GENERATION_CONFLICT",
            "playback generation is stale",
        ));
    }
    if parse_revision(&p.expected_output_revision)? != parse_revision(&i.output.revision)? {
        return Err(PlaybackError::conflict(
            "OUTPUT_REVISION_CONFLICT",
            "output revision is stale",
        ));
    }
    let runtime = i.outputs.as_ref().ok_or_else(|| {
        PlaybackError::invalid("OUTPUT_UNAVAILABLE", "output discovery unavailable")
    })?;
    if runtime.invalid_config && !p.replace_invalid_config {
        return Err(PlaybackError::invalid(
            "OUTPUT_CONFIG_INVALID",
            "reset output setting explicitly before choosing an output",
        ));
    }
    let inventory = runtime.discovery.inventory(false);
    if let Some(code) = inventory.discovery.error
        && !inventory.discovery.can_resolve()
    {
        return Err(PlaybackError::invalid(code, "output discovery unavailable"));
    }
    let selected = inventory
        .discovery
        .outputs
        .into_iter()
        .find(|o| o.output_id == p.output_id && o.available)
        .ok_or_else(|| {
            PlaybackError::invalid("OUTPUT_UNAVAILABLE", "choose an available output")
        })?;
    if i.output
        .selected
        .as_ref()
        .is_some_and(|o| o.output_id == selected.output_id && o.available)
        && i.output.pending.is_none()
        && i.output.error.is_none()
    {
        return snapshot(i);
    }
    let revision = parse_revision(&i.output.revision)?
        .checked_add(1)
        .filter(|r| *r <= i64::MAX as u64)
        .ok_or_else(|| PlaybackError::invalid("INVALID_OUTPUT", "output revision overflow"))?;
    i.output_gate.store(false, Ordering::Release);
    super::super::audio::global().control(ControlAction::Pause);
    sample_progress(i, ingress)?;
    if let Some(position) = super::super::audio::global().captured_position(&i.generation_id) {
        if let Some(preview) = i.preview.as_mut() {
            preview.position_ms = preview.position_ms.max(position);
        } else {
            i.session.position_ms = i.session.position_ms.max(position);
        }
        i.dirty = true;
    }
    supersede_pending_seek(i);
    serial.fetch_add(1, Ordering::AcqRel);
    i.control_epoch.fetch_add(1, Ordering::AcqRel);
    i.generation_id = Uuid::new_v4().to_string();
    i.output.revision = revision.to_string();
    i.output.pending = Some(selected.clone());
    i.output.status = "switching".into();
    i.output.error = None;
    i.state_sequence += 1;
    let runtime = i.outputs.as_mut().unwrap();
    runtime.switch_generation = Some(i.generation_id.clone());
    runtime.effect = None;
    runtime.preferences.submit(SaveRequest {
        revision,
        config: PlaybackConfig {
            schema_version: 1,
            output: selected.preference,
        },
        replace_invalid: p.replace_invalid_config,
    });
    snapshot(i)
}

pub(super) fn reconcile_outputs(
    i: &mut Inner,
    ingress: &Mutex<ProgressIngress>,
    serial: &AtomicU64,
    fenced: bool,
) {
    let Some(runtime) = i.outputs.as_mut() else {
        return;
    };
    let before = i.output.clone();
    let mut checkpoint_output_state = false;
    let inventory = runtime.discovery.inventory(false);
    if !fenced
        && runtime.initialize_missing_default
        && i.output.selected.is_none()
        && i.output.pending.is_none()
        && inventory.discovery.error.is_none()
    {
        let mut defaults = inventory
            .discovery
            .outputs
            .iter()
            .filter(|output| output.available && output.is_default);
        let default = match (defaults.next(), defaults.next()) {
            (Some(selected), None) => Some(selected),
            _ => None,
        };
        runtime.initialize_missing_default = false;
        if let Some(selected) = default {
            let revision = parse_revision(&i.output.revision)
                .expect("output revision is always valid")
                .checked_add(1)
                .filter(|revision| *revision <= i64::MAX as u64)
                .expect("initial output revision cannot overflow");
            i.output.revision = revision.to_string();
            i.output.pending = Some(selected.clone());
            i.output.status = "switching".into();
            runtime.preferences.submit(SaveRequest {
                revision,
                config: PlaybackConfig {
                    schema_version: 1,
                    output: selected.preference.clone(),
                },
                replace_invalid: false,
            });
        }
    }
    if let Some(result) = runtime.preferences.take_result() {
        runtime.invalid_config = result.committed.is_err();
        i.output.selected = result
            .committed
            .ok()
            .and_then(|c| c.output)
            .map(saved_descriptor);
        if result.revision.to_string() == i.output.revision {
            i.output.pending = None;
            if result.error.is_some() {
                i.output.error = Some(failure("OUTPUT_PREFERENCE_SAVE_FAILED"));
                i.output.status = "error".into();
                i.output_gate.store(false, Ordering::Release);
                if let Some(preview) = i.preview.as_mut() {
                    preview.state = TransportState::Paused;
                    i.playback.status = PlaybackStatus::Paused;
                    i.dirty = true;
                    checkpoint_output_state = true;
                } else if i.session.current_occurrence_id.is_some() {
                    i.session.state = TransportState::Paused;
                    i.playback.status = PlaybackStatus::Paused;
                }
                runtime.switch_generation = None;
            } else if !fenced {
                runtime.effect = runtime.switch_generation.take();
                runtime.opening_generation = runtime.effect.clone();
            }
        }
    }
    if let Some(selected) = i.output.selected.as_mut()
        && let Some(preference) = selected.preference.as_ref()
    {
        match devices::resolve(preference, &inventory.discovery.outputs) {
            Ok(found) if inventory.discovery.can_resolve() => *selected = found.clone(),
            _ => selected.available = false,
        }
    }
    if runtime
        .opening_generation
        .as_ref()
        .is_some_and(|g| g != &i.generation_id)
    {
        runtime.opening_generation = None;
    }
    // The durable selection may already name the replacement while the old
    // stream is retiring. Check the identity of that actual stream separately.
    let lost = i.output.active.as_ref().is_some_and(|active| {
        !inventory.discovery.can_resolve()
            || active.preference.as_ref().is_none_or(|preference| {
                devices::resolve(preference, &inventory.discovery.outputs).is_err()
            })
    }) && runtime.active_generation.as_deref() == Some(i.generation_id.as_str());
    runtime.discovery_sequence = inventory.sequence;
    if lost && !fenced {
        i.output_gate.store(false, Ordering::Release);
        let _ = sample_progress(i, ingress);
        if let Some(position) = super::super::audio::global().captured_position(&i.generation_id) {
            if let Some(preview) = i.preview.as_mut() {
                preview.position_ms = preview.position_ms.max(position);
            } else {
                i.session.position_ms = i.session.position_ms.max(position);
            }
            i.dirty = true;
        }
        super::super::audio::global().control(ControlAction::Stop);
        supersede_pending_seek(i);
        serial.fetch_add(1, Ordering::AcqRel);
        i.generation_id = Uuid::new_v4().to_string();
        // The generation-tagged worker still owns the stream until output_closed.
        // Requesting Stop is not a native retirement acknowledgment.
        i.output.error = Some(failure("OUTPUT_LOST"));
        i.output.status = "unavailable".into();
        if let Some(preview) = i.preview.as_mut() {
            preview.resume_inhibited = true;
            preview.state = TransportState::Paused;
            i.playback.status = PlaybackStatus::Paused;
            i.dirty = true;
        } else if i.session.current_occurrence_id.is_some() {
            i.session.state = TransportState::Paused;
            i.playback.status = PlaybackStatus::Paused;
        }
        let _ = checkpoint_inner(i);
        refresh_ingress(i, ingress);
    } else if i.output.pending.is_none()
        && i.output.status != "error"
        && i.outputs
            .as_ref()
            .is_some_and(|r| r.effect.is_none() && r.opening_generation.is_none())
    {
        i.output.status = if i.output.selected.is_none() {
            "unselected"
        } else if i.output.selected.as_ref().is_some_and(|o| o.available) {
            "available"
        } else {
            "unavailable"
        }
        .into();
    }
    if checkpoint_output_state {
        let _ = checkpoint_inner(i);
    }
    if before != i.output {
        i.state_sequence += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn fixture() -> (tempfile::TempDir, PlaybackSession, OutputDescriptor) {
        fixture_with_outputs(false)
    }

    fn fixture_with_outputs(two: bool) -> (tempfile::TempDir, PlaybackSession, OutputDescriptor) {
        let dir = tempfile::tempdir().unwrap();
        let session = PlaybackSession::restore(
            Arc::new(Database::memory().unwrap()),
            Uuid::new_v4().to_string(),
        );
        let preference = OutputPreference {
            backend: "coreaudio".into(),
            stable_id: "test-output".into(),
            display_name: "Test headphones".into(),
            identity_properties: BTreeMap::new(),
        };
        let mut endpoint = saved_descriptor(preference);
        endpoint.available = true;
        let mut outputs = vec![endpoint.clone()];
        if two {
            let mut second = endpoint.clone();
            second.preference.as_mut().unwrap().stable_id = "second-output".into();
            second.output_id = devices::output_id(second.preference.as_ref().unwrap());
            outputs.push(second);
        }
        session.enable_outputs_with(dir.path().join("playback.json"), move || Discovery {
            outputs: outputs.clone(),
            error: None,
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        while session
            .list_outputs(ListOutputsParams { schema_version: 1 })
            .unwrap()
            .outputs
            .is_empty()
        {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        (dir, session, endpoint)
    }

    fn params(snapshot: &SessionSnapshot, endpoint: &OutputDescriptor) -> SelectOutputParams {
        SelectOutputParams {
            schema_version: 1,
            instance_id: snapshot.instance_id.clone(),
            session_id: snapshot.session_id.clone(),
            command_id: Uuid::new_v4().to_string(),
            expected_output_revision: snapshot.output.revision.clone(),
            expected_generation_id: snapshot.generation_id.clone(),
            output_id: endpoint.output_id.clone(),
            replace_invalid_config: false,
        }
    }

    #[test]
    fn missing_configuration_selects_and_saves_the_concrete_default_without_an_audio_effect() {
        let dir = tempfile::tempdir().unwrap();
        let session = PlaybackSession::restore(
            Arc::new(Database::memory().unwrap()),
            Uuid::new_v4().to_string(),
        );
        let preference = OutputPreference {
            backend: "wasapi".into(),
            stable_id: "default-endpoint".into(),
            display_name: "Built-in Speakers".into(),
            identity_properties: BTreeMap::new(),
        };
        let mut endpoint = saved_descriptor(preference.clone());
        endpoint.available = true;
        endpoint.is_default = true;
        let discovered = endpoint.clone();
        let path = dir.path().join("playback.json");
        session.enable_outputs_with(path.clone(), move || Discovery {
            outputs: vec![discovered.clone()],
            error: None,
        });

        let selected = wait_for(&session, |snapshot| {
            snapshot.output.pending.is_none()
                && snapshot.output.selected.as_ref().is_some_and(|output| {
                    output.output_id == endpoint.output_id && output.available
                })
        });

        assert_eq!(selected.state, TransportState::Idle);
        assert!(selected.current.is_none());
        assert_eq!(
            config::load(&path).unwrap().output,
            Some(preference),
            "the concrete default, not a floating default route, is durable"
        );
        assert!(session.take_output_effect().is_none());
        assert!(!session.output_gate.load(Ordering::Acquire));
        session.stop_and_join().unwrap();
    }

    #[test]
    fn selection_is_durable_without_queue_mutation_and_replay_has_no_new_effect() {
        let (dir, session, endpoint) = fixture();
        let before = session.snapshot().unwrap();
        let request = params(&before, &endpoint);
        let accepted = session.select_output(request.clone(), None).unwrap();
        assert_eq!(accepted.output.revision, "1");
        assert_eq!(accepted.queue_revision, before.queue_revision);
        assert_eq!(accepted.state, TransportState::Idle);
        assert!(accepted.current.is_none());
        let deadline = Instant::now() + Duration::from_secs(2);
        let committed = loop {
            let snapshot = session.snapshot().unwrap();
            if snapshot.output.pending.is_none() {
                break snapshot;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(
            committed.output.selected.as_ref().unwrap().output_id,
            endpoint.output_id
        );
        assert_eq!(
            config::load(&dir.path().join("playback.json"))
                .unwrap()
                .output,
            endpoint.preference
        );
        assert!(session.take_output_effect().is_some());
        session.select_output(request.clone(), None).unwrap();
        assert!(session.take_output_effect().is_none());
        let mut changed = request;
        changed.replace_invalid_config = true;
        assert_eq!(
            session.select_output(changed, None).unwrap_err().code,
            "COMMAND_ID_REUSED"
        );
        assert!(!session.output_gate.load(Ordering::Acquire));
        session.stop_and_join().unwrap();
    }

    #[test]
    fn first_play_keeps_queue_but_cannot_emit_without_explicit_output_choice() {
        let (_dir, session, _endpoint) = fixture();
        let before = session.snapshot().unwrap();
        let result = session
            .apply(ApplySessionParams {
                schema_version: 1,
                instance_id: before.instance_id,
                session_id: before.session_id,
                command_id: Uuid::new_v4().to_string(),
                expected_queue_revision: before.queue_revision,
                operation: SessionOperation::PlayTrack {
                    source: TrackSource {
                        server_id: "server".into(),
                        track_id: "track".into(),
                    },
                },
            })
            .unwrap();
        assert!(!result.start_audio);
        let snapshot = session.snapshot().unwrap();
        assert!(snapshot.current.is_some());
        assert_eq!(snapshot.state, TransportState::Paused);
        assert_eq!(snapshot.output.error.unwrap().code, "OUTPUT_UNAVAILABLE");
        session.stop_and_join().unwrap();
    }

    fn committed(session: &PlaybackSession) -> SessionSnapshot {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let current = session.snapshot().unwrap();
            if current.output.pending.is_none() {
                return current;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn preview_params(snapshot: &SessionSnapshot, track_id: &str) -> PreviewTrackParams {
        PreviewTrackParams {
            schema_version: SCHEMA_VERSION,
            instance_id: snapshot.instance_id.clone(),
            session_id: snapshot.session_id.clone(),
            command_id: Uuid::new_v4().to_string(),
            expected_queue_revision: snapshot.queue_revision.clone(),
            expected_generation_id: snapshot.generation_id.clone(),
            source: TrackSource {
                server_id: "offline".into(),
                track_id: track_id.into(),
            },
        }
    }

    fn select_fixture_output(
        session: &PlaybackSession,
        endpoint: &OutputDescriptor,
    ) -> SessionSnapshot {
        session
            .select_output(params(&session.snapshot().unwrap(), endpoint), None)
            .unwrap();
        let selected = committed(session);
        session.take_output_effect();
        selected
    }

    #[test]
    fn preview_output_open_uses_buffering_overlay_over_paused_main() {
        let (_dir, session, endpoint) = fixture();
        let selected = select_fixture_output(&session, &endpoint);
        session
            .apply(ApplySessionParams {
                schema_version: SCHEMA_VERSION,
                instance_id: selected.instance_id,
                session_id: selected.session_id,
                command_id: Uuid::new_v4().to_string(),
                expected_queue_revision: selected.queue_revision,
                operation: SessionOperation::PlayTrack {
                    source: TrackSource {
                        server_id: "offline".into(),
                        track_id: "main".into(),
                    },
                },
            })
            .unwrap();
        let preview = session
            .preview_with_guard(
                preview_params(&session.snapshot().unwrap(), "audition"),
                None,
            )
            .unwrap();

        assert_eq!(preview.state, TransportState::Buffering);
        assert!(session.output_opened(
            &preview.generation_id,
            endpoint.preference.as_ref().unwrap()
        ));
        assert!(session.output_gate.load(Ordering::Acquire));
        session.stop_and_join().unwrap();
    }

    #[test]
    fn preview_output_open_uses_buffering_overlay_over_idle_main() {
        let (_dir, session, endpoint) = fixture();
        select_fixture_output(&session, &endpoint);
        let preview = session
            .preview_with_guard(
                preview_params(&session.snapshot().unwrap(), "audition"),
                None,
            )
            .unwrap();

        assert_eq!(preview.state, TransportState::Buffering);
        assert!(session.output_opened(
            &preview.generation_id,
            endpoint.preference.as_ref().unwrap()
        ));
        assert!(session.output_gate.load(Ordering::Acquire));
        session.stop_and_join().unwrap();
    }

    #[test]
    fn preview_output_open_keeps_paused_overlay_gate_closed() {
        let (_dir, session, endpoint) = fixture();
        select_fixture_output(&session, &endpoint);
        let preview = session
            .preview_with_guard(
                preview_params(&session.snapshot().unwrap(), "audition"),
                None,
            )
            .unwrap();
        let paused = session
            .control_with_guard(
                ControlParams {
                    schema_version: SCHEMA_VERSION,
                    instance_id: preview.instance_id,
                    session_id: preview.session_id,
                    command_id: Uuid::new_v4().to_string(),
                    expected_generation_id: preview.generation_id,
                    occurrence_id: preview.current.unwrap().occurrence_id,
                    action: ControlAction::Pause,
                },
                None,
            )
            .unwrap();

        assert_eq!(paused.state, TransportState::Paused);
        assert!(
            session.output_opened(&paused.generation_id, endpoint.preference.as_ref().unwrap())
        );
        assert!(!session.output_gate.load(Ordering::Acquire));
        session.stop_and_join().unwrap();
    }

    fn wait_for(
        session: &PlaybackSession,
        predicate: impl Fn(&SessionSnapshot) -> bool,
    ) -> SessionSnapshot {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            session
                .list_outputs(ListOutputsParams { schema_version: 1 })
                .unwrap();
            let snapshot = session.snapshot().unwrap();
            if predicate(&snapshot) {
                return snapshot;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn rename_and_unrelated_discovery_errors_preserve_current_endpoint() {
        let (dir, original, endpoint) = fixture();
        original.stop_and_join().unwrap();
        let inventory = Arc::new(Mutex::new(Discovery {
            outputs: vec![endpoint.clone()],
            error: None,
        }));
        let discovered = inventory.clone();
        let session = PlaybackSession::restore(
            Arc::new(Database::memory().unwrap()),
            Uuid::new_v4().to_string(),
        );
        session.enable_outputs_with(dir.path().join("playback.json"), move || {
            discovered.lock().unwrap().clone()
        });
        wait_for(&session, |_| {
            session
                .list_outputs(ListOutputsParams { schema_version: 1 })
                .unwrap()
                .outputs
                .len()
                == 1
        });
        session
            .select_output(params(&session.snapshot().unwrap(), &endpoint), None)
            .unwrap();
        let saved = committed(&session);
        session.take_output_effect();
        {
            let mut inventory = inventory.lock().unwrap();
            inventory.outputs[0].display_name = "Renamed headphones".into();
            inventory.outputs[0]
                .preference
                .as_mut()
                .unwrap()
                .display_name = "Renamed headphones".into();
        }
        wait_for(&session, |s| {
            s.output.selected.as_ref().unwrap().display_name == "Renamed headphones"
        });
        // The opening worker captured the previous label, but the same identity.
        assert!(session.output_opened(&saved.generation_id, endpoint.preference.as_ref().unwrap()));
        assert_eq!(
            session
                .snapshot()
                .unwrap()
                .output
                .active
                .unwrap()
                .display_name,
            "Renamed headphones"
        );
        for code in [
            "OUTPUT_DISCOVERY_PARTIAL",
            "OUTPUT_ENUMERATION_TRUNCATED",
            "OUTPUT_IDENTITY_AMBIGUOUS",
        ] {
            {
                let mut inventory = inventory.lock().unwrap();
                inventory.error = Some(code);
                inventory.outputs[0].detail = code.into();
            }
            let snapshot = wait_for(&session, |s| {
                s.output.selected.as_ref().unwrap().detail == code
            });
            assert_eq!(snapshot.generation_id, saved.generation_id);
            assert!(snapshot.output.selected.as_ref().unwrap().available);
            assert!(snapshot.output.active.is_some());
            assert!(snapshot.output.error.is_none());
            // A valid endpoint is also selectable despite an incomplete inventory.
            assert!(
                session
                    .select_output(params(&snapshot, &endpoint), None)
                    .is_ok()
            );
        }
        session.stop_and_join().unwrap();
    }

    #[test]
    fn terminal_source_failures_finish_output_switch_but_stale_failures_do_not() {
        for code in ["SOURCE_UNAVAILABLE", "PLAYBACK_TIMEOUT", "DECODE_FAILED"] {
            let (_dir, session, endpoint) = fixture();
            session
                .select_output(params(&session.snapshot().unwrap(), &endpoint), None)
                .unwrap();
            let saved = committed(&session);
            session.take_output_effect();
            session.publish_event(
                Uuid::new_v4().to_string(),
                PlaybackEvent::Failed {
                    code: code.into(),
                    retryable: true,
                },
            );
            assert_eq!(session.snapshot().unwrap().output.status, "switching");
            session.publish_event(
                saved.generation_id,
                PlaybackEvent::Failed {
                    code: code.into(),
                    retryable: true,
                },
            );
            let failed = wait_for(&session, |s| s.output.status != "switching");
            assert_eq!(failed.output.status, "available");
            assert_eq!(failed.playback.error.unwrap().code, code);
            assert!(!session.output_gate.load(Ordering::Acquire));
            session.stop_and_join().unwrap();
        }
    }

    #[test]
    fn pause_stop_and_quit_fence_late_output_open_without_losing_preference() {
        for action in [ControlAction::Pause, ControlAction::Stop] {
            let (_dir, session, endpoint) = fixture();
            let initial = session.snapshot().unwrap();
            session
                .apply(ApplySessionParams {
                    schema_version: 1,
                    instance_id: initial.instance_id.clone(),
                    session_id: initial.session_id.clone(),
                    command_id: Uuid::new_v4().to_string(),
                    expected_queue_revision: initial.queue_revision.clone(),
                    operation: SessionOperation::PlayTrack {
                        source: TrackSource {
                            server_id: "server".into(),
                            track_id: "track".into(),
                        },
                    },
                })
                .unwrap();
            let before = session.snapshot().unwrap();
            let accepted = session
                .select_output(params(&before, &endpoint), None)
                .unwrap();
            session
                .control_with_guard(
                    ControlParams {
                        schema_version: 1,
                        instance_id: accepted.instance_id,
                        session_id: accepted.session_id,
                        command_id: Uuid::new_v4().to_string(),
                        expected_generation_id: accepted.generation_id.clone(),
                        occurrence_id: accepted.current.unwrap().occurrence_id,
                        action,
                    },
                    None,
                )
                .unwrap();
            let saved = committed(&session);
            assert!(saved.output.selected.is_some());
            assert_eq!(saved.queue_revision, before.queue_revision);
            assert_eq!(
                session.output_opened(
                    &accepted.generation_id,
                    endpoint.preference.as_ref().unwrap()
                ),
                action == ControlAction::Pause
            );
            assert!(!session.output_gate.load(Ordering::Acquire));
            if action == ControlAction::Stop {
                assert!(session.take_output_effect().is_none());
            }
            session
                .begin_shutdown_checkpoint()
                .unwrap()
                .recv()
                .unwrap()
                .unwrap();
            assert!(
                !session.output_opened(&saved.generation_id, endpoint.preference.as_ref().unwrap())
            );
            session.stop_and_join().unwrap();
        }
    }

    #[test]
    fn configuration_reset_requires_explicit_consent_and_restart_never_opens() {
        let (dir, session, endpoint) = fixture();
        session.stop_and_join().unwrap();
        let path = dir.path().join("playback.json");
        std::fs::write(&path, b"invalid evidence").unwrap();
        let restored = PlaybackSession::restore(
            Arc::new(Database::memory().unwrap()),
            Uuid::new_v4().to_string(),
        );
        let discovered = endpoint.clone();
        restored.enable_outputs_with(path.clone(), move || Discovery {
            outputs: vec![discovered.clone()],
            error: None,
        });
        let initial = restored.snapshot().unwrap();
        assert_eq!(
            initial.output.error.as_ref().unwrap().code,
            "OUTPUT_CONFIG_INVALID"
        );
        let request = params(&initial, &endpoint);
        assert_eq!(
            restored
                .select_output(request.clone(), None)
                .unwrap_err()
                .code,
            "OUTPUT_CONFIG_INVALID"
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"invalid evidence");
        assert!(restored.take_output_effect().is_none());
        let deadline = Instant::now() + Duration::from_secs(2);
        while restored
            .list_outputs(ListOutputsParams { schema_version: 1 })
            .unwrap()
            .outputs
            .is_empty()
        {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        let mut reset = params(&restored.snapshot().unwrap(), &endpoint);
        reset.replace_invalid_config = true;
        restored.select_output(reset, None).unwrap();
        committed(&restored);
        restored.stop_and_join().unwrap();
        let reopened = PlaybackSession::restore(
            Arc::new(Database::memory().unwrap()),
            Uuid::new_v4().to_string(),
        );
        reopened.enable_outputs_with(path, || Discovery {
            outputs: vec![],
            error: None,
        });
        let snapshot = reopened.snapshot().unwrap();
        assert!(snapshot.output.selected.is_some());
        assert!(!snapshot.output.selected.unwrap().available);
        assert!(snapshot.output.active.is_none());
        assert!(reopened.take_output_effect().is_none());
        assert!(!reopened.output_gate.load(Ordering::Acquire));
        reopened.stop_and_join().unwrap();
    }

    #[test]
    fn rapid_choices_only_claim_latest_effect_and_late_open_cannot_replace_it() {
        let (dir, session, a) = fixture();
        session.stop_and_join().unwrap();
        let mut b = a.clone();
        b.preference.as_mut().unwrap().stable_id = "endpoint-b".into();
        b.output_id = devices::output_id(b.preference.as_ref().unwrap());
        let mut c = a.clone();
        c.preference.as_mut().unwrap().stable_id = "endpoint-c".into();
        c.output_id = devices::output_id(c.preference.as_ref().unwrap());
        let session = PlaybackSession::restore(
            Arc::new(Database::memory().unwrap()),
            Uuid::new_v4().to_string(),
        );
        let inventory = vec![a.clone(), b.clone(), c.clone()];
        session.enable_outputs_with(dir.path().join("playback.json"), move || Discovery {
            outputs: inventory.clone(),
            error: None,
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        while session
            .list_outputs(ListOutputsParams { schema_version: 1 })
            .unwrap()
            .outputs
            .is_empty()
        {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        let mut generations = Vec::new();
        for endpoint in [&a, &b, &c] {
            let snapshot = session.snapshot().unwrap();
            let accepted = session
                .select_output(params(&snapshot, endpoint), None)
                .unwrap();
            generations.push(accepted.generation_id);
        }
        let final_state = committed(&session);
        assert_eq!(
            final_state.output.selected.as_ref().unwrap().output_id,
            c.output_id
        );
        assert_eq!(
            config::load(&dir.path().join("playback.json"))
                .unwrap()
                .output,
            c.preference
        );
        let effect = session.take_output_effect().unwrap();
        assert_eq!(effect.generation_id, generations[2]);
        assert!(session.take_output_effect().is_none());
        assert!(!session.output_opened(&generations[0], a.preference.as_ref().unwrap()));
        assert!(!session.output_opened(&generations[1], b.preference.as_ref().unwrap()));
        assert!(session.output_opened(&generations[2], c.preference.as_ref().unwrap()));
        session.output_closed(&generations[0]);
        assert_eq!(
            session.snapshot().unwrap().output.active.unwrap().output_id,
            c.output_id
        );
        assert!(!session.output_gate.load(Ordering::Acquire));
        let before = session.snapshot().unwrap();
        let same = session.select_output(params(&before, &c), None).unwrap();
        assert_eq!(same.generation_id, before.generation_id);
        assert_eq!(same.output.revision, before.output.revision);
        assert!(session.take_output_effect().is_none());
        session.stop_and_join().unwrap();
    }

    #[test]
    fn loss_freezes_progress_and_reconnect_or_default_change_never_reopens() {
        let (dir, original, endpoint) = fixture();
        original.stop_and_join().unwrap();
        let inventory = Arc::new(Mutex::new(vec![endpoint.clone()]));
        let discovered = inventory.clone();
        let session = PlaybackSession::restore(
            Arc::new(Database::memory().unwrap()),
            Uuid::new_v4().to_string(),
        );
        session.enable_outputs_with(dir.path().join("playback.json"), move || Discovery {
            outputs: discovered.lock().unwrap().clone(),
            error: None,
        });
        let wait = |predicate: &dyn Fn(&SessionSnapshot) -> bool| {
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                session
                    .list_outputs(ListOutputsParams { schema_version: 1 })
                    .unwrap();
                let snapshot = session.snapshot().unwrap();
                if predicate(&snapshot) {
                    break snapshot;
                }
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(5));
            }
        };
        let deadline = Instant::now() + Duration::from_secs(2);
        while session
            .list_outputs(ListOutputsParams { schema_version: 1 })
            .unwrap()
            .outputs
            .is_empty()
        {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        session
            .select_output(params(&session.snapshot().unwrap(), &endpoint), None)
            .unwrap();
        committed(&session);
        session.take_output_effect();
        let selected = session.snapshot().unwrap();
        session
            .apply(ApplySessionParams {
                schema_version: 1,
                instance_id: selected.instance_id,
                session_id: selected.session_id,
                command_id: Uuid::new_v4().to_string(),
                expected_queue_revision: selected.queue_revision,
                operation: SessionOperation::PlayTrack {
                    source: TrackSource {
                        server_id: "server".into(),
                        track_id: "track".into(),
                    },
                },
            })
            .unwrap();
        let playing = session.snapshot().unwrap();
        assert!(session.output_opened(
            &playing.generation_id,
            endpoint.preference.as_ref().unwrap()
        ));
        let occurrence = playing.current.as_ref().unwrap().occurrence_id.clone();
        session
            .report_progress(&playing.generation_id, &occurrence, 1, 1234)
            .unwrap();
        inventory.lock().unwrap()[0].is_default = true;
        wait(&|s| s.output.selected.as_ref().is_some_and(|o| o.is_default));
        assert_eq!(
            session.snapshot().unwrap().generation_id,
            playing.generation_id
        );
        assert!(session.output_gate.load(Ordering::Acquire));
        inventory.lock().unwrap().clear();
        let lost = wait(&|s| {
            s.output
                .error
                .as_ref()
                .is_some_and(|e| e.code == "OUTPUT_LOST")
        });
        assert_eq!(lost.state, TransportState::Paused);
        assert_eq!(lost.position_ms, 1234);
        assert_eq!(lost.queue_revision, playing.queue_revision);
        assert_ne!(lost.generation_id, playing.generation_id);
        assert!(
            lost.output.active.is_some(),
            "loss does not acknowledge native retirement"
        );
        session.output_closed(&lost.generation_id); // Wrong generation cannot clear the old handle.
        assert!(session.snapshot().unwrap().output.active.is_some());
        session.output_closed(&playing.generation_id);
        assert!(session.snapshot().unwrap().output.active.is_none());
        assert!(!session.output_gate.load(Ordering::Acquire));
        session.publish_event(playing.generation_id.clone(), PlaybackEvent::Active);
        assert!(
            session
                .report_progress(&playing.generation_id, &occurrence, 2, 9999)
                .is_err()
        );
        inventory.lock().unwrap().push(endpoint.clone());
        let reconnected = wait(&|s| s.output.selected.as_ref().is_some_and(|o| o.available));
        assert_eq!(reconnected.state, TransportState::Paused);
        assert_eq!(reconnected.position_ms, 1234);
        assert!(reconnected.output.active.is_none());
        assert!(session.take_output_effect().is_none());
        assert!(!session.output_gate.load(Ordering::Acquire));
        session.stop_and_join().unwrap();
    }

    #[test]
    fn native_loss_after_close_rotates_generation_and_rejects_late_activity() {
        let (_dir, session, endpoint) = fixture();
        session
            .select_output(params(&session.snapshot().unwrap(), &endpoint), None)
            .unwrap();
        committed(&session);
        session.take_output_effect();
        let selected = session.snapshot().unwrap();
        session
            .apply(ApplySessionParams {
                schema_version: 1,
                instance_id: selected.instance_id,
                session_id: selected.session_id,
                command_id: Uuid::new_v4().to_string(),
                expected_queue_revision: selected.queue_revision,
                operation: SessionOperation::PlayTrack {
                    source: TrackSource {
                        server_id: "server".into(),
                        track_id: "track".into(),
                    },
                },
            })
            .unwrap();
        let playing = session.snapshot().unwrap();
        session.output_opened(
            &playing.generation_id,
            endpoint.preference.as_ref().unwrap(),
        );
        let occurrence = playing.current.unwrap().occurrence_id;
        session
            .report_progress(&playing.generation_id, &occurrence, 1, 2345)
            .unwrap();
        // Real workers close their native stream before publishing terminal failure.
        session.output_closed(&playing.generation_id);
        session.publish_event(
            playing.generation_id.clone(),
            PlaybackEvent::Failed {
                code: "OUTPUT_LOST".into(),
                retryable: true,
            },
        );
        let deadline = Instant::now() + Duration::from_secs(2);
        let failed = loop {
            let snapshot = session.snapshot().unwrap();
            if snapshot.playback.error.is_some() {
                break snapshot;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_ne!(failed.generation_id, playing.generation_id);
        assert_eq!(failed.position_ms, 2345);
        assert_eq!(failed.state, TransportState::Paused);
        session.publish_event(playing.generation_id.clone(), PlaybackEvent::Active);
        assert!(
            session
                .report_progress(&playing.generation_id, &occurrence, 2, 9999)
                .is_err()
        );
        assert!(!session.output_opened(
            &playing.generation_id,
            endpoint.preference.as_ref().unwrap()
        ));
        assert!(!session.output_gate.load(Ordering::Acquire));
        assert!(session.take_output_effect().is_none());
        session.stop_and_join().unwrap();
    }

    #[test]
    fn selection_preserves_every_transport_intent_occurrence_and_position() {
        for (target, buffering) in [
            (PlaybackStatus::Idle, false),
            (PlaybackStatus::Loading, false),
            (PlaybackStatus::Active, false),
            (PlaybackStatus::Loading, true),
            (PlaybackStatus::Paused, false),
            (PlaybackStatus::Stopped, false),
            (PlaybackStatus::Completed, false),
        ] {
            let (_dir, session, a) = fixture_with_outputs(true);
            let b = session
                .list_outputs(ListOutputsParams { schema_version: 1 })
                .unwrap()
                .outputs
                .into_iter()
                .find(|o| o.output_id != a.output_id)
                .unwrap();
            session
                .select_output(params(&session.snapshot().unwrap(), &a), None)
                .unwrap();
            committed(&session);
            session.take_output_effect();
            if target != PlaybackStatus::Idle {
                let snapshot = session.snapshot().unwrap();
                session
                    .apply(ApplySessionParams {
                        schema_version: 1,
                        instance_id: snapshot.instance_id,
                        session_id: snapshot.session_id,
                        command_id: Uuid::new_v4().to_string(),
                        expected_queue_revision: snapshot.queue_revision,
                        operation: SessionOperation::PlayTrack {
                            source: TrackSource {
                                server_id: "server".into(),
                                track_id: "track".into(),
                            },
                        },
                    })
                    .unwrap();
                let current = session.snapshot().unwrap();
                session.output_opened(&current.generation_id, a.preference.as_ref().unwrap());
                session
                    .report_progress(
                        &current.generation_id,
                        &current.current.as_ref().unwrap().occurrence_id,
                        1,
                        4567,
                    )
                    .unwrap();
                match target {
                    PlaybackStatus::Active => {
                        session.publish_event(current.generation_id, PlaybackEvent::Active)
                    }
                    PlaybackStatus::Loading if buffering => {
                        session.publish_event(current.generation_id, PlaybackEvent::Buffering)
                    }
                    PlaybackStatus::Completed => session.publish_event(
                        current.generation_id,
                        PlaybackEvent::Completed { position_ms: 4567 },
                    ),
                    PlaybackStatus::Paused | PlaybackStatus::Stopped => {
                        session
                            .control_with_guard(
                                ControlParams {
                                    schema_version: 1,
                                    instance_id: current.instance_id,
                                    session_id: current.session_id,
                                    command_id: Uuid::new_v4().to_string(),
                                    expected_generation_id: current.generation_id,
                                    occurrence_id: current.current.unwrap().occurrence_id,
                                    action: if target == PlaybackStatus::Paused {
                                        ControlAction::Pause
                                    } else {
                                        ControlAction::Stop
                                    },
                                },
                                None,
                            )
                            .unwrap();
                    }
                    _ => {}
                }
            }
            let deadline = Instant::now() + Duration::from_secs(2);
            let before = loop {
                let snapshot = session.snapshot().unwrap();
                if snapshot.playback.status == target {
                    break snapshot;
                }
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(5));
            };
            session.select_output(params(&before, &b), None).unwrap();
            let saved = committed(&session);
            let effect = session.take_output_effect().unwrap();
            assert_ne!(effect.generation_id, before.generation_id);
            assert_eq!(saved.queue_revision, before.queue_revision);
            // Selection must sample the newest coalesced progress, which may
            // be newer than the last periodically refreshed snapshot.
            let expected_position =
                if matches!(target, PlaybackStatus::Idle | PlaybackStatus::Stopped) {
                    0
                } else {
                    4567
                };
            assert_eq!(saved.position_ms, expected_position);
            assert_eq!(effect.position_ms, expected_position);
            assert_eq!(
                saved.current.as_ref().map(|c| &c.occurrence_id),
                before.current.as_ref().map(|c| &c.occurrence_id)
            );
            assert_eq!(saved.state, before.state);
            assert_eq!(saved.playback.status, before.playback.status);
            assert!(!session.output_gate.load(Ordering::Acquire));
            assert!(session.output_opened(&effect.generation_id, b.preference.as_ref().unwrap()));
            assert_eq!(
                session.output_gate.load(Ordering::Acquire),
                matches!(
                    before.state,
                    TransportState::Playing | TransportState::Buffering
                )
            );
            session.stop_and_join().unwrap();
        }
    }

    #[test]
    fn select_command_namespace_and_revisions_are_strict() {
        let (_dir, session, endpoint) = fixture();
        let initial = session.snapshot().unwrap();
        let mut request = params(&initial, &endpoint);
        request.expected_output_revision = "01".into();
        assert!(session.select_output(request, None).is_err());
        let request = params(&initial, &endpoint);
        session.select_output(request.clone(), None).unwrap();
        let result = session.apply(ApplySessionParams {
            schema_version: 1,
            instance_id: initial.instance_id.clone(),
            session_id: initial.session_id.clone(),
            command_id: request.command_id,
            expected_queue_revision: initial.queue_revision.clone(),
            operation: SessionOperation::Clear,
        });
        assert_eq!(result.unwrap_err().code, "COMMAND_ID_REUSED");
        let mut stale = params(&initial, &endpoint);
        stale.expected_generation_id = session.snapshot().unwrap().generation_id;
        assert_eq!(
            session.select_output(stale, None).unwrap_err().code,
            "OUTPUT_REVISION_CONFLICT"
        );
        let mut json =
            serde_json::to_value(params(&session.snapshot().unwrap(), &endpoint)).unwrap();
        json["unexpected"] = true.into();
        assert!(serde_json::from_value::<SelectOutputParams>(json).is_err());
        session.stop_and_join().unwrap();
    }
    #[test]
    fn review_output_switch_supersedes_seek_and_resume_dispatches_again() {
        let (_dir, session, endpoint) = fixture_with_outputs(true);
        let before = session.snapshot().unwrap();
        session
            .select_output(params(&before, &endpoint), None)
            .unwrap();
        let before = committed(&session);
        session
            .apply(ApplySessionParams {
                schema_version: 1,
                instance_id: before.instance_id,
                session_id: before.session_id,
                command_id: Uuid::new_v4().to_string(),
                expected_queue_revision: before.queue_revision,
                operation: SessionOperation::PlayTrack {
                    source: TrackSource {
                        server_id: "server".into(),
                        track_id: "track".into(),
                    },
                },
            })
            .unwrap();
        let current = session.snapshot().unwrap();
        session.publish_event(
            current.generation_id.clone(),
            PlaybackEvent::Resolved {
                metadata: PlaybackTrackMetadata {
                    source: current.current.as_ref().unwrap().source.clone(),
                    title: "fixture".into(),
                    artist: None,
                    album: None,
                },
                duration_ms: Some(10_000),
                representation: "wav".into(),
                seek: SeekCapability::jellyfin_pcm_wav(),
            },
        );
        let qualified = session.snapshot().unwrap();
        let pending = session
            .seek_with_guard(
                SeekParams {
                    schema_version: 1,
                    instance_id: qualified.instance_id,
                    session_id: qualified.session_id,
                    command_id: Uuid::new_v4().to_string(),
                    expected_generation_id: qualified.generation_id,
                    occurrence_id: qualified.current.unwrap().occurrence_id,
                    position_ms: 4_000,
                },
                None,
            )
            .unwrap();
        let replacement = session
            .list_outputs(ListOutputsParams { schema_version: 1 })
            .unwrap()
            .outputs
            .into_iter()
            .find(|o| o.output_id != endpoint.output_id)
            .unwrap();
        session
            .select_output(params(&pending, &replacement), None)
            .unwrap();
        let switched = committed(&session);
        assert!(switched.playback.pending_seek.is_none());
        assert_eq!(
            switched.playback.seek_outcome.as_ref().unwrap().status,
            "superseded"
        );
        session.publish_event_at_epoch(
            pending.generation_id,
            PlaybackEvent::SeekCommitted {
                operation_id: pending.playback.pending_seek.unwrap().operation_id,
                requested_position_ms: 4_000,
                actual_position_ms: 4_000,
            },
            pending.seek_epoch,
        );
        assert_eq!(session.snapshot().unwrap().position_ms, 0);
        let resumed = session
            .native_control(NativeControlIntent::Play, None)
            .unwrap();
        assert!(resumed.resume_audio);
        assert!(resumed.playback.pending_seek.is_none());
        session.stop_and_join().unwrap();
    }
}
