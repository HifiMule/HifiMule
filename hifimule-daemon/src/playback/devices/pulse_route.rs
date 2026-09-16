//! DONT_MOVE pins a sink, not a port within it. We cannot control server policy.
//! Physical sinks must expose one concrete port; multi-port/unknown routes are
//! unavailable. Virtual sinks are explicitly labeled and do not certify physical
//! routing. Apply this policy both in discovery and to the actually opened sink.
pub(super) fn supported(virtual_output: bool, port_count: usize, sole_port_active: bool) -> bool {
    virtual_output || (port_count == 1 && sole_port_active)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switchable_or_unknown_physical_ports_cannot_certify_speaker_safety() {
        assert!(!supported(false, 2, true)); // Headphones and speakers, same sink.
        assert!(!supported(false, 0, false)); // No verifiable port topology.
        assert!(!supported(false, 1, false)); // Selected port is not the sole port.
        assert!(supported(false, 1, true));
        assert!(supported(true, 0, false)); // Explicitly virtual downstream routing.
    }
}
