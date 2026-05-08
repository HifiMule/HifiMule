param(
  [Parameter(Mandatory = $true)]
  [string] $Tag,

  [Parameter(Mandatory = $true)]
  [string] $Pattern,

  [Parameter(Mandatory = $true)]
  [string] $Directory
)

$ErrorActionPreference = "Stop"

if (-not $env:GITHUB_REPOSITORY) {
  throw "GITHUB_REPOSITORY is not set"
}

if (-not $env:GH_TOKEN) {
  throw "GH_TOKEN is not set"
}

New-Item -ItemType Directory -Force -Path $Directory | Out-Null

$releasesJson = gh api "repos/$env:GITHUB_REPOSITORY/releases?per_page=100"
$releases = $releasesJson | ConvertFrom-Json
$release = $releases | Where-Object { $_.tag_name -eq $Tag } | Select-Object -First 1

if (-not $release) {
  throw "Release with tag '$Tag' was not found, including draft releases"
}

$asset = $release.assets |
  Where-Object { [System.Management.Automation.WildcardPattern]::new($Pattern).IsMatch($_.name) } |
  Select-Object -First 1

if (-not $asset) {
  $assetNames = ($release.assets | ForEach-Object { $_.name }) -join ", "
  throw "No release asset matched '$Pattern' for '$Tag'. Available assets: $assetNames"
}

$outputPath = Join-Path $Directory $asset.name
Write-Host "Downloading $($asset.name) from release '$Tag' to $outputPath"
Invoke-WebRequest `
  -Headers @{
    Accept = "application/octet-stream"
    Authorization = "Bearer $env:GH_TOKEN"
    "X-GitHub-Api-Version" = "2022-11-28"
  } `
  -Uri $asset.url `
  -OutFile $outputPath
