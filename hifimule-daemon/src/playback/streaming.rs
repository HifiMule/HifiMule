use bytes::Bytes;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};

pub const COMPRESSED_CHUNK_BYTES: usize = 64 * 1024;
pub const COMPRESSED_CAPACITY_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamReadError {
    Timeout,
    Source(String),
}

impl StreamReadError {
    fn kind(&self) -> StreamFailureKind {
        match self {
            Self::Timeout => StreamFailureKind::Timeout,
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
            Self::Source(message) => message.fmt(formatter),
        }
    }
}

impl std::error::Error for StreamReadError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamFailureKind {
    Timeout = 1,
    Source = 2,
}

#[derive(Clone, Default)]
pub struct StreamFailureState {
    kind: Arc<AtomicU8>,
    error: Arc<std::sync::Mutex<Option<StreamReadError>>>,
}

impl StreamFailureState {
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
    seek_history: Option<std::fs::File>,
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
                seek_history: None,
            },
        )
    }

    pub fn enable_seek_history(&mut self) -> io::Result<()> {
        if self.seek_history.is_none() {
            self.seek_history = Some(tempfile::tempfile()?);
        }
        Ok(())
    }

    pub fn failure_state(&self) -> StreamFailureState {
        self.failure.clone()
    }

    fn receive_next(&mut self) -> io::Result<()> {
        loop {
            if self.cancel.load(Ordering::Acquire) {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "playback generation cancelled",
                ));
            }
            match self.rx.try_recv() {
                Ok(Ok(bytes)) => {
                    let overflow = self
                        .buffer
                        .len()
                        .saturating_add(bytes.len())
                        .saturating_sub(COMPRESSED_CAPACITY_BYTES);
                    if overflow > 0 {
                        let consumed =
                            usize::try_from(self.position - self.base).unwrap_or(usize::MAX);
                        if overflow > consumed {
                            return Err(io::Error::new(
                                io::ErrorKind::Unsupported,
                                "random-access source exceeds bounded compressed window",
                            ));
                        }
                        if let Some(history) = self.seek_history.as_mut() {
                            history.seek(SeekFrom::Start(self.base))?;
                            history.write_all(&self.buffer[..overflow])?;
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
        if self.position < self.base {
            let history = self.seek_history.as_mut().ok_or_else(|| {
                io::Error::new(io::ErrorKind::Unsupported, "seek history is unavailable")
            })?;
            history.seek(SeekFrom::Start(self.position))?;
            let available = usize::try_from(self.base - self.position).unwrap_or(usize::MAX);
            let requested = target.len().min(available);
            let count = history.read(&mut target[..requested])?;
            self.position += count as u64;
            return Ok(count);
        }
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
        if matches!(from, SeekFrom::End(_)) && !self.eof {
            let mut scratch = [0u8; COMPRESSED_CHUNK_BYTES];
            while !self.eof {
                let _ = self.read(&mut scratch)?;
            }
        }
        let end = self.base + self.buffer.len() as u64;
        let target = match from {
            SeekFrom::Start(value) => i128::from(value),
            SeekFrom::Current(delta) => i128::from(self.position) + i128::from(delta),
            SeekFrom::End(delta) => i128::from(end) + i128::from(delta),
        };
        let earliest = if self.seek_history.is_some() {
            0
        } else {
            self.base
        };
        if target < i128::from(earliest) || target > i128::from(end) {
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
    fn seek_history_restores_evicted_bytes_without_growing_memory_window() {
        let cancel = Arc::new(AtomicBool::new(false));
        let high_water = Arc::new(AtomicU64::new(0));
        let (tx, mut reader) =
            BoundedHttpReader::channel_with_high_water(cancel, high_water.clone());
        reader.enable_seek_history().unwrap();
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
        reader.seek(SeekFrom::Start(0)).unwrap();
        reader.read_exact(&mut scratch).unwrap();

        producer.join().unwrap();
        assert!(scratch.iter().all(|byte| *byte == 0));
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
