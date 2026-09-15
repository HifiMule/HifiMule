use bytes::Bytes;
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};

pub const COMPRESSED_CHUNK_BYTES: usize = 64 * 1024;
pub const COMPRESSED_CAPACITY_BYTES: usize = 8 * 1024 * 1024;
// Reserve both a decoder read chunk and the upstream HTTP chunk allocation.
// Hyper's adaptive HTTP/1 read buffer can exceed 64 KiB. Reserve its
// retained chunk separately from our fixed read scratch and seek window.
pub const NETWORK_CHUNK_CAPACITY_BYTES: usize = 1024 * 1024;
pub const COMPRESSED_WINDOW_BYTES: usize =
    COMPRESSED_CAPACITY_BYTES - NETWORK_CHUNK_CAPACITY_BYTES - COMPRESSED_CHUNK_BYTES;

pub trait CompressedSource: Read + Seek + Send {
    fn length(&self) -> Option<u64> {
        None
    }
    fn retained_bytes(&self) -> usize {
        0
    }
    fn preparation(&self) -> Option<super::http_source::Preparation> {
        None
    }
}
impl CompressedSource for std::fs::File {
    fn length(&self) -> Option<u64> {
        self.metadata().ok().map(|metadata| metadata.len())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamReadError {
    Timeout,
    Unsupported,
    Source(String),
}

impl StreamReadError {
    fn kind(&self) -> StreamFailureKind {
        match self {
            Self::Timeout => StreamFailureKind::Timeout,
            Self::Unsupported => StreamFailureKind::Unsupported,
            Self::Source(_) => StreamFailureKind::Source,
        }
    }
}

impl From<String> for StreamReadError {
    fn from(message: String) -> Self {
        Self::Source(message)
    }
}

impl From<&str> for StreamReadError {
    fn from(message: &str) -> Self {
        Self::Source(message.to_owned())
    }
}

impl std::fmt::Display for StreamReadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout => write!(formatter, "source made no byte progress"),
            Self::Unsupported => write!(formatter, "source cannot provide bounded random access"),
            Self::Source(message) => message.fmt(formatter),
        }
    }
}

impl std::error::Error for StreamReadError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamFailureKind {
    Timeout = 1,
    Source = 2,
    Unsupported = 3,
}

#[derive(Clone, Default)]
pub struct StreamFailureState {
    kind: Arc<AtomicU8>,
    error: Arc<std::sync::Mutex<Option<StreamReadError>>>,
}

impl StreamFailureState {
    fn record_io(&self, error: &io::Error) {
        self.record(&match error.kind() {
            io::ErrorKind::TimedOut => StreamReadError::Timeout,
            io::ErrorKind::Unsupported => StreamReadError::Unsupported,
            _ => StreamReadError::Source(error.to_string()),
        });
    }
    fn record(&self, error: &StreamReadError) {
        *self
            .error
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(error.clone());
        self.kind.store(error.kind() as u8, Ordering::Release);
    }

    #[cfg(test)]
    pub fn kind(&self) -> Option<StreamFailureKind> {
        match self.kind.load(Ordering::Acquire) {
            1 => Some(StreamFailureKind::Timeout),
            2 => Some(StreamFailureKind::Source),
            3 => Some(StreamFailureKind::Unsupported),
            _ => None,
        }
    }

