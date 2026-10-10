<#
.SYNOPSIS
Measure preparation and retained transfer memory in fresh daemon test processes.
.DESCRIPTION
Use test executables containing sync::tests::benchmark_large_sync_retention.
The baseline fixture must match the current fixture: N existing manifest tracks,
N pending additions, representative metadata, a local mock provider, and a
blocked first device write. This measures the real executor, with no real device.
Use -CommittedBeforeHold to pause after successful transfers and manifest commits.
Build through scripts/build-daemon.mjs so the controlled FFmpeg runtime is verified.
Executable SHA-256 hashes and every sample are retained with the measurements.
Pass both executables for a comparison, or one for an individual measurement.
These debug test-process results are a controlled comparison, not a prediction
of total memory for a particular user's server, metadata, or release build.
.EXAMPLE
rtk proxy powershell -NoProfile -ExecutionPolicy Bypass -File scripts/measure-sync-memory.ps1 `
  -BaselineExecutable target/sync-memory-baseline-build/debug/deps/hifimule_daemon-992c6a39a14163eb.exe `
  -CurrentExecutable target/debug/deps/hifimule_daemon-992c6a39a14163eb.exe
.EXAMPLE
rtk proxy powershell -NoProfile -ExecutionPolicy Bypass -File scripts/measure-sync-memory.ps1 `
  -CurrentExecutable target/debug/deps/hifimule_daemon-992c6a39a14163eb.exe `
  -Counts 40000 -HoldSeconds 120 -CommittedBeforeHold 5
#>
[CmdletBinding()]
param(
    [string]$BaselineExecutable,
    [string]$CurrentExecutable,
    [ValidateRange(1, 1000000)][int[]]$Counts = @(5000, 40000, 80000),
    [ValidateRange(2, 3600)][int]$HoldSeconds = 20,
    [ValidateRange(0, 100)][int]$CommittedBeforeHold = 0,
    [string]$OutputDirectory,
    [string]$RuntimeDirectory,
    [ValidatePattern('^[a-zA-Z0-9_:]+$')]
    [string]$TestName = 'sync::tests::benchmark_large_sync_retention'
)

$ErrorActionPreference = 'Stop'
# Some launchers supply both PATH and Path. .NET Framework refuses to build
# the child environment from those duplicate keys. Normalize this process only.
$inheritedSearchPath = $env:PATH
[Environment]::SetEnvironmentVariable('PATH', $null, 'Process')
[Environment]::SetEnvironmentVariable('Path', $null, 'Process')
[Environment]::SetEnvironmentVariable('Path', $inheritedSearchPath, 'Process')
$workspaceDirectory = Split-Path -Parent $PSScriptRoot
if (-not $BaselineExecutable -and -not $CurrentExecutable) {
    throw 'Pass -BaselineExecutable and/or -CurrentExecutable (daemon test executable paths).'
}
if (-not $OutputDirectory) {
    $OutputDirectory = Join-Path $workspaceDirectory 'target/sync-memory-measurements'
}
$null = New-Item -ItemType Directory -Path $OutputDirectory -Force
$OutputDirectory = (Resolve-Path -LiteralPath $OutputDirectory).Path
$manifestPath = Join-Path $workspaceDirectory 'hifimule-daemon/audio-runtime.json'
$runtimeManifest = Get-Content -Raw -LiteralPath $manifestPath | ConvertFrom-Json
if (-not $RuntimeDirectory) {
    if ($env:FFMPEG_DIR) {
        $RuntimeDirectory = $env:FFMPEG_DIR
    } else {
        $release = $runtimeManifest.ffmpegRelease
        $architecture = if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') { 'aarch64' } else { 'x86_64' }
        $RuntimeDirectory = Join-Path $workspaceDirectory "target/audio-runtime/ffmpeg-$release-official-$architecture-pc-windows-msvc"
    }
}
$runtimeBin = Join-Path (Resolve-Path -LiteralPath $RuntimeDirectory).Path 'bin'
foreach ($library in $runtimeManifest.requiredLibraries) {
    $major = $runtimeManifest.abiVersions.$library.Split('.')[0]
    if (-not (Test-Path -LiteralPath (Join-Path $runtimeBin "$library-$major.dll"))) {
        throw "The controlled FFmpeg runtime is unavailable under $runtimeBin. Build through scripts/build-daemon.mjs first."
    }
}

