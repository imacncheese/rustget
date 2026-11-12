param(
    [string]$WixDirectory = "D:\dsh\.tools\wix\bin",
    [string]$OutputDirectory = "dist",
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$candle = Join-Path $WixDirectory "candle.exe"
$light = Join-Path $WixDirectory "light.exe"
if (-not (Test-Path $candle) -or -not (Test-Path $light)) {
    throw "WiX candle.exe/light.exe not found in $WixDirectory"
}

if (-not $SkipBuild) {
    & "$PSScriptRoot\env.ps1"
    cargo build --release -p rustget-cli -p rustget-native-host -p rustget-tray
}

$objectDirectory = Join-Path $OutputDirectory "wix-obj"
New-Item -ItemType Directory -Force -Path $objectDirectory | Out-Null
$tempDirectory = Join-Path $root ".tmp\wix-temp"
New-Item -ItemType Directory -Force -Path $tempDirectory | Out-Null
$env:TEMP = $tempDirectory
$env:TMP = $tempDirectory
$wixobj = Join-Path $objectDirectory "RustGet.wixobj"
$msi = Join-Path $OutputDirectory "RustGet-0.1.0-x64.msi"
if (Test-Path $msi) { Remove-Item -Force $msi }

& $candle -arch x64 -out $wixobj "packaging\windows\RustGet.wxs"
if ($LASTEXITCODE -ne 0) { throw "candle failed" }
& $light -sval -out $msi $wixobj
if ($LASTEXITCODE -ne 0) { throw "light failed" }
$hash = (Get-FileHash -Algorithm SHA256 $msi).Hash.ToLowerInvariant()
Write-Output "msi:    $msi"
Write-Output "sha256: $hash"
Write-Output "bytes:  $((Get-Item $msi).Length)"
