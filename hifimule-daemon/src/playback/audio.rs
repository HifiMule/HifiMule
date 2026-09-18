#[cfg(target_os = "linux")]
mod pulse_output;
use super::decoder::{UNKNOWN_SEEK_LANDING_FRAME, decode_stream_with_seek};
use super::model::{PlaybackEvent, PlaybackTrackMetadata};
use super::streaming::{BoundedHttpReader, StreamFailureKind, StreamFailureState, StreamReadError};
use crate::providers::{PlaybackDescription, PlaybackRequest, select_playback_representation};
use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{FromSample, Sample, SizedSample};
use crossbeam_queue::ArrayQueue;
#[cfg(target_os = "linux")]
use pulse_output::run_output;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

const PCM_TARGET_MILLISECONDS: usize = 500;
const PCM_CAPACITY_MAX_BYTES: usize = 1024 * 1024;
const STARTUP_FILL_MILLISECONDS: usize = 100;
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlaybackPipelineStage {
    Cancelled,
    Provider,
    Timeout,
    Decode,
    OutputOpen,
    OutputLost,
}

impl PlaybackPipelineStage {
    fn as_str(self) -> &'static str {
        match self {
            Self::Cancelled => "cancelled",
            Self::Provider => "provider",
            Self::Timeout => "timeout",
            Self::Decode => "decode",
            Self::OutputOpen => "output-open",
            Self::OutputLost => "output-lost",
        }
    }
}

#[derive(Debug)]
pub(crate) struct PlaybackPipelineError {
    stage: PlaybackPipelineStage,
    code: &'static str,
    retryable: bool,
    representation: String,
    source: anyhow::Error,
}

impl PlaybackPipelineError {
    fn output_policy(code: &'static str) -> Self {
        let mut error = Self::output_open(anyhow::anyhow!("selected output is unavailable"));
        error.code = code;
        if code == "GENERATION_CONFLICT" {
            error.stage = PlaybackPipelineStage::Cancelled;
        }
        error
    }
    pub(crate) fn from_provider_error(error: crate::providers::ProviderError) -> Self {
        use crate::providers::ProviderError;
        match error {
            ProviderError::UnsupportedCapability(_) => {
                Self::unsupported(anyhow::anyhow!("provider playback is unsupported"))
            }
            ProviderError::Http { status, .. } => {
                Self::source(anyhow::anyhow!("provider HTTP failure status={status:?}"))
            }
            ProviderError::Auth(_) => {
                Self::source(anyhow::anyhow!("provider authentication failed"))
            }
            ProviderError::NotFound { .. } => {
                Self::source(anyhow::anyhow!("provider playback item was not found"))
            }
            ProviderError::Deserialization(_) => Self::source(anyhow::anyhow!(
                "provider playback response deserialization failed"
            )),
            ProviderError::Other(_) => {
                Self::source(anyhow::anyhow!("provider playback operation failed"))
            }
        }
    }

    #[cfg(test)]
    fn from_decode_error(source: anyhow::Error) -> Self {
        let stream_failure = source.chain().find_map(|cause| {
            cause
                .downcast_ref::<super::streaming::StreamReadError>()
                .or_else(|| {
                    cause
                        .downcast_ref::<std::io::Error>()
                        .and_then(std::io::Error::get_ref)
                        .and_then(|inner| inner.downcast_ref::<super::streaming::StreamReadError>())
                })
                .map(|error| match error {
                    super::streaming::StreamReadError::Timeout => StreamFailureKind::Timeout,
                    super::streaming::StreamReadError::Unsupported => {
                        StreamFailureKind::Unsupported
                    }
                    super::streaming::StreamReadError::Source(_) => StreamFailureKind::Source,
                })
        });
        Self::from_decode_error_and_stream_failure(source, stream_failure)
    }

    fn from_decode_error_and_stream_failure(
        source: anyhow::Error,
        stream_failure: Option<StreamFailureKind>,
    ) -> Self {
        match stream_failure {
            Some(StreamFailureKind::Timeout) => Self::timeout(source),
            Some(StreamFailureKind::Unsupported) => Self::unsupported(source),
            Some(StreamFailureKind::Source) => Self::source(source),
            None => Self::decode(source),
        }
    }

    fn from_decode_error_and_stream_state(
        source: anyhow::Error,
        stream_failure: &StreamFailureState,
    ) -> Self {
        let Some(error) = stream_failure.error() else {
            return Self::decode(source);
        };
        let kind = match &error {
            StreamReadError::Timeout => StreamFailureKind::Timeout,
            StreamReadError::Unsupported => StreamFailureKind::Unsupported,
            StreamReadError::Source(_) => StreamFailureKind::Source,
        };
        let source = anyhow::Error::new(error).context(format!("decoder failed: {source:#}"));
        Self::from_decode_error_and_stream_failure(source, Some(kind))
    }

    fn cancelled(source: anyhow::Error) -> Self {
        Self {
            stage: PlaybackPipelineStage::Cancelled,
            code: "DECODE_FAILED",
            retryable: false,
            representation: "unknown".into(),
            source,
        }
    }

    fn source(source: anyhow::Error) -> Self {
        Self {
            stage: PlaybackPipelineStage::Provider,
            code: "SOURCE_UNAVAILABLE",
            retryable: true,
            representation: "unknown".into(),
            source,
        }
    }

    fn unsupported(source: anyhow::Error) -> Self {
        Self {
            stage: PlaybackPipelineStage::Provider,
            code: "PLAYBACK_UNSUPPORTED",
            retryable: false,
            representation: "unknown".into(),
            source,
        }
    }

    fn timeout(source: anyhow::Error) -> Self {
        Self {
            stage: PlaybackPipelineStage::Timeout,
            code: "PLAYBACK_TIMEOUT",
            retryable: true,
            representation: "unknown".into(),
            source,
        }
    }

    pub(crate) fn seek_timeout() -> Self {
        Self::timeout(anyhow::anyhow!("seek preparation timed out"))
    }

    fn decode(source: anyhow::Error) -> Self {
        Self {
            stage: PlaybackPipelineStage::Decode,
            code: "DECODE_FAILED",
            retryable: true,
            representation: "unknown".into(),
            source,
        }
    }

    fn output_open(source: anyhow::Error) -> Self {
        Self {
            stage: PlaybackPipelineStage::OutputOpen,
            code: "OUTPUT_UNAVAILABLE",
            retryable: true,
            representation: "unknown".into(),
            source,
        }
    }

    fn output_lost(source: anyhow::Error) -> Self {
        Self {
            stage: PlaybackPipelineStage::OutputLost,
            code: "OUTPUT_LOST",
            retryable: true,
            representation: "unknown".into(),
            source,
        }
    }

    fn stage(&self) -> &'static str {
        self.stage.as_str()
    }

    pub(crate) fn code(&self) -> &'static str {
        self.code
    }

    pub(crate) fn retryable(&self) -> bool {
        self.retryable
    }

    pub(crate) fn is_publishable(&self) -> bool {
        self.stage != PlaybackPipelineStage::Cancelled
    }

    fn with_representation(mut self, representation: &str) -> Self {
        self.representation = diagnostic_representation(representation);
        self
    }

    fn diagnostic_chain(&self) -> String {
        self.source
            .chain()
            .map(|cause| crate::providers::sanitize_secret_message(&cause.to_string()))
            .collect::<Vec<_>>()
            .join(": ")
    }
}

impl std::fmt::Display for PlaybackPipelineError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.source.fmt(formatter)
    }
}

impl std::error::Error for PlaybackPipelineError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}

pub(crate) fn log_pipeline_failure(
    session: &super::PlaybackSession,
    generation: &str,
    error: &PlaybackPipelineError,
) -> bool {
    if !error.is_publishable() {
        return false;
    }
    session
        .with_current_generation(generation, || {
            crate::daemon_log!(
                "[Playback] failure stage={} code={} representation={} chain={}",
                error.stage(),
                error.code(),
                error.representation,
                error.diagnostic_chain()
            );
        })
        .is_some()
}

pub(crate) fn publish_pipeline_failure(
    session: &super::PlaybackSession,
    generation: String,
    error: PlaybackPipelineError,
) {
    if !log_pipeline_failure(session, &generation, &error) {
        return;
    }
    session.publish_event(
        generation,
        PlaybackEvent::Failed {
            code: error.code().into(),
            retryable: error.retryable(),
        },
    );
}

