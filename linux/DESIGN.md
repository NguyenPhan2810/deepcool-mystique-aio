# Design notes

Companion to `README.md`. This file records the decisions that are not obvious
from the code, and the exact boundary between what is implemented and what is
still unknown.

## Scope

The deliverable is a userspace Linux driver for the MYSTIQUE LCD:

- **telemetry** (Machine Info mode), ported byte-for-byte from the Windows C#
  reference in this repository, and
- **image/GIF upload**, implemented from the public hardware-confirmed protocol
  record in `dinoyoshi/serial-lcd-panel-client` (MIT), which credits
  `mymymy1303/qt-deepcool` (MIT) for the original reverse engineering.

The unresolved limitation is the return path: after an upload, the device cannot
reliably be put back into Machine Info mode. That is documented, not papered over.

```
mystique/protocol.py      pure builders/parsers; no I/O, no USB import
mystique/image_upload.py  pure header + upload plan; Pillow only inside encode_frames
mystique/device.py        pyusb: discovery, exclusive claim, transfers, faults
mystique/telemetry.py     sysfs/procfs sources, nvidia-smi fallback
mystique/cli.py           verbs, one-update-per-second loop
```

`protocol.py` and the upload-plan half of `image_upload.py` are deliberately
importable with no pyusb and no Pillow. A unit test runs `mystique.protocol` in a
subprocess with `usb` blocked from `sys.meta_path`; image encoding tests use
`pytest.importorskip("PIL")`.

## Ownership model

- **One process owns interface 0.** `MystiqueDevice.open()` finds the device,
  claims the interface, validates the four expected endpoints, and returns the
  only object that can perform transfers. There is no module-level handle and no
  second entry point.
- **The timeout is part of ownership.** `open()` requires a positive timeout
  (default 1500 ms) and stores it; every `write`/`read` passes it explicitly.
  Bulk endpoints default to infinite in both WinUSB and libusb, so a future call
  site that forgets the argument is the one bug that would hang the loop with no
  exception. The explicit `timeout=` on every call is the guard.
- **No kernel driver is detached.** `3633:0009` is vendor-specific and nothing
  binds to it. Calling `detach_kernel_driver` would be wrong if something ever
  did bind; a failed claim is reported as `ACCESS_DENIED` with the likely cause
  (another process, or a kernel driver holding the interface).
- **Claiming is exclusive.** On Linux the claim is a `USBDEVFS_CLAIMINTERFACE`
  ioctl; a second opener gets `LIBUSB_ERROR_BUSY`. There is no permissive
  sharing equivalent to the Windows `CreateFile` share mode, and the driver does
  not try to emulate one. Running the CLI while `mystique.service` is streaming
  fails honestly instead of fighting for the device.
- **Nothing is sent on close.** `close()` releases the interface and disposes
  the handle. In telemetry mode the panel keeps its last frame, which reads as
  "nobody is driving me". In image mode the uploaded content persists on the
  device by design. There is no mode-reset command on shutdown.
- **A reopen requires re-initialisation.** Every `live` start runs the Machine
  Info handshake. A systemd restart after a replug therefore cannot silently ACK
  telemetry into a panel that is not listening.
- **Privilege is a packaging concern, not a code path.** The udev rule grants
  `plugdev` (or `uaccess`) access; the driver never escalates and never needs
  root if the rule is installed. The shipped unit runs as root only to work
  without the rule; the README gives the non-root recipe.

## Threading model for the update loop

There is **one thread and no background worker**. Each telemetry iteration is
strictly sequential:

1. read the five sensors (short sysfs/procfs reads, one optional `nvidia-smi`
   subprocess only when no DRM hwmon node exists and at most once per 30 s);
2. build the 39-byte payload;
3. blocking bulk write followed by blocking bulk read, each with the 1500 ms
   timeout;
4. sleep 1 s, interruptibly.

Why no reader thread: the protocol has no request id and no sequence number, so
two exchanges must never overlap or their replies could be paired with the wrong
command. A queue would add that failure mode for no throughput gain at 1 Hz and
48 bytes per exchange.

Signals are the only concurrency. `SIGINT`/`SIGTERM` set a `threading.Event`;
the loop wakes between frames and exits cleanly. A transfer already in flight is
bounded by the 1500 ms timeout, and the sleep is interruptible, so shutdown is
under ~2.5 s worst case (the unit allows 10 s).

There is deliberately **no `AbortPendingTransfers` equivalent**. pyusb's
synchronous API has no safe way to cancel a transfer from another thread, and
adding one would put device I/O on the signal path. The timeout makes the
blocking call self-limiting instead.

