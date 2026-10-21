use std::process::Command;

const SERVICE_NAME: &str = "hifimule-daemon";

/// Remove a legacy Windows service. A running service is marked for deletion
/// until it stops; failure to stop must not prevent deletion.
pub fn uninstall() -> anyhow::Result<()> {
    let _ = Command::new("sc.exe").args(["stop", SERVICE_NAME]).output();
    let output = Command::new("sc.exe")
        .args(["delete", SERVICE_NAME])
        .output()?;
    anyhow::ensure!(
        output.status.success(),
        "Failed to delete legacy service: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}
