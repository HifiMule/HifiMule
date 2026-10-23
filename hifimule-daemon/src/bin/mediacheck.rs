//! Read-only, target-independent audits using the same source decoder as sync.
use anyhow::{Context, Result, bail, ensure};
use hifimule_daemon::source_audio::{SourceDecoder, SourceReport, supported_extensions};
use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::fs::{File, OpenOptions, ReadDir};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "Usage: mediacheck FOLDER [--log PATH]\n\
Recursively audit audio sources with HifiMule's controlled audio runtime.\n\
No source files are modified and no transcoded output is created.\n\
Directory symlinks are skipped. Windows UNC shares and mapped drives are supported.\n\
Logs use create-new semantics; existing files are never overwritten.\n\
Exit codes: 0 = no bad media, 1 = bad media found, 2 = incomplete audit or usage error.\n\
Options: --log PATH  Report location (default: unique mediacheck-*.log in current directory)\n\
         --help, -h  Show this help\n\
         --version   Show the version";

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Help,
    Version,
    Audit {
        folder: PathBuf,
        log: Option<PathBuf>,
    },
}

fn parse_args(args: impl IntoIterator<Item = OsString>) -> Result<Command> {
    let mut args = args.into_iter();
    let mut folder = None;
    let mut log = None;
    let mut positional = false;
    while let Some(arg) = args.next() {
        if !positional {
            if arg == "--help" || arg == "-h" {
                return Ok(Command::Help);
            }
            if arg == "--version" {
                return Ok(Command::Version);
            }
            if arg == "--" {
                positional = true;
                continue;
            }
            if arg == "--log" {
                ensure!(log.is_none(), "--log may only be supplied once");
                log = Some(PathBuf::from(args.next().context("--log requires a path")?));
                continue;
            }
            ensure!(
                !arg.to_string_lossy().starts_with('-'),
                "Unknown option {}",
                escaped(&arg)
            );
        }
        ensure!(folder.is_none(), "Only one folder may be supplied");
        folder = Some(PathBuf::from(arg));
    }
    Ok(Command::Audit {
        folder: folder.context("A folder is required")?,
        log,
    })
}

fn main() -> ExitCode {
    let result = (|| -> Result<u8> {
        match parse_args(std::env::args_os().skip(1))? {
            Command::Help => {
                writeln!(std::io::stdout().lock(), "{USAGE}")?;
                Ok(0)
            }
            Command::Version => {
                writeln!(
                    std::io::stdout().lock(),
                    "mediacheck {}",
                    env!("CARGO_PKG_VERSION")
                )?;
                Ok(0)
            }
            Command::Audit { folder, log } => audit(&folder, log.as_deref()),
        }
    })();
    match result {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("mediacheck: {error:#}\nUse mediacheck --help for usage.");
            ExitCode::from(2)
        }
    }
}

/// Escape control characters so a media filename cannot forge report lines.
fn escaped(value: impl AsRef<OsStr>) -> String {
    format!("{:?}", value.as_ref())
}

#[derive(Default)]
struct Counts {
    checked: u64,
    ignored: u64,
    recoverable: u64,
    bad: u64,
    operational_errors: u64,
}

impl Counts {
    fn summary(&self) -> String {
        format!(
            "Checked files: {}\nIgnored files: {}\nRecoverable files: {}\nBad files: {}\nOperational errors: {}",
            self.checked, self.ignored, self.recoverable, self.bad, self.operational_errors
        )
    }
}

fn create_log(explicit: Option<&Path>) -> Result<(PathBuf, File)> {
    let create = |path: &Path| OpenOptions::new().write(true).create_new(true).open(path);
    if let Some(path) = explicit {
        let file = create(path).with_context(|| {
            format!(
                "Cannot create report {} (existing files are never overwritten)",
                escaped(path)
            )
        })?;
        return Ok((path.to_path_buf(), file));
    }
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S-%3f");
    let cwd =
        std::env::current_dir().context("Cannot determine current directory for the report")?;
    for suffix in 0..1000 {
        let path = cwd.join(format!(
            "mediacheck-{stamp}-{}-{suffix}.log",
            std::process::id()
        ));
        match create(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("Cannot create report {}", escaped(&path)));
            }
        }
    }
    bail!("Cannot select an unused report name; use --log PATH")
}

