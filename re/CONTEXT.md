# Shared brief — DeepCool MYSTIQUE LCD reverse engineering

Read this first. Do not re-derive what is already verified here; extend it.

## Mission

Produce everything needed to drive the DeepCool MYSTIQUE AIO LCD on **Linux**, from the
protocol level up. The owner is replacing Windows on this machine. Today only a Windows
WinUSB implementation exists. There is no vendor Linux driver.

Hard constraint: **no firmware flashing, no firmware images, no write to `resources/fw/*`**.
Firmware is out of scope permanently. Research and protocol work only.

## Verified hardware facts

| Fact | Value | How known |
|---|---|---|
| Device | DeepCool MYSTIQUE (360) AIO LCD | USB product string reported as `MYSTIQUE` |
| VID:PID | `3633:0009` | PnP enumeration on the live machine |
| Instance ID | `USB\VID_3633&PID_0009\<serial>` | `Get-PnpDevice` |
| Windows class | `USBDevice` (vendor-specific, **not** HID, **not** a COM port) | same |
| Windows driver | WinUSB (custom INF) | working C# code uses `WinUsb_Initialize` |
| Endpoints | `0x01` OUT / `0x81` IN — init + info; `0x02` OUT / `0x82` IN — status + telemetry | `WinUsb_QueryPipe` on live device |
| Transfer type | Bulk | same |
| Packet size | 48 bytes out, up to 64 bytes read buffer | working implementation |
| Pipe transfer timeout used | 1500 ms | working implementation |

Windows class being `USBDevice` and the driver being WinUSB matters a lot for Linux: the
device exposes **vendor-specific bulk endpoints**, so on Linux it binds to `usbfs`/`libusb`,
not `hidraw` and not a serial driver. No kernel driver is needed for a userspace driver.

## Verified wire format

Request packet, 48 bytes:

```
off  0     1     2      3 .. 41              42..45      46..47
     AA    2E    CMD    payload (max 39)     "HIDC"      LE16 sum of bytes 0..45
```

Response packet:

```
off  0     1     2                 3        4..41                     42..45   46..47
     55    2E    CMD or 00        (flags)   device id fields          "HIDC"  (checksum)
```

- Response byte 2 echoes the command, or is `0x00` for some replies. Treat both as valid.
- Response bytes 4..11 are a 12-char ASCII device id; bytes 12..41 hold a second stable
  24-char hex identifier. Both are per-device. **Never publish these** — redact.
- Checksum is a plain 16-bit sum, little-endian, over bytes 0..45 inclusive. No CRC.

## Verified commands

| Cmd | Pipe | Meaning | Known payload |
|---|---|---|---|
| `0x12` | 01/81 | device info | none |
| `0x10` | 02/82 | status | none. Response byte 4 = rotation (0-3), byte 5 = mode (`0xFF` machine-info, `0x00` image/gif) |
| `0x01` | 02/82 | telemetry frame | 39-byte layout below |
| `0x02` | 01/81 | display config | `01 00 <rot> 01 24`, rot 0-3 = 0/90/180/270° |

Machine-info handshake, **13 commands, order is not ascending — do not tidy it**:

```
12 {}  02 {01 00 ROT 01 24}  03 {01}  04 {05 00 00 01}  07 {00 02}
08 {00 04}  05 {01 01}  0B {}  06 {01}  15 {2D 2D}  16 {2D 2D}
17 {2D 2D}  0A {EA 07 02 02 02 27 21}
```

Each settles ~10 ms; `0x0A` settles ~100 ms. After the sequence, `0x10` must report mode
`0xFF` or the device did not enter machine-info mode.

Telemetry payload (39 bytes, indices are payload-relative):

```
[0]  cpu temp °C          [3]  cpu usage %        [6]  memory load %
[8]  05|06|09  (GPU block variant, temperature-banded)
[9]  03         [11] gpu temp °C   [12] 05      [14] 06   [15] 0C   [17] 07
[18] cpu clock whole GHz  [20] clock fraction marker (min 0x0A)
[21..22] LE16 = (mhz % 1000 / 10) * 100      [24..25] duplicate of [21..22]
```

The `[8]`/`[9]`/`[11]`..`[25]` block is only partly understood. Its exact semantics —
especially the `05/06/09` selector and the duplicated fraction field — are an open question.
Do not present guesses as facts.

## Corrections since this brief was written

- **Image and GIF upload is no longer undocumented.** See
  `re/findings/06-image-upload-protocol.md`. It was recovered from a public project that
  confirmed it against a Mystique 240, and includes the frame header layout, the required
  transfer sequence, and three silent-failure traps.
- **Do not gate init on `0x10` byte 5 being `0xFF`.** A real capture showed `0x00` while the
  panel was visibly live in machine-info mode. Treat it as advisory.
