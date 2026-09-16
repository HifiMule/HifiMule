//! Daemon-owned HTTP IO. FFmpeg receives bytes only, never a URL or credentials.
use super::streaming::NETWORK_CHUNK_CAPACITY_BYTES;
use crate::providers::PlaybackRequest;
use bytes::Bytes;
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

#[derive(Clone)]
pub(crate) struct Preparation {
    deadline: Instant,
    ready: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
}
impl Preparation {
    #[cfg(target_os = "linux")]
    pub fn deadline(&self) -> Instant {
        self.deadline
    }
    pub fn new(deadline: Instant, cancel: Arc<AtomicBool>) -> Self {
        Self {
            deadline,
            ready: Arc::new(AtomicBool::new(false)),
            cancel,
        }
    }
    pub fn check(&self) -> io::Result<()> {
        if self.cancel.load(Ordering::Acquire) {
            Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "playback cancelled",
            ))
        } else if !self.ready.load(Ordering::Acquire) && Instant::now() >= self.deadline {
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "playback preparation timed out",
            ))
        } else {
            Ok(())
        }
    }
    pub fn ready(&self) {
        self.ready.store(true, Ordering::Release);
    }
    pub async fn run<T>(
        &self,
        future: impl std::future::Future<Output = io::Result<T>>,
    ) -> io::Result<T> {
        tokio::pin!(future);
        let stalled = tokio::time::sleep(Duration::from_secs(15));
        tokio::pin!(stalled);
        loop {
            self.check()?;
            tokio::select! {
                result = &mut future => return result,
                _ = &mut stalled => return Err(io::Error::new(io::ErrorKind::TimedOut, "source made no byte progress")),
                _ = tokio::time::sleep(Duration::from_millis(5)) => {},
            }
        }
    }
}

pub(crate) struct HttpSource {
    runtime: tokio::runtime::Handle,
    request: PlaybackRequest,
    response: Option<reqwest::Response>,
    pending: Bytes,
    pending_allocation: usize,
    position: u64,
    length: Option<u64>,
    validator: Option<reqwest::header::HeaderValue>,
    preparation: Preparation,
}
impl HttpSource {
    pub fn new(
        request: PlaybackRequest,
        response: reqwest::Response,
        preparation: Preparation,
    ) -> Self {
        let validator = response
            .headers()
            .get(reqwest::header::ETAG)
            .filter(|value| !value.as_bytes().starts_with(b"W/"))
            .or_else(|| response.headers().get(reqwest::header::LAST_MODIFIED))
            .cloned();
        Self {
            runtime: tokio::runtime::Handle::current(),
            request,
            length: response.content_length(),
            validator,
            response: Some(response),
            pending: Bytes::new(),
            pending_allocation: 0,
            position: 0,
            preparation,
        }
    }
}
impl Read for HttpSource {
    fn read(&mut self, target: &mut [u8]) -> io::Result<usize> {
        self.preparation.check()?;
        if target.is_empty() {
            return Ok(0);
        }
        if self.pending.is_empty() {
            self.pending_allocation = 0;
            let Some(response) = self.response.as_mut() else {
                return Ok(0);
            };
            let chunk = self.runtime.block_on(self.preparation.run(async {
                response
                    .chunk()
                    .await
                    .map_err(|_| io::Error::other("source read failed"))
            }))?;
            let Some(chunk) = chunk else {
                if self.length.is_some_and(|length| self.position < length) {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "source body truncated",
                    ));
                }
                self.response = None;
                return Ok(0);
            };
            // The retained network chunk has an explicit share of the byte
            // budget. Do not retain an arbitrarily large upstream allocation.
            validate_chunk_length(chunk.len())?;
            self.pending = chunk;
            self.pending_allocation = self.pending.len();
        }
        let count = target.len().min(self.pending.len());
        target[..count].copy_from_slice(&self.pending[..count]);
        self.pending = self.pending.slice(count..);
        if self.pending.is_empty() {
            self.pending = Bytes::new();
            // Keep the allocation observed during this read until the next
            // read starts. The bounded reader samples it after our copy, when
            // the scratch buffer still contributes to the same peak budget.
        }
        self.position += count as u64;
        Ok(count)
    }
}
impl Seek for HttpSource {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        self.preparation.check()?;
        let target = match from {
            SeekFrom::Start(value) => i128::from(value),
            SeekFrom::Current(delta) => i128::from(self.position) + i128::from(delta),
            SeekFrom::End(delta) => {
                i128::from(self.length.ok_or_else(|| {
                    io::Error::new(io::ErrorKind::Unsupported, "source length is unknown")
                })?) + i128::from(delta)
            }
        };
        if target < 0
            || target > i128::from(u64::MAX)
            || self
                .length
                .is_some_and(|length| target > i128::from(length))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid source offset",
            ));
        }
        let target = target as u64;
        if target == self.position {
            return Ok(target);
        }
        // Close the old response first: only one HTTP request owns this source.
        self.response = None;
        self.pending = Bytes::new();
        self.pending_allocation = 0;
        if self.length == Some(target) {
            self.position = target;
            return Ok(target);
        }
        let mut headers = self.request.headers.clone();
        headers.insert(
            reqwest::header::RANGE,
            format!("bytes={target}-").parse().unwrap(),
        );
        headers.insert(
            reqwest::header::ACCEPT_ENCODING,
            reqwest::header::HeaderValue::from_static("identity"),
        );
        if let Some(validator) = &self.validator {
            headers.insert(reqwest::header::IF_RANGE, validator.clone());
        }
        let url = self.request.url.clone();
        let response = self.runtime.block_on(self.preparation.run(async move {
            let client = reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(10))
                .build()
                .map_err(|_| io::Error::other("source client unavailable"))?;
            client
                .get(url)
                .headers(headers)
                .send()
                .await
                .map_err(|_| io::Error::other("source seek failed"))
        }))?;
        let header = response
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|value| value.to_str().ok());
        if response.status() != reqwest::StatusCode::PARTIAL_CONTENT
            || !valid_content_range(header, target, self.length)
            || response
                .headers()
                .get(reqwest::header::CONTENT_ENCODING)
                .is_some_and(|v| v != "identity")
        {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "source did not honor the requested byte range",
            ));
        }
        self.response = Some(response);
        self.position = target;
        Ok(target)
    }
}
impl super::streaming::CompressedSource for HttpSource {
    fn length(&self) -> Option<u64> {
        self.length
    }
    fn retained_bytes(&self) -> usize {
        self.pending_allocation
    }
    fn preparation(&self) -> Option<Preparation> {
        Some(self.preparation.clone())
    }
}
fn validate_chunk_length(length: usize) -> io::Result<()> {
    if length > NETWORK_CHUNK_CAPACITY_BYTES {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "source chunk exceeds playback budget",
        ))
    } else {
        Ok(())
    }
}
fn valid_content_range(header: Option<&str>, offset: u64, length: Option<u64>) -> bool {
    let Some(value) = header.and_then(|value| value.strip_prefix("bytes ")) else {
        return false;
    };
    let Some((range, total)) = value.split_once('/') else {
        return false;
    };
    let Some((start, end)) = range.split_once('-') else {
        return false;
    };
    matches!((start.parse::<u64>(), end.parse::<u64>(), total.parse::<u64>()),
        (Ok(start), Ok(end), Ok(total)) if start == offset && start <= end && end < total && length == Some(total))
}