**Image upload is a one-shot synchronous sequence.** It runs on the same thread,
holds the claim for its duration, and writes frames back-to-back with no reader
thread and no per-frame reply. Progress is a plain callback in `cli.py`, not a
worker. Interrupting an upload (Ctrl+C) stops between operations, so a frame
boundary is the worst case. The upload is not resumable; the next run starts
with erase.

Consequence: `MystiqueDevice` is not thread-safe. It is documented as such, and
the package never hands it to a thread.

## Recovery behaviour

The taxonomy mirrors `Mystique.Core.Device/UsbFault.cs`, with two Linux-specific
kinds (`access-denied`, `stall`) because libusb names those conditions directly.

| Fault | Where it comes from | In-process action | What the caller does |
|---|---|---|---|
| `not-present` | `usb.core.find` returned None, or libusb `-5` | none | exit 1; systemd restarts every 5 s until the device appears |
| `access-denied` | libusb `-3` (EACCES), `-6` (interface busy) | none | exit 1 with the udev/plugdev or "another owner" hint |
| `timeout` | libusb `-7` | `recover(ABORT_AND_FLUSH)` — a no-op for synchronous transfers, which have no queue left to abort | `live` retries up to 3 consecutive faults, then exits; other verbs and uploads exit |
| `stall` | libusb `-9` | `recover(ABORT_FLUSH_RESET)` calls `clear_halt` on all four endpoints | same retry policy as timeout |
| `ambiguous` | `-1`, `-11`, `-13`, anything unknown | `clear_halt` as above | same retry policy; never treated as fatal on its own |
| `surprise-removal` | libusb `-4`, or `-10` when we did not abort | none | exit 1; systemd restarts, and the next run re-runs the init handshake |
| `self-aborted` | `-10` after our own cancellation | none | not reachable cross-thread today; would stop the loop |
| `programming` | `-2`, `-12`, missing backend, closed handle | none, or reopen | exit 1 with a message; this is a bug or a missing dependency, not a device state |

Notes:

- `recover()` only does work for `ABORT_FLUSH_RESET`. `clear_halt` on a pipe
  that is not halted returns an error; that error is ignored because the reset
  is best effort.
- Full device resets (`usb.core.Device.reset()`) are **not** issued
  automatically. A port reset is disruptive and the observed faults do not
  require it. If repeated stalls survive `clear_halt`, the process exits and
  systemd restarts it — a fresh open plus init is the stronger reset.
- There is no in-process reopen loop, jittered backoff, or log escalation. That
  machinery exists in the Windows service because a Windows service cannot be
  restarted cheaply and must keep a tray state consistent. Here systemd is the
  supervisor: `Restart=on-failure`, `RestartSec=5s`, `StartLimitBurst` defaults
  keep a dead device from hammering, and journald handles retention. The cost is
  one journal line per restart while the AIO is unplugged.
- An image upload that fails midway leaves an incomplete image. The device is
  not bricked: the next upload starts with `0x14` erase. If a frame checksum is
  wrong the device can drop off the USB bus and re-enumerate; the fault path
  reports `surprise-removal`, systemd restarts, and the panel keeps whatever it
  had. Do not disable the header checksum tests.
- If the owner wants reconnect-without-restart later, the place to add it is
  between `open()` and `enter_machine_info_mode()` in `cli.py`; the fault already
  carries the recovery action.

## Init success is not gated on status byte 5

The C# reference throws when command `0x10` reports a mode other than `0xFF`.
The public hardware record contradicts that gate: a capture returned byte 5 =
`0x00` while the panel was demonstrably showing live Machine Info. This driver
therefore treats init as successful when the 13-command handshake completes
without a transfer error, performs the status read because the reference
sequence does, and prints an advisory note when the mode is not `MachineInfo`.

Consequence: `mystique init` no longer fails on a healthy panel whose firmware
reports a different mode byte. The cost is that a genuinely failed mode switch
is only visible by looking at the panel, not by the exit code. That is the
trade the hardware evidence demands.

## Commands: implemented vs known-but-unimplemented

Implemented, with byte-exact tests:

