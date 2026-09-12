use super::decoder::decode_stream;
use super::model::{PlaybackEvent, PlaybackTrackMetadata};
use super::streaming::{BoundedHttpReader, COMPRESSED_CHUNK_BYTES};
use crate::providers::{PlaybackDescription, PlaybackRequest, select_playback_representation};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SizedSample};
use crossbeam_queue::ArrayQueue;
use futures::StreamExt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

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
    ) -> anyhow::Result<()> {
        let _start_guard = self.starts.lock().await;
        let (generation_serial, expected_serial) = session
            .generation_guard(&generation)
            .ok_or_else(|| anyhow::anyhow!("playback generation was superseded"))?;
        verify_runtime()?;
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
                .map_err(|_| anyhow::anyhow!("retiring audio worker join failed"))?
                .map_err(|_| anyhow::anyhow!("retiring audio worker panicked"))?;
        }
        let representation = select_playback_representation(description.representations)
            .map_err(|_| anyhow::anyhow!("no supported representation"))?;
        let representation_name = representation_name(&representation);
        let decoder_hint = decoder_hint(&representation);
        let request = representation.request;
        let response = fetch(&request).await?;
        if generation_serial.load(Ordering::Acquire) != expected_serial {
            anyhow::bail!("playback generation was superseded");
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
                            let _ = tx
                                .send(Err("source made no byte progress for 15 seconds".into()))
                                .await;
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
                        let _ = tx.send(Err(format!("source read failed: {error}"))).await;
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
                representation: representation_name,
            },
        );
        let worker_cancel = cancel.clone();
        let worker_gate = gate.clone();
        let alive = Arc::new(AtomicBool::new(true));
        let worker_alive = alive.clone();
        let pipeline_generation = generation.clone();
        let worker_pcm_high_water = pcm_high_water.clone();
        let worker_endpoint = endpoint.clone();
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
                );
                match result {
                    Err(error) if !worker_cancel.load(Ordering::Acquire) => {
                        let message = error.to_string();
                        session.publish_event(
                            generation,
                            PlaybackEvent::Failed {
                                code: if message.contains("output lost") {
                                    "OUTPUT_LOST"
                                } else if message.contains("output") {
                                    "OUTPUT_UNAVAILABLE"
                                } else if message.contains("no byte progress") {
                                    "PLAYBACK_TIMEOUT"
                                } else if message.contains("source read failed") {
                                    "SOURCE_UNAVAILABLE"
                                } else {
                                    "DECODE_FAILED"
                                }
                                .into(),
                                retryable: true,
                            },
                        );
                    }
                    _ => {}
                }
                worker_alive.store(false, Ordering::Release);
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

async fn fetch(request: &PlaybackRequest) -> anyhow::Result<reqwest::Response> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()?;
    let response = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        client
            .get(request.url.clone())
            .headers(request.headers.clone())
            .send(),
    )
    .await
    .map_err(|_| anyhow::anyhow!("playback preparation timeout"))??;
    if !response.status().is_success() {
        anyhow::bail!("source unavailable ({})", response.status());
    }
    if response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.contains("json") || value.contains("xml"))
    {
        anyhow::bail!("provider returned a non-audio response");
    }
    Ok(response)
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
) -> anyhow::Result<()> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| anyhow::anyhow!("output device unavailable"))?;
    *endpoint.lock().unwrap_or_else(|error| error.into_inner()) = device.name().ok();
    let supported = device.default_output_config()?;
    let config: cpal::StreamConfig = supported.clone().into();
    if !matches!(config.channels, 1 | 2) {
        anyhow::bail!("unsupported output layout");
    }
    let capacity = ((config.sample_rate.0 as usize * config.channels as usize) / 2)
        .min(1024 * 1024 / std::mem::size_of::<f32>());
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
        )?,
        cpal::SampleFormat::I16 => build_stream::<i16>(
            &device,
            &config,
            pcm.clone(),
            gate.clone(),
            consumed.clone(),
            output_lost.clone(),
            generation_serial.clone(),
            expected_serial,
        )?,
        cpal::SampleFormat::U16 => build_stream::<u16>(
            &device,
            &config,
            pcm.clone(),
            gate.clone(),
            consumed.clone(),
            output_lost.clone(),
            generation_serial.clone(),
            expected_serial,
        )?,
        _ => anyhow::bail!("unsupported output sample format"),
    };
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
    while pcm.len() < (rate as usize * channels as usize / 10)
        && !decoder.is_finished()
        && !cancel.load(Ordering::Acquire)
    {
        pcm_high_water.fetch_max(pcm.len() as u64, Ordering::AcqRel);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    stream.play()?;
    let occurrence = session
        .snapshot()
        .map_err(|error| anyhow::anyhow!(error.message))?
        .current
        .map(|value| value.occurrence_id)
        .unwrap_or_default();
    let mut active = false;
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
            anyhow::bail!("output lost");
        }
        let samples = consumed.load(Ordering::Acquire);
        if samples > last_samples && !active {
            active = true;
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
                .map_err(|_| anyhow::anyhow!("decoder worker panicked"))??;
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
    use crate::providers::{PlaybackProvenance, PlaybackRepresentation};

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
