# smoke-windows.ps1 — Windows MSI smoke test for JellyfinSync
# Runs from the directory containing the .msi installer artifact.
#
# Steps:
#   1. Silent MSI install
#   2. Launch installed application
#   3. Poll daemon health endpoint (30s timeout)
#   4. Silent MSI uninstall
#
# Exit code 0 = PASS, non-zero = FAIL with diagnostic output.

$ErrorActionPreference = "Stop"

function Write-Step([string]$msg) {
    Write-Host ""
    Write-Host "==> $msg"
}

function Fail([string]$platform, [string]$step, [string]$message) {
    Write-Host "FAIL [platform=$platform] [step=$step]: $message"
    exit 1
}

$Platform = "windows"

function Get-InstallDir {
    $registryPaths = @(
        "HKLM:\SOFTWARE\JellyfinSync",
        "HKLM:\SOFTWARE\WOW6432Node\JellyfinSync",
        "HKCU:\SOFTWARE\JellyfinSync"
    )
    foreach ($path in $registryPaths) {
        $value = Get-ItemProperty -Path $path -Name "InstallDir" -ErrorAction SilentlyContinue
        if ($value -and $value.InstallDir -and (Test-Path $value.InstallDir)) {
            return $value.InstallDir
        }
    }

    $fallbacks = @(
        "C:\Program Files\JellyfinSync",
        "C:\Program Files (x86)\JellyfinSync"
    )
    foreach ($path in $fallbacks) {
        if (Test-Path $path) {
            return $path
        }
    }
    return $null
}

# --- STEP 1: Install ---
Write-Step "STEP 1: Installing MSI ..."
$msi = Get-Item "*.msi" -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $msi) {
    Fail $Platform "install" "No .msi file found in working directory: $(Get-Location)"
}
Write-Host "  Installer: $($msi.Name)"

$proc = Start-Process msiexec.exe `
    -ArgumentList "/i `"$($msi.FullName)`" /qn /norestart" `
    -Wait -PassThru -NoNewWindow
if ($proc.ExitCode -ne 0) {
    Fail $Platform "install" "msiexec returned exit code $($proc.ExitCode)"
}
Write-Host "  Install OK"

# --- STEP 2: Launch ---
Write-Step "STEP 2: Launching JellyfinSync ..."
$installDir = Get-InstallDir
if (-not $installDir) {
    Fail $Platform "launch" "Install directory not found in registry or common install locations"
}
$exe = Get-ChildItem $installDir -Filter "jellyfinsync.exe" -Recurse -ErrorAction SilentlyContinue |
       Select-Object -First 1
if (-not $exe) {
    # Fallback: find any .exe in install dir
    $exe = Get-ChildItem $installDir -Filter "*.exe" -Recurse -ErrorAction SilentlyContinue |
           Where-Object { $_.Name -notlike "unins*" } |
           Select-Object -First 1
}
if (-not $exe) {
    Fail $Platform "launch" "No executable found under $installDir"
}
Write-Host "  Executable: $($exe.FullName)"
$appProc = Start-Process $exe.FullName -WindowStyle Hidden -PassThru

# --- STEP 3: Daemon health poll ---
Write-Step "STEP 3: Polling daemon health (30s timeout) ..."
$body = '{"jsonrpc":"2.0","method":"daemon.health","params":{},"id":1}'
$ok = $false
for ($i = 0; $i -lt 30; $i++) {
    try {
        $r = Invoke-RestMethod `
            -Uri "http://127.0.0.1:19140" `
            -Method Post `
            -Body $body `
            -ContentType "application/json" `
            -TimeoutSec 2 `
            -ErrorAction SilentlyContinue
        if ($r.result.data.status -eq "ok") {
            $ok = $true
            break
        }
    } catch {
        # Daemon not ready yet — keep polling
    }
    Start-Sleep 1
}
if (-not $ok) {
    Write-Host "DIAGNOSTIC: Attempting verbose request to http://127.0.0.1:19140 ..."
    try {
        Invoke-WebRequest -Uri "http://127.0.0.1:19140" -Method Post -Body $body `
            -ContentType "application/json" -TimeoutSec 5 | Select-Object StatusCode, Content
    } catch {
        Write-Host "  Error: $_"
    }
    Fail $Platform "daemon-health" "Daemon did not respond with status=ok after 30s"
}
Write-Host "  Daemon responded OK"

# --- STEP 4: Uninstall ---
Write-Step "STEP 4: Uninstalling ..."
if ($appProc -and -not $appProc.HasExited) {
    Stop-Process -Id $appProc.Id -Force -ErrorAction SilentlyContinue
    Start-Sleep 2
}
$proc = Start-Process msiexec.exe `
    -ArgumentList "/x `"$($msi.FullName)`" /qn /norestart" `
    -Wait -PassThru -NoNewWindow
if ($proc.ExitCode -ne 0) {
    Fail $Platform "uninstall" "msiexec /x returned exit code $($proc.ExitCode)"
}
Write-Host "  Uninstall OK"

Write-Host ""
Write-Host "PASS: Windows smoke test complete"
