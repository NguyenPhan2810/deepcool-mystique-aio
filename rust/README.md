# mystique — Rust driver foundation for the DeepCool MYSTIQUE AIO LCD

A cross-platform, protocol-first implementation for the MYSTIQUE 360 LCD
(`VID:PID 3633:0009`, USB class `0xFF`, WinUSB on Windows, `usbfs`/libusb on Linux).

The foundation everything else is built on: byte-exactness and honest evidence grades
over feature count. The Python tree under `linux/` is superseded but left untouched.

- No firmware flashing, no firmware images, no write to `resources/fw/*`.
- No `unsafe` anywhere (`#![forbid(unsafe_code)]` in every crate).
- No `unwrap()` on fallible I/O; malformed input produces an error, never a panic.

## Layout

```
rust/
  Cargo.toml                 workspace
  crates/
    mystique-protocol/       pure: packets, replies, init, telemetry, DCLd, upload plans
    mystique-usb/            rusb transport, discovery, lifecycle, mock transport
    mystique-sensors/        /proc + /sys on Linux, vendor pipe + sysinfo on Windows
    mystique-media/          image/GIF decoding and panel fitting, shared by CLI and UI
    mystique-cli/            the `mystique` binary, including `explore`
  apps/
    mystique-ui/             Tauri 2 desktop control panel (static frontend, no Node)
  packaging/
    linux/                   udev rule + systemd user unit (data files, not build steps)
    windows/                 WinUSB note: no INF needed
```

## Build and test

