use super::*;
use crate::playback::{
    decoder::decode_stream_with_seek_and_gain,
    devices::pulse_stream::PinnedStream,
    output::{BoundaryPcmConsumer, PresentationActivity, PresentationLedger},
};
use std::time::Duration;

const STARTUP_LOCAL_RESERVE_MILLISECONDS: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct StartupPlan {
    server_samples: usize,
    local_reserve_samples: usize,
    initial_samples: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct StartupPrimer {
    target_samples: usize,
    written_samples: usize,
}

#[derive(Default)]
struct SubmissionCursor {
    next_frame: u64,
}

impl SubmissionCursor {
    fn record(
        &mut self,
        cursor: Option<u64>,
        submitted_samples: usize,
        channels: u16,
        rendered: &crate::playback::output::Rendered,
        ledger: &mut PresentationLedger,
    ) -> bool {
        let channels = u64::from(channels);
        let start = cursor.unwrap_or(self.next_frame).max(self.next_frame);
        self.next_frame = start.saturating_add(submitted_samples as u64 / channels);
        let Some(end) = rendered.last_audio_frame else {
            return true;
        };
        let frames = rendered.samples / channels;
        ledger.record(start + end as u64 - frames, frames)
    }
}

impl StartupPrimer {
    fn new(target_samples: usize) -> Self {
        Self {
            target_samples,
            written_samples: 0,
        }
    }

    fn next_write(&self, writable_samples: usize, scratch_samples: usize) -> usize {
        writable_samples
            .min(scratch_samples)
            .min(self.target_samples.saturating_sub(self.written_samples))
    }

    fn record_write(&mut self, samples: usize) {
        self.written_samples = self
            .written_samples
            .saturating_add(samples)
            .min(self.target_samples);
    }

    fn is_ready(self, decoder_finished: bool, pcm_empty: bool) -> bool {
        self.written_samples >= self.target_samples || decoder_finished && pcm_empty
    }
}

fn startup_plan(rate: u32, channels: u16, max_bytes: usize) -> StartupPlan {
    let channels = channels as usize;
    let server_samples = (max_bytes / std::mem::size_of::<f32>()) / channels * channels;
    let local_reserve_samples =
        rate as usize * channels * STARTUP_LOCAL_RESERVE_MILLISECONDS / 1000;
    StartupPlan {
        server_samples,
        local_reserve_samples,
        initial_samples: server_samples + local_reserve_samples,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn run_output(
    reader: BoundedHttpReader,
    hint: &str,
    generation: String,
    session: crate::playback::PlaybackSession,
    cancel: Arc<AtomicBool>,
    gate: Arc<AtomicBool>,
    generation_serial: Arc<AtomicU64>,
    expected_serial: u64,
    event_epoch: Arc<AtomicU64>,
    start_ms: u64,
    pcm_high_water: Arc<AtomicU64>,
    endpoint: Arc<Mutex<Option<String>>>,
    position_ms: Arc<AtomicU64>,
    _output_control: Arc<OutputControl>,
    successor_rx: mpsc::Receiver<PreparedSuccessor>,
    preparation: crate::playback::http_source::Preparation,
    seek_mechanism: Option<crate::providers::PlaybackSeekMechanism>,
    provider_duration_ms: u64,
    mut seek_commit: Option<(String, u64)>,
    mut back_commit: Option<String>,
    back_pending: Arc<AtomicBool>,
    boundary_pending: Arc<AtomicBool>,
    handoff: Arc<HandoffReceipt>,
    successor_fence: Arc<SuccessorFence>,
    successor_slot_free: Arc<AtomicBool>,
    gain: f32,
    qualified_suffix: Option<String>,
) -> Result<(), PlaybackPipelineError> {
    let preference = session
        .selected_output(&generation)
        .map_err(PlaybackPipelineError::output_policy)?;
    if preference.backend != "pulse" {
        return Err(PlaybackPipelineError::output_policy(
            "OUTPUT_SHARED_UNSUPPORTED",
        ));
    }
    let mut output = PinnedStream::open(&preference, preparation.deadline())
        .map_err(PlaybackPipelineError::output_policy)?;
    SERVER_BUFFER_MAX_BYTES.store(output.max_bytes as u64, Ordering::Release);
    let rate = output.rate;
    let channels = output.channels;
    let startup = startup_plan(rate, channels, output.target_bytes);
    *endpoint.lock().unwrap_or_else(|e| e.into_inner()) = Some(preference.display_name.clone());
    let capacity = (rate as usize * channels as usize * PCM_TARGET_MILLISECONDS / 1000)
        .min(PCM_CAPACITY_MAX_BYTES / 4);
    let pcm = Arc::new(ArrayQueue::new(capacity));
    let successor_pcm = Arc::new(ArrayQueue::new(capacity));
    let decoder_pcm = pcm.clone();
    let decoder_cancel = cancel.clone();
    let hint = hint.to_owned();
    let media_seek_requested = seek_commit.is_some();
    let seek_candidate = seek_mechanism.is_some();
    let seek_qualified = Arc::new(AtomicU64::new(0));
    let decoder_seek_qualified = seek_qualified.clone();
    let seek_landing_frame = media_seek_requested.then(|| {
        Arc::new(AtomicU64::new(
            crate::playback::decoder::UNKNOWN_SEEK_LANDING_FRAME,
        ))
    });
    let decoder_seek_landing = seek_landing_frame.clone();
    let stream_failure = reader.failure_state();
    let decoder = crate::playback::output::DecoderWorker::spawn_result(cancel.clone(), move || {
        decode_stream_with_seek_and_gain(
            reader,
            Some(&hint),
            rate,
            channels,
            start_ms.saturating_mul(u64::from(rate)) / 1000,
            seek_mechanism,
            media_seek_requested,
            Some(provider_duration_ms),
            Some(decoder_seek_qualified),
            decoder_seek_landing,
            gain,
            qualified_suffix.as_deref(),
            decoder_pcm,
            decoder_cancel,
        )
    });
    let mut consumer = BoundaryPcmConsumer::with_gate(
        channels as usize,
        rate as usize * channels as usize / 10,
        gate.clone(),
    )
    .with_successor_fence(successor_fence.clone());
    let mut ledger = PresentationLedger::new();
    let mut scratch = vec![0f32; rate as usize * channels as usize / 100];
    let mut occurrence = session
        .snapshot()
        .ok()
        .and_then(|s| s.current)
        .map(|o| o.occurrence_id)
        .unwrap_or_default();
    let mut sequence = 0;
    let mut base_position_ms = start_ms;
    let mut last_frames = 0;
    let mut last_tick = std::time::SystemTime::now();
    let mut ready = false;
    let mut startup_primer = StartupPrimer::new(startup.server_samples);
    let mut submission_cursor = SubmissionCursor::default();
    let mut activity = PresentationActivity::default();
    let mut successor_decoder: Option<
        crate::playback::output::DecoderWorker<
            anyhow::Result<crate::playback::decoder::DecodeSummary>,
        >,
    > = None;
    let mut slot_a_decoder = Some(decoder);
    let mut slot_a_ready = true;
    let mut active_slot = 0usize;
    let mut successor_identity: Option<(
        crate::playback::continuity::HandoffToken,
        PlaybackTrackMetadata,
        u64,
        String,
    )> = None;
    let mut successor_preparation = None;
    let mut installed_successor_epoch = None;
    let mut successor_ready = false;
    let mut boundary_played_frame = None;
    let mut submitted_audio_frames = 0u64;
    let mut boundary_audio_frame = 0u64;
    let mut handoff_published = false;
    let mut presentation_base_frames = 0u64;
    let activity_clock = std::time::Instant::now();
    let mut report_stall = || {
        session.publish_pipeline_event(
            expected_serial,
            generation.clone(),
            PlaybackEvent::Failed {
                code: "OUTPUT_RETIREMENT_PENDING".into(),
                retryable: true,
            },
            event_epoch.load(Ordering::Acquire),
        )
    };
    let (pause_tx, pause_rx) = mpsc::sync_channel::<mpsc::SyncSender<bool>>(1);
    let result = (|| {
        loop {
            if let Ok(reply) = pause_rx.try_recv() {
                let result = output.pause_snapshot(&mut report_stall);
                if let Ok(played) = result {
                    let frames = ledger.advance(played);
                    if boundary_played_frame.is_some_and(|boundary| played >= boundary) {
                        handoff.pulse_presented(frames.saturating_sub(boundary_audio_frame));
                    }
                    position_ms.store(
                        base_position_ms
                            + frames
                                .saturating_sub(presentation_base_frames)
                                .saturating_mul(1000)
                                / u64::from(rate),
                        Ordering::Release,
                    );
                }
                let _ = reply.send(result.is_ok());
                result.map_err(PlaybackPipelineError::output_policy)?;
            }
            if cancel.load(Ordering::Acquire)
                || generation_serial.load(Ordering::Acquire) != expected_serial
            {
                break;
            }
            if last_tick
                .elapsed()
                .map_or(true, |gap| gap > Duration::from_secs(2))
            {
                return Err(PlaybackPipelineError::output_policy("OUTPUT_LOST"));
            }
            last_tick = std::time::SystemTime::now();
            output
                .pump()
                .map_err(PlaybackPipelineError::output_policy)?;
            if installed_successor_epoch.is_some_and(|epoch| epoch != successor_fence.epoch()) {
                retire_successor_slot(
                    active_slot,
                    &mut slot_a_decoder,
                    &mut successor_decoder,
                    &pcm,
                    &successor_pcm,
                );
                if active_slot == 0 {
                    successor_ready = false;
                } else {
                    slot_a_ready = false;
                }
                successor_identity = None;
                successor_preparation = None;
                installed_successor_epoch = None;
                handoff.clear();
                successor_slot_free.store(true, Ordering::Release);
            }
            if successor_identity.is_none()
                && let Ok(prepared) = successor_rx.try_recv()
            {
                let PreparedSuccessor {
                    token,
                    reader,
                    decoder_hint,
                    duration_ms,
                    metadata,
                    representation,
                    preparation,
                    seek_mechanism,
                    gain,
                    qualified_suffix,
                    successor_epoch: prepared_epoch,
                    cancel: successor_cancel,
                } = prepared;
                if prepared_epoch != successor_fence.epoch() {
                    drop(reader);
                    successor_slot_free.store(true, Ordering::Release);
                    continue;
                }
                installed_successor_epoch = Some(prepared_epoch);
                let qualified = Arc::new(AtomicU64::new(0));
                let decoder_qualified = qualified.clone();
                handoff.arm(
                    PlaybackEvent::HandoffPresented {
                        token: token.clone(),
                        metadata: metadata.clone(),
                        duration_ms,
                        representation: representation.clone(),
                        predecessor_position_ms: match seek_qualified.load(Ordering::Acquire) {
                            0 => provider_duration_ms,
                            verified => verified,
                        },
                        successor_offset_frames: 0,
                        sample_rate: rate,
                        seek: crate::playback::model::SeekCapability::unavailable(
                            "seek.unqualified",
                        ),
                    },
                    qualified,
                    channels,
                );
                let target_slot = 1usize.saturating_sub(active_slot);
                let queue = if target_slot == 0 {
                    pcm.clone()
                } else {
                    successor_pcm.clone()
                };
                let decoder_cancel = successor_cancel.clone();
                let worker = crate::playback::output::DecoderWorker::spawn_result(
                    successor_cancel,
                    move || {
                        decode_stream_with_seek_and_gain(
                            reader,
                            Some(&decoder_hint),
                            rate,
                            channels,
                            0,
                            seek_mechanism,
                            false,
                            Some(duration_ms),
                            Some(decoder_qualified),
                            None,
                            gain,
                            qualified_suffix.as_deref(),
                            queue,
                            decoder_cancel,
                        )
                    },
                );
                if target_slot == 0 {
                    slot_a_decoder = Some(worker);
                    slot_a_ready = false;
                } else {
                    successor_decoder = Some(worker);
                    successor_ready = false;
                }
                successor_identity = Some((token, metadata, duration_ms, representation));
                successor_preparation = Some(preparation);
            }
            let slot_a_finished = slot_a_decoder
                .as_ref()
                .is_some_and(|worker| worker.clean_eof());
            let slot_b_finished = successor_decoder
                .as_ref()
                .is_some_and(|worker| worker.clean_eof());
            let active_failed = if active_slot == 0 {
                &slot_a_decoder
            } else {
                &successor_decoder
            }
            .as_ref()
            .is_some_and(|worker| worker.failed());
            if active_failed && (successor_identity.is_none() || !handoff.submitted()) {
                let worker = if active_slot == 0 {
                    slot_a_decoder.take()
                } else {
                    successor_decoder.take()
                };
                return Err(decoder_failure(worker.expect("failed decoder exists")));
            }
            let prepared_slot = 1usize.saturating_sub(active_slot);
            let (prepared_queue, prepared_finished, prepared_ready) = if prepared_slot == 0 {
                (&pcm, slot_a_finished, &mut slot_a_ready)
            } else {
                (&successor_pcm, slot_b_finished, &mut successor_ready)
            };
            let prepared_failed = if prepared_slot == 0 {
                &slot_a_decoder
            } else {
                &successor_decoder
            }
            .as_ref()
            .is_some_and(|worker| worker.failed());
            if successor_identity.is_some()
                && prepared_failed
                && installed_successor_epoch.is_some_and(|epoch| successor_fence.disarm(epoch))
            {
                *prepared_ready = false;
                let worker = if prepared_slot == 0 {
                    slot_a_decoder.take()
                } else {
                    successor_decoder.take()
                };
                if let Some(worker) = worker {
                    let _ = worker.join();
                }
                while prepared_queue.pop().is_some() {}
                successor_identity = None;
                successor_preparation = None;
                installed_successor_epoch = None;
                handoff.clear();
                successor_slot_free.store(true, Ordering::Release);
            }
            if successor_identity.is_some()
                && !*prepared_ready
                && (prepared_queue.len() >= rate as usize * channels as usize / 10
                    || prepared_finished && prepared_queue.len() >= channels as usize)
            {
                if let Some(epoch) = installed_successor_epoch {
                    successor_fence.authorize(epoch);
                }
                *prepared_ready = true;
                if let Some(preparation) = successor_preparation.take() {
                    preparation.ready();
                }
            }
            if successor_identity.is_some()
                && prepared_finished
                && prepared_queue.is_empty()
                && !*prepared_ready
                && installed_successor_epoch.is_some_and(|epoch| successor_fence.disarm(epoch))
            {
                let failed = if prepared_slot == 0 {
                    slot_a_decoder.take()
                } else {
                    successor_decoder.take()
                };
                if let Some(worker) = failed {
                    let _ = worker.join();
                }
                successor_identity = None;
                successor_preparation = None;
                installed_successor_epoch = None;
                handoff.clear();
                successor_slot_free.store(true, Ordering::Release);
            }
            pcm_high_water.fetch_max(pcm.len() as u64, Ordering::AcqRel);
            if !ready {
                preparation
                    .check()
                    .map_err(|e| PlaybackPipelineError::timeout(e.into()))?;
                if pcm.len() >= startup.initial_samples || slot_a_finished {
                    if !session.output_opened(&generation, &preference) {
                        return Err(PlaybackPipelineError::output_policy("GENERATION_CONFLICT"));
                    }
                    preparation.ready();
                    if seek_candidate {
                        let duration = seek_qualified.load(Ordering::Acquire);
                        if !media_seek_requested
                            && start_ms > 0
                            && start_ms
                                == if duration > 0 {
                                    duration
                                } else {
                                    provider_duration_ms
                                }
                        {
                            base_position_ms = 0;
                            position_ms.store(0, Ordering::Release);
                        }
                        session.publish_pipeline_event(
                            expected_serial,
                            generation.clone(),
                            seek_qualification_event(duration),
                            event_epoch.load(Ordering::Acquire),
                        );
                    }
                    if let Some((operation_id, requested_position_ms)) = seek_commit.take() {
                        let actual_frame = seek_landing_frame
                            .as_ref()
                            .map(|value| value.load(Ordering::Acquire))
                            .filter(|value| {
                                *value != crate::playback::decoder::UNKNOWN_SEEK_LANDING_FRAME
                            })
                            .ok_or_else(|| {
                                PlaybackPipelineError::decode(anyhow::anyhow!(
                                    "decoder did not report a seek landing position"
                                ))
                            })?;
                        let actual_position_ms =
                            actual_frame.saturating_mul(1000) / u64::from(rate);
                        if actual_position_ms.abs_diff(requested_position_ms) > 50 {
                            return Err(PlaybackPipelineError::decode(anyhow::anyhow!(
                                "decoder seek landing exceeded 50 ms tolerance"
                            )));
                        }
                        base_position_ms = actual_position_ms;
                        position_ms.store(actual_position_ms, Ordering::Release);
                        session.publish_pipeline_event(
                            expected_serial,
                            generation.clone(),
                            PlaybackEvent::SeekCommitted {
                                operation_id,
                                requested_position_ms,
                                actual_position_ms,
                            },
                            event_epoch.load(Ordering::Acquire),
                        );
                    }
                    ready = true;
                    _output_control.configure_pulse(pause_tx.clone());
                }
            }
            if ready
                && gate.load(Ordering::Acquire)
                && !startup_primer.is_ready(slot_a_finished, pcm.is_empty())
            {
                let len = startup_primer.next_write(output.writable_samples(), scratch.len());
                if len > 0 {
                    let cursor = output.write_cursor_frames();
                    let write_start = cursor
                        .unwrap_or(submission_cursor.next_frame)
                        .max(submission_cursor.next_frame);
                    let rendered = consumer.render(
                        &mut scratch[..len],
                        &pcm,
                        slot_a_ready,
                        slot_a_finished,
                        &successor_pcm,
                        successor_ready,
                        slot_b_finished,
                        true,
                    );
                    if let Some(offset) = rendered.boundary_frame {
                        boundary_pending.store(true, Ordering::Release);
                        boundary_played_frame = Some(write_start.saturating_add(offset as u64));
                        handoff
                            .boundary_frame
                            .store(write_start.saturating_add(offset as u64), Ordering::Release);
                        boundary_audio_frame = submitted_audio_frames.saturating_add(
                            (rendered.rendered.samples - rendered.successor_samples)
                                / u64::from(channels),
                        );
                    }
                    submitted_audio_frames += rendered.rendered.samples / u64::from(channels);
                    active_slot = rendered.active_slot;
                    if !submission_cursor.record(
                        cursor,
                        len,
                        channels,
                        &rendered.rendered,
                        &mut ledger,
                    ) {
                        return Err(PlaybackPipelineError::output_policy("OUTPUT_SWITCH_FAILED"));
                    }
                    output
                        .write(&scratch[..len])
                        .map_err(PlaybackPipelineError::output_policy)?;
                    startup_primer.record_write(rendered.rendered.samples as usize);
                }
            }
            let primed = startup_primer.is_ready(slot_a_finished, pcm.is_empty());
            let enabled = ready && primed && gate.load(Ordering::Acquire);
            let epoch = event_epoch.load(Ordering::Acquire);
            output
                .set_paused(!enabled, &mut report_stall)
                .map_err(PlaybackPipelineError::output_policy)?;
            if ready
                && (!gate.load(Ordering::Acquire) || enabled)
                && let Some(operation_id) = back_commit.take()
            {
                session.publish_pipeline_event(
                    expected_serial,
                    generation.clone(),
                    PlaybackEvent::BackCommitted { operation_id },
                    event_epoch.load(Ordering::Acquire),
                );
                back_pending.store(false, Ordering::Release);
            }
            if enabled {
                let len = output.writable_samples().min(scratch.len());
                if len > 0 {
                    let cursor = output.write_cursor_frames();
                    let write_start = cursor
                        .unwrap_or(submission_cursor.next_frame)
                        .max(submission_cursor.next_frame);
                    let rendered = consumer.render(
                        &mut scratch[..len],
                        &pcm,
                        slot_a_ready,
                        slot_a_finished,
                        &successor_pcm,
                        successor_ready,
                        slot_b_finished,
                        true,
                    );
                    if let Some(offset) = rendered.boundary_frame {
                        boundary_pending.store(true, Ordering::Release);
                        boundary_played_frame = Some(write_start.saturating_add(offset as u64));
                        handoff
                            .boundary_frame
                            .store(write_start.saturating_add(offset as u64), Ordering::Release);
                        boundary_audio_frame = submitted_audio_frames.saturating_add(
                            (rendered.rendered.samples - rendered.successor_samples)
                                / u64::from(channels),
                        );
                    }
                    submitted_audio_frames += rendered.rendered.samples / u64::from(channels);
                    active_slot = rendered.active_slot;
                    if !submission_cursor.record(
                        cursor,
                        len,
                        channels,
                        &rendered.rendered,
                        &mut ledger,
                    ) {
                        return Err(PlaybackPipelineError::output_policy("OUTPUT_SWITCH_FAILED"));
                    }
                    output
                        .write(&scratch[..len])
                        .map_err(PlaybackPipelineError::output_policy)?;
                }
            }
            if let Some(played) = output.played_frames() {
                let frames = ledger.advance(played);
                if boundary_played_frame.is_some_and(|boundary| played >= boundary) {
                    handoff.pulse_presented(frames.saturating_sub(boundary_audio_frame));
                }
                if !handoff_published && let Some(event) = handoff.presented(epoch) {
                    session.publish_pipeline_event(
                        expected_serial,
                        generation.clone(),
                        event,
                        epoch,
                    );
                    handoff_published = true;
                }
                let occurrence_frames = frames.saturating_sub(presentation_base_frames);
                if occurrence_frames > last_frames {
                    sequence += 1;
                    position_ms.store(
                        base_position_ms + occurrence_frames.saturating_mul(1000) / u64::from(rate),
                        Ordering::Release,
                    );
                    let _ = session.report_progress(
                        &generation,
                        &occurrence,
                        sequence,
                        base_position_ms + occurrence_frames.saturating_mul(1000) / u64::from(rate),
                    );
                    last_frames = occurrence_frames;
                }
            }
            if handoff_published
                && let Some((token, _, _, _)) = successor_identity.as_ref()
                && occurrence != token.successor_occurrence_id
                && handoff.adopted.load(Ordering::Acquire)
            {
                occurrence = token.successor_occurrence_id.clone();
                sequence = 0;
                base_position_ms = 0;
                last_frames = 0;
                presentation_base_frames = boundary_audio_frame;
                let retired = if active_slot == 0 {
                    successor_decoder.take()
                } else {
                    slot_a_decoder.take()
                };
                if let Some(worker) = retired {
                    let _ = worker.join();
                }
                if active_slot == 0 {
                    successor_ready = false;
                    while successor_pcm.pop().is_some() {}
                } else {
                    slot_a_ready = false;
                    while pcm.pop().is_some() {}
                }
                successor_identity = None;
                successor_preparation = None;
                boundary_played_frame = None;
                handoff_published = false;
                boundary_pending.store(false, Ordering::Release);
                handoff.clear();
                installed_successor_epoch = None;
                successor_fence.adopted();
                successor_slot_free.store(true, Ordering::Release);
            }
            if let Some(active) = activity.observe(
                enabled,
                last_frames,
                activity_clock.elapsed().as_millis() as u64,
            ) {
                session.publish_pipeline_event(
                    expected_serial,
                    generation.clone(),
                    if active {
                        PlaybackEvent::Active
                    } else {
                        PlaybackEvent::Buffering
                    },
                    epoch,
                );
            }
            let active_done = if active_slot == 0 {
                slot_a_decoder
                    .as_ref()
                    .is_some_and(|worker| worker.is_finished() && pcm.is_empty())
            } else {
                successor_decoder
                    .as_ref()
                    .is_some_and(|worker| worker.is_finished() && successor_pcm.is_empty())
            };
            if enabled && successor_identity.is_none() && active_done {
                output
                    .drain(&mut report_stall)
                    .map_err(PlaybackPipelineError::output_policy)?;
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        Ok(())
    })();
    gate.store(false, Ordering::Release);
    let retirement = output
        .retire(&mut report_stall)
        .map_err(PlaybackPipelineError::output_policy);
    if result.is_err()
        || retirement.is_err()
        || cancel.load(Ordering::Acquire)
        || generation_serial.load(Ordering::Acquire) != expected_serial
    {
        cancel.store(true, Ordering::Release);
        result?;
        retirement?;
        return Ok(());
    }
    let worker = if active_slot == 0 {
        slot_a_decoder.take()
    } else {
        successor_decoder.take()
    }
    .expect("active decoder exists until completion");
    let decoded = worker
        .join()
        .map_err(|_| PlaybackPipelineError::decode(anyhow::anyhow!("decoder worker panicked")))?
        .map_err(|e| {
            PlaybackPipelineError::from_decode_error_and_stream_state(e, &stream_failure)
        })?;
    let decoded_ms = decoded.emitted_frames.saturating_mul(1000) / u64::from(rate);
    if decoded.emitted_frames == 0 {
        return Err(PlaybackPipelineError::decode(anyhow::anyhow!(
            "decoder produced no audio"
        )));
    }
    session.publish_pipeline_event(
        expected_serial,
        generation,
        PlaybackEvent::Completed {
            position_ms: base_position_ms.saturating_add(decoded_ms),
        },
        event_epoch.load(Ordering::Acquire),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_plan_primes_the_server_and_retains_a_local_reserve() {
        let plan = startup_plan(48_000, 2, 38_400);
        assert_eq!(plan.server_samples, 9_600);
        assert_eq!(plan.local_reserve_samples, 19_200);
        assert_eq!(plan.initial_samples, 28_800);
    }

    #[test]
    fn startup_plan_keeps_pcm_frame_aligned() {
        let plan = startup_plan(48_000, 2, 38_401);
        assert_eq!(plan.server_samples % 2, 0);
        assert_eq!(
            plan.initial_samples - plan.server_samples,
            plan.local_reserve_samples
        );
    }

    #[test]
    fn startup_primer_waits_for_its_full_target_across_partial_writes() {
        let mut primer = StartupPrimer::new(9_600);
        assert_eq!(primer.next_write(4_800, 960), 960);
        primer.record_write(960);
        assert!(!primer.is_ready(false, false));
        primer.record_write(8_640);
        assert!(primer.is_ready(false, false));
    }

    #[test]
    fn startup_primer_allows_a_finished_short_track_to_start() {
        let mut primer = StartupPrimer::new(9_600);
        primer.record_write(480);
        assert!(!primer.is_ready(true, false));
        assert!(primer.is_ready(true, true));
    }

    #[test]
    fn submission_cursor_accounts_for_pcm_without_timing_metadata() {
        let mut ledger = PresentationLedger::new();
        let mut cursor = SubmissionCursor::default();
        let rendered = crate::playback::output::Rendered {
            samples: 960,
            last_audio_frame: Some(480),
        };
        assert!(cursor.record(None, 960, 2, &rendered, &mut ledger));
        assert!(cursor.record(Some(480), 960, 2, &rendered, &mut ledger));
        assert_eq!(ledger.advance(960), 960);
    }
}
