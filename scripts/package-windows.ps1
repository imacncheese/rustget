param(
    [string]$OutputDirectory = "dist",
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

if (-not $SkipBuild) {
    & "$PSScriptRoot\env.ps1"
    cargo build --release --workspace
}

$version = (Select-String -Path "Cargo.toml" -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches.Groups[1].Value
$name = "rustget-$version-windows-x64"
$stage = Join-Path $OutputDirectory $name
if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Force -Path $stage | Out-Null

$binaries = @(
    "target\release\rustget.exe",
    "target\release\rustget-native-host.exe",
    "target\release\rustget-tray.exe"
)
foreach ($binary in $binaries) {
    if (-not (Test-Path $binary)) { throw "missing release binary: $binary" }
    Copy-Item $binary $stage
}
Copy-Item README.md, CHANGELOG.md, LICENSE-MIT, LICENSE-APACHE -Destination $stage -ErrorAction SilentlyContinue
Copy-Item extensions -Destination $stage -Recurse
Copy-Item docs -Destination $stage -Recurse
Copy-Item "scripts\install-windows.ps1" -Destination $stage

$zip = Join-Path $OutputDirectory "$name.zip"
if (Test-Path $zip) { Remove-Item -Force $zip }
Compress-Archive -Path (Join-Path $stage "*") -DestinationPath $zip -CompressionLevel Optimal
$hash = (Get-FileHash -Algorithm SHA256 $zip).Hash.ToLowerInvariant()
Write-Output "package: $zip"
Write-Output "sha256:  $hash"
Write-Output "bytes:   $((Get-Item $zip).Length)"
