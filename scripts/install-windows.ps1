param(
    [Parameter(Mandatory = $true)][string]$SourceZip,
    [string]$InstallDirectory = (Join-Path $env:LOCALAPPDATA "RustGet"),
    [switch]$AddToPath,
    [switch]$StartMenuShortcut
)

$ErrorActionPreference = "Stop"
if (-not (Test-Path $SourceZip)) { throw "package not found: $SourceZip" }
$temporary = Join-Path ([System.IO.Path]::GetTempPath()) ("rustget-install-" + [guid]::NewGuid())
New-Item -ItemType Directory -Force -Path $temporary | Out-Null
try {
    Expand-Archive -Path $SourceZip -DestinationPath $temporary -Force
    $bin = Join-Path $InstallDirectory "bin"
    New-Item -ItemType Directory -Force -Path $bin | Out-Null
    Copy-Item (Join-Path $temporary "*.exe") -Destination $bin -Force
    Copy-Item (Join-Path $temporary "*.dll") -Destination $bin -Force
    Copy-Item (Join-Path $temporary "extensions") -Destination $InstallDirectory -Recurse -Force -ErrorAction SilentlyContinue
    Write-Output "installed: $bin"

    if ($AddToPath) {
        $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
        if ($userPath -notlike "*$bin*") {
            [Environment]::SetEnvironmentVariable("Path", "$userPath;$bin", "User")
            Write-Output "added to user PATH (open a new terminal)"
        }
    }
    if ($StartMenuShortcut) {
        $programs = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs"
        New-Item -ItemType Directory -Force -Path $programs | Out-Null
        $shell = New-Object -ComObject WScript.Shell
        $shortcut = $shell.CreateShortcut((Join-Path $programs "RustGet.lnk"))
        $shortcut.TargetPath = Join-Path $bin "rustget-desktop.exe"
        $shortcut.Save()
        Write-Output "start menu shortcut created"
    }
}
finally {
    Remove-Item -Recurse -Force $temporary -ErrorAction SilentlyContinue
}
