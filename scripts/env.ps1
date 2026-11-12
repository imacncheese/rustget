# Dot-source this from the repository root: . .\scripts\env.ps1
$repo = Split-Path $PSScriptRoot -Parent
$root = Split-Path $repo -Parent

# The development snapshot uses a portable toolchain in ../.tools; a normal
# checkout can just use rustup on PATH.
$toolRoots = @((Join-Path $repo '.tools'), (Join-Path $root '.tools'))
foreach ($tools in $toolRoots) {
    if (Test-Path (Join-Path $tools 'cargo\bin\rustc.exe')) {
        $env:RUSTUP_HOME = Join-Path $tools 'rustup'
        $env:CARGO_HOME = Join-Path $tools 'cargo'
        $env:PATH = (Join-Path $tools 'w64devkit\w64devkit\bin') + ';' +
            (Join-Path $tools 'cargo\bin') + ';' +
            (Join-Path $tools 'mingit\cmd') + ';' + $env:PATH
        break
    }
}

$tmp = Join-Path $repo '.tmp'
New-Item -ItemType Directory -Force $tmp | Out-Null
$env:TMP = $tmp
$env:TEMP = $tmp
$env:TMPDIR = $tmp
$env:CARGO_TARGET_DIR = Join-Path $repo 'target'

$rustc = Get-Command rustc -ErrorAction SilentlyContinue
if ($rustc) {
    Write-Host "rustget dev shell: $(& rustc --version) [$($rustc.Source)]"
} else {
    Write-Host "rustget dev shell: rustc not found; install Rust or restore ../.tools"
}
