# Windows packaging

RustGet ships a WiX MSI (x86-64) and a portable per-user Windows package.

```powershell
.\scripts\build-msi.ps1
# dist\RustGet-0.1.0-x64.msi
```

The MSI installs to `Program Files\RustGet`, creates a Start Menu shortcut to
`rustget-desktop.exe` and is per-machine. It requires an x86-64 Windows host and
bundles `WebView2Loader.dll`; the WebView2 runtime must be present (current
Windows 10/11 images include it). The MSI payload was verified
by decompiling it with WiX `dark.exe`.

`scripts\build-msi.ps1` expects WiX 3 candle/light in `D:\dsh\.tools\wix\bin`;
override with `-WixDirectory`.

## Portable package

```powershell
.\scripts\package-windows.ps1
# dist\rustget-0.1.0-windows-x64.zip
```

The archive contains `rustget.exe`, `rustget-native-host.exe`, `rustget-tray.exe`,
README, changelog, licenses, extensions, docs and `install-windows.ps1`.

```powershell
.\install-windows.ps1 -SourceZip .\rustget-0.1.0-windows-x64.zip -AddToPath -StartMenuShortcut
```

The installer copies binaries to `%LOCALAPPDATA%\RustGet\bin`, optionally adds
that directory to the user PATH and creates a Start Menu shortcut. It does not
require administrator rights.

The package is not yet MSI/NSIS, not code-signed and its build is not yet proven
bit-for-bit reproducible.
