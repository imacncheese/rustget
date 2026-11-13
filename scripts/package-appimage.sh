#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

version="0.1.0"
root="target/linux/release"
app="/tmp/RustGet.AppDir"
output="dist/linux/RustGet-${version}-x86_64.AppImage"
rm -rf "$app"
mkdir -p "$app/usr/bin"
cp "$root/rustget" "$root/rustget-native-host" "$app/usr/bin/"
cp docs/assets/ui-main.png "$app/rustget.png"
cp docs/assets/ui-main.png "$app/.DirIcon"

cat > "$app/AppRun" <<'EOF'
#!/bin/sh
exec "$APPDIR/usr/bin/rustget" serve --open "$@"
EOF
chmod +x "$app/AppRun"

cat > "$app/rustget.desktop" <<'EOF'
[Desktop Entry]
Name=RustGet
Exec=rustget serve --open
Icon=rustget
Type=Application
Categories=Network;FileTransfer;
EOF

ARCH=x86_64 /tmp/squashfs-root/AppRun "$app" "$output"
chmod +x "$output"
file "$output"
