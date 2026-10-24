use hifimule_lifecycle::{
    DESCRIPTOR_MAX_BYTES, LifecycleErrorCode, OwnerDescriptor, OwnerGuard, PROTOCOL_VERSION,
    SCHEMA_VERSION, UiInstanceGuard, acknowledge_ui_activation, cancel_launch_ticket,
    constant_time_token_eq, create_launch_ticket, read_descriptor, read_ui_activation_request,
    request_ui_activation, validate_launch_ticket,
};

#[test]
fn playback_settings_activation_is_explicit_and_acknowledged() {
    let profile = tempfile::tempdir().unwrap();
    let request_id =
        hifimule_lifecycle::request_ui_playback_settings_activation(profile.path()).unwrap();
    let request = hifimule_lifecycle::read_ui_activation_detail(profile.path())
        .unwrap()
        .unwrap();
    assert_eq!(request.request_id, request_id);
    assert!(request.open_playback_settings);
    assert!(!request.open_audio_output);
    acknowledge_ui_activation(profile.path(), &request_id).unwrap();
    assert_eq!(
        hifimule_lifecycle::read_ui_activation_ack(profile.path()).unwrap(),
        Some(request_id)
    );
    request_ui_activation(profile.path()).unwrap();
    assert!(
        !hifimule_lifecycle::read_ui_activation_detail(profile.path())
            .unwrap()
            .unwrap()
            .open_playback_settings
    );
    hifimule_lifecycle::request_ui_audio_output_activation(profile.path()).unwrap();
    let request = hifimule_lifecycle::read_ui_activation_detail(profile.path())
        .unwrap()
        .unwrap();
    assert!(request.open_audio_output);
    assert!(!request.open_playback_settings);
}

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

type ActivationTarget = fn(&std::path::Path) -> Result<String, hifimule_lifecycle::LifecycleError>;

