use std::io::Write;
use std::path::Path;
use std::process::{Command, Output};

fn mediacheck(cwd: &Path, args: &[&std::ffi::OsStr]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mediacheck"))
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap()
}

fn wav(path: &Path, seconds: u32) {
    let samples = 48_000 * seconds;
    let mut file = std::fs::File::create(path).unwrap();
    file.write_all(b"RIFF").unwrap();
    file.write_all(&(36 + samples * 4).to_le_bytes()).unwrap();
    file.write_all(b"WAVEfmt ").unwrap();
    file.write_all(&16u32.to_le_bytes()).unwrap();
    file.write_all(&1u16.to_le_bytes()).unwrap();
    file.write_all(&2u16.to_le_bytes()).unwrap();
    file.write_all(&48_000u32.to_le_bytes()).unwrap();
    file.write_all(&192_000u32.to_le_bytes()).unwrap();
    file.write_all(&4u16.to_le_bytes()).unwrap();
    file.write_all(&16u16.to_le_bytes()).unwrap();
    file.write_all(b"data").unwrap();
    file.write_all(&(samples * 4).to_le_bytes()).unwrap();
    for index in 0..samples {
        let sample =
            ((index as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 12_000.0) as i16;
        file.write_all(&sample.to_le_bytes()).unwrap();
        file.write_all(&sample.to_le_bytes()).unwrap();
    }
}

fn mp3(wav: &Path, output: &Path) -> Vec<u8> {
    let ffmpeg = std::env::var_os("HIFIMULE_TEST_FFMPEG")
        .expect("Run through npm run build:daemon to use the verified controlled runtime");
    let result = Command::new(ffmpeg)
        .args(["-v", "error", "-y", "-i"])
        .arg(wav)
        .args(["-c:a", "libmp3lame", "-b:a", "192k"])
        .arg(output)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    std::fs::read(output).unwrap()
}

fn damage_mp3(clean: &Path, damaged_packets: &[usize]) -> Vec<u8> {
    use ffmpeg_next::{format, media};
    let mut bytes = std::fs::read(clean).unwrap();
    let mut input = format::input(clean).unwrap();
    let index = input.streams().best(media::Type::Audio).unwrap().index();
    for (number, (_, packet)) in input
        .packets()
        .filter(|(stream, _)| stream.index() == index)
        .enumerate()
    {
        if damaged_packets.contains(&number) {
            let at = packet.position() as usize;
            bytes[at + 4..at + 36].fill(0xff);
        }
    }
    bytes
}

#[test]
fn ac3_extensions_are_checked_case_insensitively_and_corruption_is_bad() {
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().join("library");
    std::fs::create_dir(&folder).unwrap();
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/generated-ac3-5point1.ac3");
    let original = std::fs::read(fixture).unwrap();
    for name in ["lower.ac3", "upper.AC3"] {
        std::fs::write(folder.join(name), &original).unwrap();
    }
    let healthy_log = temp.path().join("healthy.log");
    let result = mediacheck(
        temp.path(),
        &[
            folder.as_os_str(),
            "--log".as_ref(),
            healthy_log.as_os_str(),
        ],
    );
    assert_eq!(
        result.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&result.stdout)
    );
    let report = std::fs::read_to_string(healthy_log).unwrap();
    assert!(report.contains("Checked files: 2"), "{report}");
    assert!(report.contains("Ignored files: 0"), "{report}");
    assert!(report.contains("Bad files: 0"), "{report}");
    assert_eq!(
        report
            .lines()
            .filter(|line| line.starts_with("OK\t"))
            .count(),
        2
    );
    let invalid = b"invalid AC3 audio";
    std::fs::write(folder.join("broken.ac3"), invalid).unwrap();
    let bad_log = temp.path().join("bad.log");
    let result = mediacheck(
        temp.path(),
        &[folder.as_os_str(), "--log".as_ref(), bad_log.as_os_str()],
    );
    assert_eq!(result.status.code(), Some(1));
    let report = std::fs::read_to_string(bad_log).unwrap();
    assert!(report.contains("Checked files: 3"), "{report}");
    assert!(report.contains("Ignored files: 0"), "{report}");
    assert!(report.contains("Bad files: 1"), "{report}");
    assert_eq!(
        report
            .lines()
            .filter(|line| line.starts_with("BAD\t"))
            .count(),
        1
    );
    for name in ["lower.ac3", "upper.AC3"] {
        assert_eq!(std::fs::read(folder.join(name)).unwrap(), original);
    }
    assert_eq!(std::fs::read(folder.join("broken.ac3")).unwrap(), invalid);
}