    pub fn error(&self) -> Option<StreamReadError> {
        self.error
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

pub struct BoundedHttpReader {
    rx: tokio::sync::mpsc::Receiver<Result<Bytes, StreamReadError>>,
    buffer: Vec<u8>,
    base: u64,
    position: u64,
    eof: bool,
    cancel: Arc<AtomicBool>,
    high_water: Arc<AtomicU64>,
    failure: StreamFailureState,
    source: Option<Box<dyn CompressedSource>>,
}

impl BoundedHttpReader {
    #[cfg(test)]
    pub fn channel(
        cancel: Arc<AtomicBool>,
    ) -> (
        tokio::sync::mpsc::Sender<Result<Bytes, StreamReadError>>,
        Self,
    ) {
        Self::channel_with_high_water(cancel, Arc::new(AtomicU64::new(0)))
    }

    #[cfg(test)]
    pub fn channel_with_high_water(
        cancel: Arc<AtomicBool>,
        high_water: Arc<AtomicU64>,
    ) -> (
        tokio::sync::mpsc::Sender<Result<Bytes, StreamReadError>>,
        Self,
    ) {
        let (tx, rx) =
            tokio::sync::mpsc::channel(COMPRESSED_CAPACITY_BYTES / COMPRESSED_CHUNK_BYTES);
        (
            tx,
            Self {
                rx,
                buffer: Vec::new(),
                base: 0,
                position: 0,
                eof: false,
                cancel,
                high_water,
                failure: StreamFailureState::default(),
                source: None,
            },
        )
    }

    pub fn from_source(
        source: impl CompressedSource + 'static,
        cancel: Arc<AtomicBool>,
        high_water: Arc<AtomicU64>,
    ) -> Self {
        let (_, rx) = tokio::sync::mpsc::channel(1);
        Self {
            rx,
            buffer: Vec::with_capacity(COMPRESSED_WINDOW_BYTES),
            base: 0,
            position: 0,
            eof: false,
            cancel,
            high_water,
            failure: StreamFailureState::default(),
            source: Some(Box::new(source)),
        }
    }

    pub fn failure_state(&self) -> StreamFailureState {
        self.failure.clone()
    }

    pub(crate) fn preparation(&self) -> Option<super::http_source::Preparation> {
        self.source.as_ref().and_then(|source| source.preparation())
    }

    fn position_source_for_read(&mut self) -> io::Result<()> {
        let end = self.base + self.buffer.len() as u64;
        if self.position < self.base || self.position > end {
            let source = self.source.as_mut().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::Unsupported,
                    "seek outside bounded compressed window",
                )
            })?;
            source
                .seek(SeekFrom::Start(self.position))
                .inspect_err(|error| self.failure.record_io(error))?;
            self.buffer.clear();
            self.base = self.position;
            self.eof = false;
        }
        Ok(())
    }

    fn at_known_end(&self) -> bool {
        self.source.as_ref().and_then(|source| source.length()) == Some(self.position)
    }

    fn receive_next(&mut self) -> io::Result<()> {
        loop {
            if self.cancel.load(Ordering::Acquire) {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "playback generation cancelled",
                ));
            }
            let next = if let Some(source) = self.source.as_mut() {
                let mut bytes = vec![0u8; COMPRESSED_CHUNK_BYTES];
                match source.read(&mut bytes) {
                    Ok(0) => Err(tokio::sync::mpsc::error::TryRecvError::Disconnected),
                    Ok(count) => {
                        self.high_water.fetch_max(
                            (self.buffer.len() + bytes.capacity() + source.retained_bytes()) as u64,
                            Ordering::AcqRel,
                        );
                        bytes.truncate(count);
                        Ok(Ok(Bytes::from(bytes)))
                    }
                    Err(error) => {
                        self.failure.record_io(&error);
                        return Err(error);
                    }
                }
            } else {
                self.rx.try_recv()
            };
            match next {
                Ok(Ok(bytes)) => {
                    let overflow = self
                        .buffer
                        .len()
                        .saturating_add(bytes.len())
                        .saturating_sub(COMPRESSED_WINDOW_BYTES);
                    if overflow > 0 {
                        let consumed =
                            usize::try_from(self.position - self.base).unwrap_or(usize::MAX);
                        if overflow > consumed {
                            return Err(io::Error::new(
                                io::ErrorKind::Unsupported,
                                "random-access source exceeds bounded compressed window",
                            ));
                        }
                        self.buffer.drain(..overflow);
                        self.base += overflow as u64;
                    }
                    self.buffer.extend_from_slice(&bytes);
                    self.high_water
                        .fetch_max(self.buffer.len() as u64, Ordering::AcqRel);
                    return Ok(());
                }
                Ok(Err(error)) => {
                    self.failure.record(&error);
                    return Err(io::Error::other(error));
                }
                Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                    self.eof = true;
                    return Ok(());
                }
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
            }
        }
    }

    pub fn peek_prefix(&mut self, count: usize) -> io::Result<Vec<u8>> {
        if count == 0 || self.at_known_end() {
            return Ok(Vec::new());
        }
        self.position_source_for_read()?;
        while self
            .buffer
            .len()
            .saturating_sub((self.position - self.base) as usize)
            < count
            && !self.eof
        {
            self.receive_next()?;
        }
        let offset = usize::try_from(self.position - self.base)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "stream offset overflow"))?;
        let end = offset.saturating_add(count).min(self.buffer.len());
        Ok(self.buffer[offset..end].to_vec())
    }
}

impl Read for BoundedHttpReader {
    fn read(&mut self, target: &mut [u8]) -> io::Result<usize> {
        if target.is_empty() || self.at_known_end() {
            return Ok(0);
        }
        self.position_source_for_read()?;
        while self.position == self.base + self.buffer.len() as u64 && !self.eof {
            self.receive_next()?;
        }
        let offset = usize::try_from(self.position - self.base)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "stream offset overflow"))?;
        if offset >= self.buffer.len() {
            return Ok(0);
        }
        let count = target.len().min(self.buffer.len() - offset);
        target[..count].copy_from_slice(&self.buffer[offset..offset + count]);
        self.position += count as u64;
        Ok(count)
    }
}