fn should_publish_worker_failure(error: &PlaybackPipelineError, _worker_cancelled: bool) -> bool {
    error.is_publishable()
}

struct Pipeline {
    cancel: Arc<AtomicBool>,
    gate: Arc<AtomicBool>,
    alive: Arc<AtomicBool>,
    generation: String,
    event_epoch: Arc<AtomicU64>,
    worker: std::thread::JoinHandle<()>,
    compressed_high_water: Arc<AtomicU64>,
    pcm_high_water: Arc<AtomicU64>,
    endpoint: Arc<Mutex<Option<String>>>,
    position_ms: Arc<AtomicU64>,
}

fn decoder_hint(representation: &crate::providers::PlaybackRepresentation) -> String {
    representation
        .container
        .as_deref()
        .and_then(normalize_container)
        .map(|value| format!("stream.{value}"))
        .unwrap_or_else(|| representation.request.url.path().to_owned())
}

fn normalize_container(value: &str) -> Option<String> {
    let value = value
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .trim()
        .trim_start_matches('.');
    (!value.is_empty()).then(|| value.to_ascii_lowercase())
}

fn representation_name(representation: &crate::providers::PlaybackRepresentation) -> String {
    representation
        .container
        .as_deref()
        .and_then(normalize_container)
        .or_else(|| {
            representation
                .codec
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_ascii_lowercase)
        })
        .unwrap_or_else(|| "unknown".into())
}

fn diagnostic_representation(value: &str) -> String {
    let value = normalize_container(value).unwrap_or_default();
    if matches!(
        value.as_str(),
        "wav"
            | "wave"
            | "flac"
            | "mp3"
            | "mpeg"
            | "m4a"
            | "mp4"
            | "aac"
            | "alac"
            | "opus"
            | "ogg"
            | "oga"
            | "aif"
            | "aiff"
            | "vorbis"
            | "wma"
            | "asf"
            | "wmav1"
            | "wmav2"
            | "wmapro"
            | "wmalossless"
            | "pcm_s16be"
            | "pcm_s24be"
            | "pcm_s32be"
            | "pcm_s16le"
            | "pcm_s24le"
            | "pcm_s32le"
    ) {
        value
    } else {
        "unknown".into()
    }
}

pub struct AudioEngine {
    current: Mutex<Option<Pipeline>>,
    starts: tokio::sync::Mutex<()>,
}
static ENGINE: OnceLock<AudioEngine> = OnceLock::new();
pub fn global() -> &'static AudioEngine {
    ENGINE.get_or_init(|| AudioEngine {
        current: Mutex::new(None),
        starts: tokio::sync::Mutex::new(()),
    })
}

impl AudioEngine {
    async fn retire(
        old: Pipeline,
        session: &super::PlaybackSession,
        generation: &str,
        expected_epoch: u64,
    ) -> Result<(), PlaybackPipelineError> {
        old.gate.store(false, Ordering::Release);
        old.cancel.store(true, Ordering::Release);
        let mut retirement = tokio::task::spawn_blocking(move || old.worker.join());
        let result = tokio::select! {
            result = &mut retirement => result,
            _ = tokio::time::sleep(std::time::Duration::from_millis(250)) => {
                session.publish_event_at_epoch(generation.to_owned(), PlaybackEvent::Failed {
                    code: "OUTPUT_RETIREMENT_PENDING".into(), retryable: true,
                }, expected_epoch);
                // Keep ownership and the serialized-start guard. A timeout is
                // not evidence that the old native stream has stopped.
                retirement.await
            }
        };
        result
            .map_err(|_| PlaybackPipelineError::output_policy("OUTPUT_SWITCH_FAILED"))?
            .map_err(|_| PlaybackPipelineError::output_policy("OUTPUT_SWITCH_FAILED"))
    }

    pub fn captured_position(&self, generation: &str) -> Option<u64> {
        self.current
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .filter(|p| p.generation == generation)
            .map(|p| p.position_ms.load(Ordering::Acquire))
    }
    pub async fn validate_selected_output(
        &self,
        session: super::PlaybackSession,
        generation: String,
    ) -> Result<(), PlaybackPipelineError> {
        let _guard = self.starts.lock().await;
        // An idle-selection effect may have waited behind a newer PlayTrack.
        // Reject it before taking ownership of that generation's pipeline.
        session
            .selected_output(&generation)
            .map_err(PlaybackPipelineError::output_policy)?;
        let old = self
            .current
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(old) = old {
            Self::retire(old, &session, &generation, session.control_epoch()).await?;
        }
        let preference = session
            .selected_output(&generation)
            .map_err(PlaybackPipelineError::output_policy)?;
        tokio::task::spawn_blocking(move || {
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            {
                let device = super::devices::open_device(&preference)
                    .map_err(PlaybackPipelineError::output_policy)?;
                let config = device
                    .default_output_config()
                    .map_err(|_| PlaybackPipelineError::output_policy("OUTPUT_UNAVAILABLE"))?;
                let _stream = device
                    .build_output_stream_raw(
                        config.config(),
                        config.sample_format(),
                        |data, _| {
                            // This stream is never started. No source or queue is created.
                            data.bytes_mut().fill(0);
                        },
                        |_| {},
                        None,
                    )
                    .map_err(|_| PlaybackPipelineError::output_policy("OUTPUT_SWITCH_FAILED"))?;
                if !session.output_opened(&generation, &preference) {
                    return Err(PlaybackPipelineError::output_policy("GENERATION_CONFLICT"));
                }
                drop(_stream);
                session.output_closed(&generation);
                Ok(())
            }
            #[cfg(target_os = "linux")]
            {
                let mut stream = super::devices::pulse_stream::PinnedStream::open(
                    &preference,
                    std::time::Instant::now() + std::time::Duration::from_secs(5),
                )
                .map_err(PlaybackPipelineError::output_policy)?;
                if !session.output_opened(&generation, &preference) {
                    return Err(PlaybackPipelineError::output_policy("GENERATION_CONFLICT"));
                }
                stream
                    .retire(&mut || {})
                    .map_err(PlaybackPipelineError::output_policy)?;
                session.output_closed(&generation);
                Ok(())
            }
        })
        .await
        .map_err(|_| PlaybackPipelineError::output_policy("OUTPUT_SWITCH_FAILED"))?
    }