#[test]
fn ac3_concealment_is_recoverable_or_bad_according_to_shared_limits() {
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().join("library");
    std::fs::create_dir(&folder).unwrap();
    let fixture = include_bytes!("fixtures/generated-ac3-5point1.ac3");
    let mut originals = Vec::new();
    for (name, repeats, damaged) in [
        ("recoverable.ac3", 3, vec![60]),
        ("sustained.AC3", 3, vec![60, 61, 62, 63]),
        ("ratio.ac3", 3, vec![20, 60, 100]),
        ("concealed-middle.ac3", 1, (15..45).collect()),
    ] {
        let mut bytes = fixture.repeat(repeats);
        for frame in damaged {
            bytes[frame * 2048 + 100..frame * 2048 + 1900].fill(0xff);
        }
        originals.push((name, bytes));
    }
    let mut crc_damage = fixture.to_vec();
    for frame in crc_damage.chunks_exact_mut(2048) {
        frame[2] ^= 1;
    }
    originals.push(("crc.AC3", crc_damage));
    let mut truncated = fixture.to_vec();
    truncated.truncate(truncated.len() - 500);
    originals.push(("truncated.ac3", truncated));
    for (name, bytes) in &originals {
        std::fs::write(folder.join(name), bytes).unwrap();
    }
    let log = temp.path().join("audit.log");
    let result = mediacheck(
        temp.path(),
        &[folder.as_os_str(), "--log".as_ref(), log.as_os_str()],
    );
    assert_eq!(result.status.code(), Some(1));
    let report = std::fs::read_to_string(log).unwrap();
    for expected in [
        "Checked files: 6",
        "Ignored files: 0",
        "Recoverable files: 1",
        "Bad files: 5",
        "Operational errors: 0",
    ] {
        assert!(report.contains(expected), "{report}");
    }
    assert_eq!(
        report
            .lines()
            .filter(|line| line.starts_with("BAD\t"))
            .count(),
        5
    );
    assert_eq!(
        report
            .lines()
            .filter(|line| line.starts_with("RECOVERABLE\t"))
            .count(),
        1
    );
    assert!(!report.lines().any(|line| line.starts_with("OK\t")));
    assert!(
        report.contains("Sustained invalid audio decoding"),
        "{report}"
    );
    assert!(report.contains("exceeds one percent"), "{report}");
    for (name, bytes) in originals {
        assert_eq!(std::fs::read(folder.join(name)).unwrap(), bytes);
    }
}

