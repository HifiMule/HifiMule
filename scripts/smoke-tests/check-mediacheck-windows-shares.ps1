# Execute the native decoder through real SMB UNC and mapped-drive paths.
# Requires an elevated Windows runner; failures are fatal rather than skipped.
[CmdletBinding()]
param([Parameter(Mandatory = $true)][string]$MediaCheck)

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
$Executable = (Resolve-Path -LiteralPath $MediaCheck).Path
$Scratch = Join-Path ([IO.Path]::GetTempPath()) ('mediacheck-smb-' + [Guid]::NewGuid().ToString('N'))
$ShareName = 'Mediacheck' + [Guid]::NewGuid().ToString('N')
$ShareRoot = Join-Path $Scratch 'share'
$Library = Join-Path $ShareRoot 'Library shared'
$Nested = Join-Path $Library 'Björk été'
$Wave = Join-Path $Nested 'healthy source.wav'
$Drive = $null
$ShareCreated = $false
$MappingCreated = $false

function Assert-Audit([string]$Folder, [string]$Report) {
    # No Set-Location: all source and report paths are passed independently.
    $Output = (& $Executable $Folder --log $Report 2>&1 | Out-String)
    $Exit = $LASTEXITCODE
    Write-Host $Output
    if ($Exit -ne 0) { throw "mediacheck failed for $Folder with exit $Exit" }
    foreach ($Expected in @('Checked files: 1', 'Ignored files: 1', 'Recoverable files: 0', 'Bad files: 0', 'Operational errors: 0')) {
        if ($Output -notmatch ('(?m)^' + [Regex]::Escape($Expected) + '\r?$')) {
            throw "Missing summary '$Expected' for $Folder"
        }
    }
    $Log = Get-Content -LiteralPath $Report -Raw -Encoding utf8
    $OkLines = @($Log -split '\r?\n' | Where-Object { $_ -match '^OK\t' })
    if ($OkLines.Count -ne 1 -or $OkLines[0] -notmatch 'healthy source\.wav') {
        throw "Expected one healthy WAV result in $Report"
    }
    if ($Log -match '(?m)^(BAD|RECOVERABLE|ERROR)\t') { throw "Unexpected failure in $Report" }
}

try {
    New-Item -ItemType Directory -Path $Nested -Force | Out-Null
    # A one-second mono PCM WAV; native decoding must read sample data over SMB.
    $Stream = [IO.File]::Create($Wave)
    $Writer = [IO.BinaryWriter]::new($Stream)
    try {
        $Writer.Write([Text.Encoding]::ASCII.GetBytes('RIFF'))
        $Writer.Write([uint32](36 + 16000))
        $Writer.Write([Text.Encoding]::ASCII.GetBytes('WAVEfmt '))
        $Writer.Write([uint32]16)
        $Writer.Write([uint16]1)
        $Writer.Write([uint16]1)
        $Writer.Write([uint32]8000)
        $Writer.Write([uint32]16000)
        $Writer.Write([uint16]2)
        $Writer.Write([uint16]16)
        $Writer.Write([Text.Encoding]::ASCII.GetBytes('data'))
        $Writer.Write([uint32]16000)
        for ($Sample = 0; $Sample -lt 8000; $Sample++) {
            $Writer.Write([int16](12000 * [Math]::Sin(2 * [Math]::PI * 440 * $Sample / 8000)))
        }
    } finally { $Writer.Dispose() }
    [IO.File]::WriteAllText((Join-Path $Library 'ignored notes.txt'), 'not audio')
    $Before = (Get-FileHash -LiteralPath $Wave -Algorithm SHA256).Hash

    Start-Service LanmanServer
    $Account = [Security.Principal.WindowsIdentity]::GetCurrent().Name
    New-SmbShare -Name $ShareName -Path $ShareRoot -FullAccess $Account | Out-Null
    $ShareCreated = $true
    $Unc = "\\localhost\$ShareName"
    Assert-Audit "$Unc\Library shared" (Join-Path $Scratch 'unc audit.log')

    $UsedDrives = [IO.Directory]::GetLogicalDrives()
    $Drive = @([char[]](90..68) | ForEach-Object { "${_}:" } | Where-Object { "$($_)\" -notin $UsedDrives })[0]
    if (-not $Drive) { throw 'No unused Windows drive letter for the SMB audit' }
    # net use creates an OS mapping visible to the native child process.
    & net.exe use $Drive $Unc /persistent:no
    if ($LASTEXITCODE -ne 0) { throw 'Could not map the temporary SMB share' }
    $MappingCreated = $true
    Assert-Audit "$Drive\Library shared" (Join-Path $Scratch 'mapped audit.log')
    if ((Get-FileHash -LiteralPath $Wave -Algorithm SHA256).Hash -ne $Before) { throw 'Audit modified the source WAV' }
    Write-Host 'mediacheck native SMB UNC and mapped-drive audits passed.'
} finally {
    try {
        if ($MappingCreated) {
            & net.exe use $Drive /delete /y
            if ($LASTEXITCODE -ne 0) { throw "Could not remove SMB mapping $Drive" }
        }
    } finally {
        try {
            if ($ShareCreated) { Remove-SmbShare -Name $ShareName -Force }
        } finally {
            if (Test-Path -LiteralPath $Scratch) { Remove-Item -LiteralPath $Scratch -Recurse -Force }
        }
    }
}
