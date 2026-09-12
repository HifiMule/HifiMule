use bytes::Bytes;
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub const COMPRESSED_CHUNK_BYTES: usize = 64 * 1024;
pub const COMPRESSED_CAPACITY_BYTES: usize = 8 * 1024 * 1024;

pub struct BoundedHttpReader {
    rx: tokio::sync::mpsc::Receiver<Result<Bytes, String>>,
    buffer: Vec<u8>,
    base: u64,
    position: u64,
    eof: bool,
    cancel: Arc<AtomicBool>,
    high_water: Arc<AtomicU64>,
}

impl BoundedHttpReader {
    #[cfg(test)]
    pub fn channel(
        cancel: Arc<AtomicBool>,
    ) -> (tokio::sync::mpsc::Sender<Result<Bytes, String>>, Self) {
        Self::channel_with_high_water(cancel, Arc::new(AtomicU64::new(0)))
    }

    pub fn channel_with_high_water(
        cancel: Arc<AtomicBool>,
        high_water: Arc<AtomicU64>,
    ) -> (tokio::sync::mpsc::Sender<Result<Bytes, String>>, Self) {
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
            },
        )
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
                        self.buffer.drain(..overflow);
                        self.base += overflow as u64;
                    }
                    self.buffer.extend_from_slice(&bytes);
                    self.high_water
                        .fetch_max(self.buffer.len() as u64, Ordering::AcqRel);
                    return Ok(());
                }
                Ok(Err(message)) => return Err(io::Error::other(message)),
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
}
