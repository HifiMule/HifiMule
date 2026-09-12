//! Shared native daemon lifecycle contract.
//!
//! Secrets and discovery remain on the native side. The operating-system file lock,
//! rather than descriptor metadata or a PID probe, is the ownership authority.

use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;
use uuid::Uuid;

pub const SCHEMA_VERSION: u32 = 1;
pub const PROTOCOL_VERSION: u32 = 1;
pub const DESCRIPTOR_MAX_BYTES: usize = 16 * 1024;
pub const STARTUP_DEADLINE: Duration = Duration::from_secs(15);
pub const HEALTH_TIMEOUT: Duration = Duration::from_secs(2);
pub const POLL_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LifecycleErrorCode {
    StartupTimeout,
    SpawnFailed,
    UnsafeRuntimePath,
    LocalAccessDenied,
    ProtocolMismatch,
    LegacyDaemonRunning,
    LegacyEndpointOccupied,
    OwnerChanged,
    DaemonStopped,
    QuitBlockedActiveSync,
    QuitPersistenceFailed,
    ShutdownTimeout,
}

impl LifecycleErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StartupTimeout => "STARTUP_TIMEOUT",
            Self::SpawnFailed => "SPAWN_FAILED",
            Self::UnsafeRuntimePath => "UNSAFE_RUNTIME_PATH",
            Self::LocalAccessDenied => "LOCAL_ACCESS_DENIED",
            Self::ProtocolMismatch => "PROTOCOL_MISMATCH",
            Self::LegacyDaemonRunning => "LEGACY_DAEMON_RUNNING",
            Self::LegacyEndpointOccupied => "LEGACY_ENDPOINT_OCCUPIED",
            Self::OwnerChanged => "OWNER_CHANGED",
            Self::DaemonStopped => "DAEMON_STOPPED",
            Self::QuitBlockedActiveSync => "QUIT_BLOCKED_ACTIVE_SYNC",
            Self::QuitPersistenceFailed => "QUIT_PERSISTENCE_FAILED",
            Self::ShutdownTimeout => "SHUTDOWN_TIMEOUT",
        }
    }
}

#[derive(Debug, Error)]
#[error("{code:?}: {message}")]
pub struct LifecycleError {
    code: LifecycleErrorCode,
    message: String,
}

