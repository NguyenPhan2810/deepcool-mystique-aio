# 04 — Linux landscape for the DeepCool MYSTIQUE LCD (`3633:0009`)

Scope: research and architecture only. No device access, no hardware touched, nothing outside
`re/` modified. Every external claim is cited by URL; anything I inferred rather than read is
marked **Inference**. Where a source is stale, ambiguous, or contradicted by another source, I
say so instead of picking the more convenient one.

---

## Verdict

**Linux support already exists — three times over, at hobbyist level — and none of it is
packaged, maintained by more than one person, or reusable as-is.**

Confirmed prior art that genuinely targets `3633:0009` and does real work:

| Project | Language | Telemetry | Images/GIF | Maintained |
|---|---|---|---|---|
| [`mymymy1303/qt-deepcool`](https://github.com/mymymy1303/qt-deepcool) | C++/Qt6 + libusb | yes | **no** | last commit 2026-02-12 |
| [`dinoyoshi/serial-lcd-panel-client`](https://github.com/dinoyoshi/serial-lcd-panel-client) | TypeScript + node-usb | yes | **yes, hardware-confirmed** | single 2-day burst, 2026-08-08→09 |
| [`gpteixeira/mystique-control`](https://github.com/gpteixeira/mystique-control) | unknown (AppImage only) | yes | **claimed** | 1 commit, 2026-10-23 |
| [`ngeran/deepcool-mystique`](https://github.com/ngeran/deepcool-mystique) | Python + pyusb | yes | no | single day, 2026-07-26 |

What is missing, concretely:

1. **Nothing is packaged.** AUR has four DeepCool packages and none is for the MYSTIQUE
   ([AUR search](https://aur.archlinux.org/rpc/?v=5&type=search&arg=deepcool)). No distro package,
   no nixpkgs entry, no `.deb`/`.rpm`.
2. **No kernel driver exists and none is wanted.** LKDDb has no driver for `3633:0009` in any
   kernel up to 7.1, and all 12 recorded probes are marked *failed*
   ([linux-hardware.org](https://linux-hardware.org/?id=usb:3633-0009)).
3. **The only implementation with published source that does image/GIF upload is a 0-star,
   two-day-old TypeScript side project.** The one with a polished GUI ships a binary blob.
4. **Nothing implements the vendor app's full surface** — brightness, screen on/off, the three
   vendor modes, RGB sync, gyroscope toggle. `gpteixeira` claims all of it; `dinoyoshi` proves the
   hard part (upload) but explicitly cannot get *back* to machine-info mode afterwards.

What we must build: **one single-owner userspace daemon** that (a) re-implements the telemetry
path we already verified on Windows, (b) ports the image/GIF sequence that `dinoyoshi` documented
against real hardware, and (c) owns the device exclusively so nothing can desynchronise it.

---

## 0. Corrections to `re/CONTEXT.md` — read this before writing code

These matter because the parent effort may currently be gating on a check that is wrong.

### 0.1 The `0x10` mode byte is **not** a reliable "did init succeed" gate

`CONTEXT.md` states: *"After the sequence, `0x10` must report mode `0xFF` or the device did not
enter machine-info mode."*

`dinoyoshi` captured a real `0x10` response from a physical Mystique 240 **while the LCD was
demonstrably showing live machine-info stats**, and byte 5 was `0x00`, not `0xFF`:

```
55 2e 00 00 00 00 00 00 ... 00 48 49 44 43 9b 01
```

— [PROTOCOL_DEEPCOOL_MYSTIQUE.md, "Status response"](https://raw.githubusercontent.com/dinoyoshi/serial-lcd-panel-client/main/docs/PROTOCOL_DEEPCOOL_MYSTIQUE.md)

Their conclusion: either byte 5 is not the mode flag, or it means something different on that
firmware, or the response layout differs. Unresolved. They therefore **do not gate on it** —
success is "init sequence completed with no transfer error".

**Implication for us:** a Linux port must not fail init on `byte5 != 0xFF`. Treat the flag as
advisory and log it. Our Windows implementation's use of it is a latent bug that happens to
work on the unit it was developed against. *(This is a contradiction between two sources, not a
disproof of our Windows code — I have not tested our unit's `0x10` on Linux.)*

### 0.2 Panel resolution is answered (vendor-stated)

DeepCool's own product page says the panel is a 2.8-inch TFT with *"very sharp **640x480** pixel
resolution"*, 16.7M colors, and *"built-in gyroscopic sensors"*
([deepcool.com](https://www.deepcool.com/products/Cooling/cpuliquidcoolers/MYSTIQUE-360-5th-Gen-Liquid-Cooler-with-LCD-1851-1700-AM5/2024/18059.shtml)).

`gpteixeira` independently states uploads are converted to **480×640** JPEG
([README](https://github.com/gpteixeira/mystique-control)) — the same panel in portrait
orientation. So: **640×480 landscape, 480×640 portrait, JPEG payload.**

### 0.3 Three "unknown" commands now have meanings

From `dinoyoshi`'s usbmon captures of genuine DeepCreative traffic (hardware-confirmed):

| Cmd | Meaning | Evidence |
|---|---|---|
| `0x0F` | begin image upload (empty payload) | confirmed |
| `0x11` | serial request, image-mode handshake only | confirmed (exchange), response unparsed |
| `0x14` | **erase stored images** — required before a 2nd upload | confirmed |

And one presumed meaning is **disproven**: `0x15`–`0x17` are *not* "display labels" — three
distinct 3-byte ASCII payloads produced no on-screen change
([same doc, "Display labels"](https://raw.githubusercontent.com/dinoyoshi/serial-lcd-panel-client/main/docs/PROTOCOL_DEEPCOOL_MYSTIQUE.md)).
They stay in the init sequence because the sequence works; their purpose is unknown.

### 0.4 `CONTEXT.md`'s "image/GIF upload is completely undocumented" is now out of date

It is documented, in public, with a byte-level header layout. See §7.

---

## 1. Prior art — honest coverage assessment

### 1.1 `mymymy1303/qt-deepcool` — the reference implementation everyone ports

- <https://github.com/mymymy1303/qt-deepcool> · MIT · 25★ / 6 forks · 56 commits · last commit
  **2026-02-12** (`5446ecb`, "Add rotation options")
  ([API](https://api.github.com/repos/mymymy1303/qt-deepcool/commits?per_page=5))
- **Covers:** `3633:0009` only, explicitly (`Supported Devices: DeepCool MYSTIQUE 360 (VID:
  0x3633, PID: 0x0009)` — [README](https://raw.githubusercontent.com/mymymy1303/qt-deepcool/main/README.md)).
- **Implements:** telemetry (`0x01`), rotation (`0x02`), status (`0x10`), device info (`0x12`),
  the 13-command machine-info handshake. CPU/GPU/RAM/clock only.
- **Does not implement images or GIFs.** Verified by inspection, not by trusting the README: the
  CLI source contains **zero** occurrences of `image`, `gif`, `jpeg`, `upload`, `0x0f`, `0x14`,
  or `DCLd` ([main_cli.cpp](https://raw.githubusercontent.com/mymymy1303/qt-deepcool/main/main_cli.cpp)).
  Its own protocol doc defers images to the vendor app: *"Images/GIFs uploaded via Windows
  software persist in device memory"*
  ([PROTOCOL.md](https://raw.githubusercontent.com/mymymy1303/qt-deepcool/main/PROTOCOL.md)).
- **Their protocol doc agrees with our wire format byte-for-byte** (48 bytes, `AA 2E`, payload at
  3..41, `HIDC` at 42..45, LE16 sum over 0..45; responses `55 2E`). Independent confirmation of
  `CONTEXT.md`.
- **Their udev rule is wrong for this device** and is the specific trap worth flagging: it leads
  with `SUBSYSTEM=="hidraw", ATTRS{idVendor}=="3633", MODE="0666"`. The Mystique is USB class
  `ff-00-00` ([linux-hardware.org](https://linux-hardware.org/?id=usb:3633-0009)), exposes no HID
  interface, and therefore **has no `/dev/hidraw*` node** — that line can never match. It only
  works because a *second* line, `SUBSYSTEM=="usb", ATTR{idVendor}=="3633", MODE="0666"`, happens
  to be present for PID 0009
  ([99-deepcool.rules](https://raw.githubusercontent.com/mymymy1303/qt-deepcool/main/99-deepcool.rules)).
  Everything is `MODE="0666"`.
- **Maintenance:** one author, one PR merged, no commits in ~9 months. Treat as a *specification*,
  not a dependency.

### 1.2 `dinoyoshi/serial-lcd-panel-client` — the only source-available image/GIF proof

- <https://github.com/dinoyoshi/serial-lcd-panel-client> · MIT · TypeScript · **0★** · created
  **2026-08-08**, last push **2026-08-09** ([API](https://api.github.com/repos/dinoyoshi/serial-lcd-panel-client))
- **Covers:** `3633:0009`, verified against a **Mystique 240**.
- **Implements:** machine-info dashboard **and** image/GIF upload that persists on the device.
  Upload protocol reverse-engineered from **usbmon captures of genuine DeepCreative traffic**
  (Windows 11 VM, USB passthrough, capture on the Linux host) — exactly the methodology we would
  use.
- **Caveats it states itself:**
  - only a Mystique **240** was tested; 360 untested;
  - **cannot return to machine-info mode after an upload** — confirmed bug, unresolved, with the
    leading untested hypothesis being that DeepCreative closes and reopens the USB handle;
  - the `0x10` mode byte is known-wrong on their unit (§0.1);
  - `0x15`–`0x17` do not do what their name says.
- **Status is honest and unusually well-evidenced.** This is the single most valuable document in
  the entire landscape. It is also a 0-star, 48-hour project — *nobody has independently
  reproduced its image path*. That is the confidence ceiling for image support on Linux.

### 1.3 `gpteixeira/mystique-control` — full parity claimed, source withheld

- <https://github.com/gpteixeira/mystique-control> · repo is **README + icon + screenshots only**;
  **1 commit**, `db16073`, **2026-10-23**; 1★
  ([API](https://api.github.com/repos/gpteixeira/mystique-control/commits?per_page=5))
- Ships `Mystique_Control-0.6.0-x86_64.AppImage`, 63.6 MB, 33 downloads
  ([release](https://github.com/gpteixeira/mystique-control/releases/tag/v0.6.0)).
- **Claims** the widest coverage of anything: brightness, screen on/off, rotation *and* gyroscope,
  idle screen, RGB sync, the three vendor modes (situational / motion / recorder), picture + GIF
  upload (480×640 JPEG per frame, up to 9 of each), crop UI, tray icon, CLI, "tested on Ubuntu
  24.04, Debian 13, Fedora 42, Arch and openSUSE Tumbleweed".
- It also independently documents the operational truth we care about: *"Only one program can use
  the cooler at a time."*
- **Assessment:** strong evidence that *full* parity is achievable on Linux, and that our whole
  gap list is closable. **Zero** evidence about *how*. Do not cite it as a protocol source. I did
  not run the AppImage (would require the device, out of scope here).

### 1.4 `ngeran/deepcool-mystique` — a personal Python port

- <https://github.com/ngeran/deepcool-mystique> · MIT · created and last pushed **2026-07-26**,
  0★ ([API](https://api.github.com/repos/ngeran/deepcool-mystique))
- Python + pyusb, `3633:0009` only, telemetry only ("PROTOCOL.md is the authoritative packet
  reference" — it points at the qt-deepcool doc). Adds a Quickshell sink mode and a NixOS module.
- **Useful for:** it is the closest thing to an existence proof for **pyusb** on this exact
  device, and its README states plainly that the daemon "needs root (claiming/detaching interface
  0)". It is a single-person, single-day repo — no maintenance signal at all.

### 1.5 Projects that look relevant and are not

| Project | Why it is not coverage |
|---|---|
| `Nortank12/deepcool-digital-linux` (439★, GPL-3, Rust) | HID-class digital-display coolers only. Its own README says of the MYSTIQUE: *"These devices are unique since they have an LCD display, and I do not personally own one"* and defers to [discussion #18](https://github.com/Nortank12/deepcool-digital-linux/discussions/18). That discussion contains a user posting `Bus 001 Device 008: ID 3633:0009 DeepCool MYSTIQUE` and nothing else. Its rules file only matches `SUBSYSTEM=="hidraw"` — no-op here. |
| `liquidctl` | **No DeepCool driver at all.** Verified from the driver registry: `liquidctl/driver/__init__.py` imports aquacomputer, asetek, asetek_pro, asus_ryujin/ryuo, aura_led, commander_core/pro, control_hub, coolit, corsair_hid_psu, hydro_platinum, kraken2/3, lianli_uni, msi, nzxt_epsu, rgb_fusion2, smart_device, ga2_lcd — **no `deepcool` module** ([source](https://raw.githubusercontent.com/liquidctl/liquidctl/main/liquidctl/driver/__init__.py)). It *does* prove the architecture (see §2). |
| `OpenRGB` | No DeepCool controller in the `Controllers/` tree (API listing, 35+ dirs returned before truncation, no `Deep*.` entry — [listing](https://api.github.com/repos/CalcProgrammer1/OpenRGB/contents/Controllers)). OpenRGB is an RGB-lighting project; an LCD framebuffer is out of its scope regardless. |
| `coolero` → `CoolerControl` | LCD/USB device support in CoolerControl is provided *through liquidctl* ([docs](https://docs.coolercontrol.org/getting-started)). No liquidctl driver → no CoolerControl coverage. **Inference**, but a direct one. |
| `zetlaska/deepcool-lm360-lcd`, `daedlock/deepcool-lm` | **Different PID.** LM360 is `3633:0026`, 320×240 ([README](https://raw.githubusercontent.com/zetlaska/deepcool-lm360-lcd/main/README.md)). Adjacent, not applicable — but see §2.5: they are the best architectural precedent we have. |
| Kernel / `usb-serial` / `hwmon` | None. LKDDb: no driver up to kernel 7.1; 12/12 probes "failed" ([linux-hardware.org](https://linux-hardware.org/?id=usb:3633-0009)). |
| AUR | `deepcool-ak620-digital-linux-git`, `deepcool-digital-linux-git`, `deepcool-lm`, `deepcool-lm-arkui`. None for MYSTIQUE ([AUR API](https://aur.archlinux.org/rpc/?v=5&type=search&arg=deepcool)). |

---

## 2. Architecture

### 2.1 Recommendation: **libusb 1.0, userspace, one owning daemon; Python + pyusb for v1**

Reasoning, in the order that actually decides it:

1. **There is nothing for a kernel driver to be.** A kernel driver earns its place by (a) serving
   a standard class, (b) exposing sensors to `hwmon`, or (c) being needed by unprivileged
   processes that cannot hold a device open. None applies. The telemetry flows **host → device**:
   we push CPU/GPU/RAM into the panel. Even fan/pump RPM comes from the motherboard's Super I/O,
   not from the cooler ([gpteixeira README](https://github.com/gpteixeira/mystique-control)). A
   `hwmon` driver would have nothing to expose.
2. **The protocol is still being discovered.** `0x15`–`0x17` are unexplained, `0x0A` has at least
   three payloads, the `0x10` flag is wrong on some firmware, and the return-to-machine-info path
   is unsolved. Every one of those needs fast edit-run-observe cycles and a packet dump. A kernel
   module converts a 5-second iteration into a rebuild-and-reboot.
3. **Upstream would (correctly) refuse it.** A vendor-class device with no public specification,
   one model, and an application-level framing that carries JPEG frames over bulk is what
   userspace is for. The right long-term home is `liquidctl`-style userspace, not `drivers/usb/`.
4. **`usbfs` semantics already give us what a driver would provide:** exclusive claim, permission
   control via udev, and per-device PM interaction (§6, R5).
5. **libusb is what every working implementation already uses** — qt-deepcool
   (`libusb_claim_interface(0)`), ngeran (pyusb), dinoyoshi (node-usb, a libusb binding). Choosing
   anything else means re-solving problems three projects already solved.

**Python + pyusb vs a compiled tool.** Choose Python.

- The steady-state loop is 1 Hz with ~10 ms inter-command sleeps and a 1500 ms transfer timeout.
  There is no throughput requirement and no latency requirement. The only CPU-heavy step is JPEG
  encoding, which happens once per uploaded frame.
- pyusb is a `ctypes` wrapper over libusb 1.0 ([pyusb docs](https://pyusb.github.io/pyusb/)) — same
  kernel path, same `usbfs` semantics, no behavioural difference that matters here.
- Python makes the *undocumented* parts cheap to explore. That is where the remaining work is.
- The cost is honest: pyusb has **no hotplug callback**, so re-plug detection is a poll or an
  exception-driven retry loop, not an event. Budget for that (it is ~20 lines).
- Precedent: `zetlaska/deepcool-lm360-lcd` is exactly this shape — Python 3.10+, PyUSB 1.2+,
  Pillow, systemd service, Unix-socket IPC for CLI-vs-daemon contention — for the same vendor's
  LCD class at a different PID
  ([README](https://raw.githubusercontent.com/zetlaska/deepcool-lm360-lcd/main/README.md)).

If a single static binary later becomes a requirement, the daemon boundary (§2.5) means only the
USB layer changes; the protocol layer is pure functions on `bytes` and ports trivially.

### 2.2 Rejected: `hidraw`

**It does not apply, and this is not a judgment call.** The device is USB class `ff-00-00`
(vendor-specific), per 12 independent Linux probes
([linux-hardware.org](https://linux-hardware.org/?id=usb:3633-0009)). `hidraw` nodes are created
only for interfaces bound by `usbhid`, i.e. class 3 (HID). There is no HID interface, therefore no
`/dev/hidraw*` node exists, therefore any `SUBSYSTEM=="hidraw"` rule is dead text.

This matters beyond pedantry: **two of the three published udev rules for this device contain
`hidraw` lines that can never fire** (§1.1, and Nortank12's). Anyone copying them inherits a rule
that looks like it works and does nothing. Diagnose with `ls -l /dev/hidraw*` and
`lsusb -t` — if the interface shows `Driver=[none]`, there is no hidraw and no driver to detach.

### 2.3 Rejected: raw `usbfs` ioctls / `LIBUSB_OPTION_NO_DEVICE_DISCOVERY`

- **Raw `USBDEVFS_*` ioctls:** libusb *is* a usbfs client. Re-implementing `USBDEVFS_CLAIMINTERFACE`
  / `USBDEVFS_BULK` / `USBDEVFS_REAPURB` buys nothing and costs the hotplug/portability work that
  libusb already did.
- **`LIBUSB_OPTION_NO_DEVICE_DISCOVERY`:** exists to skip enumeration when the process *cannot*
  enumerate, and is documented as the companion to `libusb_wrap_sys_device()` on an
  already-open file descriptor — *"In particular, this might be needed on Android if you don't
  have authority to access USB devices in general. Setting this option with `libusb_set_option`
  is deprecated."* ([libusb docs](https://libusb.sourceforge.io/api-1.0/group__libusb__dev.html)).
  On a normal Linux desktop it is strictly counterproductive: it removes
  `libusb_get_device_list()`, which is precisely how we find a device by VID/PID. **Do not use it.**
  The one legitimate use would be a sandboxed/portal-based deployment where an fd is handed to us —
  not our case.

### 2.4 Rejected: a kernel driver (upstream or out-of-tree)

- **Upstream:** see §2.1(1)–(3). No class spec, one device, host→device telemetry, active protocol
  discovery. Also: an upstream driver cannot iterate on `0x14`/`0x0F` semantics without kernel
  review cycles.
- **Out-of-tree (`usb-serial` or custom):** adds DKMS/rebuild burden on every kernel upgrade, and
  *takes the interface away* from the userspace tools we need for RE. It also creates a new
  failure mode: an out-of-tree module that binds the interface means libusb must
  `libusb_detach_kernel_driver()` before every experiment.
- **`usb-serial` specifically:** the generic driver will only bind a device that matches an
  explicit `vendor=`/`product=` module parameter — `usb_serial_generic_register()` builds its
  `id_table` from `vendor`/`product` and sets
  `USB_DEVICE_ID_MATCH_VENDOR | USB_DEVICE_ID_MATCH_PRODUCT`
  ([generic.c](https://raw.githubusercontent.com/torvalds/linux/master/drivers/usb/serial/generic.c)).
  So it will **not** auto-hijack the Mystique. But running
  `modprobe usbserial vendor=0x3633 product=0x0009` would hand a tty line discipline a 48-byte
  binary frame protocol — a guaranteed corruption. **Never do that**, and note it as a support
  hazard in the README.

### 2.5 The shape to build: one owner, everyone else talks to it

```
        ┌──────────────┐   unix socket    ┌────────────────────────┐
  CLI ──┤              │◄────────────────►│  mystique.service      │
  GUI ──┤  clients     │                  │  (system, one process) │
  prom ─┤              │                  │  owns the USB interface│
        └──────────────┘                  └───────────┬────────────┘
                                                      │ libusb bulk 0x01/0x81, 0x02/0x82
                                                      ▼
                                              ┌───────────────┐
                                              │   MYSTIQUE    │
                                              └───────────────┘
```

This is not invented for us — it is what `gpteixeira` ships ("the background service is the only
program that talks to the cooler", CLI/GUI/tray talk over a Unix socket) and what both LM-series
projects independently arrived at ("IPC Communication: Unix socket for conflict-free CLI usage").
The reason is §6, R3: the device is exclusive-access, so *someone* must be the owner, and a
socket is the cheapest way to make that true without root for every client.

Daemon responsibilities, in dependency order:

1. **Own the fd.** `libusb_open` → `claim_interface(0)` → hold. Releasing releases exclusivity.
2. **Serialise everything.** One worker thread; no concurrent transfers, ever.
3. **Re-init on every (re)enumeration and on every transfer error.** Machine-info mode is not
   durable across power cycles.
4. **Expose intent, not bytes.** `set rotation 90`, `upload /tmp/a.gif`, `set mode situational`.
   Clients must never be able to compose a packet.
5. **Persist state** (`~/.config/…` or `/var/lib/…`): rotation, last mode, intended image. After a
   replug the daemon must be able to restore without a client connected.

### 2.6 Concrete libusb call sequence (matches our verified Windows behaviour)

```
libusb_init
find by idVendor=0x3633 idProduct=0x0009        # no VID/PID ambiguity in this vendor's line
if libusb_kernel_driver_active(h, 0)            # expected 0 on a clean system
    libusb_detach_kernel_driver(h, 0)
libusb_claim_interface(h, 0)                    # interface 0 — see §3.2
# init: 13 commands on EP 0x01, ~10 ms apart, 0x0A last with ~100 ms settle
# each: bulk OUT 0x01 (48 bytes, timeout 1500 ms), then bulk IN 0x81 (read up to 64)
# steady state: 0x10 on 0x02/0x82, then 0x01 on 0x02/0x82, 1 Hz
# image: see §7
libusb_release_interface(h, 0); libusb_close(h)
```

`libusb_detach_kernel_driver` is what `qt-deepcool` does and what `ngeran` describes as
"claiming/detaching interface 0". On a stock kernel it will be a no-op, because nothing binds
`ff-00-00`. Keep the call anyway — it costs nothing and it saves you on a machine where someone
loaded `usbserial`.

---

## 3. udev and permissions

### 3.1 The rule

```udev
# /etc/udev/rules.d/99-mystique.rules
#
# DeepCool MYSTIQUE AIO LCD - vendor-specific USB function (class ff), bulk endpoints
# 0x01/0x81 (init) and 0x02/0x82 (status + display). NOT HID: there is no hidraw node.
#
# MODE/GROUP serve the system daemon; uaccess additionally gives the *locally logged-in*
# user an ACL so interactive tools (capture, one-shot CLI) work without sudo.
# Do not widen to 0666: see re/findings/04-linux-landscape.md section 3.2.
#
# One rule, one line. udev documents no line-continuation syntax, so do not wrap it.

ACTION=="add|change", SUBSYSTEM=="usb", ATTR{idVendor}=="3633", ATTR{idProduct}=="0009", MODE="0660", GROUP="mystique", TAG+="uaccess", TAG+="systemd", SYMLINK+="mystique", ENV{SYSTEMD_WANTS}+="mystique.service", ATTR{power/control}="on"
```

Install:

```bash
sudo groupadd -r mystique            # the service user is created separately, see §4.2
sudo install -m 0644 99-mystique.rules /etc/udev/rules.d/99-mystique.rules
sudo udevadm control --reload-rules
sudo udevadm trigger --subsystem-match=usb --action=change

# Confirm the rule actually fired, on the real device node (read bus/device with lsusb):
sudo udevadm test "$(udevadm info -q path -n /dev/bus/usb/001/008)" 2>&1 | grep -E 'MODE|GROUP|uaccess|systemd'
ls -l /dev/mystique && getfacl /dev/mystique
```

Why each key:

- `SUBSYSTEM=="usb"` — the devnode is `/dev/bus/usb/BBB/DDD`, created for USB devices. `ATTR{}`
  (not `ATTRS{}`) reads the device's *own* attribute; `ATTRS{}` walks up the parent chain and is
  the right choice for child subsystems like `hidraw`, which this device does not have.
- `ACTION=="add|change"` — `change` re-applies after re-enumeration, so permissions do not drift
  after a replug or resume.
- `SYMLINK+="mystique"` + `TAG+="systemd"` — gives a stable `/dev/mystique` **and** a
  `dev-mystique.device` unit to order the service against. This is the documented liquidctl
  pattern: *"just with `TAG+="systemd"` alone a device unit will be made available as
  `dev-bus-usb-<bus>-<device>.device` … Setting a custom name with `SYMLINK` is optional"*
  ([liquidctl doc](https://raw.githubusercontent.com/liquidctl/liquidctl/main/docs/linux/making-systemd-units-wait-for-devices.md)).
- `ATTR{power/control}="on"` — best-effort belt-and-braces against autosuspend (§6, R5). udev may
  fail to write it during early boot; the daemon must not rely on it.

### 3.2 Why `MODE="0660"` + `TAG+="uaccess"` and not `MODE="0666"`

Every published rule for this device uses `MODE="0666"`, and it is wrong here for three concrete
reasons:

1. **The device is exclusive-access.** Only one process can claim the interface; a second gets
   `-EBUSY` (§6, R3). With `0666`, *any* local user's process can win that race — either denying
   the daemon (a trivial local DoS on the cooler's display) or, worse, winning it and writing
   half-understood commands while the daemon retries. A permission bit is the cheapest possible
   defence against the single worst failure mode.
2. **`uaccess` is narrower and self-revoking.** The udev `uaccess` builtin applies an ACL only to
   the **active session on the device's seat** — the source calls `sd_seat_get_active()` and, when
   there is no active session, does nothing
   ([udev-builtin-uaccess.c](https://raw.githubusercontent.com/systemd/systemd/main/src/udev/udev-builtin-uaccess.c)).
   The ACL dies with the session. `0666` grants every local account forever, including service
   accounts and sandboxed processes.
3. **`uaccess` cannot serve a system daemon, and 0660 cannot serve a human.** The daemon runs
   without a session, so it gets nothing from `uaccess` and needs `GROUP="mystique"`; the human
   running `tshark`/a one-shot CLI is not in that group and needs `uaccess`. **Use both.** They are
   complementary, not alternatives: mode bits and POSIX ACLs are evaluated together, and the ACL
   only ever adds.

Also worth knowing, because it will bite during bring-up: **`uaccess` does not apply to SSH.** It
is keyed to a logind seat, so a remote session gets no ACL — this is a long-standing, documented
complaint ([Fedora discussion](https://discussion.fedoraproject.org/t/uaccess-tagged-udev-rules-in-usermode-systemd-services-and-ssh-access-for-sdrs/149482);
the mechanism itself is still [undocumented upstream](https://github.com/systemd/systemd/issues/4288)).
Plan on `sudo` for remote debugging.

### 3.3 Avoiding conflict with distro-supplied rules

- **Check first, do not assume.** `grep -rIl 3633 /usr/lib/udev/rules.d /etc/udev/rules.d /run/udev/rules.d`
  and `udevadm info -q all -n /dev/bus/usb/BBB/DDD | grep -E 'MODE|GROUP|TAG'`.
- **Ordering is lexical across all directories.** *"All rules files are collectively sorted and
  processed in lexicographic order, regardless of the directories in which they live"*
  ([udev(7)](https://www.freedesktop.org/software/systemd/man/latest/udev.html)). So a later file's
  `MODE=`/`GROUP=` wins, and `TAG+=` accumulates. This has a trap: the community rules are named
  `99-deepcool.rules` and `99-deepcool-mystique.rules`, and `-` (0x2d) sorts **before** `.` (0x2e),
  so `99-deepcool-mystique.rules` would be applied **before** `99-deepcool.rules` and lose to its
  `0666`. Name yours `99-mystique.rules` — `d` < `m`, so it lands after both.
- **If a distro rule genuinely conflicts** (none does for `0x3633` today), do not edit the vendor
  file: replace it by name in `/etc`, or disable it with a `/dev/null` symlink — *"a symlink in
  `/etc/` with the same name as a rules file in `/usr/lib/`, pointing to `/dev/null`, disables the
  rules file entirely"* (same page).
- **Do not use a vendor-wide match.** `SUBSYSTEM=="usb", ATTR{idVendor}=="3633"` alone (as
  qt-deepcool and ngeran write it) claims every DeepCool device on the machine, including HID
  digital coolers that `Nortank12`'s daemon may own. Always pin `idProduct`.
- **`usbguard`**: if installed and in `strict` mode it will block the device outright and the
  symptom is "device missing", not "permission denied". One line in the troubleshooting section,
  nothing more.

---

## 4. The environment the user will actually have

Distro-agnostic requirements, with the one decision that actually constrains the choice.

### 4.1 Runtime dependencies

| Need | Package (Debian/Ubuntu · Arch · Fedora) | Why |
|---|---|---|
| libusb 1.0 | `libusb-1.0-0` · `libusb` · `libusb1` | pyusb's native backend |
| Python ≥3.10 | `python3` · `python` · `python3` | daemon |
| pyusb | `python3-usb` · `python-pyusb` · `python3-pyusb` | USB access ([pyusb](https://pyusb.github.io/pyusb/)) |
| Pillow | `python3-pil` · `python-pillow` · `python3-pillow` | JPEG encode for uploads |
| psutil | `python3-psutil` · `python-psutil` · `python3-psutil` | CPU/RAM/clock |
| sensors | `lm-sensors` · `lm_sensors` · `lm_sensors` | temps |
| tshark (optional) | `tshark` · `wireshark-cli` · `wireshark-cli` | capture/decode |

**Python vs compiled is not a hard constraint** — the deliverable is a 1 Hz control loop (§2.1).
A compiled static binary would remove the interpreter dependency and nothing else. Do not pay for
that until the protocol is finished.

### 4.2 systemd unit

```ini
# /etc/systemd/system/mystique.service
[Unit]
Description=DeepCool MYSTIQUE AIO LCD daemon
Documentation=file:/usr/local/share/doc/mystique/README.md
# Wait for the device when it is present at boot.
After=dev-mystique.device
Wants=dev-mystique.device
# NOT Requires=/BindsTo=: see the note below.

[Service]
Type=simple
ExecStart=/usr/local/bin/mystique-daemon --config /etc/mystique/config.toml
Restart=on-failure
RestartSec=2
# Device access is via GROUP=mystique, not root.
User=mystique
Group=mystique
# Tidy-up, in rough order of how much they cost you if you skip them.
NoNewPrivileges=yes
PrivateTmp=yes
ProtectSystem=strict
ProtectHome=yes
StateDirectory=mystique
# NOTE: no WatchdogSec=. It only works with Type=notify plus sd_notify(WATCHDOG=1)
# in the daemon; adding it to a plain Type=simple service kills the unit every
# interval. Add both together, or neither.

[Install]
WantedBy=multi-user.target
```

Create the account and directories before enabling:

```bash
sudo useradd -r -s /usr/sbin/nologin -d /var/lib/mystique mystique
sudo install -d -o mystique -g mystique -m 0750 /etc/mystique
sudo systemctl daemon-reload && sudo systemctl enable --now mystique.service
```

Hotplug: `ENV{SYSTEMD_WANTS}+="mystique.service"` in the udev rule (§3.1) starts the unit on every
`add`, which covers replug. This is the second half of liquidctl's documented pattern.

**The trap to avoid:** `Requires=dev-mystique.device` looks correct and is not. When the device
disappears, systemd *stops* the unit — and a stop caused by a vanished dependency is not a
failure, so `Restart=on-failure` will not bring it back. The panel stays dead until the next
`add` event or a manual start. Use `Wants=` + `After=` (order, no hard dependency) plus the udev
`SYSTEMD_WANTS` hook, and let the daemon own reconnection.

**Do not require the daemon to run as root.** It needs nothing but the device node. Root buys you
an unbounded blast radius on a process that parses untrusted GIFs.

### 4.3 Suspend/resume

Two different things, often conflated:

- **Runtime autosuspend (USB PM):** already prevented while the fd is open — `usbdev_open()` calls
  `usb_autoresume_device()` and `usbdev_release()` only calls `usb_autosuspend_device()` afterwards
  ([devio.c](https://raw.githubusercontent.com/torvalds/linux/master/drivers/usb/core/devio.c)).
  Holding the device is therefore itself the mitigation. `ATTR{power/control}="on"` is the
  belt-and-braces version for the window before the daemon starts.
- **System suspend:** the fd may survive, the device state may not. Assume machine-info mode is
  lost and re-run the handshake.

Preferred handling is **in the daemon**: on any transfer error, `-ENODEV`, or a failed status
poll, close, re-find, re-claim, re-init. That single code path covers unplug, replug, resume, and
a wedged device — which is why I recommend it over a sleep hook.

If you want an explicit hook anyway, systemd runs executables in `/usr/lib/systemd/system-sleep/`
with `pre`/`post` plus the action name, in parallel, and blocks suspend until they finish
([systemd-sleep(8)](https://www.freedesktop.org/software/systemd/man/latest/systemd-sleep.html)):

```bash
#!/bin/sh
# /usr/lib/systemd/system-sleep/50-mystique
case "$1" in
  post) systemctl try-restart mystique.service ;;
esac
```

Note the same man page calls such hooks *"hacks"* and points applications at inhibitor locks
instead. This is a fallback, not the design.

### 4.4 `usbmon` availability

`usbmon` is a **kernel build-time option**, `CONFIG_USB_MON`, tristate, defaulting to "say Y if
allowed, otherwise M" ([Kconfig](https://raw.githubusercontent.com/torvalds/linux/master/drivers/usb/mon/Kconfig)).
Generic distro kernels ship it as a module. **Verify, do not assume:**

```bash
grep CONFIG_USB_MON /boot/config-$(uname -r)     # =m or =y
ls /sys/kernel/debug/usb/usbmon/ 2>/dev/null || sudo modprobe usbmon
ls /dev/usbmon*                                  # binary API (what tshark uses)
```

Two independent APIs exist: the **binary** one via `/dev/usbmonN` (a stable ABI, what libpcap and
therefore tshark/wireshark use) and the **text** one via debugfs, *"deprecated since 2.6.35, but
available for convenience"* ([usbmon docs](https://www.kernel.org/doc/html/latest/usb/usbmon.html)).
Prefer binary.

Caveat worth recording: it cannot be added after the fact. The hooks live in `usbcore`, so there is
no out-of-tree `usbmon.ko` — reported for the OrbStack kernel
([orbstack#2800](https://github.com/orbstack/orbstack/issues/2800)). Irrelevant on bare metal with
a distro kernel; fatal inside WSL2 or a container, which is why capture must happen on the host
(§5, and exactly how `dinoyoshi` did it: capture on the Linux host, vendor app in a Windows VM with
USB passthrough).

### 4.5 Wireshark / tshark

Any packaged Wireshark ≥ 1.2 can read `usbmon` ([Wireshark wiki](https://wiki.wireshark.org/CaptureSetup/USB)).
The Linux packaging question is only about *permissions* on `/dev/usbmon*`, and distros differ:
Fedora/RHEL ship `SUBSYSTEM=="usbmon", GROUP="usbmon", MODE="640"`; others use a `wireshark`
group ([same page](https://wiki.wireshark.org/CaptureSetup/USB); also
[liquidctl's capture guide](https://raw.githubusercontent.com/liquidctl/liquidctl/main/docs/developer/capturing-usb-traffic.md)).
Check with `ls -l /dev/usbmon*` and add yourself to whichever group owns them.

No custom dissector is needed. Our framing is 48 bytes of opaque payload on a bulk endpoint; the
generic USB dissector plus `tshark -T fields` is enough, and the fields are the ones you want
anyway.

---

## 5. Capture procedure on Linux, after the switch

### 5.1 One-time setup

```bash
sudo modprobe usbmon
sudo grep -q '^CONFIG_USB_MON=[my]' /boot/config-$(uname -r) && echo "usbmon available"
ls -l /dev/usbmon*                      # note the owning group
sudo usermod -aG wireshark "$USER"      # or usbmon, whichever owns them; then re-login
```

### 5.2 Find the bus and device, then correlate endpoints

```bash
lsusb -d 3633:0009
# Bus 001 Device 008: ID 3633:0009 DeepCool MYSTIQUE
lsusb -v -d 3633:0009 2>/dev/null | grep -E 'bInterfaceClass|bInterfaceNumber|bNumEndpoints|Endpoint|bmAttributes|wMaxPacketSize'
lsusb -t                                        # shows which driver (if any) is bound
cat /sys/kernel/debug/usb/devices | grep -A8 'Vendor=3633'
```

Note the **bus** (`Bus 001` → `/dev/usbmon1` and `.../usbmon/1u`) and the **device address**
(`Device 008`). The address changes on every replug — re-read it before every session; a stale
address silently captures nothing.

Endpoint correlation, given our verified layout:

| Endpoint | Direction | usbmon text | Used for |
|---|---|---|---|
| `0x01` | OUT | `Bo:1:8:1` | init/config commands |
| `0x81` | IN | `Bi:1:8:1` | init replies |
| `0x02` | OUT | `Bo:1:8:2` | status + display frames + upload |
| `0x82` | IN | `Bi:1:8:2` | status replies |

(The address word is `type:bus:dev:endpoint`, *"Bus number, Device address, and Endpoint are
decimal numbers"* — [usbmon docs](https://www.kernel.org/doc/html/latest/usb/usbmon.html).)

### 5.3 Capture with tshark (binary API, filterable)

```bash
sudo tshark -i usbmon1 -w /tmp/mystique-$(date +%s).pcapng -q
# in another shell, drive the device:
sudo systemctl start mystique.service        # or run the vendor app in a Windows VM
# Ctrl-C the capture when the interesting action is done
```

### 5.4 Decode

```bash
# Which device is which on this bus
tshark -r cap.pcapng -Y 'usb.idVendor == 0x3633' \
       -T fields -e usb.bus_id -e usb.device_address -e usb.endpoint_address | sort -u

# The packet stream, one row per transfer
tshark -r cap.pcapng -Y 'usb.idVendor == 0x3633' \
       -T fields -e frame.time_relative -e usb.endpoint_address -e usb.data_len \
                   -e usb.capdata -e usb.data_fragment
```

Notes:

- `usb.capdata` ("Leftover Capture Data") and `usb.data_fragment` ("Data Fragment") both exist in
  Wireshark's USB dissector and both cover bulk payloads; which one is populated varies by version
  (both are listed in the [display filter reference](https://www.wireshark.org/docs/dfref/u/usb.html)).
  Emit both, take whichever is non-empty, and if a field errors out use
  `tshark -G fields | grep '^F.*usb\.'` to find the right name for your build.
- Wireshark stores the **submission** URB with its length but often no data, and the **completion**
  with the data. Reassemble by URB id — the first column of the text format is that id; for tshark
  use `tshark -G fields | grep -i urb` to get the exact field name for your build rather than
  guessing it.
- If you prefer no Wireshark at all:

```bash
sudo cat /sys/kernel/debug/usb/usbmon/1u > /tmp/mystique-1u.txt
grep 'Bo:1:8:2' /tmp/mystique-1u.txt | head     # display frames
grep 'Bi:1:8:2' /tmp/mystique-1u.txt | head     # status replies
```

The text format gives you `S`/`C` events, data length, and the data words as hex — enough to
diff a 48-byte frame by eye or with `awk`.

### 5.5 Practicalities

- Capture is passive; it does **not** take the device, so you can capture your own daemon's traffic
  while it runs. That is the fastest loop for the remaining unknowns.
- Capturing the **vendor** app is the highest-value target (it is where the image-path truth lives),
  and it must happen on the Linux host with the app in a Windows VM — `usbmon` sees traffic even
  when the device is passed through to a guest, because passthrough still transits the host USB
  stack. `dinoyoshi` documents exactly this.
- **Redaction.** Response frames carry per-device identifiers at packet bytes 4..11 and 12..41
  (`CONTEXT.md` §"Verified wire format"). Do not try to regex them out of a hex blob — strip the
  data column before sharing the capture at all, and if you must keep payloads, zero those byte
  ranges in every reassembled 48-byte response.
- Also strip image payloads: an upload capture is mostly JPEG bytes, and megabytes of it.

---

## 6. Risk register

Calibration first: **I found no command in the observed set that writes firmware.** The vendor
firmware file exists on the Windows side, but nothing in the documented protocol touches it, and
`CONTEXT.md` forbids firmware work permanently. So "brick" here realistically means **a panel stuck
on a placeholder, a logo, or a stale image until a physical power cycle** — plus a daemon that
believes it succeeded. Treat the risks as desynchronisation, not destruction. If evidence later
shows an erase/write command reaching flash, this whole assessment changes.

### R1 — Partial or short write leaves the device mid-command

**What goes wrong.** The 48-byte packet is a single logical unit: header, payload, `HIDC`, sum. A
short write (or a write split into two transfers) gives the firmware a frame it will never
complete. It then sits waiting, and the next frame we send is interpreted as the tail of the
previous one — silent desync that looks like "the display just stopped updating".
**Mitigation.**
- Build the whole 48-byte buffer in memory, sum bytes 0..45, append the LE16, then **one**
  `bulk_transfer` with the 1500 ms timeout we already use on Windows.
- Check the return value: `transferred == 48`. Anything else is fatal for the session, not
  advisory.
- On any short write: stop, release, re-open, re-init. Never retry the same frame.
- Never write to a device you have not just successfully claimed.

### R2 — Leaving the device mid-handshake

**What goes wrong.** The 13-command sequence is order-sensitive and not ascending (`12, 02, 03, 04,
07, 08, 05, 0B, 06, 15, 16, 17, 0A`) — `CONTEXT.md` explicitly warns not to tidy it. Abort it
halfway and the device is in an undefined mode; `0x0A` last is what actually commits the mode
switch. A process killed between command 5 and 13 leaves the panel in whatever partial state that
produced.
**Mitigation.**
- Make the handshake atomic from the daemon's perspective: a single function, no cancellation
  points, no signal handlers that can interrupt between packets (block `SIGTERM` during init, or
  set a flag and finish).
- `Restart=on-failure` + a startup path that **always** re-inits rather than assuming prior state.
- Do not gate success on the `0x10` byte (§0.1). Gate on "all 13 transfers returned 48/OK, then
  `0x10` answered at all".
- Log every command and its reply length. When it desyncs you will need the last 5 frames.

### R3 — Concurrent access

**What goes wrong.** Two processes on one vendor interface. The kernel arbitrates at *interface*
granularity: `usb_driver_claim_interface()` returns `-EBUSY` if `dev->driver` is already set
([driver.c](https://raw.githubusercontent.com/torvalds/linux/master/drivers/usb/core/driver.c)),
which libusb surfaces as `LIBUSB_ERROR_BUSY`. Since the Mystique exposes one vendor function,
claiming interface 0 is whole-device contention. The failure is not symmetric: the loser may fail
at `open` (clean) or at `claim` (clean) — but two processes that each open, claim, do one command,
and release can interleave *across* sessions with no error at all, which is the dangerous case.
**Mitigation.**
- One owner process, forever (§2.5). Everyone else over a Unix socket.
- Second gate: `flock(LOCK_EX|LOCK_NB)` on `/run/mystique.lock` at daemon start. `claim_interface`
  does not stop a client from doing open→single-command→close in the gaps.
- Treat `LIBUSB_ERROR_BUSY` as "another owner exists", not "retry": print who, and exit non-zero.
  A retry loop here produces exactly the interleaving above.
- In the README: *"stop the vendor app in any VM, and any other driver, before starting this."*
  `gpteixeira` documents the same requirement.

### R4 — Suspend/resume loses machine-info mode

**What goes wrong.** The panel keeps its mode in volatile state. Across a system suspend the USB
device may be reset or re-enumerated; the fd goes stale (`-ENODEV`) or survives but the panel is
back on its logo. A daemon that only writes `0x01` frames will keep writing into a device that is
no longer listening, forever, with no error — the "display frozen at boot logo" bug.
**Mitigation.**
- One reconnect path, used for every cause: transfer error → `-ENODEV` → close → re-find (VID/PID,
  not cached path) → claim → full 13-command init → restore rotation and intended mode from
  persisted state → resume the 1 Hz loop.
- Poll `0x10` at low rate (e.g. every 10th tick) as a liveness probe; a silent device answers
  nothing.
- A real watchdog (`Type=notify` + `sd_notify(WATCHDOG=1)`) as the last resort if the daemon itself
  wedges — but only wire both halves together, or systemd will kill a healthy unit.
- Optional sleep hook (§4.3), for people who never unplug and always suspend.

### R5 — USB autosuspend

**What goes wrong.** If something autosuspends the device (or the port) between our sessions, the
first transfer after resume can time out (1500 ms) or return a stall; a naive driver retries,
double-writes, or gives up and leaves the panel blank.
**Mitigation.**
- Holding the fd already prevents runtime autosuspend: `usbdev_open()` → `usb_autoresume_device()`,
  and `usbdev_release()` only re-enables autosuspend afterwards
  ([devio.c](https://raw.githubusercontent.com/torvalds/linux/master/drivers/usb/core/devio.c)).
  So a long-lived owner is the fix, not a workaround.
- Belt-and-braces: `ATTR{power/control}="on"` in udev (§3.1) and, if you want it explicit, write
  `on` to `/sys/bus/usb/devices/<id>/power/control` at daemon start. Per the kernel docs, `on`
  means *"the device should be resumed and autosuspend is not allowed"*
  ([power management](https://www.kernel.org/doc/html/latest/driver-api/usb/power-management.html)).
- First transfer after any gap: allow one retry, then take the R4 reconnect path. Never retry a
  *write* blindly — a timed-out bulk write may have landed.

### R6 — Silent no-op uploads (add this one; it is the highest-value finding)

**What goes wrong.** The image path has four distinct ways to "succeed" with no visible effect:

1. **Splitting the 64-byte `DCLd` header from the JPEG data** into two transfers. The header alone
   is exactly one `wMaxPacketSize`, and `dinoyoshi` reports that libusb then inserts a zero-length
   packet the genuine capture never has — *"didn't error, but silently no-opped the whole upload
   (device processed it with no error, screen never updated)"*. Their fix was to send header+JPEG
   as **one contiguous buffer**. I am reporting their mechanism as theirs; the actionable rule is
   the single-buffer conclusion.
2. **Not sending `0x14` (erase stored images) before a second upload.** Two consecutive genuine
   vendor uploads were captured, both delivering real frame data, both leaving the previous
   content on screen; the fix was this one command. *"Empirically confirmed: three consecutive
   uploads … each showing correctly … with no manual steps or replug in between."*
3. **Running the mode-switch handshake only once** (before the frames). It must run **again** after
   `dcldfinish` + `0x08 [01 01]`, or the upload has zero visible effect.
4. **JPEG encoder mismatch.** Genuine frames carry a JFIF APP0 marker and use standard Huffman
   tables; `sharp`'s defaults emit neither.

**Mitigation.** Treat the upload as a state machine with an explicit `verify` step — read back or
re-capture one frame and compare — and never report success on "transfers returned OK". For the
encoder, start from Pillow (`optimize=False`, JFIF on) and **byte-diff the first output against a
captured genuine frame** before trusting it. *(Pillow's JFIF/default-table behaviour is my
expectation, not a documented guarantee I could cite — verify it, do not assume it.)*

### R7 — Cannot get back to machine-info mode after an upload

**What goes wrong.** Confirmed on real hardware by `dinoyoshi`: after `uploadAnimation()` the
device stays in image mode, and `initMachineInfoMode()` afterwards — even on a brand-new
connection — does not bring the dashboard back. It shows the uploaded image, or the "please upload
an image" placeholder if `0x14` was sent, with no protocol-level error either way. Four hypotheses
were tested and none reproduced reliably; the leading untested one is that the vendor app closes
and **reopens the USB handle** during the transition.
**Mitigation.**
- Do not treat "upload" and "dashboard" as freely interleaved operations. Make it an explicit mode
  with persisted intent, and require an explicit transition.
- Implement the transition as: full teardown → `libusb_close` → re-open → re-init. That is the one
  variable the failed reproductions did not control for, and it is nearly free to try.
- If it still fails, be honest in the UI: "image mode — replug the cooler to return to the
  dashboard". A wrong-but-silent state is worse than a documented limitation.

### R8 — Checksum/endianness mistakes are silent

A wrong sum does not produce an error — the device ignores the frame. Because the sum is over bytes
0..45 and includes `HIDC`, an off-by-one in the payload offset shifts everything and still produces
a plausible number. **Mitigation:** unit-test the builder against the literal bytes in
`PROTOCOL.md` and against our Windows implementation, and assert `len == 48` and
`sum(packet[0:46]) & 0xFFFF == packet[46] | packet[47] << 8` on every outbound frame in debug
builds.

---

## 7. What the image/GIF path actually involves (for planning, not implementation)

Because this is the whole reason a Linux port is hard, here is the sequence `dinoyoshi` documented
and confirmed on hardware, so the estimate is grounded
([source](https://raw.githubusercontent.com/dinoyoshi/serial-lcd-panel-client/main/docs/PROTOCOL_DEEPCOOL_MYSTIQUE.md)):

1. `0x0A` with the **image-mode** payload `EA 07 08 09 11 3A 09 00` (vs machine-info's
   `EA 07 02 02 02 27 21 00`) — sent **before and after** the frame data.
2. `0x10`, then `0x01` with an unexplained `00 00 00 00 00 00 54 00 1E`.
3. `0x12`, then `0x11` (empty).
4. The rest of the init sequence (config → labels).
5. `0x14` erase stored images. **Required for any upload after the first.**
6. `0x0F` begin upload.
7. Per frame: one transfer = 64-byte `DCLd` header **immediately followed by** the raw JPEG.
   Header: `"DCLd"`, byte4 `0x02`, bytes5-8 JPEG length (u32 LE), bytes9-12 16-bit sum of the JPEG
   mod 65536 as u32 LE, byte13 total frame count, bytes15-16 per-frame delay ms (u16 LE), byte17
   0-based frame index, bytes20-51 32-char ASCII-hex content/session id (constant per upload; an
   MD5 of the concatenated JPEGs works — the vendor's derivation appears unvalidated by firmware),
   bytes62-63 16-bit sum of the header's first 62 bytes.
8. ASCII `"dcldfinish"` (64 bytes, zero-padded), then `0x08` with `[0x01, 0x01]` (commit — distinct
   from init's `[0x00, 0x04]`).
9. The step-1..4 handshake **again**.

Content persists on the device — the panel keeps animating after the host process exits. That is
important: **uploads are a one-shot operation, not a stream**, so the steady-state daemon never
needs to hold the device for animation.

---

## 8. Build vs reuse

**Reuse the protocol knowledge. Build the daemon. Do not adopt any existing project as a
dependency.**

| Source | Licence | Reuse how |
|---|---|---|
| `dinoyoshi/serial-lcd-panel-client` → `docs/PROTOCOL_DEEPCOOL_MYSTIQUE.md` | MIT | **Port the image/GIF sequence.** It is the only public, hardware-confirmed account, and it is unusually candid about what failed. Port the *facts* (byte layouts, ordering), not the code — their own doc does the same with qt-deepcool's work. |
| `mymymy1303/qt-deepcool` → `PROTOCOL.md` | MIT | Cross-check our telemetry layout. It independently matches `CONTEXT.md`, which raises confidence in both. |
| Our `Mystique.Core.Device/` | ours | The telemetry, status parsing, and timeout behaviour are already verified on real hardware. Reuse the logic, not the WinUSB plumbing. |
| `ngeran/deepcool-mystique` | MIT | Read it for the pyusb shape on this exact device; do not depend on a single-day, 0★ repo. |
| `zetlaska/deepcool-lm360-lcd` | (check before use) | Architectural precedent: PyUSB + Pillow + systemd + socket IPC, different PID. |
| `gpteixeira/mystique-control` | MIT (stated) but **no source** | **Do not use.** It proves the target is reachable; it teaches nothing. Its own step of installing a udev rule from a GUI prompt is a pattern I would not copy. |
| `liquidctl` | GPL-3 | Not a dependency (no DeepCool driver), but copy its *documented patterns* — device units, capture guide — and consider it the eventual upstream home if the protocol ever stabilises. |
| `Nortank12/deepcool-digital-linux` | GPL-3 | No. HID-only, MYSTIQUE explicitly unsupported. |

**Why not fork `qt-deepcool`:** it pulls Qt 6 into a process whose entire job is a 1 Hz write, it
has no image support at all, and its udev rule is wrong for the device (§1.1). You would inherit
the dependency and still write the hard half.

**Why not wrap `gpteixeira`'s AppImage:** no source, one commit, and a 63 MB blob you cannot audit
that asks for a password to install a udev rule. Even if it works, it is unmaintainable and
unfixable when the panel gets stuck in image mode (R7) — which it will.

**Recommended plan, smallest first:**

1. **v0 — telemetry daemon, no images.** pyusb, one process, udev rule, systemd unit, 1 Hz `0x01`,
   `0x10` liveness, reconnect path. This is a day's work and it fixes the machine's daily use.
   Validate against the Windows implementation frame-for-frame.
2. **v1 — rotation + persisted state.** `0x02` at runtime is independently confirmed working at
   all four angles (§1.2).
3. **v2 — image/GIF upload.** Port §7. Budget most of the effort here, and expect R6/R7 to eat it.
   Build the `0x14`-before-reupload and single-buffer rules in from the start rather than
   discovering them.
4. **v3 — brightness / screen on-off / vendor modes.** `gpteixeira` proves these exist; nobody
   documents them. This is fresh RE work: capture the vendor app doing each one. Do not schedule it.

---

## 9. What I could not verify (read this as the confidence boundary)

- **I ran nothing on hardware.** Every protocol statement here is from a cited source or from
  `CONTEXT.md`.
- **`gpteixeira`'s feature list is unverified.** It is a README claim backed by screenshots and a
  binary. I did not download or run the AppImage (it would need the device).
- **`dinoyoshi`'s image path has one reporter.** Hardware-confirmed on a **Mystique 240**, not a
  360, by a 0★ project in a 48-hour window. No independent reproduction. Confidence: moderate —
  high for "this is the shape", lower for "these exact bytes on our unit".
- **The ZLP mechanism in R6.1 is their explanation, not mine.** The conclusion (single contiguous
  buffer) is what matters; the mechanism may be an artifact of their stack.
- **Pillow's JPEG defaults for JFIF/standard-Huffman** are my expectation, not a cited guarantee.
  Verify by byte-diffing.
- **`gitlab.com/public_projects5782906/Qt-deepcool-digital`** (credited as the inspiration by
  qt-deepcool) returned 404 on fetch. I make **no claim** about what it supports. If it matters,
  it needs a fresh look — it may predate everything above.
- **Interface count = 1** is an inference: the class is `ff-00-00`, both reference implementations
  claim interface 0 without selecting an alternate setting, and `qt-deepcool` hardcodes
  `interfaceNumber = 0`. Confirm with `lsusb -v` before relying on it.
- **No distro currently ships a conflicting udev rule for `0x3633`.** I checked the projects, not
  every distro's rule set. §3.3 gives the check to run.
- The AUR query and the OpenRGB `Controllers/` listing were both fetched via API and may be
  incomplete (the GitHub listing was truncated at 30 KB). Absence of a DeepCool entry is strong but
  not exhaustive evidence.

---

## Sources

Primary protocol / prior art
- qt-deepcool — <https://github.com/mymymy1303/qt-deepcool> · [PROTOCOL.md](https://raw.githubusercontent.com/mymymy1303/qt-deepcool/main/PROTOCOL.md) · [deepcooldevice.cpp](https://raw.githubusercontent.com/mymymy1303/qt-deepcool/main/deepcooldevice.cpp) · [main_cli.cpp](https://raw.githubusercontent.com/mymymy1303/qt-deepcool/main/main_cli.cpp) · [99-deepcool.rules](https://raw.githubusercontent.com/mymymy1303/qt-deepcool/main/99-deepcool.rules) · [commits](https://api.github.com/repos/mymymy1303/qt-deepcool/commits?per_page=5)
- serial-lcd-panel-client — <https://github.com/dinoyoshi/serial-lcd-panel-client> · [PROTOCOL_DEEPCOOL_MYSTIQUE.md](https://raw.githubusercontent.com/dinoyoshi/serial-lcd-panel-client/main/docs/PROTOCOL_DEEPCOOL_MYSTIQUE.md) · [rules](https://raw.githubusercontent.com/dinoyoshi/serial-lcd-panel-client/main/resources/99-deepcool-mystique.rules) · [repo metadata](https://api.github.com/repos/dinoyoshi/serial-lcd-panel-client)
- mystique-control — <https://github.com/gpteixeira/mystique-control> · [v0.6.0 release](https://github.com/gpteixeira/mystique-control/releases/tag/v0.6.0) · [commit](https://api.github.com/repos/gpteixeira/mystique-control/commits?per_page=5)
- deepcool-mystique (Python) — <https://github.com/ngeran/deepcool-mystique>
- deepcool-digital-linux — <https://github.com/Nortank12/deepcool-digital-linux> · [discussion #18](https://github.com/Nortank12/deepcool-digital-linux/discussions/18)
- deepcool-lm360-lcd — <https://github.com/zetlaska/deepcool-lm360-lcd>
- deepcool-lm — <https://github.com/daedlock/deepcool-lm>
- liquidctl driver registry — <https://raw.githubusercontent.com/liquidctl/liquidctl/main/liquidctl/driver/__init__.py>
- OpenRGB controllers — <https://api.github.com/repos/CalcProgrammer1/OpenRGB/contents/Controllers>
- CoolerControl — <https://docs.coolercontrol.org/getting-started>
- AUR — <https://aur.archlinux.org/rpc/?v=5&type=search&arg=deepcool>
- Hardware presence / driver status — <https://linux-hardware.org/?id=usb:3633-0009>
- Vendor spec (640×480, 2.8", 16.7M, gyro) — <https://www.deepcool.com/products/Cooling/cpuliquidcoolers/MYSTIQUE-360-5th-Gen-Liquid-Cooler-with-LCD-1851-1700-AM5/2024/18059.shtml>

Kernel and system
- usbmon — <https://www.kernel.org/doc/html/latest/usb/usbmon.html>
- `CONFIG_USB_MON` — <https://raw.githubusercontent.com/torvalds/linux/master/drivers/usb/mon/Kconfig>
- `usbdev_open` / `usbdev_release` (autoresume) — <https://raw.githubusercontent.com/torvalds/linux/master/drivers/usb/core/devio.c>
- `usb_driver_claim_interface` (`-EBUSY`) — <https://raw.githubusercontent.com/torvalds/linux/master/drivers/usb/core/driver.c>
- `usbserial_generic` id_table — <https://raw.githubusercontent.com/torvalds/linux/master/drivers/usb/serial/generic.c>
- USB power management — <https://www.kernel.org/doc/html/latest/driver-api/usb/power-management.html>
- udev(7) — <https://www.freedesktop.org/software/systemd/man/latest/udev.html>
- `uaccess` builtin (seat/session scoping) — <https://raw.githubusercontent.com/systemd/systemd/main/src/udev/udev-builtin-uaccess.c>
- udev tag reference — <https://wiki.archlinux.org/title/Udev>
- `uaccess` + SSH caveat — <https://discussion.fedoraproject.org/t/uaccess-tagged-udev-rules-in-usermode-systemd-services-and-ssh-access-for-sdrs/149482>
- `uaccess` documentation gap — <https://github.com/systemd/systemd/issues/4288>
- systemd-sleep(8) — <https://www.freedesktop.org/software/systemd/man/latest/systemd-sleep.html>
- liquidctl, waiting for devices — <https://raw.githubusercontent.com/liquidctl/liquidctl/main/docs/linux/making-systemd-units-wait-for-devices.md>
- liquidctl, capturing USB traffic — <https://raw.githubusercontent.com/liquidctl/liquidctl/main/docs/developer/capturing-usb-traffic.md>
- Wireshark USB capture — <https://wiki.wireshark.org/CaptureSetup/USB>
- Wireshark USB display filters — <https://www.wireshark.org/docs/dfref/u/usb.html>

libusb / pyusb
- `libusb_wrap_sys_device` + `NO_DEVICE_DISCOVERY` — <https://libusb.sourceforge.io/api-1.0/group__libusb__dev.html>
- pyusb — <https://pyusb.github.io/pyusb/>

Third-party corroboration (weaker)
- `usbmon` cannot be built out-of-tree — <https://github.com/orbstack/orbstack/issues/2800>
