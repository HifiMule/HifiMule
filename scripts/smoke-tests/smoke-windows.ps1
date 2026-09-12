# smoke-windows.ps1 — Windows MSI smoke test for HifiMule
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
        "HKLM:\SOFTWARE\HifiMule",
        "HKLM:\SOFTWARE\WOW6432Node\HifiMule",
        "HKCU:\SOFTWARE\HifiMule"
    )
    foreach ($path in $registryPaths) {
        $value = Get-ItemProperty -Path $path -Name "InstallDir" -ErrorAction SilentlyContinue
        if ($value -and $value.InstallDir -and (Test-Path $value.InstallDir)) {
            return $value.InstallDir
        }
    }

    $fallbacks = @(
        "C:\Program Files\HifiMule",
        "C:\Program Files (x86)\HifiMule"
    )
    foreach ($path in $fallbacks) {
        if (Test-Path $path) {
            return $path
        }
    }
    return $null
}

function Assert-UiReady([string]$marker) {
    $runtime = Join-Path $env:APPDATA "HifiMule\runtime"
    if ($env:HIFIMULE_APP_DATA_DIR) { $runtime = Join-Path $env:HIFIMULE_APP_DATA_DIR "runtime" }
    $readyPath = Join-Path $runtime "ui-ready-$marker.json"
    $deadline = [DateTime]::UtcNow.AddSeconds(30)
    while ([DateTime]::UtcNow -lt $deadline) {
        try {
            $ready = Get-Content $readyPath -Raw | ConvertFrom-Json
            $owner = Get-Content (Join-Path $runtime "owner.json") -Raw | ConvertFrom-Json
            if ($ready.smokeId -eq $marker -and $ready.state -eq "hydrated" -and
                $ready.daemonPid -eq $owner.pid -and $ready.instanceId -eq $owner.instanceId -and
                (Get-Process -Id $ready.uiPid -ErrorAction SilentlyContinue)) {
                Write-Host "UI_ATTACHMENT_EVIDENCE $($ready | ConvertTo-Json -Compress)"
                Remove-Item $readyPath
                return
            }
        } catch { }
        Start-Sleep -Milliseconds 250
    }
    Fail $Platform "ui-hydration" "Installed UI did not confirm current-state hydration"
}

# --- STEP 1: Install ---
Write-Step "STEP 1: Installing MSI ..."
$msi = Get-Item "*.msi" -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $msi) {
    Fail $Platform "install" "No .msi file found in working directory: $(Get-Location)"
}
Write-Host "  Installer: $($msi.Name)"
Write-Host "  SHA256: $((Get-FileHash $msi.FullName -Algorithm SHA256).Hash)"

$proc = Start-Process msiexec.exe `
    -ArgumentList "/i `"$($msi.FullName)`" /qn /norestart" `
    -Wait -PassThru -NoNewWindow
if ($proc.ExitCode -ne 0) {
    Fail $Platform "install" "msiexec returned exit code $($proc.ExitCode)"
}
Write-Host "  Install OK"

# --- STEP 2: Launch ---
Write-Step "STEP 2: Launching HifiMule ..."
$installDir = Get-InstallDir
if (-not $installDir) {
    Fail $Platform "launch" "Install directory not found in registry or common install locations"
}
$exe = Get-ChildItem $installDir -Filter "hifimule-ui.exe" -Recurse -ErrorAction SilentlyContinue |
       Select-Object -First 1
if (-not $exe) {
    # Legacy package-name fallback. Never mistake the daemon for the UI.
    $exe = Get-ChildItem $installDir -Filter "hifimule.exe" -Recurse -ErrorAction SilentlyContinue |
           Select-Object -First 1
}
if (-not $exe) {
    $exe = Get-ChildItem $installDir -Filter "*.exe" -Recurse -ErrorAction SilentlyContinue |
           Where-Object { $_.Name -notlike "unins*" -and $_.Name -ne "hifimule-daemon.exe" } |
           Select-Object -First 1
}
if (-not $exe) {
    Fail $Platform "launch" "No executable found under $installDir"
}
Write-Host "  Executable: $($exe.FullName)"
$smokeId = [guid]::NewGuid().ToString()
$appProc = Start-Process $exe.FullName -ArgumentList "--smoke-id $smokeId" -WindowStyle Hidden -PassThru