#[cfg(test)]
mod tests {
    use super::super::streaming::{BoundedHttpReader, COMPRESSED_CHUNK_BYTES};
    use super::*;
    use std::sync::atomic::AtomicU64;

    #[tokio::test]
    async fn size_probes_preserve_non_range_http_response_and_cached_bytes() {
        let mut server = mockito::Server::new_async().await;
        let payload: Vec<u8> = (0..3 * COMPRESSED_CHUNK_BYTES)
            .map(|i| (i % 251) as u8)
            .collect();
        let original = server
            .mock("GET", "/audio")
            .match_header("range", mockito::Matcher::Missing)
            .with_body(payload.clone())
            .expect(1)
            .create_async()
            .await;
        let url = reqwest::Url::parse(&format!("{}/audio", server.url())).unwrap();
        let response = reqwest::get(url.clone()).await.unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let source = HttpSource::new(
            PlaybackRequest {
                url,
                headers: Default::default(),
                range_supported: false,
            },
            response,
            Preparation::new(Instant::now() + Duration::from_secs(60), cancel.clone()),
        );
        let mut reader =
            BoundedHttpReader::from_source(source, cancel, Arc::new(AtomicU64::new(0)));
        tokio::task::spawn_blocking(move || {
            // Probe before any bytes have entered the cache, including an EOF
            // read before FFmpeg restores its original position.
            assert_eq!(reader.seek(SeekFrom::End(0)).unwrap(), payload.len() as u64);
            assert_eq!(reader.read(&mut [0; 1]).unwrap(), 0);
            reader.seek(SeekFrom::Start(0)).unwrap();
            let mut prefix = [0; 17];
            reader.read_exact(&mut prefix).unwrap();
            assert_eq!(prefix, payload[..17]);
            let position = reader.stream_position().unwrap();
            assert_eq!(reader.seek(SeekFrom::End(0)).unwrap(), payload.len() as u64);
            reader.seek(SeekFrom::Start(position)).unwrap();
            let mut remainder = Vec::new();
            reader.read_to_end(&mut remainder).unwrap();
            assert_eq!(remainder, payload[17..]);
            assert!(reader.failure_state().error().is_none());
        })
        .await
        .unwrap();
        original.assert_async().await;
    }

