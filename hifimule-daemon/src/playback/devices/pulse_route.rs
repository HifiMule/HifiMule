//! DONT_MOVE pins a sink, not a port within it. We cannot control server policy.
//! Physical sinks normally need one concrete active port. A stable default with
//! a known active multi-port route is a narrowly scoped last resort when no
//! certified route exists. Virtual sinks remain explicitly labeled and do not
//! certify physical routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Eligibility {
    Certified,
    LastResort,
    Unsupported,
}

pub(super) fn supported(virtual_output: bool, port_count: usize, active_port_known: bool) -> bool {
    virtual_output || (port_count == 1 && active_port_known)
}

pub(super) fn eligibility(
    virtual_output: bool,
    port_count: usize,
    active_port_known: bool,
    is_default: bool,
    identity_stable: bool,
    has_certified_output: bool,
    inventory_complete: bool,
) -> Eligibility {
    if supported(virtual_output, port_count, active_port_known) && identity_stable {
        Eligibility::Certified
    } else if !virtual_output
        && port_count > 1
        && active_port_known
        && is_default
        && identity_stable
        && !has_certified_output
        && inventory_complete
    {
        Eligibility::LastResort
    } else {
        Eligibility::Unsupported
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_only_a_stable_default_multi_port_sink_as_last_resort() {
        assert_eq!(
            eligibility(false, 2, true, true, true, false, true),
            Eligibility::LastResort
        );
        assert_eq!(
            eligibility(false, 2, true, true, true, true, true),
            Eligibility::Unsupported
        );
    }

    #[test]
    fn rejects_uncertain_or_non_default_last_resort_candidates() {
        assert_eq!(
            eligibility(false, 2, false, true, true, false, true),
            Eligibility::Unsupported
        );
        assert_eq!(
            eligibility(false, 2, true, false, true, false, true),
            Eligibility::Unsupported
        );
        assert_eq!(
            eligibility(false, 2, true, true, false, false, true),
            Eligibility::Unsupported
        );
    }

    #[test]
    fn rejects_last_resort_when_output_inventory_is_incomplete() {
        assert_eq!(
            eligibility(false, 2, true, true, true, false, false),
            Eligibility::Unsupported
        );
    }

    #[test]
    fn switchable_or_unknown_physical_ports_cannot_certify_speaker_safety() {
        assert!(!supported(false, 2, true)); // Headphones and speakers, same sink.
        assert!(!supported(false, 0, false)); // No verifiable port topology.
        assert!(!supported(false, 1, false)); // Selected port is not the sole port.
        assert!(supported(false, 1, true));
        assert!(supported(true, 0, false)); // Explicitly virtual downstream routing.
    }
}
