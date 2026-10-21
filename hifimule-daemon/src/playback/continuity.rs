//! Prepared-track handoff contract.

use std::sync::atomic::{AtomicU64, Ordering};

/// One atomic decision shared by edit admission, preparation publication and
/// the audio consumer. Claiming a boundary is irreversible until the backend
/// presents it and the owner adopts it. The callback never locks or waits.
#[derive(Default)]
pub(crate) struct SuccessorFence(AtomicU64);

impl SuccessorFence {
    const READY: u64 = 1;
    const CLAIMED: u64 = 2;
    const MASK: u64 = 3;

    pub fn epoch(&self) -> u64 {
        self.0.load(Ordering::Acquire) & !Self::MASK
    }

    pub fn authorize(&self, epoch: u64) -> bool {
        self.0
            .compare_exchange(
                epoch,
                epoch | Self::READY,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }

    pub fn claim(&self) -> bool {
        let state = self.0.load(Ordering::Acquire);
        state & Self::MASK == Self::READY
            && self
                .0
                .compare_exchange(
                    state,
                    (state & !Self::MASK) | Self::CLAIMED,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_ok()
    }

    /// False means submission won; rejection must not change its authorization.
    pub fn revoke(&self) -> bool {
        self.0
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |state| {
                if state & Self::MASK == Self::CLAIMED {
                    None
                } else {
                    (state & !Self::MASK).checked_add(Self::MASK + 1)
                }
            })
            .is_ok()
    }

    pub fn claimed(&self) -> bool {
        self.0.load(Ordering::Acquire) & Self::MASK == Self::CLAIMED
    }

    pub fn disarm(&self, epoch: u64) -> bool {
        self.0
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |state| {
                (state & !Self::MASK == epoch && state & Self::MASK != Self::CLAIMED)
                    .then_some(epoch)
            })
            .is_ok()
    }

    /// Called only after backend presentation, owner adoption and slot retirement.
    pub fn adopted(&self) {
        let _ = self
            .0
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |state| {
                (state & Self::MASK == Self::CLAIMED)
                    .then(|| (state & !Self::MASK).checked_add(Self::MASK + 1))
                    .flatten()
            });
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HandoffToken {
    pub instance_id: String,
    pub session_id: String,
    pub predecessor_occurrence_id: String,
    pub successor_occurrence_id: String,
    pub queue_revision: u64,
    pub control_epoch: u64,
    pub preparation_generation: u64,
    pub output_epoch: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SuccessorCandidate {
    pub instance_id: String,
    pub session_id: String,
    pub predecessor_occurrence_id: String,
    pub successor: crate::playback::model::Occurrence,
    pub queue_revision: u64,
    pub control_epoch: u64,
    pub preparation_generation: u64,
    pub gain_bits: u32,
    pub qualified_suffix: Option<String>,
}