| Cmd | Pipe | Payload used | Implemented as |
|---|---|---|---|
| `0x12` | `01/81` | empty | `info`; first init step; image handshake |
| `0x10` | `02/82`, `01/81` | empty | `status`; post-init advisory; image handshake |
| `0x01` | `02/82` | 39-byte telemetry layout | `stream`, `live` |
| `0x01` | `01/81` | `00 00 00 00 00 00 54 00 1E` | image handshake heartbeat, verbatim |
| `0x02` | `01/81` | `01 00 <rot> 01 24` | init rotation; image handshake config |
| `0x03` | `01/81` | `01` | init step; image handshake |
| `0x04` | `01/81` | `05 00 00 01` | init step; image handshake |
| `0x05` | `01/81` | `01 01` | init step; image handshake |
| `0x06` | `01/81` | `01` | init step; image handshake |
| `0x07` | `01/81` | `00 02` | init step; image handshake |
| `0x08` | `01/81` | `00 04` | init step |
| `0x08` | `01/81` | `01 01` | image commit after `dcldfinish` |
| `0x0A` | `01/81` | `EA 07 02 02 02 27 21 00` | Machine Info mode switch (init) |
| `0x0A` | `01/81` | `EA 07 08 09 11 3A 09 00` | Image mode switch (upload) |
| `0x0B` | `01/81` | empty | init step; image handshake |
| `0x0F` | `01/81` | empty | begin image upload |
| `0x11` | `01/81` | empty | serial request, image handshake only |
| `0x14` | `01/81` | empty | erase stored images, before every upload |
| `0x15`–`0x17` | `01/81` | `2D 2D` | init step; image handshake |
| raw | `01/81` | 64-byte `DCLd` header + JPEG | one write per frame |
| raw | `01/81` | `dcldfinish` zero-padded to 64 | finish marker |

Known but **not** implemented / not understood:

1. **Returning to Machine Info after an upload.** The main gap. See below.
2. **Individual semantics** of the setup commands, the mode-switch payload
   bytes, the display-label commands, and the image-handshake heartbeat. They
   are reproduced as working sequences, not decoded.
3. **Reply checksum / `HIDC` marker validation.** The reference only checks the
   `55 2E` header and the echoed command byte (or `0x00`). This driver matches
   that. Whether the device always fills the checksum is unverified.
4. **Whether the image-mode handshake accepts a non-zero rotation.** The public
   implementation always uses 0; this driver defaults to 0 and forwards
   `--rotation` only as an explicit opt-in. Unverified.
5. **Panel native resolution.** 480×640 is the confirmed upload size, not a
   claim about the panel's internal panel geometry.
6. **Multiple LCD families.** Only `3633:0009` is matched.
7. **Firmware.** No code, no images, no writes to `resources/fw/*`, permanently.

## Image upload sequence

The plan is built as pure `Operation(endpoint, data, reply_endpoint,
reply_optional, settle_ms, description)` values in `image_upload.py` and executed
by `MystiqueDevice.upload_images()`. The order is copied from the public
implementation and the usbmon capture, not re-derived:

1. `0x14` erase stored images, once, unconditionally. Without it a second upload
   is accepted and silently keeps the previous content.
2. Image-mode handshake, all on endpoint 1:
   `0x0A` image payload, `0x10`, `0x01` heartbeat payload, `0x12`, `0x11`, then
   the shared setup steps `0x02` through `0x17` (10 ms each). The leading
   `0x12` and trailing Machine-Info `0x0A` of the normal init sequence are
   excluded because they are sent explicitly or replaced.
3. `0x0F` begin upload.
4. For each frame: **one** write of `header + jpeg`. The header is 64 bytes,
   exactly one `wMaxPacketSize`. Two writes cause libusb to terminate the first
   transfer with a ZLP at that boundary; the device then silently ignores the
   upload. This is the single most important implementation detail in the image
   path, and a unit test asserts that the header and JPEG are one `Operation`.
5. `dcldfinish` zero-padded to 64 bytes, one write.
6. `0x08` with `[0x01, 0x01]` — the commit, distinct from init's `[0x00, 0x04]`.
7. The image-mode handshake again. The public record confirms the upload is
   accepted but invisible if this replay is omitted.

Image-mode command replies are drained but **not parsed**: the reference ignores
their contents, and this capture-derived flow is synchronized by command order,
not reply matching. A missing reply (timeout) is tolerated, because the public
implementation catches every read in this flow. A stall, removal, or short read
is still fatal: a dead device must not be mistaken for a quiet one. Telemetry
and init replies remain strictly validated.

The header fields are byte-exact: magic `DCLd`, byte 4 `0x02`, JPEG length u32
LE, JPEG sum-of-bytes mod 65536 as u32 LE, total frames u8, delay u16 LE,
0-based index u8, 32-char ASCII hex content id, and a 16-bit sum of the first 62
header bytes. The content id is MD5 of the concatenated JPEGs, as in the public
implementation; the docs note the firmware appears not to validate the
derivation.

