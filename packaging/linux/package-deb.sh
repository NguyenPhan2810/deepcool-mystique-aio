#!/usr/bin/env bash
# Build a .deb from an already-built release workspace.
#
# Usage: VERSION=0.1.1 bash packaging/linux/package-deb.sh
# Requires: dpkg-deb, rust/target/release populated by
#   cargo build --release --workspace --locked
set -euo pipefail

REPO_ROOT=$(cd "$(dirname "$0")/../.." && pwd)
VERSION=${VERSION:?set VERSION (for example 0.1.1)}
ARCH=${ARCH:-amd64}
OUT=${OUT:-$REPO_ROOT/dist}
BIN=$REPO_ROOT/rust/target/release
NAME=deepcool-mystique-aio

[ -x "$BIN/mystique-ui" ] || { echo "build first: missing $BIN/mystique-ui" >&2; exit 1; }

root=$OUT/debroot
rm -rf "$root"
mkdir -p "$root/DEBIAN" "$root/usr/bin" \
         "$root/usr/lib/systemd/user" "$root/usr/lib/udev/rules.d" \
         "$root/usr/share/applications" "$root/usr/share/licenses/$NAME"

install -m755 "$BIN/mystique"        "$root/usr/bin/mystique"
install -m755 "$BIN/mystique-daemon" "$root/usr/bin/mystique-daemon"
install -m755 "$BIN/mystique-ui"     "$root/usr/bin/mystique-ui"

# The shipped unit targets a ~/.local/bin install; a package must use /usr/bin.
sed 's|%h/\.local/bin/mystique-daemon|/usr/bin/mystique-daemon|' \
  "$REPO_ROOT/packaging/linux/mystique-daemon.service" \
  > "$root/usr/lib/systemd/user/mystique-daemon.service"
install -m644 "$REPO_ROOT/packaging/linux/99-mystique.rules" \
  "$root/usr/lib/udev/rules.d/99-mystique.rules"
install -m644 "$REPO_ROOT/packaging/arch/mystique-ui.desktop" \
  "$root/usr/share/applications/mystique-ui.desktop"

for size in 16 24 32 48 64 128 256 512; do
  install -Dm644 "$REPO_ROOT/rust/apps/mystique-ui/icons/icon-${size}.png" \
    "$root/usr/share/icons/hicolor/${size}x${size}/apps/mystique.png"
done

install -m644 "$REPO_ROOT/LICENSE-MIT"    "$root/usr/share/licenses/$NAME/LICENSE-MIT"
install -m644 "$REPO_ROOT/LICENSE-APACHE" "$root/usr/share/licenses/$NAME/LICENSE-APACHE"

installed_kb=$(du -sk --exclude=DEBIAN "$root" | cut -f1)

cat > "$root/DEBIAN/control" <<EOF
Package: $NAME
Version: $VERSION
Section: utils
Priority: optional
Architecture: $ARCH
Depends: libgtk-3-0t64 | libgtk-3-0, libwebkit2gtk-4.1-0, libxdo3, hicolor-icon-theme
Installed-Size: $installed_kb
Maintainer: Nguyen Phan <nguyen@nguyenphan.org>
Homepage: https://github.com/NguyenPhan2810/deepcool-mystique-aio
Description: Driver and control panel for the DeepCool MYSTIQUE AIO LCD
 Headless daemon that owns the panel, a Tauri control panel and a CLI.
 The package installs the per-user systemd unit and the udev rule; see
 /usr/share/doc/$NAME/README.md for the enable steps.
EOF

mkdir -p "$root/usr/share/doc/$NAME"
install -m644 "$REPO_ROOT/docs/INSTALL-linux.md" "$root/usr/share/doc/$NAME/README.md"

mkdir -p "$OUT"
dpkg-deb --build --root-owner-group "$root" "$OUT/${NAME}_${VERSION}_${ARCH}.deb"
ls -l "$OUT/${NAME}_${VERSION}_${ARCH}.deb"