    #[tokio::test]
    async fn fast_large_http_response_accepts_transport_chunks_above_read_scratch() {
        let mut server = mockito::Server::new_async().await;
        let payload = vec![0x5a; 4 * 1024 * 1024];
        let original = server
            .mock("GET", "/large")
            .with_body(payload.clone())
            .create_async()
            .await;
        let url = reqwest::Url::parse(&format!("{}/large", server.url())).unwrap();
        let response = reqwest::get(url.clone()).await.unwrap();
        let mut source = HttpSource::new(
            PlaybackRequest {
                url,
                headers: Default::default(),
                range_supported: false,
            },
            response,
            Preparation::new(
                Instant::now() + Duration::from_secs(60),
                Arc::new(AtomicBool::new(false)),
            ),
        );
        tokio::task::spawn_blocking(move || {
            let mut scratch = vec![0; COMPRESSED_CHUNK_BYTES];
            let mut received = 0;
            let mut largest_chunk = 0;
            loop {
                let count = source.read(&mut scratch).unwrap();
                largest_chunk = largest_chunk.max(source.pending_allocation);
                if count == 0 {
                    break;
                }
                assert!(
                    source.pending_allocation >= count,
                    "last-read transport allocation must remain visible to telemetry"
                );
                assert!(scratch[..count].iter().all(|byte| *byte == 0x5a));
                received += count;
            }
            assert_eq!(received, payload.len());
            assert!(largest_chunk <= NETWORK_CHUNK_CAPACITY_BYTES);
        })
        .await
        .unwrap();
        original.assert_async().await;
    }
    #[test]
    fn transport_chunk_budget_accepts_larger_than_read_scratch() {
        assert!(validate_chunk_length(2 * COMPRESSED_CHUNK_BYTES).is_ok());
        assert!(validate_chunk_length(NETWORK_CHUNK_CAPACITY_BYTES).is_ok());
        assert_eq!(
            validate_chunk_length(NETWORK_CHUNK_CAPACITY_BYTES + 1)
                .unwrap_err()
                .kind(),
            io::ErrorKind::Unsupported
        );
    }
    #[test]
    fn range_validation_rejects_wrong_offsets_and_changed_lengths() {
        assert!(valid_content_range(
            Some("bytes 100-199/200"),
            100,
            Some(200)
        ));
        for header in [
            "bytes 0-199/200",
            "bytes 100-199/201",
            "bytes 100-200/200",
            "bytes */200",
        ] {
            assert!(!valid_content_range(Some(header), 100, Some(200)));
        }
    }
    #[tokio::test]
    async fn trickle_cannot_extend_the_preparation_deadline() {
        let prep = Preparation::new(
            Instant::now() + Duration::from_millis(20),
            Arc::new(AtomicBool::new(false)),
        );
        let result = prep
            .run(async {
                tokio::time::sleep(Duration::from_millis(100)).await;
                Ok(())
            })
            .await;
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::TimedOut);
        prep.ready();
        assert!(prep.check().is_ok());
    }

    #[tokio::test]
    async fn verified_range_reads_preserve_source_bytes() {
        let mut server = mockito::Server::new_async().await;
        let original = server
            .mock("GET", "/audio")
            .match_header("range", mockito::Matcher::Missing)
            .with_body("abcdef")
            .create_async()
            .await;
        let range = server
            .mock("GET", "/audio")
            .match_header("range", "bytes=4-")
            .match_header("authorization", "Bearer test")
            .with_status(206)
            .with_header("content-range", "bytes 4-5/6")
            .with_body("ef")
            .create_async()
            .await;
        let url = reqwest::Url::parse(&format!("{}/audio", server.url())).unwrap();
        let response = reqwest::get(url.clone()).await.unwrap();
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::AUTHORIZATION,
            "Bearer test".parse().unwrap(),
        );
        let mut source = HttpSource::new(
            PlaybackRequest {
                url,
                headers,
                range_supported: false,
            },
            response,
            Preparation::new(
                Instant::now() + Duration::from_secs(60),
                Arc::new(AtomicBool::new(false)),
            ),
        );
        tokio::task::spawn_blocking(move || {
            let mut bytes = [0; 2];
            source.read_exact(&mut bytes).unwrap();
            assert_eq!(&bytes, b"ab");
            source.seek(SeekFrom::Start(4)).unwrap();
            source.read_exact(&mut bytes).unwrap();
            assert_eq!(&bytes, b"ef");
        })
        .await
        .unwrap();
        original.assert_async().await;
        range.assert_async().await;
    }

    #[tokio::test]
    async fn ignored_range_is_explicitly_unsupported() {
        let mut server = mockito::Server::new_async().await;
        let original = server
            .mock("GET", "/audio")
            .match_header("range", mockito::Matcher::Missing)
            .with_body("abcdef")
            .create_async()
            .await;
        let range = server
            .mock("GET", "/audio")
            .match_header("range", "bytes=4-")
            .with_body("abcdef")
            .create_async()
            .await;
        let url = reqwest::Url::parse(&format!("{}/audio", server.url())).unwrap();
        let response = reqwest::get(url.clone()).await.unwrap();
        let mut source = HttpSource::new(
            PlaybackRequest {
                url,
                headers: Default::default(),
                range_supported: false,
            },
            response,
            Preparation::new(
                Instant::now() + Duration::from_secs(60),
                Arc::new(AtomicBool::new(false)),
            ),
        );
        tokio::task::spawn_blocking(move || {
            assert_eq!(
                source.seek(SeekFrom::Start(4)).unwrap_err().kind(),
                io::ErrorKind::Unsupported
            );
        })
        .await
        .unwrap();
        original.assert_async().await;
        range.assert_async().await;
    }
}
