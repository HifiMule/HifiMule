use super::decoder::decode_stream;
use super::model::{PlaybackEvent, PlaybackTrackMetadata};
use super::streaming::{
    BoundedHttpReader, COMPRESSED_CHUNK_BYTES, StreamFailureKind, StreamFailureState,
    StreamReadError,
};
use crate::providers::{PlaybackDescription, PlaybackRequest, select_playback_representation};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SizedSample};
use crossbeam_queue::ArrayQueue;
use futures::StreamExt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

const PCM_TARGET_MILLISECONDS: usize = 500;
const PCM_CAPACITY_MAX_BYTES: usize = 1024 * 1024;
const STARTUP_FILL_MILLISECONDS: usize = 100;
const ENDPOINT_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(100);
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
    producer: tokio::task::JoinHandle<()>,
    worker: std::thread::JoinHandle<()>,
    compressed_high_water: Arc<AtomicU64>,
    pcm_high_water: Arc<AtomicU64>,
    endpoint: Arc<Mutex<Option<String>>>,
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
                    pipeline.producer.abort();
                }
            }
        }
    }

    pub fn resume_existing(&self, generation: &str) -> bool {
        let current = self.current.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(pipeline) = current.as_ref().filter(|pipeline| {
            pipeline.generation == generation
                && pipeline.alive.load(Ordering::Acquire)
                && !pipeline.cancel.load(Ordering::Acquire)
        }) {
            pipeline.gate.store(true, Ordering::Release);
            true
        } else {
            false
        }
    }

    pub fn stop_and_join(&self) -> anyhow::Result<()> {
        if let Some(pipeline) = self
            .current
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            pipeline.gate.store(false, Ordering::Release);
            pipeline.cancel.store(true, Ordering::Release);
            pipeline.producer.abort();
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
    ) -> Result<(), PlaybackPipelineError> {
        let _start_guard = self.starts.lock().await;
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
            old.gate.store(false, Ordering::Release);
            old.cancel.store(true, Ordering::Release);
            old.producer.abort();
            tokio::task::spawn_blocking(move || old.worker.join())
                .await
                .map_err(|_| {
                    PlaybackPipelineError::output_open(anyhow::anyhow!(
                        "retiring audio worker join failed"
                    ))
                })?
                .map_err(|_| {
                    PlaybackPipelineError::output_open(anyhow::anyhow!(
                        "retiring audio worker panicked"
                    ))
                })?;
        }
        let representation = select_playback_representation(description.representations)
            .map_err(PlaybackPipelineError::from_provider_error)?;
        let representation_name = representation_name(&representation);
        let decoder_hint = decoder_hint(&representation);
        let request = representation.request;
        let response = fetch(&request)
            .await
            .map_err(|error| error.with_representation(&representation_name))?;
        if generation_serial.load(Ordering::Acquire) != expected_serial {
            return Err(PlaybackPipelineError::cancelled(anyhow::anyhow!(
                "playback generation was superseded"
            )));
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let gate = Arc::new(AtomicBool::new(true));
        let compressed_high_water = Arc::new(AtomicU64::new(0));
        let pcm_high_water = Arc::new(AtomicU64::new(0));
        let endpoint = Arc::new(Mutex::new(None));
        let (tx, reader) = BoundedHttpReader::channel_with_high_water(
            cancel.clone(),
            compressed_high_water.clone(),
        );
        let producer_cancel = cancel.clone();
        let producer = tokio::spawn(async move {
            let mut body = response.bytes_stream();
            loop {
                let item =
                    match tokio::time::timeout(std::time::Duration::from_secs(15), body.next())
                        .await
                    {
                        Ok(Some(item)) => item,
                        Ok(None) => break,
                        Err(_) => {
                            let _ = tx.send(Err(StreamReadError::Timeout)).await;
                            break;
                        }
                    };
                if producer_cancel.load(Ordering::Acquire) {
                    break;
                }
                match item {
                    Ok(bytes) => {
                        for part in bytes.chunks(COMPRESSED_CHUNK_BYTES) {
                            if tx
                                .send(Ok(bytes::Bytes::copy_from_slice(part)))
                                .await
                                .is_err()
                            {
                                return;
                            }
                        }
                    }
                    Err(error) => {
                        let _ = tx
                            .send(Err(StreamReadError::Source(format!(
                                "source read failed: {error}"
                            ))))
                            .await;
                        return;
                    }
                }
            }
        });
        let metadata = PlaybackTrackMetadata {
            source,
            title: description.song.title.clone(),
            artist: description.song.artist_name.clone(),
            album: description.song.album_title.clone(),
        };
        session.publish_event(
            generation.clone(),
            PlaybackEvent::Resolved {
                metadata,
                duration_ms: Some(u64::from(description.song.duration_seconds) * 1000),
                representation: representation_name.clone(),
            },
        );
        let worker_cancel = cancel.clone();
        let worker_gate = gate.clone();
        let alive = Arc::new(AtomicBool::new(true));
        let worker_alive = alive.clone();
        let pipeline_generation = generation.clone();
        let worker_pcm_high_water = pcm_high_water.clone();
        let worker_endpoint = endpoint.clone();
        let worker_representation = representation_name.clone();
        let worker = std::thread::Builder::new()
            .name("hifimule-audio".into())
            .spawn(move || {
                let result = run_output(
                    reader,
                    &decoder_hint,
                    generation.clone(),
                    session.clone(),
                    worker_cancel.clone(),
                    worker_gate,
                    generation_serial.clone(),
                    expected_serial,
                    start_ms,
                    worker_pcm_high_water,
                    worker_endpoint,
                )
                .map_err(|error| error.with_representation(&worker_representation));
                match result {
                    Err(error)
                        if should_publish_worker_failure(
                            &error,
                            worker_cancel.load(Ordering::Acquire),
                        ) =>
                    {
                        publish_pipeline_failure(&session, generation, error);
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
        *self.current.lock().unwrap_or_else(|e| e.into_inner()) = Some(Pipeline {
            cancel,
            gate,
            alive,
            generation: pipeline_generation,
            producer,
            worker,
            compressed_high_water,
            pcm_high_water,
            endpoint,
        });
        Ok(())
    }
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
    start_ms: u64,
    pcm_high_water: Arc<AtomicU64>,
    endpoint: Arc<Mutex<Option<String>>>,
) -> Result<(), PlaybackPipelineError> {
    let stream_failure = reader.failure_state();
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or_else(|| {
        PlaybackPipelineError::output_open(anyhow::anyhow!("output device unavailable"))
    })?;
    let opened_endpoint = device.name().map_err(|error| {
        PlaybackPipelineError::output_open(
            anyhow::Error::new(error).context("read active output endpoint identity"),
        )
    })?;
    *endpoint.lock().unwrap_or_else(|error| error.into_inner()) = Some(opened_endpoint.clone());
    let supported = device.default_output_config().map_err(|error| {
        PlaybackPipelineError::output_open(
            anyhow::Error::new(error).context("read default output configuration"),
        )
    })?;
    let config: cpal::StreamConfig = supported.clone().into();
    if !matches!(config.channels, 1 | 2) {
        return Err(PlaybackPipelineError::output_open(anyhow::anyhow!(
            "unsupported output layout"
        )));
    }
    let samples_per_second = config.sample_rate.0 as usize * config.channels as usize;
    let capacity = (samples_per_second * PCM_TARGET_MILLISECONDS / 1000)
        .min(PCM_CAPACITY_MAX_BYTES / std::mem::size_of::<f32>());
    let pcm = Arc::new(ArrayQueue::new(capacity));
    let consumed = Arc::new(AtomicU64::new(0));
    let output_lost = Arc::new(AtomicBool::new(false));
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
        ),
        _ => Err(anyhow::anyhow!("unsupported output sample format")),
    }
    .map_err(PlaybackPipelineError::output_open)?;
    let decoder_pcm = pcm.clone();
    let decoder_cancel = cancel.clone();
    let rate = config.sample_rate.0;
    let channels = config.channels;
    let hint = hint.to_string();
    let decoder = std::thread::spawn(move || {
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
    while pcm.len() < (rate as usize * channels as usize * STARTUP_FILL_MILLISECONDS / 1000)
        && !decoder.is_finished()
        && !cancel.load(Ordering::Acquire)
    {
        pcm_high_water.fetch_max(pcm.len() as u64, Ordering::AcqRel);
        std::thread::sleep(std::time::Duration::from_millis(5));
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
    let mut buffering = false;
    let mut last_samples = 0;
    let mut seq = 0;
    let mut last_endpoint_check = std::time::Instant::now();
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
        if last_endpoint_check.elapsed() >= ENDPOINT_POLL_INTERVAL {
            last_endpoint_check = std::time::Instant::now();
            let current_endpoint = host
                .default_output_device()
                .and_then(|device| device.name().ok());
            if !endpoint_is_current(Some(&opened_endpoint), current_endpoint.as_deref()) {
                cancel.store(true, Ordering::Release);
                let _ = decoder.join();
                return Err(PlaybackPipelineError::output_lost(anyhow::anyhow!(
                    "active output endpoint changed or disappeared"
                )));
            }
        }
        let samples = consumed.load(Ordering::Acquire);
        let (next_active, became_active) =
            activity_transition(gate.load(Ordering::Acquire), samples, last_samples, active);
        active = next_active;
        if became_active {
            buffering = false;
            session.publish_event(generation.clone(), PlaybackEvent::Active);
        }
        if active && pcm.is_empty() && !decoder.is_finished() && !buffering {
            buffering = true;
            active = false;
            session.publish_event(generation.clone(), PlaybackEvent::Buffering);
        }
        seq += 1;
        let position = start_ms
            .saturating_add(samples.saturating_mul(1000) / u64::from(channels) / u64::from(rate));
        if active {
            let _ = session.report_progress(&generation, &occurrence, seq, position);
        }
        last_samples = samples;
        if decoder.is_finished() && pcm.is_empty() {
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
            session.publish_event(
                generation,
                PlaybackEvent::Completed {
                    position_ms: result.frames.saturating_mul(1000) / u64::from(rate),
                },
            );
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    Ok(())
}

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
) -> anyhow::Result<cpal::Stream>
where
    T: SizedSample + FromSample<f32> + Sample,
{
    Ok(device.build_output_stream(
        config,
        move |output: &mut [T], _| {
            let enabled = gate.load(Ordering::Acquire)
                && generation_serial.load(Ordering::Acquire) == expected_serial;
            let mut count = 0u64;
            for target in output {
                let value = if enabled {
                    match pcm.pop() {
                        Some(sample) => {
                            count += 1;
                            sample
                        }
                        None => 0.0,
                    }
                } else {
                    0.0
                };
                *target = T::from_sample(value);
            }
            consumed.fetch_add(count, Ordering::Release);
        },
        move |_| {
            lost.store(true, Ordering::Release);
        },
        None,
    )?)
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
}
