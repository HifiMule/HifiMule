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

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SubmittedFrame {
    samples: [f32; 2],
    channels: u8,
    logical_audio: bool,
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
impl SubmittedFrame {
    fn new(samples: [f32; 2], channels: usize, logical_audio: bool) -> Self {
        Self {
            samples,
            channels: channels as u8,
            logical_audio,
        }
    }
}

/// Fixed-capacity history of the newest frames handed to a native backend.
/// The callback is allowed to evict the oldest entry; pause reconciliation
/// fails closed if the backend reports a tail larger than this history.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub struct SubmittedTail {
    frames: ArrayQueue<SubmittedFrame>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub enum TailReconcileError {
    SnapshotExceedsRetainedTail,
    ReplayCapacityExceeded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub struct ReconciledTail {
    pub pending_frames: u64,
    pub logical_audio_frames: u64,
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
impl SubmittedTail {
    pub fn new(capacity_frames: usize) -> Self {
        Self {
            frames: ArrayQueue::new(capacity_frames.max(1)),
        }
    }

    fn record(&self, frame: SubmittedFrame) {
        if let Err(frame) = self.frames.push(frame) {
            let _ = self.frames.pop();
            // One callback is the only producer. A concurrent pause first stops
            // that callback, so the retry cannot race another producer.
            let _ = self.frames.push(frame);
        }
    }

    pub fn reconcile(
        &self,
        pending_frames: u64,
        replay: &ArrayQueue<SubmittedFrame>,
    ) -> Result<ReconciledTail, TailReconcileError> {
        let retained = self.frames.len() as u64;
        if pending_frames > retained {
            return Err(TailReconcileError::SnapshotExceedsRetainedTail);
        }
        if pending_frames > replay.capacity() as u64 {
            return Err(TailReconcileError::ReplayCapacityExceeded);
        }
        while replay.pop().is_some() {}
        for _ in 0..retained - pending_frames {
            let _ = self.frames.pop();
        }
        let mut logical_audio_frames = 0;
        while let Some(frame) = self.frames.pop() {
            logical_audio_frames += u64::from(frame.logical_audio);
            replay
                .push(frame)
                .map_err(|_| TailReconcileError::ReplayCapacityExceeded)?;
        }
        Ok(ReconciledTail {
            pending_frames,
            logical_audio_frames,
        })
    }
}

pub struct BoundaryRendered {
    pub rendered: Rendered,
    /// Frame offset within this output buffer where successor audio begins.
    pub boundary_frame: Option<usize>,
    pub successor_samples: u64,
    pub active_slot: usize,
}

/// Allocation-free consumer for one active occurrence plus one prepared successor.
/// It switches only after active EOF and never inserts refill silence at a ready
/// boundary. Both queues are created outside the callback.
pub struct BoundaryPcmConsumer {
    channels: usize,
    refill: usize,
    buffering: bool,
    successor: bool,
}

impl BoundaryPcmConsumer {
    pub fn new(channels: usize, refill: usize) -> Self {
        Self {
            channels,
            refill,
            buffering: true,
            successor: false,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render<T: cpal::Sample + cpal::FromSample<f32>>(
        &mut self,
        output: &mut [T],
        slot_a: &ArrayQueue<f32>,
        slot_a_ready: bool,
        slot_a_finished: bool,
        slot_b: &ArrayQueue<f32>,
        slot_b_ready: bool,
        slot_b_finished: bool,
        enabled: bool,
    ) -> BoundaryRendered {
        let mut result = BoundaryRendered {
            rendered: Rendered {
                samples: 0,
                last_audio_frame: None,
            },
            boundary_frame: None,
            successor_samples: 0,
            active_slot: usize::from(self.successor),
        };
        for (index, frame) in output.chunks_mut(self.channels).enumerate() {
            let (current, current_finished, other, other_ready) = if self.successor {
                (slot_b, slot_b_finished, slot_a, slot_a_ready)
            } else {
                (slot_a, slot_a_finished, slot_b, slot_b_ready)
            };
            if current_finished
                && current.len() < self.channels
                && other_ready
                && other.len() >= self.channels
            {
                self.successor = !self.successor;
                self.buffering = false;
                result.boundary_frame = Some(index);
                result.active_slot = usize::from(self.successor);
            }
            let (pcm, finished) = if self.successor {
                (slot_b, slot_b_finished)
            } else {
                (slot_a, slot_a_finished)
            };
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
                for target in frame {
                    *target = T::from_sample(pcm.pop().unwrap_or(0.0));
                }
                result.rendered.samples += self.channels as u64;
                result.rendered.last_audio_frame = Some(index + 1);
                if result.boundary_frame.is_some() || result.active_slot != 0 {
                    result.successor_samples += self.channels as u64;
                }
            } else {
                frame.fill(T::from_sample(0.0));
                if enabled {
                    self.buffering = true;
                }
            }
        }
        result
    }

    #[allow(clippy::too_many_arguments)]
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub fn render_with_tail<T: cpal::Sample + cpal::FromSample<f32>>(
        &mut self,
        output: &mut [T],
        active: &ArrayQueue<f32>,
        active_ready: bool,
        active_finished: bool,
        successor: &ArrayQueue<f32>,
        successor_ready: bool,
        successor_finished: bool,
        replay: &ArrayQueue<SubmittedFrame>,
        tail: &SubmittedTail,
        enabled: bool,
    ) -> BoundaryRendered {
        let mut result = BoundaryRendered {
            rendered: Rendered {
                samples: 0,
                last_audio_frame: None,
            },
            boundary_frame: None,
            successor_samples: 0,
            active_slot: usize::from(self.successor),
        };
        for (index, output_frame) in output.chunks_mut(self.channels).enumerate() {
            if enabled && let Some(replayed) = replay.pop() {
                for (channel, target) in output_frame.iter_mut().enumerate() {
                    *target = T::from_sample(replayed.samples[channel]);
                }
                if replayed.logical_audio {
                    result.rendered.samples += self.channels as u64;
                    result.rendered.last_audio_frame = Some(index + 1);
                }
                tail.record(replayed);
                continue;
            }
            let (current, current_finished, other, other_ready) = if self.successor {
                (successor, successor_finished, active, active_ready)
            } else {
                (active, active_finished, successor, successor_ready)
            };
            if current_finished
                && current.len() < self.channels
                && other_ready
                && other.len() >= self.channels
            {
                self.successor = !self.successor;
                self.buffering = false;
                result.boundary_frame = Some(index);
                result.active_slot = usize::from(self.successor);
            }
            let (pcm, finished) = if self.successor {
                (successor, successor_finished)
            } else {
                (active, active_finished)
            };
            if self.buffering
                && (pcm.len() >= self.refill || finished && pcm.len() >= self.channels)
            {
                self.buffering = false;
            }
            let ready = enabled
                && !self.buffering
                && output_frame.len() == self.channels
                && pcm.len() >= self.channels;
            let mut samples = [0.0; 2];
            if ready {
                for (channel, target) in output_frame.iter_mut().enumerate() {
                    samples[channel] = pcm.pop().unwrap_or(0.0);
                    *target = T::from_sample(samples[channel]);
                }
                result.rendered.samples += self.channels as u64;
                result.rendered.last_audio_frame = Some(index + 1);
                if result.boundary_frame.is_some() || result.active_slot != 0 {
                    result.successor_samples += self.channels as u64;
                }
            } else {
                output_frame.fill(T::from_sample(0.0));
                if enabled {
                    self.buffering = true;
                }
            }
            tail.record(SubmittedFrame::new(samples, self.channels, ready));
        }
        result
    }
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

    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub fn render_with_tail<T: cpal::Sample + cpal::FromSample<f32>>(
        &mut self,
        output: &mut [T],
        pcm: &ArrayQueue<f32>,
        replay: &ArrayQueue<SubmittedFrame>,
        tail: &SubmittedTail,
        enabled: bool,
        finished: bool,
    ) -> Rendered {
        let mut result = Rendered {
            samples: 0,
            last_audio_frame: None,
        };
        for (index, output_frame) in output.chunks_mut(self.channels).enumerate() {
            if enabled && let Some(replayed) = replay.pop() {
                debug_assert_eq!(usize::from(replayed.channels), self.channels);
                for (channel, target) in output_frame.iter_mut().enumerate() {
                    *target = T::from_sample(replayed.samples[channel]);
                }
                if replayed.logical_audio {
                    result.samples += self.channels as u64;
                    result.last_audio_frame = Some(index + 1);
                }
                tail.record(replayed);
                continue;
            }
            if self.buffering
                && (pcm.len() >= self.refill || finished && pcm.len() >= self.channels)
            {
                self.buffering = false;
            }
            let ready = enabled
                && !self.buffering
                && output_frame.len() == self.channels
                && pcm.len() >= self.channels;
            let mut samples = [0.0; 2];
            if ready {
                for (channel, target) in output_frame.iter_mut().enumerate() {
                    samples[channel] = pcm.pop().unwrap_or(0.0);
                    *target = T::from_sample(samples[channel]);
                }
                result.samples += self.channels as u64;
                result.last_audio_frame = Some(index + 1);
            } else {
                output_frame.fill(T::from_sample(0.0));
                if enabled {
                    self.buffering = true;
                }
            }
            tail.record(SubmittedFrame::new(samples, self.channels, ready));
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
    fn prepared_successor_joins_inside_one_callback_without_gap_or_duplicate() {
        let active = ArrayQueue::new(8);
        let successor = ArrayQueue::new(8);
        for sample in [1.0, 2.0, 3.0, 4.0] {
            active.push(sample).unwrap();
        }
        for sample in [5.0, 6.0, 7.0, 8.0] {
            successor.push(sample).unwrap();
        }
        let mut consumer = BoundaryPcmConsumer::new(2, 4);
        let mut output = [0.0f32; 8];
        let rendered = consumer.render(
            &mut output,
            &active,
            true,
            true,
            &successor,
            true,
            true,
            true,
        );
        assert_eq!(output, [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]);
        assert_eq!(rendered.boundary_frame, Some(2));
        assert_eq!(rendered.successor_samples, 4);
        assert!(active.is_empty());
        assert!(successor.is_empty());
    }

    #[test]
    fn unready_successor_never_leaks_after_active_eof() {
        let active = ArrayQueue::new(2);
        let successor = ArrayQueue::new(2);
        active.push(1.0).unwrap();
        active.push(2.0).unwrap();
        successor.push(3.0).unwrap();
        successor.push(4.0).unwrap();
        let mut consumer = BoundaryPcmConsumer::new(2, 2);
        let mut output = [9.0f32; 4];
        let rendered = consumer.render(
            &mut output,
            &active,
            true,
            true,
            &successor,
            false,
            false,
            true,
        );
        assert_eq!(output, [1.0, 2.0, 0.0, 0.0]);
        assert_eq!(rendered.boundary_frame, None);
        assert_eq!(successor.len(), 2);
    }

    #[test]
    fn retired_slot_can_be_reused_for_a_second_gapless_boundary() {
        let slot_a = ArrayQueue::new(8);
        let slot_b = ArrayQueue::new(8);
        for sample in [1.0, 2.0] {
            slot_a.push(sample).unwrap();
        }
        for sample in [3.0, 4.0] {
            slot_b.push(sample).unwrap();
        }
        let mut consumer = BoundaryPcmConsumer::new(2, 2);
        let mut first = [0.0f32; 4];
        let rendered = consumer.render(&mut first, &slot_a, true, true, &slot_b, true, true, true);
        assert_eq!(first, [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(rendered.active_slot, 1);

        for sample in [5.0, 6.0] {
            slot_a.push(sample).unwrap();
        }
        let mut second = [0.0f32; 2];
        let rendered = consumer.render(&mut second, &slot_a, true, true, &slot_b, true, true, true);
        assert_eq!(second, [5.0, 6.0]);
        assert_eq!(rendered.boundary_frame, Some(0));
        assert_eq!(rendered.active_slot, 0);
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

    #[test]
    fn acknowledged_pause_replays_exact_pending_suffix_once() {
        let pcm = ArrayQueue::new(8);
        for sample in [1.0, 2.0, 3.0, 4.0, 5.0, 6.0] {
            pcm.push(sample).unwrap();
        }
        let replay = ArrayQueue::new(4);
        let tail = SubmittedTail::new(2);
        let mut consumer = PcmConsumer::new(2, 2);
        let mut first = [0.0f32; 6];
        assert_eq!(
            consumer
                .render_with_tail(&mut first, &pcm, &replay, &tail, true, true)
                .samples,
            6
        );
        assert_eq!(first, [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);

        let reconciled = tail.reconcile(2, &replay).unwrap();
        assert_eq!(reconciled.logical_audio_frames, 2);
        let mut paused = [9.0f32; 2];
        assert_eq!(
            consumer
                .render_with_tail(&mut paused, &pcm, &replay, &tail, false, true)
                .samples,
            0
        );
        assert_eq!(paused, [0.0, 0.0]);
        assert_eq!(replay.len(), 2);
        let mut resumed = [0.0f32; 4];
        assert_eq!(
            consumer
                .render_with_tail(&mut resumed, &pcm, &replay, &tail, true, true)
                .samples,
            4
        );
        assert_eq!(resumed, [3.0, 4.0, 5.0, 6.0]);
        assert!(replay.is_empty());
    }

    #[test]
    fn pause_reconciliation_fails_when_native_tail_exceeds_bound() {
        let tail = SubmittedTail::new(1);
        tail.record(SubmittedFrame::new([1.0, 2.0], 2, true));
        let replay = ArrayQueue::new(2);
        assert_eq!(
            tail.reconcile(2, &replay),
            Err(TailReconcileError::SnapshotExceedsRetainedTail)
        );
    }
}
