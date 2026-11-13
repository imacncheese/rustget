#!/usr/bin/env bash
# Run on macOS. Builds a CLI app bundle and a DMG. The native WebView2 shell is
# Windows-only, so the macOS bundle runs the daemon/browser UI.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo build --release -p rustget-cli -p rustget-native-host
app="dist/macos/RustGet.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/rustget "$app/Contents/MacOS/rustget"
cp target/release/rustget-native-host "$app/Contents/MacOS/rustget-native-host"
cp packaging/macos/Info.plist "$app/Contents/Info.plist"
cp README.md CHANGELOG.md LICENSE-MIT LICENSE-APACHE "$app/Contents/Resources/"

if command -v codesign >/dev/null 2>&1; then
    codesign --force --deep --sign - "$app"
fi

hdiutil create -volname RustGet -srcfolder "$app" -ov -format UDZO \
    "dist/macos/RustGet-0.1.0-macos.dmg"
shasum -a 256 "dist/macos/RustGet-0.1.0-macos.dmg"
