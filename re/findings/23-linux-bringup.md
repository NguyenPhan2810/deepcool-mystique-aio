# 23 — Linux bring-up: first native run, the clock pipe, and the uaccess gap

Date: 2026-11-20. Host: Arch desktop `arch` (i9-14900K, RTX 4070), systemd 259, rust 1.100.0,
WebKitGTK 2.54. Device: the real MYSTIQUE 360, `3633:0009`.

This is the first time the Rust tree was built and run on Linux. Everything before this
finding was Windows (`re/HANDOVER.md`).

## What ran

- `cargo build --workspace` — clean, 35 s debug, 51 s release.
- `cargo test --workspace` — 408 passed on the first run; 409 after the clock fix below.
- `cargo clippy --workspace --all-targets -- -D warnings` — clean; `cargo fmt --check` — clean.
- `mystique-daemon` held the panel for hours at 1 Hz; `mystique-ui` (Tauri/WebKitGTK)
  attached over `$XDG_RUNTIME_DIR/mystique/control.sock` and survived UI restarts.

## The panel clock was 10 minutes ahead — `0x0A` was on the wrong pipe

Symptom: the panel's Time area read 12:13 while the host read 12:03, and it kept its own
offset. Telemetry was correct throughout.

Cause: `Command::ModeSwitch` carried `0x0A` and routed to `Pipe::Init` for every call site,
including the once-per-second `sync_clock`. The sweep (`re/findings/11-sweep-results.md`)
records that the init pipe only returns a neutral `55 2E 00` ACK for `0x0A`, while the data
pipe honours it with a non-empty payload. The daemon was therefore reporting success and
never setting the panel's clock — the same symptom `re/findings/22-clock.md` describes.

Fix: a distinct `Command::Clock` — same byte `0x0A`, `Pipe::Data`, payload-gated. The init
handshake keeps `ModeSwitch` on the init pipe, so the proven 13-step sequence is unchanged.
A test pins both routings and the shared byte.

Verified on hardware: within one second of restarting the daemon, the panel read the host
time. **This bug was cross-platform**: the Windows daemon had the same routing, so its panel
clock was equally wrong whenever the daemon, not the vendor app, owned the panel. The
Windows hardware pass never checked the panel clock, which is why it went unnoticed.

## `TAG+="uaccess"` tagged the device but set no ACL

`packaging/linux/99-mystique.rules` relies on `TAG+="uaccess"` for access. On systemd 259 the
device ends up with `TAGS=:uaccess:seat:` and `ID_FOR_SEAT=...`, yet `getfacl
/dev/bus/usb/001/011` shows only `user::rw-` and libusb fails with access-denied. `udevadm
test` confirms the tag is applied and the seat is assigned.

Local deviation on this host: a `RUN+="/usr/bin/setfacl -m u:nguyen:rw $devnode"` clause,
kept alongside the tag. Verified with `udevadm test`, which expands `$devnode` and schedules
the command. The packaged rule is unchanged; the deviation lives in `/etc/udev/rules.d/`,
which overrides the packaged copy of the same name.

Re-check before deleting the RUN line: if a future systemd materialises the ACL from the tag,
the `getfacl` output will show `user:<name>:rw-` without help.

## Install on this host

- Binaries: `~/.local/bin/{mystique,mystique-daemon,mystique-ui}` (release).
- Daemon: `mystique-daemon.service` from `packaging/linux/`, per-user, enabled, `Linger=yes`
  so it starts at boot without a login — the ACL comes from the udev `RUN`, not a session.
- UI: desktop entry `MYSTIQUE LCD Control`, icon at `hicolor/32x32/apps/mystique.png`.
- The Python `linux/` driver was removed from the system; `linux/` as source is untouched.

## Arch package

`packaging/arch/PKGBUILD` builds `mystique` from this tree: `/usr/bin/{mystique,
mystique-daemon, mystique-ui}`, the user unit, the udev rule, the desktop entry and the icon.
Two traps, both found by building it:

- **`options=(!lto)` is required.** `makepkg`'s `lto` option appends `-flto=auto` to `CFLAGS`,
  and `rusb` uses `libusb1-sys` with `vendored`, so libusb is compiled from C by the `cc`
  crate. The GCC LTO objects in that archive do not resolve at rustc's link step and the
  build dies on undefined `libusb_*` symbols. The Rust release profile has no LTO, so nothing
  is lost.
- **The packaged unit must point at `/usr/bin`.** `packaging/linux/mystique-daemon.service`
  is written for a hand install and hardcodes `%h/.local/bin/mystique-daemon`; installed
  verbatim, the package's service fails with `status=203/EXEC`. `package()` rewrites the
  path.

Host-specific settings live outside the package on purpose: the ACL `RUN` in
`/etc/udev/rules.d/` and the label drop-in in `~/.config/systemd/user/`.

## Not verified

- Clock across a USB reset or power cycle.
- Boot start with nobody logged in — linger is enabled, but this needs a real reboot.
- UI behaviour under the NVIDIA/WebKitGL path: WebKitGTK falls back to software rendering
  here (`GTK failed to initialize GL`), which works but is worth knowing.
