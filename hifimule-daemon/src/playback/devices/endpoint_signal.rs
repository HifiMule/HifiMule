use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

pub(super) struct EndpointSignal {
    id: Vec<u16>,
    gate: Arc<AtomicBool>,
    lost: Arc<AtomicBool>,
}
impl EndpointSignal {
    pub fn new(id: &str, gate: Arc<AtomicBool>, lost: Arc<AtomicBool>) -> Self {
        Self {
            id: id.encode_utf16().chain(Some(0)).collect(),
            gate,
            lost,
        }
    }
    /// The OS supplies a valid NUL-terminated string for the callback duration.
    /// Comparison is bounded by the already validated selected ID and allocates nothing.
    pub unsafe fn unavailable(&self, id: *const u16) {
        if id.is_null() {
            return;
        }
        for (index, expected) in self.id.iter().enumerate() {
            // Stop at the first mismatch, including an earlier NUL terminator.
            if unsafe { *id.add(index) } != *expected {
                return;
            }
        }
        self.gate.store(false, Ordering::Release);
        self.lost.store(true, Ordering::Release);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_exact_selected_endpoint_gates_audio_and_loss_is_sticky() {
        let gate = Arc::new(AtomicBool::new(true));
        let lost = Arc::new(AtomicBool::new(false));
        let signal = EndpointSignal::new("selected-endpoint", gate.clone(), lost.clone());
        unsafe {
            signal.unavailable(std::ptr::null());
        }
        for id in ["other", "selected", "selected-endpoint-extra", ""] {
            let wide: Vec<_> = id.encode_utf16().chain(Some(0)).collect();
            unsafe {
                signal.unavailable(wide.as_ptr());
            }
            assert!(gate.load(Ordering::Acquire));
            assert!(!lost.load(Ordering::Acquire));
        }
        let selected: Vec<_> = "selected-endpoint".encode_utf16().chain(Some(0)).collect();
        unsafe {
            signal.unavailable(selected.as_ptr());
        }
        assert!(!gate.load(Ordering::Acquire));
        assert!(lost.load(Ordering::Acquire));
    }
}