#[test]
fn mixed_library_reports_all_classifications_without_modifying_sources() {
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().join("Müsic with spaces");
    let nested = folder.join("album");
    std::fs::create_dir_all(&nested).unwrap();
    let fixture = temp.path().join("fixture.wav");
    wav(&fixture, 4);
    let clean = temp.path().join("fixture.mp3");
    mp3(&fixture, &clean);
    let good = folder.join("good.wav");
    let second = nested.join("second.WAV");
    std::fs::copy(&fixture, &good).unwrap();
    std::fs::copy(&fixture, &second).unwrap();
    let recoverable = folder.join("recoverable.mp3");
    let sustained = nested.join("sustained.mp3");
    std::fs::write(&recoverable, damage_mp3(&clean, &[60])).unwrap();
    std::fs::write(&sustained, damage_mp3(&clean, &[60, 61, 62, 63])).unwrap();
    let broken = nested.join("broken.flac");
    std::fs::write(&broken, b"invalid original audio").unwrap();
    std::fs::write(folder.join("cover.jpg"), b"ignored").unwrap();
    for image in ["cover.avif", "cover.heic", "cover.heif"] {
        std::fs::write(folder.join(image), b"ignored image").unwrap();
    }
    let originals: Vec<_> = [&good, &second, &recoverable, &sustained, &broken]
        .into_iter()
        .map(|path| (path.clone(), std::fs::read(path).unwrap()))
        .collect();
    // A report with a supported media suffix must still exclude itself.
    let log = folder.join("audit.wav");
    let result = mediacheck(
        temp.path(),
        &[folder.as_os_str(), "--log".as_ref(), log.as_os_str()],
    );
    let stdout = String::from_utf8_lossy(&result.stdout);
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert_eq!(result.status.code(), Some(1), "{stdout}\n{stderr}");
    assert!(
        stderr.is_empty(),
        "native logging obscures progress: {stderr}"
    );
    for expected in [
        "Checking [5]",
        "Checked files: 5",
        "Ignored files: 4",
        "Recoverable files: 1",
        "Bad files: 2",
        "Operational errors: 0",
    ] {
        assert!(stdout.contains(expected), "missing {expected}: {stdout}");
    }
    let report = std::fs::read_to_string(&log).unwrap();
    assert_eq!(
        report
            .lines()
            .filter(|line| line.starts_with("OK\t"))
            .count(),
        2
    );
    assert_eq!(
        report
            .lines()
            .filter(|line| line.starts_with("BAD\t"))
            .count(),
        2
    );
    assert_eq!(
        report
            .lines()
            .filter(|line| line.starts_with("RECOVERABLE\t"))
            .count(),
        1
    );
    assert!(
        report.contains("Sustained invalid audio decoding (maximum 3 consecutive events)"),
        "{report}"
    );
    assert!(
        report.contains("Cannot open original audio")
            || report.contains("Original contains no audio stream"),
        "{report}"
    );
    assert!(report.contains("recovery: 1 invalid events"));
    assert!(report.contains("AVFMT_DURATION_FROM_STREAM"));
    let only_recoverable = temp.path().join("only recoverable");
    std::fs::create_dir(&only_recoverable).unwrap();
    std::fs::copy(&recoverable, only_recoverable.join("recoverable.mp3")).unwrap();
    let recovered_log = temp.path().join("recovered.log");
    let result = mediacheck(
        temp.path(),
        &[
            only_recoverable.as_os_str(),
            "--log".as_ref(),
            recovered_log.as_os_str(),
        ],
    );
    assert_eq!(result.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&result.stdout).contains("Recoverable files: 1"));
    for (path, bytes) in originals {
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
}

#[test]
fn healthy_library_default_log_is_unique_and_existing_paths_are_never_overwritten() {
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().join("library");
    std::fs::create_dir(&folder).unwrap();
    let good = folder.join("good.wav");
    wav(&good, 1);
    for _ in 0..2 {
        let result = mediacheck(temp.path(), &[folder.as_os_str()]);
        assert_eq!(
            result.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    let logs: Vec<_> = std::fs::read_dir(temp.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension() == Some("log".as_ref()))
        .collect();
    assert_eq!(logs.len(), 2);
    let inside = mediacheck(&folder, &[".".as_ref()]);
    assert_eq!(inside.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&inside.stdout).contains("Ignored files: 0"));
    let before = std::fs::read(&good).unwrap();
    let result = mediacheck(
        temp.path(),
        &[folder.as_os_str(), "--log".as_ref(), good.as_os_str()],
    );
    assert_eq!(result.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&result.stderr).contains("never overwritten"));
    assert_eq!(std::fs::read(&good).unwrap(), before);
}

