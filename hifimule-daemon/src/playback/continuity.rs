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

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Authorization {
    Authorized,
    AlreadyAuthorized,
    Rejected,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Readiness {
    Ready,
    Rejected,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Adoption {
    Advance { successor_offset_frames: u64 },
    NotPresented,
    Duplicate,
    Rejected,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PauseDecision {
    GateClosed,
    Rejected,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum TerminalDecision {
    Completed,
    FailurePreserved,
    Rejected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Pending,
    Authorized,
    Ready { boundary_frame: u64 },
    Pausing { boundary_frame: u64 },
    Adopted,
    Failed,
    Completed,
}

pub(crate) struct HandoffContract {
    token: HandoffToken,
    state: State,
}

impl HandoffContract {
    pub fn new(token: HandoffToken) -> Self {
        Self {
            token,
            state: State::Pending,
        }
    }

    fn matches(&self, token: &HandoffToken) -> bool {
        self.token == *token
    }

    pub fn authorize(&mut self, token: &HandoffToken) -> Authorization {
        if !self.matches(token) {
            return Authorization::Rejected;
        }
        match self.state {
            State::Pending => {
                self.state = State::Authorized;
                Authorization::Authorized
            }
            State::Authorized | State::Ready { .. } | State::Pausing { .. } => {
                Authorization::AlreadyAuthorized
            }
            State::Adopted | State::Failed | State::Completed => Authorization::Rejected,
        }
    }

    pub fn ready(&mut self, token: &HandoffToken, boundary_frame: u64) -> Readiness {
        if !self.matches(token) || !matches!(self.state, State::Authorized) {
            return Readiness::Rejected;
        }
        self.state = State::Ready { boundary_frame };
        Readiness::Ready
    }

    pub fn presented(&mut self, token: &HandoffToken, played_frame: u64) -> Adoption {
        if !self.matches(token) {
            return Adoption::Rejected;
        }
        match self.state {
            State::Ready { boundary_frame } | State::Pausing { boundary_frame }
                if played_frame >= boundary_frame =>
            {
                self.state = State::Adopted;
                Adoption::Advance {
                    successor_offset_frames: played_frame - boundary_frame,
                }
            }
            State::Ready { .. } | State::Pausing { .. } => Adoption::NotPresented,
            State::Adopted => Adoption::Duplicate,
            _ => Adoption::Rejected,
        }
    }

    pub fn pause_requested(&mut self, token: &HandoffToken) -> PauseDecision {
        if !self.matches(token) {
            return PauseDecision::Rejected;
        }
        match self.state {
            State::Ready { boundary_frame } => {
                self.state = State::Pausing { boundary_frame };
                PauseDecision::GateClosed
            }
            State::Pending | State::Authorized => PauseDecision::GateClosed,
            _ => PauseDecision::Rejected,
        }
    }

    pub fn pause_acknowledged(&mut self, token: &HandoffToken, played_frame: u64) -> Adoption {
        self.presented(token, played_frame)
    }

    pub fn fail(&mut self, token: &HandoffToken) -> TerminalDecision {
        if !self.matches(token) {
            return TerminalDecision::Rejected;
        }
        self.state = State::Failed;
        TerminalDecision::FailurePreserved
    }

    pub fn complete(&mut self, token: &HandoffToken) -> TerminalDecision {
        if !self.matches(token) {
            return TerminalDecision::Rejected;
        }
        match self.state {
            State::Failed => TerminalDecision::FailurePreserved,
            State::Completed => TerminalDecision::Completed,
            _ => {
                self.state = State::Completed;
                TerminalDecision::Completed
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token() -> HandoffToken {
        HandoffToken {
            instance_id: "instance".into(),
            session_id: "session".into(),
            predecessor_occurrence_id: "one".into(),
            successor_occurrence_id: "two".into(),
            queue_revision: 7,
            control_epoch: 11,
            preparation_generation: 13,
            output_epoch: 17,
        }
    }

    #[test]
    fn authorization_requires_every_identity_and_only_one_pending_boundary() {
        let mut contract = HandoffContract::new(token());
        assert_eq!(contract.authorize(&token()), Authorization::Authorized);
        assert_eq!(
            contract.authorize(&token()),
            Authorization::AlreadyAuthorized
        );
        let mut stale = token();
        stale.successor_occurrence_id = "repeated-source-other-occurrence".into();
        assert_eq!(contract.authorize(&stale), Authorization::Rejected);
    }

    #[test]
    fn presentation_acknowledgment_advances_the_owner_exactly_once() {
        let mut contract = HandoffContract::new(token());
        contract.authorize(&token());
        assert_eq!(contract.ready(&token(), 480), Readiness::Ready);
        assert_eq!(
            contract.presented(&token(), 512),
            Adoption::Advance {
                successor_offset_frames: 32
            }
        );
        assert_eq!(contract.presented(&token(), 700), Adoption::Duplicate);
    }

    #[test]
    fn pause_closes_consumption_immediately_and_backend_ack_decides_the_race() {
        let mut before = HandoffContract::new(token());
        before.authorize(&token());
        before.ready(&token(), 480);
        assert_eq!(before.pause_requested(&token()), PauseDecision::GateClosed);
        assert_eq!(
            before.pause_acknowledged(&token(), 479),
            Adoption::NotPresented
        );

        let mut after = HandoffContract::new(token());
        after.authorize(&token());
        after.ready(&token(), 480);
        assert_eq!(after.pause_requested(&token()), PauseDecision::GateClosed);
        assert_eq!(
            after.pause_acknowledged(&token(), 481),
            Adoption::Advance {
                successor_offset_frames: 1
            }
        );
    }

    #[test]
    fn terminal_failure_dominates_delayed_completion_for_the_same_attempt() {
        let mut contract = HandoffContract::new(token());
        contract.fail(&token());
        assert_eq!(
            contract.complete(&token()),
            TerminalDecision::FailurePreserved
        );
    }
}
