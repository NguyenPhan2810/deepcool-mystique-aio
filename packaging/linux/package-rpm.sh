#!/usr/bin/env bash
# Build an .rpm from an already-built release workspace.
#
# Usage: VERSION=0.1.1 bash packaging/linux/package-rpm.sh
# Requires: rpmbuild, rust/target/release populated by
#   cargo build --release --workspace --locked
set -euo pipefail

REPO_ROOT=$(cd "$(dirname "$0")/../.." && pwd)
VERSION=${VERSION:?set VERSION (for example 0.1.1)}
OUT=${OUT:-$REPO_ROOT/dist}
BIN=$REPO_ROOT/rust/target/release
NAME=deepcool-mystique-aio

[ -x "$BIN/mystique-ui" ] || { echo "build first: missing $BIN/mystique-ui" >&2; exit 1; }

topdir=$OUT/rpmbuild
stage=$OUT/rpmstage
rm -rf "$topdir" "$stage"
mkdir -p "$topdir"/{BUILD,RPMS,SOURCES,SPECS,SRPMS}
mkdir -p "$stage/usr/bin" "$stage/usr/lib/systemd/user" "$stage/usr/lib/udev/rules.d" \
         "$stage/usr/share/applications" "$stage/usr/share/licenses/$NAME" \
         "$stage/usr/share/doc/$NAME"

install -m755 "$BIN/mystique"        "$stage/usr/bin/mystique"
install -m755 "$BIN/mystique-daemon" "$stage/usr/bin/mystique-daemon"
install -m755 "$BIN/mystique-ui"     "$stage/usr/bin/mystique-ui"

sed 's|%h/\.local/bin/mystique-daemon|/usr/bin/mystique-daemon|' \
  "$REPO_ROOT/packaging/linux/mystique-daemon.service" \
  > "$stage/usr/lib/systemd/user/mystique-daemon.service"
install -m644 "$REPO_ROOT/packaging/linux/99-mystique.rules" \
  "$stage/usr/lib/udev/rules.d/99-mystique.rules"
install -m644 "$REPO_ROOT/packaging/arch/mystique-ui.desktop" \
  "$stage/usr/share/applications/mystique-ui.desktop"

for size in 16 24 32 48 64 128 256 512; do
  install -Dm644 "$REPO_ROOT/rust/apps/mystique-ui/icons/icon-${size}.png" \
    "$stage/usr/share/icons/hicolor/${size}x${size}/apps/mystique.png"
done

install -m644 "$REPO_ROOT/LICENSE-MIT"    "$stage/usr/share/licenses/$NAME/LICENSE-MIT"
install -m644 "$REPO_ROOT/LICENSE-APACHE" "$stage/usr/share/licenses/$NAME/LICENSE-APACHE"
install -m644 "$REPO_ROOT/docs/INSTALL-linux.md" "$stage/usr/share/doc/$NAME/README.md"

tar -czf "$topdir/SOURCES/stage.tar.gz" -C "$stage" .

cat > "$topdir/SPECS/$NAME.spec" <<EOF
Name:           $NAME
Version:        $VERSION
Release:        1%{?dist}
Summary:        Driver and control panel for the DeepCool MYSTIQUE AIO LCD
License:        MIT OR Apache-2.0
URL:            https://github.com/NguyenPhan2810/deepcool-mystique-aio
Source0:        stage.tar.gz
AutoReqProv:    no
Requires:       gtk3
Requires:       webkit2gtk4.1
Requires:       xdotool
Requires:       hicolor-icon-theme

%description
Headless daemon that owns the panel, a Tauri control panel and a CLI for the
DeepCool MYSTIQUE AIO LCD (USB 3633:0009). Installs a per-user systemd unit and
a udev rule; see /usr/share/doc/$NAME/README.md for the enable steps.

%prep
%setup -q -c -T

%build

%install
mkdir -p %{buildroot}
tar -xzf %{SOURCE0} -C %{buildroot}

%files
/usr/bin/mystique
/usr/bin/mystique-daemon
/usr/bin/mystique-ui
/usr/lib/systemd/user/mystique-daemon.service
/usr/lib/udev/rules.d/99-mystique.rules
/usr/share/applications/mystique-ui.desktop
/usr/share/icons/hicolor/*/apps/mystique.png
/usr/share/licenses/$NAME
/usr/share/doc/$NAME
EOF

rpmbuild --define "_topdir $topdir" -bb "$topdir/SPECS/$NAME.spec"
mkdir -p "$OUT"
find "$topdir/RPMS" -name '*.rpm' -exec cp -v {} "$OUT/" \;
