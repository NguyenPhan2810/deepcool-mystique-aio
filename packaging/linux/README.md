# Linux packaging — MYSTIQUE daemon

Data files only. Nothing here is consumed by `cargo build`; the installer is you.

Two files matter:

| File | Purpose |
|---|---|
| `99-mystique.rules` | udev rule for USB `3633:0009`, so the seated user can open the panel through libusb |
| `mystique-daemon.service` | systemd **user** unit running the daemon, restart on failure |

There is no Linux build of the vendor `MystiqueService`, and no vendor installer. The
Rust daemon is the single owner of the device on this platform; the desktop UI, the
CLI and anything else are clients that talk to it over the control socket.

## 1. Build and install the binary

From `rust/`:

```sh
cargo build --release -p mystique-daemon
install -Dm755 target/release/mystique-daemon ~/.local/bin/mystique-daemon
```

The daemon needs no root and no setuid bit. It opens the panel through libusb, which
talks to `usbfs`; the udev rule below is what grants the permission.

The optional UI is built separately (it needs the WebKitGTK development packages):

```sh
cargo build --release -p mystique-ui
```

## 2. Install the udev rule

```sh
sudo install -Dm644 packaging/linux/99-mystique.rules /etc/udev/rules.d/99-mystique.rules
sudo udevadm control --reload-rules
sudo udevadm trigger --subsystem-match=usb --attr-match=idVendor=3633
```

Unplug and replug the panel once so the node is re-created with the new rule. Verify:

```sh
getfacl /dev/bus/usb/$(lsusb -d 3633:0009 | cut -d: -f1 | cut -d' ' -f2,4 | tr ':' '/')
```

`TAG+="uaccess"` gives the user on the active seat an ACL. It is deliberately not a
`GROUP="plugdev"` rule: a group needs membership applied before the rule runs, and it
leaves the device open to every member even when nobody is sitting at the machine. The
rule is named `99-mystique.rules` because udev applies files in lexical order and
`m` sorts after `d`, so a vendor rule such as `99-deepcool.rules` cannot override it.

**Over SSH there is no seat**, so `uaccess` grants nothing. Either run the daemon from a
local graphical/console session, or add a group rule of your own. The daemon itself does
not care how it got access.

## 3. Enable the user unit

```sh
systemctl --user daemon-reload
systemctl --user enable --now mystique-daemon.service
systemctl --user status mystique-daemon.service
journalctl --user -u mystique-daemon.service -f
```

The unit assumes `~/.local/bin/mystique-daemon`; edit `ExecStart=` if the binary lives
elsewhere. A user unit starts at login. To keep it running when nobody is logged in:

```sh
loginctl enable-linger "$USER"
```

The control socket is `$XDG_RUNTIME_DIR/mystique/control.sock` (directory `0700`, socket
`0600`), falling back to `/tmp/mystique-<uid>/control.sock` when `XDG_RUNTIME_DIR` is
unset. `--endpoint /absolute/path.sock` overrides it.

## What owns the device

- The daemon holds the panel for its whole lifetime. Closing the UI, logging out of the
  UI's session or restarting the window does not stop the stream.
- Starting the UI when no daemon runs makes it spawn one. Starting a second daemon exits
  at the instance lock before it touches the device.
- The CLI (`mystique ...`) opens the device directly. Stop the daemon first:
  `systemctl --user stop mystique-daemon`. Two owners are refused by the kernel, not by
  politeness.

## Troubleshooting

| Symptom | Cause |
|---|---|
| `access denied` opening the device | rule not installed, panel not replugged, or no seat session (SSH) |
| `another mystique-daemon is already running` | the user unit is active; stop it before running the CLI |
| socket missing under `$XDG_RUNTIME_DIR` | the daemon never started; check `journalctl --user -u mystique-daemon` |
| panel stops answering after a failed write | libusb pipe is wedged; unplug/replug or power-cycle the USB port. Restarting the daemon is not enough |
| UI window opens but shows no telemetry | the daemon is not running; the UI's Connection section names the spawn failure and the log path |

## Verified on this platform

Ubuntu 26.04, rustc 1.98.0, gcc 15.2.0. `cargo build --workspace`, `cargo test
--workspace`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt
--all -- --check` pass, and the daemon was started, queried over its Unix socket and
stopped with `SIGTERM` on this machine. No panel was attached to that machine, so the
USB path itself is still unproven on Linux — see `re/HANDOVER.md`.