#[test]
fn help_version_and_usage_failures_have_expected_exit_codes() {
    let temp = tempfile::tempdir().unwrap();
    for arg in ["--help", "--version"] {
        let result = mediacheck(temp.path(), &[arg.as_ref()]);
        assert!(result.status.success());
        assert!(
            String::from_utf8_lossy(&result.stdout).contains(if arg == "--help" {
                "Usage: mediacheck"
            } else {
                env!("CARGO_PKG_VERSION")
            })
        );
    }
    for args in [
        vec![],
        vec!["absent".as_ref()],
        vec!["--log".as_ref()],
        vec!["--unknown".as_ref()],
    ] {
        assert_eq!(mediacheck(temp.path(), &args).status.code(), Some(2));
    }
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn symlinks_are_ignored_and_control_characters_do_not_forge_log_records() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().join("library");
    std::fs::create_dir(&folder).unwrap();
    let good = folder.join("music\nBAD\tforged.wav");
    wav(&good, 1);
    symlink(&folder, folder.join("loop")).unwrap();
    symlink(&good, folder.join("linked.wav")).unwrap();
    std::fs::write(folder.join("._track.wav"), b"Finder metadata").unwrap();
    let log = temp.path().join("audit.log");
    let result = mediacheck(
        temp.path(),
        &[folder.as_os_str(), "--log".as_ref(), log.as_os_str()],
    );
    assert_eq!(result.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(stdout.contains("Checked files: 1"), "{stdout}");
    assert!(stdout.contains("Ignored files: 3"), "{stdout}");
    let report = std::fs::read_to_string(log).unwrap();
    assert_eq!(
        report
            .lines()
            .filter(|line| line.starts_with("OK\t"))
            .count(),
        1
    );
    assert!(!report.lines().any(|line| line.starts_with("BAD\t")));
    assert!(report.contains(r"music\nBAD\tforged.wav"));
}

#[cfg(unix)]
#[test]
fn unreadable_subtree_marks_the_audit_incomplete() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().join("library");
    let inaccessible = folder.join("inaccessible");
    std::fs::create_dir_all(&inaccessible).unwrap();
    wav(&folder.join("good.wav"), 1);
    std::fs::set_permissions(&inaccessible, std::fs::Permissions::from_mode(0)).unwrap();
    let log = temp.path().join("audit.log");
    let result = mediacheck(
        temp.path(),
        &[folder.as_os_str(), "--log".as_ref(), log.as_os_str()],
    );
    std::fs::set_permissions(&inaccessible, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(
        result.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&result.stdout)
    );
    let report = std::fs::read_to_string(log).unwrap();
    assert!(report.contains("ERROR\t"), "{report}");
    assert!(report.contains("Checked files: 1"), "{report}");
    assert!(report.contains("Operational errors: 1"), "{report}");
}

#[cfg(windows)]
#[test]
fn windows_directory_junction_cycles_are_ignored() {
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().join("shared Müsic");
    std::fs::create_dir(&folder).unwrap();
    wav(&folder.join("good.wav"), 1);
    let junction = folder.join("junction loop");
    let created = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", "$ErrorActionPreference = 'Stop'; New-Item -ItemType Junction -Path $env:MEDIACHECK_TEST_JUNCTION -Target $env:MEDIACHECK_TEST_TARGET | Out-Null"])
        .env("MEDIACHECK_TEST_JUNCTION", &junction)
        .env("MEDIACHECK_TEST_TARGET", &folder)
        .output()
        .unwrap();
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );
    let log = temp.path().join("audit.log");
    let result = mediacheck(
        temp.path(),
        &[folder.as_os_str(), "--log".as_ref(), log.as_os_str()],
    );
    std::fs::remove_dir(&junction).unwrap();
    assert_eq!(result.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(stdout.contains("Checked files: 1"), "{stdout}");
    assert!(stdout.contains("Ignored files: 1"), "{stdout}");
}

#[cfg(target_os = "linux")]
#[test]
fn non_utf8_filename_does_not_abort_the_remaining_audit() {
    use std::os::unix::ffi::OsStrExt;
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().join("library");
    std::fs::create_dir(&folder).unwrap();
    wav(
        &folder.join(std::ffi::OsStr::from_bytes(b"invalid-\xff.wav")),
        1,
    );
    wav(&folder.join("healthy.wav"), 1);
    let log = temp.path().join("audit.log");
    let result = mediacheck(
        temp.path(),
        &[folder.as_os_str(), "--log".as_ref(), log.as_os_str()],
    );
    assert_eq!(result.status.code(), Some(1));
    let report = std::fs::read_to_string(log).unwrap();
    assert!(report.contains("path is not valid UTF-8"), "{report}");
    assert!(report.contains("Checked files: 2"), "{report}");
    assert!(report.contains("Bad files: 1"), "{report}");
    assert_eq!(
        report
            .lines()
            .filter(|line| line.starts_with("OK\t"))
            .count(),
        1
    );
}
