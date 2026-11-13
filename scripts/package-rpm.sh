#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

version="0.1.0"
repo="$PWD"
top="/tmp/rustget-rpm"
rm -rf "$top"
mkdir -p "$top"/{BUILD,BUILDROOT,RPMS,SOURCES,SPECS,SRPMS}
cat > "$top/SPECS/rustget.spec" <<EOF
Name:           rustget
Version:        ${version}
Release:        1
Summary:        Fast open-source download manager
License:        MIT or Apache-2.0
BuildArch:      x86_64
AutoReqProv:    no

%description
RustGet is a multi-connection download manager with crash-safe resume,
queues, a local API, media support and browser integration.

%install
mkdir -p %{buildroot}/usr/bin
mkdir -p %{buildroot}/usr/share/doc/rustget
install -m 755 ${repo}/target/linux/release/rustget %{buildroot}/usr/bin/rustget
install -m 755 ${repo}/target/linux/release/rustget-native-host %{buildroot}/usr/bin/rustget-native-host
install -m 644 ${repo}/README.md %{buildroot}/usr/share/doc/rustget/README.md
install -m 644 ${repo}/CHANGELOG.md %{buildroot}/usr/share/doc/rustget/CHANGELOG.md
install -m 644 ${repo}/LICENSE-MIT %{buildroot}/usr/share/doc/rustget/LICENSE-MIT
install -m 644 ${repo}/LICENSE-APACHE %{buildroot}/usr/share/doc/rustget/LICENSE-APACHE

%files
/usr/bin/rustget
/usr/bin/rustget-native-host
/usr/share/doc/rustget/README.md
/usr/share/doc/rustget/CHANGELOG.md
/usr/share/doc/rustget/LICENSE-MIT
/usr/share/doc/rustget/LICENSE-APACHE
EOF
"${RPMBUILD:-rpmbuild}" -bb --define "_topdir $top" "$top/SPECS/rustget.spec"
cp "$top/RPMS/x86_64/rustget-${version}-1.x86_64.rpm" "dist/linux/rustget-${version}-1.x86_64.rpm"