function Measure-Executor {
    param([string]$Executable, [string]$Label, [int]$Count)
    $executablePath = (Resolve-Path -LiteralPath $Executable).Path
    $timestamp = Get-Date -Format 'yyyyMMdd-HHmmss-fff'
    $prefix = Join-Path $OutputDirectory "$Label-$Count-$timestamp"
    $writer = [System.IO.StreamWriter]::new("$prefix.log", $false, [System.Text.UTF8Encoding]::new($false))
    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo.FileName = $executablePath
    $process.StartInfo.Arguments = "$TestName --exact --ignored --nocapture --test-threads=1"
    $process.StartInfo.WorkingDirectory = $workspaceDirectory
    $process.StartInfo.UseShellExecute = $false
    $process.StartInfo.CreateNoWindow = $true
    $process.StartInfo.RedirectStandardOutput = $true
    $process.StartInfo.RedirectStandardError = $true
    $process.StartInfo.Environment['HIFIMULE_SYNC_BENCH_COUNT'] = [string]$Count
    $process.StartInfo.Environment['HIFIMULE_SYNC_BENCH_HOLD_SECS'] = [string]$HoldSeconds
    $process.StartInfo.Environment['HIFIMULE_SYNC_BENCH_COMMITTED'] = [string]$CommittedBeforeHold
    $process.StartInfo.Environment['PATH'] = "$runtimeBin;$env:PATH"
    $samples = [System.Collections.Generic.List[object]]::new()
    $timer = [System.Diagnostics.Stopwatch]::StartNew()
    $phase = 'startup'
    $exitCode = $null
    try {
        $null = $process.Start()
        $lineTask = $process.StandardOutput.ReadLineAsync()
        $stderrTask = $process.StandardError.ReadToEndAsync()
        do {
            while ($lineTask.IsCompleted) {
                $line = $lineTask.GetAwaiter().GetResult()
                if ($null -eq $line) { break }
                $writer.WriteLine($line)
                if ($line -match 'SYNC_MEMORY_PHASE=(\w+)') {
                    $phase = $Matches[1]
                    Write-Host "$Label $Count tracks: $phase"
                }
                $lineTask = $process.StandardOutput.ReadLineAsync()
            }
            $writer.Flush()
            try {
                $process.Refresh()
                if (-not $process.HasExited) {
                    $samples.Add([pscustomobject][ordered]@{
                        seconds = [Math]::Round($timer.Elapsed.TotalSeconds, 3)
                        phase = $phase
                        workingSet = $process.WorkingSet64
                        privateBytes = $process.PrivateMemorySize64
                    })
                }
            } catch [System.InvalidOperationException] {
                if (-not $process.HasExited) { throw }
            }
            if ($process.HasExited) { break }
            if ($timer.Elapsed.TotalSeconds -gt ($HoldSeconds + 120)) {
                throw "Benchmark exceeded the transfer hold plus 120 seconds; see $prefix.log"
            }
            Start-Sleep -Milliseconds 100
        } while ($true)
        $process.WaitForExit()
        while ($true) {
            $line = $lineTask.GetAwaiter().GetResult()
            if ($null -eq $line) { break }
            $writer.WriteLine($line)
            $lineTask = $process.StandardOutput.ReadLineAsync()
        }
        $writer.Write($stderrTask.GetAwaiter().GetResult())
        $writer.Flush()
        $exitCode = $process.ExitCode
    } finally {
        try { if ($process.Id -and -not $process.HasExited) { $process.Kill(); $process.WaitForExit() } } catch {}
        $process.Dispose()
        $writer.Dispose()
        $timer.Stop()
    }

    $summary = [ordered]@{}
    foreach ($name in ($samples | ForEach-Object { $_.phase } | Select-Object -Unique)) {
        $values = @($samples | Where-Object { $_.phase -eq $name })
        $summary[$name] = [ordered]@{
            samples = $values.Count
            peakWorkingSet = ($values | Measure-Object -Property workingSet -Maximum).Maximum
            peakPrivateBytes = ($values | Measure-Object -Property privateBytes -Maximum).Maximum
            lastWorkingSet = $values[-1].workingSet
            lastPrivateBytes = $values[-1].privateBytes
        }
    }
    $hashAlgorithm = [System.Security.Cryptography.SHA256]::Create()
    $binaryStream = [System.IO.File]::OpenRead($executablePath)
    try {
        $binaryHash = [BitConverter]::ToString($hashAlgorithm.ComputeHash($binaryStream)).Replace('-', '')
    } finally {
        $binaryStream.Dispose()
        $hashAlgorithm.Dispose()
    }
    $result = [ordered]@{
        label = $Label
        tracks = $Count
        holdSeconds = $HoldSeconds
        committedBeforeHold = $CommittedBeforeHold
        executable = $executablePath
        executableSha256 = $binaryHash
        exitCode = $exitCode
        summary = $summary
        samples = @($samples.ToArray())
    }
    $result | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath "$prefix.json" -Encoding UTF8
    if ($exitCode -ne 0) { throw "$Label benchmark failed ($exitCode); see $prefix.log" }
    foreach ($required in @('idle', 'preparation', 'transfer')) {
        if (-not $summary.Contains($required)) {
            throw "Benchmark did not reach $required; refusing incomplete measurement. See $prefix.log"
        }
    }
    Write-Host ('{0} {1}: transfer private {2:N1} MiB; working set {3:N1} MiB' -f $Label, $Count,
        ($summary.transfer.peakPrivateBytes / 1MB), ($summary.transfer.peakWorkingSet / 1MB))
    return $result
}