impl LifecycleError {
    pub fn new(code: LifecycleErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn code(&self) -> LifecycleErrorCode {
        self.code
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnerDescriptor {
    pub schema_version: u32,
    pub protocol_version: u32,
    pub instance_id: String,
    pub pid: u32,
    pub port: u16,
    pub token: String,
    pub launch_generation: String,
}

impl std::fmt::Debug for OwnerDescriptor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OwnerDescriptor")
            .field("schema_version", &self.schema_version)
            .field("protocol_version", &self.protocol_version)
            .field("instance_id", &self.instance_id)
            .field("pid", &self.pid)
            .field("port", &self.port)
            .field("token", &"[REDACTED]")
            .field("launch_generation", &self.launch_generation)
            .finish()
    }
}

impl OwnerDescriptor {
    pub fn validate_compatible(&self) -> Result<(), LifecycleError> {
        if self.schema_version != SCHEMA_VERSION || self.protocol_version != PROTOCOL_VERSION {
            return Err(LifecycleError::new(
                LifecycleErrorCode::ProtocolMismatch,
                "The running HifiMule daemon uses an incompatible lifecycle protocol",
            ));
        }
        let token_valid =
            self.token.len() == 64 && self.token.bytes().all(|byte| byte.is_ascii_hexdigit());
        if self.instance_id.is_empty()
            || self.pid == 0
            || self.port == 0
            || !token_valid
            || self.launch_generation.parse::<u64>().is_err()
        {
            return Err(LifecycleError::new(
                LifecycleErrorCode::LocalAccessDenied,
                "Daemon discovery metadata is invalid",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleStatus {
    pub state: LifecycleState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<LifecycleErrorCode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LifecycleState {
    Starting,
    Ready,
    Failed,
    Stopping,
    Stopped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LaunchTicket {
    attempt_id: String,
    expected_generation: String,
    expires_at_ms: u128,
}

pub struct LaunchTicketGuard {
    app_data: PathBuf,
    lock_file: File,
}

impl LaunchTicketGuard {
    pub fn validate(
        &self,
        attempt_id: &str,
        expected_generation: u64,
    ) -> Result<(), LifecycleError> {
        validate_launch_ticket_unlocked(&self.app_data, attempt_id, expected_generation)
    }
}

impl Drop for LaunchTicketGuard {
    fn drop(&mut self) {
        let _ = self.lock_file.unlock();
    }
}

pub fn lock_launch_ticket(app_data: &Path) -> Result<LaunchTicketGuard, LifecycleError> {
    let runtime = prepare_runtime_dir(app_data)?;
    let lock_file = private_open(&runtime.join("launch-ticket.lock"), true)?;
    lock_file.lock().map_err(|error| {
        LifecycleError::new(
            LifecycleErrorCode::LocalAccessDenied,
            format!("Cannot lock launch coordination: {error}"),
        )
    })?;
    Ok(LaunchTicketGuard {
        app_data: app_data.to_path_buf(),
        lock_file,
    })
}

pub fn create_launch_ticket(
    app_data: &Path,
    expected_generation: u64,
) -> Result<String, LifecycleError> {
    let runtime = prepare_runtime_dir(app_data)?;
    let _guard = lock_launch_ticket(app_data)?;
    prune_expired_tickets(&runtime);
    let attempt_id = Uuid::new_v4().to_string();
    let ticket = LaunchTicket {
        attempt_id: attempt_id.clone(),
        expected_generation: expected_generation.to_string(),
        expires_at_ms: monotonic_ticket_expiry(),
    };
    atomic_write_json(&runtime.join(format!("attempt-{attempt_id}.json")), &ticket)?;
    Ok(attempt_id)
}

fn prune_expired_tickets(runtime: &Path) {
    let now = monotonic_millis();
    let Ok(entries) = std::fs::read_dir(runtime) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("attempt-") || !name.ends_with(".json") {
            continue;
        }
        let path = entry.path();
        let expired = (|| {
            let mut bytes = Vec::new();
            private_read(&path)
                .ok()?
                .take((DESCRIPTOR_MAX_BYTES + 1) as u64)
                .read_to_end(&mut bytes)
                .ok()?;
            if bytes.len() > DESCRIPTOR_MAX_BYTES {
                return None;
            }
            serde_json::from_slice::<LaunchTicket>(&bytes).ok()
        })()
        .is_none_or(|ticket| ticket.expires_at_ms <= now);
        if expired {
            let _ = std::fs::remove_file(path);
        }
    }
}

pub fn validate_launch_ticket(
    app_data: &Path,
    attempt_id: &str,
    expected_generation: u64,
) -> Result<(), LifecycleError> {
    let guard = lock_launch_ticket(app_data)?;
    guard.validate(attempt_id, expected_generation)
}

fn validate_launch_ticket_unlocked(
    app_data: &Path,
    attempt_id: &str,
    expected_generation: u64,
) -> Result<(), LifecycleError> {
    if Uuid::parse_str(attempt_id).is_err() {
        return Err(LifecycleError::new(
            LifecycleErrorCode::DaemonStopped,
            "Invalid launch attempt",
        ));
    }
    let runtime = validate_runtime_dir(app_data)?;
    let path = runtime.join(format!("attempt-{attempt_id}.json"));
    ensure_regular_private_file(&path)?;
    let mut bytes = Vec::new();
    private_read(&path)?
        .take((DESCRIPTOR_MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(access_error)?;
    if bytes.len() > DESCRIPTOR_MAX_BYTES {
        return Err(LifecycleError::new(
            LifecycleErrorCode::DaemonStopped,
            "Launch attempt is invalid",
        ));
    }
    let ticket: LaunchTicket = serde_json::from_slice(&bytes).map_err(|_| {
        LifecycleError::new(
            LifecycleErrorCode::DaemonStopped,
            "Launch attempt is malformed",
        )
    })?;
    let now = monotonic_millis();
    if ticket.attempt_id != attempt_id
        || ticket.expected_generation != expected_generation.to_string()
        || ticket.expires_at_ms <= now
    {
        return Err(LifecycleError::new(
            LifecycleErrorCode::DaemonStopped,
            "Launch attempt expired or was cancelled",
        ));
    }
    Ok(())
}

pub fn cancel_launch_ticket(app_data: &Path, attempt_id: &str) -> Result<(), LifecycleError> {
    if Uuid::parse_str(attempt_id).is_err() {
        return Ok(());
    }
    let _guard = lock_launch_ticket(app_data)?;
    let path = validate_runtime_dir(app_data)?.join(format!("attempt-{attempt_id}.json"));
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(access_error(error)),
    }
}

#[derive(Debug)]
pub struct OwnerGuard {
    app_data: PathBuf,
    runtime: PathBuf,
    lock_file: File,
    instance_id: Option<String>,
}

impl OwnerGuard {
    pub fn acquire(app_data: &Path) -> Result<Self, LifecycleError> {
        let runtime = prepare_runtime_dir(app_data)?;
        let lock_path = runtime.join("owner.lock");
        let lock_file = private_open(&lock_path, true)?;
        lock_file.try_lock().map_err(|error| {
            let code = match error {
                std::fs::TryLockError::WouldBlock => LifecycleErrorCode::OwnerChanged,
                std::fs::TryLockError::Error(_) => LifecycleErrorCode::LocalAccessDenied,
            };
            LifecycleError::new(code, format!("Cannot acquire daemon ownership: {error}"))
        })?;
        Ok(Self {
            app_data: app_data.to_path_buf(),
            runtime,
            lock_file,
            instance_id: None,
        })
    }

    pub fn runtime_dir(&self) -> &Path {
        &self.runtime
    }

    pub fn launch_generation(&self) -> Result<u64, LifecycleError> {
        read_generation(&self.app_data)
    }

    pub fn publish_ready(&mut self, port: u16) -> Result<OwnerDescriptor, LifecycleError> {
        let descriptor = self.prepare_descriptor(port)?;
        self.publish_descriptor(&descriptor)?;
        Ok(descriptor)
    }

    pub fn prepare_descriptor(&self, port: u16) -> Result<OwnerDescriptor, LifecycleError> {
        if port == 0 {
            return Err(LifecycleError::new(
                LifecycleErrorCode::SpawnFailed,
                "The daemon listener did not receive a usable port",
            ));
        }
        let mut token_bytes = [0_u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut token_bytes);
        let token = token_bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let instance_id = Uuid::new_v4().to_string();
        let descriptor = OwnerDescriptor {
            schema_version: SCHEMA_VERSION,
            protocol_version: PROTOCOL_VERSION,
            instance_id: instance_id.clone(),
            pid: std::process::id(),
            port,
            token,
            launch_generation: self.launch_generation()?.to_string(),
        };
        descriptor.validate_compatible()?;
        Ok(descriptor)
    }

    pub fn publish_descriptor(
        &mut self,
        descriptor: &OwnerDescriptor,
    ) -> Result<(), LifecycleError> {
        descriptor.validate_compatible()?;
        atomic_write_json(&self.runtime.join("owner.json"), descriptor)?;
        self.instance_id = Some(descriptor.instance_id.clone());
        Ok(())
    }

    pub fn verify_expected_generation(&self, expected: u64) -> Result<(), LifecycleError> {
        if self.launch_generation()? != expected {
            return Err(LifecycleError::new(
                LifecycleErrorCode::DaemonStopped,
                "This launch attempt was fenced by an explicit Quit",
            ));
        }
        Ok(())
    }

    pub fn advance_generation(&self) -> Result<u64, LifecycleError> {
        let current = self.launch_generation().map_err(|error| {
            LifecycleError::new(
                LifecycleErrorCode::QuitPersistenceFailed,
                format!("Cannot read launch generation: {error}"),
            )
        })?;
        let next = current.checked_add(1).ok_or_else(|| {
            LifecycleError::new(
                LifecycleErrorCode::QuitPersistenceFailed,
                "Launch generation is exhausted",
            )
        })?;
        atomic_write_bytes(
            &self.runtime.join("launch-generation.json"),
            next.to_string().as_bytes(),
        )
        .map_err(|error| {
            LifecycleError::new(
                LifecycleErrorCode::QuitPersistenceFailed,
                format!("Cannot persist launch generation: {error}"),
            )
        })?;
        Ok(next)
    }
}

impl Drop for OwnerGuard {
    fn drop(&mut self) {
        if let Some(instance_id) = self.instance_id.as_deref()
            && read_descriptor(&self.app_data)
                .is_ok_and(|descriptor| descriptor.instance_id == instance_id)
        {
            let _ = std::fs::remove_file(self.runtime.join("owner.json"));
        }
        let _ = self.lock_file.unlock();
    }
}

pub fn read_descriptor(app_data: &Path) -> Result<OwnerDescriptor, LifecycleError> {
    let runtime = validate_runtime_dir(app_data)?;
    let path = runtime.join("owner.json");
    ensure_regular_private_file(&path)?;
    let mut file = private_read(&path)?;
    let metadata = file.metadata().map_err(access_error)?;
    if metadata.len() as usize > DESCRIPTOR_MAX_BYTES {
        return Err(LifecycleError::new(
            LifecycleErrorCode::LocalAccessDenied,
            "Daemon discovery metadata is too large",
        ));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut bytes).map_err(access_error)?;
    let descriptor: OwnerDescriptor = serde_json::from_slice(&bytes).map_err(|_| {
        LifecycleError::new(
            LifecycleErrorCode::LocalAccessDenied,
            "Daemon discovery metadata is malformed",
        )
    })?;
    descriptor.validate_compatible()?;
    Ok(descriptor)
}

/// Resolve the canonical application-data profile without a current-directory fallback.
pub fn resolve_app_data_dir() -> Result<PathBuf, LifecycleError> {
    if let Ok(override_path) = std::env::var("HIFIMULE_APP_DATA_DIR") {
        let path = PathBuf::from(override_path);
        if path.is_absolute() {
            return Ok(path);
        }
        return Err(LifecycleError::new(
            LifecycleErrorCode::UnsafeRuntimePath,
            "HIFIMULE_APP_DATA_DIR must be absolute",
        ));
    }
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    let base = std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|path| path.join("Library/Application Support"));
    #[cfg(target_os = "linux")]
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|path| path.join(".local/share"))
        });
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    let base: Option<PathBuf> = None;
    let path = base
        .filter(|path| path.is_absolute())
        .ok_or_else(|| {
            LifecycleError::new(
                LifecycleErrorCode::UnsafeRuntimePath,
                "Cannot resolve an absolute application-data directory",
            )
        })?
        .join("HifiMule");
    Ok(path)
}

pub fn read_generation(app_data: &Path) -> Result<u64, LifecycleError> {
    let runtime = prepare_runtime_dir(app_data)?;
    let path = runtime.join("launch-generation.json");
    if !path.exists() {
        return Ok(0);
    }
    ensure_regular_private_file(&path)?;
    let mut value = String::new();
    private_read(&path)?
        .read_to_string(&mut value)
        .map_err(access_error)?;
    value.trim().parse().map_err(|_| {
        LifecycleError::new(
            LifecycleErrorCode::LocalAccessDenied,
            "Launch generation metadata is malformed",
        )
    })
}

pub fn constant_time_token_eq(expected: &[u8], actual: &[u8]) -> bool {
    let mut difference = expected.len() ^ actual.len();
    let maximum = expected.len().max(actual.len());
    for index in 0..maximum {
        let left = expected.get(index).copied().unwrap_or(0);
        let right = actual.get(index).copied().unwrap_or(0);
        difference |= usize::from(left ^ right);
    }
    difference == 0
}

fn prepare_runtime_dir(app_data: &Path) -> Result<PathBuf, LifecycleError> {
    if !app_data.is_absolute() {
        return Err(LifecycleError::new(
            LifecycleErrorCode::UnsafeRuntimePath,
            "Application data path must be absolute",
        ));
    }
    if app_data
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_symlink() || !metadata.is_dir())
    {
        return Err(LifecycleError::new(
            LifecycleErrorCode::UnsafeRuntimePath,
            "Application data path must be a real directory",
        ));
    }
    let runtime = app_data.join("runtime");
    if runtime
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        return Err(LifecycleError::new(
            LifecycleErrorCode::UnsafeRuntimePath,
            "Runtime directory must not be a symbolic link",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true).mode(0o700);
        builder.create(&runtime).map_err(access_error)?;
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&runtime, std::fs::Permissions::from_mode(0o700))
            .map_err(access_error)?;
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(&runtime).map_err(access_error)?;
        #[cfg(windows)]
        protect_windows_path(&runtime)?;
    }
    validate_runtime_dir(app_data)
}

fn validate_runtime_dir(app_data: &Path) -> Result<PathBuf, LifecycleError> {
    if !app_data.is_absolute() {
        return Err(LifecycleError::new(
            LifecycleErrorCode::UnsafeRuntimePath,
            "Application data path must be absolute",
        ));
    }
    let runtime = app_data.join("runtime");
    let metadata = runtime.symlink_metadata().map_err(access_error)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(LifecycleError::new(
            LifecycleErrorCode::UnsafeRuntimePath,
            "Runtime path is not a private local directory",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.uid() != unsafe { libc::geteuid() }
            || metadata.permissions().mode() & 0o077 != 0
        {
            return Err(LifecycleError::new(
                LifecycleErrorCode::LocalAccessDenied,
                "Runtime directory ownership or permissions are unsafe",
            ));
        }
    }
    #[cfg(windows)]
    validate_windows_path(&runtime)?;
    Ok(runtime)
}

fn private_open(path: &Path, create: bool) -> Result<File, LifecycleError> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(create);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path).map_err(access_error)?;
    #[cfg(windows)]
    protect_windows_path(path)?;
    ensure_regular_private_file(path)?;
    Ok(file)
}

fn private_read(path: &Path) -> Result<File, LifecycleError> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path).map_err(access_error)?;
    ensure_regular_private_file(path)?;
    Ok(file)
}