impl Seek for BoundedHttpReader {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        if let Some(source) = self.source.as_ref() {
            let length = source.length();
            let target = match from {
                SeekFrom::Start(value) => i128::from(value),
                SeekFrom::Current(delta) => i128::from(self.position) + i128::from(delta),
                SeekFrom::End(delta) => {
                    i128::from(length.ok_or_else(|| {
                        // A demuxer's optional size probe is not a failed body read.
                        io::Error::new(io::ErrorKind::Unsupported, "source length is unknown")
                    })?) + i128::from(delta)
                }
            };
            if target < 0
                || target > i128::from(u64::MAX)
                || length.is_some_and(|length| target > i128::from(length))
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "invalid source offset",
                ));
            }
            // FFmpeg probes size with End(0), then restores the cursor. Keep
            // cached bytes and the live response until an actual read requires
            // repositioning, so a size query never requires HTTP Range support.
            self.position = target as u64;
            return Ok(self.position);
        }
        if matches!(from, SeekFrom::End(_)) {
            // A small finite test/sequential source can establish its length
            // inside the window; never evict data just to discover an end.
            while !self.eof {
                if self.buffer.len() >= COMPRESSED_WINDOW_BYTES {
                    return Err(io::Error::new(
                        io::ErrorKind::Unsupported,
                        "source length exceeds bounded window",
                    ));
                }
                self.receive_next()?;
            }
        }
        let end = self.base + self.buffer.len() as u64;
        let target = match from {
            SeekFrom::Start(value) => i128::from(value),
            SeekFrom::Current(delta) => i128::from(self.position) + i128::from(delta),
            SeekFrom::End(delta) => i128::from(end) + i128::from(delta),
        };
        if target < i128::from(self.base) || target > i128::from(end) {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "seek outside bounded compressed window",
            ));
        }
        self.position = target as u64;
        Ok(self.position)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_source_size_probe_is_advisory_and_preserves_reads() {
        struct UnknownLength(std::io::Cursor<Vec<u8>>);
        impl Read for UnknownLength {
            fn read(&mut self, target: &mut [u8]) -> io::Result<usize> {
                self.0.read(target)
            }
        }
        impl Seek for UnknownLength {
            fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
                self.0.seek(from)
            }
        }
        impl CompressedSource for UnknownLength {}
        let mut reader = BoundedHttpReader::from_source(
            UnknownLength(std::io::Cursor::new(b"abcdef".to_vec())),
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicU64::new(0)),
        );
        assert_eq!(
            reader.seek(SeekFrom::End(0)).unwrap_err().kind(),
            io::ErrorKind::Unsupported
        );
        assert!(reader.failure_state().error().is_none());
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"abcdef");
    }

    #[test]
    fn reading_virtual_end_offset_repositions_source_only_when_needed() {
        struct KnownLength(std::io::Cursor<Vec<u8>>);
        impl Read for KnownLength {
            fn read(&mut self, target: &mut [u8]) -> io::Result<usize> {
                self.0.read(target)
            }
        }
        impl Seek for KnownLength {
            fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
                self.0.seek(from)
            }
        }
        impl CompressedSource for KnownLength {
            fn length(&self) -> Option<u64> {
                Some(self.0.get_ref().len() as u64)
            }
        }
        let mut reader = BoundedHttpReader::from_source(
            KnownLength(std::io::Cursor::new(b"abcdef".to_vec())),
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicU64::new(0)),
        );
        reader.seek(SeekFrom::End(-2)).unwrap();
        assert_eq!(reader.peek_prefix(2).unwrap(), b"ef");
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"ef");
        reader.seek(SeekFrom::Start(0)).unwrap();
        bytes.clear();
        reader.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"abcdef");
    }
    #[test]
    fn reader_preserves_chunk_order_and_eof() {
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, mut reader) = BoundedHttpReader::channel(cancel);
        tx.blocking_send(Ok(Bytes::from_static(b"ab"))).unwrap();
        tx.blocking_send(Ok(Bytes::from_static(b"cdef"))).unwrap();
        drop(tx);
        let mut result = Vec::new();
        reader.read_to_end(&mut result).unwrap();
        assert_eq!(result, b"abcdef");
    }

    #[test]
    fn peek_preserves_position_and_cancellation_interrupts_open_channel() {
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, mut reader) = BoundedHttpReader::channel(cancel.clone());
        tx.blocking_send(Ok(Bytes::from_static(b"fLaCpayload")))
            .unwrap();
        assert_eq!(reader.peek_prefix(4).unwrap(), b"fLaC");
        let mut bytes = [0; 4];
        reader.read_exact(&mut bytes).unwrap();
        assert_eq!(&bytes, b"fLaC");
        let mut remaining = [0; 16];
        reader.read_exact(&mut remaining[..7]).unwrap();
        cancel.store(true, Ordering::Release);
        assert_eq!(
            reader.read(&mut remaining).unwrap_err().kind(),
            io::ErrorKind::Interrupted
        );
        drop(tx);
    }

    #[test]
    fn sequential_stream_evicts_consumed_bytes_and_rejects_evicted_seek() {
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, mut reader) = BoundedHttpReader::channel(cancel);
        let chunk = Bytes::from(vec![0x5a; COMPRESSED_CHUNK_BYTES]);
        let chunks = COMPRESSED_CAPACITY_BYTES / COMPRESSED_CHUNK_BYTES + 2;
        let producer = std::thread::spawn(move || {
            for _ in 0..chunks {
                tx.blocking_send(Ok(chunk.clone())).unwrap();
            }
        });
        let mut scratch = vec![0; COMPRESSED_CHUNK_BYTES];
        for _ in 0..chunks {
            reader.read_exact(&mut scratch).unwrap();
            assert!(scratch.iter().all(|byte| *byte == 0x5a));
        }
        producer.join().unwrap();
        assert!(reader.base > 0);
        assert!(reader.buffer.len() <= COMPRESSED_CAPACITY_BYTES);
        assert_eq!(
            reader.seek(SeekFrom::Start(0)).unwrap_err().kind(),
            io::ErrorKind::Unsupported
        );
    }

    #[test]
    fn evicted_bytes_require_a_verified_random_access_source() {
        let cancel = Arc::new(AtomicBool::new(false));
        let high_water = Arc::new(AtomicU64::new(0));
        let (tx, mut reader) =
            BoundedHttpReader::channel_with_high_water(cancel, high_water.clone());
        let chunks = COMPRESSED_CAPACITY_BYTES / COMPRESSED_CHUNK_BYTES + 2;
        let producer = std::thread::spawn(move || {
            for index in 0..chunks {
                tx.blocking_send(Ok(Bytes::from(vec![index as u8; COMPRESSED_CHUNK_BYTES])))
                    .unwrap();
            }
            drop(tx);
        });

        let mut scratch = vec![0; COMPRESSED_CHUNK_BYTES];
        for _ in 0..chunks {
            reader.read_exact(&mut scratch).unwrap();
        }
        assert_eq!(
            reader.seek(SeekFrom::Start(0)).unwrap_err().kind(),
            io::ErrorKind::Unsupported
        );

        producer.join().unwrap();
        assert!(reader.buffer.len() <= COMPRESSED_CAPACITY_BYTES);
        assert!(high_water.load(Ordering::Acquire) <= COMPRESSED_CAPACITY_BYTES as u64);
    }

    #[test]
    fn source_timeout_is_preserved_as_a_typed_reader_error() {
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, mut reader) = BoundedHttpReader::channel(cancel);
        tx.blocking_send(Err(StreamReadError::Timeout)).unwrap();
        let mut byte = [0u8; 1];
        let error = reader.read(&mut byte).unwrap_err();
        assert!(matches!(
            error
                .get_ref()
                .and_then(|source| source.downcast_ref::<StreamReadError>()),
            Some(StreamReadError::Timeout)
        ));
    }

    #[test]
    fn source_timeout_updates_shared_failure_state() {
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, mut reader) = BoundedHttpReader::channel(cancel);
        let failure = reader.failure_state();
        tx.blocking_send(Err(StreamReadError::Timeout)).unwrap();
        let mut byte = [0u8; 1];
        let _ = reader.read(&mut byte);
        assert_eq!(failure.kind(), Some(StreamFailureKind::Timeout));
    }

    #[test]
    fn source_failure_state_retains_native_error_detail() {
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, mut reader) = BoundedHttpReader::channel(cancel);
        let failure = reader.failure_state();
        tx.blocking_send(Err(StreamReadError::Source(
            "native transport reset".into(),
        )))
        .unwrap();
        let mut byte = [0u8; 1];
        let _ = reader.read(&mut byte);
        assert_eq!(
            failure.error(),
            Some(StreamReadError::Source("native transport reset".into()))
        );
    }
}