$comparisons = @()
foreach ($count in $Counts) {
    if ($CommittedBeforeHold -ge $count) {
        throw '-CommittedBeforeHold must be smaller than every track count.'
    }
    $before = if ($BaselineExecutable) { Measure-Executor $BaselineExecutable 'baseline' $count }
    $after = if ($CurrentExecutable) { Measure-Executor $CurrentExecutable 'current' $count }
    if ($before -and $after) {
        $beforeBytes = $before.summary.transfer.peakPrivateBytes
        $afterBytes = $after.summary.transfer.peakPrivateBytes
        $comparison = [ordered]@{
            tracks = $count
            holdSeconds = $HoldSeconds
            committedBeforeHold = $CommittedBeforeHold
            baselinePrivateMiB = [Math]::Round($beforeBytes / 1MB, 2)
            currentPrivateMiB = [Math]::Round($afterBytes / 1MB, 2)
            reductionPercent = [Math]::Round(100 * (1 - $afterBytes / $beforeBytes), 2)
            baselineWorkingSetMiB = [Math]::Round($before.summary.transfer.peakWorkingSet / 1MB, 2)
            currentWorkingSetMiB = [Math]::Round($after.summary.transfer.peakWorkingSet / 1MB, 2)
            baselineSha256 = $before.executableSha256
            currentSha256 = $after.executableSha256
        }
        $comparisons += $comparison
        Write-Host ('{0} tracks: private memory reduced {1:N1}%' -f $count, $comparison.reductionPercent)
    }
}
if ($comparisons.Count) {
    $comparisons | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $OutputDirectory 'comparison.json') -Encoding UTF8
}
Write-Host "Full samples and logs: $OutputDirectory"
