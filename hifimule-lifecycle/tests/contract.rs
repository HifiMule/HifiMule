use hifimule_lifecycle::{
    DESCRIPTOR_MAX_BYTES, LifecycleErrorCode, OwnerDescriptor, OwnerGuard, PROTOCOL_VERSION,
    SCHEMA_VERSION, UiInstanceGuard, acknowledge_ui_activation, cancel_launch_ticket,
    constant_time_token_eq, create_launch_ticket, read_descriptor, read_ui_activation_request,
    request_ui_activation, validate_launch_ticket,
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
fn ui_loser_requests_activation_across_processes_and_exit_releases_lock() {
    use std::io::BufRead;
    let profile = tempfile::tempdir().unwrap();
    let other_profile = tempfile::tempdir().unwrap();
    let binary = env!("CARGO_BIN_EXE_lifecycle-owner-probe");
    let mut winner = std::process::Command::new(binary)
        .arg(profile.path())
        .arg("5000")
        .arg("--ui")
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    std::io::BufReader::new(winner.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert_eq!(line.trim(), "acquired");
    assert!(UiInstanceGuard::acquire(profile.path()).unwrap().is_none());
    assert!(
        UiInstanceGuard::acquire(other_profile.path())
            .unwrap()
            .is_some()
    );
    assert_eq!(read_ui_activation_request(profile.path()).unwrap(), None);
    let loser = std::process::Command::new(binary)
        .arg(profile.path())
        .arg("0")
        .arg("--ui")
        .output()
        .unwrap();
    assert!(loser.status.success());
    assert_eq!(loser.stdout, b"requested\n");
    let first_request = read_ui_activation_request(profile.path()).unwrap().unwrap();
    let second_request = request_ui_activation(profile.path()).unwrap();
    assert_ne!(first_request, second_request);
    assert_eq!(
        read_ui_activation_request(profile.path()).unwrap(),
        Some(second_request)
    );
    winner.kill().unwrap();
    winner.wait().unwrap();
    assert!(profile.path().join("runtime/ui.lock").exists());
    assert!(UiInstanceGuard::acquire(profile.path()).unwrap().is_some());
}

#[test]
fn ui_loser_takes_over_when_owner_exits_before_activation() {
    use std::io::BufRead;
    let profile = tempfile::tempdir().unwrap();
    let guard = UiInstanceGuard::acquire(profile.path()).unwrap().unwrap();
    let binary = env!("CARGO_BIN_EXE_lifecycle-owner-probe");
    let mut loser = std::process::Command::new(binary)
        .arg(profile.path())
        .arg("0")
        .arg("--ui-handoff")
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut output = std::io::BufReader::new(loser.stdout.take().unwrap());
    let mut first = String::new();
    output.read_line(&mut first).unwrap();
    assert_eq!(first.trim(), "requested");
    drop(guard);
    let mut result = String::new();
    output.read_line(&mut result).unwrap();
    assert_eq!(result.trim(), "acquired");
    assert!(loser.wait().unwrap().success());
}

#[test]
fn ui_loser_exits_promptly_after_activation_acknowledgment() {
    use std::io::BufRead;
    let profile = tempfile::tempdir().unwrap();
    let _guard = UiInstanceGuard::acquire(profile.path()).unwrap().unwrap();
    let binary = env!("CARGO_BIN_EXE_lifecycle-owner-probe");
    let mut loser = std::process::Command::new(binary)
        .arg(profile.path())
        .arg("0")
        .arg("--ui-handoff")
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut output = std::io::BufReader::new(loser.stdout.take().unwrap());
    let mut first = String::new();
    output.read_line(&mut first).unwrap();
    assert_eq!(first.trim(), "requested");
    let request_id = read_ui_activation_request(profile.path()).unwrap().unwrap();
    let started = std::time::Instant::now();
    acknowledge_ui_activation(profile.path(), &request_id).unwrap();
    let mut result = String::new();
    output.read_line(&mut result).unwrap();
    assert_eq!(result.trim(), "handled");
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
    assert!(loser.wait().unwrap().success());
}

#[test]
fn malformed_or_oversized_ui_requests_are_rejected() {
    let profile = tempfile::tempdir().unwrap();
    request_ui_activation(profile.path()).unwrap();
    let path = profile.path().join("runtime/ui-activation.json");
    std::fs::write(&path, b"not json").unwrap();
    assert!(read_ui_activation_request(profile.path()).is_err());
    std::fs::write(&path, vec![b'x'; 257]).unwrap();
    assert!(read_ui_activation_request(profile.path()).is_err());
}

#[test]
fn simultaneous_activation_requests_coalesce_to_a_valid_request() {
    let profile = tempfile::tempdir().unwrap();
    let _winner = UiInstanceGuard::acquire(profile.path()).unwrap().unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(9));
    let workers: Vec<_> = (0..8)
        .map(|_| {
            let path = profile.path().to_path_buf();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                request_ui_activation(&path).unwrap()
            })
        })
        .collect();
    barrier.wait();
    let issued: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    let stored = read_ui_activation_request(profile.path()).unwrap().unwrap();
    assert!(issued.contains(&stored));
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

