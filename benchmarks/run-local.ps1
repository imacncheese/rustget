param(
  [int]$Size = 134217728,
  [int]$Connections = 16,
  [int]$Runs = 3,
  [int]$Port = 59552
)

$ErrorActionPreference = "Stop"
$repo = Split-Path $PSScriptRoot -Parent
Set-Location $repo
& "$repo/scripts/env.ps1" | Out-Null

Write-Host "building release binaries..."
cargo build --release -p rustget-cli -p rustget-testkit | Out-Host

$work = Join-Path $repo ".tmp/bench"
New-Item -ItemType Directory -Force $work | Out-Null
$testkitLog = Join-Path $work "testkit.log"
$testkit = Start-Process -FilePath "$repo/target/release/rustget-testkit.exe" `
  -ArgumentList @("--listen", "127.0.0.1:$Port", "--size", "$Size", "--name", "bench.bin") `
  -RedirectStandardOutput $testkitLog -PassThru -WindowStyle Hidden
Start-Sleep -Milliseconds 600

try {
  $url = "http://127.0.0.1:$Port/files/bench.bin"
  $rust = @()
  $aria = @()
  $aria2 = if ($env:ARIA2C) { $env:ARIA2C } else { (Get-Command aria2c -ErrorAction SilentlyContinue).Source }

  for ($i = 1; $i -le $Runs; $i++) {
    $dl = Join-Path $work "rust-$i"
    Remove-Item -Recurse -Force $dl -ErrorAction SilentlyContinue
    $sw = [Diagnostics.Stopwatch]::StartNew()
    & "$repo/target/release/rustget.exe" --local `
      --data-dir (Join-Path $work "data-$i") --download-dir $dl `
      add $url --connections $Connections --output (Join-Path $dl "file.bin") --quiet | Out-Null
    $sw.Stop()
    $rust += $sw.Elapsed.TotalSeconds
    $bytes = (Get-Item (Join-Path $dl "file.bin")).Length
    if ($bytes -ne $Size) { throw "RustGet size mismatch: $bytes" }
  }

  if ($aria2) {
    for ($i = 1; $i -le $Runs; $i++) {
      $dl = Join-Path $work "aria-$i"
      Remove-Item -Recurse -Force $dl -ErrorAction SilentlyContinue
      New-Item -ItemType Directory -Force $dl | Out-Null
      $sw = [Diagnostics.Stopwatch]::StartNew()
      & $aria2 --allow-overwrite=true --auto-file-renaming=false --file-allocation=none `
        -x$Connections -s$Connections --dir $dl --out file.bin $url 2>$null | Out-Null
      $sw.Stop()
      $aria += $sw.Elapsed.TotalSeconds
      $bytes = (Get-Item (Join-Path $dl "file.bin")).Length
      if ($bytes -ne $Size) { throw "aria2 size mismatch: $bytes" }
    }
  }

  $rustMedian = ($rust | Sort-Object)[[int][math]::Floor($Runs / 2)]
  Write-Host ("RustGet runs: {0}" -f ($rust -join ", "))
  Write-Host ("RustGet median: {0:N3}s  {1:N1} MiB/s" -f $rustMedian, ($Size / 1MB / $rustMedian))
  if ($aria.Count -gt 0) {
    $ariaMedian = ($aria | Sort-Object)[[int][math]::Floor($Runs / 2)]
    Write-Host ("aria2 runs: {0}" -f ($aria -join ", "))
    Write-Host ("aria2 median: {0:N3}s  {1:N1} MiB/s" -f $ariaMedian, ($Size / 1MB / $ariaMedian))
    Write-Host ("ratio (rust/aria wall): {0:N2}x" -f ($ariaMedian / $rustMedian))
  } else {
    Write-Host "aria2c not found; set ARIA2C to compare."
  }
} finally {
  if ($testkit -and -not $testkit.HasExited) { Stop-Process -Id $testkit.Id -Force }
}
