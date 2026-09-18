//! Translate reset-relative native clocks into cumulative presented frame positions.
#[derive(Default)]
pub(crate) struct PresentationEpoch {
    presented_base: u64,
    generation: u64,
}

impl PresentationEpoch {
    pub(crate) fn presented_frames(&self, position: u64, frequency: u64, rate: u32) -> u64 {
        assert_ne!(frequency, 0, "native clock frequency must be validated");
        let frames = u128::from(position) * u128::from(rate) / u128::from(frequency);
        self.presented_base
            .saturating_add(frames.min(u128::from(u64::MAX)) as u64)
    }

    // Call only after Reset succeeds: a failed reset retains the previous origin.
    pub(crate) fn did_reset(&mut self, presented_frames: u64) {
        self.presented_base = presented_frames;
        self.generation = self.generation.wrapping_add(1);
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resets_preserve_presented_origin_without_counting_discarded_padding() {
        let mut clock = PresentationEpoch::default();
        let first = clock.presented_frames(48_000, 48_000, 48_000);
        assert_eq!(first, 48_000);
        clock.did_reset(first);
        assert_eq!(clock.generation(), 1);
        // An immediate repeated pause sees the reset clock at zero.
        let repeated = clock.presented_frames(0, 48_000, 48_000);
        assert_eq!(repeated, first);
        clock.did_reset(repeated);
        assert_eq!(clock.generation(), 2);
        // Pending audio discarded by Reset contributes only after its replay.
        let resumed = clock.presented_frames(240, 48_000, 48_000);
        assert_eq!(resumed, 48_240);
        clock.did_reset(resumed);
        assert_eq!(clock.presented_frames(480, 48_000, 48_000), 48_720);
    }

    #[test]
    fn clock_units_are_rescaled_without_intermediate_overflow() {
        let clock = PresentationEpoch::default();
        assert_eq!(
            clock.presented_frames(10_000_000, 10_000_000, 48_000),
            48_000
        );
        assert_eq!(clock.presented_frames(u64::MAX, u64::MAX, 48_000), 48_000);
    }
}
