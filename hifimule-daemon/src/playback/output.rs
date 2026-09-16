//! Allocation-free PCM consumption and owned decoder retirement.
use crossbeam_queue::ArrayQueue;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::{Duration, Instant};

/// Bounded presentation ledger for backends whose write callback precedes
/// physical playback. Silence and gaps never contribute to logical position.
#[cfg(any(target_os = "linux", test))]
pub struct PresentationLedger {
    pending: std::collections::VecDeque<(u64, u64)>,
    presented: u64,
}
#[cfg(any(target_os = "linux", test))]
impl PresentationLedger {
    pub fn new() -> Self {
        Self {
            pending: std::collections::VecDeque::new(),
            presented: 0,
        }
    }
    pub fn record(&mut self, start: u64, frames: u64) -> bool {
        if frames == 0 {
            return true;
        }
        if self.pending.len() == 256 {
            return false;
        }
        self.pending
            .push_back((start, start.saturating_add(frames)));
        true
    }
    pub fn advance(&mut self, played: u64) -> u64 {
        while let Some((start, end)) = self.pending.front_mut() {
            if played <= *start {
                break;
            }
            let next = played.min(*end);
            self.presented = self.presented.saturating_add(next - *start);
            *start = next;
            if next == *end {
                self.pending.pop_front();
            } else {
                break;
            }
        }
        self.presented
    }
}

/// Emit meaningful server-presentation transitions, never one event per tick.
#[cfg(any(target_os = "linux", test))]
#[derive(Default)]
pub struct PresentationActivity {
    frames: u64,
    last_progress_ms: u64,
    state: Option<bool>,
}
#[cfg(any(target_os = "linux", test))]
impl PresentationActivity {
    pub fn observe(&mut self, enabled: bool, frames: u64, now_ms: u64) -> Option<bool> {
        let advanced = frames > self.frames;
        self.frames = frames;
        if !enabled {
            self.state = None;
            self.last_progress_ms = now_ms;
            return None;
        }
        let next = if advanced {
            self.last_progress_ms = now_ms;
            Some(true)
        } else if now_ms.saturating_sub(self.last_progress_ms) >= 100 {
            Some(false)
        } else {
            None
        };
        if next.is_some() && next != self.state {
            self.state = next;
            next
        } else {
            None
        }
    }
}

pub struct Rendered {
    pub samples: u64,
    pub last_audio_frame: Option<usize>,
}

pub struct PcmConsumer {
    channels: usize,
    refill: usize,
    buffering: bool,
}

impl PcmConsumer {
    pub fn new(channels: usize, refill: usize) -> Self {
        Self {
            channels,
            refill,
            buffering: true,
        }
    }

    pub fn render<T: cpal::Sample + cpal::FromSample<f32>>(
        &mut self,
        output: &mut [T],
        pcm: &ArrayQueue<f32>,
        enabled: bool,
        finished: bool,
    ) -> Rendered {
        let mut result = Rendered {
            samples: 0,
            last_audio_frame: None,
        };
        for (index, frame) in output.chunks_mut(self.channels).enumerate() {
            if self.buffering
                && (pcm.len() >= self.refill || finished && pcm.len() >= self.channels)
            {
                self.buffering = false;
            }
            let ready = enabled
                && !self.buffering
                && frame.len() == self.channels
                && pcm.len() >= self.channels;
            if ready {
                // One consumer: a full frame observed above cannot be removed
                // by the producer. Never consume half of an interleaved frame.
                for target in frame {
                    *target = T::from_sample(pcm.pop().unwrap_or(0.0));
                }
                result.samples += self.channels as u64;
                result.last_audio_frame = Some(index + 1);
            } else {
                frame.fill(T::from_sample(0.0));
                if enabled {
                    self.buffering = true;
                }
            }
        }
        result
    }
}

pub struct PresentationClock {
    origin: Instant,
    until_ns: AtomicU64,
    callback_sequence: AtomicU64,
}
impl PresentationClock {
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
            until_ns: AtomicU64::new(0),
            callback_sequence: AtomicU64::new(0),
        }
    }
    pub fn now_ns(&self) -> u64 {
        self.origin.elapsed().as_nanos().min(u64::MAX as u128) as u64
    }
    pub fn submit(&self, now_ns: u64, latency: Duration, frame_end: usize, rate: u32) {
        let duration = (frame_end as u64).saturating_mul(1_000_000_000) / u64::from(rate);
        let deadline = now_ns
            .saturating_add(latency.as_nanos().min(u64::MAX as u128) as u64)
            .saturating_add(duration);
        self.until_ns.fetch_max(deadline, Ordering::Release);
    }
    pub fn drained_at(&self, now_ns: u64) -> bool {
        now_ns >= self.until_ns.load(Ordering::Acquire)
    }
    pub fn begin_callback(&self) {
        self.callback_sequence.fetch_add(1, Ordering::SeqCst);
    }
    pub fn end_callback(&self) {
        self.callback_sequence.fetch_add(1, Ordering::SeqCst);
    }
    pub fn drained_when(&self, empty: impl FnOnce() -> bool) -> bool {
        let before = self.callback_sequence.load(Ordering::SeqCst);
        before.is_multiple_of(2)
            && empty()
            && self.drained_at(self.now_ns())
            && before == self.callback_sequence.load(Ordering::SeqCst)
    }
}