#[test]
fn contended_ticket_lock_respects_deadline() {
    use hifimule_lifecycle::{lock_launch_ticket, lock_launch_ticket_until};
    use std::time::{Duration, Instant};
    let temp = tempfile::tempdir().unwrap();
    let held = lock_launch_ticket(temp.path()).unwrap();
    let started = Instant::now();
    let error = lock_launch_ticket_until(temp.path(), started + Duration::from_millis(60))
        .err()
        .unwrap();
    assert_eq!(error.code(), LifecycleErrorCode::StartupTimeout);
    assert!(started.elapsed() < Duration::from_secs(1));
    drop(held);
    assert!(
        lock_launch_ticket_until(temp.path(), Instant::now() + Duration::from_millis(60)).is_ok()
    );
}

#[test]
fn startup_failure_is_sanitized_and_attempt_scoped() {
    use hifimule_lifecycle::{clear_launch_failure, publish_launch_failure, read_launch_failure};
    let temp = tempfile::tempdir().unwrap();
    let id = create_launch_ticket(temp.path(), 0).unwrap();
    let other = create_launch_ticket(temp.path(), 0).unwrap();
    publish_launch_failure(temp.path(), &id, LifecycleErrorCode::LegacyDaemonRunning).unwrap();
    assert_eq!(
        read_launch_failure(temp.path(), &id).unwrap(),
        Some(LifecycleErrorCode::LegacyDaemonRunning)
    );
    assert_eq!(read_launch_failure(temp.path(), &other).unwrap(), None);
    let saved =
        std::fs::read_to_string(temp.path().join(format!("runtime/failure-{id}.json"))).unwrap();
    assert_eq!(saved, "\"LEGACY_DAEMON_RUNNING\"");
    clear_launch_failure(temp.path(), &id);
    assert_eq!(read_launch_failure(temp.path(), &id).unwrap(), None);
}

#[test]
fn health_reports_access_protocol_identity_and_stopping_distinctly() {
    use hifimule_lifecycle::{LifecycleState, check_owner_health};
    use std::io::{Read, Write};
    use std::time::Duration;
    let temp = tempfile::tempdir().unwrap();
    let owner = OwnerGuard::acquire(temp.path()).unwrap();
    for (http_status, body, expected) in [
        (401, "{}", Err(LifecycleErrorCode::LocalAccessDenied)),
        (
            200,
            r#"{"result":{"data":{"protocolVersion":2,"instanceId":"owner","status":"ok"}}}"#,
            Err(LifecycleErrorCode::ProtocolMismatch),
        ),
        (
            200,
            r#"{"result":{"data":{"protocolVersion":1,"instanceId":"other","status":"ok"}}}"#,
            Err(LifecycleErrorCode::OwnerChanged),
        ),
        (
            200,
            r#"{"result":{"data":{"protocolVersion":1,"instanceId":"owner","status":"stopping"}}}"#,
            Ok(LifecycleState::Stopping),
        ),
    ] {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let mut descriptor = owner
            .prepare_descriptor(listener.local_addr().unwrap().port())
            .unwrap();
        descriptor.instance_id = "owner".into();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = [0; 4096];
            let length = stream.read(&mut request).unwrap();
            assert!(
                String::from_utf8_lossy(&request[..length])
                    .to_lowercase()
                    .contains("authorization: bearer ")
            );
            write!(stream, "HTTP/1.1 {http_status} Result\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        });
        assert_eq!(
            check_owner_health(&descriptor, Duration::from_secs(2)).map_err(|error| error.code()),
            expected
        );
        server.join().unwrap();
    }
}

#[test]
fn ui_evidence_contains_hydration_identity_but_no_credentials() {
    let temp = tempfile::tempdir().unwrap();
    let owner = OwnerGuard::acquire(temp.path()).unwrap();
    let descriptor = owner.prepare_descriptor(31234).unwrap();
    let marker = uuid::Uuid::new_v4().to_string();
    hifimule_lifecycle::publish_ui_ready(temp.path(), &marker, &descriptor).unwrap();
    let bytes =
        std::fs::read_to_string(temp.path().join(format!("runtime/ui-ready-{marker}.json")))
            .unwrap();
    assert!(!bytes.contains(&descriptor.token));
    let evidence: serde_json::Value = serde_json::from_str(&bytes).unwrap();
    assert_eq!(evidence["state"], "hydrated");
    assert_eq!(evidence["instanceId"], descriptor.instance_id);
    assert_eq!(evidence["uiPid"], std::process::id());
    assert!(
        hifimule_lifecycle::publish_ui_ready(temp.path(), "../../outside", &descriptor).is_err()
    );
}