fn ensure_regular_private_file(path: &Path) -> Result<(), LifecycleError> {
    let metadata = path.symlink_metadata().map_err(access_error)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(LifecycleError::new(
            LifecycleErrorCode::UnsafeRuntimePath,
            "Lifecycle metadata must be a regular file",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.uid() != unsafe { libc::geteuid() }
            || metadata.permissions().mode() & 0o077 != 0
        {
            return Err(LifecycleError::new(
                LifecycleErrorCode::LocalAccessDenied,
                "Lifecycle metadata ownership or permissions are unsafe",
            ));
        }
    }
    #[cfg(windows)]
    validate_windows_path(path)?;
    Ok(())
}

fn atomic_write_json(path: &Path, value: &impl Serialize) -> Result<(), LifecycleError> {
    let bytes = serde_json::to_vec(value).map_err(|error| {
        LifecycleError::new(
            LifecycleErrorCode::LocalAccessDenied,
            format!("Cannot encode lifecycle metadata: {error}"),
        )
    })?;
    atomic_write_bytes(path, &bytes)
}

fn atomic_write_bytes(path: &Path, bytes: &[u8]) -> Result<(), LifecycleError> {
    let parent = path.parent().ok_or_else(|| {
        LifecycleError::new(
            LifecycleErrorCode::UnsafeRuntimePath,
            "Missing runtime directory",
        )
    })?;
    let temp = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name().unwrap_or_default().to_string_lossy(),
        Uuid::new_v4()
    ));
    let mut file = private_open(&temp, true)?;
    file.write_all(bytes).map_err(access_error)?;
    file.sync_all().map_err(access_error)?;
    atomic_replace(&temp, path)?;
    #[cfg(unix)]
    File::open(parent)
        .and_then(|dir| dir.sync_all())
        .map_err(access_error)?;
    Ok(())
}

