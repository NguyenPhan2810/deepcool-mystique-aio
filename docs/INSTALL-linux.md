# Building on Linux

Everything except the control panel is plain Rust: `mystique` (CLI) and `mystique-daemon`
depend on no GTK, WebKit or desktop library at all. Only `mystique-ui` needs the full desktop
stack. So a headless install on a server or a minimal distro is cheap; the UI is the expensive
part.

Prebuilt packages are attached to each release: Arch (`pacman -U`), Debian/Ubuntu
(`apt install ./file.deb`) and Fedora (`dnf install ./file.rpm`). The Arch PKGBUILD lives in
`packaging/arch/`. On every other distribution the supported path is building from source — a prebuilt binary would be tied to
this machine's glibc and WebKitGTK versions.

## 1. Toolchain and system libraries

Rust 1.90 or newer (the workspace floor is 1.85, the UI needs 1.90):

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
. "$HOME/.cargo/env"
```

Distribution packages (names from Tauri's Linux prerequisites; adjust if your release differs):

**Debian / Ubuntu**

```sh
sudo apt update
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
```

**Fedora**

```sh
sudo dnf install webkit2gtk4.1-devel openssl-devel curl wget file \
  libappindicator-gtk3-devel librsvg2-devel libxdo-devel
sudo dnf group install "c-development"
```

**openSUSE**

```sh
sudo zypper in webkit2gtk4.1-devel libopenssl-devel curl wget file \
  libappindicator3-1 librsvg-devel libxdo-devel
sudo zypper in -t pattern devel_basis
```

If your openSUSE release only ships `webkit2gtk3-devel`, use that — the 4.1 API is what Rust
binds, so check `pkg-config --modversion webkit2gtk-4.1` before building.

**Alpine (musl)**

```sh
sudo apk add build-base webkit2gtk-4.1-dev curl wget file openssl \
  libayatana-appindicator-dev librsvg font-dejavu
```

Headless only (no UI, so no GTK/WebKit needed — `build-base` and Rust are enough):

```sh
cargo build --release -p mystique-daemon -p mystique-cli --locked
```

## 2. Build

```sh
git clone https://github.com/NguyenPhan2810/deepcool-mystique-aio
cd deepcool-mystique-aio/rust
cargo build --release --workspace --locked
```

Binaries land in `rust/target/release/`: `mystique`, `mystique-daemon`, `mystique-ui`.

Distro packagers: disable LTO for the C build (`options=(!lto)` in a PKGBUILD, or clear
`-flto` from `CFLAGS`). `rusb` vendors libusb and compiles it from C; GCC LTO objects there do
not resolve at rustc's link step and the build dies on undefined `libusb_*` symbols.

## 3. Install

```sh
install -Dm755 rust/target/release/mystique        ~/.local/bin/mystique
install -Dm755 rust/target/release/mystique-daemon ~/.local/bin/mystique-daemon
install -Dm755 rust/target/release/mystique-ui     ~/.local/bin/mystique-ui
```

Permissions: the panel is a vendor-specific USB device (`3633:0009`) with no kernel driver, so
the user needs write access to its node. Either copy `packaging/linux/99-mystique.rules` into
`/etc/udev/rules.d/` (it uses `TAG+="uaccess"`), or grant the node to a group.

```sh
sudo install -Dm644 packaging/linux/99-mystique.rules /etc/udev/rules.d/99-mystique.rules
sudo udevadm control --reload-rules
sudo udevadm trigger --subsystem-match=usb --attr-match=idVendor=3633
```

If the ACL does not appear (`getfacl /dev/bus/usb/...` shows no entry for your user), add
`RUN+="/usr/bin/setfacl -m u:$USER:rw $devnode"` to that rule — some systemd versions tag the
device `uaccess` without materialising the ACL.

Daemon unit (per user, no root):

```sh
install -Dm644 packaging/linux/mystique-daemon.service ~/.config/systemd/user/mystique-daemon.service
systemctl --user daemon-reload
systemctl --user enable --now mystique-daemon
sudo loginctl enable-linger "$USER"     # start at boot, before login
```

Desktop entry (optional, for the menu):

```sh
install -Dm644 packaging/arch/mystique-ui.desktop ~/.local/share/applications/
install -Dm644 rust/apps/mystique-ui/icons/icon-256.png \
  ~/.local/share/icons/hicolor/256x256/apps/mystique.png
```

The entry must be named `mystique-ui.desktop`: that is the Wayland app id KWin/GTK reports, and
renaming it breaks the window icon, grouping and launcher matching.

## 4. Check it

```sh
systemctl --user status mystique-daemon
journalctl --user -u mystique-daemon -f
curl --unix-socket "$XDG_RUNTIME_DIR/mystique/control.sock" http://localhost/ -s || true
```

Exactly one process may own the panel. If the daemon is running, `mystique status` will report
the device as busy — stop the unit first.
