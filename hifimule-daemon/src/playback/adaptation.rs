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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapacityEvidence {
    pub bytes_per_second: u64,
    pub eligible_samples: usize,
    pub oldest_ms: u64,
    pub newest_ms: u64,
    pub depleted: bool,
}

#[derive(Debug, Default)]
pub struct AdaptationHistory {
    scopes: HashMap<ObservationScope, VecDeque<DeliveryObservation>>,
}

#[derive(Default)]
struct RuntimePolicy {
    history: AdaptationHistory,
    selected: HashMap<String, String>,
    reasons: HashMap<String, AdaptationReason>,
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

pub(crate) fn select_for_boundary(
    server_id: &str,
    representations: Vec<crate::providers::PlaybackRepresentation>,
) -> Result<
    (crate::providers::PlaybackRepresentation, AdaptationReason),
    crate::providers::ProviderError,
> {
    let ordered = crate::providers::select_ordered_playback_representations(representations)?;
    let mut runtime = RUNTIME
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let current_id = runtime.selected.get(server_id).cloned();
    let current_index = current_id
        .as_ref()
        .and_then(|id| ordered.iter().position(|candidate| &candidate.id.0 == id));
    let Some(index) = current_index else {
        let selected = ordered.into_iter().next().unwrap();
        runtime
            .selected
            .insert(server_id.into(), selected.id.0.clone());
        runtime
            .reasons
            .insert(server_id.into(), AdaptationReason::Startup);
        return Ok((selected, AdaptationReason::Startup));
    };
    let scope = ObservationScope {
        server_id: server_id.into(),
        representation_id: ordered[index].id.0.clone(),
    };
    let Some(evidence) = runtime.history.evidence(&scope, monotonic_ms()) else {
        return Ok((
            ordered.into_iter().nth(index).unwrap(),
            AdaptationReason::Sustainable,
        ));
    };
    let current_required = ordered[index].quality.required_bytes_per_second;
    if evidence.depleted
        && current_required.is_some_and(|required| !sustainable(required, evidence, false))
    {
        let chosen = ordered
            .iter()
            .skip(index + 1)
            .find(|candidate| {
                candidate
                    .quality
                    .required_bytes_per_second
                    .is_none_or(|required| sustainable(required, evidence, false))
            })
            .cloned()
            .unwrap_or_else(|| ordered.last().unwrap().clone());
        runtime
            .selected
            .insert(server_id.into(), chosen.id.0.clone());
        runtime
            .reasons
            .insert(server_id.into(), AdaptationReason::ReducedBufferPressure);
        return Ok((chosen, AdaptationReason::ReducedBufferPressure));
    }
    if index > 0 {
        if let Some(chosen) = ordered[..index]
            .iter()
            .find(|candidate| {
                candidate
                    .quality
                    .required_bytes_per_second
                    .is_some_and(|required| sustainable(required, evidence, true))
            })
            .cloned()
        {
            runtime
                .selected
                .insert(server_id.into(), chosen.id.0.clone());
            runtime
                .reasons
                .insert(server_id.into(), AdaptationReason::RecoveredCapacity);
            return Ok((chosen, AdaptationReason::RecoveredCapacity));
        }
    }
    Ok((
        ordered.into_iter().nth(index).unwrap(),
        AdaptationReason::Sustainable,
    ))
}

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

impl AdaptationHistory {
    pub fn observe(&mut self, scope: ObservationScope, sample: DeliveryObservation) {
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
    }

    pub fn evidence(&self, scope: &ObservationScope, now_ms: u64) -> Option<CapacityEvidence> {
        let eligible: Vec<_> = self
            .scopes
            .get(scope)?
            .iter()
            .copied()
            .filter(|sample| {
                now_ms.saturating_sub(sample.monotonic_ms) <= OBSERVATION_WINDOW_MS
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
        Some(CapacityEvidence {
            bytes_per_second,
            eligible_samples: eligible.len(),
            oldest_ms: eligible.first()?.monotonic_ms,
            newest_ms: eligible.last()?.monotonic_ms,
            depleted: eligible
                .iter()
                .filter(|sample| sample.compressed_buffer_ms < 100 || sample.pcm_buffer_ms < 100)
                .count()
                >= 2,
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
            || evidence.newest_ms.saturating_sub(evidence.oldest_ms) >= OBSERVATION_WINDOW_MS)
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
        assert!(history.evidence(&a, 2_000).is_none());
        history.observe(
            a.clone(),
            sample(3_000, 25_000, 50, 50, SampleEligibility::Eligible),
        );
        let evidence = history.evidence(&a, 3_000).unwrap();
        assert_eq!(evidence.bytes_per_second, 25_000);
        assert!(evidence.depleted);
        assert_ne!(history.evidence(&b, 3_000), history.evidence(&a, 3_000));
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
        };
        assert!(!sustainable(32_000, slow, false));
        let burst = CapacityEvidence {
            bytes_per_second: 80_000,
            eligible_samples: 3,
            oldest_ms: 0,
            newest_ms: 5_000,
            depleted: false,
        };
        assert!(!sustainable(32_000, burst, true));
        let recovered = CapacityEvidence {
            newest_ms: 60_000,
            ..burst
        };
        assert!(sustainable(32_000, recovered, true));
    }
}
