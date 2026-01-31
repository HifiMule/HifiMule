use super::*;

#[test]
fn test_daemon_state_variants() {
    // Verify all variants can be constructed and debugged
    let idle = DaemonState::Idle;
    let syncing = DaemonState::Syncing;
    let error = DaemonState::Error;
    
    assert_eq!(format!("{:?}", idle), "Idle");
    assert_eq!(format!("{:?}", syncing), "Syncing");
    assert_eq!(format!("{:?}", error), "Error");
}

#[test]
fn test_load_icon_empty_bytes() {
    // Verify that loading invalid bytes returns an error
    let bytes: [u8; 0] = [];
    let res = load_icon(&bytes, "test_icon");
    assert!(res.is_err());
}

#[test]
fn test_load_icon_garbage_bytes() {
    // Verify that loading garbage bytes returns an error
    let bytes = vec![0, 1, 2, 3];
    let res = load_icon(&bytes, "test_icon");
    assert!(res.is_err());
}
