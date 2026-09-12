use hifimule_lifecycle::{
    DESCRIPTOR_MAX_BYTES, LifecycleErrorCode, OwnerDescriptor, OwnerGuard, PROTOCOL_VERSION,
    SCHEMA_VERSION, cancel_launch_ticket, constant_time_token_eq, create_launch_ticket,
    read_descriptor, validate_launch_ticket,
};

#[test]
fn descriptor_round_trips_and_rejects_mismatched_protocol() {
    let temp = tempfile::tempdir().unwrap();
    let mut owner = OwnerGuard::acquire(temp.path()).unwrap();
    let descriptor = owner.publish_ready(32123).unwrap();
    assert_eq!(descriptor.schema_version, SCHEMA_VERSION);
    assert_eq!(descriptor.protocol_version, PROTOCOL_VERSION);
    assert_eq!(descriptor.token.len(), 64);

    let discovered = read_descriptor(temp.path()).unwrap();
    assert_eq!(discovered.instance_id, descriptor.instance_id);
    discovered.validate_compatible().unwrap();

    let mut incompatible: OwnerDescriptor = discovered;
    incompatible.protocol_version += 1;
    assert_eq!(
        incompatible.validate_compatible().unwrap_err().code(),
        LifecycleErrorCode::ProtocolMismatch
    );
}

#[test]
fn ownership_is_exclusive_and_released_without_unlinking_lock() {
    let temp = tempfile::tempdir().unwrap();
    let owner = OwnerGuard::acquire(temp.path()).unwrap();
    let error = OwnerGuard::acquire(temp.path()).unwrap_err();
    assert_eq!(error.code(), LifecycleErrorCode::OwnerChanged);
    let lock_path = temp.path().join("runtime/owner.lock");
    drop(owner);
    assert!(lock_path.exists());
    OwnerGuard::acquire(temp.path()).unwrap();
}

#[test]
fn malformed_and_oversized_discovery_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let runtime = temp.path().join("runtime");
    std::fs::create_dir_all(&runtime).unwrap();
    std::fs::write(
        runtime.join("owner.json"),
        vec![b'x'; DESCRIPTOR_MAX_BYTES + 1],
    )
    .unwrap();
    assert!(read_descriptor(temp.path()).is_err());
    std::fs::write(runtime.join("owner.json"), b"not json").unwrap();
    assert!(read_descriptor(temp.path()).is_err());
}

#[test]
fn tokens_use_timing_resistant_comparison() {
    assert!(constant_time_token_eq(b"same", b"same"));
    assert!(!constant_time_token_eq(b"same", b"sand"));
    assert!(!constant_time_token_eq(b"same", b"shorter"));
}

#[test]
fn runtime_path_must_be_absolute_and_not_a_symlink() {
    assert_eq!(
        OwnerGuard::acquire(std::path::Path::new("relative-profile"))
            .unwrap_err()
            .code(),
        LifecycleErrorCode::UnsafeRuntimePath
    );
}

#[test]
fn simultaneous_process_candidates_have_one_owner_and_crash_releases_lock() {
    use std::io::BufRead;
    let temp = tempfile::tempdir().unwrap();
    let binary = env!("CARGO_BIN_EXE_lifecycle-owner-probe");
    let mut first = std::process::Command::new(binary)
        .arg(temp.path())
        .arg("750")
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    std::io::BufReader::new(first.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert_eq!(line.trim(), "acquired");

    let contender = std::process::Command::new(binary)
        .arg(temp.path())
        .arg("0")
        .status()
        .unwrap();
    assert_eq!(contender.code(), Some(2));
    first.kill().unwrap();
    let _ = first.wait().unwrap();
    assert!(
        std::process::Command::new(binary)
            .arg(temp.path())
            .arg("0")
            .status()
            .unwrap()
            .success()
    );
}

#[test]
fn cancelled_and_old_generation_tickets_cannot_initialize() {
    let temp = tempfile::tempdir().unwrap();
    let ticket = create_launch_ticket(temp.path(), 0).unwrap();
    validate_launch_ticket(temp.path(), &ticket, 0).unwrap();
    assert_eq!(
        validate_launch_ticket(temp.path(), &ticket, 1)
            .unwrap_err()
            .code(),
        LifecycleErrorCode::DaemonStopped
    );
    cancel_launch_ticket(temp.path(), &ticket).unwrap();
    assert!(validate_launch_ticket(temp.path(), &ticket, 0).is_err());
}

#[test]
fn ownership_handle_is_not_inherited_by_children() {
    let temp = tempfile::tempdir().unwrap();
    let owner = OwnerGuard::acquire(temp.path()).unwrap();
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_lifecycle-owner-probe"))
        .arg(temp.path().join("different-profile"))
        .arg("500")
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    drop(owner);
    OwnerGuard::acquire(temp.path()).unwrap();
    assert!(child.wait().unwrap().success());
}

#[cfg(unix)]
#[test]
fn unsafe_runtime_permissions_are_rejected() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let runtime = temp.path().join("runtime");
    std::fs::create_dir(&runtime).unwrap();
    std::fs::set_permissions(&runtime, std::fs::Permissions::from_mode(0o777)).unwrap();
    assert_eq!(
        read_descriptor(temp.path()).unwrap_err().code(),
        LifecycleErrorCode::LocalAccessDenied
    );
}
