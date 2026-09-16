use super::*;
use crate::playback::{
    devices::pulse_stream::PinnedStream,
    output::{PcmConsumer, PresentationActivity, PresentationLedger},
};
use std::time::Duration;

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
    preparation: crate::playback::http_source::Preparation,
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
    *endpoint.lock().unwrap_or_else(|e| e.into_inner()) = Some(preference.display_name.clone());
    let capacity = (rate as usize * channels as usize * PCM_TARGET_MILLISECONDS / 1000)
        .min(PCM_CAPACITY_MAX_BYTES / 4);
    let pcm = Arc::new(ArrayQueue::new(capacity));
    let decoder_pcm = pcm.clone();
    let decoder_cancel = cancel.clone();
    let hint = hint.to_owned();
    let stream_failure = reader.failure_state();
    let decoder = crate::playback::output::DecoderWorker::spawn(cancel.clone(), move || {
        decode_stream(
            reader,
            Some(&hint),
            rate,
            channels,
            start_ms.saturating_mul(u64::from(rate)) / 1000,
            decoder_pcm,
            decoder_cancel,
        )
    });
    let mut consumer = PcmConsumer::new(channels as usize, rate as usize * channels as usize / 10);
    let mut ledger = PresentationLedger::new();
    let mut scratch = vec![0f32; rate as usize * channels as usize / 100];
    let occurrence = session
        .snapshot()
        .ok()
        .and_then(|s| s.current)
        .map(|o| o.occurrence_id)
        .unwrap_or_default();
    let mut sequence = 0;
    let mut last_frames = 0;
    let mut last_tick = std::time::SystemTime::now();
    let mut ready = false;
    let mut activity = PresentationActivity::default();
    let activity_clock = std::time::Instant::now();
    let mut report_stall = || {
        session.publish_event_at_epoch(
            generation.clone(),
            PlaybackEvent::Failed {
                code: "OUTPUT_RETIREMENT_PENDING".into(),
                retryable: true,
            },
            event_epoch.load(Ordering::Acquire),
        )
    };
    let result = (|| {
        loop {
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
            pcm_high_water.fetch_max(pcm.len() as u64, Ordering::AcqRel);
            if !ready {
                preparation
                    .check()
                    .map_err(|e| PlaybackPipelineError::timeout(e.into()))?;
                if pcm.len() >= rate as usize * channels as usize / 10 || decoder.is_finished() {
                    if !session.output_opened(&generation, &preference) {
                        return Err(PlaybackPipelineError::output_policy("GENERATION_CONFLICT"));
                    }
                    preparation.ready();
                    ready = true;
                }
            }
            let enabled = ready && gate.load(Ordering::Acquire);
            let epoch = event_epoch.load(Ordering::Acquire);
            output
                .set_paused(!enabled, &mut report_stall)
                .map_err(PlaybackPipelineError::output_policy)?;
            if enabled {
                let len = output.writable_samples().min(scratch.len());
                if len > 0 {
                    if let Some(cursor) = output.write_cursor_frames() {
                        let rendered =
                            consumer.render(&mut scratch[..len], &pcm, true, decoder.is_finished());
                        if let Some(end) = rendered.last_audio_frame {
                            let frames = rendered.samples / u64::from(channels);
                            if !ledger.record(cursor + end as u64 - frames, frames) {
                                return Err(PlaybackPipelineError::output_policy(
                                    "OUTPUT_SWITCH_FAILED",
                                ));
                            }
                        }
                        output
                            .write(&scratch[..len])
                            .map_err(PlaybackPipelineError::output_policy)?;
                    }
                }
            }
            if let Some(played) = output.played_frames() {
                let frames = ledger.advance(played);
                if frames > last_frames {
                    sequence += 1;
                    position_ms.store(
                        start_ms + frames.saturating_mul(1000) / u64::from(rate),
                        Ordering::Release,
                    );
                    let _ = session.report_progress(
                        &generation,
                        &occurrence,
                        sequence,
                        start_ms + frames.saturating_mul(1000) / u64::from(rate),
                    );
                    last_frames = frames;
                }
            }
            if let Some(active) = activity.observe(
                enabled,
                last_frames,
                activity_clock.elapsed().as_millis() as u64,
            ) {
                session.publish_event_at_epoch(
                    generation.clone(),
                    if active {
                        PlaybackEvent::Active
                    } else {
                        PlaybackEvent::Buffering
                    },
                    epoch,
                );
            }
            if enabled && decoder.is_finished() && pcm.is_empty() {
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
    let decoded = decoder
        .join()
        .map_err(|_| PlaybackPipelineError::decode(anyhow::anyhow!("decoder worker panicked")))?
        .map_err(|e| {
            PlaybackPipelineError::from_decode_error_and_stream_state(e, &stream_failure)
        })?;
    session.publish_event_at_epoch(
        generation,
        PlaybackEvent::Completed {
            position_ms: decoded.frames.saturating_mul(1000) / u64::from(rate),
        },
        event_epoch.load(Ordering::Acquire),
    );
    Ok(())
}
