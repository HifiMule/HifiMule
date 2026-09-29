//! Pure, bounded track-boundary playback adaptation policy (v1).
//!
//! Rates are bytes/second and ages are monotonic milliseconds. Only complete,
//! origin-delivery samples participate: startup, range/seek, cache, partial and
//! failed requests are retained only as bounded diagnostics.

use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, OnceLock};

pub const POLICY_VERSION: u8 = 1;
pub const OBSERVATION_WINDOW_MS: u64 = 60_000;
pub const RECOVERY_WINDOW_MS: u64 = 120_000;
pub const MIN_ELIGIBLE_SAMPLES: usize = 3;
pub const MAX_SAMPLES_PER_SCOPE: usize = 16;
pub const MAX_OBSERVATION_SCOPES: usize = 64;
pub const MAX_SELECTED_SERVERS: usize = 16;
pub const DOWNGRADE_HEADROOM_PERCENT: u64 = 125;
pub const RECOVERY_HEADROOM_PERCENT: u64 = 175;
pub const STARTUP_BUFFER_MS: u64 = 100;
pub const PREPARATION_DEADLINE_MS: u64 = 60_000;
pub const MAX_SUCCESSOR_REPLACEMENTS: u8 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ObservationScope {
    pub server_id: String,
    pub representation_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleEligibility {
    Eligible,
    Startup,
    Cached,
    RangeOrSeek,
    Partial,
    Failed,
    TooShort,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeliveryObservation {
    pub monotonic_ms: u64,
    pub bytes: u64,
    pub elapsed_ms: u64,
    pub compressed_buffer_ms: u64,
    pub pcm_buffer_ms: u64,
    pub eligibility: SampleEligibility,
}

impl DeliveryObservation {
    fn bytes_per_second(self) -> Option<u64> {
        (self.eligibility == SampleEligibility::Eligible
            && self.elapsed_ms >= 250
            && self.bytes > 0)
            .then(|| self.bytes.saturating_mul(1_000) / self.elapsed_ms.max(1))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdaptationReason {
    Startup,
    Sustainable,
    ReducedBufferPressure,
    RecoveredCapacity,
    NoSustainableAlternative,
    AlbumGainConstraint,
}

impl AdaptationReason {
    pub fn wire_code(self) -> &'static str {
        match self {
            Self::Startup => "startup",
            Self::Sustainable => "sustainable",
            Self::ReducedBufferPressure => "reducedBufferPressure",
            Self::RecoveredCapacity => "recoveredCapacity",
            Self::NoSustainableAlternative => "noSustainableAlternative",
            Self::AlbumGainConstraint => "albumGainConstraint",
        }
    }

    pub fn from_wire_code(value: Option<&str>) -> Self {
        match value {
            Some("reducedBufferPressure") => Self::ReducedBufferPressure,
            Some("recoveredCapacity") => Self::RecoveredCapacity,
            Some("noSustainableAlternative") => Self::NoSustainableAlternative,
            Some("albumGainConstraint") => Self::AlbumGainConstraint,
            _ => Self::Sustainable,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapacityEvidence {
    pub bytes_per_second: u64,
    pub eligible_samples: usize,
    pub oldest_ms: u64,
    pub newest_ms: u64,
    pub depleted: bool,
    pub compressed_low_observed: bool,
    pub pcm_low_observed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SyncEvidenceClass {
    Unknown,
    Risk,
    Recovering,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SyncEvidence {
    pub class: SyncEvidenceClass,
    pub eligible_samples: usize,
    pub oldest_ms: u64,
    pub newest_ms: u64,
    pub reason: SyncEvidenceReason,
    pub recovery_qualified: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SyncEvidenceReason {
    InsufficientEvidence,
    CompressedDepletion,
    PcmDepletion,
    BufferDepletion,
    UnsustainableDelivery,
    RecoveryPending,
}

#[derive(Debug, Default)]
pub struct AdaptationHistory {
    scopes: HashMap<ObservationScope, VecDeque<DeliveryObservation>>,
    scope_order: VecDeque<ObservationScope>,
}

#[derive(Default)]
struct RuntimePolicy {
    history: AdaptationHistory,
    selected: HashMap<String, String>,
    reasons: HashMap<String, AdaptationReason>,
    server_order: VecDeque<String>,
}

static RUNTIME: OnceLock<Mutex<RuntimePolicy>> = OnceLock::new();
static MONOTONIC_ORIGIN: OnceLock<std::time::Instant> = OnceLock::new();

pub fn monotonic_ms() -> u64 {
    u64::try_from(
        MONOTONIC_ORIGIN
            .get_or_init(std::time::Instant::now)
            .elapsed()
            .as_millis(),
    )
    .unwrap_or(u64::MAX)
}

pub fn record(scope: ObservationScope, sample: DeliveryObservation) {
    RUNTIME
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .history
        .observe(scope, sample);
}

pub(crate) fn sync_evidence(
    scope: &ObservationScope,
    required_bytes_per_second: Option<u64>,
    now_ms: u64,
) -> SyncEvidence {
    let runtime = RUNTIME
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let Some(risk) = runtime
        .history
        .evidence(scope, now_ms, OBSERVATION_WINDOW_MS)
    else {
        return SyncEvidence {
            class: SyncEvidenceClass::Unknown,
            eligible_samples: 0,
            oldest_ms: now_ms,
            newest_ms: now_ms,
            reason: SyncEvidenceReason::InsufficientEvidence,
            recovery_qualified: false,
        };
    };
    let unsustainable =
        required_bytes_per_second.is_some_and(|required| !sustainable(required, risk, false));
    if risk.depleted || unsustainable {
        return SyncEvidence {
            class: SyncEvidenceClass::Risk,
            eligible_samples: risk.eligible_samples,
            oldest_ms: risk.oldest_ms,
            newest_ms: risk.newest_ms,
            reason: if unsustainable {
                SyncEvidenceReason::UnsustainableDelivery
            } else if risk.compressed_low_observed && risk.pcm_low_observed {
                SyncEvidenceReason::BufferDepletion
            } else if risk.compressed_low_observed {
                SyncEvidenceReason::CompressedDepletion
            } else {
                SyncEvidenceReason::PcmDepletion
            },
            recovery_qualified: false,
        };
    }
    let recovery_qualified = required_bytes_per_second.is_some_and(|required| {
        risk.bytes_per_second.saturating_mul(100)
            >= required.saturating_mul(RECOVERY_HEADROOM_PERCENT)
    });
    SyncEvidence {
        class: SyncEvidenceClass::Recovering,
        eligible_samples: risk.eligible_samples,
        oldest_ms: risk.oldest_ms,
        newest_ms: risk.newest_ms,
        reason: SyncEvidenceReason::RecoveryPending,
        recovery_qualified,
    }
}

pub(crate) fn select_for_boundary(
    server_id: &str,
    representations: Vec<crate::providers::PlaybackRepresentation>,
) -> Result<
    (crate::providers::PlaybackRepresentation, AdaptationReason),
    crate::providers::ProviderError,
> {
    let ordered = crate::providers::select_ordered_playback_representations(representations)?;
    let runtime = RUNTIME
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let current_id = runtime.selected.get(server_id).cloned();
    let current_index = current_id
        .as_ref()
        .and_then(|id| ordered.iter().position(|candidate| &candidate.id.0 == id));
    let Some(index) = current_index else {
        let selected = ordered.into_iter().next().unwrap();
        return Ok((selected, AdaptationReason::Startup));
    };
    let scope = ObservationScope {
        server_id: server_id.into(),
        representation_id: ordered[index].id.0.clone(),
    };
    let now_ms = monotonic_ms();
    let Some(downgrade_evidence) = runtime
        .history
        .evidence(&scope, now_ms, OBSERVATION_WINDOW_MS)
    else {
        return Ok((
            ordered.into_iter().nth(index).unwrap(),
            AdaptationReason::Sustainable,
        ));
    };
    let current_required = ordered[index].quality.required_bytes_per_second;
    if downgrade_evidence.depleted
        && current_required
            .is_some_and(|required| !sustainable(required, downgrade_evidence, false))
    {
        let chosen = ordered
            .iter()
            .skip(index + 1)
            .find(|candidate| {
                candidate
                    .quality
                    .required_bytes_per_second
                    .is_some_and(|required| sustainable(required, downgrade_evidence, false))
            })
            .cloned();
        return Ok(match chosen {
            Some(chosen) => (chosen, AdaptationReason::ReducedBufferPressure),
            None => (
                ordered.into_iter().nth(index).unwrap(),
                AdaptationReason::NoSustainableAlternative,
            ),
        });
    }
    if index > 0
        && let Some(recovery_evidence) =
            runtime.history.evidence(&scope, now_ms, RECOVERY_WINDOW_MS)
        && let Some(chosen) = ordered[..index]
            .iter()
            .find(|candidate| {
                candidate
                    .quality
                    .required_bytes_per_second
                    .is_some_and(|required| sustainable(required, recovery_evidence, true))
            })
            .cloned()
    {
        return Ok((chosen, AdaptationReason::RecoveredCapacity));
    }
    Ok((
        ordered.into_iter().nth(index).unwrap(),
        AdaptationReason::Sustainable,
    ))
}

#[cfg(test)]
pub(crate) fn selected_status(server_id: &str) -> Option<(String, AdaptationReason)> {
    let runtime = RUNTIME
        .get()?
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    Some((
        runtime.selected.get(server_id)?.clone(),
        runtime
            .reasons
            .get(server_id)
            .copied()
            .unwrap_or(AdaptationReason::Sustainable),
    ))
}

pub(crate) fn commit_selection(server_id: &str, representation_id: &str, reason: AdaptationReason) {
    let mut runtime = RUNTIME
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if !runtime.selected.contains_key(server_id) {
        runtime.server_order.push_back(server_id.to_owned());
    }
    runtime
        .selected
        .insert(server_id.to_owned(), representation_id.to_owned());
    runtime.reasons.insert(server_id.to_owned(), reason);
    while runtime.server_order.len() > MAX_SELECTED_SERVERS {
        if let Some(expired) = runtime.server_order.pop_front() {
            runtime.selected.remove(&expired);
            runtime.reasons.remove(&expired);
        }
    }
}

impl AdaptationHistory {
    pub fn observe(&mut self, scope: ObservationScope, sample: DeliveryObservation) {
        if !self.scopes.contains_key(&scope) {
            self.scope_order.push_back(scope.clone());
        }
        let samples = self.scopes.entry(scope).or_default();
        samples.push_back(sample);
        while samples.len() > MAX_SAMPLES_PER_SCOPE {
            samples.pop_front();
        }
        while samples.front().is_some_and(|first| {
            sample.monotonic_ms.saturating_sub(first.monotonic_ms) > RECOVERY_WINDOW_MS
        }) {
            samples.pop_front();
        }
        while self.scope_order.len() > MAX_OBSERVATION_SCOPES {
            if let Some(expired) = self.scope_order.pop_front() {
                self.scopes.remove(&expired);
            }
        }
    }

    pub fn evidence(
        &self,
        scope: &ObservationScope,
        now_ms: u64,
        window_ms: u64,
    ) -> Option<CapacityEvidence> {
        let eligible: Vec<_> = self
            .scopes
            .get(scope)?
            .iter()
            .copied()
            .filter(|sample| {
                now_ms.saturating_sub(sample.monotonic_ms) <= window_ms
                    && sample.bytes_per_second().is_some()
            })
            .collect();
        if eligible.len() < MIN_ELIGIBLE_SAMPLES {
            return None;
        }
        let bytes_per_second = eligible
            .iter()
            .filter_map(|sample| sample.bytes_per_second())
            .min()?;
        let compressed_low_observed = eligible
            .iter()
            .any(|sample| sample.compressed_buffer_ms < 100);
        let pcm_low_observed = eligible.iter().any(|sample| sample.pcm_buffer_ms < 100);
        let depleted = eligible
            .iter()
            .filter(|sample| sample.compressed_buffer_ms < 100 || sample.pcm_buffer_ms < 100)
            .count()
            >= 2;
        Some(CapacityEvidence {
            bytes_per_second,
            eligible_samples: eligible.len(),
            oldest_ms: eligible.first()?.monotonic_ms,
            newest_ms: eligible.last()?.monotonic_ms,
            depleted,
            compressed_low_observed,
            pcm_low_observed,
        })
    }
}

pub fn sustainable(
    required_bytes_per_second: u64,
    evidence: CapacityEvidence,
    recovery: bool,
) -> bool {
    let headroom = if recovery {
        RECOVERY_HEADROOM_PERCENT
    } else {
        DOWNGRADE_HEADROOM_PERCENT
    };
    evidence.bytes_per_second.saturating_mul(100)
        >= required_bytes_per_second.saturating_mul(headroom)
        && (!recovery
            || evidence.newest_ms.saturating_sub(evidence.oldest_ms) >= RECOVERY_WINDOW_MS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(
        at: u64,
        rate: u64,
        compressed: u64,
        pcm: u64,
        eligibility: SampleEligibility,
    ) -> DeliveryObservation {
        DeliveryObservation {
            monotonic_ms: at,
            bytes: rate,
            elapsed_ms: 1_000,
            compressed_buffer_ms: compressed,
            pcm_buffer_ms: pcm,
            eligibility,
        }
    }

    #[test]
    fn evidence_is_scoped_bounded_and_requires_three_real_samples() {
        let mut history = AdaptationHistory::default();
        let a = ObservationScope {
            server_id: "a".into(),
            representation_id: "aac-192".into(),
        };
        let b = ObservationScope {
            server_id: "b".into(),
            representation_id: "aac-192".into(),
        };
        history.observe(
            a.clone(),
            sample(0, 20_000, 50, 50, SampleEligibility::Cached),
        );
        for at in [1_000, 2_000] {
            history.observe(
                a.clone(),
                sample(
                    at,
                    30_000,
                    if at == 2_000 { 50 } else { 500 },
                    if at == 2_000 { 50 } else { 500 },
                    SampleEligibility::Eligible,
                ),
            );
        }
        history.observe(
            b.clone(),
            sample(2_000, 1, 0, 0, SampleEligibility::Eligible),
        );
        assert!(history.evidence(&a, 2_000, OBSERVATION_WINDOW_MS).is_none());
        history.observe(
            a.clone(),
            sample(3_000, 25_000, 50, 50, SampleEligibility::Eligible),
        );
        let evidence = history.evidence(&a, 3_000, OBSERVATION_WINDOW_MS).unwrap();
        assert_eq!(evidence.bytes_per_second, 25_000);
        assert!(evidence.depleted);
        assert_ne!(
            history.evidence(&b, 3_000, OBSERVATION_WINDOW_MS),
            history.evidence(&a, 3_000, OBSERVATION_WINDOW_MS)
        );
        for at in 4..30 {
            history.observe(
                a.clone(),
                sample(at * 1_000, 40_000, 500, 500, SampleEligibility::Eligible),
            );
        }
        assert!(history.scopes[&a].len() <= MAX_SAMPLES_PER_SCOPE);
    }

    #[test]
    fn downgrade_and_recovery_use_distinct_hysteresis() {
        let slow = CapacityEvidence {
            bytes_per_second: 30_000,
            eligible_samples: 3,
            oldest_ms: 0,
            newest_ms: 5_000,
            depleted: true,
            compressed_low_observed: true,
            pcm_low_observed: true,
        };
        assert!(!sustainable(32_000, slow, false));
        let burst = CapacityEvidence {
            bytes_per_second: 80_000,
            eligible_samples: 3,
            oldest_ms: 0,
            newest_ms: 5_000,
            depleted: false,
            compressed_low_observed: false,
            pcm_low_observed: false,
        };
        assert!(!sustainable(32_000, burst, true));
        let recovered = CapacityEvidence {
            newest_ms: RECOVERY_WINDOW_MS,
            ..burst
        };
        assert!(sustainable(32_000, recovered, true));
    }

    #[test]
    fn mixed_compressed_and_pcm_low_water_samples_preserve_v1_depletion_semantics() {
        let mut history = AdaptationHistory::default();
        let scope = ObservationScope {
            server_id: "mixed".into(),
            representation_id: "original".into(),
        };
        history.observe(
            scope.clone(),
            sample(1_000, 100_000, 50, 500, SampleEligibility::Eligible),
        );
        history.observe(
            scope.clone(),
            sample(2_000, 100_000, 500, 50, SampleEligibility::Eligible),
        );
        history.observe(
            scope.clone(),
            sample(3_000, 100_000, 500, 500, SampleEligibility::Eligible),
        );
        let evidence = history
            .evidence(&scope, 3_000, OBSERVATION_WINDOW_MS)
            .unwrap();
        assert!(evidence.depleted);
        assert!(evidence.compressed_low_observed);
        assert!(evidence.pcm_low_observed);
    }

    fn representation(
        id: &str,
        required_bytes_per_second: u64,
        provenance: crate::providers::PlaybackProvenance,
    ) -> crate::providers::PlaybackRepresentation {
        crate::providers::PlaybackRepresentation {
            id: crate::providers::RepresentationId(id.into()),
            quality: crate::providers::PlaybackQuality {
                tier: 100,
                required_bytes_per_second: Some(required_bytes_per_second),
            },
            codec: Some("mp3".into()),
            container: Some("mp3".into()),
            bitrate_kbps: Some((required_bytes_per_second / 125) as u32),
            sample_rate: None,
            bit_depth: None,
            provenance,
            seek_mechanism: None,
            request: crate::providers::PlaybackRequest {
                url: reqwest::Url::parse(&format!("https://example.invalid/{id}")).unwrap(),
                headers: reqwest::header::HeaderMap::new(),
                range_supported: false,
                cleanup: None,
                refresh: None,
                expected_content_type: None,
            },
        }
    }

    fn install_slow_evidence(server_id: &str, representation_id: &str, rate: u64) {
        let now = monotonic_ms();
        for age in [2_000, 1_000, 0] {
            record(
                ObservationScope {
                    server_id: server_id.into(),
                    representation_id: representation_id.into(),
                },
                sample(
                    now.saturating_sub(age),
                    rate,
                    50,
                    500,
                    SampleEligibility::Eligible,
                ),
            );
        }
    }

    #[test]
    fn boundary_selection_downgrades_without_committing_before_handoff() {
        let server = "selection-downgrade-fixture";
        commit_selection(server, "original-high", AdaptationReason::Sustainable);
        install_slow_evidence(server, "original-high", 30_000);
        let (selected, reason) = select_for_boundary(
            server,
            vec![
                representation(
                    "original-high",
                    100_000,
                    crate::providers::PlaybackProvenance::Original,
                ),
                representation(
                    "fallback-low",
                    20_000,
                    crate::providers::PlaybackProvenance::Alternative,
                ),
            ],
        )
        .unwrap();
        assert_eq!(selected.id.0, "fallback-low");
        assert_eq!(reason, AdaptationReason::ReducedBufferPressure);
        assert_eq!(selected_status(server).unwrap().0, "original-high");
    }

    #[test]
    fn no_sustainable_fallback_keeps_current_and_reports_limitation() {
        let server = "selection-exhausted-fixture";
        commit_selection(server, "original-high", AdaptationReason::Sustainable);
        install_slow_evidence(server, "original-high", 30_000);
        let (selected, reason) = select_for_boundary(
            server,
            vec![
                representation(
                    "original-high",
                    100_000,
                    crate::providers::PlaybackProvenance::Original,
                ),
                representation(
                    "fallback-too-large",
                    40_000,
                    crate::providers::PlaybackProvenance::Alternative,
                ),
            ],
        )
        .unwrap();
        assert_eq!(selected.id.0, "original-high");
        assert_eq!(reason, AdaptationReason::NoSustainableAlternative);
    }

    #[test]
    fn scope_history_evicts_old_identities() {
        let mut history = AdaptationHistory::default();
        for index in 0..=MAX_OBSERVATION_SCOPES {
            history.observe(
                ObservationScope {
                    server_id: format!("server-{index}"),
                    representation_id: "original".into(),
                },
                sample(1, 1, 500, 500, SampleEligibility::Eligible),
            );
        }
        assert_eq!(history.scopes.len(), MAX_OBSERVATION_SCOPES);
        assert!(
            !history
                .scopes
                .keys()
                .any(|scope| scope.server_id == "server-0")
        );
    }

    #[test]
    fn startup_cache_seek_partial_failure_and_short_samples_never_become_capacity() {
        let mut history = AdaptationHistory::default();
        let scope = ObservationScope {
            server_id: "excluded".into(),
            representation_id: "original".into(),
        };
        for (index, eligibility) in [
            SampleEligibility::Startup,
            SampleEligibility::Cached,
            SampleEligibility::RangeOrSeek,
            SampleEligibility::Partial,
            SampleEligibility::Failed,
            SampleEligibility::TooShort,
        ]
        .into_iter()
        .enumerate()
        {
            history.observe(
                scope.clone(),
                sample(index as u64 * 1_000, 1_000_000, 0, 0, eligibility),
            );
        }
        assert!(
            history
                .evidence(&scope, 10_000, RECOVERY_WINDOW_MS)
                .is_none()
        );
    }

    #[test]
    fn recovery_requires_the_full_recovery_window_not_a_fast_burst() {
        let burst = CapacityEvidence {
            bytes_per_second: 100_000,
            eligible_samples: 16,
            oldest_ms: 60_000,
            newest_ms: 119_999,
            depleted: false,
            compressed_low_observed: false,
            pcm_low_observed: false,
        };
        assert!(!sustainable(40_000, burst, true));
        assert!(sustainable(
            40_000,
            CapacityEvidence {
                oldest_ms: 0,
                newest_ms: RECOVERY_WINDOW_MS,
                ..burst
            },
            true
        ));
    }
}
