$repo = Split-Path $PSScriptRoot -Parent
$root = Split-Path $repo -Parent
$env:RUSTUP_HOME = Join-Path $root '.tools\rustup'
$env:CARGO_HOME = Join-Path $root '.tools\cargo'
$env:PATH = (Join-Path $root '.tools\w64devkit\w64devkit\bin') + ';' + (Join-Path $root '.tools\cargo\bin') + ';' + (Join-Path $root '.tools\mingit\cmd') + ';' + $env:PATH
$tmp = Join-Path $repo '.tmp'
New-Item -ItemType Directory -Force $tmp | Out-Null
$env:TMP = $tmp
$env:TEMP = $tmp
$env:TMPDIR = $tmp
$env:CARGO_TARGET_DIR = Join-Path $repo 'target'
Write-Host "rustget dev shell: $(& rustc --version)"
