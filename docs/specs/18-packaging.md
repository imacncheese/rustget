# Windows packaging

RustGet ships a WiX MSI (x86-64) and a portable per-user Windows package.

```powershell
.\scripts\build-msi.ps1
# dist\RustGet-0.1.0-x64.msi
```

The MSI uses `WixUI_FeatureTree`: it lets the user choose the install directory,
creates a Start Menu shortcut and offers an optional desktop shortcut. The
shortcut launches the native WPF `RustGet.Native.exe`; the installer also sets
the RustGet icon for Add/Remove Programs. It requires an x86-64 Windows host and
bundles `WebView2Loader.dll`; the WebView2 runtime must be present (current
Windows 10/11 images include it). The MSI payload was verified
by decompiling it with WiX `dark.exe`.

`scripts\build-msi.ps1` expects WiX 3 candle/light in `D:\dsh\.tools\wix\bin`;
override with `-WixDirectory`.

## Linux

```bash
./scripts/package-linux.sh
# dist/linux/rustget_0.1.0_amd64.deb
# dist/linux/rustget-0.1.0-linux-x64.tar.gz
```

The `.deb` installs `rustget` and `rustget-native-host` under `/usr/bin` plus
docs under `/usr/share/doc/rustget`. It was built and its packaged binary
verified in Ubuntu 26.04 (amd64).

`scripts/package-appimage.sh` produces the AppImage (requires a local
`appimagetool` extracted at `/tmp/squashfs-root`).

`scripts/package-rpm.sh` produces the RPM. WSL can recreate a temporary
`rpmbuild` with `scripts/setup-rpmbuild-wsl.sh`; the built package lists
`/usr/bin/rustget`, `/usr/bin/rustget-native-host` and the docs.

## macOS

`scripts/package-macos.sh` builds `RustGet.app`, ad-hoc signs it when
`codesign` is available, and creates a compressed DMG with `hdiutil`. The
desktop WebView2 shell is Windows-only, so the macOS bundle runs the daemon and
browser UI. This path is unverified because the build host is Windows.

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