```sh
cd rust
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

`cargo test` requires no hardware. The protocol crate is tested against
`re/fixtures/*.json` (byte-exact exchanges) and `re/captures/upload-decoded.json`
(the vendor's 360 upload), and the USB crate against an in-memory device that behaves
like the panel.

## Desktop control UI

`apps/mystique-ui` is a Tauri 2 desktop app around the same crates. It covers every
settings verb, uploads images with cover/contain cropping and a before-send plan, and
streams `0x01` telemetry at 1 Hz while the device is open. It has a plain static
frontend — no Node, no npm, no dev server — so `cargo run -p mystique-ui` builds and
launches it with the Rust toolchain only. The service must be stopped before the panel
can be opened. See `apps/mystique-ui/README.md` for the IPC contract, the stable element
ids and the proven/unverified list.

## rusb / libusb

`rusb 0.9` is used with the **`vendored` feature explicitly enabled**:

```toml
rusb = { version = "0.9", features = ["vendored"] }
```

That makes `libusb1-sys 0.7` compile its bundled libusb from source with the `cc`
crate instead of probing for a system library. It built first try here with
`rustc 1.100.0`, `cargo 1.100.0` and the MSVC toolchain — **no workaround was
needed**. Setting the feature explicitly instead of relying on the fallback in
`libusb1-sys`'s build script keeps the build deterministic on a machine with a
different system libusb.

Two operational consequences:

- The device is already bound to WinUSB through the Microsoft OS descriptor
  (`USB\MS_COMP_WINUSB`). No INF, no Zadig, no driver replacement.
- On Linux, libusb talks to `usbfs`; no kernel module is involved. The udev rule in
  `packaging/linux/` grants access.

## The commands

```
mystique pipes                 list endpoints and descriptor summary
mystique status                read 0x10 (rotation/mode, advisory)
mystique info [--show-device-id]
mystique init [--rotation 0-3] [--cpu NAME] [--gpu NAME]
mystique stream [--seconds N]  synthetic telemetry at 1 Hz (0 = until Ctrl+C)
mystique live [--count N]      init, then real sensors at 1 Hz
mystique brightness <0-100> [--send]
mystique rotate <0-3> [--send]
mystique gyro <on|off> [--send]
mystique flag0 <on|off> [--send]  the idle screen bit (0x02[0]), not screen control
mystique areas --main <n> [--style <0-2>] --aux <n> [--send]
mystique media-timing --time <0-2> --animation <0-2> [--send]
mystique image <path> [--send] [--cpu NAME] [--gpu NAME]
mystique gif <path> [--send] [--cpu NAME] [--gpu NAME]
mystique explore ...           the command-space mapper
mystique probe ...             explicit command/payload/pipe triples
```

`image` and `gif` are dry runs unless `--send` is given: the plan is printed and
nothing is transmitted. An upload leaves the panel showing the stored content; `0x14`
erase is the verified way back to machine info (see "not yet known" for what remains
open). JPEG input is passed through after a JFIF check; PNG and GIF are decoded, fitted to
480x640 portrait on black, and re-encoded with standard Huffman tables at quality 90.

The settings verbs are dry runs too: without `--send` they print the command, the
pipe, the full payload and the exact 48-byte packet. All of them need the vendor service
stopped before they can reach the panel, because the device is single-owner.

## Settings — idle, brightness, rotation, gyro, RGB, areas, media timing, recording

Decoded from the 2026-11-18 settings captures (`re/findings/13-settings-decode.md`,
`re/findings/17-mode-and-idle-screen.md`, `re/findings/18-rgb-and-recording.md`), in
which the owner changed one control at a time and the config block was diffed. Each
payload is **absolute state**, not a delta: `0x02` always carries all five bytes, so a
brightness write also sends idle screen, gyro, rotation and RGB sync (at their defaults,
unless the caller built a full `DisplayConfig`). Those four have no read-back; `0x10`
reports rotation, and even that byte is advisory.

| Setting | Command | Payload | Index | Values | Confidence |
|---|---|---|---|---|---|
| Idle screen display | `0x02` | 5 bytes | 0 | `01` enabled, `00` disabled | Verified |
| Gyroscopes allowed | `0x02` | 5 bytes | 1 | `01` allowed, `00` off | High |
| Rotation | `0x02` | 5 bytes | 2 | `00`–`03` = 0/90/180/270° | High |
| RGB light effect sync | `0x02` | 5 bytes | 3 | `01` synced, `00` not | Verified |
| Brightness | `0x02` | 5 bytes | 4 | `00`–`64` hex = 0–100 | High |
| Display mode | `0x03` | 1 byte | 0 | `01` Monitor, `02` Media, `03` Recording | Verified; no setter yet |
| Main display area | `0x04` | 4 bytes | 0 | `00`, `01`, `03`, `05` observed | High |
| Style Type | `0x04` | 4 bytes | 1 | `00` Normal, `01` Ring, `02` Stopwatch | Verified; renders only with main `00` |
| — reserved | `0x04` | 4 bytes | 2 | always sent `00`; never observed non-zero | **Unknown** |
| Auxiliary style | `0x04` | 4 bytes | 3 | `00` Voltage Mode, `01` System Monitor, `02` Core Data | Verified; renders only with main `00` |
| CPU frequency graph | `0x05` | 2 bytes | 0 | `01` on, `00` off | Verified |
| CPU temperature graph | `0x05` | 2 bytes | 1 | `01` on, `00` off | Verified |
| Image switch time | `0x07` | 2 bytes | 0 | `00`/`01`/`02` = 3/5/7 s | High |
| Switch animation | `0x07` | 2 bytes | 1 | `00`/`01`/`02` = Split/Scroll/Fade In/Out | High |

**`0x02` byte 0 is the idle screen display, not screen enable.** The first hardware pass
(`re/findings/15-driver-verification.md`) showed the byte does not blank the panel;
`re/findings/17-mode-and-idle-screen.md` then labelled it from a capture in which it moved
with the vendor's Idle Screen Display control. The driver sends `01` by default, and
**brightness 0 already blanks the panel**. Screen off is `mystique brightness 0`; screen
on is any non-zero brightness. `mystique-ui` keeps the last lit value and restores it.

**Rotation is gated on the gyro.** The panel ignores rotation while the gyroscopes are
enabled, and the vendor UI disables the rotation control until they are off. The driver
sends gyro-off and rotation in the same payload (`rotate_with_gyro_off`), and `rotate`
in the CLI is that operation.

Main and aux accept `0..=15`; the findings list clicked values, not the vendor's full option
list, so neither is a closed enum. Style Type is closed at `0..=2` (Normal, Ring, Stopwatch),
so larger values are rejected. The payload is `[main, style, 0, aux]`
(`re/findings/16-aux-area-layout.md`), and **style and aux only have a visible effect when main
is `00` CPU Frequency**. Index 2 is never observed non-zero, so it stays Unknown and is always
written `00`. Styles appear only after a telemetry frame, so the UI pushes `0x01` after `0x04`.

`0x03` is the verified display mode (`01` Monitor, `02` Media, `03` Recording), but the
crate has no setter: the UI's mode selector is a view filter, and stored media dominates
the panel until `0x14` erase. `0x05` is `[CPU frequency graph, CPU temperature graph]`;
the captured init payload is `01 01`, and the driver pushes one `0x01` frame after a change.
**Erase is how you leave media.** `0x14` with an empty payload on the init pipe deletes
the stored image or GIF and the panel falls back to machine info; it is destructive and
has no read-back, so the UI warns before sending it.

Not encoded: `0x09` and `0x17`. The RGB **temperature warning** and fan I/O mapping produce
no LCD traffic; they belong to the LED and fan controllers and are annotated as such.

## Frame transfer (captured on this 360)

The 2026-11-18 USBPcap capture of the vendor app (`re/captures/usbpcap-upload.pcap`,
decoded to `re/captures/upload-decoded.json`, written up in
`re/findings/12-usbpcap-360-protocol.md`) supersedes the 240-derived framing:

- the 64-byte `DCLd` header is **one USB transfer**;
- the JPEG follows as **64-byte transfers**, the last one partial;
- `dcldfinish` is a **55-byte** transfer of its own (ten ASCII bytes plus 45 zeros);
- all 14 headers passed the header checksum; static images use `frame_count = 0`,
  `frame_index = 0`, `delay = 0` and header byte 4 `01`; the 12-frame GIF uses
  `frame_count = 12`, indices 0..11, `delay = 858` and byte 4 `02`;
- the content id is lowercase hex. It is a per-upload tag, not a content hash: two
  byte-identical static JPEGs were uploaded with different ids. The CLI derives a fresh
  id from a nanosecond nonce; `ContentId::for_jpegs` stays deterministic for tests.

`Operation` has no combined frame variant. A frame is `Header([u8; 64])` followed by
`Chunk(Vec<u8>)` values of at most 64 bytes, and the transport writes each one as its
own transfer. `validate_plan` rejects an oversized or empty chunk before it can reach
the wire.

The captured upload sequence is:

```text
0x12, 0x02, 0x03, 0x04, 0x07, 0x08, 0x05, 0x0B, 0x06,
0x15 <CPU name>, 0x16 <GPU name>, 0x17 "--", 0x0A <local date and time>
0x14                      erase
0x0F                      begin
DCLd header (64 bytes, own transfer)
JPEG chunks (64 bytes each, last partial)
dcldfinish (55 bytes, own transfer)
0x08 [01]                 commit
0x0A <local date and time>, 0x10, 0x12
```

`0x0A` is the panel's clock: `EA 07 <month> <day> <hour> <minute> <second>`, sent about
once a second (`re/findings/22-clock.md`). It is not a mode switch and the last bytes are
not a counter — the "frozen prefix" was the capture's own date and hour. The init
sequence and the upload preamble carry the same live reading; a caller with no clock
source passes an explicit zero `Clock` rather than a plausible-looking date.

The vendor repeats the init block continuously while the panel is active; it is a
keepalive, not a one-time handshake.

## `explore` — mapping the command language

The device is the specification. `explore` is how the protocol gets completed without
the vendor app.

```sh
# 1. always start here: prints the plan, sends nothing
mystique explore --phase presence

# 2. the conservative real run (allowlist only, empty payloads)
mystique explore --phase presence --send

# 3. state changes
mystique explore --phase delta --send

# 4. payload sensitivity (only commands that changed state)
mystique explore --phase payload --send

# 5. regression fixtures for the init handshake
mystique explore --phase sequence --send

# full sweep, explicit responsibility
mystique explore --phase presence --send --unsafe-all --skip 0x03,0x04
```

### Phases

| Phase | What it does |
|---|---|
| `presence` | Sweeps command bytes with an **empty** payload. Records reply/no-reply, echoed byte, result code, reply length, elapsed time. |
| `delta` | Runs presence internally, then brackets every accepted command with `0x10` status reads and records `status_before` / `status_after`. |
| `payload` | Runs presence and delta internally, then sends four 39-byte payloads (zero, `0xFF`, ascending, ASCII marker) to commands whose status changed. |
| `sequence` | Replays the 13-step init handshake with its settle delays. `--include-upload-preamble` also replays the image-upload preamble. |

### Gates

- **Dry run by default.** Nothing is sent without `--send`.
- **Allowlist for real sends:** `0x01`, `0x02`, `0x03`, `0x04`, `0x05`, `0x06`,
  `0x07`, `0x08`, `0x0A`, `0x0B`, `0x10`, `0x12`, `0x15`, `0x16`, `0x17`.
- **`--unsafe-all`** is required to probe every byte `0x00..0xFF`, and prints a
  prominent warning. It is also required for non-empty payloads to non-allowlisted
  commands and for the upload preamble.
- **`--skip`** is applied last, even under `--unsafe-all`. A skipped byte is never sent.
- Unknown command bytes always get an **empty** payload. There is no code path that
  sends a non-empty payload to a command whose semantics are unknown unless the
  operator passed `--unsafe-all`.
- `--pipe auto` (default) sends allowlisted commands on their documented pipe and
  unknown bytes on the data pipe. The init-only space was measured separately with
  `--pipe init` on 2026-11-18; that pass is what found the eight extra echoing bytes.

### A timeout is data

A read timeout is recorded with its duration and the sweep continues. Only a **blocked
write** stops it: that is the wedge signature observed on this unit. The sweep stops
after the first one, prints the recovery instruction, writes the report, and exits
with code `3`. It never retries into a second wedge.

Recovery, printed verbatim by the tool:

```
Windows: pnputil /restart-device "USB\VID_3633&PID_0009\<instance-id>"
         find it with: Get-PnpDevice -InstanceId "USB\VID_3633&PID_0009\*"
         A service restart alone is not enough.
Linux:   unplug and replug the device, or power-cycle the USB port.
```

The real instance id is never printed or written, only the redaction-safe template.

### Report

One file per run: `re/reports/explore-<YYYYMMDD-HHMMSS>.json`. Pretty-printed with a
fixed key order, so runs diff cleanly. Every field is written on every run; absent
values are `null`, not omitted.

```json
{
  "version": 1,
  "tool": "mystique 0.1.0 explore",
  "generated_at": "2026-11-18T09:12:33Z",
  "phase": "delta",
  "mode": "send",
  "scope": { "first": 0, "last": 255, "count": 256 },
  "allowlist": [1, 2, 3, 7, 8, 5, 11, 6, 21, 22, 23, 16, 18, 10, 2],
  "skipped": [],
  "probe_timeout_ms": 750,
  "device": { "vendor_id": 13875, "product_id": 9, "bus": 1, "address": 5,
              "release": 582, "product": "MYSTIQUE", "identifiers_redacted": true },
  "redaction": { "policy": "bytes 4..36 masked when they match the 32-uppercase-hex identifier shape; serial and device id never written" },
  "wedge": null,
  "summary": { "probed": 15, "replied": 15, "success": 15, "timeouts": 0,
               "errors": 0, "state_changes": 0, "discovered": 15 },
  "entries": [
    {
      "index": 0,
      "cmd": 16,
      "pipe": "data",
      "payload_hex": "",
      "label": "delta",
      "replied": true,
      "echo": 16,
      "result_code": 0,
      "reply_len": 48,
      "reply_hex_redacted": "55 2E 10 00 00 FF ...",
      "elapsed_ms": 2,
      "outcome": "reply",
      "error": null,
      "status_before": { "rotation": 0, "degrees": 0, "mode_raw": 255, "mode": "machine-info", "result_code": 0 },
      "status_after":  { "rotation": 0, "degrees": 0, "mode_raw": 255, "mode": "machine-info", "result_code": 0 }
    }
  ]
}
```

`outcome` is one of `reply`, `timeout`, `stall`, `disconnected`, `error` (a wedge
entry also exists, but the run stops immediately after it). `result_code` is the raw
response byte 3; the enum mapping lives in `mystique_protocol::ResultCode`.

### `--emit-constants`

```sh
mystique explore --phase presence --send --emit-constants
```

Rewrites only the block between

```
// BEGIN GENERATED: observed-commands
// END GENERATED: observed-commands
```

in `crates/mystique-protocol/src/commands.rs`. It emits one `ObservedCommand` per
command whose byte was **echoed back**, sorted by byte, each with the observed pipe,
the payload gate, the semantic grade from the hand-maintained table and the report
path as a source. A neutral `55 2E 00` ACK is not emitted: it proves receipt, not
recognition. An echo never promotes a command to `Verified`: `0xA1` stays `Unknown`
and `0x0F`/`0x11` stay `Inferred`. Commands that timed out are not emitted:
absence of a reply to one empty payload is not proof that a command does not exist.
It refuses to touch a file without the markers. It never writes a semantic name of
its own.

## `probe` — explicit command/payload/pipe triples

`explore` answers "does this byte reply on this pipe". `probe` answers "is this
command payload-gated", "does it behave differently on the other pipe", or "what does
this exact payload do". It sends exactly what you give it, on the pipe you choose.
The 2026-11-18 runs used it to close the init-pipe coverage gap and to prove the
`0x0A` payload gate.

```sh
# the batch that closed the coverage gap
mystique probe --try 0x0A:ea070b120a2c3a:init \
               --try 0x15:496e74656c20436f72652069392d31343930304b:init \
               --try 0x0F::init --try 0x0F::data

# or from a file
mystique probe --from-file probes.txt --send
```

```
--try <cmd_hex>:<payload_hex>:<init|data>   repeatable, order preserved
--from-file <path>                          one `cmd_hex payload_hex pipe` per line
--send                                      actually transmit
--timeout-ms <N>                            default 750
--out <DIR>                                 default re/reports
```

- `cmd` is hexadecimal with or without `0x`; `0x0A` and `0a` are the same byte.
- An empty payload is `0x10::data` (or `0x10:-:data`). There is no default payload and
  no built-in try list: **every byte comes from the operator**.
- File lines are `cmd_hex payload_hex pipe`, with `#` comments and blank lines ignored.
  `cmd pipe` is accepted as an empty-payload shorthand. `--try` entries keep their
  order; file entries follow them in file order.
- Without `--send` nothing is transmitted. The exact 48-byte packet for every try is
  printed in full, checksum included:

  ```
  AA 2E 0A EA 07 02 02 02 27 21 00 ... 48 49 44 43 39 03
  ```

- Commands outside the conservative allowlist produce a warning, not a block: the
  point of this verb is to test them. A blocked write stops the batch after that
  command, exactly as in `explore`.

Reports go to `re/reports/probe-<YYYYMMDD-HHMMSS>.json` and use the **same `entries`
schema** as `explore`, so `re/tools/analyze_explore.py` works unchanged when given the
file name:

```sh
python re/tools/analyze_explore.py probe-20261118-010203.json
```

Each entry carries `cmd`, `payload_hex`, `pipe`, `replied`, `echo`, `result_code`,
`reply_len`, `reply_hex_redacted`, `elapsed_ms` (plus `label`, `outcome`, `error` and
null `status_before`/`status_after`). `phase` is `probe`.

A neutral `55 2E 00` ACK means the packet was received, not understood. The echo byte
is the signal, and a command can act without echoing.

## Command table and provenance

`Verified` = observed on this 360 and recorded under `re/fixtures/` or `re/reports/`.
`Inferred` = vendor code or third-party prior art; presence may be verified, but the
behaviour was never exercised here.
`Unknown` = present, semantics unresolved.

**Pipe** is where the driver sends the command. **Recognised on** is where the
firmware echoed the command byte; an empty payload unless stated. The two are not
the same question, and `0x0A` is the proof.

| Cmd | Name | Pipe | Recognised on | Presence | Semantics | Notes |
|---|---|---|---|---|---|---|
| `0x01` | telemetry | data `0x02/0x82` | data | Verified | Inferred | 39-byte frame captured; five display fields understood, the rest not. The UI streams these at 1 Hz; a layout with no numbers means no `0x01` arrived. `re/fixtures/04-telemetry-baseline-0x01.json` |
| `0x02` | display-config | init `0x01/0x81` | init | Verified | Verified | five absolute bytes: idle screen (0), gyro (1), rotation (2), RGB sync (3), brightness (4); every byte moved under a labelled vendor action (`re/findings/13`, `17`, `18`). Byte 0 is **not** screen enable: brightness 0 blanks the panel (`re/findings/15-driver-verification.md`) |
| `0x03` | display-mode | init | init | Verified | Verified | `01` Monitor, `02` Media, `03` Recording, with four labelled transitions (`re/findings/17-mode-and-idle-screen.md`); stored media still dominates while it exists, so `0x14` erase is the visible way back. No setter yet |
| `0x04` | display-areas | init | init | Verified | Unknown | four bytes `[main, style, 0, aux]`; index 0 is Main Display Area, index 1 is Style Type (`00` Normal, `01` Ring, `02` Stopwatch), index 3 is Auxiliary Display Area (`00` Voltage Mode, `01` System Monitor, `02` Core Data) (`re/findings/16-aux-area-layout.md`); **style and aux render only with main `00` CPU Frequency**; index 2 has never been observed non-zero and is always written `00`, so its meaning is Unknown |
| `0x05` | recording-options | init | init | Verified | Verified | `[CPU frequency graph, CPU temperature graph]`; captured init payload `01 01`, both bits moved with the vendor Recording toggles (`re/findings/18-rgb-and-recording.md`) |
| `0x06` | init-step-0x06 | init | init | Verified | Unknown | payload `01` |
| `0x07` | init-step-0x07 | init | init | Verified | Unknown | captured payload is empty on this 360; the settings capture maps index 0 to image switch time (3/5/7 s) and index 1 to animation |
| `0x08` | init-step-0x08 | init | init | Verified | Unknown | captured init payload is empty; upload commit is `01` |
| `0x09` | unknown-0x09 | init | init | Verified | Unknown | captured twice, payload `01` then `00`; not in either sequence; semantics unknown |
| `0x0A` | clock | init | data, only with a payload | Verified | Inferred | the panel's clock: `EA 07 <month> <day> <hour> <minute> <second>`, sent about once a second; what earlier notes read as a frozen prefix plus a big-endian counter was the capture's own date, hour and seconds (`re/findings/22-clock.md`); empty payload is a silent no-op |
| `0x0B` | init-step-0x0B | init | init | Verified | Unknown | empty payload |
| `0x0C` | unknown-0x0C | init | init | Verified | Unknown | newly recognised; semantics unknown |
| `0x0D` | unknown-0x0D | init | init | Verified | Unknown | newly recognised; semantics unknown |
| `0x0E` | unknown-0x0E | init | init | Verified | Unknown | newly recognised; semantics unknown |
| `0x0F` | begin-upload | init | init | Verified | Inferred | captured three times with an empty payload, immediately before the frame headers |
| `0x10` | status | data `0x02/0x82` | data | Verified | Inferred | byte 4 rotation, byte 5 mode; the mode byte is unreliable |
| `0x11` | request-0x11 | init | init | Verified | Inferred | 240 prior art calls it a serial request; the 360 vendor never sends it; the body is all zeros, so no serial is parsed |
| `0x12` | device-info | init `0x01/0x81` | init | Verified | Verified | 32 uppercase hex identifier bytes at 4..35, redacted by default |
| `0x13` | unknown-0x13 | init | init | Verified | Unknown | newly recognised; semantics unknown |
| `0x14` | erase-images | init | init | Verified | Verified | captured once between uploads, empty payload; the 2026-11-18 hardware pass verified that erase alone returns the panel from media to machine info; required before a second upload or the next one silently no-ops |
| `0x15` | init-step-0x15 | init | — (silent) | Verified | Unknown | ASCII text label up to 39 bytes; the vendor sends the CPU name; display effect not independently observed |
| `0x16` | init-step-0x16 | init | — (silent) | Verified | Unknown | ASCII text label up to 39 bytes; the vendor sends the GPU name; display effect not independently observed |
| `0x17` | init-step-0x17 | init | — (silent) | Verified | Unknown | ASCII text label; the vendor sends `--` in every captured block |
| `0xA1` | unknown-0xA1 | init | init | Verified | Unknown | newly discovered singleton; semantics unknown, nothing invented |

Init order is **not ascending** and is not sorted in code:
`12 02 03 04 07 08 05 0B 06 15 16 17 0A`, 10 ms per step, 100 ms after `0x0A`.
That order and every payload now match the 360 capture. `re/fixtures/03-init-sequence.json`
records the older, 240-derived exchange our own code sent; it remains a valid packet
framing fixture but is no longer the payload reference.

### What the sweeps established

The first sweep (`re/reports/explore-20261117-174553.json`) asked every byte on the
**data** pipe. That was a coverage error: the init chain lives on the other pipe, so
bytes implemented only there looked exactly like a no-op. Three runs on 2026-11-18
closed the gap:

| Pass | Pipe | Echoing bytes |
|---|---|---|
| `explore-20261117-175642.json` | documented, allowlist only | 11 |
| `explore-20261117-175644.json` | allowlist on documented, unknown on **data** | 11 |
| `explore-20261117-175646.json` | every byte on **init** | 18 |

- The init pipe echoes 18 bytes: `0x02–0x09`, `0x0B–0x0F`, `0x11–0x14`, `0xA1`.
- The data pipe echoes `0x01` and `0x10` with an empty payload, and `0x0A` only with
  a non-empty payload.
- The union is **21 echo-positive bytes**. `0x15`, `0x16` and `0x17` are silent
  everywhere, including under two different payloads; they are write-only, not
  unimplemented.
- `0x0F`, `0x11` and `0x14` are real on this 360, on the init pipe, with empty
  payloads. Their earlier neutral ACK was the coverage gap, not the firmware.
- `0x0A` is the bridge: silent on init, payload-gated on data. An empty `0x0A` is a
  silent no-op, and the driver refuses to send one (`Command::requires_payload`).

Every reply has the same generic 48-byte `55 2E <cmd> ...` shape. An echo proves
recognition, not behaviour: no arguments, state delta or effect is established for
`0x09`, `0x0C`–`0x0E`, `0x13` or `0xA1`, so they stay `Unknown` and are named
`unknown-0xNN`. `0x00` cannot be scored by echo at all: `55 2E 00` is also the
neutral ACK.

The command space is therefore **mapped, not unmapped**: 24 bytes are accounted for
and the remaining 232 are neutral on both pipes. What is missing is semantics, not
inventory.

## Building on Linux

The workspace was built natively on Ubuntu 26.04 (`rustc 1.98.0`, `gcc 15.2.0`):

```sh
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

Prerequisites: a C toolchain (`cc`) for the vendored libusb, and for `mystique-ui` the
Tauri Linux stack — `libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `libsoup-3.0-dev`,
`libjavascriptcoregtk-4.1-dev`, `librsvg2-dev` and `libayatana-appindicator3-dev`.
Without those the rest of the workspace still builds with
`cargo build --workspace --exclude mystique-ui`.

Cross-compiling from Windows is still possible, with the prerequisites below:

```sh
rustup target add x86_64-unknown-linux-gnu
cargo build --release --target x86_64-unknown-linux-gnu
```

1. the target's `std` must be installed (`rustup target add`), otherwise cargo fails
   with `E0463: can't find crate for 'core'`;
2. a GNU cross linker and C cross compiler must be on `PATH`, because
   `libusb1-sys` compiles vendored libusb with `cc`. Set `CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER`
   and `CC_x86_64_unknown_linux_gnu` accordingly.

## Packaging

`packaging/` contains data files only; nothing in the build consumes them.

- `../packaging/linux/99-mystique.rules` — `uaccess` ACL and `MODE="0660"` for
  `3633:0009`. No group is used. Install to `/etc/udev/rules.d/` and replug.
- `../packaging/linux/mystique-daemon.service` — systemd **user** unit for the daemon,
  `Restart=on-failure`. The UI is an optional client; the daemon owns the device.
- `../packaging/linux/README.md` — install steps, and the fact that the vendor
  `MystiqueService` has no Linux equivalent.
- `packaging/windows/README.md` — the device is already WinUSB-bound; no INF.

Single-owner is real: a second open gets access-denied. Do not run the CLI and a
daemon at the same time.

## What is not yet known

These are open questions, not omissions. Anything unknown is either absent from the
code or named with `Evidence::Unknown`; no plausible semantics were invented.

1. **Semantics of the recognised-but-semantically-unknown bytes.** `0x03`–`0x09`, `0x0B`–`0x0E`,
   `0x13`, `0x15`–`0x17` and `0xA1` are proven to be present on the init pipe. What
   they do is unknown. `explore --phase delta` is the intended way to find out.
2. **`0x11`.** Prior art calls it a serial request, and the presence sweep confirmed
   the command on the init pipe. The reply body is 38 zero bytes on this unit, so it
   is returned raw (`Link::request_11_body`) and never parsed as a serial. `0x12` remains
   the only identity source.
3. **`0x0F` behaviour.** It exists on this 360's init pipe with an empty payload, and
   the vendor capture shows it three times bracketing successful uploads. The sequence is
   captured, and the upload path has now been exercised, but `0x0F`'s own semantics stay
   `Inferred`. `0x14` is no longer in this list: erase was verified on hardware.
4. **Upload frame pipe.** Resolved by capture: headers, JPEG chunks and the terminator
   all use OUT `0x01`. Each is its own transfer.
5. **Upload preamble and replay.** Resolved by capture: the vendor sends the same
   13-command block it repeats continuously, then erase/begin, frames, terminator,
   `0x08 [01]`, then `0x0A`, `0x10`, `0x12`. There is no second full preamble.
6. **`0x09` and the `0x0A` counter.** Both were observed but their meaning is unknown.
   `0x09` is not sent by any plan; the `0x0A` counter is caller-supplied. `0x02` byte 3
   is no longer in this list: it is RGB light effect sync, verified in
   `re/findings/18-rgb-and-recording.md`. `0x02` byte 0 is the idle screen display, also
   verified (`re/findings/17-mode-and-idle-screen.md`), and is not a screen control.
7. **Returning to Machine Info mode after an upload.** Resolved on 2026-11-18: erasing
   the stored images with `0x14` alone returned the panel to machine info. `0x03` is not
   a mode byte; media is content presence (`re/findings/15-driver-verification.md`).
8. **GIF behaviour on the panel.** Frame pacing is carried in the header; the actual
   accepted frame count, size ceiling and delay semantics are unmeasured.
9. **Telemetry offset 8 and the duplicated clock fields.** The firmware does not
   validate them; their display meaning is unresolved. See `telemetry.rs`.
10. **Response lengths above 48 bytes.** Never observed. The reader offers 64.
11. **Whether a `0x01` frame that returns result `0x02` is still applied.** Unknown;
    for `0x10` the payload is returned anyway.
12. **Checksum-vs-marker precedence** when both are wrong. The experiment that would
    have answered it wedged the device.
13. **A firmware path.** Permanently out of scope. `explore` stops on an unusually
    large response rather than retrying.
14. **The Windows vendor sensor pipe.** The handshake in `mystique-sensors` is a direct
    port of `SensorBridgeClient.cs` (`SERVER_HELLO` → `CLIENT_HELLO`,
    `SETUP_DATA_CHANNEL` → `DATA_CHANNEL_READY`, then `GET_SENSOR_DATA`). It is
    untested against a live `SensorBridgeServer.exe` in this session; if any step
    fails, the sampler falls back to `sysinfo` and says so in `describe()`.
15. **Which init-pipe byte causes the post-sweep `ERROR_BAD_COMMAND`.** The side
    effect is real but the byte is not identified; see Troubleshooting. A bisect is
    wanted before the next full init sweep.

### Verification status

Everything below the USB boundary is covered by `cargo test` with no hardware. The sweep
and probe verbs ran against the device; reports are in `re/reports/` and the map is
`re/findings/11-sweep-results.md`. Vendor upload framing is captured and conformance-tested
(`re/captures/upload-decoded.json`), and our upload path ran on 2026-11-18.

## Troubleshooting

### `ERROR_BAD_COMMAND` after exploratory probing

A full init-pipe sweep can leave the panel where the next normal write is refused with
Windows `ERROR_BAD_COMMAND` (Win32 22). It happened on 2026-11-18 immediately after the
256-byte init sweep: the service acquired the device, logged
`Streaming -> Reconnecting: unclassified Win32 error 22`, and stayed in `Reconnecting` for
154 s until it was stopped (`re/findings/11-sweep-results.md`). A single service stop/start
cleared it; `pnputil` was not needed.

This is not a blocked-write wedge and not a disconnect. The driver maps it to
`DeviceError::BadCommand`, for which `is_recoverable()` is true and `is_terminal()`
is false, so it can retry instead of abandoning the run. Practical rule: after a full
init-pipe sweep, confirm that telemetry still streams; if it does not, restart the
service once before touching the USB device.

## Safety notes

- A packet whose first two bytes are not `AA 2E` produces no reply and wedged this device
  for ~24 minutes; `build_packet` is the only constructor and always writes them.
- The device is bus-powered logic only; cutting USB does not stop the pump.
- `explore` refuses to send non-empty payloads to unknown commands, and refuses to
  run the upload preamble without `--unsafe-all`.
- The device identifier and serial are redacted in `Debug`, `Display`, `Serialize`,
  the report, and the CLI default. `--show-device-id` is the only reveal path.