#[cfg(unix)]
fn atomic_replace(temp: &Path, destination: &Path) -> Result<(), LifecycleError> {
    std::fs::rename(temp, destination).map_err(access_error)
}

#[cfg(windows)]
fn atomic_replace(temp: &Path, destination: &Path) -> Result<(), LifecycleError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    let source: Vec<u16> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
    let target: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            target.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(access_error(std::io::Error::last_os_error()))
    } else {
        Ok(())
    }
}

fn access_error(error: std::io::Error) -> LifecycleError {
    LifecycleError::new(
        LifecycleErrorCode::LocalAccessDenied,
        format!("Lifecycle metadata access failed: {error}"),
    )
}

#[cfg(windows)]
fn protect_windows_path(path: &Path) -> Result<(), LifecycleError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::{
        DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
        SetFileSecurityW,
    };
    // Protected DACL: owner, LocalSystem and administrators only. `OW` resolves
    // against the file owner and avoids embedding a user SID in discovery data.
    let sddl: Vec<u16> = "D:P(A;;FA;;;OW)(A;;FA;;;SY)(A;;FA;;;BA)"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    let converted = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            std::ptr::null_mut(),
        )
    };
    if converted == 0 {
        return Err(access_error(std::io::Error::last_os_error()));
    }
    let wide_path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let secured = unsafe {
        SetFileSecurityW(
            wide_path.as_ptr(),
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            descriptor,
        )
    };
    unsafe { LocalFree(descriptor) };
    if secured == 0 {
        Err(access_error(std::io::Error::last_os_error()))
    } else {
        Ok(())
    }
}