### JPEG encoding constraints

Two properties are load-bearing and were confirmed by real-hardware experiments
in the public project:

- a **JFIF APP0 segment immediately after SOI** (`FF D8 FF E0`, containing
  `JFIF\0`). Encoders such as sharp omit it by default; Pillow emits it, and
  `ensure_jfif()` inserts it if absent.
- **standard, not content-optimized Huffman tables.** Pillow is called with
  `optimize=False`, which makes libjpeg emit the ITU T.81 Annex K tables. The
  test parses the DHT segments and asserts the four standard bit-length arrays,
  so a future Pillow or a careless edit cannot silently regress this.

Without both, the panel accepts the upload without an error and displays
nothing. That failure mode is why these are tests, not comments.

## Returning to Machine Info is unresolved

After a successful upload the device is in Image mode, and the public record's
attempts to switch back were not reproducible. Hypotheses that failed or were
not confirmed: sending `0x14` before init (worked once, then not), a third
ModeSwitch payload seen in a vendor capture, and the vendor app closing and
reopening the USB handle (an untested lead, not a working theory).

This driver does not attempt a fix. `mystique init` / `live` after an upload may
complete without a transfer error and leave the image on screen. The README
states this plainly. Reopening the handle inside one process is the most
plausible next experiment, but it is not implemented because it is unverified
and could make the state worse.

## The empirical telemetry block

`build_display_payload()` reproduces `MystiquePackets.BuildDisplayPayload`
exactly. Bytes 0, 3, 6, 18, and 20 are understood. Bytes 8, 9, 11–17, and 21–25
are only partly understood: the `05|06|09` selector at byte 8 bands on GPU
temperature (below 35 °C, below 38 °C, otherwise), bytes 9/12/14/15/17 are fixed
constants, the whole-GHz and hundredths values are duplicated, and 21–22 is
repeated verbatim at 24–25 for no known reason. This is labelled empirical in the
code because that is what it is. It is not to be "cleaned up" without a capture
proving the device accepts a different layout.

The public TypeScript implementation computes the clock fraction with floating
point (`trunc((ghz - trunc(ghz)) * 100)`), which can produce 59 instead of 60 for
exactly 4600 MHz. This driver keeps the C# integer arithmetic (`mhz % 1000 / 10`)
because the C# implementation is the verified telemetry reference and its output
was observed on the panel. The layout is identical; only the rounding path
differs, and the C# path is the one to preserve.

## Validation performed

- **Protocol, upload-plan, sensor, argument and fault tests:** 141 hardware-free
  tests pass with Pillow 12.3.0 installed. Without Pillow, 136 pass and the 5
  encoding tests skip. The suite was run on Windows; nothing in it is OS-specific,
  and the protocol test blocks `usb` imports in a subprocess to prove the pure
  layer.
- **USB telemetry path against the real device:** the development machine had
  the AIO free and a libusb backend available, so the Python driver was run
  against it through libusb. `pipes`, `status`, `info`, `init 0`, `stream 2`,
  `live 2` (which also sets rotation 180°, the preserved positional quirk), and
  `init 0` to restore rotation 0 all succeeded. This exercised discovery,
  exclusive claim, endpoint validation, both pipe pairs, response validation,
  the 13-command handshake, and 39-byte telemetry writes. All replies were ACKs;
  `0x15`/`0x16`/`0x17`/`0x0A` returned command byte `0x00`, confirming the
  wildcard-ACK rule. Device identifiers were masked in that run. A later attempt
  while `MystiqueService` was running failed with `access-denied`, which the
  taxonomy reported correctly once POSIX/Win32 errno values (13, 10060, …) were
  mapped alongside libusb's negative codes.
- **Image upload path:** unit-tested only, **not** executed against this machine's
  panel. Doing so would strand it in Image mode, which is the unresolved
  limitation above. The protocol facts and the two JPEG constraints come from
  the public project's real-hardware confirmation (usbmon capture of genuine
  DeepCreative traffic plus three consecutive successful uploads), not from a
  guess here.
- **Linux sensor sources:** tested only against a synthetic `/proc`+`/sys` tree.
  The real sysfs layouts (coretemp/k10temp, amdgpu/i915 DRM hwmon, cpufreq,
  nvidia-smi) follow documented kernel interfaces, but they have not been
  exercised on the owner's future Linux install. That is the main unverified
  area on the sensor side and the first thing to check after boot.
