# Handover — DeepCool MYSTIQUE 360 LCD

Last updated: 2026-11-18, late. Device `3633:0009`. Committed and pushed to Forgejo at
`34aa41a`; work after that commit is uncommitted.

Read this first, then jump to whichever detail file you need.

## Where this stands

A cross-platform driver exists and has been **verified on the real panel**:

- `rust/apps/mystique-daemon` — headless, owns the USB device, streams telemetry, serves a
  local control API. Survives the UI being closed.
- `rust/apps/mystique-ui` — Tauri desktop app, **client only**, speaks to the daemon over a
  named pipe (Windows) / Unix socket (Linux).
- `rust/crates/*` — protocol, USB transport, sensors, image media, IPC types, CLI.
- The old `MystiqueService` (C#) is the current production driver on Windows and still works.
  The Rust daemon is its replacement and has driven the panel with the service stopped.

378+ tests green at the last run; all four gates (`build`, `test`, `clippy -D warnings`,
`fmt --check`) pass. File-size rules are in `AGENTS.md`.

## Stream rate

1 Hz by default, **30 Hz measured clean** and selectable at runtime
(`set_stream_rate {"hz":30}`). The link was never the limit: the panel acknowledges a frame
in **0.49 ms**, and 30 Hz is 0.19% of the bus. The old ceiling was a blocking read on the
vendor sensor bridge, which pushes once per ~1.1 s; it now runs on its own thread and the
sampler copies the newest snapshot. Above 30 Hz the Windows 15.6 ms timer grain appears.
Numbers still change at the sensor rate — the frame rate buys smoothness, not freshness.
Full measurements, including the untested Recording-graph question, in
`findings/20-stream-rate.md`.

## The protocol is fully mapped

Every command byte, from captures and hardware tests. Full detail in `16`–`20`.

| Byte | Meaning |
|---|---|
| `0x01` | telemetry frame, 39-byte payload: CPU °C `[0]`, CPU % `[3]`, MEM % `[6]`, GPU °C `[11]`, clock GHz `[18]`, clock fraction `[20]`, **CPU fan RPM `[21..22]`**, **pump RPM `[24..25]`** |
| `0x02` | `[idle screen, gyro, rotation, RGB sync, brightness]` |
| `0x03` | display mode: 1 Monitor, 2 Media, 3 Recording |
| `0x04` | `[main, style, 0, aux]` — index 2 never non-zero |
| `0x05` | `[recording: CPU clock, recording: CPU temp]` |
| `0x06` | accepted, no visible effect |
| `0x07` | `[switch time, switch animation]` |
| `0x08` | upload commit; no visible effect otherwise |
| `0x0A` | **clock**: `EA 07 <month> <day> <hour> <minute> <second>`, sent about once a second; it was previously misread as a heartbeat with a counter |
| `0x0B`–`0x0E`, `0x13` | accepted, no visible effect |
| `0x0F` | begin image upload |
| `0x10` | status: rotation, mode |
| `0x11` | returns zeros — not a serial read |
| `0x12` | device info, 32-hex id at bytes 4..35 |
| `0x14` | erase stored images |
| `0x15` / `0x16` | CPU / GPU name labels, ASCII, 39 bytes max |
| `0x17` | third label slot, no visible effect |
| `0xA1` | **panel colour test pattern** — sustained by traffic; never send |

Wire format: 48-byte packet `AA 2E <cmd> <39 payload> "HIDC" <sum16 LE>`, reply `55 2E`. Result
byte 3: `00` ok, `01` bad trailer, `02` bad checksum. Two pipes: init `0x01/0x81`, data
`0x02/0x82`. Command on the wrong pipe looks like a command that does not exist.

Image upload: `0x0F`, then a **64-byte `DCLd` header as its own transfer**, then the JPEG in
**64-byte chunks**, then `dcldfinish` (55 bytes), then `0x08 [01]`. Works, ~2 s for a GIF, and
the driver crops to 480×640 by default. Content id is a per-upload tag, not a hash.

## Verified on hardware from our own code

Brightness, rotation (gyro must be off), gyro toggle, main area, style type, aux, mode switching,
recording options, labels, image/GIF upload, erase-to-return-to-dashboard, telemetry streaming
at 1–30 Hz, fan and pump RPM on the dual dial, daemon surviving UI close.

## Not verified / hazards

- **`0xA1`** starts a colour test pattern. It animates while the panel receives traffic and stops
  when traffic stops. Never send it from the driver.
- **Stored media holds the display**: after an upload the panel shows the image regardless of
  mode until `0x14` erase. That is content presence, not a mode flag.
- `0x04` index 2 is always zero; reserved, not understood.
- `0x01[8]` varies in vendor captures (`0x28`…`0xF7`) and its meaning is unresolved; our
  `0x05/0x06/0x09` is an inherited guess.
- **Fan RPM needs the vendor helper on Windows.** `Deep Cool Helper Service`
  (`SensorBridgeServer.exe`) is the only fan source; without it the frame carries 0 and the
  panel draws `0 RPM`. Linux reads hwmon fan channels, with the pump only when a channel is
  labelled `pump`.
- **Recording mode graphs at 30 Hz are untested.** If the panel advances those graphs per
  received frame, a high rate will scroll them proportionally faster than the vendor app.
  Check this before making 30 Hz a default; 1 Hz remains the default.
- The UI's browser-pass was done with a stubbed bridge; the live daemon→panel event path was
  verified separately in the terminal but should be re-checked in the final UI.
- **Linux ran on 2026-11-20** — first native build, 409 tests, daemon and Tauri UI on the
  real panel. It exposed a cross-platform clock bug (`0x0A` was routed to the init pipe);
  see `re/findings/23-linux-bringup.md`.

## How to run

```powershell
cd C:\Users\nguyen\dev\Deepcool-AIO-LCD\rust
cargo build --workspace
.\target\debug\mystique-ui.exe          # spawns the daemon if none is listening
```

The daemon owns the device, so `MystiqueService` must be stopped and `DeepCool.exe` closed
(it leaves six background processes that keep the device open even after the window closes).

Sensors come from **Deep Cool Helper Service** (`SensorBridgeServer.exe`), which runs
independently of `MystiqueService` and pushes one snapshot per ~1.1 s. The daemon reads it on
a worker thread and falls back to `sysinfo` when it is absent or stalled; Linux always uses
`procfs`/`sysinfo`.

Direct CLI (takes the device, so stop the daemon/service first):

```powershell
cargo run -p mystique-cli -- status
cargo run -p mystique-cli -- probe --try 0x10::data      # dry run, prints the packet
cargo run -p mystique-cli -- probe --send --try 0x10::data
cargo run -p mystique-cli -- explore --send --phase presence
```

Recovery: a wedged pipe needs `pnputil /restart-device "USB\VID_3633&PID_0009\<serial>"` or a
replug; restarting the service is not enough.

## Where the evidence lives

| File | Contents |
|---|---|
| `00-hardware-identity.md` | descriptors, class 0xFF, single-owner |
| `03-live-capture.md` | ground truth: response layout, result codes, checksum behaviour |
| `06` / `12` | image protocol; the 240-derived version and the corrected 360 capture |
| `10` / `11` | command map, pipe coverage, sweep results |
| `13`–`19` | settings decode, UI inventory, hardware verification, `0x04` layout, mode, RGB/recording, probe results |
| `20-stream-rate.md` | why the stream was 1 Hz, the sampler fix, the measured ramp to 30 Hz |
| `21-fan-and-pump-rpm.md` | fan/pump RPM bytes, and the clock-fraction bug that filled them |
| `04-linux-landscape.md` | prior art, architecture rationale, udev/systemd |
| `design/direction-approved.md` | UI direction history, including A being rejected |
| `../AGENTS.md` | binding conventions |

## In flight

Nothing. The stream-rate control (1/5/10/20/30 Hz presets, transfer counters in the session
sheet) and the mode selector's `0x03` write are both shipped and working against the daemon.

Linux packaging landed on 2026-11-20 — udev rule, per-user systemd unit, desktop entry, and
an Arch package built from this tree. Next milestone: a tagged release with versioned
artifacts, and a real reboot test of the boot-time start (`re/findings/23-linux-bringup.md`).

## Machine-change log (for honesty, not blame)

During this work the panel was frozen for ~32 minutes by a disguised `ping.exe` used to force
the service to yield; the camera was opened once for ~2 minutes by an agent without being asked;
the service was stopped/started many times; USBPcap was installed as a filter driver; UAC was
temporarily disabled by the owner. All of it is recorded in `03-live-capture.md` §0 and
`AGENTS.md` now forbids camera/microphone/screen capture without explicit approval.