fn activation_targets() -> [(ActivationTarget, [bool; 3], &'static str); 3] {
    [
        (
            hifimule_lifecycle::request_ui_playback_settings_activation,
            [true, false, false],
            "settings",
        ),
        (
            hifimule_lifecycle::request_ui_audio_output_activation,
            [false, true, false],
            "output",
        ),
        (
            hifimule_lifecycle::request_ui_device_profiles_activation,
            [false, false, true],
            "profiles",
        ),
    ]
}

fn assert_activation_target(profile: &std::path::Path, request_id: &str, flags: [bool; 3]) {
    let request = hifimule_lifecycle::read_ui_activation_detail(profile)
        .unwrap()
        .unwrap();
    assert_eq!(request.request_id, request_id);
    assert_eq!(
        [
            request.open_playback_settings,
            request.open_audio_output,
            request.open_device_profiles
        ],
        flags
    );
}

// New process regressions must fail within a bound and reap children on panic.
struct UiProbe {
    child: std::process::Child,
    lines: std::sync::mpsc::Receiver<String>,
}

impl UiProbe {
    fn spawn(profile: &std::path::Path, args: &[&str]) -> Self {
        use std::io::BufRead;
        let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_lifecycle-owner-probe"))
            .arg(profile)
            .args(["0", "--ui-handoff"])
            .args(args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (sender, lines) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(stdout).lines() {
                if sender.send(line.unwrap()).is_err() {
                    break;
                }
            }
        });
        Self { child, lines }
    }

    fn line(&self) -> String {
        self.lines
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("UI probe did not report progress within five seconds")
    }

    fn finish(mut self) -> std::process::ExitStatus {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "UI probe did not exit within five seconds"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}

impl Drop for UiProbe {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn correlated_duplicates_preserve_every_target_and_new_owners_dispatch_once_despite_prior_ack() {
    use hifimule_lifecycle::UI_ACTIVATION_REQUEST_ARG as ARG;
    for (post, flags, destination) in activation_targets() {
        let profile = tempfile::tempdir().unwrap();
        let _daemon = OwnerGuard::acquire(profile.path()).unwrap();
        let guard = UiInstanceGuard::acquire(profile.path()).unwrap().unwrap();
        let request_id = post(profile.path()).unwrap();
        let duplicate = UiProbe::spawn(profile.path(), &[ARG, &request_id]);
        assert_eq!(duplicate.line(), "requested");
        assert_activation_target(profile.path(), &request_id, flags);
        assert!(UiInstanceGuard::acquire(profile.path()).unwrap().is_none());
        assert_eq!(
            OwnerGuard::acquire(profile.path()).unwrap_err().code(),
            LifecycleErrorCode::OwnerChanged
        );
        acknowledge_ui_activation(profile.path(), &request_id).unwrap();
        assert_eq!(duplicate.line(), "handled");
        assert!(duplicate.finish().success());

        // An acknowledgment before a delayed duplicate starts must not create
        // another generic request, and the live owner still handles it.
        let delayed = UiProbe::spawn(profile.path(), &[ARG, &request_id]);
        assert_eq!(delayed.line(), "requested");
        assert_eq!(delayed.line(), "handled");
        assert!(delayed.finish().success());
        assert_activation_target(profile.path(), &request_id, flags);

        // The old owner has exited after acknowledgment. The new owner must
        // actually dispatch the matching surface, not merely keep its mailbox.
        drop(guard);
        let reopened = UiProbe::spawn(profile.path(), &[ARG, &request_id]);
        assert_eq!(reopened.line(), "acquired");
        assert_eq!(reopened.line(), format!("ready:{destination}"));
        assert_eq!(reopened.line(), "ready:none");
        assert!(reopened.finish().success());
        assert_activation_target(profile.path(), &request_id, flags);
    }
}

#[test]
fn correlated_duplicate_takes_over_and_dispatches_every_target_when_owner_exits_during_handoff() {
    for (post, flags, destination) in activation_targets() {
        let profile = tempfile::tempdir().unwrap();
        let guard = UiInstanceGuard::acquire(profile.path()).unwrap().unwrap();
        let request_id = post(profile.path()).unwrap();
        let duplicate = UiProbe::spawn(
            profile.path(),
            &[hifimule_lifecycle::UI_ACTIVATION_REQUEST_ARG, &request_id],
        );
        assert_eq!(duplicate.line(), "requested");
        drop(guard);
        assert_eq!(duplicate.line(), "acquired");
        assert_eq!(duplicate.line(), format!("ready:{destination}"));
        assert_eq!(duplicate.line(), "ready:none");
        assert!(duplicate.finish().success());
        assert_activation_target(profile.path(), &request_id, flags);
    }
}

#[test]
fn delayed_launcher_neither_replaces_nor_replays_a_superseded_menu_choice() {
    let profile = tempfile::tempdir().unwrap();
    let guard = UiInstanceGuard::acquire(profile.path()).unwrap().unwrap();
    let older =
        hifimule_lifecycle::request_ui_playback_settings_activation(profile.path()).unwrap();
    let newer = hifimule_lifecycle::request_ui_device_profiles_activation(profile.path()).unwrap();
    acknowledge_ui_activation(profile.path(), &newer).unwrap();
    let duplicate = UiProbe::spawn(
        profile.path(),
        &[hifimule_lifecycle::UI_ACTIVATION_REQUEST_ARG, &older],
    );
    assert_eq!(duplicate.line(), "requested");
    assert_eq!(duplicate.line(), "handled");
    assert!(duplicate.finish().success());
    assert_activation_target(profile.path(), &newer, [false, false, true]);
    drop(guard);
    let reopened = UiProbe::spawn(
        profile.path(),
        &[hifimule_lifecycle::UI_ACTIVATION_REQUEST_ARG, &older],
    );
    assert_eq!(reopened.line(), "acquired");
    assert_eq!(reopened.line(), "ready:none");
    assert_eq!(reopened.line(), "ready:none");
    assert!(reopened.finish().success());
    assert_activation_target(profile.path(), &newer, [false, false, true]);

    // A superseded launcher that wins ownership still dispatches a newer,
    // unacknowledged destination through normal readiness delivery.
    let latest = hifimule_lifecycle::request_ui_audio_output_activation(profile.path()).unwrap();
    let reopened = UiProbe::spawn(
        profile.path(),
        &[hifimule_lifecycle::UI_ACTIVATION_REQUEST_ARG, &older],
    );
    assert_eq!(reopened.line(), "acquired");
    assert_eq!(reopened.line(), "ready:output");
    assert_eq!(reopened.line(), "ready:none");
    assert!(reopened.finish().success());
    assert_activation_target(profile.path(), &latest, [false, true, false]);
}

#[test]
fn manual_duplicate_posts_fresh_foreground_activation_instead_of_adopting_stale_target() {
    let profile = tempfile::tempdir().unwrap();
    let _guard = UiInstanceGuard::acquire(profile.path()).unwrap().unwrap();
    let stale = hifimule_lifecycle::request_ui_device_profiles_activation(profile.path()).unwrap();
    let duplicate = UiProbe::spawn(profile.path(), &[]);
    assert_eq!(duplicate.line(), "requested");
    let fresh = read_ui_activation_request(profile.path()).unwrap().unwrap();
    assert_ne!(fresh, stale);
    assert_activation_target(profile.path(), &fresh, [false, false, false]);
    acknowledge_ui_activation(profile.path(), &fresh).unwrap();
    assert_eq!(duplicate.line(), "handled");
    assert!(duplicate.finish().success());
}

#[test]
fn invalid_launch_correlation_never_replaces_the_mailbox() {
    use hifimule_lifecycle::UI_ACTIVATION_REQUEST_ARG as ARG;
    let profile = tempfile::tempdir().unwrap();
    let _guard = UiInstanceGuard::acquire(profile.path()).unwrap().unwrap();
    let request_id =
        hifimule_lifecycle::request_ui_audio_output_activation(profile.path()).unwrap();
    for args in [
        vec![ARG],
        vec![ARG, "not-a-uuid"],
        vec![ARG, &request_id, ARG, &request_id],
    ] {
        assert_eq!(
            UiProbe::spawn(profile.path(), &args).finish().code(),
            Some(3)
        );
        assert_activation_target(profile.path(), &request_id, [false, true, false]);
    }
    assert!(
        hifimule_lifecycle::request_ui_activation_for_launch(profile.path(), Some("invalid"))
            .is_err()
    );
    assert_activation_target(profile.path(), &request_id, [false, true, false]);
}

#[test]
fn ui_launch_parser_distinguishes_manual_launches_from_valid_correlation() {
    use hifimule_lifecycle::parse_ui_activation_request_args as parse;
    let profile = tempfile::tempdir().unwrap();
    let request_id = request_ui_activation(profile.path()).unwrap();
    assert_eq!(
        parse(["--smoke-id", "unrelated"].map(Into::into)).unwrap(),
        None
    );
    assert_eq!(
        parse(
            [
                "--smoke-id",
                "unrelated",
                hifimule_lifecycle::UI_ACTIVATION_REQUEST_ARG,
                &request_id
            ]
            .map(Into::into)
        )
        .unwrap(),
        Some(request_id)
    );
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        assert!(
            parse([
                hifimule_lifecycle::UI_ACTIVATION_REQUEST_ARG.into(),
                std::ffi::OsString::from_vec(vec![0xff])
            ])
            .is_err()
        );
    }
}

#[test]
fn simultaneous_activation_requests_coalesce_to_a_valid_request() {
    assert_simultaneous_activation_requests(true);
}

#[test]
fn simultaneous_activation_requests_initialize_a_valid_runtime() {
    assert_simultaneous_activation_requests(false);
}

#[cfg(windows)]
#[test]
fn activation_replacement_preserves_the_mailbox_on_permanent_access_denial() {
    let profile = tempfile::tempdir().unwrap();
    let first_request = request_ui_activation(profile.path()).unwrap();
    let path = profile.path().join("runtime/ui-activation.json");
    let writable = std::fs::metadata(&path).unwrap().permissions();
    let mut readonly = writable.clone();
    readonly.set_readonly(true);
    std::fs::set_permissions(&path, readonly).unwrap();
    let started = std::time::Instant::now();
    let result = request_ui_activation(profile.path());
    std::fs::set_permissions(&path, writable).unwrap();
    assert_eq!(
        result.unwrap_err().code(),
        LifecycleErrorCode::LocalAccessDenied
    );
    assert!(started.elapsed() < std::time::Duration::from_secs(3));
    assert_eq!(
        read_ui_activation_request(profile.path()).unwrap(),
        Some(first_request)
    );
}

fn assert_simultaneous_activation_requests(acquire_owner: bool) {
    let profile = tempfile::tempdir().unwrap();
    let _winner = acquire_owner.then(|| UiInstanceGuard::acquire(profile.path()).unwrap().unwrap());
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(9));
    let workers: Vec<_> = (0..8)
        .map(|_| {
            let path = profile.path().to_path_buf();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                request_ui_activation(&path)
            })
        })
        .collect();
    barrier.wait();
    // Join every worker before propagating failures so the profile stays alive.
    let results: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    let issued: Vec<_> = results.into_iter().map(Result::unwrap).collect();
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

#[test]
fn device_profiles_activation_is_available_without_a_daemon_and_acknowledged() {
    let profile = tempfile::tempdir().unwrap();
    let id = hifimule_lifecycle::request_ui_device_profiles_activation(profile.path()).unwrap();
    let request = hifimule_lifecycle::read_ui_activation_detail(profile.path())
        .unwrap()
        .unwrap();
    assert!(request.open_device_profiles);
    assert!(!request.open_audio_output);
    assert!(!request.open_playback_settings);
    acknowledge_ui_activation(profile.path(), &id).unwrap();
    assert_eq!(
        hifimule_lifecycle::read_ui_activation_ack(profile.path()).unwrap(),
        Some(id)
    );
    // Older activation records omit the new metadata and remain readable.
    std::fs::write(
        profile.path().join("runtime/ui-activation.json"),
        format!(
            r#"{{"schemaVersion":1,"requestId":"{}"}}"#,
            request.request_id
        ),
    )
    .unwrap();
    assert!(
        !hifimule_lifecycle::read_ui_activation_detail(profile.path())
            .unwrap()
            .unwrap()
            .open_device_profiles
    );
}