struct Audit<'a> {
    log: BufWriter<File>,
    log_path: PathBuf,
    log_identity: PathBuf,
    extensions: BTreeSet<String>,
    counts: Counts,
    console: &'a mut dyn Write,
}

impl Audit<'_> {
    fn operational_error(&mut self, path: &Path, error: anyhow::Error) -> Result<()> {
        self.counts.operational_errors += 1;
        writeln!(
            self.log,
            "ERROR\t{}\n  {}",
            escaped(path),
            escaped(format!("{error:#}"))
        )?;
        self.log.flush().context("Cannot flush audit report")?;
        writeln!(self.console, "Cannot inspect {}: {error:#}", escaped(path))?;
        Ok(())
    }

    fn is_active_log(&self, path: &Path) -> bool {
        path == self.log_path
            || (path.file_name() == self.log_path.file_name()
                && path.canonicalize().ok().as_ref() == Some(&self.log_identity))
    }

    fn inspect_file(&mut self, path: &Path) -> Result<()> {
        if self.is_active_log(path) {
            return Ok(());
        }
        let extension = path
            .extension()
            .and_then(OsStr::to_str)
            .unwrap_or("")
            .to_ascii_lowercase();
        // AppleDouble sidecars look like audio but contain Finder metadata.
        let sidecar = path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with("._"));
        if sidecar || !self.extensions.contains(&extension) {
            self.counts.ignored += 1;
            return Ok(());
        }
        self.counts.checked += 1;
        writeln!(
            self.console,
            "Checking [{}]: {}",
            self.counts.checked,
            escaped(path)
        )?;
        self.console.flush()?;
        let decoded =
            SourceDecoder::open(path).and_then(|source| source.decode(|| Ok(()), |_| Ok(())));
        self.record_result(path, decoded)
    }

    fn record_result(&mut self, path: &Path, decoded: Result<SourceReport>) -> Result<()> {
        match decoded {
            Ok(report) => match report.validate_source() {
                Err(error) => {
                    self.counts.bad += 1;
                    writeln!(
                        self.log,
                        "BAD\t{}\n  {}",
                        escaped(path),
                        escaped(format!("{error:#}"))
                    )?;
                    self.write_report(&report)?;
                }
                Ok(()) if report.recovery.invalid_events > 0 => {
                    self.counts.recoverable += 1;
                    writeln!(
                        self.log,
                        "RECOVERABLE\t{}\n  Audible defects may remain.",
                        escaped(path)
                    )?;
                    self.write_report(&report)?;
                }
                Ok(()) => writeln!(self.log, "OK\t{}", escaped(path))?,
            },
            Err(error) => {
                self.counts.bad += 1;
                writeln!(
                    self.log,
                    "BAD\t{}\n  {}",
                    escaped(path),
                    escaped(format!("{error:#}"))
                )?;
            }
        }
        self.log.flush().context("Cannot flush audit report")?;
        Ok(())
    }

    fn write_report(&mut self, report: &SourceReport) -> Result<()> {
        writeln!(
            self.log,
            "  probed source: {:.6}s; decoded: {:.6}s; estimation: {:?}\n  recovery: {} invalid events; {} charged bytes; first position: {:?}",
            report.probed_duration_micros as f64 / 1_000_000.0,
            report.decoded_duration_micros as f64 / 1_000_000.0,
            report.duration_estimation_method,
            report.recovery.invalid_events,
            report.recovery.rejected_bytes,
            report.recovery.first_position,
        )?;
        Ok(())
    }

    fn open_directory(&mut self, path: &Path) -> Result<Option<ReadDir>> {
        match std::fs::read_dir(path) {
            Ok(entries) => Ok(Some(entries)),
            Err(error) => {
                self.operational_error(path, error.into())?;
                Ok(None)
            }
        }
    }

    fn walk(&mut self, folder: &Path) -> Result<()> {
        // One iterator per depth, rather than collecting the entire library.
        // Filesystem symlinks are never traversed, including links to directories.
        let mut stack = Vec::new();
        if let Some(entries) = self.open_directory(folder)? {
            stack.push((folder.to_path_buf(), entries));
        }
        while let Some((directory, entries)) = stack.last_mut() {
            let Some(entry) = entries.next() else {
                stack.pop();
                continue;
            };
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    self.operational_error(directory, error.into())?;
                    continue;
                }
            };
            let path = entry.path();
            let kind = match entry.file_type() {
                Ok(kind) => kind,
                Err(error) => {
                    self.operational_error(&path, error.into())?;
                    continue;
                }
            };
            // On Windows, FileType::is_symlink includes name-surrogate reparse
            // tags such as junctions. Cloud placeholders are not redirects and
            // must remain traversable rather than skipping their entire album.
            if kind.is_symlink() {
                self.counts.ignored += 1;
            } else if kind.is_dir() {
                if let Some(entries) = self.open_directory(&path)? {
                    stack.push((path, entries));
                }
            } else if kind.is_file() {
                // Source opening failures are bad files, not fatal walk errors.
                let result = self.inspect_file(&path);
                if let Err(error) = result {
                    return Err(error).context("Audit could not be completed");
                }
            } else {
                self.counts.ignored += 1;
            }
        }
        Ok(())
    }
}

