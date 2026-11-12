# Windows packaging

RustGet ships a portable, per-user Windows package:

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