# --- STEP 3: Daemon health poll ---
Write-Step "STEP 3: Polling daemon health (30s timeout) ..."
$body = '{"jsonrpc":"2.0","method":"daemon.health","params":{},"id":1}'
$descriptorPath = Join-Path $env:APPDATA "HifiMule\runtime\owner.json"
$ok = $false
for ($i = 0; $i -lt 30; $i++) {
    try {
        $descriptor = Get-Content $descriptorPath -Raw | ConvertFrom-Json
        if ($descriptor.schemaVersion -ne 1 -or $descriptor.protocolVersion -ne 1) { throw "incompatible descriptor" }
        $headers = @{ Authorization = "Bearer $($descriptor.token)" }
        $r = Invoke-RestMethod `
            -Uri "http://127.0.0.1:$($descriptor.port)" `
            -Method Post `
            -Body $body `
            -ContentType "application/json" `
            -Headers $headers `
            -TimeoutSec 2 `
            -ErrorAction SilentlyContinue
        if ($r.result.data.status -eq "ok" -and $r.result.data.instanceId -eq $descriptor.instanceId) {
            $ok = $true
            break
        }
    } catch {
        # Daemon not ready yet — keep polling
    }
    Start-Sleep 1
}
if (-not $ok) {
    if (Test-Path $descriptorPath) {
        $safe = Get-Content $descriptorPath -Raw | ConvertFrom-Json | Select-Object schemaVersion, protocolVersion, instanceId, pid, port, launchGeneration
        Write-Host "DIAGNOSTIC: $($safe | ConvertTo-Json -Compress)"
    } else {
        Write-Host "DIAGNOSTIC: owner descriptor was not published"
    }
    Fail $Platform "daemon-health" "Daemon did not respond with status=ok after 30s"
}
Assert-UiReady $smokeId
Write-Host "  Daemon responded OK"
$safe = Get-Content $descriptorPath -Raw | ConvertFrom-Json | Select-Object schemaVersion, protocolVersion, instanceId, pid, port, launchGeneration
Write-Host "  LIFECYCLE_EVIDENCE os=Windows architecture=$env:PROCESSOR_ARCHITECTURE descriptor=$($safe | ConvertTo-Json -Compress)"

try {
    Invoke-WebRequest -Uri "http://127.0.0.1:$($descriptor.port)" -Method Post -Body $body `
        -ContentType "application/json" -TimeoutSec 2 -ErrorAction Stop | Out-Null
    Fail $Platform "local-access" "Unauthenticated health request was accepted"
} catch {
    if ($_.Exception.Response.StatusCode.value__ -ne 401) {
        Fail $Platform "local-access" "Unauthenticated health request did not return 401"
    }
}

Write-Step "STEP 3a: Concurrent launch and UI close/reopen ..."
$initialPid = [int]$descriptor.pid
$initialInstance = [string]$descriptor.instanceId
$secondSmokeId = [guid]::NewGuid().ToString()
$secondUi = Start-Process $exe.FullName -ArgumentList "--smoke-id $secondSmokeId" -WindowStyle Hidden -PassThru
Assert-UiReady $secondSmokeId
Start-Sleep 2
$afterConcurrent = Get-Content $descriptorPath -Raw | ConvertFrom-Json
if ($afterConcurrent.pid -ne $initialPid -or $afterConcurrent.instanceId -ne $initialInstance) {
    Fail $Platform "concurrent-launch" "Daemon identity changed"
}
if ($secondUi -and -not $secondUi.HasExited) { Stop-Process -Id $secondUi.Id -Force }
if ($appProc -and -not $appProc.HasExited) { Stop-Process -Id $appProc.Id -Force }
Start-Sleep 1
if (-not (Get-Process -Id $initialPid -ErrorAction SilentlyContinue)) {
    Fail $Platform "close-ui" "Closing the UI stopped the daemon"
}
$smokeId = [guid]::NewGuid().ToString()
$appProc = Start-Process $exe.FullName -ArgumentList "--smoke-id $smokeId" -WindowStyle Hidden -PassThru
Start-Sleep 2
Assert-UiReady $smokeId
$afterReopen = Get-Content $descriptorPath -Raw | ConvertFrom-Json
if ($afterReopen.pid -ne $initialPid -or $afterReopen.instanceId -ne $initialInstance) {
    Fail $Platform "reopen-ui" "Reopen created a competing daemon"
}
Write-Host "  Concurrent launch and close/reopen preserved PID and instance"

Write-Step "STEP 3b: Daemon crash recovery ..."
Stop-Process -Id $initialPid -Force
for ($i = 0; $i -lt 10; $i++) {
    if (-not (Get-Process -Id $initialPid -ErrorAction SilentlyContinue)) { break }
    Start-Sleep -Milliseconds 500
}
if (Get-Process -Id $initialPid -ErrorAction SilentlyContinue) {
    Fail $Platform "crash-recovery" "Unable to terminate the original daemon"
}
if ($appProc -and -not $appProc.HasExited) {
    Stop-Process -Id $appProc.Id -Force
}
$smokeId = [guid]::NewGuid().ToString()
$appProc = Start-Process $exe.FullName -ArgumentList "--smoke-id $smokeId" -WindowStyle Hidden -PassThru
$recovered = $null
for ($i = 0; $i -lt 30; $i++) {
    try {
        $candidate = Get-Content $descriptorPath -Raw | ConvertFrom-Json
        if ($candidate.instanceId -eq $initialInstance -or $candidate.pid -eq $initialPid) {
            throw "stale descriptor"
        }
        $headers = @{ Authorization = "Bearer $($candidate.token)" }
        $r = Invoke-RestMethod -Uri "http://127.0.0.1:$($candidate.port)" -Method Post `
            -Body $body -ContentType "application/json" -Headers $headers -TimeoutSec 2
        if ($r.result.data.status -eq "ok" -and $r.result.data.instanceId -eq $candidate.instanceId) {
            $recovered = $candidate
            break
        }
    } catch {
        # The new daemon has not replaced the stale descriptor yet.
    }
    Start-Sleep 1
}
if (-not $recovered) {
    Fail $Platform "crash-recovery" "UI did not recover a fresh authenticated daemon within 30s"
}
Assert-UiReady $smokeId
$safeRecovered = $recovered | Select-Object schemaVersion, protocolVersion, instanceId, pid, port, launchGeneration
Write-Host "  RECOVERY_EVIDENCE os=Windows architecture=$env:PROCESSOR_ARCHITECTURE descriptor=$($safeRecovered | ConvertTo-Json -Compress)"
$initialPid = [int]$recovered.pid

# --- STEP 4: Uninstall ---
Write-Step "STEP 4: Uninstalling ..."
if ($appProc -and -not $appProc.HasExited) {
    Stop-Process -Id $appProc.Id -Force -ErrorAction SilentlyContinue
    Start-Sleep 2
}
Stop-Process -Id $initialPid -Force -ErrorAction SilentlyContinue
$proc = Start-Process msiexec.exe `
    -ArgumentList "/x `"$($msi.FullName)`" /qn /norestart" `
    -Wait -PassThru -NoNewWindow
if ($proc.ExitCode -ne 0) {
    Fail $Platform "uninstall" "msiexec /x returned exit code $($proc.ExitCode)"
}
Write-Host "  Uninstall OK"

Write-Host ""
Write-Host "DEVICE_SHUTDOWN_EVIDENCE os=Windows architecture=$env:PROCESSOR_ARCHITECTURE status=UNVERIFIED transport=UNVERIFIED artifact=installed shutdownId=UNVERIFIED elapsedMs=UNVERIFIED outcome=UNVERIFIED integrity=UNVERIFIED ownershipCleanup=UNVERIFIED reason=no-real-device-or-human-tray-session"
Write-Host "PASS: Windows smoke test complete (real-device active Quit remains UNVERIFIED)"