    pub fn control(&self, action: super::model::ControlAction) {
        if let Some(pipeline) = self
            .current
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            match action {
                super::model::ControlAction::Pause => pipeline.gate.store(false, Ordering::Release),
                super::model::ControlAction::Resume => pipeline.gate.store(true, Ordering::Release),
                super::model::ControlAction::Stop => {
                    pipeline.gate.store(false, Ordering::Release);
                    pipeline.cancel.store(true, Ordering::Release);
                }
            }
        }
    }

    pub fn resume_existing(
        &self,
        generation: &str,
        session: &super::PlaybackSession,
        expected_epoch: u64,
    ) -> bool {
        session.with_current_generation(generation, || {
            if session.control_epoch() != expected_epoch {
                return false;
            }
            let current = self.current.lock().unwrap_or_else(|e| e.into_inner());
            current.as_ref().is_some_and(|pipeline| {
                let reusable = pipeline.generation == generation
                    && pipeline.alive.load(Ordering::Acquire)
                    && !pipeline.cancel.load(Ordering::Acquire);
                if reusable {
                    // Only a newly admitted Resume may authorize an installed
                    // pipeline to report preparation failures in a newer epoch.
                    pipeline
                        .event_epoch
                        .store(expected_epoch, Ordering::Release);
                }
                reusable
            })
        }) == Some(true)
    }

    pub fn stop_and_join(&self) -> anyhow::Result<()> {
        // Shutdown runs this on a blocking worker and must also retire a start
        // that was admitted before the session fence but is still fetching.
        let _start_guard = self.starts.blocking_lock();
        let pipeline = {
            self.current
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take()
        };
        if let Some(pipeline) = pipeline {
            pipeline.gate.store(false, Ordering::Release);
            pipeline.cancel.store(true, Ordering::Release);
            pipeline
                .worker
                .join()
                .map_err(|_| anyhow::anyhow!("audio worker panicked"))?;
        }
        Ok(())
    }

    pub async fn start(
        &self,
        description: PlaybackDescription,
        source: super::model::TrackSource,
        start_ms: u64,
        generation: String,
        session: super::PlaybackSession,
        deadline: std::time::Instant,
    ) -> Result<(), PlaybackPipelineError> {
        let epoch = session.control_epoch();
        self.start_at_epoch(
            description,
            source,
            start_ms,
            generation,
            session,
            deadline,
            epoch,
        )
        .await
    }

    /// Start only work belonging to the transport command that admitted it.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn start_at_epoch(
        &self,
        description: PlaybackDescription,
        source: super::model::TrackSource,
        start_ms: u64,
        generation: String,
        session: super::PlaybackSession,
        deadline: std::time::Instant,
        expected_epoch: u64,
    ) -> Result<(), PlaybackPipelineError> {
        self.start_at_epoch_kind(
            description,
            source,
            start_ms,
            generation,
            session,
            deadline,
            expected_epoch,
            None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn start_seek_at_epoch(
        &self,
        description: PlaybackDescription,
        source: super::model::TrackSource,
        start_ms: u64,
        operation_id: String,
        generation: String,
        session: super::PlaybackSession,
        deadline: std::time::Instant,
        expected_epoch: u64,
    ) -> Result<(), PlaybackPipelineError> {
        self.start_at_epoch_kind(
            description,
            source,
            start_ms,
            generation,
            session,
            deadline,
            expected_epoch,
            Some(operation_id),
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn start_at_epoch_kind(
        &self,
        description: PlaybackDescription,
        source: super::model::TrackSource,
        start_ms: u64,
        generation: String,
        session: super::PlaybackSession,
        deadline: std::time::Instant,
        expected_epoch: u64,
        seek_operation_id: Option<String>,
    ) -> Result<(), PlaybackPipelineError> {
        require_preparation_epoch(&session, expected_epoch)?;
        let duration_ms = u64::from(description.song.duration_seconds).saturating_mul(1000);
        // Persisted sessions intentionally omit transient Completed metadata.
        // Once the same duration is resolved, an explicit Resume from its
        // terminal cursor restarts from zero just like a live Completed resume.
        let pcm_wav_candidate = description.representations.iter().any(|representation| {
            representation.seek_mechanism
                == Some(crate::providers::PlaybackSeekMechanism::JellyfinOriginalPcmWav)
        });
        let start_ms =
            if !pcm_wav_candidate && seek_operation_id.is_none() && start_ms == duration_ms {
                0
            } else {
                start_ms
            };
        let _start_guard = self.starts.lock().await;
        require_preparation_epoch(&session, expected_epoch)?;
        session
            .selected_output(&generation)
            .map_err(PlaybackPipelineError::output_policy)?;
        // A Resume admitted while the first start was preparing shares that
        // generation. Once preparation finishes, reuse it instead of restarting.
        if self.resume_existing(&generation, &session, expected_epoch) {
            return Ok(());
        }
        let (generation_serial, expected_serial) =
            session.generation_guard(&generation).ok_or_else(|| {
                PlaybackPipelineError::cancelled(anyhow::anyhow!(
                    "playback generation was superseded"
                ))
            })?;
        verify_runtime().map_err(PlaybackPipelineError::decode)?;
        let old = {
            self.current
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take()
        };
        if let Some(old) = old {
            Self::retire(old, &session, &generation, expected_epoch).await?;
        }
        require_preparation_epoch(&session, expected_epoch)?;
        let representation = select_playback_representation(description.representations)
            .map_err(PlaybackPipelineError::from_provider_error)?;
        let seek_candidate = representation.seek_mechanism
            == Some(crate::providers::PlaybackSeekMechanism::JellyfinOriginalPcmWav)
            && representation.request.range_supported
            && duration_ms > 0;
        if seek_operation_id.is_some() && !seek_candidate {
            return Err(PlaybackPipelineError::unsupported(anyhow::anyhow!(
                "representation is not qualified for media-time seeking"
            )));
        }
        let representation_name = representation_name(&representation);
        let decoder_hint = decoder_hint(&representation);
        let request = representation.request;
        let response = tokio::select! {
            result = tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), fetch(&request)) => result,
            _ = async {
                while generation_serial.load(Ordering::Acquire) == expected_serial
                    && session.control_epoch() == expected_epoch
                {
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                }
            } => return Err(PlaybackPipelineError::cancelled(anyhow::anyhow!("playback generation was superseded"))),
        }
            .map_err(|_| PlaybackPipelineError::timeout(anyhow::anyhow!("playback preparation timed out")))?
            .map_err(|error| error.with_representation(&representation_name))?;
        if generation_serial.load(Ordering::Acquire) != expected_serial
            || session.control_epoch() != expected_epoch
            || session.generation_guard(&generation).is_none()
        {
            return Err(PlaybackPipelineError::cancelled(anyhow::anyhow!(
                "playback generation was superseded"
            )));
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let gate = session.output_gate();
        let compressed_high_water = Arc::new(AtomicU64::new(0));
        let pcm_high_water = Arc::new(AtomicU64::new(0));
        let endpoint = Arc::new(Mutex::new(None));
        let position_ms = Arc::new(AtomicU64::new(start_ms));
        let worker_position = position_ms.clone();
        let preparation = super::http_source::Preparation::new(deadline, cancel.clone());
        let source_reader =
            super::http_source::HttpSource::new(request, response, preparation.clone());
        let reader = BoundedHttpReader::from_source(
            source_reader,
            cancel.clone(),
            compressed_high_water.clone(),
        );
        let metadata = PlaybackTrackMetadata {
            source,
            title: description.song.title.clone(),
            artist: description.song.artist_name.clone(),
            album: description.song.album_title.clone(),
        };
        session.publish_event_at_epoch(
            generation.clone(),
            PlaybackEvent::Resolved {
                metadata,
                duration_ms: Some(duration_ms),
                representation: representation_name.clone(),
                seek: super::model::SeekCapability::unavailable(if seek_candidate {
                    "seek.validating"
                } else {
                    "seek.representation_unqualified"
                }),
            },
            expected_epoch,
        );
        let worker_cancel = cancel.clone();
        let worker_gate = gate.clone();
        let alive = Arc::new(AtomicBool::new(true));
        let worker_alive = alive.clone();
        let pipeline_generation = generation.clone();
        let worker_pcm_high_water = pcm_high_water.clone();
        let worker_endpoint = endpoint.clone();
        let worker_representation = representation_name.clone();
        let event_epoch = Arc::new(AtomicU64::new(expected_epoch));
        let pipeline_event_epoch = event_epoch.clone();
        let install_session = session.clone();
        let install_generation = generation.clone();
        let worker_seek = seek_operation_id
            .clone()
            .map(|operation_id| (operation_id, start_ms));
        // A worker cannot open native output until installation has passed the
        // owner's generation and command-epoch fence under the owner lock.
        let (installed_tx, installed_rx) = std::sync::mpsc::sync_channel::<()>(1);
        let worker = std::thread::Builder::new()
            .name("hifimule-audio".into())
            .spawn(move || {
                if installed_rx.recv().is_err() {
                    worker_alive.store(false, Ordering::Release);
                    return;
                }
                let worker_seek_failure = worker_seek.clone();
                let result = run_output(
                    reader,
                    &decoder_hint,
                    generation.clone(),
                    session.clone(),
                    worker_cancel.clone(),
                    worker_gate,
                    generation_serial.clone(),
                    expected_serial,
                    event_epoch.clone(),
                    start_ms,
                    worker_pcm_high_water,
                    worker_endpoint,
                    worker_position,
                    preparation,
                    seek_candidate,
                    duration_ms,
                    worker_seek,
                )
                .map_err(|error| error.with_representation(&worker_representation));
                session.output_closed(&generation);
                match result {
                    Err(error)
                        if should_publish_worker_failure(
                            &error,
                            worker_cancel.load(Ordering::Acquire),
                        ) && log_pipeline_failure(&session, &generation, &error) =>
                    {
                        let event = worker_seek_failure.map_or_else(
                            || PlaybackEvent::Failed {
                                code: error.code().into(),
                                retryable: error.retryable(),
                            },
                            |(operation_id, _)| PlaybackEvent::SeekPipelineFailed {
                                operation_id,
                                code: error.code().into(),
                                retryable: error.retryable(),
                            },
                        );
                        session.publish_event_at_epoch(
                            generation,
                            event,
                            event_epoch.load(Ordering::Acquire),
                        );
                    }
                    _ => {}
                }
                worker_alive.store(false, Ordering::Release);
            })
            .map_err(|error| {
                PlaybackPipelineError::output_open(
                    anyhow::Error::new(error).context("spawn audio worker"),
                )
                .with_representation(&representation_name)
            })?;
        let mut pipeline = Some(Pipeline {
            cancel,
            gate,
            alive,
            generation: pipeline_generation,
            event_epoch: pipeline_event_epoch,
            position_ms,
            worker,
            compressed_high_water,
            pcm_high_water,
            endpoint,
        });
        let installed = install_session.with_current_generation(&install_generation, || {
            if install_session.control_epoch() != expected_epoch {
                return false;
            }
            *self.current.lock().unwrap_or_else(|e| e.into_inner()) = pipeline.take();
            let _ = installed_tx.send(());
            true
        }) == Some(true);
        if !installed {
            drop(installed_tx);
            if let Some(pipeline) = pipeline {
                pipeline.cancel.store(true, Ordering::Release);
                // The worker has not opened output or entered decoder work.
                let _ = pipeline.worker.join();
            }
            return Err(PlaybackPipelineError::cancelled(anyhow::anyhow!(
                "playback preparation was superseded"
            )));
        }
        Ok(())
    }
}

// Runtime qualification enables only the implemented Jellyfin original PCM-WAV
// path after FFmpeg has verified its format and reconciled media duration. The
// installed matrix remains a separate release-evidence concern.
fn seek_qualification_event(duration_ms: u64) -> PlaybackEvent {
    PlaybackEvent::SeekQualified {
        capability: if duration_ms > 0 {
            super::model::SeekCapability::jellyfin_pcm_wav()
        } else {
            super::model::SeekCapability::unavailable("seek.duration_unavailable")
        },
        duration_ms: (duration_ms > 0).then_some(duration_ms),
    }
}

fn require_preparation_epoch(
    session: &super::PlaybackSession,
    expected_epoch: u64,
) -> Result<(), PlaybackPipelineError> {
    if session.control_epoch() != expected_epoch || session.is_fenced() {
        return Err(PlaybackPipelineError::cancelled(anyhow::anyhow!(
            "playback preparation was superseded"
        )));
    }
    Ok(())
}

async fn fetch(request: &PlaybackRequest) -> Result<reqwest::Response, PlaybackPipelineError> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|error| {
            PlaybackPipelineError::source(
                anyhow::Error::new(error).context("build playback HTTP client"),
            )
        })?;
    let response = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        client
            .get(request.url.clone())
            .headers(request.headers.clone())
            .send(),
    )
    .await
    .map_err(|_| PlaybackPipelineError::timeout(anyhow::anyhow!("playback preparation timeout")))?
    .map_err(|error| {
        PlaybackPipelineError::source(anyhow::Error::new(error).context("request playback source"))
    })?;
    if !response.status().is_success() {
        return Err(PlaybackPipelineError::source(anyhow::anyhow!(
            "source unavailable ({})",
            response.status()
        )));
    }
    if response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(content_type_is_non_audio)
    {
        return Err(PlaybackPipelineError::source(anyhow::anyhow!(
            "provider returned a non-audio response"
        )));
    }
    Ok(response)
}