pub struct DecoderWorker<T> {
    handle: Option<std::thread::JoinHandle<T>>,
    cancel: Arc<AtomicBool>,
    pub finished: Arc<AtomicBool>,
}
impl<T: Send + 'static> DecoderWorker<T> {
    pub fn spawn(cancel: Arc<AtomicBool>, work: impl FnOnce() -> T + Send + 'static) -> Self {
        let finished = Arc::new(AtomicBool::new(false));
        let done = finished.clone();
        let handle = std::thread::spawn(move || {
            struct Finished(Arc<AtomicBool>);
            impl Drop for Finished {
                fn drop(&mut self) {
                    self.0.store(true, Ordering::Release);
                }
            }
            let _finished = Finished(done);
            work()
        });
        Self {
            handle: Some(handle),
            cancel,
            finished,
        }
    }
    pub fn is_finished(&self) -> bool {
        self.handle
            .as_ref()
            .is_none_or(|handle| handle.is_finished())
    }
    pub fn join(mut self) -> std::thread::Result<T> {
        self.handle.take().expect("owned decoder").join()
    }
}
impl<T> Drop for DecoderWorker<T> {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            self.cancel.store(true, Ordering::Release);
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn server_activity_coalesces_ticks_and_reports_refill_without_reviving_paused_audio() {
        let mut state = PresentationActivity::default();
        assert_eq!(state.observe(true, 0, 0), None);
        assert_eq!(state.observe(true, 480, 10), Some(true));
        assert_eq!(state.observe(true, 960, 20), None);
        assert_eq!(state.observe(true, 960, 119), None);
        assert_eq!(state.observe(true, 960, 120), Some(false));
        assert_eq!(state.observe(true, 960, 130), None);
        assert_eq!(state.observe(true, 1440, 140), Some(true));
        assert_eq!(state.observe(false, 1920, 150), None);
        assert_eq!(state.observe(false, 1920, 500), None);
        assert_eq!(state.observe(true, 1920, 501), None);
        assert_eq!(state.observe(true, 2400, 510), Some(true));
    }

    #[test]
    fn server_clock_excludes_queued_frames_and_silence_and_remains_bounded() {
        let mut ledger = PresentationLedger::new();
        assert!(ledger.record(100, 100));
        assert!(ledger.record(300, 100));
        assert_eq!(ledger.advance(150), 50);
        assert_eq!(ledger.advance(250), 100);
        assert_eq!(ledger.advance(350), 150);
        assert_eq!(ledger.advance(100), 150);
        assert_eq!(ledger.advance(1000), 200);
        for n in 0..256 {
            assert!(ledger.record(1000 + n, 1));
        }
        assert!(!ledger.record(2000, 1));
    }
    #[test]
    fn completion_waits_for_callback_to_publish_final_deadline() {
        let clock = PresentationClock::new();
        clock.begin_callback();
        assert!(!clock.drained_when(|| true));
        clock.submit(clock.now_ns(), Duration::from_secs(1), 480, 48000);
        clock.end_callback();
        assert!(!clock.drained_when(|| true));
        let clock = PresentationClock::new();
        assert!(!clock.drained_when(|| {
            clock.begin_callback();
            clock.end_callback();
            true
        }));
        assert!(clock.drained_when(|| true));
    }
    #[test]
    fn underrun_never_splits_stereo_and_waits_for_refill() {
        let pcm = ArrayQueue::new(8);
        let mut consumer = PcmConsumer::new(2, 4);
        pcm.push(1.0).unwrap();
        let mut output = [9.0f32; 2];
        consumer.render(&mut output, &pcm, true, false);
        assert_eq!(output, [0.0, 0.0]);
        assert_eq!(pcm.len(), 1);
        for sample in [2.0, 3.0, 4.0] {
            pcm.push(sample).unwrap();
        }
        let mut output = [0.0f32; 6];
        assert_eq!(consumer.render(&mut output, &pcm, true, false).samples, 4);
        assert_eq!(output, [1.0, 2.0, 3.0, 4.0, 0.0, 0.0]);
        pcm.push(5.0).unwrap();
        pcm.push(6.0).unwrap();
        consumer.render(&mut output, &pcm, true, false);
        assert_eq!(pcm.len(), 2);
        consumer.render(&mut output, &pcm, true, true);
        assert_eq!(output[..2], [5.0, 6.0]);
    }
    #[test]
    fn last_buffer_waits_for_backend_latency_and_frame_tail() {
        let clock = PresentationClock::new();
        clock.submit(1_000_000, Duration::from_millis(20), 480, 48_000);
        assert!(!clock.drained_at(30_999_999));
        assert!(clock.drained_at(31_000_000));
    }
    #[test]
    fn cancellation_drop_joins_decoder_before_returning() {
        let cancel = Arc::new(AtomicBool::new(false));
        let observed = cancel.clone();
        let retired = Arc::new(AtomicBool::new(false));
        let done = retired.clone();
        let worker = DecoderWorker::spawn(cancel, move || {
            while !observed.load(Ordering::Acquire) {
                std::thread::yield_now();
            }
            done.store(true, Ordering::Release);
        });
        drop(worker);
        assert!(retired.load(Ordering::Acquire));
    }
}
