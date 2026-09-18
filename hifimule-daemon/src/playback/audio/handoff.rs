//! One bounded presentation receipt shared by the output worker and owner.
//! The callback touches only atomics; metadata stays behind a non-callback lock.
use super::*;

pub(super) struct HandoffReceipt {
    pending: Mutex<Option<(PlaybackEvent, Arc<AtomicU64>)>>,
    pub boundary_frame: Arc<AtomicU64>,
    pub boundary_samples: Arc<AtomicU64>,
    pub deadline_ns: Arc<AtomicU64>,
    pub consumed: Arc<AtomicU64>,
    pub occurrence_base: Arc<AtomicU64>,
    pub clock: Arc<super::super::output::PresentationClock>,
    pulse_offset: AtomicU64,
    channels: AtomicU64,
    pub adopted: AtomicBool,
}

impl HandoffReceipt {
    pub fn new() -> Self {
        Self {
            pending: Mutex::new(None),
            boundary_frame: Arc::new(AtomicU64::new(u64::MAX)),
            boundary_samples: Arc::new(AtomicU64::new(0)),
            deadline_ns: Arc::new(AtomicU64::new(u64::MAX)),
            consumed: Arc::new(AtomicU64::new(0)),
            occurrence_base: Arc::new(AtomicU64::new(0)),
            clock: Arc::new(super::super::output::PresentationClock::new()),
            pulse_offset: AtomicU64::new(u64::MAX),
            channels: AtomicU64::new(1),
            adopted: AtomicBool::new(false),
        }
    }

    pub fn arm(&self, event: PlaybackEvent, qualified: Arc<AtomicU64>, channels: u16) {
        self.channels.store(u64::from(channels), Ordering::Release);
        *self.pending.lock().unwrap_or_else(|e| e.into_inner()) = Some((event, qualified));
    }

    pub fn submitted(&self) -> bool {
        self.boundary_frame.load(Ordering::Acquire) != u64::MAX
            || self.pulse_offset.load(Ordering::Acquire) != u64::MAX
    }

    #[cfg(any(target_os = "linux", test))]
    pub fn pulse_presented(&self, offset: u64) {
        self.pulse_offset.store(offset, Ordering::Release);
    }

    pub fn presented(&self, epoch: u64) -> Option<PlaybackEvent> {
        self.presented_at(epoch, self.clock.now_ns())
    }

    fn presented_at(&self, epoch: u64, now: u64) -> Option<PlaybackEvent> {
        let pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        let (event, qualified) = pending.as_ref()?;
        let mut event = event.clone();
        let PlaybackEvent::HandoffPresented {
            token,
            duration_ms,
            successor_offset_frames,
            sample_rate,
            seek,
            ..
        } = &mut event
        else {
            return None;
        };
        let pulse = self.pulse_offset.load(Ordering::Acquire);
        let deadline = self.deadline_ns.load(Ordering::Acquire);
        *successor_offset_frames = if pulse != u64::MAX {
            pulse
        } else if deadline != u64::MAX && now >= deadline {
            let submitted = self
                .consumed
                .load(Ordering::Acquire)
                .saturating_sub(self.boundary_samples.load(Ordering::Acquire))
                / self.channels.load(Ordering::Acquire).max(1);
            ((now - deadline).saturating_mul(u64::from(*sample_rate)) / 1_000_000_000)
                .min(submitted)
        } else {
            return None;
        };
        // Only Pause/Resume retain this output generation. The owner explicitly
        // advances event_epoch for these controls after reconciling presentation.
        token.control_epoch = epoch;
        let verified_duration = qualified.load(Ordering::Acquire);
        if let PlaybackEvent::SeekQualified { capability, .. } =
            seek_qualification_event(verified_duration)
        {
            *seek = capability;
        }
        if verified_duration > 0 {
            *duration_ms = verified_duration;
        }
        Some(event)
    }

    pub fn clear(&self) {
        *self.pending.lock().unwrap_or_else(|e| e.into_inner()) = None;
        self.boundary_frame.store(u64::MAX, Ordering::Release);
        self.deadline_ns.store(u64::MAX, Ordering::Release);
        self.pulse_offset.store(u64::MAX, Ordering::Release);
        self.adopted.store(false, Ordering::Release);
    }

    pub fn freeze(&self, epoch: u64) {
        if let Some(PlaybackEvent::HandoffPresented {
            successor_offset_frames,
            ..
        }) = self.presented(epoch)
        {
            self.pulse_offset
                .store(successor_offset_frames, Ordering::Release);
        } else {
            self.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn receipt() -> HandoffReceipt {
        let receipt = HandoffReceipt::new();
        receipt.arm(
            PlaybackEvent::HandoffPresented {
                token: crate::playback::continuity::HandoffToken {
                    instance_id: "i".into(),
                    session_id: "s".into(),
                    predecessor_occurrence_id: "a".into(),
                    successor_occurrence_id: "b".into(),
                    queue_revision: 1,
                    control_epoch: 2,
                    preparation_generation: 3,
                    output_epoch: 4,
                },
                metadata: PlaybackTrackMetadata {
                    source: crate::playback::model::TrackSource {
                        server_id: "server".into(),
                        track_id: "b".into(),
                    },
                    title: "b".into(),
                    artist: None,
                    album: None,
                },
                duration_ms: 9_000,
                representation: "wav".into(),
                successor_offset_frames: 0,
                sample_rate: 48_000,
                seek: crate::playback::model::SeekCapability::unavailable("preparing"),
            },
            Arc::new(AtomicU64::new(10_000)),
            2,
        );
        receipt
    }

    #[test]
    fn receipt_waits_for_presentation_and_keeps_delayed_owner_offset_and_qualification() {
        let r = receipt();
        r.boundary_samples.store(192_000, Ordering::Release);
        r.consumed.store(194_880, Ordering::Release);
        r.deadline_ns.store(1_000_000_000, Ordering::Release);
        assert!(r.presented_at(2, 999_999_999).is_none());
        let Some(PlaybackEvent::HandoffPresented {
            successor_offset_frames,
            duration_ms,
            seek,
            ..
        }) = r.presented_at(2, 1_020_000_000)
        else {
            panic!("receipt missing")
        };
        assert_eq!(successor_offset_frames, 960);
        assert_eq!(duration_ms, 10_000);
        assert!(seek.available);
        let Some(PlaybackEvent::HandoffPresented {
            successor_offset_frames,
            ..
        }) = r.presented_at(2, 2_000_000_000)
        else {
            panic!("receipt missing")
        };
        assert_eq!(
            successor_offset_frames, 1440,
            "never claim unsubmitted frames"
        );
    }

    #[test]
    fn retained_preparation_rebinds_only_to_owner_supplied_control_epoch() {
        let r = receipt();
        assert!(r.presented(5).is_none());
        r.pulse_presented(321);
        let Some(PlaybackEvent::HandoffPresented {
            token,
            successor_offset_frames,
            ..
        }) = r.presented(5)
        else {
            panic!("receipt missing")
        };
        assert_eq!(token.control_epoch, 5);
        assert_eq!(token.predecessor_occurrence_id, "a");
        assert_eq!(successor_offset_frames, 321);
        r.clear();
        assert!(r.presented(6).is_none());
    }

    #[test]
    fn winning_pause_discards_an_unpresented_receipt_permanently() {
        let r = receipt();
        r.boundary_frame.store(100, Ordering::Release);
        r.deadline_ns.store(u64::MAX - 1, Ordering::Release);
        r.freeze(2);
        assert!(r.presented_at(3, u64::MAX).is_none());
        assert!(!r.submitted());
    }
}