fn content_type_is_non_audio(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    value.contains("json") || value.contains("xml")
}

#[cfg(not(target_os = "linux"))]
#[allow(clippy::too_many_arguments)]
fn run_output(
    reader: BoundedHttpReader,
    hint: &str,
    generation: String,
    session: super::PlaybackSession,
    cancel: Arc<AtomicBool>,
    gate: Arc<AtomicBool>,
    generation_serial: Arc<AtomicU64>,
    expected_serial: u64,
    event_epoch: Arc<AtomicU64>,
    start_ms: u64,
    pcm_high_water: Arc<AtomicU64>,
    endpoint: Arc<Mutex<Option<String>>>,
    position_ms: Arc<AtomicU64>,
    preparation: super::http_source::Preparation,
    seek_candidate: bool,
    provider_duration_ms: u64,
    seek_commit: Option<(String, u64)>,
) -> Result<(), PlaybackPipelineError> {
    let stream_failure = reader.failure_state();
    let preference = session
        .selected_output(&generation)
        .map_err(PlaybackPipelineError::output_policy)?;
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    let device =
        super::devices::open_device(&preference).map_err(PlaybackPipelineError::output_policy)?;
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let device = return Err(PlaybackPipelineError::output_policy(
        "OUTPUT_SHARED_UNSUPPORTED",
    ));
    *endpoint.lock().unwrap_or_else(|error| error.into_inner()) =
        Some(preference.display_name.clone());
    let supported = device.default_output_config().map_err(|error| {
        PlaybackPipelineError::output_open(
            anyhow::Error::new(error).context("read default output configuration"),
        )
    })?;
    let config: cpal::StreamConfig = supported.into();
    if !matches!(config.channels, 1 | 2) {
        return Err(PlaybackPipelineError::output_open(anyhow::anyhow!(
            "unsupported output layout"
        )));
    }
    let samples_per_second = config.sample_rate as usize * config.channels as usize;
    let capacity = (samples_per_second * PCM_TARGET_MILLISECONDS / 1000)
        .min(PCM_CAPACITY_MAX_BYTES / std::mem::size_of::<f32>());
    let pcm = Arc::new(ArrayQueue::new(capacity));
    let consumed = Arc::new(AtomicU64::new(0));
    let base_position_ms = Arc::new(AtomicU64::new(start_ms));
    let output_lost = Arc::new(AtomicBool::new(false));
    #[cfg(target_os = "windows")]
    let _endpoint_monitor = super::devices::wasapi_monitor::SelectedEndpointMonitor::new(
        &preference.stable_id,
        gate.clone(),
        output_lost.clone(),
    )
    .map_err(PlaybackPipelineError::output_policy)?;
    let decoder_pcm = pcm.clone();
    let decoder_cancel = cancel.clone();
    let rate = config.sample_rate;
    let channels = config.channels;
    let hint = hint.to_string();
    let media_seek_requested = seek_commit.is_some();
    let seek_qualified = Arc::new(AtomicU64::new(0));
    let decoder_seek_qualified = seek_qualified.clone();
    let seek_landing_frame =
        media_seek_requested.then(|| Arc::new(AtomicU64::new(UNKNOWN_SEEK_LANDING_FRAME)));
    let decoder_seek_landing = seek_landing_frame.clone();
    let decoder = super::output::DecoderWorker::spawn(cancel.clone(), move || {
        decode_stream_with_seek(
            reader,
            Some(&hint),
            rate,
            channels,
            start_ms.saturating_mul(u64::from(rate)) / 1000,
            seek_candidate,
            media_seek_requested,
            Some(provider_duration_ms),
            Some(decoder_seek_qualified),
            decoder_seek_landing,
            decoder_pcm,
            decoder_cancel,
        )
    });
    let presentation = Arc::new(super::output::PresentationClock::new());
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => build_stream::<f32>(
            &device,
            &config,
            pcm.clone(),
            gate.clone(),
            consumed.clone(),
            output_lost.clone(),
            generation_serial.clone(),
            expected_serial,
            cancel.clone(),
            decoder.finished.clone(),
            presentation.clone(),
            position_ms.clone(),
            base_position_ms.clone(),
        ),
        cpal::SampleFormat::I32 => build_stream::<i32>(
            &device,
            &config,
            pcm.clone(),
            gate.clone(),
            consumed.clone(),
            output_lost.clone(),
            generation_serial.clone(),
            expected_serial,
            cancel.clone(),
            decoder.finished.clone(),
            presentation.clone(),
            position_ms.clone(),
            base_position_ms.clone(),
        ),
        cpal::SampleFormat::F64 => build_stream::<f64>(
            &device,
            &config,
            pcm.clone(),
            gate.clone(),
            consumed.clone(),
            output_lost.clone(),
            generation_serial.clone(),
            expected_serial,
            cancel.clone(),
            decoder.finished.clone(),
            presentation.clone(),
            position_ms.clone(),
            base_position_ms.clone(),
        ),
        cpal::SampleFormat::I16 => build_stream::<i16>(
            &device,
            &config,
            pcm.clone(),
            gate.clone(),
            consumed.clone(),
            output_lost.clone(),
            generation_serial.clone(),
            expected_serial,
            cancel.clone(),
            decoder.finished.clone(),
            presentation.clone(),
            position_ms.clone(),
            base_position_ms.clone(),
        ),
        cpal::SampleFormat::U16 => build_stream::<u16>(
            &device,
            &config,
            pcm.clone(),
            gate.clone(),
            consumed.clone(),
            output_lost.clone(),
            generation_serial.clone(),
            expected_serial,
            cancel.clone(),
            decoder.finished.clone(),
            presentation.clone(),
            position_ms.clone(),
            base_position_ms.clone(),
        ),
        _ => Err(anyhow::anyhow!("unsupported output sample format")),
    }
    .map_err(PlaybackPipelineError::output_open)?;
    while pcm.len() < (rate as usize * channels as usize * STARTUP_FILL_MILLISECONDS / 1000)
        && !decoder.is_finished()
        && !cancel.load(Ordering::Acquire)
    {
        if let Err(error) = preparation.check() {
            return Err(PlaybackPipelineError::timeout(anyhow::Error::new(error)));
        }
        pcm_high_water.fetch_max(pcm.len() as u64, Ordering::AcqRel);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    preparation
        .check()
        .map_err(|error| PlaybackPipelineError::timeout(anyhow::Error::new(error)))?;
    preparation.ready();
    if !session.output_opened(&generation, &preference) {
        return Err(PlaybackPipelineError::cancelled(anyhow::anyhow!(
            "output selection superseded"
        )));
    }
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
            base_position_ms.store(0, Ordering::Release);
            position_ms.store(0, Ordering::Release);
        }
        session.publish_event_at_epoch(
            generation.clone(),
            seek_qualification_event(duration),
            event_epoch.load(Ordering::Acquire),
        );
    }
    if let Some((operation_id, requested_position_ms)) = seek_commit {
        let actual_frame = seek_landing_frame
            .as_ref()
            .map(|value| value.load(Ordering::Acquire))
            .filter(|value| *value != UNKNOWN_SEEK_LANDING_FRAME)
            .ok_or_else(|| {
                PlaybackPipelineError::decode(anyhow::anyhow!(
                    "decoder did not report a seek landing position"
                ))
            })?;
        let actual_position_ms = actual_frame.saturating_mul(1000) / u64::from(rate);
        if actual_position_ms.abs_diff(requested_position_ms) > 50 {
            return Err(PlaybackPipelineError::decode(anyhow::anyhow!(
                "decoder seek landing exceeded 50 ms tolerance"
            )));
        }
        base_position_ms.store(actual_position_ms, Ordering::Release);
        position_ms.store(actual_position_ms, Ordering::Release);
        session.publish_event_at_epoch(
            generation.clone(),
            PlaybackEvent::SeekCommitted {
                operation_id,
                requested_position_ms,
                actual_position_ms,
            },
            event_epoch.load(Ordering::Acquire),
        );
    }
    if let Err(error) = stream.play() {
        cancel.store(true, Ordering::Release);
        let _ = decoder.join();
        return Err(PlaybackPipelineError::output_open(
            anyhow::Error::new(error).context("start output stream"),
        ));
    }
    let snapshot = match session.snapshot() {
        Ok(snapshot) => snapshot,
        Err(error) => {
            cancel.store(true, Ordering::Release);
            let _ = decoder.join();
            return Err(PlaybackPipelineError::decode(anyhow::anyhow!(
                error.message
            )));
        }
    };
    let occurrence = snapshot
        .current
        .map(|value| value.occurrence_id)
        .unwrap_or_default();
    let mut active = false;
    let mut control_epoch = event_epoch.load(Ordering::Acquire);
    let mut buffering = false;
    let mut last_samples = 0;
    let mut seq = 0;
    while !cancel.load(Ordering::Acquire) {
        pcm_high_water.fetch_max(pcm.len() as u64, Ordering::AcqRel);
        if generation_serial.load(Ordering::Acquire) != expected_serial {
            cancel.store(true, Ordering::Release);
            break;
        }
        if output_lost.load(Ordering::Acquire) {
            cancel.store(true, Ordering::Release);
            let _ = decoder.join();
            return Err(PlaybackPipelineError::output_lost(anyhow::anyhow!(
                "output stream callback reported loss"
            )));
        }
        let samples = consumed.load(Ordering::Acquire);
        let next_epoch = event_epoch.load(Ordering::Acquire);
        if next_epoch != control_epoch {
            active = false;
            control_epoch = next_epoch;
        }
        let (next_active, became_active) =
            activity_transition(gate.load(Ordering::Acquire), samples, last_samples, active);
        active = next_active;
        if became_active {
            buffering = false;
            session.publish_event_at_epoch(
                generation.clone(),
                PlaybackEvent::Active,
                control_epoch,
            );
        }
        if active && pcm.is_empty() && !decoder.is_finished() && !buffering {
            buffering = true;
            active = false;
            session.publish_event_at_epoch(
                generation.clone(),
                PlaybackEvent::Buffering,
                control_epoch,
            );
        }
        seq += 1;
        let position = base_position_ms
            .load(Ordering::Acquire)
            .saturating_add(samples.saturating_mul(1000) / u64::from(channels) / u64::from(rate));
        if active {
            let _ = session.report_progress(&generation, &occurrence, seq, position);
        }
        last_samples = samples;
        if decoder.is_finished() && presentation.drained_when(|| pcm.is_empty()) {
            let result = decoder
                .join()
                .map_err(|_| {
                    PlaybackPipelineError::decode(anyhow::anyhow!("decoder worker panicked"))
                })?
                .map_err(|error| {
                    PlaybackPipelineError::from_decode_error_and_stream_state(
                        error,
                        &stream_failure,
                    )
                })?;
            let decoded_ms = if media_seek_requested {
                result.emitted_frames
            } else {
                result.frames
            }
            .saturating_mul(1000)
                / u64::from(rate);
            session.publish_event_at_epoch(
                generation,
                PlaybackEvent::Completed {
                    position_ms: if media_seek_requested {
                        base_position_ms
                            .load(Ordering::Acquire)
                            .saturating_add(decoded_ms)
                    } else {
                        decoded_ms
                    },
                },
                event_epoch.load(Ordering::Acquire),
            );
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    Ok(())
}

#[cfg(test)]
fn endpoint_is_current(opened: Option<&str>, current: Option<&str>) -> bool {
    matches!((opened, current), (Some(opened), Some(current)) if opened == current)
}

fn activity_transition(
    gate_open: bool,
    samples: u64,
    last_samples: u64,
    active: bool,
) -> (bool, bool) {
    if !gate_open {
        return (false, false);
    }
    let became_active = samples > last_samples && !active;
    (active || became_active, became_active)
}

#[allow(clippy::too_many_arguments)]
fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    pcm: Arc<ArrayQueue<f32>>,
    gate: Arc<AtomicBool>,
    consumed: Arc<AtomicU64>,
    lost: Arc<AtomicBool>,
    generation_serial: Arc<AtomicU64>,
    expected_serial: u64,
    cancel: Arc<AtomicBool>,
    decoded: Arc<AtomicBool>,
    presentation: Arc<super::output::PresentationClock>,
    position_ms: Arc<AtomicU64>,
    base_position_ms: Arc<AtomicU64>,
) -> anyhow::Result<cpal::Stream>
where
    T: SizedSample + FromSample<f32> + Sample,
{
    let rate = config.sample_rate;
    let channels = config.channels;
    let mut consumer = super::output::PcmConsumer::new(
        config.channels as usize,
        rate as usize * config.channels as usize * STARTUP_FILL_MILLISECONDS / 1000,
    );
    let callback_lost = lost.clone();
    let mut previous_callback: Option<std::time::SystemTime> = None;
    let error_gate = gate.clone();
    Ok(device.build_output_stream(
        *config,
        move |output: &mut [T], info| {
            presentation.begin_callback();
            let now = std::time::SystemTime::now();
            if previous_callback.is_some_and(|last| {
                now.duration_since(last)
                    .map_or(true, |gap| gap > std::time::Duration::from_secs(2))
            }) {
                gate.store(false, Ordering::Release);
                callback_lost.store(true, Ordering::Release);
            }
            previous_callback = Some(now);
            let enabled = !callback_lost.load(Ordering::Acquire)
                && gate.load(Ordering::Acquire)
                && !cancel.load(Ordering::Acquire)
                && generation_serial.load(Ordering::Acquire) == expected_serial;
            let rendered = consumer.render(output, &pcm, enabled, decoded.load(Ordering::Acquire));
            if let Some(frame_end) = rendered.last_audio_frame {
                let timestamps = info.timestamp();
                let latency = timestamps.playback.duration_since(timestamps.callback);
                presentation.submit(presentation.now_ns(), latency, frame_end, rate);
            }
            let total = consumed.fetch_add(rendered.samples, Ordering::Release) + rendered.samples;
            position_ms.store(
                base_position_ms.load(Ordering::Acquire).saturating_add(
                    total.saturating_mul(1000) / u64::from(channels) / u64::from(rate),
                ),
                Ordering::Release,
            );
            presentation.end_callback();
        },
        move |error| {
            if matches!(
                error.kind(),
                cpal::ErrorKind::Xrun | cpal::ErrorKind::RealtimeDenied
            ) {
                return;
            }
            error_gate.store(false, Ordering::Release);
            lost.store(true, Ordering::Release);
        },
        None,
    )?)
}

