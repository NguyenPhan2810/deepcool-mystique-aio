# mystique — DeepCool MYSTIQUE AIO LCD on Linux

A userspace driver for the DeepCool MYSTIQUE AIO LCD (`3633:0009`), written in
Python with [pyusb](https://github.com/pyusb/pyusb) on the libusb backend.

No kernel driver, no DKMS module, no vendor service. One process claims
interface 0 and can:

- drive the five-value **Machine Info telemetry** display, and
- upload a **static image or animated GIF** that persists on the device.

This is the Linux replacement for `MystiqueUsbTool.exe` for telemetry, with the
image path added from a public, hardware-confirmed protocol record (see
[Credits](#credits)).

> **Known limitation, not solved:** after an image/GIF upload the device cannot
> reliably be returned to Machine Info mode. `mystique init` may appear to
> succeed and leave the uploaded image on screen. Plan uploads accordingly;
> see [Returning to Machine Info](#returning-to-machine-info-is-not-reliable).

## Supported hardware

| | |
|---|---|
| Device | DeepCool MYSTIQUE (240/360) AIO LCD |
| VID:PID | `3633:0009` |
| USB class | vendor-specific, bulk endpoints, **not** HID, **not** serial |
| Endpoints | `0x01` OUT / `0x81` IN (init, info, image upload), `0x02` OUT / `0x82` IN (status, telemetry) |
| Command packet | 48 bytes out, up to 64 bytes in, 1500 ms timeout |
| Upload frame | one 64-byte `DCLd` header + raw JPEG in a single bulk write |
| Upload size | 480×640 portrait (hardware-confirmed for uploads) |

`re/CONTEXT.md` records the telemetry protocol. The image path is documented in
`DESIGN.md` and is derived from the public sources listed under
[Credits](#credits).

## Requirements

- Linux with sysfs/procfs (any modern distribution)
- Python 3.9 or newer
- `libusb-1.0` (Debian/Ubuntu: `libusb-1.0-0`; Fedora: `libusb1`)
- `pyusb` (installed automatically)
- **Pillow** only for `image`/`gif` uploads (optional extra)

## Install

```bash
cd linux
python3 -m venv .venv

# Telemetry only:
.venv/bin/pip install -e .

# Telemetry + image/GIF:
.venv/bin/pip install -e '.[image]'

# USB permissions: use the group rule, or the uaccess alternative in it.
sudo install -m 0644 packaging/udev/99-mystique.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules
sudo udevadm trigger --subsystem-match=usb
sudo groupadd -f plugdev                     # Debian/Ubuntu already have it
sudo usermod -aG plugdev "$USER"             # log out and back in
```

Replug the AIO once so udev applies the rule to the existing device node.

Check it:

```bash
.venv/bin/mystique status
```

Expected shape (identifier redacted, as always by default):

```
Device: 3633:0009 bus=001 address=010
Interface 0: 4 endpoints
  0x81 Bulk      max-packet=64
  0x01 Bulk      max-packet=64
  0x82 Bulk      max-packet=64
  0x02 Bulk      max-packet=64
Status: mode=MachineInfo (0xFF), rotation=0 degrees
```

If you prefer a system-wide command, `pipx install '.[image]'` works too and
puts `mystique` on `PATH`.

### Permissions: plugdev or uaccess

The shipped rule grants `MODE="0660"` and `GROUP="plugdev"`. On a single-user
desktop you can replace those two settings with `TAG+="uaccess"`; systemd-logind
then grants an ACL to the user holding the active seat. `uaccess` is wrong for a
service account that is never logged in — use `plugdev` there.

## Verbs

```
mystique [pipes|status|info|init [rotation]|stream [seconds]|live [updates]|image <path>|gif <path>]
         [--show-device-id] [--rotation=0..3] [--fit=contain|cover]
```

| Verb | What it does |
|---|---|
| `pipes` | Open the device and list interface 0's endpoints (the default) |
| `status` | Read command `0x10`: display mode and rotation |
| `info` | Send command `0x12` and print the reply, identifier masked |
| `init [rotation]` | Run the 13-command Machine Info handshake |
| `stream [seconds]` | Send *synthetic* one-per-second frames (default 30, 1–600) |
| `live [updates]` | Init, then stream real Linux sensor readings (default forever) |
| `image <path>` | Upload a static image (or every frame of an animated file) |
| `gif <path>` | Same upload path, named for animated files |

`rotation` is `0`–`3`, meaning 0°, 90°, 180°, 270°. Anything else falls back to
`0`, matching the Windows tool.

`--rotation=N` is a Linux-only addition: it overrides the positional, so a unit
can run `mystique --rotation=2 live 0` and get an unlimited stream at 180°. It
is also passed to the image-mode handshake, but **whether rotation affects
image/GIF playback is unverified**; the public implementation always uses 0.
Leave it unset for uploads unless you have tested it on your panel.

`--fit` applies to `image`/`gif`: `contain` (default) letterboxes on `#0b0f14`,
`cover` centre-crops. Both target 480×640.

### The `live` positional quirk (preserved on purpose)

The second positional argument is read by **both** `init` and the update
counter, exactly as in `MystiqueUsbTool`:

- `mystique live 0` — rotation 0, stream until Ctrl+C
- `mystique live 100` — 100 updates; `100` is not a valid rotation, so rotation
  stays 0
- `mystique live 2` — **sets rotation to 180°** *and* sends 2 updates

Use a count above `3` (or `0`) when you only mean "how many updates". The
systemd unit uses `live 0` deliberately.

### Examples

```bash
mystique pipes
mystique status
mystique info
mystique init 1                       # 90 degrees
mystique stream 5                     # synthetic values, no sensors needed
mystique live 0                       # real sensors, Ctrl+C to stop
mystique --rotation=1 live 0          # 90 degrees, unlimited updates
mystique image ~/Pictures/logo.png    # letterboxed
mystique image ~/Pictures/wall.jpg --fit cover
mystique gif ~/Pictures/loop.gif
```

`Ctrl+C` and `SIGTERM` stop `live` between frames; the panel keeps its last
frame. Nothing is sent on exit, by design (see `DESIGN.md`).

## Image and GIF upload

`image`/`gif` decode with Pillow, resize to 480×640, encode JPEG, and run the
full device sequence:

1. `0x14` erase stored images — **mandatory**, or a second upload silently keeps
   the previous content;
2. image-mode handshake (`0x0A` with `EA 07 08 09 11 3A 09 00`, status,
   a heartbeat display packet, `0x12`, `0x11`, then the shared setup steps);
3. `0x0F` begin upload;
4. per frame, one 64-byte `DCLd` header **immediately followed by the JPEG in a
   single bulk write**. Splitting them makes libusb insert a zero-length packet
   at the 64-byte boundary and the device silently ignores the upload;
5. `dcldfinish`, `0x08` commit with `[0x01, 0x01]`;
6. the image-mode handshake again — required; without the replay the upload is
   accepted but nothing appears.

The encoder handles the two silent-rejection traps for you:

- **JFIF APP0 immediately after SOI.** Pillow emits it; the code inserts it if
  a future Pillow stops doing so, and a test asserts `FF D8 FF E0` plus
  `JFIF\0`.
- **Standard, not content-optimized Huffman tables.** Pillow is called with
  `optimize=False`; a test parses the DHT segments and asserts the ITU T.81
  Annex K bit-length tables.

If you bypass `mystique` and upload your own JPEGs, both properties matter: a
JPEG without them uploads "successfully" and never appears.

GIFs are limited to 255 frames (the header's frame-count byte) and each frame's
delay is clamped to 20–65535 ms; a static image uses one frame at 100 ms.

### Returning to Machine Info is not reliable

After a successful upload the device stays in Image mode. This driver does not
claim to know how to switch it back. The public protocol record tested several
hypotheses and none reproduced reliably:

- sending `0x14` before `init` worked once and then did not;
- a third ModeSwitch payload seen in a vendor capture was not reproducible;
- the vendor app may close and reopen the USB handle during the transition, but
  that is an untested lead.

So `mystique init` / `mystique live` after an upload may run without a transfer
error and leave the image on screen. A replug or power cycle may or may not
help; do not rely on it. Practically: upload when you want the panel to show
that image, not between telemetry sessions. This is the biggest open issue in
the project.

## Run at boot (systemd)

```bash
sudo install -m 0644 packaging/systemd/mystique.service /etc/systemd/system/
# Edit ExecStart if `mystique` is not /usr/bin/mystique:
sudo systemctl edit --full mystique
sudo systemctl daemon-reload
sudo systemctl enable --now mystique
journalctl -u mystique -f
```

The unit runs `mystique live 0`, restarts on failure after 5 s, and stops with
`SIGTERM`. A successful init is re-run by every start, which is required: a
reopened (or rebooted) device is not in Machine Info mode, and telemetry writes
would be acknowledged while displaying nothing. For a mounting rotation other
than 0°, change `ExecStart` to `mystique --rotation=1 live 0` (or `2`, `3`).

Init success is **not** gated on status byte 5. A real capture recorded byte 5
= `0x00` while the panel was demonstrably showing live Machine Info, so the
driver treats that byte as advisory and prints a note when it is not `0xFF`.

The unit runs as root by default, which needs no udev rule. To run unprivileged,
create a system user, add it to `plugdev`, and uncomment `User=` /
`SupplementaryGroups=` in the unit. Do not use `uaccess` for a service account:
it keys off an active login session.

## Sensors on Linux

There is no DeepCool Sensor Bridge here. `mystique/telemetry.py` reads:

| Value | Source |
|---|---|
| CPU temperature | `/sys/class/hwmon/hwmon*/` named `coretemp`, `k10temp`, `zenpower`, `cpu_thermal`; prefers the `Package id 0` / `Tctl` / `Tdie` label over individual cores |
| CPU usage | delta of the aggregate `cpu` line in `/proc/stat` (idle + iowait are idle) |
| CPU clock | `cpu0/cpufreq/scaling_cur_freq`, then `cpuinfo_cur_freq`, then `cpu MHz` in `/proc/cpuinfo` |
| Memory load | `/proc/meminfo`: `MemAvailable`, else free + buffers + cached |
| GPU temperature | `/sys/class/drm/card*/device/hwmon/*/temp1_input`, preferring NVIDIA, then AMD, then anything; `nvidia-smi` fallback when the proprietary driver exposes no hwmon |

`live` prints the resolved sources before streaming:

```
Sensors: cpu=coretemp gpu=amd clock=cpufreq memory=meminfo
```

A missing sensor becomes `0` and the panel keeps updating. That is deliberate:
a machine with no discrete GPU should still show CPU, RAM, and clock rather than
go dark. If every source is missing you will see `cpu=missing` and zeros —
check the hwmon names above.

`nvidia-smi` is only consulted when no DRM hwmon node exists. A failed call is
cached for 30 s so a machine without the tool does not fork it every second.

## Troubleshooting

| Symptom | Cause and fix |
|---|---|
| `ERROR (access-denied ...)` | The udev rule is not installed or not applied. Install it, `udevadm control --reload`, `udevadm trigger`, add your user to `plugdev`, log out and back in, replug. |
| `ERROR (access-denied ... interface is already claimed ...)` | Another process owns interface 0. Stop any other driver or service using the AIO. |
| `ERROR (not-present ...)` | `lsusb \| grep -i 3633:0009` returns nothing. Check the AIO's internal USB cable, try another internal header, look at `dmesg \| tail`. |
| `ERROR (timeout ...)` / `ERROR (stall ...)` | Transient USB fault. Retry; if it repeats, replug the AIO or reboot (a port reset is often needed after suspend). systemd restarts the service automatically. |
| `ERROR (programming ... no libusb backend ...)` | Install `libusb-1.0-0` (or your distro's equivalent), then reinstall pyusb. |
| `ERROR: image/GIF support needs Pillow ...` | Install the extra: `pip install 'deepcool-mystique[image]'` (or `pip install Pillow`). |
| Upload finishes, nothing appears | Use this driver's encoder. Custom JPEGs must have a JFIF APP0 segment and standard (not optimized) Huffman tables; the panel silently rejects otherwise. |
| Kernel message `usb ... reset` then upload stops | A failed frame checksum can make the device drop off the bus. Do not patch the header builder without re-running the tests. |
| Status says `mode=ImageOrGif` after `init` | Advisory byte, not a failure (see above). If telemetry is not updating, see the limitation below. |
| Rotation did not stick | Every `init` re-sends rotation, and the unit starts with `live 0` = 0°. Put the rotation in the unit: `mystique --rotation=1 live 0`. |
| CPU temperature is 0 °C | Load `coretemp` / `k10temp` (`sudo modprobe coretemp`) or check `ls /sys/class/hwmon/*/name`. |
| GPU temperature is 0 °C | No DRM hwmon node and no working `nvidia-smi`. AMD/Intel should expose `temp1_input`; proprietary NVIDIA needs `nvidia-smi`. |

Device identity is never printed: the path shows bus/address only, and response
bytes 4–41 are masked. `--show-device-id` is the explicit opt-in, exactly as in
the Windows tool.

## What is not implemented

Honest list:

1. **Reliable return from Image mode to Machine Info.** The main gap. See above.
2. **Full semantics of the setup commands** `0x03`–`0x0B`, `0x15`–`0x17`, and
   the `0x0A` mode-switch bytes. They are sent as verified sequences; their
   individual meanings are unknown, so there is no API to send them ad hoc.
3. **The unexplained image-handshake heartbeat payload** (`0x01` on endpoint 1
   with `00 00 00 00 00 00 54 00 1E`) is reproduced verbatim, not decoded.
4. **Whether rotation applies to image/GIF playback.** `--rotation` is passed
   through, but only the public implementation's default of 0 is confirmed.
5. **Reply checksum / `HIDC` verification.** The references do not verify them,
   so neither does this driver. Whether the device always fills them in is an
   open question.
6. **In-process reconnect/backoff.** systemd restarts the process on a hard
   fault; pipe-level faults are recovered with `clear_halt`. There is no
   sensor-bridge-style supervisor, no rotation persistence beyond the command
   line, and no tray.
7. **Other LCD families.** Whether `C122`, `L086`, `L122`, `L136`, `L142` share
   this protocol is unknown; this driver matches `3633:0009` only.
8. **Panel native resolution.** 480×640 is the confirmed upload size, not a
   claim about the panel's internal resolution.
9. **Firmware.** Permanently out of scope. No firmware code, no images, no
   writes to `resources/fw/*`.

## Credits

The telemetry protocol comes from the C# reference in this repository, itself
verified against the live device.

The image/GIF upload protocol is documented and hardware-confirmed by
[`dinoyoshi/serial-lcd-panel-client`](https://github.com/dinoyoshi/serial-lcd-panel-client)
(MIT), which in turn credits
[`mymymy1303/qt-deepcool`](https://github.com/mymymy1303/qt-deepcool) (MIT) for
the original reverse engineering. The Python implementation here is independent
code written from those published facts; no TypeScript or C++ was copied.

## Development

Pure protocol and image-plan tests need no hardware. Encoding tests need Pillow;
without it they skip and the rest still pass:

```bash
py -m pytest linux/tests -q          # from the repository root, any OS
python3 -m pytest linux/tests -q     # Linux
```

Layout:

```
mystique/protocol.py      pure command packets, status, telemetry payload
mystique/image_upload.py  pure DCLd headers + upload plan, Pillow encoding
mystique/device.py        pyusb discovery, claim, transfers, fault taxonomy
mystique/telemetry.py     sysfs/procfs sensor sources
mystique/cli.py           the eight verbs
packaging/udev/           device permissions
packaging/systemd/        boot service
tests/                    hardware-free tests
```

`DESIGN.md` records the ownership model, the threading model, recovery
behaviour, the exact upload sequence, and the implemented-vs-unknown command
list.