fn audit(folder: &Path, log_path: Option<&Path>) -> Result<u8> {
    ensure!(
        !folder.as_os_str().is_empty(),
        "Folder path cannot be empty"
    );
    ensure!(
        std::fs::metadata(folder)
            .with_context(|| format!("Cannot inspect folder {}", escaped(folder)))?
            .is_dir(),
        "Input is not a folder: {}",
        escaped(folder)
    );
    let extensions = supported_extensions()?;
    ffmpeg_next::log::set_level(ffmpeg_next::log::Level::Quiet);
    let folder = if folder.is_absolute() {
        folder.to_path_buf()
    } else {
        std::env::current_dir()
            .context("Cannot determine current directory")?
            .join(folder)
    };
    let (log_path, file) = create_log(log_path)?;
    // Canonical identity is used only to exclude the report. Source paths stay
    // native and unchanged for UNC shares, mapped drives and platform decoders.
    let log_identity = log_path
        .canonicalize()
        .context("Cannot identify the audit report")?;
    let mut console = std::io::stdout().lock();
    writeln!(
        console,
        "Auditing: {}\nReport: {}",
        escaped(&folder),
        escaped(&log_path)
    )?;
    let mut audit = Audit {
        log: BufWriter::new(file),
        log_path,
        log_identity,
        extensions,
        counts: Counts::default(),
        console: &mut console,
    };
    writeln!(
        audit.log,
        "mediacheck {}\nFolder: {}\nRuntime: HifiMule controlled audio runtime\nSupported extensions: {}\n",
        env!("CARGO_PKG_VERSION"),
        escaped(&folder),
        audit
            .extensions
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join(", ")
    )?;
    audit.walk(&folder)?;
    let summary = audit.counts.summary();
    writeln!(audit.log, "\n{summary}")?;
    audit.log.flush().context("Cannot finish audit report")?;
    writeln!(
        audit.console,
        "\n{summary}\nReport: {}",
        escaped(&audit.log_path)
    )?;
    audit.console.flush()?;
    Ok(if audit.counts.operational_errors > 0 {
        2
    } else if audit.counts.bad > 0 {
        1
    } else {
        0
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_keep_native_unicode_and_windows_share_paths() {
        for folder in [
            r"\\server\Music share\Björk",
            r"Z:\Music share\Björk",
            "music with spaces/été",
        ] {
            assert_eq!(
                parse_args([folder.into(), "--log".into(), "audit file.log".into()]).unwrap(),
                Command::Audit {
                    folder: folder.into(),
                    log: Some("audit file.log".into())
                }
            );
        }
        assert_eq!(
            parse_args(["--".into(), "-music".into()]).unwrap(),
            Command::Audit {
                folder: "-music".into(),
                log: None
            }
        );
        for args in [
            vec![],
            vec!["--log"],
            vec!["one", "two"],
            vec!["--unknown"],
            vec!["one", "--log", "a", "--log", "b"],
        ] {
            assert!(parse_args(args.into_iter().map(OsString::from)).is_err());
        }
    }
}