fn pulse_runtime_version() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        Some(
            libpulse_binding::version::get_library_version()
                .to_string_lossy()
                .into_owned(),
        )
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

#[cfg(target_os = "linux")]
static SERVER_BUFFER_MAX_BYTES: AtomicU64 = AtomicU64::new(0);

fn server_buffer_max_bytes() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        Some(SERVER_BUFFER_MAX_BYTES.load(Ordering::Acquire))
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

pub fn runtime_identity() -> serde_json::Value {
    let version = |packed: u32| {
        format!(
            "{}.{}.{}",
            packed >> 16,
            (packed >> 8) & 0xff,
            packed & 0xff
        )
    };
    let (endpoint, compressed_high_water_bytes, pcm_high_water_samples) = global()
        .current
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .as_ref()
        .map(|pipeline| {
            (
                pipeline
                    .endpoint
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .clone(),
                pipeline.compressed_high_water.load(Ordering::Acquire),
                pipeline.pcm_high_water.load(Ordering::Acquire),
            )
        })
        .unwrap_or((None, 0, 0));
    serde_json::json!({
        "binding": "ffmpeg-next-9.0.0",
        "sharedBackend": if cfg!(target_os="linux") {"pulse"} else if cfg!(target_os="macos") {"coreaudio"} else {"wasapi"},
        "cpalVersion": "0.18.2",
        "pulseVersion": pulse_runtime_version(),
        "pulseServerBufferMaxBytes": server_buffer_max_bytes(),
        "avcodec": version(ffmpeg_next::codec::version()),
        "avformat": version(ffmpeg_next::format::version()),
        "avutil": version(ffmpeg_next::util::version()),
        "swresample": version(ffmpeg_next::software::resampling::version()),
        "sharedEndpoint": endpoint,
        "compressedHighWaterBytes": compressed_high_water_bytes,
        "pcmHighWaterSamples": pcm_high_water_samples,
        "manifest": serde_json::from_str::<serde_json::Value>(include_str!("../../audio-runtime.json"))
            .unwrap_or(serde_json::Value::Null)
    })
}