- **Panel is 640x480 landscape** (vendor-stated); uploads are sent as 480x640 portrait JPEGs.
- **No shipped native addon serves PID `0x0009`**, and the `HIDC` framing appears in no
  native binary. The proprietary addons cover other PID families. See
  `re/findings/02-native-addons.md`.
- **The vendor app's main process is V8 bytecode** (`out/main/index.jsc`, bytenode), which is
  why static recovery of its protocol stops there.

## Unknown / unverified — the actual work

1. ~~Image and GIF upload~~ — recovered, see the correction above. Now the open question is
   whether it behaves identically on a **360**; every prior-art confirmation came from a 240.
2. Whether an image upload can be reversed back to machine-info mode at all. Nobody has
   solved this. Strong lead: the vendor app may close and reopen the USB handle.
3. Full command table beyond `0x01/0x02/0x10/0x12`. The init sequence implies at least
   `0x03..0x0B`, `0x15..0x17`, `0x0A` exist with unknown effect.
4. Whether multiple LCD families share this protocol (see addons below).
5. Chunking/acknowledgement for large transfers, if any.

## Vendor software on this machine

Root: `C:\DeepCool\`

```
resources/app.asar                          327 MB, Electron bundle
resources/app.asar.unpacked/resources/
  C122/index.node                           per-family native N-API addons
  CH690/CH690.node
  L086/index.node  L086/preset/default.jpg
  L122/index.node, index_back.node
  L136/L136.node
  L142/index.node
  capture/wincapture-node.node
  system/system_info_x64.node
  opencv/{deep_cv_x64.node, pixel.node}
  magick/{magick_x64.exe, magick_x86.exe}
  ffmpeg/{ffmpeg.exe, ffprobe.exe, ffmplayer.node}
  fw/MYSTIQUE_2.70.bin              <-- FIRMWARE. READ-ONLY. DO NOT TOUCH.
resources/service/x64/
  deep_service.exe                  (runs as "Deep Cool Helper Service")
  DeepCoolDisplayService.exe        ("Deep Cool Display Service")
  SensorBridgeServer.exe            (supplies sensors over named pipe deepcool_sensor_data)
  HWiNFO64.dll, sensor.json
```

The per-family `.node` addons are the highest-value target: they are where the real USB
protocol is compiled. The ladder of names (`C122`, `CH690`, `L086`, `L122`, `L136`, `L142`)
suggests one addon per LCD panel generation. Which one serves `3633:0009` is **unknown** —
determine it from evidence, not from the name.

## Existing implementation in this repo

`Mystique.Core.Device/` (C#) is a working Windows driver for this device:

- `MystiquePackets.cs` — wire format, init sequence, telemetry layout, status parsing
- `MystiqueDevice.cs` — WinUSB handle ownership, pipe policy, exchange
- `DeviceLocator.cs` — presence detection by VID/PID
- `SensorBridgeClient.cs` — reads sensors from DeepCool's named pipe
- `MystiqueService/` — Windows service supervisor

Treat it as the reference for everything it covers. It is **Windows-only** and must not be
modified by this effort. `docs/superpowers/specs/2026-08-06-mystique-service-design.md`
contains hard-won operational knowledge worth reading before touching hardware.

`deepcool-worker.js` at the repo root is vendor code extracted from the asar. It is
proprietary and must not be redistributed; read it, do not publish it.

## Tooling available

- `rea` CLI v6.7.0 is installed globally (`rea --help`). REA = reverse-engineering toolkit:
  `analyze-javascript-application`, `inspect`, `decompile`, `function`, `capabilities`,
  `doctor`, evidence bundles.
- **No** Ghidra, IDA, or Hopper is installed. Java 11 and Python 3.12 are.
  `rea`'s native providers therefore need setup before `decompile` will work; static PE
  inspection (`inspect`) works without any engine.
- Node 24, pnpm, `npx asar` are usable.
- **Do not run `rea setup`** — it rewrites the live agent configuration. Configure a
  provider only via environment variables or CLI flags if you genuinely need decompilation.
- Do not install kernel-mode drivers (no USBPcap) without being asked.

## Working agreements

- Write only inside your designated paths. Do not modify `Mystique.*`, `docs/`, or `.git`.
- Never commit. Never `git add`.
- Cite evidence for every claim: file path plus line/offset, or the exact byte sequence, or
  the command you ran and its output. Mark inference as inference.
- Records of the device ID, serial, or hostname must be redacted in anything you write.
- If a step needs hardware that is busy, check first:
  `Get-Service MystiqueService` — the service owns the device when running.
- Prefer small, reproducible scripts saved to disk over one-off shell incantations.
