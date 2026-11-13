#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

target="${CARGO_TARGET_DIR:-target/linux}"
cargo build --release -p rustget-cli -p rustget-native-host --target-dir "$target"
version="$(grep -m1 '^version = "' Cargo.toml | cut -d'"' -f2)"
mkdir -p dist/linux
root="${TMPDIR:-/tmp}/rustget_${version}_amd64"
rm -rf "$root"
install -Dm755 "$target/release/rustget" "$root/usr/bin/rustget"
install -Dm755 "$target/release/rustget-native-host" "$root/usr/bin/rustget-native-host"
install -Dm644 README.md "$root/usr/share/doc/rustget/README.md"
install -Dm644 CHANGELOG.md "$root/usr/share/doc/rustget/CHANGELOG.md"
install -Dm644 LICENSE-MIT "$root/usr/share/doc/rustget/LICENSE-MIT"
install -Dm644 LICENSE-APACHE "$root/usr/share/doc/rustget/LICENSE-APACHE"
mkdir -p "$root/DEBIAN"
cat > "$root/DEBIAN/control" <<EOF
Package: rustget
Version: ${version}
Section: net
Priority: optional
Architecture: amd64
Maintainer: RustGet Contributors <contributors@rustget.invalid>
Description: Fast open-source download manager
 RustGet is a multi-connection download manager with crash-safe resume,
 queues, a local API, media support and browser integration.
EOF
dpkg-deb --root-owner-group --build "$root" "dist/linux/rustget_${version}_amd64.deb"
tar -C "$root" -czf "dist/linux/rustget-${version}-linux-x64.tar.gz" usr
dpkg-deb --info "dist/linux/rustget_${version}_amd64.deb"
dpkg-deb --contents "dist/linux/rustget_${version}_amd64.deb"