fn verify_runtime() -> anyhow::Result<()> {
    let major = |version: u32| version >> 16;
    let actual = [
        ("libavcodec", major(ffmpeg_next::codec::version()), 63),
        ("libavformat", major(ffmpeg_next::format::version()), 63),
        ("libavutil", major(ffmpeg_next::util::version()), 61),
        (
            "libswresample",
            major(ffmpeg_next::software::resampling::version()),
            7,
        ),
    ];
    for (name, found, expected) in actual {
        if found != expected {
            anyhow::bail!("{name} ABI mismatch: expected {expected}, found {found}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::{PlaybackProvenance, PlaybackRepresentation, ProviderError};

    fn queued_session() -> crate::playback::PlaybackSession {
        use crate::playback::model::*;
        let session = crate::playback::PlaybackSession::restore(
            Arc::new(crate::db::Database::memory().unwrap()),
            uuid::Uuid::new_v4().to_string(),
        );
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
                        server_id: "fixture".into(),
                        track_id: "track".into(),
                    }],
                },
            })
            .unwrap();
        session
    }

    fn unused_description() -> PlaybackDescription {
        PlaybackDescription {
            song: serde_json::from_value(serde_json::json!({
                "id": "track", "title": "Track", "duration": 1
            }))
            .unwrap(),
            representations: vec![],
        }
    }

    #[tokio::test]
    async fn stale_resume_epoch_is_rejected_before_setup_and_after_waiting_for_start_lock() {
        use crate::playback::NativeControlIntent;
        for pause_while_waiting in [false, true] {
            let session = queued_session();
            let admitted = session
                .native_control(NativeControlIntent::Play, None)
                .unwrap();
            let engine = AudioEngine {
                current: Mutex::new(None),
                starts: tokio::sync::Mutex::new(()),
            };
            let held = engine.starts.lock().await;
            let start = engine.start_at_epoch(
                unused_description(),
                admitted.current.as_ref().unwrap().source.clone(),
                0,
                admitted.generation_id.clone(),
                session.clone(),
                std::time::Instant::now() + std::time::Duration::from_secs(1),
                admitted.resume_epoch,
            );
            tokio::pin!(start);
            if pause_while_waiting {
                // Poll the real start until it is pending on the held start lock.
                tokio::select! {
                    biased;
                    _ = &mut start => panic!("start bypassed its serialized lock"),
                    _ = std::future::ready(()) => {},
                }
            }
            session
                .native_control(NativeControlIntent::Pause, None)
                .unwrap();
            drop(held);
            let error = start.await.unwrap_err();
            assert!(
                !error.is_publishable(),
                "stale work must cancel before output setup"
            );
            assert!(engine.current.lock().unwrap().is_none());
            assert_eq!(
                session.snapshot().unwrap().generation_id,
                admitted.generation_id
            );
            assert!(!session.output_gate().load(Ordering::Acquire));
            session.stop_and_join().unwrap();
        }
    }

    #[test]
    fn installed_pipeline_epoch_advances_only_for_a_current_admitted_resume() {
        use crate::playback::NativeControlIntent;
        let session = queued_session();
        let admitted = session
            .native_control(NativeControlIntent::Play, None)
            .unwrap();
        let authorized = Arc::new(AtomicU64::new(admitted.resume_epoch));
        let engine = AudioEngine {
            starts: tokio::sync::Mutex::new(()),
            current: Mutex::new(Some(Pipeline {
                cancel: Arc::new(AtomicBool::new(false)),
                gate: session.output_gate(),
                alive: Arc::new(AtomicBool::new(true)),
                generation: admitted.generation_id.clone(),
                event_epoch: authorized.clone(),
                worker: std::thread::spawn(|| {}),
                compressed_high_water: Arc::new(AtomicU64::new(0)),
                pcm_high_water: Arc::new(AtomicU64::new(0)),
                endpoint: Arc::new(Mutex::new(None)),
                position_ms: Arc::new(AtomicU64::new(0)),
            })),
        };
        session
            .native_control(NativeControlIntent::Pause, None)
            .unwrap();
        assert!(!engine.resume_existing(&admitted.generation_id, &session, admitted.resume_epoch));
        assert_eq!(authorized.load(Ordering::Acquire), admitted.resume_epoch);
        let resumed = session
            .native_control(NativeControlIntent::Play, None)
            .unwrap();
        assert!(engine.resume_existing(&resumed.generation_id, &session, resumed.resume_epoch));
        assert_eq!(authorized.load(Ordering::Acquire), resumed.resume_epoch);
        assert!(engine.current.lock().unwrap().is_some());
        engine.stop_and_join().unwrap();
        session.stop_and_join().unwrap();
    }

    #[tokio::test]
    async fn stalled_retirement_keeps_start_owned_and_shutdown_owner_responsive() {
        use crate::playback::{
            config::{OutputPreference, PlaybackConfig},
            devices::{Discovery, OutputDescriptor},
        };
        let directory = tempfile::tempdir().unwrap();
        let preference = OutputPreference {
            backend: "coreaudio".into(),
            stable_id: "test".into(),
            display_name: "Test".into(),
            identity_properties: Default::default(),
        };
        let path = directory.path().join("playback.json");
        crate::playback::config::save(
            &path,
            &PlaybackConfig {
                schema_version: 1,
                output: Some(preference.clone()),
            },
            false,
        )
        .unwrap();
        let session = crate::playback::PlaybackSession::restore(
            Arc::new(crate::db::Database::memory().unwrap()),
            uuid::Uuid::new_v4().to_string(),
        );
        session.enable_outputs_with(path, move || Discovery {
            outputs: vec![OutputDescriptor {
                output_id: crate::playback::devices::output_id(&preference),
                display_name: preference.display_name.clone(),
                detail: String::new(),
                backend: preference.backend.clone(),
                available: true,
                is_default: false,
                identity_confidence: "stable".into(),
                is_virtual: false,
                preference: Some(preference.clone()),
            }],
            error: None,
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !session
            .snapshot()
            .unwrap()
            .output
            .selected
            .unwrap()
            .available
        {
            assert!(std::time::Instant::now() < deadline);
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        let generation = session.snapshot().unwrap().generation_id;
        let gate = Arc::new(AtomicBool::new(true));
        let cancel = Arc::new(AtomicBool::new(false));
        let exited = Arc::new(AtomicBool::new(false));
        let worker_exited = exited.clone();
        let (release, wait) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            wait.recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
            worker_exited.store(true, Ordering::Release);
        });
        let engine = Arc::new(AudioEngine {
            starts: tokio::sync::Mutex::new(()),
            current: Mutex::new(Some(Pipeline {
                gate: gate.clone(),
                cancel: cancel.clone(),
                alive: Arc::new(AtomicBool::new(true)),
                generation: generation.clone(),
                event_epoch: Arc::new(AtomicU64::new(session.control_epoch())),
                worker,
                compressed_high_water: Arc::new(AtomicU64::new(0)),
                pcm_high_water: Arc::new(AtomicU64::new(0)),
                endpoint: Arc::new(Mutex::new(None)),
                position_ms: Arc::new(AtomicU64::new(0)),
            })),
        });
        let starting = engine.clone();
        let owner = session.clone();
        let replacement =
            tokio::spawn(async move { starting.validate_selected_output(owner, generation).await });
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            if session
                .snapshot()
                .unwrap()
                .output
                .error
                .as_ref()
                .is_some_and(|e| e.code == "OUTPUT_RETIREMENT_PENDING")
            {
                break;
            }
            assert!(tokio::time::Instant::now() < deadline);
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(!gate.load(Ordering::Acquire));
        assert!(cancel.load(Ordering::Acquire));
        assert!(!replacement.is_finished());
        assert!(!exited.load(Ordering::Acquire));
        assert!(
            engine.starts.try_lock().is_err(),
            "new output cannot bypass retirement"
        );
        assert!(
            engine.current.try_lock().is_ok(),
            "join does not retain pipeline mutex"
        );
        session
            .begin_shutdown_checkpoint()
            .unwrap()
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap()
            .unwrap();
        assert!(
            !replacement.is_finished(),
            "shutdown fence does not abandon native retirement"
        );
        release.send(()).unwrap();
        let result = tokio::time::timeout(std::time::Duration::from_secs(1), replacement)
            .await
            .unwrap()
            .unwrap();
        assert!(
            result.is_err(),
            "shutdown prevents replacement open after retirement"
        );
        assert!(exited.load(Ordering::Acquire));
        assert!(engine.starts.try_lock().is_ok());
        session.stop_and_join().unwrap();
    }

    #[tokio::test]
    async fn stale_idle_validation_does_not_retire_newer_playtrack_pipeline() {
        let session = crate::playback::PlaybackSession::restore(
            Arc::new(crate::db::Database::memory().unwrap()),
            uuid::Uuid::new_v4().to_string(),
        );
        let generation = session.snapshot().unwrap().generation_id;
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let worker = std::thread::spawn(move || {
            while !worker_cancel.load(Ordering::Acquire) {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        });
        let gate = Arc::new(AtomicBool::new(true));
        let engine = AudioEngine {
            starts: tokio::sync::Mutex::new(()),
            current: Mutex::new(Some(Pipeline {
                gate: gate.clone(),
                cancel: cancel.clone(),
                alive: Arc::new(AtomicBool::new(true)),
                generation,
                event_epoch: Arc::new(AtomicU64::new(session.control_epoch())),
                worker,
                position_ms: Arc::new(AtomicU64::new(1234)),
                compressed_high_water: Arc::new(AtomicU64::new(0)),
                pcm_high_water: Arc::new(AtomicU64::new(0)),
                endpoint: Arc::new(Mutex::new(None)),
            })),
        };
        let result = engine
            .validate_selected_output(session.clone(), uuid::Uuid::new_v4().to_string())
            .await;
        let untouched = !cancel.load(Ordering::Acquire)
            && gate.load(Ordering::Acquire)
            && engine.current.lock().unwrap().is_some();
        cancel.store(true, Ordering::Release);
        if let Some(pipeline) = engine.current.lock().unwrap().take() {
            pipeline.worker.join().unwrap();
        }
        assert_eq!(result.unwrap_err().code(), "GENERATION_CONFLICT");
        assert!(
            untouched,
            "stale validation must not gate, cancel or remove newer playback"
        );
        session.stop_and_join().unwrap();
    }

    fn request(url: &str) -> PlaybackRequest {
        PlaybackRequest {
            url: reqwest::Url::parse(url).unwrap(),
            headers: reqwest::header::HeaderMap::new(),
            range_supported: false,
        }
    }

    fn representation(
        container: Option<&str>,
        codec: Option<&str>,
        url: &str,
    ) -> PlaybackRepresentation {
        PlaybackRepresentation {
            codec: codec.map(str::to_owned),
            container: container.map(str::to_owned),
            bitrate_kbps: None,
            sample_rate: None,
            bit_depth: None,
            provenance: PlaybackProvenance::Original,
            seek_mechanism: None,
            request: request(url),
        }
    }

    #[test]
    fn new_format_hints_and_diagnostics_are_normalized() {
        for label in ["aif", "aiff", "ogg", "oga", "wma", "asf"] {
            let container = format!(".{}", label.to_ascii_uppercase());
            let source = representation(
                Some(&container),
                None,
                "https://music.example/Items/id/Download",
            );
            assert_eq!(decoder_hint(&source), format!("stream.{label}"));
            assert_eq!(diagnostic_representation(&container), label);
        }
        for codec in [
            "vorbis",
            "wmav1",
            "wmav2",
            "wmapro",
            "wmalossless",
            "pcm_s16be",
            "pcm_s24be",
            "pcm_s32be",
        ] {
            assert_eq!(
                diagnostic_representation(&codec.to_ascii_uppercase()),
                codec
            );
        }
        assert_eq!(diagnostic_representation("private-title-secret"), "unknown");
    }

    #[test]
    fn decoder_hint_normalizes_containers_without_treating_codec_as_container() {
        let route = "https://media.example/Items/id/Download";
        let empty = representation(Some("  "), Some("flac"), route);
        assert_eq!(decoder_hint(&empty), "/Items/id/Download");
        assert_eq!(representation_name(&empty), "flac");

        let mime = representation(Some(" audio/FLAC ; charset=binary "), None, route);
        assert_eq!(decoder_hint(&mime), "stream.flac");

        let codec_only = representation(None, Some("flac"), route);
        assert_eq!(decoder_hint(&codec_only), "/Items/id/Download");

        let wrapped = representation(Some("ogg"), Some("flac"), route);
        assert_eq!(decoder_hint(&wrapped), "stream.ogg");

        let m4r = representation(Some(".M4R"), Some("aac"), route);
        assert_eq!(decoder_hint(&m4r), "stream.m4r");
    }

    #[test]
    fn decode_error_text_containing_output_stays_decode_failed() {
        let error = PlaybackPipelineError::decode(anyhow::anyhow!(
            "decoder rejected unsupported output layout"
        ));
        assert_eq!(error.stage(), "decode");
        assert_eq!(error.code(), "DECODE_FAILED");
        assert!(error.retryable());
    }

    #[test]
    fn output_open_error_maps_to_output_unavailable() {
        let error = PlaybackPipelineError::output_open(anyhow::anyhow!("device open failed"));
        assert_eq!(error.stage(), "output-open");
        assert_eq!(error.code(), "OUTPUT_UNAVAILABLE");
        assert!(error.retryable());
    }

    #[test]
    fn output_loss_maps_to_output_lost() {
        let error = PlaybackPipelineError::output_lost(anyhow::anyhow!("stream callback failed"));
        assert_eq!(error.stage(), "output-lost");
        assert_eq!(error.code(), "OUTPUT_LOST");
        assert!(error.retryable());
    }

    #[test]
    fn output_loss_remains_publishable_after_worker_cancels_decode() {
        let error = PlaybackPipelineError::output_lost(anyhow::anyhow!("endpoint disappeared"));
        assert!(should_publish_worker_failure(&error, true));

        let cancelled = PlaybackPipelineError::cancelled(anyhow::anyhow!("explicit stop"));
        assert!(!should_publish_worker_failure(&cancelled, true));
    }

    #[test]
    fn active_endpoint_change_or_disappearance_is_output_loss() {
        assert!(endpoint_is_current(
            Some("Jabra Link 390"),
            Some("Jabra Link 390")
        ));
        assert!(!endpoint_is_current(
            Some("Jabra Link 390"),
            Some("MacBook Speakers")
        ));
        assert!(!endpoint_is_current(Some("Jabra Link 390"), None));
        assert!(!endpoint_is_current(None, Some("MacBook Speakers")));
    }

    #[test]
    fn resumed_consumption_republishes_active_after_pause() {
        let (active, publish) = activity_transition(false, 100, 100, true);
        assert!(!active);
        assert!(!publish);

        let (active, publish) = activity_transition(true, 101, 100, active);
        assert!(active);
        assert!(publish);
    }

    #[test]
    fn source_failure_maps_to_source_unavailable() {
        let error = PlaybackPipelineError::source(anyhow::anyhow!("provider read failed"));
        assert_eq!(error.stage(), "provider");
        assert_eq!(error.code(), "SOURCE_UNAVAILABLE");
        assert!(error.retryable());
    }

    #[test]
    fn stalled_source_maps_to_playback_timeout() {
        let error = PlaybackPipelineError::timeout(anyhow::anyhow!("source stalled"));
        assert_eq!(error.stage(), "timeout");
        assert_eq!(error.code(), "PLAYBACK_TIMEOUT");
        assert!(error.retryable());
    }

    #[test]
    fn unsupported_representation_maps_to_non_retryable_public_failure() {
        let error = PlaybackPipelineError::unsupported(anyhow::anyhow!("unsupported codec"));
        assert_eq!(error.stage(), "provider");
        assert_eq!(error.code(), "PLAYBACK_UNSUPPORTED");
        assert!(!error.retryable());
    }

    #[test]
    fn superseded_generation_error_is_not_publishable() {
        let error = PlaybackPipelineError::cancelled(anyhow::anyhow!("generation superseded"));
        assert!(!error.is_publishable());
    }

    #[test]
    fn diagnostic_chain_redacts_urls_credentials_titles_and_ids() {
        let source = anyhow::anyhow!(
            "GET https://music.example/Items/private-id?token=private-token title=private-title trackId=private-id"
        )
        .context("while decoding response");
        let diagnostic = PlaybackPipelineError::decode(source).diagnostic_chain();
        assert!(diagnostic.contains("while decoding response"));
        assert!(diagnostic.contains("[redacted-url]"));
        for private in [
            "https://",
            "music.example",
            "private-token",
            "private-title",
            "private-id",
        ] {
            assert!(
                !diagnostic.contains(private),
                "leaked {private}: {diagnostic}"
            );
        }
    }

    #[test]
    fn provider_not_found_diagnostic_omits_item_id() {
        let error = PlaybackPipelineError::from_provider_error(ProviderError::NotFound {
            item_type: "track".into(),
            id: "private-track-id".into(),
        });
        assert_eq!(error.stage(), "provider");
        assert_eq!(error.code(), "SOURCE_UNAVAILABLE");
        assert!(!error.diagnostic_chain().contains("private-track-id"));
    }

    #[test]
    fn typed_stream_timeout_survives_decoder_context() {
        let source = anyhow::Error::new(std::io::Error::other(
            super::super::streaming::StreamReadError::Timeout,
        ))
        .context("decode input");
        let error = PlaybackPipelineError::from_decode_error(source);
        assert_eq!(error.stage(), "timeout");
        assert_eq!(error.code(), "PLAYBACK_TIMEOUT");
    }

    #[test]
    fn diagnostic_representation_rejects_arbitrary_provider_text() {
        assert_eq!(diagnostic_representation("private album title"), "unknown");
        assert_eq!(diagnostic_representation("audio/MP4"), "mp4");
    }

    #[test]
    fn non_audio_content_type_detection_is_case_insensitive() {
        assert!(content_type_is_non_audio("Application/JSON; Charset=UTF-8"));
        assert!(content_type_is_non_audio("APPLICATION/XML"));
        assert!(!content_type_is_non_audio("audio/mp4"));
    }

    #[test]
    fn ffmpeg_error_uses_shared_stream_timeout_stage() {
        let error = PlaybackPipelineError::from_decode_error_and_stream_failure(
            anyhow::anyhow!("ffmpeg input/output error"),
            Some(super::super::streaming::StreamFailureKind::Timeout),
        );
        assert_eq!(error.stage(), "timeout");
        assert_eq!(error.code(), "PLAYBACK_TIMEOUT");
    }

    #[test]
    fn shared_stream_source_detail_is_preserved_in_sanitized_diagnostic() {
        use std::io::Read as _;

        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, mut reader) = BoundedHttpReader::channel(cancel);
        let failure = reader.failure_state();
        tx.blocking_send(Err(StreamReadError::Source(
            "GET https://music.example/private?token=secret reset".into(),
        )))
        .unwrap();
        let _ = reader.read(&mut [0u8; 1]);

        let error = PlaybackPipelineError::from_decode_error_and_stream_state(
            anyhow::anyhow!("ffmpeg input/output error"),
            &failure,
        );
        let diagnostic = error.diagnostic_chain();
        assert_eq!(error.stage(), "provider");
        assert!(diagnostic.contains("ffmpeg input/output error"));
        assert!(diagnostic.contains("[redacted-url]"));
        assert!(!diagnostic.contains("music.example"));
        assert!(!diagnostic.contains("secret"));
    }

    #[tokio::test]
    async fn fetch_rejects_redirects_and_provider_error_documents() {
        let mut server = mockito::Server::new_async().await;
        let redirect = server
            .mock("GET", "/redirect")
            .with_status(302)
            .with_header("location", "https://other.example/audio")
            .create_async()
            .await;
        let document = server
            .mock("GET", "/document")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"error":"credential-must-not-escape"}"#)
            .create_async()
            .await;

        assert!(
            fetch(&request(&format!("{}/redirect", server.url())))
                .await
                .unwrap_err()
                .to_string()
                .contains("source unavailable")
        );
        let error = fetch(&request(&format!("{}/document", server.url())))
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("non-audio"));
        assert!(!error.contains("credential-must-not-escape"));
        redirect.assert_async().await;
        document.assert_async().await;
    }
    #[test]
    fn review_runtime_qualification_enables_only_verified_pcm_wav_duration() {
        for duration in [0, 2_000, 10_500] {
            let PlaybackEvent::SeekQualified {
                capability,
                duration_ms,
            } = seek_qualification_event(duration)
            else {
                panic!("qualification event expected")
            };
            assert_eq!(duration_ms, (duration > 0).then_some(duration));
            if duration > 0 {
                assert!(capability.available);
                assert_eq!(
                    capability.mechanism.as_deref(),
                    Some("ffmpeg-post-open-media-time-seek")
                );
                assert_eq!(capability.decoded_landing_tolerance_ms, Some(50));
            } else {
                assert!(!capability.available);
                assert_eq!(
                    capability.reason.as_deref(),
                    Some("seek.duration_unavailable")
                );
            }
        }
    }
}
