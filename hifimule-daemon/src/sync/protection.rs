//! Pure playback-protection admission policy. Device work is never owned here.

use std::sync::{Arc, Mutex};

pub(crate) const POLICY_VERSION: u8 = 1;
pub(crate) const ATTRIBUTABLE_DELAY_MS: u64 = 500;
pub(crate) const UNATTRIBUTABLE_DELAY_MS: u64 = 100;
pub(crate) const INEFFECTIVE_AFTER_MS: u64 = 120_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Health {
    Unknown,
    Healthy,
    Risk,
    Recovering,
    Stale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reason {
    Healthy,
    Unknown,
    Stale,
    NoActivePlayback,
    SourceNotInSyncSet,
    AttributableSharedSource,
    UnattributableOutputRisk,
    RecoveryPending,
    IneffectiveProtection,
}

impl Reason {
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Unknown => "unknown",
            Self::Stale => "stale",
            Self::NoActivePlayback => "noActivePlayback",
            Self::SourceNotInSyncSet => "sourceNotInSyncSet",
            Self::AttributableSharedSource => "attributableSharedSource",
            Self::UnattributableOutputRisk => "unattributableOutputRisk",
            Self::RecoveryPending => "recoveryPending",
            Self::IneffectiveProtection => "ineffectiveProtection",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Snapshot {
    pub policy_version: u8,
    pub session_id: String,
    pub generation_id: String,
    pub preview: bool,
    pub server_id: Option<String>,
    pub representation_id: String,
    pub observed_ms: u64,
    pub expires_ms: u64,
    pub risk_started_ms: Option<u64>,
    pub eligible_samples: usize,
    pub health: Health,
}

#[derive(Clone, Default)]
pub(crate) struct Observer(Arc<Mutex<Option<Snapshot>>>);

impl Observer {
    pub(crate) fn publish(&self, snapshot: Option<Snapshot>) {
        *self.0.lock().unwrap_or_else(|error| error.into_inner()) = snapshot;
    }
    pub(crate) fn snapshot(&self) -> Option<Snapshot> {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Decision {
    pub delay_ms: u64,
    pub reason: Reason,
}
impl Decision {
    pub(crate) const fn normal(reason: Reason) -> Self {
        Self {
            delay_ms: 0,
            reason,
        }
    }
}

pub(crate) fn decide(
    snapshot: Option<&Snapshot>,
    producer_server_id: Option<&str>,
    now_ms: u64,
) -> Decision {
    let Some(snapshot) = snapshot else {
        return Decision::normal(Reason::NoActivePlayback);
    };
    if snapshot.policy_version != POLICY_VERSION || now_ms > snapshot.expires_ms {
        return Decision::normal(Reason::Stale);
    }
    match snapshot.health {
        Health::Unknown => Decision::normal(Reason::Unknown),
        Health::Healthy => Decision::normal(Reason::Healthy),
        Health::Stale => Decision::normal(Reason::Stale),
        Health::Risk | Health::Recovering => {
            let ineffective = snapshot
                .risk_started_ms
                .is_some_and(|started| now_ms.saturating_sub(started) >= INEFFECTIVE_AFTER_MS);
            match snapshot.server_id.as_deref() {
                Some(source) if producer_server_id == Some(source) => Decision {
                    delay_ms: ATTRIBUTABLE_DELAY_MS,
                    reason: if ineffective {
                        Reason::IneffectiveProtection
                    } else if snapshot.health == Health::Recovering {
                        Reason::RecoveryPending
                    } else {
                        Reason::AttributableSharedSource
                    },
                },
                Some(_) => Decision::normal(Reason::SourceNotInSyncSet),
                None => Decision {
                    delay_ms: UNATTRIBUTABLE_DELAY_MS,
                    reason: if ineffective {
                        Reason::IneffectiveProtection
                    } else {
                        Reason::UnattributableOutputRisk
                    },
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot_at(at: u64, server: Option<&str>, health: Health) -> Snapshot {
        Snapshot {
            policy_version: POLICY_VERSION,
            session_id: "session".into(),
            generation_id: "generation".into(),
            preview: false,
            server_id: server.map(str::to_owned),
            representation_id: "representation".into(),
            observed_ms: at,
            expires_ms: at + 60_000,
            risk_started_ms: (health == Health::Risk).then_some(at),
            eligible_samples: 3,
            health,
        }
    }
    #[test]
    fn exact_risk_threshold_targets_only_shared_source() {
        let s = snapshot_at(1_000, Some("shared"), Health::Risk);
        assert_eq!(
            decide(Some(&s), Some("shared"), 1_000),
            Decision {
                delay_ms: 500,
                reason: Reason::AttributableSharedSource
            }
        );
        assert_eq!(
            decide(Some(&s), Some("other"), 1_000),
            Decision::normal(Reason::SourceNotInSyncSet)
        );
    }
    #[test]
    fn stale_unknown_and_healthy_are_exactly_normal() {
        for health in [Health::Unknown, Health::Healthy, Health::Stale] {
            let s = snapshot_at(1_000, Some("shared"), health);
            assert_eq!(decide(Some(&s), Some("shared"), 70_001).delay_ms, 0);
        }
    }
    #[test]
    fn unattributable_risk_is_weakest_global_delay() {
        let s = snapshot_at(1_000, None, Health::Risk);
        assert_eq!(
            decide(Some(&s), Some("any"), 1_000),
            Decision {
                delay_ms: 100,
                reason: Reason::UnattributableOutputRisk
            }
        );
    }
    #[test]
    fn ineffective_never_escalates_delay() {
        let mut s = snapshot_at(1_000, Some("shared"), Health::Risk);
        s.expires_ms = 200_000;
        assert_eq!(
            decide(Some(&s), Some("shared"), 121_000),
            Decision {
                delay_ms: 500,
                reason: Reason::IneffectiveProtection
            }
        );
    }

    #[test]
    fn recovery_pending_keeps_the_same_bounded_delay() {
        let mut snapshot = snapshot_at(1_000, Some("shared"), Health::Recovering);
        snapshot.risk_started_ms = Some(500);
        assert_eq!(
            decide(Some(&snapshot), Some("shared"), 1_000),
            Decision {
                delay_ms: ATTRIBUTABLE_DELAY_MS,
                reason: Reason::RecoveryPending,
            }
        );
    }

    #[test]
    fn publisher_replacement_and_close_cannot_revive_old_generation() {
        let observer = Observer::default();
        let old = snapshot_at(1_000, Some("shared"), Health::Risk);
        observer.publish(Some(old));
        let mut replacement = snapshot_at(2_000, Some("shared"), Health::Healthy);
        replacement.generation_id = "new-generation".into();
        observer.publish(Some(replacement.clone()));
        assert_eq!(observer.snapshot(), Some(replacement));
        observer.publish(None);
        assert_eq!(observer.snapshot(), None);
        assert_eq!(
            decide(observer.snapshot().as_ref(), Some("shared"), 2_000).delay_ms,
            0
        );
    }

    #[tokio::test]
    async fn cancellation_preempts_an_active_protection_wait() {
        let observer = Observer::default();
        let now = crate::playback::adaptation::monotonic_ms();
        observer.publish(Some(snapshot_at(now, Some("shared"), Health::Risk)));
        let manager = std::sync::Arc::new(super::super::SyncOperationManager::new());
        manager.create_operation("protected-wait".into(), 1).await;
        let wait_observer = observer.clone();
        let wait_manager = manager.clone();
        let waiter = tokio::spawn(async move {
            super::super::wait_for_protection_admission(
                Some(&wait_observer),
                Some("shared"),
                &wait_manager,
                "protected-wait",
            )
            .await
        });
        tokio::task::yield_now().await;
        assert!(manager.request_cancel("protected-wait").await);
        assert!(
            !tokio::time::timeout(std::time::Duration::from_millis(100), waiter)
                .await
                .expect("cancellation must preempt the 500 ms delay")
                .unwrap()
        );
    }
}
