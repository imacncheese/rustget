#!/usr/bin/env bash
set -euo pipefail
root="/tmp/rpmroot"
rm -rf "$root"
mkdir -p "$root"
cd /tmp
apt-get download rpm rpm-common librpmbuild10 librpm10 librpmio10 librpmsign10 \
    liblua5.3-0 librpm-sequoia-1 libfsverity0
for package in rpm_*.deb rpm-common_*.deb librpmbuild10_*.deb librpm10_*.deb \
    librpmio10_*.deb librpmsign10_*.deb liblua5.3-0_*.deb \
    librpm-sequoia-1_*.deb libfsverity0_*.deb; do
    dpkg-deb -x "$package" "$root"
done
LD_LIBRARY_PATH="$root/usr/lib/x86_64-linux-gnu" \
    "$root/usr/bin/rpmbuild" --version