#[cfg(windows)]
fn validate_windows_path(path: &Path) -> Result<(), LifecycleError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Authorization::{
        ConvertSecurityDescriptorToStringSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::{
        DACL_SECURITY_INFORMATION, GetFileSecurityW, GetSecurityDescriptorControl,
        PSECURITY_DESCRIPTOR, SE_DACL_PROTECTED,
    };
    let wide_path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut required = 0_u32;
    unsafe {
        GetFileSecurityW(
            wide_path.as_ptr(),
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            0,
            &mut required,
        );
    }
    if required == 0 {
        return Err(access_error(std::io::Error::last_os_error()));
    }
    let words = (required as usize).div_ceil(std::mem::size_of::<usize>());
    let mut storage = vec![0_usize; words];
    let descriptor = storage.as_mut_ptr().cast::<core::ffi::c_void>();
    let loaded = unsafe {
        GetFileSecurityW(
            wide_path.as_ptr(),
            DACL_SECURITY_INFORMATION,
            descriptor,
            required,
            &mut required,
        )
    };
    if loaded == 0 {
        return Err(access_error(std::io::Error::last_os_error()));
    }
    let mut control = 0_u16;
    let mut revision = 0_u32;
    if unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) } == 0
        || control & SE_DACL_PROTECTED == 0
    {
        return Err(LifecycleError::new(
            LifecycleErrorCode::LocalAccessDenied,
            "Lifecycle path does not have a protected Windows DACL",
        ));
    }
    let mut sddl_ptr: windows_sys::core::PWSTR = std::ptr::null_mut();
    let mut sddl_len = 0_u32;
    let converted = unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor as PSECURITY_DESCRIPTOR,
            SDDL_REVISION_1,
            DACL_SECURITY_INFORMATION,
            &mut sddl_ptr,
            &mut sddl_len,
        )
    };
    if converted == 0 {
        return Err(access_error(std::io::Error::last_os_error()));
    }
    let sddl = String::from_utf16_lossy(unsafe {
        std::slice::from_raw_parts(sddl_ptr, sddl_len as usize)
    });
    unsafe { LocalFree(sddl_ptr.cast()) };
    if sddl.contains(";;;WD)") || sddl.contains(";;;BU)") {
        return Err(LifecycleError::new(
            LifecycleErrorCode::LocalAccessDenied,
            "Lifecycle path grants access to Everyone or built-in Users",
        ));
    }
    Ok(())
}

pub fn monotonic_ticket_expiry() -> u128 {
    monotonic_millis().saturating_add(STARTUP_DEADLINE.as_millis())
}

#[cfg(unix)]
fn monotonic_millis() -> u128 {
    let mut value = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut value) } != 0 {
        return 0;
    }
    (value.tv_sec as u128)
        .saturating_mul(1_000)
        .saturating_add((value.tv_nsec as u128) / 1_000_000)
}

#[cfg(windows)]
fn monotonic_millis() -> u128 {
    unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() as u128 }
}

#[cfg(not(any(unix, windows)))]
fn monotonic_millis() -> u128 {
    0
}
